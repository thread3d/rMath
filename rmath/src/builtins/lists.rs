//! List, string, and system built-ins.

use crate::builtins::core::{expr_to_int, unevaluated_call};
use crate::builtins::num::num_val;
use crate::builtins::num::nv_f64;
use crate::eval::{sort_cmp, EvalRes, Interp};
use crate::expr::{canon_cmp, same_q, Expr};
use std::cmp::Ordering;
use std::time::Instant;

pub fn bool_e(i: &crate::eval::Interp, v: bool) -> Expr {
    Expr::Sym(if v { i.t.sys.r#true } else { i.t.sys.r#false })
}

// ---------- comparisons ----------

pub fn b_less(i: &mut Interp, args: &[Expr]) -> EvalRes {
    cmp_chain(i, args, |o| o == Ordering::Less, "Less")
}
pub fn b_greater(i: &mut Interp, args: &[Expr]) -> EvalRes {
    cmp_chain(i, args, |o| o == Ordering::Greater, "Greater")
}
pub fn b_lessequal(i: &mut Interp, args: &[Expr]) -> EvalRes {
    cmp_chain(i, args, |o| o != Ordering::Greater, "LessEqual")
}
pub fn b_greaterequal(i: &mut Interp, args: &[Expr]) -> EvalRes {
    cmp_chain(i, args, |o| o != Ordering::Less, "GreaterEqual")
}

/// Compare two values: exactly when either is a BigReal, else at machine
/// precision. None when they are not both numeric.
fn cmp_pair(i: &Interp, a: &Expr, b: &Expr) -> Option<Ordering> {
    if let Some(o) = crate::builtins::num::big_cmp(&i.t, a, b) {
        return Some(o);
    }
    nv_f64(&num_val(a)?).partial_cmp(&nv_f64(&num_val(b)?))
}

fn cmp_chain(i: &mut Interp, args: &[Expr], ok: fn(Ordering) -> bool, name: &str) -> EvalRes {
    for w in args.windows(2) {
        match cmp_pair(i, &w[0], &w[1]) {
            Some(o) => {
                if !ok(o) {
                    return Ok(bool_e(i, false));
                }
            }
            None => return Ok(unevaluated_call(i, name, args)),
        }
    }
    Ok(bool_e(i, true))
}

/// Ordering of two values by numeric comparison, `Some(Equal)` when they are
/// the very same expression, `None` when neither applies (so the caller must
/// leave the relation unevaluated).
fn cmp_three_way(i: &Interp, a: &Expr, b: &Expr) -> Option<Ordering> {
    if let Some(o) = cmp_pair(i, a, b) {
        return Some(o);
    }
    if same_q(a, b) {
        return Some(Ordering::Equal);
    }
    None
}

/// `Equal[a, b, c]` is the chain `a == b == c`, as in Mathematica.
pub fn b_equal(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        return Ok(unevaluated_call(i, "Equal", args));
    }
    let mut unknown = false;
    for w in args.windows(2) {
        match cmp_three_way(i, &w[0], &w[1]) {
            Some(Ordering::Equal) => {}
            Some(_) => return Ok(bool_e(i, false)),
            None => unknown = true,
        }
    }
    if unknown {
        Ok(unevaluated_call(i, "Equal", args))
    } else {
        Ok(bool_e(i, true))
    }
}

/// `Unequal[a, b, c]` is true when the values are pairwise different.
pub fn b_unequal(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        return Ok(unevaluated_call(i, "Unequal", args));
    }
    let mut unknown = false;
    for a in 0..args.len() {
        for b in (a + 1)..args.len() {
            match cmp_three_way(i, &args[a], &args[b]) {
                Some(Ordering::Equal) => return Ok(bool_e(i, false)),
                Some(_) => {}
                None => unknown = true,
            }
        }
    }
    if unknown {
        Ok(unevaluated_call(i, "Unequal", args))
    } else {
        Ok(bool_e(i, true))
    }
}

