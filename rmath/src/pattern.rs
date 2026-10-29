//! Pattern matcher: Blank/BlankSequence/BlankNullSequence, Pattern,
//! Alternatives, Condition, PatternTest, Repeated/RepeatedNull,
//! Optional with defaults, Except, Verbatim, HoldPattern; attribute-aware
//! matching for Orderless and Flat heads (Plus, Times, ...).

use crate::expr::{has, same_q, Expr, SymbolTable, Sys, A_FLAT, A_ORDERLESS};
use std::collections::HashMap;

/// Values bound to pattern names: a single expression or a sequence.
#[derive(Clone, Debug)]
pub enum Bound {
    One(Expr),
    Many(Vec<Expr>),
}

#[derive(Clone, Debug, Default)]
pub struct Bindings {
    pub map: HashMap<usize, Bound>,
}

impl Bindings {
    pub fn get_one(&self, id: usize) -> Option<Expr> {
        match self.map.get(&id) {
            Some(Bound::One(e)) => Some(e.clone()),
            Some(Bound::Many(v)) if v.len() == 1 => Some(v[0].clone()),
            _ => None,
        }
    }
    pub fn get_many(&self, id: usize) -> Option<Vec<Expr>> {
        match self.map.get(&id) {
            Some(Bound::Many(v)) => Some(v.clone()),
            Some(Bound::One(e)) => Some(vec![e.clone()]),
            None => None,
        }
    }
    pub fn is_bound(&self, id: usize) -> bool {
        self.map.contains_key(&id)
    }
    fn bind(&mut self, id: usize, val: Bound) -> bool {
        match self.map.get(&id) {
            None => {
                self.map.insert(id, val);
                true
            }
            Some(prev) => match (prev, &val) {
                (Bound::One(a), Bound::One(b)) => same_q(a, b),
                (Bound::Many(a), Bound::Many(b)) => {
                    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| same_q(x, y))
                }
                (Bound::Many(v), Bound::One(e)) => v.len() == 1 && same_q(&v[0], e),
                (Bound::One(e), Bound::Many(v)) => v.len() == 1 && same_q(e, &v[0]),
            },
        }
    }
}

/// Callbacks the matcher needs from the evaluator (avoids module cycles).
pub trait MatchCtx {
    /// Evaluate `test[val]`; report whether the result is True.
    fn eval_test(&mut self, test: &Expr, val: &Expr) -> Result<bool, ()>;
    /// Evaluate a condition with bindings substituted; True?
    fn eval_cond(&mut self, cond: &Expr, b: &Bindings) -> Result<bool, ()>;
    /// Evaluate an expression (Optional defaults).
    fn eval_plain(&mut self, e: &Expr) -> Result<Expr, ()>;
    /// Head symbol id of an expression (type symbol for atoms).
    fn head_sym(&mut self, e: &Expr) -> usize;
    /// Attributes of a symbol.
    fn attrs_of(&mut self, id: usize) -> u32;
    /// Canonical ordering of two expressions.
    fn canon(&mut self, a: &Expr, b: &Expr) -> std::cmp::Ordering;
    /// Default value for `x_.` under this head (Default[f], position pos).
    fn default_for(&mut self, head: Option<usize>, pos: usize) -> Option<Expr>;
    /// Rational/Complex atoms presented as nodes for matching.
    fn atom_as_node_parts(&mut self, e: &Expr) -> Option<(Expr, Vec<Expr>)>;
    /// System symbol id table (Copy).
    fn sys_syms(&mut self) -> Sys;
}

/// Match a single expression against a pattern.
pub fn match_top(ctx: &mut dyn MatchCtx, pat: &Expr, e: &Expr) -> bool {
    let sys = ctx.sys_syms();
    let mut b = Bindings::default();
    let mut m = Matcher { ctx, sys };
    m.pat(pat, e, &mut b, false)
}

