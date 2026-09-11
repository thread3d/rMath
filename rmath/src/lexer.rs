//! Lexer for Mathematica-style input.

#[derive(Clone, Debug, PartialEq)]
pub enum TokKind {
    // literals (payload in Tok.text)
    Int,
    Real,
    Str,
    Sym,
    // punctuation / operators
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    DblLBracket, // [[
    DblRBracket, // ]]
    LAssoc,      // <|
    RAssoc,      // |>
    Comma,
    Semi,
    SemiSemi, // ;;
    Colon,
    ColonColon, // ::
    Question,
    At,
    AtAt,
    AtAtAt,
    SlashAt,      // /@
    SlashSlashAt, // //@
    SlashAtAt,    // /@@ (MapApply)
    Comp,         // @*
    RightComp,    // /*
    Rule,         // ->
    RuleDelayed,  // :>
    Set,          // =
    SetDelayed,   // :=
    Unset,        // =.
    AddTo,        // +=
    SubtractFrom, // -=
    TimesBy,      // *=
    DivideBy,     // /=
    UpSet,        // ^=
    UpSetDelayed, // ^:=
    TagSetMark,   // /:
    ReplaceAll,   // /.
    ReplaceRepeated, // //.
    SlashSemi,    // /; (Condition)
    SlashSlash,   // // (Postfix)
    Put,          // >> (write file)
    PutAppend,    // >>> (AppendToFile)
    PlusPlus,     // ++ (parser: prefix or postfix)
    MinusMinus,   // -- (parser: prefix or postfix)
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Dot,
    DotDot,    // .. (Repeated)
    DotDotDot, // ... (RepeatedNull)
    Underscore,
    Underscore2,
    Underscore3,
    Bang,
    BangBang,
    Quote,
    Amp,
    AmpAmp,
    BarBar,
    Bar,
    BarArrow, // |->
    Less,
    Greater,
    LessEq,
    GreaterEq,
    EqEq,
    SameEq,
    BangEq,
    UnsameEq,
    Percent,
    Slot,
    StrJoin, // <>
    Tilde,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: TokKind,
    /// symbol name, string contents, or numeric payload text
    pub text: String,
    /// whitespace immediately before this token (comments don't count)
    pub space_before: bool,
    pub line: u32,
}

#[derive(Debug)]
pub struct LexError {
    pub msg: String,
    pub line: u32,
}

