//! Expression printers: FullForm, OutputForm, InputForm.

use crate::expr::{same_q, Expr, NumVal, SymbolTable};

/// FullForm: fully spelled-out functional notation, like Mathematica.
pub fn fullform(t: &SymbolTable, e: &Expr) -> String {
    let mut s = String::new();
    fullform_into(t, e, &mut s);
    s
}

fn fullform_into(t: &SymbolTable, e: &Expr, out: &mut String) {
    match e {
        Expr::Int(n) => out.push_str(&n.to_string()),
        Expr::Rat(r) => {
            out.push_str("Rational[");
            out.push_str(&r.num.to_string());
            out.push_str(", ");
            out.push_str(&r.den.to_string());
            out.push(']');
        }
        Expr::Real(x) => out.push_str(&format!("{}", x)),
        Expr::Str(s) => {
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Expr::Sym(i) => out.push_str(t.name_of(*i)),
        Expr::Cnum(c) => {
            out.push_str("Complex[");
            fullform_into(t, &numval_to_expr(&c.re), out);
            out.push_str(", ");
            fullform_into(t, &numval_to_expr(&c.im), out);
            out.push(']');
        }
        Expr::Node(nd) => {
            fullform_into(t, &nd.head, out);
            out.push('[');
            for (i, a) in nd.args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                fullform_into(t, a, out);
            }
            out.push(']');
        }
    }
}

pub fn numval_to_expr(n: &NumVal) -> Expr {
    match n {
        NumVal::R(r) => Expr::rat(r.clone()),
        NumVal::F(f) => Expr::real(*f),
    }
}

// ---------- InputForm ----------

pub fn input_form(t: &SymbolTable, e: &Expr) -> String {
    let mut s = String::new();
    input_into(t, e, 0, &mut s);
    s
}

fn input_into(t: &SymbolTable, e: &Expr, _depth: usize, out: &mut String) {
    match e {
        Expr::Int(n) => out.push_str(&n.to_string()),
        Expr::Rat(r) => {
            out.push_str(&r.num.to_string());
            out.push('/');
            out.push_str(&r.den.to_string());
        }
        Expr::Real(x) => out.push_str(&format!("{}", x)),
        Expr::Str(s) => {
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Expr::Sym(i) => out.push_str(t.name_of(*i)),
        Expr::Cnum(c) => {
            out.push_str("Complex[");
            input_into(t, &numval_to_expr(&c.re), 0, out);
            out.push_str(", ");
            input_into(t, &numval_to_expr(&c.im), 0, out);
            out.push(']');
        }
        Expr::Node(nd) => {
            input_into(t, &nd.head, 0, out);
            out.push('[');
            for (i, a) in nd.args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                input_into(t, a, 0, out);
            }
            out.push(']');
        }
    }
}

// ---------- OutputForm (ASCII, Mathematica-like infix) ----------

pub fn output_form(t: &SymbolTable, e: &Expr) -> String {
    let mut s = String::new();
    output_into(t, e, &mut s);
    s
}

/// Operator precedence ranks for OutputForm printing.
fn prec_of(t: &SymbolTable, e: &Expr) -> u8 {
    if let Some((h, _)) = e.parts() {
        if let Expr::Sym(id) = h {
            let sys = t.sys;
            if *id == sys.plus {
                return 10;
            }
            if *id == sys.times {
                return 20;
            }
            if *id == sys.power {
                return 30;
            }
            if *id == sys.rule || *id == sys.ruledelayed {
                return 8;
            }
            if *id == sys.equal || *id == sys.unequal || *id == sys.less || *id == sys.greater
                || *id == sys.lessequal || *id == sys.greaterequal || *id == sys.sameq
                || *id == sys.unsameq
            {
                return 12;
            }
            if *id == sys.and {
                return 14;
            }
            if *id == sys.or {
                return 13;
            }
        }
    }
    99
}

