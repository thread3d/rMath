//! Symbolic closed forms for `Sum` and `Product`: constants, polynomials in the
//! index (Faulhaber, degrees 0..=4), geometric series (finite and infinite),
//! the Basel sums, and factorials. Anything not recognised returns `None` so
//! the caller can fall back to numeric iteration or stay unevaluated.

use crate::bigint::BigInt;
use crate::eval::{Flow, Interp};
use crate::expr::{same_q, Expr};
use crate::rational::Rat;

fn plus(i: &Interp, args: Vec<Expr>) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.plus), args)
}
fn times(i: &Interp, args: Vec<Expr>) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.times), args)
}
fn power(i: &Interp, a: Expr, b: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.power), vec![a, b])
}
fn rat(a: i64, b: i64) -> Expr {
    Expr::rat(Rat::new(BigInt::from_i64(a), BigInt::from_i64(b)))
}
fn pi(i: &Interp) -> Expr {
    // Pi is interned during setup_system.
    Expr::Sym(i.t.get("Pi").expect("Pi symbol"))
}

/// Does `e` avoid `var`? (heads are not inspected, matching the calculus code)
fn free_of(e: &Expr, var: &Expr) -> bool {
    if same_q(e, var) {
        return false;
    }
    if let Some((_, args)) = e.parts() {
        return args.iter().all(|a| free_of(a, var));
    }
    true
}

/// `Sum_{i=1}^{n} i^k` as an expression (Faulhaber), for k = 0..=4.
fn faulhaber(i: &Interp, k: i64, n: &Expr) -> Expr {
    let n1 = plus(i, vec![n.clone(), Expr::int_i64(1)]);
    match k {
        0 => n.clone(),
        1 => times(i, vec![rat(1, 2), n.clone(), n1]),
        2 => times(
            i,
            vec![
                rat(1, 6),
                n.clone(),
                n1,
                plus(
                    i,
                    vec![times(i, vec![Expr::int_i64(2), n.clone()]), Expr::int_i64(1)],
                ),
            ],
        ),
        3 => times(
            i,
            vec![
                rat(1, 4),
                power(i, n.clone(), Expr::int_i64(2)),
                power(i, n1, Expr::int_i64(2)),
            ],
        ),
        4 => times(
            i,
            vec![
                rat(1, 30),
                n.clone(),
                n1,
                plus(
                    i,
                    vec![times(i, vec![Expr::int_i64(2), n.clone()]), Expr::int_i64(1)],
                ),
                plus(
                    i,
                    vec![
                        times(i, vec![Expr::int_i64(3), power(i, n.clone(), Expr::int_i64(2))]),
                        times(i, vec![Expr::int_i64(3), n.clone()]),
                        Expr::int_i64(-1),
                    ],
                ),
            ],
        ),
        _ => n.clone(),
    }
}

fn sum_power(i: &mut Interp, k: i64, lo: &Expr, hi: &Expr) -> Result<Expr, Flow> {
    let hi_expr = faulhaber(i, k, hi);
    let f_hi = i.eval(&hi_expr)?;
    let lo_m1_expr = plus(i, vec![lo.clone(), Expr::int_i64(-1)]);
    let lo_m1 = i.eval(&lo_m1_expr)?;
    if crate::builtins::num::is_numeric_zero(&lo_m1) {
        return Ok(f_hi);
    }
    let lo_expr = faulhaber(i, k, &lo_m1);
    let f_lo = i.eval(&lo_expr)?;
    let diff = plus(i, vec![f_hi, times(i, vec![Expr::int_i64(-1), f_lo])]);
    i.eval(&diff)
}

fn count_expr(i: &mut Interp, lo: &Expr, hi: &Expr) -> Result<Expr, Flow> {
    let e = plus(
        i,
        vec![
            hi.clone(),
            times(i, vec![Expr::int_i64(-1), lo.clone()]),
            Expr::int_i64(1),
        ],
    );
    i.eval(&e)
}

/// `Sum_{i=lo}^{hi} r^i = (r^(hi+1) - r^lo)/(r-1)`.
fn geom_finite(i: &mut Interp, r: &Expr, lo: &Expr, hi: &Expr) -> Result<Expr, Flow> {
    let hi1 = plus(i, vec![hi.clone(), Expr::int_i64(1)]);
    let hi1 = i.eval(&hi1)?;
    let neg_rlo = times(i, vec![Expr::int_i64(-1), power(i, r.clone(), lo.clone())]);
    let num = plus(i, vec![power(i, r.clone(), hi1), neg_rlo]);
    let num = i.eval(&num)?;
    let den = plus(i, vec![r.clone(), Expr::int_i64(-1)]);
    let den = i.eval(&den)?;
    let q = times(i, vec![num, power(i, den, Expr::int_i64(-1))]);
    i.eval(&q)
}

