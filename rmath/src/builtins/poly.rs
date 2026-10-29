//! Polynomial utilities and algebra built-ins:
//! Expand, ExpandAll, Together, Cancel, Collect, Coefficient(s), Exponent,
//! Variables, Factor (content + rational roots), polynomial division/GCD.

use crate::bigint::BigInt;
use crate::builtins::core::unevaluated_call;
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::{has, same_q, Expr};
use crate::rational::Rat;

fn eval_call(i: &mut Interp, head: Expr, args: Vec<Expr>) -> EvalRes {
    i.eval(&Expr::node(head, args))
}

// ============================ Expand ============================

/// Distribute Times/Power over Plus, collecting like terms via evaluation.
pub fn expand_expr(i: &mut Interp, e: &Expr) -> EvalRes {
    let sys = i.t.sys;
    match e {
        Expr::Node(nd) if nd.head.is_symbol(sys.plus) => {
            let mut parts = Vec::new();
            for a in &nd.args {
                parts.push(expand_expr(i, a)?);
            }
            eval_call(i, Expr::Sym(sys.plus), parts)
        }
        Expr::Node(nd) if nd.head.is_symbol(sys.times) => {
            let mut alts: Vec<Vec<Expr>> = Vec::new();
            for f in &nd.args {
                match expand_summands(i, f)? {
                    Some(sum) => alts.push(sum),
                    None => alts.push(vec![f.clone()]),
                }
            }
            // Already fully distributed (no factor is a sum): just evaluate.
            if alts.iter().all(|a| a.len() == 1) {
                return eval_call(i, Expr::Sym(sys.times), nd.args.to_vec());
            }
            let mut acc: Vec<Expr> = vec![Expr::int_i64(1)];
            for alternatives in &alts {
                let mut next = Vec::new();
                for p in &acc {
                    for s in alternatives {
                        let call = Expr::node(Expr::Sym(sys.times), vec![p.clone(), s.clone()]);
                        next.push(expand_expr(i, &call)?);
                    }
                }
                acc = next;
            }
            eval_call(i, Expr::Sym(sys.plus), acc)
        }
        Expr::Node(nd) if nd.head.is_symbol(sys.power) && nd.args.len() == 2 => {
            let exp = crate::builtins::core::expr_to_int(&nd.args[1]);
            if let Some(n) = exp {
                if n >= 0 && n <= 64 {
                    if let Some(sum) = expand_summands(i, &nd.args[0])? {
                        let mut acc = Expr::int_i64(1);
                        for _ in 0..n {
                            let mut prods = Vec::new();
                            for s in &sum {
                                let call = Expr::node(
                                    Expr::Sym(sys.times),
                                    vec![acc.clone(), s.clone()],
                                );
                                prods.push(expand_expr(i, &call)?);
                            }
                            acc = eval_call(i, Expr::Sym(sys.plus), prods)?;
                        }
                        return Ok(acc);
                    }
                }
            }
            Ok(e.clone())
        }
        _ => Ok(e.clone()),
    }
}

/// If e expands to a Plus, return its summands (Some); else None.
fn expand_summands(i: &mut Interp, e: &Expr) -> Result<Option<Vec<Expr>>, Flow> {
    let sys = i.t.sys;
    let expanded = expand_expr(i, e)?;
    if let Some((h, args)) = expanded.parts() {
        if h.is_symbol(sys.plus) {
            return Ok(Some(args.to_vec()));
        }
    }
    Ok(None)
}

pub fn b_expand(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Expand", "argr");
        return Ok(unevaluated_call(i, "Expand", args));
    }
    let e = i.eval(&args[0])?;
    expand_expr(i, &e)
}

pub fn b_expandall(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "ExpandAll", args));
    }
    let e = i.eval(&args[0])?;
    expand_all(i, &e)
}

fn expand_all(i: &mut Interp, e: &Expr) -> EvalRes {
    if e.is_atom() {
        return Ok(e.clone());
    }
    let sys = i.t.sys;
    let (h, args) = e.parts().unwrap();
    let mut na = Vec::new();
    for a in args {
        na.push(expand_all(i, a)?);
    }
    let rebuilt = Expr::node(h.clone(), na);
    if h.is_symbol(sys.plus) || h.is_symbol(sys.times) || h.is_symbol(sys.power) {
        expand_expr(i, &rebuilt)
    } else {
        Ok(rebuilt)
    }
}

// ============================ polynomial coefficients ============================

/// Coefficients of `e` in variable `var` (expanded), lowest to highest.
/// Rational-coefficient polynomials only; None otherwise.
pub fn poly_coeffs(i: &mut Interp, e: &Expr, var: &Expr) -> Result<Option<Vec<Rat>>, Flow> {
    let expanded = expand_expr(i, e)?;
    let sys = i.t.sys;
    let terms: Vec<Expr> = match expanded.parts() {
        Some((h, args)) if h.is_symbol(sys.plus) => args.to_vec(),
        _ => vec![expanded],
    };
    let mut coeffs: Vec<Rat> = Vec::new();
    for t in &terms {
        let Some((c, deg)) = term_degree(i, t, var)? else {
            return Ok(None);
        };
        if deg >= coeffs.len() {
            coeffs.resize(deg + 1, Rat::from_i64(0));
        }
        coeffs[deg] = coeffs[deg].add(&c);
    }
    while coeffs.last().map_or(false, |c| c.is_zero()) {
        coeffs.pop();
    }
    Ok(Some(coeffs))
}

/// (coefficient, degree) of a term in var; None if not a polynomial term.
fn term_degree(i: &mut Interp, t: &Expr, var: &Expr) -> Result<Option<(Rat, usize)>, Flow> {
    let sys = i.t.sys;
    if same_q(t, var) {
        return Ok(Some((Rat::from_i64(1), 1)));
    }
    if !contains_var(i, t, var)? {
        let c = match t {
            Expr::Int(_) | Expr::Rat(_) => t.as_rat_exact(),
            _ => None,
        };
        return Ok(c.map(|r| (r, 0usize)));
    }
    if let Some((h, hargs)) = t.parts() {
        if h.is_symbol(sys.power) && hargs.len() == 2 && same_q(&hargs[0], var) {
            let n = crate::builtins::core::expr_to_int(&hargs[1]).unwrap_or(-1);
            if n >= 0 {
                return Ok(Some((Rat::from_i64(1), n as usize)));
            }
            return Ok(None);
        }
        if h.is_symbol(sys.times) {
            let mut c = Rat::from_i64(1);
            let mut deg = 0usize;
            for f in hargs {
                if !contains_var(i, f, var)? {
                    match f {
                        Expr::Int(_) | Expr::Rat(_) => {
                            c = c.mul(&f.as_rat_exact().unwrap());
                        }
                        _ => return Ok(None),
                    }
                } else if same_q(f, var) {
                    deg += 1;
                } else if let Some((h2, a2)) = f.parts() {
                    if h2.is_symbol(sys.power) && a2.len() == 2 && same_q(&a2[0], var) {
                        match crate::builtins::core::expr_to_int(&a2[1]) {
                            Some(n) if n >= 0 => deg += n as usize,
                            _ => return Ok(None),
                        }
                    } else {
                        return Ok(None);
                    }
                } else {
                    return Ok(None);
                }
            }
            return Ok(Some((c, deg)));
        }
    }
    Ok(None)
}

