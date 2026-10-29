//! Robustness fuzzing for the lexer, parser and evaluator.
//!
//! Every input must either parse/evaluate or fail cleanly: no panic escaping the
//! engine, and no unbounded evaluation. Built-ins have a `catch_unwind` backstop
//! that turns a panic into a message, so a panic reaching this test is a real
//! bug in the lexer, parser or evaluation core.
//!
//! The generator is a deterministic PRNG (no external crates), so a failure is
//! reproducible from the printed input. Inputs are bounded (short token strings,
//! at most a few `^`) to keep the run fast and away from astronomically large
//! exact powers, and each evaluation carries a wall-clock deadline so a runaway
//! built-in loop is reported as an abort rather than hanging the suite.

use rmath::eval::Interp;
use rmath::parser::parse_input;
use std::time::{Duration, Instant};

/// xorshift64* — tiny, deterministic, good enough to mix token soup.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

const POOL: &[&str] = &[
    "1", "2", "0", "-1", "1/2", "1.5", "x", "y", "f", "Sin", "Pi", "I", "a$", "$$",
    "+", "-", "*", "/", "^", ".", "==", "=.", "=", ":=", "->", ":>", "/;", "//.", "/.",
    "&", "#", "##", "#2", "%", "%%", "_", "__", "___", "_?", "_:", "|",
    "[", "]", "{", "}", "(", ")", "<|", "|>", "<", ">", "<=", ">=", ",", ";",
    "@", "//", "@@", "@@@", "~", "!", "'", "√", "°", "\"a\"", "(*", "*)", "2^^", "\\",
    "\n", "  ", "?",
];

const MUT_CHARS: &[char] = &[
    '(', ')', '[', ']', '{', '}', '^', '/', '*', '+', '-', ',', ';', '#', '&', '@', '%',
    '"', '\\', '\n', '_', ':', '.', '!', '<', '>', '|', 'π', '√', '\u{1F600}',
];

const UNICODE: &[char] = &[
    'π', '√', '∞', '≤', '≥', '≠', '→', '−', '∕', '⁄', 'θ', 'α', '°', '\u{200b}',
    '\u{0301}', '\u{1F600}', 'é', '中',
];

const CORPUS: &[&str] = &[
    "f[n_] := f[n-1]*n; f[0] = 1; f[10]",
    "D[Sin[x]^2, x]",
    "Integrate[x^2, {x, 0, 1}]",
    "Expand[(x + 1)^5]",
    "Factor[x^2 - 1]",
    "Solve[x^2 - 3 x + 2 == 0, x]",
    "Table[i^2, {i, 5}]",
    "Module[{x = 3}, x^2]",
    "Block[{x = 3}, x^2]",
    "#1 + #2 &[3, 4]",
    "Fold[Plus, 0, {1, 2, 3, 4}]",
    "a ~ f ~ b",
    "x // f",
    "f @ x",
    "<|\"a\" -> 1, \"b\" -> 2|>",
    "StringJoin[\"a\", \"b\"]",
    "Sum[1/n^2, {n, 1, Infinity}]",
    "{a, b, c}[[2]]",
    "Simplify[Sin[x]^2 + Cos[x]^2]",
    "x_ + y_ /; x > 0",
    "2^^101 + 16^^ff",
    "Plot[Sin[x], {x, 0, 6}]",
    "\"a\\\"b\"",
    "1.5*^-3",
    "a = 5; b := a^2; b",
    "Cases[{1, 2, 3}, _?EvenQ]",
    "Integrate[1/(x^3 + x), x]",
    "Limit[Sin[x]/x, x -> 0]",
];

const ATOMS: &[&str] = &[
    "x", "y", "a", "n", "1", "2", "0", "3", "1/2", "1.5", "Pi", "E", "I", "True",
    "False", "Infinity",
];
const HEADS_1: &[&str] = &[
    "f", "g", "Sin", "Cos", "Tan", "Abs", "Sqrt", "Log", "Exp", "Expand", "Factor",
    "Simplify", "Total", "Length", "Head", "Sort", "Reverse", "Flatten", "First",
    "Numerator", "Denominator",
];
const HEADS_2: &[&str] = &[
    "f", "Plus", "Times", "Max", "Min", "Mod", "Map", "Fold", "Cases", "Count",
    "Position", "StringJoin", "Coefficient", "Table",
];

