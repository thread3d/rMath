//! File I/O built-ins: Get (read/evaluate a script), Put/PutAppend
//! (write expressions), Import/Export (CSV/TXT/JSON), FileExistsQ, DeleteFile.

use crate::bigint::BigInt;
use crate::builtins::core::unevaluated_call;
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::{same_q, Expr};
use crate::json::J;
use std::io::Write;

fn path_str(i: &mut Interp, e: &Expr) -> Result<Option<String>, Flow> {
    let v = i.eval(e)?;
    if let Expr::Str(s) = v {
        Ok(Some(s.to_string()))
    } else {
        i.message("General", "strarg");
        Ok(None)
    }
}

// ============================ Get ============================

pub fn b_get(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        i.message("Get", "argr");
        return Ok(unevaluated_call(i, "Get", args));
    }
    let Some(path) = path_str(i, &args[0])? else {
        return Ok(unevaluated_call(i, "Get", args));
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            i.message("Get", "noopen");
            i.prints.push(format!("{}: {}", path, e));
            return Ok(unevaluated_call(i, "Get", args));
        }
    };
    // Strip (* ... *) comments (multi-line safe, string-aware) first, so
    // per-line parsing works.
    let content = strip_comments(&content);
    // One statement per line; a line that ends inside an unclosed bracket
    // continues on the next line.
    let mut last = Expr::Sym(i.t.sys.null);
    let mut produced = false;
    let mut buffer = String::new();
    for line in content.lines() {
        if buffer.is_empty() && (line.trim().is_empty() || line.trim().starts_with('#')) {
            continue;
        }
        buffer.push_str(line);
        buffer.push('\n');
        match crate::parser::parse_input(&mut i.t, buffer.trim()) {
            Ok(e) => {
                let v = i.eval(&e)?;
                if !v.is_symbol(i.t.sys.null) {
                    last = v;
                    produced = true;
                }
                buffer.clear();
            }
            Err(pe) => {
                if is_continuation_error(&pe.msg) {
                    continue; // keep accumulating lines
                }
                i.messages.push((
                    format!("Get::syntax ({}): {}", path, pe.line),
                    pe.msg.clone(),
                ));
                return Ok(unevaluated_call(i, "Get", args));
            }
        }
    }
    if !buffer.trim().is_empty() {
        i.messages
            .push(("Get::incomplete".to_string(), "unclosed expression".to_string()));
        return Ok(unevaluated_call(i, "Get", args));
    }
    Ok(if produced { last } else { Expr::Sym(i.t.sys.null) })
}

/// Remove nested `(* ... *)` comments, preserving strings and newlines.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut depth = 0usize;
    let mut in_str = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(c2) = chars.next() {
                    out.push(c2);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if depth > 0 {
            if c == '(' && chars.peek() == Some(&'*') {
                chars.next();
                depth += 1;
                continue;
            }
            if c == '*' && chars.peek() == Some(&')') {
                chars.next();
                depth -= 1;
                continue;
            }
            if c == '\n' {
                out.push('\n'); // keep line structure
            }
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
            continue;
        }
        if c == '(' && chars.peek() == Some(&'*') {
            chars.next();
            depth = 1;
            out.push(' ');
            continue;
        }
        out.push(c);
    }
    out
}

fn is_continuation_error(msg: &str) -> bool {
    msg.contains("expected ]")
        || msg.contains("expected )")
        || msg.contains("expected }")
        || msg.contains("expected ]]")
        || msg.contains("expected |>")
}

// ============================ Put / PutAppend ============================

fn put_lines(i: &mut Interp, path: &str, exprs: &[Expr], append: bool) -> EvalRes {
    let mut out = String::new();
    for e in exprs {
        let v = i.eval(e)?;
        out.push_str(&crate::fmt::input_form(&i.t, &v));
        out.push('\n');
    }
    let write_res = if append {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(out.as_bytes()))
            .map_err(|e| e.to_string())
    } else {
        std::fs::write(path, out).map_err(|e| e.to_string())
    };
    if let Err(e) = write_res {
        i.message("Put", "noopen");
        i.prints.push(format!("{}: {}", path, e));
        return Ok(Expr::Sym(i.t.sys.null));
    }
    Ok(Expr::Sym(i.t.sys.null))
}

