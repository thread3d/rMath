//! Special functions: SinIntegral, CosIntegral, ExpIntegralEi, LogIntegral,
//! Erf, Erfc — with exact special values and machine-precision evaluation, so
//! Integrate can return closed forms such as ∫ sin(x)/x dx = SinIntegral[x].

use crate::builtins::core::unevaluated_call;
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::Expr;

const SQRT_PI: f64 = 1.772_453_850_905_516;

fn num1(
    i: &mut Interp,
    name: &str,
    args: &[Expr],
    f: impl Fn(f64) -> f64,
) -> EvalRes {
    if args.len() != 1 {
        i.message(name, "argr");
        return Ok(unevaluated_call(i, name, args));
    }
    // Exact special values first.
    if let Some(v) = special_value(i, name, &args[0]) {
        return Ok(v);
    }
    let arg = i.eval(&args[0])?;
    // Erf[1] stays exact (as in Mathematica); Erf[1.] evaluates numerically.
    match &arg {
        Expr::Real(x) => Ok(Expr::real(f(*x))),
        _ => Ok(unevaluated_call(i, name, &[arg.clone()])),
    }
}



/// A few exact values: Erf[0], Erf[±Infinity], SinIntegral[0], and the
/// asymptotes at Infinity.
fn special_value(i: &Interp, name: &str, arg: &Expr) -> Option<Expr> {
    let sys = i.t.sys;
    let is = |e: &Expr, id: usize| e.is_symbol(id);
    let zero = matches!(arg, Expr::Int(n) if n.to_i64() == Some(0));
    let pos_inf = is(arg, sys.infinity);
    let neg_inf = {
        // -Infinity is Times[-1, Infinity]
        if let Some((h, a)) = arg.parts() {
            h.is_symbol(sys.times)
                && a.len() == 2
                && matches!(&a[0], Expr::Int(n) if n.to_i64() == Some(-1))
                && a[1].is_symbol(sys.infinity)
        } else {
            false
        }
    };
    match name {
        "Erf" => {
            if zero {
                return Some(Expr::int_i64(0));
            }
            if pos_inf {
                return Some(Expr::int_i64(1));
            }
            if neg_inf {
                return Some(Expr::int_i64(-1));
            }
        }
        "Erfc" => {
            if zero {
                return Some(Expr::int_i64(1));
            }
            if pos_inf {
                return Some(Expr::int_i64(0));
            }
            if neg_inf {
                return Some(Expr::int_i64(2));
            }
        }
        "SinIntegral" | "ExpIntegralEi" => {
            if zero {
                return Some(Expr::int_i64(0));
            }
        }
        "CosIntegral" => {
            if pos_inf {
                return Some(Expr::int_i64(0));
            }
        }
        "LogIntegral" => {
            if same_q_num(arg, 1.0) {
                return Some(Expr::Sym(sys.infinity));
            }
        }
        _ => {}
    }
    // SinIntegral[Infinity] = Pi/2
    if name == "SinIntegral" && pos_inf {
        if let Some(pi) = i.t.get("Pi") {
            let half = Expr::rat(crate::rational::Rat::new(
                crate::bigint::BigInt::from_i64(1),
                crate::bigint::BigInt::from_i64(2),
            ));
            return Some(Expr::node(Expr::Sym(sys.times), vec![half, Expr::Sym(pi)]));
        }
    }
    None
}

fn same_q_num(e: &Expr, v: f64) -> bool {
    match e {
        Expr::Int(n) => n.to_f64() == v,
        Expr::Real(x) => *x == v,
        _ => false,
    }
}

pub fn b_sinintegral(i: &mut Interp, args: &[Expr]) -> EvalRes {
    // ∫ ... Si(0) = 0 handled in special_value; otherwise numeric.
    num1(i, "SinIntegral", args, si_f64)
}

pub fn b_cosintegral(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, "CosIntegral", args, ci_f64)
}

pub fn b_expintegralei(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, "ExpIntegralEi", args, ei_f64)
}

pub fn b_logintegral(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, "LogIntegral", args, |x| {
        if x <= 1.0 {
            f64::NAN
        } else {
            ei_f64(x.ln())
        }
    })
}

