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
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::BigReal(_) | Expr::Str(_)
        | Expr::Cnum(_) => Ok(Expr::int_i64(0)),
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
                let res = eval_call(i, Expr::Sym(sys.plus), parts)?;
                i.step("sum rule", &res);
                return Ok(res);
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
                let res = eval_call(i, Expr::Sym(sys.plus), parts)?;
                i.step("product rule", &res);
                return Ok(res);
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
                    let res = eval_call(i, Expr::Sym(sys.times), vec![ex.clone(), p1, db])?;
                    i.step("power rule", &res);
                    return Ok(res);
                }
                if b_free {
                    let log_id = i.t.intern("Log");
                    let logb = eval_call(i, Expr::Sym(log_id), vec![b.clone()])?;
                    let dexp = d_expr(i, ex, var)?;
                    let res = eval_call(
                        i,
                        Expr::Sym(sys.times),
                        vec![e.clone(), logb, dexp],
                    )?;
                    i.step("exponential rule", &res);
                    return Ok(res);
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
                let res = eval_call(i, Expr::Sym(sys.times), vec![e.clone(), inner])?;
                i.step("logarithmic differentiation", &res);
                return Ok(res);
            }
            // Chain rule via the derivative table (single-arg functions).
            if nd.args.len() == 1 {
                if let Some(df) = derivative_table(i, hid, &nd.args[0]) {
                    let fname = i.t.name_of(hid).to_string();
                    let inner = d_expr(i, &nd.args[0], var)?;
                    let res = eval_call(i, Expr::Sym(sys.times), vec![df, inner])?;
                    let label = format!("chain rule: d/dx {}", fname);
                    i.step(&label, &res);
                    return Ok(res);
                }
            }
            // Unknown function of the variable: leave a Derivative expression
            // rather than returning the function unchanged, which silently
            // dropped the derivative (D[f[x], x] gave f[x]).
            unknown_derivative(i, head, &nd.args, var)
        }
    }
}

