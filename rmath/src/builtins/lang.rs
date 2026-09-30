//! Everyday language surface added in the hardening pass: Piecewise, string
//! splitting/replacement/matching, association access, and common list
//! primitives (Array, ConstantArray, Partition, Accumulate, Differences,
//! Ordering, TakeWhile, DeleteCases, Riffle).

use crate::eval::{EvalRes, Interp};
use crate::expr::{same_q, Expr};

fn list(i: &Interp, v: Vec<Expr>) -> Expr {
    Expr::node(Expr::Sym(i.t.sys.list), v)
}
fn bool_e(i: &Interp, b: bool) -> Expr {
    Expr::Sym(if b { i.t.sys.r#true } else { i.t.sys.r#false })
}
fn unev(i: &mut Interp, name: &str, args: &[Expr]) -> EvalRes {
    Ok(crate::builtins::core::unevaluated_call(i, name, args))
}
fn as_int(e: &Expr) -> Option<i64> {
    crate::builtins::core::expr_to_int(e)
}

// ============================ Piecewise ============================

pub fn b_piecewise(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        return unev(i, "Piecewise", args);
    }
    let Some((h, pairs)) = args[0].parts() else {
        return unev(i, "Piecewise", args);
    };
    if !h.is_symbol(i.t.sys.list) {
        return unev(i, "Piecewise", args);
    }
    for pair in pairs {
        let Some((ph, pa)) = pair.parts() else {
            return unev(i, "Piecewise", args);
        };
        if !ph.is_symbol(i.t.sys.list) || pa.len() != 2 {
            return unev(i, "Piecewise", args);
        }
        let cond = i.eval(&pa[1])?;
        if cond.is_symbol(i.t.sys.r#true) {
            return i.eval(&pa[0]);
        }
        if !cond.is_symbol(i.t.sys.r#false) {
            // An undecided condition: leave the whole Piecewise alone.
            return unev(i, "Piecewise", args);
        }
    }
    if args.len() > 1 {
        return i.eval(&args[1]);
    }
    Ok(Expr::int_i64(0))
}

// ============================ strings ============================

pub fn b_stringsplit(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let Expr::Str(s) = &args[0] else {
        return unev(i, "StringSplit", args);
    };
    let parts: Vec<String> = if args.len() == 1 {
        s.split_whitespace().map(String::from).collect()
    } else {
        match &args[1] {
            Expr::Str(sep) => s.split(sep.as_ref()).map(String::from).collect(),
            _ => return unev(i, "StringSplit", args),
        }
    };
    let out: Vec<Expr> = parts.iter().map(|p| Expr::from_str(p)).collect();
    Ok(list(i, out))
}

pub fn b_stringreplace(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let Expr::Str(s0) = &args[0] else {
        return unev(i, "StringReplace", args);
    };
    if args.len() != 2 {
        return unev(i, "StringReplace", args);
    }
    // One rule, or a list of rules applied in order.
    let rules: Vec<Expr> = match args[1].parts() {
        Some((h, rs)) if h.is_symbol(i.t.sys.list) => rs.to_vec(),
        _ => vec![args[1].clone()],
    };
    let mut s = s0.to_string();
    for r in &rules {
        let Some((rh, ra)) = r.parts() else {
            return unev(i, "StringReplace", args);
        };
        if !(rh.is_symbol(i.t.sys.rule) || rh.is_symbol(i.t.sys.ruledelayed)) || ra.len() != 2 {
            return unev(i, "StringReplace", args);
        }
        let (Expr::Str(from), Expr::Str(to)) = (&ra[0], &ra[1]) else {
            return unev(i, "StringReplace", args);
        };
        s = s.replace(from.as_ref(), to.as_ref());
    }
    Ok(Expr::from_str(&s))
}

fn glob_match(pat: &[char], s: &[char]) -> bool {
    let (mut p, mut si) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while si < s.len() {
        if p < pat.len() && (pat[p] == '?' || pat[p] == s[si]) {
            p += 1;
            si += 1;
        } else if p < pat.len() && pat[p] == '*' {
            star = Some(p);
            p += 1;
            mark = si;
        } else if let Some(st) = star {
            p = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == '*' {
        p += 1;
    }
    p == pat.len()
}

pub fn b_stringmatchq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return unev(i, "StringMatchQ", args);
    }
    let (Expr::Str(s), Expr::Str(p)) = (&args[0], &args[1]) else {
        return unev(i, "StringMatchQ", args);
    };
    let sc: Vec<char> = s.chars().collect();
    let pc: Vec<char> = p.chars().collect();
    Ok(bool_e(i, glob_match(&pc, &sc)))
}

pub fn b_stringcontainsq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return unev(i, "StringContainsQ", args);
    }
    let (Expr::Str(s), Expr::Str(sub)) = (&args[0], &args[1]) else {
        return unev(i, "StringContainsQ", args);
    };
    // A pattern containing * or ? is matched as a glob.
    let hit = if sub.contains('*') || sub.contains('?') {
        let sc: Vec<char> = s.chars().collect();
        let pc: Vec<char> = sub.chars().collect();
        glob_match(&pc, &sc)
    } else {
        s.contains(sub.as_ref())
    };
    Ok(bool_e(i, hit))
}