pub fn b_not(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Not", "argr");
        return Ok(unevaluated_call(i, "Not", args));
    }
    let v = i.eval(&args[0])?;
    if v.is_symbol(i.t.sys.r#true) {
        return Ok(Expr::Sym(i.t.sys.r#false));
    }
    if v.is_symbol(i.t.sys.r#false) {
        return Ok(Expr::Sym(i.t.sys.r#true));
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.not), vec![v]))
}

pub fn b_and(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let sys = i.t.sys;
    let mut unevaluated: Vec<Expr> = Vec::new();
    for a in args {
        let v = i.eval(a)?;
        if v.is_symbol(sys.r#false) {
            return Ok(Expr::Sym(sys.r#false));
        }
        if !v.is_symbol(sys.r#true) {
            unevaluated.push(v);
        }
    }
    if unevaluated.is_empty() {
        return Ok(Expr::Sym(sys.r#true));
    }
    Ok(Expr::node(Expr::Sym(sys.and), unevaluated))
}

pub fn b_or(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let sys = i.t.sys;
    let mut unevaluated: Vec<Expr> = Vec::new();
    for a in args {
        let v = i.eval(a)?;
        if v.is_symbol(sys.r#true) {
            return Ok(Expr::Sym(sys.r#true));
        }
        if !v.is_symbol(sys.r#false) {
            unevaluated.push(v);
        }
    }
    if unevaluated.is_empty() {
        return Ok(Expr::Sym(sys.r#false));
    }
    Ok(Expr::node(Expr::Sym(sys.or), unevaluated))
}

pub fn b_boole(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = i.eval(&args[0])?;
    if v.is_symbol(i.t.sys.r#true) {
        return Ok(Expr::int_i64(1));
    }
    if v.is_symbol(i.t.sys.r#false) {
        return Ok(Expr::int_i64(0));
    }
    Ok(unevaluated_call(i, "Boole", args))
}

pub fn b_trueq(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(bool_e(_i, args[0].is_symbol(_i.t.sys.r#true)))
}

// ---------- primes ----------

pub fn b_primeq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "PrimeQ", args));
    }
    match &args[0] {
        Expr::Int(n) => match is_prime_big(n) {
            Some(p) => Ok(bool_e(i, p)),
            // Never guess: a wrong `False` is worse than no answer, so an
            // integer beyond the deterministic test stays unevaluated.
            None => {
                i.message("PrimeQ", "bigint");
                Ok(unevaluated_call(i, "PrimeQ", args))
            }
        },
        // Non-integer numbers are simply not prime (`PrimeQ[1/2]` is False).
        Expr::Real(_) | Expr::Rat(_) | Expr::Cnum(_) | Expr::BigReal(_) => Ok(bool_e(i, false)),
        // Anything symbolic stays unevaluated, as in Mathematica.
        _ => Ok(unevaluated_call(i, "PrimeQ", args)),
    }
}

pub fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    if n % 2 == 0 {
        return n == 2;
    }
    for p in [3u64, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n % p == 0 {
            return n == p;
        }
    }
    let mut d = n - 1;
    let mut r = 0u32;
    while d % 2 == 0 {
        d /= 2;
        r += 1;
    }
    'outer: for a in [2u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if a >= n {
            continue;
        }
        let mut x = mod_pow(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 0..r.saturating_sub(1) {
            x = ((x as u128 * x as u128) % n as u128) as u64;
            if x == n - 1 {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

/// `Some(is_prime)` when the deterministic Miller-Rabin test applies,
/// `None` when the integer is too large for it (the caller must then leave the
/// expression unevaluated rather than answer `False`, which would be a lie).
pub fn is_prime_big(n: &crate::bigint::BigInt) -> Option<bool> {
    if n.sign < 0 {
        return Some(false);
    }
    n.to_u64_pub().map(is_prime)
}

pub fn b_prime(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Prime", args));
    }
    let n = match expr_to_int(&args[0]) {
        Some(n) => n,
        None => return Ok(unevaluated_call(i, "Prime", args)),
    };
    if n < 1 || n > 200_000 {
        i.message("Prime", "argr");
        return Ok(unevaluated_call(i, "Prime", args));
    }
    let mut count = 0i64;
    let mut k: u64 = 2;
    loop {
        if is_prime(k) {
            count += 1;
            if count == n {
                return Ok(Expr::from_u64(k));
            }
        }
        k += 1;
    }
}

fn mod_pow(b: u64, e: u64, m: u64) -> u64 {
    // Small modpow in u128 space (enough for MR with u64 modulus).
    if m == 1 {
        return 0;
    }
    let mut result: u128 = 1;
    let mut base: u128 = b as u128 % m as u128;
    let mut e = e;
    let mm = m as u128;
    while e > 0 {
        if e & 1 == 1 {
            result = result * base % mm;
        }
        base = base * base % mm;
        e >>= 1;
    }
    result as u64
}



// ---------- mapping / application ----------

pub fn b_map(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Map", "argr");
        return Ok(unevaluated_call(i, "Map", args));
    }
    let f = &args[0];
    let list = i.eval(&args[1])?;
    let mut out = Vec::new();
    for x in list.node_args() {
        let call = Expr::node(f.clone(), vec![x]);
        out.push(i.eval(&call)?);
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_apply(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Apply", "argr");
        return Ok(unevaluated_call(i, "Apply", args));
    }
    let f = &args[0];
    let e = i.eval(&args[1])?;
    // Apply[f, expr, {1}] — map over elements
    if args.len() > 2 {
        let lvl = i.eval(&args[2])?;
        let is_lvl1 = match &lvl {
            Expr::Node(nd) => {
                nd.head.is_symbol(i.t.sys.list)
                    && nd.args.len() == 1
                    && crate::expr::same_q(&nd.args[0], &Expr::int_i64(1))
            }
            _ => false,
        };
        if is_lvl1 {
            let mut out = Vec::new();
            for x in e.node_args() {
                let call = Expr::node(f.clone(), x.node_args());
                out.push(i.eval(&call)?);
            }
            return Ok(Expr::node(Expr::Sym(i.t.sys.list), out));
        }
    }
    let call = Expr::node(f.clone(), e.node_args());
    i.eval(&call)
}

pub fn b_mapall(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("MapAll", "argr");
        return Ok(unevaluated_call(i, "MapAll", args));
    }
    let f = args[0].clone();
    let e = i.eval(&args[1])?;
    let out = map_all(i, &f, &e)?;
    let call = Expr::node(f, vec![out]);
    i.eval(&call)
}

fn map_all(i: &mut Interp, f: &Expr, e: &Expr) -> EvalRes {
    if let Some((h, args)) = e.parts() {
        let nh = map_all(i, f, h)?;
        let mut na = Vec::new();
        for a in args {
            na.push(map_all(i, f, a)?);
        }
        return Ok(Expr::node(nh, na));
    }
    Ok(e.clone())
}

pub fn b_mapthread(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("MapThread", "argr");
        return Ok(unevaluated_call(i, "MapThread", args));
    }
    let f = args[0].clone();
    let lists = i.eval(&args[1])?;
    let mut cols: Vec<Vec<Expr>> = Vec::new();
    for l in lists.node_args() {
        cols.push(l.node_args());
    }
    if cols.is_empty() {
        return Ok(Expr::node(Expr::Sym(i.t.sys.list), vec![]));
    }
    let n = cols[0].len();
    if cols.iter().any(|c| c.len() != n) {
        i.message("MapThread", "dims");
        return Ok(unevaluated_call(i, "MapThread", args));
    }
    let mut out = Vec::new();
    for k in 0..n {
        let mut one = Vec::new();
        for c in &cols {
            one.push(c[k].clone());
        }
        let call = Expr::node(f.clone(), one);
        out.push(i.eval(&call)?);
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_thread(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Thread", args));
    }
    let e = i.eval(&args[0])?;
    let Some((h, hargs)) = e.parts() else {
        return Ok(e.clone());
    };
    // Thread over the first List argument.
    let mut list_pos: Option<usize> = None;
    let mut n = 0usize;
    for (k, a) in hargs.iter().enumerate() {
        if a.is_list(&i.t) {
            list_pos = Some(k);
            n = a.list_len();
            break;
        }
    }
    let Some(p) = list_pos else {
        return Ok(e.clone());
    };
    let mut out = Vec::new();
    for k in 0..n {
        let mut one = Vec::new();
        for (j, a) in hargs.iter().enumerate() {
            if j == p {
                one.push(a.node_args()[k].clone());
            } else {
                one.push(a.clone());
            }
        }
        out.push(Expr::node(h.clone(), one));
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

// ---------- iterators ----------

pub fn b_table(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Table", "argr");
        return Ok(unevaluated_call(i, "Table", args));
    }
    let Some(iters) = crate::builtins::core::build_iters(i, &args[1..])? else {
        i.message("Table", "itform");
        return Ok(unevaluated_call(i, "Table", args));
    };
    table_rec(i, &iters, &mut Vec::new(), &args[0])
}

fn table_rec(i: &mut Interp, iters: &[crate::builtins::core::Iter], env: &mut Vec<(usize, Expr)>, body: &Expr) -> EvalRes {
    if iters.is_empty() {
        let names: Vec<usize> = env.iter().map(|(k, _)| *k).collect();
        let vals: Vec<Expr> = env.iter().map(|(_, v)| v.clone()).collect();
        let sub = crate::eval::subst_syms(&i.t, body, &names, &vals);
        return i.eval(&sub);
    }
    let it = &iters[0];
    let mut out = Vec::new();
    match it {
        crate::builtins::core::Iter::Count(n) => {
            for _ in 0..*n {
                out.push(table_rec(i, &iters[1..], env, body)?);
            }
        }
        crate::builtins::core::Iter::Range { var, lo, hi, step } => {
            if *step > 0 {
                let mut k = *lo;
                while k <= *hi {
                    env.push((*var, Expr::int_i64(k)));
                    out.push(table_rec(i, &iters[1..], env, body)?);
                    env.pop();
                    k += *step;
                }
            } else if *step < 0 {
                let mut k = *lo;
                while k >= *hi {
                    env.push((*var, Expr::int_i64(k)));
                    out.push(table_rec(i, &iters[1..], env, body)?);
                    env.pop();
                    k += *step;
                }
            }
        }
        crate::builtins::core::Iter::List { var, vals } => {
            for v in vals {
                env.push((*var, v.clone()));
                out.push(table_rec(i, &iters[1..], env, body)?);
                env.pop();
            }
        }
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_sum(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() == 2 {
        if let Some(v) =
            crate::builtins::sums::symbolic_sum_product(i, &args[0], &args[1], true)?
        {
            return Ok(v);
        }
    }
    iter_accumulate(i, args, "Sum", true)
}

pub fn b_product(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() == 2 {
        if let Some(v) =
            crate::builtins::sums::symbolic_sum_product(i, &args[0], &args[1], false)?
        {
            return Ok(v);
        }
    }
    iter_accumulate(i, args, "Product", false)
}

fn iter_accumulate(i: &mut Interp, args: &[Expr], name: &str, plus: bool) -> EvalRes {
    if args.len() < 2 {
        i.message(name, "argr");
        return Ok(unevaluated_call(i, name, args));
    }
    let Some(iters) = crate::builtins::core::build_iters(i, &args[1..])? else {
        // Symbolic or malformed bounds with no closed form: stay unevaluated.
        return Ok(unevaluated_call(i, name, args));
    };
    let mut env: Vec<(usize, Expr)> = Vec::new();
    let mut acc = if plus { Expr::int_i64(0) } else { Expr::int_i64(1) };
    let head = if plus { i.t.sys.plus } else { i.t.sys.times };
    fn sink_fn(plus: bool, head: usize, acc: &mut Expr, i: &mut Interp, sub: &Expr) -> Result<(), crate::eval::Flow> {
        let v = i.eval(sub)?;
        *acc = i.eval(&Expr::node(Expr::Sym(head), vec![acc.clone(), v]))?;
        let _ = plus;
        Ok(())
    }
    struct SinkCtx { acc: Expr, head: usize, _p: bool }
    let mut sc = SinkCtx { acc, head, _p: plus };
    let _ = crate::builtins::core::run_iters(i, &iters, &mut env, &args[0], &mut |i: &mut Interp, sub: &Expr| {
        sink_fn(plus, sc.head, &mut sc.acc, i, sub)
    });
    Ok(sc.acc)
}

// ---------- predicates / pattern helpers ----------

pub fn b_sameq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(bool_e(i, args.len() == 2 && same_q(&args[0], &args[1])))
}

pub fn b_unsameq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(bool_e(i, !(args.len() == 2 && same_q(&args[0], &args[1]))))
}

pub fn b_integerq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(bool_e(i, matches!(args[0], Expr::Int(_))))
}

pub fn b_evenq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = match &args[0] {
        Expr::Int(n) => n.is_even(),
        _ => false,
    };
    Ok(bool_e(i, v))
}

pub fn b_oddq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = match &args[0] {
        Expr::Int(n) => n.is_odd(),
        _ => false,
    };
    Ok(bool_e(i, v))
}

pub fn b_numberq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(bool_e(i, args[0].is_number()))
}

pub fn b_numericq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(bool_e(i, crate::builtins::num::is_numeric_expr(i, &args[0])))
}

pub fn b_listq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(bool_e(i, args[0].is_list(&i.t)))
}

pub fn b_atomq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(bool_e(i, args[0].is_atom()))
}

pub fn b_head(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(args[0].head_of(&i.t))
}

pub fn b_length(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = i.eval(&args[0])?;
    Ok(Expr::int_i64(e.list_len() as i64))
}

pub fn b_matchq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        i.message("MatchQ", "argr");
        return Ok(unevaluated_call(i, "MatchQ", args));
    }
    let e = i.eval(&args[0])?;
    let m = crate::pattern::match_top(i, &args[1], &e);
    Ok(bool_e(i, m))
}

