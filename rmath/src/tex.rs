//! TeX printer: converts expressions to LaTeX (like Mathematica's TeXForm),
//! with fractions, powers, roots, trig, greek letters, and pattern forms.

use crate::expr::{has, same_q, Expr, SymbolTable, A_CONSTANT};

/// Output conventions for the TeX printer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// As in Mathematica's StandardForm: `Sin[x]`, `Factor[x^2]`.
    Standard,
    /// As in TraditionalForm: `sin(x)`, italic function names.
    Traditional,
}

/// TeX in StandardForm (the default).
pub fn tex_form(t: &SymbolTable, e: &Expr) -> String {
    tex_form_as(t, e, Form::Standard)
}

/// TeX in the requested form.
pub fn tex_form_as(t: &SymbolTable, e: &Expr, form: Form) -> String {
    let mut s = String::new();
    tex_into(t, e, form, &mut s);
    s
}

/// The bracket pair used for function application in a given form.
fn call_brackets(form: Form) -> (&'static str, &'static str) {
    match form {
        Form::Standard => ("\\!\\left[", "\\right]"),
        Form::Traditional => ("\\!\\left(", "\\right)"),
    }
}

/// Symbols with special TeX forms.
fn symbol_tex(name: &str) -> Option<&'static str> {
    Some(match name {
        "Pi" => "\\pi",
        "E" => "e",
        "I" => "i",
        "Infinity" => "\\infty",
        "ComplexInfinity" => "\\tilde{\\infty}",
        "GoldenRatio" => "\\varphi",
        "EulerGamma" => "\\gamma",
        "Catalan" => "G",
        "True" => "\\mathrm{True}",
        "False" => "\\mathrm{False}",
        "Indeterminate" => "\\text{Indeterminate}",
        "Alpha" => "\\alpha",
        "Beta" => "\\beta",
        "Gamma" => "\\gamma",
        "Delta" => "\\delta",
        "Epsilon" => "\\epsilon",
        "Zeta" => "\\zeta",
        "Eta" => "\\eta",
        "Theta" => "\\theta",
        "Iota" => "\\iota",
        "Kappa" => "\\kappa",
        "Lambda" => "\\lambda",
        "Mu" => "\\mu",
        "Nu" => "\\nu",
        "Xi" => "\\xi",
        "Omicron" => "o",
        "Rho" => "\\rho",
        "Sigma" => "\\sigma",
        "Tau" => "\\tau",
        "Upsilon" => "\\upsilon",
        "Phi" => "\\phi",
        "Chi" => "\\chi",
        "Psi" => "\\psi",
        "Omega" => "\\omega",
        "CapitalDelta" => "\\Delta",
        "CapitalOmega" => "\\Omega",
        // The degree sign needs an empty group as its base: a bare
        // ^{\circ} has nothing to attach to and is a parse error in KaTeX.
        "Degree" => "{}^{\\circ}",
        _ => return None,
    })
}

/// Escape characters that are special in KaTeX math mode. Symbol names may
/// contain `$` ($Version, x$1, Pattern$Marker), which must not reach the
/// output bare or every KaTeX parse fails.
fn tex_escape_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        match c {
            '$' | '%' | '&' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\\' => out.push_str("\\backslash "),
            '~' => out.push_str("\\textasciitilde "),
            '^' => out.push_str("\\textasciicircum "),
            _ => out.push(c),
        }
    }
    out
}

fn tex_symbol_as(name: &str, form: Form) -> String {
    if let Some(tex) = symbol_tex(name) {
        return tex.to_string();
    }
    let escaped = tex_escape_name(name);
    if name.len() == 1 {
        escaped
    } else {
        match form {
            Form::Standard => format!("\\text{{{}}}", escaped),
            Form::Traditional => format!("\\mathit{{{}}}", escaped),
        }
    }
}

/// Machine reals in scientific notation carry Mathematica's `*^` marker;
/// KaTeX needs it as `\times 10^{n}` (the old code emitted `2.4329*^18`
/// verbatim, which rendered as nonsense).
fn tex_real(x: f64) -> String {
    let s = crate::fmt::format_real_out(x);
    match s.split_once("*^") {
        Some((m, e)) => format!("{} \\times 10^{{{}}}", m, e),
        None => s,
    }
}

/// A `Step[label, expr]` node (the head of a derivation step).
fn is_step_node(t: &SymbolTable, e: &Expr) -> bool {
    if let Some((h, a)) = e.parts() {
        if a.len() == 2 {
            if let Some(id) = h.sym_id() {
                return t.name_of(id) == "Step";
            }
        }
    }
    false
}

