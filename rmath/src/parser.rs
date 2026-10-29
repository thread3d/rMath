//! Recursive-descent parser for Mathematica-style syntax.
//!
//! Precedence (loose -> tight), validated against Wolfram parsing behavior:
//!   ;  <  //  <  set-family  <  & /@ /@@ //@ @@ @@@ ~ @  <  //.  <  /.  <  -> :>
//!   <  /;  <  pattern-colon  <  |  <  .. ...  <  ?  <  ;;  <  ||  <  &&
//!   <  relations  <  + -  <  * / (with implicit Times)  <  @* /*  <  ^
//!   <  .  <  postfix (! !! ' ++ -- [[..]] ::)

use crate::expr::{Expr, SymbolTable, Sys};
use crate::lexer::{lex, Tok, TokKind};
use std::rc::Rc;

#[derive(Debug)]
pub struct ParseError {
    pub msg: String,
    pub line: u32,
}

pub fn parse_input(t: &mut SymbolTable, src: &str) -> Result<Expr, ParseError> {
    let toks = lex(src).map_err(|e| ParseError { msg: e.msg, line: e.line })?;
    let mut p = Parser { toks, i: 0, t };
    let e = p.parse_l1()?;
    p.expect_eof()?;
    Ok(e)
}

struct Parser<'t> {
    toks: Vec<Tok>,
    i: usize,
    t: &'t mut SymbolTable,
}

