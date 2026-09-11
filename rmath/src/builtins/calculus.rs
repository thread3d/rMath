//! Calculus built-ins: D (symbolic differentiation), Integrate (table +
//! linearity), Series (Taylor via SeriesData), Limit (via series), Normal.

use crate::bigint::BigInt;
use crate::builtins::core::unevaluated_call;
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::{same_q, Expr};
use crate::rational::Rat;

fn eval_call(i: &mut Interp, head: Expr, args: Vec<Expr>) -> EvalRes {
    i.eval(&Expr::node(head, args))
}

fn is_free_of(i: &mut Interp, e: &Expr, var: &Expr) -> Result<bool, Flow> {
    if same_q(e, var) {
        return Ok(false);
    }
    if let Some((_, args)) = e.parts() {
        for a in args {
            if !is_free_of(i, a, var)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Substitute var -> val syntactically, then evaluate.
pub fn subst_var(i: &mut Interp, e: &Expr, var: &Expr, val: &Expr) -> EvalRes {
    let sub = crate::pattern::subst_syms_all(
        &i.t,
        e,
        &[var.sym_id().unwrap_or(usize::MAX)],
        &[val.clone()],
    );
    i.eval(&sub)
}

// ---------- small constructors ----------

fn app(i: &mut Interp, name: &str, y: &Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.intern(name)), vec![y.clone()])
}
fn pw(i: &mut Interp, b: &Expr, e: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.power), vec![b.clone(), e])
}
fn neg1(i: &mut Interp, e: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.times), vec![Expr::int_i64(-1), e])
}
fn half() -> Expr {
    Expr::rat(Rat::new(BigInt::from_i64(1), BigInt::from_i64(2)))
}

// ============================ differentiation ============================

/// D[expr, x] core (expr already evaluated).
pub fn d_expr(i: &mut Interp, e: &Expr, var: &Expr) -> EvalRes {
    let sys = i.t.sys;
    match e {
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::Str(_) | Expr::Cnum(_) => {
            Ok(Expr::int_i64(0))
        }
        Expr::Sym(_) => {
            if same_q(e, var) {
                Ok(Expr::int_i64(1))
            } else {
                Ok(Expr::int_i64(0))
            }
        }
        Expr::Node(nd) => {
            let head = &nd.head;
            let Some(hid) = head.sym_id() else {
                return Ok(e.clone());
            };
            if hid == sys.plus {
                let mut parts = Vec::new();
                for a in &nd.args {
                    parts.push(d_expr(i, a, var)?);
                }
                return eval_call(i, Expr::Sym(sys.plus), parts);
            }
            if hid == sys.times && !nd.args.is_empty() {
                let mut parts = Vec::new();
                for k in 0..nd.args.len() {
                    let mut one = Vec::new();
                    for (j, a) in nd.args.iter().enumerate() {
                        if j == k {
                            one.push(d_expr(i, a, var)?);
                        } else {
                            one.push(a.clone());
                        }
                    }
                    parts.push(eval_call(i, Expr::Sym(sys.times), one)?);
                }
                return eval_call(i, Expr::Sym(sys.plus), parts);
            }
            if hid == sys.power && nd.args.len() == 2 {
                let b = &nd.args[0];
                let ex = &nd.args[1];
                let ex_free = is_free_of(i, ex, var)?;
                let b_free = is_free_of(i, b, var)?;
                if ex_free && b_free {
                    return Ok(Expr::int_i64(0));
                }
                if ex_free {
                    let em1 = eval_call(
                        i,
                        Expr::Sym(sys.plus),
                        vec![ex.clone(), Expr::int_i64(-1)],
                    )?;
                    let db = d_expr(i, b, var)?;
                    let p1 = eval_call(i, Expr::Sym(sys.power), vec![b.clone(), em1])?;
                    return eval_call(i, Expr::Sym(sys.times), vec![ex.clone(), p1, db]);
                }
                if b_free {
                    let log_id = i.t.intern("Log");
                    let logb = eval_call(i, Expr::Sym(log_id), vec![b.clone()])?;
                    let dexp = d_expr(i, ex, var)?;
                    return eval_call(
                        i,
                        Expr::Sym(sys.times),
                        vec![e.clone(), logb, dexp],
                    );
                }
                let db2 = d_expr(i, b, var)?;
                let inv_b = pw(i, b, Expr::int_i64(-1));
                let db_over_b = eval_call(
                    i,
                    Expr::Sym(sys.times),
                    vec![inv_b, db2],
                )?;
                let dexp2 = d_expr(i, ex, var)?;
                let lg2 = app(i, "Log", b);
                let dx_log = eval_call(
                    i,
                    Expr::Sym(sys.times),
                    vec![lg2, dexp2],
                )?;
                let inner = eval_call(i, Expr::Sym(sys.plus), vec![db_over_b, dx_log])?;
                return eval_call(i, Expr::Sym(sys.times), vec![e.clone(), inner]);
            }
            // Chain rule via the derivative table (single-arg functions).
            if nd.args.len() == 1 {
                if let Some(df) = derivative_table(i, hid, &nd.args[0]) {
                    let inner = d_expr(i, &nd.args[0], var)?;
                    return eval_call(i, Expr::Sym(sys.times), vec![df, inner]);
                }
            }
            Ok(e.clone())
        }
    }
}