fn fn_tex_name(t: &SymbolTable, head_id: usize) -> Option<&'static str> {
    Some(match t.name_of(head_id) {
        "Sin" => "\\sin",
        "Cos" => "\\cos",
        "Tan" => "\\tan",
        "Cot" => "\\cot",
        "Sec" => "\\sec",
        "Csc" => "\\csc",
        "ArcSin" => "\\arcsin",
        "ArcCos" => "\\arccos",
        "ArcTan" => "\\arctan",
        "Sinh" => "\\sinh",
        "Cosh" => "\\cosh",
        "Tanh" => "\\tanh",
        "Log" => "\\ln",
        "Exp" => "\\exp",
        "Sqrt" => "\\sqrt",
        "Mod" => "\\bmod",
        "Min" => "\\min",
        "Max" => "\\max",
        "Det" => "\\det",
        "Sum" => "\\sum",
        "Product" => "\\prod",
        "Plot" => "\\text{Plot}",
        "ListPlot" => "\\text{ListPlot}",
        "Integrate" => "\\int",
        "D" => "\\dfrac{d}{dx}",
        _ => return None,
    })
}

fn is_negative(t: &SymbolTable, e: &Expr) -> bool {
    match e {
        Expr::Int(n) => n.sign < 0,
        Expr::Rat(r) => r.is_neg(),
        Expr::Real(x) => *x < 0.0,
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
        _ => {}
    }
    if let Some((h, hargs)) = e.parts() {
        if h.is_symbol(t.sys.times) && !hargs.is_empty() && is_negative(t, e) {
            let mut args = hargs.to_vec();
            let first = args[0].clone();
            args.remove(0);
            let positive = match first {
                Expr::Int(n) => Expr::Int(n.abs()),
                Expr::Rat(r) => Expr::rat(r.abs()),
                Expr::Real(x) => Expr::real(-x),
                other => other,
            };
            args.insert(0, positive);
            let one = Expr::int_i64(1);
            let filtered: Vec<Expr> =
                args.into_iter().filter(|a| !same_q(a, &one)).collect();
            return match filtered.len() {
                0 => Expr::int_i64(1),
                1 => filtered.into_iter().next().unwrap(),
                _ => Expr::node(Expr::Sym(t.sys.times), filtered),
            };
        }
    }
    e.clone()
}

