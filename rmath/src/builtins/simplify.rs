//! A conservative `Simplify` / `FullSimplify` with assumptions.
//!
//! The passes are deliberately safe: inverse-function cancellations, nested
//! powers with integer outer exponents, assumption-driven square roots and
//! absolute values, the Pythagorean identities, and cancellation of a
//! univariate rational function by its polynomial GCD. `Assuming` pushes
//! assumptions onto `Interp::assumptions`, and `$Assumptions` is consulted too.

use crate::builtins::core::unevaluated_call;
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::{same_q, Expr};

fn is_zero(e: &Expr) -> bool {
    crate::builtins::num::is_numeric_zero(e)
}
fn is_two(e: &Expr) -> bool {
    crate::builtins::core::expr_to_int(e) == Some(2)
}
fn is_half(e: &Expr) -> bool {
    matches!(e, Expr::Rat(r) if r.num.to_i64() == Some(1) && r.den.to_i64() == Some(2))
}
fn neg(i: &Interp, e: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.times), vec![Expr::int_i64(-1), e])
}
fn app(i: &mut Interp, name: &str, y: &Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.intern(name)), vec![y.clone()])
}

/// Flatten And[...] / List[...] assumption containers into a flat vector.
fn flatten_assumptions(i: &Interp, a: &Expr, out: &mut Vec<Expr>) {
    if let Some((h, args)) = a.parts() {
        if h.is_symbol(i.t.sys.and) || h.is_symbol(i.t.sys.list) {
            for x in args {
                flatten_assumptions(i, x, out);
            }
            return;
        }
    }
    if !a.is_symbol(i.t.sys.r#true) {
        out.push(a.clone());
    }
}

/// (nonneg, nonpos, pos, neg) implied for `x` by the assumption list.
fn assum_cmp(i: &Interp, assums: &[Expr], x: &Expr) -> (bool, bool, bool, bool) {
    let (mut nn, mut np, mut p, mut n) = (false, false, false, false);
    let sys = i.t.sys;
    let cmp = |e: &Expr| -> Option<(bool, bool, bool, bool)> {
        let (h, a) = e.parts()?;
        if a.len() != 2 {
            return None;
        }
        let (l, r) = (&a[0], &a[1]);
        let x_left = same_q(l, x);
        let x_right = same_q(r, x);
        let zero_left = is_zero(l);
        let zero_right = is_zero(r);
        let mut out = (false, false, false, false);
        if h.is_symbol(sys.greaterequal) {
            if x_left && zero_right {
                out.0 = true;
            }
            if x_right && zero_left {
                out.1 = true;
            }
        }
        if h.is_symbol(sys.lessequal) {
            if x_right && zero_left {
                out.0 = true;
            }
            if x_left && zero_right {
                out.1 = true;
            }
        }
        if h.is_symbol(sys.greater) {
            if x_left && zero_right {
                out.0 = true;
                out.2 = true;
            }
            if x_right && zero_left {
                out.1 = true;
                out.3 = true;
            }
        }
        if h.is_symbol(sys.less) {
            if x_right && zero_left {
                out.0 = true;
                out.2 = true;
            }
            if x_left && zero_right {
                out.1 = true;
                out.3 = true;
            }
        }
        if h.is_symbol(sys.equal) && ((x_left && zero_right) || (x_right && zero_left)) {
            out.0 = true;
            out.1 = true;
        }
        Some(out)
    };
    for a in assums {
        if let Some((onn, onp, op, on)) = cmp(a) {
            nn |= onn;
            np |= onp;
            p |= op;
            n |= on;
        }
    }
    let _ = (&mut p, &mut n);
    (nn, np, p, n)
}

/// Square of a named function: Power[Sin[u], 2] -> Some(u).
fn sq_of(i: &Interp, e: &Expr, fname: &str) -> Option<Expr> {
    let (h, a) = e.parts()?;
    if !h.is_symbol(i.t.sys.power) || a.len() != 2 || !is_two(&a[1]) {
        return None;
    }
    let (fh, fa) = a[0].parts()?;
    let id = fh.sym_id()?;
    if i.t.name_of(id) == fname && fa.len() == 1 {
        Some(fa[0].clone())
    } else {
        None
    }
}

