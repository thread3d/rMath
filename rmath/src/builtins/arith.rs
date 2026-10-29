//! Arithmetic built-ins built on the numeric core in `num.rs`.

use crate::bigint::BigInt;
use crate::builtins::core::{expr_to_int, unevaluated_call};
use crate::builtins::num::*;
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::{Expr, NumVal};
use crate::rational::Rat;

pub fn bool_e(sys: crate::expr::Sys, v: bool) -> Expr {
    Expr::Sym(if v { sys.r#true } else { sys.r#false })
}

pub fn eval_int_of(i: &mut Interp, e: &Expr) -> EvalRes {
    i.eval(e)
}

// ============================ Plus ============================

/// Route an arithmetic operation through the arbitrary-precision evaluator when
/// an operand is a BigReal. If the shape cannot be evaluated at precision (it
/// needs a machine transcendental), the BigReals are demoted to f64 and the
/// normal path handles it, so arithmetic never leaves a BigReal stranded.
fn bigreal_op(i: &mut Interp, head: usize, args: &[Expr]) -> Option<EvalRes> {
    use crate::builtins::num::{big_real_eval, contains_bigreal, demote_bigreal, min_bigreal_prec};
    if !args.iter().any(contains_bigreal) {
        return None;
    }
    let prec = args.iter().filter_map(min_bigreal_prec).min().unwrap_or(20);
    let call = Expr::node(Expr::Sym(head), args.to_vec());
    if let Some(v) = big_real_eval(&call, &i.t, prec) {
        return Some(Ok(v));
    }
    let demoted: Vec<Expr> = args.iter().map(demote_bigreal).collect();
    let sys = i.t.sys;
    Some(if head == sys.plus {
        b_plus(i, &demoted)
    } else if head == sys.times {
        b_times(i, &demoted)
    } else {
        b_power(i, &demoted)
    })
}

pub fn b_plus(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let sys = i.t.sys;
    if let Some(r) = bigreal_op(i, sys.plus, args) {
        return r;
    }
    // Infinity is not an ordinary term: `Indeterminate + x` stays
    // Indeterminate, and `Infinity + (-Infinity)` is an indeterminate form.
    // (Previously the two infinities merged to a zero coefficient and the sum
    // silently collapsed to 0.)
    let mut saw_pos_inf = false;
    let mut saw_neg_inf = false;
    let mut saw_indet = false;
    for a in args {
        if is_undetermined(a, &i.t) {
            saw_indet = true;
            continue;
        }
        if is_infinity_like(a, &i.t) {
            saw_pos_inf = true;
            continue;
        }
        if let Some((h, hargs)) = a.parts() {
            if h.is_symbol(sys.times) {
                let mut has_inf = false;
                let mut negative = false;
                for x in hargs {
                    if is_infinity_like(x, &i.t) {
                        has_inf = true;
                    }
                    match x {
                        Expr::Int(n) if n.sign < 0 => negative = true,
                        Expr::Rat(r) if r.is_neg() => negative = true,
                        Expr::Real(v) if *v < 0.0 => negative = true,
                        _ => {}
                    }
                }
                if has_inf {
                    if negative {
                        saw_neg_inf = true;
                    } else {
                        saw_pos_inf = true;
                    }
                }
            }
        }
    }
    if saw_indet || (saw_pos_inf && saw_neg_inf) {
        i.message("Plus", "indet");
        return Ok(Expr::Sym(sys.indeterminate));
    }
    let times_id = sys.times;
    let mut exact_sum = Rat::from_i64(0);
    let mut any_machine = false;
    let mut machine_sum = 0.0f64;
    let mut c_re: Option<NumVal> = None;
    let mut c_im: Option<NumVal> = None;
    let mut terms: Vec<(Expr, Rat, Option<f64>)> = Vec::new();
    // Key -> index in `terms`. Merging like terms by scanning the vector compared
    // every key with every other one, i.e. O(k^2) deep `same_q` calls for a sum of
    // k terms — 740k comparisons for the 861 terms of `Expand[(x+y+z)^40]`.
    let mut term_index: std::collections::HashMap<Expr, usize> = std::collections::HashMap::new();

    for a in args {
        match a {
            Expr::Int(_) | Expr::Rat(_) => {
                exact_sum = exact_sum.add(&a.as_rat_exact().unwrap());
            }
            Expr::Real(x) => {
                any_machine = true;
                machine_sum += x;
            }
            Expr::Cnum(c) => {
                let (re, im) = (c.re.clone(), c.im.clone());
                c_re = Some(match &c_re {
                    None => re,
                    Some(p) => nv_add(p, &re),
                });
                c_im = Some(match &c_im {
                    None => im,
                    Some(p) => nv_add(p, &im),
                });
                if matches!(c.re, NumVal::F(_)) || matches!(c.im, NumVal::F(_)) {
                    any_machine = true;
                }
            }
            other => {
                let (ec, mc, key) = term_coeff(other, &i.t);
                if let Some(&idx) = term_index.get(&key) {
                    let (_, e, m) = &mut terms[idx];
                    *e = e.add(&ec);
                    *m = match (*m, mc) {
                        (None, None) => None,
                        (Some(x), Some(y)) => Some(x + y),
                        (None, Some(y)) => Some(y),
                        (Some(x), None) => Some(x),
                    };
                } else {
                    term_index.insert(key.clone(), terms.len());
                    terms.push((key, ec, mc));
                }
            }
        }
    }

    let mut out: Vec<Expr> = Vec::new();

    // Complex part, if any.
    if let Some(im) = &c_im {
        // The real part is the complex real part *plus* any exact/machine
        // terms (1 + I was collapsing to 0 + 1 I).
        let base = if any_machine {
            NumVal::F(exact_sum.to_f64() + machine_sum)
        } else {
            NumVal::R(exact_sum.clone())
        };
        let re = match &c_re {
            Some(r) => nv_add(r, &base),
            None => base,
        };
        let e = Expr::cnum(re, im.clone());
        if !crate::expr::same_q(&e, &Expr::int_i64(0)) {
            out.push(e);
        }
    } else {
        let num = if any_machine {
            Expr::real(exact_sum.to_f64() + machine_sum)
        } else {
            Expr::rat(exact_sum.clone())
        };
        if !crate::expr::same_q(&num, &Expr::int_i64(0)) {
            out.push(num);
        }
    }

    for (key, ec, mc) in terms {
        let nonzero = match mc {
            Some(m) => m != 0.0 || !ec.is_zero(),
            None => !ec.is_zero(),
        };
        if !nonzero {
            continue;
        }
        out.push(coeff_term(&key, &ec, mc, times_id));
    }

    if out.is_empty() {
        return Ok(Expr::int_i64(0));
    }
    if out.len() == 1 {
        return Ok(out.pop().unwrap());
    }
    Ok(Expr::node(Expr::Sym(sys.plus), out))
}

// ============================ Times ============================

pub fn b_times(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let sys = i.t.sys;
    if let Some(r) = bigreal_op(i, sys.times, args) {
        return r;
    }
    let times_id = sys.times;
    let power_id = sys.power;
    let mut ec = Rat::from_i64(1);
    let mut mc: Option<f64> = None;
    let mut c_re: Option<NumVal> = None;
    let mut c_im: Option<NumVal> = None;
    // (base, exponent) — the exponent may be exact, machine, or symbolic, so it
    // is kept as an expression. The old three-field form stored a symbolic
    // exponent as 0, which silently deleted the factor (`x^n y` -> `y`).
    let mut powers: Vec<(Expr, Expr)> = Vec::new();
    let mut power_index: std::collections::HashMap<Expr, usize> = std::collections::HashMap::new();

    for a in args {
        match a {
            Expr::Int(_) | Expr::Rat(_) => {
                ec = ec.mul(&a.as_rat_exact().unwrap());
            }
            Expr::Real(v) => {
                mc = Some(mc.unwrap_or(1.0) * v);
            }
            Expr::Cnum(c) => {
                let (re, im) = (c.re.clone(), c.im.clone());
                // The first complex factor is assigned, not multiplied into
                // zero (which silently dropped the imaginary part: 3 I -> 0).
                match (&c_re, &c_im) {
                    (None, None) => {
                        c_re = Some(re);
                        c_im = Some(im);
                    }
                    _ => {
                        let cre = c_re.clone().unwrap_or_else(nv_zero);
                        let cim = c_im.clone().unwrap_or_else(nv_zero);
                        let nre = nv_sub(&nv_mul(&cre, &re), &nv_mul(&cim, &im));
                        let nim = nv_add(&nv_mul(&cre, &im), &nv_mul(&cim, &re));
                        c_re = Some(nre);
                        c_im = Some(nim);
                    }
                }
                if matches!(c.re, NumVal::F(_)) || matches!(c.im, NumVal::F(_)) {
                    mc = Some(mc.unwrap_or(1.0)); // keep machine flag
                }
            }
            Expr::Node(nd) if nd.head.is_symbol(power_id) && nd.args.len() == 2 => {
                push_power(i, &mut powers, &mut power_index, &nd.args[0], &nd.args[1])?;
            }
            other => {
                push_power(i, &mut powers, &mut power_index, other, &Expr::int_i64(1))?;
            }
        }
    }

    // Zero coefficient short-circuits — unless an infinity is involved, in
    // which case the product is an indeterminate form (0*Infinity, 0/0, ...).
    let complex_zero = matches!(
        (&c_re, &c_im),
        (Some(re), Some(im)) if nv_is_zero(re) && nv_is_zero(im)
    );
    let coeff_zero =
        (ec.is_zero() && mc.is_none()) || mc == Some(0.0) || complex_zero;
    if coeff_zero {
        if args.iter().any(|a| contains_undetermined(a, &i.t)) {
            i.message("Times", "indet");
            return Ok(Expr::Sym(sys.indeterminate));
        }
        return Ok(Expr::int_i64(0));
    }

    let mut out: Vec<Expr> = Vec::new();
    // Coefficient first.
    if let (Some(re), Some(im)) = (&c_re, &c_im) {
        // The exact/machine coefficient must scale the complex part too
        // (3 I was becoming 1 I).
        let (sre, sim) = match mc {
            Some(m) => (
                nv_mul(re, &NumVal::F(m)),
                nv_mul(im, &NumVal::F(m)),
            ),
            None => (
                nv_mul(re, &NumVal::R(ec.clone())),
                nv_mul(im, &NumVal::R(ec.clone())),
            ),
        };
        out.push(Expr::cnum(sre, sim));
    } else {
        if let Some(m) = mc {
            let total = m * ec.to_f64();
            if total != 1.0 {
                out.push(Expr::real(total));
            }
        } else if !ec.is_int() || !ec.num.is_one() {
            out.push(Expr::rat(ec.clone()));
        }
    }
    for (base, exp) in powers {
        let one = Expr::int_i64(1);
        if crate::expr::same_q(&exp, &one) {
            out.push(base.clone());
        } else if is_numeric_zero(&exp) {
            // base^0 is 1: the factor contributes nothing (x * x^-1 -> 1).
            continue;
        } else {
            out.push(Expr::node(Expr::Sym(power_id), vec![base.clone(), exp]));
        }
    }
    if out.is_empty() {
        return Ok(Expr::int_i64(1));
    }
    if out.len() == 1 {
        return Ok(out.pop().unwrap());
    }
    Ok(Expr::node(Expr::Sym(times_id), out))
}

/// Merge `base^exp` into the power list, adding the exponents of equal bases.
fn push_power(
    i: &mut Interp,
    p: &mut Vec<(Expr, Expr)>,
    index: &mut std::collections::HashMap<Expr, usize>,
    base: &Expr,
    exp: &Expr,
) -> Result<(), Flow> {
    // Same fix as the like-term merge in `Plus`: a linear scan compares every base
    // with every other one (O(k^2) deep `same_q`), so a product of many factors was
    // quadratic in its length.
    if let Some(&idx) = index.get(base) {
        let (_, e) = &mut p[idx];
        let sum = Expr::node(Expr::Sym(i.t.sys.plus), vec![e.clone(), exp.clone()]);
        *e = i.eval(&sum)?;
        return Ok(());
    }
    index.insert(base.clone(), p.len());
    p.push((base.clone(), exp.clone()));
    Ok(())
}

// ============================ Power ============================

/// Some(()) when the exponent is exactly 1/2.
fn as_half(e: &Expr) -> Option<()> {
    match e {
        Expr::Rat(r) if r.num.to_i64() == Some(1) && r.den.to_i64() == Some(2) => Some(()),
        _ => None,
    }
}

/// A complex number raised to a non-negative integer power.
fn cnum_int_pow(re: &NumVal, im: &NumVal, n: u64) -> (NumVal, NumVal) {
    let mut r = NumVal::R(Rat::from_i64(1));
    let mut i = NumVal::R(Rat::from_i64(0));
    for _ in 0..n {
        let nr = nv_sub(&nv_mul(&r, re), &nv_mul(&i, im));
        let ni = nv_add(&nv_mul(&r, im), &nv_mul(&i, re));
        r = nr;
        i = ni;
    }
    (r, i)
}

/// 1 / (re + i·im) = (re - i·im) / (re² + im²).
fn cnum_recip(re: &NumVal, im: &NumVal) -> (NumVal, NumVal) {
    let den = nv_add(&nv_mul(re, re), &nv_mul(im, im));
    (nv_div(re, &den), nv_neg(&nv_div(im, &den)))
}

pub fn b_power(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Power", "argr");
        return Ok(unevaluated_call(i, "Power", args));
    }
    let base = &args[0];
    let exp = &args[1];
    let sys = i.t.sys;
    if let Some(r) = bigreal_op(i, sys.power, args) {
        return r;
    }
    // Integer powers of a complex number: multiply out (I^2 = -1, (1+I)^2 = 2 I).
    // Negative powers invert the positive power exactly; recursing through
    // Power[z, -1] never terminated (1/I overflowed the stack).
    if let (Expr::Cnum(c), Some(n)) = (base, expr_to_int(exp)) {
        if (0..=64).contains(&n) {
            let (re, im) = cnum_int_pow(&c.re, &c.im, n as u64);
            return Ok(Expr::cnum(re, im));
        }
        if n < 0 && n >= -64 {
            let (re, im) = cnum_int_pow(&c.re, &c.im, (-n) as u64);
            let (rr, ri) = cnum_recip(&re, &im);
            return Ok(Expr::cnum(rr, ri));
        }
    }
    let base_inf = is_infinity_like(base, &i.t);
    let base_indet = is_undetermined(base, &i.t);
    if let Some(e0) = expr_to_int(exp) {
        if base_inf || base_indet {
            // Infinity^0 and Indeterminate^anything are indeterminate forms.
            if e0 == 0 || base_indet {
                i.message("Power", "indet");
                return Ok(Expr::Sym(sys.indeterminate));
            }
            // Infinity^positive = Infinity, Infinity^negative = 0.
            return Ok(if e0 > 0 { base.clone() } else { Expr::int_i64(0) });
        }
        if e0 == 0 {
            if is_numeric_zero(base) {
                i.message("Power", "indet");
                return Ok(Expr::Sym(sys.indeterminate));
            }
            return Ok(Expr::int_i64(1));
        }
        if e0 == 1 {
            return Ok(base.clone());
        }
    }
    if base_indet {
        return Ok(Expr::Sym(sys.indeterminate));
    }
    if base_inf {
        if let Some(s) = exp_sign(exp) {
            if s > 0 {
                return Ok(base.clone());
            }
            if s < 0 {
                return Ok(Expr::int_i64(0));
            }
        }
    }
    if crate::expr::same_q(base, &Expr::int_i64(1)) {
        return Ok(Expr::int_i64(1));
    }
    if is_numeric_zero(base) {
        if let Some(s) = exp_sign(exp) {
            if s > 0 {
                return Ok(Expr::int_i64(0));
            }
            if s < 0 {
                i.message("Power", "infy");
                return Ok(Expr::Sym(sys.complexinfinity));
            }
        }
    }
    // Sqrt of an integer: extract square factors so Sqrt[8] prints as
    // 2 Sqrt[2] (and Solve reports Sqrt[2] rather than 1/2 Sqrt[8]).
    if let (Expr::Int(n), Some(r)) = (base, as_half(exp)) {
        let _ = r;
        let pos = n.clone().abs();
        if let Some((coef, rad)) = crate::builtins::num::simplify_sqrt_int(&pos) {
            let sqrt_head = i.t.intern("Sqrt");
            let root = Expr::node(Expr::Sym(sqrt_head), vec![Expr::Int(rad)]);
            let coeff = Expr::Int(coef);
            let prod = if n.sign < 0 {
                let i_sym = Expr::Sym(i.t.intern("I"));
                Expr::node(Expr::Sym(sys.times), vec![i_sym, coeff, root])
            } else {
                Expr::node(Expr::Sym(sys.times), vec![coeff, root])
            };
            return i.eval(&prod);
        }
    }
    if let (Some(bv), Some(ev)) = (num_val(base), num_val(exp)) {
        if let Some(v) = power_numeric(&bv, &ev) {
            return Ok(v);
        }
        // Negative radicand with an even root: pull out the imaginary unit so
        // the answer appears as I Sqrt[q] rather than Sqrt[-q] (Sqrt[-8]).
        if let (NumVal::R(br), NumVal::R(er)) = (&bv, &ev) {
            if br.num.sign < 0 && !er.is_int() {
                if let (Some(p), Some(q)) = (er.num.to_i64(), er.den.to_i64()) {
                    if q % 2 == 0 && p > 0 {
                        let abs = Rat { num: br.num.clone().abs(), den: br.den.clone() };
                        let root = Expr::node(
                            Expr::Sym(sys.power),
                            vec![Expr::rat(abs), Expr::rat(er.clone())],
                        );
                        let i_sym = Expr::Sym(i.t.intern("I"));
                        let ip = Expr::node(
                            Expr::Sym(sys.power),
                            vec![i_sym, Expr::int_i64(p)],
                        );
                        let prod = Expr::node(Expr::Sym(sys.times), vec![ip, root]);
                        return i.eval(&prod);
                    }
                }
            }
        }
    }
    Ok(unevaluated_call(i, "Power", args))
}

pub fn b_sqrt(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Sqrt", "argr");
        return Ok(unevaluated_call(i, "Sqrt", args));
    }
    // Sqrt[x] == Power[x, 1/2] (evaluated)
    let half = Expr::node(
        Expr::Sym(i.t.sys.power),
        vec![
            args[0].clone(),
            Expr::rat(Rat::new(BigInt::from_i64(1), BigInt::from_i64(2))),
        ],
    );
    i.eval(&half)
}

