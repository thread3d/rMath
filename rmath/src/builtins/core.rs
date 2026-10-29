//! Built-ins for assignment, scoping, nonstandard evaluation, control flow,
//! replacement rules, iterators (Do/Table/Sum/Product), and Part.

use crate::eval::{subst_syms, EvalRes, Flow, Interp};
use crate::expr::{has, same_q, A_PROTECTED, DownRule, Expr, OwnValue};
use crate::pattern::match_top;
use std::time::{Duration, Instant};

pub fn sym_(i: &mut Interp, name: &str) -> Expr {
    Expr::Sym(i.t.intern(name))
}

pub fn unevaluated_call(i: &mut Interp, name: &str, args: &[Expr]) -> Expr {
    let id = i.t.intern(name);
    Expr::node(Expr::Sym(id), args.to_vec())
}

fn is_protected(i: &Interp, id: usize) -> bool {
    has(i.t.attrs(id), A_PROTECTED)
}

// ============================ control flow ============================

pub fn b_compound(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut last = Expr::Sym(i.t.sys.null);
    for a in args {
        last = i.eval(a)?;
    }
    Ok(last)
}

pub fn b_if(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() || args.len() > 4 {
        i.message("If", "argr");
        return Ok(unevaluated_call(i, "If", args));
    }
    let c = i.eval(&args[0])?;
    let sys = i.t.sys;
    if c.is_symbol(sys.r#true) {
        if args.len() >= 2 {
            i.eval(&args[1])
        } else {
            Ok(Expr::Sym(sys.null))
        }
    } else if c.is_symbol(sys.r#false) {
        if args.len() >= 3 {
            i.eval(&args[2])
        } else {
            Ok(Expr::Sym(sys.null))
        }
    } else if args.len() >= 4 {
        i.eval(&args[3])
    } else {
        Ok(unevaluated_call(i, "If", args))
    }
}

pub fn b_which(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() || args.len() % 2 == 1 {
        i.message("Which", "argr");
        return Ok(unevaluated_call(i, "Which", args));
    }
    for k in 0..args.len() / 2 {
        let c = i.eval(&args[2 * k])?;
        if c.is_symbol(i.t.sys.r#true) {
            return i.eval(&args[2 * k + 1]);
        }
        if !c.is_symbol(i.t.sys.r#false) {
            return Ok(unevaluated_call(i, "Which", args));
        }
    }
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_switch(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("Switch", "argr");
        return Ok(unevaluated_call(i, "Switch", args));
    }
    let e = i.eval(&args[0])?;
    let mut k = 1;
    while k + 1 < args.len() {
        let p = &args[k];
        let matched = match p.parts() {
            Some((h, _)) if h.is_symbol(i.t.sys.blank) => true,
            _ => match_top(i, p, &e),
        };
        if matched {
            return i.eval(&args[k + 1]);
        }
        k += 2;
    }
    Ok(unevaluated_call(i, "Switch", args))
}

// ---------- iterators (Do / Table / Sum / Product) ----------

pub enum Iter {
    Count(i64),
    Range { var: usize, lo: i64, hi: i64, step: i64 },
    List { var: usize, vals: Vec<Expr> },
}

fn eval_int(i: &mut Interp, e: &Expr) -> Result<Option<i64>, Flow> {
    let v = i.eval(e)?;
    Ok(expr_to_int(&v))
}

pub fn expr_to_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n) => n.to_i64(),
        Expr::Real(x) if x.fract() == 0.0 && x.is_finite() => Some(*x as i64),
        Expr::Rat(r) if r.is_int() => r.num.to_i64(),
        _ => None,
    }
}

