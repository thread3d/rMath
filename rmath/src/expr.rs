//! The symbolic expression model: atoms (exact integers/rationals, machine
//! reals, strings, symbols, exact/real complex numbers) and normal
//! expressions `head[args...]`, plus the symbol table with attributes,
//! own-values, down-values and built-in dispatch.

use crate::bigint::BigInt;
use crate::rational::Rat;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

pub type SymId = usize;

// ---------- attributes ----------
pub const A_LISTABLE: u32 = 1 << 0;
pub const A_ORDERLESS: u32 = 1 << 1;
pub const A_FLAT: u32 = 1 << 2;
pub const A_ONEIDENTITY: u32 = 1 << 3;
pub const A_HOLDALL: u32 = 1 << 4;
pub const A_HOLDFIRST: u32 = 1 << 5;
pub const A_HOLDREST: u32 = 1 << 6;
pub const A_HOLDALLCOMPLETE: u32 = 1 << 7;
pub const A_SEQUENCEHOLD: u32 = 1 << 8;
pub const A_PROTECTED: u32 = 1 << 9;
pub const A_CONSTANT: u32 = 1 << 10;
pub const A_NUMERICFUNCTION: u32 = 1 << 11;
pub const A_NHOLDALL: u32 = 1 << 12;
pub const A_LOCKED: u32 = 1 << 13;

#[inline]
pub fn has(attrs: u32, bit: u32) -> bool {
    attrs & bit != 0
}

/// Merge two attr sets the way Mathematica's SetAttributes does.
pub fn attrs_union(a: u32, b: u32) -> u32 {
    a | b
}

// ---------- nodes ----------
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Node {
    pub head: Expr,
    pub args: Vec<Expr>,
}

// ---------- numeric atoms ----------
/// Exact rational or machine real; complex numbers are built from these.
/// f64 compared by bits (normalized, NaN-free) so Eq/Hash stay consistent.
#[derive(Clone, Debug)]
pub enum NumVal {
    R(Rat),
    F(f64),
}