/// d/dx f(u1, ..., uk) = Σ_i Derivative[0,..,1,..,0][f][u...] · du_i/dx for an
/// unknown function head `f`. Returns 0 when no argument depends on `var`.
fn unknown_derivative(i: &mut Interp, head: &Expr, args: &[Expr], var: &Expr) -> EvalRes {
    let sys = i.t.sys;
    let mut any_depends = false;
    for a in args {
        if !is_free_of(i, a, var)? {
            any_depends = true;
        }
    }
    if !any_depends {
        return Ok(Expr::int_i64(0));
    }
    let mut terms: Vec<Expr> = Vec::new();
    for (pos, arg) in args.iter().enumerate() {
        if is_free_of(i, arg, var)? {
            continue;
        }
        let darg = d_expr(i, arg, var)?;
        if same_q(&darg, &Expr::int_i64(0)) {
            continue;
        }
        let multi: Vec<Expr> = if args.len() == 1 {
            vec![Expr::int_i64(1)]
        } else {
            (0..args.len())
                .map(|j| Expr::int_i64(if j == pos { 1 } else { 0 }))
                .collect()
        };
        let dspec = Expr::node(Expr::Sym(sys.derivativet), multi);
        let df = Expr::node(dspec, vec![head.clone()]);
        let applied = Expr::node(df, args.to_vec());
        terms.push(eval_call(i, Expr::Sym(sys.times), vec![applied, darg])?);
    }
    if terms.is_empty() {
        return Ok(Expr::int_i64(0));
    }
    let res = eval_call(i, Expr::Sym(sys.plus), terms)?;
    i.step("derivative of an unknown function", &res);
    Ok(res)
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
        // special functions
        "SinIntegral" | "Si" => {
            let s = app(i, "Sin", y);
            let inv = pw(i, y, Expr::int_i64(-1));
            Some(mul2(i, s, inv))
        }
        "CosIntegral" | "Ci" => {
            let c = app(i, "Cos", y);
            let inv = pw(i, y, Expr::int_i64(-1));
            Some(mul2(i, c, inv))
        }
        "ExpIntegralEi" | "Ei" => {
            let e1 = app(i, "Exp", y);
            let inv = pw(i, y, Expr::int_i64(-1));
            Some(mul2(i, e1, inv))
        }
        "LogIntegral" | "li" => {
            let l = app(i, "Log", y);
            Some(pw(i, &l, Expr::int_i64(-1)))
        }
        "SinhIntegral" | "Shi" => {
            let s = app(i, "Sinh", y);
            let inv = pw(i, y, Expr::int_i64(-1));
            Some(mul2(i, s, inv))
        }
        "CoshIntegral" | "Chi" => {
            let c = app(i, "Cosh", y);
            let inv = pw(i, y, Expr::int_i64(-1));
            Some(mul2(i, c, inv))
        }
        "Sinc" => {
            // d/dx (Sin[x]/x) = Cos[x]/x - Sin[x]/x^2
            let c = app(i, "Cos", y);
            let inv = pw(i, y, Expr::int_i64(-1));
            let term1 = mul2(i, c, inv);
            let s = app(i, "Sin", y);
            let inv2 = pw(i, y, Expr::int_i64(-2));
            let prod = mul2(i, s, inv2);
            let term2 = neg1(i, prod);
            let plus = i.t.sys.plus;
            Some(Expr::node(Expr::Sym(plus), vec![term1, term2]))
        }
        "Erf" | "Erfc" | "erf" | "erfc" => {
            let sq = pw(i, y, Expr::int_i64(2));
            let neg_sq = neg1(i, sq);
            let e = app(i, "Exp", &neg_sq);
            let pi = i.t.get("Pi").unwrap_or(i.t.sys.indeterminate);
            let root_pi = Expr::node(Expr::Sym(i.t.intern("Sqrt")), vec![Expr::Sym(pi)]);
            let inv = pw(i, &root_pi, Expr::int_i64(-1));
            let prod = mul2(i, e, inv);
            let v = mul2(i, Expr::int_i64(2), prod);
            Some(if name == "Erfc" || name == "erfc" { neg1(i, v) } else { v })
        }
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


// ============================ integration ============================

/// Integrate[expr, x] core (expr already evaluated).


/// Some(c) when `e` is `c*x` for a non-zero integer c (c = 1 for plain x).
fn coefficient_of_var(i: &Interp, e: &Expr, var: &Expr) -> Option<i64> {
    if same_q(e, var) {
        return Some(1);
    }
    if let Some((h, args)) = e.parts() {
        if h.is_symbol(i.t.sys.times) && args.len() == 2 {
            if let Expr::Int(n) = &args[0] {
                if same_q(&args[1], var) {
                    let c = n.to_i64()?;
                    if c != 0 {
                        return Some(c);
                    }
                }
            }
        }
    }
    None
}

/// Antiderivatives that need a special function:
///   Sin[u]/u -> SinIntegral[u],  Cos[u]/u -> CosIntegral[u],
///   Exp[u]/u -> ExpIntegralEi[u], 1/Log[u] -> LogIntegral[u],
///   Exp[-x^2] -> Sqrt[Pi]/2 Erf[x].
fn special_antiderivative(i: &mut Interp, e: &Expr, var: &Expr) -> Option<Expr> {
    let sys = i.t.sys;
    let (h, args) = e.parts()?;
    let head_name = h.sym_id().map(|id| i.t.name_of(id).to_string());
    let head_is = |n: &str| head_name.as_deref() == Some(n);

    // Exp[-x^2]
    if head_is("Exp") && args.len() == 1 {
        let (eh, eargs) = args[0].parts()?;
        if eh.is_symbol(sys.times) && eargs.len() == 2 {
            let minus_one = matches!(&eargs[0], Expr::Int(n) if n.to_i64() == Some(-1));
            let x_squared = Expr::node(
                Expr::Sym(sys.power),
                vec![var.clone(), Expr::int_i64(2)],
            );
            if minus_one && same_q(&eargs[1], &x_squared) {
                // Sqrt[Pi]/2 * Erf[x]
                let pi = i.t.get("Pi").unwrap_or(i.t.sys.indeterminate);
                let root_pi = Expr::node(Expr::Sym(i.t.intern("Sqrt")), vec![Expr::Sym(pi)]);
                let half = Expr::rat(crate::rational::Rat::new(
                    crate::bigint::BigInt::from_i64(1),
                    crate::bigint::BigInt::from_i64(2),
                ));
                let coef = mul2(i, half, root_pi);
                let erf = app(i, "Erf", var);
                return Some(mul2(i, coef, erf));
            }
        }
        return None;
    }

    // 1/Log[u] -> LogIntegral[u]
    if h.is_symbol(sys.power) && args.len() == 2 {
        let minus_one = matches!(&args[1], Expr::Int(n) if n.to_i64() == Some(-1));
        if minus_one {
            if let Some((lh, largs)) = args[0].parts() {
                let lname = lh.sym_id().map(|id| i.t.name_of(id).to_string());
                if lname.as_deref() == Some("Log") && largs.len() == 1 && same_q(&largs[0], var) {
                    let u = largs[0].clone();
                    return Some(app(i, "LogIntegral", &u));
                }
            }
        }
        return None;
    }

    // f(c x)/(a x)  ->  (1/a) · Special(c x), for f in {Sin, Cos, Exp}.
    // Times is Orderless, so either argument may be the reciprocal.
    if h.is_symbol(sys.times) && args.len() == 2 {
        for (num_idx, den_idx) in [(0usize, 1usize), (1usize, 0usize)] {
            let num = &args[num_idx];
            let den = &args[den_idx];
            let Some((ph, pargs)) = den.parts() else { continue };
            if !ph.is_symbol(sys.power) || pargs.len() != 2 {
                continue;
            }
            if !matches!(&pargs[1], Expr::Int(n) if n.to_i64() == Some(-1)) {
                continue;
            }
            let Some(a) = coefficient_of_var(i, &pargs[0], var) else { continue };
            let Some((fh, fargs)) = num.parts() else { continue };
            if fargs.len() != 1 {
                continue;
            }
            let Some(_c) = coefficient_of_var(i, &fargs[0], var) else { continue };
            let fname = fh.sym_id().map(|id| i.t.name_of(id).to_string());
            let special = match fname.as_deref() {
                Some("Sin") => "SinIntegral",
                Some("Cos") => "CosIntegral",
                Some("Exp") => "ExpIntegralEi",
                _ => continue,
            };
            let v = fargs[0].clone();
            let anti = app(i, special, &v);
            if a == 1 {
                return Some(anti);
            }
            let scale = Expr::rat(crate::rational::Rat::new(
                crate::bigint::BigInt::from_i64(1),
                crate::bigint::BigInt::from_i64(a),
            ));
            return Some(mul2(i, scale, anti));
        }
        return None;
    }
    None
}

pub fn integrate_expr(i: &mut Interp, e: &Expr, var: &Expr) -> EvalRes {
    integrate_rec(i, e, var, 0)
}

/// True when `e` still contains an unevaluated `Integrate[..., var]`, i.e. no
/// antiderivative was found. The definite path must not substitute bounds into
/// such an expression: doing so produced nonsense like
/// `Integrate[Exp[-\[Infinity]^2] Cos[\[Infinity]]^2, \[Infinity]] - ...`.
fn contains_integrate_of(integrate_id: usize, e: &Expr, var: &Expr) -> bool {
    if let Some((h, args)) = e.parts() {
        if h.sym_id() == Some(integrate_id)
            && args.iter().any(|a| same_q(a, var) || contains_integrate_of(integrate_id, a, var))
        {
            return true;
        }
        return args.iter().any(|a| contains_integrate_of(integrate_id, a, var));
    }
    false
}

/// Depth-limited worker: no integrand rule here is recursive enough to need
/// more than a few levels, so anything deeper is left unevaluated rather than
/// risking a stack overflow.
fn integrate_rec(i: &mut Interp, e: &Expr, var: &Expr, depth: u32) -> EvalRes {
    if depth > 32 {
        return Ok(unevaluated_call(i, "Integrate", &[e.clone(), var.clone()]));
    }
    let sys = i.t.sys;
    // ∫ x dx  (a bare symbol is x^1).
    if same_q(e, var) {
        return power_rule(i, var, 1);
    }
    // ∫ c dx = c x for anything free of the variable.
    if is_free_of(i, e, var)? {
        return eval_call(i, Expr::Sym(sys.times), vec![e.clone(), var.clone()]);
    }
    // Linearity over Plus first: a sum is integrated term by term, which always
    // terminates. The heuristic rules below (by parts, arc forms) can otherwise
    // match a whole sum and recurse without progress — Integrate[-3 x^2 + 5 x^3, x]
    // used to hang this way.
    if let Some((h, args)) = e.parts() {
        if h.is_symbol(sys.plus) {
            let mut parts = Vec::new();
            for a in args {
                parts.push(integrate_rec(i, a, var, depth + 1)?);
            }
            return eval_call(i, Expr::Sym(sys.plus), parts);
        }
    }
    // Recognisable special-function antiderivatives first.
    if let Some(anti) = special_antiderivative(i, e, var) {
        i.step("special function antiderivative", &anti);
        return Ok(anti);
    }
    // Sin[u]^n / Cos[u]^n by the reduction formula.
    if let Some(anti) = trig_power_antiderivative(i, e, var)? {
        i.step("trigonometric power reduction", &anti);
        return Ok(anti);
    }
    if let Some(anti) = trig_product_antiderivative(i, e, var)? {
        i.step("trigonometric substitution", &anti);
        return Ok(anti);
    }
    // x^n f(x) by parts, x^n Log[x], and the 1/quadratic arc forms.
    if let Some(v) = by_parts(i, e, var, depth)? {
        i.step("integration by parts", &v);
        return Ok(v);
    }
    if let Some(anti) = log_power_antiderivative(i, e, var)? {
        i.step("x^n Log[x] rule", &anti);
        return Ok(anti);
    }
    if let Some(v) = quadratic_antiderivative(i, e, var)? {
        i.step("arc form", &v);
        return Ok(v);
    }
    if let Some(v) = repeated_quadratic_antiderivative(i, e, var)? {
        i.step("repeated quadratic reduction", &v);
        return Ok(v);
    }
    if let Some(v) = linear_over_quadratic_antiderivative(i, e, var)? {
        i.step("linear over quadratic", &v);
        return Ok(v);
    }
    if let Some((h, args)) = e.parts() {
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
            // Without a factor free of the variable there is nothing to pull
            // out; fall through instead of stopping here, so a product that is
            // really a rational function still reaches the partial-fraction
            // rule below. Recursing on the same product would loop
            // (e.g. Integrate[Sin[x]/x, x]), and `same_q` guards that.
            if let Some(rest) = rest {
                if !consts.is_empty() && !same_q(&rest, e) {
                    let c = eval_call(i, Expr::Sym(sys.times), consts)?;
                    let inner = integrate_rec(i, &rest, var, depth + 1)?;
                    return eval_call(i, Expr::Sym(sys.times), vec![c, inner]);
                }
            }
        }
        // x^a for any exponent a free of x (x^n, Sqrt[x], 1/x, ...), plus
        // 1/(b + a x). The old code forced every exponent through
        // `expr_to_int(...).unwrap_or(-1)`, so Integrate[x^n, x] and
        // Integrate[Sqrt[x], x] silently returned Log[x].
        if h.is_symbol(sys.power) && args.len() == 2 {
            if same_q(&args[0], var) {
                let ex = &args[1];
                if !is_free_of(i, ex, var)? {
                    return Ok(unevaluated_call(i, "Integrate", &[e.clone(), var.clone()]));
                }
                let np1 = eval_call(i, Expr::Sym(sys.plus), vec![ex.clone(), Expr::int_i64(1)])?;
                if crate::builtins::num::is_numeric_zero(&np1) {
                    let log_id = i.t.intern("Log");
                    return eval_call(i, Expr::Sym(log_id), vec![var.clone()]);
                }
                let p = eval_call(i, Expr::Sym(sys.power), vec![var.clone(), np1.clone()])?;
                let inv = pw(i, &np1, Expr::int_i64(-1));
                return eval_call(i, Expr::Sym(sys.times), vec![inv, p]);
            }
            // 1/(a x + b): Power[linear, -1]
            let n_is_minus_one = matches!(&args[1], Expr::Int(k) if k.to_i64() == Some(-1));
            if n_is_minus_one {
                if let Some((la, _)) = linear_form(i, &args[0], var)? {
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
    // Rational functions: split into partial fractions and integrate those
    // (apart_expr records the "partial fractions" step itself).
    if let Some(apart) = crate::builtins::poly::apart_expr(i, e, var)? {
        if !same_q(&apart, e) {
            return integrate_rec(i, &apart, var, depth + 1);
        }
    }
    Ok(unevaluated_call(i, "Integrate", &[e.clone(), var.clone()]))
}

/// `x^n * {Sin, Cos, Exp}[x]` by integration by parts, n a positive integer.
/// Constants free of the variable are carried through.
fn by_parts(i: &mut Interp, e: &Expr, var: &Expr, depth: u32) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;
    let Some((h, args)) = e.parts() else {
        return Ok(None);
    };
    if !h.is_symbol(sys.times) {
        return Ok(None);
    }
    let mut n: Option<i64> = None;
    let mut fname: Option<String> = None;
    let mut consts: Vec<Expr> = Vec::new();
    for a in args {
        if same_q(a, var) {
            n = Some(1);
            continue;
        }
        if let Some((ph, pa)) = a.parts() {
            if ph.is_symbol(sys.power) && pa.len() == 2 && same_q(&pa[0], var) {
                if let Some(k) = crate::builtins::core::expr_to_int(&pa[1]) {
                    if k >= 1 {
                        n = Some(k);
                        continue;
                    }
                }
                return Ok(None);
            }
            if pa.len() == 1 && same_q(&pa[0], var) {
                if let Some(id) = ph.sym_id() {
                    let name = i.t.name_of(id).to_string();
                    if name == "Sin" || name == "Cos" || name == "Exp" {
                        if fname.is_some() {
                            return Ok(None);
                        }
                        fname = Some(name);
                        continue;
                    }
                }
            }
        }
        if is_free_of(i, a, var)? {
            consts.push(a.clone());
            continue;
        }
        return Ok(None);
    }
    let (Some(n), Some(name)) = (n, fname) else {
        return Ok(None);
    };
    let f_anti = match name.as_str() {
        "Sin" => neg_app(i, "Cos", var),
        "Cos" => app(i, "Sin", var),
        _ => app(i, "Exp", var),
    };
    // ∫ x^n f = x^n F - n ∫ x^(n-1) F.
    let xn = pw(i, var, Expr::int_i64(n));
    let xn_f = eval_call(i, Expr::Sym(sys.times), vec![xn, f_anti.clone()])?;
    let nm1 = pw(i, var, Expr::int_i64(n - 1));
    let inner_integrand = eval_call(i, Expr::Sym(sys.times), vec![nm1, f_anti])?;
    let inner = integrate_rec(i, &inner_integrand, var, depth + 1)?;
    let neg_n_inner = eval_call(
        i,
        Expr::Sym(sys.times),
        vec![Expr::int_i64(-n), inner],
    )?;
    let bracket = eval_call(i, Expr::Sym(sys.plus), vec![xn_f, neg_n_inner])?;
    if consts.is_empty() {
        return Ok(Some(bracket));
    }
    let c = eval_call(i, Expr::Sym(sys.times), consts)?;
    Ok(Some(eval_call(i, Expr::Sym(sys.times), vec![c, bracket])?))
}

/// ∫ Sin[u]^n du (or Cos[u]^n) with respect to `u`, by the standard reduction
/// formula
///
/// ```text
/// ∫ sin^n = -(1/n) sin^(n-1) cos + ((n-1)/n) ∫ sin^(n-2)
/// ∫ cos^n =  (1/n) cos^(n-1) sin + ((n-1)/n) ∫ cos^(n-2)
/// ```
///
/// which is what makes `Integrate[Sin[x]^2, x]` (and every other power) work
/// without expanding products of different frequencies.
fn trig_power_integral(i: &mut Interp, name: &str, u: &Expr, n: i64) -> Result<Expr, Flow> {
    let sys = i.t.sys;
    if n <= 1 {
        return Ok(match (name, n) {
            ("Sin", 1) => neg_app(i, "Cos", u),
            ("Cos", 1) => app(i, "Sin", u),
            _ => u.clone(), // ∫ 1 du = u
        });
    }
    let prev = trig_power_integral(i, name, u, n - 2)?;
    let f = app(i, name, u);
    let fp = eval_call(
        i,
        Expr::Sym(sys.power),
        vec![f, Expr::int_i64(n - 1)],
    )?;
    let other = if name == "Sin" {
        app(i, "Cos", u)
    } else {
        app(i, "Sin", u)
    };
    let pair = eval_call(i, Expr::Sym(sys.times), vec![fp, other])?;
    let sign = if name == "Sin" { -1 } else { 1 };
    let coef1 = Expr::rat(Rat::new(BigInt::from_i64(sign), BigInt::from_i64(n)));
    let coef2 = Expr::rat(Rat::new(
        BigInt::from_i64(n - 1),
        BigInt::from_i64(n),
    ));
    let t1 = eval_call(i, Expr::Sym(sys.times), vec![coef1, pair])?;
    let t2 = eval_call(i, Expr::Sym(sys.times), vec![coef2, prev])?;
    eval_call(i, Expr::Sym(sys.plus), vec![t1, t2])
}

/// `Sin[u]^n`/`Cos[u]^n` with `u` linear in the variable, via the reduction
/// formula above (the inverse of the inner slope comes out as a factor).
fn trig_power_antiderivative(
    i: &mut Interp,
    e: &Expr,
    var: &Expr,
) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;
    let Some((h, args)) = e.parts() else {
        return Ok(None);
    };
    if !h.is_symbol(sys.power) || args.len() != 2 {
        return Ok(None);
    }
    let Some(n) = crate::builtins::core::expr_to_int(&args[1]) else {
        return Ok(None);
    };
    if n < 1 || n > 200 {
        return Ok(None);
    }
    let Some((ih, iargs)) = args[0].parts() else {
        return Ok(None);
    };
    if iargs.len() != 1 {
        return Ok(None);
    }
    let name = if ih.is_symbol_named(&i.t, "Sin") {
        "Sin"
    } else if ih.is_symbol_named(&i.t, "Cos") {
        "Cos"
    } else {
        return Ok(None);
    };
    let u = iargs[0].clone();
    if is_free_of(i, &u, var)? {
        return Ok(None); // a constant power is handled as a constant factor
    }
    let Some((a, _b)) = linear_form(i, &u, var)? else {
        return Ok(None);
    };
    if a == 0 {
        return Ok(None);
    }
    let inner = trig_power_integral(i, name, &u, n)?;
    let inv_a = Expr::rat(Rat::new(BigInt::one(), BigInt::from_i64(a)));
    let res = eval_call(i, Expr::Sym(sys.times), vec![inv_a, inner])?;
    // The recurrence nests the coefficients; Expand keeps the antiderivative in
    // the shape a reader expects (3/8 x + 3/8 Cos[x] Sin[x] + ...).
    let expanded = app(i, "Expand", &res);
    Ok(Some(i.eval(&expanded)?))
}

/// `∫ dx/(c0 + c2 x^2)^j` for j >= 1, by the reduction
///
/// ```text
/// ∫ dx/Q^j = x c2^j/(2 c0 (j-1) Q^(j-1)) + ((2j-3) c2/(2 c0 (j-1))) ∫ dx/Q^(j-1)
/// ```
///
/// with the arc-tangent as the base case. Needs `c0, c2 > 0`.
fn quad_power_integral(
    i: &mut Interp,
    var: &Expr,
    c0: &Rat,
    c2: &Rat,
    j: i64,
    q_expr: &Expr,
) -> Result<Expr, Flow> {
    let sys = i.t.sys;
    if j == 1 {
        let den_sqrt = rat_sqrt_expr(i, &c0.mul(c2))?;
        let ratio_sqrt = rat_sqrt_expr(i, &c2.div(c0))?;
        let arg = eval_call(i, Expr::Sym(sys.times), vec![var.clone(), ratio_sqrt])?;
        let atan = app(i, "ArcTan", &arg);
        let inv = Expr::node(Expr::Sym(sys.power), vec![den_sqrt, Expr::int_i64(-1)]);
        return eval_call(i, Expr::Sym(sys.times), vec![inv, atan]);
    }
    // first term: x · c2^j / (2 c0 (j-1) Q^(j-1))
    let q_pow = eval_call(
        i,
        Expr::Sym(sys.power),
        vec![q_expr.clone(), Expr::int_i64(-(j - 1))],
    )?;
    let mut c2_pow = Rat::from_i64(1);
    for _ in 0..j {
        c2_pow = c2_pow.mul(c2);
    }
    let denom = Rat::from_i64(2).mul(c0).mul(&Rat::from_i64(j - 1));
    let coef1 = c2_pow.div(&denom);
    let first = eval_call(
        i,
        Expr::Sym(sys.times),
        vec![Expr::rat(coef1), var.clone(), q_pow],
    )?;
    // second term: (2j-3) c2/(2 c0 (j-1)) · I_(j-1)
    let coef2 = Rat::from_i64(2 * j - 3).mul(c2).div(&denom);
    let inner = quad_power_integral(i, var, c0, c2, j - 1, q_expr)?;
    let second = eval_call(i, Expr::Sym(sys.times), vec![Expr::rat(coef2), inner])?;
    eval_call(i, Expr::Sym(sys.plus), vec![first, second])
}

/// `(b x + c)/(c0 + c2 x^2)^j` for j >= 2 — what `Apart` produces for a repeated
/// quadratic factor. The `b x` part is a log, the constant part the reduction above.
fn repeated_quadratic_antiderivative(
    i: &mut Interp,
    e: &Expr,
    var: &Expr,
) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;

    // Either a bare Power[Q, -j] or a product carrying one such factor.
    let (q_expr, j, num_expr) = {
        let mut found: Option<(Expr, i64, Expr)> = None;
        let factors: Vec<Expr> = match e.parts() {
            Some((h, args)) if h.is_symbol(sys.times) => args.to_vec(),
            _ => vec![e.clone()],
        };
        for (idx, f) in factors.iter().enumerate() {
            let Some((fh, fargs)) = f.parts() else { continue };
            if !fh.is_symbol(sys.power) || fargs.len() != 2 {
                continue;
            }
            let Some(n) = crate::builtins::core::expr_to_int(&fargs[1]) else {
                continue;
            };
            if n >= 0 {
                continue;
            }
            if found.is_some() {
                return Ok(None);
            }
            let mut rest: Vec<Expr> = Vec::new();
            for (k, g) in factors.iter().enumerate() {
                if k != idx {
                    rest.push(g.clone());
                }
            }
            let num = if rest.is_empty() {
                Expr::int_i64(1)
            } else {
                eval_call(i, Expr::Sym(sys.times), rest)?
            };
            found = Some((fargs[0].clone(), -n, num));
        }
        match found {
            Some(v) => v,
            None => return Ok(None),
        }
    };
    // The kernel keeps `(1 + x^2)^-2` as `Power[Power[1 + x^2, 2], -1]`, so a
    // `-1` exponent with a power as its base is the repeated factor.
    let (q_expr, j) = if j == 1 {
        match q_expr.parts() {
            Some((bh, bargs))
                if bh.is_symbol(sys.power)
                    && bargs.len() == 2
                    && crate::builtins::core::expr_to_int(&bargs[1]).map_or(false, |m| m >= 2) =>
            {
                let m = crate::builtins::core::expr_to_int(&bargs[1]).unwrap();
                (bargs[0].clone(), m)
            }
            _ => (q_expr, j),
        }
    } else {
        (q_expr, j)
    };
    if j < 2 {
        return Ok(None); // j == 1 is the plain quadratic rule
    }
    let Some(den) = crate::builtins::poly::poly_coeffs(i, &q_expr, var)? else {
        return Ok(None);
    };
    if den.len() != 3 || !den[1].is_zero() {
        return Ok(None);
    }
    let (c0, c2) = (den[0].clone(), den[2].clone());
    if c0.is_zero() || c2.is_zero() || c0.is_neg() || c2.is_neg() {
        return Ok(None);
    }
    let Some(num) = crate::builtins::poly::poly_coeffs(i, &num_expr, var)? else {
        return Ok(None);
    };
    if num.len() > 2 {
        return Ok(None);
    }
    let zero = Rat::from_i64(0);
    let c = num.first().cloned().unwrap_or_else(|| zero.clone());
    let b = num.get(1).cloned().unwrap_or_else(|| zero.clone());
    let mut terms: Vec<Expr> = Vec::new();
    if !b.is_zero() {
        let coef = b.div(&Rat::from_i64(2).mul(&c2).mul(&Rat::from_i64(j - 1)));
        let q_pow = eval_call(
            i,
            Expr::Sym(sys.power),
            vec![q_expr.clone(), Expr::int_i64(-(j - 1))],
        )?;
        terms.push(eval_call(
            i,
            Expr::Sym(sys.times),
            vec![Expr::rat(zero.sub(&coef)), q_pow],
        )?);
    }
    if !c.is_zero() {
        let arc = quad_power_integral(i, var, &c0, &c2, j, &q_expr)?;
        terms.push(eval_call(i, Expr::Sym(sys.times), vec![Expr::rat(c), arc])?);
    }
    if terms.is_empty() {
        return Ok(None);
    }
    let out = if terms.len() == 1 {
        terms.pop().unwrap()
    } else {
        Expr::node(Expr::Sym(sys.plus), terms)
    };
    Ok(Some(i.eval(&out)?))
}

/// `(b x + c)/(c0 + c2 x^2) -> b/(2 c2) Log[c0 + c2 x^2]` + c/sqrt(c0 c2) ArcTan[x sqrt(c2/c0)]`
/// — how a rational function with an irreducible quadratic factor integrates,
/// after `Apart` has split it (`Integrate[1/(x^3 + x), x]`).
fn linear_over_quadratic_antiderivative(
    i: &mut Interp,
    e: &Expr,
    var: &Expr,
) -> Result<Option<Expr>, Flow> {
    let (num_e, den_e) = crate::builtins::poly::together_parts(i, e)?;
    let Some(den) = crate::builtins::poly::poly_coeffs(i, &den_e, var)? else {
        return Ok(None);
    };
    if den.len() != 3 || !den[1].is_zero() {
        return Ok(None);
    }
    let (c0, c2) = (den[0].clone(), den[2].clone());
    if c0.is_zero() || c2.is_zero() {
        return Ok(None);
    }
    let Some(num) = crate::builtins::poly::poly_coeffs(i, &num_e, var)? else {
        return Ok(None);
    };
    if num.len() > 2 {
        return Ok(None);
    }
    let zero = Rat::from_i64(0);
    let c = num.first().cloned().unwrap_or_else(|| zero.clone());
    let b = num.get(1).cloned().unwrap_or_else(|| zero.clone());
    if c.is_zero() && b.is_zero() {
        return Ok(None);
    }
    let mut terms: Vec<Expr> = Vec::new();
    // The logarithmic part exists whenever the linear coefficient does.
    if !b.is_zero() {
        let coef = b.div(&Rat::from_i64(2).mul(&c2));
        let den_expr = crate::builtins::poly::coeffs_to_expr(i, &den, var)?;
        let log = app(i, "Log", &den_expr);
        terms.push(eval_call(
            i,
            Expr::Sym(i.t.sys.times),
            vec![Expr::rat(coef), log],
        )?);
    }
    // The arc-tangent part needs a positive quadratic form.
    if !c.is_zero() && !c0.is_neg() && !c2.is_neg() {
        let den_sqrt = rat_sqrt_expr(i, &c0.mul(&c2))?;
        let ratio_sqrt = rat_sqrt_expr(i, &c2.div(&c0))?;
        let arg = eval_call(i, Expr::Sym(i.t.sys.times), vec![var.clone(), ratio_sqrt])?;
        let atan = app(i, "ArcTan", &arg);
        let scale = eval_call(
            i,
            Expr::Sym(i.t.sys.times),
            vec![Expr::rat(c), Expr::node(Expr::Sym(i.t.sys.power), vec![den_sqrt, Expr::int_i64(-1)])],
        )?;
        terms.push(eval_call(i, Expr::Sym(i.t.sys.times), vec![scale, atan])?);
    }
    if terms.is_empty() {
        return Ok(None);
    }
    let out = if terms.len() == 1 {
        terms.pop().unwrap()
    } else {
        Expr::node(Expr::Sym(i.t.sys.plus), terms)
    };
    Ok(Some(i.eval(&out)?))
}

/// Coefficients (ascending) of `(1 - t^2)^k * t^p`.
fn one_minus_t2_power(k: i64, p: i64) -> Vec<Rat> {
    let mut c = vec![Rat::from_i64(1)];
    for _ in 0..k {
        // multiply by (1 - t^2)
        let mut out = vec![Rat::from_i64(0); c.len() + 2];
        for (j, v) in c.iter().enumerate() {
            out[j] = out[j].add(v);
            out[j + 2] = out[j + 2].sub(v);
        }
        c = out;
    }
    if p > 0 {
        let mut out = vec![Rat::from_i64(0); p as usize];
        out.extend(c);
        c = out;
    }
    c
}

/// `∫ sin^m u cos^n u du` by the substitution `t = sin u` (n odd) or `t = cos u`
/// (m odd): the integrand becomes a polynomial in `t`, integrated term by term.
/// The `-1` for the cosine case is `du = -sin u du`.
fn trig_substitution_integral(
    i: &mut Interp,
    u: &Expr,
    m: i64,
    n: i64,
) -> Result<Expr, Flow> {
    let sys = i.t.sys;
    let (k, p, base_name, sign) = if n % 2 == 1 {
        ((n - 1) / 2, m, "Sin", 1i64)
    } else {
        ((m - 1) / 2, n, "Cos", -1i64)
    };
    let coeffs = one_minus_t2_power(k, p);
    let base = app(i, base_name, u);
    let mut terms: Vec<Expr> = Vec::new();
    for (j, c) in coeffs.iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        let power = (j + 1) as i64;
        let coef = c.div(&Rat::from_i64(power));
        let pw = eval_call(
            i,
            Expr::Sym(sys.power),
            vec![base.clone(), Expr::int_i64(power)],
        )?;
        terms.push(eval_call(
            i,
            Expr::Sym(sys.times),
            vec![Expr::rat(coef), pw],
        )?);
    }
    let mut poly = eval_call(i, Expr::Sym(sys.plus), terms)?;
    if sign < 0 {
        let neg = neg1(i, poly);
        poly = i.eval(&neg)?;
    }
    Ok(poly)
}