fn contains_var(i: &mut Interp, e: &Expr, var: &Expr) -> Result<bool, Flow> {
    if same_q(e, var) {
        return Ok(true);
    }
    if let Some((_, args)) = e.parts() {
        for a in args {
            if contains_var(i, a, var)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Free (non-constant) symbols of an expression, canonically sorted.
pub fn vars_of(i: &mut Interp, e: &Expr) -> Result<Vec<Expr>, Flow> {
    let mut vars: Vec<Expr> = Vec::new();
    collect_vars(i, e, &mut vars)?;
    let t = &i.t;
    vars.sort_by(|a, b| crate::expr::canon_cmp(t, a, b));
    Ok(vars)
}

fn collect_vars(i: &mut Interp, e: &Expr, out: &mut Vec<Expr>) -> Result<(), Flow> {
    if e.is_atom() {
        if e.sym_id().is_some() {
            let id = e.sym_id().unwrap();
            let attrs = i.t.attrs(id);
            if !has(attrs, crate::expr::A_CONSTANT) && !out.iter().any(|v| same_q(v, e)) {
                out.push(e.clone());
            }
        }
        return Ok(());
    }
    let (_, args) = e.parts().unwrap();
    for a in args {
        collect_vars(i, a, out)?;
    }
    Ok(())
}

// ------------------------ coefficient builtins ------------------------

/// Decompose `form` into `(base, exponent)` factors when it is a monomial:
/// a symbol, a power with an integer exponent, or a product of those. Returns
/// None for anything else (a function, a sum, a non-integer power).
fn monomial_form(i: &Interp, form: &Expr) -> Option<Vec<(Expr, i64)>> {
    let sys = i.t.sys;
    let mut out: Vec<(Expr, i64)> = Vec::new();
    if form.sym_id().is_some() {
        out.push((form.clone(), 1));
        return Some(out);
    }
    if let Some((h, args)) = form.parts() {
        if h.is_symbol(sys.power) && args.len() == 2 {
            let n = crate::builtins::core::expr_to_int(&args[1])?;
            out.push((args[0].clone(), n));
            return Some(out);
        }
        if h.is_symbol(sys.times) {
            for f in args {
                out.extend(monomial_form(i, f)?);
            }
            return Some(out);
        }
    }
    None
}

/// Split a term into variable `(base, exponent)` factors and the remaining
/// coefficient factors. A factor that is not a symbol or an integer power is a
/// coefficient (this is what keeps `Sin[x]` out of a coefficient in `x`).
fn split_term(i: &Interp, t: &Expr, bases: &mut Vec<(Expr, i64)>, coeffs: &mut Vec<Expr>) {
    let sys = i.t.sys;
    if let Some((h, args)) = t.parts() {
        if h.is_symbol(sys.times) {
            for f in args {
                split_term(i, f, bases, coeffs);
            }
            return;
        }
        if h.is_symbol(sys.power) && args.len() == 2 {
            if let Some(n) = crate::builtins::core::expr_to_int(&args[1]) {
                bases.push((args[0].clone(), n));
                return;
            }
        }
    }
    if t.sym_id().is_some() {
        bases.push((t.clone(), 1));
        return;
    }
    coeffs.push(t.clone());
}

/// Coefficient[expr, form] / Coefficient[expr, form, n].
///
/// Mathematica semantics: `expr` is expanded first, then each term is matched
/// by its exact exponent in every part of `form`; the leftover factors are the
/// coefficient. So `Coefficient[a x + b y + c, x]` is `a`, `Coefficient[x^2, x]`
/// is `0` (x^2 is not part of x), and `Coefficient[(x+y)^4, x y^3]` is `4`.
/// Unlike `Exponent`/`CoefficientList`, the coefficient may be symbolic.
pub fn b_coefficient(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 || args.len() > 3 {
        i.message("Coefficient", "argr");
        return Ok(unevaluated_call(i, "Coefficient", args));
    }
    let e = i.eval(&args[0])?;
    let form = i.eval(&args[1])?;
    let Some(mut want) = monomial_form(i, &form) else {
        return Ok(unevaluated_call(i, "Coefficient", args));
    };
    let scale = if args.len() == 3 {
        match crate::builtins::core::expr_to_int(&i.eval(&args[2])?) {
            Some(n) => n,
            None => return Ok(unevaluated_call(i, "Coefficient", args)),
        }
    } else {
        1
    };
    for (_, exp) in want.iter_mut() {
        *exp *= scale;
    }
    let expanded = expand_expr(i, &e)?;
    let sys = i.t.sys;
    let terms: Vec<Expr> = match expanded.parts() {
        Some((h, a)) if h.is_symbol(sys.plus) => a.to_vec(),
        _ => vec![expanded],
    };
    let mut total: Vec<Expr> = Vec::new();
    for t in &terms {
        let mut bases = Vec::new();
        let mut coeffs = Vec::new();
        split_term(i, t, &mut bases, &mut coeffs);
        let matches = want.iter().all(|(wb, wexp)| {
            bases
                .iter()
                .filter(|(b, _)| same_q(b, wb))
                .map(|(_, e)| *e)
                .sum::<i64>()
                == *wexp
        });
        if !matches {
            continue;
        }
        for (b, e) in &bases {
            let consumed = want.iter().any(|(wb, wexp)| same_q(b, wb) && wexp == e);
            if !consumed {
                coeffs.push(if *e == 1 {
                    b.clone()
                } else {
                    Expr::node(Expr::Sym(sys.power), vec![b.clone(), Expr::int_i64(*e)])
                });
            }
        }
        let prod = if coeffs.is_empty() {
            Expr::int_i64(1)
        } else {
            i.eval(&Expr::node(Expr::Sym(sys.times), coeffs))?
        };
        total.push(prod);
    }
    if total.is_empty() {
        return Ok(Expr::int_i64(0));
    }
    i.eval(&Expr::node(Expr::Sym(sys.plus), total))
}

pub fn b_coefficientlist(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "CoefficientList", args));
    }
    let e = i.eval(&args[0])?;
    let var = i.eval(&args[1])?;
    match poly_coeffs(i, &e, &var)? {
        Some(c) => {
            let out: Vec<Expr> = c.into_iter().map(Expr::rat).collect();
            Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
        }
        None => Ok(unevaluated_call(i, "CoefficientList", args)),
    }
}

pub fn b_exponent(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Exponent", args));
    }
    let e = i.eval(&args[0])?;
    let var = i.eval(&args[1])?;
    match poly_coeffs(i, &e, &var)? {
        Some(c) => Ok(Expr::int_i64(c.len() as i64 - 1)),
        None => Ok(unevaluated_call(i, "Exponent", args)),
    }
}

pub fn b_variables(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Variables", args));
    }
    let e = i.eval(&args[0])?;
    Ok(Expr::node(Expr::Sym(i.t.sys.list), vars_of(i, &e)?))
}

// ============================ Together / Cancel ============================

/// Split an evaluated term into (numerator, denominator) expressions.
fn term_nd(i: &mut Interp, t: &Expr) -> Result<(Expr, Expr), Flow> {
    let sys = i.t.sys;
    match t {
        Expr::Int(_) | Expr::Rat(_) => Ok((t.clone(), Expr::int_i64(1))),
        Expr::Node(nd) if nd.head.is_symbol(sys.power) && nd.args.len() == 2 => {
            let e = crate::builtins::core::expr_to_int(&nd.args[1]).unwrap_or(1);
            if e < 0 {
                let d = eval_call(
                    i,
                    Expr::Sym(sys.power),
                    vec![nd.args[0].clone(), Expr::int_i64(-e)],
                )?;
                Ok((Expr::int_i64(1), d))
            } else {
                Ok((t.clone(), Expr::int_i64(1)))
            }
        }
        Expr::Node(nd) if nd.head.is_symbol(sys.times) => {
            let mut numer: Vec<Expr> = Vec::new();
            let mut denom: Vec<Expr> = Vec::new();
            for f in &nd.args {
                if let Some((h, hargs)) = f.parts() {
                    if h.is_symbol(sys.power)
                        && hargs.len() == 2
                        && crate::builtins::core::expr_to_int(&hargs[1]).map_or(false, |e| e < 0)
                    {
                        let e = -crate::builtins::core::expr_to_int(&hargs[1]).unwrap();
                        let d = eval_call(
                            i,
                            Expr::Sym(sys.power),
                            vec![hargs[0].clone(), Expr::int_i64(e)],
                        )?;
                        denom.push(d);
                        continue;
                    }
                }
                numer.push(f.clone());
            }
            let n = eval_call(i, Expr::Sym(sys.times), numer)?;
            let d = eval_call(i, Expr::Sym(sys.times), denom)?;
            Ok((n, d))
        }
        _ => Ok((t.clone(), Expr::int_i64(1))),
    }
}