pub fn b_freeq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let e = i.eval(&args[0])?;
    let found = free_q(i, &e, &args[1]);
    Ok(bool_e(i, !found))
}

fn free_q(i: &mut Interp, e: &Expr, pat: &Expr) -> bool {
    if crate::pattern::match_top(i, pat, e) {
        return true;
    }
    if let Some((_, args)) = e.parts() {
        for a in args {
            if free_q(i, a, pat) {
                return true;
            }
        }
    }
    false
}

pub fn b_memberq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "MemberQ", args));
    }
    let list = i.eval(&args[0])?;
    for x in list.node_args() {
        if crate::pattern::match_top(i, &args[1], &x) {
            return Ok(bool_e(i, true));
        }
    }
    Ok(bool_e(i, false))
}

pub fn b_cases(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Cases", args));
    }
    let list = i.eval(&args[0])?;
    let mut out = Vec::new();
    for x in list.node_args() {
        if crate::pattern::match_top(i, &args[1], &x) {
            out.push(x);
        }
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_select(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Select", args));
    }
    let list = i.eval(&args[0])?;
    let mut out = Vec::new();
    for x in list.node_args() {
        let call = Expr::node(args[1].clone(), vec![x.clone()]);
        let v = i.eval(&call)?;
        if v.is_symbol(i.t.sys.r#true) {
            out.push(x);
        }
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_count(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Count", args));
    }
    let list = i.eval(&args[0])?;
    let mut n = 0i64;
    for x in list.node_args() {
        if crate::pattern::match_top(i, &args[1], &x) {
            n += 1;
        }
    }
    Ok(Expr::int_i64(n))
}

pub fn b_position(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "Position", args));
    }
    let e = i.eval(&args[0])?;
    let mut out: Vec<Expr> = Vec::new();
    walk_positions(i, &e, &args[1], &mut Vec::new(), &mut out);
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