/// `(num, den)` when `e` is an exact rational multiple of `Pi`, reduced with a
/// positive denominator (`Pi` is `(1, 1)`, `Pi/6` is `(1, 6)`). An approximate
/// coefficient (`0.5 Pi`) is not exact and returns None.
fn pi_frac(i: &Interp, pi: usize, e: &Expr) -> Option<(i64, i64)> {
    if e.sym_id() == Some(pi) {
        return Some((1, 1));
    }
    let (h, args) = e.parts()?;
    if !h.is_symbol(i.t.sys.times) {
        return None;
    }
    let mut coeff: Option<Rat> = None;
    let mut seen_pi = false;
    for a in args {
        if a.sym_id() == Some(pi) {
            if seen_pi {
                return None; // Pi^2 is not a multiple of Pi
            }
            seen_pi = true;
            continue;
        }
        match num_val(a)? {
            NumVal::R(r) => coeff = Some(match coeff { Some(c) => c.mul(&r), None => r }),
            NumVal::F(_) => return None,
        }
    }
    if !seen_pi {
        return None;
    }
    let c = coeff?;
    let (num, den) = (c.num.to_i64()?, c.den.to_i64()?);
    if den == 0 {
        return None;
    }
    Some(reduce_frac(num, den))
}

fn gcd_i64(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

/// Reduce `(num, den)` to lowest terms with `den > 0`.
fn reduce_frac(num: i64, den: i64) -> (i64, i64) {
    let g = gcd_i64(num, den);
    let (num, den) = (num / g, den / g);
    if den < 0 {
        (-num, -den)
    } else {
        (num, den)
    }
}

/// A value the exact-value tables express exactly: the rational `a/b`, or
/// `(a/b) Sqrt[root]` (so `Sqrt[3]/2` is `Sqrt(1, 2, 3)`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Exact {
    Rat(i64, i64),
    Sqrt(i64, i64, i64),
}