/// (numerator, denominator) of e with a common denominator.
pub fn together_parts(i: &mut Interp, e: &Expr) -> Result<(Expr, Expr), Flow> {
    let sys = i.t.sys;
    let terms: Vec<Expr> = match e.parts() {
        Some((h, args)) if h.is_symbol(sys.plus) => args.to_vec(),
        _ => vec![e.clone()],
    };
    let mut numer = Expr::int_i64(0);
    let mut denom = Expr::int_i64(1);
    for t in &terms {
        let (tn, td) = term_nd(i, t)?;
        let n1 = eval_call(i, Expr::Sym(sys.times), vec![numer.clone(), td.clone()])?;
        let n2 = eval_call(i, Expr::Sym(sys.times), vec![tn, denom.clone()])?;
        numer = eval_call(i, Expr::Sym(sys.plus), vec![n1, n2])?;
        denom = eval_call(i, Expr::Sym(sys.times), vec![denom, td])?;
    }
    Ok((numer, denom))
}

pub fn b_together(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Together", args));
    }
    let e = i.eval(&args[0])?;
    let (n, d) = together_parts(i, &e)?;
    if same_q(&d, &Expr::int_i64(1)) {
        return Ok(n);
    }
    eval_call(
        i,
        Expr::Sym(i.t.sys.times),
        vec![n, Expr::node(Expr::Sym(i.t.sys.power), vec![d, Expr::int_i64(-1)])],
    )
}

/// gcd of two rationals: gcd(a/b, c/d) = gcd(a·d, c·b)/(b·d), both positive.
fn rat_gcd(a: &Rat, b: &Rat) -> Rat {
    let a = a.clone().abs();
    let b = b.clone().abs();
    let num = a.num.mul(&b.den).gcd(&b.num.mul(&a.den));
    let den = a.den.mul(&b.den);
    Rat::new(num, den)
}

/// The summands of `e` after expansion (a single term is a one-element list).
fn expanded_terms(i: &mut Interp, e: &Expr) -> Result<Vec<Expr>, Flow> {
    let ex = expand_expr(i, e)?;
    if let Some((h, args)) = ex.parts() {
        if h.is_symbol(i.t.sys.plus) {
            return Ok(args.to_vec());
        }
    }
    Ok(vec![ex])
}

/// The rational coefficient of a monomial term, or None when it is not rational
/// (a symbolic coefficient such as `a` in `a x`).
fn term_rat_coeff(i: &Interp, t: &Expr) -> Option<Rat> {
    let mut bases = Vec::new();
    let mut coeffs = Vec::new();
    split_term(i, t, &mut bases, &mut coeffs);
    let mut out = Rat::from_i64(1);
    for c in coeffs {
        match c.as_rat_exact() {
            Some(r) => out = out.mul(&r),
            None => return None,
        }
    }
    Some(out)
}

/// Exponent of the symbol `v` in one monomial term (0 when it does not appear).
fn term_power(i: &Interp, t: &Expr, v: &Expr) -> i64 {
    let mut bases = Vec::new();
    let mut coeffs = Vec::new();
    split_term(i, t, &mut bases, &mut coeffs);
    bases
        .iter()
        .filter(|(b, _)| same_q(b, v))
        .map(|(_, n)| *n)
        .sum()
}

