//! Numeric core: NumVal helpers, exact powers, term coefficient extraction,
//! machine numeric evaluation (N), sign helpers.

use crate::bigint::BigInt;
use crate::expr::{CNum, Expr, NumVal, SymbolTable, A_CONSTANT};
use crate::rational::Rat;

pub fn nv_to_expr(n: &NumVal) -> Expr {
    match n {
        NumVal::R(r) => Expr::rat(r.clone()),
        NumVal::F(f) => Expr::real(*f),
    }
}

pub fn num_val(e: &Expr) -> Option<NumVal> {
    match e {
        Expr::Int(n) => Some(NumVal::R(Rat { num: n.clone(), den: BigInt::one() })),
        Expr::Rat(r) => Some(NumVal::R(r.clone())),
        Expr::Real(x) => Some(NumVal::F(*x)),
        _ => None,
    }
}

pub fn nv_f64(n: &NumVal) -> f64 {
    match n {
        NumVal::R(r) => r.to_f64(),
        NumVal::F(v) => *v,
    }
}

pub fn nv_add(a: &NumVal, b: &NumVal) -> NumVal {
    match (a, b) {
        (NumVal::R(x), NumVal::R(y)) => NumVal::R(x.add(y)),
        _ => NumVal::F(nv_f64(a) + nv_f64(b)),
    }
}

pub fn nv_mul(a: &NumVal, b: &NumVal) -> NumVal {
    match (a, b) {
        (NumVal::R(x), NumVal::R(y)) => NumVal::R(x.mul(y)),
        _ => NumVal::F(nv_f64(a) * nv_f64(b)),
    }
}

pub fn nv_sub(a: &NumVal, b: &NumVal) -> NumVal {
    match (a, b) {
        (NumVal::R(x), NumVal::R(y)) => NumVal::R(x.sub(y)),
        _ => NumVal::F(nv_f64(a) - nv_f64(b)),
    }
}

pub fn nv_neg(a: &NumVal) -> NumVal {
    match a {
        NumVal::R(r) => NumVal::R(r.clone().neg()),
        NumVal::F(v) => NumVal::F(-*v),
    }
}

pub fn nv_zero() -> NumVal {
    NumVal::R(Rat::from_i64(0))
}

pub fn nv_one() -> NumVal {
    NumVal::R(Rat::from_i64(1))
}

pub fn nv_is_zero(n: &NumVal) -> bool {
    match n {
        NumVal::R(r) => r.is_zero(),
        NumVal::F(v) => *v == 0.0,
    }
}

pub fn is_num(e: &Expr) -> bool {
    num_val(e).is_some()
}

pub fn is_numeric_zero(e: &Expr) -> bool {
    match e {
        Expr::Int(n) => n.is_zero(),
        Expr::Rat(r) => r.is_zero(),
        Expr::Real(x) => *x == 0.0,
        _ => false,
    }
}

pub fn exp_sign(e: &Expr) -> Option<i32> {
    match e {
        Expr::Int(n) => Some(n.sign as i32),
        Expr::Rat(r) => Some(if r.is_zero() { 0 } else if r.is_neg() { -1 } else { 1 }),
        Expr::Real(x) => Some(if *x > 0.0 { 1 } else if *x < 0.0 { -1 } else { 0 }),
        _ => None,
    }
}

pub fn exp_sign_pub(e: &Expr) -> Option<i32> {
    exp_sign(e)
}

/// base^(p/q) exactly when the root is exact (base rational, p/q exact).
pub fn exact_rat_root(base: &Rat, p: i64, q: i64) -> Option<Rat> {
    if q <= 0 || p == 0 {
        return None;
    }
    let neg = p < 0;
    let pu = p.unsigned_abs() as u32;
    let num = base.num.pow(pu);
    let den = base.den.pow(pu);
    let root_num = num.clone().iroot(q as u32);
    let root_den = den.clone().iroot(q as u32);
    if root_num.pow(q as u32).cmp(&num) == std::cmp::Ordering::Equal
        && root_den.pow(q as u32).cmp(&den) == std::cmp::Ordering::Equal
    {
        let r = Rat::new(root_num, root_den);
        return Some(if neg { r.pow_i64(-1) } else { r });
    }
    None
}

pub fn exact_pow(base: &Rat, exp: i64) -> Option<Rat> {
    Some(base.pow_i64(exp))
}