impl Exact {
    fn one() -> Exact {
        Exact::Rat(1, 1)
    }
    fn neg(self) -> Exact {
        match self {
            Exact::Rat(a, b) => Exact::Rat(-a, b),
            Exact::Sqrt(a, b, r) => Exact::Sqrt(-a, b, r),
        }
    }
}

fn ex_div(a: Exact, b: Exact) -> Option<Exact> {
    match (a, b) {
        (Exact::Rat(p, q), Exact::Rat(r, s)) if r != 0 => Some(Exact::Rat(p * s, q * r)),
        (Exact::Sqrt(p, q, k), Exact::Rat(r, s)) if r != 0 => Some(Exact::Sqrt(p * s, q * r, k)),
        // 1/Sqrt[k] is Sqrt[k]/k
        (Exact::Rat(p, q), Exact::Sqrt(r, s, k)) if r != 0 => Some(Exact::Sqrt(p * s, q * r * k, k)),
        (Exact::Sqrt(p, q, k), Exact::Sqrt(r, s, m)) if r != 0 && k == m => {
            Some(Exact::Rat(p * s, q * r))
        }
        _ => None,
    }
}

/// Build the expression for an exact value, in the kernel's own canonical
/// form (`1/2 Sqrt[3]` rather than `Sqrt[3]/2`).
fn exact_expr(i: &mut Interp, v: Exact) -> Expr {
    let rat = |a: i64, b: i64| match reduce_frac(a, b) {
        (n, 1) => Expr::int_i64(n),
        (n, d) => Expr::rat(Rat::new(BigInt::from_i64(n), BigInt::from_i64(d))),
    };
    match v {
        Exact::Rat(a, b) => rat(a, b),
        Exact::Sqrt(a, b, root) => {
            let sqrt = Expr::node(
                Expr::Sym(i.t.sys.power),
                vec![
                    Expr::int_i64(root),
                    Expr::rat(Rat::new(BigInt::one(), BigInt::from_i64(2))),
                ],
            );
            if a == 1 && b == 1 {
                sqrt
            } else {
                Expr::node(Expr::Sym(i.t.sys.times), vec![rat(a, b), sqrt])
            }
        }
    }
}

