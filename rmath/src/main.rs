//! rmath kernel CLI: REPL, one-shot eval, and a JSON-RPC stdio server mode.

use rmath::eval::{Flow, Interp};
use rmath::expr::Expr;
use rmath::fmt::{fullform, output_form};
use rmath::parser::parse_input;
use std::io::{BufRead, Write};

fn run_input(i: &mut Interp, src: &str) -> (Option<String>, Vec<String>) {
    i.messages.clear();
    i.prints.clear();
    let (out, fatal) = match parse_input(&mut i.t, src) {
        Err(e) => (
            None,
            Some(format!("Syntax error at line {}: {}", e.line, e.msg)),
        ),
        Ok(e) => match i.eval(&e) {
            Ok(v) => {
                for p in i.prints.drain(..) {
                    println!("{}", p);
                }
                let is_null = v.is_symbol(i.t.sys.null);
                let text = output_form(&i.t, &v);
                if is_null {
                    (None, None)
                } else {
                    i.out_history.push(v);
                    let n = i.out_history.len();
                    (Some(format!("Out[{}]= {}", n, text)), None)
                }
            }
            Err(Flow::Halt(msg)) => {
                i.depth = 0;
                (None, Some(msg))
            }
            Err(Flow::Throw(v, _)) => {
                let _ = v;
                (None, Some("Throw::nocatch: Throw not caught.".to_string()))
            }
            Err(Flow::Return(v)) => {
                let v = v.unwrap_or(Expr::Sym(i.t.sys.null));
                i.depth = 0;
                let text = output_form(&i.t, &v);
                i.out_history.push(v);
                let n = i.out_history.len();
                (Some(format!("Out[{}]= {}", n, text)), None)
            }
            Err(Flow::Abort) => {
                i.depth = 0;
                let v = Expr::Sym(i.t.intern("$Aborted"));
                i.out_history.push(v);
                let n = i.out_history.len();
                (Some(format!("Out[{}]= $Aborted", n)), None)
            }
            Err(_) => (None, Some("Evaluation aborted.".to_string())),
        },
    };
    // Collect any messages the evaluation produced (previously these were
    // dropped, so `rmath -e '1/0'` was silent about the infinite expression).
    let mut msgs: Vec<String> = i
        .messages
        .drain(..)
        .map(|(tag, text)| format!("{}: {}", tag, text))
        .collect();
    if let Some(f) = fatal {
        msgs.push(f);
    }
    (out, msgs)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Run on a big stack to allow deep recursion.
    let child = std::thread::Builder::new()
        .stack_size(1 << 29) // 512 MiB
        .spawn(move || real_main(&args[1..]))
        .unwrap();
    match child.join() {
        Ok(_) => {}
        Err(_) => {
            eprintln!("kernel panicked");
            std::process::exit(101);
        }
    }
}

fn real_main(args: &[String]) {
    if args.iter().any(|a| a == "--version") {
        println!("rmath 0.1.4 (stand-alone Mathematica-style kernel, Rust)");
        return;
    }
    // --server: JSON-RPC stdio mode for the Python front end.
    if args.iter().any(|a| a == "--server") {
        rmath::rpc::serve();
        return;
    }
    // -e "expr"
    if let Some(pos) = args.iter().position(|a| a == "-e" || a == "--eval") {
        if let Some(src) = args.get(pos + 1) {
            let mut i = Interp::new();
            let (out, msgs) = run_input(&mut i, src);
            for m in msgs {
                eprintln!("{}", m);
            }
            if let Some(o) = out {
                println!("{}", o.trim_start_matches(|c: char| c.is_ascii_digit() || c == '[' || c == ']' || c == '=' || c == ' '));
            }
            return;
        }
    }
    // Default: interactive REPL.
    let mut i = Interp::new();
    println!("rMath kernel 0.1.0 — a stand-alone Mathematica-style system (Rust)");
    println!("Enter expressions; Quit[] or Ctrl-D exits.");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut n = 0usize;
    loop {
        n += 1;
        print!("In[{}]:= ", n);
        let _ = out.flush();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "Quit[]" || line == "Exit[]" {
            break;
        }
        if line.starts_with('?') {
            let name = line.trim_start_matches('?').trim();
            if let Some(id) = i.t.get(name) {
                match &i.t.data[id].usage {
                    Some(u) => println!("{}", u),
                    None => println!("{} is a symbol.", name),
                }
            } else {
                println!("Unknown symbol {}", name);
            }
            continue;
        }
        let (out, msgs) = run_input(&mut i, line);
        for m in msgs {
            println!("  {}", m);
        }
        if let Some(o) = out {
            println!("{}", o);
        }
    }
    let _ = fullform; // silence unused in some cfgs
}