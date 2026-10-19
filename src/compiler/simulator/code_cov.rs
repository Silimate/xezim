//! Code coverage: statement, branch and toggle counts, collected when the run
//! asks for them (`--code-coverage`, `+cover`) and written to the results
//! file beside the functional coverage.
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
//! time slot, off the dirty set the waveform writers share.
//!
//! Code a constant condition rules out (a `case` on a parameter) is left
//! out of the totals: instrumentation records each `Decision` a constant
//! may make, and `cov_resolve_constants` makes them once the design is
//! compiled, with the evaluator the run uses, and marks the sites excluded.
use super::*;
use crate::ast::Span;
use crate::compiler::elaborate::InitialBlock;

pub const KIND_STATEMENT: u8 = 1;
pub const KIND_BRANCH: u8 = 2;
pub const KIND_TOGGLE: u8 = 4;
/// Result names of the kinds, bit `i` of a kind mask naming entry `i`.
const KIND_NAMES: [&str; 3] = ["statement", "branch", "toggle"];
type Tallies = [Tally; KIND_NAMES.len()];

/// `$__xz_cov(<n>)`: counts counter `n` (a statement or a branch arm).
pub(crate) const HIT_TASK: &str = "$__xz_cov";
/// `$__xz_covc(<n>, cond)`: yields `cond`, counting counter `n` when it is
/// true and `n + 1` when it is false (a `?:` condition).
pub(crate) const COND_FN: &str = "$__xz_covc";

/// Library packages left out unless `--code-coverage-scope` names them.
const LIBRARY_PACKAGES: [&str; 2] = ["std", "uvm_pkg"];