/// `∫ sin^m u cos^n u du` for integer exponents >= 0, with respect to `u`.
///
/// One odd exponent: the substitution above. Both even: the reduction
///
/// ```text
/// ∫ sin^m cos^n = sin^(m+1)u cos^(n-1)u/(m+n) + ((n-1)/(m+n)) ∫ sin^m cos^(n-2)
/// ```
///
/// which lowers `n` by two until it reaches zero (and then `m`, by the mirror
/// recurrence). This is the product-to-sum identity in disguise: it leaves the
/// answer in terms of `sin u cos u` rather than `sin 2u`, which is a valid
/// antiderivative either way.
fn trig_product_integral_u(i: &mut Interp, u: &Expr, m: i64, n: i64) -> Result<Expr, Flow> {
    let sys = i.t.sys;
    if m == 0 && n == 0 {
        return Ok(u.clone());
    }
    if m % 2 == 1 || n % 2 == 1 {
        return trig_substitution_integral(i, u, m, n);
    }
    let (mm, nn) = (m, n);
    // Reduce the larger even exponent by two.
    if nn >= 2 {
        let s = app(i, "Sin", u);
        let c = app(i, "Cos", u);
        let sp = eval_call(i, Expr::Sym(sys.power), vec![s, Expr::int_i64(mm + 1)])?;
        let cp = eval_call(i, Expr::Sym(sys.power), vec![c, Expr::int_i64(nn - 1)])?;
        let pair = eval_call(i, Expr::Sym(sys.times), vec![sp, cp])?;
        let coef = Rat::new(BigInt::one(), BigInt::from_i64(mm + nn));
        let first = eval_call(
            i,
            Expr::Sym(sys.times),
            vec![Expr::rat(coef.clone()), pair],
        )?;
        let rest = trig_product_integral_u(i, u, mm, nn - 2)?;
        let coef2 = Rat::new(BigInt::from_i64(nn - 1), BigInt::from_i64(mm + nn));
        let second = eval_call(
            i,
            Expr::Sym(sys.times),
            vec![Expr::rat(coef2), rest],
        )?;
        return eval_call(i, Expr::Sym(sys.plus), vec![first, second]);
    }
    // n == 0, m even >= 2: ∫ sin^m = -(sin^(m-1) cos)/m + ((m-1)/m) ∫ sin^(m-2)
    let s = app(i, "Sin", u);
    let c = app(i, "Cos", u);
    let sp = eval_call(i, Expr::Sym(sys.power), vec![s, Expr::int_i64(mm - 1)])?;
    let pair = eval_call(i, Expr::Sym(sys.times), vec![sp, c])?;
    let neg = Rat::new(BigInt::from_i64(-1), BigInt::from_i64(mm));
    let first = eval_call(i, Expr::Sym(sys.times), vec![Expr::rat(neg), pair])?;
    let rest = trig_product_integral_u(i, u, mm - 2, 0)?;
    let coef2 = Rat::new(BigInt::from_i64(mm - 1), BigInt::from_i64(mm));
    let second = eval_call(i, Expr::Sym(sys.times), vec![Expr::rat(coef2), rest])?;
    eval_call(i, Expr::Sym(sys.plus), vec![first, second])
}

