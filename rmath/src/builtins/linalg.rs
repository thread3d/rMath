//! Linear algebra and equation solving:
//! Det, Inverse, Transpose, Dot, IdentityMatrix, DiagonalMatrix, MatrixPower,
//! RowReduce, MatrixRank, LinearSolve, NullSpace, Eigenvalues (2x2),
//! Solve (linear systems + low-degree polynomials), NSolve (Durand-Kerner),
//! FindRoot (Newton).

use crate::bigint::BigInt;
use crate::builtins::core::unevaluated_call;
use crate::builtins::poly::divisors64;
use crate::builtins::num::{num_val, nv_f64};
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::{same_q, Expr};
use crate::rational::Rat;

fn eval_call(i: &mut Interp, head: Expr, args: Vec<Expr>) -> EvalRes {
    i.eval(&Expr::node(head, args))
}

// ---------- structure helpers ----------

/// Rows of a matrix (List of equal-length Lists).
pub fn as_rows(e: &Expr, sys: crate::expr::Sys) -> Option<Vec<Vec<Expr>>> {
    if let Some((h, hargs)) = e.parts() {
        if !h.is_symbol(sys.list) {
            return None;
        }
        let mut rows: Vec<Vec<Expr>> = Vec::new();
        let mut width: Option<usize> = None;
        for r in hargs {
            if let Some((h2, a2)) = r.parts() {
                if h2.is_symbol(sys.list) {
                    let w = a2.len();
                    match width {
                        None => width = Some(w),
                        Some(p) if p == w => {}
                        Some(_) => return None,
                    }
                    rows.push(a2.to_vec());
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some(rows)
    } else {
        None
    }
}

fn rows_expr(i: &Interp, rows: Vec<Vec<Expr>>) -> Expr {
    let lists: Vec<Expr> = rows
        .into_iter()
        .map(|r| Expr::node(Expr::Sym(i.t.sys.list), r))
        .collect();
    Expr::node(Expr::Sym(i.t.sys.list), lists)
}

fn rat_matrix(e: &Expr, sys: crate::expr::Sys) -> Option<Vec<Vec<Rat>>> {
    let rows = as_rows(e, sys)?;
    let mut out = Vec::new();
    for r in rows {
        let mut rr = Vec::new();
        for c in r {
            match c {
                Expr::Int(_) | Expr::Rat(_) => rr.push(c.as_rat_exact().unwrap()),
                _ => return None,
            }
        }
        out.push(rr);
    }
    Some(out)
}

fn f64_matrix(e: &Expr, sys: crate::expr::Sys) -> Option<Vec<Vec<f64>>> {
    let rows = as_rows(e, sys)?;
    let mut out = Vec::new();
    for r in rows {
        let mut rr = Vec::new();
        for c in r {
            match num_val(&c) {
                Some(n) => rr.push(nv_f64(&n)),
                None => return None,
            }
        }
        out.push(rr);
    }
    Some(out)
}

// ============================ Dot ============================

pub fn b_dot(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Dot", "argr");
        return Ok(unevaluated_call(i, "Dot", args));
    }
    let a = i.eval(&args[0])?;
    let b = i.eval(&args[1])?;
    dot_of(i, &a, &b)
}

fn dot_of(i: &mut Interp, a: &Expr, b: &Expr) -> EvalRes {
    let sys = i.t.sys;
    let av = a.node_args();
    let bv = b.node_args();
    let a_is_list = a.is_list(&i.t);
    let b_is_list = b.is_list(&i.t);
    // vector . vector -> scalar
    if a_is_list && b_is_list {
        // matrices?
        if let (Some(am), Some(bm)) = (as_rows(a, sys), as_rows(b, sys)) {
            // matrix . matrix
            let n = am.len();
            let k = am[0].len();
            if bm.len() != k {
                i.message("Dot", "dims");
                return Ok(unevaluated_call(i, "Dot", &[a.clone(), b.clone()]));
            }
            let m = bm[0].len();
            let mut out = vec![vec![Expr::int_i64(0); m]; n];
            for r in 0..n {
                for c in 0..m {
                    let mut acc = Expr::int_i64(0);
                    for j in 0..k {
                        let t = eval_call(
                            i,
                            Expr::Sym(sys.times),
                            vec![am[r][j].clone(), bm[j][c].clone()],
                        )?;
                        out[r][c] =
                            eval_call(i, Expr::Sym(sys.plus), vec![out[r][c].clone(), t])?;
                    }
                }
            }
            return Ok(rows_expr(i, out));
        }
        // vector . matrix or matrix . vector or vector . vector
        if let Some(bm) = as_rows(b, sys) {
            // a vector . matrix
            if av.len() != bm.len() {
                i.message("Dot", "dims");
                return Ok(unevaluated_call(i, "Dot", &[a.clone(), b.clone()]));
            }
            let m = bm[0].len();
            let mut out = Vec::new();
            for c in 0..m {
                let mut acc = Expr::int_i64(0);
                for (j, x) in av.iter().enumerate() {
                    let t = eval_call(
                        i,
                        Expr::Sym(sys.times),
                        vec![x.clone(), bm[j][c].clone()],
                    )?;
                    acc = eval_call(i, Expr::Sym(sys.plus), vec![acc, t])?;
                }
                out.push(acc);
            }
            return Ok(Expr::node(Expr::Sym(sys.list), out));
        }
        // vector . vector
        if b_is_list {
            if av.len() != bv.len() {
                i.message("Dot", "dims");
                return Ok(unevaluated_call(i, "Dot", &[a.clone(), b.clone()]));
            }
            let mut acc = Expr::int_i64(0);
            for (x, y) in av.iter().zip(bv.iter()) {
                let t = eval_call(i, Expr::Sym(sys.times), vec![x.clone(), y.clone()])?;
                acc = eval_call(i, Expr::Sym(sys.plus), vec![acc, t])?;
            }
            return Ok(acc);
        }
    }
    Ok(unevaluated_call(i, "Dot", &[a.clone(), b.clone()]))
}

// ---------- Det ----------

pub fn b_det(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Det", "argr");
        return Ok(unevaluated_call(i, "Det", args));
    }
    let e = i.eval(&args[0])?;
    let sys = i.t.sys;
    if let Some(m) = rat_matrix(&e, sys) {
        let n = m.len();
        if n == 0 || m[0].len() != n {
            i.message("Det", "matsq");
            return Ok(unevaluated_call(i, "Det", args));
        }
        return Ok(Expr::rat(det_rat(&m)));
    }
    if let Some(m) = f64_matrix(&e, sys) {
        let n = m.len();
        if n == 0 || m[0].len() != n {
            i.message("Det", "matsq");
            return Ok(unevaluated_call(i, "Det", args));
        }
        return Ok(Expr::real(det_f64(&m)));
    }
    Ok(unevaluated_call(i, "Det", args))
}

fn det_rat(m: &[Vec<Rat>]) -> Rat {
    let n = m.len();
    let mut a = m.to_vec();
    let mut det = Rat::from_i64(1);
    for c in 0..n {
        let mut p = None;
        for r in c..n {
            if !a[r][c].is_zero() {
                p = Some(r);
                break;
            }
        }
        let Some(p) = p else { return Rat::from_i64(0) };
        if p != c {
            a.swap(c, p);
            det = Rat::from_i64(0).sub(&det);
        }
        det = det.mul(&a[c][c].clone());
        for r in (c + 1)..n {
            if !a[r][c].is_zero() {
                let f = a[r][c].div(&a[c][c]);
                for k in 0..n {
                    let sub = a[c][k].clone().mul(&f);
                    a[r][k] = a[r][k].sub(&sub);
                }
            }
        }
    }
    det
}

fn det_f64(m: &[Vec<f64>]) -> f64 {
    let n = m.len();
    let mut a = m.to_vec();
    let mut det = 1.0;
    for c in 0..n {
        let mut p = c;
        for r in c..n {
            if a[r][c].abs() > a[p][c].abs() {
                p = r;
            }
        }
        if a[p][c].abs() < 1e-300 {
            return 0.0;
        }
        if p != c {
            a.swap(c, p);
            det = -det;
        }
        det *= a[c][c];
        let pivot = a[c][c];
        for r in (c + 1)..n {
            let f = a[r][c] / pivot;
            for k in c..n {
                a[r][k] -= f * a[c][k];
            }
        }
    }
    det
}


/// Exact Gauss-Jordan inverse.
pub fn b_inverse(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Inverse", "argr");
        return Ok(unevaluated_call(i, "Inverse", args));
    }
    let e = i.eval(&args[0])?;
    let sys = i.t.sys;
    let Some(m) = rat_matrix(&e, sys) else {
        i.message("Inverse", "normal");
        return Ok(unevaluated_call(i, "Inverse", args));
    };
    let n = m.len();
    if n == 0 || m[0].len() != n {
        i.message("Inverse", "matsq");
        return Ok(unevaluated_call(i, "Inverse", args));
    }
    // [A | I]
    let mut a = m.clone();
    for r in 0..n {
        for c in 0..n {
            a[r].push(Rat::from_i64(if r == c { 1 } else { 0 }));
        }
    }
    for c in 0..n {
        let mut p = None;
        for r in c..n {
            if !a[r][c].is_zero() {
                p = Some(r);
                break;
            }
        }
        let Some(p) = p else {
            i.message("Inverse", "sing");
            return Ok(unevaluated_call(i, "Inverse", args));
        };
        if p != c {
            a.swap(c, p);
        }
        let piv = a[c][c].clone();
        for k in 0..2 * n {
            a[c][k] = a[c][k].div(&piv);
        }
        for r in 0..n {
            if r != c && !a[r][c].is_zero() {
                let f = a[r][c].clone();
                for k in 0..2 * n {
                    let sub = a[c][k].clone().mul(&f);
                    a[r][k] = a[r][k].sub(&sub);
                }
            }
        }
    }
    let inv: Vec<Vec<Expr>> = (0..n)
        .map(|r| (0..n).map(|c| Expr::rat(a[r][n + c].clone())).collect())
        .collect();
    Ok(rows_expr(i, inv))
}