/// A grammar-based generator: the output is nearly always syntactically valid,
/// so it actually drives the evaluator (unlike token soup, which mostly fails
/// to parse). Depth is bounded and exponents stay small.
fn gen_expr(rng: &mut Rng, depth: usize) -> String {
    if depth == 0 || rng.below(5) == 0 {
        return (*rng.pick(ATOMS)).to_string();
    }
    let d = depth - 1;
    match rng.below(18) {
        0 => format!("({} + {})", gen_expr(rng, d), gen_expr(rng, d)),
        1 => format!("({})*({})", gen_expr(rng, d), gen_expr(rng, d)),
        2 => format!("({})/({})", gen_expr(rng, d), gen_expr(rng, d)),
        3 => format!("({})^({})", gen_expr(rng, d), rng.below(4)),
        4 => format!("{{{}, {}}}", gen_expr(rng, d), gen_expr(rng, d)),
        5 | 6 => format!("{}[{}]", rng.pick(HEADS_1), gen_expr(rng, d)),
        7 => format!("{}[{}, {}]", rng.pick(HEADS_2), gen_expr(rng, d), gen_expr(rng, d)),
        8 => format!("({}) == ({})", gen_expr(rng, d), gen_expr(rng, d)),
        9 => format!("({}) -> ({})", gen_expr(rng, d), gen_expr(rng, d)),
        10 => format!("D[{}, x]", gen_expr(rng, d)),
        11 => format!("Integrate[{}, x]", gen_expr(rng, d)),
        12 => format!("Table[{}, {{i, 1, {}}}]", gen_expr(rng, d), 1 + rng.below(4)),
        13 => format!("Sum[{}, {{i, 1, {}}}]", gen_expr(rng, d), 1 + rng.below(4)),
        14 => format!("Module[{{t = {}}}, {}]", gen_expr(rng, 0), gen_expr(rng, d)),
        15 => format!(
            "If[({}) > 0, {}, {}]",
            gen_expr(rng, d),
            gen_expr(rng, d),
            gen_expr(rng, d)
        ),
        16 => format!("({}_ + {}_) /; ({} > 0)", "x", "y", gen_expr(rng, d)),
        _ => format!("{}[{}]", rng.pick(HEADS_1), gen_expr(rng, d)),
    }
}

/// Parse and evaluate `src`. `Ok(true)` means it parsed (and so exercised the
/// evaluator), `Ok(false)` a clean syntax error, `Err(())` a panic.
fn exercise(src: &str) -> Result<bool, ()> {
    let mut i = Interp::new();
    i.rec_limit = 256;
    i.iter_limit = 20_000;
    i.deadline = Some(Instant::now() + Duration::from_millis(500));
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match parse_input(&mut i.t, src) {
            Ok(e) => {
                let _ = i.eval(&e);
                true
            }
            Err(_) => false,
        }
    }));
    // A built-in has a `catch_unwind` backstop that turns its own panic into a
    // `Name::panic` message, so a panic that never escapes still counts as a
    // failure here. That is the check that would have caught `Integrate`
    // dividing by zero when a rule met its own base case.
    if i.messages.iter().any(|(tag, _)| tag.ends_with("::panic")) {
        return Err(());
    }
    run.map_err(|_| ())
}

fn gen_soup(rng: &mut Rng) -> String {
    let n = 1 + rng.below(28);
    let mut out = String::new();
    let mut carets = 0;
    for _ in 0..n {
        let mut tok = *rng.pick(POOL);
        if tok == "^" {
            carets += 1;
            if carets > 3 {
                tok = "+";
            }
        }
        out.push_str(tok);
        if rng.below(3) == 0 {
            out.push(' ');
        }
    }
    out
}

