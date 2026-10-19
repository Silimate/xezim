//! Code coverage: statement, branch, condition, expression and toggle counts,
//! collected when the run asks for them (`--code-coverage`, `+cover`) and
//! written to the results file beside the functional coverage.
//!
//! Statements and branches are instrumented at compile time, on the
//! elaborated design, before any block is lowered: a counted statement gets a
//! `$__xz_cov(<n>)` call in front of it, a branch arm one at its start, and a
//! `?:` condition is wrapped in `$__xz_covc(<n>, cond)`. The bytecode
//! compiler turns those into `Insn::CovHit`; the interpreter counts them
//! itself. Every execution path therefore counts the same way, and a run
//! without code coverage compiles exactly the design it did before.
//! Continuous assignments count in their comb entry (`ca_counter`). Toggles
//! compare each tracked signal with its value at the end of the previous
//! time slot, off the dirty set the waveform writers share; a slot sampled
//! more than once (once per tick and at its end) counts only its net change.
//!
//! Code a constant condition rules out (a `case` on a parameter) is left
//! out of the totals: instrumentation records each `Decision` a constant
//! may make, and `cov_resolve_constants` makes them once the design is
//! compiled, with the evaluator the run uses, and marks the sites excluded.
//!
//! Condition and expression coverage are focused expression coverage: a
//! logic expression (an `if`, `while` or `?:` condition, or a 1-bit
//! assignment value) is split into its input terms, and each term's two rows
//! (the term at 0, and at 1) count when the term alone decides the result.
//! The expression is wrapped in `$__xz_cove(<n>, expr)`, which the
//! interpreter evaluates itself (the bytecode hands it over): it walks the
//! item's logic tree (`ExprSite::nodes`) in the order the interpreter would,
//! with the same short-circuits, and then marks the rows whose term is not
//! masked. An item of a continuous assignment or at the top level of a
//! combinational block also counts the transient states its inputs went
//! through since its last evaluation (`Simulator::cov_replay`).
use super::fsm_cov::{FSM_MARK, FsmCov, FsmRow};
use super::*;
use crate::ast::Span;
use crate::compiler::elaborate::InitialBlock;

pub const KIND_STATEMENT: u8 = 1;
pub const KIND_BRANCH: u8 = 2;
pub const KIND_TOGGLE: u8 = 4;
pub const KIND_CONDITION: u8 = 8;
pub const KIND_EXPRESSION: u8 = 16;
pub const KIND_FSM: u8 = 32;
/// Every kind: what `--code-coverage` and `+cover` collect by default.
pub const KIND_ALL: u8 = 63;
/// The kinds instrumented in the code (toggles watch the signals instead;
/// FSMs mark their state processes after these).
const KIND_CODE: u8 = KIND_STATEMENT | KIND_BRANCH | KIND_CONDITION | KIND_EXPRESSION;
/// Result names of the kinds, bit `i` of a kind mask naming entry `i`.
const KIND_NAMES: [&str; 6] = [
    "statement",
    "branch",
    "toggle",
    "condition",
    "expression",
    "fsm",
];
type Tallies = [Tally; KIND_NAMES.len()];

/// `$__xz_cov(<n>)`: counts counter `n` (a statement or a branch arm).
pub(crate) const HIT_TASK: &str = "$__xz_cov";
/// `$__xz_covc(<n>, cond)`: yields `cond`, counting counter `n` when it is
/// true and `n + 1` when it is false (a `?:` condition).
pub(crate) const COND_FN: &str = "$__xz_covc";
/// `$__xz_cove(<n>, expr)`: yields `expr` (a 1-bit value), counting the rows
/// of condition or expression item `n` (`CodeCov::exprs`).
pub(crate) const EXPR_FN: &str = "$__xz_cove";
/// Items with more input terms than this are not covered.
const MAX_TERMS: usize = 32;
/// Message tasks whose arguments' `?:` conditions are condition items.
const DISPLAY_TASKS: [&str; 14] = [
    "$display",
    "$displayb",
    "$displayh",
    "$displayo",
    "$write",
    "$strobe",
    "$fdisplay",
    "$fwrite",
    "$fstrobe",
    "$sformat",
    "$error",
    "$warning",
    "$info",
    "$fatal",
];

/// Library packages left out unless `--code-coverage-scope` names them.
const LIBRARY_PACKAGES: [&str; 2] = ["std", "uvm_pkg"];

/// What the command line asked for. `scopes` limits every kind to the
/// instances at or below the listed paths and to the listed packages (empty:
/// the whole design). `exclude_input_ports` leaves input and inout ports out
/// of toggle coverage, so the net that drives one carries its toggles
/// (`--no-toggle-input-ports`); by default each instance counts its own.
/// `integers` keeps the signals declared with an integer atom type
/// (`TOGGLE_INTEGER_TYPES`) in toggle coverage; on by default,
/// `--no-toggle-integers` turns it off.
/// `keep_constant` keeps the statements and branch arms a constant
/// condition rules out in the totals (see `Decision`).
///
/// The rest select other counting rules, each off by default (see
/// docs/coverage-guide.md, "Counting rules"):
/// - `enums` keeps enum variables in toggle coverage; `--no-toggle-enums`
///   leaves them out.
/// - `generate` keeps the signals declared in generate blocks in toggle
///   coverage; `--no-toggle-generate` leaves them out.
/// - `logical` scores only the conditions that hold a `&&` or `||`, a term
///   counting once it has been 0 and 1 (`--code-coverage-expressions=logical`).
/// - `blocks` counts straight-line blocks instead of statements
///   (`--code-coverage-granularity=block`).
/// - `fsm_reset` keeps the FSM transitions that only a reset makes;
///   `--no-fsm-reset-transitions` leaves them out.
#[derive(Clone, Debug)]
pub struct CodeCoverage {
    pub kinds: u8,
    pub scopes: Vec<String>,
    pub exclude_input_ports: bool,
    pub integers: bool,
    pub keep_constant: bool,
    pub enums: bool,
    pub generate: bool,
    pub logical: bool,
    pub blocks: bool,
    pub fsm_reset: bool,
}

impl Default for CodeCoverage {
    fn default() -> Self {
        CodeCoverage {
            kinds: 0,
            scopes: Vec::new(),
            exclude_input_ports: false,
            integers: true,
            keep_constant: false,
            enums: true,
            generate: true,
            logical: false,
            blocks: false,
            fsm_reset: true,
        }
    }
}

/// `--code-coverage-expressions=<mode>`: `all` (the default) scores every
/// condition and expression item; `logical` only the conditions with a `&&`
/// or `||`, their operands the terms, each covered once it has been 0 and 1.
pub fn parse_expression_mode(spec: &str) -> Result<bool, String> {
    match spec.trim() {
        "all" => Ok(false),
        "logical" => Ok(true),
        other => Err(format!(
            "unknown expression mode '{}' (want all or logical)",
            other
        )),
    }
}

/// `--code-coverage-granularity=<unit>`: `statement` (the default) counts
/// each statement; `block` each straight-line block.
pub fn parse_granularity(spec: &str) -> Result<bool, String> {
    match spec.trim() {
        "statement" => Ok(false),
        "block" => Ok(true),
        other => Err(format!(
            "unknown granularity '{}' (want statement or block)",
            other
        )),
    }
}

/// Why a statement, branch or arm is left out of the totals.
const EXCLUDED: &str = "constant";

/// `--code-coverage-constant=<mode>`: `exclude` (the default) leaves code a
/// constant condition rules out of the totals, `keep` counts it.
pub fn parse_constant_mode(spec: &str) -> Result<bool, String> {
    match spec.trim() {
        "exclude" => Ok(false),
        "keep" => Ok(true),
        other => Err(format!(
            "unknown constant-code mode '{}' (want exclude or keep)",
            other
        )),
    }
}

/// The integer atom types of §6.11 (Table 6-8) whose signals toggle
/// coverage leaves out without `integers`; `time` is never tracked.
const TOGGLE_INTEGER_TYPES: [&str; 5] = ["byte", "shortint", "int", "longint", "integer"];

static CONFIG: Mutex<Option<CodeCoverage>> = Mutex::new(None);

/// Select code coverage for the simulators built after this call; `None` or
/// no kinds turns it off.
pub fn set_code_coverage(cfg: Option<CodeCoverage>) {
    if let Ok(mut c) = CONFIG.lock() {
        *c = cfg.filter(|c| c.kinds != 0);
    }
}

pub(crate) fn code_coverage() -> Option<CodeCoverage> {
    CONFIG.lock().ok().and_then(|c| c.clone())
}

/// The kinds a `--code-coverage` list names: `stmt` (or `statement`, `s`),
/// `branch` (`b`), `cond` (`condition`, `c`), `expr` (`expression`, `e`),
/// `toggle` (`t`), `fsm` (`f`) or `all`, separated by commas.
pub fn parse_kinds(spec: &str) -> Result<u8, String> {
    let mut kinds = 0u8;
    for k in spec.split(',').map(str::trim).filter(|k| !k.is_empty()) {
        kinds |= match k {
            "stmt" | "statement" | "s" => KIND_STATEMENT,
            "branch" | "b" => KIND_BRANCH,
            "cond" | "condition" | "c" => KIND_CONDITION,
            "expr" | "expression" | "e" => KIND_EXPRESSION,
            "toggle" | "t" => KIND_TOGGLE,
            "fsm" | "f" => KIND_FSM,
            "all" => KIND_ALL,
            _ => {
                return Err(format!(
                    "unknown code coverage kind '{}' (want stmt, branch, cond, expr, toggle, fsm or all)",
                    k
                ));
            }
        };
    }
    if kinds == 0 {
        return Err("--code-coverage needs at least one kind".to_string());
    }
    Ok(kinds)
}

/// The counter a coverage marker's first argument carries.
pub(crate) fn marker_id(e: &Expression) -> Option<u32> {
    match &e.kind {
        ExprKind::Number(NumberLiteral::Integer { value, .. }) => value.parse().ok(),
        _ => None,
    }
}

fn number(n: u32, span: Span) -> Expression {
    Expression::new(
        ExprKind::Number(NumberLiteral::Integer {
            size: None,
            signed: false,
            base: NumberBase::Decimal,
            value: n.to_string(),
            cached_val: Cell::new(None),
        }),
        span,
    )
}

fn hit(counter: u32, span: Span) -> Statement {
    Statement::new(
        StatementKind::Expr(Expression::new(
            ExprKind::SystemCall {
                name: HIT_TASK.to_string(),
                args: vec![number(counter, span)],
            },
            span,
        )),
        span,
    )
}

/// One statement list as a single statement.
fn block(mut v: Vec<Statement>, span: Span) -> Statement {
    if v.len() == 1 {
        v.pop().unwrap()
    } else {
        Statement::new(
            StatementKind::SeqBlock {
                name: None,
                stmts: v,
            },
            span,
        )
    }
}

fn unparen(mut e: &Expression) -> &Expression {
    while let ExprKind::Paren(inner) = &e.kind {
        e = inner;
    }
    e
}

fn unparen_mut(mut e: &mut Expression) -> &mut Expression {
    while matches!(e.kind, ExprKind::Paren(_)) {
        let ExprKind::Paren(inner) = &mut e.kind else {
            unreachable!()
        };
        e = inner;
    }
    e
}

/// One operator of a condition or expression item's logic tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    /// Input term `n`. With `inv` set the node is `t == 0` (or `t != 1`) on
    /// a 1-bit `t`, and the term is `t`: its value is the node's, negated.
    Term(u16, bool),
    /// A constant operand: no term of its own.
    Const,
    /// `!` or `~` (on a 1-bit operand).
    Not,
    And,
    Or,
    /// `&&` and `||` under a negation (`!(a && b)`, `~(a || b)`): the
    /// reference simulator takes them for `~&` and `~|`, so each operand
    /// decides as for `&` and `|` (both are evaluated when they read
    /// without side effects).
    AndFull,
    OrFull,
    /// `&`, `|`, `^` and `~^` on 1-bit operands.
    BitAnd,
    BitOr,
    BitXor,
    BitXnor,
    /// `c ? t : e` on 1-bit arms, in an expression item.
    Mux,
}

/// A node of an item's logic tree, in pre-order; `kids` index the tree.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    op: Op,
    kids: [u32; 3],
}

/// A node value in an item's evaluation: 0, 1, x (or z), or not evaluated.
const X: u8 = 2;
const UNEVALUATED: u8 = 3;

/// A condition or expression item.
pub(crate) struct ExprSite {
    scope: u32,
    span: Span,
    /// `if`, `while`, `do` or `ternary` for a condition; `assign` or
    /// `statement` for an expression.
    kind: &'static str,
    condition: bool,
    /// The generate block within the scope (`gen[1]`), to keep the items
    /// of a generate loop's iterations apart.
    block: String,
    text: String,
    terms: Vec<String>,
    /// Counter of term `t`'s row `v` (`v` = 0 or 1): `row0 + 2 * t + v`;
    /// in a `bimodal` item, with the item's value `o`, `row0 + 4 * t + 2 *
    /// v + o`.
    row0: u32,
    /// The item has a `^`, `~^` or `?:`: the reference simulator counts its
    /// rows by the value the item takes, and a term is covered when its two
    /// rows were hit with the item at different values (with those a term
    /// can decide the same value at both of its values).
    bimodal: bool,
    nodes: Arc<[Node]>,
    /// A continuous assignment's: the scope its names resolve in (`None`
    /// inside for the top; see `Simulator::cov_expr`). `None` in a process,
    /// which has its own.
    hint: Option<Option<String>>,
    /// Tells apart the copies a generate loop makes of one continuous
    /// assignment (its expression with the loop index in place).
    copy: String,
    /// An item at the top level of a combinational `always` block (it runs
    /// each time the block does): the block's scope and source offset.
    proc: Option<(String, usize)>,
    /// A continuous assignment's, or a `proc` item's: the term values of
    /// its last evaluation (empty before the first), and the earliest and
    /// latest step each term can settle at (`None` until first needed,
    /// empty when the item has nothing to replay); see
    /// `Simulator::cov_replay`.
    last: Vec<u8>,
    steps: Option<Vec<(u32, u32)>>,
    /// In code a constant condition rules out (`cov_resolve_constants`): not
    /// reported, like that code's statements and branches.
    excluded: bool,
}

/// An item's logic tree while it is built.
#[derive(Default)]
struct Plan {
    nodes: Vec<Node>,
    terms: Vec<String>,
    /// Per node: a relational term (`a == b`, `x < 3`).
    relational: Vec<bool>,
    /// Per node: constant, with its value when it is a plain number.
    konst: Vec<Option<Option<u64>>>,
    /// A term is a system function call.
    system: bool,
}

/// Where instrumented code is, for its names: the block scope
/// (`u0.gen[1]`), and the widths of the locals and arguments declared so
/// far (see `Ctx::local`).
#[derive(Default)]
struct Here {
    path: String,
    locals: HashMap<String, (u32, bool)>,
}

/// What instrumenting needs to read while the design's blocks are taken
/// apart: signal and function widths (to tell 1-bit operands) and the
/// source text (to name items and terms).
pub(crate) struct Ctx<'a> {
    module: &'a ElaboratedModule,
    names: &'a NameMap,
    widths: &'a [u32],
    /// Function name -> the width of its return type.
    rets: Arc<HashMap<String, u32>>,
}

impl Ctx<'_> {
    /// The names a bare identifier may have from block scope `path`: as
    /// written, then qualified by each enclosing scope, then by the top.
    fn candidates(&self, name: &str, path: &str) -> Vec<String> {
        let mut out = vec![name.to_string()];
        let mut p = path;
        while !p.is_empty() {
            out.push(format!("{}.{}", p, name));
            p = p.rsplit_once('.').map(|(a, _)| a).unwrap_or("");
        }
        out.push(format!("{}.{}", self.module.name, name));
        out
    }

    fn ident_name(h: &crate::ast::expr::HierarchicalIdentifier) -> Option<String> {
        if h.path.iter().any(|s| !s.selects.is_empty()) {
            return None;
        }
        Some(
            h.path
                .iter()
                .map(|s| s.name.name.as_str())
                .collect::<Vec<_>>()
                .join("."),
        )
    }

    fn signal_width(&self, name: &str, path: &str) -> Option<u32> {
        self.candidates(name, path)
            .iter()
            .find_map(|n| self.names.get(n))
            .and_then(|&id| self.widths.get(id).copied())
    }

    fn parameter(&self, name: &str, path: &str) -> Option<&Value> {
        self.candidates(name, path)
            .iter()
            .find_map(|n| self.module.parameters.get(n.as_str()))
    }

    /// A constant operand's value (`None` inside when it is not a plain
    /// number): a literal, a parameter, or an operator on constants.
    fn constant(&self, e: &Expression, at: &Here) -> Option<Option<u64>> {
        let e = unparen(e);
        match &e.kind {
            ExprKind::Number(_) => Some(Simulator::try_const_u64(e)),
            ExprKind::Ident(h) => {
                let n = Self::ident_name(h)?;
                if at.locals.contains_key(&n) {
                    return None;
                }
                self.parameter(&n, &at.path).map(|v| v.to_u64())
            }
            ExprKind::Unary { operand, .. } => self.constant(operand, at).map(|_| None),
            ExprKind::Binary { left, right, .. } => {
                self.constant(left, at)?;
                self.constant(right, at).map(|_| None)
            }
            // A constant bit of a concatenation: `{1'b0, a}[1]`, where a
            // port actual stands in for the port.
            ExprKind::Index { expr, index } => {
                let i = Simulator::try_const_u64(index)?;
                match &unparen(expr).kind {
                    ExprKind::Concatenation(parts) => {
                        let mut lo = 0u64;
                        for p in parts.iter().rev() {
                            let w = self.width(p, at)? as u64;
                            if i < lo + w {
                                let v = self.constant(p, at)?;
                                let s = (i - lo) as u32;
                                return Some(v.map(|v| v.checked_shr(s).unwrap_or(0) & 1));
                            }
                            lo += w;
                        }
                        None
                    }
                    _ => self.constant(expr, at).map(|_| None),
                }
            }
            _ => None,
        }
    }

    /// The self-determined width of `e` where it can be told without
    /// evaluating it; `None` otherwise.
    fn width(&self, e: &Expression, at: &Here) -> Option<u32> {
        let path = at.path.as_str();
        let e = unparen(e);
        match &e.kind {
            ExprKind::Number(NumberLiteral::Integer { size, .. }) => Some(size.unwrap_or(32)),
            ExprKind::Number(NumberLiteral::UnbasedUnsized(_)) => Some(1),
            ExprKind::Ident(h) => {
                let n = Self::ident_name(h)?;
                if let Some(&(w, _)) = at.locals.get(&n) {
                    return (w > 0).then_some(w);
                }
                if let Some(v) = self.parameter(&n, path) {
                    return Some(v.width);
                }
                self.signal_width(&n, path)
            }
            ExprKind::Index { expr, .. } => match &unparen(expr).kind {
                // A bit of a concatenation (a port actual put in place).
                ExprKind::Concatenation(_) => Some(1),
                ExprKind::Ident(h) => {
                    let n = Self::ident_name(h)?;
                    if let Some(&(_, bit)) = at.locals.get(&n) {
                        return bit.then_some(1);
                    }
                    let m = self.module;
                    for c in self.candidates(&n, path) {
                        let c = c.as_str();
                        if let Some(&(_, _, w)) = m.arrays.get(c) {
                            return Some(w);
                        }
                        if m.arrays_nd.contains_key(c)
                            || m.dynamic_arrays.contains(c)
                            || m.associative_arrays.contains_key(c)
                            || m.queue_vars.contains(c)
                            || m.string_signals.contains(c)
                        {
                            return None;
                        }
                        // An element of a multi-dimensional packed vector.
                        if let Some(&w) = m.packed_signal_elem_widths.get(c) {
                            return Some(w);
                        }
                        if self.names.get(c).is_some() {
                            return Some(1);
                        }
                    }
                    None
                }
                _ => None,
            },
            ExprKind::Call { func, .. } => match &func.kind {
                ExprKind::Ident(h) if h.path.len() == 1 => {
                    self.rets.get(h.path[0].name.name.as_str()).copied()
                }
                _ => None,
            },
            ExprKind::RangeSelect {
                kind, left, right, ..
            } => {
                let (l, r) = (
                    Simulator::try_const_u64(left),
                    Simulator::try_const_u64(right),
                );
                match kind {
                    crate::ast::expr::RangeKind::Constant => Some(l?.abs_diff(r?) as u32 + 1),
                    _ => r.map(|w| w as u32),
                }
            }
            ExprKind::Unary { op, operand } => match op {
                UnaryOp::BitNot | UnaryOp::Plus | UnaryOp::Minus => self.width(operand, at),
                UnaryOp::LogNot
                | UnaryOp::BitAnd
                | UnaryOp::BitNand
                | UnaryOp::BitOr
                | UnaryOp::BitNor
                | UnaryOp::BitXor
                | UnaryOp::BitXnor => Some(1),
                _ => None,
            },
            ExprKind::Binary { op, left, right } => match op {
                BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::BitXnor => {
                    Some(self.width(left, at)?.max(self.width(right, at)?))
                }
                _ if Self::relational(*op) || matches!(op, BinaryOp::LogAnd | BinaryOp::LogOr) => {
                    Some(1)
                }
                _ => None,
            },
            ExprKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => Some(self.width(then_expr, at)?.max(self.width(else_expr, at)?)),
            ExprKind::Concatenation(parts) => parts.iter().map(|p| self.width(p, at)).sum(),
            _ => None,
        }
    }

    /// A local variable's (or subroutine argument's) width, and whether a
    /// bit-select of it is one bit; `(0, false)` when neither is known.
    fn local(&self, dt: &DataType, unpacked: bool) -> (u32, bool) {
        let bits = match dt {
            DataType::IntegerVector { dimensions, .. } | DataType::Implicit { dimensions, .. } => {
                dimensions.len() <= 1
            }
            DataType::IntegerAtom { .. } => true,
            _ => return (0, false),
        };
        if unpacked {
            return (0, false);
        }
        let w = crate::compiler::elaborate::resolve_type_width(
            dt,
            Some(&self.module.parameters),
            Some(&self.module.typedefs),
        );
        (w, bits && w > 0)
    }

    /// A `foreach` index variable's width and bit-select (as `local`): `int`
    /// for an array of the design, an associative array's key type when it
    /// is integral; `(0, false)` for any other collection.
    fn index_type(&self, array: &Expression, at: &Here) -> (u32, bool) {
        let ExprKind::Ident(h) = &unparen(array).kind else {
            return (0, false);
        };
        let Some(n) = Self::ident_name(h) else {
            return (0, false);
        };
        if at.locals.contains_key(&n) {
            return (0, false);
        }
        let m = self.module;
        for c in self.candidates(&n, &at.path) {
            let c = c.as_str();
            if m.associative_arrays.contains_key(c) {
                return match m.assoc_index_widths.get(c) {
                    Some(&(w, _)) if w > 0 => (w, true),
                    _ => (0, false),
                };
            }
        }
        (32, true)
    }

    fn relational(op: BinaryOp) -> bool {
        matches!(
            op,
            BinaryOp::Eq
                | BinaryOp::Neq
                | BinaryOp::CaseEq
                | BinaryOp::CaseNeq
                | BinaryOp::WildcardEq
                | BinaryOp::WildcardNeq
                | BinaryOp::Lt
                | BinaryOp::Leq
                | BinaryOp::Gt
                | BinaryOp::Geq
        )
    }

    /// The source text of `e`, comments dropped and white space squeezed,
    /// without enclosing parentheses; a rendering of the tree when the
    /// source is not at hand.
    fn text(&self, e: &Expression, src: Option<u32>) -> String {
        match self.source(e, src) {
            Some(s) => strip_parens(&s),
            None => strip_parens(&show(e)),
        }
    }

    /// The source text of `e` in text `src` (or the only text long enough),
    /// comments dropped and white space squeezed.
    fn source(&self, e: &Expression, src: Option<u32>) -> Option<String> {
        let span = e.span;
        let texts = &self.module.source_texts;
        let fits = |t: &String| span.start < span.end && span.end <= t.len();
        let text = match src
            .map(|s| s as usize)
            .filter(|&s| texts.get(s).is_some_and(fits))
        {
            Some(i) => texts.get(i),
            None => {
                let mut all = texts.iter().filter(|t| fits(t));
                match (all.next(), all.next()) {
                    (Some(t), None) => Some(t),
                    _ => None,
                }
            }
        };
        text.and_then(|t| t.get(span.start..span.end))
            .map(squeeze)
            .filter(|s| !s.is_empty())
    }
}

/// Byte offsets in `s` of the binary operator `tok` outside brackets and
/// strings: after an operand, and not part of a longer operator (`&` of
/// `&&` or `~&`, `^` of `~^`).
fn binary_ops(s: &str, tok: &str) -> Vec<usize> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ if depth == 0 && s.is_char_boundary(i) && s[i..].starts_with(tok) => {
                let prev = s[..i].trim_end().bytes().last();
                let before = i.checked_sub(1).map(|j| b[j]);
                let after = b.get(i + tok.len()).copied();
                let operand = prev.is_some_and(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(c, b'_' | b')' | b']' | b'}' | b'\'' | b'"')
                });
                let first = tok.as_bytes()[0];
                let last = tok.as_bytes()[tok.len() - 1];
                let longer = match tok {
                    "&&" | "||" => after == Some(first) || before == Some(first),
                    "&" | "|" => after == Some(last) || matches!(before, Some(b'~')),
                    "^" => matches!(after, Some(b'~')),
                    "==" | "!=" => after == Some(b'='),
                    _ => false,
                };
                if operand && !longer {
                    out.push(i);
                    i += tok.len();
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// `s`, a left-associative binary operation on `tok` (`~^` also spelled
/// `^~`), split at its operator: the last one outside brackets.
fn split_binary(s: &str, tok: &str) -> Option<(String, String)> {
    let mut at: Vec<(usize, usize)> = binary_ops(s, tok)
        .into_iter()
        .map(|p| (p, tok.len()))
        .collect();
    if tok == "~^" {
        at.extend(binary_ops(s, "^~").into_iter().map(|p| (p, 2)));
    }
    let &(p, n) = at.iter().max()?;
    let (l, r) = (s[..p].trim(), s[p + n..].trim());
    (!l.is_empty() && !r.is_empty()).then(|| (l.to_string(), r.to_string()))
}

/// `s`, a `c ? a : b`, split into its condition and arms.
fn split_mux(s: &str) -> Option<(String, String, String)> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut q = None;
    let mut nest = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'?' if depth == 0 => {
                if q.is_none() {
                    q = Some(i);
                } else {
                    nest += 1;
                }
            }
            b':' if depth == 0 && b.get(i + 1) == Some(&b':') => i += 1,
            b':' if depth == 0 && q.is_some() => {
                if nest == 0 {
                    let q = q?;
                    let parts = (s[..q].trim(), s[q + 1..i].trim(), s[i + 1..].trim());
                    if parts.0.is_empty() || parts.1.is_empty() || parts.2.is_empty() {
                        return None;
                    }
                    return Some((parts.0.into(), parts.1.into(), parts.2.into()));
                }
                nest -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `s`, a unary `op` (`!` or `~`) applied to an operand: the operand.
fn unary_operand(s: &str, op: char) -> Option<String> {
    let rest = s.strip_prefix(op)?.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

/// `s` without comments, its white space runs one space.
fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    let mut space = false;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            space = true;
            continue;
        }
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i += 2;
            space = true;
            continue;
        }
        if b[i].is_ascii_whitespace() {
            space = true;
            i += 1;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        let n = s[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        out.push_str(&s[i..i + n]);
        i += n;
    }
    out
}

/// `s` without parentheses that enclose all of it.
fn strip_parens(s: &str) -> String {
    let mut s = s.trim();
    while s.starts_with('(') && s.ends_with(')') {
        let inner = &s[1..s.len() - 1];
        let mut depth = 0i32;
        let balanced = inner.chars().all(|c| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            depth >= 0
        }) && depth == 0;
        if !balanced {
            break;
        }
        s = inner.trim();
    }
    s.to_string()
}