/// RREF of a rational matrix; None if non-rational.
pub fn rref(m: Vec<Vec<Rat>>) -> Vec<Vec<Rat>> {
    let mut a = m;
    let n = a.len();
    if n == 0 {
        return a;
    }
    let m_cols = a[0].len();
    let mut lead = 0usize;
    for r in 0..n {
        if lead >= m_cols {
            break;
        }
        let mut i = r;
        while i < n && a[i][lead].is_zero() {
            i += 1;
        }
        if i == n {
            lead += 1;
            if lead >= m_cols {
                break;
            }
            continue;
        }
        if i != r {
            a.swap(i, r);
        }
        let piv = a[r][lead].clone();
        for k in 0..m_cols {
            a[r][k] = a[r][k].div(&piv);
        }
        for rr in 0..n {
            if rr != r && !a[rr][lead].is_zero() {
                let f = a[rr][lead].clone();
                for k in 0..m_cols {
                    let sub = a[r][k].clone().mul(&f);
                    a[rr][k] = a[rr][k].sub(&sub);
                }
            }
        }
        lead += 1;
    }
    a
}

pub fn b_rowreduce(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "RowReduce", args));
    }
    let e = i.eval(&args[0])?;
    let sys = i.t.sys;
    let Some(m) = rat_matrix(&e, sys) else {
        i.message("RowReduce", "normal");
        return Ok(unevaluated_call(i, "RowReduce", args));
    };
    let r: Vec<Vec<Expr>> = rref(m)
        .into_iter()
        .map(|row| row.into_iter().map(Expr::rat).collect())
        .collect();
    Ok(rows_expr(i, r))
}