fn pythagorean(i: &Interp, args: &[Expr]) -> Option<Expr> {
    let mut sinu: Option<(usize, Expr)> = None;
    let mut cosu: Option<(usize, Expr)> = None;
    for (k, a) in args.iter().enumerate() {
        if let Some(u) = sq_of(i, a, "Sin") {
            if sinu.is_none() {
                sinu = Some((k, u));
            }
        } else if let Some(u) = sq_of(i, a, "Cos") {
            if cosu.is_none() {
                cosu = Some((k, u));
            }
        }
    }
    let ((si, su), (ci, cu)) = (sinu?, cosu?);
    if !same_q(&su, &cu) {
        return None;
    }
    let mut out: Vec<Expr> = Vec::new();
    for (k, a) in args.iter().enumerate() {
        if k != si && k != ci {
            out.push(a.clone());
        }
    }
    out.push(Expr::int_i64(1));
    Some(Expr::node(Expr::Sym(i.t.sys.plus), out))
}

/// One targeted rewrite at the head of an already-evaluated expression.
fn rewrite(i: &mut Interp, e: &Expr, assums: &[Expr]) -> Result<Option<Expr>, Flow> {
    let sys = i.t.sys;
    let Some((h, args)) = e.parts() else {
        return Ok(None);
    };
    if let Some(id) = h.sym_id() {
        let name = i.t.name_of(id).to_string();
        if name == "Exp" && args.len() == 1 {
            if let Some((lh, la)) = args[0].parts() {
                if lh.is_symbol_named(&i.t, "Log") && la.len() == 1 {
                    let r = la[0].clone();
                    i.step("inverse functions: Exp[Log[u]]", &r);
                    return Ok(Some(r));
                }
            }
        }
        if name == "Log" && args.len() == 1 {
            if let Some((ih, ia)) = args[0].parts() {
                if ih.is_symbol_named(&i.t, "Exp") && ia.len() == 1 {
                    let r = ia[0].clone();
                    i.step("inverse functions: Log[Exp[u]]", &r);
                    return Ok(Some(r));
                }
                if ih.is_symbol(sys.power)
                    && ia.len() == 2
                    && ia[0].is_symbol_named(&i.t, "E")
                {
                    let r = ia[1].clone();
                    i.step("inverse functions: Log[E^u]", &r);
                    return Ok(Some(r));
                }
            }
        }
        if name == "Abs" && args.len() == 1 {
            let (nn, np, _, _) = assum_cmp(i, assums, &args[0]);
            if nn {
                let r = args[0].clone();
                i.step("assumption: Abs[x] = x", &r);
                return Ok(Some(r));
            }
            if np {
                let r = neg(i, args[0].clone());
                i.step("assumption: Abs[x] = -x", &r);
                return Ok(Some(r));
            }
        }
    }
    if h.is_symbol(sys.power) && args.len() == 2 {
        let (base, exp) = (&args[0], &args[1]);
        // Sqrt[x^2] -> x / -x / Abs[x].
        if is_half(exp) {
            if let Some((bh, ba)) = base.parts() {
                if bh.is_symbol(sys.power) && ba.len() == 2 && is_two(&ba[1]) {
                    let x = &ba[0];
                    let (nn, np, _, _) = assum_cmp(i, assums, x);
                    if nn {
                        let r = x.clone();
                        i.step("assumption: Sqrt[x^2] = x", &r);
                        return Ok(Some(r));
                    }
                    if np {
                        let r = neg(i, x.clone());
                        i.step("assumption: Sqrt[x^2] = -x", &r);
                        return Ok(Some(r));
                    }
                    let r = app(i, "Abs", x);
                    i.step("Sqrt[x^2] = Abs[x]", &r);
                    return Ok(Some(r));
                }
            }
        }
        // (b^m)^n -> b^(m*n) for an integer outer exponent.
        if let Some(n) = crate::builtins::core::expr_to_int(exp) {
            if n != 0 {
                if let Some((bh, ba)) = base.parts() {
                    if bh.is_symbol(sys.power) && ba.len() == 2 {
                        let prod = Expr::node(
                            Expr::Sym(sys.times),
                            vec![ba[1].clone(), Expr::int_i64(n)],
                        );
                        let new_exp = i.eval(&prod)?;
                        let r =
                            Expr::node(Expr::Sym(sys.power), vec![ba[0].clone(), new_exp]);
                        i.step("combine powers", &r);
                        return Ok(Some(r));
                    }
                }
            }
        }
    }
    if h.is_symbol(sys.plus) {
        if let Some(v) = pythagorean(i, args) {
            i.step("Pythagorean identity", &v);
            return Ok(Some(v));
        }
    }
    Ok(None)
}