pub fn lex(src: &str) -> Result<Vec<Tok>, LexError> {
    Lexer::new(src).run()
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
    line: u32,
    toks: Vec<Tok>,
    space: bool,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Lexer<'a> {
        Lexer { s: src.as_bytes(), i: 0, line: 1, toks: Vec::new(), space: false }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn peek2(&self) -> Option<u8> {
        self.s.get(self.i + 1).copied()
    }
    fn peek3(&self) -> Option<u8> {
        self.s.get(self.i + 2).copied()
    }
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek();
        if let Some(c) = c {
            if c == b'\n' {
                self.line += 1;
            }
            self.i += 1;
        }
        c
    }

    fn push(&mut self, kind: TokKind, text: String) {
        self.toks.push(Tok { kind, text, space_before: self.space, line: self.line });
        self.space = false;
    }

    fn err(&self, msg: &str) -> LexError {
        LexError { msg: msg.to_string(), line: self.line }
    }

    fn run(mut self) -> Result<Vec<Tok>, LexError> {
        loop {
            // whitespace
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') => {
                    self.space = true;
                    self.bump();
                    continue;
                }
                Some(b'\n') => {
                    self.bump();
                    self.space = true;
                    continue;
                }
                _ => {}
            }
            // comments (* ... *) with nesting
            if self.peek() == Some(b'(') && self.peek2() == Some(b'*') {
                self.bump();
                self.bump();
                let mut depth = 1usize;
                loop {
                    match self.peek() {
                        None => return Err(self.err("unterminated comment")),
                        Some(b'(') if self.peek2() == Some(b'*') => {
                            self.bump();
                            self.bump();
                            depth += 1;
                        }
                        Some(b'*') if self.peek2() == Some(b')') => {
                            self.bump();
                            self.bump();
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Some(_) => {
                            self.bump();
                        }
                    }
                }
                self.space = true;
                continue;
            }
            let c = match self.peek() {
                None => {
                    self.push(TokKind::Eof, String::new());
                    return Ok(self.toks);
                }
                Some(c) => c,
            };
            match c {
                b'0'..=b'9' => self.number()?,
                b'.' if self.peek2().map_or(false, |c| c.is_ascii_digit()) => self.number()?,
                b'A'..=b'Z' | b'a'..=b'z' => self.ident()?,
                b'`' => {
                    self.ident()?
                }
                b'"' => self.string()?,
                b'(' => {
                    self.bump();
                    self.push(TokKind::LParen, String::new());
                }
                b')' => {
                    self.bump();
                    self.push(TokKind::RParen, String::new());
                }
                b'{' => {
                    self.bump();
                    self.push(TokKind::LBrace, String::new());
                }
                b'}' => {
                    self.bump();
                    self.push(TokKind::RBrace, String::new());
                }
                b'[' => {
                    if self.peek2() == Some(b'[') {
                        self.bump();
                        self.bump();
                        self.push(TokKind::DblLBracket, String::new());
                    } else {
                        self.bump();
                        self.push(TokKind::LBracket, String::new());
                    }
                }
                b']' => {
                    self.bump();
                    self.push(TokKind::RBracket, String::new());
                }
                b',' => {
                    self.bump();
                    self.push(TokKind::Comma, String::new());
                }
                b';' => {
                    if self.peek2() == Some(b';') {
                        self.bump();
                        self.bump();
                        self.push(TokKind::SemiSemi, String::new());
                    } else {
                        self.bump();
                        self.push(TokKind::Semi, String::new());
                    }
                }
                b':' => match self.peek2() {
                    Some(b':') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::ColonColon, String::new());
                    }
                    Some(b'>') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::RuleDelayed, String::new());
                    }
                    Some(b'=') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::SetDelayed, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Colon, String::new());
                    }
                },
                b'?' => {
                    self.bump();
                    self.push(TokKind::Question, String::new());
                }
                b'@' => match self.peek2() {
                    Some(b'*') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::Comp, String::new());
                    }
                    Some(b'@') => match self.peek3() {
                        Some(b'@') => {
                            self.bump();
                            self.bump();
                            self.bump();
                            self.push(TokKind::AtAtAt, String::new());
                        }
                        _ => {
                            self.bump();
                            self.bump();
                            self.push(TokKind::AtAt, String::new());
                        }
                    },
                    _ => {
                        self.bump();
                        self.push(TokKind::At, String::new());
                    }
                },
                b'/' => match (self.peek2(), self.peek3()) {
                    (Some(b'/'), Some(b'@')) => {
                        self.bump();
                        self.bump();
                        self.bump();
                        self.push(TokKind::SlashSlashAt, String::new());
                    }
                    (Some(b'/'), Some(b'.')) => {
                        self.bump();
                        self.bump();
                        self.bump();
                        self.push(TokKind::ReplaceRepeated, String::new());
                    }
                    (Some(b'/'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::SlashSlash, String::new());
                    }
                    (Some(b'@'), Some(b'@')) => {
                        self.bump();
                        self.bump();
                        self.bump();
                        self.push(TokKind::SlashAtAt, String::new());
                    }
                    (Some(b'@'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::SlashAt, String::new());
                    }
                    (Some(b'.'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::ReplaceAll, String::new());
                    }
                    (Some(b';'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::SlashSemi, String::new());
                    }
                    (Some(b':'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::TagSetMark, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Slash, String::new());
                    }
                },
                b'-' => match self.peek2() {
                    Some(b'-') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::MinusMinus, String::new());
                    }
                    Some(b'=') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::SubtractFrom, String::new());
                    }
                    Some(b'>') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::Rule, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Minus, String::new());
                    }
                },
                b'+' => match self.peek2() {
                    Some(b'+') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::PlusPlus, String::new());
                    }
                    Some(b'=') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::AddTo, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Plus, String::new());
                    }
                },
                b'*' => match self.peek2() {
                    Some(b'=') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::TimesBy, String::new());
                    }
                    Some(b'^') => {
                        return Err(self.err("unexpected *^ (only valid in numbers)"));
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Star, String::new());
                    }
                },
                b'^' => match (self.peek2(), self.peek3()) {
                    (Some(b':'), Some(b'=')) => {
                        self.bump();
                        self.bump();
                        self.bump();
                        self.push(TokKind::UpSetDelayed, String::new());
                    }
                    (Some(b'='), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::UpSet, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Caret, String::new());
                    }
                },
                b'=' => match (self.peek2(), self.peek3()) {
                    (Some(b'='), Some(b'=')) => {
                        self.bump();
                        self.bump();
                        self.bump();
                        self.push(TokKind::SameEq, String::new());
                    }
                    (Some(b'!'), Some(b'=')) => {
                        self.bump();
                        self.bump();
                        self.bump();
                        self.push(TokKind::UnsameEq, String::new());
                    }
                    (Some(b'='), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::EqEq, String::new());
                    }
                    (Some(b'.'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::Unset, String::new());
                    }
                    (Some(b':'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::SetDelayed, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Set, String::new());
                    }
                },
                b'!' => match self.peek2() {
                    Some(b'!') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::BangBang, String::new());
                    }
                    Some(b'=') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::BangEq, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Bang, String::new());
                    }
                },
                b'<' => match self.peek2() {
                    Some(b'=') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::LessEq, String::new());
                    }
                    Some(b'|') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::LAssoc, String::new());
                    }
                    Some(b'>') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::StrJoin, String::new());
                    }
                    Some(b'-') if self.peek3() == Some(b'>') => {
                        return Err(self.err("<-> not supported"));
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Less, String::new());
                    }
                },
                b'>' => match self.peek2() {
                    Some(b'=') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::GreaterEq, String::new());
                    }
                    Some(b'>') => match self.peek3() {
                        Some(b'>') => {
                            self.bump();
                            self.bump();
                            self.bump();
                            self.push(TokKind::PutAppend, String::new());
                        }
                        _ => {
                            self.bump();
                            self.bump();
                            self.push(TokKind::Put, String::new());
                        }
                    },
                    _ => {
                        self.bump();
                        self.push(TokKind::Greater, String::new());
                    }
                },
                b'|' => match (self.peek2(), self.peek3()) {
                    (Some(b'-'), Some(b'>')) => {
                        self.bump();
                        self.bump();
                        self.bump();
                        self.push(TokKind::BarArrow, String::new());
                    }
                    (Some(b'|'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::BarBar, String::new());
                    }
                    (Some(b'>'), _) => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::RAssoc, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Bar, String::new());
                    }
                },
                b'&' => match self.peek2() {
                    Some(b'&') => {
                        self.bump();
                        self.bump();
                        self.push(TokKind::AmpAmp, String::new());
                    }
                    _ => {
                        self.bump();
                        self.push(TokKind::Amp, String::new());
                    }
                },
                b'%' => {
                    self.bump();
                    let mut digits = String::new();
                    while self.peek().map_or(false, |c| c.is_ascii_digit()) {
                        digits.push(self.bump().unwrap() as char);
                    }
                    self.push(TokKind::Percent, digits);
                }
                b'#' => {
                    self.bump();
                    let mut text = String::new();
                    if self.peek() == Some(b'#') {
                        self.bump();
                        text.push('#');
                    }
                    if self.peek().map_or(false, |c| c.is_ascii_digit()) {
                        while self.peek().map_or(false, |c| c.is_ascii_digit()) {
                            text.push(self.bump().unwrap() as char);
                        }
                    } else if self.peek().map_or(false, |c| c.is_ascii_alphabetic()) {
                        while self
                            .peek()
                            .map_or(false, |c| c.is_ascii_alphanumeric() || c == b'`' || c == b'$')
                        {
                            text.push(self.bump().unwrap() as char);
                        }
                    }
                    self.push(TokKind::Slot, text);
                }
                b'_' => {
                    let n = if self.peek2() == Some(b'_') {
                        if self.peek3() == Some(b'_') {
                            3
                        } else {
                            2
                        }
                    } else {
                        1
                    };
                    for _ in 0..n {
                        self.bump();
                    }
                    let kind = match n {
                        1 => TokKind::Underscore,
                        2 => TokKind::Underscore2,
                        _ => TokKind::Underscore3,
                    };
                    self.push(kind, String::new());
                }
                b'~' => {
                    self.bump();
                    self.push(TokKind::Tilde, String::new());
                }
                b'\'' => {
                    self.bump();
                    self.push(TokKind::Quote, String::new());
                }
                b'.' => {
                    if self.peek2() == Some(b'.') && self.peek3() == Some(b'.') {
                        self.bump();
                        self.bump();
                        self.bump();
                        self.push(TokKind::DotDotDot, String::new());
                    } else if self.peek2() == Some(b'.') {
                        self.bump();
                        self.bump();
                        self.push(TokKind::DotDot, String::new());
                    } else {
                        self.bump();
                        self.push(TokKind::Dot, String::new());
                    }
                }
                b'\\' => {
                    return Err(self.err("unexpected backslash"));
                }
                _ => {
                    return Err(self.err(&format!("unexpected character {:?}", c as char)));
                }
            }
        }
    }

    fn number(&mut self) -> Result<(), LexError> {
        let start = self.i;
        let mut has_dot = false;
        if self.peek() == Some(b'.') {
            self.bump();
            has_dot = true;
        }
        while self.peek().map_or(false, |c| c.is_ascii_digit()) {
            self.bump();
        }
        if !has_dot && self.peek() == Some(b'.') {
            self.bump();
            has_dot = true;
            while self.peek().map_or(false, |c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        let mut text = String::from_utf8_lossy(&self.s[start..self.i]).to_string();
        // base notation: 2^^101
        if self.peek() == Some(b'^') && self.peek2() == Some(b'^') {
            self.bump();
            self.bump();
            let bstart = self.i;
            while self.peek().map_or(false, |c| c.is_ascii_alphanumeric()) {
                self.bump();
            }
            let base: u32 = text.parse().map_err(|_| self.err("bad base"))?;
            if !(2..=36).contains(&base) {
                return Err(self.err("base must be between 2 and 36"));
            }
            let digits = String::from_utf8_lossy(&self.s[bstart..self.i]).to_string();
            let val = crate::bigint::BigInt::from_radix(&digits, base)
                .ok_or_else(|| self.err("bad digits for base"))?;
            self.push(TokKind::Int, val.to_string_dec());
            return Ok(());
        }
        // exponent: mantissa *^ [+/-] digits
        if self.peek() == Some(b'*') && self.peek2() == Some(b'^') {
            self.bump();
            self.bump();
            let mut exp = String::new();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                exp.push(self.bump().unwrap() as char);
            }
            if !self.peek().map_or(false, |c| c.is_ascii_digit()) {
                return Err(self.err("bad *^ exponent"));
            }
            while self.peek().map_or(false, |c| c.is_ascii_digit()) {
                exp.push(self.bump().unwrap() as char);
            }
            let expv: i64 = exp.parse().unwrap_or(0);
            if has_dot || expv < 0 {
                let f: f64 = format!("{}e{}", text, exp).parse().unwrap_or(f64::NAN);
                self.push(TokKind::Real, format_real(f));
            } else {
                if expv > 100_000 {
                    return Err(self.err("exponent too large"));
                }
                let e = expv as u32;
                let mant = crate::bigint::BigInt::from_str_dec(&text)
                    .ok_or_else(|| self.err("bad number"))?;
                // value = mant * 10^e, computed in chunks to avoid overflow
                let mut val = mant;
                let mut left = e;
                while left > 0 {
                    let step = left.min(9);
                    val = val.mul_small(10u32.pow(step));
                    left -= step;
                }
                self.push(TokKind::Int, val.to_string_dec());
            }
            return Ok(());
        }
        if has_dot {
            let f: f64 = text.parse().map_err(|_| self.err("bad real"))?;
            self.push(TokKind::Real, format_real(f));
        } else {
            self.push(TokKind::Int, text);
        }
        Ok(())
    }

    fn ident(&mut self) -> Result<(), LexError> {
        let start = self.i;
        while self
            .peek()
            .map_or(false, |c| c.is_ascii_alphanumeric() || c == b'`' || c == b'$')
        {
            self.bump();
        }
        if start == self.i {
            return Err(self.err("empty identifier"));
        }
        let name = String::from_utf8_lossy(&self.s[start..self.i]).to_string();
        self.push(TokKind::Sym, name);
        Ok(())
    }

    fn string(&mut self) -> Result<(), LexError> {
        self.bump(); // opening quote
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.bump();
                    break;
                }
                Some(b'\\') => {
                    self.bump();
                    match self.peek() {
                        Some(b'n') => {
                            out.push('\n');
                            self.bump();
                        }
                        Some(b't') => {
                            out.push('\t');
                            self.bump();
                        }
                        Some(b'r') => {
                            out.push('\r');
                            self.bump();
                        }
                        Some(b'\\') => {
                            out.push('\\');
                            self.bump();
                        }
                        Some(b'"') => {
                            out.push('"');
                            self.bump();
                        }
                        Some(b'[') => {
                            self.bump();
                            let mut name = String::new();
                            while self.peek().map_or(false, |c| c != b']') {
                                name.push(self.bump().unwrap() as char);
                            }
                            self.bump(); // ]
                            out.push_str(&named_char(&name));
                        }
                        Some(b':') => {
                            self.bump();
                            let mut hex = String::new();
                            for _ in 0..4 {
                                if self.peek().map_or(false, |c| c.is_ascii_hexdigit()) {
                                    hex.push(self.bump().unwrap() as char);
                                } else {
                                    break;
                                }
                            }
                            if hex.len() == 4 {
                                if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                                    if let Some(ch) = char::from_u32(cp) {
                                        out.push(ch);
                                    }
                                }
                            }
                        }
                        Some(b'\n') => {
                            self.bump(); // line continuation
                        }
                        _ => {
                            out.push('\\');
                            if let Some(c) = self.bump() {
                                out.push(c as char);
                            }
                        }
                    }
                }
                Some(_) => {
                    let rest = std::str::from_utf8(&self.s[self.i..]).unwrap_or("");
                    let ch = rest.chars().next().unwrap_or('\u{fffd}');
                    out.push(ch);
                    for _ in 0..ch.len_utf8() {
                        self.bump();
                    }
                }
            }
        }
        self.push(TokKind::Str, out);
        Ok(())
    }
}