pub fn b_matrixrank(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "MatrixRank", args));
    }
    let e = i.eval(&args[0])?;
    let sys = i.t.sys;
    let Some(m) = rat_matrix(&e, sys) else {
        return Ok(unevaluated_call(i, "MatrixRank", args));
    };
    let r = rref(m);
    let mut rank = 0usize;
    for row in &r {
        if row.iter().any(|c| !c.is_zero()) {
            rank += 1;
        }
    }
    Ok(Expr::int_i64(rank as i64))
}

/// Solve m.x = b (b a vector); returns None when singular.
pub fn linear_solve_rat(m: Vec<Vec<Rat>>, b: Vec<Rat>) -> Option<Vec<Rat>> {
    let n = m.len();
    if b.len() != n {
        return None;
    }
    let mut a = m;
    for r in 0..n {
        a[r].push(b[r].clone());
    }
    let r = rref(a);
    // Consistency + uniqueness: every column except last must be pivot column.
    let mut pivot_of_row: Vec<Option<usize>> = vec![None; n];
    for (ri, row) in r.iter().enumerate() {
        for (ci, c) in row.iter().enumerate() {
            if !c.is_zero() {
                if ci == row.len() - 1 && row[..ci].iter().all(|x| x.is_zero()) {
                    return None; // inconsistent
                }
                pivot_of_row[ri] = Some(ci);
                break;
            }
        }
    }
    let mut x = vec![Rat::from_i64(0); n];
    let mut seen = vec![false; n];
    for (ri, row) in r.iter().enumerate() {
        if let Some(p) = pivot_of_row[ri] {
            if p < n {
                seen[p] = true;
            }
        }
    }
    if seen.iter().any(|s| !*s) {
        return None; // underdetermined (v1: no free-parameter solutions)
    }
    for (ri, row) in r.iter().enumerate() {
        if let Some(p) = pivot_of_row[ri] {
            if p < n {
                x[p] = row[n].clone();
            }
        }
    }
    Some(x)
}

