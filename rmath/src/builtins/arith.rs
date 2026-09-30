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

pub fn b_plus(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let sys = i.t.sys;
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
                let mut merged = false;
                for (k, e, m) in terms.iter_mut() {
                    if crate::expr::same_q(k, &key) {
                        *e = e.add(&ec);
                        *m = match (*m, mc) {
                            (None, None) => None,
                            (Some(x), Some(y)) => Some(x + y),
                            (None, Some(y)) => Some(y),
                            (Some(x), None) => Some(x),
                        };
                        merged = true;
                        break;
                    }
                }
                if !merged {
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
                push_power(i, &mut powers, &nd.args[0], &nd.args[1])?;
            }
            other => {
                push_power(i, &mut powers, other, &Expr::int_i64(1))?;
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
    base: &Expr,
    exp: &Expr,
) -> Result<(), Flow> {
    for (b, e) in p.iter_mut() {
        if crate::expr::same_q(b, base) {
            let sum = Expr::node(Expr::Sym(i.t.sys.plus), vec![e.clone(), exp.clone()]);
            *e = i.eval(&sum)?;
            return Ok(());
        }
    }
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

/// Some(k) when `e` is `k Pi` (or `-k Pi`) with integer k.
fn pi_multiple(i: &Interp, pi: usize, e: &Expr) -> Option<i64> {
    if e.sym_id() == Some(pi) {
        return Some(1);
    }
    let (h, args) = e.parts()?;
    if h.is_symbol(i.t.sys.times) && args.len() == 2 {
        let (c, rest) = if args[1].sym_id() == Some(pi) {
            (&args[0], &args[1])
        } else if args[0].sym_id() == Some(pi) {
            (&args[1], &args[0])
        } else {
            return None;
        };
        let _ = rest;
        return crate::builtins::core::expr_to_int(c);
    }
    None
}

// ============================ elementary ============================

fn num1(i: &mut Interp, args: &[Expr], f: fn(f64) -> f64, name: &str) -> EvalRes {
    if args.len() != 1 {
        i.message(name, "argr");
        return Ok(unevaluated_call(i, name, args));
    }
    // Exact special values, including integer multiples of Pi: Sin[Pi] = 0,
    // Cos[Pi] = -1, which definite integrals over [0, Pi] rely on.
    let pi_id = i.t.intern("Pi");
    if let Some(k) = pi_multiple(i, pi_id, &args[0]) {
        if name == "Sin" || name == "Tan" {
            return Ok(Expr::int_i64(0));
        }
        if name == "Cos" {
            return Ok(Expr::int_i64(if k % 2 == 0 { 1 } else { -1 }));
        }
    }
    match name {
        "Sin" | "Cos" | "Tan" | "Cot" | "Csc" | "Sec" => {
            if args[0].to_i64_pub() == Some(0) {
                let v = match name {
                    "Sin" | "Tan" | "Cot" | "Csc" => 0,
                    _ => 1,
                };
                return Ok(Expr::int_i64(v));
            }
        }
        "Exp" => {
            if args[0].to_i64_pub() == Some(0) {
                return Ok(Expr::int_i64(1));
            }
        }
        "Log" => {
            if args[0].to_i64_pub() == Some(1) {
                return Ok(Expr::int_i64(0));
            }
            // Log[E] = 1 (and Log[E^n] = n)
            if args[0].is_symbol(i.t.intern("E")) {
                return Ok(Expr::int_i64(1));
            }
            if let Some((h, a)) = args[0].parts() {
                if h.is_symbol(i.t.sys.power) && a.len() == 2 && a[0].is_symbol(i.t.intern("E")) {
                    return Ok(a[1].clone());
                }
            }
        }
        _ => {}
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

pub fn b_floor(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = &args[0];
    match e {
        Expr::Int(_) => Ok(e.clone()),
        Expr::Rat(r) => {
            let (q, _) = r.num.div_mod_floor(&r.den);
            Ok(Expr::Int(q))
        }
        Expr::Real(x) => Ok(Expr::real(x.floor())),
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
        Expr::Real(x) => Ok(Expr::real(x.ceil())),
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
        Expr::Real(x) => Ok(Expr::real(x.round_ties_even())),
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
        Expr::Real(x) => Ok(Expr::real(x.trunc())),
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
    if let (Expr::Real(a), Expr::Real(b)) = (&args[0], &args[1]) {
        if *b != 0.0 {
            let mut r = a % b;
            if r != 0.0 && ((r < 0.0) != (b < &0.0)) {
                r += *b;
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

pub fn b_gcd(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut g = BigInt::zero();
    for a in args {
        match expr_to_int(a) {
            Some(n) => {
                let v = BigInt::from_i64(n).abs();
                g = g.gcd(&v);
            }
            None => return Ok(unevaluated_call(i, "GCD", args)),
        }
    }
    Ok(Expr::Int(g))
}

pub fn b_lcm(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut l = BigInt::from_i64(1);
    for a in args {
        match expr_to_int(a) {
            Some(n) => {
                let v = BigInt::from_i64(n).abs();
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
    if args.len() >= 2 {
        // `N[expr, prec]`: the kernel stores reals as f64, so a precision above
        // machine precision cannot be honoured. Rather than silently truncating
        // (which used to make `N[Pi, 50]` return 6 digits with no hint), report
        // it and return the machine-precision result.
        let p = i.eval(&args[1])?;
        let machine = p
            .sym_id()
            .map_or(false, |id| i.t.name_of(id) == "MachinePrecision");
        match crate::builtins::core::expr_to_int(&p) {
            Some(n) if n >= 1 => {
                if n > 16 {
                    i.message("N", "meprec");
                }
            }
            _ if machine => {}
            _ => {
                i.message("N", "prec");
                return Ok(unevaluated_call(i, "N", args));
            }
        }
    }
    let e = i.eval(&args[0])?;
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
pub fn b_chop(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Chop", args));
    }
    match &args[0] {
        Expr::Real(x) if x.abs() < 1e-10 => Ok(Expr::int_i64(0)),
        Expr::Int(_) | Expr::Rat(_) => Ok(args[0].clone()),
        _ => Ok(unevaluated_call(i, "Chop", args)),
    }
}