fn output_into(t: &SymbolTable, e: &Expr, out: &mut String) {
    match e {
        Expr::Int(n) => out.push_str(&n.to_string()),
        Expr::Rat(r) => {
            out.push_str(&r.num.to_string());
            out.push('/');
            out.push_str(&r.den.to_string());
        }
        Expr::Real(x) => out.push_str(&format_real_out(*x)),
        Expr::Str(s) => {
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Expr::Sym(i) => out.push_str(t.name_of(*i)),
        Expr::Cnum(c) => {
            // Mathematica's OutputForm: I, -I, 2 I, 1 + I, 1 - I, 3 + 2 I.
            let re_zero = is_zero_nv(&c.re);
            if re_zero {
                if is_exact_one_nv(&c.im) {
                    out.push('I');
                } else if is_exact_neg_one_nv(&c.im) {
                    out.push_str("-I");
                } else {
                    output_into(t, &numval_to_expr(&c.im), out);
                    out.push_str(" I");
                }
                return;
            }
            let re = numval_to_expr(&c.re);
            output_into(t, &re, out);
            if is_exact_one_nv(&c.im) {
                out.push_str(" + I");
            } else if is_exact_neg_one_nv(&c.im) {
                out.push_str(" - I");
            } else if !re_zero {
                out.push_str(" + ");
                output_into(t, &numval_to_expr(&c.im), out);
                out.push_str(" I");
            }
        }
        Expr::Node(nd) => {
            // Operator forms
            if let Expr::Sym(id) = &nd.head {
                let sys = t.sys;
                if *id == sys.plus && !nd.args.is_empty() {
                    let mut first = true;
                    for a in &nd.args {
                        let p = prec_of(t, a);
                        if first {
                            first = false;
                            if is_negative(t, a) {
                                out.push('-');
                                parenthesize(t, &strip_neg(t, a), 10, out);
                            } else {
                                output_into(t, a, out);
                            }
                        } else if is_negative(t, a) {
                            out.push_str(" - ");
                            parenthesize(t, &strip_neg(t, a), 10, out);
                        } else {
                            out.push_str(" + ");
                            parenthesize(t, a, 10, out);
                        }
                    }
                    return;
                }
                if *id == sys.times && !nd.args.is_empty() {
                    // Print negatives and 1/x specially.
                    let mut neg = false;
                    let mut denom: Vec<&Expr> = Vec::new();
                    let mut numer: Vec<Expr> = Vec::new();
                    for a in &nd.args {
                        if is_negative(t, a) {
                            neg = true;
                            numer.push(strip_neg(t, a));
                        } else if let Some((h, hargs)) = a.parts() {
                            if h.is_symbol(sys.power)
                                && hargs.len() == 2
                                && is_negative_one(&hargs[1])
                            {
                                denom.push(&hargs[0]);
                            } else {
                                numer.push(a.clone());
                            }
                        } else {
                            numer.push(a.clone());
                        }
                    }
                    if neg {
                        out.push('-');
                    }
                    if numer.is_empty() && !denom.is_empty() {
                        out.push('1');
                    }
                    let numer: Vec<Expr> = numer
                        .iter()
                        .filter(|a| !same_q(a, &Expr::int_i64(1)))
                        .cloned()
                        .collect();
                    for (k, a) in numer.iter().enumerate() {
                        if k > 0 {
                            out.push(' ');
                        }
                        parenthesize(t, a, 20, out);
                    }
                    if !denom.is_empty() {
                        out.push('/');
                        if denom.len() == 1 {
                            parenthesize(t, denom[0], 21, out);
                        } else {
                            let joined: Vec<Expr> = denom.iter().map(|d| (*d).clone()).collect();
                            let node = Expr::node(Expr::Sym(sys.times), joined);
                            parenthesize(t, &node, 21, out);
                        }
                    }
                    return;
                }
                if *id == sys.power && nd.args.len() == 2 {
                    // Half powers print as radicals (as in Mathematica) and
                    // other rational exponents are parenthesized, so `x^1/2`
                    // can never be misread as (x^1)/2.
                    match &nd.args[1] {
                        Expr::Rat(r) if r.num.to_i64() == Some(1) && r.den.to_i64() == Some(2) => {
                            out.push_str("Sqrt[");
                            output_into(t, &nd.args[0], out);
                            out.push(']');
                            return;
                        }
                        Expr::Rat(r)
                            if r.num.to_i64() == Some(-1) && r.den.to_i64() == Some(2) =>
                        {
                            out.push_str("1/Sqrt[");
                            output_into(t, &nd.args[0], out);
                            out.push(']');
                            return;
                        }
                        // Negative integer powers print as reciprocals
                        // (1/x, 1/x^2, 1/(2 x)), as in Mathematica.
                        Expr::Int(n) if n.to_i64().map_or(false, |v| v < 0) => {
                            let e = -n.to_i64().unwrap();
                            out.push_str("1/");
                            parenthesize(t, &nd.args[0], 21, out);
                            if e != 1 {
                                out.push('^');
                                out.push_str(&e.to_string());
                            }
                            return;
                        }
                        Expr::Rat(_) | Expr::Real(_) => {
                            parenthesize(t, &nd.args[0], 31, out);
                            out.push('^');
                            let mut tmp = String::new();
                            output_into(t, &nd.args[1], &mut tmp);
                            out.push('(');
                            out.push_str(&tmp);
                            out.push(')');
                            return;
                        }
                        _ => {}
                    }
                    parenthesize(t, &nd.args[0], 31, out);
                    out.push('^');
                    parenthesize(t, &nd.args[1], 30, out);
                    return;
                }
                if (*id == sys.rule || *id == sys.ruledelayed) && nd.args.len() == 2 {
                    parenthesize(t, &nd.args[0], 9, out);
                    out.push_str(if *id == sys.rule { " -> " } else { " :> " });
                    parenthesize(t, &nd.args[1], 9, out);
                    return;
                }
                if *id == sys.holdform && nd.args.len() == 1 {
                    output_into(t, &nd.args[0], out);
                    return;
                }
                if *id == sys.list {
                    out.push('{');
                    for (k, a) in nd.args.iter().enumerate() {
                        if k > 0 {
                            out.push_str(", ");
                        }
                        output_into(t, a, out);
                    }
                    out.push('}');
                    return;
                }
                if *id == sys.association {
                    out.push_str("<|");
                    for (k, a) in nd.args.iter().enumerate() {
                        if k > 0 {
                            out.push_str(", ");
                        }
                        output_into(t, a, out);
                    }
                    out.push_str("|>");
                    return;
                }
                if *id == graphics_sym(t) {
                    out.push_str("-Graphics-");
                    return;
                }
                if *id == sys.compoundexpression {
                    for (k, a) in nd.args.iter().enumerate() {
                        if k > 0 {
                            out.push_str("; ");
                        }
                        output_into(t, a, out);
                    }
                    return;
                }
                if *id == sys.blank && nd.args.is_empty() {
                    out.push('_');
                    return;
                }
                if (*id == sys.blanksequence || *id == sys.blanknullsequence) && nd.args.is_empty()
                {
                    out.push_str(if *id == sys.blanksequence { "__" } else { "___" });
                    return;
                }
            }
            // Generic call.
            output_into(t, &nd.head, out);
            out.push('[');
            for (k, a) in nd.args.iter().enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                output_into(t, a, out);
            }
            out.push(']');
        }
    }
}

