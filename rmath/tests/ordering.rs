//! Property tests for the ordering invariants the evaluator depends on.
//!
//! `Plus`/`Times` normalization sorts an Orderless head and then compares the
//! result with `same_q` to decide whether it reached a fixpoint, so the
//! comparators must be total orders and every printed result must re-parse to
//! itself. A regression here shows up as an evaluation loop or an unstable
//! output, which is what these tests pin.

use rmath::eval::{sort_cmp, Interp};
use rmath::expr::Expr;
use rmath::fmt::output_form;
use rmath::parser::parse_input;
use std::cmp::Ordering as O;

fn eval_str(i: &mut Interp, src: &str) -> String {
    let e = parse_input(&mut i.t, src).unwrap_or_else(|e| panic!("parse {src:?}: {}", e.msg));
    match i.eval(&e) {
        Ok(v) => output_form(&i.t, &v),
        Err(_) => format!("<aborted: {src}>"),
    }
}

fn parse_all(i: &mut Interp, srcs: &[&str]) -> Vec<Expr> {
    srcs.iter()
        .map(|s| parse_input(&mut i.t, s).unwrap_or_else(|e| panic!("parse {s:?}: {}", e.msg)))
        .collect()
}

#[test]
fn sort_cmp_is_a_total_order() {
    let mut i = Interp::new();
    let exprs = parse_all(
        &mut i,
        &[
            "x", "y", "x^2", "y^3", "x + y", "a + b", "a + b + c", "2", "3/2", "1.5",
            "Pi", "E", "GoldenRatio", "Sqrt[2]", "Sin[x]", "Cos[x]", "Log[x]", "Exp[x]",
            "2 x", "x y", "1/2 x^2",
        ],
    );
    for a in &exprs {
        for b in &exprs {
            assert_eq!(
                sort_cmp(&i.t, a, b),
                sort_cmp(&i.t, b, a).reverse(),
                "antisymmetry: {} vs {}",
                output_form(&i.t, a),
                output_form(&i.t, b)
            );
        }
    }
    for a in &exprs {
        for b in &exprs {
            for c in &exprs {
                if sort_cmp(&i.t, a, b) != O::Greater && sort_cmp(&i.t, b, c) != O::Greater {
                    assert_ne!(
                        sort_cmp(&i.t, a, c),
                        O::Greater,
                        "transitivity: {} <= {} <= {}",
                        output_form(&i.t, a),
                        output_form(&i.t, b),
                        output_form(&i.t, c)
                    );
                }
            }
        }
    }
}

#[test]
fn sort_is_idempotent_and_agrees_with_ordering() {
    let mut i = Interp::new();
    let list = "{y, x^2, x + y, y^3, Pi, E, 2, 3, 1, Sqrt[2], a + b + c, a + b, a + c}";
    let once = eval_str(&mut i, &format!("Sort[{list}]"));
    let twice = eval_str(&mut i, &format!("Sort[Sort[{list}]]"));
    assert_eq!(once, twice, "Sort must be idempotent: {once} vs {twice}");
    let via_ordering = eval_str(&mut i, &format!("{list}[[Ordering[{list}]]]"));
    assert_eq!(
        once, via_ordering,
        "Sort[list] must equal list[[Ordering[list]]]"
    );
}

#[test]
fn evaluation_output_is_a_fixpoint() {
    // Re-evaluating a printed result must reproduce it exactly; an unstable
    // Orderless order would show up here as churn.
    let mut i = Interp::new();
    let corpus = [
        "Expand[(x + y + z)^3]",
        "Expand[(x + 1)^10]",
        "Together[1/x + 1/y]",
        "Cancel[(x^2 - 1)/(x - 1)]",
        "Apart[1/(x^2 - 1), x]",
        "Factor[x^3 - 1]",
        "Collect[x + x^2 + 2 x^2 + 3, x]",
        "D[Sin[x]^2, x]",
        "Integrate[x^2 Exp[x], x]",
        "Integrate[1/(x^2 - 1), x]",
        "Sum[1/n^2, {n, 1, Infinity}]",
        "Limit[Sin[x]/x, x -> 0]",
        "Solve[x^2 - 3 x + 2 == 0, x]",
        "Coefficient[(x + y)^4, x y^3]",
        "Sort[{y, x^2, x + y, y^3}]",
    ];
    for src in corpus {
        let out = eval_str(&mut i, src);
        assert!(!out.starts_with("<aborted"), "{src} aborted");
        let again = eval_str(&mut i, &out);
        assert_eq!(out, again, "output of {src} is not a fixpoint");
    }
}