/// The number literals of `e`, in order: what tells a generate loop's
/// copies of one expression apart (the loop index is put in as a number).
fn numbers(e: &Expression, out: &mut String) {
    match &e.kind {
        ExprKind::Number(NumberLiteral::Integer { value, .. }) => {
            out.push_str(value);
            out.push(',');
        }
        ExprKind::Paren(x) | ExprKind::Unary { operand: x, .. } => numbers(x, out),
        ExprKind::Binary { left, right, .. } => {
            numbers(left, out);
            numbers(right, out);
        }
        ExprKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            numbers(condition, out);
            numbers(then_expr, out);
            numbers(else_expr, out);
        }
        ExprKind::Index { expr, index } => {
            numbers(expr, out);
            numbers(index, out);
        }
        ExprKind::RangeSelect {
            expr, left, right, ..
        } => {
            numbers(expr, out);
            numbers(left, out);
            numbers(right, out);
        }
        ExprKind::Concatenation(parts) => parts.iter().for_each(|p| numbers(p, out)),
        ExprKind::Call { args, .. } => args.iter().for_each(|a| numbers(a, out)),
        // A coverage marker's first argument is its counter.
        ExprKind::SystemCall { name, args } => {
            let skip = usize::from(name == COND_FN || name == EXPR_FN);
            args.iter().skip(skip).for_each(|a| numbers(a, out));
        }
        _ => {}
    }
}

/// A rendering of `e` for an item or term without source text.
fn show(e: &Expression) -> String {
    let bin = |op: BinaryOp| match op {
        BinaryOp::LogAnd => "&&",
        BinaryOp::LogOr => "||",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::BitXnor => "~^",
        BinaryOp::Eq => "==",
        BinaryOp::Neq => "!=",
        BinaryOp::CaseEq => "===",
        BinaryOp::CaseNeq => "!==",
        BinaryOp::Lt => "<",
        BinaryOp::Leq => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Geq => ">=",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        _ => "?",
    };
    match &e.kind {
        ExprKind::Paren(inner) => format!("({})", show(inner)),
        ExprKind::Ident(h) => h
            .path
            .iter()
            .map(|s| s.name.name.as_str())
            .collect::<Vec<_>>()
            .join("."),
        ExprKind::Number(NumberLiteral::Integer { value, .. }) => value.clone(),
        ExprKind::Unary { op, operand } => {
            let o = match op {
                UnaryOp::LogNot => "!",
                UnaryOp::BitNot => "~",
                UnaryOp::BitAnd => "&",
                UnaryOp::BitOr => "|",
                UnaryOp::BitXor => "^",
                _ => "?",
            };
            format!("{}{}", o, show(operand))
        }
        ExprKind::Binary { op, left, right } => {
            format!("{} {} {}", show(left), bin(*op), show(right))
        }
        ExprKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } => format!(
            "{} ? {} : {}",
            show(condition),
            show(then_expr),
            show(else_expr)
        ),
        ExprKind::Index { expr, index } => format!("{}[{}]", show(expr), show(index)),
        ExprKind::SystemCall { name, args } if (name == COND_FN || name == EXPR_FN) => {
            args.get(1).map(show).unwrap_or_default()
        }
        ExprKind::SystemCall { name, args } => format!(
            "{}({})",
            name,
            args.iter().map(show).collect::<Vec<_>>().join(", ")
        ),
        ExprKind::Call { func, args } => format!(
            "{}({})",
            show(func),
            args.iter().map(show).collect::<Vec<_>>().join(", ")
        ),
        _ => "?".to_string(),
    }
}

/// Mark in `rows` (bit `v` of `rows[t]`: term `t` at `v`) the terms of node
/// `i`'s subtree that decide its value, given the node values of one
/// evaluation. Short-circuit aware: the left operand of `&&` decides alone
/// when it is 0 (the right one is not evaluated), and so does the left of
/// `||` at 1 and a `?:` condition whose other arm was not evaluated.
fn sensitize(nodes: &[Node], i: u32, vals: &[u8], rows: &mut [u8]) {
    let n = nodes[i as usize];
    let [k0, k1, k2] = n.kids;
    let (l, r) = (vals[k0 as usize], vals[k1 as usize]);
    let mut go = |k: u32, on: bool| {
        if on {
            sensitize(nodes, k, vals, rows);
        }
    };
    match n.op {
        Op::Term(t, inv) => {
            let v = vals[i as usize];
            if v <= 1 {
                rows[t as usize] |= 1 << (v ^ inv as u8);
            }
        }
        Op::Const => {}
        Op::Not => go(k0, true),
        Op::And => {
            go(k0, l == 0 || (l == 1 && r == 1));
            go(k1, l == 1);
        }
        Op::Or => {
            go(k0, l == 1 || (l == 0 && r == 0));
            go(k1, l == 0);
        }
        Op::BitAnd | Op::AndFull => {
            go(k0, r == 1);
            go(k1, l == 1);
        }
        Op::BitOr | Op::OrFull => {
            go(k0, r == 0);
            go(k1, l == 0);
        }
        Op::BitXor | Op::BitXnor => {
            go(k0, r <= 1);
            go(k1, l <= 1);
        }
        Op::Mux => {
            let (t, e) = (vals[k1 as usize], vals[k2 as usize]);
            let (taken, other) = match l {
                1 => (t, e),
                0 => (e, t),
                _ => (X, X),
            };
            go(k0, taken <= 1 && (other == UNEVALUATED || other != taken));
            go(k1, l == 1);
            go(k2, l == 0);
        }
    }
}

/// Mark in `rows` every term an evaluation of a logical-mode item read, at
/// the value it had: sum-of-products scoring, where a term is covered once
/// it has been 0 and 1, whether or not it decided the item's value. A term
/// short-circuiting skipped is not read.
fn sop(nodes: &[Node], vals: &[u8], rows: &mut [u8]) {
    for (i, n) in nodes.iter().enumerate() {
        if let Op::Term(t, inv) = n.op {
            if vals[i] <= 1 {
                rows[t as usize] |= 1 << (vals[i] ^ inv as u8);
            }
        }
    }
}

/// The leaves of node `i`'s subtree (`e` is its expression): each term and
/// constant node, with its expression.
fn leaves_of<'e>(nodes: &[Node], i: u32, e: &'e Expression, out: &mut Vec<(u32, &'e Expression)>) {
    let e = unparen(e);
    let n = nodes[i as usize];
    let [k0, k1, k2] = n.kids;
    match (n.op, &e.kind) {
        (Op::Term(..) | Op::Const, _) => out.push((i, e)),
        (Op::Not, ExprKind::Unary { operand, .. }) => leaves_of(nodes, k0, operand, out),
        (
            Op::Mux,
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            },
        ) => {
            let c = match &unparen(condition).kind {
                ExprKind::SystemCall { name, args } if name == COND_FN && args.len() == 2 => &args[1],
                _ => &**condition,
            };
            leaves_of(nodes, k0, c, out);
            leaves_of(nodes, k1, then_expr, out);
            leaves_of(nodes, k2, else_expr, out);
        }
        (_, ExprKind::Binary { left, right, .. }) => {
            leaves_of(nodes, k0, left, out);
            leaves_of(nodes, k1, right, out);
        }
        _ => {}
    }
}

/// Evaluate node `i` of an item's tree from the values of its terms
/// (`terms[t]`) and constants (`konst`, by node) with the operators and
/// short-circuits of `Simulator::cov_node`, noting each node's value in
/// `vals`.
fn replay(nodes: &[Node], i: u32, terms: &[u8], konst: &[u8], vals: &mut [u8]) -> u8 {
    let n = nodes[i as usize];
    let [k0, k1, k2] = n.kids;
    let v = match n.op {
        Op::Term(t, inv) => match terms[t as usize] {
            v @ 0..=1 => v ^ inv as u8,
            _ => X,
        },
        Op::Const => konst[i as usize],
        Op::Not => match replay(nodes, k0, terms, konst, vals) {
            v @ 0..=1 => v ^ 1,
            _ => X,
        },
        Op::And | Op::Or => {
            // The value that decides alone: 0 for `&&`, 1 for `||`.
            let d = (n.op == Op::Or) as u8;
            let l = replay(nodes, k0, terms, konst, vals);
            if l == d {
                d
            } else {
                let r = replay(nodes, k1, terms, konst, vals);
                if r == d {
                    d
                } else if l == d ^ 1 && r == d ^ 1 {
                    d ^ 1
                } else {
                    X
                }
            }
        }
        Op::BitAnd | Op::BitOr | Op::AndFull | Op::OrFull => {
            let d = matches!(n.op, Op::BitOr | Op::OrFull) as u8;
            let l = replay(nodes, k0, terms, konst, vals);
            let r = replay(nodes, k1, terms, konst, vals);
            if l == d || r == d {
                d
            } else if l == d ^ 1 && r == d ^ 1 {
                d ^ 1
            } else {
                X
            }
        }
        Op::BitXor | Op::BitXnor => {
            let l = replay(nodes, k0, terms, konst, vals);
            let r = replay(nodes, k1, terms, konst, vals);
            if l > 1 || r > 1 {
                X
            } else {
                l ^ r ^ (n.op == Op::BitXnor) as u8
            }
        }
        Op::Mux => match replay(nodes, k0, terms, konst, vals) {
            c @ 0..=1 => {
                let (taken, other) = if c == 1 { (k1, k2) } else { (k2, k1) };
                let v = replay(nodes, taken, terms, konst, vals);
                if nodes[other as usize].op == Op::Const {
                    replay(nodes, other, terms, konst, vals);
                }
                v
            }
            _ => {
                let t = replay(nodes, k1, terms, konst, vals);
                let f = replay(nodes, k2, terms, konst, vals);
                if t == f { t } else { X }
            }
        },
    };
    vals[i as usize] = v;
    v
}

/// Group terms by when they settle, given the earliest and latest step
/// each can (`steps`), so that the groups settle one after another
/// whichever signals upstream changed: a term whose earliest step is not
/// after the latest of the group before it joins that group (their order
/// is not certain, so no state between them is counted). Each listed term
/// with its group, in step order.
fn order(steps: &[(u32, u32)], mut terms: Vec<usize>) -> Vec<(usize, usize)> {
    terms.sort_by_key(|&t| steps[t].0);
    let (mut group, mut end) = (0, None);
    terms
        .into_iter()
        .map(|t| {
            let (lo, hi) = steps[t];
            end = Some(match end {
                Some(e) if lo > e => {
                    group += 1;
                    hi
                }
                Some(e) => hi.max(e),
                None => hi,
            });
            (t, group)
        })
        .collect()
}

/// Reading `e` has no side effects (no function call), so coverage may read
/// it again (quietly: the coverage markers in it count nothing then).
fn pure(e: &Expression) -> bool {
    match &e.kind {
        ExprKind::Number(_) | ExprKind::TypeLiteral(_) => true,
        ExprKind::Ident(h) => h
            .path
            .iter()
            .all(|s| s.selects.iter().all(pure)),
        ExprKind::Unary { op, operand } => {
            !matches!(
                op,
                UnaryOp::PreIncr | UnaryOp::PreDecr | UnaryOp::PostIncr | UnaryOp::PostDecr
            ) && pure(operand)
        }
        ExprKind::Binary { left, right, .. } => pure(left) && pure(right),
        ExprKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } => pure(condition) && pure(then_expr) && pure(else_expr),
        ExprKind::Concatenation(v) => v.iter().all(pure),
        ExprKind::Replication { count, exprs } => pure(count) && exprs.iter().all(pure),
        ExprKind::Inside { expr, ranges } => pure(expr) && ranges.iter().all(pure),
        ExprKind::Range(a, b) => pure(a) && pure(b),
        ExprKind::MemberAccess { expr, .. } | ExprKind::Paren(expr) => pure(expr),
        ExprKind::Index { expr, index } => pure(expr) && pure(index),
        ExprKind::RangeSelect {
            expr, left, right, ..
        } => pure(expr) && pure(left) && pure(right),
        ExprKind::SystemCall { name, args } => {
            matches!(
                name.as_str(),
                COND_FN
                    | EXPR_FN
                    | "$signed"
                    | "$unsigned"
                    | "$__xz_size_cast"
                    | "$__xz_type_cast"
                    | "$__xz_named_cast"
                    | "$bits"
                    | "$countones"
                    | "$onehot"
                    | "$onehot0"
                    | "$isunknown"
            ) && args.iter().all(pure)
        }
        _ => false,
    }
}

/// A reported scope: a module instance, or the package (or `$unit`, or the
/// declaring module) that holds classes and subroutines outside instances.
struct Scope {
    /// Printed name: the hierarchical instance path, or the package name.
    name: String,
    unit: String,
    kind: &'static str,
    src: Option<u32>,
}

struct StmtSite {
    scope: u32,
    span: Span,
    kind: &'static str,
    counter: u32,
    /// In an arm a constant condition rules out: left out of the totals.
    excluded: bool,
}

struct Arm {
    span: Span,
    label: &'static str,
    /// `None` for the implicit `else` / `default`: the branch's entry count
    /// minus the other arms.
    counter: Option<u32>,
    /// A constant condition rules this arm out.
    excluded: bool,
}

struct BranchSite {
    scope: u32,
    span: Span,
    kind: &'static str,
    entry: Option<u32>,
    arms: Vec<Arm>,
    /// The decision is a constant, or the branch is in an excluded arm: all
    /// of its arms are left out of the totals.
    excluded: bool,
}

/// What a constant condition leaves of one arm of an `if` chain or a `case`.
#[derive(Clone, Copy, PartialEq)]
enum Fate {
    /// Not decided by a constant: the arm counts.
    Live,
    /// The constant condition selects this arm: its statements count, and
    /// the arm counts unless the whole decision is constant.
    Taken,
    /// The arm can never run: it and its statements are left out.
    Dead,
}

/// The fates of an `if` chain's arms, one per condition and one for the
/// final `else` (written or implicit), from the conditions' constant truths,
/// and whether every condition is a constant (then the chain is no decision
/// at all). The arm of the first constantly true condition takes everything
/// that reaches it; arms whose condition is constantly false, and every arm
/// after a constantly true one, never run.
fn chain_fates(truths: &[Option<bool>]) -> (Vec<Fate>, bool) {
    let mut fates = Vec::with_capacity(truths.len() + 1);
    let mut whole = true;
    let mut open = true;
    for t in truths {
        fates.push(match (open, t) {
            (false, _) => Fate::Dead,
            (true, Some(true)) => {
                open = false;
                Fate::Taken
            }
            (true, Some(false)) => Fate::Dead,
            (true, None) => {
                whole = false;
                Fate::Live
            }
        });
    }
    fates.push(match (open, whole) {
        (false, _) => Fate::Dead,
        (true, true) => Fate::Taken,
        (true, false) => Fate::Live,
    });
    (fates, whole)
}

/// A condition reduced to the parts a constant can decide (see
/// `Instr::skeleton`).
enum Cond {
    /// An elaboration-time constant expression (IEEE 1800 §11.2.1),
    /// evaluated once the design is compiled.
    Const(Expression),
    Not(Box<Cond>),
    /// `&&` (or `&` when `bitwise`): false when either side is.
    And(Box<Cond>, Box<Cond>, bool),
    /// `||` (or `|` when `bitwise`): true when either side is.
    Or(Box<Cond>, Box<Cond>, bool),
    Unknown,
}

/// The statement sites and branch sites one arm or condition holds, as
/// ranges of `CodeCov::stmts` and `CodeCov::branches`.
/// The statement, branch and condition/expression item sites some code made.
type Region = (
    std::ops::Range<usize>,
    std::ops::Range<usize>,
    std::ops::Range<usize>,
);

/// What a decision is made of.
enum Choice {
    /// An `if` chain's conditions.
    If(Vec<Cond>),
    /// A `case` whose selector and items are constants (items keep their
    /// patterns only).
    Case(CaseKind, Expression, Vec<CaseItem>),
    /// A `?:` condition.
    Ternary(Cond),
    /// The left operand of an `&&` (`||` when `true`) with a `?:` in its
    /// right operand, which never runs when the left one is constantly
    /// false (true).
    Operand(Cond, bool),
}

/// A decision a constant may make, recorded while the design is
/// instrumented and made once it is compiled (`cov_resolve_constants`), by
/// the simulator's own evaluator. Instrumentation does not depend on it.
struct Decision {
    choice: Choice,
    /// The branch site, if branch coverage is on.
    site: Option<usize>,
    /// What each arm holds, in arm order (the implicit `else` / `default`
    /// last, empty).
    arms: Vec<Region>,
    /// What each condition holds: its condition item and the `?:` branches
    /// inside it.
    conds: Vec<Region>,
}

/// One 64-bit word of a tracked signal, as toggle coverage last saw it.
#[derive(Clone, Copy)]
struct Word {
    /// Value at the last sample.
    v: u64,
    x: u64,
    /// Time of the last change, `u64::MAX` before the first.
    at: u64,
    /// Value at the end of the slot before that change: the slot's toggles
    /// count from it.
    bv: u64,
    bx: u64,
}

/// Per-bit rise/fall counts of the tracked signals.
pub(crate) struct Toggle {
    /// Signal id -> tracked index, `u32::MAX` when not tracked.
    slot: Vec<u32>,
    ids: Vec<u32>,
    width: Vec<u32>,
    /// First word of each tracked signal in `words`.
    word0: Vec<u32>,
    /// First bit of each tracked signal in `rise` / `fall`.
    bit0: Vec<u32>,
    words: Vec<Word>,
    rise: Vec<u32>,
    fall: Vec<u32>,
    /// (scope, name within the scope, tracked index, data type); aliases of
    /// one signal share the tracked index.
    names: Vec<(u32, String, u32, &'static str)>,
}

impl Toggle {
    /// Sample word `word` of tracked signal `k` at time `now`. A slot can be
    /// sampled more than once (every tick flushes, and so does the end of
    /// the slot); the counts hold the change from the end of the previous
    /// slot to the latest sample, so a value changed and changed back within
    /// one slot does not toggle.
    fn sample(&mut self, k: usize, v: u64, x: u64, word: usize, now: u64) {
        let w = self.word0[k] as usize + word;
        let s = &mut self.words[w];
        let (pv, px) = (s.v, s.x);
        if v == pv && x == px {
            return;
        }
        s.v = v;
        s.x = x;
        let base = self.bit0[k] as usize + word * 64;
        if s.at == now {
            self.resample(w, base, pv, px);
            return;
        }
        // The slot's first change: the previous sample is the value at the
        // end of the previous slot.
        s.at = now;
        s.bv = pv;
        s.bx = px;
        let known = !(x | px);
        let mut rise = v & !pv & known;
        let mut fall = pv & !v & known;
        while rise != 0 {
            let b = rise.trailing_zeros() as usize;
            self.rise[base + b] = self.rise[base + b].saturating_add(1);
            rise &= rise - 1;
        }
        while fall != 0 {
            let b = fall.trailing_zeros() as usize;
            self.fall[base + b] = self.fall[base + b].saturating_add(1);
            fall &= fall - 1;
        }
    }

    /// Word `w` changed again in the slot it last changed in: move its
    /// counts from the transition counted so far (baseline to `pv`/`px`)
    /// to the one from the baseline to its new value.
    #[cold]
    fn resample(&mut self, w: usize, base: usize, pv: u64, px: u64) {
        let s = self.words[w];
        let (v, x, bv, bx) = (s.v, s.x, s.bv, s.bx);
        let was = !(bx | px);
        let known = !(bx | x);
        Self::recount(&mut self.rise, base, pv & !bv & was, v & !bv & known);
        Self::recount(&mut self.fall, base, bv & !pv & was, bv & !v & known);
    }

    /// Move the counts of bits `bit..bit + 64` from the transitions in `old`
    /// to those in `new`.
    fn recount(n: &mut [u32], bit: usize, old: u64, new: u64) {
        let mut add = new & !old;
        while add != 0 {
            let b = bit + add.trailing_zeros() as usize;
            n[b] = n[b].saturating_add(1);
            add &= add - 1;
        }
        let mut sub = old & !new;
        while sub != 0 {
            let b = bit + sub.trailing_zeros() as usize;
            n[b] = n[b].saturating_sub(1);
            sub &= sub - 1;
        }
    }
}

pub(crate) struct CodeCov {
    kinds: u8,
    /// `--code-coverage-constant=keep`: nothing is excluded.
    keep_constant: bool,
    /// `--code-coverage-expressions=logical` (see `CodeCoverage`).
    logical: bool,
    /// `--code-coverage-granularity=block` (see `CodeCoverage`).
    blocks: bool,
    limit: Vec<String>,
    scopes: Vec<Scope>,
    scope_index: HashMap<String, u32>,
    stmts: Vec<StmtSite>,
    branches: Vec<BranchSite>,
    /// Condition and expression items; `$__xz_cove(<n>, ..)` names `exprs[n]`.
    exprs: Vec<ExprSite>,
    /// Function name -> the width of its return type (see `Ctx`).
    rets: Arc<HashMap<String, u32>>,
    counters: u32,
    /// Continuous assignment (origin span start, end, scope) -> its counter.
    ca_counter: HashMap<(usize, usize, String), u32>,
    /// Instance paths below the top.
    inst_paths: HashSet<String>,
    /// Decisions constants may make, until `cov_resolve_constants`.
    decisions: Vec<Decision>,
    pub(crate) toggle: Option<Toggle>,
    pub(crate) fsm: Option<FsmCov>,
    /// Signal id -> the earliest and latest step it settles at, and the
    /// scope and source offset of the combinational `always` block that
    /// writes it (see `Simulator::cov_steps`).
    steps: Vec<(u32, u32)>,
    writers: HashMap<usize, (String, usize)>,
    /// The names clocked `always` blocks write, by scope, and the
    /// continuous assignments that invert a signal (target, operand,
    /// scope), for `cov_steps`.
    clocked: Vec<(String, HashSet<String>)>,
    inverters: Vec<(String, String, String)>,
    /// The targets of `assign` statements (name, scope), for `cov_steps`.
    assigned: Vec<(String, String)>,
    /// Item evaluations not counted yet, by item (see `Simulator::cov_expr`).
    pending: HashMap<u32, Pending>,
    /// Inside a read that only coverage makes (`Simulator::cov_quietly`):
    /// count nothing.
    quiet: u32,
}

/// An item's evaluations not counted yet: the process whose write ran its
/// block and the settle that did, and each evaluation's node values and
/// term and constant values.
#[allow(clippy::type_complexity)]
struct Pending {
    pid: usize,
    run: u64,
    evals: Vec<(Vec<u8>, Option<(Vec<u8>, Vec<u8>)>)>,
}

/// What instrumenting an `if` chain collects: its arms, what each holds,
/// and the `?:` branches in each condition.
#[derive(Default)]
struct Chain {
    arms: Vec<Arm>,
    regions: Vec<Region>,
    conds: Vec<Region>,
}

/// The instance a block scope (`u0.u1.genblk2`) belongs to, its longest
/// prefix that names an instance (`""` for the top), and the rest of it.
fn split_scope<'s>(inst_paths: &HashSet<String>, scope: &'s str) -> (&'s str, &'s str) {
    let mut path = scope;
    loop {
        if path.is_empty() || inst_paths.contains(path) {
            let rest = scope[path.len()..].trim_start_matches('.');
            return (path, rest);
        }
        path = path.rsplit_once('.').map(|(p, _)| p).unwrap_or("");
    }
}