fn mutate(rng: &mut Rng, base: &str) -> String {
    let mut chars: Vec<char> = base.chars().collect();
    for _ in 0..1 + rng.below(3) {
        if chars.is_empty() {
            break;
        }
        let i = rng.below(chars.len());
        match rng.below(3) {
            0 => {
                chars.remove(i);
            }
            1 => chars.insert(i, *rng.pick(MUT_CHARS)),
            _ => chars[i] = *rng.pick(MUT_CHARS),
        }
    }
    chars.into_iter().collect()
}

fn gen_unicode(rng: &mut Rng) -> String {
    let n = 1 + rng.below(20);
    (0..n)
        .map(|_| match rng.below(4) {
            0 => char::from_u32(0x1F600 + rng.below(80) as u32).unwrap_or('?'),
            1 => char::from_u32(rng.below(0x3000) as u32).unwrap_or('?'),
            2 => char::from_u32(0x2200 + rng.below(0x100) as u32).unwrap_or('?'),
            _ => *rng.pick(UNICODE),
        })
        .collect()
}

/// Returns `(panicking inputs, inputs that parsed)`. The parsed count is a
/// coverage floor: if generation drifts into pure syntax errors the test should
/// fail rather than quietly stop exercising the evaluator.
fn run_cases(seed: u64) -> (Vec<String>, usize) {
    fn record(s: String, failures: &mut Vec<String>, parsed: &mut usize) {
        match exercise(&s) {
            Ok(true) => *parsed += 1,
            Ok(false) => {}
            Err(()) => failures.push(s),
        }
    }
    let mut rng = Rng::new(seed);
    let mut failures = Vec::new();
    let mut parsed = 0usize;
    for _ in 0..1200 {
        record(gen_soup(&mut rng), &mut failures, &mut parsed);
    }
    for _ in 0..600 {
        let base = *rng.pick(CORPUS);
        record(mutate(&mut rng, base), &mut failures, &mut parsed);
    }
    for _ in 0..400 {
        record(gen_unicode(&mut rng), &mut failures, &mut parsed);
    }
    for _ in 0..400 {
        record(gen_expr(&mut rng, 4), &mut failures, &mut parsed);
    }
    (failures, parsed)
}

/// Run the corpus on a roomy stack, like the kernel binary's own (it runs on
/// 512 MiB), so `$RecursionLimit`-bounded recursion does not overflow the
/// default test thread.
fn run_on_big_stack(seed: u64) -> (Vec<String>, usize) {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || run_cases(seed))
        .expect("spawn fuzz thread")
        .join()
        .expect("fuzz thread panicked")
}

#[test]
fn lexer_parser_evaluator_survive_fuzzing() {
    let (failures, parsed) = run_on_big_stack(0x9E37_79B9_7F4A_7C15);
    println!("fuzz: {parsed} inputs parsed, {} panicked", failures.len());
    assert!(
        failures.is_empty(),
        "{} fuzz input(s) panicked; first few: {:#?}",
        failures.len(),
        &failures[..failures.len().min(5)]
    );
    assert!(
        parsed >= 400,
        "only {parsed} inputs parsed; the generator stopped exercising the evaluator"
    );
}

#[test]
fn generators_are_deterministic() {
    // The same seed must reproduce the same corpus of inputs, so a reported
    // failure can be replayed. (Checked on the generators, not by re-running
    // every evaluation, to keep the test fast.)
    let gen_all = |seed| {
        let mut rng = Rng::new(seed);
        let mut out = Vec::new();
        for _ in 0..100 {
            out.push(gen_soup(&mut rng));
        }
        for _ in 0..100 {
            out.push(mutate(&mut rng, "D[Sin[x]^2, x]"));
        }
        for _ in 0..100 {
            out.push(gen_unicode(&mut rng));
        }
        for _ in 0..100 {
            out.push(gen_expr(&mut rng, 4));
        }
        out
    };
    assert_eq!(gen_all(12345), gen_all(12345));
    assert_ne!(gen_all(12345), gen_all(999));
}