/// `Sin[u]^m Cos[u]^n` for any integer exponents >= 0, with `u` linear in the
/// variable (the inverse slope comes out as a factor).
fn trig_product_antiderivative(
    i: &mut Interp,
    e: &Expr,
    var: &Expr,
) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;
    let Some((h, args)) = e.parts() else {
        return Ok(None);
    };
    if !h.is_symbol(sys.times) {
        return Ok(None);
    }
    let mut sin_exp: Option<i64> = None;
    let mut cos_exp: Option<i64> = None;
    let mut arg: Option<Expr> = None;
    for a in args {
        let (name, inner, exp) = match a.parts() {
            Some((ph, pargs)) if pargs.len() == 1 && ph.is_symbol_named(&i.t, "Sin") => {
                ("Sin", pargs[0].clone(), 1)
            }
            Some((ph, pargs)) if pargs.len() == 1 && ph.is_symbol_named(&i.t, "Cos") => {
                ("Cos", pargs[0].clone(), 1)
            }
            Some((ph, pargs)) if pargs.len() == 2 && ph.is_symbol(sys.power) => {
                let Some(e2) = crate::builtins::core::expr_to_int(&pargs[1]) else {
                    return Ok(None);
                };
                let Some((bh, bargs)) = pargs[0].parts() else {
                    return Ok(None);
                };
                if bargs.len() != 1 {
                    return Ok(None);
                }
                if bh.is_symbol_named(&i.t, "Sin") {
                    ("Sin", bargs[0].clone(), e2)
                } else if bh.is_symbol_named(&i.t, "Cos") {
                    ("Cos", bargs[0].clone(), e2)
                } else {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        };
        if exp < 0 {
            return Ok(None);
        }
        match arg {
            None => arg = Some(inner.clone()),
            Some(ref u) if same_q(u, &inner) => {}
            Some(_) => return Ok(None), // different frequencies: not this rule
        }
        let slot = if name == "Sin" { &mut sin_exp } else { &mut cos_exp };
        match slot {
            None => *slot = Some(exp),
            Some(_) => return Ok(None), // a power split over several factors
        }
    }
    let (Some(m), Some(n), Some(u)) = (sin_exp, cos_exp, arg) else {
        return Ok(None);
    };
    if m > 400 || n > 400 {
        return Ok(None);
    }
    let Some((slope, _)) = linear_form(i, &u, var)? else {
        return Ok(None);
    };
    if slope == 0 {
        return Ok(None);
    }
    let poly = trig_product_integral_u(i, &u, m, n)?;
    let scale = Expr::rat(Rat::new(BigInt::one(), BigInt::from_i64(slope)));
    let res = eval_call(i, Expr::Sym(sys.times), vec![scale, poly])?;
    let expanded = app(i, "Expand", &res);
    Ok(Some(i.eval(&expanded)?))
}

/// `1/(c0 + c2 x^2) -> ArcTan`, and `1/Sqrt[c0 - a x^2] -> ArcSin`.
fn quadratic_antiderivative(
    i: &mut Interp,
    e: &Expr,
    var: &Expr,
) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;
    let Some((h, args)) = e.parts() else {
        return Ok(None);
    };
    if !h.is_symbol(sys.power) || args.len() != 2 {
        return Ok(None);
    }
    // Normalise Power[Power[den, 1/2], -1] to (den, -1/2).
    let (den, exp) = if let Some((ih, ia)) = args[0].parts() {
        if ih.is_symbol(sys.power) && ia.len() == 2 && same_q(&args[1], &Expr::int_i64(-1)) {
            let ne = neg1(i, ia[1].clone());
            (ia[0].clone(), i.eval(&ne)?)
        } else {
            (args[0].clone(), args[1].clone())
        }
    } else {
        (args[0].clone(), args[1].clone())
    };
    let Some(c) = crate::builtins::poly::poly_coeffs(i, &den, var)? else {
        return Ok(None);
    };
    if c.len() != 3 || !c[1].is_zero() {
        return Ok(None);
    }
    let (c0, c2) = (c[0].clone(), c[2].clone());
    if same_q(&exp, &Expr::int_i64(-1)) {
        if c0.is_zero() || c2.is_zero() || c0.is_neg() || c2.is_neg() {
            return Ok(None);
        }
        let den_sqrt = rat_sqrt_expr(i, &c0.mul(&c2))?;
        let ratio_sqrt = rat_sqrt_expr(i, &c2.div(&c0))?;
        let arg = eval_call(i, Expr::Sym(sys.times), vec![var.clone(), ratio_sqrt])?;
        let atan = app(i, "ArcTan", &arg);
        let inv = Expr::node(Expr::Sym(sys.power), vec![den_sqrt, Expr::int_i64(-1)]);
        return Ok(Some(eval_call(i, Expr::Sym(sys.times), vec![inv, atan])?));
    }
    let is_minus_half =
        matches!(&exp, Expr::Rat(r) if r.num.to_i64() == Some(-1) && r.den.to_i64() == Some(2));
    if is_minus_half {
        if c0.is_zero() || !c2.is_neg() {
            return Ok(None);
        }
        let a = c2.clone().neg();
        let a_sqrt = rat_sqrt_expr(i, &a)?;
        let ratio_sqrt = rat_sqrt_expr(i, &a.div(&c0))?;
        let arg = eval_call(i, Expr::Sym(sys.times), vec![var.clone(), ratio_sqrt])?;
        let asin = app(i, "ArcSin", &arg);
        let inv = Expr::node(Expr::Sym(sys.power), vec![a_sqrt, Expr::int_i64(-1)]);
        return Ok(Some(eval_call(i, Expr::Sym(sys.times), vec![inv, asin])?));
    }
    Ok(None)
}