/// How an `always` block (`kind`, body `s`) runs: `Some(true)` when it is
/// combinational (`always_comb`, `always @*`, `always @(a or b)`),
/// `Some(false)` when clocked (`always_ff`, `always @(posedge clk)`), and
/// `None` otherwise (a delay loop).
fn combinational(kind: &str, s: &Statement) -> Option<bool> {
    match (kind, &s.kind) {
        ("always_comb" | "always_latch", _) => Some(true),
        ("always_ff", _) => Some(false),
        (
            _,
            StatementKind::TimingControl {
                control: TimingControl::Event(ev),
                ..
            },
        ) => match ev {
            EventControl::Star | EventControl::ParenStar => Some(true),
            EventControl::EventExpr(es) if es.iter().any(|e| e.edge.is_some()) => Some(false),
            EventControl::EventExpr(es) if es.iter().all(|e| e.iff.is_none()) => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// Rewrites one scope's statements; see the module docs.
struct Instr<'a> {
    cov: &'a mut CodeCov,
    ctx: &'a Ctx<'a>,
    kinds: u8,
    scope: u32,
    /// The design whose parameters make conditions constant, or `None` when
    /// nothing is excluded: `--code-coverage-constant=keep`, and scopes
    /// outside the instance tree, whose bare names are not resolved yet.
    consts: Option<&'a ElaboratedModule>,
    /// Names declared in the code being rewritten (variables, subroutine
    /// arguments): never constants, even where they shadow a parameter.
    locals: HashSet<String>,
    /// Where the code is, for its names.
    here: Here,
    /// The block scope within the scope's instance (`gen[1]`).
    block: String,
    /// The items' `ExprSite::hint`.
    hint: Option<Option<String>>,
    /// At the top level of a combinational `always` block, where each
    /// statement runs every time the block does: the block's source offset
    /// (for `ExprSite::proc`).
    top: Option<usize>,
    /// `--code-coverage-expressions=logical`: only `&&` and `||` split an
    /// item into terms, and only conditions are items.
    logical: bool,
    /// `--code-coverage-granularity=block`: `count` counts a straight-line
    /// block at its first statement; `open` while the statements being
    /// instrumented continue the block counted last.
    blocks: bool,
    open: bool,
}

impl<'a> Instr<'a> {
    /// `consts` is the design whose parameters make conditions constant
    /// (see `Instr::consts`); `--code-coverage-constant=keep` drops it.
    fn new(
        cov: &'a mut CodeCov,
        ctx: &'a Ctx<'a>,
        inst_paths: &HashSet<String>,
        scope: u32,
        path: &str,
        consts: Option<&'a ElaboratedModule>,
    ) -> Self {
        let kinds = cov.kinds;
        let (logical, blocks) = (cov.logical, cov.blocks);
        let block = split_scope(inst_paths, path).1.to_string();
        let consts = consts.filter(|_| !cov.keep_constant);
        Instr {
            cov,
            ctx,
            kinds,
            scope,
            consts,
            locals: HashSet::default(),
            here: Here {
                path: path.to_string(),
                locals: HashMap::default(),
            },
            block,
            hint: None,
            top: None,
            logical,
            blocks,
            open: false,
        }
    }

    /// A subroutine (named by `here.path`): its arguments are locals, and its
    /// generate block is that of the scope it is declared in.
    fn args(
        mut self,
        ports: &[crate::ast::decl::FunctionPort],
        inst_paths: &HashSet<String>,
    ) -> Self {
        for p in ports {
            let w = self.ctx.local(&p.data_type, !p.dimensions.is_empty());
            self.here.locals.insert(p.name.name.clone(), w);
            // Never constants, even where they shadow a parameter.
            self.locals.insert(p.name.name.clone());
            self.locals.insert(format!("{}.{}", self.here.path, p.name.name));
        }
        let scope = self.here.path.rsplit_once('.').map_or("", |(p, _)| p);
        self.block = split_scope(inst_paths, scope).1.to_string();
        self
    }
}

impl Instr<'_> {
    fn counter(&mut self) -> u32 {
        let c = self.cov.counters;
        self.cov.counters += 1;
        c
    }

    fn count(&mut self, span: Span, kind: &'static str, out: &mut Vec<Statement>) {
        if self.kinds & KIND_STATEMENT == 0 {
            return;
        }
        // `--code-coverage-granularity=block`: one counter per straight-line
        // block, at its first statement (see `boundary`).
        let kind = match self.blocks {
            true if self.open => return,
            true => {
                self.open = true;
                "block"
            }
            false => kind,
        };
        let counter = self.counter();
        self.cov.stmts.push(StmtSite {
            scope: self.scope,
            span,
            kind,
            counter,
            excluded: false,
        });
        out.push(hit(counter, span));
    }

    /// The end of a straight-line block: in block granularity the next
    /// statement starts a new one. Branches (`if`, `case` and their arms),
    /// loops and their bodies, `begin`/`end`, `fork` branches, event and
    /// delay controls and `wait` are boundaries, and so are a `return`,
    /// `break`, `continue` or `disable`.
    fn boundary(&mut self) {
        self.open = false;
    }

    /// Run `f`, and return the statement and branch sites it created.
    fn region<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (T, Region) {
        let (s0, b0, x0) = (self.cov.stmts.len(), self.cov.branches.len(), self.cov.exprs.len());
        let r = f(self);
        (
            r,
            (
                s0..self.cov.stmts.len(),
                b0..self.cov.branches.len(),
                x0..self.cov.exprs.len(),
            ),
        )
    }

    fn decide(
        &mut self,
        choice: Choice,
        site: Option<usize>,
        arms: Vec<Region>,
        conds: Vec<Region>,
    ) {
        self.cov.decisions.push(Decision {
            choice,
            site,
            arms,
            conds,
        });
    }

    /// Whether an identifier names a parameter (an enum constant or a
    /// genvar included, which elaboration records or substitutes as one).
    /// Elaboration has already resolved an instance's names to flat paths
    /// (`u0.WIDTH`); `pkg::P` (§26.3) is keyed by its package, and a
    /// package's enum constants have a table of their own.
    fn param(&self, h: &HierarchicalIdentifier) -> bool {
        let Some(module) = self.consts else {
            return false;
        };
        if h.root.is_some() || h.path.iter().any(|s| !s.selects.is_empty()) {
            return false;
        }
        let name = h
            .path
            .iter()
            .map(|s| s.name.name.as_str())
            .collect::<Vec<_>>()
            .join(".");
        if self.locals.contains(&name) {
            return false;
        }
        if module.parameters.contains_key(&name) {
            return true;
        }
        let [pkg, leaf] = &h.path[..] else {
            return false;
        };
        module
            .parameters
            .contains_key(&format!("{}::{}", pkg.name.name, leaf.name.name))
            || module
                .package_enum_members
                .get(&pkg.name.name)
                .is_some_and(|m| m.contains_key(&leaf.name.name))
    }

    /// Whether `e` is an elaboration-time constant expression (IEEE 1800
    /// §11.2.1): literals and parameters under operators, `?:`,
    /// concatenation, replication, selects of a parameter and the constant
    /// system functions. Its value comes from the simulator's evaluator.
    fn constant(&self, e: &Expression) -> bool {
        self.split(e).is_ok()
    }

    /// What constants can decide of condition `e`: the constant parts,
    /// under the `!`, `&&`, `||`, `&` and `|` that combine them.
    fn skeleton(&self, e: &Expression) -> Cond {
        match self.split(e) {
            Ok(()) => Cond::Const(e.clone()),
            Err(c) => c,
        }
    }

    /// `Ok` when `e` is a constant expression (see `constant`), else what
    /// constants can decide of it (see `skeleton`), in one walk. A `&&` or
    /// `&` with a constantly false operand is false whatever the other
    /// operand is, and a `||` or `|` with a constantly true one true, as in
    /// the reference simulator.
    fn split(&self, e: &Expression) -> Result<(), Cond> {
        let Some(module) = self.consts else {
            return Err(Cond::Unknown);
        };
        let known = |ok: bool| if ok { Ok(()) } else { Err(Cond::Unknown) };
        let all = |es: &[&Expression]| es.iter().all(|x| self.split(x).is_ok());
        match &e.kind {
            ExprKind::Number(_) | ExprKind::StringLiteral(_) => Ok(()),
            ExprKind::Ident(h) => known(self.param(h)),
            ExprKind::Paren(x) => self.split(x),
            ExprKind::Unary { op, operand } => match (op, self.split(operand)) {
                (UnaryOp::LogNot, Err(Cond::Unknown)) => Err(Cond::Unknown),
                (UnaryOp::LogNot, Err(c)) => Err(Cond::Not(Box::new(c))),
                (
                    UnaryOp::PreIncr
                    | UnaryOp::PreDecr
                    | UnaryOp::PostIncr
                    | UnaryOp::PostDecr
                    | UnaryOp::HashHash
                    | UnaryOp::SEventually
                    | UnaryOp::SAlways,
                    _,
                ) => Err(Cond::Unknown),
                (_, s) => known(s.is_ok()),
            },
            ExprKind::Binary { op, left, right } => {
                let (l, r) = (self.split(left), self.split(right));
                match op {
                    BinaryOp::LogAnd | BinaryOp::LogOr | BinaryOp::BitAnd | BinaryOp::BitOr => {
                        if l.is_ok() && r.is_ok() {
                            return Ok(());
                        }
                        // An operand of `&` or `|` takes the width of the
                        // other (§11.6.1), so a constant one is used only
                        // when its value does not depend on that width.
                        let bitwise = matches!(op, BinaryOp::BitAnd | BinaryOp::BitOr);
                        let part = |s: Result<(), Cond>, x: &Expression| match s {
                            Ok(()) if bitwise && !context_free(x) => Cond::Unknown,
                            Ok(()) => Cond::Const(x.clone()),
                            Err(c) => c,
                        };
                        let (l, r) = (part(l, left), part(r, right));
                        if matches!((&l, &r), (Cond::Unknown, Cond::Unknown)) {
                            return Err(Cond::Unknown);
                        }
                        let (l, r) = (Box::new(l), Box::new(r));
                        Err(match op {
                            BinaryOp::LogAnd => Cond::And(l, r, false),
                            BinaryOp::BitAnd => Cond::And(l, r, true),
                            BinaryOp::LogOr => Cond::Or(l, r, false),
                            _ => Cond::Or(l, r, true),
                        })
                    }
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Power
                    | BinaryOp::Eq
                    | BinaryOp::Neq
                    | BinaryOp::CaseEq
                    | BinaryOp::CaseNeq
                    | BinaryOp::WildcardEq
                    | BinaryOp::WildcardNeq
                    | BinaryOp::LogImplies
                    | BinaryOp::LogEquiv
                    | BinaryOp::Lt
                    | BinaryOp::Leq
                    | BinaryOp::Gt
                    | BinaryOp::Geq
                    | BinaryOp::BitXor
                    | BinaryOp::BitXnor
                    | BinaryOp::ShiftLeft
                    | BinaryOp::ShiftRight
                    | BinaryOp::ArithShiftLeft
                    | BinaryOp::ArithShiftRight => known(l.is_ok() && r.is_ok()),
                    _ => Err(Cond::Unknown),
                }
            }
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => known(all(&[condition, then_expr, else_expr])),
            ExprKind::Concatenation(parts) => {
                known(!parts.is_empty() && parts.iter().all(|p| self.split(p).is_ok()))
            }
            ExprKind::Replication { count, exprs } => {
                known(self.split(count).is_ok() && exprs.iter().all(|p| self.split(p).is_ok()))
            }
            ExprKind::Index { expr, index } => {
                known(matches!(&expr.kind, ExprKind::Ident(h) if self.param(h)) && all(&[index]))
            }
            ExprKind::RangeSelect {
                expr, left, right, ..
            } => known(
                matches!(&expr.kind, ExprKind::Ident(h) if self.param(h)) && all(&[left, right]),
            ),
            ExprKind::SystemCall { name, args } => match (name.as_str(), &args[..]) {
                (
                    "$clog2" | "$signed" | "$unsigned" | "$countones" | "$onehot" | "$onehot0",
                    [a],
                ) => known(all(&[a])),
                // §20.6.2: the width of a type, or of a fixed-width net or
                // variable.
                ("$bits", [a]) => match &a.kind {
                    ExprKind::TypeLiteral(_) => Ok(()),
                    ExprKind::Ident(h)
                        if h.root.is_none()
                            && h.path.len() == 1
                            && h.path[0].selects.is_empty() =>
                    {
                        let n = h.path[0].name.name.as_str();
                        known(
                            !self.locals.contains(n)
                                && !module.dynamic_arrays.contains(n)
                                && !module.associative_arrays.contains_key(n)
                                && !module.queue_vars.contains(n)
                                && !module.string_signals.contains(n),
                        )
                    }
                    _ => known(all(&[a])),
                },
                _ => Err(Cond::Unknown),
            },
            _ => Err(Cond::Unknown),
        }
    }

    /// The choice a `case` makes when its selector and every item are
    /// constants. `None` otherwise: as in the reference simulator, a `case`
    /// with any item that is not a constant keeps all of its arms, and so
    /// does a `case inside`.
    fn case_choice(&self, kind: CaseKind, expr: &Expression, items: &[CaseItem]) -> Option<Choice> {
        let constant = !matches!(kind, CaseKind::CaseInside)
            && self.constant(expr)
            && items.iter().all(|it| {
                it.pattern.is_none()
                    && it.guard.is_none()
                    && it.patterns.iter().all(|p| self.constant(p))
            });
        constant.then(|| {
            let items = items
                .iter()
                .map(|it| CaseItem {
                    patterns: it.patterns.clone(),
                    is_default: it.is_default,
                    stmt: Statement::new(StatementKind::Null, it.span),
                    span: it.span,
                    pattern: None,
                    guard: None,
                })
                .collect();
            Choice::Case(kind, expr.clone(), items)
        })
    }

    /// Note the names a statement declares (see `locals`).
    fn declare(&mut self, s: &Statement) {
        match &s.kind {
            StatementKind::VarDecl { declarators, .. } => {
                for d in declarators {
                    self.locals.insert(d.name.name.clone());
                }
            }
            StatementKind::For { init, .. } => {
                for i in init {
                    if let ForInit::VarDecl { name, .. } = i {
                        self.locals.insert(name.name.clone());
                    }
                }
            }
            StatementKind::Foreach { vars, .. } => {
                for v in vars.iter().flatten() {
                    self.locals.insert(v.name.clone());
                }
            }
            _ => {}
        }
    }

    /// A statement in a single-statement position.
    fn wrap(&mut self, s: Statement) -> Statement {
        let span = s.span;
        let mut v = Vec::new();
        self.boundary();
        let top = self.top.take();
        self.stmt(s, &mut v);
        self.top = top;
        block(v, span)
    }

    /// A branch arm: its counter, then the arm's own statements, and the
    /// sites they hold.
    fn arm(&mut self, s: Statement, counter: u32) -> (Statement, Region) {
        let span = s.span;
        let mut v = vec![hit(counter, span)];
        self.boundary();
        let top = self.top.take();
        let ((), r) = self.region(|me| me.stmt(s, &mut v));
        self.top = top;
        (block(v, span), r)
    }

    fn stmts(&mut self, items: Vec<Statement>) -> Vec<Statement> {
        let mut v = Vec::with_capacity(items.len() * 2);
        for s in items {
            self.stmt(s, &mut v);
        }
        v
    }

    /// An `always` body: the construct itself counts each time the body
    /// starts (after its leading event or delay control, if any), and the
    /// body's statements count as usual.
    fn always(&mut self, s: Statement, kind: &'static str) -> Statement {
        let span = s.span;
        let mut v = Vec::new();
        let top = (combinational(kind, &s) == Some(true)).then_some(span.start);
        match s.kind {
            StatementKind::TimingControl {
                control,
                stmt: inner,
            } => {
                self.process(span, kind, &mut v);
                self.top = top;
                self.stmt(*inner, &mut v);
                self.top = None;
                Statement::new(
                    StatementKind::TimingControl {
                        control,
                        stmt: Box::new(block(v, span)),
                    },
                    span,
                )
            }
            other => {
                self.process(span, kind, &mut v);
                self.top = top;
                self.stmt(Statement::new(other, span), &mut v);
                self.top = None;
                block(v, span)
            }
        }
    }

    /// In block granularity without branch coverage, the counter of an
    /// implicit `else` or `default` (`kind`), as a statement to put in it.
    /// With branch coverage the arm's count comes from the branch instead
    /// (see `cov_report`).
    fn implicit_block(&mut self, span: Span, kind: &'static str) -> Option<Statement> {
        if !self.blocks || self.kinds & KIND_STATEMENT == 0 {
            return None;
        }
        let counter = self.counter();
        self.cov.stmts.push(StmtSite {
            scope: self.scope,
            span,
            kind,
            counter,
            excluded: false,
        });
        Some(hit(counter, span))
    }

    /// An `always` construct's own count, each time its body starts; in
    /// block granularity the body's first block counts that instead.
    fn process(&mut self, span: Span, kind: &'static str, out: &mut Vec<Statement>) {
        if self.blocks {
            self.boundary();
        } else {
            self.count(span, kind, out);
        }
    }

    fn stmt(&mut self, s: Statement, out: &mut Vec<Statement>) {
        let span = s.span;
        let rebuilt = |kind| Statement::new(kind, span);
        if self.consts.is_some() {
            self.declare(&s);
        }
        match s.kind {
            StatementKind::SeqBlock { name, stmts } => {
                // A block's declarations end with it.
                let outer = self.here.locals.clone();
                self.boundary();
                let stmts = self.stmts(stmts);
                self.boundary();
                self.here.locals = outer;
                out.push(rebuilt(StatementKind::SeqBlock { name, stmts }));
            }
            // Each fork branch stays one process.
            StatementKind::ParBlock {
                name,
                join_type,
                stmts,
            } => {
                let outer = self.here.locals.clone();
                let stmts = stmts.into_iter().map(|c| self.wrap(c)).collect();
                self.boundary();
                self.here.locals = outer;
                out.push(rebuilt(StatementKind::ParBlock {
                    name,
                    join_type,
                    stmts,
                }));
            }
            StatementKind::If {
                unique_priority,
                condition,
                then_stmt,
                else_stmt,
            } => {
                // (A decision ends the block that evaluates it.)
                if self.blocks {
                    self.count(span, "statement", out);
                }
                self.if_stmt(unique_priority, condition, then_stmt, else_stmt, span, out);
                self.boundary();
            }
            StatementKind::Case {
                unique_priority,
                kind,
                expr,
                items,
            } => {
                if self.blocks {
                    self.count(span, "statement", out);
                }
                self.case_stmt(unique_priority, kind, expr, items, span, out);
                self.boundary();
            }
            StatementKind::RandCase { items } => {
                if self.blocks {
                    self.count(span, "statement", out);
                }
                let items = if self.kinds & KIND_BRANCH != 0 {
                    let mut arms = Vec::new();
                    let items = items
                        .into_iter()
                        .map(|(w, st)| {
                            let c = self.counter();
                            arms.push(Arm {
                                span: st.span,
                                label: "item",
                                counter: Some(c),
                                excluded: false,
                            });
                            (w, self.arm(st, c).0)
                        })
                        .collect();
                    self.cov.branches.push(BranchSite {
                        scope: self.scope,
                        span,
                        kind: "randcase",
                        entry: None,
                        arms,
                        excluded: false,
                    });
                    items
                } else {
                    items
                        .into_iter()
                        .map(|(w, st)| (w, self.wrap(st)))
                        .collect()
                };
                self.boundary();
                out.push(rebuilt(StatementKind::RandCase { items }));
            }
            StatementKind::For {
                init,
                condition,
                step,
                body,
            } => {
                self.count(span, "for", out);
                // The loop's own variables are locals of its body.
                let outer = self.here.locals.clone();
                for i in &init {
                    if let crate::ast::stmt::ForInit::VarDecl {
                        data_type, name, ..
                    } = i
                    {
                        let w = self.ctx.local(data_type, false);
                        self.here.locals.insert(name.name.clone(), w);
                    }
                }
                let body = Box::new(self.wrap(*body));
                self.boundary();
                self.here.locals = outer;
                out.push(rebuilt(StatementKind::For {
                    init,
                    condition,
                    step,
                    body,
                }));
            }
            StatementKind::Foreach { array, vars, body } => {
                self.count(span, "foreach", out);
                // Index variables take the array's index type (§12.7.3):
                // `int`, or an associative array's key type.
                let outer = self.here.locals.clone();
                let index = self.ctx.index_type(&array, &self.here);
                for v in vars.iter().flatten() {
                    self.here.locals.insert(v.name.clone(), index);
                }
                let body = Box::new(self.wrap(*body));
                self.boundary();
                self.here.locals = outer;
                out.push(rebuilt(StatementKind::Foreach { array, vars, body }));
            }
            StatementKind::While {
                mut condition,
                body,
            } => {
                self.count(span, "while", out);
                self.cond(&mut condition, "while", false);
                let body = Box::new(self.wrap(*body));
                self.boundary();
                out.push(rebuilt(StatementKind::While { condition, body }));
            }
            StatementKind::DoWhile {
                body,
                mut condition,
            } => {
                self.count(span, "do", out);
                let body = Box::new(self.wrap(*body));
                self.boundary();
                self.cond(&mut condition, "do", false);
                out.push(rebuilt(StatementKind::DoWhile { body, condition }));
            }
            StatementKind::Repeat { count, body } => {
                self.count(span, "repeat", out);
                let body = Box::new(self.wrap(*body));
                self.boundary();
                out.push(rebuilt(StatementKind::Repeat { count, body }));
            }
            StatementKind::Forever { body } => {
                self.count(span, "forever", out);
                let body = Box::new(self.wrap(*body));
                self.boundary();
                out.push(rebuilt(StatementKind::Forever { body }));
            }
            StatementKind::TimingControl {
                control,
                stmt: inner,
            } => {
                let kind = match &control {
                    TimingControl::Delay(_) => "delay",
                    TimingControl::Event(_) => "event",
                };
                // In block granularity the control ends the block before it
                // and the statement it controls starts one (in `wrap`).
                if !self.blocks {
                    self.count(span, kind, out);
                }
                let inner = Box::new(self.wrap(*inner));
                out.push(rebuilt(StatementKind::TimingControl {
                    control,
                    stmt: inner,
                }));
            }
            StatementKind::Wait {
                condition,
                stmt: inner,
            } => {
                if !self.blocks {
                    self.count(span, "wait", out);
                }
                let inner = Box::new(self.wrap(*inner));
                out.push(rebuilt(StatementKind::Wait {
                    condition,
                    stmt: inner,
                }));
            }
            // A declaration is no statement; its variables are locals for
            // the code after it.
            StatementKind::VarDecl {
                data_type,
                lifetime,
                declarators,
            } => {
                for d in &declarators {
                    let w = self.ctx.local(&data_type, !d.dimensions.is_empty());
                    self.here.locals.insert(d.name.name.clone(), w);
                }
                out.push(rebuilt(StatementKind::VarDecl {
                    data_type,
                    lifetime,
                    declarators,
                }));
            }
            // Not statements of their own: the markers and bodies the
            // executor and other constructs own.
            kind @ (StatementKind::Null
            | StatementKind::Typedef(_)
            | StatementKind::Coverpoint { .. }
            | StatementKind::Cross { .. }
            | StatementKind::ScopePop
            | StatementKind::LoopStep
            | StatementKind::ForeachTail { .. }
            | StatementKind::ForeverTail { .. }
            | StatementKind::RsAction { .. }
            | StatementKind::RsReturn) => out.push(rebuilt(kind)),
            StatementKind::BlockingAssign { lvalue, mut rvalue } => {
                self.count(span, "statement", out);
                self.value(&mut rvalue, &lvalue, "statement", true);
                out.push(rebuilt(StatementKind::BlockingAssign { lvalue, rvalue }));
            }
            StatementKind::NonblockingAssign {
                lvalue,
                delay,
                mut rvalue,
            } => {
                self.count(span, "statement", out);
                self.value(&mut rvalue, &lvalue, "statement", true);
                out.push(rebuilt(StatementKind::NonblockingAssign {
                    lvalue,
                    delay,
                    rvalue,
                }));
            }
            StatementKind::Expr(mut e) => {
                self.count(span, "statement", out);
                match &mut e.kind {
                    ExprKind::Call { args, .. } => {
                        for a in args.iter_mut() {
                            self.expr(a);
                        }
                    }
                    ExprKind::AssignExpr { rvalue, .. } => self.expr(rvalue),
                    ExprKind::Binary {
                        op: BinaryOp::Assign,
                        right,
                        ..
                    } => self.expr(right),
                    // A `?:` in a message is not a branch, but its condition
                    // is a condition, as in the reference simulator.
                    ExprKind::SystemCall { name, args }
                        if DISPLAY_TASKS.contains(&name.as_str()) =>
                    {
                        for a in args.iter_mut() {
                            self.message(a);
                        }
                    }
                    _ => {}
                }
                out.push(rebuilt(StatementKind::Expr(e)));
            }
            StatementKind::Return(Some(mut e)) => {
                self.count(span, "statement", out);
                self.expr(&mut e);
                self.boundary();
                out.push(rebuilt(StatementKind::Return(Some(e))));
            }
            kind => {
                self.count(span, "statement", out);
                if matches!(
                    kind,
                    StatementKind::Return(_)
                        | StatementKind::Break
                        | StatementKind::Continue
                        | StatementKind::Disable(_)
                ) {
                    self.boundary();
                }
                out.push(rebuilt(kind));
            }
        }
    }

    /// An `if`, with its `else if` chain as one branch: one arm per
    /// condition plus the final `else`, written or implicit. A `unique` or
    /// `priority` chain has no implicit arm (no match is a violation).
    /// Conditions constants can decide make a `Choice::If`.
    fn if_stmt(
        &mut self,
        unique_priority: Option<UniquePriority>,
        mut condition: Expression,
        then_stmt: Box<Statement>,
        else_stmt: Option<Box<Statement>>,
        span: Span,
        out: &mut Vec<Statement>,
    ) {
        if self.kinds & KIND_BRANCH == 0 {
            let cond = self.skeleton(&condition);
            self.cond(&mut condition, "if", true);
            let (then_stmt, t) = self.region(|me| Box::new(me.wrap(*then_stmt)));
            let implicit = else_stmt.is_none() && unique_priority.is_none();
            let (else_stmt, e) = self.region(|me| match else_stmt {
                Some(e) => Some(Box::new(me.wrap(*e))),
                // Block granularity counts the implicit `else` without
                // branch coverage too: an `else` holding only its counter.
                None if implicit => me.implicit_block(span, "else (implicit)").map(Box::new),
                None => None,
            });
            if !matches!(cond, Cond::Unknown) {
                self.decide(Choice::If(vec![cond]), None, vec![t, e], Vec::new());
            }
            out.push(Statement::new(
                StatementKind::If {
                    unique_priority,
                    condition,
                    then_stmt,
                    else_stmt,
                },
                span,
            ));
            return;
        }
        let mut conds = vec![self.skeleton(&condition)];
        let mut has_else = false;
        let mut tail = else_stmt.as_deref();
        while let Some(e) = tail {
            match &e.kind {
                StatementKind::If {
                    condition,
                    else_stmt,
                    ..
                } => {
                    conds.push(self.skeleton(condition));
                    tail = else_stmt.as_deref();
                }
                _ => {
                    has_else = true;
                    break;
                }
            }
        }
        let implicit = !has_else && unique_priority.is_none();
        let entry = implicit.then(|| self.counter());
        let site = self.cov.branches.len();
        self.cov.branches.push(BranchSite {
            scope: self.scope,
            span,
            kind: "if",
            entry,
            arms: Vec::new(),
            excluded: false,
        });
        let mut chain = Chain::default();
        let ((), c) = self.region(|me| me.cond(&mut condition, "if", true));
        chain.conds.push(c);
        // (An `else if` condition does not run each time the `if` does.)
        let top = self.top.take();
        let rewritten = self.if_arms(
            unique_priority,
            condition,
            then_stmt,
            else_stmt,
            span,
            true,
            &mut chain,
        );
        self.top = top;
        if implicit {
            chain.arms.push(Arm {
                span,
                label: "else (implicit)",
                counter: None,
                excluded: false,
            });
        }
        if !has_else {
            chain.regions.push((0..0, 0..0, 0..0));
        }
        self.cov.branches[site].arms = chain.arms;
        if conds.iter().any(|c| !matches!(c, Cond::Unknown)) {
            self.decide(Choice::If(conds), Some(site), chain.regions, chain.conds);
        }
        if let Some(e) = entry {
            out.push(hit(e, span));
        }
        out.push(rewritten);
    }

    /// The arms of an `if` chain from the one `chain` has reached on.
    #[allow(clippy::too_many_arguments)]
    fn if_arms(
        &mut self,
        unique_priority: Option<UniquePriority>,
        condition: Expression,
        then_stmt: Box<Statement>,
        else_stmt: Option<Box<Statement>>,
        span: Span,
        first: bool,
        chain: &mut Chain,
    ) -> Statement {
        let c = self.counter();
        chain.arms.push(Arm {
            span: then_stmt.span,
            label: if first { "if" } else { "else if" },
            counter: Some(c),
            excluded: false,
        });
        let (then_stmt, r) = self.arm(*then_stmt, c);
        chain.regions.push(r);
        let else_stmt = else_stmt.map(|e| {
            let e = *e;
            let espan = e.span;
            Box::new(match e.kind {
                StatementKind::If {
                    unique_priority: up,
                    condition: mut cond,
                    then_stmt: t,
                    else_stmt: el,
                } => {
                    let ((), c) = self.region(|me| me.cond(&mut cond, "if", true));
                    chain.conds.push(c);
                    self.if_arms(up, cond, t, el, espan, false, chain)
                }
                kind => {
                    let c = self.counter();
                    chain.arms.push(Arm {
                        span: espan,
                        label: "else",
                        counter: Some(c),
                        excluded: false,
                    });
                    let (s, r) = self.arm(Statement::new(kind, espan), c);
                    chain.regions.push(r);
                    s
                }
            })
        });
        Statement::new(
            StatementKind::If {
                unique_priority,
                condition,
                then_stmt: Box::new(then_stmt),
                else_stmt,
            },
            span,
        )
    }

    /// A `case`: one arm per item, plus an implicit `default` when there is
    /// none (except for `unique` / `priority` cases). A constant selector
    /// with constant items makes a `Choice::Case`.
    fn case_stmt(
        &mut self,
        unique_priority: Option<UniquePriority>,
        kind: CaseKind,
        expr: Expression,
        mut items: Vec<CaseItem>,
        span: Span,
        out: &mut Vec<Statement>,
    ) {
        let choice = self.case_choice(kind, &expr, &items);
        let mut regions = Vec::with_capacity(items.len() + 1);
        let mut site = None;
        // What the implicit `default` holds: nothing, or its block.
        let mut fallback: Region = (0..0, 0..0, 0..0);
        if self.kinds & KIND_BRANCH == 0 {
            for it in items.iter_mut() {
                let st = std::mem::replace(&mut it.stmt, Statement::new(StatementKind::Null, span));
                let (s, r) = self.region(|me| me.wrap(st));
                it.stmt = s;
                regions.push(r);
            }
            // Block granularity counts the implicit `default` without branch
            // coverage too: a `default` item holding only its counter (an
            // ordinary `case`; not a `case matches`).
            let implicit = !items.iter().any(|it| it.is_default)
                && unique_priority.is_none()
                && items.first().is_some_and(|it| it.pattern.is_none());
            if implicit {
                let (stmt, r) = self.region(|me| me.implicit_block(span, "default (implicit)"));
                fallback = r;
                if let Some(stmt) = stmt {
                    let mut d = items[0].clone();
                    d.patterns.clear();
                    d.is_default = true;
                    d.stmt = stmt;
                    d.span = span;
                    items.push(d);
                }
            }
        } else {
            let implicit = !items.iter().any(|it| it.is_default) && unique_priority.is_none();
            let entry = implicit.then(|| self.counter());
            site = Some(self.cov.branches.len());
            self.cov.branches.push(BranchSite {
                scope: self.scope,
                span,
                kind: match kind {
                    CaseKind::Case => "case",
                    CaseKind::Casez => "casez",
                    CaseKind::Casex => "casex",
                    CaseKind::CaseInside => "case inside",
                },
                entry,
                arms: Vec::new(),
                excluded: false,
            });
            let mut arms = Vec::new();
            for it in items.iter_mut() {
                let c = self.counter();
                arms.push(Arm {
                    span: it.span,
                    label: if it.is_default { "default" } else { "item" },
                    counter: Some(c),
                    excluded: false,
                });
                let st = std::mem::replace(&mut it.stmt, Statement::new(StatementKind::Null, span));
                let (s, r) = self.arm(st, c);
                it.stmt = s;
                regions.push(r);
            }
            if implicit {
                arms.push(Arm {
                    span,
                    label: "default (implicit)",
                    counter: None,
                    excluded: false,
                });
            }
            if let Some(i) = site {
                self.cov.branches[i].arms = arms;
            }
            if let Some(e) = entry {
                out.push(hit(e, span));
            }
        }
        regions.push(fallback);
        if let Some(choice) = choice {
            self.decide(choice, site, regions, Vec::new());
        }
        out.push(Statement::new(
            StatementKind::Case {
                unique_priority,
                kind,
                expr,
                items,
            },
            span,
        ));
    }

    /// A `?:` as a branch: its arms' counters, and its condition wrapped in
    /// `COND_FN`. `cond` is what constants make of the condition, taken
    /// before it was rewritten, and `t` and `f` what the arms hold: a `?:`
    /// whose condition constants can decide makes a `Choice::Ternary`.
    fn ternary(&mut self, e: &mut Expression, cond: Cond, t: Region, f: Region) {
        let espan = e.span;
        let ExprKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } = &mut e.kind
        else {
            return;
        };
        let tc = self.counter();
        let fc = self.counter();
        let site = self.cov.branches.len();
        self.cov.branches.push(BranchSite {
            scope: self.scope,
            span: espan,
            kind: "ternary",
            entry: None,
            arms: vec![
                Arm {
                    span: then_expr.span,
                    label: "true",
                    counter: Some(tc),
                    excluded: false,
                },
                Arm {
                    span: else_expr.span,
                    label: "false",
                    counter: Some(fc),
                    excluded: false,
                },
            ],
            excluded: false,
        });
        if !matches!(cond, Cond::Unknown) {
            self.decide(Choice::Ternary(cond), Some(site), vec![t, f], Vec::new());
        }
        let cspan = condition.span;
        let cond = std::mem::replace(&mut **condition, Expression::new(ExprKind::Empty, cspan));
        **condition = Expression::new(
            ExprKind::SystemCall {
                name: COND_FN.to_string(),
                args: vec![number(tc, cspan), cond],
            },
            cspan,
        );
    }

    /// Instrument every `?:` in `e`: its condition as a condition item, and
    /// the `?:` as a branch (see `COND_FN`). Stops at system calls, whose
    /// arguments some tasks read by shape. A `?:` whose condition constants
    /// can decide makes a `Choice::Ternary`, and an `&&`, `||`, `&` or `|`
    /// with a `?:` inside a `Choice::Operand`.
    fn expr(&mut self, e: &mut Expression) {
        if self.kinds & (KIND_BRANCH | KIND_CONDITION) == 0 || !has_ternary(e) {
            return;
        }
        // An `&&` (`||`) never evaluates its right operand when the left one
        // is false (true). Taken before the `?:`s inside are rewritten,
        // which hides the constants among them.
        let decider = match &e.kind {
            ExprKind::Binary {
                op: op @ (BinaryOp::LogAnd | BinaryOp::LogOr),
                left,
                right,
            } if has_ternary(right) => Some((self.skeleton(left), *op == BinaryOp::LogOr)),
            _ => None,
        };
        let mut operand = None;
        match &mut e.kind {
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond = self.skeleton(condition);
                self.cond(condition, "ternary", true);
                let ((), t) = self.region(|me| me.expr(then_expr));
                let ((), f) = self.region(|me| me.expr(else_expr));
                if self.kinds & KIND_BRANCH != 0 {
                    self.ternary(e, cond, t, f);
                }
            }
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { left, right, .. } => {
                self.expr(left);
                operand = Some(self.region(|me| me.expr(right)).1);
            }
            ExprKind::Paren(inner) => self.expr(inner),
            ExprKind::Concatenation(parts) => {
                for p in parts.iter_mut() {
                    self.expr(p);
                }
            }
            ExprKind::Replication { exprs, .. } => {
                for p in exprs.iter_mut() {
                    self.expr(p);
                }
            }
            ExprKind::Index { expr, index } => {
                self.expr(expr);
                self.expr(index);
            }
            ExprKind::Call { args, .. } => {
                for a in args.iter_mut() {
                    self.expr(a);
                }
            }
            _ => {}
        }
        if let (Some((cond, skip)), Some(r)) = (decider, operand)
            && !r.1.is_empty()
            && !matches!(cond, Cond::Unknown)
        {
            self.decide(Choice::Operand(cond, skip), None, vec![r], Vec::new());
        }
    }
}

