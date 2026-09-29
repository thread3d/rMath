//! `Steps[expr]` — an ordered derivation of an expression, and `Trace[expr]` —
//! a general evaluation trace.
//!
//! Instrumented operations (D, Integrate, Limit, Simplify, Apart, Factor,
//! Solve) record named steps as they work; anything else falls back to the
//! general trace of successive evaluation forms. Steps are returned as a list
//! of `Step[label, expr]` nodes, which the OutputForm and TeX printers render
//! as `label: expr`.

use crate::builtins::core::unevaluated_call;
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::{same_q, Expr};

fn step_node(i: &mut Interp, label: &str, e: &Expr) -> Expr {
    let id = i.t.intern("Step");
    Expr::node(Expr::Sym(id), vec![Expr::from_str(label), e.clone()])
}

fn as_list(i: &mut Interp, steps: &[(String, Expr)]) -> Expr {
    let out: Vec<Expr> = steps.iter().map(|(l, e)| step_node(i, l, e)).collect();
    Expr::node(Expr::Sym(i.t.sys.list), out)
}

fn generic_steps(raw: &Expr, forms: Vec<Expr>) -> Vec<(String, Expr)> {
    let mut steps = vec![("Given".to_string(), raw.clone())];
    for f in forms {
        steps.push(("eval".to_string(), f));
    }
    steps
}

/// Evaluate `raw` with both recorders active. Returns the labeled steps
/// (including the Given and Result) and the evaluation result. Shared by the
/// `Steps` builtin and the notebook's "Show steps" toggle.
pub fn run_with_steps(i: &mut Interp, raw: &Expr) -> Result<(Vec<(String, Expr)>, Expr), Flow> {
    let saved_steps = i.steps.take();
    let saved_trace = i.eval_trace.take();
    i.steps = Some(vec![("Given".to_string(), raw.clone())]);
    i.eval_trace = Some(Vec::new());
    let res = i.eval(raw);
    let mut steps = i.steps.take().unwrap_or_default();
    let forms = i.eval_trace.take().unwrap_or_default();
    i.steps = saved_steps;
    i.eval_trace = saved_trace;
    let result = res?;
    // Nothing instrumented fired: use the general evaluation trace.
    let instrumented = steps.len() > 1;
    if !instrumented {
        steps = generic_steps(raw, forms);
    }
    // Keep the last rule's name, and add the final result (the generic trace
    // already ends at the result, so only add it when it differs).
    if instrumented || steps.last().map_or(true, |(_, e)| !same_q(e, &result)) {
        steps.push(("Result".to_string(), result.clone()));
    }
    Ok((steps, result))
}

pub fn b_steps(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("Steps", "argr");
        return Ok(unevaluated_call(i, "Steps", args));
    }
    let raw = args[0].clone();
    let (steps, _result) = run_with_steps(i, &raw)?;
    Ok(as_list(i, &steps))
}

pub fn b_trace(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.is_empty() {
        i.message("Trace", "argr");
        return Ok(unevaluated_call(i, "Trace", args));
    }
    let raw = args[0].clone();
    let saved = i.eval_trace.take();
    i.eval_trace = Some(Vec::new());
    let res = i.eval(&raw);
    let forms = i.eval_trace.take().unwrap_or_default();
    i.eval_trace = saved;
    res?;
    let steps = generic_steps(&raw, forms);
    Ok(as_list(i, &steps))
}