pub fn b_linearsolve(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("LinearSolve", "argr");
        return Ok(unevaluated_call(i, "LinearSolve", args));
    }
    let m = i.eval(&args[0])?;
    let b = i.eval(&args[1])?;
    let sys = i.t.sys;
    let Some(mm) = rat_matrix(&m, sys) else {
        return Ok(unevaluated_call(i, "LinearSolve", args));
    };
    let b_len = b.list_len();
    let bb: Option<Vec<Rat>> = if b.is_list(&i.t) {
        Some(b.node_args().iter().filter_map(|x| x.as_rat_exact()).collect())
    } else {
        None
    };
    if let Some(bv) = bb {
        if mm.len() != bv.len() {
            i.message("LinearSolve", "dims");
            return Ok(unevaluated_call(i, "LinearSolve", args));
        }
        match linear_solve_rat(mm, bv) {
            Some(x) => {
                let out: Vec<Expr> = x.into_iter().map(Expr::rat).collect();
                return Ok(Expr::node(Expr::Sym(sys.list), out));
            }
            None => {
                i.message("LinearSolve", "sing");
                return Ok(unevaluated_call(i, "LinearSolve", args));
            }
        }
    }
    Ok(unevaluated_call(i, "LinearSolve", args))
}

// ============================ Inverse / RowReduce ============================

pub fn b_transpose(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Transpose", args));
    }
    let e = i.eval(&args[0])?;
    let sys = i.t.sys;
    let Some(rows) = as_rows(&e, sys) else {
        i.message("Transpose", "normal");
        return Ok(unevaluated_call(i, "Transpose", args));
    };
    if rows.is_empty() {
        return Ok(e.clone());
    }
    let mut out = vec![vec![Expr::int_i64(0); rows.len()]; rows[0].len()];
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            out[c][r] = v.clone();
        }
    }
    Ok(rows_expr(i, out))
}

pub fn b_identitymatrix(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "IdentityMatrix", args));
    }
    let n = match crate::builtins::core::expr_to_int(&i.eval(&args[0])?) {
        Some(n) => n.max(0) as usize,
        None => return Ok(unevaluated_call(i, "IdentityMatrix", args)),
    };
    let mut rows = Vec::new();
    for r in 0..n {
        let mut row = Vec::new();
        for c in 0..n {
            row.push(Expr::int_i64(if r == c { 1 } else { 0 }));
        }
        rows.push(row);
    }
    Ok(rows_expr(i, rows))
}