/// A selection of common named characters.
fn named_char(name: &str) -> String {
    let s = match name {
        "Alpha" => "\u{03b1}",
        "Beta" => "\u{03b2}",
        "Gamma" => "\u{03b3}",
        "Delta" => "\u{03b4}",
        "Epsilon" => "\u{03b5}",
        "Zeta" => "\u{03b6}",
        "Eta" => "\u{03b7}",
        "Theta" => "\u{03b8}",
        "Iota" => "\u{03b9}",
        "Kappa" => "\u{03ba}",
        "Lambda" => "\u{03bb}",
        "Mu" => "\u{03bc}",
        "Nu" => "\u{03bd}",
        "Xi" => "\u{03be}",
        "Omicron" => "\u{03bf}",
        "Pi" => "\u{03c0}",
        "Rho" => "\u{03c1}",
        "Sigma" => "\u{03c3}",
        "Tau" => "\u{03c4}",
        "Upsilon" => "\u{03c5}",
        "Phi" => "\u{03c6}",
        "Chi" => "\u{03c7}",
        "Psi" => "\u{03c8}",
        "Omega" => "\u{03c9}",
        "Infinity" => "\u{221e}",
        "EmptySet" => "\u{2205}",
        "Element" => "\u{2208}",
        "Sqrt" => "\u{221a}",
        "PartialD" => "\u{2202}",
        "Nabla" => "\u{2207}",
        "Integral" => "\u{222b}",
        "Sum" => "\u{2211}",
        "Product" => "\u{220f}",
        "Times" => "\u{00d7}",
        "Divide" => "\u{00f7}",
        "PlusMinus" => "\u{00b1}",
        "Degree" => "\u{00b0}",
        "Rule" => "\u{2192}",
        "RuleDelayed" => "\u{21a6}",
        "RightArrow" => "\u{2192}",
        "LeftArrow" => "\u{2190}",
        "Equal" => "\u{2261}",
        "LessSlantEqual" => "\u{2264}",
        "GreaterSlantEqual" => "\u{2265}",
        "NotEqual" => "\u{2260}",
        "And" => "\u{2227}",
        "Or" => "\u{2228}",
        "Not" => "\u{00ac}",
        "ForAll" => "\u{2200}",
        "Exists" => "\u{2203}",
        "CapitalDelta" => "\u{0394}",
        "CapitalOmega" => "\u{03a9}",
        "DifferentialD" => "\u{2146}",
        "ExponentialE" => "\u{2147}",
        "ImaginaryI" => "\u{2148}",
        "ImaginaryJ" => "\u{2149}",
        "CurlyPi" => "\u{03c0}",
        _ => return format!("\\[{}]", name),
    };
    s.to_string()
}