impl PartialEq for NumVal {
    fn eq(&self, o: &NumVal) -> bool {
        match (self, o) {
            (NumVal::R(a), NumVal::R(b)) => a == b,
            (NumVal::F(a), NumVal::F(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}
impl Eq for NumVal {}
impl Hash for NumVal {
    fn hash<H: Hasher>(&self, h: &mut H) {
        std::mem::discriminant(self).hash(h);
        match self {
            NumVal::R(r) => {
                r.num.hash(h);
                r.den.hash(h);
            }
            NumVal::F(f) => f.to_bits().hash(h),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CNum {
    pub re: NumVal,
    pub im: NumVal,
}

// ---------- the expression type ----------
#[derive(Clone, Debug)]
pub enum Expr {
    Int(BigInt),
    Rat(Rat),
    Real(f64),
    Str(Rc<str>),
    Sym(SymId),
    Cnum(Box<CNum>),
    Node(Rc<Node>),
}

impl PartialEq for Expr {
    /// SameQ semantics: exact integers never equal machine reals, etc.
    fn eq(&self, o: &Expr) -> bool {
        same_q(self, o)
    }
}
impl Eq for Expr {}
impl Hash for Expr {
    fn hash<H: Hasher>(&self, h: &mut H) {
        std::mem::discriminant(self).hash(h);
        match self {
            Expr::Int(n) => n.hash(h),
            Expr::Rat(r) => {
                r.num.hash(h);
                r.den.hash(h);
            }
            Expr::Real(x) => x.to_bits().hash(h),
            Expr::Str(s) => s.hash(h),
            Expr::Sym(i) => i.hash(h),
            Expr::Cnum(c) => c.hash(h),
            Expr::Node(nd) => nd.hash(h),
        }
    }
}

impl Expr {
    // ----- constructors -----
    pub fn int_i64(v: i64) -> Expr {
        Expr::Int(BigInt::from_i64(v))
    }
    pub fn from_u64(v: u64) -> Expr {
        Expr::Int(BigInt::from_u64(v))
    }
    pub fn int_big(v: BigInt) -> Expr {
        Expr::Int(v)
    }
    /// Normalizes integer-valued rationals to Int.
    pub fn rat(r: Rat) -> Expr {
        if r.is_int() {
            Expr::Int(r.num)
        } else {
            Expr::Rat(r)
        }
    }
    pub fn real(x: f64) -> Expr {
        debug_assert!(x.is_finite(), "non-finite real must be handled by caller");
        let x = if x == 0.0 { 0.0 } else { x }; // normalize -0.0
        Expr::Real(x)
    }
    pub fn from_str(s: &str) -> Expr {
        Expr::Str(Rc::from(s))
    }
    pub fn cnum(re: NumVal, im: NumVal) -> Expr {
        // Normalize: Complex[re, 0] reduces to re.
        let im_zero = match &im {
            NumVal::R(r) => r.is_zero(),
            NumVal::F(f) => *f == 0.0,
        };
        if im_zero {
            match re {
                NumVal::R(r) => Expr::rat(r),
                NumVal::F(f) => Expr::real(f),
            }
        } else {
            Expr::Cnum(Box::new(CNum { re, im }))
        }
    }
    pub fn node(head: Expr, args: Vec<Expr>) -> Expr {
        Expr::Node(Rc::new(Node { head, args }))
    }

    // ----- shape predicates -----
    pub fn is_atom(&self) -> bool {
        !matches!(self, Expr::Node(_))
    }
    pub fn sym_id(&self) -> Option<SymId> {
        if let Expr::Sym(i) = self {
            Some(*i)
        } else {
            None
        }
    }
    pub fn is_symbol(&self, id: SymId) -> bool {
        matches!(self, Expr::Sym(i) if *i == id)
    }
    pub fn is_symbol_named(&self, t: &SymbolTable, name: &str) -> bool {
        match self {
            Expr::Sym(i) => t.name_of(*i) == name,
            _ => false,
        }
    }
    pub fn parts(&self) -> Option<(&Expr, &[Expr])> {
        if let Expr::Node(nd) = self {
            Some((&nd.head, &nd.args))
        } else {
            None
        }
    }
    /// Unwrap a uniquely-owned Node (used by the parser for flat n-ary folds).
    pub fn take_node_mut(self) -> Option<Node> {
        match self {
            Expr::Node(rc) => Rc::try_unwrap(rc).ok(),
            _ => None,
        }
    }
    /// For pattern matching, Rational/Complex atoms present as nodes.
    pub fn atom_as_node(&self, t: &SymbolTable) -> Option<(Expr, Vec<Expr>)> {
        match self {
            Expr::Rat(r) => Some((
                Expr::Sym(t.sys.rational),
                vec![Expr::Int(r.num.clone()), Expr::Int(r.den.clone())],
            )),
            Expr::Cnum(c) => {
                let re = match &c.re {
                    NumVal::R(r) => Expr::rat(r.clone()),
                    NumVal::F(f) => Expr::real(*f),
                };
                let im = match &c.im {
                    NumVal::R(r) => Expr::rat(r.clone()),
                    NumVal::F(f) => Expr::real(*f),
                };
                Some((Expr::Sym(t.sys.complex), vec![re, im]))
            }
            _ => None,
        }
    }

    /// Head of the expression (type symbol for atoms).
    pub fn head_of(&self, t: &SymbolTable) -> Expr {
        match self {
            Expr::Node(nd) => nd.head.clone(),
            Expr::Int(_) => Expr::Sym(t.sys.integer),
            Expr::Rat(_) => Expr::Sym(t.sys.rational),
            Expr::Real(_) => Expr::Sym(t.sys.real),
            Expr::Str(_) => Expr::Sym(t.sys.string),
            Expr::Sym(_) => Expr::Sym(t.sys.symbol),
            Expr::Cnum(_) => Expr::Sym(t.sys.complex),
        }
    }

    pub fn is_number(&self) -> bool {
        match self {
            Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) => true,
            Expr::Cnum(_) => true,
            _ => false,
        }
    }
    pub fn is_exact(&self) -> bool {
        matches!(self, Expr::Int(_) | Expr::Rat(_))
    }
    pub fn is_real_number(&self) -> bool {
        matches!(self, Expr::Int(_) | Expr::Rat(_) | Expr::Real(_))
    }
    /// List test used constantly by listable builtins.
    pub fn is_list(&self, t: &SymbolTable) -> bool {
        match self {
            Expr::Node(nd) => nd.head.is_symbol(t.sys.list),
            _ => false,
        }
    }
    pub fn list_len(&self) -> usize {
        match self {
            Expr::Node(nd) => nd.args.len(),
            _ => 0,
        }
    }
    pub fn as_rat_exact(&self) -> Option<Rat> {
        match self {
            Expr::Int(n) => Some(Rat { num: n.clone(), den: BigInt::one() }),
            Expr::Rat(r) => Some(r.clone()),
            _ => None,
        }
    }
    /// Integer value of an atom (exact ints, integer-valued rationals/reals).
    pub fn to_i64_pub(&self) -> Option<i64> {
        match self {
            Expr::Int(n) => n.to_i64(),
            Expr::Rat(r) if r.is_int() => r.num.to_i64(),
            Expr::Real(x) if x.is_finite() && x.fract() == 0.0 => Some(*x as i64),
            _ => None,
        }
    }
    /// Arguments of a node (empty for atoms).
    pub fn node_args(&self) -> Vec<Expr> {
        match self {
            Expr::Node(nd) => nd.args.clone(),
            _ => Vec::new(),
        }
    }
}

pub fn same_q(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Int(x), Expr::Int(y)) => x == y,
        (Expr::Rat(x), Expr::Rat(y)) => x == y,
        (Expr::Real(x), Expr::Real(y)) => x.to_bits() == y.to_bits(),
        (Expr::Str(x), Expr::Str(y)) => x == y,
        (Expr::Sym(x), Expr::Sym(y)) => x == y,
        (Expr::Cnum(x), Expr::Cnum(y)) => x == y,
        (Expr::Node(x), Expr::Node(y)) => x == y,
        _ => false,
    }
}

/// Canonical ordering used for Orderless sorting and Sort[].
/// Roughly: exact numbers < machine reals < complex < strings < symbols < nodes.
pub fn canon_cmp(t: &SymbolTable, a: &Expr, b: &Expr) -> Ordering {
    fn rank(e: &Expr) -> u8 {
        match e {
            Expr::Int(_) | Expr::Rat(_) => 0,
            Expr::Real(_) => 1,
            Expr::Cnum(_) => 2,
            Expr::Str(_) => 3,
            Expr::Sym(_) => 4,
            Expr::Node(_) => 5,
        }
    }
    match rank(a).cmp(&rank(b)) {
        Ordering::Equal => {}
        ord => return ord,
    }
    match (a, b) {
        (Expr::Int(x), Expr::Int(y)) => x.cmp(y),
        (Expr::Rat(x), Expr::Rat(y)) => x.cmp(y),
        (Expr::Int(x), Expr::Rat(y)) | (Expr::Rat(y), Expr::Int(x)) => {
            let rx = Rat { num: x.clone(), den: BigInt::one() };
            rx.cmp(y)
        }
        (Expr::Real(x), Expr::Real(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Expr::Cnum(x), Expr::Cnum(y)) => {
            let (rx, ix) = numval_f64(&x.re, &x.im);
            let (ry, iy) = numval_f64(&y.re, &y.im);
            rx.partial_cmp(&ry).unwrap_or(Ordering::Equal).then(ix.partial_cmp(&iy).unwrap_or(Ordering::Equal))
        }
        (Expr::Str(x), Expr::Str(y)) => x.cmp(y),
        (Expr::Sym(x), Expr::Sym(y)) => t.name_of(*x).cmp(t.name_of(*y)),
        (Expr::Node(x), Expr::Node(y)) => {
            let h = canon_cmp(t, &x.head, &y.head);
            if h != Ordering::Equal {
                return h;
            }
            match x.args.len().cmp(&y.args.len()) {
                Ordering::Equal => {}
                ord => return ord,
            }
            for (xa, ya) in x.args.iter().zip(y.args.iter()) {
                let c = canon_cmp(t, xa, ya);
                if c != Ordering::Equal {
                    return c;
                }
            }
            Ordering::Equal
        }
        _ => Ordering::Equal,
    }
}

fn numval_f64(re: &NumVal, im: &NumVal) -> (f64, f64) {
    let f = |n: &NumVal| match n {
        NumVal::R(r) => r.to_f64(),
        NumVal::F(v) => *v,
    };
    (f(re), f(im))
}

// ---------- symbol data ----------
#[derive(Clone, Debug)]
pub enum OwnValue {
    /// x = value (already evaluated at assignment time)
    Set(Expr),
    /// x := body (evaluated on each reference)
    SetDelayed(Expr),
}

#[derive(Clone, Debug)]
pub struct DownRule {
    /// Full lhs node, e.g. f[patterns...]; may be wrapped in HoldPattern.
    pub pat: Expr,
    pub body: Expr,
    /// true for := (SetDelayed)
    pub delayed: bool,
    /// Condition captured from `lhs /; cond`, if any.
    pub cond: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct SymbolData {
    pub name: String,
    pub attrs: u32,
    pub own: Option<OwnValue>,
    pub down: Vec<DownRule>,
    /// Default[f], Default[f, i], Default[f, i, n]
    pub defaults: Vec<Option<Expr>>,
    /// Options[f] rule list
    pub options: Vec<Expr>,
    /// Index into the builtins registry
    pub builtin: Option<usize>,
    /// Usage string for ?Name
    pub usage: Option<String>,
}

impl SymbolData {
    pub fn new(name: &str) -> SymbolData {
        SymbolData {
            name: name.to_string(),
            attrs: 0,
            own: None,
            down: Vec::new(),
            defaults: vec![None, None, None],
            options: Vec::new(),
            builtin: None,
            usage: None,
        }
    }
}

// ---------- system symbols (macro-generated struct) ----------
macro_rules! sys_syms {
    ($($fname:ident : $name:literal),* $(,)?) => {
        #[derive(Clone, Copy, Default)]
        pub struct Sys {
            $(pub $fname: SymId,)*
        }
        impl Sys {
            fn intern_all(t: &mut SymbolTable) -> Sys {
                let mut s = Sys::default();
                $(s.$fname = t.intern($name);)*
                s
            }
        }
    };
}

sys_syms! {
    integer: "Integer", real: "Real", rational: "Rational", complex: "Complex",
    string: "String", symbol: "Symbol",
    list: "List", association: "Association", plus: "Plus", times: "Times",
    power: "Power", rule: "Rule", ruledelayed: "RuleDelayed", set: "Set",
    setdelayed: "SetDelayed", unset: "Unset", upset: "UpSet", tagset: "TagSet",
    blank: "Blank", blanksequence: "BlankSequence", blanknullsequence: "BlankNullSequence",
    pattern: "Pattern", optional: "Optional", patterntest: "PatternTest",
    condition: "Condition", alternatives: "Alternatives", except: "Except",
    verbatim: "Verbatim", repeated: "Repeated", repeatednull: "RepeatedNull",
    hold: "Hold", holdpattern: "HoldPattern", holdform: "HoldForm",
    holdcomplete: "HoldComplete", unevaluated: "Unevaluated", sequence: "Sequence",
    function: "Function", slot: "Slot", slotsequence: "SlotSequence", span: "Span",
    part: "Part", out: "Out", compoundexpression: "CompoundExpression",
    increment: "Increment", decrement: "Decrement", preincrement: "PreIncrement",
    predecrement: "PreDecrement", addto: "AddTo", subtractfrom: "SubtractFrom",
    timesby: "TimesBy", divideby: "DivideBy", powertwo: "PowerTwo",
    replaceall: "ReplaceAll", replacerepeated: "ReplaceRepeated",
    stringjoin: "StringJoin", sameq: "SameQ", unsameq: "UnsameQ",
    equal: "Equal", unequal: "Unequal", less: "Less", greater: "Greater",
    lessequal: "LessEqual", greaterequal: "GreaterEqual",
    and: "And", or: "Or", not: "Not", factorial: "Factorial",
    derivativet: "Derivative", map: "Map", mapall: "MapAll", apply: "Apply",
    composition: "Composition", rightcomposition: "RightComposition",
    infinity: "Infinity", complexinfinity: "ComplexInfinity",
    indeterminate: "Indeterminate", directedinfinity: "DirectedInfinity",
    r#true: "True", r#false: "False", null: "Null", missing: "Missing",
    seriesdata: "SeriesData", directededge: "DirectedEdge",
    pattern_head_marker: "Pattern$Marker", all: "All", automatic: "Automatic",
    dot: "Dot", tagsetdelayed: "TagSetDelayed", message: "Message",
    evaluate: "Evaluate",
}

pub struct SymbolTable {
    names: HashMap<String, SymId>,
    pub data: Vec<SymbolData>,
    pub sys: Sys,
}

impl SymbolTable {
    pub fn new() -> SymbolTable {
        let mut t = SymbolTable {
            names: HashMap::new(),
            data: Vec::new(),
            sys: Sys::default(),
        };
        t.sys = Sys::intern_all(&mut t);
        t
    }
    pub fn intern(&mut self, name: &str) -> SymId {
        if let Some(&id) = self.names.get(name) {
            return id;
        }
        let id = self.data.len();
        self.data.push(SymbolData::new(name));
        self.names.insert(name.to_string(), id);
        id
    }
    pub fn get(&self, name: &str) -> Option<SymId> {
        self.names.get(name).copied()
    }
    pub fn name_of(&self, id: SymId) -> &str {
        &self.data[id].name
    }
    pub fn attrs(&self, id: SymId) -> u32 {
        self.data[id].attrs
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        SymbolTable::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> SymbolTable {
        SymbolTable::new()
    }

    #[test]
    fn symbols_intern_stably() {
        let mut tt = t();
        let x = tt.intern("x");
        let y = tt.intern("y");
        assert_eq!(tt.intern("x"), x);
        assert_eq!(tt.get("y"), Some(y));
        assert_eq!(tt.name_of(x), "x");
    }

    #[test]
    fn same_q_semantics() {
        let tt = t();
        let one = Expr::int_i64(1);
        let one_real = Expr::real(1.0);
        assert!(!same_q(&one, &one_real)); // SameQ[1, 1.] is False
        assert!(same_q(&Expr::rat(Rat::from_i64(1)), &one)); // 2/2 normalizes
        assert!(same_q(&Expr::node(Expr::Sym(tt.sys.plus), vec![one.clone()]),
                       &Expr::node(Expr::Sym(tt.sys.plus), vec![one.clone()])));
        assert!(same_q(&Expr::from_str("a"), &Expr::from_str("a")));
        assert!(!same_q(&Expr::from_str("a"), &Expr::from_str("b")));
    }

    #[test]
    fn rat_normalizes_to_int() {
        let r = Rat::new(BigInt::from_i64(4), BigInt::from_i64(2));
        match Expr::rat(r) {
            Expr::Int(n) => assert_eq!(n.to_string(), "2"),
            other => panic!("expected Int, got {:?}", other),
        }
    }

    #[test]
    fn cnum_normalizes() {
        let zero = NumVal::R(Rat::from_i64(0));
        let one = NumVal::R(Rat::from_i64(1));
        match Expr::cnum(one.clone(), zero) {
            Expr::Int(n) => assert_eq!(n.to_string(), "1"),
            other => panic!("expected Int, got {:?}", other),
        }
    }

    #[test]
    fn canon_order() {
        let mut tt = t();
        let x = tt.intern("x");
        // 1 < x, symbols alphabetical, nodes after symbols.
        assert_eq!(canon_cmp(&tt, &Expr::int_i64(1), &Expr::Sym(x)), Ordering::Less);
        let a = tt.intern("a");
        let b = tt.intern("b");
        assert_eq!(canon_cmp(&tt, &Expr::Sym(a), &Expr::Sym(b)), Ordering::Less);
        let node_a = Expr::node(Expr::Sym(a), vec![]);
        assert_eq!(canon_cmp(&tt, &Expr::Sym(b), &node_a), Ordering::Less);
    }

    #[test]
    fn head_of_atoms() {
        let tt = t();
        assert!(Expr::int_i64(3).head_of(&tt).is_symbol(tt.sys.integer));
        assert!(Expr::real(1.5).head_of(&tt).is_symbol(tt.sys.real));
        assert!(Expr::from_str("hi").head_of(&tt).is_symbol(tt.sys.string));
    }
}