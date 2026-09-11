//! JSON-RPC stdio server: newline-delimited JSON requests.
//! Request:  {"id": n, "op": "eval"|"reset"|"names", "input": "..."}
//! Response: {"id": n, "status": "ok"|"error", "out": n, "result": "...",
//!            "input_form": "...", "graphics": {...}|null,
//!            "prints": [...], "messages": [...]}

use crate::eval::{Flow, Interp};
use crate::expr::Expr;
use crate::fmt::{input_form, output_form};
use crate::json::obj;
use crate::parser::parse_input;
use std::io::{BufRead, Write};

pub fn graphics_json(t: &mut crate::expr::SymbolTable, e: &Expr) -> Option<crate::json::J> {
    let Some((h, hargs)) = e.parts() else {
        return None;
    };
    if !h.is_symbol(t.intern("Graphics")) {
        return None;
    }
    let mut prims: Vec<crate::json::J> = Vec::new();
    let mut plotrange: Option<[f64; 4]> = None;
    if let Some(first) = hargs.first() {
        for p in first.node_args() {
            let Some((ph, pargs)) = p.parts() else { continue };
            let pname = match ph.sym_id() {
                Some(id) => t.name_of(id).to_string(),
                None => continue,
            };
            match pname.as_str() {
                "Line" => {
                    if let Some(seg) = pargs.first() {
                        let mut pts: Vec<crate::json::J> = Vec::new();
                        for pt in seg.node_args() {
                            let c = pt.node_args();
                            if c.len() == 2 {
                                if let (Some(x), Some(y)) = (num_f64(&c[0]), num_f64(&c[1])) {
                                    pts.push(crate::json::J::Arr(vec![
                                        crate::json::J::Num(x),
                                        crate::json::J::Num(y),
                                    ]));
                                }
                            }
                        }
                        prims.push(
                            crate::json::J::obj()
                                .set("type", crate::json::J::str_of("Line"))
                                .set("pts", crate::json::J::Arr(pts)),
                        );
                    }
                }
                "Point" => {
                    if let Some(pt) = pargs.first() {
                        let c = pt.node_args();
                        if c.len() == 2 {
                            if let (Some(x), Some(y)) = (num_f64(&c[0]), num_f64(&c[1])) {
                                prims.push(
                                    crate::json::J::obj()
                                        .set("type", crate::json::J::str_of("Point"))
                                        .set(
                                            "pt",
                                            crate::json::J::Arr(vec![
                                                crate::json::J::Num(x),
                                                crate::json::J::Num(y),
                                            ]),
                                        ),
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // PlotRange rule in the remaining args.
    for a in hargs.iter().skip(1) {
        if let Some((rh, rhargs)) = a.parts() {
            if rh.is_symbol(t.sys.rule) && rhargs.len() == 2 {
                if let Some(kid) = rhargs[0].sym_id() {
                    if t.name_of(kid) == "PlotRange" {
                        let v = rhargs[1].node_args();
                        if v.len() == 2 {
                            let xr = v[0].node_args();
                            let yr = v[1].node_args();
                            if xr.len() == 2 && yr.len() == 2 {
                                if let (Some(x0), Some(x1), Some(y0), Some(y1)) = (
                                    num_f64(&xr[0]),
                                    num_f64(&xr[1]),
                                    num_f64(&yr[0]),
                                    num_f64(&yr[1]),
                                ) {
                                    plotrange = Some([x0, x1, y0, y1]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut g = crate::json::J::obj()
        .set("type", crate::json::J::str_of("Graphics"))
        .set("primitives", crate::json::J::Arr(prims));
    if let Some(pr) = plotrange {
        g = g.set(
            "plotrange",
            crate::json::J::Arr(vec![
                crate::json::J::Num(pr[0]),
                crate::json::J::Num(pr[1]),
                crate::json::J::Num(pr[2]),
                crate::json::J::Num(pr[3]),
            ]),
        );
    }
    Some(g)
}

fn num_f64(e: &Expr) -> Option<f64> {
    match e {
        Expr::Int(n) => Some(n.to_f64()),
        Expr::Rat(r) => Some(r.to_f64()),
        Expr::Real(x) => Some(*x),
        _ => None,
    }
}

/// Handle one request line; returns the JSON response (or None for EOF).
pub fn handle(i: &mut Interp, line: &str, out: &mut String) -> bool {
    let req = match crate::json::parse(line) {
        Ok(v) => v,
        Err(e) => {
            let resp = obj(vec![
                ("id", crate::json::J::Null),
                ("status", crate::json::J::str_of("error")),
                ("error", crate::json::J::str_of(&format!("bad request: {}", e))),
            ]);
            resp.write(out);
            return true;
        }
    };
    let id = req.get("id").cloned().unwrap_or(crate::json::J::Null);
    let op = req.get("op").and_then(|v| v.as_str()).unwrap_or("eval").to_string();
    let input = req.get("input").and_then(|v| v.as_str()).unwrap_or("").to_string();
    match op.as_str() {
        "reset" => {
            *i = Interp::new();
            let resp = obj(vec![
                ("id", id.clone()),
                ("status", crate::json::J::str_of("ok")),
            ]);
            resp.write(out);
            true
        }
        "names" => {
            let mut names: Vec<String> = i
                .t
                .data
                .iter()
                .map(|d| d.name.clone())
                .filter(|n| n.chars().all(|c| c.is_ascii_alphabetic() || c == '$'))
                .collect();
            names.sort();
            let resp = obj(vec![
                ("id", id.clone()),
                ("status", crate::json::J::str_of("ok")),
                (
                    "names",
                    crate::json::J::Arr(names.into_iter().map(crate::json::J::Str).collect()),
                ),
            ]);
            resp.write(out);
            true
        }
        "eval" => {
            i.messages.clear();
            i.prints.clear();
            match parse_input(&mut i.t, &input) {
                Err(pe) => {
                    let resp = obj(vec![
                        ("id", id.clone()),
                        ("status", crate::json::J::str_of("error")),
                        (
                            "error",
                            crate::json::J::str_of(&format!(
                                "Syntax error at line {}: {}",
                                pe.line, pe.msg
                            )),
                        ),
                    ]);
                    resp.write(out);
                    true
                }
                Ok(e) => {
                    let tex_in = crate::tex::tex_form(&i.t, &e);
                    let mut msgs: Vec<crate::json::J> = Vec::new();
                    let mut status = "ok";
                    let mut out_n: Option<usize> = None;
                    let mut tex_text: Option<String> = None;
                    let mut result_text = String::new();
                    let mut input_text = String::new();
                    let mut graphics = None;
                    let mut flow_err: Option<String> = None;
                    match i.eval(&e) {
                        Ok(v) => {
                            for (tag, text) in i.messages.drain(..) {
                                msgs.push(
                                    crate::json::J::obj()
                                        .set("tag", crate::json::J::str_of(&tag))
                                        .set("text", crate::json::J::str_of(&text)),
                                );
                            }
                            let is_null = v.is_symbol(i.t.sys.null);
                            result_text = output_form(&i.t, &v);
                            input_text = input_form(&i.t, &v);
                            tex_text = if crate::tex::is_graphic(&i.t, &v) {
                                None
                            } else if is_null {
                                None
                            } else {
                                Some(crate::tex::tex_form(&i.t, &v))
                            };
                            graphics = graphics_json(&mut i.t, &v);
                            if !is_null {
                                i.out_history.push(v);
                                out_n = Some(i.out_history.len());
                            }
                        }
                        Err(Flow::Halt(msg)) => {
                            i.depth = 0;
                            flow_err = Some(msg);
                        }
                        Err(Flow::Throw(v, _)) => {
                            let _ = v;
                            flow_err = Some("Throw::nocatch: Throw not caught.".to_string());
                        }
                        Err(Flow::Return(v)) => {
                            let v = v.unwrap_or(Expr::Sym(i.t.sys.null));
                            i.depth = 0;
                            result_text = output_form(&i.t, &v);
                            input_text = input_form(&i.t, &v);
                            i.out_history.push(v);
                            out_n = Some(i.out_history.len());
                        }
                        Err(_) => {
                            flow_err = Some("Evaluation aborted.".to_string());
                        }
                    }
                    let mut resp = obj(vec![("id", id.clone())]);
                    if let Some(err) = flow_err {
                        resp = resp
                            .set("status", crate::json::J::str_of("error"))
                            .set("error", crate::json::J::str_of(&err));
                    } else {
                        resp = resp
                            .set("status", crate::json::J::str_of("ok"))
                            .set("result", crate::json::J::str_of(&result_text))
                            .set("input_form", crate::json::J::str_of(&input_text))
                            .set("tex_in", crate::json::J::str_of(&tex_in));
                        if let Some(tx) = tex_text {
                            resp = resp.set("tex", crate::json::J::str_of(&tx));
                        }
                        if let Some(n) = out_n {
                            resp = resp.set("out", crate::json::J::Num(n as f64));
                        }
                        if let Some(g) = graphics {
                            resp = resp.set("graphics", g);
                        }
                        if !i.prints.is_empty() {
                            resp = resp.set(
                                "prints",
                                crate::json::J::Arr(
                                    i.prints.drain(..).map(crate::json::J::Str).collect(),
                                ),
                            );
                        }
                        if !msgs.is_empty() {
                            resp = resp.set("messages", crate::json::J::Arr(msgs));
                        }
                    }
                    resp.write(out);
                    true
                }
            }
        }
        "shutdown" => false,
        _ => {
            let resp = obj(vec![
                ("id", id.clone()),
                ("status", crate::json::J::str_of("error")),
                ("error", crate::json::J::str_of("unknown op")),
            ]);
            resp.write(out);
            true
        }
    }
}

pub fn serve() {
    let mut i = Interp::new();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        match line {
            Ok(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                let mut resp = String::new();
                if !handle(&mut i, &line, &mut resp) {
                    break;
                }
                if !resp.is_empty() {
                    let _ = writeln!(out, "{}", resp);
                    let _ = out.flush();
                }
            }
            Err(_) => break,
        }
    }
}