/// What the command line asked for. `scopes` limits every kind to the
/// instances at or below the listed paths and to the listed packages (empty:
/// the whole design). `exclude_input_ports` leaves input and inout ports out
/// of toggle coverage, so the net that drives one carries its toggles
/// (`--no-toggle-input-ports`); by default each instance counts its own.
/// `keep_constant` keeps the statements and branch arms a constant
/// condition rules out in the totals (see `Decision`).
#[derive(Clone, Debug, Default)]
pub struct CodeCoverage {
    pub kinds: u8,
    pub scopes: Vec<String>,
    pub exclude_input_ports: bool,
    pub keep_constant: bool,
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
/// `branch` (`b`), `toggle` (`t`) or `all`, separated by commas.
pub fn parse_kinds(spec: &str) -> Result<u8, String> {
    let mut kinds = 0u8;
    for k in spec.split(',').map(str::trim).filter(|k| !k.is_empty()) {
        kinds |= match k {
            "stmt" | "statement" | "s" => KIND_STATEMENT,
            "branch" | "b" => KIND_BRANCH,
            "toggle" | "t" => KIND_TOGGLE,
            "all" => KIND_STATEMENT | KIND_BRANCH | KIND_TOGGLE,
            _ => {
                return Err(format!(
                    "unknown code coverage kind '{}' (want stmt, branch, toggle or all)",
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
type Region = (std::ops::Range<usize>, std::ops::Range<usize>);

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
    /// The `?:` branches inside each condition.
    conds: Vec<std::ops::Range<usize>>,
}

/// Per-bit rise/fall counts of the tracked signals.
pub(crate) struct Toggle {
    /// Signal id -> tracked index, `u32::MAX` when not tracked.
    slot: Vec<u32>,
    ids: Vec<u32>,
    width: Vec<u32>,
    /// First word of each tracked signal in `prev_v` / `prev_x`.
    word0: Vec<u32>,
    /// First bit of each tracked signal in `rise` / `fall`.
    bit0: Vec<u32>,
    prev_v: Vec<u64>,
    prev_x: Vec<u64>,
    rise: Vec<u32>,
    fall: Vec<u32>,
    /// (scope, name within the scope, tracked index); aliases of one signal
    /// share the tracked index.
    names: Vec<(u32, String, u32)>,
}

impl Toggle {
    fn sample(&mut self, k: usize, v: u64, x: u64, word: usize) {
        let w = self.word0[k] as usize + word;
        let (pv, px) = (self.prev_v[w], self.prev_x[w]);
        self.prev_v[w] = v;
        self.prev_x[w] = x;
        let known = !(x | px);
        let mut rise = v & !pv & known;
        let mut fall = pv & !v & known;
        let base = self.bit0[k] as usize + word * 64;
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
}

pub(crate) struct CodeCov {
    kinds: u8,
    /// `--code-coverage-constant=keep`: nothing is excluded.
    keep_constant: bool,
    limit: Vec<String>,
    scopes: Vec<Scope>,
    scope_index: HashMap<String, u32>,
    stmts: Vec<StmtSite>,
    branches: Vec<BranchSite>,
    counters: u32,
    /// Continuous assignment (origin span start, end, scope) -> its counter.
    ca_counter: HashMap<(usize, usize, String), u32>,
    /// Instance paths below the top.
    inst_paths: HashSet<String>,
    /// Decisions constants may make, until `cov_resolve_constants`.
    decisions: Vec<Decision>,
    pub(crate) toggle: Option<Toggle>,
}

/// What instrumenting an `if` chain collects: its arms, what each holds,
/// and the `?:` branches in each condition.
#[derive(Default)]
struct Chain {
    arms: Vec<Arm>,
    regions: Vec<Region>,
    conds: Vec<std::ops::Range<usize>>,
}

/// Rewrites one scope's statements; see the module docs.
struct Instr<'a> {
    cov: &'a mut CodeCov,
    kinds: u8,
    scope: u32,
    /// The design whose parameters make conditions constant, or `None` when
    /// nothing is excluded: `--code-coverage-constant=keep`, and scopes
    /// outside the instance tree, whose bare names are not resolved yet.
    consts: Option<&'a ElaboratedModule>,
    /// Names declared in the code being rewritten (variables, subroutine
    /// arguments): never constants, even where they shadow a parameter.
    locals: HashSet<String>,
}

impl<'a> Instr<'a> {
    fn new(
        cov: &'a mut CodeCov,
        kinds: u8,
        scope: u32,
        consts: Option<&'a ElaboratedModule>,
    ) -> Self {
        let consts = consts.filter(|_| !cov.keep_constant);
        Instr {
            cov,
            kinds,
            scope,
            consts,
            locals: HashSet::default(),
        }
    }

    fn counter(&mut self) -> u32 {
        let c = self.cov.counters;
        self.cov.counters += 1;
        c
    }

    fn count(&mut self, span: Span, kind: &'static str, out: &mut Vec<Statement>) {
        if self.kinds & KIND_STATEMENT == 0 {
            return;
        }
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

    /// Run `f`, and return the statement and branch sites it created.
    fn region<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (T, Region) {
        let (s0, b0) = (self.cov.stmts.len(), self.cov.branches.len());
        let r = f(self);
        (r, (s0..self.cov.stmts.len(), b0..self.cov.branches.len()))
    }

    fn decide(
        &mut self,
        choice: Choice,
        site: Option<usize>,
        arms: Vec<Region>,
        conds: Vec<std::ops::Range<usize>>,
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
        self.stmt(s, &mut v);
        block(v, span)
    }

    /// A branch arm: its counter, then the arm's own statements, and the
    /// sites they hold.
    fn arm(&mut self, s: Statement, counter: u32) -> (Statement, Region) {
        let span = s.span;
        let mut v = vec![hit(counter, span)];
        let ((), r) = self.region(|me| me.stmt(s, &mut v));
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
        match s.kind {
            StatementKind::TimingControl {
                control,
                stmt: inner,
            } => {
                self.count(span, kind, &mut v);
                self.stmt(*inner, &mut v);
                Statement::new(
                    StatementKind::TimingControl {
                        control,
                        stmt: Box::new(block(v, span)),
                    },
                    span,
                )
            }
            other => {
                self.count(span, kind, &mut v);
                self.stmt(Statement::new(other, span), &mut v);
                block(v, span)
            }
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
                let stmts = self.stmts(stmts);
                out.push(rebuilt(StatementKind::SeqBlock { name, stmts }));
            }
            // Each fork branch stays one process.
            StatementKind::ParBlock {
                name,
                join_type,
                stmts,
            } => {
                let stmts = stmts.into_iter().map(|c| self.wrap(c)).collect();
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
            } => self.if_stmt(unique_priority, condition, then_stmt, else_stmt, span, out),
            StatementKind::Case {
                unique_priority,
                kind,
                expr,
                items,
            } => self.case_stmt(unique_priority, kind, expr, items, span, out),
            StatementKind::RandCase { items } => {
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
                out.push(rebuilt(StatementKind::RandCase { items }));
            }
            StatementKind::For {
                init,
                condition,
                step,
                body,
            } => {
                self.count(span, "for", out);
                let body = Box::new(self.wrap(*body));
                out.push(rebuilt(StatementKind::For {
                    init,
                    condition,
                    step,
                    body,
                }));
            }
            StatementKind::Foreach { array, vars, body } => {
                self.count(span, "foreach", out);
                let body = Box::new(self.wrap(*body));
                out.push(rebuilt(StatementKind::Foreach { array, vars, body }));
            }
            StatementKind::While { condition, body } => {
                self.count(span, "while", out);
                let body = Box::new(self.wrap(*body));
                out.push(rebuilt(StatementKind::While { condition, body }));
            }
            StatementKind::DoWhile { body, condition } => {
                self.count(span, "do", out);
                let body = Box::new(self.wrap(*body));
                out.push(rebuilt(StatementKind::DoWhile { body, condition }));
            }
            StatementKind::Repeat { count, body } => {
                self.count(span, "repeat", out);
                let body = Box::new(self.wrap(*body));
                out.push(rebuilt(StatementKind::Repeat { count, body }));
            }
            StatementKind::Forever { body } => {
                self.count(span, "forever", out);
                let body = Box::new(self.wrap(*body));
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
                self.count(span, kind, out);
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
                self.count(span, "wait", out);
                let inner = Box::new(self.wrap(*inner));
                out.push(rebuilt(StatementKind::Wait {
                    condition,
                    stmt: inner,
                }));
            }
            // Not statements of their own: declarations, and the markers and
            // bodies the executor and other constructs own.
            kind @ (StatementKind::Null
            | StatementKind::VarDecl { .. }
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
                self.expr(&mut rvalue);
                out.push(rebuilt(StatementKind::BlockingAssign { lvalue, rvalue }));
            }
            StatementKind::NonblockingAssign {
                lvalue,
                delay,
                mut rvalue,
            } => {
                self.count(span, "statement", out);
                self.expr(&mut rvalue);
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
                    _ => {}
                }
                out.push(rebuilt(StatementKind::Expr(e)));
            }
            StatementKind::Return(Some(mut e)) => {
                self.count(span, "statement", out);
                self.expr(&mut e);
                out.push(rebuilt(StatementKind::Return(Some(e))));
            }
            kind => {
                self.count(span, "statement", out);
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
            let (then_stmt, t) = self.region(|me| Box::new(me.wrap(*then_stmt)));
            let (else_stmt, e) = self.region(|me| else_stmt.map(|e| Box::new(me.wrap(*e))));
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
        let ((), (_, c)) = self.region(|me| me.expr(&mut condition));
        chain.conds.push(c);
        let rewritten = self.if_arms(
            unique_priority,
            condition,
            then_stmt,
            else_stmt,
            span,
            true,
            &mut chain,
        );
        if implicit {
            chain.arms.push(Arm {
                span,
                label: "else (implicit)",
                counter: None,
                excluded: false,
            });
        }
        if !has_else {
            chain.regions.push((0..0, 0..0));
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
                    let ((), (_, c)) = self.region(|me| me.expr(&mut cond));
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
        if self.kinds & KIND_BRANCH == 0 {
            for it in items.iter_mut() {
                let st = std::mem::replace(&mut it.stmt, Statement::new(StatementKind::Null, span));
                let (s, r) = self.region(|me| me.wrap(st));
                it.stmt = s;
                regions.push(r);
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
        // The implicit `default` holds nothing.
        regions.push((0..0, 0..0));
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

    /// Wrap the condition of every `?:` in `e` (see `COND_FN`). Stops at
    /// system calls, whose arguments some tasks read by shape. A `?:` whose
    /// condition constants can decide makes a `Choice::Ternary`, and an
    /// `&&`, `||`, `&` or `|` with a `?:` inside a `Choice::Operand`.
    fn expr(&mut self, e: &mut Expression) {
        if self.kinds & KIND_BRANCH == 0 || !has_ternary(e) {
            return;
        }
        let espan = e.span;
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
                self.expr(condition);
                let ((), t) = self.region(|me| me.expr(then_expr));
                let ((), f) = self.region(|me| me.expr(else_expr));
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
                let cond =
                    std::mem::replace(&mut **condition, Expression::new(ExprKind::Empty, cspan));
                **condition = Expression::new(
                    ExprKind::SystemCall {
                        name: COND_FN.to_string(),
                        args: vec![number(tc, cspan), cond],
                    },
                    cspan,
                );
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
    rise: Vec<u32>,
    fall: Vec<u32>,
}

#[derive(Default)]
struct Report {
    stmts: Vec<StmtRow>,
    branches: Vec<BranchRow>,
    toggles: Vec<ToggleRow>,
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
    }

    /// Add another instance of the same design unit (rows keyed by source
    /// position and signal name).
    fn merge(&mut self, o: &Report) {
        self.stmts.extend(o.stmts.iter().cloned());
        self.branches.extend(o.branches.iter().cloned());
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
        self.normalize();
    }

    fn json(&self, kinds: u8, indent: &str, out: &mut String) {
        let t = self.tallies();
        for (i, name) in KIND_NAMES.iter().enumerate() {
            if kinds & (1 << i) != 0 {
                out.push_str(&format!("{}\"{}\": {},\n", indent, name, t[i].json()));
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
                    "{}{{\"signal\": \"{}\", \"width\": {}, \"rise\": [{}], \"fall\": [{}]}}",
                    if i == 0 { "" } else { ", " },
                    json_escape(&r.name),
                    r.rise.len(),
                    list(&r.rise),
                    list(&r.fall)
                ));
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
            limit: cfg.scopes,
            scopes: Vec::new(),
            scope_index: HashMap::default(),
            stmts: Vec::new(),
            branches: Vec::new(),
            counters: 0,
            ca_counter: HashMap::default(),
            inst_paths: inst_paths.clone(),
            decisions: Vec::new(),
            toggle: None,
        };
        // Initial blocks and continuous assignments are instrumented where
        // `compile` and `build_comb_entries` take them (`cov_initial`,
        // `cov_contassign`): most of them are still lazy here, and building
        // them early would change the order and the passes they go through.
        if kinds & (KIND_STATEMENT | KIND_BRANCH) != 0 {
            let mut program = std::mem::take(&mut self.module.program_initial_blocks);
            for ib in program.iter_mut() {
                if let Some(sc) =
                    Self::cov_instance_scope(&self.module, &mut cov, &inst_paths, &ib.scope)
                {
                    let span = ib.stmt.span;
                    let s =
                        std::mem::replace(&mut ib.stmt, Statement::new(StatementKind::Null, span));
                    ib.stmt = Instr::new(&mut cov, kinds, sc, Some(&self.module)).wrap(s);
                }
            }
            self.module.program_initial_blocks = program;
            let mut fin = std::mem::take(&mut self.module.final_blocks);
            for fb in fin.iter_mut() {
                if let Some(sc) =
                    Self::cov_instance_scope(&self.module, &mut cov, &inst_paths, &fb.scope)
                {
                    let span = fb.stmt.span;
                    let s =
                        std::mem::replace(&mut fb.stmt, Statement::new(StatementKind::Null, span));
                    fb.stmt = Instr::new(&mut cov, kinds, sc, Some(&self.module)).wrap(s);
                }
            }
            self.module.final_blocks = fin;
            let mut always = std::mem::take(&mut self.module.always_blocks);
            for ab in always.iter_mut() {
                if let Some(sc) =
                    Self::cov_instance_scope(&self.module, &mut cov, &inst_paths, &ab.scope)
                {
                    let kind = match ab.kind {
                        crate::ast::decl::AlwaysKind::Always => "always",
                        crate::ast::decl::AlwaysKind::AlwaysComb => "always_comb",
                        crate::ast::decl::AlwaysKind::AlwaysFf => "always_ff",
                        crate::ast::decl::AlwaysKind::AlwaysLatch => "always_latch",
                    };
                    let span = ab.stmt.span;
                    let s =
                        std::mem::replace(&mut ab.stmt, Statement::new(StatementKind::Null, span));
                    ab.stmt = Instr::new(&mut cov, kinds, sc, Some(&self.module)).always(s, kind);
                }
            }
            self.module.always_blocks = always;
            // Subroutines: a package's belong to the package, the others to
            // the instance their qualified name starts with.
            // A bare-named copy of an instance's subroutine (same source
            // span) is not the top module's own.
            let dotted: HashSet<(String, usize)> = self
                .module
                .functions
                .iter()
                .map(|(k, f)| (k, f.span))
                .chain(self.module.tasks.iter().map(|(k, t)| (k, t.span)))
                .filter_map(|(k, sp)| {
                    let (_, leaf) = k.rsplit_once('.')?;
                    Some((leaf.to_string(), sp.start))
                })
                .collect();
            let copy = |k: &str, sp: Span| {
                !k.contains('.') && !k.contains("::") && dotted.contains(&(k.to_string(), sp.start))
            };
            let mut functions = std::mem::take(&mut self.module.functions).into_map();
            for (key, fd) in functions.iter_mut() {
                if copy(key, fd.span) && !self.module.pkg_subr_owner.contains_key(key.as_str()) {
                    continue;
                }
                if let Some(sc) = self.cov_subroutine_scope(&mut cov, &inst_paths, key) {
                    let items = std::mem::take(&mut fd.items);
                    fd.items = self
                        .cov_subroutine_instr(&mut cov, kinds, sc, key, &fd.ports)
                        .stmts(items);
                }
            }
            self.module.functions = xezim_core::elaborate::FuncTable::from_map(functions);
            let mut tasks = std::mem::take(&mut self.module.tasks);
            for (key, td) in tasks.iter_mut() {
                if copy(key, td.span) && !self.module.pkg_subr_owner.contains_key(key.as_str()) {
                    continue;
                }
                if let Some(sc) = self.cov_subroutine_scope(&mut cov, &inst_paths, key) {
                    let items = std::mem::take(&mut td.items);
                    td.items = self
                        .cov_subroutine_instr(&mut cov, kinds, sc, key, &td.ports)
                        .stmts(items);
                }
            }
            self.module.tasks = tasks;
            self.cov_instrument_classes(&mut cov);
        }
        if kinds & KIND_TOGGLE != 0 {
            let names = std::mem::take(&mut self.code_cov_toggle_names);
            let names = self.cov_drop_fixed(names);
            cov.toggle = Some(self.cov_build_toggle(&mut cov, &inst_paths, names));
            self.enable_dump_dirty_tracking();
        }
        self.code_cov_hits = vec![0; cov.counters as usize];
        self.code_cov = Some(Box::new(cov));
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
        if kinds & (KIND_STATEMENT | KIND_BRANCH) != 0 && !Self::cov_synthesized_initial(&ib.stmt) {
            let paths = std::mem::take(&mut cov.inst_paths);
            if let Some(sc) = Self::cov_instance_scope(&self.module, &mut cov, &paths, &ib.scope) {
                let span = ib.stmt.span;
                let s = std::mem::replace(&mut ib.stmt, Statement::new(StatementKind::Null, span));
                ib.stmt = Instr::new(&mut cov, kinds, sc, Some(&self.module)).wrap(s);
            }
            cov.inst_paths = paths;
            self.code_cov_hits.resize(cov.counters as usize, 0);
        }
        self.code_cov = Some(cov);
        ib
    }

    /// Instrument a continuous assignment as `build_comb_entries` takes it:
    /// its `?:`s become branches, and it gets its statement counter (shared
    /// by the pieces one assignment expands to), which is returned. Takes
    /// the fields it needs: the caller's iterator holds `module`.
    pub(super) fn cov_contassign(
        module: &ElaboratedModule,
        code_cov: &mut Option<Box<CodeCov>>,
        hits: &mut Vec<u64>,
        ca: &mut crate::compiler::elaborate::ContinuousAssignment,
    ) -> Option<u32> {
        let (span, scope) = ca.origin.clone()?;
        let cov = code_cov.as_mut()?;
        let kinds = cov.kinds;
        let scope = if scope.is_empty() {
            Self::cov_lhs_base(&ca.lhs)
                .and_then(|n| n.rsplit_once('.').map(|(p, _)| p.to_string()))
                .unwrap_or_default()
        } else {
            scope
        };
        let paths = std::mem::take(&mut cov.inst_paths);
        let sc = Self::cov_instance_scope(module, cov, &paths, &scope);
        cov.inst_paths = paths;
        let sc = sc?;
        let mut ins = Instr::new(cov, kinds, sc, Some(module));
        ins.expr(&mut ca.rhs);
        let mut counter = None;
        if kinds & KIND_STATEMENT != 0 {
            let key = (span.start, span.end, scope);
            counter = Some(match ins.cov.ca_counter.get(&key) {
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
            });
        }
        hits.resize(cov.counters as usize, 0);
        counter
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
    fn cov_instance_scope(
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

    /// The rewriter for subroutine `key`: an instance's sees its parameters,
    /// and its arguments shadow them; a package's excludes nothing.
    fn cov_subroutine_instr<'a>(
        &'a self,
        cov: &'a mut CodeCov,
        kinds: u8,
        scope: u32,
        key: &str,
        ports: &[crate::ast::decl::FunctionPort],
    ) -> Instr<'a> {
        let pkg = key.contains("::") || self.module.pkg_subr_owner.contains_key(key);
        let mut ins = Instr::new(cov, kinds, scope, (!pkg).then_some(&self.module));
        for p in ports {
            ins.locals.insert(p.name.name.clone());
            ins.locals.insert(format!("{}.{}", key, p.name.name));
        }
        ins
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
            let mut dead = |(s, b): &Region| {
                for x in &mut cov.stmts[s.clone()] {
                    x.excluded = true;
                }
                for x in &mut cov.branches[b.clone()] {
                    x.excluded = true;
                }
            };
            for (f, r) in fates.iter().zip(&d.arms) {
                if *f == Fate::Dead {
                    dead(r);
                }
            }
            for (out, b) in conds.iter().zip(&d.conds) {
                if *out {
                    dead(&(0..0, b.clone()));
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
        let kinds = cov.kinds;
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
                let items = match &mut method.kind {
                    crate::ast::decl::ClassMethodKind::Function(fd)
                    | crate::ast::decl::ClassMethodKind::Extern(fd) => &mut fd.items,
                    crate::ast::decl::ClassMethodKind::Task(td) => &mut td.items,
                    crate::ast::decl::ClassMethodKind::PureVirtual(_) => continue,
                };
                let body = std::mem::take(items);
                *items = Instr::new(&mut *cov, kinds, sc, None).stmts(body);
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
        if let Some(h) = self.code_cov_hits.get_mut(c as usize) {
            *h += 1;
        }
    }

    /// `TsInsn::CovHit`: count, and note the count on the save list so a
    /// bail takes it back (`ts_restore_saved`) before the block runs again.
    #[inline(never)]
    pub(super) fn ts_cov_hit(&mut self, c: u32) {
        self.cov_count(c);
        self.ts_save_list.push((c | TS_SAVE_COV, 0, 0));
    }

    /// Count a coverage marker's counter (interpreter side).
    pub(super) fn cov_hit(&mut self, arg: Option<&Expression>) {
        if let Some(c) = arg
            .and_then(marker_id)
            .and_then(|id| self.code_cov_hits.get_mut(id as usize))
        {
            *c += 1;
        }
    }

    /// `$__xz_covc(<n>, cond)`: count the condition's outcome, yield it.
    pub(super) fn cov_cond(&mut self, args: &[Expression]) -> Value {
        let Some(cond) = args.get(1) else {
            return Value::new(1);
        };
        let v = self.eval_expr(cond);
        if let Some(id) = args.first().and_then(marker_id) {
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

    /// Nets and variables toggle coverage tracks, by name, taken while the
    /// elaboration signal table still holds their declarations: the integral
    /// ones of instances, ports included, without unpacked arrays, `time`
    /// variables, subroutine variables and class properties. An input or
    /// inout port is a net of its instance holding the value its connection
    /// gives it (§23.3.3): a whole-net connection collapsed onto the actual
    /// shares the actual's counts, an expression or select toggles as its
    /// value, and a constant or open port never toggles.
    /// `exclude_input_ports` leaves input and inout ports to the signal that
    /// drives them.
    ///
    /// Each name carries what `cov_drop_fixed` needs: `None` for a port, a net
    /// and a variable of a struct, enum or named type, which always stay
    /// tracked; for any other variable, whether a constant declaration
    /// initializer gave it its elaborated value (a bit that is not x, or is
    /// set in a 2-state variable). One declared in a generate block counts
    /// as not initialized: the reference simulator keeps those.
    pub(super) fn cov_toggle_candidates(module: &ElaboratedModule) -> Vec<(String, Option<bool>)> {
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
        let mut out: Vec<(String, Option<bool>)> = module
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
                (name.clone(), var.then(|| init && !nested(n)))
            })
            .collect();
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
    fn cov_drop_fixed(&self, names: Vec<(String, Option<bool>)>) -> Vec<String> {
        let index: HashMap<String, usize> = names
            .iter()
            .enumerate()
            .filter(|(_, (_, f))| f.is_some())
            .map(|(i, (n, _))| (n.clone(), i))
            .collect();
        if index.is_empty() {
            return names.into_iter().map(|(n, _)| n).collect();
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
            .filter(|(i, (_, f))| match f {
                Some(init) => c.more[*i] || c.once[*i] != u32::from(!*init),
                None => true,
            })
            .map(|(_, (n, _))| n)
            .collect()
    }

    fn cov_build_toggle(
        &mut self,
        cov: &mut CodeCov,
        inst_paths: &HashSet<String>,
        names: Vec<String>,
    ) -> Toggle {
        let mut t = Toggle {
            slot: vec![u32::MAX; self.signal_table.len()],
            ids: Vec::new(),
            width: Vec::new(),
            word0: Vec::new(),
            bit0: Vec::new(),
            prev_v: Vec::new(),
            prev_x: Vec::new(),
            rise: Vec::new(),
            fall: Vec::new(),
            names: Vec::new(),
        };
        let top_prefix = format!("{}.", self.module.name);
        for name in names {
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
                t.word0.push(t.prev_v.len() as u32);
                t.bit0.push(t.rise.len() as u32);
                let words = width.div_ceil(64) as usize;
                let v = &self.signal_table[id];
                for w in 0..words {
                    let n = (width as usize - w * 64).min(64);
                    let (pv, px) = v.slice_bits_swar(w * 64, n);
                    t.prev_v.push(pv);
                    t.prev_x.push(px);
                }
                t.rise.resize(t.rise.len() + width as usize, 0);
                t.fall.resize(t.fall.len() + width as usize, 0);
                k
            };
            t.names.push((sc, rest.to_string(), k));
        }
        t
    }

    /// End of a time slot: compare every tracked signal written since the
    /// last one with its previous value.
    pub(super) fn cov_toggle_sample(&mut self) {
        let Some(t) = self.code_cov.as_mut().and_then(|c| c.toggle.as_mut()) else {
            return;
        };
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
                t.sample(k, v & mask, x & mask, 0);
            } else {
                for w in 0..width.div_ceil(64) {
                    let n = (width - w * 64).min(64);
                    let (v, x) = self.signal_table[id].slice_bits_swar(w * 64, n);
                    t.sample(k, v, x, w);
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
            let arms = b
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
            reports[b.scope as usize].branches.push(BranchRow {
                file,
                line,
                start: b.span.start,
                kind: b.kind,
                arms,
                excluded: b.excluded,
            });
        }
        if let Some(t) = &cov.toggle {
            for (sc, name, k) in &t.names {
                let k = *k as usize;
                let (b, w) = (t.bit0[k] as usize, t.width[k] as usize);
                reports[*sc as usize].toggles.push(ToggleRow {
                    name: name.clone(),
                    rise: t.rise[b..b + w].to_vec(),
                    fall: t.fall[b..b + w].to_vec(),
                });
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
        for r in &reports {
            for (t, x) in total.iter_mut().zip(r.tallies()) {
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
                json.push_str(&format!("    \"{}\": {},\n", n, total[i].json()));
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