/// Match a whole expression against a pattern, returning the bindings so the
/// caller can substitute a rule body (used for up-values).
///
/// When both sides are calls of the same head, the arguments are matched the way
/// a down-value of that head would be — attribute-aware, so an Orderless head
/// (or a Flat one) permutes and folds. `f /: Plus[f[a_], f[b_]] := ...` only fires
/// on `f[3] + f[4]` because of this.
pub fn match_top_bindings(ctx: &mut dyn MatchCtx, pat: &Expr, e: &Expr) -> Option<Bindings> {
    if let (Some((ph, pargs)), Some((eh, eargs))) = (pat.parts(), e.parts()) {
        if same_q(ph, eh) {
            if let Some(id) = eh.sym_id() {
                return match_call_args(ctx, id, pargs, eargs);
            }
        }
    }
    let sys = ctx.sys_syms();
    let mut b = Bindings::default();
    let mut m = Matcher { ctx, sys };
    if m.pat(pat, e, &mut b, false) {
        Some(b)
    } else {
        None
    }
}

/// Match argument list `args` against pattern args for a call of `head_id`
/// (attribute-aware: Orderless permutation, Flat grouping).
pub fn match_call_args(
    ctx: &mut dyn MatchCtx,
    head_id: usize,
    pats: &[Expr],
    args: &[Expr],
) -> Option<Bindings> {
    let attrs = ctx.attrs_of(head_id);
    let sys = ctx.sys_syms();
    let mut b = Bindings::default();
    let mut m = Matcher { ctx, sys };
    if m.seq(pats, args, attrs, Some(head_id), &mut b) {
        Some(b)
    } else {
        None
    }
}

/// Syntactically substitute bound pattern names into an expression
/// (no evaluation; bound symbols and Pattern nodes become their values).
pub fn subst_bindings(sys: &Sys, e: &Expr, b: &Bindings) -> Expr {
    if let Expr::Sym(id) = e {
        if b.is_bound(*id) {
            if let Some(v) = b.get_one(*id) {
                return v;
            }
            if let Some(vs) = b.get_many(*id) {
                if vs.len() == 1 {
                    return vs[0].clone();
                }
                return Expr::node(Expr::Sym(sys.sequence), vs);
            }
        }
        return e.clone();
    }
    if let Some((h, args)) = e.parts() {
        if h.is_symbol(sys.pattern) && args.len() == 2 {
            if let Expr::Sym(id) = &args[0] {
                if let Some(v) = b.get_one(*id) {
                    return v;
                }
                if let Some(vs) = b.get_many(*id) {
                    if vs.len() == 1 {
                        return vs[0].clone();
                    }
                    return Expr::node(Expr::Sym(sys.sequence), vs);
                }
            }
        }
        let new_head = subst_bindings(sys, h, b);
        let new_args: Vec<Expr> = args.iter().map(|a| subst_bindings(sys, a, b)).collect();
        return Expr::node(new_head, new_args);
    }
    e.clone()
}

/// Rename symbols syntactically (Module lexical renaming).
pub fn rename_syms(t: &SymbolTable, e: &Expr, renames: &[(usize, usize)]) -> Expr {
    if renames.is_empty() {
        return e.clone();
    }
    if let Expr::Sym(id) = e {
        for (from, to) in renames {
            if *from == *id {
                return Expr::Sym(*to);
            }
        }
        return e.clone();
    }
    if let Some((h, args)) = e.parts() {
        let new_head = rename_syms(t, h, renames);
        let new_args: Vec<Expr> = args.iter().map(|a| rename_syms(t, a, renames)).collect();
        return Expr::node(new_head, new_args);
    }
    e.clone()
}

/// Does a rule lhs contain any blank/pattern variables?
pub fn lhs_has_blanks(lhs: &Expr, t: &SymbolTable) -> bool {
    let sys = t.sys;
    if let Some((h, args)) = lhs.parts() {
        if h.is_symbol(sys.blank)
            || h.is_symbol(sys.blanksequence)
            || h.is_symbol(sys.blanknullsequence)
        {
            return true;
        }
        if h.is_symbol(sys.pattern) && !args.is_empty() {
            return true;
        }
        return args.iter().any(|a| lhs_has_blanks(a, t));
    }
    false
}