pub fn b_diagonalmatrix(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "DiagonalMatrix", args));
    }
    let vals = i.eval(&args[0])?.node_args();
    let n = vals.len();
    let mut rows = Vec::new();
    for r in 0..n {
        let mut row = Vec::new();
        for c in 0..n {
            row.push(if r == c { vals[r].clone() } else { Expr::int_i64(0) });
        }
        rows.push(row);
    }
    Ok(rows_expr(i, rows))
}

pub fn b_matrixpower(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "MatrixPower", args));
    }
    let m = i.eval(&args[0])?;
    let n = match crate::builtins::core::expr_to_int(&i.eval(&args[1])?) {
        Some(n) => n,
        None => return Ok(unevaluated_call(i, "MatrixPower", args)),
    };
    let sys = i.t.sys;
    let Some(rows) = as_rows(&m, sys) else {
        i.message("MatrixPower", "normal");
        return Ok(unevaluated_call(i, "MatrixPower", args));
    };
    let k = rows.len();
    // Start from identity
    let mut acc: Vec<Vec<Expr>> = (0..k)
        .map(|r| (0..k).map(|c| Expr::int_i64(if r == c { 1 } else { 0 })).collect())
        .collect();
    let mut base = rows.clone();
    let mut n = n;
    while n > 0 {
        if n & 1 == 1 {
            acc = mat_mul(i, &acc, &base)?;
        }
        base = mat_mul(i, &base, &base)?;
        n >>= 1;
    }
    Ok(rows_expr(i, acc))
}

fn mat_mul(i: &mut Interp, a: &[Vec<Expr>], b: &[Vec<Expr>]) -> Result<Vec<Vec<Expr>>, Flow> {
    let sys = i.t.sys;
    let n = a.len();
    let k = a[0].len();
    let m = b[0].len();
    let mut out = vec![vec![Expr::int_i64(0); m]; n];
    for r in 0..n {
        for c in 0..m {
            let mut acc = Expr::int_i64(0);
            for j in 0..k {
                let t = eval_call(
                    i,
                    Expr::Sym(sys.times),
                    vec![a[r][j].clone(), b[j][c].clone()],
                )?;
                out[r][c] = eval_call(i, Expr::Sym(sys.plus), vec![out[r][c].clone(), t])?;
            }
        }
    }
    Ok(out)
}
// ============================ Solve ============================

/// Extract "lhs == rhs" as an expanded polynomial expression (lhs - rhs).
pub fn eq_poly(i: &mut Interp, eq: &Expr) -> Result<Option<Expr>, Flow> {
    let Some((h, hargs)) = eq.parts() else {
        return Ok(None);
    };
    if !(h.is_symbol(i.t.sys.equal) && hargs.len() == 2) {
        return Ok(None);
    }
    let l = i.eval(&hargs[0])?;
    let r = i.eval(&hargs[1])?;
    let neg = neg1_expr(i, r);
    let diff = eval_call(i, Expr::Sym(i.t.sys.plus), vec![l, neg])?;
    Ok(Some(diff))
}