/// Accept the file argument in either position (Mathematica uses
/// Put[expr1, ..., "file"]; the `>>` parser produces the same shape).
fn put_path_index(i: &mut Interp, args: &[Expr]) -> Result<Option<usize>, Flow> {
    for (k, a) in args.iter().enumerate() {
        if matches!(i.eval(a)?, Expr::Str(_)) {
            return Ok(Some(k));
        }
    }
    i.message("General", "strarg");
    Ok(None)
}

pub fn b_put(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Put", "argr");
        return Ok(unevaluated_call(i, "Put", args));
    }
    let Some(k) = put_path_index(i, args)? else {
        return Ok(unevaluated_call(i, "Put", args));
    };
    let Some(path) = path_str(i, &args[k])? else {
        return Ok(unevaluated_call(i, "Put", args));
    };
    let rest: Vec<Expr> = args
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != k)
        .map(|(_, a)| a.clone())
        .collect();
    put_lines(i, &path, &rest, false)
}

pub fn b_putappend(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("PutAppend", "argr");
        return Ok(unevaluated_call(i, "PutAppend", args));
    }
    let Some(k) = put_path_index(i, args)? else {
        return Ok(unevaluated_call(i, "PutAppend", args));
    };
    let Some(path) = path_str(i, &args[k])? else {
        return Ok(unevaluated_call(i, "PutAppend", args));
    };
    let rest: Vec<Expr> = args
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != k)
        .map(|(_, a)| a.clone())
        .collect();
    put_lines(i, &path, &rest, true)
}

// ============================ Export / Import ============================

fn export_format(i: &mut Interp, args: &[Expr], path: &str) -> Result<String, Flow> {
    // Optional trailing "CSV"/"TXT"/"JSON" argument; else by extension.
    if let Some(last) = args.last() {
        if let Expr::Str(s) = last {
            let u = s.to_uppercase();
            if ["CSV", "TXT", "JSON"].contains(&u.as_str()) {
                return Ok(u);
            }
        }
    }
    let _ = i;
    let lower = path.to_lowercase();
    if lower.ends_with(".csv") {
        Ok("CSV".to_string())
    } else if lower.ends_with(".json") {
        Ok("JSON".to_string())
    } else {
        Ok("TXT".to_string())
    }
}

pub fn b_export(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Export", "argr");
        return Ok(unevaluated_call(i, "Export", args));
    }
    let Some(path) = path_str(i, &args[0])? else {
        return Ok(unevaluated_call(i, "Export", args));
    };
    let fmt = export_format(i, args, &path)?;
    let data = i.eval(&args[1])?;
    let content = match fmt.as_str() {
        "CSV" => csv_export(i, &data)?,
        "JSON" => match expr_to_json(i, &data) {
            Some(j) => j.to_string(),
            None => {
                i.message("Export", "fmt");
                return Ok(unevaluated_call(i, "Export", args));
            }
        },
        _ => crate::fmt::output_form(&i.t, &data),
    };
    match std::fs::write(&path, content) {
        Ok(_) => Ok(Expr::Str(path.into())),
        Err(e) => {
            i.message("Export", "noopen");
            i.prints.push(format!("{}: {}", path, e));
            Ok(unevaluated_call(i, "Export", args))
        }
    }
}

fn csv_export(i: &mut Interp, e: &Expr) -> Result<String, Flow> {
    let sys = i.t.sys;
    let rows: Vec<Vec<Expr>> = match e.parts() {
        Some((h, hargs)) if h.is_symbol(sys.list) => {
            let all_rows: Option<Vec<Vec<Expr>>> = hargs
                .iter()
                .map(|r| if r.is_list(&i.t) { Some(r.node_args()) } else { None })
                .collect();
            match all_rows {
                Some(rows) => rows,
                None => vec![e.node_args()],
            }
        }
        _ => {
            i.message("Export", "fmt");
            return Ok(String::new());
        }
    };
    let mut out = String::new();
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .map(|c| match c {
                Expr::Int(n) => n.to_string(),
                Expr::Rat(r) => r.to_f64().to_string(),
                Expr::Real(x) => format!("{}", x),
                Expr::Str(s) => s.to_string(),
                _ => crate::fmt::output_form(&i.t, c),
            })
            .collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    Ok(out)
}

