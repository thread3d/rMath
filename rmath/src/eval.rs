//! The evaluator: standard evaluation loop, hold attributes, Sequence and
//! Unevaluated splicing, rule application (down-values), built-in dispatch,
//! function application with slots, and the MatchCtx bridge to the matcher.

use crate::expr::{
    canon_cmp, has, same_q, A_FLAT, A_HOLDALL, A_HOLDALLCOMPLETE, A_HOLDFIRST, A_HOLDREST,
    A_LISTABLE, A_ORDERLESS, A_PROTECTED, A_SEQUENCEHOLD, Expr, OwnValue, SymbolTable,
};
use crate::pattern::{match_call_args, subst_bindings, Bindings, MatchCtx};
use std::cmp::Ordering;

#[derive(Debug)]
pub enum Flow {
    /// Throw[value, tag?] — caught by Catch only.
    Throw(Expr, Option<Expr>),
    /// Break[] — caught by loops.
    Break,
    /// Continue[] — caught by loops.
    Continue,
    /// Return[value?] — caught by function application and the top level.
    Return(Option<Expr>),
    /// Abort[] / TimeConstrained timeout — caught by TimeConstrained and
    /// Check, reported as `$Aborted` at the top level.
    Abort,
    /// Fatal abort (e.g. recursion limit).
    Halt(String),
}

pub type EvalRes = Result<Expr, Flow>;

/// Alternative names accepted for special functions. Mathematica itself is
/// case- and name-strict here (it has no `Si`), but these abbreviations are
/// standard elsewhere, so they are accepted as aliases.
pub fn alias_target(name: &str) -> Option<&'static str> {
    Some(match name {
        "Si" => "SinIntegral",
        "Ci" => "CosIntegral",
        "Ei" => "ExpIntegralEi",
        "li" => "LogIntegral",
        "erf" => "Erf",
        "erfc" => "Erfc",
        "Shi" => "SinhIntegral",
        "Chi" => "CoshIntegral",
        _ => return None,
    })
}

pub struct Interp {
    pub t: SymbolTable,
    pub depth: usize,
    pub out_history: Vec<Expr>,
    /// Number of evaluations submitted (drives the In[n]:= labels).
    pub in_count: usize,
    pub prints: Vec<String>,
    pub sowed: Vec<Expr>,
    pub module_counter: u64,
    pub unique_counter: u64,
    pub rng: u64,
    /// (tag, text) messages collected during the current input.
    pub messages: Vec<(String, String)>,
    pub rec_limit: usize,
    pub iter_limit: usize,
    /// Wall-clock deadline for `TimeConstrained`; checked in the eval loop.
    pub deadline: Option<std::time::Instant>,
    /// Suppress all messages (Off[]) ...
    pub messages_off: bool,
    /// ... or just those from these function names (Off[f]).
    pub off_names: Vec<String>,
    /// Assumptions pushed by Assuming[...] for Simplify.
    pub assumptions: Vec<Expr>,
    /// Active derivation recorder (`Steps`/`Trace`); None when not recording.
    pub steps: Option<Vec<(String, Expr)>>,
    /// Active general evaluation trace (`Trace`, and the `Steps` fallback).
    pub eval_trace: Option<Vec<Expr>>,
}

impl Interp {
    pub fn new() -> Interp {
        let mut i = Interp {
            t: SymbolTable::new(),
            depth: 0,
            out_history: Vec::new(),
            in_count: 0,
            prints: Vec::new(),
            sowed: Vec::new(),
            module_counter: 0,
            unique_counter: 0,
            rng: 0x2545_F491_4F6C_DD1D,
            messages: Vec::new(),
            rec_limit: 4096,
            iter_limit: 200_000,
            deadline: None,
            messages_off: false,
            off_names: Vec::new(),
            assumptions: Vec::new(),
            steps: None,
            eval_trace: None,
        };
        i.setup_system();
        i
    }

