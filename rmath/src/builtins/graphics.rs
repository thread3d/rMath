//! Graphics built-ins: Plot, ListPlot, ParametricPlot producing a
//! Graphics[primitives, options] node that the front end renders as SVG.

use crate::builtins::core::unevaluated_call;
use crate::eval::{EvalRes, Flow, Interp};
use crate::expr::Expr;

/// Split trailing Rule arguments as options; returns (positional, options).
pub fn split_options(i: &mut Interp, args: &[Expr]) -> Result<(Vec<Expr>, Vec<(String, Expr)>), Flow> {
    let sys = i.t.sys;
    let mut pos = Vec::new();
    let mut opts = Vec::new();
    for a in args {
        if let Some((h, hargs)) = a.parts() {
            if h.is_symbol(sys.rule) && hargs.len() == 2 {
                if let Some(name) = hargs[0].sym_id() {
                    let v = i.eval(&hargs[1])?;
                    opts.push((i.t.name_of(name).to_string(), v));
                    continue;
                }
            }
        }
        pos.push(a.clone());
    }
    Ok((pos, opts))
}

fn opt_value<'a>(opts: &'a [(String, Expr)], name: &str) -> Option<&'a Expr> {
    opts.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn real_val(i: &mut Interp, e: &Expr) -> Result<Option<f64>, Flow> {
    let v = i.eval(e)?;
    let direct = match &v {
        Expr::Int(n) => Some(n.to_f64()),
        Expr::Rat(r) => Some(r.to_f64()),
        Expr::Real(x) => Some(*x),
        _ => None,
    };
    if direct.is_some() {
        return Ok(direct);
    }
    // Symbolic numerics like 2 Pi: evaluate to machine precision.
    let approx = crate::builtins::num::numeric_eval(i, &v);
    match &approx {
        Expr::Real(x) => Ok(Some(*x)),
        Expr::Int(n) => Ok(Some(n.to_f64())),
        _ => Ok(None),
    }
}

/// Build a Graphics expression: Graphics[prims, {plotrange rules, ...}]
pub fn make_graphics(
    i: &mut Interp,
    prims: Vec<Expr>,
    opts: Vec<(String, Expr)>,
    plot_range: Option<[f64; 4]>,
) -> EvalRes {
    let sys = i.t.sys;
    let mut args = vec![Expr::node(Expr::Sym(sys.list), prims)];
    if let Some(pr) = plot_range {
        args.push(Expr::node(
            Expr::Sym(sys.rule),
            vec![
                Expr::Sym(i.t.intern("PlotRange")),
                Expr::node(
                    Expr::Sym(sys.list),
                    vec![
                        Expr::node(
                            Expr::Sym(sys.list),
                            vec![Expr::real(pr[0]), Expr::real(pr[1])],
                        ),
                        Expr::node(
                            Expr::Sym(sys.list),
                            vec![Expr::real(pr[2]), Expr::real(pr[3])],
                        ),
                    ],
                ),
            ],
        ));
    }
    for (k, v) in opts {
        args.push(Expr::node(Expr::Sym(sys.rule), vec![Expr::Sym(i.t.intern(&k)), v]));
    }
    Ok(Expr::node(Expr::Sym(i.t.intern("Graphics")), args))
}

fn line_prim(i: &mut Interp, pts: Vec<(f64, f64)>) -> Expr {
    let pts: Vec<Expr> = pts
        .into_iter()
        .map(|(x, y)| {
            Expr::node(
                Expr::Sym(i.t.sys.list),
                vec![Expr::real(x), Expr::real(y)],
            )
        })
        .collect();
    Expr::node(
        Expr::Sym(i.t.intern("Line")),
        vec![Expr::node(Expr::Sym(i.t.sys.list), pts)],
    )
}

// ============================ Plot ============================