/// Alias with a self-describing name for the same check.
pub fn lhs_has_blanks_pub(lhs: &Expr, t: &SymbolTable) -> bool {
    lhs_has_blanks(lhs, t)
}

/// Whether the lhs is pattern-free (a literal-argument rule).
pub fn lhs_has_blanks_alias(lhs: &Expr, t: &SymbolTable) -> bool {
    lhs_has_blanks(lhs, t)
}

/// Substitution alias used by With.
pub fn subst_syms_all(
    t: &SymbolTable,
    body: &Expr,
    names: &[usize],
    vals: &[Expr],
) -> Expr {
    crate::eval::subst_syms(t, body, names, vals)
}

/// Extract (lhs, rhs, delayed) triples from a rules expression.
pub fn collect_rules(rules: &Expr, sys: &Sys) -> Option<Vec<(Expr, Expr, bool)>> {
    if let Some((h, args)) = rules.parts() {
        if h.is_symbol(sys.rule) && args.len() == 2 {
            return Some(vec![(args[0].clone(), args[1].clone(), false)]);
        }
        if h.is_symbol(sys.ruledelayed) && args.len() == 2 {
            return Some(vec![(args[0].clone(), args[1].clone(), true)]);
        }
        if h.is_symbol(sys.list) {
            let mut out = Vec::new();
            for a in args {
                out.extend(collect_rules(a, sys)?);
            }
            return Some(out);
        }
    }
    None
}

/// Replace at every level, bottom-up (children before parents).
pub fn replace_all(
    ctx: &mut dyn MatchCtx,
    e: &Expr,
    rules: &Expr,
) -> Result<(Expr, bool), ()> {
    let sys = ctx.sys_syms();
    let Some(rs) = collect_rules(rules, &sys) else {
        return Err(());
    };
    replace_walk(ctx, e, &rs)
}

fn replace_walk(
    ctx: &mut dyn MatchCtx,
    e: &Expr,
    rules: &[(Expr, Expr, bool)],
) -> Result<(Expr, bool), ()> {
    // Children first.
    if let Expr::Node(nd) = e {
        let (new_head, hch) = replace_walk(ctx, &nd.head, rules)?;
        let mut new_args = Vec::with_capacity(nd.args.len());
        let mut ach = false;
        for a in &nd.args {
            let (a2, c) = replace_walk(ctx, a, rules)?;
            ach |= c;
            new_args.push(a2);
        }
        let cur = Expr::node(new_head, new_args);
        if let Some(v) = try_rules_at(ctx, &cur, rules)? {
            return Ok((v, true));
        }
        return Ok((cur, hch || ach));
    }
    if let Some(v) = try_rules_at(ctx, e, rules)? {
        return Ok((v, true));
    }
    Ok((e.clone(), false))
}

/// Replace only at the top level (Replace[expr, rules]).
pub fn replace_at_top(
    ctx: &mut dyn MatchCtx,
    e: &Expr,
    rules: &Expr,
) -> Result<(Expr, bool), ()> {
    let sys = ctx.sys_syms();
    let Some(rs) = collect_rules(rules, &sys) else {
        return Err(());
    };
    if let Some(v) = try_rules_at(ctx, e, &rs)? {
        return Ok((v, true));
    }
    Ok((e.clone(), false))
}

fn try_rules_at(
    ctx: &mut dyn MatchCtx,
    e: &Expr,
    rules: &[(Expr, Expr, bool)],
) -> Result<Option<Expr>, ()> {
    let sys = ctx.sys_syms();
    for (lhs, rhs, delayed) in rules {
        let mut b = Bindings::default();
        let mut m = MatchTop2 { ctx, sys };
        if !m.pat(lhs, e, &mut b, false) {
            continue;
        }
        if *delayed {
            let sub = subst_bindings(&sys, rhs, &b);
            let v = ctx.eval_plain(&sub)?;
            return Ok(Some(v));
        } else {
            // Rule: rhs was evaluated at construction; substitute pattern names.
            let v = subst_bindings(&sys, rhs, &b);
            return Ok(Some(v));
        }
    }
    Ok(None)
}