/// Whether `e` has the same value in a wider context (§11.6.1): a
/// self-determined expression (a literal, a parameter or a select of one,
/// a concatenation, a system function, a comparison, or a logical or
/// reduction operator), so that whether it is zero, or has a 1 bit, does
/// not depend on the operator it is an operand of. `4'hf + 4'h1` is 0 on
/// its own but 8'h10 beside an 8-bit operand.
fn context_free(e: &Expression) -> bool {
    match &e.kind {
        ExprKind::Paren(x) => context_free(x),
        ExprKind::Number(_)
        | ExprKind::StringLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::Index { .. }
        | ExprKind::RangeSelect { .. }
        | ExprKind::Concatenation(_)
        | ExprKind::Replication { .. }
        | ExprKind::SystemCall { .. } => true,
        ExprKind::Unary { op, .. } => {
            !matches!(op, UnaryOp::Plus | UnaryOp::Minus | UnaryOp::BitNot)
        }
        ExprKind::Binary { op, .. } => matches!(
            op,
            BinaryOp::Eq
                | BinaryOp::Neq
                | BinaryOp::CaseEq
                | BinaryOp::CaseNeq
                | BinaryOp::WildcardEq
                | BinaryOp::WildcardNeq
                | BinaryOp::Lt
                | BinaryOp::Leq
                | BinaryOp::Gt
                | BinaryOp::Geq
                | BinaryOp::LogAnd
                | BinaryOp::LogOr
                | BinaryOp::LogImplies
                | BinaryOp::LogEquiv
        ),
        _ => false,
    }
}

/// Whether `expr` would find a `?:` to instrument in `e`: it walks the same
/// operands.
fn has_ternary(e: &Expression) -> bool {
    match &e.kind {
        ExprKind::Conditional { .. } => true,
        ExprKind::Unary { operand, .. } | ExprKind::Paren(operand) => has_ternary(operand),
        ExprKind::Binary { left, right, .. } => has_ternary(left) || has_ternary(right),
        ExprKind::Concatenation(parts) | ExprKind::Replication { exprs: parts, .. } => {
            parts.iter().any(has_ternary)
        }
        ExprKind::Index { expr, index } => has_ternary(expr) || has_ternary(index),
        ExprKind::Call { args, .. } => args.iter().any(has_ternary),
        _ => false,
    }
}

impl Instr<'_> {
    /// The `?:` conditions in a message task's argument, as condition items.
    fn message(&mut self, e: &mut Expression) {
        if self.kinds & KIND_CONDITION == 0 {
            return;
        }
        match &mut e.kind {
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.cond(condition, "ternary", false);
                self.message(then_expr);
                self.message(else_expr);
            }
            ExprKind::Paren(inner) | ExprKind::Unary { operand: inner, .. } => self.message(inner),
            ExprKind::Binary { left, right, .. } => {
                self.message(left);
                self.message(right);
            }
            ExprKind::Concatenation(parts) => {
                for p in parts.iter_mut() {
                    self.message(p);
                }
            }
            _ => {}
        }
    }

    /// An `if`, `while` or `?:` condition: a condition item when it is a
    /// logic expression. `deep` instruments the `?:`s inside its terms (not
    /// done in loop conditions, whose `?:`s are not branches).
    fn cond(&mut self, e: &mut Expression, kind: &'static str, deep: bool) {
        if self.kinds & KIND_CONDITION != 0 {
            if let Some(p) = self.plan(e, false) {
                self.rewrite(e, &p.nodes, 0, deep);
                self.item(e, p, kind, true);
                return;
            }
        }
        if deep {
            self.expr(e);
        }
    }

    /// An assignment's value: an expression item when it is a logic
    /// expression whose value is 1 bit wide where it is assigned. `deep`
    /// also instruments its `?:`s (not done in port connections, which are
    /// not statements).
    fn value(&mut self, e: &mut Expression, lhs: &Expression, kind: &'static str, deep: bool) {
        // A compound assignment (`a |= b`) arrives as `a = a | b`, the
        // copy of `a` at its own place: not an expression of the source.
        let compound = e.span.start == lhs.span.start;
        // The `?:`s of an expression item are its multiplexers, not
        // conditions, whichever kinds are collected: the item is planned
        // for condition coverage too, so that it never counts them.
        // `--code-coverage-expressions=logical`: only conditions are items.
        if self.kinds & (KIND_EXPRESSION | KIND_CONDITION) != 0 && !compound && !self.logical {
            if let Some(p) = self.plan(e, true) {
                // `~`, the bitwise operators and `?:` take their width from
                // where the value goes.
                let sized = match &unparen(e).kind {
                    ExprKind::Unary {
                        op: UnaryOp::BitNot,
                        ..
                    }
                    | ExprKind::Conditional { .. } => true,
                    ExprKind::Binary { op, .. } => matches!(
                        op,
                        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::BitXnor
                    ),
                    _ => false,
                };
                if !sized || self.ctx.width(lhs, &self.here) == Some(1) {
                    self.rewrite(e, &p.nodes, 0, deep);
                    if self.kinds & KIND_EXPRESSION != 0 {
                        self.item(e, p, kind, false);
                    }
                    return;
                }
            }
        }
        if deep {
            self.expr(e);
        }
    }

    /// The logic tree of `e` when it is a condition or expression item: it
    /// has a logic operator on two non-constant operands (or a `?:` in an
    /// expression), or it is a relation (`a == b`; a negated one, `!(a ==
    /// b)`, is not, as in the reference simulator), and at most `MAX_TERMS`
    /// input terms.
    fn plan(&self, e: &Expression, mux: bool) -> Option<Plan> {
        let mut p = Plan::default();
        let text = self.ctx.source(e, self.cov.scopes[self.scope as usize].src);
        self.build(e, text, mux, &mut p);
        // A system function as a term (`!$isunknown(x) && y`) leaves the
        // expression out, as in the reference simulator.
        if p.terms.is_empty() || p.terms.len() > MAX_TERMS || p.system {
            return None;
        }
        let constant = |k: u32| p.nodes[k as usize].op == Op::Const;
        // `--code-coverage-expressions=logical`: an item has a `&&` or `||`
        // on two non-constant operands; a relation alone is none.
        if self.logical {
            let logical = p.nodes.iter().any(|n| {
                matches!(n.op, Op::And | Op::Or | Op::AndFull | Op::OrFull)
                    && !constant(n.kids[0])
                    && !constant(n.kids[1])
            });
            return logical.then_some(p);
        }
        let operators = p.nodes.iter().any(|n| match n.op {
            Op::And
            | Op::Or
            | Op::AndFull
            | Op::OrFull
            | Op::BitAnd
            | Op::BitOr
            | Op::BitXor
            | Op::BitXnor => !constant(n.kids[0]) && !constant(n.kids[1]),
            Op::Mux => !constant(n.kids[0]),
            _ => false,
        });
        (operators || p.relational[0]).then_some(p)
    }

    /// Fold node `i` of `p` (its kids built) to a constant when its value
    /// is: a constant operand that decides it (`x & 0`, `x || 1`), or
    /// constant operands only. The reference simulator leaves such parts of
    /// an expression out; their terms (those after the first `terms`) go
    /// with them.
    fn fold(p: &mut Plan, i: usize, op: Op, kids: [u32; 3], terms: usize) -> Op {
        let k = |j: usize| p.konst[kids[j] as usize];
        let value = match op {
            Op::Not => k(0).map(|v| v.map(|v| (v == 0) as u64)),
            Op::And | Op::BitAnd | Op::AndFull | Op::Or | Op::BitOr | Op::OrFull => {
                let decides = matches!(op, Op::Or | Op::BitOr | Op::OrFull) as u64;
                let ctl = |c: Option<Option<u64>>| {
                    c.flatten().is_some_and(|v| (v != 0) as u64 == decides)
                };
                if ctl(k(0)) || ctl(k(1)) {
                    Some(Some(decides))
                } else {
                    (k(0).is_some() && k(1).is_some()).then_some(None)
                }
            }
            Op::BitXor | Op::BitXnor => (k(0).is_some() && k(1).is_some()).then_some(None),
            _ => None,
        };
        let Some(value) = value else {
            return op;
        };
        p.nodes.truncate(i + 1);
        p.relational.truncate(i + 1);
        p.konst.truncate(i + 1);
        p.terms.truncate(terms);
        p.konst[i] = Some(value);
        Op::Const
    }

    /// Add `e` to `p` as a tree node; returns its index. `text` is `e`'s
    /// source text, cut from the item's by the operators above it (the
    /// spans of an instance's port references can point into its parent's
    /// file).
    fn build(&self, e: &Expression, text: Option<String>, mux: bool, p: &mut Plan) -> u32 {
        let e = unparen(e);
        let text = text.map(|t| strip_parens(&t));
        let i = p.nodes.len();
        let (terms, system) = (p.terms.len(), p.system);
        p.nodes.push(Node {
            op: Op::Const,
            kids: [0; 3],
        });
        p.relational.push(false);
        p.konst.push(self.ctx.constant(e, &self.here));
        let one = |x: &Expression| self.ctx.width(x, &self.here) == Some(1);
        let reduction = |x: &Expression| {
            matches!(
                &unparen(x).kind,
                ExprKind::Unary {
                    op: UnaryOp::BitAnd | UnaryOp::BitOr | UnaryOp::BitXor,
                    ..
                }
            )
        };
        let t = text.as_deref();
        let (op, kids): (Op, Vec<(&Expression, Option<String>)>) = match &e.kind {
            // Folded away, as by the reference simulator: no term.
            _ if p.konst[i].is_some() => (Op::Const, Vec::new()),
            // `--code-coverage-expressions=logical`: only `&&` and `||`
            // split an item; anything else under them is a term.
            ExprKind::Binary {
                op: BinaryOp::LogAnd | BinaryOp::LogOr,
                ..
            } if self.logical => self.split_logical(e, t),
            // A `!` over a `&&` or `||` keeps the tree below it.
            ExprKind::Unary {
                op: UnaryOp::LogNot,
                operand,
            } if self.logical
                && matches!(
                    &unparen(operand).kind,
                    ExprKind::Binary {
                        op: BinaryOp::LogAnd | BinaryOp::LogOr,
                        ..
                    }
                ) =>
            {
                (
                    Op::Not,
                    vec![(operand, t.and_then(|t| unary_operand(t, '!')))],
                )
            }
            _ if self.logical => (self.term(e, t, p, i), Vec::new()),
            ExprKind::Unary {
                op: UnaryOp::LogNot,
                operand,
            } => (
                Op::Not,
                vec![(operand, t.and_then(|t| unary_operand(t, '!')))],
            ),
            // `~^x` (`~(^x)` once parsed) is a term, the reduction xnor.
            ExprKind::Unary {
                op: UnaryOp::BitNot,
                operand,
            } if one(operand) && !reduction(operand) => (
                Op::Not,
                vec![(operand, t.and_then(|t| unary_operand(t, '~')))],
            ),
            ExprKind::Binary { op, left, right } => {
                let op = match op {
                    BinaryOp::LogAnd => Some((Op::And, "&&")),
                    BinaryOp::LogOr => Some((Op::Or, "||")),
                    BinaryOp::BitAnd if one(left) && one(right) => Some((Op::BitAnd, "&")),
                    BinaryOp::BitOr if one(left) && one(right) => Some((Op::BitOr, "|")),
                    BinaryOp::BitXor if one(left) && one(right) => Some((Op::BitXor, "^")),
                    BinaryOp::BitXnor if one(left) && one(right) => Some((Op::BitXnor, "~^")),
                    _ => None,
                };
                match op {
                    Some((op, tok)) => {
                        let (l, r) = match t.and_then(|t| split_binary(t, tok)) {
                            Some((l, r)) => (Some(l), Some(r)),
                            None => (None, None),
                        };
                        (op, vec![(left, l), (right, r)])
                    }
                    None => (self.term(e, t, p, i), Vec::new()),
                }
            }
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } if mux && one(then_expr) && one(else_expr) => {
                let (c, a, b) = match t.and_then(split_mux) {
                    Some((c, a, b)) => (Some(c), Some(a), Some(b)),
                    None => (None, None, None),
                };
                (
                    Op::Mux,
                    vec![(condition, c), (then_expr, a), (else_expr, b)],
                )
            }
            _ => (self.term(e, t, p, i), Vec::new()),
        };
        let mut ks = [0u32; 3];
        for (j, (k, kt)) in kids.into_iter().enumerate() {
            ks[j] = self.build(k, kt, mux, p);
        }
        if op == Op::Not {
            let k = &mut p.nodes[ks[0] as usize].op;
            *k = match *k {
                Op::And => Op::AndFull,
                Op::Or => Op::OrFull,
                k => k,
            };
        }
        let op = Self::fold(p, i, op, ks, terms);
        if op == Op::Const {
            p.system = system;
            ks = [0; 3];
        }
        p.nodes[i] = Node { op, kids: ks };
        i as u32
    }

    /// A `&&` or `||` node of a logical-mode item (see `build`): its
    /// operator and operands, each with its source text.
    #[allow(clippy::type_complexity)]
    fn split_logical<'e>(
        &self,
        e: &'e Expression,
        t: Option<&str>,
    ) -> (Op, Vec<(&'e Expression, Option<String>)>) {
        let ExprKind::Binary { op, left, right } = &e.kind else {
            unreachable!("split_logical takes a binary operator");
        };
        let (op, tok) = match op {
            BinaryOp::LogAnd => (Op::And, "&&"),
            _ => (Op::Or, "||"),
        };
        let (l, r) = match t.and_then(|t| split_binary(t, tok)) {
            Some((l, r)) => (Some(l), Some(r)),
            None => (None, None),
        };
        (op, vec![(left, l), (right, r)])
    }

    /// The term `e` (node `i` of `p`, source text `text`) stands for. A
    /// 1-bit `t == 0` (or `t != 1`, either way round) is the term `t`,
    /// inverted, as in the reference simulator; another relation is a term
    /// of its own.
    fn term(&self, e: &Expression, text: Option<&str>, p: &mut Plan, i: usize) -> Op {
        let mut named = text.map(str::to_string);
        let mut inv = false;
        match &e.kind {
            ExprKind::Binary { op, left, right } if matches!(op, BinaryOp::Eq | BinaryOp::Neq) => {
                let bit = |x: &Expression| self.ctx.width(x, &self.here) == Some(1);
                let k = |x: &Expression| self.ctx.constant(x, &self.here).flatten();
                let pick = match (k(left), k(right)) {
                    (None, Some(v)) if v <= 1 && bit(left) => Some((&**left, v, 0)),
                    (Some(v), None) if v <= 1 && bit(right) => Some((&**right, v, 1)),
                    _ => None,
                };
                match pick {
                    Some((t, v, side)) => {
                        inv = (v == 0) == matches!(op, BinaryOp::Eq);
                        let tok = if matches!(op, BinaryOp::Eq) {
                            "=="
                        } else {
                            "!="
                        };
                        named = text
                            .and_then(|s| split_binary(s, tok))
                            .map(|(l, r)| strip_parens(if side == 0 { &l } else { &r }))
                            .or_else(|| Some(show(t)));
                    }
                    None => p.relational[i] = true,
                }
            }
            ExprKind::Binary { op, .. } if Ctx::relational(*op) => p.relational[i] = true,
            ExprKind::SystemCall { .. } => p.system = true,
            _ => {}
        }
        let text = named.unwrap_or_else(|| strip_parens(&show(e)));
        let t = match p.terms.iter().position(|x| *x == text) {
            Some(t) => t,
            None => {
                p.terms.push(text);
                p.terms.len() - 1
            }
        };
        Op::Term(t as u16, inv)
    }

    /// Instrument what an item's tree leaves to the rest of the design:
    /// the `?:`s inside its terms (with `deep`) and the `?:` branches of its
    /// multiplexers, whose conditions it evaluates itself.
    fn rewrite(&mut self, e: &mut Expression, nodes: &[Node], i: u32, deep: bool) {
        let e = unparen_mut(e);
        let n = nodes[i as usize];
        match n.op {
            // (A folded constant may hold a `?:` too.)
            Op::Term(..) | Op::Const => {
                if deep {
                    self.expr(e);
                }
            }
            Op::Mux => {
                let mut parts = (Cond::Unknown, (0..0, 0..0, 0..0), (0..0, 0..0, 0..0));
                if let ExprKind::Conditional {
                    condition,
                    then_expr,
                    else_expr,
                } = &mut e.kind
                {
                    let cond = self.skeleton(condition);
                    self.rewrite(condition, nodes, n.kids[0], deep);
                    let ((), t) = self.region(|me: &mut Self| me.rewrite(then_expr, nodes, n.kids[1], deep));
                    let ((), f) = self.region(|me: &mut Self| me.rewrite(else_expr, nodes, n.kids[2], deep));
                    parts = (cond, t, f);
                }
                if deep && self.kinds & KIND_BRANCH != 0 {
                    self.ternary(e, parts.0, parts.1, parts.2);
                }
            }
            _ => match &mut e.kind {
                ExprKind::Unary { operand, .. } => self.rewrite(operand, nodes, n.kids[0], deep),
                ExprKind::Binary { left, right, .. } => {
                    self.rewrite(left, nodes, n.kids[0], deep);
                    self.rewrite(right, nodes, n.kids[1], deep);
                }
                _ => {}
            },
        }
    }

    /// Register `e` as a condition or expression item and wrap it in
    /// `EXPR_FN`.
    fn item(&mut self, e: &mut Expression, p: Plan, kind: &'static str, condition: bool) {
        let src = self.cov.scopes[self.scope as usize].src;
        let id = self.cov.exprs.len() as u32;
        let row0 = self.cov.counters;
        let bimodal = p
            .nodes
            .iter()
            .any(|n| matches!(n.op, Op::BitXor | Op::BitXnor | Op::Mux));
        self.cov.counters += (2 + 2 * bimodal as u32) * p.terms.len() as u32;
        self.cov.exprs.push(ExprSite {
            scope: self.scope,
            span: e.span,
            kind,
            condition,
            block: self.block.clone(),
            text: self.ctx.text(e, src),
            terms: p.terms,
            row0,
            bimodal,
            nodes: p.nodes.into(),
            hint: self.hint.clone(),
            copy: if self.hint.is_some() {
                let mut s = String::new();
                numbers(e, &mut s);
                s
            } else {
                String::new()
            },
            proc: self
                .top
                .filter(|_| matches!(kind, "if" | "statement"))
                .map(|s| (self.here.path.clone(), s)),
            last: Vec::new(),
            steps: None,
            excluded: false,
        });
        let span = e.span;
        let inner = std::mem::replace(e, Expression::new(ExprKind::Empty, span));
        *e = Expression::new(
            ExprKind::SystemCall {
                name: EXPR_FN.to_string(),
                args: vec![number(id, span), inner],
            },
            span,
        );
    }
}

/// Newline offsets of each retained source text, built on first use.
struct Lines<'a> {
    module: &'a ElaboratedModule,
    nl: HashMap<usize, Vec<usize>>,
}

impl Lines<'_> {
    /// File and line of `span`, in the text `src` names or else the only
    /// text long enough to contain it.
    fn locate(&mut self, span: Span, src: Option<u32>) -> Option<(String, u32)> {
        let texts = &self.module.source_texts;
        let i = match src
            .map(|s| s as usize)
            .filter(|&s| texts.get(s).is_some_and(|t| span.start < t.len()))
        {
            Some(i) => i,
            None => {
                let mut fits = texts
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| span.start < t.len());
                let (i, _) = fits.next()?;
                if fits.next().is_some() {
                    return None;
                }
                i
            }
        };
        let nl = self.nl.entry(i).or_insert_with(|| {
            texts[i]
                .bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(p, _)| p)
                .collect()
        });
        let out_line = nl.partition_point(|&p| p < span.start);
        if let Some(m) = self.module.source_line_maps.get(i).and_then(|m| m.as_ref()) {
            if let Some((f, l)) = m.file_line(out_line) {
                return Some((f.to_string(), l));
            }
        }
        let file = self.module.source_files.get(i).filter(|f| !f.is_empty())?;
        Some((file.clone(), out_line as u32 + 1))
    }
}

/// One reported statement, arm or toggle row after locating and merging.
#[derive(Clone)]
struct StmtRow {
    file: String,
    line: u32,
    start: usize,
    kind: &'static str,
    count: u64,
    excluded: bool,
}

#[derive(Clone)]
struct ArmRow {
    line: u32,
    label: &'static str,
    count: u64,
    excluded: bool,
}

#[derive(Clone)]
struct BranchRow {
    file: String,
    line: u32,
    start: usize,
    kind: &'static str,
    arms: Vec<ArmRow>,
    excluded: bool,
}

/// Add the count of another copy of a row (another instance of the design
/// unit, or a copy elaboration made) to a row's. The row stays excluded
/// only when both are, and an included row does not take in an excluded
/// copy's count: the taken arm of a constant decision runs, but it is no
/// arm where the decision is not constant.
fn add_count(count: &mut u64, excluded: &mut bool, other: u64, other_excluded: bool) {
    match (*excluded, other_excluded) {
        (true, false) => {
            *count = other;
            *excluded = false;
        }
        (false, true) => {}
        _ => *count += other,
    }
}

/// The JSON member marking an excluded row.
fn excluded_json(excluded: bool) -> String {
    if excluded {
        format!(", \"excluded\": \"{}\"", EXCLUDED)
    } else {
        String::new()
    }
}

#[derive(Clone)]
struct ToggleRow {
    name: String,
    ty: &'static str,
    rise: Vec<u32>,
    fall: Vec<u32>,
}

/// A condition or expression item: each term with the counts of its two
/// rows (the term at 0, and at 1, deciding the result), and in a bimodal
/// item those by the item's value too (row 0 at 0, row 0 at 1, row 1 at 0,
/// row 1 at 1; see `ExprSite::bimodal`).
#[derive(Clone)]
struct ItemRow {
    file: String,
    line: u32,
    start: usize,
    kind: &'static str,
    block: String,
    copy: String,
    text: String,
    terms: Vec<(String, [u64; 2])>,
    outs: Vec<[u64; 4]>,
}