/// Real tokens carry a round-trip-accurate decimal text; parsed to f64 later.
fn format_real(f: f64) -> String {
    format!("{}", f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokKind> {
        lex(src)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| *k != TokKind::Eof)
            .collect()
    }

    #[test]
    fn basics() {
        assert_eq!(kinds("1 + 2"), vec![TokKind::Int, TokKind::Plus, TokKind::Int]);
        assert_eq!(
            kinds("f[x]"),
            vec![TokKind::Sym, TokKind::LBracket, TokKind::Sym, TokKind::RBracket]
        );
        assert_eq!(
            kinds("{a, b}"),
            vec![TokKind::LBrace, TokKind::Sym, TokKind::Comma, TokKind::Sym, TokKind::RBrace]
        );
    }

    #[test]
    fn operators() {
        assert_eq!(kinds("a -> b"), vec![TokKind::Sym, TokKind::Rule, TokKind::Sym]);
        assert_eq!(kinds("x := 1"), vec![TokKind::Sym, TokKind::SetDelayed, TokKind::Int]);
        assert_eq!(kinds("a /. r"), vec![TokKind::Sym, TokKind::ReplaceAll, TokKind::Sym]);
        assert_eq!(kinds("a // f"), vec![TokKind::Sym, TokKind::SlashSlash, TokKind::Sym]);
        assert_eq!(kinds("a //. r"), vec![TokKind::Sym, TokKind::ReplaceRepeated, TokKind::Sym]);
        assert_eq!(kinds("#^2&"), vec![TokKind::Slot, TokKind::Caret, TokKind::Int, TokKind::Amp]);
        assert_eq!(
            kinds("a[[1]]"),
            vec![TokKind::Sym, TokKind::DblLBracket, TokKind::Int, TokKind::RBracket, TokKind::RBracket]
        );
        assert_eq!(
            kinds("<|a -> 1|>"),
            vec![TokKind::LAssoc, TokKind::Sym, TokKind::Rule, TokKind::Int, TokKind::RAssoc]
        );
        assert_eq!(kinds("f@x"), vec![TokKind::Sym, TokKind::At, TokKind::Sym]);
        assert_eq!(kinds("f@*g"), vec![TokKind::Sym, TokKind::Comp, TokKind::Sym]);
        assert_eq!(kinds("f @@ x"), vec![TokKind::Sym, TokKind::AtAt, TokKind::Sym]);
        assert_eq!(kinds("f @@@ x"), vec![TokKind::Sym, TokKind::AtAtAt, TokKind::Sym]);
        assert_eq!(kinds("a ~f~ b"), vec![TokKind::Sym, TokKind::Tilde, TokKind::Sym, TokKind::Tilde, TokKind::Sym]);
        assert_eq!(kinds("x_ /; x > 0"), vec![
            TokKind::Sym, TokKind::Underscore, TokKind::SlashSemi, TokKind::Sym,
            TokKind::Greater, TokKind::Int
        ]);
        assert_eq!(kinds("a;;b"), vec![TokKind::Sym, TokKind::SemiSemi, TokKind::Sym]);
    }

    #[test]
    fn numbers() {
        let toks = lex("1.5*^-3 2^^101 .5 123. 1*^3").unwrap();
        assert_eq!(toks[0].kind, TokKind::Real);
        assert_eq!(toks[0].text, "0.0015");
        assert_eq!(toks[1].kind, TokKind::Int);
        assert_eq!(toks[1].text, "5");
        assert_eq!(toks[2].kind, TokKind::Real);
        assert_eq!(toks[2].text, "0.5");
        assert_eq!(toks[3].kind, TokKind::Real);
        assert_eq!(toks[4].kind, TokKind::Int);
        assert_eq!(toks[4].text, "1000");
    }

    #[test]
    fn strings_and_comments() {
        let toks = lex("\"a\\\"b\" (* c *) x").unwrap();
        assert_eq!(toks[0].kind, TokKind::Str);
        assert_eq!(toks[0].text, "a\"b");
        assert_eq!(toks[1].kind, TokKind::Sym);
        assert!(toks[1].space_before);
    }

    #[test]
    fn slots_and_out() {
        let toks = lex("# ## #name ##2 %5 %%").unwrap();
        assert_eq!(toks[0].kind, TokKind::Slot);
        assert_eq!(toks[0].text, "");
        assert_eq!(toks[1].kind, TokKind::Slot);
        assert_eq!(toks[1].text, "#");
        assert_eq!(toks[2].kind, TokKind::Slot);
        assert_eq!(toks[2].text, "name");
        assert_eq!(toks[3].kind, TokKind::Slot);
        assert_eq!(toks[3].text, "#2");
        assert_eq!(toks[4].kind, TokKind::Percent);
        assert_eq!(toks[4].text, "5");
        assert_eq!(toks[5].kind, TokKind::Percent);
        assert_eq!(toks[5].text, "");
        // two consecutive percents lex as two tokens; parser counts them
        assert_eq!(kinds("%%"), vec![TokKind::Percent, TokKind::Percent]);
    }
}