/// Reduce `num/den` by a common monomial factor and by the numeric content —
/// the part of `Cancel` that works for several variables (`x^2 y / (x y^2)` ->
/// `x/y`), which the univariate GCD below cannot see.
fn cancel_monomial_and_content(
    i: &mut Interp,
    num: &Expr,
    den: &Expr,
) -> Result<Option<(Expr, Expr)>, Flow> {
    let nterms = expanded_terms(i, num)?;
    let dterms = expanded_terms(i, den)?;
    let mut cur_num = Expr::int_i64(1);
    let mut cur_den = Expr::int_i64(1);
    let mut changed = false;
    // Symbols that occur in at least one side; only a symbol present in *both*
    // numerator and denominator can be cancelled.
    let mut syms: Vec<Expr> = Vec::new();
    for t in nterms.iter().chain(dterms.iter()) {
        let mut bases = Vec::new();
        let mut coeffs = Vec::new();
        split_term(i, t, &mut bases, &mut coeffs);
        for (b, _) in bases {
            if b.sym_id().is_some() && !same_q(&b, &Expr::int_i64(1))
                && !syms.iter().any(|s| same_q(s, &b))
            {
                syms.push(b);
            }
        }
    }
    for v in &syms {
        let vn = nterms.iter().map(|t| term_power(i, t, v)).min().unwrap_or(0);
        let vd = dterms.iter().map(|t| term_power(i, t, v)).min().unwrap_or(0);
        let common = vn.min(vd);
        if common > 0 {
            let inv = Expr::node(
                Expr::Sym(i.t.sys.power),
                vec![v.clone(), Expr::int_i64(-common)],
            );
            cur_num = eval_call(i, Expr::Sym(i.t.sys.times), vec![cur_num.clone(), inv.clone()])?;
            cur_den = eval_call(i, Expr::Sym(i.t.sys.times), vec![cur_den, inv])?;
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    // Divide *term by term*: a sum cannot be divided by a monomial in one step
    // without leaving a fraction behind, so `(x^2 y + x y^2)/(x y)` would keep
    // its denominator instead of becoming `x + y`.
    let divide_terms = |i: &mut Interp, terms: &[Expr], factor: &Expr| -> Result<Expr, Flow> {
        let mut out = Vec::with_capacity(terms.len());
        for t in terms {
            out.push(eval_call(
                i,
                Expr::Sym(i.t.sys.times),
                vec![factor.clone(), t.clone()],
            )?);
        }
        eval_call(i, Expr::Sym(i.t.sys.plus), out)
    };
    let new_num = divide_terms(i, &nterms, &cur_num)?;
    let new_den = divide_terms(i, &dterms, &cur_den)?;
    // Numeric content: a rational factor shared by every term of both sides.
    let (n2, d2) = (expanded_terms(i, &new_num)?, expanded_terms(i, &new_den)?);
    let content = |terms: &[Expr]| -> Option<Rat> {
        let mut g: Option<Rat> = None;
        for t in terms {
            let c = term_rat_coeff(i, t)?.abs();
            g = Some(match g {
                None => c,
                Some(prev) => rat_gcd(&prev, &c),
            });
        }
        g
    };
    let (Some(cn), Some(cd)) = (content(&n2), content(&d2)) else {
        return Ok(Some((new_num, new_den)));
    };
    let g = rat_gcd(&cn, &cd);
    if g.is_zero() || g.cmp(&Rat::from_i64(1)) != std::cmp::Ordering::Equal {
        let inv = Rat::from_i64(1).div(&g);
        let scale = Expr::rat(inv);
        let scaled_num = eval_call(i, Expr::Sym(i.t.sys.times), vec![scale.clone(), new_num])?;
        let scaled_den = eval_call(i, Expr::Sym(i.t.sys.times), vec![scale, new_den])?;
        return Ok(Some((scaled_num, scaled_den)));
    }
    Ok(Some((new_num, new_den)))
}

pub fn b_cancel(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return b_together(i, args);
    }
    // Combine into one fraction, then reduce N/D by their univariate polynomial
    // GCD. `Cancel` used to be a straight alias for `Together`, so
    // Cancel[(x^2-9)/(x-3)] came back unchanged instead of 3+x.
    let combined = b_together(i, args)?;
    let (mut num, mut den) = together_parts(i, &combined)?;
    // Multivariate first: a common monomial factor or numeric content is
    // invisible to the univariate GCD below (`x^2 y/(x y^2)` -> `x/y`).
    if let Some((n, d)) = cancel_monomial_and_content(i, &num, &den)? {
        num = n;
        den = d;
        if same_q(&den, &Expr::int_i64(1)) {
            return Ok(num);
        }
    }
    // A shared polynomial factor such as `x + y` — the univariate GCD below cannot
    // see it because the coefficients of one are not rational in the other.
    {
        let mut all_vars: Vec<Expr> = vars_of(i, &num)?;
        for v in vars_of(i, &den)? {
            if !all_vars.iter().any(|x| same_q(x, &v)) {
                all_vars.push(v);
            }
        }
        if all_vars.len() >= 2 && all_vars.len() < 6 {
            if let (Some(pn), Some(pd)) = (
                m_from_expr(i, &num, &all_vars)?,
                m_from_expr(i, &den, &all_vars)?,
            ) {
                if pn.len() < 4096 && pd.len() < 4096 {
                    if let Some(g) = multivariate_gcd(i, &pn, &pd, &all_vars) {
                        if let (Some(qn), Some(qd)) =
                            (m_div_exact(&pn, &g), m_div_exact(&pd, &g))
                        {
                            num = m_to_expr(i, &qn, &all_vars)?;
                            den = m_to_expr(i, &qd, &all_vars)?;
                            if same_q(&den, &Expr::int_i64(1)) {
                                return Ok(num);
                            }
                        }
                    }
                }
            }
        }
    }
    // Whatever the univariate step manages, the monomial/content reduction
    // above is already an improvement, so the fallbacks below return the current
    // fraction rather than the original expression.
    let fraction = |i: &mut Interp, n: Expr, d: Expr| -> EvalRes {
        if same_q(&d, &Expr::int_i64(1)) {
            return Ok(n);
        }
        let inv = Expr::node(Expr::Sym(i.t.sys.power), vec![d, Expr::int_i64(-1)]);
        eval_call(i, Expr::Sym(i.t.sys.times), vec![n, inv])
    };
    let var = {
        let mut vars = vars_of(i, &num)?;
        vars.extend(vars_of(i, &den)?);
        match vars.into_iter().next() {
            Some(v) => v,
            None => return fraction(i, num, den),
        }
    };
    let (Some(cn), Some(cd)) = (poly_coeffs(i, &num, &var)?, poly_coeffs(i, &den, &var)?) else {
        return fraction(i, num, den);
    };
    let Some((qn, qd)) = cancel_coeffs(&cn, &cd) else {
        return fraction(i, num, den);
    };
    let n = coeffs_to_expr(i, &qn, &var)?;
    let d = coeffs_to_expr(i, &qd, &var)?;
    if same_q(&d, &Expr::int_i64(1)) {
        return Ok(n);
    }
    let inv = Expr::node(Expr::Sym(i.t.sys.power), vec![d, Expr::int_i64(-1)]);
    eval_call(i, Expr::Sym(i.t.sys.times), vec![n, inv])
}

// ============================ Collect ============================

pub fn b_collect(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Collect", args));
    }
    let e = i.eval(&args[0])?;
    let var = i.eval(&args[1])?;
    match poly_coeffs(i, &e, &var)? {
        Some(coeffs) => {
            let mut out: Vec<Expr> = Vec::new();
            for (k, c) in coeffs.iter().enumerate() {
                if c.is_zero() {
                    continue;
                }
                let pw = if k == 0 {
                    Expr::int_i64(1)
                } else if k == 1 {
                    var.clone()
                } else {
                    Expr::node(
                        Expr::Sym(i.t.sys.power),
                        vec![var.clone(), Expr::int_i64(k as i64)],
                    )
                };
                out.push(Expr::node(
                    Expr::Sym(i.t.sys.times),
                    vec![Expr::rat(c.clone()), pw],
                ));
            }
            if out.is_empty() {
                return Ok(Expr::int_i64(0));
            }
            eval_call(i, Expr::Sym(i.t.sys.plus), out)
        }
        None => Ok(unevaluated_call(i, "Collect", args)),
    }
}

// ============================ Factor (v1: rational roots) ============================

/// Horner evaluation of a coefficient list at x.
pub fn horner(coeffs: &[Rat], x: &Rat) -> Rat {
    let mut acc = Rat::from_i64(0);
    for c in coeffs.iter().rev() {
        acc = acc.mul(x).add(c);
    }
    acc
}

/// Divide a Rat-coefficient polynomial by (x - root) (synthetic division).
fn synth_div(coeffs: &[Rat], root: &Rat) -> Vec<Rat> {
    let n = coeffs.len();
    let mut b: Vec<Rat> = Vec::with_capacity(n - 1);
    b.push(coeffs[n - 1].clone());
    for k in (0..n - 1).rev() {
        let prev = b[b.len() - 1].clone();
        let prod = prev.mul(root);
        b.push(coeffs[k].add(&prod));
    }
    b.pop(); // remainder
    b.reverse();
    b
}

#[allow(dead_code)]
fn horner_pub(coeffs: &[Rat], x: &Rat) -> Rat {
    horner(coeffs, x)
}