/// Thin adapter exposing Matcher.pat to the replacement machinery.
struct MatchTop2<'a> {
    ctx: &'a mut dyn MatchCtx,
    sys: Sys,
}

impl<'a> MatchTop2<'a> {
    fn pat(&mut self, pat: &Expr, e: &Expr, b: &mut Bindings, verbatim: bool) -> bool {
        let mut m = Matcher { ctx: self.ctx, sys: self.sys };
        m.pat(pat, e, b, verbatim)
    }
}

// ---------- classification of pattern elements in sequence position ----------

enum PatKind {
    /// BlankSequence[head?]
    BlankSeq(Option<usize>),
    /// BlankNullSequence[head?]
    BlankNull(Option<usize>),
    /// Repeated[p] / RepeatedNull[p]
    Repeated(Expr, bool),
    /// Pattern[name, seq-ish]
    PatternSeq(usize, Expr),
    /// Optional[p, d?]
    Optional(Expr, Option<Expr>),
    Other,
}

fn head_arg(args: &[Expr]) -> Option<usize> {
    args.first().and_then(|a| a.sym_id())
}

fn classify(p: &Expr, sys: &Sys) -> PatKind {
    if let Some((h, args)) = p.parts() {
        if h.is_symbol(sys.blanksequence) {
            return PatKind::BlankSeq(head_arg(args));
        }
        if h.is_symbol(sys.blanknullsequence) {
            return PatKind::BlankNull(head_arg(args));
        }
        if h.is_symbol(sys.repeated) && args.len() >= 1 {
            return PatKind::Repeated(args[0].clone(), false);
        }
        if h.is_symbol(sys.repeatednull) && args.len() >= 1 {
            return PatKind::Repeated(args[0].clone(), true);
        }
        if h.is_symbol(sys.pattern) && args.len() == 2 {
            if let Expr::Sym(name) = &args[0] {
                let inner = &args[1];
                if let Some((h2, _)) = inner.parts() {
                    if h2.is_symbol(sys.blanksequence)
                        || h2.is_symbol(sys.blanknullsequence)
                        || h2.is_symbol(sys.repeated)
                        || h2.is_symbol(sys.repeatednull)
                    {
                        return PatKind::PatternSeq(*name, inner.clone());
                    }
                }
                // fall through: Pattern with a single-value inner
                if let Some((h2, a2)) = inner.parts() {
                    if h2.is_symbol(sys.optional) {
                        // Pattern[name, Optional[p, d]]: bind name when default
                        // fills; otherwise normal binding happens via pat().
                        if let Some(d) = a2.get(1) {
                            return PatKind::Optional(inner.clone(), Some(d.clone()));
                        }
                        return PatKind::Optional(inner.clone(), None);
                    }
                }
                return PatKind::Other;
            }
            return PatKind::Other;
        }
        if h.is_symbol(sys.optional) && args.len() >= 1 {
            let d = args.get(1).cloned();
            return PatKind::Optional(args[0].clone(), d);
        }
    }
    PatKind::Other
}

// ---------- the matcher ----------

struct Matcher<'a> {
    ctx: &'a mut dyn MatchCtx,
    sys: Sys,
}