/// `Sqrt[r]` as an evaluated expression (exact when the root is exact).
fn rat_sqrt_expr(i: &mut Interp, r: &Rat) -> Result<Expr, Flow> {
    let e = Expr::node(Expr::Sym(i.t.intern("Sqrt")), vec![Expr::rat(r.clone())]);
    i.eval(&e)
}

/// `[x^n Log[x] dx` in closed form, for integer n >= 0.
fn log_power_antiderivative(
    i: &mut Interp,
    e: &Expr,
    var: &Expr,
) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;
    let Some((h, args)) = e.parts() else { return Ok(None) };
    if !h.is_symbol(sys.times) || args.len() != 2 {
        return Ok(None);
    }
    let log_id = i.t.intern("Log");
    // Log[x] is a Node, so compare its head symbol, not sym_id() of the node.
    let is_log_of_var = |e: &Expr| -> bool {
        if let Some((lh, la)) = e.parts() {
            lh.sym_id() == Some(log_id) && la.len() == 1 && same_q(&la[0], var)
        } else {
            false
        }
    };
    let (p_part, log_part) = if is_log_of_var(&args[1]) {
        (&args[0], &args[1])
    } else if is_log_of_var(&args[0]) {
        (&args[1], &args[0])
    } else {
        return Ok(None);
    };
    let n = if same_q(p_part, var) {
        1i64
    } else if let Some((hp, ap)) = p_part.parts() {
        if hp.is_symbol(sys.power) && ap.len() == 2 && same_q(&ap[0], var) {
            match crate::builtins::core::expr_to_int(&ap[1]) {
                Some(k) if k >= 0 => k,
                _ => return Ok(None),
            }
        } else {
            return Ok(None);
        }
    } else {
        return Ok(None);
    };
    let Some((hlog, alog)) = log_part.parts() else { return Ok(None) };
    if hlog.sym_id() != Some(log_id) || alog.len() != 1 || !same_q(&alog[0], var) {
        return Ok(None);
    }
    let np1 = n + 1;
    let xp = pw(i, var, Expr::int_i64(np1));
    let logx = app(i, "Log", var);
    let inv = Expr::node(Expr::Sym(sys.power), vec![Expr::int_i64(np1), Expr::int_i64(-1)]);
    let t1 = eval_call(i, Expr::Sym(sys.times), vec![inv, xp.clone(), logx])?;
    let inv2 = Expr::node(
        Expr::Sym(sys.power),
        vec![Expr::int_i64(np1 * np1), Expr::int_i64(-1)],
    );
    let t2 = eval_call(i, Expr::Sym(sys.times), vec![inv2, xp])?;
    let nt2 = neg1(i, t2);
    Ok(Some(eval_call(i, Expr::Sym(sys.plus), vec![t1, nt2])?))
}