/// `sin(p Pi / q)` for a fraction in `[0, 1/2]`, the only range the table needs.
fn sin_small(p: i64, q: i64) -> Option<Exact> {
    Some(match (p, q) {
        (0, _) => Exact::Rat(0, 1),
        (1, 6) => Exact::Rat(1, 2),
        (1, 4) => Exact::Sqrt(1, 2, 2),
        (1, 3) => Exact::Sqrt(1, 2, 3),
        (1, 2) => Exact::Rat(1, 1),
        _ => return None,
    })
}

/// `sin(k Pi / n)` for any exact rational `k/n`, by symmetry into `[0, 1/2]`.
fn sin_pi(k: i64, n: i64) -> Option<Exact> {
    if n <= 0 || n > i64::MAX / 4 {
        return None;
    }
    let two_n = 2 * n;
    let r = k.rem_euclid(two_n); // angle is in [0, 2 Pi)
    let (p, neg) = if r <= n { (r, false) } else { (two_n - r, true) };
    let (p, q) = if 2 * p <= n { (p, n) } else { (n - p, n) };
    let (p, q) = reduce_frac(p, q);
    let v = sin_small(p, q)?;
    Some(if neg { v.neg() } else { v })
}

/// `cos(k Pi / n)`, using `cos(x) = sin(x + Pi/2)`.
fn cos_pi(k: i64, n: i64) -> Option<Exact> {
    let (k, n) = reduce_frac(k, n);
    sin_pi(k.checked_mul(2)?.checked_add(n)?, n.checked_mul(2)?)
}

/// The exact value of an elementary function at the common special arguments,
/// or None when the table has nothing to say (the caller then tries `f64`).
fn exact_value(i: &mut Interp, name: &str, arg: &Expr) -> Option<Expr> {
    let pi_id = i.t.intern("Pi");
    let e_id = i.t.intern("E");
    let zero = matches!(num_val(arg), Some(NumVal::R(ref r)) if r.is_zero());
    let small_rat = match num_val(arg) {
        Some(NumVal::R(r)) => r.num.to_i64().zip(r.den.to_i64()),
        _ => None,
    };

    // Trigonometry and its reciprocals at an exact multiple of Pi.
    if matches!(name, "Sin" | "Cos" | "Tan" | "Cot" | "Sec" | "Csc") {
        if let Some((k, n)) = pi_frac(i, pi_id, arg) {
            let s = sin_pi(k, n)?;
            let c = cos_pi(k, n)?;
            // A vanishing denominator is ComplexInfinity (Tan[Pi/2], Cot[0]).
            let v = match name {
                "Sin" => Some(s),
                "Cos" => Some(c),
                "Tan" => ex_div(s, c),
                "Cot" => ex_div(c, s),
                "Sec" => ex_div(Exact::one(), c),
                _ => ex_div(Exact::one(), s), // Csc
            };
            return Some(match v {
                Some(v) => exact_expr(i, v),
                None => Expr::Sym(i.t.sys.complexinfinity),
            });
        }
        if zero {
            return Some(match name {
                "Cos" | "Sec" => Expr::int_i64(1),
                "Cot" | "Csc" => Expr::Sym(i.t.sys.complexinfinity),
                _ => Expr::int_i64(0),
            });
        }
    }

    // Hyperbolics vanish at 0 (Sinh[0] must be exactly 0, not 0.).
    if zero {
        match name {
            "Sinh" | "Tanh" => return Some(Expr::int_i64(0)),
            "Cosh" => return Some(Expr::int_i64(1)),
            _ => {}
        }
    }

    match name {
        "Exp" => {
            if zero {
                return Some(Expr::int_i64(1));
            }
            if small_rat == Some((1, 1)) {
                return Some(Expr::Sym(e_id));
            }
            // Exp[Log[x]] = x
            if let Some((h, a)) = arg.parts() {
                if a.len() == 1 && h.is_symbol_named(&i.t, "Log") {
                    return Some(a[0].clone());
                }
            }
        }
        "Log" => {
            if zero {
                return None; // Log[0] stays symbolic (Log[0] is -Infinity in Mathematica)
            }
            if small_rat == Some((1, 1)) {
                return Some(Expr::int_i64(0));
            }
            if arg.is_symbol(e_id) {
                return Some(Expr::int_i64(1));
            }
            if let Some((h, a)) = arg.parts() {
                if h.is_symbol(i.t.sys.power) && a.len() == 2 && a[0].is_symbol(e_id) {
                    return Some(a[1].clone());
                }
            }
        }
        // Inverse trigonometric functions at 0, +/-1 and +/-1/2.
        "ArcSin" | "ArcCos" | "ArcTan" => {
            let angle = match (name, small_rat) {
                ("ArcSin", Some((0, 1))) | ("ArcTan", Some((0, 1))) => Some(Exact::Rat(0, 1)),
                ("ArcSin", Some((1, 1))) => Some(Exact::Rat(1, 2)),
                ("ArcSin", Some((-1, 1))) => Some(Exact::Rat(-1, 2)),
                ("ArcSin", Some((1, 2))) => Some(Exact::Rat(1, 6)),
                ("ArcCos", Some((1, 1))) => Some(Exact::Rat(0, 1)),
                ("ArcCos", Some((0, 1))) => Some(Exact::Rat(1, 2)),
                ("ArcCos", Some((-1, 1))) => Some(Exact::Rat(1, 1)),
                ("ArcCos", Some((1, 2))) => Some(Exact::Rat(1, 3)),
                ("ArcTan", Some((1, 1))) => Some(Exact::Rat(1, 4)),
                ("ArcTan", Some((-1, 1))) => Some(Exact::Rat(-1, 4)),
                _ => None,
            };
            if let Some(Exact::Rat(p, q)) = angle {
                let arg_expr = match (p, q) {
                    (0, _) => Expr::int_i64(0),
                    (1, 1) => Expr::Sym(pi_id),
                    (n, d) => Expr::node(
                        Expr::Sym(i.t.sys.times),
                        vec![exact_expr(i, Exact::Rat(n, d)), Expr::Sym(pi_id)],
                    ),
                };
                return Some(arg_expr);
            }
        }
        _ => {}
    }
    None
}