/// Parse iterator specs (evaluating bounds) for Table/Sum/Product/Do.
/// Returns Ok(None) when a spec is not a concrete integer/range/list iterator
/// (the caller then leaves the expression unevaluated instead of aborting).
pub fn build_iters(i: &mut Interp, specs: &[Expr]) -> Result<Option<Vec<Iter>>, Flow> {
    let mut out = Vec::new();
    for spec in specs {
        match parse_iter(i, spec)? {
            Some(it) => out.push(it),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn parse_iter(i: &mut Interp, spec: &Expr) -> Result<Option<Iter>, Flow> {
    let sys = i.t.sys;
    let Some((h, hargs)) = spec.parts() else {
        return Ok(None);
    };
    if !h.is_symbol(sys.list) {
        return Ok(None);
    }
    match hargs.len() {
        1 => {
            let n = match eval_int(i, &hargs[0])? {
                Some(n) => n,
                None => return Ok(None),
            };
            Ok(Some(Iter::Count(n.max(0))))
        }
        2 => {
            // {i, n} or {i, list}
            let Some(var) = hargs[0].sym_id() else { return Ok(None) };
            let bound = i.eval(&hargs[1])?;
            if bound.is_list(&i.t) {
                let vals = bound.node_args();
                Ok(Some(Iter::List { var, vals }))
            } else {
                let n = match expr_to_int(&bound) {
                    Some(n) => n,
                    None => return Ok(None),
                };
                Ok(Some(Iter::Range { var, lo: 1, hi: n, step: 1 }))
            }
        }
        3 | 4 => {
            let Some(var) = hargs[0].sym_id() else { return Ok(None) };
            let Some(lo) = eval_int(i, &hargs[1])? else { return Ok(None) };
            let Some(hi) = eval_int(i, &hargs[2])? else { return Ok(None) };
            let step = if hargs.len() == 4 {
                match eval_int(i, &hargs[3])? {
                    Some(s) => s,
                    None => return Ok(None),
                }
            } else {
                1
            };
            Ok(Some(Iter::Range { var, lo, hi, step }))
        }
        _ => Ok(None),
    }
}

/// Run nested iterators, substituting loop variables into `body`.
pub fn run_iters(
    i: &mut Interp,
    iters: &[Iter],
    env: &mut Vec<(usize, Expr)>,
    body: &Expr,
    sink: &mut dyn FnMut(&mut Interp, &Expr) -> Result<(), Flow>,
) -> Result<(), Flow> {
    if iters.is_empty() {
        let names: Vec<usize> = env.iter().map(|(k, _)| *k).collect();
        let vals: Vec<Expr> = env.iter().map(|(_, v)| v.clone()).collect();
        let sub = subst_syms(&i.t, body, &names, &vals);
        match sink(i, &sub) {
            Ok(()) => {}
            Err(Flow::Break) => return Ok(()),
            Err(Flow::Continue) => {}
            Err(e) => return Err(e),
        }
        return Ok(());
    }
    let it = &iters[0];
    let rest = &iters[1..];
    match it {
        Iter::Count(n) => {
            for _ in 0..*n {
                run_iters(i, rest, env, body, sink)?;
            }
        }
        Iter::Range { var, lo, hi, step } => {
            if *step > 0 {
                let mut k = *lo;
                while (*hi - k) * (*step) >= 0 && k <= *hi {
                    env.push((*var, Expr::int_i64(k)));
                    run_iters(i, rest, env, body, sink)?;
                    env.pop();
                    k += *step;
                }
            } else if *step < 0 {
                let mut k = *lo;
                while k >= *hi {
                    env.push((*var, Expr::int_i64(k)));
                    run_iters(i, rest, env, body, sink)?;
                    env.pop();
                    k += *step;
                }
            }
        }
        Iter::List { var, vals } => {
            for v in vals {
                env.push((*var, v.clone()));
                run_iters(i, rest, env, body, sink)?;
                env.pop();
            }
        }
    }
    Ok(())
}

pub fn b_do(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Do", "argr");
        return Ok(unevaluated_call(i, "Do", args));
    }
    let Some(iters) = build_iters(i, &args[1..])? else {
        i.message("Do", "itform");
        return Ok(unevaluated_call(i, "Do", args));
    };
    let mut env: Vec<(usize, Expr)> = Vec::new();
    let mut body = args[0].clone();
    // Break / Continue end the body, but an abort — a `TimeConstrained` deadline or
    // the server's per-request limit — must propagate: swallowing it made a runaway
    // `Do` return `Null` as if it had finished.
    match run_iters(i, &iters, &mut env, &body, &mut |i, sub| match i.eval(sub) {
        Ok(_) => Ok(()),
        Err(Flow::Break) => Err(Flow::Break),
        Err(Flow::Continue) => Ok(()),
        Err(e) => Err(e),
    }) {
        Ok(_) | Err(Flow::Break) => {}
        Err(e) => return Err(e),
    }
    let _ = &mut body;
    Ok(Expr::Sym(i.t.sys.null))
}

// ---------- loops ----------

pub fn b_while(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() || args.len() > 2 {
        i.message("While", "argr");
        return Ok(Expr::Sym(i.t.sys.null));
    }
    let body = if args.len() > 1 { Some(&args[1]) } else { None };
    loop {
        let c = i.eval(&args[0])?;
        if !c.is_symbol(i.t.sys.r#true) {
            break;
        }
        if let Some(b) = body {
            match i.eval(b) {
                Ok(_) => {}
                Err(Flow::Break) => break,
                Err(Flow::Continue) => continue,
                Err(e) => return Err(e),
            }
        }
    }
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_for(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("For", "argr");
        return Ok(Expr::Sym(i.t.sys.null));
    }
    i.eval(&args[0])?;
    let incr = if args.len() > 2 { Some(&args[2]) } else { None };
    let body = if args.len() > 3 { Some(&args[3]) } else { None };
    loop {
        let c = i.eval(&args[1])?;
        if !c.is_symbol(i.t.sys.r#true) {
            break;
        }
        if let Some(b) = body {
            match i.eval(b) {
                Ok(_) => {}
                Err(Flow::Break) => break,
                Err(Flow::Continue) => {}
                Err(e) => return Err(e),
            }
        }
        if let Some(s) = incr {
            i.eval(s)?;
        }
    }
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_nest(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 3 {
        i.message("Nest", "argr");
        return Ok(unevaluated_call(i, "Nest", args));
    }
    let n = i.eval(&args[2])?;
    let count = expr_to_int(&n).unwrap_or(0).max(0) as usize;
    let mut cur = i.eval(&args[1])?;
    for _ in 0..count {
        let call = Expr::node(args[0].clone(), vec![cur]);
        cur = i.eval(&call)?;
    }
    Ok(cur)
}

pub fn b_nestlist(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 3 {
        i.message("NestList", "argr");
        return Ok(unevaluated_call(i, "NestList", args));
    }
    let n = i.eval(&args[2])?;
    let count = expr_to_int(&n).unwrap_or(0).max(0) as usize;
    let mut cur = i.eval(&args[1])?;
    let mut out = vec![cur.clone()];
    for _ in 0..count {
        let call = Expr::node(args[0].clone(), vec![cur]);
        cur = i.eval(&call)?;
        out.push(cur.clone());
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_fold(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 3 {
        i.message("Fold", "argr");
        return Ok(unevaluated_call(i, "Fold", args));
    }
    let mut acc = i.eval(&args[1])?;
    let list = i.eval(&args[2])?;
    for x in list.node_args() {
        let call = Expr::node(args[0].clone(), vec![acc, x.clone()]);
        acc = i.eval(&call)?;
    }
    Ok(acc)
}

pub fn b_fixedpoint(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("FixedPoint", "argr");
        return Ok(unevaluated_call(i, "FixedPoint", args));
    }
    let mut cur = i.eval(&args[1])?;
    for _ in 0..10000 {
        let call = Expr::node(args[0].clone(), vec![cur.clone()]);
        let next = i.eval(&call)?;
        if same_q(&next, &cur) {
            return Ok(next);
        }
        cur = next;
    }
    i.message("FixedPoint", "itlim");
    Ok(cur)
}

// ---------- holds ----------

pub fn b_hold(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(Expr::node(Expr::Sym(i.t.intern("Hold")), args.to_vec()))
}

pub fn b_releasehold(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("ReleaseHold", "argr");
        return Ok(unevaluated_call(i, "ReleaseHold", args));
    }
    let e = i.eval(&args[0])?;
    let stripped = strip_holds(&i.t, &e);
    i.eval(&stripped)
}

fn strip_holds(t: &crate::expr::SymbolTable, e: &Expr) -> Expr {
    let sys = t.sys;
    if let Some((h, args)) = e.parts() {
        if (h.is_symbol(sys.hold)
            || h.is_symbol(sys.holdform)
            || h.is_symbol(sys.holdcomplete)
            || h.is_symbol(sys.unevaluated))
            && args.len() == 1
        {
            return strip_holds(t, &args[0]);
        }
        let nh = strip_holds(t, h);
        let na: Vec<Expr> = args.iter().map(|a| strip_holds(t, a)).collect();
        return Expr::node(nh, na);
    }
    e.clone()
}

pub fn b_evaluate(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Evaluate", args));
    }
    i.eval(&args[0])
}

pub fn b_unevaluated(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(unevaluated_call(i, "Unevaluated", args))
}

pub fn b_sequence(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(unevaluated_call(i, "Sequence", args))
}

// ---------- catch / throw / return ----------

pub fn b_catch(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("Catch", "argr");
        return Ok(unevaluated_call(i, "Catch", args));
    }
    let r = i.eval(&args[0]);
    match r {
        Err(Flow::Throw(v, tag)) => {
            if args.len() == 1 {
                if tag.is_none() {
                    return Ok(v);
                }
                return Err(Flow::Throw(v, tag));
            }
            if let Some(tag) = tag.clone() {
                let form = i.eval(&args[1])?;
                if match_top(i, &form, &tag) {
                    return Ok(v);
                }
            }
            Err(Flow::Throw(v, tag))
        }
        other => other,
    }
}

pub fn b_throw(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() || args.len() > 2 {
        i.message("Throw", "argr");
        return Ok(unevaluated_call(i, "Throw", args));
    }
    let v = i.eval(&args[0])?;
    let tag = if args.len() > 1 { Some(i.eval(&args[1])?) } else { None };
    Err(Flow::Throw(v, tag))
}

pub fn b_return(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        return Err(Flow::Return(None));
    }
    let v = i.eval(&args[0])?;
    Err(Flow::Return(Some(v)))
}