    fn setup_system(&mut self) {
        let sys = self.t.sys;
        for id in [sys.r#true, sys.r#false] {
            self.t.data[id].attrs |= A_PROTECTED | crate::expr::A_CONSTANT;
        }
        for id in [sys.null, sys.indeterminate, sys.infinity, sys.complexinfinity] {
            self.t.data[id].attrs |= A_PROTECTED | crate::expr::A_CONSTANT;
        }
        for name in ["Pi", "E", "GoldenRatio", "EulerGamma", "Catalan"] {
            let id = self.t.intern(name);
            self.t.data[id].attrs |= A_PROTECTED | crate::expr::A_CONSTANT;
        }
        // Degree = Pi/180
        let degree = self.t.intern("Degree");
        self.t.data[degree].attrs |= A_PROTECTED | crate::expr::A_CONSTANT;
        let pi_id = self.t.intern("Pi");
        self.t.data[degree].own = Some(OwnValue::Set(Expr::node(
            Expr::Sym(sys.times),
            vec![
                Expr::rat(crate::rational::Rat::new(
                    crate::bigint::BigInt::from_i64(1),
                    crate::bigint::BigInt::from_i64(180),
                )),
                Expr::Sym(pi_id),
            ],
        )));
        // I = Complex[0, 1]
        let iv = Expr::cnum(
            crate::expr::NumVal::R(crate::rational::Rat::from_i64(0)),
            crate::expr::NumVal::R(crate::rational::Rat::from_i64(1)),
        );
        let ii = self.t.intern("I");
        self.t.data[ii].attrs |= A_PROTECTED | crate::expr::A_CONSTANT;
        self.t.data[ii].own = Some(OwnValue::Set(iv));
        // $Assumptions defaults to True; Simplify consults it (and Assuming).
        let assum = self.t.intern("$Assumptions");
        self.t.data[assum].own = Some(OwnValue::Set(Expr::Sym(sys.r#true)));
        // $Version is now typable (leading $ lexes) and reports the version.
        let ver = self.t.intern("$Version");
        self.t.data[ver].own = Some(OwnValue::Set(Expr::from_str(
            "rMath 0.1.3 (stand-alone Mathematica-style system, Rust)",
        )));
        // Register built-ins from the registry.
        for (idx, def) in crate::builtins::BUILTINS.iter().enumerate() {
            let id = self.t.intern(def.name);
            self.t.data[id].attrs |= A_PROTECTED | def.attrs;
            self.t.data[id].builtin = Some(idx);
            if !def.usage.is_empty() {
                self.t.data[id].usage = Some(def.usage.to_string());
            }
        }
    }

    // ---------- messages ----------
    pub fn message(&mut self, name: &str, tag: &str) {
        if self.messages_off {
            return;
        }
        if !self.off_names.is_empty() {
            let full = format!("{}::{}", name, tag);
            if self.off_names.iter().any(|n| *n == name || *n == full) {
                return;
            }
        }
        let text = crate::builtins::message_text(tag, name);
        self.messages.push((format!("{}::{}", name, tag), text));
    }

    // ---------- evaluation ----------

    /// Record one derivation step, if a recorder is active.
    pub fn step(&mut self, label: &str, e: &Expr) {
        if let Some(v) = self.steps.as_mut() {
            v.push((label.to_string(), e.clone()));
        }
    }

    pub fn recording(&self) -> bool {
        self.steps.is_some()
    }

    /// True (and clears the deadline) once a `TimeConstrained` deadline has
    /// passed. Long-running built-in loops call this too.
    pub fn timed_out(&mut self) -> bool {
        if let Some(dl) = self.deadline {
            if std::time::Instant::now() >= dl {
                self.deadline = None;
                return true;
            }
        }
        false
    }

    pub fn eval(&mut self, e0: &Expr) -> EvalRes {
        let mut cur = e0.clone();
        let mut count = 0usize;
        loop {
            if self.timed_out() {
                return Err(Flow::Abort);
            }
            count += 1;
            if count > self.iter_limit {
                self.message("$IterationLimit", "itlim");
                return Ok(cur);
            }
            self.depth += 1;
            if self.depth > self.rec_limit {
                self.depth -= 1;
                return Err(Flow::Halt(format!(
                    "$RecursionLimit::reclim: Recursion depth of {} exceeded.",
                    self.rec_limit
                )));
            }
            let r = self.eval_once(&cur);
            self.depth -= 1;
            let (next, changed) = r?;
            if !changed {
                return Ok(next);
            }
            if let Some(tr) = self.eval_trace.as_mut() {
                if tr.len() < 2000 {
                    tr.push(next.clone());
                }
            }
            cur = next;
        }
    }

    fn eval_once(&mut self, e: &Expr) -> Result<(Expr, bool), Flow> {
        match e {
            Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::BigReal(_) | Expr::Str(_)
            | Expr::Cnum(_) => Ok((e.clone(), false)),
            Expr::Sym(id) => {
                let has_delayed = matches!(&self.t.data[*id].own, Some(OwnValue::SetDelayed(_)));
                if has_delayed {
                    let body = match &self.t.data[*id].own {
                        Some(OwnValue::SetDelayed(b)) => b.clone(),
                        _ => unreachable!(),
                    };
                    let v = self.eval(&body)?;
                    let ch = !same_q(&v, e);
                    return Ok((v, ch));
                }
                if let Some(OwnValue::Set(v)) = &self.t.data[*id].own {
                    let val = v.clone();
                    let ch = !same_q(&val, e);
                    return Ok((val, ch));
                }
                Ok((e.clone(), false))
            }
            Expr::Node(nd) => self.eval_node(&nd.head, &nd.args, e),
        }
    }

    fn eval_node(&mut self, head: &Expr, args: &[Expr], orig: &Expr) -> Result<(Expr, bool), Flow> {
        // 1. Evaluate the head; own values may rewrite it (f = g; f[1] -> g[1]).
        let (head, hchanged) = self.eval_once(head)?;
        if hchanged {
            return Ok((Expr::node(head, args.to_vec()), true));
        }
        // 2. Pure function application.
        if let Expr::Node(fh) = &head {
            if fh.head.is_symbol(self.t.sys.function) {
                let v = self.apply_function(&head, args)?;
                let ch = !same_q(&v, orig);
                    return Ok((v, ch));
            }
        }
        // 3. Evaluate arguments according to hold attributes.
        let (mut vargs, achanged) = self.eval_args(head.sym_id(), args)?;
        // 4. Attribute normalization (Flat / Orderless).
        let nchanged = self.normalize(head.sym_id(), &mut vargs);
        let changed_pre = hchanged || achanged || nchanged;
        // 5. Dispatch: down-values, then built-ins (plain symbol heads only).
        if let Some(id) = head.sym_id() {
            if let Some(v) = self.try_downvalues(id, &vargs)? {
                let ch = !same_q(&v, orig);
                return Ok((v, ch));
            }
            let attrs = self.t.attrs(id);
            if let Some(bi) = self.t.data[id].builtin {
                let attrs = self.t.data[id].attrs;
                let res = self.call_builtin(id, bi, attrs, &vargs)?;
                let ch = !same_q(&res, orig);
                return Ok((res, ch));
            }
            // User symbols with Listable attribute thread over lists.
            if has(attrs, A_LISTABLE)
                && vargs.iter().any(|a| a.is_list(&self.t))
                && !vargs.is_empty()
            {
                if let Some(res) = self.thread_listable(id, &vargs)? {
                    let ch = !same_q(&res, orig);
                    return Ok((res, ch));
                }
            }
            // Abbreviations used by other computer-algebra systems (SymPy,
            // Maxima, scipy, mpmath) and by hand-written mathematics resolve to
            // the canonical Wolfram names: Si -> SinIntegral, Ei ->
            // ExpIntegralEi, li -> LogIntegral, erf -> Erf, ...
            if self.t.data[id].builtin.is_none() && self.t.data[id].down.is_empty() {
                let name = self.t.name_of(id).to_string();
                if let Some(target) = alias_target(&name) {
                    let tid = self.t.intern(target);
                    return self.eval_node(&Expr::Sym(tid), args, orig);
                }
            }
            // Nothing matched: if the name differs from a real function only by
            // case, say so (typing sin[x] instead of Sin[x] is an easy slip, and
            // the message is emitted once per name).
            if self.t.data[id].builtin.is_none() && self.t.data[id].down.is_empty() {
                let name = self.t.name_of(id).to_string();
                if !name.is_empty() && !name.ends_with('$') {
                    if let Some(real) = self.t.case_insensitive_match(&name) {
                        if real != name {
                            let tag = format!("{}::case", name);
                            if !self.messages.iter().any(|(t, _)| *t == tag) {
                                self.messages.push((
                                    tag,
                                    format!(
                                        "{} is not a defined function; did you mean {}?",
                                        name, real
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok((Expr::node(head, vargs), changed_pre))
    }

    /// Evaluate arguments respecting Hold* attributes; splice Sequence and
    /// strip Unevaluated; honor Evaluate inside held positions.
    fn eval_args(
        &mut self,
        head_id: Option<usize>,
        args: &[Expr],
    ) -> Result<(Vec<Expr>, bool), Flow> {
        let sys = self.t.sys;
        let attrs = match head_id {
            Some(id) => self.t.attrs(id),
            None => 0,
        };
        let complete = has(attrs, A_HOLDALLCOMPLETE);
        let holdall = has(attrs, A_HOLDALL);
        let holdfirst = has(attrs, A_HOLDFIRST);
        let holdrest = has(attrs, A_HOLDREST);
        let seqhold = has(attrs, A_SEQUENCEHOLD);
        let mut out: Vec<Expr> = Vec::with_capacity(args.len());
        let mut changed = false;

        let splice = |out: &mut Vec<Expr>, v: &Expr, changed: &mut bool| {
            if !seqhold {
                if let Some((h, hargs)) = v.parts() {
                    if h.is_symbol(sys.sequence) {
                        for x in hargs {
                            out.push(x.clone());
                        }
                        *changed = true;
                        return;
                    }
                }
            }
            out.push(v.clone());
        };

        for (pos, a) in args.iter().enumerate() {
            let held = complete || holdall || (pos == 0 && holdfirst) || (pos > 0 && holdrest);
            // Unevaluated[v] — strip and use raw (one layer).
            if !complete {
                if let Some((h, hargs)) = a.parts() {
                    if h.is_symbol(sys.unevaluated) && hargs.len() == 1 {
                        changed = true;
                        if !seqhold {
                            if let Some((h2, a2)) = hargs[0].parts() {
                                if h2.is_symbol(sys.sequence) {
                                    for x in a2 {
                                        out.push(x.clone());
                                    }
                                    continue;
                                }
                            }
                        }
                        out.push(hargs[0].clone());
                        continue;
                    }
                }
            }
            if held {
                // Evaluate[...] inside held positions forces evaluation.
                if !complete {
                    if let Some((h, hargs)) = a.parts() {
                        if h.is_symbol(sys.evaluate) && hargs.len() == 1 {
                            let v = self.eval(&hargs[0])?;
                            changed = true;
                            splice(&mut out, &v, &mut changed);
                            continue;
                        }
                    }
                }
                // Sequence splices even under Hold (unless SequenceHold).
                if !seqhold {
                    if let Some((h, _)) = a.parts() {
                        if h.is_symbol(sys.sequence) {
                            let (_, hargs) = a.parts().unwrap();
                            for x in hargs {
                                out.push(x.clone());
                            }
                            changed = true;
                            continue;
                        }
                    }
                }
                out.push(a.clone());
                continue;
            }
            let v = self.eval(a)?;
            splice(&mut out, &v, &mut changed);
        }
        Ok((out, changed))
    }

    /// Flatten Flat heads and sort Orderless heads.
    fn normalize(&mut self, head_id: Option<usize>, args: &mut Vec<Expr>) -> bool {
        let attrs = match head_id {
            Some(id) => self.t.attrs(id),
            None => 0,
        };
        let mut changed = false;
        if has(attrs, A_FLAT) {
            if let Some(hid) = head_id {
                let mut out = Vec::with_capacity(args.len());
                for a in args.iter() {
                    if let Expr::Node(nd) = a {
                        if nd.head.is_symbol(hid) {
                            for x in &nd.args {
                                out.push(x.clone());
                            }
                            changed = true;
                            continue;
                        }
                    }
                    out.push(a.clone());
                }
                *args = out;
            }
        }
        if has(attrs, A_ORDERLESS) && args.len() > 1 {
            let t = &self.t;
            let mut s: Vec<Expr> = args.clone();
            if head_id == Some(t.sys.plus) {
                // Plus uses Mathematica's polynomial ordering (constants first,
                // then monomials by exponent vector from the last variable to
                // the first). Other Orderless heads keep the plain canonical
                // order, so `Times` still prints its numeric coefficient first.
                let mut vars: Vec<Expr> = Vec::new();
                for a in &s {
                    collect_free_syms(t, a, &mut vars);
                }
                vars.sort_by(|a, b| canon_cmp(t, a, b));
                s.sort_by(|a, b| plus_cmp(t, &vars, a, b));
            } else if head_id == Some(t.sys.times) {
                s.sort_by(|a, b| times_cmp(t, a, b));
            } else {
                s.sort_by(|a, b| canon_cmp(t, a, b));
            }
            let same = args.len() == s.len()
                && args.iter().zip(s.iter()).all(|(a, b)| same_q(a, b));
            if !same {
                changed = true;
            }
            *args = s;
        }
        changed
    }

    fn try_downvalues(&mut self, head_id: usize, args: &[Expr]) -> Result<Option<Expr>, Flow> {
        let rules: Vec<crate::expr::DownRule> = self.t.data[head_id].down.clone();
        for rule in rules {
            let Some((pats, cond)) = split_down_pattern(&self.t, &rule.pat, head_id) else {
                continue;
            };
            let Some(bind) = match_call_args(self, head_id, &pats, args) else {
                continue;
            };
            if let Some(c) = &cond {
                let cond_e = subst_bindings(&self.t.sys, c, &bind);
                match self.eval(&cond_e) {
                    Ok(v) if v.is_symbol(self.t.sys.r#true) => {}
                    Ok(_) => continue,
                    Err(Flow::Throw(v, tag)) => return Err(Flow::Throw(v, tag)),
                    Err(_) => continue,
                }
            }
            let body = subst_bindings(&self.t.sys, &rule.body, &bind);
            // Top-level Condition in the body gates the rule.
            let effective = if let Some((h, hargs)) = body.parts() {
                if h.is_symbol(self.t.sys.condition) && hargs.len() == 2 {
                    let c = self.eval(&hargs[1])?;
                    if c.is_symbol(self.t.sys.r#true) {
                        hargs[0].clone()
                    } else if c.is_symbol(self.t.sys.r#false) {
                        continue;
                    } else {
                        return Ok(Some(body));
                    }
                } else {
                    body
                }
            } else {
                body
            };
            // Return[expr] inside a function body returns from that function
            // (previously it unwound all the way out, aborting the caller).
            let v = match self.eval(&effective) {
                Ok(v) => v,
                Err(Flow::Return(Some(v))) => v,
                Err(Flow::Return(None)) => Expr::Sym(self.t.sys.null),
                Err(e) => return Err(e),
            };
            return Ok(Some(v));
        }
        Ok(None)
    }

    fn call_builtin(
        &mut self,
        sym_id: usize,
        idx: usize,
        attrs: u32,
        args: &[Expr],
    ) -> EvalRes {
        let def = &crate::builtins::BUILTINS[idx];
        let name = def.name;
        // Central arity guard: a call with too few arguments is reported and
        // left unevaluated rather than reaching an implementation that would
        // index past the end of `args` (which used to panic and abort the
        // whole process, losing the session).
        let min = crate::builtins::min_args(name);
        if args.len() < min {
            self.message(name, "argr");
            return Ok(Expr::node(Expr::Sym(sym_id), args.to_vec()));
        }
        // Listable threading for numeric/list functions.
        if has(attrs, A_LISTABLE) && args.iter().any(|a| a.is_list(&self.t)) {
            if let Some(res) = self.thread_listable(sym_id, args)? {
                return Ok(res);
            }
        }
        let f = def.f;
        // Backstop: any panic inside a built-in becomes an unevaluated call plus
        // a message, instead of tearing down the kernel.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self, args))) {
            Ok(r) => r,
            Err(payload) => {
                let detail = panic_text(&payload);
                self.messages.push((
                    format!("{}::panic", name),
                    format!("internal error while evaluating {}: {}", name, detail),
                ));
                Ok(Expr::node(Expr::Sym(sym_id), args.to_vec()))
            }
        }
    }

    /// Thread f over List arguments (Listable). None if threading impossible.
    fn thread_listable(&mut self, sym_id: usize, args: &[Expr]) -> Result<Option<Expr>, Flow> {
        let mut lists: Vec<usize> = Vec::new();
        let mut len: Option<usize> = None;
        for (i, a) in args.iter().enumerate() {
            if a.is_list(&self.t) {
                let l = a.list_len();
                match len {
                    None => len = Some(l),
                    Some(p) if p == l => {}
                    Some(_) => {
                        self.message("General", "dims");
                        return Ok(None);
                    }
                }
                lists.push(i);
            }
        }
        if lists.is_empty() {
            return Ok(None);
        }
        let n = len.unwrap();
        let mut rows = Vec::with_capacity(n);
        for k in 0..n {
            let mut one = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                if lists.contains(&i) {
                    if let Expr::Node(nd) = a {
                        one.push(nd.args[k].clone());
                    }
                } else {
                    one.push(a.clone());
                }
            }
            let call = Expr::node(Expr::Sym(sym_id), one);
            rows.push(self.eval(&call)?);
        }
        Ok(Some(Expr::node(Expr::Sym(self.t.sys.list), rows)))
    }

    /// Apply a Function node to arguments.
    pub fn apply_function(&mut self, f: &Expr, args: &[Expr]) -> EvalRes {
        let Some((fh, fargs)) = f.parts() else {
            return Ok(f.clone());
        };
        if !fh.is_symbol(self.t.sys.function) || fargs.is_empty() || fargs.len() > 3 {
            return Ok(f.clone());
        }
        let sys = self.t.sys;
        let body = match fargs.len() {
            1 => subst_slots(&self.t, &fargs[0], args, f, true)?,
            _ => {
                // Function[vars, body] or Function[vars, body, attrs]
                let vars = &fargs[0];
                let names: Vec<usize> = if let Some((h, hargs)) = vars.parts() {
                    if h.is_symbol(sys.list) {
                        hargs
                            .iter()
                            .filter_map(|a| a.sym_id())
                            .collect()
                    } else {
                        vars.sym_id().into_iter().collect()
                    }
                } else {
                    vars.sym_id().into_iter().collect()
                };
                let body = subst_syms(&self.t, &fargs[1], &names, args);
                body
            }
        };
        match self.eval(&body) {
            Ok(v) => Ok(v),
            Err(Flow::Return(v)) => Ok(v.unwrap_or(Expr::Sym(sys.null))),
            Err(e) => Err(e),
        }
    }
}

/// Human-readable text for a panic payload caught by `call_builtin`.
fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unexpected internal panic".to_string()
    }
}

/// Strip HoldPattern / split top-level Condition from a stored rule lhs.
pub fn split_down_pattern(
    t: &SymbolTable,
    pat: &Expr,
    head_id: usize,
) -> Option<(Vec<Expr>, Option<Expr>)> {
    let mut p = pat;
    if let Some((h, args)) = p.parts() {
        if h.is_symbol(t.sys.holdpattern) && args.len() == 1 {
            p = &args[0];
        }
    }
    let mut cond = None;
    if let Some((h, args)) = p.parts() {
        if h.is_symbol(t.sys.condition) && args.len() == 2 {
            cond = Some(args[1].clone());
            let inner = &args[0];
            let (h2, a2) = inner.parts()?;
            if h.is_symbol(t.sys.condition) {
                return None;
            }
            if h2.is_symbol(head_id) {
                return Some((a2.to_vec(), cond));
            }
            return None;
        }
    }
    let (h, a) = p.parts()?;
    if h.is_symbol(head_id) {
        Some((a.to_vec(), cond))
    } else {
        None
    }
}

/// Replace Slot[n] / SlotSequence[n] / Slot[0] in a Function body.
pub fn subst_slots(
    t: &SymbolTable,
    body: &Expr,
    args: &[Expr],
    fexpr: &Expr,
    with_seq: bool,
) -> Result<Expr, Flow> {
    let sys = t.sys;
    if let Some((h, hargs)) = body.parts() {
        if h.is_symbol(sys.slot) && hargs.len() == 1 {
            if let Expr::Int(n) = &hargs[0] {
                let ni = n.to_i64().unwrap_or(0);
                if ni == 0 {
                    return Ok(fexpr.clone());
                }
                let idx = ni - 1;
                if (idx as usize) < args.len() {
                    return Ok(args[idx as usize].clone());
                }
                return Ok(body.clone());
            }
        }
        if h.is_symbol(sys.slotsequence) && hargs.len() == 1 {
            if let Expr::Int(n) = &hargs[0] {
                let ni = n.to_i64().unwrap_or(1);
                if ni >= 1 && (ni as usize) <= args.len() {
                    let vs: Vec<Expr> = args[(ni as usize - 1)..].to_vec();
                    return Ok(Expr::node(Expr::Sym(sys.sequence), vs));
                }
            }
        }
        let new_head = subst_slots(t, h, args, fexpr, with_seq)?;
        let mut new_args = Vec::with_capacity(hargs.len());
        for a in hargs {
            let av = subst_slots(t, a, args, fexpr, with_seq)?;
            new_args.push(av);
        }
        return Ok(Expr::node(new_head, new_args));
    }
    Ok(body.clone())
}

/// Replace listed symbols with expressions (Function[{x, y}, body] apply).
pub fn subst_syms(t: &SymbolTable, body: &Expr, names: &[usize], vals: &[Expr]) -> Expr {
    if names.is_empty() {
        return body.clone();
    }
    if let Expr::Sym(id) = body {
        for (k, n) in names.iter().enumerate() {
            if *n == *id {
                if let Some(v) = vals.get(k) {
                    return v.clone();
                }
            }
        }
        return body.clone();
    }
    if let Some((h, hargs)) = body.parts() {
        let new_head = subst_syms(t, h, names, vals);
        let new_args: Vec<Expr> = hargs
            .iter()
            .map(|a| subst_syms(t, a, names, vals))
            .collect();
        return Expr::node(new_head, new_args);
    }
    body.clone()
}

/// Free symbols of `e` (argument positions only; heads are not variables), in
/// discovery order. Used to build the variable list for `Plus` ordering.
fn collect_free_syms(t: &SymbolTable, e: &Expr, out: &mut Vec<Expr>) {
    match e {
        Expr::Sym(id) => {
            if !has(t.attrs(*id), crate::expr::A_CONSTANT)
                && !out.iter().any(|v| matches!(v, Expr::Sym(i) if i == id))
            {
                out.push(e.clone());
            }
        }
        Expr::Node(nd) => {
            for a in &nd.args {
                collect_free_syms(t, a, out);
            }
        }
        _ => {}
    }
}

/// The exponent vector of `e` as a monomial in `vars`, or None when `e` is not
/// a polynomial monomial (so it cannot take part in the polynomial order).
/// Numeric atoms and constant symbols (Pi, E, …) are coefficients: zero in
/// every position.
fn monomial_exponents(t: &SymbolTable, vars: &[Expr], e: &Expr) -> Option<Vec<i64>> {
    let zeros = || vec![0i64; vars.len()];
    match e {
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::BigReal(_) | Expr::Cnum(_) => {
            Some(zeros())
        }
        Expr::Sym(id) => {
            if let Some(k) = vars.iter().position(|v| v.is_symbol(*id)) {
                let mut v = zeros();
                v[k] = 1;
                Some(v)
            } else if has(t.attrs(*id), crate::expr::A_CONSTANT) {
                Some(zeros())
            } else {
                None
            }
        }
        Expr::Node(nd) => {
            if nd.head.is_symbol(t.sys.times) {
                let mut acc = zeros();
                for f in &nd.args {
                    let part = monomial_exponents(t, vars, f)?;
                    for (a, p) in acc.iter_mut().zip(part.iter()) {
                        *a += *p;
                    }
                }
                Some(acc)
            } else if nd.head.is_symbol(t.sys.power) && nd.args.len() == 2 {
                if let Some(base) = nd.args[0].sym_id() {
                    if let Some(k) = vars.iter().position(|v| v.is_symbol(base)) {
                        // A power of a variable: non-negative integer exponents
                        // only (negative powers are not polynomial monomials).
                        let n = crate::builtins::core::expr_to_int(&nd.args[1])?;
                        if n < 0 {
                            return None;
                        }
                        let mut v = zeros();
                        v[k] = n;
                        return Some(v);
                    }
                    // A constant base (`Pi^k`, `E^x`) is a coefficient.
                    if has(t.attrs(base), crate::expr::A_CONSTANT) {
                        return Some(zeros());
                    }
                    return None;
                }
                // A numeric base (`2^k`, `Sqrt[2]` = `2^(1/2)`) is a coefficient.
                if matches!(
                    nd.args[0],
                    Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::Cnum(_)
                ) {
                    return Some(zeros());
                }
                None
            } else if t.get("Exp").map_or(false, |id| nd.head.is_symbol(id)) {
                // `Exp[x]` is `E^x`: a coefficient for the polynomial order, so
                // `-Exp[x] + Exp[x] x` (not `Exp[x] x - Exp[x]`).
                Some(zeros())
            } else {
                None
            }
        }
        Expr::Str(_) => None,
    }
}

/// Compare the exponent vectors in the reverse variable order (last variable
/// first), ascending — the order Mathematica uses for `Plus` terms.
fn cmp_exponents(x: &[i64], y: &[i64]) -> Ordering {
    for i in (0..x.len().min(y.len())).rev() {
        match x[i].cmp(&y[i]) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    x.len().cmp(&y.len())
}

/// Order two `Plus` terms: monomials before non-monomials, monomials by the
/// reverse-lexicographic exponent rule, everything else by `canon_cmp`.
fn plus_cmp(t: &SymbolTable, vars: &[Expr], a: &Expr, b: &Expr) -> Ordering {
    match (monomial_exponents(t, vars, a), monomial_exponents(t, vars, b)) {
        (Some(x), Some(y)) => cmp_exponents(&x, &y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => {
            // Mathematica puts a term carrying a positive power of a variable
            // before one that does not: `-x Cos[x] + Sin[x]`, `x Sin[x] + Cos[x]`.
            // Negative powers and functions of the variable do not count, which
            // keeps partial fractions and `D[Sin[x]/x]` in their own order.
            let pa = has_positive_var_power(t, vars, a);
            let pb = has_positive_var_power(t, vars, b);
            match (pa, pb) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => canon_cmp(t, a, b),
            }
        }
    }
}

/// True when `e` has a multiplicative factor that is a variable or a positive
/// integer power of one. `Sin[x]` and `1/x` do not count; `x`, `x^2` do.
fn has_positive_var_power(t: &SymbolTable, vars: &[Expr], e: &Expr) -> bool {
    if let Some((h, args)) = e.parts() {
        if h.is_symbol(t.sys.times) {
            return args.iter().any(|f| has_positive_var_power(t, vars, f));
        }
        if h.is_symbol(t.sys.power) && args.len() == 2 {
            if let Some(id) = args[0].sym_id() {
                if vars.iter().any(|v| v.is_symbol(id)) {
                    return crate::builtins::core::expr_to_int(&args[1]).map_or(false, |n| n > 0);
                }
            }
        }
        // A function application (Sin[x], Log[1 + x], …) is not a variable power.
        return false;
    }
    e.sym_id().map_or(false, |id| vars.iter().any(|v| v.is_symbol(id)))
}

/// A `Times` factor's ordering key: the base name and exponent of a variable or
/// power. `Exp[u]` is `E^u` in Mathematica's canonical order, so it keys on
/// base `E` with `u` as its exponent.
struct FactorKey {
    name: String,
    exp: i64,
    exp_expr: Expr,
}

fn factor_key(t: &SymbolTable, e: &Expr) -> Option<FactorKey> {
    if let Expr::Sym(id) = e {
        return Some(FactorKey {
            name: t.name_of(*id).to_string(),
            exp: 1,
            exp_expr: Expr::int_i64(1),
        });
    }
    if let Expr::Node(nd) = e {
        if let Some(exp_id) = t.get("Exp") {
            if nd.head.is_symbol(exp_id) && nd.args.len() == 1 {
                return Some(FactorKey {
                    name: "E".to_string(),
                    exp: 0,
                    exp_expr: nd.args[0].clone(),
                });
            }
        }
        if nd.head.is_symbol(t.sys.power) && nd.args.len() == 2 {
            let base = nd.args[0].sym_id()?;
            let name = t.name_of(base).to_string();
            return Some(match crate::builtins::core::expr_to_int(&nd.args[1]) {
                Some(n) => FactorKey { name, exp: n, exp_expr: nd.args[1].clone() },
                None => FactorKey { name, exp: 0, exp_expr: nd.args[1].clone() },
            });
        }
    }
    None
}

/// True for a factor that is a number: a numeric atom, a constant symbol, or a
/// Plus/Times/Power (or numeric function) built only from numbers. `Sqrt[2]` is
/// a coefficient, so it sorts before variables like `2` does.
fn is_numeric_value(t: &SymbolTable, e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::BigReal(_) | Expr::Cnum(_) => true,
        Expr::Sym(id) => has(t.attrs(*id), crate::expr::A_CONSTANT),
        Expr::Node(nd) => {
            let structural = nd.head.is_symbol(t.sys.plus)
                || nd.head.is_symbol(t.sys.times)
                || nd.head.is_symbol(t.sys.power)
                || nd
                    .head
                    .sym_id()
                    .map_or(false, |h| has(t.attrs(h), crate::expr::A_NUMERICFUNCTION));
            structural && !nd.args.is_empty() && nd.args.iter().all(|a| is_numeric_value(t, a))
        }
        Expr::Str(_) => false,
    }
}

/// Rank a `Times` factor: numbers, then other numeric-valued factors (`Sqrt[2]`,
/// `Sin[1]`), then factors with a symbolic base (`x`, `x^2`, `E^x`, `Pi`), then
/// everything else.
fn factor_rank(t: &SymbolTable, e: &Expr) -> u8 {
    match e {
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::Cnum(_) => 0,
        _ if factor_key(t, e).is_some() => 2,
        _ if is_numeric_value(t, e) => 1,
        _ => 3,
    }
}

fn cmp_factor_keys(a: &FactorKey, b: &FactorKey, t: &SymbolTable) -> Ordering {
    a.name
        .cmp(&b.name)
        .then(a.exp.cmp(&b.exp))
        .then_with(|| canon_cmp(t, &a.exp_expr, &b.exp_expr))
}

/// Numeric value of a real atom, for comparing exact and machine numbers
/// together the way `Sort` does.
fn real_f64(e: &Expr) -> Option<f64> {
    match e {
        Expr::Int(_) | Expr::Rat(_) => e.as_rat_exact().map(|r| r.to_f64()),
        Expr::Real(x) => Some(*x),
        _ => None,
    }
}

fn number_cmp(a: &Expr, b: &Expr) -> Ordering {
    match (a.as_rat_exact(), b.as_rat_exact()) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => match (real_f64(a), real_f64(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => Ordering::Equal,
        },
    }
}

fn sort_rank(t: &SymbolTable, e: &Expr) -> u8 {
    match e {
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::Cnum(_) => 0,
        _ if factor_key(t, e).is_some() => 2,
        _ if is_numeric_value(t, e) => 1,
        Expr::Node(nd) if nd.head.is_symbol(t.sys.plus) => 3,
        _ => 4,
    }
}

/// Compare two nodes part by part (used for equal-length sums).
fn seq_cmp(t: &SymbolTable, a: &Expr, b: &Expr) -> Ordering {
    let (xa, xb) = match (a.parts(), b.parts()) {
        (Some((_, xa)), Some((_, xb))) => (xa, xb),
        _ => return Ordering::Equal,
    };
    for (x, y) in xa.iter().zip(xb.iter()) {
        let c = sort_cmp(t, x, y);
        if c != Ordering::Equal {
            return c;
        }
    }
    Ordering::Equal
}

/// Mathematica's string collation: a case-insensitive dictionary order first,
/// with the lowercase spelling of a letter before its uppercase form. This is
/// what makes `Sort[{"cat", "fish", "catfish", "Cat"}]` come out as
/// `{"cat", "Cat", "catfish", "fish"}` rather than in byte order.
fn str_cmp(a: &str, b: &str) -> Ordering {
    // Primary pass: case-insensitive dictionary order over the whole string.
    let lower = |s: &str| -> Vec<char> { s.chars().flat_map(|c| c.to_lowercase()).collect() };
    match lower(a).cmp(&lower(b)) {
        Ordering::Equal => {}
        o => return o,
    }
    // Only when they tie ignoring case does the case decide: lowercase first.
    for (x, y) in a.chars().zip(b.chars()) {
        if x.is_uppercase() != y.is_uppercase() {
            return if x.is_uppercase() {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
    }
    a.cmp(b)
}

/// Mathematica's canonical order for `Sort` (an approximation of its `Order`):
/// numbers by value (exact and machine together), numeric-valued expressions,
/// monomials by base name and degree, sums by length then parts, then anything
/// else. This reproduces the documentation's examples — `{y, x^2, x + y, y^3}`
/// sorts to `{x^2, y, y^3, x + y}` and `{Pi, E, 2, 3, 1, Sqrt[2]}` to
/// `{1, 2, 3, Sqrt[2], E, Pi}` — while `canon_cmp` (the internal Orderless
/// order) keeps its specialised `plus_cmp`/`times_cmp` comparators.
pub fn sort_cmp(t: &SymbolTable, a: &Expr, b: &Expr) -> Ordering {
    let (ra, rb) = (sort_rank(t, a), sort_rank(t, b));
    if ra != rb {
        return ra.cmp(&rb);
    }
    match ra {
        0 => {
            if matches!(a, Expr::Cnum(_)) || matches!(b, Expr::Cnum(_)) {
                canon_cmp(t, a, b)
            } else {
                number_cmp(a, b)
            }
        }
        2 => match (factor_key(t, a), factor_key(t, b)) {
            (Some(ka), Some(kb)) => cmp_factor_keys(&ka, &kb, t),
            _ => canon_cmp(t, a, b),
        },
        3 => a.list_len().cmp(&b.list_len()).then_with(|| seq_cmp(t, a, b)),
        _ => match (a, b) {
            (Expr::Str(x), Expr::Str(y)) => str_cmp(x, y),
            _ => canon_cmp(t, a, b),
        },
    }
}

/// Order `Times` factors the way Mathematica prints them: the numeric
/// coefficient first (`2 x`, `Pi x`, `Sqrt[2] x`), then factors with a symbolic
/// base by base name and exponent (`x^4 y`, `E^x x^2` — not `y x^4` or
/// `x^2 E^x`), then everything else canonically.
fn times_cmp(t: &SymbolTable, a: &Expr, b: &Expr) -> Ordering {
    let (ra, rb) = (factor_rank(t, a), factor_rank(t, b));
    if ra != rb {
        return ra.cmp(&rb);
    }
    if ra == 2 {
        if let (Some(ka), Some(kb)) = (factor_key(t, a), factor_key(t, b)) {
            return cmp_factor_keys(&ka, &kb, t);
        }
    }
    canon_cmp(t, a, b)
}

impl MatchCtx for Interp {
    fn eval_test(&mut self, test: &Expr, val: &Expr) -> Result<bool, ()> {
        let call = Expr::node(test.clone(), vec![val.clone()]);
        match self.eval(&call) {
            Ok(v) => Ok(v.is_symbol(self.t.sys.r#true)),
            Err(_) => Err(()),
        }
    }
    fn eval_cond(&mut self, cond: &Expr, b: &Bindings) -> Result<bool, ()> {
        let c = subst_bindings(&self.t.sys, cond, b);
        match self.eval(&c) {
            Ok(v) => Ok(v.is_symbol(self.t.sys.r#true)),
            Err(_) => Err(()),
        }
    }
    fn eval_plain(&mut self, e: &Expr) -> Result<Expr, ()> {
        self.eval(e).map_err(|_| ())
    }
    fn head_sym(&mut self, e: &Expr) -> usize {
        e.head_of(&self.t).sym_id().unwrap_or(usize::MAX)
    }
    fn attrs_of(&mut self, id: usize) -> u32 {
        self.t.attrs(id)
    }
    fn canon(&mut self, a: &Expr, b: &Expr) -> Ordering {
        canon_cmp(&self.t, a, b)
    }
    fn default_for(&mut self, head: Option<usize>, _pos: usize) -> Option<Expr> {
        let sys = self.t.sys;
        match head {
            Some(id) if id == sys.plus => Some(Expr::int_i64(0)),
            Some(id) if id == sys.times => Some(Expr::int_i64(1)),
            Some(id) if id == sys.power => Some(Expr::int_i64(1)),
            Some(id) => self.t.data[id].defaults[0].clone(),
            None => None,
        }
    }
    fn atom_as_node_parts(&mut self, e: &Expr) -> Option<(Expr, Vec<Expr>)> {
        e.atom_as_node(&self.t)
    }
    fn sys_syms(&mut self) -> crate::expr::Sys {
        self.t.sys
    }
}
#[cfg(test)]
mod tests {
    use crate::expr::Expr;

    fn eval_str(src: &str) -> (String, Vec<String>) {
        let mut i = super::Interp::new();
        let e = crate::parser::parse_input(&mut i.t, src).unwrap();
        let v = i.eval(&e).unwrap();
        let text = crate::fmt::output_form(&i.t, &v);
        let msgs = i.messages.iter().map(|(_, t)| t.clone()).collect();
        (text, msgs)
    }

    #[test]
    fn case_mismatch_hint() {
        // Typing sin[x] instead of Sin[x] should explain itself.
        let (_, msgs) = eval_str("sin[x]");
        assert!(
            msgs.iter().any(|m| m.contains("did you mean Sin?")),
            "expected a hint, got {:?}",
            msgs
        );
        // A correctly spelled call emits nothing.
        let (_, msgs) = eval_str("Sin[x]");
        assert!(msgs.is_empty(), "unexpected messages: {:?}", msgs);
        // Unknown names without a case-mate stay quiet.
        let (_, msgs) = eval_str("zzz[1]");
        assert!(msgs.is_empty(), "unexpected messages: {:?}", msgs);
    }

    #[test]
    fn special_function_aliases() {
        // Names used by other systems resolve to the Wolfram spellings.
        let (text, _) = eval_str("Si[x]");
        assert_eq!(text, "SinIntegral[x]");
        let (text, _) = eval_str("ei[x]");
        assert_eq!(text, "ei[x]", "unknown names stay unevaluated");
        let (text, msgs) = eval_str("Si[1.0]");
        assert!(text.starts_with("0.946083"), "Si[1.] is numeric: {}", text);
        assert!(msgs.is_empty(), "aliases are silent: {:?}", msgs);
        let (text, _) = eval_str("Sinc[0]");
        assert_eq!(text, "1");
    }

    #[test]
    fn arity_guard_reports_instead_of_panicking() {
        // First[] used to index past the end of args and abort the whole
        // process. The central guard reports it and leaves the call alone.
        let (text, msgs) = eval_str("First[]");
        assert_eq!(text, "First[]");
        assert!(
            msgs.iter().any(|m| m.contains("wrong number of arguments")),
            "expected an argr message, got {:?}",
            msgs
        );
        let (text, _) = eval_str("Take[x]");
        assert_eq!(text, "Take[x]");
        // A well-formed call still works.
        let (text, _) = eval_str("First[{7, 8}]");
        assert_eq!(text, "7");
    }

    fn messages_for(i: &mut super::Interp, src: &str) -> usize {
        i.messages.clear();
        let e = crate::parser::parse_input(&mut i.t, src).unwrap();
        let _ = i.eval(&e);
        i.messages.len()
    }

    #[test]
    fn off_on_and_quiet_suppress_messages() {
        let mut i = super::Interp::new();
        assert!(messages_for(&mut i, "1/0") > 0, "1/0 should message");
        assert_eq!(messages_for(&mut i, "Off[Power]; 1/0"), 0);
        assert!(messages_for(&mut i, "On[Power]; 1/0") > 0);
        assert_eq!(messages_for(&mut i, "Quiet[1/0]"), 0);
        assert_eq!(messages_for(&mut i, "Off[]; 1/0"), 0);
        assert!(messages_for(&mut i, "On[]; 1/0") > 0);
    }

    #[test]
    fn standard_form_shows_typable_names() {
        let mut i = super::Interp::new();
        let e = crate::parser::parse_input(&mut i.t, "Sin[x] + Exp[x]").unwrap();
        let tex = crate::tex::tex_form(&i.t, &e);
        assert!(tex.contains("Sin"), "StandardForm keeps Sin: {}", tex);
        assert!(tex.contains("Exp"), "StandardForm keeps Exp: {}", tex);
        let trad = crate::tex::tex_form_as(&i.t, &e, crate::tex::Form::Traditional);
        assert!(trad.contains("\\sin"), "TraditionalForm uses \\sin: {}", trad);
    }

    #[test]
    fn n_above_machine_precision_uses_bigreal() {
        // Exact shapes are evaluated with the decimal bigfloat, silently.
        let (text, msgs) = eval_str("N[Pi, 50]");
        assert_eq!(text, "3.1415926535897932384626433832795028841971693993751");
        assert!(msgs.is_empty(), "{:?}", msgs);
        let (text, _) = eval_str("N[1/3, 20]");
        assert_eq!(text, "0.33333333333333333333");
        let (text, _) = eval_str("N[Pi, 50] + 1");
        assert!(text.starts_with("4.14159265358979323846264338327950"), "{text}");
        let (text, _) = eval_str("N[Sqrt[2], 30]");
        assert!(text.starts_with("1.41421356237309504880168872421"), "{text}");
        // Elementary functions are evaluated at precision too (N holds its
        // argument, so `Sin[1]` is still exact when the bigfloat path runs).
        let (text, msgs) = eval_str("N[Sin[1], 40]");
        assert_eq!(text, "0.8414709848078965066525023216302989996226");
        assert!(msgs.is_empty(), "{:?}", msgs);
        // An f64 input cannot regain precision, and a symbolic argument cannot
        // be evaluated numerically; both are reported rather than guessed.
        let (_, msgs) = eval_str("N[1.5, 50]");
        assert!(msgs.iter().any(|m| m.contains("precision")), "{:?}", msgs);
        let (_, msgs) = eval_str("N[Sin[x], 50]");
        assert!(msgs.iter().any(|m| m.contains("precision")), "{:?}", msgs);
        // Machine precision and below stay quiet; a bad precision is refused.
        assert!(eval_str("N[Pi]").1.is_empty());
        assert!(eval_str("N[Pi, 10]").1.is_empty());
        let (text, msgs) = eval_str("N[Pi, 0]");
        assert_eq!(text, "N[Pi, 0]");
        assert!(
            msgs.iter().any(|m| m.contains("positive integer")),
            "{:?}",
            msgs
        );
    }
}
