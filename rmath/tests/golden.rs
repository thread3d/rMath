//! Golden end-to-end tests: evaluate expressions through the full
//! parse → eval → OutputForm pipeline and compare with expected strings.

use rmath::eval::Interp;
use rmath::fmt::output_form;
use rmath::parser::parse_input;

/// Run one input through the kernel, returning the printed result.
fn eval_one(i: &mut Interp, src: &str) -> Result<String, String> {
    i.messages.clear();
    i.prints.clear();
    let e = parse_input(&mut i.t, src).map_err(|e| format!("syntax: {}", e.msg))?;
    match i.eval(&e) {
        Ok(v) => {
            if v.is_symbol(i.t.sys.null) {
                Ok(String::new())
            } else {
                Ok(output_form(&i.t, &v))
            }
        }
        Err(_) => Err("evaluation aborted".to_string()),
    }
}

fn run_golden_file(path: &std::path::Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut i = Interp::new();
    let mut line_no = 0usize;
    let mut pending: Option<(String, String)> = None; // (label, expected)
    for line in text.lines() {
        line_no += 1;
        let line = line.trim();
        // Comments are "# ..." (a bare "#" starts a slot expression: #^2 &).
        if line.is_empty() || line.starts_with("# ") || line == "#" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("=>") {
            let expected = rest.trim();
            let Some((label, input)) = pending.take() else {
                panic!("{}:{}: '=>' without a preceding expression", path.display(), line_no);
            };
            let got = match eval_one(&mut i, &input) {
                Ok(v) => v,
                Err(e) => format!("<error: {}>", e),
            };
            assert_eq!(
                expected,
                &got,
                "{}:{} [{}]\n  input:    {}\n  expected: {}\n  got:      {}",
                path.display(),
                line_no,
                label,
                input,
                expected,
                got
            );
        } else {
            pending = Some((String::new(), line.to_string()));
        }
    }
    Ok(())
}

#[test]
fn golden_suite() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("golden dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "mt"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no golden files found");
    for f in files {
        let _ = run_golden_file(&f);
    }
}