// ---------- error control ----------

/// Quiet[expr] — evaluate expr, discarding any messages it generates.
pub fn b_quiet(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("Quiet", "argr");
        return Ok(unevaluated_call(i, "Quiet", args));
    }
    let saved = i.messages.len();
    let r = i.eval(&args[0]);
    i.messages.truncate(saved);
    r
}

/// Check[expr, failexpr] — failexpr if expr generated a message or was aborted.
pub fn b_check(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Check", "argr");
        return Ok(unevaluated_call(i, "Check", args));
    }
    let saved = i.messages.len();
    match i.eval(&args[0]) {
        Ok(v) => {
            if i.messages.len() > saved {
                i.eval(&args[1])
            } else {
                Ok(v)
            }
        }
        Err(Flow::Abort) => i.eval(&args[1]),
        Err(e) => Err(e),
    }
}

fn name_of_arg(i: &Interp, a: &Expr) -> Option<String> {
    match a {
        Expr::Sym(id) => Some(i.t.name_of(*id).to_string()),
        Expr::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

/// Off[] switches every message off; Off[f, ...] just those from f.
pub fn b_off(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.messages_off = true;
        return Ok(Expr::Sym(i.t.sys.null));
    }
    for a in args {
        if a.is_symbol(i.t.sys.all) {
            i.messages_off = true;
        } else if let Some(name) = name_of_arg(i, a) {
            if !i.off_names.iter().any(|n| *n == name) {
                i.off_names.push(name);
            }
        }
    }
    Ok(Expr::Sym(i.t.sys.null))
}

/// On[] restores every message; On[f, ...] restores those from f.
pub fn b_on(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.messages_off = false;
        i.off_names.clear();
        return Ok(Expr::Sym(i.t.sys.null));
    }
    for a in args {
        if a.is_symbol(i.t.sys.all) {
            i.messages_off = false;
            i.off_names.clear();
        } else if let Some(name) = name_of_arg(i, a) {
            i.off_names.retain(|n| *n != name);
        }
    }
    Ok(Expr::Sym(i.t.sys.null))
}