// ============================ elementary ============================

fn num1(i: &mut Interp, args: &[Expr], f: fn(f64) -> f64, name: &str) -> EvalRes {
    if args.len() != 1 {
        i.message(name, "argr");
        return Ok(unevaluated_call(i, name, args));
    }
    if let Some(v) = exact_value(i, name, &args[0]) {
        return Ok(v);
    }
    if let Some(n) = num_val(&args[0]) {
        let x = nv_f64(&n);
        let r = f(x);
        if r.is_finite() {
            return Ok(Expr::real(r));
        }
    }
    Ok(unevaluated_call(i, name, args))
}

pub fn b_exp(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::exp, "Exp")
}
pub fn b_sin(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::sin, "Sin")
}
pub fn b_cos(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::cos, "Cos")
}
pub fn b_tan(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::tan, "Tan")
}
pub fn b_cot(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, |x| 1.0 / x.tan(), "Cot")
}
pub fn b_sec(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, |x| 1.0 / x.cos(), "Sec")
}
pub fn b_csc(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, |x| 1.0 / x.sin(), "Csc")
}
pub fn b_arcsin(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::asin, "ArcSin")
}
pub fn b_arccos(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::acos, "ArcCos")
}
pub fn b_arctan(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() == 2 {
        if let (Some(a), Some(b)) = (num_val(&args[0]), num_val(&args[1])) {
            return Ok(Expr::real(nv_f64(&a).atan2(nv_f64(&b))));
        }
        return Ok(unevaluated_call(i, "ArcTan", args));
    }
    num1(i, args, f64::atan, "ArcTan")
}
pub fn b_sinh(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::sinh, "Sinh")
}
pub fn b_cosh(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::cosh, "Cosh")
}
pub fn b_tanh(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, args, f64::tanh, "Tanh")
}

pub fn b_log(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() == 2 {
        if let (Some(b), Some(x)) = (num_val(&args[0]), num_val(&args[1])) {
            let b = nv_f64(&b);
            let x = nv_f64(&x);
            if b > 0.0 && b != 1.0 && x > 0.0 {
                return Ok(Expr::real(x.log(b)));
            }
        }
        return Ok(unevaluated_call(i, "Log", args));
    }
    num1(i, args, f64::ln, "Log")
}

pub fn b_abs(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Abs", "argr");
        return Ok(unevaluated_call(i, "Abs", args));
    }
    match &args[0] {
        Expr::Int(n) => Ok(Expr::Int(n.clone().abs())),
        Expr::Rat(r) => Ok(Expr::rat(r.clone().abs())),
        Expr::Real(x) => Ok(Expr::real(x.abs())),
        Expr::BigReal(b) => Ok(Expr::BigReal(Box::new(b.abs()))),
        Expr::Cnum(c) => {
            // Exact: Sqrt[re^2 + im^2] simplified when perfect.
            let (re, im) = (&c.re, &c.im);
            if let (NumVal::R(r1), NumVal::R(r2)) = (re, im) {
                let sq = r1.mul(r1).add(&r2.mul(r2));
                if sq.is_int() {
                    let numcopy = sq.num.clone();
                    let s = numcopy.clone().iroot(2);
                    if s.pow(2).cmp(&numcopy) == std::cmp::Ordering::Equal {
                        let _ = sq;
                        return Ok(Expr::Int(s));
                    }
                }
            }
            Ok(unevaluated_call(i, "Abs", args))
        }
        _ => Ok(unevaluated_call(i, "Abs", args)),
    }
}

pub fn b_sign(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Sign", args));
    }
    if let Some(n) = num_val(&args[0]) {
        let x = nv_f64(&n);
        return Ok(Expr::int_i64(if x > 0.0 {
            1
        } else if x < 0.0 {
            -1
        } else {
            0
        }));
    }
    Ok(unevaluated_call(i, "Sign", args))
}

/// A rounding of a machine real is an exact integer, as in Mathematica
/// (`Floor[2.7]` is 2, not 2.0). Beyond 64 bits the integer cannot be represented
/// exactly, so the real is returned unchanged.
fn exact_int_or_real(v: f64) -> Expr {
    if v.abs() < 9.2e18 {
        Expr::int_i64(v as i64)
    } else {
        Expr::real(v)
    }
}

pub fn b_floor(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Int(_) => Ok(e.clone()),
        Expr::Rat(r) => {
            let (q, _) = r.num.div_mod_floor(&r.den);
            Ok(Expr::Int(q))
        }
        Expr::Real(x) => Ok(exact_int_or_real(x.floor())),
        _ => Ok(unevaluated_call(i, "Floor", args)),
    }
}

pub fn b_ceiling(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Int(_) => Ok(e.clone()),
        Expr::Rat(r) => {
            // ceil(n/d) = -floor(-n/d)
            let neg = Rat { num: r.num.clone().neg(), den: r.den.clone() };
            let (q, _) = neg.num.div_mod_floor(&neg.den);
            Ok(Expr::Int(q.neg()))
        }
        Expr::Real(x) => Ok(exact_int_or_real(x.ceil())),
        _ => Ok(unevaluated_call(i, "Ceiling", args)),
    }
}

pub fn b_round(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Int(_) => Ok(e.clone()),
        Expr::Rat(r) => {
            // Mathematica rounds ties to the nearest even integer
            // (Round[1/2] = 0, Round[5/2] = 2), not away from zero.
            let (q, rem) = r.num.div_mod_floor(&r.den);
            let two = BigInt::from_i64(2);
            let twice = rem.mul(&two);
            let up = match twice.cmp(&r.den) {
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Equal => !q.div_rem(&two).1.is_zero(),
            };
            Ok(Expr::Int(if up { q.add(&BigInt::one()) } else { q }))
        }
        Expr::Real(x) => Ok(exact_int_or_real(x.round_ties_even())),
        _ => Ok(unevaluated_call(i, "Round", args)),
    }
}