pub fn b_factor(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Factor", args));
    }
    let e = i.eval(&args[0])?;
    let vars = vars_of(i, &e)?;
    // Pick the first variable with a poly of degree >= 1.
    let mut chosen: Option<(Expr, Vec<Rat>)> = None;
    for v in &vars {
        if let Some(c) = poly_coeffs(i, &e, v)? {
            if c.len() >= 2 {
                chosen = Some((v.clone(), c));
                break;
            }
        }
    }
    let Some((var, coeffs)) = chosen else {
        return Ok(e.clone());
    };
    // Content: gcd of integer-valued coefficients.
    let mut content = BigInt::from_i64(0);
    let mut all_int = true;
    for c in &coeffs {
        if c.is_int() {
            content = content.gcd(&c.num.clone().abs());
        } else {
            all_int = false;
            break;
        }
    }
    let mut factors: Vec<Expr> = Vec::new();
    if all_int && !content.is_one() {
        factors.push(Expr::Int(content.clone()));
    }
    // Rational root search: p/q with p | c0.num, q | clead.num.
    // The content must be divided out first, or the leftover constant (which is
    // the content itself) is multiplied in a second time: 2x²-2 came out as
    // 4(x-1)(x+1) instead of 2(x-1)(x+1).
    let mut work: Vec<Rat> = if all_int && !content.is_one() {
        let content_rat = Rat::new(content.clone(), BigInt::from_i64(1));
        coeffs.iter().map(|x| x.div(&content_rat)).collect()
    } else {
        coeffs.clone()
    };
    loop {
        if work.len() < 2 {
            break;
        }
        let c0n = work[0].num.clone().abs().to_i64().unwrap_or(1);
        let cln = work[work.len() - 1].num.clone();
        let cln = cln.to_i64().unwrap_or(1).max(1);
        let mut root: Option<Rat> = None;
        'outer: for p in divisors64(c0n) {
            for q in divisors64(cln) {
                for s in [1i64, -1] {
                    let r = Rat::new(BigInt::from_i64(s * p), BigInt::from_i64(q));
                    if horner(&work, &r).is_zero() {
                        root = Some(r);
                        break 'outer;
                    }
                }
            }
        }
        let Some(r) = root else { break };
        work = synth_div(&work, &r);
        let lin = Expr::node(
            Expr::Sym(i.t.sys.plus),
            vec![var.clone(), Expr::rat(r.clone().neg())],
        );
        factors.push(lin);
    }
    if factors.is_empty() {
        return Ok(e.clone());
    }
    // Remaining polynomial.
    let mut rest = Expr::int_i64(0);
    for (k, c) in work.iter().enumerate().rev() {
        if c.is_zero() {
            continue;
        }
        let pw = if k == 0 {
            Expr::int_i64(1)
        } else if k == 1 {
            var.clone()
        } else {
            Expr::node(
                Expr::Sym(i.t.sys.power),
                vec![var.clone(), Expr::int_i64(k as i64)],
            )
        };
        let term = Expr::node(
            Expr::Sym(i.t.sys.times),
            vec![Expr::rat(c.clone()), pw],
        );
        rest = eval_call(i, Expr::Sym(i.t.sys.plus), vec![rest, term])?;
    }
    factors.push(rest);
    let r = eval_call(i, Expr::Sym(i.t.sys.times), factors)?;
    i.step("factorization", &r);
    Ok(r)
}

pub fn divisors64(v: i64) -> Vec<i64> {
    let v = v.abs().max(1);
    let mut out = Vec::new();
    let mut k = 1i64;
    while k * k <= v {
        if v % k == 0 {
            out.push(k);
            if k != v / k {
                out.push(v / k);
            }
        }
        k += 1;
    }
    out.sort();
    out
}

// ============================ Apart (distinct linear factors) ============================

fn poly_deriv(c: &[Rat]) -> Vec<Rat> {
    if c.len() <= 1 {
        return vec![Rat::from_i64(0)];
    }
    let mut out = Vec::with_capacity(c.len() - 1);
    for (k, ck) in c.iter().enumerate().skip(1) {
        out.push(ck.mul(&Rat::from_i64(k as i64)));
    }
    out
}

/// Split off rational roots; returns (root, multiplicity) and the leftover
/// polynomial (constant when the split was complete).
fn split_linear_factors(mut work: Vec<Rat>) -> (Vec<(Rat, usize)>, Vec<Rat>) {
    let mut roots: Vec<(Rat, usize)> = Vec::new();
    // An x factor shows as a zero constant term and the p/q search misses it.
    while work.len() > 1 && work[0].is_zero() {
        work.remove(0);
        if let Some(slot) = roots.iter_mut().find(|(x, _)| *x == Rat::from_i64(0)) {
            slot.1 += 1;
        } else {
            roots.push((Rat::from_i64(0), 1));
        }
    }
    loop {
        if work.len() < 2 {
            break;
        }
        let c0n = work[0].num.clone().abs().to_i64().unwrap_or(1);
        let cln = work[work.len() - 1].num.clone().to_i64().unwrap_or(1).max(1);
        let mut root: Option<Rat> = None;
        'outer: for p in divisors64(c0n) {
            for q in divisors64(cln) {
                for s in [1i64, -1] {
                    let r = Rat::new(BigInt::from_i64(s * p), BigInt::from_i64(q));
                    if horner(&work, &r).is_zero() {
                        root = Some(r);
                        break 'outer;
                    }
                }
            }
        }
        let Some(r) = root else { break };
        work = synth_div(&work, &r);
        if let Some(slot) = roots.iter_mut().find(|(x, _)| *x == r) {
            slot.1 += 1;
        } else {
            roots.push((r, 1));
        }
    }
    (roots, work)
}

/// An exact rational square root, or None when either part is not a square.
fn rat_sqrt(r: &Rat) -> Option<Rat> {
    if r.is_neg() {
        return None;
    }
    let n = r.num.clone().isqrt();
    let d = r.den.clone().isqrt();
    if n.mul(&n) == r.num && d.mul(&d) == r.den {
        Some(Rat { num: n, den: d })
    } else {
        None
    }
}

fn poly_mul(a: &[Rat], b: &[Rat]) -> Vec<Rat> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![Rat::from_i64(0); a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] = out[i + j].add(&x.mul(y));
        }
    }
    out
}

/// The monic-normalised quadratic `p` is a perfect square, i.e. `p = q^2` for a
/// quadratic `q` — the shape `(x^2 + 1)^2` a denominator takes. Only degree 4 is
/// supported: `q = α x^2 + β x + γ` from α² = p₄, 2αβ = p₃, β² + 2αγ = p₂, then
/// the last two coefficients are checked rather than solved.
fn square_of_quadratic(p: &[Rat]) -> Option<Vec<Rat>> {
    if p.len() != 5 {
        return None;
    }
    let alpha = rat_sqrt(&p[4])?;
    if alpha.is_zero() {
        return None;
    }
    let beta = p[3].div(&Rat::from_i64(2).mul(&alpha));
    let two_alpha = Rat::from_i64(2).mul(&alpha);
    let gamma = p[2].sub(&beta.mul(&beta)).div(&two_alpha);
    let check1 = Rat::from_i64(2).mul(&beta).mul(&gamma);
    if check1.cmp(&p[1]) != std::cmp::Ordering::Equal {
        return None;
    }
    if gamma.mul(&gamma).cmp(&p[0]) != std::cmp::Ordering::Equal {
        return None;
    }
    Some(vec![gamma, beta, alpha])
}

/// The first `n` Taylor coefficients of `p` about `r`: `p(r+u) = Σ b_j u^j`,
/// with `b_j = p^{(j)}(r)/j!` (repeated synthetic division by `x - r`).
fn taylor_coeffs(mut coeffs: Vec<Rat>, r: &Rat, n: usize) -> Vec<Rat> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        if coeffs.is_empty() {
            break;
        }
        out.push(horner(&coeffs, r));
        coeffs = synth_div(&coeffs, r);
    }
    out
}

/// `den / (x - r)^k` by `k` synthetic divisions.
fn divide_by_power(den: &[Rat], r: &Rat, k: usize) -> Vec<Rat> {
    let mut cur = den.to_vec();
    for _ in 0..k {
        if cur.len() < 2 {
            return Vec::new();
        }
        cur = synth_div(&cur, r);
    }
    cur
}