/// d/dy f(y) for known functions, at argument y (already evaluated).
fn derivative_table(i: &mut Interp, head: usize, y: &Expr) -> Option<Expr> {
    let name = i.t.name_of(head).to_string();
    match name.as_str() {
        "Sin" => Some(app(i, "Cos", y)),
        "Cos" => Some(neg_app(i, "Sin", y)),
        "Tan" => Some(pow_named(i, "Sec", y, 2)),
        "Cot" => Some(neg_pow_named(i, "Csc", y, 2)),
        "Sec" => {
            let a = app(i, "Sec", y);
            let b = app(i, "Tan", y);
            Some(mul2(i, a, b))
        }
        "Csc" => {
            let a = app(i, "Csc", y);
            let b = app(i, "Cot", y);
            let m = mul2(i, a, b);
            Some(neg1(i, m))
        }
        "Exp" => Some(app(i, "Exp", y)),
        "Log" => Some(pw(i, y, Expr::int_i64(-1))),
        "Sqrt" => Some(pw(i, y, half())),
        "ArcSin" => {
            let d = one_minus_y2(i, y);
            Some(pw(i, &d, half()))
        }
        "ArcCos" => {
            let d = one_minus_y2(i, y);
            let p = pw(i, &d, half());
            Some(neg1(i, p))
        }
        "ArcTan" => {
            let d = one_plus_y2(i, y);
            Some(pw(i, &d, Expr::int_i64(-1)))
        }
        "Sinh" => Some(app(i, "Cosh", y)),
        "Cosh" => Some(app(i, "Sinh", y)),
        "Tanh" => {
            let p = pow_named(i, "Tanh", y, 2);
            Some(sub_one(i, p))
        }
        _ => None,
    }
}

fn neg_app(i: &mut Interp, name: &str, y: &Expr) -> Expr {
    let a = app(i, name, y);
    neg1(i, a)
}
fn pow_named(i: &mut Interp, name: &str, y: &Expr, e: i64) -> Expr {
    let a = app(i, name, y);
    pw(i, &a, Expr::int_i64(e))
}
fn neg_pow_named(i: &mut Interp, name: &str, y: &Expr, e: i64) -> Expr {
    let a = pow_named(i, name, y, e);
    neg1(i, a)
}
fn mul2(i: &mut Interp, a: Expr, b: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.times), vec![a, b])
}
fn sub_one(i: &mut Interp, e: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.plus), vec![Expr::int_i64(1), neg1(i, e)])
}
fn y2_of(i: &mut Interp, y: &Expr) -> Expr {
    pw(i, y, Expr::int_i64(2))
}
fn one_plus_y2(i: &mut Interp, y: &Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.plus), vec![Expr::int_i64(1), y2_of(i, y)])
}
fn one_minus_y2(i: &mut Interp, y: &Expr) -> Expr {
    let y2 = y2_of(i, y);
    Expr::node(Expr::Sym(i.t.sys.plus), vec![Expr::int_i64(1), neg1(i, y2)])
}

fn times3(i: &mut Interp, a: Expr, b: Expr, c: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.times), vec![a, b, c])
}

// ============================ integration ============================