fn walk_positions(i: &mut Interp, e: &Expr, pat: &Expr, path: &mut Vec<Expr>, out: &mut Vec<Expr>) {
    if crate::pattern::match_top(i, pat, e) {
        out.push(Expr::node(Expr::Sym(i.t.sys.list), path.clone()));
    }
    if let Some((_, args)) = e.parts() {
        for (k, a) in args.iter().enumerate() {
            path.push(Expr::int_i64((k + 1) as i64));
            walk_positions(i, a, pat, path, out);
            path.pop();
        }
    }
}

// ---------- ordering & structure ----------

pub fn b_sort(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Sort", args));
    }
    let list = i.eval(&args[0])?;
    let mut v: Vec<Expr> = list.node_args();
    let t = &i.t;
    v.sort_by(|a, b| sort_cmp(t, a, b));
    Ok(Expr::node(Expr::Sym(i.t.sys.list), v))
}

pub fn b_sortby(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "SortBy", args));
    }
    let mut v: Vec<Expr> = args[0].node_args();
    let mut keyed: Vec<(Expr, Expr)> = Vec::new();
    for x in &v {
        let call = Expr::node(args[1].clone(), vec![x.clone()]);
        keyed.push((i.eval(&call)?, x.clone()));
    }
    let t = &i.t;
    keyed.sort_by(|a, b| sort_cmp(t, &a.0, &b.0));
    let out: Vec<Expr> = keyed.into_iter().map(|(_, x)| x).collect();
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_reverse(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut v = args[0].node_args();
    v.reverse();
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), v))
}