fn simplify_once(
    i: &mut Interp,
    e: &Expr,
    assums: &[Expr],
    depth: u32,
) -> Result<Expr, Flow> {
    if depth > 64 {
        return Ok(e.clone());
    }
    let Some((h, args)) = e.parts() else {
        return Ok(e.clone());
    };
    let mut na: Vec<Expr> = Vec::with_capacity(args.len());
    for a in args {
        na.push(simplify_once(i, a, assums, depth + 1)?);
    }
    let evaled = i.eval(&Expr::node(h.clone(), na))?;
    if let Some(r) = rewrite(i, &evaled, assums)? {
        return i.eval(&r);
    }
    Ok(evaled)
}

/// Cancel a univariate rational expression by its polynomial GCD.
fn cancel_rational(i: &mut Interp, e: &Expr) -> Result<Option<Expr>, Flow> {
    let (num, den) = crate::builtins::poly::together_parts(i, e)?;
    if same_q(&den, &Expr::int_i64(1)) {
        return Ok(None);
    }
    let vars = crate::builtins::poly::vars_of(i, &num)?;
    let Some(var) = vars.first().cloned() else {
        return Ok(None);
    };
    let Some(cn) = crate::builtins::poly::poly_coeffs(i, &num, &var)? else {
        return Ok(None);
    };
    let Some(cd) = crate::builtins::poly::poly_coeffs(i, &den, &var)? else {
        return Ok(None);
    };
    let Some((rn, rd)) = crate::builtins::poly::cancel_coeffs(&cn, &cd) else {
        return Ok(None);
    };
    let num_e = crate::builtins::poly::coeffs_to_expr(i, &rn, &var)?;
    let den_e = crate::builtins::poly::coeffs_to_expr(i, &rd, &var)?;
    if same_q(&den_e, &Expr::int_i64(1)) {
        i.step("cancel common factor", &num_e);
        return Ok(Some(num_e));
    }
    let q = Expr::node(
        Expr::Sym(i.t.sys.times),
        vec![
            num_e,
            Expr::node(Expr::Sym(i.t.sys.power), vec![den_e, Expr::int_i64(-1)]),
        ],
    );
    let r = i.eval(&q)?;
    i.step("cancel common factor", &r);
    Ok(Some(r))
}

/// The full pipeline: rewrite to a fixpoint, then cancel rationals.
pub fn simplify_expr(i: &mut Interp, e: &Expr, assums: &[Expr]) -> Result<Expr, Flow> {
    let mut cur = e.clone();
    for _ in 0..8 {
        let next = simplify_once(i, &cur, assums, 0)?;
        if same_q(&next, &cur) {
            break;
        }
        cur = next;
    }
    if let Some(c) = cancel_rational(i, &cur)? {
        if !same_q(&c, &cur) {
            return simplify_once(i, &c, assums, 0);
        }
    }
    Ok(cur)
}

fn gather_assumptions(i: &mut Interp, args: &[Expr]) -> Result<Vec<Expr>, Flow> {
    let mut out: Vec<Expr> = i.assumptions.clone();
    if let Some(id) = i.t.get("$Assumptions") {
        if let Some(crate::expr::OwnValue::Set(v)) = &i.t.data[id].own {
            let v = v.clone();
            flatten_assumptions(i, &v, &mut out);
        }
    }
    if args.len() >= 2 {
        let a = &args[1];
        let cond = if let Some((rh, ra)) = a.parts() {
            if rh.is_symbol(i.t.sys.rule)
                && ra.len() == 2
                && ra[0].is_symbol_named(&i.t, "Assumptions")
            {
                ra[1].clone()
            } else {
                a.clone()
            }
        } else {
            a.clone()
        };
        flatten_assumptions(i, &cond, &mut out);
    }
    Ok(out)
}

pub fn b_simplify(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("Simplify", "argr");
        return Ok(unevaluated_call(i, "Simplify", args));
    }
    let assums = gather_assumptions(i, args)?;
    simplify_expr(i, &args[0], &assums)
}

pub fn b_fullsimplify(i: &mut Interp, args: &[Expr]) -> EvalRes {
    b_simplify(i, args)
}

pub fn b_assuming(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Assuming", "argr");
        return Ok(unevaluated_call(i, "Assuming", args));
    }
    let assum = i.eval(&args[0])?;
    let mut vals: Vec<Expr> = Vec::new();
    flatten_assumptions(i, &assum, &mut vals);
    let saved = i.assumptions.len();
    i.assumptions.extend(vals);
    let r = i.eval(&args[1]);
    i.assumptions.truncate(saved);
    r
}