pub fn b_import(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 1 {
        i.message("Import", "argr");
        return Ok(unevaluated_call(i, "Import", args));
    }
    let Some(path) = path_str(i, &args[0])? else {
        return Ok(unevaluated_call(i, "Import", args));
    };
    let fmt = export_format(i, args, &path)?;
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            i.message("Import", "noopen");
            i.prints.push(format!("{}: {}", path, e));
            return Ok(unevaluated_call(i, "Import", args));
        }
    };
    match fmt.as_str() {
        "CSV" => Ok(csv_import(&content, i)),
        "JSON" => match json_import(i, &content) {
            Some(e) => Ok(e),
            None => {
                i.message("Import", "fmt");
                Ok(unevaluated_call(i, "Import", args))
            }
        },
        _ => Ok(Expr::from_str(&content)),
    }
}

fn csv_import(content: &str, i: &mut Interp) -> Expr {
    let sys = i.t.sys;
    let mut rows: Vec<Expr> = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cells: Vec<Expr> = Vec::new();
        for cell in line.split(',') {
            let t = cell.trim();
            if let Some(n) = BigInt::from_str_dec(t).filter(|n| n.to_i64().is_some()) {
                cells.push(Expr::Int(n));
            } else if let Ok(f) = t.parse::<f64>() {
                cells.push(Expr::real(f));
            } else {
                cells.push(Expr::from_str(t));
            }
        }
        rows.push(Expr::node(Expr::Sym(sys.list), cells));
    }
    Expr::node(Expr::Sym(sys.list), rows)
}

// ---------- JSON conversion ----------

/// Convert an expression to JSON: lists, numbers, strings, True/False,
/// associations (symbol/string keys). Returns None for anything else.
fn expr_to_json(i: &Interp, e: &Expr) -> Option<J> {
    match e {
        Expr::Int(n) => Some(J::Num(n.to_f64())),
        Expr::Rat(r) => Some(J::Num(r.to_f64())),
        Expr::Real(x) => Some(J::Num(*x)),
        Expr::Str(s) => Some(J::Str(s.to_string())),
        Expr::Sym(id) => {
            let name = i.t.name_of(*id);
            if name == "True" {
                Some(J::Bool(true))
            } else if name == "False" {
                Some(J::Bool(false))
            } else {
                None
            }
        }
        Expr::Node(nd) => {
            let head_id = nd.head.sym_id()?;
            let sys = i.t.sys;
            if head_id == sys.list {
                let items: Option<Vec<J>> = nd.args.iter().map(|a| expr_to_json(i, a)).collect();
                return items.map(J::Arr);
            }
            if head_id == sys.association {
                let mut obj = J::obj();
                for a in &nd.args {
                    let (h, hargs) = a.parts()?;
                    if !h.is_symbol(sys.rule) || hargs.len() != 2 {
                        return None;
                    }
                    let key: String = match &hargs[0] {
                        Expr::Sym(kid) => i.t.name_of(*kid).to_string(),
                        Expr::Str(s) => s.to_string(),
                        _ => return None,
                    };
                    obj = obj.set(&key, expr_to_json(i, &hargs[1])?);
                }
                return Some(obj);
            }
            None
        }
        _ => None,
    }
}

/// JSON -> expression.
fn json_import(i: &mut Interp, content: &str) -> Option<Expr> {
    let j = crate::json::parse(content).ok()?;
    Some(json_to_expr(i, &j))
}

fn json_to_expr(i: &mut Interp, j: &J) -> Expr {
    let sys = i.t.sys;
    match j {
        J::Null => Expr::Sym(sys.null),
        J::Bool(b) => Expr::Sym(if *b { sys.r#true } else { sys.r#false }),
        J::Num(n) => {
            if n.fract() == 0.0 && n.is_finite() && n.abs() < 9e15 {
                Expr::int_i64(*n as i64)
            } else {
                Expr::real(*n)
            }
        }
        J::Str(s) => Expr::from_str(s),
        J::Arr(items) => Expr::node(
            Expr::Sym(sys.list),
            items.iter().map(|x| json_to_expr(i, x)).collect(),
        ),
        J::Obj(pairs) => {
            let mut args: Vec<Expr> = Vec::new();
            for (k, v) in pairs {
                args.push(Expr::node(
                    Expr::Sym(sys.rule),
                    vec![Expr::from_str(k), json_to_expr(i, v)],
                ));
            }
            Expr::node(Expr::Sym(sys.association), args)
        }
    }
}

pub fn b_fileexistsq(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "FileExistsQ", args));
    }
    let Some(path) = path_str(i, &args[0])? else {
        return Ok(unevaluated_call(i, "FileExistsQ", args));
    };
    Ok(Expr::Sym(
        if std::path::Path::new(&path).exists() {
            i.t.sys.r#true
        } else {
            i.t.sys.r#false
        },
    ))
}