pub fn b_solve(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("Solve", "argr");
        return Ok(unevaluated_call(i, "Solve", args));
    }
    let eqs_e = i.eval(&args[0])?;
    let vars_e = i.eval(&args[1])?;
    let sys = i.t.sys;
    let eqs: Vec<Expr> = match eqs_e.parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) => hargs.to_vec(),
        _ => vec![eqs_e.clone()],
    };
    let vars: Vec<Expr> = match vars_e.parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) => hargs.to_vec(),
        _ => vec![vars_e.clone()],
    };
    if vars.is_empty() {
        return Ok(Expr::node(Expr::Sym(sys.list), vec![]));
    }
    // Single equation, single variable: polynomial.
    if eqs.len() == 1 && vars.len() == 1 {
        let Some(poly) = eq_poly(i, &eqs[0])? else {
            return Ok(unevaluated_call(i, "Solve", args));
        };
        let Some(coeffs) = crate::builtins::poly::poly_coeffs(i, &poly, &vars[0])? else {
            return Ok(unevaluated_call(i, "Solve", args));
        };
        // Trim
        let mut c = coeffs;
        while c.last().map_or(false, |x| x.is_zero()) {
            c.pop();
        }
        if c.is_empty() {
            return Ok(Expr::node(Expr::Sym(sys.list), vec![]));
        }
        let deg = c.len() - 1;
        if deg == 0 {
            // No solutions.
            return Ok(Expr::node(Expr::Sym(sys.list), vec![]));
        }
        if deg == 1 {
            let root = c[0].clone().neg().div(&c[1].clone());
            let rule = rule_var_to(i, &vars[0], Expr::rat(root));
            return Ok(Expr::node(
                Expr::Sym(sys.list),
                vec![Expr::node(Expr::Sym(sys.list), vec![rule])],
            ));
        }
        if deg == 2 {
            // Quadratic formula.
            let (bq, cq) = (c[1].clone(), c[0].clone());
            let a2 = Rat::from_i64(2);
            let disc = bq.mul(&bq).sub(&c[2].mul(&c[0]).mul(&Rat::from_i64(4)));
            if disc.is_neg() {
                return Ok(unevaluated_call(i, "Solve", args));
            }
            let sq = crate::builtins::num::exact_sqrt(&disc);
            let Some(sq) = sq else {
                return Ok(unevaluated_call(i, "Solve", args));
            };
            let d2 = Rat::new(c[2].num.clone().mul(&a2.num), c[2].den.clone().mul(&a2.den));
            let r1 = bq.clone().neg().add(&sq).div(&d2);
            let r2 = bq.clone().neg().sub(&sq).div(&d2);
            let r1r = rule_var_to(i, &vars[0], Expr::rat(r1));
            let r2r = rule_var_to(i, &vars[0], Expr::rat(r2));
            let l1 = Expr::node(Expr::Sym(sys.list), vec![r1r]);
            let l2 = Expr::node(Expr::Sym(sys.list), vec![r2r]);
            return Ok(Expr::node(Expr::Sym(sys.list), vec![l1, l2]));
        }
        // Higher degree: try rational roots.
        let roots = rational_roots(&c);
        if roots.len() as i64 == deg as i64 {
            let mut out = Vec::new();
            for r in roots {
                let rule = rule_var_to(i, &vars[0], Expr::rat(r));
                out.push(Expr::node(Expr::Sym(sys.list), vec![rule]));
            }
            return Ok(Expr::node(Expr::Sym(sys.list), out));
        }
        return Ok(unevaluated_call(i, "Solve", args));
    }
    // Linear systems (all equations linear in all vars).
    let mut rows: Vec<Vec<Rat>> = Vec::new();
    for eq in &eqs {
        let Some(poly) = eq_poly(i, eq)? else {
            return Ok(unevaluated_call(i, "Solve", args));
        };
        let expanded = crate::builtins::poly::expand_expr(i, &poly)?;
        let terms: Vec<Expr> = match expanded.parts() {
            Some((h, hargs)) if h.is_symbol(sys.list) && false => vec![],
            Some((h, hargs)) if h.is_symbol(sys.plus) => hargs.to_vec(),
            _ => vec![expanded],
        };
        let mut row: Vec<Rat> = vec![Rat::from_i64(0); vars.len() + 1];
        for t in &terms {
            let (coef, deg, vi) = match t {
                Expr::Int(_) | Expr::Rat(_) => (t.as_rat_exact().unwrap(), 0usize, None),
                Expr::Node(_) if same_q(t, &vars[0]) => (Rat::from_i64(1), 1usize, Some(0usize)),
                _ => {
                    // decompose Times
                    let factors: Vec<Expr> = match t.parts() {
                        Some((h2, hargs2)) if h2.is_symbol(sys.times) => hargs2.to_vec(),
                        _ => vec![t.clone()],
                    };
                    let mut cc = Rat::from_i64(1);
                    let mut vpos: Option<usize> = None;
                    let mut deg2 = 0usize;
                    let mut ok = true;
                    for f in &factors {
                        if let Some(k) = var_index(i, f, &vars)? {
                            vpos = Some(k);
                            deg2 += 1;
                        } else if let Some(r) = f.as_rat_exact() {
                            cc = cc.mul(&r);
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if !ok || deg2 > 1 {
                        return Ok(unevaluated_call(i, "Solve", args));
                    }
                    (cc, deg2, vpos)
                }
            };
            if deg > 1 {
                return Ok(unevaluated_call(i, "Solve", args));
            }
            if let Some(k) = vi {
                row[k] = row[k].add(&coef);
            } else {
                row[vars.len()] = row[vars.len() + 1 - 1].sub(&coef);
            }
        }
        rows.push(row);
    }
    let r = rref(rows);
    // Determine pivot columns.
    let ncols = vars.len();
    let mut pivots: Vec<Option<usize>> = vec![None; r.len()];
    for (ri, row) in r.iter().enumerate() {
        for (ci, c) in row.iter().enumerate() {
            if !c.is_zero() {
                pivots[ri] = Some(ci);
                break;
            }
        }
    }
    // Inconsistent row: [0, 0, ..., 0 | k]
    for row in &r {
        if row[..ncols].iter().all(|c| c.is_zero()) && !row[ncols].is_zero() {
            return Ok(Expr::node(Expr::Sym(sys.list), vec![]));
        }
    }
    // v1: unique solutions only.
    let mut seen = vec![false; ncols];
    for p in &pivots {
        if let Some(p) = p {
            if *p < ncols {
                seen[*p] = true;
            }
        }
    }
    if seen.iter().any(|s| !*s) {
        return Ok(unevaluated_call(i, "Solve", args));
    }
    let mut out: Vec<Expr> = Vec::new();
    for (ri, row) in r.iter().enumerate() {
        if let Some(p) = pivots[ri] {
            if p < ncols {
                out.push(rule_var_to(i, &vars[p], Expr::rat(row[ncols].clone())));
            }
        }
    }
    Ok(Expr::node(
        Expr::Sym(sys.list),
        vec![Expr::node(Expr::Sym(sys.list), out)],
    ))
}

fn var_index(i: &mut Interp, e: &Expr, vars: &[Expr]) -> Result<Option<usize>, Flow> {
    for (k, v) in vars.iter().enumerate() {
        if same_q(e, v) {
            return Ok(Some(k));
        }
    }
    Ok(None)
}

fn rule_var_to(i: &mut Interp, var: &Expr, val: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.rule), vec![var.clone(), val])
}