/// `Sum_{i=lo}^{Infinity} r^i = r^lo/(1-r)`.
fn geom_infinite(i: &mut Interp, r: &Expr, lo: &Expr) -> Result<Expr, Flow> {
    let num = power(i, r.clone(), lo.clone());
    let num = i.eval(&num)?;
    let den = plus(i, vec![Expr::int_i64(1), times(i, vec![Expr::int_i64(-1), r.clone()])]);
    let den = i.eval(&den)?;
    let q = times(i, vec![num, power(i, den, Expr::int_i64(-1))]);
    i.eval(&q)
}

/// A single additive term of the summand, or None when unrecognised.
fn sum_term(
    i: &mut Interp,
    f: &Expr,
    var: &Expr,
    lo: &Expr,
    hi: &Expr,
) -> Result<Option<Expr>, Flow> {
    // Constant: f * (hi - lo + 1).
    if free_of(f, var) {
        let count = count_expr(i, lo, hi)?;
        let e = times(i, vec![f.clone(), count]);
        return Ok(Some(i.eval(&e)?));
    }
    // Geometric r^(c*var) -> (r^c)^var, and var^k for k = 0..=4. Nested powers
    // are normalised first: 1/x^2 arrives as Power[Power[x, 2], -1].
    if let Some((base, exp)) = as_power(i, f)? {
        if free_of(&base, var) {
            if let Some(c) = exponent_coeff_with(i, &exp, var) {
                let ratio_e = power(i, base, c);
                let ratio = i.eval(&ratio_e)?;
                return Ok(Some(geom_finite(i, &ratio, lo, hi)?));
            }
        }
        if same_q(&base, var) {
            if let Some(k) = crate::builtins::core::expr_to_int(&exp) {
                if (0..=4).contains(&k) {
                    return Ok(Some(sum_power(i, k, lo, hi)?));
                }
            }
        }
    }
    // Sum over a Plus, term by term.
    if let Some((h, args)) = f.parts() {
        if h.is_symbol(i.t.sys.plus) {
            let mut acc = Expr::int_i64(0);
            for a in args {
                match sum_term(i, a, var, lo, hi)? {
                    Some(v) => {
                        let e = plus(i, vec![acc, v]);
                        acc = i.eval(&e)?;
                    }
                    None => return Ok(None),
                }
            }
            return Ok(Some(acc));
        }
    }
    if same_q(f, var) {
        return Ok(Some(sum_power(i, 1, lo, hi)?));
    }
    // c * rest with c free of var.
    if let Some((h, args)) = f.parts() {
        if h.is_symbol(i.t.sys.times) {
            let mut consts: Vec<Expr> = Vec::new();
            let mut rest: Vec<Expr> = Vec::new();
            for a in args {
                if free_of(a, var) {
                    consts.push(a.clone());
                } else {
                    rest.push(a.clone());
                }
            }
            if !consts.is_empty() && !rest.is_empty() {
                let rest_e = if rest.len() == 1 {
                    rest[0].clone()
                } else {
                    Expr::node(Expr::Sym(i.t.sys.times), rest)
                };
                if let Some(s) = sum_term(i, &rest_e, var, lo, hi)? {
                    let c = i.eval(&Expr::node(Expr::Sym(i.t.sys.times), consts))?;
                    let e = times(i, vec![c, s]);
                    return Ok(Some(i.eval(&e)?));
                }
            }
        }
    }
    Ok(None)
}

/// Normalise nested powers: `1/x^2` arrives as `Power[Power[x, 2], -1]`, which
/// this returns as `(x, -2)`; `1/2^n` becomes `(2, -n)`.
fn as_power(i: &mut Interp, e: &Expr) -> Result<Option<(Expr, Expr)>, Flow> {
    let Some((h, a)) = e.parts() else {
        return Ok(None);
    };
    if !h.is_symbol(i.t.sys.power) || a.len() != 2 {
        return Ok(None);
    }
    if let Some((ih, ia)) = a[0].parts() {
        if ih.is_symbol(i.t.sys.power) && ia.len() == 2 {
            if let Some(k) = crate::builtins::core::expr_to_int(&a[1]) {
                let prod = Expr::node(
                    Expr::Sym(i.t.sys.times),
                    vec![ia[1].clone(), Expr::int_i64(k)],
                );
                let new_exp = i.eval(&prod)?;
                return Ok(Some((ia[0].clone(), new_exp)));
            }
        }
    }
    Ok(Some((a[0].clone(), a[1].clone())))
}

/// Some(c) when `exp` is `c*var` (or just `var`), with c free of var.
fn exponent_coeff_with(i: &Interp, exp: &Expr, var: &Expr) -> Option<Expr> {
    if same_q(exp, var) {
        return Some(Expr::int_i64(1));
    }
    if let Some((h, args)) = exp.parts() {
        if h.is_symbol(i.t.sys.times) && args.len() == 2 {
            if same_q(&args[1], var) && free_of(&args[0], var) {
                return Some(args[0].clone());
            }
            if same_q(&args[0], var) && free_of(&args[1], var) {
                return Some(args[1].clone());
            }
        }
    }
    None
}