/// Numeric power: (base, exp) -> Some(expr) or None (keep symbolic).
pub fn power_numeric(b: &NumVal, e: &NumVal) -> Option<Expr> {
    match (b, e) {
        (NumVal::R(br), NumVal::R(er)) if er.is_int() => {
            let exp = er.num.to_i64()?;
            let p = exact_pow(br, exp)?;
            Some(nv_to_expr(&NumVal::R(p)))
        }
        (NumVal::R(br), NumVal::R(er)) => {
            let p = er.num.to_i64()?;
            let q = er.den.to_i64()?;
            if q <= 0 {
                return None;
            }
            if br.num.sign < 0 {
                // Negative base: exact only when the root of |b| is exact and
                // the denominator is even, e.g. Sqrt[-4] = 2 I.
                if q % 2 != 0 || p <= 0 {
                    return None;
                }
                let abs = Rat { num: br.num.clone().abs(), den: br.den.clone() };
                let r = exact_rat_root(&abs, p, q)?;
                return Some(if p % 2 == 0 {
                    // I^p is real: +1 or -1
                    if (p / 2) % 2 == 0 {
                        nv_to_expr(&NumVal::R(r))
                    } else {
                        nv_to_expr(&NumVal::R(r.neg()))
                    }
                } else {
                    // I^p is imaginary: ±I
                    let im = if ((p - 1) / 2) % 2 == 0 { r } else { r.neg() };
                    Expr::cnum(NumVal::R(Rat::from_i64(0)), NumVal::R(im))
                });
            }
            let r = exact_rat_root(br, p, q)?;
            Some(nv_to_expr(&NumVal::R(r)))
        }
        _ => machine_power(b, e),
    }
}

pub fn machine_power(b: &NumVal, e: &NumVal) -> Option<Expr> {
    let bf = nv_f64(b);
    let ef = nv_f64(e);
    if bf < 0.0 && ef.fract() != 0.0 {
        let r = (-bf).powf(ef);
        let ang = std::f64::consts::PI * ef;
        return Some(Expr::cnum(
            NumVal::F(r * ang.cos()),
            NumVal::F(r * ang.sin()),
        ));
    }
    let v = bf.powf(ef);
    if v.is_nan() || v.is_infinite() {
        return None;
    }
    Some(Expr::real(v))
}

/// Simplify Sqrt[n] for a positive integer n: n = a^2 * b, giving a Sqrt[b].
/// Bounded so huge radicands are left alone rather than stalling.
pub fn simplify_sqrt_int(n: &crate::bigint::BigInt) -> Option<(crate::bigint::BigInt, crate::bigint::BigInt)> {
    use crate::bigint::BigInt;
    let one = BigInt::from_i64(1);
    if n.cmp(&one) != std::cmp::Ordering::Greater {
        return None;
    }
    if n.to_f64() > 1e14 {
        return None;
    }
    let mut rem = n.clone();
    let mut coef = BigInt::from_i64(1);
    let mut d = BigInt::from_i64(2);
    loop {
        let d2 = d.mul(&d);
        if d2.cmp(&rem) == std::cmp::Ordering::Greater {
            break;
        }
        let (q, r) = rem.div_rem(&d2);
        if r.is_zero() {
            rem = q;
            coef = coef.mul(&d);
        } else {
            d = d.add(&BigInt::from_i64(1));
        }
    }
    if coef.to_i64() == Some(1) {
        None
    } else {
        Some((coef, rem))
    }
}

/// Extra constructor used above (machine complex).
impl Rat {
    pub fn is_one_pub(&self) -> bool {
        self.num.is_one() && self.den.is_one()
    }
}

impl NumVal {
    pub fn is_one_nv(&self) -> bool {
        match self {
            NumVal::R(r) => r.num.is_one() && r.den.is_one(),
            NumVal::F(v) => *v == 1.0,
        }
    }
}

/// Split a Times term into (coefficient-exact, coefficient-machine, key).
pub fn term_coeff(a: &Expr, t: &SymbolTable) -> (Rat, Option<f64>, Expr) {
    let sys = t.sys;
    if let Some((h, hargs)) = a.parts() {
        if h.is_symbol(sys.times) {
            let mut ec = Rat::from_i64(1);
            let mut mc: Option<f64> = None;
            let mut rest: Vec<Expr> = Vec::new();
            for x in hargs {
                match x {
                    Expr::Int(_) | Expr::Rat(_) => {
                        ec = ec.mul(&x.as_rat_exact().unwrap());
                    }
                    Expr::Real(v) => {
                        mc = Some(mc.unwrap_or(1.0) * v);
                    }
                    _ => rest.push(x.clone()),
                }
            }
            if let Some(m) = mc {
                if !ec.is_zero() {
                    mc = Some(m * ec.to_f64());
                    ec = Rat::from_i64(1);
                }
            }
            let key = match rest.len() {
                0 => Expr::int_i64(1),
                1 => rest[0].clone(),
                _ => Expr::node(Expr::Sym(sys.times), rest),
            };
            return (ec, mc, key);
        }
    }
    (Rat::from_i64(1), None, a.clone())
}