fn graphics_sym(t: &SymbolTable) -> usize {
    if let Some(id) = t.get("Graphics") {
        id
    } else {
        usize::MAX
    }
}

fn parenthesize(t: &SymbolTable, e: &Expr, min_prec: u8, out: &mut String) {
    if prec_of(t, e) < min_prec {
        out.push('(');
        output_into(t, e, out);
        out.push(')');
    } else {
        output_into(t, e, out);
    }
}

fn is_negative(t: &SymbolTable, e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Rat(_) | Expr::Real(_) => {
            matches!(crate::builtins::num::exp_sign_pub(e), Some(-1))
        }
        // -I (and multiples of it) count as negative so a Plus prints
        // "a - I Sqrt[3]" rather than "a + -I Sqrt[3]".
        Expr::Cnum(c) => {
            let re_zero = match &c.re {
                crate::expr::NumVal::R(r) => r.is_zero(),
                crate::expr::NumVal::F(f) => *f == 0.0,
            };
            let im_neg = match &c.im {
                crate::expr::NumVal::R(r) => r.is_neg(),
                crate::expr::NumVal::F(f) => *f < 0.0,
            };
            re_zero && im_neg
        }
        _ => {
            if let Some((h, hargs)) = e.parts() {
                if h.is_symbol(t.sys.times) && !hargs.is_empty() {
                    return is_negative(t, &hargs[0]);
                }
            }
            false
        }
    }
}