/// Integrate[expr, x] core (expr already evaluated).
pub fn integrate_expr(i: &mut Interp, e: &Expr, var: &Expr) -> EvalRes {
    let sys = i.t.sys;
    // Linearity over Plus.
    if let Some((h, args)) = e.parts() {
        if h.is_symbol(sys.plus) {
            let mut parts = Vec::new();
            for a in args {
                parts.push(integrate_expr(i, a, var)?);
            }
            return eval_call(i, Expr::Sym(sys.plus), parts);
        }
        // Constant factor.
        if h.is_symbol(sys.times) {
            let mut consts: Vec<Expr> = Vec::new();
            let mut rest: Option<Expr> = None;
            for a in args {
                if is_free_of(i, a, var)? {
                    consts.push(a.clone());
                } else {
                    rest = Some(match rest {
                        None => a.clone(),
                        Some(r) => eval_call(i, Expr::Sym(sys.times), vec![r, a.clone()])?,
                    });
                }
            }
            let Some(rest) = rest else {
                return Ok(unevaluated_call(i, "Integrate", &[e.clone(), var.clone()]));
            };
            let c = eval_call(i, Expr::Sym(sys.times), consts)?;
            let inner = integrate_expr(i, &rest, var)?;
            return eval_call(i, Expr::Sym(sys.times), vec![c, inner]);
        }
        // x^n
        if h.is_symbol(sys.power) && args.len() == 2 {
            let n = crate::builtins::core::expr_to_int(&args[1]).unwrap_or(-1);
            if same_q(&args[0], var) {
                return power_rule(i, var, n);
            }
            // 1/(a x + b): Power[linear, -1]
            if n == -1 {
                if let Some((la, lb)) = linear_form(i, &args[0], var)? {
                    if la != 0 {
                        let log_id = i.t.intern("Log");
                        let lg = eval_call(i, Expr::Sym(log_id), vec![args[0].clone()])?;
                        let inv = Expr::node(
                            Expr::Sym(sys.power),
                            vec![Expr::int_i64(la), Expr::int_i64(-1)],
                        );
                        return eval_call(i, Expr::Sym(sys.times), vec![inv, lg]);
                    }
                }
            }
        }
        if same_q(e, var) {
            return power_rule(i, var, 1);
        }
        // Log[x] -> x Log[x] - x
        if h.is_symbol(i.t.intern("Log")) && args.len() == 1 && same_q(&args[0], var) {
            let negv = neg1(i, var.clone());
            let xl = eval_call(i, Expr::Sym(sys.times), vec![var.clone(), e.clone()])?;
            return eval_call(i, Expr::Sym(sys.plus), vec![xl, negv]);
        }
        // Table integrals with linear argument.
        if args.len() == 1 {
            if let Some((a, _)) = linear_form(i, &args[0], var)? {
                if a != 0 {
                    let hname_id = match h.sym_id() {
                    Some(id) => id,
                    None => return Ok(unevaluated_call(i, "Integrate", &[e.clone(), var.clone()])),
                };
                    let name = i.t.name_of(hname_id).to_string();
                    let inner = args[0].clone();
                    let scale = Expr::node(
                        Expr::Sym(sys.power),
                        vec![Expr::int_i64(a), Expr::int_i64(-1)],
                    );
                    let antider = match name.as_str() {
                        "Sin" => Some(neg_app(i, "Cos", &inner)),
                        "Cos" => Some(app(i, "Sin", &inner)),
                        "Tan" => {
                            let c = app(i, "Cos", &inner);
                            let l = app(i, "Log", &c);
                            Some(neg1(i, l))
                        }
                        "Cot" => {
                            let s2 = app(i, "Sin", &inner);
                            Some(app(i, "Log", &s2))
                        }
                        "Exp" => Some(app(i, "Exp", &inner)),
                        "Sqrt" => Some(pw(i, &inner, half())),
                        _ => None,
                    };
                    if let Some(anti) = antider {
                        return eval_call(i, Expr::Sym(sys.times), vec![scale, anti]);
                    }
                }
            }
        }
    }
    Ok(unevaluated_call(i, "Integrate", &[e.clone(), var.clone()]))
}

fn power_rule(i: &mut Interp, var: &Expr, n: i64) -> EvalRes {
    let sys = i.t.sys;
    if n == -1 {
        let log_id = i.t.intern("Log");
        return eval_call(i, Expr::Sym(log_id), vec![var.clone()]);
    }
    let np1 = Expr::int_i64(n + 1);
    let p = eval_call(i, Expr::Sym(sys.power), vec![var.clone(), np1.clone()])?;
    let inv = Expr::node(
        Expr::Sym(sys.power),
        vec![np1.clone(), Expr::int_i64(-1)],
    );
    eval_call(i, Expr::Sym(sys.times), vec![inv, p])
}