impl ItemRow {
    /// Term `t` is covered: both of its rows were hit, at different values
    /// of the item when it is bimodal.
    fn covered(&self, t: usize) -> bool {
        match self.outs.get(t) {
            Some(o) => (o[0] > 0 && o[3] > 0) || (o[1] > 0 && o[2] > 0),
            None => self.terms[t].1[0] > 0 && self.terms[t].1[1] > 0,
        }
    }

    #[allow(clippy::type_complexity)]
    fn key(&self) -> (&String, u32, usize, &str, &String, &String, &String) {
        (
            &self.file,
            self.line,
            self.start,
            self.kind,
            &self.block,
            &self.text,
            &self.copy,
        )
    }
}

#[derive(Default)]
struct Report {
    stmts: Vec<StmtRow>,
    branches: Vec<BranchRow>,
    toggles: Vec<ToggleRow>,
    conds: Vec<ItemRow>,
    exprs: Vec<ItemRow>,
    fsms: Vec<FsmRow>,
}

/// Sort items and add up the copies of one (a parameterized class's, the
/// pieces of one continuous assignment, an instance's in a design unit); a
/// generate loop's iterations stay apart by their block, and copies whose
/// constants left different terms stay apart by their terms.
fn merge_items(items: &mut Vec<ItemRow>) {
    let names = |it: &ItemRow| it.terms.iter().map(|t| t.0.clone()).collect::<Vec<_>>();
    // (Bimodal and other copies of one item count differently: apart.)
    items.sort_by(|a, b| {
        (a.key(), names(a), a.outs.len()).cmp(&(b.key(), names(b), b.outs.len()))
    });
    let mut merged: Vec<ItemRow> = Vec::with_capacity(items.len());
    for it in items.drain(..) {
        match merged.last_mut() {
            Some(m)
                if m.key() == it.key()
                    && names(m) == names(&it)
                    && m.outs.len() == it.outs.len() =>
            {
                for (x, y) in m.terms.iter_mut().zip(&it.terms) {
                    x.1[0] += y.1[0];
                    x.1[1] += y.1[1];
                }
                for (x, y) in m.outs.iter_mut().zip(&it.outs) {
                    for (a, b) in x.iter_mut().zip(y) {
                        *a += b;
                    }
                }
            }
            _ => merged.push(it),
        }
    }
    *items = merged;
}

fn items_json(items: &[ItemRow], out: &mut String) {
    for (i, it) in items.iter().enumerate() {
        out.push_str(&format!(
            "{}{{\"file\": \"{}\", \"line\": {}, \"kind\": \"{}\", ",
            if i == 0 { "" } else { ", " },
            json_escape(&it.file),
            it.line,
            it.kind
        ));
        if !it.block.is_empty() {
            out.push_str(&format!("\"block\": \"{}\", ", json_escape(&it.block)));
        }
        out.push_str(&format!(
            "\"text\": \"{}\", \"terms\": [",
            json_escape(&it.text)
        ));
        for (j, (t, h)) in it.terms.iter().enumerate() {
            out.push_str(&format!(
                "{}{{\"term\": \"{}\", \"hits\": [{}, {}]",
                if j == 0 { "" } else { ", " },
                json_escape(t),
                h[0],
                h[1]
            ));
            if let Some(o) = it.outs.get(j) {
                out.push_str(&format!(
                    ", \"outputs\": [[{}, {}], [{}, {}]]",
                    o[0], o[1], o[2], o[3]
                ));
            }
            out.push('}');
        }
        out.push_str("]}");
    }
}

/// Covered and total items; `excluded` counts the ones left out of both.
#[derive(Clone, Copy, Default)]
struct Tally {
    covered: u64,
    total: u64,
    excluded: u64,
}

impl Tally {
    fn add(&mut self, o: Tally) {
        self.covered += o.covered;
        self.total += o.total;
        self.excluded += o.excluded;
    }

    fn json(&self) -> String {
        let excluded = if self.excluded > 0 {
            format!(", \"excluded\": {}", self.excluded)
        } else {
            String::new()
        };
        if self.total == 0 {
            format!(
                "{{\"covered\": 0, \"total\": 0, \"percent\": null{}}}",
                excluded
            )
        } else {
            format!(
                "{{\"covered\": {}, \"total\": {}, \"percent\": {:.2}{}}}",
                self.covered,
                self.total,
                self.percent(),
                excluded
            )
        }
    }

    fn percent(&self) -> f64 {
        100.0 * self.covered as f64 / self.total.max(1) as f64
    }

    fn text(&self) -> String {
        let mut notes = Vec::new();
        if self.total > 0 {
            notes.push(format!("{:.2}%", self.percent()));
        }
        if self.excluded > 0 {
            notes.push(format!("{} excluded", self.excluded));
        }
        if notes.is_empty() {
            format!("{}/{}", self.covered, self.total)
        } else {
            format!("{}/{} ({})", self.covered, self.total, notes.join("; "))
        }
    }

    /// The `fsm` member: states and transitions together, then apart.
    fn fsm_json(&self, split: &[Tally; 2]) -> String {
        let mut j = self.json();
        j.pop();
        format!(
            "{}, \"states\": {}, \"transitions\": {}}}",
            j,
            split[0].json(),
            split[1].json()
        )
    }
}

impl Report {
    fn tallies(&self) -> Tallies {
        let mut t = Tallies::default();
        for s in &self.stmts {
            if s.excluded {
                t[0].excluded += 1;
                continue;
            }
            t[0].total += 1;
            t[0].covered += (s.count > 0) as u64;
        }
        for b in &self.branches {
            for a in &b.arms {
                if b.excluded || a.excluded {
                    t[1].excluded += 1;
                    continue;
                }
                t[1].total += 1;
                t[1].covered += (a.count > 0) as u64;
            }
        }
        for r in &self.toggles {
            t[2].total += 2 * r.rise.len() as u64;
            t[2].covered += r.rise.iter().filter(|&&c| c > 0).count() as u64
                + r.fall.iter().filter(|&&c| c > 0).count() as u64;
        }
        // A term is covered once both of its rows are.
        for (k, items) in [(3, &self.conds), (4, &self.exprs)] {
            for it in items {
                t[k].total += it.terms.len() as u64;
                t[k].covered += (0..it.terms.len()).filter(|&j| it.covered(j)).count() as u64;
            }
        }
        let [s, x] = self.fsm_split();
        t[KIND_FSM.trailing_zeros() as usize] = Tally {
            covered: s.covered + x.covered,
            total: s.total + x.total,
            excluded: 0,
        };
        t
    }

    /// FSM states and FSM transitions, the reference simulator's two bins.
    fn fsm_split(&self) -> [Tally; 2] {
        let mut t = [Tally::default(); 2];
        for f in &self.fsms {
            let [sc, st, tc, tt] = f.tally();
            t[0].add(Tally {
                covered: sc,
                total: st,
                excluded: 0,
            });
            t[1].add(Tally {
                covered: tc,
                total: tt,
                excluded: 0,
            });
        }
        t
    }

    /// Sort, and add up rows that name the same source construct (the
    /// copies a parameterized class or an expanded assignment makes).
    fn normalize(&mut self) {
        self.stmts.sort_by(|a, b| {
            (&a.file, a.line, a.start, a.kind).cmp(&(&b.file, b.line, b.start, b.kind))
        });
        let mut merged: Vec<StmtRow> = Vec::with_capacity(self.stmts.len());
        for s in self.stmts.drain(..) {
            match merged.last_mut() {
                Some(m) if m.file == s.file && m.start == s.start && m.kind == s.kind => {
                    add_count(&mut m.count, &mut m.excluded, s.count, s.excluded)
                }
                _ => merged.push(s),
            }
        }
        self.stmts = merged;
        self.branches.sort_by(|a, b| {
            (&a.file, a.line, a.start, a.kind).cmp(&(&b.file, b.line, b.start, b.kind))
        });
        let mut merged: Vec<BranchRow> = Vec::with_capacity(self.branches.len());
        for b in self.branches.drain(..) {
            match merged.last_mut() {
                Some(m)
                    if m.file == b.file
                        && m.start == b.start
                        && m.kind == b.kind
                        && m.arms.len() == b.arms.len() =>
                {
                    // Where both copies are excluded whole, an arm's own
                    // marker says whether it can run at all; otherwise an
                    // arm is out where its branch is.
                    let whole = m.excluded && b.excluded;
                    for (x, y) in m.arms.iter_mut().zip(&b.arms) {
                        let mut out = x.excluded || (m.excluded && !whole);
                        let other = y.excluded || (b.excluded && !whole);
                        add_count(&mut x.count, &mut out, y.count, other);
                        x.excluded = out;
                    }
                    m.excluded = whole;
                }
                _ => merged.push(b),
            }
        }
        self.branches = merged;
        self.toggles.sort_by(|a, b| a.name.cmp(&b.name));
        merge_items(&mut self.conds);
        merge_items(&mut self.exprs);
        self.fsms.sort_by(|a, b| a.key().cmp(&b.key()));
    }

    /// Add another instance of the same design unit (rows keyed by source
    /// position and signal name).
    fn merge(&mut self, o: &Report) {
        self.stmts.extend(o.stmts.iter().cloned());
        self.branches.extend(o.branches.iter().cloned());
        self.conds.extend(o.conds.iter().cloned());
        self.exprs.extend(o.exprs.iter().cloned());
        let mut by_name: HashMap<String, usize> = self
            .toggles
            .iter()
            .enumerate()
            .map(|(i, r)| (r.name.clone(), i))
            .collect();
        for r in &o.toggles {
            match by_name.get(&r.name) {
                Some(&i) if self.toggles[i].rise.len() == r.rise.len() => {
                    let t = &mut self.toggles[i];
                    for (a, b) in t.rise.iter_mut().zip(&r.rise) {
                        *a = a.saturating_add(*b);
                    }
                    for (a, b) in t.fall.iter_mut().zip(&r.fall) {
                        *a = a.saturating_add(*b);
                    }
                }
                _ => {
                    by_name.insert(r.name.clone(), self.toggles.len());
                    self.toggles.push(r.clone());
                }
            }
        }
        // Instances of one FSM in different parameterizations can differ in
        // their states: add to the row of the same shape, if any.
        for f in &o.fsms {
            let added = self
                .fsms
                .iter_mut()
                .filter(|m| m.key() == f.key())
                .any(|m| m.add(f));
            if !added {
                self.fsms.push(f.clone());
            }
        }
        self.normalize();
    }

    fn json(&self, kinds: u8, indent: &str, out: &mut String) {
        let t = self.tallies();
        let split = self.fsm_split();
        for (i, name) in KIND_NAMES.iter().enumerate() {
            if kinds & (1 << i) != 0 {
                let j = if 1 << i == KIND_FSM {
                    t[i].fsm_json(&split)
                } else {
                    t[i].json()
                };
                out.push_str(&format!("{}\"{}\": {},\n", indent, name, j));
            }
        }
        if kinds & KIND_STATEMENT != 0 {
            out.push_str(&format!("{}\"statements\": [", indent));
            for (i, s) in self.stmts.iter().enumerate() {
                out.push_str(&format!(
                    "{}{{\"file\": \"{}\", \"line\": {}, \"kind\": \"{}\", \"count\": {}{}}}",
                    if i == 0 { "" } else { ", " },
                    json_escape(&s.file),
                    s.line,
                    s.kind,
                    s.count,
                    excluded_json(s.excluded)
                ));
            }
            out.push_str("],\n");
        }
        if kinds & KIND_BRANCH != 0 {
            out.push_str(&format!("{}\"branches\": [", indent));
            for (i, b) in self.branches.iter().enumerate() {
                out.push_str(&format!(
                    "{}{{\"file\": \"{}\", \"line\": {}, \"kind\": \"{}\"{}, \"arms\": [",
                    if i == 0 { "" } else { ", " },
                    json_escape(&b.file),
                    b.line,
                    b.kind,
                    excluded_json(b.excluded)
                ));
                for (j, a) in b.arms.iter().enumerate() {
                    out.push_str(&format!(
                        "{}{{\"line\": {}, \"arm\": \"{}\", \"count\": {}{}}}",
                        if j == 0 { "" } else { ", " },
                        a.line,
                        a.label,
                        a.count,
                        excluded_json(a.excluded)
                    ));
                }
                out.push_str("]}");
            }
            out.push_str("],\n");
        }
        if kinds & KIND_TOGGLE != 0 {
            out.push_str(&format!("{}\"toggles\": [", indent));
            let list = |v: &[u32]| {
                v.iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            for (i, r) in self.toggles.iter().enumerate() {
                out.push_str(&format!(
                    "{}{{\"signal\": \"{}\", \"width\": {}, \"type\": \"{}\", \"rise\": [{}], \"fall\": [{}]}}",
                    if i == 0 { "" } else { ", " },
                    json_escape(&r.name),
                    r.rise.len(),
                    r.ty,
                    list(&r.rise),
                    list(&r.fall)
                ));
            }
            out.push_str("],\n");
        }
        if kinds & KIND_CONDITION != 0 {
            out.push_str(&format!("{}\"conditions\": [", indent));
            items_json(&self.conds, out);
            out.push_str("],\n");
        }
        if kinds & KIND_EXPRESSION != 0 {
            out.push_str(&format!("{}\"expressions\": [", indent));
            items_json(&self.exprs, out);
            out.push_str("],\n");
        }
        if kinds & KIND_FSM != 0 {
            out.push_str(&format!("{}\"fsms\": [", indent));
            for (i, f) in self.fsms.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                f.json(out);
            }
            out.push_str("],\n");
        }
        // Drop the trailing comma of the last member.
        if out.ends_with(",\n") {
            out.truncate(out.len() - 2);
            out.push('\n');
        }
    }
}

/// The data types tracked signals were declared with, for the `type` of
/// their toggle rows: the keyword of an integer type (§6.11: `logic`, `bit`,
/// `reg`, `byte`, `shortint`, `int`, `longint`, `integer`), or `enum`,
/// `struct` or `union`.
struct ToggleTypes<'a> {
    module: &'a ElaboratedModule,
    /// Instance path -> the definition it instantiates.
    defs: HashMap<&'a str, &'a str>,
    /// (definition, port) -> the data type of each ANSI port, read from the
    /// sources the first time one is needed (see `ansi_ports`).
    ports: std::cell::OnceCell<HashMap<(String, String), DataType>>,
}

impl ToggleTypes<'_> {
    /// The type of signal `name`. A typedef is followed to the type it
    /// names. A member of an unpacked struct or union takes the aggregate's
    /// kind, since the variable is the aggregate; a net declared without a
    /// data type is `logic` (§6.7.1), as is a type that does not resolve.
    fn of(&self, name: &str, type_name: Option<&str>) -> &'static str {
        let aggregate = name.match_indices('.').find_map(|(i, _)| {
            self.declared(&name[..i])
                .filter(|k| matches!(*k, "struct" | "union"))
        });
        aggregate
            .or_else(|| self.declared(name))
            .or_else(|| {
                type_name
                    .and_then(|t| self.module.typedef_types.get(t))
                    .and_then(|dt| self.kind(dt))
            })
            .unwrap_or("logic")
    }

    /// The type of the variable, net or port `name` as declared.
    /// Elaboration records it in `var_decl_types`, except for the ANSI
    /// ports of an instance below the top, which come from the sources.
    fn declared(&self, name: &str) -> Option<&'static str> {
        if let Some(dt) = self.module.var_decl_types.get(name) {
            return self.kind(dt);
        }
        let (inst, port) = name.rsplit_once('.')?;
        let def = self.defs.get(inst)?;
        self.ports
            .get_or_init(|| self.ansi_ports())
            .get(&(def.to_string(), port.to_string()))
            .and_then(|dt| self.kind(dt))
    }

    /// The keyword or kind of the type `dt` names.
    fn kind(&self, dt: &DataType) -> Option<&'static str> {
        use crate::ast::types::{IntegerVectorType, StructUnionKind};
        match crate::compiler::elaborate::resolve_typedef_chain(dt, &self.module.typedef_types) {
            DataType::IntegerAtom { kind, .. } => Some(match kind {
                IntegerAtomType::Byte => "byte",
                IntegerAtomType::ShortInt => "shortint",
                IntegerAtomType::Int => "int",
                IntegerAtomType::LongInt => "longint",
                IntegerAtomType::Integer => "integer",
                IntegerAtomType::Time => "time",
            }),
            DataType::IntegerVector { kind, .. } => Some(match kind {
                IntegerVectorType::Bit => "bit",
                IntegerVectorType::Logic => "logic",
                IntegerVectorType::Reg => "reg",
            }),
            DataType::Enum(_) => Some("enum"),
            DataType::Struct(su) if su.kind == StructUnionKind::Union => Some("union"),
            DataType::Struct(_) => Some("struct"),
            DataType::Implicit { .. } => Some("logic"),
            _ => None,
        }
    }

    /// The data types of the ANSI ports of every module, interface and
    /// program, parsed again from the preprocessed sources the way the VPI
    /// model is (a design loaded from a compiled artifact has none). A port
    /// with no direction, port kind or data type takes the previous one's
    /// (§23.2.2.3).
    fn ansi_ports(&self) -> HashMap<(String, String), DataType> {
        use crate::ast::Description;
        use crate::ast::decl::ModuleItem;
        use crate::ast::module::PortList;
        fn add(
            def: &str,
            ports: &PortList,
            items: &[ModuleItem],
            out: &mut HashMap<(String, String), DataType>,
        ) {
            if let PortList::Ansi(ps) = ports {
                let mut prev: Option<&DataType> = None;
                for p in ps {
                    let dt = match &p.data_type {
                        Some(dt) => Some(dt),
                        None if p.direction.is_none() && p.net_type.is_none() && !p.var_kw => prev,
                        None => None,
                    };
                    if let Some(dt) = dt {
                        out.entry((def.to_string(), p.name.name.clone()))
                            .or_insert_with(|| dt.clone());
                    }
                    prev = dt;
                }
            }
            for it in items {
                if let ModuleItem::NestedModule(m) = it {
                    add(&m.name.name, &m.ports, &m.items, out);
                }
            }
        }
        let mut out = HashMap::default();
        let mut scan = |text: &str| {
            let tokens = xezim_core::lexer::Lexer::new(text).tokenize();
            for d in &xezim_core::parse::Parser::new(tokens)
                .parse_source_text()
                .descriptions
            {
                match d {
                    Description::Module(m) => add(&m.name.name, &m.ports, &m.items, &mut out),
                    Description::Interface(i) => add(&i.name.name, &i.ports, &i.items, &mut out),
                    Description::Program(p) => add(&p.name.name, &p.ports, &p.items, &mut out),
                    _ => {}
                }
            }
        };
        for text in &self.module.source_texts {
            scan(text);
        }
        // `-v`/`-y` library files the elaboration adopted definitions from.
        for (path, _) in xezim_core::adopted_lib_files() {
            if let Some(text) = xezim_core::preprocess_adopted_lib(&path) {
                scan(&text);
            }
        }
        out
    }
}

/// The generate scopes of each module, interface or program, read from the
/// sources (§27.5): the names declared in an unlabeled branch of a generate
/// `if` or `case`, which elaboration keeps bare in the instance, and the
/// names of the generate blocks whose declarations are hierarchical (`g.x`,
/// `g[1].x`): labeled branches and loops. An unnamed loop's are
/// `genblk<n>`.
#[allow(clippy::type_complexity)]
fn generate_decls(
    module: &ElaboratedModule,
) -> HashMap<String, (HashSet<String>, HashSet<String>)> {
    use crate::ast::Description;
    use crate::ast::decl::ModuleItem;
    type Sets = (HashSet<String>, HashSet<String>);
    // Declarations in `items`, an unlabeled branch: bare names.
    fn names(items: &[ModuleItem], out: &mut Sets) {
        for it in items {
            match it {
                ModuleItem::DataDeclaration(dd) => out
                    .0
                    .extend(dd.declarators.iter().map(|d| d.name.name.clone())),
                ModuleItem::NetDeclaration(nd) => out
                    .0
                    .extend(nd.declarators.iter().map(|d| d.name.name.clone())),
                _ => constructs(std::slice::from_ref(it), out),
            }
        }
    }
    // The generate constructs among `items`, at any depth of regions.
    fn constructs(items: &[ModuleItem], out: &mut Sets) {
        for it in items {
            match it {
                ModuleItem::GenerateRegion(gr) => constructs(&gr.items, out),
                ModuleItem::GenerateIf(gi) => {
                    for (bi, (_, items)) in gi.branches.iter().enumerate() {
                        match gi.branch_labels.get(bi).and_then(|l| l.as_ref()) {
                            None => names(items, out),
                            Some(l) => {
                                out.1.insert(l.clone());
                            }
                        }
                    }
                }
                ModuleItem::GenerateCase(gc) => {
                    for arm in &gc.arms {
                        match &arm.label {
                            None => names(&arm.items, out),
                            Some(l) => {
                                out.1.insert(l.clone());
                            }
                        }
                    }
                }
                ModuleItem::GenerateFor(gf) => {
                    if let Some(l) = &gf.name {
                        out.1.insert(l.clone());
                    }
                }
                _ => {}
            }
        }
    }
    fn add(name: &str, items: &[ModuleItem], out: &mut HashMap<String, Sets>) {
        let mut sets = Sets::default();
        constructs(items, &mut sets);
        if !sets.0.is_empty() || !sets.1.is_empty() {
            let e = out.entry(name.to_string()).or_default();
            e.0.extend(sets.0);
            e.1.extend(sets.1);
        }
        for it in items {
            if let ModuleItem::NestedModule(m) = it {
                add(&m.name.name, &m.items, out);
            }
        }
    }
    let mut out = HashMap::default();
    let mut scan = |text: &str| {
        let tokens = xezim_core::lexer::Lexer::new(text).tokenize();
        for d in &xezim_core::parse::Parser::new(tokens)
            .parse_source_text()
            .descriptions
        {
            match d {
                Description::Module(m) => add(&m.name.name, &m.items, &mut out),
                Description::Interface(i) => add(&i.name.name, &i.items, &mut out),
                Description::Program(p) => add(&p.name.name, &p.items, &mut out),
                _ => {}
            }
        }
    };
    for text in &module.source_texts {
        scan(text);
    }
    for (path, _) in xezim_core::adopted_lib_files() {
        if let Some(text) = xezim_core::preprocess_adopted_lib(&path) {
            scan(&text);
        }
    }
    out
}

impl Simulator {
    /// Instrument the design for the code coverage the command line asked
    /// for. Runs first in `compile`, before any block is lowered.
    pub(super) fn instrument_code_coverage(&mut self) {
        let Some(cfg) = code_coverage() else {
            return;
        };
        // Every always body is instrumented as a tree (normally none is
        // lazy under coverage; this covers a configuration set late).
        self.flush_deferred_always();
        let kinds = cfg.kinds;
        let inst_paths: HashSet<String> = self
            .module
            .instances
            .iter()
            .map(|i| i.path.clone())
            .collect();
        let mut cov = CodeCov {
            kinds,
            keep_constant: cfg.keep_constant,
            logical: cfg.logical,
            blocks: cfg.blocks,
            limit: cfg.scopes,
            scopes: Vec::new(),
            scope_index: HashMap::default(),
            stmts: Vec::new(),
            branches: Vec::new(),
            exprs: Vec::new(),
            rets: Arc::new(self.cov_return_widths()),
            counters: 0,
            ca_counter: HashMap::default(),
            inst_paths: inst_paths.clone(),
            decisions: Vec::new(),
            toggle: None,
            fsm: None,
            steps: Vec::new(),
            writers: HashMap::default(),
            clocked: Vec::new(),
            inverters: Vec::new(),
            assigned: Vec::new(),
            pending: HashMap::default(),
            quiet: 0,
        };
        // Initial blocks and continuous assignments are instrumented where
        // `compile` and `build_comb_entries` take them (`cov_initial`,
        // `cov_contassign`): most of them are still lazy here, and building
        // them early would change the order and the passes they go through.
        let mut always = std::mem::take(&mut self.module.always_blocks);
        // FSMs are found on the blocks as elaborated; their markers go in
        // after the other kinds have instrumented the blocks.
        if kinds & KIND_FSM != 0 {
            cov.fsm = Some(self.fsm_extract(&mut cov, &inst_paths, &always));
        }
        if kinds & KIND_CODE != 0 {
            let mut program = std::mem::take(&mut self.module.program_initial_blocks);
            let mut fin = std::mem::take(&mut self.module.final_blocks);
            let mut functions = std::mem::take(&mut self.module.functions).into_map();
            let mut tasks = std::mem::take(&mut self.module.tasks);
            {
                let ctx = self.cov_ctx(&cov);
                // Parameters make conditions constant, but not in a package's
                // subroutines, whose names are not the instances'.
                let consts = Some(&self.module);
                let subr_consts = |key: &str| {
                    let pkg = key.contains("::") || self.module.pkg_subr_owner.contains_key(key);
                    (!pkg).then_some(&self.module)
                };
                for ib in program.iter_mut().chain(fin.iter_mut()) {
                    if let Some(sc) =
                        Self::cov_instance_scope(&self.module, &mut cov, &inst_paths, &ib.scope)
                    {
                        let span = ib.stmt.span;
                        let s = std::mem::replace(
                            &mut ib.stmt,
                            Statement::new(StatementKind::Null, span),
                        );
                        ib.stmt = Instr::new(&mut cov, &ctx, &inst_paths, sc, &ib.scope, consts)
                            .wrap(s);
                    }
                }
                for ab in always.iter_mut() {
                    let kind = match ab.kind {
                        crate::ast::decl::AlwaysKind::Always => "always",
                        crate::ast::decl::AlwaysKind::AlwaysComb => "always_comb",
                        crate::ast::decl::AlwaysKind::AlwaysFf => "always_ff",
                        crate::ast::decl::AlwaysKind::AlwaysLatch => "always_latch",
                    };
                    // What clocked blocks write (see `cov_steps`).
                    if kinds & (KIND_CONDITION | KIND_EXPRESSION) != 0
                        && combinational(kind, &ab.stmt) == Some(false)
                    {
                        let body = match &ab.stmt.kind {
                            StatementKind::TimingControl { stmt, .. } => stmt,
                            _ => &ab.stmt,
                        };
                        let mut writes = HashSet::default();
                        Self::collect_stmt_rw(body, &self.module, None, &mut writes, None);
                        cov.clocked.push((ab.scope.clone(), writes));
                    }
                    if let Some(sc) =
                        Self::cov_instance_scope(&self.module, &mut cov, &inst_paths, &ab.scope)
                    {
                        let span = ab.stmt.span;
                        let s = std::mem::replace(
                            &mut ab.stmt,
                            Statement::new(StatementKind::Null, span),
                        );
                        ab.stmt =
                            Instr::new(&mut cov, &ctx, &inst_paths, sc, &ab.scope, consts)
                                .always(s, kind);
                    }
                }
                // Subroutines: a package's belong to the package, the others
                // to the instance their qualified name starts with.
                // A bare-named copy of an instance's subroutine (same source
                // span) is not the top module's own.
                let dotted: HashSet<(String, usize)> = functions
                    .iter()
                    .map(|(k, f)| (k, f.span))
                    .chain(tasks.iter().map(|(k, t)| (k, t.span)))
                    .filter_map(|(k, sp)| {
                        let (_, leaf) = k.rsplit_once('.')?;
                        Some((leaf.to_string(), sp.start))
                    })
                    .collect();
                let copy = |k: &str, sp: Span| {
                    !k.contains('.')
                        && !k.contains("::")
                        && dotted.contains(&(k.to_string(), sp.start))
                };
                for (key, fd) in functions.iter_mut() {
                    if copy(key, fd.span) && !self.module.pkg_subr_owner.contains_key(key.as_str())
                    {
                        continue;
                    }
                    if let Some(sc) = self.cov_subroutine_scope(&mut cov, &inst_paths, key) {
                        let items = std::mem::take(&mut fd.items);
                        let mut ins =
                            Instr::new(&mut cov, &ctx, &inst_paths, sc, key, subr_consts(key))
                                .args(&fd.ports, &inst_paths);
                        // The function's name is its result variable.
                        let ret = ctx.local(&fd.return_type, false);
                        ins.here.locals.insert(fd.name.name.name.clone(), ret);
                        fd.items = ins.stmts(items);
                    }
                }
                for (key, td) in tasks.iter_mut() {
                    if copy(key, td.span) && !self.module.pkg_subr_owner.contains_key(key.as_str())
                    {
                        continue;
                    }
                    if let Some(sc) = self.cov_subroutine_scope(&mut cov, &inst_paths, key) {
                        let items = std::mem::take(&mut td.items);
                        td.items =
                            Instr::new(&mut cov, &ctx, &inst_paths, sc, key, subr_consts(key))
                                .args(&td.ports, &inst_paths)
                            .stmts(items);
                    }
                }
            }
            self.module.program_initial_blocks = program;
            self.module.final_blocks = fin;
            self.module.functions = xezim_core::elaborate::FuncTable::from_map(functions);
            self.module.tasks = tasks;
            self.cov_instrument_classes(&mut cov);
        }
        if let Some(f) = &cov.fsm {
            Self::fsm_instrument(f, &mut always);
        }
        self.module.always_blocks = always;
        if kinds & KIND_TOGGLE != 0 {
            let names = std::mem::take(&mut self.code_cov_toggle_names);
            let names = self.cov_drop_fixed(names);
            cov.toggle = Some(self.cov_build_toggle(&mut cov, &inst_paths, names));
            self.enable_dump_dirty_tracking();
        }
        // FSM state processes watch their inputs off the same dirty set.
        if cov.fsm.as_ref().is_some_and(|f| !f.fsms.is_empty()) {
            self.enable_dump_dirty_tracking();
        }
        self.code_cov_hits = vec![0; cov.counters as usize];
        self.code_cov = Some(Box::new(cov));
    }