/// Build coeff * key.
pub fn coeff_term(key: &Expr, ec: &Rat, mc: Option<f64>, times: usize) -> Expr {
    let coeff: Expr = match mc {
        Some(m) => Expr::real(m * ec.to_f64()),
        None => Expr::rat(ec.clone()),
    };
    let one = Expr::int_i64(1);
    if same_q_pub(&coeff, &one) {
        return key.clone();
    }
    Expr::node(Expr::Sym(times), vec![coeff, key.clone()])
}

fn same_q_pub(a: &Expr, b: &Expr) -> bool {
    crate::expr::same_q(a, b)
}

/// Is the expression numeric (numbers or constant symbols)?
pub fn is_numeric_expr(i: &crate::eval::Interp, e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) | Expr::Cnum(_) => true,
        Expr::Sym(id) => crate::expr::has(i.t.attrs(*id), A_CONSTANT),
        _ => false,
    }
}

/// Full machine-precision evaluation of an expression.
pub fn numeric_eval(i: &mut crate::eval::Interp, e: &Expr) -> Expr {
    match e {
        Expr::Int(n) => Expr::real(n.to_f64()),
        Expr::Rat(r) => Expr::real(r.to_f64()),
        Expr::Real(_) => e.clone(),
        Expr::Cnum(c) => {
            let (re, im) = cnum_f64(c);
            Expr::cnum(NumVal::F(re), NumVal::F(im))
        }
        Expr::Sym(id) => match i.t.name_of(*id) {
            "Pi" => Expr::real(std::f64::consts::PI),
            "E" => Expr::real(std::f64::consts::E),
            "GoldenRatio" => Expr::real((1.0 + 5f64.sqrt()) / 2.0),
            "EulerGamma" => Expr::real(0.5772156649015329),
            "Catalan" => Expr::real(0.915965594177219),
            _ => e.clone(),
        },
        Expr::Str(_) => e.clone(),
        Expr::Node(nd) => {
            if let Some(id) = nd.head.sym_id() {
                let args: Vec<Expr> = nd.args.iter().map(|a| numeric_eval(i, a)).collect();
                // Try the machine-evaluation table for any builtin; fall
                // through to the rebuilt node when nothing applies.
                if let Some(v) = apply_numeric_fn(i, id, &args) {
                    return v;
                }
                if crate::expr::has(i.t.attrs(id), crate::expr::A_HOLDALL) {
                    return Expr::node(nd.head.clone(), nd.args.clone());
                }
                return Expr::node(nd.head.clone(), args);
            }
            e.clone()
        }
    }
}