// ============================ associations ============================

fn assoc_rules<'a>(i: &Interp, e: &'a Expr) -> Option<&'a [Expr]> {
    let (h, args) = e.parts()?;
    if h.is_symbol(i.t.sys.association) {
        Some(args)
    } else {
        None
    }
}

fn rule_kv<'a>(i: &Interp, r: &'a Expr) -> Option<(&'a Expr, &'a Expr)> {
    let (h, a) = r.parts()?;
    if (h.is_symbol(i.t.sys.rule) || h.is_symbol(i.t.sys.ruledelayed)) && a.len() == 2 {
        Some((&a[0], &a[1]))
    } else {
        None
    }
}

pub fn b_keys(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let Some(rules) = assoc_rules(i, &args[0]) else {
        return unev(i, "Keys", args);
    };
    let out: Vec<Expr> = rules
        .iter()
        .filter_map(|r| rule_kv(i, r).map(|(k, _)| k.clone()))
        .collect();
    Ok(list(i, out))
}

pub fn b_values(i: &mut Interp, args: &[Expr]) -> EvalRes {
    let Some(rules) = assoc_rules(i, &args[0]) else {
        return unev(i, "Values", args);
    };
    let out: Vec<Expr> = rules
        .iter()
        .filter_map(|r| rule_kv(i, r).map(|(_, v)| v.clone()))
        .collect();
    Ok(list(i, out))
}

pub fn b_keyexistsq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return unev(i, "KeyExistsQ", args);
    }
    let Some(rules) = assoc_rules(i, &args[0]) else {
        return unev(i, "KeyExistsQ", args);
    };
    let found = rules
        .iter()
        .any(|r| rule_kv(i, r).map_or(false, |(k, _)| same_q(k, &args[1])));
    Ok(bool_e(i, found))
}

pub fn b_lookup(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 || args.len() > 3 {
        return unev(i, "Lookup", args);
    }
    let Some(rules) = assoc_rules(i, &args[0]) else {
        return unev(i, "Lookup", args);
    };
    for r in rules {
        if let Some((k, v)) = rule_kv(i, r) {
            if same_q(k, &args[1]) {
                return Ok(v.clone());
            }
        }
    }
    if args.len() == 3 {
        return Ok(args[2].clone());
    }
    // Missing["KeyAbsent", key]
    let missing = Expr::Sym(i.t.sys.missing);
    let absent = Expr::from_str("KeyAbsent");
    Ok(Expr::node(missing, vec![absent, args[1].clone()]))
}

// ============================ list primitives ============================

pub fn b_constantarray(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return unev(i, "ConstantArray", args);
    }
    let Some(n) = as_int(&args[1]) else {
        return unev(i, "ConstantArray", args);
    };
    let n = n.max(0) as usize;
    Ok(list(i, vec![args[0].clone(); n]))
}