pub fn b_erf(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, "Erf", args, erf_f64)
}

pub fn b_erfc(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, "Erfc", args, |x| 1.0 - erf_f64(x))
}

// ---------------------------------------------------------------- numerics

/// Sine integral Si(x) = ∫₀ˣ sin t / t dt.
pub fn si_f64(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }
    if x.abs() < 1e-8 {
        return x;
    }
    if x.abs() > 30.0 {
        // Asymptotic: Si(x) ~ π/2 - cos(x)/x (1 - 2!/x² + ...) - sin(x)/x²(1 - 3!/x² + ...)
        let (s, c) = x.sin_cos();
        let x2 = x * x;
        let f = 1.0 - 2.0 / x2 + 24.0 / (x2 * x2);
        let g = 1.0 / x - 6.0 / (x2 * x);
        let sign = if x < 0.0 { -1.0 } else { 1.0 };
        return sign * (std::f64::consts::FRAC_PI_2 - c * f / x - s * g / x2 * x2 / x);
    }
    // Taylor series: Σ (-1)^n x^(2n+1) / ((2n+1)(2n+1)!)
    let mut term = x; // x^(2n+1)/(2n+1)!
    let mut sum = x;
    let mut n = 1.0;
    loop {
        term *= -x * x / ((2.0 * n) * (2.0 * n + 1.0));
        let add = term / (2.0 * n + 1.0);
        sum += add;
        if add.abs() < 1e-18 * sum.abs().max(1e-300) || n > 200.0 {
            break;
        }
        n += 1.0;
    }
    sum
}

/// Cosine integral Ci(x) = γ + ln|x| + ∫₀ˣ (cos t - 1)/t dt.
pub fn ci_f64(x: f64) -> f64 {
    const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.abs() > 30.0 {
        let (s, c) = x.sin_cos();
        let x2 = x * x;
        let f = 1.0 - 2.0 / x2 + 24.0 / (x2 * x2);
        let g = 1.0 / x - 6.0 / (x2 * x);
        return s * f / x - c * g / x2 * x2 / x;
    }
    // Σ (-1)^n x^(2n) / ((2n)(2n)!)
    let mut term = 1.0; // x^(2n)/(2n)!
    let mut sum = 0.0;
    let mut n = 1.0;
    loop {
        term *= -x * x / ((2.0 * n - 1.0) * (2.0 * n));
        let add = term / (2.0 * n);
        sum += add;
        if add.abs() < 1e-18 * sum.abs().max(1e-300) || n > 200.0 {
            break;
        }
        n += 1.0;
    }
    EULER_GAMMA + x.abs().ln() + sum
}

/// Exponential integral Ei(x) = γ + ln|x| + Σ xⁿ/(n·n!).
pub fn ei_f64(x: f64) -> f64 {
    const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x < 0.0 {
        // Ei(-a) = -E1(a); E1 via continued fraction / series for moderate a.
        return -e1_f64(-x);
    }
    // Series (converges well up to ~40).
    let mut term = 1.0; // x^n / n!
    let mut sum = 0.0;
    let mut n = 1.0;
    loop {
        term *= x / n;
        let add = term / n;
        sum += add;
        if add.abs() < 1e-18 * sum.abs().max(1e-300) || n > 400.0 {
            break;
        }
        n += 1.0;
    }
    EULER_GAMMA + x.ln() + sum
}

/// ∫_{-∞}^{∞} g(u) du by the trapezoidal rule: double-exponentially decaying
/// integrands converge to near machine precision with a modest step.
fn trapz_double_exp(g: impl Fn(f64) -> f64, h: f64, lo: f64, hi: f64) -> f64 {
    let n = ((hi - lo) / h).round() as i64;
    let mut sum = 0.0;
    for k in -n..=n {
        let u = k as f64 * h;
        let w = if k == -n || k == n { 0.5 } else { 1.0 };
        sum += w * g(u);
    }
    sum * h
}