/// Partial fractions for a rational function whose denominator splits into
/// distinct linear factors over the rationals. None when that does not apply
/// (repeated factors, irreducible quadratics, non-rational input).
pub fn apart_expr(i: &mut Interp, e: &Expr, var: &Expr) -> Result<Option<Expr>, Flow> {
    let (num_e, den_e) = together_parts(i, e)?;
    let Some(num) = poly_coeffs(i, &num_e, var)? else {
        return Ok(None);
    };
    let Some(den) = poly_coeffs(i, &den_e, var)? else {
        return Ok(None);
    };
    if den.len() < 2 {
        // The denominator is a constant: the input is already a polynomial, so
        // "apart" is the identity. Returning it makes Apart[x^2+1, x] = x^2+1
        // instead of an unevaluated Apart head (and lets Apart remove a common
        // factor through the Together step before this point).
        if den.len() == 1 && den[0].cmp(&Rat::from_i64(1)) == std::cmp::Ordering::Equal {
            return Ok(Some(num_e));
        }
        return Ok(None);
    }
    let Some((quo, rem)) = poly_div(&num, &den) else {
        return Ok(None);
    };
    let (roots, leftover) = split_linear_factors(den.clone());
    // A degree-3 leftover is a quadratic with no rational root — irreducible over
    // the rationals, so it can carry a (B x + C) numerator of its own. Degree 2
    // means an irrational linear factor, which this does not model.
    // The leftover cofactor is a constant (complete split), one irreducible
    // quadratic, or the square of one — `(x^2 + 1)^2` and friends.
    let (quad, quad_power) = if leftover.len() == 1 {
        (None, 0usize)
    } else if leftover.len() == 3 {
        (Some(leftover.clone()), 1usize)
    } else if let Some(q) = square_of_quadratic(&leftover) {
        // The square must itself be irreducible, or the split is not in this form.
        if q.len() == 3 && split_linear_factors(q.clone()).0.is_empty() {
            (Some(q), 2usize)
        } else {
            return Ok(None);
        }
    } else {
        return Ok(None);
    };
    if quad.is_none() && roots.is_empty() {
        return Ok(None);
    }
    let dprime = poly_deriv(&den);
    let mut terms: Vec<Expr> = Vec::new();
    let q_expr = coeffs_to_expr(i, &quo, var)?;
    if !same_q(&q_expr, &Expr::int_i64(0)) {
        terms.push(q_expr);
    }
    // Coefficients of the partial fractions: (root, exponent k, A) meaning
    // A/(x - r)^k. A repeated root r of multiplicity m contributes k = 1..m, got
    // by expanding N(x)/P(x) as a power series about r, where P = D/(x - r)^m:
    // A_k is the coefficient of u^{m-k} in that series (u = x - r). For a simple
    // root this is the cover-up rule N(r)/D'(r).
    let mut coeffs: Vec<(Rat, usize, Rat)> = Vec::new();
    for (r, m) in &roots {
        let m = *m;
        let p = divide_by_power(&den, r, m);
        if p.is_empty() {
            return Ok(None);
        }
        let np = taylor_coeffs(rem.clone(), r, m);
        let pp = taylor_coeffs(p, r, m);
        let zero = Rat::from_i64(0);
        let p0 = pp.first().cloned().unwrap_or_else(|| zero.clone());
        if p0.is_zero() {
            return Ok(None);
        }
        let mut series: Vec<Rat> = Vec::with_capacity(m);
        for j in 0..m {
            let mut acc = np.get(j).cloned().unwrap_or_else(|| zero.clone());
            for i in 1..=j {
                if let (Some(pi), Some(cj)) = (pp.get(i), series.get(j - i)) {
                    acc = acc.sub(&pi.mul(cj));
                }
            }
            series.push(acc.div(&p0));
        }
        for k in 1..=m {
            let a = series[m - k].clone();
            if !a.is_zero() {
                coeffs.push((r.clone(), k, a));
            }
        }
    }
    let _ = &dprime;
    for (r, k, a) in &coeffs {
        let lin = Expr::node(
            Expr::Sym(i.t.sys.plus),
            vec![var.clone(), Expr::rat(Rat::from_i64(0).sub(r))],
        );
        let pw = Expr::node(
            Expr::Sym(i.t.sys.power),
            vec![lin, Expr::int_i64(-(*k as i64))],
        );
        let term = Expr::node(
            Expr::Sym(i.t.sys.times),
            vec![Expr::rat(a.clone()), pw],
        );
        terms.push(i.eval(&term)?);
    }
    if let Some(q) = &quad {
        // rem = Σ A_i · den/(x - r_i)^k + Σ_j (B_j x + C_j) · L(x) · Q^(k-j), where
        // L is the product of the linear factors. Divide by L, then peel off one
        // (B_j x + C_j)/Q^j at a time by dividing by Q^(k-j): the quotient is at
        // most linear, and that is B_j x + C_j.
        let mut residual = rem.clone();
        for (r, k, a) in &coeffs {
            let cofactor = divide_by_power(&den, r, *k);
            if cofactor.is_empty() {
                return Ok(None);
            }
            residual = poly_sub(&residual, &poly_scale(&cofactor, a));
        }
        // L = product of the linear factor powers.
        let mut lin = vec![Rat::from_i64(1)];
        for (r, m) in &roots {
            let factor = vec![Rat::from_i64(0).sub(r), Rat::from_i64(1)];
            for _ in 0..*m {
                lin = poly_mul(&lin, &factor);
            }
        }
        let Some((t_poly, _)) = poly_div(&residual, &lin) else {
            return Ok(None);
        };
        let mut cur = t_poly;
        for j in 1..=quad_power {
            // Q^(power - j)
            let mut qpow = vec![Rat::from_i64(1)];
            for _ in 0..(quad_power - j) {
                qpow = poly_mul(&qpow, q);
            }
            let Some((num_part, rest)) = poly_div(&cur, &qpow) else {
                return Ok(None);
            };
            cur = rest;
            if num_part.iter().all(|c| c.is_zero()) {
                continue;
            }
            let num_expr = coeffs_to_expr(i, &num_part, var)?;
            let mut q_expr = coeffs_to_expr(i, q, var)?;
            if j > 1 {
                q_expr = eval_call(
                    i,
                    Expr::Sym(i.t.sys.power),
                    vec![q_expr, Expr::int_i64(j as i64)],
                )?;
            }
            let term = Expr::node(
                Expr::Sym(i.t.sys.times),
                vec![
                    num_expr,
                    Expr::node(Expr::Sym(i.t.sys.power), vec![q_expr, Expr::int_i64(-1)]),
                ],
            );
            terms.push(i.eval(&term)?);
        }
    }
    if terms.is_empty() {
        return Ok(Some(Expr::int_i64(0)));
    }
    let out = if terms.len() == 1 {
        terms.pop().unwrap()
    } else {
        Expr::node(Expr::Sym(i.t.sys.plus), terms)
    };
    let r = i.eval(&out)?;
    i.step("partial fractions", &r);
    Ok(Some(r))
}

pub fn b_apart(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Apart", "argr");
        return Ok(unevaluated_call(i, "Apart", args));
    }
    let e = i.eval(&args[0])?;
    let var = i.eval(&args[1])?;
    match apart_expr(i, &e, &var)? {
        Some(v) => Ok(v),
        None => Ok(unevaluated_call(i, "Apart", args)),
    }
}

// ============================ polynomial division ============================

/// Long division of Rat-coefficient polynomials (lowest-first lists).
/// Long division of Rat-coefficient polynomials (lowest-first lists).
fn poly_scale(a: &[Rat], s: &Rat) -> Vec<Rat> {
    a.iter().map(|c| c.mul(s)).collect()
}

fn poly_sub(a: &[Rat], b: &[Rat]) -> Vec<Rat> {
    let n = a.len().max(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = a.get(i).cloned().unwrap_or_else(|| Rat::from_i64(0));
        let y = b.get(i).cloned().unwrap_or_else(|| Rat::from_i64(0));
        out.push(x.sub(&y));
    }
    out
}

pub fn poly_div(a: &[Rat], b: &[Rat]) -> Option<(Vec<Rat>, Vec<Rat>)> {
    let bn = b.iter().rposition(|c| !c.is_zero())? + 1;
    let b_lead = b[bn - 1].clone();
    if b_lead.is_zero() {
        return None;
    }
    let mut rem: Vec<Rat> = a.to_vec();
    let mut quo: Vec<Rat> = Vec::new();
    loop {
        trim(&mut rem);
        let n = rem.len();
        if n < bn {
            break;
        }
        let lead = rem[n - 1].clone();
        let factor = lead.div(&b_lead);
        let shift = n - bn;
        if quo.len() < shift + 1 {
            quo.resize(shift + 1, Rat::from_i64(0));
        }
        quo[shift] = quo[shift].add(&factor);
        for k in 0..bn {
            rem[shift + k] = rem[shift + k].sub(&b[k].mul(&factor));
        }
        rem.pop();
    }
    Some((quo, rem))
}