/// Abort[] — unwind the current evaluation; the top level reports `$Aborted`.
pub fn b_abort(_i: &mut Interp, _args: &[Expr]) -> EvalRes {
    Err(Flow::Abort)
}

/// Step[label, expr] — one held step of a derivation (see Steps/Trace).
pub fn b_step(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let id = i.t.intern("Step");
    Ok(Expr::node(Expr::Sym(id), args.to_vec()))
}

/// TimeConstrained[expr, t] — evaluate expr, aborting after t seconds.
/// TimeConstrained[expr, t, fallback] returns fallback on timeout.
pub fn b_timeconstrained(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("TimeConstrained", "argr");
        return Ok(unevaluated_call(i, "TimeConstrained", args));
    }
    let tv = i.eval(&args[1])?;
    let secs = match &tv {
        Expr::Int(n) => n.to_f64(),
        Expr::Rat(r) => r.to_f64(),
        Expr::Real(x) => *x,
        _ => {
            i.message("TimeConstrained", "argr");
            return Ok(unevaluated_call(i, "TimeConstrained", args));
        }
    };
    let prev = i.deadline;
    i.deadline = Some(Instant::now() + Duration::from_secs_f64(secs.max(0.0)));
    let r = i.eval(&args[0]);
    i.deadline = prev;
    match r {
        Ok(v) => Ok(v),
        Err(Flow::Abort) => {
            if args.len() >= 3 {
                i.eval(&args[2])
            } else {
                Ok(Expr::Sym(i.t.intern("$Aborted")))
            }
        }
        Err(e) => Err(e),
    }
}

// ---------- rules ----------

pub fn b_rule(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Rule", "argr");
        return Ok(unevaluated_call(i, "Rule", args));
    }
    let rhs = i.eval(&args[1])?;
    Ok(Expr::node(Expr::Sym(i.t.sys.rule), vec![args[0].clone(), rhs]))
}

pub fn b_ruledelayed(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("RuleDelayed", "argr");
        return Ok(unevaluated_call(i, "RuleDelayed", args));
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.ruledelayed), args.to_vec()))
}

pub fn b_replaceall(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("ReplaceAll", "argr");
        return Ok(unevaluated_call(i, "ReplaceAll", args));
    }
    let e = i.eval(&args[0])?;
    let rules = i.eval(&args[1])?;
    let (out, _) = crate::pattern::replace_all(i, &e, &rules).map_err(|_| Flow::Halt("bad rules".into()))?;
    i.eval(&out)
}

pub fn b_replacerepeated(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("ReplaceRepeated", "argr");
        return Ok(unevaluated_call(i, "ReplaceRepeated", args));
    }
    let mut cur = i.eval(&args[0])?;
    let rules = i.eval(&args[1])?;
    for _ in 0..10000 {
        let (next, changed) = crate::pattern::replace_all(i, &cur, &rules).map_err(|_| Flow::Halt("bad rules".into()))?;
        if !changed {
            break;
        }
        cur = i.eval(&next)?;
    }
    Ok(cur)
}