/// E1(x) = ∫ₓ^∞ e^{-t}/t dt for x > 0, via the substitution t = x + e^u.
fn e1_f64(x: f64) -> f64 {
    const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;
    if x <= 0.0 {
        return f64::NAN;
    }
    if x < 1.0 {
        // E1(x) = -γ - ln x - Σ (-1)^n xⁿ/(n·n!)  (converges quickly for small x)
        let mut term = 1.0;
        let mut sum = 0.0;
        let mut n = 1.0;
        loop {
            term *= -x / n;
            let add = term / n;
            sum += add;
            if add.abs() < 1e-18 * sum.abs().max(1e-300) || n > 400.0 {
                break;
            }
            n += 1.0;
        }
        return -EULER_GAMMA - x.ln() - sum;
    }
    (-x).exp()
        * trapz_double_exp(
            |u| {
                let e = u.exp();
                e * (-e).exp() / (x + e)
            },
            0.25,
            -40.0,
            12.0,
        )
}

/// Error function: Taylor for |x| ≤ 3, otherwise erfc via continued fraction.
pub fn erf_f64(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }
    if x.abs() <= 3.0 {
        // (2/√π) Σ (-1)^n x^(2n+1) / (n! (2n+1))
        let mut term = x;
        let mut sum = x;
        let mut n = 1.0;
        loop {
            term *= -x * x / n;
            let add = term / (2.0 * n + 1.0);
            sum += add;
            if add.abs() < 1e-18 * sum.abs().max(1e-300) || n > 200.0 {
                break;
            }
            n += 1.0;
        }
        return 2.0 / SQRT_PI * sum;
    }
    let e = erfc_f64(x.abs());
    if x > 0.0 {
        1.0 - e
    } else {
        e - 1.0
    }
}

/// Complementary error function: 1 - erf(x) near 0, quadrature beyond.
pub fn erfc_f64(x: f64) -> f64 {
    if x < 0.0 {
        return 2.0 - erfc_f64(-x);
    }
    if x < 3.0 {
        return 1.0 - erf_f64(x);
    }
    // erfc(x) = (2/√π) ∫ₓ^∞ e^{-t²} dt, with t = x + e^u.
    2.0 / SQRT_PI
        * trapz_double_exp(
            |u| {
                let e = u.exp();
                let t = x + e;
                (-t * t).exp() * e
            },
            0.25,
            -40.0,
            14.0,
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn erf_values() {
        assert!(close(erf_f64(0.0), 0.0, 1e-12));
        assert!(close(erf_f64(1.0), 0.842_700_792_949_714_9, 1e-12));
        assert!(close(erf_f64(2.0), 0.995_322_265_018_952_7, 1e-12));
        assert!(close(erf_f64(5.0), 0.999_999_999_998_413_2, 1e-10));
        assert!(close(erf_f64(-1.0), -0.842_700_792_949_714_9, 1e-12));
        assert!(close(erfc_f64(1.0), 0.157_299_207_050_285_1, 1e-12));
        assert!(close(erfc_f64(3.0), 2.209_049_699_858_544_1e-5, 1e-9));
        assert!(close(erfc_f64(5.0), 1.537_459_794_428_035_1e-12, 1e-8));
    }

    #[test]
    fn si_ci_values() {
        assert!(close(si_f64(1.0), 0.946_083_070_367_183_0, 1e-12));
        assert!(close(si_f64(2.0), 1.605_412_976_802_694_8, 1e-12));
        assert!(close(si_f64(10.0), 1.658_347_594_218_874_0, 1e-9));
        assert!(close(ci_f64(1.0), 0.337_403_922_900_968_1, 1e-12));
        assert!(close(ci_f64(5.0), -0.190_029_749_656_643_9, 1e-9));
    }

    #[test]
    fn ei_li_values() {
        assert!(close(ei_f64(1.0), 1.895_117_816_355_936_8, 1e-12));
        assert!(close(ei_f64(-1.0), -0.219_383_934_395_520_3, 1e-9));
        assert!(close(ei_f64(10.0), 2492.228_976_241_877_8, 1e-9));
        // li(2) = Ei(ln 2) = 1.0451637801174930
        assert!(close(ei_f64(2f64.ln()), 1.045_163_780_117_493_0, 1e-12));
    }
}