pub fn b_plot(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("Plot", "argr");
        return Ok(unevaluated_call(i, "Plot", args));
    }
    let f = args[0].clone();
    let Some((h, hargs)) = args[1].parts() else {
        i.message("Plot", "argr");
        return Ok(unevaluated_call(i, "Plot", args));
    };
    if !(h.is_symbol(i.t.sys.list) && hargs.len() == 3) {
        i.message("Plot", "argr");
        return Ok(unevaluated_call(i, "Plot", args));
    }
    let v = i.eval(&hargs[0])?;
    let a = match real_val(i, &hargs[1])? {
        Some(v) => v,
        None => return Ok(unevaluated_call(i, "Plot", args)),
    };
    let b = match real_val(i, &hargs[2])? {
        Some(v) => v,
        None => return Ok(unevaluated_call(i, "Plot", args)),
    };
    let (pos, opts) = split_options(i, &args[2..])?;
    let _ = pos;
    let n = 241usize;
    let mut pts: Vec<(f64, f64)> = Vec::new();
    #[allow(unused_mut)]
    let mut segments: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut prev_y: Option<f64> = None;
    for k in 0..n {
        let t = a + (b - a) * (k as f64) / ((n - 1) as f64);
        let xe = Expr::real(t);
        let sub = crate::pattern::subst_syms_all(
            &i.t,
            &f,
            &[v.sym_id().unwrap_or(usize::MAX)],
            &[Expr::real(t)],
        );
        let y = i.eval(&sub)?;
        let yv = match &y {
            Expr::Int(nn) => Some(nn.to_f64()),
            Expr::Rat(r) => Some(r.to_f64()),
            Expr::Real(x) => Some(*x),
            _ => None,
        };
        let jump = match (prev_y, yv) {
            (Some(p), Some(c)) => (c - p).abs() > 1e6,
            _ => false,
        };
        match yv {
            Some(yv) if yv.is_finite() => {
                if let (Some(p), Some(py)) = (pts.last(), prev_y) {
                    let _ = p;
                    if (yv - py).abs() > 1e6 {
                        segments.push(std::mem::take(&mut pts));
                    }
                }
                pts.push((t, yv));
                prev_y = Some(yv);
            }
            _ => {
                if !pts.is_empty() {
                    segments.push(std::mem::take(&mut pts));
                }
                prev_y = None;
            }
        }
        let _ = sub;
    }
    if !pts.is_empty() {
        segments.push(pts);
    }
    let mut prims = Vec::new();
    for seg in &segments {
        if seg.len() >= 2 {
            let seg_owned = seg.clone();
            prims.push(line_prim(i, seg_owned));
        }
    }
    // Plot range: y over sampled points.
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for seg in &segments {
        for (_, y) in seg.iter() {
            if y.is_finite() {
                if *y < ymin {
                    ymin = *y;
                }
                if *y > ymax {
                    ymax = *y;
                }
            }
        }
    }
    let pr = match opt_value(&opts, "PlotRange") {
        Some(Expr::Sym(id)) if *id == i.t.intern("All") => {
            Some([a, b, ymin, ymax])
        }
        _ => {
            // Auto: pad a bit (and clip insane asymptote values).
            let lo = if ymin.is_finite() { ymin } else { -1.0 };
            let hi = if ymax.is_finite() { ymax } else { 1.0 };
            let pad = (hi - lo).abs().max(1.0) * 0.05;
            Some([a, b, lo - pad, hi + pad])
        }
    };
    make_graphics(i, prims, opts, pr)
}

// ============================ ListPlot ============================