pub fn b_integerpart(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Int(_) => Ok(e.clone()),
        Expr::Rat(r) => {
            let (q, _) = r.num.div_rem(&r.den);
            Ok(Expr::Int(q))
        }
        // The integer part of a machine real is an exact integer, not a real:
        // `IntegerPart[2.7]` is 2, not 2.0. (Beyond i64 the truncation cannot be
        // represented exactly, so the real is returned unchanged.)
        Expr::Real(x) => {
            let t = x.trunc();
            if t.abs() < 9.2e18 {
                Ok(Expr::int_i64(t as i64))
            } else {
                Ok(Expr::real(*x))
            }
        }
        _ => Ok(unevaluated_call(i, "IntegerPart", args)),
    }
}

pub fn b_fractionalpart(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Int(_) => Ok(Expr::int_i64(0)),
        Expr::Rat(r) => {
            let (_, m) = r.num.div_rem(&r.den);
            let sign = if r.is_neg() { -1i8 } else { 1 };
            let v = Rat { num: m.abs(), den: r.den.clone() };
            Ok(Expr::rat(if sign < 0 { v.neg() } else { v }))
        }
        Expr::Real(x) => Ok(Expr::real(x - x.trunc())),
        _ => Ok(unevaluated_call(i, "FractionalPart", args)),
    }
}

pub fn b_mod(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Mod", "argr");
        return Ok(unevaluated_call(i, "Mod", args));
    }
    if let (Some(a), Some(b)) = (expr_to_int(&args[0]), expr_to_int(&args[1])) {
        if b == 0 {
            i.message("Mod", "divz");
            return Ok(unevaluated_call(i, "Mod", args));
        }
        let a = BigInt::from_i64(a);
        let b = BigInt::from_i64(b);
        let (_, m) = a.div_mod_floor(&b);
        return Ok(Expr::Int(m));
    }
    // A mixed exact/real pair is computed in machine arithmetic, as Mathematica
    // does: `Mod[7.5, 2]` is 1.5. Only both-real was handled, so the mixed case
    // stayed unevaluated.
    if let (Some(a), Some(b)) = (num_val(&args[0]), num_val(&args[1])) {
        let (x, d) = (nv_f64(&a), nv_f64(&b));
        if d != 0.0 {
            let mut r = x % d;
            if r != 0.0 && ((r < 0.0) != (d < 0.0)) {
                r += d;
            }
            return Ok(Expr::real(r));
        }
    }
    Ok(unevaluated_call(i, "Mod", args))
}

pub fn b_quotient(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Quotient", args));
    }
    if let (Some(a), Some(b)) = (expr_to_int(&args[0]), expr_to_int(&args[1])) {
        if b == 0 {
            i.message("Quotient", "divz");
            return Ok(unevaluated_call(i, "Quotient", args));
        }
        let (q, _) = BigInt::from_i64(a).div_mod_floor(&BigInt::from_i64(b));
        return Ok(Expr::Int(q));
    }
    Ok(unevaluated_call(i, "Quotient", args))
}

/// The exact integer value of `e` as a BigInt — arbitrary size, unlike
/// `expr_to_int`, so GCD and LCM do not stop at the machine word. A machine
/// real counts only when it is integral and fits.
fn exact_int_value(e: &Expr) -> Option<BigInt> {
    match e {
        Expr::Int(n) => Some(n.clone()),
        Expr::Rat(r) if r.is_int() => Some(r.num.clone()),
        Expr::Real(x) if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.2e18 => {
            Some(BigInt::from_i64(*x as i64))
        }
        _ => None,
    }
}

pub fn b_gcd(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut g = BigInt::zero();
    for a in args {
        match exact_int_value(a) {
            Some(v) => g = g.gcd(&v.abs()),
            None => return Ok(unevaluated_call(i, "GCD", args)),
        }
    }
    Ok(Expr::Int(g))
}

pub fn b_lcm(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut l = BigInt::from_i64(1);
    for a in args {
        match exact_int_value(a) {
            Some(v) => {
                let v = v.abs();
                if v.is_zero() {
                    return Ok(Expr::int_i64(0));
                }
                l = l.mul(&v).div_rem(&l.gcd(&v)).0;
            }
            None => return Ok(unevaluated_call(i, "LCM", args)),
        }
    }
    Ok(Expr::Int(l))
}

pub fn b_re(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Cnum(c) => Ok(nv_to_expr(&c.re)),
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) => Ok(e.clone()),
        _ => Ok(unevaluated_call(i, "Re", args)),
    }
}

pub fn b_im(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Cnum(c) => Ok(nv_to_expr(&c.im)),
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) => Ok(Expr::int_i64(0)),
        _ => Ok(unevaluated_call(i, "Im", args)),
    }
}

pub fn b_conjugate(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Cnum(c) => Ok(Expr::cnum(c.re.clone(), nv_neg(&c.im))),
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) => Ok(e.clone()),
        _ => Ok(unevaluated_call(i, "Conjugate", args)),
    }
}

pub fn b_numerator(i: &mut Interp, args: &[Expr]) -> EvalRes {
    match &args[0] {
        Expr::Int(n) => Ok(Expr::Int(n.clone())),
        Expr::Rat(r) => Ok(Expr::Int(r.num.clone())),
        _ => Ok(unevaluated_call(i, "Numerator", args)),
    }
}

pub fn b_denominator(i: &mut Interp, args: &[Expr]) -> EvalRes {
    match &args[0] {
        Expr::Int(_) => Ok(Expr::int_i64(1)),
        Expr::Rat(r) => Ok(Expr::Int(r.den.clone())),
        _ => Ok(unevaluated_call(i, "Denominator", args)),
    }
}

pub fn b_rational(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Rational", args));
    }
    if let (Some(a), Some(b)) = (expr_to_int(&args[0]), expr_to_int(&args[1])) {
        if b == 0 {
            return Ok(unevaluated_call(i, "Rational", args));
        }
        return Ok(Expr::rat(Rat::new(BigInt::from_i64(a), BigInt::from_i64(b))));
    }
    Ok(unevaluated_call(i, "Rational", args))
}

pub fn b_complex(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Complex", args));
    }
    if let (Some(a), Some(b)) = (num_val(&args[0]), num_val(&args[1])) {
        return Ok(Expr::cnum(a, b));
    }
    Ok(unevaluated_call(i, "Complex", args))
}