fn trim(v: &mut Vec<Rat>) {
    while v.last().map_or(false, |c| c.is_zero()) {
        v.pop();
    }
}

pub fn b_polynomialquotient(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let (q, _) = poly_quotient_remainder(i, args)?;
    q
}

pub fn b_polynomialremainder(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let (_, r) = poly_quotient_remainder(i, args)?;
    r
}

pub fn b_polynomialquotientremainder(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let (q, r) = poly_quotient_remainder(i, args)?;
    Ok(Expr::node(Expr::Sym(i.t.sys.list), vec![q?, r?]))
}

/// Returns (Ok(quotient), Ok(remainder)) or unevaluated expressions.
fn poly_quotient_remainder(
    i: &mut Interp,
    args: &[Expr],
) -> Result<(EvalRes, EvalRes), Flow> {
    if args.len() != 3 {
        return Ok((Ok(unevaluated_call(i, "PolynomialQuotient", args)), Ok(unevaluated_call(i, "PolynomialRemainder", args))));
    }
    let a = i.eval(&args[0])?;
    let b = i.eval(&args[1])?;
    let var = i.eval(&args[2])?;
    let ca = match poly_coeffs(i, &a, &var)? {
        Some(c) => c,
        None => return Ok((Ok(unevaluated_call(i, "PolynomialQuotient", args)), Ok(unevaluated_call(i, "PolynomialRemainder", args)))),
    };
    let cb = match poly_coeffs(i, &b, &var)? {
        Some(c) => c,
        None => return Ok((Ok(unevaluated_call(i, "PolynomialQuotient", args)), Ok(unevaluated_call(i, "PolynomialRemainder", args)))),
    };
    let (q, r) = match poly_div(&ca, &cb) {
        Some(x) => x,
        None => return Ok((Ok(unevaluated_call(i, "PolynomialQuotient", args)), Ok(unevaluated_call(i, "PolynomialRemainder", args)))),
    };
    let qe = coeffs_to_expr(i, &q, &var)?;
    let re = coeffs_to_expr(i, &r, &var)?;
    Ok((Ok(qe), Ok(re)))
}

pub fn coeffs_to_expr(i: &mut Interp, coeffs: &[Rat], var: &Expr) -> EvalRes {
    let mut out: Vec<Expr> = Vec::new();
    for (k, c) in coeffs.iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        let pw = if k == 0 {
            Expr::int_i64(1)
        } else if k == 1 {
            var.clone()
        } else {
            Expr::node(Expr::Sym(i.t.sys.power), vec![var.clone(), Expr::int_i64(k as i64)])
        };
        out.push(Expr::node(Expr::Sym(i.t.sys.times), vec![Expr::rat(c.clone()), pw]));
    }
    if out.is_empty() {
        return Ok(Expr::int_i64(0));
    }
    eval_call(i, Expr::Sym(i.t.sys.plus), out)
}

/// A multivariate polynomial over ℚ: monomial exponent vectors (in the order of
/// a fixed variable list) mapped to coefficients. `BTreeMap`'s key order is a
/// monomial order (plain lexicographic on the exponent vectors), which is all
/// multivariate division needs.
type MPoly = std::collections::BTreeMap<Vec<u32>, Rat>;

fn m_from_expr(i: &mut Interp, e: &Expr, vars: &[Expr]) -> Result<Option<MPoly>, Flow> {
    let mut out: MPoly = MPoly::new();
    let terms = expanded_terms(i, e)?;
    for t in &terms {
        let mut bases = Vec::new();
        let mut coeffs = Vec::new();
        split_term(i, t, &mut bases, &mut coeffs);
        let mut exps = vec![0u32; vars.len()];
        for (b, n) in &bases {
            if *n < 0 {
                return Ok(None);
            }
            let Some(pos) = vars.iter().position(|v| same_q(v, b)) else {
                return Ok(None);
            };
            exps[pos] += *n as u32;
        }
        let mut c = Rat::from_i64(1);
        for f in &coeffs {
            match f.as_rat_exact() {
                Some(r) => c = c.mul(&r),
                None => return Ok(None),
            }
        }
        let slot = out.entry(exps).or_insert_with(|| Rat::from_i64(0));
        *slot = slot.add(&c);
    }
    out.retain(|_, c| !c.is_zero());
    Ok(Some(out))
}

fn m_to_expr(i: &mut Interp, p: &MPoly, vars: &[Expr]) -> Result<Expr, Flow> {
    let sys = i.t.sys;
    let mut terms: Vec<Expr> = Vec::new();
    for (exps, c) in p {
        let mut factors: Vec<Expr> = vec![Expr::rat(c.clone())];
        for (v, e) in vars.iter().zip(exps.iter()) {
            if *e == 0 {
                continue;
            }
            let factor = if *e == 1 {
                v.clone()
            } else {
                eval_call(
                    i,
                    Expr::Sym(sys.power),
                    vec![v.clone(), Expr::int_i64(*e as i64)],
                )?
            };
            factors.push(factor);
        }
        terms.push(eval_call(i, Expr::Sym(sys.times), factors)?);
    }
    if terms.is_empty() {
        return Ok(Expr::int_i64(0));
    }
    eval_call(i, Expr::Sym(sys.plus), terms)
}

fn m_scale(p: &MPoly, k: &Rat) -> MPoly {
    p.iter().map(|(e, c)| (e.clone(), c.mul(k))).collect()
}

fn m_sub(a: &MPoly, b: &MPoly) -> MPoly {
    let mut out = a.clone();
    for (e, c) in b {
        let slot = out.entry(e.clone()).or_insert_with(|| Rat::from_i64(0));
        *slot = slot.sub(c);
    }
    out.retain(|_, c| !c.is_zero());
    out
}

fn m_mul(a: &MPoly, b: &MPoly) -> MPoly {
    let mut out: MPoly = MPoly::new();
    for (ea, ca) in a {
        for (eb, cb) in b {
            let mut e = ea.clone();
            for (k, v) in eb.iter().enumerate() {
                e[k] += v;
            }
            let slot = out.entry(e).or_insert_with(|| Rat::from_i64(0));
            *slot = slot.add(&ca.mul(cb));
        }
    }
    out.retain(|_, c| !c.is_zero());
    out
}

/// Exact quotient `a / b` (None when the division does not come out even).
fn m_div_exact(a: &MPoly, b: &MPoly) -> Option<MPoly> {
    if b.is_empty() {
        return None;
    }
    let (lead_b, coef_b) = b.iter().next_back()?;
    let mut rem = a.clone();
    let mut quot: MPoly = MPoly::new();
    while !rem.is_empty() {
        let (lead_r, coef_r) = rem.iter().next_back()?;
        // The divisor's leading monomial must divide the remainder's.
        if lead_b.iter().zip(lead_r.iter()).any(|(x, y)| x > y) {
            return None;
        }
        let e: Vec<u32> = lead_r.iter().zip(lead_b.iter()).map(|(x, y)| x - y).collect();
        let c = coef_r.div(coef_b);
        let term: MPoly = std::iter::once((e, c)).collect();
        quot = m_sub(&quot, &m_scale(&term, &Rat::from_i64(-1)));
        rem = m_sub(&rem, &m_mul(&term, b));
    }
    Some(quot)
}