    fn cov_ctx(&self, cov: &CodeCov) -> Ctx<'_> {
        Ctx {
            module: &self.module,
            names: &self.signal_name_to_id,
            widths: &self.signal_widths,
            rets: cov.rets.clone(),
        }
    }

    /// The width of each function's return type, by the name a call uses
    /// (`call_return_width` resolves calls the same way).
    fn cov_return_widths(&self) -> HashMap<String, u32> {
        self.module
            .functions
            .iter()
            .filter(|(_, f)| {
                !matches!(f.return_type, DataType::Void { .. } | DataType::Real { .. })
            })
            .map(|(k, f)| {
                let w = crate::compiler::elaborate::resolve_type_width(
                    &f.return_type,
                    Some(&self.module.parameters),
                    Some(&self.module.typedefs),
                );
                (k.clone(), w)
            })
            .filter(|(_, w)| *w > 0)
            .collect()
    }

    /// An initial block elaboration made rather than the source: a
    /// variable declaration's initializer, or a module-level assertion.
    fn cov_synthesized_initial(s: &Statement) -> bool {
        let dummy = |sp: Span| sp.start == 0 && sp.end == 0;
        match &s.kind {
            _ if dummy(s.span) => true,
            StatementKind::BlockingAssign { lvalue, .. } => dummy(lvalue.span),
            StatementKind::Assertion(a) => a.is_property || a.deferred.is_some(),
            _ => false,
        }
    }

    /// Instrument an initial block as `compile` schedules it.
    pub(super) fn cov_initial(&mut self, mut ib: InitialBlock) -> InitialBlock {
        let Some(mut cov) = self.code_cov.take() else {
            return ib;
        };
        let kinds = cov.kinds;
        if kinds & KIND_CODE != 0 && !Self::cov_synthesized_initial(&ib.stmt) {
            let paths = std::mem::take(&mut cov.inst_paths);
            if let Some(sc) = Self::cov_instance_scope(&self.module, &mut cov, &paths, &ib.scope) {
                let span = ib.stmt.span;
                let s = std::mem::replace(&mut ib.stmt, Statement::new(StatementKind::Null, span));
                let ctx = self.cov_ctx(&cov);
                ib.stmt = Instr::new(&mut cov, &ctx, &paths, sc, &ib.scope, Some(&self.module))
                    .wrap(s);
            }
            cov.inst_paths = paths;
            self.code_cov_hits.resize(cov.counters as usize, 0);
        }
        self.code_cov = Some(cov);
        ib
    }

    /// Instrument a continuous assignment as `build_comb_entries` takes it:
    /// its value becomes an expression item or its `?:`s branches, and it
    /// gets its statement counter (shared by the pieces one assignment
    /// expands to), which is returned. An input port's connection to an
    /// expression (`.en(a & b)`) is only an expression item, of the parent
    /// instance. Takes the fields it needs: the caller's iterator holds
    /// `module`.
    pub(super) fn cov_contassign(
        module: &ElaboratedModule,
        names: &NameMap,
        widths: &[u32],
        code_cov: &mut Option<Box<CodeCov>>,
        hits: &mut Vec<u64>,
        ca: &mut crate::compiler::elaborate::ContinuousAssignment,
    ) -> Option<u32> {
        let cov = code_cov.as_mut()?;
        let kinds = cov.kinds;
        let (span, scope, port) = match ca.origin.clone() {
            Some((span, scope)) if scope.is_empty() => {
                let scope = Self::cov_lhs_base(&ca.lhs)
                    .and_then(|n| n.rsplit_once('.').map(|(p, _)| p.to_string()))
                    .unwrap_or_default();
                (span, scope, false)
            }
            Some((span, scope)) => (span, scope, false),
            None if ca.rhs_parent_scoped && kinds & KIND_EXPRESSION != 0 => {
                let base = Self::cov_lhs_base(&ca.lhs)?;
                let base = base
                    .strip_prefix(&format!("{}.", module.name))
                    .unwrap_or(&base);
                let inst = base.rsplit_once('.')?.0;
                let parent = inst.rsplit_once('.').map(|(p, _)| p).unwrap_or("");
                (ca.rhs.span, parent.to_string(), true)
            }
            None => return None,
        };
        // The target of an `assign` (see `cov_steps`).
        if !port && kinds & (KIND_CONDITION | KIND_EXPRESSION) != 0 {
            if let Some(n) = Self::cov_lhs_base(&ca.lhs) {
                cov.assigned.push((n, scope.clone()));
            }
        }
        // An inverter, `assign n = ~x` (see `cov_steps`).
        if let (false, true, ExprKind::Ident(_), ExprKind::Unary { op, operand }) = (
            port,
            kinds & (KIND_CONDITION | KIND_EXPRESSION) != 0,
            &ca.lhs.kind,
            &unparen(&ca.rhs).kind,
        ) {
            if let (UnaryOp::BitNot | UnaryOp::LogNot, ExprKind::Ident(h)) =
                (op, &unparen(operand).kind)
            {
                if let Some(n) = Self::cov_lhs_base(&ca.lhs) {
                    let x = Self::resolve_hier_name_static(h, module);
                    cov.inverters.push((n, x, scope.clone()));
                }
            }
        }
        let paths = std::mem::take(&mut cov.inst_paths);
        let sc = Self::cov_instance_scope(module, cov, &paths, &scope);
        let ctx = Ctx {
            module,
            names,
            widths,
            rets: cov.rets.clone(),
        };
        let counter = sc.map(|sc| {
            let mut ins = Instr::new(cov, &ctx, &paths, sc, &scope, Some(module));
            ins.hint = Some((!scope.is_empty()).then(|| scope.clone()));
            if port {
                ins.value(&mut ca.rhs, &ca.lhs, "port", false);
                return None;
            }
            ins.value(&mut ca.rhs, &ca.lhs, "assign", true);
            // In block granularity a continuous assignment is no block.
            (kinds & KIND_STATEMENT != 0 && !ins.cov.blocks).then(|| {
                let key = (span.start, span.end, scope.clone());
                match ins.cov.ca_counter.get(&key) {
                    Some(&c) => c,
                    None => {
                        let c = ins.counter();
                        ins.cov.stmts.push(StmtSite {
                            scope: sc,
                            span,
                            kind: "assign",
                            counter: c,
                            excluded: false,
                        });
                        ins.cov.ca_counter.insert(key, c);
                        c
                    }
                }
            })
        });
        cov.inst_paths = paths;
        hits.resize(cov.counters as usize, 0);
        counter.flatten()
    }

    /// The signal-name base of a continuous assignment's target.
    fn cov_lhs_base(e: &Expression) -> Option<String> {
        match &e.kind {
            ExprKind::Ident(h) => Some(
                h.path
                    .iter()
                    .map(|s| s.name.name.as_str())
                    .collect::<Vec<_>>()
                    .join("."),
            ),
            ExprKind::Index { expr, .. }
            | ExprKind::RangeSelect { expr, .. }
            | ExprKind::MemberAccess { expr, .. }
            | ExprKind::Paren(expr) => Self::cov_lhs_base(expr),
            _ => None,
        }
    }

    /// The instance a block scope (`u0.u1.genblk2`) belongs to: its longest
    /// prefix that names an instance, else the top. `None` when the command
    /// line's scope limit leaves it out.
    pub(super) fn cov_instance_scope(
        module: &ElaboratedModule,
        cov: &mut CodeCov,
        inst_paths: &HashSet<String>,
        scope: &str,
    ) -> Option<u32> {
        let mut path = scope;
        let inst = loop {
            if path.is_empty() || inst_paths.contains(path) {
                break path;
            }
            match path.rsplit_once('.') {
                Some((p, _)) => path = p,
                None => path = "",
            }
        };
        let wrapper = module.name == xezim_core::MULTI_TOP_WRAPPER;
        let name = if wrapper {
            inst.to_string()
        } else if inst.is_empty() {
            module.name.clone()
        } else {
            format!("{}.{}", module.name, inst)
        };
        if name.is_empty() {
            return None;
        }
        if !cov.limit.is_empty()
            && !cov.limit.iter().any(|l| {
                name == *l
                    || name.starts_with(&format!("{}.", l))
                    || (!wrapper && inst == l.as_str())
                    || (!wrapper && inst.starts_with(&format!("{}.", l)))
            })
        {
            return None;
        }
        let key = format!("i:{}", inst);
        if let Some(&i) = cov.scope_index.get(&key) {
            return Some(i);
        }
        let unit = if inst.is_empty() {
            module.name.clone()
        } else {
            module
                .instances
                .iter()
                .find(|i| i.path == inst)
                .map(|i| i.def_name.clone())
                .unwrap_or_default()
        };
        let src = module.src_file_of_module.get(&unit).copied();
        let i = cov.scopes.len() as u32;
        cov.scopes.push(Scope {
            name,
            unit,
            kind: "instance",
            src,
        });
        cov.scope_index.insert(key, i);
        Some(i)
    }

    /// A scope outside the instance tree: a package, `$unit`, or the module
    /// that declares a class. Library packages count only when named.
    fn cov_package_scope(
        &self,
        cov: &mut CodeCov,
        name: &str,
        src_key: &str,
        kind: &'static str,
    ) -> Option<u32> {
        let listed = cov.limit.iter().any(|l| l == name);
        if !cov.limit.is_empty() && !listed {
            return None;
        }
        if !listed && LIBRARY_PACKAGES.contains(&name) {
            return None;
        }
        let key = format!("p:{}", name);
        if let Some(&i) = cov.scope_index.get(&key) {
            return Some(i);
        }
        let src = self
            .module
            .src_file_of_module
            .get(src_key)
            .or_else(|| self.module.src_file_of_module.get(name))
            .copied();
        let i = cov.scopes.len() as u32;
        cov.scopes.push(Scope {
            name: name.to_string(),
            unit: name.to_string(),
            kind,
            src,
        });
        cov.scope_index.insert(key, i);
        Some(i)
    }

    fn cov_subroutine_scope(
        &self,
        cov: &mut CodeCov,
        inst_paths: &HashSet<String>,
        key: &str,
    ) -> Option<u32> {
        if let Some((pkg, _)) = key.split_once("::") {
            return self.cov_package_scope(cov, pkg, pkg, "package");
        }
        if let Some(pkg) = self.module.pkg_subr_owner.get(key) {
            let pkg = pkg.clone();
            return self.cov_package_scope(cov, &pkg, &pkg, "package");
        }
        let scope = key.rsplit_once('.').map(|(p, _)| p).unwrap_or("");
        Self::cov_instance_scope(&self.module, cov, inst_paths, scope)
    }

    /// Make the decisions constants make (see `Decision`), with the
    /// evaluator the run uses, so a decision is what the run does: exclude
    /// the arms that cannot run and what they hold, and the whole branch
    /// when the decision is constant. Runs at the end of `compile`, once
    /// every block is instrumented.
    pub(super) fn cov_resolve_constants(&mut self) {
        let Some(mut cov) = self.code_cov.take() else {
            return;
        };
        for d in std::mem::take(&mut cov.decisions) {
            // Per arm, and whether the decision is constant as a whole.
            let (fates, whole, conds) = match &d.choice {
                Choice::If(conds) => {
                    let truths: Vec<_> = conds.iter().map(|c| self.cov_truth(c)).collect();
                    let (fates, whole) = chain_fates(&truths);
                    // A condition after a constantly true one is never
                    // evaluated.
                    let dead = (0..conds.len())
                        .map(|i| fates[..i].contains(&Fate::Taken))
                        .collect();
                    (fates, whole, dead)
                }
                Choice::Case(kind, sel, items) => match self.cov_case_fates(kind, sel, items) {
                    Some(fates) => (fates, true, Vec::new()),
                    None => continue,
                },
                Choice::Ternary(c) => match self.cov_truth(c) {
                    Some(t) => {
                        let (yes, no) = if t {
                            (Fate::Taken, Fate::Dead)
                        } else {
                            (Fate::Dead, Fate::Taken)
                        };
                        (vec![yes, no], true, Vec::new())
                    }
                    None => continue,
                },
                Choice::Operand(c, skip) => match self.cov_truth(c) {
                    Some(t) if t == *skip => (vec![Fate::Dead], false, Vec::new()),
                    _ => continue,
                },
            };
            let mut dead = |(s, b, e): &Region| {
                for x in &mut cov.stmts[s.clone()] {
                    x.excluded = true;
                }
                for x in &mut cov.branches[b.clone()] {
                    x.excluded = true;
                }
                // The condition and expression items of code that cannot run.
                for x in &mut cov.exprs[e.clone()] {
                    x.excluded = true;
                }
            };
            for (f, r) in fates.iter().zip(&d.arms) {
                if *f == Fate::Dead {
                    dead(r);
                }
            }
            for (out, c) in conds.iter().zip(&d.conds) {
                if *out {
                    dead(c);
                }
            }
            if let Some(site) = d.site.and_then(|i| cov.branches.get_mut(i)) {
                site.excluded |= whole;
                for (a, f) in site.arms.iter_mut().zip(&fates) {
                    a.excluded |= *f == Fate::Dead;
                }
            }
        }
        self.code_cov = Some(cov);
    }

    /// Whether a condition's constant parts make it constantly true or
    /// false, as the run evaluates them (`Value::is_nonzero`: x or z bits
    /// and no 1 decide nothing).
    fn cov_truth(&mut self, c: &Cond) -> Option<bool> {
        match c {
            Cond::Const(e) => self.eval_expr(e).is_nonzero(),
            Cond::Not(c) => self.cov_truth(c).map(|t| !t),
            Cond::And(l, r, bitwise) => match (self.cov_truth(l), self.cov_truth(r)) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) if !bitwise => Some(true),
                _ => None,
            },
            Cond::Or(l, r, bitwise) => match (self.cov_truth(l), self.cov_truth(r)) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) if !bitwise => Some(false),
                _ => None,
            },
            Cond::Unknown => None,
        }
    }

    /// The fates of a constant `case`'s items and of its implicit
    /// `default`, matching the items as the run does: the first item that
    /// matches is taken, or the `default` (written or implicit) when none
    /// does. `None` when a comparison has x or z bits.
    fn cov_case_fates(
        &mut self,
        kind: &CaseKind,
        sel: &Expression,
        items: &[CaseItem],
    ) -> Option<Vec<Fate>> {
        let val = self.case_selector_value(kind, sel, items);
        let mut hit = None;
        for (i, it) in items.iter().enumerate() {
            for p in &it.patterns {
                let pv = self.eval_expr(p);
                let m = match kind {
                    CaseKind::Casez => val.casez_eq(&pv),
                    CaseKind::Casex => val.casex_eq(&pv),
                    _ => val.case_eq(&pv),
                };
                if m.is_nonzero()? && hit.is_none() {
                    hit = Some(i);
                }
            }
        }
        let hit = hit.or_else(|| items.iter().position(|it| it.is_default));
        let mut fates: Vec<Fate> = (0..items.len())
            .map(|i| {
                if Some(i) == hit {
                    Fate::Taken
                } else {
                    Fate::Dead
                }
            })
            .collect();
        fates.push(if hit.is_some() {
            Fate::Dead
        } else {
            Fate::Taken
        });
        Some(fates)
    }

    fn cov_instrument_classes(&mut self, cov: &mut CodeCov) {
        let mut classes = std::mem::take(&mut self.module.classes);
        let mut done: HashMap<usize, Arc<crate::compiler::elaborate::ElaboratedClass>> =
            HashMap::default();
        let mut keys: Vec<String> = classes.keys().cloned().collect();
        keys.sort();
        for key in keys {
            let Some(arc) = classes.get(&key) else {
                continue;
            };
            let ptr = Arc::as_ptr(arc) as usize;
            if let Some(same) = done.get(&ptr) {
                classes.insert(key, same.clone());
                continue;
            }
            let (scope_name, src_key, kind) = match self.module.class_decl_pkg.get(&arc.name) {
                Some(p) => (p.clone(), p.clone(), "package"),
                None => match &arc.declaring_module {
                    Some(m) => (m.clone(), m.clone(), "module"),
                    None => ("$unit".to_string(), arc.name.clone(), "unit"),
                },
            };
            let Some(sc) = self.cov_package_scope(cov, &scope_name, &src_key, kind) else {
                continue;
            };
            if scope_name == "$unit" || cov.scopes[sc as usize].src.is_none() {
                // A file-scope class: its own entry locates its methods.
                if let Some(&f) = self.module.src_file_of_module.get(&arc.name) {
                    if cov.scopes[sc as usize].src.is_none() {
                        cov.scopes[sc as usize].src = Some(f);
                    }
                }
            }
            let mut arc = classes.remove(&key).unwrap();
            let cls = Arc::make_mut(&mut arc);
            let mut names: Vec<String> = cls.methods.keys().cloned().collect();
            names.sort();
            for m in names {
                let Some(method) = Arc::make_mut(&mut cls.methods).get_mut(&m) else {
                    continue;
                };
                let (items, ports) = match &mut method.kind {
                    crate::ast::decl::ClassMethodKind::Function(fd)
                    | crate::ast::decl::ClassMethodKind::Extern(fd) => (&mut fd.items, &fd.ports),
                    crate::ast::decl::ClassMethodKind::Task(td) => (&mut td.items, &td.ports),
                    crate::ast::decl::ClassMethodKind::PureVirtual(_) => continue,
                };
                let body = std::mem::take(items);
                let ctx = self.cov_ctx(cov);
                let none = HashSet::default();
                *items = Instr::new(&mut *cov, &ctx, &none, sc, "", None)
                    .args(ports, &none)
                    .stmts(body);
            }
            done.insert(ptr, arc.clone());
            classes.insert(key, arc);
        }
        self.module.classes = classes;
    }

    /// Count counter `c` (`Insn::CovHit`). Out of line, so the executors'
    /// hot loops compile as they do without code coverage.
    #[inline(never)]
    pub(super) fn cov_count(&mut self, c: u32) {
        if c >= FSM_MARK {
            return self.fsm_mark(c - FSM_MARK);
        }
        if let Some(h) = self.code_cov_hits.get_mut(c as usize) {
            *h += 1;
        }
    }

    /// `TsInsn::CovHit`: count, and note the count on the save list so a
    /// bail takes it back (`ts_restore_saved`) before the block runs again.
    /// FSM markers need no undoing: they repeat the same marks on the rerun.
    #[inline(never)]
    pub(super) fn ts_cov_hit(&mut self, c: u32) {
        self.cov_count(c);
        if c < FSM_MARK {
            self.ts_save_list.push((c | TS_SAVE_COV, 0, 0));
        }
    }

    /// Count a coverage marker's counter (interpreter side).
    pub(super) fn cov_hit(&mut self, arg: Option<&Expression>) {
        if let Some(c) = arg.and_then(marker_id) {
            self.cov_count(c);
        }
    }

    /// `$__xz_covc(<n>, cond)`: count the condition's outcome, yield it.
    pub(super) fn cov_cond(&mut self, args: &[Expression]) -> Value {
        let Some(cond) = args.get(1) else {
            return Value::new(1);
        };
        let v = self.eval_expr(cond);
        if let Some(id) = args.first().and_then(marker_id).filter(|_| !self.cov_quiet()) {
            let slot = match v.is_nonzero() {
                Some(true) => Some(id),
                Some(false) => Some(id + 1),
                None => None,
            };
            if let Some(c) = slot.and_then(|s| self.code_cov_hits.get_mut(s as usize)) {
                *c += 1;
            }
        }
        v
    }

    /// `$__xz_cove(<n>, expr)`: evaluate condition or expression item `n`
    /// node by node, as the interpreter evaluates `expr`, count the rows of
    /// its terms that decided the result, and yield the value.
    pub(super) fn cov_expr(&mut self, args: &[Expression]) -> Value {
        let Some(e) = args.get(1) else {
            return Value::new(1);
        };
        let site = args.first().and_then(marker_id).and_then(|id| {
            let s = self.code_cov.as_ref()?.exprs.get(id as usize)?;
            let replay = s.hint.is_some() || s.proc.is_some();
            Some((id, s.nodes.clone(), (s.row0, s.bimodal), s.hint.clone(), replay))
        });
        let Some((id, nodes, row0, hint, replay)) = site.filter(|_| !self.cov_quiet()) else {
            return self.eval_expr(e);
        };
        // A compiled continuous assignment reaches here through an
        // expression fallback, which leaves name resolution to the
        // interpreter but does not set its scope: resolve in the
        // assignment's own, and put the caller's back (resolution moves it),
        // so no later process resolves its names in this one's scope.
        let saved = hint.map(|h| self.name_resolve_hint.replace(h));
        let mut vals = vec![UNEVALUATED; nodes.len()];
        let v = self.cov_node(&nodes, 0, e, &mut vals);
        let terms = if replay {
            self.cov_terms(id, &nodes, e, &vals)
        } else {
            None
        };
        if let Some(s) = saved {
            *self.name_resolve_hint.borrow_mut() = s;
        }
        // A combinational block run while a process is still writing (the
        // settle after one of its blocking writes): the reference simulator
        // runs it once the process waits, on what the process wrote by then.
        // The evaluations of its last such run count then (`cov_flush`).
        // (Any settle inside a process is such a one: an interpreted write in
        // the block clears `proc_settle_defer` for the block's later
        // statements.)
        if self.proc_settle_defer || (self.settling && self.proc_depth > 0) {
            let (pid, run) = (self.current_pid, self.settle_calls);
            // (Another process's evaluations count first.)
            if self
                .code_cov
                .as_ref()
                .and_then(|c| c.pending.get(&id))
                .is_some_and(|p| p.pid != pid)
            {
                self.cov_flush_item(id);
            }
            // The process's exit counts them (`drain_deferred_comb`, which this
            // mark wakes, so a run without coverage checks nothing more).
            if self.code_cov.as_ref().is_some_and(|c| c.pending.is_empty()) {
                self.deferred_comb.push((usize::MAX, Vec::new()));
            }
            if let Some(c) = self.code_cov.as_mut() {
                // A later run of the block replaces an earlier one; a loop in
                // it evaluates the item more than once in one run.
                let p = c.pending.entry(id).or_insert(Pending {
                    pid,
                    run,
                    evals: Vec::new(),
                });
                if p.run != run {
                    p.run = run;
                    p.evals.clear();
                }
                p.evals.push((vals, terms));
            }
            return v;
        }
        // (Deferred evaluations this one starts again: the block runs again
        // on the same inputs, and counts now.)
        if let Some(c) = self.code_cov.as_mut() {
            if c.pending
                .get(&id)
                .and_then(|p| p.evals.first())
                .is_some_and(|e| e.0 == vals)
            {
                c.pending.remove(&id);
            }
        }
        self.cov_flush_item(id);
        self.cov_count_item(id, &nodes, row0, &vals, terms);
        v
    }

    /// Count an evaluation of item `id` (`vals`), with the states its
    /// terms (`terms`, from `cov_terms`) passed through before it.
    fn cov_count_item(
        &mut self,
        id: u32,
        nodes: &[Node],
        row0: (u32, bool),
        vals: &[u8],
        terms: Option<(Vec<u8>, Vec<u8>)>,
    ) {
        if let Some((cur, konst)) = terms {
            self.cov_replay(id, nodes, cur, &konst);
        }
        self.cov_rows(nodes, vals, row0);
    }

    /// Count the deferred evaluations of item `id`, if any, in order.
    fn cov_flush_item(&mut self, id: u32) {
        let Some(c) = self.code_cov.as_mut() else {
            return;
        };
        if c.pending.is_empty() {
            return;
        }
        let Some(p) = c.pending.remove(&id) else {
            return;
        };
        let s = &c.exprs[id as usize];
        let (nodes, row0) = (s.nodes.clone(), (s.row0, s.bimodal));
        for (vals, terms) in p.evals {
            self.cov_count_item(id, &nodes, row0, &vals, terms);
        }
    }

    /// Inside a read that only coverage makes.
    fn cov_quiet(&self) -> bool {
        self.code_cov.as_ref().is_some_and(|c| c.quiet > 0)
    }

    /// Run `f`, a read that only coverage makes (a term or operand the
    /// design skipped), counting nothing: the branch counters and items
    /// inside it are not executed by the design.
    fn cov_quietly<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        if let Some(c) = self.code_cov.as_mut() {
            c.quiet += 1;
        }
        let v = f(self);
        if let Some(c) = self.code_cov.as_mut() {
            c.quiet -= 1;
        }
        v
    }

    /// Count every deferred evaluation (see `cov_expr`): when a process
    /// waits, at the end of a time slot, and before reporting.
    pub(super) fn cov_flush(&mut self) {
        let ids: Vec<u32> = match self.code_cov.as_ref() {
            Some(c) => c.pending.keys().copied().collect(),
            None => return,
        };
        for id in ids {
            self.cov_flush_item(id);
        }
    }

    /// Count the rows of the terms that decide an item's value, given its
    /// node values (`row0`: the item's first counter, and whether it is
    /// bimodal: then by the item's value, and not when that is unknown).
    fn cov_rows(&mut self, nodes: &[Node], vals: &[u8], row0: (u32, bool)) {
        let (row0, bimodal) = (row0.0 as usize, row0.1);
        let out = vals[0] as usize;
        if bimodal && out > 1 {
            return;
        }
        let mut rows = [0u8; MAX_TERMS];
        if self.code_cov.as_ref().is_some_and(|c| c.logical) {
            sop(nodes, vals, &mut rows);
        } else {
            sensitize(nodes, 0, vals, &mut rows);
        }
        for (t, r) in rows.iter().enumerate() {
            for b in 0..2 {
                if r & (1 << b) != 0 {
                    let c = if bimodal {
                        row0 + 4 * t + 2 * b + out
                    } else {
                        row0 + 2 * t + b
                    };
                    if let Some(h) = self.code_cov_hits.get_mut(c) {
                        *h += 1;
                    }
                }
            }
        }
    }

    /// A continuous assignment's item, or one at the top level of a
    /// combinational `always` block, after evaluation `vals`: count the
    /// states its inputs passed through since the last one. The reference
    /// simulator evaluates the item at each of its inputs' changes, so also
    /// in a delta cycle where one input has its new value and another not
    /// yet, while xezim settles combinational logic in dependency order and
    /// evaluates it once all have theirs. The terms that changed are
    /// replayed in the order they settle in (`cov_term_steps`), where that
    /// order is certain whichever signals upstream changed (`order`): after
    /// each group but the last, its terms and the earlier ones have their
    /// new values and the others their old ones.
    fn cov_replay(&mut self, id: u32, nodes: &[Node], cur: Vec<u8>, konst: &[u8]) {
        let Some(site) = self
            .code_cov
            .as_mut()
            .and_then(|c| c.exprs.get_mut(id as usize))
        else {
            return;
        };
        let (n, row0) = (site.terms.len(), (site.row0, site.bimodal));
        let last = std::mem::replace(&mut site.last, cur.clone());
        let Some(steps) = site.steps.as_ref().filter(|_| last.len() == n) else {
            return;
        };
        let groups = order(steps, (0..n).filter(|&t| cur[t] != last[t]).collect());
        // (The last group's state is the evaluation itself.)
        let states: Vec<Vec<u8>> = (0..groups.last().map_or(0, |g| g.1))
            .map(|k| {
                let mut state = last.clone();
                for &(t, _) in groups.iter().filter(|g| g.1 <= k) {
                    state[t] = cur[t];
                }
                state
            })
            .collect();
        for state in states {
            let mut v = vec![UNEVALUATED; nodes.len()];
            replay(nodes, 0, &state, konst, &mut v);
            self.cov_rows(nodes, &v, row0);
        }
    }

    /// The values of item `id`'s terms in evaluation `vals`, and those of
    /// its constants that decide whether a `?:` condition is masked, for
    /// `cov_replay`: what the evaluation saw, and what it skipped (all of
    /// it reads without side effects: `cov_term_steps`). `None` when the
    /// item has nothing to replay.
    fn cov_terms(
        &mut self,
        id: u32,
        nodes: &[Node],
        e: &Expression,
        vals: &[u8],
    ) -> Option<(Vec<u8>, Vec<u8>)> {
        let site = self.code_cov.as_ref()?.exprs.get(id as usize)?;
        if site.steps.as_ref().is_some_and(|s| s.is_empty()) {
            return None;
        }
        let n = site.terms.len();
        let mut leaves = Vec::new();
        leaves_of(nodes, 0, e, &mut leaves);
        if site.steps.is_none() {
            let (hint, proc) = (site.hint.clone().flatten(), site.proc.clone());
            let steps = self.cov_term_steps(nodes, &leaves, n, hint.as_deref(), proc.as_ref());
            if let Some(s) = self
                .code_cov
                .as_mut()
                .and_then(|c| c.exprs.get_mut(id as usize))
            {
                s.steps = Some(steps);
            }
            return self.cov_terms(id, nodes, e, vals);
        }
        let mut cur = vec![UNEVALUATED; n];
        let mut konst = vec![UNEVALUATED; nodes.len()];
        for &(i, x) in &leaves {
            let i = i as usize;
            let v = vals[i];
            let v = match nodes[i].op {
                Op::Term(t, _) if cur[t as usize] != UNEVALUATED => continue,
                _ if v != UNEVALUATED => v,
                _ => match self.cov_quietly(|me| me.eval_expr(x)).is_nonzero() {
                    Some(true) => 1,
                    Some(false) => 0,
                    None => X,
                },
            };
            match nodes[i].op {
                Op::Term(t, inv) => cur[t as usize] = if v <= 1 { v ^ inv as u8 } else { X },
                _ => konst[i] = v,
            }
        }
        Some((cur, konst))
    }

    /// When each term of an item can settle, for `cov_replay`: the earliest
    /// and the latest step (`cov_steps`) of the signals it reads, in a
    /// continuous assignment. A block runs after the continuous
    /// assignments of its inputs' delta cycle (`step / 2`), and sees the
    /// outputs of the blocks of its own instance written above it that ran
    /// in that cycle too. A term that reads no signal could settle at any
    /// step. Empty when the item has nothing to replay: no two of its terms
    /// ever change in a certain order, a leaf has side effects (a function
    /// call) and can't be read again, or a term reads what its own block
    /// writes. Which terms change together is decided at each evaluation,
    /// from those that changed (`cov_replay`).
    fn cov_term_steps(
        &self,
        nodes: &[Node],
        leaves: &[(u32, &Expression)],
        n: usize,
        hint: Option<&str>,
        proc: Option<&(String, usize)>,
    ) -> Vec<(u32, u32)> {
        let Some(cov) = self.code_cov.as_ref() else {
            return Vec::new();
        };
        if cov.steps.is_empty() || leaves.iter().any(|(_, x)| !pure(x)) {
            return Vec::new();
        }
        let hint = proc.map(|p| p.0.as_str()).or(hint);
        let mut steps = vec![(u32::MAX, 0u32); n];
        for &(i, x) in leaves {
            let Op::Term(t, _) = nodes[i as usize].op else {
                continue;
            };
            let mut names = HashSet::default();
            Self::collect_expr_reads(x, &self.module, &mut names);
            for id in names.iter().filter_map(|name| self.cov_signal(name, hint)) {
                let Some(&(lo, hi)) = cov.steps.get(id) else {
                    continue;
                };
                let (lo, hi) = match (proc, cov.writers.get(&id)) {
                    (Some(p), Some(w)) if w == p => return Vec::new(),
                    (Some(p), Some(w)) if w.0 == p.0 && w.1 < p.1 && lo >= 2 => {
                        (lo / 2 - 1, hi / 2 - 1)
                    }
                    (Some(_), _) => (lo / 2, hi / 2),
                    (None, _) => (lo, hi),
                };
                let s = &mut steps[t as usize];
                *s = (s.0.min(lo), s.1.max(hi));
            }
        }
        for s in steps.iter_mut().filter(|s| s.0 > s.1) {
            *s = (0, u32::MAX);
        }
        // (Some step is in every term's range, so no two of them ever have
        // a certain order.)
        let lo = steps.iter().map(|s| s.0).max().unwrap_or(0);
        if steps.iter().all(|s| s.1 >= lo) {
            return Vec::new();
        }
        steps
    }

    /// The signal a name read in scope `hint` (or one of its parents) is.
    fn cov_signal(&self, name: &str, hint: Option<&str>) -> Option<usize> {
        let mut scope = hint.unwrap_or("");
        while !scope.is_empty() {
            if let Some(&id) = self.signal_name_to_id.get(format!("{scope}.{name}").as_str()) {
                return Some(id);
            }
            scope = scope.rsplit_once('.').map_or("", |(p, _)| p);
        }
        self.resolve_read_name(name, "")
    }

    /// When each signal can settle, after the comb entries are built, for
    /// `cov_replay`: how many delta cycles after a change of the signals no
    /// combinational logic drives (the outputs of clocked and initial
    /// blocks, and testbench inputs) the reference simulator updates it, in
    /// half cycles, at the earliest and at the latest (it depends on which
    /// of those signals changed: the shortest and the longest path). An
    /// `always_comb` or `always @*` output is 2 later than a signal its
    /// block reads. A net driven by a continuous assignment (a gate, a port
    /// connection) is 1 later than a variable it reads, and as late as a
    /// net: the reference simulator updates a network of continuous
    /// assignments together. An inverter of an `always` block's output
    /// updates with it (the block computes it), one of a variable an
    /// `initial` block or the testbench writes 1 later. Also notes which
    /// block writes each combinational block output.
    pub(super) fn cov_steps(&mut self) {
        let (inverters, clocked, assigned) = match self.code_cov.as_mut() {
            Some(c) if c.exprs.iter().any(|s| s.hint.is_some() || s.proc.is_some()) => (
                std::mem::take(&mut c.inverters),
                std::mem::take(&mut c.clocked),
                std::mem::take(&mut c.assigned),
            ),
            _ => return,
        };
        let n = self.signal_table.len();
        // Outputs of `always` blocks (the combinational ones below).
        let mut always = vec![false; n];
        for (scope, names) in &clocked {
            for id in names.iter().filter_map(|x| self.cov_signal(x, Some(scope))) {
                if id < n {
                    always[id] = true;
                }
            }
        }
        // A variable an `assign` drives from the top's inputs only: with the
        // testbench writing those through the simulator's interface, the
        // reference simulator updates it with them, so it can change with
        // them or after.
        let mut loose = vec![false; n];
        for id in assigned.iter().filter_map(|(x, scope)| self.cov_signal(x, Some(scope))) {
            if id < n && !self.module.nets.contains(self.name_for_id(id)) {
                loose[id] = true;
            }
        }
        let mut top = vec![false; n];
        for (name, _) in self.vpi_port_directions.iter().filter(|(name, dir)| {
            !name.contains('.') && matches!(dir, PortDirection::Input | PortDirection::Inout)
        }) {
            if let Some(&id) = self.signal_name_to_id.get(&**name) {
                if id < n {
                    top[id] = true;
                }
            }
        }
        // Inverter target -> operand.
        let inv: HashMap<usize, usize> = inverters
            .iter()
            .filter_map(|(d, x, scope)| {
                Some((
                    self.cov_signal(d, Some(scope))?,
                    self.cov_signal(x, Some(scope))?,
                ))
            })
            .filter(|&(d, x)| d < n && x < n)
            .collect();
        let Some(cov) = self.code_cov.as_mut() else {
            return;
        };
        let kinds: Vec<Option<bool>> = self
            .comb_entries
            .iter()
            .map(|e| match e.item {
                CombItem::Noop | CombItem::TimingCheck { .. } => None,
                CombItem::AlwaysBlock { .. } | CombItem::CompiledAlwaysBlock { .. } => Some(true),
                _ => Some(false),
            })
            .collect();
        let mut net = vec![false; n];
        for (e, k) in self.comb_entries.iter().zip(&kinds) {
            for &w in e.cold.write_signal_ids.iter().filter(|&&w| w < n) {
                match k {
                    Some(false) => net[w] = true,
                    Some(true) => {
                        always[w] = true;
                        let scope = e.cold.scope_hint.clone().unwrap_or_default();
                        cov.writers.insert(w, (scope, e.cold.span.start));
                    }
                    None => {}
                }
            }
        }
        // Shortest and longest paths; a loop stops growing after a few
        // rounds.
        let mut steps: Vec<(u32, u32)> = vec![(0, 0); n];
        for (e, k) in self.comb_entries.iter().zip(&kinds) {
            for &w in e.cold.write_signal_ids.iter().filter(|_| k.is_some()) {
                if w < n {
                    steps[w].0 = u32::MAX;
                }
            }
        }
        for _ in 0..16 {
            let mut moved = false;
            for (e, k) in self.comb_entries.iter().zip(&kinds) {
                let Some(process) = *k else {
                    continue;
                };
                let writes = &e.cold.write_signal_ids;
                let reads: Vec<(u32, u32, u32)> = e
                    .cold
                    .read_signal_ids
                    .iter()
                    .filter(|&&r| r < n && !writes.contains(&r))
                    .map(|&r| {
                        let d = if process { 2 } else { u32::from(!net[r]) };
                        (steps[r].0, steps[r].1, d)
                    })
                    .collect();
                let inputs = e
                    .cold
                    .read_signal_ids
                    .iter()
                    .all(|&r| r < n && top[r]);
                for &w in writes.iter().filter(|&&w| w < n) {
                    let soon = |r: &(u32, u32, u32)| {
                        if loose[w] && inputs && !process {
                            0
                        } else {
                            r.2
                        }
                    };
                    let lo = reads
                        .iter()
                        .map(|r| r.0.saturating_add(soon(r)))
                        .min()
                        .unwrap_or(0);
                    let hi = reads.iter().map(|r| r.1 + r.2).max().unwrap_or(0);
                    let (lo, hi) = match inv.get(&w) {
                        Some(&x) if !process && always[x] => steps[x],
                        _ => (lo, hi),
                    };
                    let s = &mut steps[w];
                    if lo < s.0 || hi > s.1 {
                        *s = (s.0.min(lo), s.1.max(hi));
                        moved = true;
                    }
                }
            }
            if !moved {
                break;
            }
        }
        // (A loop with no way in never changes.)
        for s in steps.iter_mut().filter(|s| s.0 > s.1) {
            s.0 = s.1;
        }
        cov.steps = steps;
    }

    /// Evaluate node `i` of an item's tree (`e` is its expression) with the
    /// interpreter's operators and short-circuits, noting each node's value
    /// in `vals`.
    fn cov_node(&mut self, nodes: &[Node], i: u32, e: &Expression, vals: &mut [u8]) -> Value {
        let e = unparen(e);
        let n = nodes[i as usize];
        let [k0, k1, k2] = n.kids;
        let v = match (n.op, &e.kind) {
            (Op::Not, ExprKind::Unary { op, operand }) => {
                let o = self.cov_node(nodes, k0, operand, vals);
                if *op == UnaryOp::LogNot {
                    o.logic_not()
                } else {
                    o.bitwise_not()
                }
            }
            (Op::And | Op::AndFull, ExprKind::Binary { left, right, .. }) => {
                let l = self.cov_node(nodes, k0, left, vals);
                // §11.3.5: only a known 0 on the left skips the right. Under
                // a negation the right decides too, so it is read all the
                // same when that has no side effects.
                if !l.has_xz() && l.to_u64() == Some(0) {
                    if n.op == Op::AndFull && pure(right) {
                        self.cov_quietly(|me| me.cov_node(nodes, k1, right, vals));
                    }
                    Value::from_u64(0, 1)
                } else {
                    let r = self.cov_node(nodes, k1, right, vals);
                    l.logic_and(&r)
                }
            }
            (Op::Or | Op::OrFull, ExprKind::Binary { left, right, .. }) => {
                let l = self.cov_node(nodes, k0, left, vals);
                if l.to_u64().is_some_and(|v| v != 0) {
                    if n.op == Op::OrFull && pure(right) {
                        self.cov_quietly(|me| me.cov_node(nodes, k1, right, vals));
                    }
                    Value::from_u64(1, 1)
                } else {
                    let r = self.cov_node(nodes, k1, right, vals);
                    l.logic_or(&r)
                }
            }
            (
                Op::BitAnd | Op::BitOr | Op::BitXor | Op::BitXnor,
                ExprKind::Binary { left, right, .. },
            ) => {
                let l = self.cov_node(nodes, k0, left, vals);
                let r = self.cov_node(nodes, k1, right, vals);
                match n.op {
                    Op::BitAnd => l.bitwise_and(&r),
                    Op::BitOr => l.bitwise_or(&r),
                    Op::BitXor => l.bitwise_xor(&r),
                    _ => l.bitwise_xor(&r).bitwise_not(),
                }
            }
            (
                Op::Mux,
                ExprKind::Conditional {
                    condition,
                    then_expr,
                    else_expr,
                },
            ) => {
                // The condition may carry its branch counters (`COND_FN`).
                let (c, arm) = match &unparen(condition).kind {
                    ExprKind::SystemCall { name, args } if name == COND_FN && args.len() == 2 => {
                        (&args[1], marker_id(&args[0]))
                    }
                    _ => (&**condition, None),
                };
                let c = self.cov_node(nodes, k0, c, vals);
                if let Some(t) = arm.filter(|_| !self.cov_quiet()) {
                    let slot = match c.is_nonzero() {
                        Some(true) => Some(t),
                        Some(false) => Some(t + 1),
                        None => None,
                    };
                    if let Some(h) = slot.and_then(|s| self.code_cov_hits.get_mut(s as usize)) {
                        *h += 1;
                    }
                }
                if c.has_unknown() {
                    let t = self.cov_node(nodes, k1, then_expr, vals);
                    let f = self.cov_node(nodes, k2, else_expr, vals);
                    t.merge_unknown(&f)
                } else {
                    // Only the taken arm is evaluated; a constant other arm
                    // is known all the same, and decides whether the
                    // condition is masked.
                    let (taken, kt, other, ko) = if c.is_true() {
                        (then_expr, k1, else_expr, k2)
                    } else {
                        (else_expr, k2, then_expr, k1)
                    };
                    let v = self.cov_node(nodes, kt, taken, vals);
                    // (Only a literal or a parameter: reading it does nothing.)
                    let pure = matches!(
                        unparen(other).kind,
                        ExprKind::Number(_) | ExprKind::Ident(_)
                    );
                    if nodes[ko as usize].op == Op::Const && pure {
                        self.cov_node(nodes, ko, other, vals);
                    }
                    v
                }
            }
            _ => self.eval_expr(e),
        };
        vals[i as usize] = match v.is_nonzero() {
            Some(true) => 1,
            Some(false) => 0,
            None => X,
        };
        v
    }

    /// Nets and variables toggle coverage tracks, by name, taken while the
    /// elaboration signal table still holds their declarations: the integral
    /// ones of instances, ports included, without unpacked arrays, `time`
    /// variables, subroutine variables and class properties. An input or
    /// inout port is a net of its instance holding the value its connection
    /// gives it (§23.3.3): a whole-net connection collapsed onto the actual
    /// shares the actual's counts, an expression or select toggles as its
    /// value, and a constant or open port never toggles.
    /// `exclude_input_ports` leaves input and inout ports to the signal that
    /// drives them. When the run leaves integers out, the signals declared
    /// with an integer atom type are left out too. Each comes with its data
    /// type (`ToggleTypes::of`).
    ///
    /// Each name also carries what `cov_drop_fixed` needs: `None` for a port,
    /// a net and a variable of a struct, enum or named type, which always
    /// stay tracked; for any other variable, whether a constant declaration
    /// initializer gave it its elaborated value (a bit that is not x, or is
    /// set in a 2-state variable). One declared in a generate block counts
    /// as not initialized: the reference simulator keeps those.
    pub(super) fn cov_toggle_candidates(
        module: &ElaboratedModule,
    ) -> Vec<(String, &'static str, Option<bool>)> {
        let Some(cfg) = code_coverage().filter(|c| c.kinds & KIND_TOGGLE != 0) else {
            return Vec::new();
        };
        let inst_paths: HashSet<&str> = module.instances.iter().map(|i| i.path.as_str()).collect();
        // Each instance's own name, without its array index (`u` for `u[0]`
        // and `g[0].u`), and its module.
        let inst_names: HashSet<(&str, &str)> = module
            .instances
            .iter()
            .map(|i| {
                let leaf = i.path.rsplit('.').next().unwrap_or_default();
                let name = leaf.split('[').next().unwrap_or_default();
                (name, i.def_name.as_str())
            })
            .collect();
        let subr_prefixes: Vec<String> = module
            .functions
            .keys()
            .chain(module.tasks.keys())
            .map(|k| format!("{}.", k))
            .collect();
        // Declared in a generate block (or a named block) of its instance.
        let nested = |n: &str| {
            let mut cut = n.len();
            while let Some(p) = n[..cut].rfind('.') {
                if inst_paths.contains(&n[..p]) {
                    return n[p + 1..].contains('.');
                }
                cut = p;
            }
            n.contains('.')
        };
        let types = ToggleTypes {
            module,
            // A path-specific bind runs a specialized copy of its target
            // ("target__bind0"); its ports are declared under the source name.
            defs: module
                .instances
                .iter()
                .map(|i| (i.path.as_str(), xezim_core::bind_spec_base(&i.def_name)))
                .collect(),
            ports: std::cell::OnceCell::new(),
        };
        let mut out: Vec<(String, &'static str, Option<bool>)> = module
            .signals
            .iter()
            .filter(|(name, sig)| {
                if sig.is_const
                    || sig.is_real
                    || sig.width == 0
                    || (cfg.exclude_input_ports
                        && matches!(
                            sig.direction,
                            Some(PortDirection::Input) | Some(PortDirection::Inout)
                        ))
                {
                    return false;
                }
                if name.contains("__xz") || name.starts_with("__") || name.contains("::") {
                    return false;
                }
                if let Some((base, _)) = name.split_once('[') {
                    if module.arrays.contains_key(base)
                        || module.arrays_nd.contains_key(base)
                        || module.dynamic_arrays.contains(base)
                        || module.associative_arrays.contains_key(base)
                    {
                        return false;
                    }
                }
                let n = name.as_str();
                // Enum constants and parameters, and the placeholders named
                // after instances.
                if module.parameters.contains_key(n) || inst_paths.contains(n) {
                    return false;
                }
                if module.arrays.contains_key(n)
                    || module.arrays_nd.contains_key(n)
                    || module.dynamic_arrays.contains(n)
                    || module.associative_arrays.contains_key(n)
                    || module.queue_vars.contains(n)
                    || module.string_signals.contains(n)
                    || module.events.contains(n)
                {
                    return false;
                }
                if let Some(t) = &sig.type_name {
                    if module.classes.contains_key(t) || module.interfaces.contains(t) {
                        return false;
                    }
                    // Instances and generate blocks are scopes, not signals
                    // (§23.6). Elaboration gives the top module a 1-bit
                    // placeholder signal under each one's name, so that
                    // hierarchical names through it validate: typed
                    // `$genblock` for a generate block and by its module for
                    // an instance, including an instance array and one in a
                    // generate block, which have no instance path of that
                    // name.
                    if t == "$genblock" || inst_names.contains(&(n, t.as_str())) {
                        return false;
                    }
                }
                match module.var_decl_types.get(n) {
                    Some(DataType::IntegerAtom {
                        kind: IntegerAtomType::Time,
                        ..
                    })
                    | Some(DataType::Real { .. })
                    | Some(DataType::Simple { .. })
                    | Some(DataType::Interface { .. }) => return false,
                    _ => {}
                }
                !subr_prefixes.iter().any(|p| n.starts_with(p.as_str()))
            })
            .map(|(name, sig)| {
                let n = name.as_str();
                let var = sig.direction.is_none()
                    && sig.type_name.is_none()
                    && !module.nets.contains(n)
                    && !module.packed_struct_fields.contains_key(n)
                    && !module.packed_signal_elem_widths.contains_key(n)
                    && !matches!(
                        module.var_decl_types.get(n),
                        Some(DataType::Struct(_)) | Some(DataType::Enum(_))
                    );
                let two = module.two_state_signals.contains(n);
                let width = sig.width as usize;
                let init = (0..width).step_by(64).any(|lo| {
                    let w = (width - lo).min(64);
                    let mask = if w == 64 { u64::MAX } else { (1u64 << w) - 1 };
                    let (v, x) = sig.value.slice_bits_swar(lo, w);
                    (if two { v } else { v | !x }) & mask != 0
                });
                (
                    name.clone(),
                    types.of(name, sig.type_name.as_deref()),
                    var.then(|| init && !nested(n)),
                )
            })
            .filter(|(_, ty, _)| cfg.integers || !TOGGLE_INTEGER_TYPES.contains(ty))
            .filter(|(_, ty, _)| cfg.enums || *ty != "enum")
            .collect();
        if !cfg.generate {
            // §27.5: a labeled generate block or a loop's names are
            // hierarchical (`g.x`, `g[1].x`); an unlabeled `if` or `case`
            // branch's stay bare in its instance, so they are found in the
            // source.
            let decls = generate_decls(module);
            let none = (HashSet::default(), HashSet::default());
            let top = xezim_core::bind_spec_base(&module.name).to_string();
            out.retain(|(n, _, _)| {
                // The instance the signal belongs to: the longest instance
                // path it starts with.
                let inst = n
                    .match_indices('.')
                    .map(|(k, _)| &n[..k])
                    .filter(|p| inst_paths.contains(p))
                    .last()
                    .unwrap_or("");
                let rel = n[inst.len()..].trim_start_matches('.');
                let def = types.defs.get(inst).copied().unwrap_or(top.as_str());
                let (bare, blocks) = decls.get(def).unwrap_or(&none);
                // A member (`s.m`) goes with its variable; an unnamed loop's
                // block is `genblk<n>`.
                let first = rel.split(['.', '[']).next().unwrap_or(rel);
                !(bare.contains(first)
                    || (rel.contains('.') && (blocks.contains(first) || first.starts_with("genblk"))))
            });
        }
        out.sort();
        out
    }

    /// The toggle candidates without the variables whose value is fixed
    /// before the run starts, which the reference simulator leaves out of
    /// toggle coverage: one that only a constant declaration initializer
    /// sets (§6.8: it takes effect before any process starts), and one that
    /// only a single blocking assignment of a constant sets, in an `initial`
    /// block with no timing control. Any other write in the design keeps a
    /// variable: a procedural or continuous assignment (even one that never
    /// runs), a port connection, a `force`, or an argument a subroutine or
    /// system task may write. A write the census can't place keeps every
    /// variable of its name. Values a VPI client writes don't count, as on
    /// the reference simulator; a variable nothing writes that has no
    /// initializer (one a VPI client drives) stays tracked.
    fn cov_drop_fixed(
        &self,
        names: Vec<(String, &'static str, Option<bool>)>,
    ) -> Vec<(String, &'static str)> {
        let index: HashMap<String, usize> = names
            .iter()
            .enumerate()
            .filter(|(_, (_, _, f))| f.is_some())
            .map(|(i, (n, _, _))| (n.clone(), i))
            .collect();
        if index.is_empty() {
            return names.into_iter().map(|(n, t, _)| (n, t)).collect();
        }
        let mut leaves: HashMap<String, Vec<usize>> = HashMap::default();
        for (n, &i) in &index {
            let leaf = n.rsplit('.').next().unwrap_or(n);
            leaves.entry(leaf.to_string()).or_default().push(i);
        }
        let m = &self.module;
        let mut c = Census {
            sim: self,
            index,
            top: format!("{}.", m.name),
            scope: String::new(),
            leaves,
            once: vec![0; names.len()],
            more: vec![false; names.len()],
        };
        for b in &m.always_blocks {
            c.process(&b.stmt, &b.scope, false);
        }
        for p in &m.pending_always {
            let b = p.clone().materialize();
            c.process(&b.stmt, &b.scope, false);
        }
        for b in &m.initial_blocks {
            c.process(&b.stmt, &b.scope, true);
        }
        for p in &m.pending_initial {
            let b = p.clone().materialize();
            c.scope = b.scope.clone();
            c.stmt(&b.stmt, &p.source, !timed(&b.stmt));
        }
        for b in m
            .final_blocks
            .iter()
            .chain(&m.program_initial_blocks)
            .chain(&m.static_init_blocks)
        {
            c.process(&b.stmt, &b.scope, false);
        }
        for (k, f) in m.functions.iter() {
            c.subroutine(k, &f.items);
        }
        for (k, t) in m.tasks.iter() {
            c.subroutine(k, &t.items);
        }
        for cls in m.classes.values() {
            for meth in cls.methods.values() {
                use crate::ast::decl::ClassMethodKind as K;
                match &meth.kind {
                    K::Function(f) | K::Extern(f) | K::PureVirtual(f) => c.subroutine("", &f.items),
                    K::Task(t) => c.subroutine("", &t.items),
                }
            }
        }
        c.scope.clear();
        for ca in &m.continuous_assigns {
            c.mark(&ca.lhs, false);
        }
        for p in &m.pending_cont_assign {
            c.mark(&p.clone().materialize(&m.parameters).lhs, false);
        }
        names
            .into_iter()
            .enumerate()
            .filter(|(i, (_, _, f))| match f {
                Some(init) => c.more[*i] || c.once[*i] != u32::from(!*init),
                None => true,
            })
            .map(|(_, (n, t, _))| (n, t))
            .collect()
    }

    fn cov_build_toggle(
        &mut self,
        cov: &mut CodeCov,
        inst_paths: &HashSet<String>,
        names: Vec<(String, &'static str)>,
    ) -> Toggle {
        let mut t = Toggle {
            slot: vec![u32::MAX; self.signal_table.len()],
            ids: Vec::new(),
            width: Vec::new(),
            word0: Vec::new(),
            bit0: Vec::new(),
            words: Vec::new(),
            rise: Vec::new(),
            fall: Vec::new(),
            names: Vec::new(),
        };
        let top_prefix = format!("{}.", self.module.name);
        for (name, ty) in names {
            let Some(&id) = self.signal_name_to_id.get(name.as_str()).or_else(|| {
                self.signal_name_to_id
                    .get(format!("{}{}", top_prefix, name).as_str())
            }) else {
                continue;
            };
            let width = self.signal_widths.get(id).copied().unwrap_or(0);
            if width == 0 || self.signal_real.get(id).copied().unwrap_or(false) {
                continue;
            }
            // The owning instance: the longest instance path the name
            // starts with; the rest is the signal's name within it.
            let mut inst = "";
            let mut rest = name.as_str();
            let mut cut = name.len();
            while let Some(p) = name[..cut].rfind('.') {
                if inst_paths.contains(&name[..p]) {
                    inst = &name[..p];
                    rest = &name[p + 1..];
                    break;
                }
                cut = p;
            }
            let Some(sc) = Self::cov_instance_scope(&self.module, cov, inst_paths, inst) else {
                continue;
            };
            let k = if t.slot[id] != u32::MAX {
                t.slot[id]
            } else {
                let k = t.ids.len() as u32;
                t.slot[id] = k;
                t.ids.push(id as u32);
                t.width.push(width);
                t.word0.push(t.words.len() as u32);
                t.bit0.push(t.rise.len() as u32);
                let words = width.div_ceil(64) as usize;
                let v = &self.signal_table[id];
                for w in 0..words {
                    let n = (width as usize - w * 64).min(64);
                    let (pv, px) = v.slice_bits_swar(w * 64, n);
                    t.words.push(Word {
                        v: pv,
                        x: px,
                        at: u64::MAX,
                        bv: pv,
                        bx: px,
                    });
                }
                t.rise.resize(t.rise.len() + width as usize, 0);
                t.fall.resize(t.fall.len() + width as usize, 0);
                k
            };
            t.names.push((sc, rest.to_string(), k, ty));
        }
        t
    }

    /// End of a tick or of a time slot: compare every tracked signal written
    /// since the last sample with its value at the end of the previous slot.
    pub(super) fn cov_toggle_sample(&mut self) {
        let Some(t) = self.code_cov.as_mut().and_then(|c| c.toggle.as_mut()) else {
            return;
        };
        let now = self.time;
        let full = !self.dump_dirty_active;
        let n = if full {
            t.ids.len()
        } else {
            self.dump_dirty.len()
        };
        for i in 0..n {
            let k = if full {
                i
            } else {
                match t.slot.get(self.dump_dirty[i] as usize) {
                    Some(&k) if k != u32::MAX => k as usize,
                    _ => continue,
                }
            };
            let id = t.ids[k] as usize;
            let width = t.width[k] as usize;
            if width <= 64 {
                let (v, x) = match self.signal_inline_bits.get(id) {
                    Some(sl) => (sl[0], sl[1]),
                    None => self.signal_table[id].raw_bits(),
                };
                let mask = if width == 64 {
                    u64::MAX
                } else {
                    (1u64 << width) - 1
                };
                t.sample(k, v & mask, x & mask, 0, now);
            } else {
                for w in 0..width.div_ceil(64) {
                    let n = (width - w * 64).min(64);
                    let (v, x) = self.signal_table[id].slice_bits_swar(w * 64, n);
                    t.sample(k, v, x, w, now);
                }
            }
        }
    }

    fn cov_report(&self) -> Vec<Report> {
        let Some(cov) = self.code_cov.as_ref() else {
            return Vec::new();
        };
        let mut lines = Lines {
            module: &self.module,
            nl: HashMap::default(),
        };
        let mut reports: Vec<Report> = (0..cov.scopes.len()).map(|_| Report::default()).collect();
        let count = |c: u32| self.code_cov_hits.get(c as usize).copied().unwrap_or(0);
        for s in &cov.stmts {
            let src = cov.scopes[s.scope as usize].src;
            let (file, line) = lines.locate(s.span, src).unwrap_or_default();
            reports[s.scope as usize].stmts.push(StmtRow {
                file,
                line,
                start: s.span.start,
                kind: s.kind,
                count: count(s.counter),
                excluded: s.excluded,
            });
        }
        for b in &cov.branches {
            let src = cov.scopes[b.scope as usize].src;
            let (file, line) = lines.locate(b.span, src).unwrap_or_default();
            let taken: u64 = b.arms.iter().filter_map(|a| a.counter).map(count).sum();
            let arms: Vec<ArmRow> = b
                .arms
                .iter()
                .map(|a| ArmRow {
                    line: lines.locate(a.span, src).map(|(_, l)| l).unwrap_or(line),
                    label: a.label,
                    count: match a.counter {
                        Some(c) => count(c),
                        None => b.entry.map(count).unwrap_or(0).saturating_sub(taken),
                    },
                    excluded: a.excluded,
                })
                .collect();
            // In block granularity an implicit `else` or `default` is a
            // block of its own, run when no written arm is.
            if cov.blocks && cov.kinds & KIND_STATEMENT != 0 {
                for a in arms.iter().filter(|a| a.label.ends_with("(implicit)")) {
                    reports[b.scope as usize].stmts.push(StmtRow {
                        file: file.clone(),
                        line: a.line,
                        start: b.span.start,
                        kind: a.label,
                        count: a.count,
                        excluded: b.excluded || a.excluded,
                    });
                }
            }
            reports[b.scope as usize].branches.push(BranchRow {
                file,
                line,
                start: b.span.start,
                kind: b.kind,
                arms,
                excluded: b.excluded,
            });
        }
        for x in cov.exprs.iter().filter(|x| !x.excluded) {
            let src = cov.scopes[x.scope as usize].src;
            let (file, line) = lines.locate(x.span, src).unwrap_or_default();
            let outs: Vec<[u64; 4]> = if x.bimodal {
                (0..x.terms.len() as u32)
                    .map(|t| std::array::from_fn(|j| count(x.row0 + 4 * t + j as u32)))
                    .collect()
            } else {
                Vec::new()
            };
            let row = |t: usize, v: usize| match outs.get(t) {
                Some(o) => o[2 * v] + o[2 * v + 1],
                None => count(x.row0 + 2 * t as u32 + v as u32),
            };
            let item = ItemRow {
                file,
                line,
                start: x.span.start,
                kind: x.kind,
                block: x.block.clone(),
                copy: x.copy.clone(),
                text: x.text.clone(),
                terms: x
                    .terms
                    .iter()
                    .enumerate()
                    .map(|(t, name)| (name.clone(), [row(t, 0), row(t, 1)]))
                    .collect(),
                outs,
            };
            let r = &mut reports[x.scope as usize];
            if x.condition {
                r.conds.push(item);
            } else {
                r.exprs.push(item);
            }
        }
        if let Some(t) = &cov.toggle {
            for (sc, name, k, ty) in &t.names {
                let k = *k as usize;
                let (b, w) = (t.bit0[k] as usize, t.width[k] as usize);
                reports[*sc as usize].toggles.push(ToggleRow {
                    name: name.clone(),
                    ty,
                    rise: t.rise[b..b + w].to_vec(),
                    fall: t.fall[b..b + w].to_vec(),
                });
            }
        }
        if let Some(f) = &cov.fsm {
            let mut locate = |span: Span, sc: u32| {
                lines
                    .locate(span, cov.scopes[sc as usize].src)
                    .unwrap_or_default()
            };
            for (r, rows) in reports
                .iter_mut()
                .zip(self.fsm_rows(f, cov.scopes.len(), &mut locate))
            {
                r.fsms = rows;
            }
        }
        for r in reports.iter_mut() {
            r.normalize();
        }
        reports
    }

    /// The `code_coverage` member of the results file, and the `[COV]`
    /// summary lines.
    pub(super) fn code_coverage_json(&self) -> Option<String> {
        let cov = self.code_cov.as_ref()?;
        let kinds = cov.kinds;
        let reports = self.cov_report();
        let mut order: Vec<usize> = (0..cov.scopes.len()).collect();
        order.sort_by(|&a, &b| {
            (cov.scopes[a].kind, &cov.scopes[a].name)
                .cmp(&(cov.scopes[b].kind, &cov.scopes[b].name))
        });
        let mut total = Tallies::default();
        let mut split = [Tally::default(); 2];
        for r in &reports {
            for (t, x) in total.iter_mut().zip(r.tallies()) {
                t.add(x);
            }
            for (t, x) in split.iter_mut().zip(r.fsm_split()) {
                t.add(x);
            }
        }
        let kind_names: Vec<&str> = KIND_NAMES
            .iter()
            .enumerate()
            .filter(|(i, _)| kinds & (1 << i) != 0)
            .map(|(_, n)| *n)
            .collect();
        let summary = |t: &Tallies| -> String {
            KIND_NAMES
                .iter()
                .enumerate()
                .filter(|(i, _)| kinds & (1 << i) != 0)
                .map(|(i, n)| format!("{} {}", n, t[i].text()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        chatter!("[COV] code coverage: {}", summary(&total));
        const SHOWN: usize = 50;
        for &i in order.iter().take(SHOWN) {
            let s = &cov.scopes[i];
            chatter!(
                "[COV]   {} ({}): {}",
                s.name,
                s.unit,
                summary(&reports[i].tallies())
            );
        }
        if order.len() > SHOWN {
            chatter!(
                "[COV]   ... {} more scopes in the results file",
                order.len() - SHOWN
            );
        }
        let mut json = String::new();
        json.push_str("  \"code_coverage\": {\n");
        json.push_str(&format!(
            "    \"kinds\": [{}],\n",
            kind_names
                .iter()
                .map(|k| format!("\"{}\"", k))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        for (i, n) in KIND_NAMES.iter().enumerate() {
            if kinds & (1 << i) != 0 {
                let j = if 1 << i == KIND_FSM {
                    total[i].fsm_json(&split)
                } else {
                    total[i].json()
                };
                json.push_str(&format!("    \"{}\": {},\n", n, j));
            }
        }
        json.push_str("    \"scopes\": [\n");
        for (j, &i) in order.iter().enumerate() {
            let s = &cov.scopes[i];
            json.push_str(&format!(
                "      {{\n        \"scope\": \"{}\",\n        \"design_unit\": \"{}\",\n        \"kind\": \"{}\",\n",
                json_escape(&s.name),
                json_escape(&s.unit),
                s.kind
            ));
            reports[i].json(kinds, "        ", &mut json);
            json.push_str(if j + 1 < order.len() {
                "      },\n"
            } else {
                "      }\n"
            });
        }
        json.push_str("    ],\n");
        // Design units: every instance of a module added together.
        let mut units: BTreeMap<String, (usize, Report)> = BTreeMap::new();
        for &i in &order {
            let s = &cov.scopes[i];
            let e = units
                .entry(s.unit.clone())
                .or_insert((0, Report::default()));
            e.0 += 1;
            e.1.merge(&reports[i]);
        }
        json.push_str("    \"design_units\": [\n");
        let n_units = units.len();
        for (j, (unit, (n, r))) in units.iter().enumerate() {
            json.push_str(&format!(
                "      {{\n        \"design_unit\": \"{}\",\n        \"instances\": {},\n",
                json_escape(unit),
                n
            ));
            r.json(kinds, "        ", &mut json);
            json.push_str(if j + 1 < n_units {
                "      },\n"
            } else {
                "      }\n"
            });
        }
        json.push_str("    ]\n  }");
        Some(json)
    }
}

/// The writes to toggle candidates `cov_drop_fixed` counts, by candidate:
/// `once` the blocking assignments of a constant in an `initial` block
/// without a timing control, `more` any other write.
struct Census<'a> {
    sim: &'a Simulator,
    index: HashMap<String, usize>,
    /// The top module's name and a dot, which a full path starts with.
    top: String,
    /// Instance or subroutine scope of the code being read: a subroutine
    /// body may still name a variable of its instance without it.
    scope: String,
    /// Candidates by the last part of their name, for writes the census
    /// can't place (`mark_path`).
    leaves: HashMap<String, Vec<usize>>,
    once: Vec<u32>,
    more: Vec<bool>,
}

impl<'a> Census<'a> {
    fn process(&mut self, s: &Statement, scope: &str, initial: bool) {
        self.scope = scope.to_string();
        self.stmt(s, s, initial && !timed(s));
    }

    fn subroutine(&mut self, key: &str, items: &[Statement]) {
        self.scope = key
            .rsplit_once('.')
            .map(|(p, _)| p.to_string())
            .unwrap_or_default();
        for s in items {
            self.stmt(s, s, false);
        }
    }

    /// Count a write to each candidate the lvalue `e` names.
    fn mark(&mut self, e: &Expression, once: bool) {
        let mut found = Vec::new();
        paths(e, &mut found);
        for p in found {
            self.mark_path(&p, once);
        }
    }

    /// Count a write to the candidate path `p` names, whole or by a prefix
    /// (a select or member writes its variable), with and without the scope
    /// in front. A clocking block output writes the signal it is bound to
    /// (§14.3). A write the census can't place, such as one through a
    /// virtual interface or to a name it doesn't know, counts for every
    /// candidate with the same last name: when unsure, keep the variable.
    fn mark_path(&mut self, p: &str, once: bool) {
        let sim = self.sim;
        let p = p.strip_prefix(self.top.as_str()).unwrap_or(p);
        let scoped = (!self.scope.is_empty()).then(|| format!("{}.{}", self.scope, p));
        let mut placed = false;
        for full in std::iter::once(p.to_string()).chain(scoped) {
            placed |= sim.signal_name_to_id.get(&full).is_some();
            let mut end = full.len();
            loop {
                let head = &full[..end];
                if let Some(&i) = self.index.get(head) {
                    placed = true;
                    if once {
                        self.once[i] += 1;
                    } else {
                        self.more[i] = true;
                    }
                }
                if let Some(cb) = sim
                    .module
                    .clocking_blocks
                    .get(head)
                    .filter(|_| end < full.len())
                {
                    placed = true;
                    let var = full[end + 1..].split('.').next().unwrap_or_default();
                    let mut bound = Vec::new();
                    match cb
                        .signals
                        .iter()
                        .find(|s| s.name.name == var)
                        .and_then(|s| s.bound_to.as_ref())
                    {
                        Some(e) => paths(e, &mut bound),
                        None => bound.push(var.to_string()),
                    }
                    let up = head.rsplit_once('.').map(|(u, _)| u);
                    for b in bound {
                        if let Some(u) = up {
                            self.mark_path(&format!("{}.{}", u, b), false);
                        }
                        self.mark_path(&b, false);
                    }
                }
                match head.rfind('.') {
                    Some(d) => end = d,
                    None => break,
                }
            }
        }
        if !placed {
            let leaf = p.rsplit('.').next().unwrap_or(p);
            for i in self.leaves.get(leaf).cloned().unwrap_or_default() {
                self.more[i] = true;
            }
        }
    }

    /// `once`: the statement runs exactly once, at time 0 (straight-line
    /// code of an `initial` block without timing controls). `src` is the
    /// statement as an instance's module declares it, before its ports are
    /// replaced by their connections: a port tied to a constant is not a
    /// constant in the module.
    fn stmt(&mut self, s: &Statement, src: &Statement, once: bool) {
        use crate::ast::stmt::ProceduralContinuous as PC;
        match &s.kind {
            StatementKind::BlockingAssign { lvalue, rvalue } => {
                let whole = matches!(&lvalue.kind,
                    ExprKind::Ident(h) if h.path.iter().all(|p| p.selects.is_empty()));
                let fixed = once
                    && whole
                    && self.constant(rvalue)
                    && matches!(&src.kind,
                        StatementKind::BlockingAssign { rvalue: r, .. } if self.constant(r));
                self.mark(lvalue, fixed);
                self.expr(lvalue);
                self.expr(rvalue);
            }
            StatementKind::NonblockingAssign { lvalue, rvalue, .. }
            | StatementKind::ProceduralContinuous(PC::Assign { lvalue, rvalue })
            | StatementKind::ProceduralContinuous(PC::Force { lvalue, rvalue }) => {
                self.mark(lvalue, false);
                self.expr(lvalue);
                self.expr(rvalue);
            }
            StatementKind::Expr(e) | StatementKind::Return(Some(e)) => self.expr(e),
            StatementKind::SeqBlock { stmts, .. } | StatementKind::ParBlock { stmts, .. } => {
                let srcs = match &src.kind {
                    StatementKind::SeqBlock { stmts: v, .. }
                    | StatementKind::ParBlock { stmts: v, .. }
                        if v.len() == stmts.len() =>
                    {
                        Some(v)
                    }
                    _ => None,
                };
                for (i, x) in stmts.iter().enumerate() {
                    self.stmt(x, srcs.map_or(x, |v| &v[i]), once);
                }
            }
            StatementKind::If {
                condition,
                then_stmt,
                else_stmt,
                ..
            } => {
                self.expr(condition);
                self.stmt(then_stmt, then_stmt, false);
                if let Some(x) = else_stmt {
                    self.stmt(x, x, false);
                }
            }
            StatementKind::Case { expr, items, .. } => {
                self.expr(expr);
                for it in items {
                    self.stmt(&it.stmt, &it.stmt, false);
                }
            }
            StatementKind::For {
                init,
                condition,
                step,
                body,
            } => {
                for i in init {
                    match i {
                        ForInit::Assign { lvalue, rvalue } => {
                            self.mark(lvalue, false);
                            self.expr(lvalue);
                            self.expr(rvalue);
                        }
                        ForInit::VarDecl { init, .. } => self.expr(init),
                    }
                }
                for e in condition.iter().chain(step) {
                    self.expr(e);
                }
                self.stmt(body, body, false);
            }
            StatementKind::While { condition, body }
            | StatementKind::DoWhile { body, condition }
            | StatementKind::Repeat {
                count: condition,
                body,
            }
            | StatementKind::Wait {
                condition,
                stmt: body,
            } => {
                self.expr(condition);
                self.stmt(body, body, false);
            }
            StatementKind::Foreach { body, .. }
            | StatementKind::ForeachTail { body, .. }
            | StatementKind::Forever { body }
            | StatementKind::ForeverTail { body }
            | StatementKind::RsAction { body }
            | StatementKind::TimingControl { stmt: body, .. } => self.stmt(body, body, false),
            StatementKind::Assertion(a) => {
                for x in [&a.action, &a.else_action].into_iter().flatten() {
                    self.stmt(x, x, false);
                }
            }
            StatementKind::VarDecl { declarators, .. } => {
                for d in declarators {
                    if let Some(e) = &d.init {
                        self.expr(e);
                    }
                }
            }
            StatementKind::RandCase { items } => {
                for (w, x) in items {
                    self.expr(w);
                    self.stmt(x, x, false);
                }
            }
            StatementKind::WaitOrder { pass, fail, .. } => {
                for x in [pass, fail].into_iter().flatten() {
                    self.stmt(x, x, false);
                }
            }
            _ => {}
        }
    }

    /// The writes inside an expression: assignment operators, `++`/`--`,
    /// and arguments of a call that may write them.
    fn expr(&mut self, e: &Expression) {
        match &e.kind {
            ExprKind::AssignExpr {
                lvalue: l,
                rvalue: r,
            }
            | ExprKind::Binary {
                op: BinaryOp::Assign,
                left: l,
                right: r,
            } => {
                self.mark(l, false);
                self.expr(l);
                self.expr(r);
            }
            ExprKind::Unary { op, operand } => {
                if matches!(
                    op,
                    UnaryOp::PreIncr | UnaryOp::PostIncr | UnaryOp::PreDecr | UnaryOp::PostDecr
                ) {
                    self.mark(operand, false);
                }
                self.expr(operand);
            }
            ExprKind::Call { func, args } => {
                // An argument to an input port only reads; to any other
                // port, or of a call with no known declaration, it may write.
                let ports = self.ports(func);
                for (i, a) in args.iter().enumerate() {
                    let (port, a) = match &a.kind {
                        ExprKind::NamedArg {
                            name,
                            expr: Some(x),
                        } => (
                            ports.and_then(|p| p.iter().find(|q| q.name.name == name.name)),
                            &**x,
                        ),
                        _ => (ports.and_then(|p| p.get(i)), a),
                    };
                    if !port.is_some_and(|q| q.direction == PortDirection::Input) {
                        self.mark(a, false);
                    }
                    self.expr(a);
                }
            }
            ExprKind::SystemCall { name, args } => {
                let read = reads_only(name);
                for a in args {
                    if !read {
                        self.mark(a, false);
                    }
                    self.expr(a);
                }
            }
            ExprKind::Binary { left, right, .. }
            | ExprKind::Index {
                expr: left,
                index: right,
            }
            | ExprKind::Range(left, right) => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.expr(condition);
                self.expr(then_expr);
                self.expr(else_expr);
            }
            ExprKind::RangeSelect {
                expr, left, right, ..
            } => {
                self.expr(expr);
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Concatenation(v) | ExprKind::StreamOp { exprs: v, .. } => {
                for x in v {
                    self.expr(x);
                }
            }
            ExprKind::Replication { count, exprs } => {
                self.expr(count);
                for x in exprs {
                    self.expr(x);
                }
            }
            ExprKind::AssignmentPattern(v) => {
                for x in v {
                    self.expr(x.expr());
                }
            }
            ExprKind::Inside { expr, ranges } => {
                self.expr(expr);
                for x in ranges {
                    self.expr(x);
                }
            }
            ExprKind::MemberAccess { expr: x, .. }
            | ExprKind::Paren(x)
            | ExprKind::WithClause { expr: x, .. }
            | ExprKind::RandomizeWith { call: x, .. }
            | ExprKind::Specialization { base: x, .. }
            | ExprKind::ShallowCopy { source: x }
            | ExprKind::Matches { expr: x, .. }
            | ExprKind::NamedArg { expr: Some(x), .. }
            | ExprKind::Tagged { inner: Some(x), .. } => self.expr(x),
            _ => {}
        }
    }

    /// The ports of the function or task a call names, when the design
    /// declares it.
    fn ports(&self, func: &Expression) -> Option<&'a [crate::ast::decl::FunctionPort]> {
        let ExprKind::Ident(h) = &func.kind else {
            return None;
        };
        let m: &'a ElaboratedModule = &self.sim.module;
        let n = dotted(h);
        [format!("{}.{}", self.scope, n), n].iter().find_map(|k| {
            m.functions
                .get(k)
                .map(|f| f.ports.as_slice())
                .or_else(|| m.tasks.get(k).map(|t| t.ports.as_slice()))
        })
    }

    /// An expression of literals and parameters only.
    fn constant(&self, e: &Expression) -> bool {
        match &e.kind {
            ExprKind::Number(_) | ExprKind::StringLiteral(_) | ExprKind::TypeLiteral(_) => true,
            ExprKind::Ident(h) => {
                let n = dotted(h);
                let scoped = format!("{}.{}", self.scope, n);
                let leaf = n.rsplit('.').next().unwrap_or(&n);
                let params = &self.sim.module.parameters;
                let names = &self.sim.signal_name_to_id;
                params.contains_key(&n)
                    || params.contains_key(&scoped)
                    || (params.contains_key(leaf)
                        && names.get(&n).is_none()
                        && names.get(&scoped).is_none())
            }
            ExprKind::Unary { operand: x, .. } | ExprKind::Paren(x) => self.constant(x),
            ExprKind::Binary { op, left, right } => {
                !matches!(op, BinaryOp::Assign) && self.constant(left) && self.constant(right)
            }
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => self.constant(condition) && self.constant(then_expr) && self.constant(else_expr),
            ExprKind::Concatenation(v) => v.iter().all(|x| self.constant(x)),
            ExprKind::Replication { count, exprs } => {
                self.constant(count) && exprs.iter().all(|x| self.constant(x))
            }
            // Casts keep their operand last.
            ExprKind::SystemCall { name, args } => match name.as_str() {
                "$signed" | "$unsigned" | "$clog2" => args.iter().all(|x| self.constant(x)),
                "$bits" => true,
                n if n.starts_with("$__xz_") && n.ends_with("_cast") => {
                    args.last().is_some_and(|x| self.constant(x))
                }
                _ => false,
            },
            _ => false,
        }
    }
}

fn dotted(h: &HierarchicalIdentifier) -> String {
    h.path
        .iter()
        .map(|s| s.name.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

/// The variables an lvalue names, as dotted paths.
fn paths(e: &Expression, out: &mut Vec<String>) {
    match &e.kind {
        ExprKind::Ident(h) => out.push(dotted(h)),
        ExprKind::MemberAccess { expr, member } => {
            let n = out.len();
            paths(expr, out);
            for p in &mut out[n..] {
                p.push('.');
                p.push_str(&member.name);
            }
        }
        ExprKind::Index { expr, .. }
        | ExprKind::RangeSelect { expr, .. }
        | ExprKind::Paren(expr) => paths(expr, out),
        ExprKind::Concatenation(v) | ExprKind::StreamOp { exprs: v, .. } => {
            for x in v {
                paths(x, out);
            }
        }
        ExprKind::AssignmentPattern(v) => {
            for x in v {
                paths(x.expr(), out);
            }
        }
        _ => {}
    }
}

/// A statement that can suspend its process: a delay, event control, wait
/// (an intra-assignment one included) or task call.
fn timed(s: &Statement) -> bool {
    match &s.kind {
        StatementKind::TimingControl { .. }
        | StatementKind::Wait { .. }
        | StatementKind::WaitFork
        | StatementKind::WaitOrder { .. } => true,
        StatementKind::Expr(e) => matches!(e.kind, ExprKind::Call { .. }),
        // `a = #5 b;` and `a = @e b;` (§9.4.5) suspend too.
        StatementKind::BlockingAssign { rvalue, .. } => Simulator::intra_timing_suspends(rvalue),
        StatementKind::SeqBlock { stmts, .. } | StatementKind::ParBlock { stmts, .. } => {
            stmts.iter().any(timed)
        }
        StatementKind::If {
            then_stmt,
            else_stmt,
            ..
        } => timed(then_stmt) || else_stmt.as_deref().is_some_and(timed),
        StatementKind::Case { items, .. } => items.iter().any(|i| timed(&i.stmt)),
        StatementKind::For { body, .. }
        | StatementKind::Foreach { body, .. }
        | StatementKind::While { body, .. }
        | StatementKind::DoWhile { body, .. }
        | StatementKind::Repeat { body, .. }
        | StatementKind::Forever { body } => timed(body),
        _ => false,
    }
}

/// System tasks and functions that only read their arguments (by prefix:
/// `$display` covers `$displayh`).
fn reads_only(name: &str) -> bool {
    const READ: &str = "$display $write $strobe $monitor $fdisplay $fwrite $fstrobe $fmonitor \
        $error $warning $info $fatal $sformatf $psprintf $signed $unsigned $clog2 $bits $size \
        $countones $countbits $onehot $isunknown $past $rose $fell $stable $changed $sampled \
        $finish $stop $time $__xz_cov $__xz_size_cast $__xz_type_cast $__xz_named_cast";
    READ.split_whitespace().any(|r| name.starts_with(r))
}