pub fn b_join(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut out = Vec::new();
    for a in args {
        out.extend(a.node_args());
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_append(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut v = args[0].node_args();
    v.push(args[1].clone());
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), v))
}

pub fn b_prepend(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut v = args[0].node_args();
    v.insert(0, args[1].clone());
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), v))
}

pub fn b_appendto(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "AppendTo", args));
    }
    let var = match args[0].sym_id() {
        Some(id) => id,
        None => {
            i.message("AppendTo", "normal");
            return Ok(unevaluated_call(i, "AppendTo", args));
        }
    };
    let cur = i.t.data[var].own.clone();
    let cur = match cur {
        Some(crate::expr::OwnValue::Set(v)) => v,
        _ => {
            i.message("AppendTo", "normal");
            return Ok(unevaluated_call(i, "AppendTo", args));
        }
    };
    let mut v = cur.node_args();
    v.push(args[1].clone());
    let newl = Expr::node(Expr::Sym(i.t.sys.list), v);
    i.t.data[var].own = Some(crate::expr::OwnValue::Set(newl.clone()));
    Ok(newl)
}

pub fn b_first(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = args[0].node_args();
    match v.first() {
        Some(x) => Ok(x.clone()),
        None => {
            i.message("First", "normal");
            Ok(unevaluated_call(i, "First", args))
        }
    }
}

pub fn b_last(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = args[0].node_args();
    match v.last() {
        Some(x) => Ok(x.clone()),
        None => {
            i.message("Last", "normal");
            Ok(unevaluated_call(i, "Last", args))
        }
    }
}

pub fn b_most(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut v = args[0].node_args();
    v.pop();
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), v))
}

pub fn b_rest(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = args[0].node_args();
    let d = if v.is_empty() { v } else { v[1..].to_vec() };
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), d))
}

pub fn b_take(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = args[0].node_args();
    let n = expr_to_int(&args[1]).unwrap_or(0);
    let out = if n >= 0 {
        v[..(n as usize).min(v.len())].to_vec()
    } else {
        v[v.len().saturating_sub((-n) as usize)..].to_vec()
    };
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), out))
}

pub fn b_drop(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = args[0].node_args();
    let n = expr_to_int(&args[1]).unwrap_or(0);
    let out = if n >= 0 {
        v[(n as usize).min(v.len())..].to_vec()
    } else {
        v[..v.len().saturating_sub((-n) as usize)].to_vec()
    };
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), out))
}