fn m_content(p: &MPoly) -> Rat {
    let mut g: Option<Rat> = None;
    for c in p.values() {
        let c = c.clone().abs();
        g = Some(match g {
            None => c,
            Some(prev) => rat_gcd(&prev, &c),
        });
    }
    g.unwrap_or_else(|| Rat::from_i64(1))
}

/// An exact univariate GCD, made monic.
fn uni_gcd(a: &[Rat], b: &[Rat]) -> Vec<Rat> {
    let mut x = a.to_vec();
    let mut y = b.to_vec();
    trim(&mut x);
    trim(&mut y);
    while !y.is_empty() {
        let Some((_, r)) = poly_div(&x, &y) else {
            return vec![Rat::from_i64(1)];
        };
        x = y;
        y = r;
        trim(&mut y);
    }
    trim(&mut x);
    if x.is_empty() {
        return vec![Rat::from_i64(1)];
    }
    let lead = x[x.len() - 1].clone();
    if lead.is_zero() {
        return vec![Rat::from_i64(1)];
    }
    div_coeffs(&x, &lead)
}

/// A common factor of two multivariate polynomials, by Kronecker substitution:
/// replace the variables by a single one so the univariate GCD applies, then
/// check the decoded result really divides both inputs (that check is what makes
/// the shortcut safe — a degenerate substitution can only lose a factor, never
/// invent one).
fn multivariate_gcd(i: &mut Interp, a: &MPoly, b: &MPoly, vars: &[Expr]) -> Option<MPoly> {
    let degree = |p: &MPoly| -> u32 {
        p.keys().map(|e| e.iter().sum::<u32>()).max().unwrap_or(0)
    };
    let d = degree(a).max(degree(b));
    let base = d + 1;
    let encode = |e: &[u32]| -> usize {
        let mut idx = 0usize;
        let mut mul = 1usize;
        for x in e {
            idx += *x as usize * mul;
            mul *= base as usize;
        }
        idx
    };
    let to_uni = |p: &MPoly| -> Vec<Rat> {
        let mut out: Vec<Rat> =
            vec![Rat::from_i64(0); p.keys().map(|e| encode(e)).max().unwrap_or(0) + 1];
        for (e, c) in p {
            let k = encode(e);
            out[k] = out[k].add(c);
        }
        trim(&mut out);
        out
    };
    let ua = to_uni(a);
    let ub = to_uni(b);
    let ug = uni_gcd(&ua, &ub);
    if ug.len() <= 1 {
        return None;
    }
    // Decode: the digits of the index in base `base` are the exponents.
    let mut g: MPoly = MPoly::new();
    for (k, c) in ug.iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        let mut idx = k;
        let mut e = vec![0u32; vars.len()];
        for slot in e.iter_mut() {
            *slot = (idx % base as usize) as u32;
            idx /= base as usize;
        }
        if idx != 0 {
            return None; // exponent beyond the encoding's range
        }
        g.insert(e, c.clone());
    }
    // The decoded polynomial must genuinely divide both inputs: that check is what
    // makes the shortcut sound (a degenerate substitution can lose a factor, never
    // invent one).
    m_div_exact(a, &g)?;
    m_div_exact(b, &g)?;
    // A unit multiple is the same factor; pick the sign with a positive leading
    // coefficient so `Cancel[(x^2 - y^2)/(x - y)]` reads `x + y`, not `-(-x - y)`.
    if let Some((_, lead)) = g.iter().next_back() {
        if lead.is_neg() {
            g = m_scale(&g, &Rat::from_i64(-1));
        }
    }
    Some(g)
}

pub fn b_polynomialgcd(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "PolynomialGCD", args));
    }
    let a = i.eval(&args[0])?;
    let b = i.eval(&args[1])?;
    // More than one variable: a shared factor such as `x + y` is invisible to the
    // univariate GCD below, so try the multivariate one first.
    let mut all_vars: Vec<Expr> = vars_of(i, &a)?;
    for v in vars_of(i, &b)? {
        if !all_vars.iter().any(|x| same_q(x, &v)) {
            all_vars.push(v);
        }
    }
    if all_vars.len() >= 2 && all_vars.len() < 6 {
        if let (Some(pa), Some(pb)) = (
            m_from_expr(i, &a, &all_vars)?,
            m_from_expr(i, &b, &all_vars)?,
        ) {
            if pa.len() < 4096 && pb.len() < 4096 {
                if let Some(g) = multivariate_gcd(i, &pa, &pb, &all_vars) {
                    let content = m_content(&g);
                    let monic = m_scale(&g, &Rat::from_i64(1).div(&content));
                    return m_to_expr(i, &monic, &all_vars);
                }
            }
        }
    }
    let Some(var) = vars_of(i, &a)?.first().cloned() else {
        return Ok(unevaluated_call(i, "PolynomialGCD", args));
    };
    let Some(ca) = poly_coeffs(i, &a, &var)? else {
        return Ok(unevaluated_call(i, "PolynomialGCD", args));
    };
    let Some(cb) = poly_coeffs(i, &b, &var)? else {
        return Ok(unevaluated_call(i, "PolynomialGCD", args));
    };
    let mut x = ca;
    let mut y = cb;
    while y.iter().any(|c| !c.is_zero()) {
        let (_, r) = poly_div(&x, &y).unwrap_or((vec![], vec![]));
        if r.is_empty() {
            break;
        }
        x = y;
        y = r;
    }
    // Normalize: make the leading coefficient 1.
    if y.is_empty() {
        return coeffs_to_expr(i, &x, &var);
    }
    Ok(coeffs_to_expr(i, &y, &var)?)
}

fn div_coeffs(c: &[Rat], d: &Rat) -> Vec<Rat> {
    let mut out: Vec<Rat> = c.iter().map(|x| x.div(d)).collect();
    trim(&mut out);
    out
}

/// Reduce N/D by their univariate GCD, returning the reduced coefficient lists.
pub fn cancel_coeffs(a: &[Rat], b: &[Rat]) -> Option<(Vec<Rat>, Vec<Rat>)> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let mut x = a.to_vec();
    let mut y = b.to_vec();
    while y.iter().any(|c| !c.is_zero()) {
        let (_, r) = poly_div(&x, &y)?;
        if r.is_empty() {
            break;
        }
        x = y;
        y = r;
    }
    let mut g = if y.is_empty() { x } else { y };
    trim(&mut g);
    if g.is_empty() {
        return None;
    }
    if g.len() == 1 {
        let lead = g[0].clone();
        return Some((div_coeffs(a, &lead), div_coeffs(b, &lead)));
    }
    // A monic GCD keeps the reduced forms monic, so Cancel[(x^2-1)/(x^2+2x+1)]
    // reads (-1+x)/(1+x) rather than (1/2-1/2x)/(-1/2-1/2x).
    let lead = g[g.len() - 1].clone();
    if lead.cmp(&Rat::from_i64(1)) != std::cmp::Ordering::Equal {
        g = div_coeffs(&g, &lead);
    }
    let (qa, _) = poly_div(a, &g)?;
    let (qb, _) = poly_div(b, &g)?;
    // Cancel the numeric content too, so the denominator is monic:
    // (6x^2-6)/(2x-2) reduces to 3+3x rather than 1/2 (6+6x).
    let dlead = qb[qb.len() - 1].clone();
    if dlead.cmp(&Rat::from_i64(1)) != std::cmp::Ordering::Equal {
        return Some((div_coeffs(&qa, &dlead), div_coeffs(&qb, &dlead)));
    }
    Some((qa, qb))
}