pub fn b_replace(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Replace", "argr");
        return Ok(unevaluated_call(i, "Replace", args));
    }
    let e = i.eval(&args[0])?;
    let rules = i.eval(&args[1])?;
    let (out, _) = crate::pattern::replace_at_top(i, &e, &rules).map_err(|_| Flow::Halt("bad rules".into()))?;
    i.eval(&out)
}

// ---------- assignment ----------

pub fn b_set(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Set", "argr");
        return Ok(unevaluated_call(i, "Set", args));
    }
    let rhs = i.eval(&args[1])?;
    assign(i, &args[0], &rhs, false)
}

pub fn b_setdelayed(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("SetDelayed", "argr");
        return Ok(unevaluated_call(i, "SetDelayed", args));
    }
    assign(i, &args[0], &args[1], true)
}

fn assign(i: &mut Interp, lhs: &Expr, rhs: &Expr, delayed: bool) -> EvalRes {
    let sys = i.t.sys;
    let mut p = lhs.clone();
    if let Some((h, hargs)) = p.parts() {
        if h.is_symbol(sys.holdpattern) && hargs.len() == 1 {
            p = hargs[0].clone();
        }
    }
    let mut cond = None;
    if let Some((h, hargs)) = p.parts() {
        if h.is_symbol(sys.condition) && hargs.len() == 2 {
            cond = Some(hargs[1].clone());
            p = hargs[0].clone();
        }
    }
    match &p {
        Expr::Sym(id) => {
            if is_protected(i, *id) {
                {
                    let n = i.t.name_of(*id).to_string();
                    i.message(&n, "setp");
                }
                return Ok(rhs.clone());
            }
            i.t.data[*id].own = Some(if delayed {
                OwnValue::SetDelayed(rhs.clone())
            } else {
                OwnValue::Set(rhs.clone())
            });
            Ok(rhs.clone())
        }
        Expr::Node(nd) if nd.head.is_symbol(sys.part) => {
            // list[[i...]] = value
            if nd.args.is_empty() {
                i.message("Part", "ilsmn");
                return Ok(rhs.clone());
            }
            let base = i.eval(&nd.args[0])?;
            let mut idx = Vec::new();
            for a in &nd.args[1..] {
                idx.push(i.eval(a)?);
            }
            let newv = part_assign(i, &base, &idx, rhs)?;
            // Store into the container symbol.
            match &nd.args[0] {
                Expr::Sym(id) => {
                    if is_protected(i, *id) {
                        {
                    let n = i.t.name_of(*id).to_string();
                    i.message(&n, "setp");
                }
                        return Ok(rhs.clone());
                    }
                    i.t.data[*id].own = Some(OwnValue::Set(newv));
                }
                _ => {
                    i.message("Set", "setp");
                }
            }
            Ok(rhs.clone())
        }
        Expr::Node(nd) => {
            let Some(head_id) = nd.head.sym_id() else {
                i.message("Set", "setp");
                return Ok(rhs.clone());
            };
            if is_protected(i, head_id) {
                {
                let n = i.t.name_of(head_id).to_string();
                i.message(&n, "setp");
            }
                return Ok(rhs.clone());
            }
            let body = if delayed { rhs.clone() } else { rhs.clone() };
            // More specific (blank-free) rules sort before pattern rules,
            // keeping definition order otherwise — the common
            // `f[0] = 1; f[n_] := ...` idiom then works.
            let is_pat = crate::pattern::lhs_has_blanks(&p, &i.t);
            let pos = if is_pat {
                i.t.data[head_id].down.len()
            } else {
                i.t.data[head_id]
                    .down
                    .iter()
                    .position(|r| crate::pattern::lhs_has_blanks(&r.pat, &i.t))
                    .unwrap_or(i.t.data[head_id].down.len())
            };
            i.t.data[head_id].down.insert(pos, DownRule { pat: p.clone(), body, delayed, cond });
            if delayed {
                return Ok(Expr::Sym(sys.null));
            }
            Ok(rhs.clone())
        }
        _ => {
            i.message("Set", "setp");
            Ok(rhs.clone())
        }
    }
}

