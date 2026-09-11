//! Minimal JSON value type with parser and writer (zero dependencies).
//! Objects preserve insertion order (Vec of pairs) for stable output.

use std::collections::HashMap;
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn obj() -> J {
        J::Obj(Vec::new())
    }
    pub fn set(mut self, key: &str, val: J) -> J {
        if let J::Obj(ref mut v) = self {
            v.push((key.to_string(), val));
        }
        self
    }
    pub fn get(&self, key: &str) -> Option<&J> {
        if let J::Obj(v) = self {
            v.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let J::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let J::Num(n) = self {
            Some(*n)
        } else {
            None
        }
    }
    pub fn as_arr(&self) -> Option<&Vec<J>> {
        if let J::Arr(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn str_of(s: &str) -> J {
        J::Str(s.to_string())
    }

    pub fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(true) => out.push_str("true"),
            J::Bool(false) => out.push_str("false"),
            J::Num(n) => {
                if n.is_finite() {
                    let _ = write!(out, "{}", n);
                } else {
                    out.push_str("null");
                }
            }
            J::Str(s) => write_escaped(s, out),
            J::Arr(items) => {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    it.write(out);
                }
                out.push(']');
            }
            J::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
    pub fn to_string(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }
}

fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

pub fn parse(input: &str) -> Result<J, String> {
    let mut p = P { s: input.as_bytes(), i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("trailing data at byte {}", p.i));
    }
    Ok(v)
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn value(&mut self, depth: usize) -> Result<J, String> {
        if depth > 64 {
            return Err("nesting too deep".into());
        }
        self.ws();
        match self.peek() {
            None => Err("unexpected end of input".into()),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(J::Str(self.string()?)),
            Some(b't') => self.lit("true", J::Bool(true)),
            Some(b'f') => self.lit("false", J::Bool(false)),
            Some(b'n') => self.lit("null", J::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected byte {:?} at {}", c as char, self.i)),
        }
    }
    fn lit(&mut self, word: &str, val: J) -> Result<J, String> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(val)
        } else {
            Err(format!("bad literal at byte {}", self.i))
        }
    }
    fn number(&mut self) -> Result<J, String> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while self.peek().map_or(false, |c| c.is_ascii_digit()) {
            self.i += 1;
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            while self.peek().map_or(false, |c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            while self.peek().map_or(false, |c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap();
        text.parse::<f64>().map(J::Num).map_err(|e| format!("bad number {:?}: {}", text, e))
    }
    fn string(&mut self) -> Result<String, String> {
        if self.peek() != Some(b'"') {
            return Err(format!("expected string at byte {}", self.i));
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated string".into()),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some(b'"') => out.push('"'),
                        Some(b'\\') => out.push('\\'),
                        Some(b'/') => out.push('/'),
                        Some(b'n') => out.push('\n'),
                        Some(b't') => out.push('\t'),
                        Some(b'r') => out.push('\r'),
                        Some(b'b') => out.push('\u{8}'),
                        Some(b'f') => out.push('\u{c}'),
                        Some(b'u') => {
                            self.i += 1;
                            let cp = self.hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&cp) {
                                // surrogate pair
                                if self.peek() == Some(b'\\') {
                                    self.i += 1;
                                    if self.peek() == Some(b'u') {
                                        self.i += 1;
                                        let lo = self.hex4()?;
                                        let c = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                        char::from_u32(c).ok_or("bad surrogate pair")?
                                    } else {
                                        return Err("bad \\u escape".into());
                                    }
                                } else {
                                    return Err("bad surrogate".into());
                                }
                            } else {
                                char::from_u32(cp).ok_or("bad \\u escape")?
                            };
                            out.push(ch);
                            continue;
                        }
                        _ => return Err("bad escape".into()),
                    }
                    self.i += 1;
                }
                Some(_) => {
                    // Consume one UTF-8 encoded character.
                    let rest = &self.s[self.i..];
                    let text = std::str::from_utf8(rest).map_err(|_| "bad utf8".to_string())?;
                    let ch = text.chars().next().unwrap();
                    out.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        if self.i + 4 > self.s.len() {
            return Err("bad \\u escape".into());
        }
        let hex = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap();
        self.i += 4;
        u32::from_str_radix(hex, 16).map_err(|_| "bad hex".into())
    }
    fn array(&mut self, depth: usize) -> Result<J, String> {
        self.i += 1; // [
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(J::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b']') => {
                    self.i += 1;
                    return Ok(J::Arr(items));
                }
                _ => return Err(format!("expected , or ] at byte {}", self.i)),
            }
        }
    }
    fn object(&mut self, depth: usize) -> Result<J, String> {
        self.i += 1; // {
        let mut pairs: Vec<(String, J)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(J::Obj(pairs));
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(format!("expected : at byte {}", self.i));
            }
            self.i += 1;
            let val = self.value(depth + 1)?;
            pairs.push((key, val));
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b'}') => {
                    self.i += 1;
                    return Ok(J::Obj(pairs));
                }
                _ => return Err(format!("expected , or }} at byte {}", self.i)),
            }
        }
    }
}

/// Convenience: build a JSON object from a slice of (key, J) pairs.
pub fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Convenience: look up in a parsed object.
pub fn map_from(j: &J) -> HashMap<String, J> {
    let mut m = HashMap::new();
    if let J::Obj(pairs) = j {
        for (k, v) in pairs {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = r#"{"id":7,"op":"eval","input":"1 + 2","esc":"a\"b\\c\n😀","arr":[1,2.5,null,true,"x"]}"#;
        let j = parse(src).unwrap();
        assert_eq!(j.get("id").unwrap().as_f64(), Some(7.0));
        assert_eq!(j.get("op").unwrap().as_str(), Some("eval"));
        assert_eq!(j.get("input").unwrap().as_str(), Some("1 + 2"));
        assert_eq!(j.get("esc").unwrap().as_str(), Some("a\"b\\c\n😀"));
        let out = j.to_string();
        let j2 = parse(&out).unwrap();
        assert_eq!(j, j2);
    }

    #[test]
    fn writer_escapes() {
        let j = J::obj().set("s", J::str_of("quote\" back\\ nl\n"));
        assert_eq!(j.to_string(), r#"{"s":"quote\" back\\ nl\n"}"#);
    }

    #[test]
    fn nested_and_errors() {
        let j = parse(r#"{"a":{"b":[1,[2,3]]}}"#).unwrap();
        assert_eq!(j.get("a").unwrap().get("b").unwrap().as_arr().unwrap().len(), 2);
        assert!(parse("{").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("{} extra").is_err());
        assert_eq!(parse("  [ ] ").unwrap(), J::Arr(vec![]));
    }
}