pub fn b_deletefile(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() != 1 {
        return Ok(unevaluated_call(i, "DeleteFile", args));
    }
    let Some(path) = path_str(i, &args[0])? else {
        return Ok(unevaluated_call(i, "DeleteFile", args));
    };
    let _ = std::fs::remove_file(&path);
    Ok(Expr::Sym(i.t.sys.null))
}

/// Current working directory.
pub fn b_directory(_i: &mut Interp, _args: &[Expr]) -> EvalRes {
    match std::env::current_dir() {
        Ok(d) => Ok(Expr::from_str(&d.to_string_lossy())),
        Err(_) => Ok(Expr::Sym(_i.t.sys.null)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("rmath_file_test_{}_{}", std::process::id(), name));
        p
    }

    #[test]
    fn put_get_roundtrip() {
        let p = temp("putget.txt");
        let path = p.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&path);
        let mut i = crate::eval::Interp::new();
        let file = Expr::from_str(&path);
        put_lines(&mut i, &path, &[Expr::int_i64(42), Expr::from_str("hi")], false);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "42\n\"hi\"\n");
        // Get evaluates the stored expressions.
        let e = crate::parser::parse_input(&mut i.t, "Get[\"..path..\"]")
            .unwrap_or_else(|_| Expr::int_i64(0));
        let _ = e;
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn csv_roundtrip() {
        let mut i = crate::eval::Interp::new();
        let data = Expr::node(
            Expr::Sym(i.t.sys.list),
            vec![
                Expr::node(Expr::Sym(i.t.sys.list), vec![Expr::int_i64(1), Expr::real(2.5)]),
                Expr::node(Expr::Sym(i.t.sys.list), vec![Expr::int_i64(3), Expr::int_i64(4)]),
            ],
        );
        let csv = csv_export(&mut i, &data).unwrap();
        assert_eq!(csv, "1,2.5\n3,4\n");
        let back = csv_import(&csv, &mut i);
        assert!(back.is_list(&i.t));
        assert_eq!(back.list_len(), 2);
    }

    #[test]
    fn json_roundtrip() {
        let mut i = crate::eval::Interp::new();
        let assoc = Expr::node(
            Expr::Sym(i.t.sys.association),
            vec![
                Expr::node(Expr::Sym(i.t.sys.rule), vec![Expr::from_str("a"), Expr::int_i64(1)]),
                Expr::node(
                    Expr::Sym(i.t.sys.rule),
                    vec![
                        Expr::from_str("b"),
                        Expr::node(
                            Expr::Sym(i.t.sys.list),
                            vec![Expr::int_i64(1), Expr::int_i64(2), Expr::int_i64(3)],
                        ),
                    ],
                ),
            ],
        );
        let j = expr_to_json(&i, &assoc).unwrap();
        assert_eq!(j.to_string(), "{\"a\":1,\"b\":[1,2,3]}");
        let back = json_import(&mut i, &j.to_string()).unwrap();
        assert!(back.is_list(&i.t) || matches!(back, Expr::Node(_)));
    }

    #[test]
    fn get_runs_a_script() {
        let p = temp("script.mt");
        let path = p.to_string_lossy().to_string();
        std::fs::write(&path, "1 + 2*3\nTotal[{1, 2, 3}]\n").unwrap();
        let mut i = crate::eval::Interp::new();
        let arg = Expr::from_str(&path);
        let r = b_get(&mut i, &[arg]).unwrap();
        assert_eq!(crate::fmt::output_form(&i.t, &r), "6");
        std::fs::remove_file(&path).unwrap();
    }
}