/// `tag /: lhs = rhs` and `tag /: lhs := rhs` — an up-value: the rule is stored
/// on `tag`, but its pattern describes an expression *containing* `tag`
/// (`f /: g[f[x_]] := x + 1`), so it fires when such an expression is evaluated.
fn tag_set(i: &mut Interp, args: &[Expr], delayed: bool) -> EvalRes {
    if args.len() != 3 {
        i.message("TagSet", "argr");
        return Ok(unevaluated_call(i, "TagSet", args));
    }
    let Some(tag) = args[0].sym_id() else {
        i.message("TagSet", "argt");
        return Ok(Expr::Sym(i.t.sys.null));
    };
    if is_protected(i, tag) {
        let n = i.t.name_of(tag).to_string();
        i.message(&n, "setp");
        return Ok(Expr::Sym(i.t.sys.null));
    }
    let (mut pat, mut cond) = (args[1].clone(), None);
    if let Some((h, hargs)) = pat.parts() {
        if h.is_symbol(i.t.sys.condition) && hargs.len() == 2 {
            cond = Some(hargs[1].clone());
            pat = hargs[0].clone();
        }
    }
    let body = if delayed { args[2].clone() } else { args[2].clone() };
    i.t.data[tag].up.push(DownRule { pat, body: body.clone(), delayed, cond });
    if delayed {
        return Ok(Expr::Sym(i.t.sys.null));
    }
    Ok(body)
}

/// `lhs ^= rhs` / `lhs ^:= rhs`: the tag is taken from the head of `lhs`, so
/// `f[x_] ^= x^2` tags `f` and fires when `f[...]` is evaluated.
fn upset(i: &mut Interp, args: &[Expr], delayed: bool) -> EvalRes {
    if args.len() != 2 {
        i.message("UpSet", "argr");
        return Ok(unevaluated_call(i, "UpSet", args));
    }
    let Some((h, _)) = args[0].parts() else {
        i.message("UpSet", "argt");
        return Ok(unevaluated_call(i, "UpSet", args));
    };
    let Some(tag) = h.sym_id() else {
        i.message("UpSet", "argt");
        return Ok(unevaluated_call(i, "UpSet", args));
    };
    let tagged = vec![Expr::Sym(tag), args[0].clone(), args[1].clone()];
    tag_set(i, &tagged, delayed)
}

pub fn b_upset(i: &mut Interp, args: &[Expr]) -> EvalRes {
    upset(i, args, false)
}

pub fn b_upsetdelayed(i: &mut Interp, args: &[Expr]) -> EvalRes {
    upset(i, args, true)
}

pub fn b_tagset(i: &mut Interp, args: &[Expr]) -> EvalRes {
    tag_set(i, args, false)
}

pub fn b_tagsetdelayed(i: &mut Interp, args: &[Expr]) -> EvalRes {
    tag_set(i, args, true)
}

pub fn b_unset(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Unset", "argr");
        return Ok(Expr::Sym(i.t.sys.null));
    }
    match &args[0] {
        Expr::Sym(id) => {
            if is_protected(i, *id) {
                {
                    let n = i.t.name_of(*id).to_string();
                    i.message(&n, "setp");
                }
                return Ok(Expr::Sym(i.t.sys.null));
            }
            i.t.data[*id].own = None;
            Ok(Expr::Sym(i.t.sys.null))
        }
        Expr::Node(nd) => {
            if let Some(head_id) = nd.head.sym_id() {
                i.t.data[head_id].down.retain(|r| !same_q(&r.pat, &args[0]));
            }
            Ok(Expr::Sym(i.t.sys.null))
        }
        _ => Ok(Expr::Sym(i.t.sys.null)),
    }
}

pub fn b_clear(i: &mut Interp, args: &[Expr]) -> EvalRes {
    for a in args {
        if let Some(id) = a.sym_id() {
            if is_protected(i, id) {
                {
                    let n = i.t.name_of(id).to_string();
                    i.message(&n, "clear");
                }
                continue;
            }
            i.t.data[id].own = None;
        }
    }
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_clearall(i: &mut Interp, args: &[Expr]) -> EvalRes {
    for a in args {
        if let Some(id) = a.sym_id() {
            if is_protected(i, id) {
                {
                    let n = i.t.name_of(id).to_string();
                    i.message(&n, "clear");
                }
                continue;
            }
            let d = &mut i.t.data[id];
            d.own = None;
            d.down.clear();
            d.defaults = vec![None, None, None];
        }
    }
    Ok(Expr::Sym(i.t.sys.null))
}

// ---------- scoping ----------

pub fn b_module(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Module", "argr");
        return Ok(unevaluated_call(i, "Module", args));
    }
    let sys = i.t.sys;
    let locals = match args[0].parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) => hargs.to_vec(),
        _ => vec![args[0].clone()],
    };
    i.module_counter += 1;
    let tag = i.module_counter;
    let mut renames: Vec<(usize, usize)> = Vec::new();
    for l in &locals {
        let id = match l.parts() {
            Some((h, hargs)) if h.is_symbol(sys.set) && hargs.len() == 2 => {
                hargs[0].sym_id()
            }
            _ => l.sym_id(),
        };
        if let Some(id) = id {
            let new_name = format!("{}${}", i.t.name_of(id), tag);
            let new_id = i.t.intern(&new_name);
            renames.push((id, new_id));
        }
    }
    // Evaluate initializers in the OUTER scope, then store in new symbols.
    for l in &locals {
        if let Some((h, hargs)) = l.parts() {
            if h.is_symbol(sys.set) && hargs.len() == 2 {
                if let Some(old_id) = hargs[0].sym_id() {
                    if let Some(&(_, new_id)) =
                        renames.iter().find(|(from, _)| *from == old_id)
                    {
                        let init = i.eval(&hargs[1])?;
                        i.t.data[new_id].own = Some(OwnValue::Set(init));
                    }
                }
            }
        }
    }
    let body = crate::pattern::rename_syms(&i.t, &args[1], &renames);
    i.eval(&body)
}

