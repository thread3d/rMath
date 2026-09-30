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

pub fn b_cancel(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return b_together(i, args);
    }
    // Combine into one fraction, then reduce N/D by their univariate polynomial
    // GCD. `Cancel` used to be a straight alias for `Together`, so
    // Cancel[(x^2-9)/(x-3)] came back unchanged instead of 3+x.
    let combined = b_together(i, args)?;
    let (num, den) = together_parts(i, &combined)?;
    let var = {
        let mut vars = vars_of(i, &num)?;
        vars.extend(vars_of(i, &den)?);
        match vars.into_iter().next() {
            Some(v) => v,
            None => return Ok(combined),
        }
    };
    let (Some(cn), Some(cd)) = (poly_coeffs(i, &num, &var)?, poly_coeffs(i, &den, &var)?) else {
        return Ok(combined);
    };
    let Some((qn, qd)) = cancel_coeffs(&cn, &cd) else {
        return Ok(combined);
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
    if leftover.len() > 1 || roots.is_empty() || roots.iter().any(|(_, m)| *m != 1) {
        return Ok(None);
    }
    let dprime = poly_deriv(&den);
    let mut terms: Vec<Expr> = Vec::new();
    let q_expr = coeffs_to_expr(i, &quo, var)?;
    if !same_q(&q_expr, &Expr::int_i64(0)) {
        terms.push(q_expr);
    }
    for (r, _) in &roots {
        // Cover-up rule for a simple root: A = N(r) / D'(r).
        let a = horner(&rem, r).div(&horner(&dprime, r));
        if a.is_zero() {
            continue;
        }
        let lin = Expr::node(
            Expr::Sym(i.t.sys.plus),
            vec![var.clone(), Expr::rat(Rat::from_i64(0).sub(r))],
        );
        let term = Expr::node(
            Expr::Sym(i.t.sys.times),
            vec![
                Expr::rat(a),
                Expr::node(Expr::Sym(i.t.sys.power), vec![lin, Expr::int_i64(-1)]),
            ],
        );
        terms.push(i.eval(&term)?);
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

pub fn b_polynomialgcd(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "PolynomialGCD", args));
    }
    let a = i.eval(&args[0])?;
    let b = i.eval(&args[1])?;
    // v1: univariate GCD using the first variable of a.
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