fn power_rule(i: &mut Interp, var: &Expr, n: i64) -> EvalRes {
    let sys = i.t.sys;
    if n == -1 {
        let log_id = i.t.intern("Log");
        let res = eval_call(i, Expr::Sym(log_id), vec![var.clone()])?;
        i.step("log rule", &res);
        return Ok(res);
    }
    let np1 = Expr::int_i64(n + 1);
    let p = eval_call(i, Expr::Sym(sys.power), vec![var.clone(), np1.clone()])?;
    let inv = Expr::node(
        Expr::Sym(sys.power),
        vec![np1.clone(), Expr::int_i64(-1)],
    );
    let res = eval_call(i, Expr::Sym(sys.times), vec![inv, p])?;
    i.step("power rule", &res);
    Ok(res)
}

/// Coefficient of `var` when `e` is `k*var` or `var`; None otherwise.
fn linear_term(i: &mut Interp, e: &Expr, var: &Expr) -> Option<i64> {
    if same_q(e, var) {
        return Some(1);
    }
    if let Some((h, a)) = e.parts() {
        if h.is_symbol(i.t.sys.times) && a.len() == 2 {
            if same_q(&a[1], var) {
                return crate::builtins::core::expr_to_int(&a[0]);
            }
            if same_q(&a[0], var) {
                return crate::builtins::core::expr_to_int(&a[1]);
            }
        }
    }
    None
}