/// Returns (a, b) with arg = a*var + b when arg is linear (integer a,b).
fn linear_form(i: &mut Interp, arg: &Expr, var: &Expr) -> Result<Option<(i64, i64)>, Flow> {
    if same_q(arg, var) {
        return Ok(Some((1, 0)));
    }
    match arg {
        Expr::Int(n) => Ok(Some((0, n.to_i64().unwrap_or(0)))),
        _ => {
            if let Some((h, hargs)) = arg.parts() {
                if h.is_symbol(i.t.sys.times) && hargs.len() == 2 {
                    let ca = crate::builtins::core::expr_to_int(&hargs[0]).unwrap_or(1);
                    if same_q(&hargs[1], var) {
                        return Ok(Some((ca, 0)));
                    }
                }
                if h.is_symbol(i.t.sys.plus) && hargs.len() == 2 {
                    if let Some((h2, a2)) = hargs[0].parts() {
                        if h2.is_symbol(i.t.sys.times) && a2.len() == 2 && same_q(&a2[1], var) {
                            let ca = crate::builtins::core::expr_to_int(&a2[0]).unwrap_or(1);
                            if let Some(cb) = crate::builtins::core::expr_to_int(&hargs[1]) {
                                return Ok(Some((ca, cb)));
                            }
                        }
                    }
                    if let Some(cb) = crate::builtins::core::expr_to_int(&hargs[1]) {
                        if same_q(&hargs[0], var) {
                            return Ok(Some((1, cb)));
                        }
                    }
                }
            }
            Ok(None)
        }
    }
}

pub fn b_integrate(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Integrate", "argr");
        return Ok(unevaluated_call(i, "Integrate", args));
    }
    let e = i.eval(&args[0])?;
    if let Some((h, hargs)) = args[1].parts() {
        if h.is_symbol(i.t.sys.list) && hargs.len() == 3 {
            let v = i.eval(&hargs[0])?;
            let a = i.eval(&hargs[1])?;
            let b = i.eval(&hargs[2])?;
            let f = integrate_expr(i, &e, &v)?;
            let at_b = subst_var(i, &f, &v, &b)?;
            let at_a = subst_var(i, &f, &v, &a)?;
            let neg_a = neg1(i, at_a);
            return eval_call(i, Expr::Sym(i.t.sys.plus), vec![at_b, neg_a]);
        }
    }
    let v = i.eval(&args[1])?;
    integrate_expr(i, &e, &v)
}

pub fn b_d(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("D", "argr");
        return Ok(unevaluated_call(i, "D", args));
    }
    let e = i.eval(&args[0])?;
    let mut result = e;
    for spec in &args[1..] {
        match spec.parts() {
            Some((h, hargs)) if h.is_symbol(i.t.sys.list) && hargs.len() == 2 => {
                let v = i.eval(&hargs[0])?;
                let n = match i.eval(&hargs[1])?.to_i64_pub() {
                    Some(n) => n.max(0) as usize,
                    None => return Ok(unevaluated_call(i, "D", args)),
                };
                for _ in 0..n {
                    result = d_expr(i, &result, &v)?;
                }
            }
            _ => {
                let v = i.eval(spec)?;
                result = d_expr(i, &result, &v)?;
            }
        }
    }
    Ok(result)
}

// ============================ Series / Limit ============================

fn fact_int(k: usize) -> BigInt {
    let mut acc = BigInt::from_i64(1);
    for j in 2..=k as i64 {
        acc = acc.mul(&BigInt::from_i64(j));
    }
    acc
}