pub fn b_listplot(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 1 {
        i.message("ListPlot", "argr");
        return Ok(unevaluated_call(i, "ListPlot", args));
    }
    let data = i.eval(&args[0])?;
    let (pos, opts) = split_options(i, &args[1..])?;
    let _ = pos;
    let joined = matches!(opt_value(&opts, "Joined"), Some(Expr::Sym(id)) if *id == i.t.sys.r#true);
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for (k, e) in data.node_args().iter().enumerate() {
        let y: f64 = match e {
            Expr::Int(n) => n.to_f64(),
            Expr::Rat(r) => r.to_f64(),
            Expr::Real(x) => *x,
            _ => match real_val(i, e)? {
                Some(v) => v,
                None => continue,
            },
        };
        pts.push((k as f64, y));
    }
    let xs: Vec<f64> = pts.iter().map(|(x, _)| *x).collect();
    let ys: Vec<f64> = pts.iter().map(|(_, y)| *y).collect();
    let xmin = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let xmax = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let ymin = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let ymax = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mut prims = Vec::new();
    if joined {
        prims.push(line_prim(i, pts.clone()));
    } else {
        for (x, y) in &pts {
            prims.push(point_prim(i, *x, *y));
        }
    }
    make_graphics(i, prims, opts, Some([xmin, xmax, ymin, ymax]))
}

pub fn b_listlineplot(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 1 {
        return Ok(unevaluated_call(i, "ListLinePlot", args));
    }
    let data = i.eval(&args[0])?;
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for (k, e) in data.node_args().iter().enumerate() {
        if let Some(y) = real_val(i, e)? {
            pts.push((k as f64, y));
        }
    }
    let xs = pts.iter().map(|(x, _)| *x).collect::<Vec<_>>();
    let ys = pts.iter().map(|(_, y)| *y).collect::<Vec<_>>();
    let prims = vec![line_prim(i, pts)];
    make_graphics(
        i,
        prims,
        vec![],
        Some([
            xs.iter().cloned().fold(f64::INFINITY, f64::min),
            xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            ys.iter().cloned().fold(f64::INFINITY, f64::min),
            ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        ]),
    )
}

fn point_prim(i: &mut Interp, x: f64, y: f64) -> Expr {
    let pt = Expr::node(
        Expr::Sym(i.t.sys.list),
        vec![Expr::real(x), Expr::real(y)],
    );
    Expr::node(Expr::Sym(i.t.intern("Point")), vec![pt])
}

// ============================ ParametricPlot ============================

pub fn b_parametricplot(i: &mut Interp, args: &[Expr]) -> EvalRes {
    if args.len() < 2 {
        i.message("ParametricPlot", "argr");
        return Ok(unevaluated_call(i, "ParametricPlot", args));
    }
    // ParametricPlot[{fx, fy}, {t, a, b}]
    let fxy = i.eval(&args[0])?;
    let (fx, fy) = match pair_of(i, &fxy)? {
        Some(x) => x,
        None => return Ok(unevaluated_call(i, "ParametricPlot", args)),
    };
    let Some((h, hargs)) = args[1].parts() else {
        return Ok(unevaluated_call(i, "ParametricPlot", args));
    };
    if !(h.is_symbol(i.t.sys.list) && hargs.len() == 3) {
        return Ok(unevaluated_call(i, "ParametricPlot", args));
    }
    let t = i.eval(&hargs[0])?;
    let a = match real_val(i, &hargs[1])? {
        Some(v) => v,
        None => return Ok(unevaluated_call(i, "ParametricPlot", args)),
    };
    let b = match real_val(i, &hargs[2])? {
        Some(v) => v,
        None => return Ok(unevaluated_call(i, "ParametricPlot", args)),
    };
    let n = 241usize;
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for k in 0..n {
        let tv = a + (b - a) * (k as f64) / ((n - 1) as f64);
        let px = crate::pattern::subst_syms_all(
            &i.t,
            &fx,
            &[t.sym_id().unwrap_or(usize::MAX)],
            &[Expr::real(tv)],
        );
        let x = i.eval(&px)?;
        let py = crate::pattern::subst_syms_all(
            &i.t,
            &fy,
            &[t.sym_id().unwrap_or(usize::MAX)],
            &[Expr::real(tv)],
        );
        let y = i.eval(&py)?;
        if let (Some(xv), Some(yv)) = (num_f64(&x), num_f64(&y)) {
            pts.push((xv, yv));
        }
    }
    let xs = pts.iter().map(|(x, _)| *x).collect::<Vec<_>>();
    let ys = pts.iter().map(|(_, y)| *y).collect::<Vec<_>>();
    let prims = vec![line_prim(i, pts)];
    make_graphics(
        i,
        prims,
        vec![],
        Some([
            xs.iter().cloned().fold(f64::INFINITY, f64::min),
            xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            ys.iter().cloned().fold(f64::INFINITY, f64::min),
            ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        ]),
    )
}

fn pair_of(i: &mut Interp, e: &Expr) -> Result<Option<(Expr, Expr)>, Flow> {
    if e.is_list(&i.t) && e.list_len() == 2 {
        let a = e.node_args();
        return Ok(Some((a[0].clone(), a[1].clone())));
    }
    Ok(None)
}

fn num_f64(e: &Expr) -> Option<f64> {
    match e {
        Expr::Int(n) => Some(n.to_f64()),
        Expr::Rat(r) => Some(r.to_f64()),
        Expr::Real(x) => Some(*x),
        _ => None,
    }
}