impl<'a> Matcher<'a> {
    fn pat(&mut self, pat: &Expr, e: &Expr, b: &mut Bindings, verbatim: bool) -> bool {
        if let Some((h, args)) = pat.parts() {
            // Verbatim[p]: literal structural match
            if h.is_symbol(self.sys.verbatim) && args.len() == 1 {
                return same_q(&args[0], e);
            }
            // Alternatives
            if h.is_symbol(self.sys.alternatives) && !args.is_empty() {
                for alt in args {
                    let mut b2 = b.clone();
                    if self.pat(alt, e, &mut b2, false) {
                        *b = b2;
                        return true;
                    }
                }
                return false;
            }
            // Condition[p, cond]
            if h.is_symbol(self.sys.condition) && args.len() == 2 {
                let mut b2 = b.clone();
                if !self.pat(&args[0], e, &mut b2, false) {
                    return false;
                }
                match self.ctx.eval_cond(&args[1], &b2) {
                    Ok(true) => {
                        *b = b2;
                        return true;
                    }
                    _ => return false,
                }
            }
            // PatternTest[p, test]
            if h.is_symbol(self.sys.patterntest) && args.len() == 2 {
                let mut b2 = b.clone();
                if !self.pat(&args[0], e, &mut b2, false) {
                    return false;
                }
                match self.ctx.eval_test(&args[1], e) {
                    Ok(true) => {
                        *b = b2;
                        return true;
                    }
                    _ => return false,
                }
            }
            // Except[p] / Except[p, q]
            if h.is_symbol(self.sys.except) {
                return match args.len() {
                    1 => {
                        let mut b2 = b.clone();
                        !self.pat(&args[0], e, &mut b2, false)
                    }
                    2 => {
                        let mut b2 = b.clone();
                        if self.pat(&args[0], e, &mut b2, false) {
                            false
                        } else {
                            self.pat(&args[1], e, b, false)
                        }
                    }
                    _ => false,
                };
            }
            // HoldPattern[p]
            if (h.is_symbol(self.sys.holdpattern) || h.is_symbol(self.sys.holdcomplete))
                && args.len() == 1
            {
                return self.pat(&args[0], e, b, false);
            }
            // Pattern[name, p]
            if h.is_symbol(self.sys.pattern) && args.len() == 2 {
                if let Expr::Sym(name) = &args[0] {
                    let mut b2 = b.clone();
                    if !self.pat(&args[1], e, &mut b2, false) {
                        return false;
                    }
                    let val = Bound::One(e.clone());
                    if !b2.bind(*name, val) {
                        return false;
                    }
                    *b = b2;
                    return true;
                }
                return false;
            }
            // Optional[p, d] at a single position: match p.
            if h.is_symbol(self.sys.optional) && !args.is_empty() {
                return self.pat(&args[0], e, b, false);
            }
        }
        if verbatim {
            return false;
        }
        // Blank[] matches anything; Blank[head] checks the head.
        if let Some((h, args)) = pat.parts() {
            if h.is_symbol(self.sys.blank) {
                if args.is_empty() {
                    return true;
                }
                if let Some(head) = head_arg(args) {
                    return self.ctx.head_sym(e) == head;
                }
                return true;
            }
            if h.is_symbol(self.sys.blanksequence) || h.is_symbol(self.sys.blanknullsequence) {
                return false; // only valid in argument sequences
            }
            if h.is_symbol(self.sys.repeated) || h.is_symbol(self.sys.repeatednull) {
                return false;
            }
        }
        if same_q(pat, e) {
            return true;
        }
        self.pat_node(pat, e, b)
    }

    /// Node-vs-node structural matching (heads + args), with atom-as-node
    /// promotion for Rational/Complex, plus Orderless/Flat attribute logic.
    fn pat_node(&mut self, pat: &Expr, e: &Expr, b: &mut Bindings) -> bool {
        let (ph, pargs) = match pat.parts() {
            Some(x) => (x.0.clone(), x.1.to_vec()),
            None => return false,
        };
        // Promote atoms to node form when the pattern has their head.
        let (eh, eargs): (Expr, Vec<Expr>) = match e {
            Expr::Node(nd) => (nd.head.clone(), nd.args.clone()),
            other => match self.ctx.atom_as_node_parts(other) {
                Some((h, a)) => (h, a),
                None => return false,
            },
        };
        let head_ok = match &ph {
            Expr::Sym(_) => same_q(&ph, &eh),
            _ => self.pat(&ph, &eh, b, false),
        };
        if !head_ok {
            return false;
        }
        let head_sym = eh.sym_id();
        let attrs = match head_sym {
            Some(id) => self.ctx.attrs_of(id),
            None => 0,
        };
        let mut b2 = b.clone();
        if self.seq(&pargs, &eargs, attrs, head_sym, &mut b2) {
            *b = b2;
            true
        } else {
            false
        }
    }