/// Machine evaluation for known numeric function heads.
fn apply_numeric_fn(i: &crate::eval::Interp, id: usize, args: &[Expr]) -> Option<Expr> {
    let sys = i.t.sys;
    let name = i.t.name_of(id);
    let f1 = |x: f64| -> Option<f64> {
        Some(match name {
            "Sin" => x.sin(),
            "Cos" => x.cos(),
            "Tan" => x.tan(),
            "Cot" => 1.0 / x.tan(),
            "Sec" => 1.0 / x.cos(),
            "Csc" => 1.0 / x.sin(),
            "ArcSin" => x.asin(),
            "ArcCos" => x.acos(),
            "ArcTan" => x.atan(),
            "Sinh" => x.sinh(),
            "Cosh" => x.cosh(),
            "Tanh" => x.tanh(),
            "Exp" => x.exp(),
            "Log" => x.ln(),
            "Sqrt" => x.sqrt(),
            "Abs" => x.abs(),
            "Erf" => crate::builtins::special::erf_f64(x),
            "Erfc" => crate::builtins::special::erfc_f64(x),
            "SinIntegral" => crate::builtins::special::si_f64(x),
            "CosIntegral" => crate::builtins::special::ci_f64(x),
            "ExpIntegralEi" => crate::builtins::special::ei_f64(x),
            "LogIntegral" if x > 1.0 => crate::builtins::special::ei_f64(x.ln()),
            "Sinc" => crate::builtins::special::sinc_f64(x),
            "Gamma" => gamma_f64(x),
            "SinhIntegral" => crate::builtins::special::shi_f64(x),
            "CoshIntegral" => crate::builtins::special::chi_f64(x),
            "Sign" => {
                if x > 0.0 {
                    1.0
                } else if x < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            }
            "Floor" => x.floor(),
            "Ceiling" => x.ceil(),
            "IntegerPart" => x.trunc(),
            "FractionalPart" => x - x.trunc(),
            "Re" => x,
            "Im" => 0.0,
            _ => return None,
        })
    };
    if let Some(v) = f1(0.0) {
        let _ = v;
    }
    if args.len() == 1 {
        if let Some(n) = num_val(&args[0]) {
            let x = nv_f64(&n);
            match name {
                "Sin" => return Some(Expr::real(x.sin())),
                "Cos" => return Some(Expr::real(x.cos())),
                "Tan" => return Some(Expr::real(x.tan())),
                "Cot" if x != 0.0 => return Some(Expr::real(1.0 / x.tan())),
                "Sec" if x.cos() != 0.0 => return Some(Expr::real(1.0 / x.cos())),
                "Csc" if x != 0.0 => return Some(Expr::real(1.0 / x.sin())),
                "ArcSin" if (-1.0..=1.0).contains(&x) => return Some(Expr::real(x.asin())),
                "ArcCos" if (-1.0..=1.0).contains(&x) => return Some(Expr::real(x.acos())),
                "ArcTan" => return Some(Expr::real(x.atan())),
                "Sinh" => return Some(Expr::real(x.sinh())),
                "Cosh" => return Some(Expr::real(x.cosh())),
                "Tanh" => return Some(Expr::real(x.tanh())),
                "Exp" => return Some(Expr::real(x.exp())),
                "Log" if x > 0.0 => return Some(Expr::real(x.ln())),
                "Sqrt" if x >= 0.0 => return Some(Expr::real(x.sqrt())),
                "Abs" => return Some(Expr::real(x.abs())),
                "Erf" => return Some(Expr::real(crate::builtins::special::erf_f64(x))),
                "Erfc" => return Some(Expr::real(crate::builtins::special::erfc_f64(x))),
                "SinIntegral" => return Some(Expr::real(crate::builtins::special::si_f64(x))),
                "CosIntegral" => return Some(Expr::real(crate::builtins::special::ci_f64(x))),
                "ExpIntegralEi" => return Some(Expr::real(crate::builtins::special::ei_f64(x))),
                "LogIntegral" if x > 1.0 => {
                    return Some(Expr::real(crate::builtins::special::ei_f64(x.ln())))
                }
                "Sinc" => return Some(Expr::real(crate::builtins::special::sinc_f64(x))),
                "Gamma" => return Some(Expr::real(gamma_f64(x))),
                "SinhIntegral" => {
                    return Some(Expr::real(crate::builtins::special::shi_f64(x)))
                }
                "CoshIntegral" => {
                    return Some(Expr::real(crate::builtins::special::chi_f64(x)))
                }
                "Re" => return Some(Expr::real(x)),
                "Im" => return Some(Expr::real(0.0)),
                _ => {}
            }
        }
    }
    // Plus/Times fold numerically.
    if id == sys.plus && !args.is_empty() {
        let mut acc = NumVal::F(0.0);
        for a in args {
            if let Some(n) = num_val(a) {
                acc = nv_add(&acc, &n);
            } else {
                return None;
            }
        }
        return Some(nv_to_expr(&acc));
    }
    if id == sys.times && !args.is_empty() {
        let mut acc = NumVal::F(1.0);
        for a in args {
            if let Some(n) = num_val(a) {
                acc = nv_mul(&acc, &n);
            } else {
                return None;
            }
        }
        return Some(nv_to_expr(&acc));
    }
    if id == sys.power && args.len() == 2 {
        if let (Some(b), Some(e)) = (num_val(&args[0]), num_val(&args[1])) {
            if let Some(v) = machine_power(&b, &e) {
                return Some(v);
            }
        }
        return None;
    }
    None
}

pub fn cnum_f64(c: &CNum) -> (f64, f64) {
    (nv_f64(&c.re), nv_f64(&c.im))
}

/// Lanczos gamma (g=7, n=9) for machine-precision real arguments.
pub fn gamma_f64(x: f64) -> f64 {
    const G: f64 = 7.0;
    // Lanczos coefficients for g = 7, N = 9 (standard values; the first four
    // were correct but the rest had been mistyped, giving ~1e-3 errors).
    const C: [f64; 9] = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma_f64(1.0 - x))
    } else {
        let x1 = x - 1.0;
        let t = x1 + G + 0.5;
        let mut a = C[0];
        for (idx, ci) in C.iter().enumerate() {
            if idx == 0 {
                continue;
            }
            a += ci / (x1 + idx as f64);
        }
        // Lanczos: Γ(x) = sqrt(2π) t^(x1+0.5) e^-t A(x), with x1 = x - 1 and
        // t = x1 + g + 0.5. The exponent is x1 + 0.5, not t - 0.5 (that slipped
        // the constant g into the power and made every non-exact value wrong).
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(x1 + 0.5) * (-t).exp() * a
    }
}
/// Exact rational square root when the radicand is a perfect square.
pub fn exact_sqrt(r: &Rat) -> Option<Rat> {
    let sn = r.num.clone().abs().isqrt();
    let sd = r.den.clone().isqrt();
    if sn.pow(2).cmp(&r.num) == std::cmp::Ordering::Equal
        && sd.pow(2).cmp(&r.den) == std::cmp::Ordering::Equal
    {
        Some(Rat::new(sn, sd))
    } else {
        None
    }
}