fn symbolic_product(
    i: &mut Interp,
    body: &Expr,
    var: &Expr,
    lo: &Expr,
    hi: &Expr,
) -> Result<Option<Expr>, Flow> {
    if hi.is_symbol(i.t.sys.infinity) {
        return Ok(None);
    }
    // Constant factor: c^count.
    if free_of(body, var) {
        let count = count_expr(i, lo, hi)?;
        let e = power(i, body.clone(), count);
        return Ok(Some(i.eval(&e)?));
    }
    // Product_{i=1}^{n} i = n!.
    if same_q(body, var) && same_q(lo, &Expr::int_i64(1)) {
        let fact = Expr::node(Expr::Sym(i.t.intern("Factorial")), vec![hi.clone()]);
        return Ok(Some(i.eval(&fact)?));
    }
    // Product c^i = c^(Sum i).
    if let Some((h, args)) = body.parts() {
        if h.is_symbol(i.t.sys.power)
            && args.len() == 2
            && free_of(&args[0], var)
            && same_q(&args[1], var)
        {
            let s = sum_power(i, 1, lo, hi)?;
            let e = power(i, args[0].clone(), s);
            return Ok(Some(i.eval(&e)?));
        }
    }
    Ok(None)
}

/// A closed form for `Sum[body, spec]` / `Product[body, spec]`, or None.
pub fn symbolic_sum_product(
    i: &mut Interp,
    body: &Expr,
    spec: &Expr,
    plus_mode: bool,
) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;
    let Some((h, hargs)) = spec.parts() else {
        return Ok(None);
    };
    if !h.is_symbol(sys.list) || hargs.len() < 2 || hargs.len() > 4 {
        return Ok(None);
    }
    let Some(var) = hargs[0].sym_id() else {
        return Ok(None);
    };
    let var_e = Expr::Sym(var);
    let (lo, hi) = match hargs.len() {
        2 => (Expr::int_i64(1), i.eval(&hargs[1])?),
        3 | 4 => {
            if hargs.len() == 4 {
                let step = i.eval(&hargs[3])?;
                if crate::builtins::core::expr_to_int(&step) != Some(1) {
                    return Ok(None);
                }
            }
            (i.eval(&hargs[1])?, i.eval(&hargs[2])?)
        }
        _ => return Ok(None),
    };
    if !plus_mode {
        return symbolic_product(i, body, &var_e, &lo, &hi);
    }
    // A held summand often carries a leading constant factor
    // (`1/n^2` parses as Times[1, Power[Power[n, 2], -1]]): pull it out.
    if let Some((h, args)) = body.parts() {
        if h.is_symbol(sys.times) {
            let mut consts: Vec<Expr> = Vec::new();
            let mut rest: Vec<Expr> = Vec::new();
            for a in args {
                if free_of(a, &var_e) {
                    consts.push(a.clone());
                } else {
                    rest.push(a.clone());
                }
            }
            if !consts.is_empty() && !rest.is_empty() {
                let rest_e = if rest.len() == 1 {
                    rest[0].clone()
                } else {
                    Expr::node(Expr::Sym(sys.times), rest)
                };
                if let Some(s) = symbolic_sum_product(i, &rest_e, spec, true)? {
                    let c = i.eval(&Expr::node(Expr::Sym(sys.times), consts))?;
                    let e = times(i, vec![c, s]);
                    return Ok(Some(i.eval(&e)?));
                }
            }
        }
    }
    if hi.is_symbol(sys.infinity) {
        if let Some((base, exp)) = as_power(i, body)? {
            // Basel: Sum 1/i^2 = Pi^2/6, Sum 1/i^4 = Pi^4/90.
            if same_q(&lo, &Expr::int_i64(1)) && same_q(&base, &var_e) {
                if let Some(k) = crate::builtins::core::expr_to_int(&exp) {
                    if k == -2 {
                        let e = times(i, vec![rat(1, 6), power(i, pi(i), Expr::int_i64(2))]);
                        return Ok(Some(i.eval(&e)?));
                    }
                    if k == -4 {
                        let e = times(i, vec![rat(1, 90), power(i, pi(i), Expr::int_i64(4))]);
                        return Ok(Some(i.eval(&e)?));
                    }
                }
            }
            // Infinite geometric r^(c*i).
            if free_of(&base, &var_e) {
                if let Some(c) = exponent_coeff_with(i, &exp, &var_e) {
                    let ratio_e = power(i, base, c);
                    let ratio = i.eval(&ratio_e)?;
                    return Ok(Some(geom_infinite(i, &ratio, &lo)?));
                }
            }
        }
        return Ok(None);
    }
    sum_term(i, body, &var_e, &lo, &hi)
}