fn neg1_expr(i: &mut Interp, e: Expr) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.times), vec![Expr::int_i64(-1), e])
}

/// Rational roots (p/q) of a trimmed Rat-coefficient polynomial.
fn rational_roots(coeffs: &[Rat]) -> Vec<Rat> {
    if coeffs.len() < 2 {
        return vec![];
    }
    let c0n = coeffs[0].num.clone().abs().to_i64().unwrap_or(1);
    let cln = coeffs[coeffs.len() - 1].num.clone().to_i64().unwrap_or(1).max(1);
    let mut out = Vec::new();
    'outer: for p in divisors64(c0n) {
        for q in divisors64(cln) {
            for s in [1i64, -1] {
                let r = Rat::new(BigInt::from_i64(s * p), BigInt::from_i64(q));
                if horner_at(coeffs, &r).is_zero() {
                    out.push(r);
                    continue 'outer;
                }
            }
        }
    }
    out
}

fn horner_at(coeffs: &[Rat], x: &Rat) -> Rat {
    let mut acc = Rat::from_i64(0);
    for c in coeffs.iter().rev() {
        acc = acc.mul(x).add(c);
    }
    acc
}

// ============================ NSolve (Durand-Kerner) ============================

#[derive(Clone, Copy)]
struct Cx(f64, f64);
impl Cx {
    fn add(self, o: Cx) -> Cx {
        Cx(self.0 + o.0, self.1 + o.1)
    }
    fn sub(self, o: Cx) -> Cx {
        Cx(self.0 - o.0, self.1 - o.1)
    }
    fn mul(self, o: Cx) -> Cx {
        Cx(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn div(self, o: Cx) -> Cx {
        let d = o.0 * o.0 + o.1 * o.1;
        Cx((self.0 * o.0 + self.1 * o.1) / d, (self.1 * o.0 - self.0 * o.1) / d)
    }
}

fn poly_cx(coeffs: &[Cx], x: Cx) -> Cx {
    let mut acc = Cx(0.0, 0.0);
    for c in coeffs.iter().rev() {
        acc = acc.mul(x).add(*c);
    }
    acc
}

pub fn b_nsolve(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("NSolve", "argr");
        return Ok(unevaluated_call(i, "NSolve", args));
    }
    let eqs_e = i.eval(&args[0])?;
    let vars_e = i.eval(&args[1])?;
    let sys = i.t.sys;
    let var_e = match vars_e.parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) && hargs.len() == 1 => hargs[0].clone(),
        _ => vars_e.clone(),
    };
    let Some(var) = var_e.sym_id() else {
        return Ok(unevaluated_call(i, "NSolve", args));
    };
    let Some(poly) = eq_poly(i, &eqs_e)? else {
        return Ok(unevaluated_call(i, "NSolve", args));
    };
    let expanded = crate::builtins::poly::expand_expr(i, &poly)?;
    let sys2 = i.t.sys;
    let terms: Vec<Expr> = match expanded.parts() {
        Some((h2, a2)) if h2.is_symbol(sys2.plus) => a2.to_vec(),
        _ => vec![expanded],
    };
    let mut coeffs: Vec<Cx> = Vec::new();
    for t in &terms {
        let mut coeff = 1.0f64;
        let mut deg = 0i64;
        let mut ok = true;
        let factors: Vec<Expr> = match t.parts() {
            Some((h2, hargs2)) if h2.is_symbol(sys2.times) => hargs2.to_vec(),
            _ => vec![t.clone()],
        };
        for f in &factors {
            if f.is_symbol(var) {
                deg += 1;
            } else if let Some((h3, a3)) = f.parts() {
                if h3.is_symbol(sys2.power) && a3.len() == 2 && a3[0].is_symbol(var) {
                    match a3[1].to_i64_pub() {
                        Some(n) => deg += n,
                        None => {
                            ok = false;
                        }
                    }
                } else if let Some(n) = num_val(f) {
                    coeff *= nv_f64(&n);
                } else {
                    ok = false;
                }
            } else if let Some(n) = num_val(f) {
                coeff *= nv_f64(&n);
            } else {
                ok = false;
            }
        }
        if !ok {
            return Ok(unevaluated_call(i, "NSolve", args));
        }
        if coeffs.len() <= deg as usize {
            coeffs.resize(deg as usize + 1, Cx(0.0, 0.0));
        }
        coeffs[deg as usize].0 += coeff;
    }
    while coeffs.last().map_or(false, |c| c.0 == 0.0 && c.1 == 0.0) {
        coeffs.pop();
    }
    if coeffs.len() < 2 {
        return Ok(Expr::node(Expr::Sym(sys.list), vec![]));
    }
    let lead = coeffs[coeffs.len() - 1];
    let monic: Vec<Cx> = coeffs.iter().map(|c| c.div(lead)).collect();
    let n = monic.len() - 1;
    let mut roots: Vec<Cx> = Vec::new();
    let mut seed = Cx(0.4, 0.9);
    for _ in 0..n {
        roots.push(seed);
        seed = seed.mul(Cx(0.9, 0.4));
    }
    for _ in 0..200 {
        for k in 0..n {
            let num = poly_cx(&monic, roots[k]);
            let mut den = Cx(1.0, 0.0);
            for j in 0..n {
                if j != k {
                    den = den.mul(roots[k].sub(roots[j]));
                }
            }
            roots[k] = roots[k].sub(num.div(den));
        }
    }
    let mut out: Vec<Expr> = Vec::new();
    for r in roots {
        if r.1.abs() < 1e-9 {
            out.push(Expr::real(r.0));
        } else {
            out.push(Expr::cnum(
                crate::expr::NumVal::F(r.0),
                crate::expr::NumVal::F(r.1),
            ));
        }
    }
    let t = &i.t;
    out.sort_by(|a, b| crate::expr::canon_cmp(t, a, b));
    Ok(Expr::node(Expr::Sym(sys.list), out))
}
