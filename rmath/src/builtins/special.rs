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
        "SinhIntegral" => {
            if zero {
                return Some(Expr::int_i64(0));
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

/// Hyperbolic sine integral Shi(x) = ∫₀ˣ sinh(t)/t dt = (Ei(x) - Ei(-x))/2.
pub fn shi_f64(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }
    let a = x.abs();
    let v = (ei_f64(a) - ei_f64(-a)) / 2.0;
    if x < 0.0 {
        -v
    } else {
        v
    }
}

/// Hyperbolic cosine integral Chi(x) = γ + ln|x| + ∫₀ˣ (cosh t - 1)/t dt
/// = (Ei(x) + Ei(-x))/2 (even).
pub fn chi_f64(x: f64) -> f64 {
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    let a = x.abs();
    (ei_f64(a) + ei_f64(-a)) / 2.0
}

pub fn b_sinhintegral(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, "SinhIntegral", args, shi_f64)
}

pub fn b_coshintegral(i: &mut Interp, args: &[Expr]) -> EvalRes {
    num1(i, "CoshIntegral", args, chi_f64)
}

/// Sinc[x] = Sin[x]/x, with Sinc[0] = 1 (Mathematica's normalisation).
pub fn b_sinc(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Sinc", "argr");
        return Ok(unevaluated_call(i, "Sinc", args));
    }
    let x = i.eval(&args[0])?;
    if matches!(&x, Expr::Int(n) if n.to_i64() == Some(0)) {
        return Ok(Expr::int_i64(1));
    }
    if let Expr::Real(v) = &x {
        return Ok(Expr::real(if *v == 0.0 { 1.0 } else { v.sin() / v }));
    }
    Ok(unevaluated_call(i, "Sinc", &[x]))
}

/// Numeric value used by N[] (Sin[x]/x with the removable singularity at 0).
pub fn sinc_f64(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        x.sin() / x
    }
}

// ---------------------------------------------------------------- numerics

/// 16-point Gauss–Legendre on [a, b].
fn gauss16(f: &dyn Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    const X: [f64; 8] = [
        0.095_012_509_837_637_4,
        0.281_603_550_779_258_9,
        0.458_016_777_657_227_4,
        0.617_876_244_402_643_8,
        0.755_404_408_355_003_0,
        0.865_631_202_387_831_8,
        0.944_575_023_073_232_6,
        0.989_400_934_991_649_9,
    ];
    const W: [f64; 8] = [
        0.189_450_610_455_068_5,
        0.182_603_415_044_923_6,
        0.169_156_519_395_002_5,
        0.149_595_988_816_576_7,
        0.124_628_971_255_533_9,
        0.095_158_511_682_492_8,
        0.062_253_523_938_647_9,
        0.027_152_459_411_754_1,
    ];
    let c = 0.5 * (a + b);
    let h = 0.5 * (b - a);
    let mut sum = 0.0;
    for i in 0..8 {
        sum += W[i] * (f(c + h * X[i]) + f(c - h * X[i]));
    }
    sum * h
}

/// ∫₀ˣ f(t) dt by composite Gauss–Legendre (half-period subintervals, so
/// oscillatory integrands stay accurate). The Taylor series for Si/Ci loses
/// precision to cancellation well before x = 30, and the asymptotic series is
/// only good for large x, so this covers the middle.
fn integrate_0_to(x: f64, f: &dyn Fn(f64) -> f64) -> f64 {
    let step = std::f64::consts::FRAC_PI_2;
    let mut a = 0.0;
    let mut total = 0.0;
    while a < x {
        let b = (a + step).min(x);
        total += gauss16(f, a, b);
        a = b;
    }
    total
}

/// Sine integral Si(x) = ∫₀ˣ sin t / t dt (odd: Si(-x) = -Si(x)).
pub fn si_f64(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }
    let a = x.abs();
    let v = si_positive(a);
    if x < 0.0 {
        -v
    } else {
        v
    }
}

fn si_positive(a: f64) -> f64 {
    if a < 1e-8 {
        return a;
    }
    if a <= 4.0 {
        // Taylor series: Σ (-1)^n a^(2n+1) / ((2n+1)(2n+1)!)
        let mut term = a;
        let mut sum = a;
        let mut n = 1.0;
        loop {
            term *= -a * a / ((2.0 * n) * (2.0 * n + 1.0));
            let add = term / (2.0 * n + 1.0);
            sum += add;
            if add.abs() < 1e-18 * sum.abs().max(1e-300) || n > 200.0 {
                break;
            }
            n += 1.0;
        }
        return sum;
    }
    if a < 100.0 {
        return integrate_0_to(a, &|t: f64| t.sin() / t);
    }
    // Asymptotic (A&S 5.2): Si(a) ~ π/2 - cos(a)/a·f - sin(a)/a²·g
    let (s, c) = a.sin_cos();
    let a2 = a * a;
    let a4 = a2 * a2;
    let f = 1.0 - 2.0 / a2 + 24.0 / a4 - 720.0 / (a4 * a2) + 40320.0 / (a4 * a4);
    let g = 1.0 - 6.0 / a2 + 120.0 / a4 - 5040.0 / (a4 * a2);
    std::f64::consts::FRAC_PI_2 - c * f / a - s * g / a2
}