pub fn b_array(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 || args.len() > 3 {
        return unev(i, "Array", args);
    }
    let Some(n) = as_int(&args[1]) else {
        return unev(i, "Array", args);
    };
    let start = if args.len() == 3 {
        match as_int(&args[2]) {
            Some(a) => a,
            None => return unev(i, "Array", args),
        }
    } else {
        1
    };
    let mut out = Vec::new();
    for k in 0..n.max(0) {
        let call = Expr::node(args[0].clone(), vec![Expr::int_i64(start + k)]);
        out.push(i.eval(&call)?);
    }
    Ok(list(i, out))
}

pub fn b_partition(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 || args.len() > 3 {
        return unev(i, "Partition", args);
    }
    let Some(n) = as_int(&args[1]) else {
        return unev(i, "Partition", args);
    };
    let n = n.max(1) as usize;
    let step = if args.len() == 3 {
        match as_int(&args[2]) {
            Some(s) if s >= 1 => s as usize,
            _ => return unev(i, "Partition", args),
        }
    } else {
        n
    };
    let v = args[0].node_args();
    let mut out = Vec::new();
    let mut k = 0usize;
    while k + n <= v.len() {
        out.push(list(i, v[k..k + n].to_vec()));
        k += step;
    }
    Ok(list(i, out))
}

fn fold_arith(i: &mut Interp, a: Expr, b: Expr, minus: bool) -> EvalRes {
    let sys = i.t.sys;
    if minus {
        let nb = Expr::node(Expr::Sym(sys.times), vec![Expr::int_i64(-1), b]);
        i.eval(&Expr::node(Expr::Sym(sys.plus), vec![a, nb]))
    } else {
        i.eval(&Expr::node(Expr::Sym(sys.plus), vec![a, b]))
    }
}

pub fn b_accumulate(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return unev(i, "Accumulate", args);
    }
    let v = args[0].node_args();
    let mut out = Vec::new();
    let mut acc = Expr::int_i64(0);
    for x in v {
        acc = fold_arith(i, acc, x, false)?;
        out.push(acc.clone());
    }
    Ok(list(i, out))
}

pub fn b_differences(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return unev(i, "Differences", args);
    }
    let v = args[0].node_args();
    let mut out = Vec::new();
    for w in v.windows(2) {
        out.push(fold_arith(i, w[1].clone(), w[0].clone(), true)?);
    }
    Ok(list(i, out))
}

pub fn b_ordering(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return unev(i, "Ordering", args);
    }
    let v = args[0].node_args();
    let mut idx: Vec<usize> = (0..v.len()).collect();
    let t = &i.t;
    idx.sort_by(|&a, &b| crate::eval::sort_cmp(t, &v[a], &v[b]));
    let out: Vec<Expr> = idx.iter().map(|k| Expr::int_i64((*k + 1) as i64)).collect();
    Ok(list(i, out))
}

pub fn b_takewhile(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return unev(i, "TakeWhile", args);
    }
    let v = args[0].node_args();
    let mut out = Vec::new();
    for x in v {
        let call = Expr::node(args[1].clone(), vec![x.clone()]);
        let c = i.eval(&call)?;
        if c.is_symbol(i.t.sys.r#true) {
            out.push(x);
        } else {
            break;
        }
    }
    Ok(list(i, out))
}

pub fn b_deletecases(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return unev(i, "DeleteCases", args);
    }
    let v = args[0].node_args();
    let mut out = Vec::new();
    for x in v {
        if !crate::pattern::match_top(i, &args[1], &x) {
            out.push(x);
        }
    }
    Ok(list(i, out))
}

pub fn b_riffle(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 2 {
        return unev(i, "Riffle", args);
    }
    let v = args[0].node_args();
    let sep = args[1].clone();
    let mut out = Vec::new();
    for (k, x) in v.into_iter().enumerate() {
        if k > 0 {
            out.push(sep.clone());
        }
        out.push(x);
    }
    Ok(list(i, out))
}