pub fn b_flatten(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let out = flatten_all(args[0].node_args(), i.t.sys.list);
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

fn flatten_all(v: Vec<Expr>, sys_list: usize) -> Vec<Expr> {
    let mut out = Vec::new();
    for x in v {
        if let Some((h, args)) = x.parts() {
            if h.is_symbol(sys_list) {
                out.extend(flatten_all(args.to_vec(), sys_list));
            } else {
                out.push(x.clone());
            }
        } else {
            out.push(x);
        }
    }
    out
}

pub fn b_union(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut all = Vec::new();
    for a in args {
        all.extend(a.node_args());
    }
    let t = &i.t;
    all.sort_by(|a, b| canon_cmp(t, a, b));
    all.dedup_by(|a, b| same_q(a, b));
    Ok(Expr::node(Expr::Sym(i.t.sys.list), all))
}

pub fn b_intersection(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut a = args[0].node_args();
    let mut b = args[1].node_args();
    let t = &i.t;
    a.sort_by(|x, y| canon_cmp(t, x, y));
    b.sort_by(|x, y| canon_cmp(t, x, y));
    let mut out = Vec::new();
    for x in a {
        if b.iter().any(|y| same_q(&x, y)) && !out.iter().any(|y| same_q(&x, y)) {
            out.push(x);
        }
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_complement(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let all = args[0].node_args();
    let remove = args[1].node_args();
    let mut out = Vec::new();
    for x in all {
        if !remove.iter().any(|y| same_q(&x, y)) && !out.iter().any(|y| same_q(&x, y)) {
            out.push(x);
        }
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub fn b_deleteduplicates(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = args[0].node_args();
    let mut out: Vec<Expr> = Vec::new();
    for x in v {
        if !out.iter().any(|y| same_q(&x, y)) {
            out.push(x);
        }
    }
    Ok(Expr::node(Expr::Sym(_i.t.sys.list), out))
}

pub fn b_identity(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(args[0].clone())
}

pub fn b_valueq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let id = match args[0].sym_id() {
        Some(id) => id,
        None => return Ok(bool_e(i, false)),
    };
    Ok(bool_e(i, i.t.data[id].own.is_some()))
}

// ---------- strings ----------

pub fn b_stringjoin(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    let mut s = String::new();
    for a in args {
        match a {
            Expr::Str(v) => s.push_str(v),
            _ => return Ok(unevaluated_call(_i, "StringJoin", args)),
        }
    }
    Ok(Expr::from_str(&s))
}

pub fn b_stringlength(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    match &args[0] {
        Expr::Str(v) => Ok(Expr::int_i64(v.chars().count() as i64)),
        _ => Ok(unevaluated_call(_i, "StringLength", args)),
    }
}

pub fn b_stringtake(i: &mut Interp, args: &[Expr]) -> EvalRes {
    string_take_drop(i, args, true)
}

pub fn b_stringdrop(i: &mut Interp, args: &[Expr]) -> EvalRes {
    string_take_drop(i, args, false)
}

fn string_take_drop(i: &mut Interp, args: &[Expr], take: bool) -> EvalRes {
    let s = match &args[0] {
        Expr::Str(v) => v.clone(),
        _ => return Ok(unevaluated_call(i, if take { "StringTake" } else { "StringDrop" }, args)),
    };
    let chars: Vec<char> = s.chars().collect();
    let (lo, hi) = match &args[1] {
        Expr::Int(n) => {
            let k = n.to_i64().unwrap_or(0);
            if k >= 0 {
                (0usize, (k as usize).min(chars.len()))
            } else {
                (chars.len().saturating_sub((-k) as usize), chars.len())
            }
        }
        Expr::Node(nd) if nd.head.is_symbol(i.t.sys.list) && nd.args.len() == 2 => {
            let a = expr_to_int(&nd.args[0]).unwrap_or(1);
            let b = expr_to_int(&nd.args[1]).unwrap_or(0);
            (
                (a.max(1) - 1) as usize,
                (b.min(chars.len() as i64).max(0)) as usize,
            )
        }
        _ => return Ok(unevaluated_call(i, if take { "StringTake" } else { "StringDrop" }, args)),
    };
    let sel: String = if take {
        chars.get(lo..hi).unwrap_or(&[]).iter().collect()
    } else {
        let mut o = String::new();
        for (k, c) in chars.iter().enumerate() {
            if k < lo || k >= hi {
                o.push(*c);
            }
        }
        o
    };
    Ok(Expr::from_str(&sel))
}

pub fn b_characters(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    match &args[0] {
        Expr::Str(v) => {
            let out: Vec<Expr> = v.chars().map(|c| Expr::from_str(&c.to_string())).collect();
            Ok(Expr::node(Expr::Sym(_i.t.sys.list), out))
        }
        _ => Ok(unevaluated_call(_i, "Characters", args)),
    }
}

pub fn b_touppercase(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    match &args[0] {
        Expr::Str(v) => Ok(Expr::from_str(&v.to_uppercase())),
        _ => Ok(unevaluated_call(_i, "ToUpperCase", args)),
    }
}

pub fn b_tolowercase(_i: &mut Interp, args: &[Expr]) -> EvalRes {
    match &args[0] {
        Expr::Str(v) => Ok(Expr::from_str(&v.to_lowercase())),
        _ => Ok(unevaluated_call(_i, "ToLowerCase", args)),
    }
}

pub fn b_tostring(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(Expr::from_str(&crate::fmt::input_form(&i.t, &args[0])))
}

pub fn b_toexpression(i: &mut Interp, args: &[Expr]) -> EvalRes {
    match &args[0] {
        Expr::Str(v) => {
            match crate::parser::parse_input(&mut i.t, v) {
                Ok(e) => i.eval(&e),
                Err(pe) => {
                    i.message("ToExpression", "syntax");
                    Ok(unevaluated_call(i, "ToExpression", args))
                }
            }
        }
        _ => Ok(unevaluated_call(i, "ToExpression", args)),
    }
}

// ---------- system ----------

pub fn b_fullform(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(Expr::from_str(&crate::fmt::fullform(&i.t, &args[0])))
}

pub fn b_inputform(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(Expr::from_str(&crate::fmt::input_form(&i.t, &args[0])))
}

pub fn b_texform(i: &mut Interp, args: &[Expr]) -> EvalRes {
    Ok(Expr::from_str(&crate::tex::tex_form(&i.t, &args[0])))
}

pub fn b_attributes(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "Attributes", args));
    }
    let Some(id) = args[0].sym_id() else {
        return Ok(Expr::node(Expr::Sym(i.t.sys.list), vec![]));
    };
    let a = i.t.data[id].attrs;
    let mut out = Vec::new();
    for (bit, name) in ATTR_TABLE {
        if a & bit != 0 {
            out.push(Expr::Sym(i.t.intern(name)));
        }
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

pub const ATTR_TABLE: &[(u32, &str)] = &[
    (crate::expr::A_LISTABLE, "Listable"),
    (crate::expr::A_ORDERLESS, "Orderless"),
    (crate::expr::A_FLAT, "Flat"),
    (crate::expr::A_ONEIDENTITY, "OneIdentity"),
    (crate::expr::A_HOLDALL, "HoldAll"),
    (crate::expr::A_HOLDFIRST, "HoldFirst"),
    (crate::expr::A_HOLDREST, "HoldRest"),
    (crate::expr::A_HOLDALLCOMPLETE, "HoldAllComplete"),
    (crate::expr::A_SEQUENCEHOLD, "SequenceHold"),
    (crate::expr::A_PROTECTED, "Protected"),
    (crate::expr::A_CONSTANT, "Constant"),
    (crate::expr::A_NUMERICFUNCTION, "NumericFunction"),
];

pub fn b_setattributes(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "SetAttributes", args));
    }
    let Some(id) = args[0].sym_id() else {
        i.message("SetAttributes", "normal");
        return Ok(args[0].clone());
    };
    let mut bits = 0u32;
    let list = match args[1].parts() {
        Some((h, hargs)) if h.is_symbol(i.t.sys.list) => hargs.to_vec(),
        _ => vec![args[1].clone()],
    };
    for a in &list {
        if let Expr::Sym(nid) = a {
            let name = i.t.name_of(*nid);
            for (bit, n) in ATTR_TABLE {
                if *n == name {
                    bits |= bit;
                }
            }
        }
    }
    i.t.data[id].attrs |= bits;
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_clearattributes(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return Ok(unevaluated_call(i, "ClearAttributes", args));
    }
    let Some(id) = args[0].sym_id() else {
        return Ok(Expr::Sym(i.t.sys.null));
    };
    let list = match args[1].parts() {
        Some((h, hargs)) if h.is_symbol(i.t.sys.list) => hargs.to_vec(),
        _ => vec![args[1].clone()],
    };
    for a in &list {
        if let Expr::Sym(nid) = a {
            let name = i.t.name_of(*nid).to_string();
            for (bit, n) in ATTR_TABLE {
                if *n == name {
                    i.t.data[id].attrs &= !bit;
                }
            }
        }
    }
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_options(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if let Some(id) = args[0].sym_id() {
        let opts = i.t.data[id].options.clone();
        return Ok(Expr::node(Expr::Sym(i.t.sys.list), opts));
    }
    Ok(Expr::node(Expr::Sym(i.t.sys.list), vec![]))
}

pub fn b_definition(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if let Some(id) = args[0].sym_id() {
        let name = i.t.name_of(id).to_string();
        let mut lines: Vec<String> = Vec::new();
        if let Some(own) = &i.t.data[id].own {
            let body = match own {
                crate::expr::OwnValue::Set(v) => v.clone(),
                crate::expr::OwnValue::SetDelayed(v) => v.clone(),
            };
            lines.push(format!("{} = {}", name, crate::fmt::output_form(&i.t, &body)));
        }
        for r in &i.t.data[id].down {
            lines.push(format!(
                "{}[...] {} {}",
                name,
                if r.delayed { ":=" } else { "=" },
                crate::fmt::output_form(&i.t, &r.body)
            ));
        }
        return Ok(Expr::from_str(&lines.join("\n")));
    }
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_names(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let pat = match &args[0] {
        Expr::Str(s) => s.clone(),
        _ => return Ok(Expr::node(Expr::Sym(i.t.sys.list), vec![])),
    };
    let (prefix, suffix) = match pat.split_once('*') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        
        None => (pat.to_string(), String::new()),
    };
    let mut names: Vec<String> = i
        .t
        .data
        .iter()
        .map(|d| d.name.clone())
        .filter(|n| n.starts_with(&prefix) && n.ends_with(&*suffix))
        .collect();
    names.sort();
    let out: Vec<Expr> = names.into_iter().map(|n| Expr::from_str(&n)).collect();
    Ok(Expr::node(Expr::Sym(i.t.sys.list), out))
}

// ---------- random ----------

fn next_rng(i: &mut Interp) -> u64 {
    let mut x = i.rng;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    i.rng = x;
    x
}

pub fn b_seedrandom(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if let Some(n) = expr_to_int(&args[0]) {
        i.rng = n as u64 | 1;
    }
    Ok(Expr::Sym(i.t.sys.null))
}

pub fn b_randominteger(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        return Ok(Expr::int_i64(0));
    }
    let e = i.eval(&args[0])?;
    let (lo, hi) = match &e {
        Expr::Node(nd) if nd.head.is_symbol(i.t.sys.list) && nd.args.len() == 2 => {
            match (expr_to_int(&nd.args[0]), expr_to_int(&nd.args[1])) {
                (Some(a), Some(b)) => (a, b),
                _ => return Ok(unevaluated_call(i, "RandomInteger", args)),
            }
        }
        other => match expr_to_int(other) {
            Some(n) => (0, n.max(0)),
            _ => return Ok(unevaluated_call(i, "RandomInteger", args)),
        },
    };
    let span = (hi - lo + 1).max(1);
    let v = lo + (next_rng(i) % (span as u64)) as i64;
    Ok(Expr::int_i64(v))
}