/// Taylor-expand f at x0 to order n as a SeriesData node.
fn series_at(i: &mut Interp, f: &Expr, v: &Expr, x0: &Expr, n: usize) -> EvalRes {
    let mut coeffs: Vec<Expr> = Vec::new();
    let mut deriv = f.clone();
    for k in 0..=n {
        if k > 0 {
            deriv = d_expr(i, &deriv, v)?;
        }
        let at = subst_var(i, &deriv, v, x0)?;
        let fact_inv = pw(i, &Expr::Int(fact_int(k)), Expr::int_i64(-1));
        let c = eval_call(
            i,
            Expr::Sym(i.t.sys.times),
            vec![at, fact_inv],
        )?;
        coeffs.push(c);
    }
    let nmin = coeffs
        .iter()
        .position(|c| !same_q(c, &Expr::int_i64(0)))
        .unwrap_or(0);
    Ok(Expr::node(
        Expr::Sym(i.t.intern("SeriesData")),
        vec![
            v.clone(),
            x0.clone(),
            Expr::node(Expr::Sym(i.t.sys.list), coeffs),
            Expr::int_i64(nmin as i64),
            Expr::int_i64(n as i64),
            Expr::int_i64(1),
        ],
    ))
}

pub fn b_series(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Series", "argr");
        return Ok(unevaluated_call(i, "Series", args));
    }
    let e = i.eval(&args[0])?;
    let Some((h, hargs)) = args[1].parts() else {
        return Ok(unevaluated_call(i, "Series", args));
    };
    if !(h.is_symbol(i.t.sys.list) && hargs.len() == 3) {
        return Ok(unevaluated_call(i, "Series", args));
    }
    let v = i.eval(&hargs[0])?;
    let x0 = i.eval(&hargs[1])?;
    let n = match i.eval(&hargs[2])?.to_i64_pub() {
        Some(n) => n.max(0) as usize,
        None => return Ok(unevaluated_call(i, "Series", args)),
    };
    series_at(i, &e, &v, &x0, n)
}

pub fn b_normal(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Normal", args));
    }
    let e = i.eval(&args[0])?;
    if let Some((h, hargs)) = e.parts() {
        if h.is_symbol(i.t.intern("SeriesData")) && hargs.len() == 6 {
            let v = &hargs[0];
            let x0 = &hargs[1];
            let nmin = hargs[3].to_i64_pub().unwrap_or(0);
            let mut out: Vec<Expr> = Vec::new();
            for (k, c) in hargs[2].node_args().iter().enumerate() {
                let deg = nmin + k as i64;
                if deg < 0 || same_q(c, &Expr::int_i64(0)) {
                    continue;
                }
                let base = if x0.to_i64_pub() == Some(0) {
                    v.clone()
                } else {
                    let neg0 = neg1(i, x0.clone());
                    eval_call(i, Expr::Sym(i.t.sys.plus), vec![v.clone(), neg0])?
                };
                let p = pw(i, &base, Expr::int_i64(deg));
                out.push(eval_call(i, Expr::Sym(i.t.sys.times), vec![c.clone(), p])?);
            }
            if out.is_empty() {
                return Ok(Expr::int_i64(0));
            }
            return eval_call(i, Expr::Sym(i.t.sys.plus), out);
        }
    }
    Ok(e.clone())
}

pub fn b_limit(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Limit", "argr");
        return Ok(unevaluated_call(i, "Limit", args));
    }
    let Some((h, hargs)) = args[1].parts() else {
        return Ok(unevaluated_call(i, "Limit", args));
    };
    if !(h.is_symbol(i.t.sys.rule) && hargs.len() == 2) {
        return Ok(unevaluated_call(i, "Limit", args));
    }
    let v = i.eval(&hargs[0])?;
    let a = i.eval(&hargs[1])?;
    let f = i.eval(&args[0])?;
    let direct = subst_var(i, &f, &v, &a)?;
    let indet = direct.is_symbol(i.t.sys.indeterminate)
        || direct.is_symbol(i.t.sys.complexinfinity)
        || direct.is_symbol(i.t.sys.infinity);
    if !indet {
        return Ok(direct);
    }
    // Series to order 4; the first nonzero coefficient decides.
    let s = series_at(i, &f, &v, &a, 4)?;
    if let Some((h, hargs)) = s.parts() {
        if h.is_symbol(i.t.intern("SeriesData")) && hargs.len() == 6 {
            let coeffs = hargs[2].node_args();
            match coeffs.iter().position(|c| !same_q(c, &Expr::int_i64(0))) {
                Some(0) => return Ok(coeffs[0].clone()),
                Some(_) => return Ok(Expr::Sym(i.t.sys.indeterminate)),
                None => return Ok(Expr::int_i64(0)),
            }
        }
    }
    Ok(unevaluated_call(i, "Limit", args))
}