pub fn b_factorial(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Factorial", args));
    }
    match &args[0] {
        Expr::Int(n) => {
            let k = match n.to_i64() {
                Some(v) if v >= 0 => v,
                _ => return Ok(unevaluated_call(i, "Factorial", args)),
            };
            let mut acc = BigInt::from_i64(1);
            let mut k2 = k;
            while k2 > 1 {
                acc = acc.mul(&BigInt::from_i64(k2));
                k2 -= 1;
            }
            Ok(Expr::Int(acc))
        }
        Expr::Real(x) if *x >= 0.0 => Ok(Expr::real(gamma_f64(x + 1.0))),
        _ => Ok(unevaluated_call(i, "Factorial", args)),
    }
}

// ============================ N ============================

pub fn b_n(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("N", "argr");
        return Ok(unevaluated_call(i, "N", args));
    }
    let mut want_prec: Option<usize> = None;
    if args.len() >= 2 {
        // `N[expr, prec]`: prec above machine precision is evaluated with the
        // arbitrary-precision decimal when the expression supports it; shapes
        // that need a machine transcendental fall back to f64 and report it.
        let p = i.eval(&args[1])?;
        let machine = p
            .sym_id()
            .map_or(false, |id| i.t.name_of(id) == "MachinePrecision");
        match crate::builtins::core::expr_to_int(&p) {
            Some(n) if n >= 1 => {
                if n > 16 {
                    want_prec = Some(n as usize);
                }
            }
            _ if machine => {}
            _ => {
                i.message("N", "prec");
                return Ok(unevaluated_call(i, "N", args));
            }
        }
    }
    // `N` holds its argument, so the raw form is still available. Prefer the
    // evaluated form (`Sin[Pi]` is exactly 0, `Pi^Pi` stays symbolic), and fall
    // back to the raw form when ordinary evaluation already collapsed an exact
    // call to a machine real (`Sin[1]`, `Log[2]`) at the requested precision.
    let raw = args[0].clone();
    let e = i.eval(&raw)?;
    if let Some(prec) = want_prec {
        if let Some(v) = crate::builtins::num::big_real_eval(&e, &i.t, prec) {
            return Ok(v);
        }
        if !crate::expr::same_q(&e, &raw) {
            if let Some(v) = crate::builtins::num::big_real_eval(&raw, &i.t, prec) {
                return Ok(v);
            }
        }
        i.message("N", "meprec");
    }
    Ok(numeric_eval(i, &e))
}

// ============================ lists (basic) ============================

pub fn b_list(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), args.to_vec()))
}

pub fn b_total(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Total", "argr");
        return Ok(unevaluated_call(i, "Total", args));
    }
    let list = i.eval(&args[0])?;
    if !list.is_list(&i.t) {
        i.message("Total", "normal");
        return Ok(unevaluated_call(i, "Total", args));
    }
    let mut acc = Expr::int_i64(0);
    for x in list.node_args() {
        let call = Expr::node(Expr::Sym(i.t.sys.plus), vec![acc, x]);
        acc = i.eval(&call)?;
    }
    Ok(acc)
}

pub fn b_range(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let sys = i.t.sys;
    if args.is_empty() || args.len() > 3 {
        return Ok(unevaluated_call(i, "Range", args));
    }
    let mut vals: Vec<Expr> = Vec::new();
    for a in args {
        vals.push(i.eval(a)?);
    }
    let list = |out: Vec<Expr>| Expr::node(Expr::Sym(sys.list), out);
    const LIMIT: usize = 1_000_000;

    // Exact rationals (integers included): Range[0, 1, 1/4] stays exact.
    let rats: Option<Vec<Rat>> = vals.iter().map(|v| v.as_rat_exact()).collect();
    if let Some(r) = rats {
        let (lo, hi, step) = match r.len() {
            1 => (Rat::from_i64(1), r[0].clone(), Rat::from_i64(1)),
            2 => (r[0].clone(), r[1].clone(), Rat::from_i64(1)),
            _ => (r[0].clone(), r[1].clone(), r[2].clone()),
        };
        if step.is_zero() {
            return Ok(unevaluated_call(i, "Range", args));
        }
        let up = !step.is_neg();
        let mut out = Vec::new();
        let mut x = lo;
        while if up { x.cmp(&hi) != std::cmp::Ordering::Greater } else { x.cmp(&hi) != std::cmp::Ordering::Less } {
            if i.timed_out() {
                return Err(Flow::Abort);
            }
            out.push(Expr::rat(x.clone()));
            x = x.add(&step);
            if out.len() > LIMIT {
                return Ok(unevaluated_call(i, "Range", args));
            }
        }
        return Ok(list(out));
    }

    // Machine numbers.
    let fs: Option<Vec<f64>> = vals
        .iter()
        .map(|v| num_val(v).map(|n| nv_f64(&n)))
        .collect();
    if let Some(f) = fs {
        let (lo, hi, step) = match f.len() {
            1 => (1.0, f[0], 1.0),
            2 => (f[0], f[1], 1.0),
            _ => (f[0], f[1], f[2]),
        };
        if step == 0.0 {
            return Ok(unevaluated_call(i, "Range", args));
        }
        let mut out = Vec::new();
        let mut x = lo;
        let tol = 1e-12 * hi.abs().max(1.0);
        while if step > 0.0 { x <= hi + tol } else { x >= hi - tol } {
            if i.timed_out() {
                return Err(Flow::Abort);
            }
            out.push(Expr::real(x));
            x += step;
            if out.len() > LIMIT {
                return Ok(unevaluated_call(i, "Range", args));
            }
        }
        return Ok(list(out));
    }
    Ok(unevaluated_call(i, "Range", args))
}
/// Chop one numeric component of a complex value: a machine real close to zero
/// becomes the exact zero, anything else is left as it is.
fn chop_nv(n: &NumVal, tol: f64) -> NumVal {
    match n {
        NumVal::F(f) if f.abs() < tol => NumVal::R(Rat::new(BigInt::zero(), BigInt::one())),
        other => other.clone(),
    }
}

/// Replace approximate numbers that are close to zero by the exact integer 0.
/// Threads through lists and into the arguments of any expression, the way
/// Mathematica's Chop does; exact numbers are left alone.
fn chop_expr(e: &Expr, tol: f64) -> Expr {
    match e {
        Expr::Real(x) if x.abs() < tol => Expr::int_i64(0),
        Expr::Cnum(c) => Expr::cnum(chop_nv(&c.re, tol), chop_nv(&c.im, tol)),
        Expr::Node(nd) => {
            let args: Vec<Expr> = nd.args.iter().map(|a| chop_expr(a, tol)).collect();
            if args == nd.args {
                e.clone()
            } else {
                Expr::node(nd.head.clone(), args)
            }
        }
        _ => e.clone(),
    }
}

pub fn b_chop(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() || args.len() > 2 {
        i.message("Chop", "argr");
        return Ok(unevaluated_call(i, "Chop", args));
    }
    let tol = match args.get(1) {
        None => 1e-10,
        Some(t) => match num_val(t) {
            Some(n) => {
                let v = nv_f64(&n);
                if v.is_finite() && v > 0.0 {
                    v
                } else {
                    i.message("Chop", "argr");
                    return Ok(unevaluated_call(i, "Chop", args));
                }
            }
            None => return Ok(unevaluated_call(i, "Chop", args)),
        },
    };
    Ok(chop_expr(&args[0], tol))
}