fn strip_neg(t: &SymbolTable, e: &Expr) -> Expr {
    match e {
        Expr::Int(n) => return Expr::Int(n.clone().abs()),
        Expr::Rat(r) => return Expr::rat(r.clone().abs()),
        Expr::Real(x) => return Expr::real(-*x),
        Expr::Cnum(c) => {
            let re_zero = match &c.re {
                crate::expr::NumVal::R(r) => r.is_zero(),
                crate::expr::NumVal::F(f) => *f == 0.0,
            };
            let im_neg = match &c.im {
                crate::expr::NumVal::R(r) => r.is_neg(),
                crate::expr::NumVal::F(f) => *f < 0.0,
            };
            if re_zero && im_neg {
                let neg_im = crate::builtins::num::nv_neg(&c.im);
                return Expr::cnum(c.re.clone(), neg_im);
            }
        }
        _ => {}
    }
    if let Some((h, hargs)) = e.parts() {
        if h.is_symbol(t.sys.times) && !hargs.is_empty() && is_negative(t, e) {
            let mut args: Vec<Expr> = hargs.to_vec();
            let first = args[0].clone();
            args.remove(0);
            let positive = match first {
                Expr::Int(n) => Expr::Int(n.abs()),
                Expr::Rat(r) => Expr::rat(r.abs()),
                Expr::Real(x) => Expr::real(-x),
                Expr::Cnum(c) => Expr::cnum(c.re.clone(), crate::builtins::num::nv_neg(&c.im)),
                other => other,
            };
            if args.is_empty() {
                return positive;
            }
            let mut all = vec![positive];
            all.extend(args.iter().cloned());
            return Expr::node(Expr::Sym(t.sys.times), all);
        }
    }
    e.clone()
}

/// Exact +1 / -1 (machine values keep their decimal point, so 1. I prints as
/// "1. I" while the exact unit prints as "I").
fn is_exact_one_nv(n: &crate::expr::NumVal) -> bool {
    matches!(n, crate::expr::NumVal::R(r) if r.is_one_pub())
}

fn is_exact_neg_one_nv(n: &crate::expr::NumVal) -> bool {
    matches!(n, crate::expr::NumVal::R(r) if r.num.to_i64() == Some(-1) && r.den.to_i64() == Some(1))
}

/// True when the machine/exact value is -1.
fn is_negative_one_nv(n: &crate::expr::NumVal) -> bool {
    match n {
        crate::expr::NumVal::R(r) => r.num.to_i64() == Some(-1) && r.den.to_i64() == Some(1),
        crate::expr::NumVal::F(v) => *v == -1.0,
    }
}

/// True when the machine/exact value is 1.
fn is_one_nv(n: &crate::expr::NumVal) -> bool {
    match n {
        crate::expr::NumVal::R(r) => r.is_one_pub(),
        crate::expr::NumVal::F(v) => *v == 1.0,
    }
}

fn is_negative_one(e: &Expr) -> bool {
    matches!(e, Expr::Int(n) if n.to_i64() == Some(-1))
}

fn is_zero_nv(n: &crate::expr::NumVal) -> bool {
    crate::builtins::num::nv_is_zero(n)
}

/// OutputForm-style real formatting: 6 significant digits, *^ exponents.
pub fn format_real_out(x: f64) -> String {
    if x == 0.0 {
        return "0.".to_string();
    }
    let e = x.abs().log10().floor() as i32;
    let plain = e >= -4 && e < 6;
    if plain {
        let decimals = (5 - e).clamp(0, 12) as usize;
        let mut v = format!("{:.*}", decimals, x);
        if v.contains('.') {
            while v.ends_with('0') {
                v.pop();
            }
            if v.ends_with('.') {
                v.push('0');
            }
        }
        v
    } else {
        let m = x / 10f64.powi(e);
        let mut ms = format!("{:.5}", m);
        while ms.ends_with('0') {
            ms.pop();
        }
        if ms.ends_with('.') {
            ms.push('0');
        }
        format!("{}*^{}", ms, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{SymbolTable, Sys};

    #[test]
    fn fullform_basics() {
        let mut t = SymbolTable::new();
        let x = t.intern("x");
        let y = t.intern("y");
        // 1 + 2 x parsed then printed
        let e = Expr::node(
            Expr::Sym(t.sys.plus),
            vec![
                Expr::int_i64(1),
                Expr::node(
                    Expr::Sym(t.sys.times),
                    vec![Expr::int_i64(2), Expr::Sym(x)],
                ),
            ],
        );
        assert_eq!(fullform(&t, &e), "Plus[1, Times[2, x]]");
        let r = crate::rational::Rat::new(crate::bigint::BigInt::from_i64(1), crate::bigint::BigInt::from_i64(2));
        assert_eq!(fullform(&t, &Expr::rat(r)), "Rational[1, 2]");
        let _ = y;
        let _ = Sys::default();
    }
}