fn tex_into(t: &SymbolTable, e: &Expr, form: Form, out: &mut String) {
    match e {
        Expr::Int(n) => out.push_str(&n.to_string()),
        Expr::Rat(r) => {
            if r.is_int() {
                out.push_str(&r.num.to_string());
            } else {
                out.push_str(&format!("\\dfrac{{{}}}{{{}}}", r.num, r.den));
            }
        }
        Expr::Real(x) => out.push_str(&tex_real(*x)),
        Expr::BigReal(b) => out.push_str(&b.to_string()),
        Expr::Str(s) => {
            // Text mode: only LaTeX specials need escaping. A double quote is
            // literal (escaping it as \" would be an accent command, which
            // KaTeX rejects and renders in red).
            out.push_str("\\text{\u{201C}");
            for ch in s.chars() {
                match ch {
                    '\\' => out.push_str("\\textbackslash{}"),
                    '{' => out.push_str("\\{"),
                    '}' => out.push_str("\\}"),
                    '&' => out.push_str("\\&"),
                    '%' => out.push_str("\\%"),
                    '#' => out.push_str("\\#"),
                    '$' => out.push_str("\\$"),
                    '_' => out.push_str("\\_"),
                    '^' => out.push_str("\\textasciicircum{}"),
                    '~' => out.push_str("\\textasciitilde{}"),
                    c => out.push(c),
                }
            }
            out.push_str("\u{201D}}");
        }
        Expr::Sym(id) => out.push_str(&tex_symbol_as(t.name_of(*id), form)),
        Expr::Cnum(c) => {
            // re + im*i, compact
            let (re, im) = (&c.re, &c.im);
            let re_zero = match re {
                crate::expr::NumVal::R(r) => r.is_zero(),
                crate::expr::NumVal::F(f) => *f == 0.0,
            };
            let im_one = match im {
                crate::expr::NumVal::R(r) => r.num.to_i64() == Some(1) && r.den.to_i64() == Some(1),
                crate::expr::NumVal::F(f) => *f == 1.0,
            };
            let im_neg_one = match im {
                crate::expr::NumVal::R(r) => r.num.to_i64() == Some(-1) && r.den.to_i64() == Some(1),
                crate::expr::NumVal::F(f) => *f == -1.0,
            };
            if !re_zero {
                tex_into(t, &crate::fmt::numval_to_expr(re), form, out);
                if im_neg_one {
                    out.push_str(" - i");
                } else {
                    out.push_str(" + ");
                    if !im_one {
                        tex_into(t, &crate::fmt::numval_to_expr(im), form, out);
                        out.push(' ');
                    }
                    out.push('i');
                }
            } else if im_neg_one {
                out.push_str("-i");
            } else if im_one {
                out.push('i');
            } else {
                tex_into(t, &crate::fmt::numval_to_expr(im), form, out);
                out.push_str("\\,i");
            }
        }
        Expr::Node(nd) => {
            // Operator forms
            if let Expr::Sym(id) = &nd.head {
                let sys = t.sys;
                // Derivation step: `\text{label: } expr`.
                if nd.args.len() == 2 && t.name_of(*id) == "Step" {
                    if let Expr::Str(label) = &nd.args[0] {
                        if !label.is_empty() {
                            out.push_str("\\text{");
                            for ch in label.chars() {
                                match ch {
                                    '\\' => out.push_str("\\textbackslash{}"),
                                    '{' => out.push_str("\\{"),
                                    '}' => out.push_str("\\}"),
                                    '&' => out.push_str("\\&"),
                                    '%' => out.push_str("\\%"),
                                    '#' => out.push_str("\\#"),
                                    '$' => out.push_str("\\$"),
                                    '_' => out.push_str("\\_"),
                                    '^' => out.push_str("\\textasciicircum{}"),
                                    '~' => out.push_str("\\textasciitilde{}"),
                                    c => out.push(c),
                                }
                            }
                            out.push_str(": } ");
                        }
                    }
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.plus && !nd.args.is_empty() {
                    let mut first = true;
                    for a in &nd.args {
                        if first {
                            first = false;
                            if is_negative(t, a) {
                                out.push('-');
                                tex_into(t, &strip_neg(t, a), form, out);
                            } else {
                                tex_into(t, a, form, out);
                            }
                        } else if is_negative(t, a) {
                            out.push_str(" - ");
                            tex_into(t, &strip_neg(t, a), form, out);
                        } else {
                            out.push_str(" + ");
                            tex_into(t, a, form, out);
                        }
                    }
                    return;
                }
                if *id == sys.times && !nd.args.is_empty() {
                    // Fraction: split denominator (negative integer powers).
                    let mut numer: Vec<Expr> = Vec::new();
                    let mut denom: Vec<Expr> = Vec::new();
                    let mut neg = false;
                    for a in &nd.args {
                        let mut handled = false;
                        if let Some((h2, a2)) = a.parts() {
                            if h2.is_symbol(sys.power) && a2.len() == 2 {
                                if let Some(n) = a2[1].to_i64_pub() {
                                    if n < 0 {
                                        if n == -1 {
                                            denom.push(a2[0].clone());
                                        } else {
                                            denom.push(Expr::node(
                                                Expr::Sym(sys.power),
                                                vec![a2[0].clone(), Expr::int_i64(-n)],
                                            ));
                                        }
                                        handled = true;
                                    }
                                }
                            }
                        }
                        if !handled {
                            if is_negative(t, a) {
                                neg = true;
                                numer.push(strip_neg(t, a));
                            } else {
                                numer.push(a.clone());
                            }
                        }
                    }
                    if neg {
                        out.push('-');
                    }
                    let one = Expr::int_i64(1);
                    let numer: Vec<Expr> = numer
                        .into_iter()
                        .filter(|a| !same_q(a, &one))
                        .collect();
                    let emit = |items: &Vec<Expr>, form: Form, out: &mut String| {
                        match items.len() {
                            0 => out.push('1'),
                            1 => tex_into(t, &items[0], form, out),
                            _ => {
                                for (k, x) in items.iter().enumerate() {
                                    if k > 0 {
                                        out.push_str(" \\, ");
                                    }
                                    tex_atom(t, x, 40, form, out);
                                }
                            }
                        }
                    };
                    if denom.is_empty() {
                        emit(&numer, form, out);
                    } else {
                        out.push_str("\\dfrac{");
                        emit(&numer, form, out);
                        out.push_str("}{");
                        if denom.len() == 1 {
                            tex_into(t, &denom[0], form, out);
                        } else {
                            let d = Expr::node(Expr::Sym(sys.times), denom);
                            tex_into(t, &d, form, out);
                        }
                        out.push('}');
                    }
                    return;
                }
                if *id == sys.power && nd.args.len() == 2 {
                    // Sqrt forms
                    let exp_half = same_q(
                        &nd.args[1],
                        &Expr::rat(crate::rational::Rat::new(
                            crate::bigint::BigInt::from_i64(1),
                            crate::bigint::BigInt::from_i64(2),
                        )),
                    );
                    if exp_half {
                        out.push_str("\\sqrt{");
                        tex_into(t, &nd.args[0], form, out);
                        out.push('}');
                        return;
                    }
                    // Reciprocals print as fractions (1/x, 1/x^2, 1/(2 x),
                    // 1/Sqrt[x]) rather than as negative powers, as in
                    // Mathematica's typeset output.
                    match &nd.args[1] {
                        Expr::Int(n) if n.to_i64().map_or(false, |v| v < 0) => {
                            let e = -n.to_i64().unwrap();
                            out.push_str("\\dfrac{1}{");
                            if e == 1 {
                                tex_into(t, &nd.args[0], form, out);
                            } else {
                                tex_atom(t, &nd.args[0], 50, form, out);
                                out.push_str("^{");
                                out.push_str(&e.to_string());
                                out.push('}');
                            }
                            out.push('}');
                            return;
                        }
                        Expr::Rat(r) if r.num.to_i64() == Some(-1) && r.den.to_i64() == Some(2) => {
                            out.push_str("\\dfrac{1}{\\sqrt{");
                            tex_into(t, &nd.args[0], form, out);
                            out.push_str("}}");
                            return;
                        }
                        _ => {}
                    }
                    tex_atom(t, &nd.args[0], 50, form, out);
                    out.push('^');
                    out.push('{');
                    tex_into(t, &nd.args[1], form, out);
                    out.push('}');
                    return;
                }
                if (*id == sys.rule || *id == sys.ruledelayed) && nd.args.len() == 2 {
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str(" \\to ");
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.equal && nd.args.len() == 2 {
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str(" = ");
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.unequal && nd.args.len() == 2 {
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str(" \\neq ");
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.less && nd.args.len() == 2 {
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str(" < ");
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.greater && nd.args.len() == 2 {
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str(" > ");
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.lessequal && nd.args.len() == 2 {
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str(" \\leq ");
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.greaterequal && nd.args.len() == 2 {
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str(" \\geq ");
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.and {
                    join_tex(t, &nd.args, " \\land ", form, out);
                    return;
                }
                if *id == sys.or {
                    join_tex(t, &nd.args, " \\lor ", form, out);
                    return;
                }
                if *id == sys.not && nd.args.len() == 1 {
                    out.push_str("\\neg ");
                    tex_atom(t, &nd.args[0], 30, form, out);
                    return;
                }
                if *id == sys.list {
                    // A derivation (`Steps`/`Trace`) reads far better with each
                    // step on its own line than as a one-line `{…}` list.
                    if !nd.args.is_empty() && nd.args.iter().all(|a| is_step_node(t, a)) {
                        out.push_str("\\begin{array}{l}");
                        for (k, a) in nd.args.iter().enumerate() {
                            if k > 0 {
                                out.push_str(" \\\\ ");
                            }
                            tex_into(t, a, form, out);
                        }
                        out.push_str("\\end{array}");
                        return;
                    }
                    out.push_str("\\left\\{");
                    for (k, a) in nd.args.iter().enumerate() {
                        if k > 0 {
                            out.push_str(", ");
                        }
                        tex_into(t, a, form, out);
                    }
                    out.push_str("\\right\\}");
                    return;
                }
                if *id == sys.span {
                    out.push_str("\\text{Span}");
                    return;
                }
                if *id == sys.set || *id == sys.setdelayed {
                    // f[x_] := body
                    if nd.args.len() == 2 {
                        tex_into(t, &nd.args[0], form, out);
                        out.push_str(if *id == sys.set { " = " } else { " := " });
                        tex_into(t, &nd.args[1], form, out);
                        return;
                    }
                }
                if *id == sys.condition && nd.args.len() == 2 {
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str("\\;/\\; ");
                    tex_into(t, &nd.args[1], form, out);
                    return;
                }
                if *id == sys.pattern && nd.args.len() == 2 {
                    // x_ / x_Integer / x:pat
                    if let Expr::Sym(name) = &nd.args[0] {
                        out.push_str(&tex_symbol_as(t.name_of(*name), form));
                        if let Some((h2, a2)) = nd.args[1].parts() {
                            if h2.is_symbol(sys.blank) {
                                if a2.is_empty() {
                                    out.push_str("\\_");
                                } else if let Some(hid) = a2[0].sym_id() {
                                    out.push_str(&format!("\\_\\text{{{}}}", t.name_of(hid)));
                                }
                                return;
                            }
                            if h2.is_symbol(sys.blanksequence) {
                                if a2.is_empty() {
                                    out.push_str("\\text{\\_\\_}");
                                } else if let Some(hid) = a2[0].sym_id() {
                                    out.push_str(&format!("\\text{{\\_\\_\\text{{{}}}}}", t.name_of(hid)));
                                }
                                return;
                            }
                            if h2.is_symbol(sys.blanknullsequence) {
                                if a2.is_empty() {
                                    out.push_str("\\text{\\_\\_\\_}");
                                } else if let Some(hid) = a2[0].sym_id() {
                                    out.push_str(&format!(
                                        "\\text{{\\_\\_\\_\\text{{{}}}}}",
                                        t.name_of(hid)
                                    ));
                                }
                                return;
                            }
                        }
                        // generic: x:rhs
                        out.push_str(":");
                        tex_into(t, &nd.args[1], form, out);
                        return;
                    }
                }
                if *id == sys.blank {
                    if nd.args.is_empty() {
                        out.push_str("\\_");
                        return;
                    }
                    if let Some(hid) = nd.args[0].sym_id() {
                        out.push_str(&format!("\\_\\text{{{}}}", t.name_of(hid)));
                        return;
                    }
                }
            }
            // Function heads. In StandardForm the name is shown exactly as it
            // must be typed (Sin[x], Exp[x], Abs[x]), because rendering Sin as
            // the operator "sin" invites typing an undefined lowercase name.
            // TraditionalForm uses the conventional operators (sin(x), |x|).
            if let Some(hid) = nd.head.sym_id() {
                let name = t.name_of(hid).to_string();
                let traditional = form == Form::Traditional;
                if name == "Sqrt" && nd.args.len() == 1 {
                    out.push_str("\\sqrt{");
                    tex_into(t, &nd.args[0], form, out);
                    out.push('}');
                    return;
                }
                if traditional && name == "Exp" && nd.args.len() == 1 {
                    out.push_str("\\exp");
                    let (bo, bc) = call_brackets(form);
                    out.push_str(bo);
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str(bc);
                    return;
                }
                if traditional && name == "Abs" && nd.args.len() == 1 {
                    out.push_str("\\left|");
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str("\\right|");
                    return;
                }
                if traditional && name == "Log" && nd.args.len() == 2 {
                    let (bo, bc) = call_brackets(form);
                    out.push_str("\\log_{");
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str("}");
                    out.push_str(bo);
                    tex_into(t, &nd.args[1], form, out);
                    out.push_str(bc);
                    return;
                }
                if traditional && nd.args.len() == 1 {
                    if let Some(op) = fn_tex_name(t, hid) {
                        let (bo, bc) = call_brackets(form);
                        out.push_str(op);
                        out.push_str(bo);
                        tex_into(t, &nd.args[0], form, out);
                        out.push_str(bc);
                        return;
                    }
                }
                if name == "Graphics" {
                    out.push_str("\\text{-Graphics-}");
                    return;
                }
                if name == "Rational" && nd.args.len() == 2 {
                    out.push_str("\\dfrac{");
                    tex_into(t, &nd.args[0], form, out);
                    out.push_str("}{");
                    tex_into(t, &nd.args[1], form, out);
                    out.push('}');
                    return;
                }
            }
            // Generic f[a, b, ...] — brackets depend on the form.
            let (bo, bc) = call_brackets(form);
            tex_into(t, &nd.head, form, out);
            out.push_str(bo);
            for (k, a) in nd.args.iter().enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                tex_into(t, a, form, out);
            }
            out.push_str(bc);
        }
    }
}

fn join_tex(t: &SymbolTable, args: &[Expr], sep: &str, form: Form, out: &mut String) {
    for (k, a) in args.iter().enumerate() {
        if k > 0 {
            out.push_str(sep);
        }
        tex_into(t, a, form, out);
    }
}

fn tex_atom(t: &SymbolTable, e: &Expr, min_rank: u8, form: Form, out: &mut String) {
    if node_rank(t, e) < min_rank {
        out.push_str("\\left(");
        tex_into(t, e, form, out);
        out.push_str("\\right)");
    } else {
        tex_into(t, e, form, out);
    }
}

/// Rough precedence rank for parenthesization decisions.
fn node_rank(t: &SymbolTable, e: &Expr) -> u8 {
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
        }
    }
    99
}

/// Number of non-constant symbols (used to skip TeX for plot-like results).
pub fn is_graphic(t: &SymbolTable, e: &Expr) -> bool {
    if let Some((h, _)) = e.parts() {
        if let Some(id) = h.sym_id() {
            return t.name_of(id) == "Graphics";
        }
    }
    false
}

pub fn is_constant_sym(t: &SymbolTable, e: &Expr) -> bool {
    match e {
        Expr::Sym(id) => has(t.attrs(*id), A_CONSTANT),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::SymbolTable;

    fn tex(src: &str) -> String {
        let mut t = SymbolTable::new();
        let e = crate::parser::parse_input(&mut t, src).unwrap();
        tex_form(&t, &e)
    }

    #[test]
    fn tex_basics() {
        assert_eq!(tex("1/2 + 1/3"), "\\dfrac{1}{2} + \\dfrac{1}{3}");
        assert_eq!(tex("x^2"), "x^{2}");
        assert_eq!(tex("Sqrt[x]"), "\\sqrt{x}");
        assert_eq!(tex("1/x"), "\\dfrac{1}{x}");
        assert_eq!(tex("Pi"), "\\pi");
        assert_eq!(tex("Sin[x]"), "\\text{Sin}\\!\\left[x\\right]");
    }

    #[test]
    fn tex_strings_escape_specials() {
        let mut i = crate::eval::Interp::new();
        let e = crate::parser::parse_input(&mut i.t, "\"a_b^c 100% {x} \\\" \"").unwrap();
        let tex = tex_form(&i.t, &e);
        assert!(tex.contains("\\_"), "underscore escaped: {}", tex);
        assert!(tex.contains("\\%"), "percent escaped: {}", tex);
        assert!(tex.contains("\\textasciicircum"), "caret escaped: {}", tex);
        assert!(!tex.contains("\\\""), "quotes must not be accent-escaped: {}", tex);
        assert!(tex.contains('"'), "inner quotes kept: {}", tex);
        assert!(tex.contains("\\u{201C}") || tex.contains("“"), "opens with a curly quote: {}", tex);
    }

    #[test]
    fn tex_forms_differ() {
        let mut i = crate::eval::Interp::new();
        let e = crate::parser::parse_input(&mut i.t, "Factor[x^2 - 1]").unwrap();
        let std = tex_form_as(&i.t, &e, Form::Standard);
        let trad = tex_form_as(&i.t, &e, Form::Traditional);
        assert!(std.contains("\\left["), "standard uses brackets: {}", std);
        assert!(trad.contains("\\left("), "traditional uses parens: {}", trad);
        assert!(trad.contains("\\mathit{Factor}"), "traditional italicises: {}", trad);
        let e2 = crate::parser::parse_input(&mut i.t, "Sin[x]").unwrap();
        assert_eq!(tex_form_as(&i.t, &e2, Form::Traditional),
                   "\\sin\\!\\left(x\\right)");
    }

    #[test]
    fn tex_evaluated() {
        let mut i = crate::eval::Interp::new();
        let e = crate::parser::parse_input(&mut i.t, "1/2 + 1/3").unwrap();
        let v = i.eval(&e).unwrap();
        assert_eq!(tex_form(&i.t, &v), "\\dfrac{5}{6}");
        let e = crate::parser::parse_input(&mut i.t, "D[Sin[x]^2, x]").unwrap();
        let v = i.eval(&e).unwrap();
        assert_eq!(tex_form(&i.t, &v), "2 \\, \\text{Cos}\\!\\left[x\\right] \\, \\text{Sin}\\!\\left[x\\right]");
        let e = crate::parser::parse_input(&mut i.t, "f[x_] := x^2").unwrap();
        assert_eq!(tex_form(&i.t, &e), "f\\!\\left[x\\_\\right] := x^{2}");
    }
}