/// Cosine integral Ci(x) = γ + ln|x| + ∫₀ˣ (cos t - 1)/t dt (even).
pub fn ci_f64(x: f64) -> f64 {
    const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    let a = x.abs();
    if a <= 4.0 {
        // Σ (-1)^n a^(2n) / ((2n)(2n)!)
        let mut term = 1.0;
        let mut sum = 0.0;
        let mut n = 1.0;
        loop {
            term *= -a * a / ((2.0 * n - 1.0) * (2.0 * n));
            let add = term / (2.0 * n);
            sum += add;
            if add.abs() < 1e-18 * sum.abs().max(1e-300) || n > 200.0 {
                break;
            }
            n += 1.0;
        }
        return EULER_GAMMA + a.ln() + sum;
    }
    if a < 100.0 {
        return EULER_GAMMA
            + a.ln()
            + integrate_0_to(a, &|t: f64| (t.cos() - 1.0) / t);
    }
    let (s, c) = a.sin_cos();
    let a2 = a * a;
    let a4 = a2 * a2;
    let f = 1.0 - 2.0 / a2 + 24.0 / a4 - 720.0 / (a4 * a2) + 40320.0 / (a4 * a4);
    let g = 1.0 - 6.0 / a2 + 120.0 / a4 - 5040.0 / (a4 * a2);
    s * f / a - c * g / a2
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

    /// Compare against 17-digit references generated with mpmath (mp.si, mp.ci,
    /// mp.erf, mp.erfc, mp.ei, mp.li). The Taylor series alone lost precision to
    /// cancellation by x ~ 15 and the negative-x asymptotic branch was wrong;
    /// this table pins the accuracy down across the whole range.
    fn check(actual: f64, ref_text: &str) {
        let reference: f64 = ref_text.parse().unwrap();
        // Absolute for O(1) quantities (Si and Ci cross zero), relative for
        // the exponentially-growing ones.
        let scale = reference.abs().max(1.0);
        let err = (actual - reference).abs() / scale;
        assert!(
            err < 1e-12,
            "got {:.17}, want {} (scaled error {:.2e})",
            actual,
            ref_text,
            err
        );
    }

    #[test]
    fn reference_table() {
    // si
    check(si_f64(1.0f64), "0.94608307036718301");
    check(si_f64(2.0f64), "1.6054129768026948");
    check(si_f64(5.0f64), "1.5499312449446741");
    check(si_f64(10.0f64), "1.658347594218874");
    check(si_f64(20.0f64), "1.5482417010434398");
    check(si_f64(30.0f64), "1.5667565400303511");
    check(si_f64(40.0f64), "1.5869851193547845");
    check(si_f64(-7.5f64), "-1.5106815309433859");
    check(si_f64(-40.0f64), "-1.5869851193547845");
    check(si_f64(150.0f64), "1.5661668327225208");
    check(si_f64(1000.0f64), "1.5702331219687712");
    // ci
    check(ci_f64(1.0f64), "0.33740392290096813");
    check(ci_f64(5.0f64), "-0.19002974965664388");
    check(ci_f64(10.0f64), "-0.045456433004455373");
    check(ci_f64(20.0f64), "0.044419820845353317");
    check(ci_f64(50.0f64), "-0.0056283863241163054");
    check(ci_f64(99.0f64), "-0.01009500171205846");
    check(ci_f64(-40.0f64), "0.019020007896208767");
    check(ci_f64(150.0f64), "-0.0047964889929105475");
    // erf
    check(erf_f64(0.5f64), "0.52049987781304654");
    check(erf_f64(1.0f64), "0.84270079294971487");
    check(erf_f64(2.0f64), "0.99532226501895273");
    check(erf_f64(3.0f64), "0.99997790950300141");
    check(erf_f64(5.0f64), "0.99999999999846254");
    check(erf_f64(-2.5f64), "-0.99959304798255504");
    // erfc
    check(erfc_f64(0.5f64), "0.47950012218695346");
    check(erfc_f64(1.0f64), "0.15729920705028513");
    check(erfc_f64(2.0f64), "0.0046777349810472658");
    check(erfc_f64(3.0f64), "2.2090496998585441e-5");
    check(erfc_f64(5.0f64), "1.5374597944280349e-12");
    check(erfc_f64(10.0f64), "2.0884875837625448e-45");
    // ei
    check(ei_f64(0.5f64), "0.45421990486317358");
    check(ei_f64(1.0f64), "1.8951178163559368");
    check(ei_f64(5.0f64), "40.185275355803177");
    check(ei_f64(10.0f64), "2492.2289762418778");
    check(ei_f64(50.0f64), "1.0585636897131691e+20");
    check(ei_f64(-1.0f64), "-0.21938393439552027");
    check(ei_f64(-10.0f64), "-4.1569689296853243e-6");
    // li
    check(ei_f64(1.5f64.ln()), "0.12506498631529636");
    check(ei_f64(2.0f64.ln()), "1.0451637801174928");
    check(ei_f64(10.0f64.ln()), "6.1655995047872979");
    check(ei_f64(100.0f64.ln()), "30.12614158407963");
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
    fn hyperbolic_integrals() {
        // references from mpmath (mp.shi / mp.chi)
        check(shi_f64(1.0), "1.0572508753757285");
        check(chi_f64(1.0), "0.83786694098020824");
        check(shi_f64(2.0), "2.5015674333549756");
        check(chi_f64(2.0), "2.4526669226469145");
        check(chi_f64(-2.0), "2.4526669226469145");
        check(shi_f64(-1.0), "-1.0572508753757285");
        check(shi_f64(10.0), "1246.1144901994233");
        check(chi_f64(10.0), "1246.1144860424544");
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