pub fn b_block(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Block", "argr");
        return Ok(unevaluated_call(i, "Block", args));
    }
    let sys = i.t.sys;
    let locals = match args[0].parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) => hargs.to_vec(),
        _ => vec![args[0].clone()],
    };
    let mut saved: Vec<(usize, Option<OwnValue>)> = Vec::new();
    for l in &locals {
        let (id, init) = match l.parts() {
            Some((h, hargs)) if h.is_symbol(sys.set) && hargs.len() == 2 => {
                match hargs[0].sym_id() {
                    Some(id) => {
                        let init = i.eval(&hargs[1])?;
                        (id, Some(init))
                    }
                    None => continue,
                }
            }
            _ => match l.sym_id() {
                Some(id) => (id, None),
                None => continue,
            },
        };
        saved.push((id, i.t.data[id].own.clone()));
        if let Some(init) = init {
            i.t.data[id].own = Some(OwnValue::Set(init));
        } else {
            i.t.data[id].own = None;
        }
    }
    let result = i.eval(&args[1]);
    for (id, own) in saved {
        i.t.data[id].own = own;
    }
    result
}

pub fn b_with(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("With", "argr");
        return Ok(unevaluated_call(i, "With", args));
    }
    let sys = i.t.sys;
    let locals = match args[0].parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) => hargs.to_vec(),
        _ => vec![args[0].clone()],
    };
    let mut names: Vec<usize> = Vec::new();
    let mut vals: Vec<Expr> = Vec::new();
    for l in &locals {
        if let Some((h, hargs)) = l.parts() {
            if h.is_symbol(sys.set) && hargs.len() == 2 {
                if let Some(id) = hargs[0].sym_id() {
                    let v = i.eval(&hargs[1])?;
                    names.push(id);
                    vals.push(v);
                }
            }
        }
    }
    let body = crate::pattern::subst_syms_all(&i.t, &args[1], &names, &vals);
    i.eval(&body)
}

// ---------- output / side channels ----------

pub fn b_out(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(Expr::Sym(i.t.sys.null));
    }
    let n = match i.eval(&args[0])?.to_i64_pub() {
        Some(n) => n,
        None => return Ok(Expr::Sym(i.t.sys.null)),
    };
    let len = i.out_history.len() as i64;
    let idx = if n > 0 { n - 1 } else { len + n };
    if idx < 0 || idx >= len {
        return Ok(Expr::Sym(i.t.sys.null));
    }
    Ok(i.out_history[idx as usize].clone())
}

pub fn b_print(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut parts = Vec::new();
    for a in args {
        let v = i.eval(a)?;
        // Print writes a bare string without the quotes that OutputForm adds.
        match &v {
            Expr::Str(msg) => parts.push(msg.to_string()),
            _ => parts.push(crate::fmt::output_form(&i.t, &v)),
        }
    }
    i.prints.push(parts.join(""));
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_sow(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Sow", "argr");
        return Ok(unevaluated_call(i, "Sow", args));
    }
    let v = i.eval(&args[0])?;
    i.sowed.push(v.clone());
    Ok(v)
}

pub fn b_reap(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("Reap", "argr");
        return Ok(unevaluated_call(i, "Reap", args));
    }
    let saved = std::mem::take(&mut i.sowed);
    let v = i.eval(&args[0]);
    let sowed = std::mem::replace(&mut i.sowed, saved);
    match v {
        Ok(v) => {
            let list = Expr::node(Expr::Sym(i.t.sys.list), sowed);
            Ok(Expr::node(
                Expr::Sym(i.t.sys.list),
                vec![v, Expr::node(Expr::Sym(i.t.sys.list), vec![list])],
            ))
        }
        Err(e) => Err(e),
    }
}

// ---------- Part ----------

pub fn b_part(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("Part", "ilsmn");
        return Ok(unevaluated_call(i, "Part", args));
    }
    let base = i.eval(&args[0])?;
    let mut idx = Vec::new();
    for a in &args[1..] {
        idx.push(i.eval(a)?);
    }
    part_get(i, &base, &idx)
}