/// Returns (a, b) with arg = a*var + b when arg is linear (integer a, b).
/// Handles either canonical order of the sum (`x + 1` and `1 + 2 x`).
fn linear_form(i: &mut Interp, arg: &Expr, var: &Expr) -> Result<Option<(i64, i64)>, Flow> {
    if same_q(arg, var) {
        return Ok(Some((1, 0)));
    }
    match arg {
        Expr::Int(n) => Ok(Some((0, n.to_i64().unwrap_or(0)))),
        _ => {
            if let Some((h, hargs)) = arg.parts() {
                if h.is_symbol(i.t.sys.times) && hargs.len() == 2 {
                    if let Some(k) = linear_term(i, arg, var) {
                        return Ok(Some((k, 0)));
                    }
                }
                if h.is_symbol(i.t.sys.plus) && hargs.len() == 2 {
                    for (t, c) in [(&hargs[0], &hargs[1]), (&hargs[1], &hargs[0])] {
                        if let Some(k) = linear_term(i, t, var) {
                            if let Some(cb) = crate::builtins::core::expr_to_int(c) {
                                return Ok(Some((k, cb)));
                            }
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
            let integrate_id = i.t.intern("Integrate");
            if contains_integrate_of(integrate_id, &f, &v) {
                // No antiderivative: leave the definite integral alone (keeping
                // extra arguments such as Assumptions) instead of substituting
                // the bounds into an Integrate head.
                return Ok(unevaluated_call(i, "Integrate", args));
            }
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
    // SeriesData's coefficient list is indexed from `nmin`, so drop the leading
    // zeros: the list holds c_nmin, c_{nmin+1}, ... (consumers add `nmin` to the
    // list index). Keeping the leading zeros here made
    // Normal[Series[Sin[x], {x, 0, 5}]] start at x^2 instead of x.
    let nmin = coeffs
        .iter()
        .position(|c| !same_q(c, &Expr::int_i64(0)))
        .unwrap_or(0);
    let coeffs: Vec<Expr> = coeffs.into_iter().skip(nmin).collect();
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

/// Split `f` into (numerator, denominator) when it is a quotient: `a/b`,
/// `a b^-1`, `a b^-2`, ... Returns None for anything else.
fn quotient_parts(i: &mut Interp, f: &Expr) -> Option<(Expr, Expr)> {
    let sys = i.t.sys;
    let (h, args) = f.parts()?;
    if h.is_symbol(sys.power) && args.len() == 2 {
        if matches!(&args[1], Expr::Int(n) if n.to_i64() == Some(-1)) {
            return Some((Expr::int_i64(1), args[0].clone()));
        }
        return None;
    }
    if !h.is_symbol(sys.times) {
        return None;
    }
    let mut num: Vec<Expr> = Vec::new();
    let mut den: Vec<Expr> = Vec::new();
    for a in args {
        if let Some((ph, pargs)) = a.parts() {
            if ph.is_symbol(sys.power) && pargs.len() == 2 {
                if let Some(k) = crate::builtins::core::expr_to_int(&pargs[1]) {
                    if k < 0 {
                        den.push(Expr::node(
                            Expr::Sym(sys.power),
                            vec![pargs[0].clone(), Expr::int_i64(-k)],
                        ));
                        continue;
                    }
                }
            }
        }
        num.push(a.clone());
    }
    if den.is_empty() {
        return None;
    }
    let num_e = match num.len() {
        0 => Expr::int_i64(1),
        1 => num[0].clone(),
        _ => Expr::node(Expr::Sym(sys.times), num),
    };
    let den_e = if den.len() == 1 {
        den[0].clone()
    } else {
        Expr::node(Expr::Sym(sys.times), den)
    };
    Some((num_e, den_e))
}

/// Does `e` contain Power[base, exponent] with both base and exponent
/// depending on the limit variable? At an infinite limit such a form can hide
/// an indeterminate case that naive substitution erases: (1 + 1/x)^x would
/// substitute to 1^Infinity = 1, but its limit is E.
fn power_with_var_exponent(i: &mut Interp, e: &Expr, var: &Expr) -> Result<bool, Flow> {
    if let Some((h, args)) = e.parts() {
        if h.is_symbol(i.t.sys.power) && args.len() == 2 {
            if !is_free_of(i, &args[0], var)? && !is_free_of(i, &args[1], var)? {
                return Ok(true);
            }
        }
        for a in args {
            if power_with_var_exponent(i, a, var)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// l'Hôpital's rule for a 0/0 quotient. Returns None when the form does not
/// resolve, so the caller can stay unevaluated rather than report a number.
fn lhopital_limit(i: &mut Interp, f: &Expr, v: &Expr, a: &Expr) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;
    let Some((mut num, mut den)) = quotient_parts(i, f) else {
        return Ok(None);
    };
    for _ in 0..8 {
        let nv = subst_var(i, &num, v, a)?;
        let dv = subst_var(i, &den, v, a)?;
        let n_zero = crate::builtins::num::is_numeric_zero(&nv)
            || crate::builtins::num::is_undetermined(&nv, &i.t);
        let d_zero = crate::builtins::num::is_numeric_zero(&dv)
            || crate::builtins::num::is_undetermined(&dv, &i.t);
        if n_zero && d_zero {
            num = d_expr(i, &num, v)?;
            den = d_expr(i, &den, v)?;
            continue;
        }
        // Evaluate the ratio of the differentiated parts AT the limit point:
        // returning it still in x would give Cos[x] instead of Cos[0] = 1.
        let ratio = Expr::node(
            Expr::Sym(sys.times),
            vec![
                num.clone(),
                Expr::node(Expr::Sym(sys.power), vec![den.clone(), Expr::int_i64(-1)]),
            ],
        );
        let q = subst_var(i, &ratio, v, a)?;
        if !crate::builtins::num::contains_undetermined(&q, &i.t) {
            return Ok(Some(q));
        }
        return Ok(None);
    }
    Ok(None)
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
    // Direct substitution and l'Hôpital probing deliberately evaluate forms
    // such as 0/0; those messages belong to the search, not the answer, and
    // must not surface under an otherwise clean Limit result.
    let saved = i.messages.len();
    let result = limit_core(i, &f, &v, &a, args);
    i.messages.truncate(saved);
    result
}

fn limit_core(i: &mut Interp, f: &Expr, v: &Expr, a: &Expr, args: &[Expr]) -> EvalRes {
    let sys = i.t.sys;
    let a_is_inf = a.is_symbol(sys.infinity) || a.is_symbol(sys.complexinfinity);
    let direct = subst_var(i, f, v, a)?;
    let dangerous = a_is_inf && power_with_var_exponent(i, f, v)?;
    let direct_clean = !crate::builtins::num::contains_undetermined(&direct, &i.t);
    if direct_clean && !dangerous {
        i.step("direct substitution", &direct);
        return Ok(direct);
    }
    // At infinity a bare Infinity is a legitimate limit (x, x^2, ...).
    if a_is_inf
        && !dangerous
        && (direct.is_symbol(sys.infinity) || direct.is_symbol(sys.complexinfinity))
    {
        i.step("limit at infinity", &direct);
        return Ok(direct);
    }
    // 0/0 and relatives: l'Hôpital. The old series-only path silently returned
    // 0 for Limit[Sin[x]/x, x -> 0].
    if let Some(q) = lhopital_limit(i, f, v, a)? {
        i.step("l'Hopital (0/0)", &q);
        return Ok(q);
    }
    // Taylor series at a finite point: the lowest non-zero coefficient.
    if !crate::builtins::num::contains_undetermined(a, &i.t) {
        let s = series_at(i, f, v, a, 4)?;
        if let Some((sh, shargs)) = s.parts() {
            if sh.is_symbol(i.t.intern("SeriesData")) && shargs.len() == 6 {
                let coeffs = shargs[2].node_args();
                let nmin = shargs[3].to_i64_pub().unwrap_or(0);
                if nmin > 0 || coeffs.is_empty() {
                    let z = Expr::int_i64(0);
                    i.step("series expansion", &z);
                    return Ok(z);
                }
                i.step("series expansion", &coeffs[0]);
                return Ok(coeffs[0].clone());
            }
        }
    }
    // No trustworthy answer: stay unevaluated rather than invent one.
    Ok(unevaluated_call(i, "Limit", args))
}