impl<'t> Parser<'t> {
    // ----- token helpers -----
    fn peek(&self) -> &Tok {
        &self.toks[self.i.min(self.toks.len() - 1)]
    }
    fn peek_kind(&self) -> &TokKind {
        &self.peek().kind
    }
    fn at(&self, k: &TokKind) -> bool {
        self.peek_kind() == k
    }
    fn advance(&mut self) -> Tok {
        let tk = self.toks[self.i.min(self.toks.len() - 1)].clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        tk
    }
    fn at_space(&self, k: &TokKind) -> bool {
        self.at(k) && !self.peek().space_before
    }
    fn cur_line(&self) -> u32 {
        self.peek().line
    }
    fn err_here(&self, msg: &str) -> ParseError {
        ParseError { msg: msg.to_string(), line: self.cur_line() }
    }
    fn expect_eof(&mut self) -> Result<(), ParseError> {
        if self.at(&TokKind::Eof) {
            Ok(())
        } else {
            Err(self.err_here("unexpected trailing input"))
        }
    }
    fn sym(&mut self, name: &str) -> Expr {
        Expr::Sym(self.t.intern(name))
    }
    fn node(&self, head: Expr, args: Vec<Expr>) -> Expr {
        Expr::node(head, args)
    }
    /// Build `head[lhs, rhs]`, folding when lhs is already `head[...]`
    /// (Mathematica's parser produces flat nodes for Flat operators).
    fn push_op(&self, head_id: usize, lhs: Expr, rhs: Expr) -> Expr {
        if matches!(&lhs, Expr::Node(nd) if nd.head.is_symbol(head_id)) {
            if let Expr::Node(rc) = lhs {
                match std::rc::Rc::try_unwrap(rc) {
                    Ok(mut node) => {
                        node.args.push(rhs);
                        return Expr::Node(Rc::new(node));
                    }
                    Err(rc) => {
                        let mut args = rc.args.clone();
                        args.push(rhs);
                        return Expr::node(rc.head.clone(), args);
                    }
                }
            }
        }
        Expr::node(Expr::Sym(head_id), vec![lhs, rhs])
    }
    fn starts_expr_terminator(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokKind::Comma
                | TokKind::RBracket
                | TokKind::DblRBracket
                | TokKind::RBrace
                | TokKind::RParen
                | TokKind::Semi
                | TokKind::Eof
        )
    }

    // ----- L1: CompoundExpression `;` -----
    fn parse_l1(&mut self) -> Result<Expr, ParseError> {
        if self.at(&TokKind::Eof) {
            return Ok(self.sym("Null"));
        }
        let sys = self.t.sys;
        let mut args = vec![self.parse_l1b()?];
        let mut trailing = false;
        while self.at(&TokKind::Semi) {
            self.advance();
            trailing = true;
            if self.at(&TokKind::Eof) || self.at(&TokKind::RParen) || self.at(&TokKind::RBrace) {
                break;
            }
            args.push(self.parse_l2()?);
            trailing = false;
        }
        if trailing {
            args.push(Expr::Sym(sys.null));
        }
        if args.len() == 1 {
            Ok(args.pop().unwrap())
        } else {
            Ok(self.node(Expr::Sym(sys.compoundexpression), args))
        }
    }

    // ----- L1b: file output >> / >>> -----
    fn parse_l1b(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_l2()?;
        if self.at(&TokKind::Put) {
            self.advance();
            let rhs = self.parse_l2()?;
            let h = self.sym("Put");
            return Ok(self.node(h, vec![lhs, rhs]));
        }
        if self.at(&TokKind::PutAppend) {
            self.advance();
            let rhs = self.parse_l2()?;
            let h = self.sym("PutAppend");
            return Ok(self.node(h, vec![lhs, rhs]));
        }
        Ok(lhs)
    }

    // ----- L2: postfix `//` -----
    fn parse_l2(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_l3()?;
        while self.at(&TokKind::SlashSlash) {
            self.advance();
            let f = self.parse_l3()?;
            lhs = self.node(f, vec![lhs]);
        }
        Ok(lhs)
    }

    // ----- L3: Set family (right-assoc) + TagSet -----
    fn parse_l3(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_l4()?;
        let sys = self.t.sys;
        if self.at(&TokKind::TagSetMark) {
            self.advance();
            // Parse the tagged lhs WITHOUT descending into the Set level,
            // so the assignment operator stays for us.
            let lhs2 = self.parse_l4()?;
            let op = self.peek_kind().clone();
            let rhs = match op {
                TokKind::Set | TokKind::SetDelayed | TokKind::UpSet | TokKind::UpSetDelayed => {
                    self.advance();
                    self.parse_l3()?
                }
                _ => return Err(self.err_here("expected assignment after /:")),
            };
            let head = if matches!(op, TokKind::SetDelayed | TokKind::UpSetDelayed) {
                sys.tagsetdelayed
            } else {
                sys.tagset
            };
            return Ok(self.node(Expr::Sym(head), vec![lhs, lhs2, rhs]));
        }
        match self.peek_kind() {
            TokKind::Set => {
                self.advance();
                let rhs = self.parse_l3()?;
                Ok(self.node(Expr::Sym(sys.set), vec![lhs, rhs]))
            }
            TokKind::SetDelayed => {
                self.advance();
                let rhs = self.parse_l3()?;
                Ok(self.node(Expr::Sym(sys.setdelayed), vec![lhs, rhs]))
            }
            TokKind::UpSet => {
                self.advance();
                let rhs = self.parse_l3()?;
                Ok(self.node(Expr::Sym(sys.upset), vec![lhs, rhs]))
            }
            TokKind::UpSetDelayed => {
                self.advance();
                let rhs = self.parse_l3()?;
                Ok(self.node(Expr::Sym(sys.upsetdelayed), vec![lhs, rhs]))
            }
            TokKind::Unset => {
                self.advance();
                Ok(self.node(Expr::Sym(sys.unset), vec![lhs]))
            }
            TokKind::AddTo => {
                self.advance();
                let rhs = self.parse_l3()?;
                Ok(self.node(Expr::Sym(sys.addto), vec![lhs, rhs]))
            }
            TokKind::SubtractFrom => {
                self.advance();
                let rhs = self.parse_l3()?;
                Ok(self.node(Expr::Sym(sys.subtractfrom), vec![lhs, rhs]))
            }
            TokKind::TimesBy => {
                self.advance();
                let rhs = self.parse_l3()?;
                Ok(self.node(Expr::Sym(sys.timesby), vec![lhs, rhs]))
            }
            TokKind::DivideBy => {
                self.advance();
                let rhs = self.parse_l3()?;
                Ok(self.node(Expr::Sym(sys.divideby), vec![lhs, rhs]))
            }
            _ => Ok(lhs),
        }
    }

    // ----- L4: application mix: & /@ /@@ //@ @@ @@@ ~ @ -----
    fn parse_l4(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_l5()?;
        loop {
            let sys = self.t.sys;
            match self.peek_kind() {
                TokKind::Amp => {
                    self.advance();
                    lhs = self.node(Expr::Sym(sys.function), vec![lhs]);
                    // A pure function can be applied immediately: #^2 &[7].
                    if self.at(&TokKind::LBracket) {
                        lhs = self.apply_args(lhs)?;
                    }
                }
                TokKind::SlashAt => {
                    self.advance();
                    let rhs = self.parse_at_chain()?;
                    lhs = self.node(Expr::Sym(sys.map), vec![lhs, rhs]);
                }
                TokKind::SlashSlashAt => {
                    self.advance();
                    let rhs = self.parse_at_chain()?;
                    lhs = self.node(Expr::Sym(sys.mapall), vec![lhs, rhs]);
                }
                TokKind::SlashAtAt | TokKind::AtAtAt => {
                    self.advance();
                    let rhs = self.parse_at_chain()?;
                    let lvl = self.node(Expr::Sym(sys.list), vec![Expr::int_i64(1)]);
                    lhs = self.node(Expr::Sym(sys.apply), vec![lhs, rhs, lvl]);
                }
                TokKind::AtAt => {
                    self.advance();
                    let rhs = self.parse_at_chain()?;
                    lhs = self.node(Expr::Sym(sys.apply), vec![lhs, rhs]);
                }
                TokKind::At => {
                    self.advance();
                    let rhs = self.parse_at_chain()?;
                    lhs = self.node(lhs, vec![rhs]);
                }
                TokKind::Tilde => {
                    self.advance();
                    let op = self.parse_at_chain()?;
                    if !self.at(&TokKind::Tilde) {
                        return Err(self.err_here("expected ~ in a~f~b"));
                    }
                    self.advance();
                    let rhs = self.parse_l5()?;
                    lhs = self.node(op, vec![lhs, rhs]);
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    /// Right-assoc prefix `@` chain: f @ g @ x -> f[g[x]].
    fn parse_at_chain(&mut self) -> Result<Expr, ParseError> {
        let e = self.parse_l5()?;
        if self.at(&TokKind::At) {
            self.advance();
            let inner = self.parse_at_chain()?;
            Ok(self.node(e, vec![inner]))
        } else {
            Ok(e)
        }
    }

    /// `head[arg, ...]` — arguments are full expressions, so a compound
    /// expression or an assignment is allowed inside the brackets.
    fn apply_args(&mut self, head: Expr) -> Result<Expr, ParseError> {
        self.advance(); // [
        let mut args = Vec::new();
        if !self.at(&TokKind::RBracket) {
            loop {
                args.push(self.parse_l1()?);
                if self.at(&TokKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        if !self.at(&TokKind::RBracket) {
            return Err(self.err_here("expected ]"));
        }
        self.advance();
        Ok(self.node(head, args))
    }

    // ----- L5: /. and //. -----
    fn parse_l5(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_l6()?;
        loop {
            let sys = self.t.sys;
            match self.peek_kind() {
                TokKind::ReplaceAll => {
                    self.advance();
                    let rhs = self.parse_l6()?;
                    lhs = self.node(Expr::Sym(sys.replaceall), vec![lhs, rhs]);
                }
                TokKind::ReplaceRepeated => {
                    self.advance();
                    let rhs = self.parse_l6()?;
                    lhs = self.node(Expr::Sym(sys.replacerepeated), vec![lhs, rhs]);
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    // ----- L6: rules -> :> (right-assoc) -----
    fn parse_l6(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_l7()?;
        let sys = self.t.sys;
        match self.peek_kind() {
            TokKind::Rule => {
                self.advance();
                let rhs = self.parse_l6()?;
                Ok(self.node(Expr::Sym(sys.rule), vec![lhs, rhs]))
            }
            TokKind::RuleDelayed => {
                self.advance();
                let rhs = self.parse_l6()?;
                Ok(self.node(Expr::Sym(sys.ruledelayed), vec![lhs, rhs]))
            }
            _ => Ok(lhs),
        }
    }

    // ----- L7: /; Condition -----
    fn parse_l7(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_l75()?;
        while self.at(&TokKind::SlashSemi) {
            self.advance();
            let rhs = self.parse_l75()?;
            lhs = self.node(Expr::Sym(self.t.sys.condition), vec![lhs, rhs]);
        }
        Ok(lhs)
    }

    // ----- L7.5: pattern colon: x:pat / x_:default -----
    fn parse_l75(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_l8()?;
        if !self.at(&TokKind::Colon) {
            return Ok(lhs);
        }
        self.advance();
        let rhs_starts_pattern = matches!(
            self.peek_kind(),
            TokKind::Underscore | TokKind::Underscore2 | TokKind::Underscore3
        );
        let rhs = self.parse_l8()?;
        let sys = self.t.sys;
        if rhs_starts_pattern || !is_pattern_expr(&lhs, &self.t.sys) {
            Ok(self.node(Expr::Sym(sys.pattern), vec![lhs, rhs]))
        } else {
            Ok(self.node(Expr::Sym(sys.optional), vec![lhs, rhs]))
        }
    }

    // ----- L8: | Alternatives -----
    fn parse_l8(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let mut lhs = self.parse_l9()?;
        while self.at(&TokKind::Bar) {
            self.advance();
            let rhs = self.parse_l9()?;
            lhs = self.push_op(sys.alternatives, lhs, rhs);
        }
        Ok(lhs)
    }

    // ----- L9: .. ... Repeated -----
    fn parse_l9(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_l10()?;
        if self.at(&TokKind::DotDot) {
            self.advance();
            return Ok(self.node(Expr::Sym(self.t.sys.repeated), vec![lhs]));
        }
        if self.at(&TokKind::DotDotDot) {
            self.advance();
            return Ok(self.node(Expr::Sym(self.t.sys.repeatednull), vec![lhs]));
        }
        Ok(lhs)
    }

    // ----- L10: ? PatternTest -----
    fn parse_l10(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_l11()?;
        while self.at(&TokKind::Question) {
            self.advance();
            let rhs = self.parse_l11()?;
            lhs = self.node(Expr::Sym(self.t.sys.patterntest), vec![lhs, rhs]);
        }
        Ok(lhs)
    }

    // ----- L11: ;; Span (accepted outside Part too; evaluator validates) -----
    fn parse_l11(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let mut comps: Vec<Option<Expr>> = Vec::new();
        if self.at(&TokKind::SemiSemi) {
            comps.push(None);
        } else {
            comps.push(Some(self.parse_l12()?));
        }
        if !self.at(&TokKind::SemiSemi) {
            return Ok(comps.pop().flatten().unwrap());
        }
        while self.at(&TokKind::SemiSemi) {
            self.advance();
            if self.at(&TokKind::SemiSemi) {
                comps.push(None);
            } else if self.starts_expr_terminator() {
                comps.push(None);
                break;
            } else {
                comps.push(Some(self.parse_l12()?));
            }
        }
        let min_e = comps.first().cloned().flatten().unwrap_or(Expr::int_i64(1));
        let max_e = comps
            .get(1)
            .cloned()
            .flatten()
            .unwrap_or(Expr::Sym(sys.all));
        match comps.get(2).cloned().flatten() {
            Some(step) => Ok(self.node(Expr::Sym(sys.span), vec![min_e, max_e, step])),
            None => Ok(self.node(Expr::Sym(sys.span), vec![min_e, max_e])),
        }
    }

    // ----- L12: || Or -----
    fn parse_l12(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let mut lhs = self.parse_l13()?;
        while self.at(&TokKind::BarBar) {
            self.advance();
            let rhs = self.parse_l13()?;
            lhs = self.push_op(sys.or, lhs, rhs);
        }
        Ok(lhs)
    }

    // ----- L13: && And -----
    fn parse_l13(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let mut lhs = self.parse_l14()?;
        while self.at(&TokKind::AmpAmp) {
            self.advance();
            let rhs = self.parse_l14()?;
            lhs = self.push_op(sys.and, lhs, rhs);
        }
        Ok(lhs)
    }

    // ----- L14: relations -----
    /// `a > b > c` is one n-ary comparison, not `(a > b) > c` (which compared
    /// `True` with a number and stayed unevaluated). Mathematica prints a
    /// mixed chain such as `a < b > c` as `Inequality[...]`, i.e. the
    /// conjunction of its neighbouring pairs, which is what a mixed chain
    /// becomes here.
    fn parse_l14(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let mut operands = vec![self.parse_l15()?];
        let mut heads: Vec<usize> = Vec::new();
        loop {
            let head = match self.peek_kind() {
                TokKind::EqEq => sys.equal,
                TokKind::BangEq => sys.unequal,
                TokKind::Less => sys.less,
                TokKind::Greater => sys.greater,
                TokKind::LessEq => sys.lessequal,
                TokKind::GreaterEq => sys.greaterequal,
                TokKind::SameEq => sys.sameq,
                TokKind::UnsameEq => sys.unsameq,
                _ => break,
            };
            self.advance();
            operands.push(self.parse_l15()?);
            heads.push(head);
        }
        if heads.is_empty() {
            return Ok(operands.pop().unwrap());
        }
        if heads.windows(2).all(|w| w[0] == w[1]) {
            let args = operands;
            return Ok(Expr::node(Expr::Sym(heads[0]), args));
        }
        // Mixed chain: `a < b > c` is `a < b && b > c`.
        let mut pairs = Vec::with_capacity(heads.len());
        for (i, head) in heads.iter().enumerate() {
            let a = operands[i].clone();
            let b = operands[i + 1].clone();
            pairs.push(Expr::node(Expr::Sym(*head), vec![a, b]));
        }
        Ok(Expr::node(Expr::Sym(sys.and), pairs))
    }

    // ----- L15: + - -----
    fn parse_l15(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let mut lhs = self.parse_l16()?;
        loop {
            match self.peek_kind() {
                TokKind::Plus => {
                    self.advance();
                    let rhs = self.parse_l16()?;
                    lhs = self.push_op(sys.plus, lhs, rhs);
                }
                TokKind::Minus => {
                    self.advance();
                    let rhs = self.parse_l16()?;
                    let neg = self.node(Expr::Sym(sys.times), vec![Expr::int_i64(-1), rhs]);
                    lhs = self.push_op(sys.plus, lhs, neg);
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    // ----- L16: * / and implicit juxtaposition -----
    fn parse_l16(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let mut lhs = self.parse_l17()?;
        loop {
            match self.peek_kind() {
                TokKind::Star => {
                    self.advance();
                    let rhs = self.parse_l17()?;
                    lhs = self.push_op(sys.times, lhs, rhs);
                }
                TokKind::Slash => {
                    self.advance();
                    let rhs = self.parse_l17()?;
                    let inv = self.node(Expr::Sym(sys.power), vec![rhs, Expr::int_i64(-1)]);
                    lhs = self.push_op(sys.times, lhs, inv);
                }
                TokKind::StrJoin => {
                    self.advance();
                    let rhs = self.parse_l17()?;
                    let h = self.sym("StringJoin");
                    lhs = self.node(h, vec![lhs, rhs]);
                }
                TokKind::Int
                | TokKind::Real
                | TokKind::Str
                | TokKind::Sym
                | TokKind::LParen
                | TokKind::LBrace
                | TokKind::LAssoc
                | TokKind::Slot => {
                    let rhs = self.parse_l17()?;
                    lhs = self.push_op(sys.times, lhs, rhs);
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    // ----- L17: @* /* (and leading unary - that must bind above ^) -----
    fn parse_l17(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        // √x — the radical sign is a prefix operator for Sqrt.
        if self.at(&TokKind::Radical) {
            self.advance();
            let e = self.parse_l17()?;
            let h = self.sym("Sqrt");
            return Ok(self.node(h, vec![e]));
        }
        match self.peek_kind() {
            TokKind::Minus => {
                self.advance();
                let e = self.parse_l17()?;
                return Ok(self.node(Expr::Sym(sys.times), vec![Expr::int_i64(-1), e]));
            }
            TokKind::Plus => {
                self.advance();
                return self.parse_l17();
            }
            _ => {}
        }
        let mut lhs = self.parse_l18()?;
        loop {
            match self.peek_kind() {
                TokKind::Comp => {
                    self.advance();
                    let rhs = self.parse_l18()?;
                    lhs = self.node(Expr::Sym(sys.composition), vec![lhs, rhs]);
                }
                TokKind::RightComp => {
                    self.advance();
                    let rhs = self.parse_l18()?;
                    lhs = self.node(Expr::Sym(sys.rightcomposition), vec![lhs, rhs]);
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    // ----- L18: ^ right-assoc -----
    fn parse_l18(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_l19()?;
        if self.at(&TokKind::Caret) {
            self.advance();
            let rhs = self.parse_l18()?;
            return Ok(self.node(Expr::Sym(self.t.sys.power), vec![lhs, rhs]));
        }
        Ok(lhs)
    }

    // ----- L19: . Dot -----
    fn parse_l19(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let mut lhs = self.parse_l20()?;
        while self.at(&TokKind::Dot) {
            self.advance();
            let rhs = self.parse_l20()?;
            lhs = self.push_op(sys.dot, lhs, rhs);
        }
        Ok(lhs)
    }

    // ----- L20: prefixes + primary + postfix -----
    fn parse_l20(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        match self.peek_kind() {
            TokKind::Minus => {
                self.advance();
                let e = self.parse_l20()?;
                return Ok(self.node(Expr::Sym(sys.times), vec![Expr::int_i64(-1), e]));
            }
            TokKind::Plus => {
                self.advance();
                return self.parse_l20();
            }
            TokKind::Bang => {
                self.advance();
                let e = self.parse_l20()?;
                return Ok(self.node(Expr::Sym(sys.not), vec![e]));
            }
            TokKind::PlusPlus => {
                self.advance();
                let e = self.parse_l20()?;
                return Ok(self.node(Expr::Sym(sys.preincrement), vec![e]));
            }
            TokKind::MinusMinus => {
                self.advance();
                let e = self.parse_l20()?;
                return Ok(self.node(Expr::Sym(sys.predecrement), vec![e]));
            }
            _ => {}
        }
        let mut lhs = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                TokKind::LBracket => {
                    lhs = self.apply_args(lhs)?;
                }
                TokKind::DblLBracket => {
                    self.advance();
                    let mut args = vec![lhs];
                    if !self.at(&TokKind::RBracket) {
                        loop {
                            args.push(self.parse_l11()?);
                            if self.at(&TokKind::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    if !self.at(&TokKind::RBracket) {
                        return Err(self.err_here("expected ]]"));
                    }
                    self.advance();
                    if !self.at(&TokKind::RBracket) {
                        return Err(self.err_here("expected ]]"));
                    }
                    self.advance();
                    lhs = self.node(Expr::Sym(sys.part), args);
                }
                TokKind::Quote => {
                    let mut n = 0usize;
                    while self.at(&TokKind::Quote) {
                        self.advance();
                        n += 1;
                    }
                    let d1 = self.node(
                        Expr::Sym(sys.derivativet),
                        vec![Expr::int_i64(n as i64)],
                    );
                    lhs = self.node(d1, vec![lhs]);
                }
                TokKind::Bang => {
                    if self.peek().space_before {
                        break;
                    }
                    self.advance();
                    let h = self.sym("Factorial");
                    lhs = self.node(h, vec![lhs]);
                }
                TokKind::BangBang => {
                    if self.peek().space_before {
                        break;
                    }
                    self.advance();
                    let h = self.sym("Factorial2");
                    lhs = self.node(h, vec![lhs]);
                }
                TokKind::PlusPlus => {
                    if self.peek().space_before {
                        break;
                    }
                    self.advance();
                    lhs = self.node(Expr::Sym(sys.increment), vec![lhs]);
                }
                TokKind::MinusMinus => {
                    if self.peek().space_before {
                        break;
                    }
                    self.advance();
                    lhs = self.node(Expr::Sym(sys.decrement), vec![lhs]);
                }
                TokKind::ColonColon => {
                    self.advance();
                    let tag = match self.peek_kind() {
                        TokKind::Sym | TokKind::Str => {
                            let tk = self.advance();
                            Expr::from_str(&tk.text)
                        }
                        _ => return Err(self.err_here("expected message tag after ::")),
                    };
                    let h = self.sym("MessageName");
                    lhs = self.node(h, vec![lhs, tag]);
                }
                TokKind::Dot => {
                    if !self.at_space(&TokKind::Dot) || !is_pattern_expr(&lhs, &self.t.sys) {
                        break;
                    }
                    self.advance();
                    lhs = self.node(Expr::Sym(sys.optional), vec![lhs]);
                }
                TokKind::Underscore | TokKind::Underscore2 | TokKind::Underscore3 => {
                    if self.peek().space_before {
                        break;
                    }
                    if !matches!(lhs, Expr::Sym(_)) {
                        return Err(self.err_here("pattern _ after non-symbol"));
                    }
                    let name = lhs.sym_id().unwrap();
                    let blank = self.parse_blank()?;
                    lhs = self.node(
                        Expr::Sym(sys.pattern),
                        vec![Expr::Sym(name), blank],
                    );
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    /// Parse `_`, `__`, `___` with optional adjacent head.
    fn parse_blank(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        let kind = match self.peek_kind() {
            TokKind::Underscore => sys.blank,
            TokKind::Underscore2 => sys.blanksequence,
            TokKind::Underscore3 => sys.blanknullsequence,
            _ => return Err(self.err_here("expected pattern blank")),
        };
        self.advance();
        let head = if self.at(&TokKind::Sym) && !self.peek().space_before {
            let tk = self.advance();
            Some(Expr::Sym(self.t.intern(&tk.text)))
        } else {
            None
        };
        Ok(match head {
            Some(h) => self.node(Expr::Sym(kind), vec![h]),
            None => self.node(Expr::Sym(kind), vec![]),
        })
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let sys = self.t.sys;
        match self.peek_kind() {
            TokKind::Int => {
                let tk = self.advance();
                let n = crate::bigint::BigInt::from_str_dec(&tk.text)
                    .ok_or_else(|| self.err_here("bad integer"))?;
                Ok(Expr::Int(n))
            }
            TokKind::Real => {
                let tk = self.advance();
                let f: f64 = tk.text.parse().map_err(|_| self.err_here("bad real number"))?;
                if !f.is_finite() {
                    return Err(self.err_here("real number out of range"));
                }
                Ok(Expr::real(f))
            }
            TokKind::Str => {
                let tk = self.advance();
                Ok(Expr::from_str(&tk.text))
            }
            TokKind::Sym => {
                let tk = self.advance();
                Ok(Expr::Sym(self.t.intern(&tk.text)))
            }
            TokKind::LParen => {
                self.advance();
                let e = self.parse_l1()?;
                if !self.at(&TokKind::RParen) {
                    return Err(self.err_here("expected )"));
                }
                self.advance();
                Ok(e)
            }
            TokKind::LBrace => {
                self.advance();
                let mut args = Vec::new();
                if !self.at(&TokKind::RBrace) {
                    loop {
                        args.push(self.parse_l1()?);
                        if self.at(&TokKind::Comma) {
                            self.advance();
                            if self.at(&TokKind::RBrace) {
                                break; // tolerate trailing comma
                            }
                        } else {
                            break;
                        }
                    }
                }
                if !self.at(&TokKind::RBrace) {
                    return Err(self.err_here("expected }"));
                }
                self.advance();
                Ok(self.node(Expr::Sym(sys.list), args))
            }
            TokKind::LAssoc => {
                self.advance();
                let mut args = Vec::new();
                if !self.at(&TokKind::RAssoc) {
                    loop {
                        let e = self.parse_l6()?;
                        let is_rule = matches!(e.parts(),
                            Some((h, _)) if h.is_symbol(sys.rule) || h.is_symbol(sys.ruledelayed));
                        if !is_rule {
                            return Err(self.err_here("Association requires key -> value rules"));
                        }
                        args.push(e);
                        if self.at(&TokKind::Comma) {
                            self.advance();
                            if self.at(&TokKind::RAssoc) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                if !self.at(&TokKind::RAssoc) {
                    return Err(self.err_here("expected |>"));
                }
                self.advance();
                Ok(self.node(Expr::Sym(sys.association), args))
            }
            TokKind::Underscore | TokKind::Underscore2 | TokKind::Underscore3 => {
                self.parse_blank()
            }
            TokKind::Slot => {
                let tk = self.advance();
                let txt = tk.text.as_str();
                if let Some(rest) = txt.strip_prefix('#') {
                    let n: i64 = if rest.is_empty() {
                        1
                    } else {
                        rest.parse().map_err(|_| self.err_here("bad slot number"))?
                    };
                    Ok(self.node(Expr::Sym(sys.slotsequence), vec![Expr::int_i64(n)]))
                } else if txt.is_empty() {
                    Ok(self.node(Expr::Sym(sys.slot), vec![Expr::int_i64(1)]))
                } else if txt.chars().all(|c| c.is_ascii_digit()) {
                    let n: i64 = txt.parse().unwrap_or(1);
                    Ok(self.node(Expr::Sym(sys.slot), vec![Expr::int_i64(n)]))
                } else {
                    Ok(self.node(Expr::Sym(sys.slot), vec![Expr::from_str(txt)]))
                }
            }
            TokKind::Percent => {
                let mut count = 0usize;
                let mut named: Option<i64> = None;
                while self.at(&TokKind::Percent) {
                    let tk = self.advance();
                    count += 1;
                    if !tk.text.is_empty() {
                        if count == 1 {
                            named = Some(tk.text.parse().unwrap_or(0));
                        } else {
                            return Err(self.err_here("bad % reference"));
                        }
                    }
                }
                let n = match named {
                    Some(n) => n,
                    None => -(count as i64),
                };
                Ok(self.node(Expr::Sym(sys.out), vec![Expr::int_i64(n)]))
            }
            TokKind::Question => {
                Err(self.err_here("unexpected ? (use ?Name at start of input)"))
            }
            _ => Err(self.err_here("unexpected token")),
        }
    }
}

fn is_pattern_expr(e: &Expr, sys: &Sys) -> bool {
    match e {
        Expr::Node(nd) => {
            if let Expr::Sym(id) = &nd.head {
                if *id == sys.blank
                    || *id == sys.blanksequence
                    || *id == sys.blanknullsequence
                    || *id == sys.pattern
                    || *id == sys.optional
                {
                    return true;
                }
                if *id == sys.patterntest || *id == sys.repeated || *id == sys.repeatednull {
                    return nd.args.first().map_or(false, |a| is_pattern_expr(a, sys));
                }
                if *id == sys.alternatives {
                    return !nd.args.is_empty()
                        && nd.args.iter().all(|a| is_pattern_expr(a, sys));
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmt::fullform;

    fn ff(src: &str) -> String {
        let mut t = SymbolTable::new();
        match parse_input(&mut t, src) {
            Ok(e) => fullform(&t, &e),
            Err(e) => format!("ERROR: {}", e.msg),
        }
    }

    #[test]
    fn arithmetic_forms() {
        assert_eq!(ff("1 + 2"), "Plus[1, 2]");
        assert_eq!(ff("a - b"), "Plus[a, Times[-1, b]]");
        assert_eq!(ff("a/b"), "Times[a, Power[b, -1]]");
        assert_eq!(ff("2^3^4"), "Power[2, Power[3, 4]]");
        assert_eq!(ff("-x^2"), "Times[-1, Power[x, 2]]");
        assert_eq!(ff("2x"), "Times[2, x]");
        assert_eq!(ff("2x^2"), "Times[2, Power[x, 2]]");
        assert_eq!(ff("a . b"), "Dot[a, b]");
        assert_eq!(ff("a!"), "Factorial[a]");
        assert_eq!(ff("2/3*4"), "Times[2, Power[3, -1], 4]");
    }

    #[test]
    fn application_forms() {
        assert_eq!(ff("f[x, y]"), "f[x, y]");
        assert_eq!(ff("f@x + y"), "f[Plus[x, y]]"); // classic gotcha
        assert_eq!(ff("x // f"), "f[x]");
        assert_eq!(ff("f@x^2"), "f[Power[x, 2]]");
        assert_eq!(ff("f@@x"), "Apply[f, x]");
        assert_eq!(ff("f@@@x"), "Apply[f, x, List[1]]");
        assert_eq!(ff("f /@ x"), "Map[f, x]");
        assert_eq!(ff("#^2 & /@ l"), "Map[Function[Power[Slot[1], 2]], l]");
        assert_eq!(ff("f@#&"), "Function[f[Slot[1]]]");
        assert_eq!(ff("f = #^2 &"), "Set[f, Function[Power[Slot[1], 2]]]");
        assert_eq!(ff("a~f~b"), "f[a, b]");
        assert_eq!(ff("f@*g"), "Composition[f, g]");
    }

    #[test]
    fn patterns_forms() {
        assert_eq!(ff("f[x_]"), "f[Pattern[x, Blank[]]]");
        assert_eq!(ff("f[x_Integer]"), "f[Pattern[x, Blank[Integer]]]");
        assert_eq!(ff("x_?IntegerQ"), "PatternTest[Pattern[x, Blank[]], IntegerQ]");
        assert_eq!(ff("x_ /; x > 0"), "Condition[Pattern[x, Blank[]], Greater[x, 0]]");
        assert_eq!(ff("f[x__]"), "f[Pattern[x, BlankSequence[]]]");
        assert_eq!(ff("f[x___]"), "f[Pattern[x, BlankNullSequence[]]]");
        assert_eq!(ff("f[x_: 1]"), "f[Optional[Pattern[x, Blank[]], 1]]");
        assert_eq!(ff("f[x_.]"), "f[Optional[Pattern[x, Blank[]]]]");
        assert_eq!(ff("a | b"), "Alternatives[a, b]");
        assert_eq!(ff("x : _Integer"), "Pattern[x, Blank[Integer]]");
        assert_eq!(ff("f[x_ /; x > 0]"), "f[Condition[Pattern[x, Blank[]], Greater[x, 0]]]");
    }

    #[test]
    fn assignments_and_rules() {
        assert_eq!(ff("x = 5"), "Set[x, 5]");
        assert_eq!(ff("f[x_] := x^2"), "SetDelayed[f[Pattern[x, Blank[]]], Power[x, 2]]");
        assert_eq!(ff("a -> b"), "Rule[a, b]");
        assert_eq!(ff("a :> b"), "RuleDelayed[a, b]");
        assert_eq!(ff("x /. r"), "ReplaceAll[x, r]");
        assert_eq!(ff("x =."), "Unset[x]");
        assert_eq!(ff("x += 1"), "AddTo[x, 1]");
        assert_eq!(ff("f /: g[f[x_]] = v"), "TagSet[f, g[f[Pattern[x, Blank[]]]], v]");
    }

    #[test]
    fn structures() {
        assert_eq!(ff("{1, 2}"), "List[1, 2]");
        assert_eq!(ff("<|a -> 1|>"), "Association[Rule[a, 1]]");
        assert_eq!(ff("m[[1, 2]]"), "Part[m, 1, 2]");
        assert_eq!(ff("m[[1 ;; 3]]"), "Part[m, Span[1, 3]]");
        assert_eq!(ff("m[[;; 3]]"), "Part[m, Span[1, 3]]");
        assert_eq!(ff("m[[All, 2]]"), "Part[m, All, 2]");
        assert_eq!(ff("a; b; c"), "CompoundExpression[a, b, c]");
        assert_eq!(ff("f'"), "Derivative[1][f]");
        assert_eq!(ff("f''[x]"), "Derivative[2][f][x]");
        assert_eq!(ff("a::msg"), "MessageName[a, \"msg\"]");
        assert_eq!(ff("2^^101"), "5");
        assert_eq!(ff("\"hi\""), "\"hi\"");
        assert_eq!(ff("f[]"), "f[]");
    }

    #[test]
    fn misc_operators() {
        assert_eq!(ff("a == b"), "Equal[a, b]");
        assert_eq!(ff("a && b || c"), "Or[And[a, b], c]");
        assert_eq!(ff("!x"), "Not[x]");
        assert_eq!(ff("x /. a -> b"), "ReplaceAll[x, Rule[a, b]]");
        assert_eq!(ff("a;;b;;c"), "Span[a, b, c]");
        assert_eq!(ff("-x^2"), "Times[-1, Power[x, 2]]");
        assert_eq!(ff("2^-3"), "Power[2, Times[-1, 3]]");
        assert_eq!(ff("f (x + y)"), "Times[f, Plus[x, y]]");
        assert_eq!(ff("f (x) (y)"), "Times[f, x, y]");
        assert_eq!(ff("a.b.c"), "Dot[a, b, c]");
    }

    #[test]
    fn precedence_of_replace_and_set() {
        assert_eq!(ff("a = b /. r"), "Set[a, ReplaceAll[b, r]]");
        assert_eq!(ff("f[x_] := x /; x > 0"), "SetDelayed[f[Pattern[x, Blank[]]], Condition[x, Greater[x, 0]]]");
        assert_eq!(ff("x /. r //. r2"), "ReplaceRepeated[ReplaceAll[x, r], r2]");
    }
}
#[cfg(test)]
mod relation_chain_tests {
    use super::*;
    use crate::fmt::fullform;

    fn ff(src: &str) -> String {
        let mut t = SymbolTable::new();
        match parse_input(&mut t, src) {
            Ok(e) => fullform(&t, &e),
            Err(e) => format!("ERROR: {}", e.msg),
        }
    }

    #[test]
    fn chains_are_flat() {
        assert_eq!(ff("1 > 2 > 3"), "Greater[1, 2, 3]");
        assert_eq!(ff("1 < 2 < 3 < 4"), "Less[1, 2, 3, 4]");
        assert_eq!(ff("1 == 1 == 1"), "Equal[1, 1, 1]");
        assert_eq!(ff("1 != 2 != 3"), "Unequal[1, 2, 3]");
        // A single relation is unchanged.
        assert_eq!(ff("1 > 2"), "Greater[1, 2]");
        // A mixed chain is the conjunction of its neighbouring pairs.
        assert_eq!(ff("a < b > c"), "And[Less[a, b], Greater[b, c]]");
        // Relations still bind looser than arithmetic and tighter than &&.
        assert_eq!(ff("1 + 1 > 2"), "Greater[Plus[1, 1], 2]");
    }
}