pub fn b_randomreal(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let (lo, hi) = if args.is_empty() {
        (0.0, 1.0)
    } else {
        let e = i.eval(&args[0])?;
        match &e {
            Expr::Node(nd) if nd.head.is_symbol(i.t.sys.list) && nd.args.len() == 2 => {
                match (&nd.args[0], &nd.args[1]) {
                    (Expr::Real(a), Expr::Real(b)) => (*a, *b),
                    (Expr::Real(a), Expr::Int(b)) => (*a, expr_to_int(&Expr::Int(b.clone())).unwrap_or(0) as f64),
                    (Expr::Int(a), Expr::Real(b)) => (expr_to_int(&Expr::Int(a.clone())).unwrap_or(0) as f64, *b),
                    (Expr::Int(a), Expr::Int(b)) => (expr_to_int(&Expr::Int(a.clone())).unwrap_or(0) as f64, expr_to_int(&Expr::Int(b.clone())).unwrap_or(0) as f64),
                    _ => return Ok(unevaluated_call(i, "RandomReal", args)),
                }
            }
            Expr::Real(v) => (0.0, *v),
            Expr::Int(v) => (0.0, expr_to_int(&Expr::Int(v.clone())).unwrap_or(0) as f64),
            _ => return Ok(unevaluated_call(i, "RandomReal", args)),
        }
    };
    let u = (next_rng(i) >> 11) as f64 / (1u64 << 53) as f64;
    Ok(Expr::real(lo + u * (hi - lo)))
}

pub fn b_randomchoice(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let v = args[0].node_args();
    if v.is_empty() {
        return Ok(unevaluated_call(i, "RandomChoice", args));
    }
    let k = (next_rng(i) as usize) % v.len();
    Ok(v[k].clone())
}

pub fn b_absolutetiming(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let start = Instant::now();
    let v = i.eval(&args[0])?;
    let secs = start.elapsed().as_secs_f64();
    Ok(Expr::node(
        Expr::Sym(i.t.sys.list),
        vec![Expr::real(secs), v],
    ))
}

pub fn b_quit(_i: &mut Interp, _args: &[Expr]) -> EvalRes {
    Err(crate::eval::Flow::Halt("Quit".to_string()))
}