pub fn part_get(i: &mut Interp, e: &Expr, idx: &[Expr]) -> EvalRes {
    let sys = i.t.sys;
    if idx.is_empty() {
        return Ok(e.clone());
    }
    // Association keys
    if let Expr::Node(nd) = e {
        if nd.head.is_symbol(sys.association) && idx.len() == 1 {
            for pair in nd.args.chunks(2) {
                if pair.len() == 2 {
                    if let Some((h, hargs)) = pair[0].parts() {
                        if h.is_symbol(sys.rule) && same_q(&hargs[0], &idx[0]) {
                            return Ok(pair[1].clone());
                        }
                    }
                }
            }
            i.message("Part", "partw");
            return Ok(Expr::node(Expr::Sym(sys.missing), vec![sym_(i, "KeyAbsent")]));
        }
    }
    let items = match e.parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) => hargs.to_vec(),
        _ => {
            i.message("Part", "normal");
            return Ok(unevaluated_call(i, "Part", &args_of(e, idx)));
        }
    };
    // Span
    if let Some((h, hargs)) = idx[0].parts() {
        if h.is_symbol(sys.span) {
            let (lo, hi, step) = span_bounds(&hargs, items.len());
            let mut out = Vec::new();
            let mut k = lo;
            while k <= hi && step > 0 {
                if let Some(v) = items.get(k) {
                    if idx.len() > 1 {
                        out.push(part_get(i, v, &idx[1..])?);
                    } else {
                        out.push(v.clone());
                    }
                }
                k += step;
            }
            return Ok(Expr::node(Expr::Sym(sys.list), out));
        }
    }
    // All
    if idx[0].is_symbol(sys.all) {
        let mut out = Vec::new();
        for o in &items {
            out.push(part_get(i, o, &idx[1..])?);
        }
        return Ok(Expr::node(Expr::Sym(sys.list), out));
    }
    // A list of indices: `list[[{3, 1, 2}]]` is `{list[[3]], list[[1]], list[[2]]}`,
    // which is how `list[[Ordering[list]]]` sorts a list.
    if let Some((h, hargs)) = idx[0].parts() {
        if h.is_symbol(sys.list) {
            let mut out = Vec::with_capacity(hargs.len());
            for k in hargs {
                let mut sub = Vec::with_capacity(idx.len());
                sub.push(k.clone());
                sub.extend_from_slice(&idx[1..]);
                out.push(part_get(i, e, &sub)?);
            }
            return Ok(Expr::node(Expr::Sym(sys.list), out));
        }
    }
    let Some(n) = expr_to_int(&idx[0]) else {
        i.message("Part", "ilsmn");
        return Ok(unevaluated_call(i, "Part", &args_of(e, idx)));
    };
    let pos = if n > 0 { n - 1 } else { items.len() as i64 + n };
    if pos < 0 || pos >= items.len() as i64 {
        i.message("Part", "partw");
        return Ok(unevaluated_call(i, "Part", &args_of(e, idx)));
    }
    part_get(i, &items[pos as usize], &idx[1..])
}

fn args_of(e: &Expr, idx: &[Expr]) -> Vec<Expr> {
    let mut v = vec![e.clone()];
    v.extend(idx.iter().cloned());
    v
}

/// Span[min, max, step?] bounds (0-based, inclusive).
fn span_bounds(span_args: &[Expr], len: usize) -> (usize, usize, usize) {
    let lo = match span_args[0].to_i64_pub() {
        Some(v) if v > 0 => (v - 1) as usize,
        Some(v) if v < 0 => (len as i64 + v).max(0) as usize,
        _ => 0,
    };
    let hi = match span_args.get(1) {
        None => len.saturating_sub(1),
        Some(e) => match e.to_i64_pub() {
            Some(v) if v > 0 => ((v - 1).min(len as i64 - 1)).max(-1) as usize,
            Some(v) if v < 0 => (len as i64 + v).max(-1).max(0) as usize,
            _ => 0,
        },
    };
    let step = match span_args.get(2) {
        Some(e) => e.to_i64_pub().unwrap_or(1).max(1) as usize,
        None => 1,
    };
    (lo, hi, step)
}

pub fn part_assign(i: &mut Interp, base: &Expr, idx: &[Expr], val: &Expr) -> EvalRes {
    let sys = i.t.sys;
    if idx.is_empty() {
        return Ok(val.clone());
    }
    let items = match base.parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) => hargs.to_vec(),
        _ => {
            i.message("Part", "normal");
            return Ok(base.clone());
        }
    };
    let Some(n) = expr_to_int(&idx[0]) else {
        i.message("Part", "ilsmn");
        return Ok(base.clone());
    };
    let pos = if n > 0 { n - 1 } else { items.len() as i64 + n };
    if pos < 0 || pos >= items.len() as i64 {
        i.message("Part", "ilsmn");
        return Ok(base.clone());
    }
    let mut new_items = items.clone();
    new_items[pos as usize] = part_assign(i, &items[pos as usize], &idx[1..], val)?;
    Ok(Expr::node(Expr::Sym(sys.list), new_items))
}