    /// Match pattern args against argument values (attribute-aware).
    fn seq(
        &mut self,
        pats: &[Expr],
        args: &[Expr],
        attrs: u32,
        head_id: Option<usize>,
        b: &mut Bindings,
    ) -> bool {
        if has(attrs, A_ORDERLESS) && pats.len() >= 2 && pats.len() <= 6 {
            let idx: Vec<usize> = (0..pats.len()).collect();
            for perm in permutations(&idx) {
                let permuted: Vec<Expr> = perm.iter().map(|&i| pats[i].clone()).collect();
                let mut b2 = b.clone();
                if self.seq_ordered(&permuted, args, attrs, head_id, &mut b2) {
                    *b = b2;
                    return true;
                }
            }
            return false;
        }
        if has(attrs, A_ORDERLESS) && pats.len() > 6 {
            let mut sorted: Vec<Expr> = pats.to_vec();
            sorted.sort_by(|x, y| self.ctx.canon(x, y));
            let mut b2 = b.clone();
            if self.seq_ordered(&sorted, args, attrs, head_id, &mut b2) {
                *b = b2;
                return true;
            }
            return false;
        }
        self.seq_ordered(pats, args, attrs, head_id, b)
    }

    fn seq_ordered(
        &mut self,
        pats: &[Expr],
        args: &[Expr],
        attrs: u32,
        head_id: Option<usize>,
        b: &mut Bindings,
    ) -> bool {
        if pats.is_empty() {
            return args.is_empty();
        }
        let sys = self.sys;
        let flat = has(attrs, A_FLAT);
        let p = &pats[0];
        let rest = &pats[1..];

        match classify(p, &sys) {
            PatKind::BlankSeq(head) => {
                if args.is_empty() {
                    return false;
                }
                // Greedy: longest first.
                for k in (1..=args.len()).rev() {
                    if !self.heads_ok(head, &args[..k]) {
                        continue;
                    }
                    let mut b2 = b.clone();
                    if self.seq_ordered(rest, &args[k..], attrs, head_id, &mut b2) {
                        *b = b2;
                        return true;
                    }
                }
                false
            }
            PatKind::BlankNull(head) => {
                for k in (0..=args.len()).rev() {
                    if !self.heads_ok(head, &args[..k]) {
                        continue;
                    }
                    let mut b2 = b.clone();
                    if self.seq_ordered(rest, &args[k..], attrs, head_id, &mut b2) {
                        *b = b2;
                        return true;
                    }
                }
                false
            }
            PatKind::Repeated(inner, null_ok) => {
                let lo = if null_ok { 0 } else { 1 };
                for k in (lo..=args.len()).rev() {
                    let mut b2 = b.clone();
                    let mut all = true;
                    for item in &args[..k] {
                        let mut bi = b2.clone();
                        if !self.pat(&inner, item, &mut bi, false) {
                            all = false;
                            break;
                        }
                        b2 = bi;
                    }
                    if all && self.seq_ordered(rest, &args[k..], attrs, head_id, &mut b2) {
                        *b = b2;
                        return true;
                    }
                }
                false
            }
            PatKind::PatternSeq(name, inner) => {
                match classify(&inner, &sys) {
                    PatKind::Repeated(p2, null) => {
                        let lo = if null { 0 } else { 1 };
                        for k in (lo..=args.len()).rev() {
                            let mut b2 = b.clone();
                            let mut all = true;
                            for item in &args[..k] {
                                let mut bi = b2.clone();
                                if !self.pat(&p2, item, &mut bi, false) {
                                    all = false;
                                    break;
                                }
                                b2 = bi;
                            }
                            if all
                                && b2.bind(name, Bound::Many(args[..k].to_vec()))
                                && self.seq_ordered(rest, &args[k..], attrs, head_id, &mut b2)
                            {
                                *b = b2;
                                return true;
                            }
                        }
                        false
                    }
                    pk @ (PatKind::BlankSeq(_) | PatKind::BlankNull(_)) => {
                        let lo = match pk {
                            PatKind::BlankNull(_) => 0,
                            _ => 1,
                        };
                        let head = match pk {
                            PatKind::BlankSeq(h) | PatKind::BlankNull(h) => h,
                            _ => None,
                        };
                        for k in (lo..=args.len()).rev() {
                            if !self.heads_ok(head, &args[..k]) {
                                continue;
                            }
                            let mut b2 = b.clone();
                            if b2.bind(name, Bound::Many(args[..k].to_vec()))
                                && self.seq_ordered(rest, &args[k..], attrs, head_id, &mut b2)
                            {
                                *b = b2;
                                return true;
                            }
                        }
                        false
                    }
                    _ => false,
                }
            }
            PatKind::Optional(p2, default) => {
                if args.is_empty() {
                    // Default fills this position. Bind if Pattern-wrapped.
                    if let Some((h, hargs)) = p2.parts() {
                        if h.is_symbol(self.sys.pattern) && hargs.len() == 2 {
                            if let Expr::Sym(name) = &hargs[0] {
                                let d = match &default {
                                    Some(d) => match self.ctx.eval_plain(d) {
                                        Ok(v) => v,
                                        Err(_) => return false,
                                    },
                                    None => match self.ctx.default_for(head_id, 0) {
                                        Some(v) => v,
                                        None => return false,
                                    },
                                };
                                if !b.bind(*name, Bound::One(d)) {
                                    return false;
                                }
                            }
                        }
                    }
                    return self.seq_ordered(rest, args, attrs, head_id, b);
                }
                // Non-empty: match the inner pattern against the first arg.
                let mut b2 = b.clone();
                if self.pat(&p2, &args[0], &mut b2, false)
                    && self.seq_ordered(rest, &args[1..], attrs, head_id, &mut b2)
                {
                    *b = b2;
                    return true;
                }
                false
            }
            PatKind::Other => {
                if args.is_empty() {
                    return false;
                }
                // FLAT: a non-blank element may swallow a contiguous group.
                if flat && head_id.is_some() {
                    let hid = head_id.unwrap();
                    for k in (2..=args.len()).rev() {
                        let group = Expr::node(Expr::Sym(hid), args[..k].to_vec());
                        let mut b2 = b.clone();
                        if self.pat(p, &group, &mut b2, false)
                            && self.seq_ordered(rest, &args[k..], attrs, head_id, &mut b2)
                        {
                            *b = b2;
                            return true;
                        }
                    }
                    // A single argument is matched by itself: `Plus[x]` *is* `x`
                    // (OneIdentity), and wrapping it would stop `Plus[u[a_], u[b_]]`
                    // from matching `u[3] + u[4]`.
                    let mut b2 = b.clone();
                    if self.pat(p, &args[0], &mut b2, false)
                        && self.seq_ordered(rest, &args[1..], attrs, head_id, &mut b2)
                    {
                        let _ = hid;
                        *b = b2;
                        return true;
                    }
                    return false;
                }
                let mut b2 = b.clone();
                if self.pat(p, &args[0], &mut b2, false)
                    && self.seq_ordered(rest, &args[1..], attrs, head_id, &mut b2)
                {
                    *b = b2;
                    return true;
                }
                false
            }
        }
    }

    fn heads_ok(&mut self, head: Option<usize>, vals: &[Expr]) -> bool {
        match head {
            None => true,
            Some(h) => vals.iter().all(|v| self.ctx.head_sym(v) == h),
        }
    }
}

/// Permutations (Heap's algorithm).
pub fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
    if items.is_empty() {
        return vec![vec![]];
    }
    let mut res: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = items.to_vec();
    fn heap(k: usize, cur: &mut Vec<usize>, res: &mut Vec<Vec<usize>>) {
        if k == 1 {
            res.push(cur.clone());
            return;
        }
        for i in 0..k {
            heap(k - 1, cur, res);
            let j = if k % 2 == 0 { i } else { 0 };
            cur.swap(j, k - 1);
        }
    }
    heap(cur.len(), &mut cur, &mut res);
    res
}