// ============================ Max, Min, Binomial ============================

/// The larger (or smaller) of the arguments. Numbers only: a symbolic argument
/// leaves the call unevaluated, as `Max[x, 1]` does in Mathematica.
fn extremum(i: &mut Interp, args: &[Expr], want_max: bool, name: &str) -> EvalRes {
    if args.is_empty() {
        // Max[] is -Infinity, Min[] is Infinity.
        let sym = Expr::Sym(i.t.sys.infinity);
        let e = if want_max {
            Expr::node(Expr::Sym(i.t.sys.times), vec![Expr::int_i64(-1), sym])
        } else {
            sym
        };
        return Ok(i.eval(&e)?);
    }
    let mut best: Option<(f64, Expr)> = None;
    for a in args {
        let Some(n) = num_val(a) else {
            return Ok(unevaluated_call(i, name, args));
        };
        let v = nv_f64(&n);
        if !v.is_finite() {
            return Ok(unevaluated_call(i, name, args));
        }
        best = match best {
            None => Some((v, a.clone())),
            Some((bv, be)) => {
                let better = if want_max { v > bv } else { v < bv };
                if better {
                    Some((v, a.clone()))
                } else {
                    Some((bv, be))
                }
            }
        };
    }
    Ok(best.map(|(_, e)| e).unwrap_or(Expr::int_i64(0)))
}

pub fn b_max(i: &mut Interp, args: &[Expr]) -> EvalRes {
    extremum(i, args, true, "Max")
}

pub fn b_min(i: &mut Interp, args: &[Expr]) -> EvalRes {
    extremum(i, args, false, "Min")
}

pub fn b_binomial(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Binomial", "argr");
        return Ok(unevaluated_call(i, "Binomial", args));
    }
    let (Some(n), Some(k)) = (expr_to_int(&args[0]), expr_to_int(&args[1])) else {
        return Ok(unevaluated_call(i, "Binomial", args));
    };
    if n < 0 || k < 0 {
        return Ok(unevaluated_call(i, "Binomial", args));
    }
    // Beyond the end of the row the coefficient is 0, as in Mathematica.
    if k > n {
        return Ok(Expr::int_i64(0));
    }
    // C(n, k) = Π (n-k+i)/i, exact at every step.
    let mut c = BigInt::one();
    for j in 1..=k {
        let num = BigInt::from_i64(n - k + j);
        let den = BigInt::from_i64(j);
        c = c.mul(&num).div_rem(&den).0;
    }
    Ok(Expr::Int(c))
}

/// `IntegerDigits[n]` / `IntegerDigits[n, b]`: the digits of a non-negative
/// integer, most significant first, in base 10 by default.
pub fn b_integerdigits(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() || args.len() > 2 {
        i.message("IntegerDigits", "argr");
        return Ok(unevaluated_call(i, "IntegerDigits", args));
    }
    // Arbitrary-size integers, like GCD/LCM: a machine-word conversion would give
    // up on `IntegerDigits[10^20]`.
    let n = match &args[0] {
        Expr::Int(n) => n.clone(),
        Expr::Rat(r) if r.is_int() => r.num.clone(),
        _ => return Ok(unevaluated_call(i, "IntegerDigits", args)),
    };
    let base = match args.get(1) {
        None => 10i64,
        Some(b) => match expr_to_int(b) {
            Some(b) if (2..=36).contains(&b) => b,
            _ => return Ok(unevaluated_call(i, "IntegerDigits", args)),
        },
    };
    if n.sign < 0 {
        return Ok(unevaluated_call(i, "IntegerDigits", args));
    }
    let mut v = n;
    let base_b = BigInt::from_i64(base);
    let mut digits: Vec<Expr> = Vec::new();
    loop {
        let (q, r) = v.div_rem(&base_b);
        digits.push(Expr::Int(r));
        if q.is_zero() {
            break;
        }
        v = q;
    }
    digits.reverse();
    Ok(Expr::node(Expr::Sym(i.t.sys.list), digits))
}

/// `FactorInteger[n]`: the prime factorisation as `{{p, e}, ...}`, ascending.
///
/// Trial division up to 10^5, then the cofactor is accepted only when it is 1 or a
/// prime of a size the built-in test can decide (`is_prime_big`). Anything else
/// leaves the call unevaluated rather than reporting a composite as prime.
pub fn b_factorinteger(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("FactorInteger", "argr");
        return Ok(unevaluated_call(i, "FactorInteger", args));
    }
    let n = match &args[0] {
        Expr::Int(n) => n.clone(),
        Expr::Rat(r) if r.is_int() => r.num.clone(),
        _ => return Ok(unevaluated_call(i, "FactorInteger", args)),
    };
    let negative = n.sign < 0;
    let mut rem = n.abs();
    if rem.is_zero() {
        return Ok(unevaluated_call(i, "FactorInteger", args));
    }
    if rem.cmp(&BigInt::one()) == std::cmp::Ordering::Equal {
        // Mathematica: FactorInteger[1] is {{1, 1}}.
        let pair = Expr::node(
            Expr::Sym(i.t.sys.list),
            vec![Expr::int_i64(1), Expr::int_i64(1)],
        );
        return Ok(Expr::node(Expr::Sym(i.t.sys.list), vec![pair]));
    }
    let limit = 100_000i64;
    let mut factors: Vec<(BigInt, i64)> = Vec::new();
    let mut p = 2i64;
    while p <= limit {
        let pb = BigInt::from_i64(p);
        let mut count = 0i64;
        loop {
            let (q, r) = rem.div_rem(&pb);
            if !r.is_zero() {
                break;
            }
            rem = q;
            count += 1;
        }
        if count > 0 {
            factors.push((pb, count));
        }
        if rem.cmp(&BigInt::one()) == std::cmp::Ordering::Equal {
            break;
        }
        p = if p == 2 { 3 } else { p + 2 };
    }
    if rem.cmp(&BigInt::one()) != std::cmp::Ordering::Equal {
        match crate::builtins::lists::is_prime_big(&rem) {
            Some(true) => factors.push((rem, 1)),
            _ => {
                i.message("FactorInteger", "bigint");
                return Ok(unevaluated_call(i, "FactorInteger", args));
            }
        }
    }
    let mut items: Vec<Expr> = Vec::new();
    if negative {
        items.push(Expr::node(
            Expr::Sym(i.t.sys.list),
            vec![Expr::int_i64(-1), Expr::int_i64(1)],
        ));
    }
    for (f, e) in factors {
        items.push(Expr::node(
            Expr::Sym(i.t.sys.list),
            vec![Expr::Int(f), Expr::int_i64(e)],
        ));
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), items))
}
