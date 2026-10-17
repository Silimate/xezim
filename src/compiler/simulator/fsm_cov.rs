//! FSM coverage: the finite state machines of the design, how often the run
//! visited each state and how often it took each transition. Collected as a
//! code coverage kind (`--code-coverage=fsm`, `+cover=f`); the rules are in
//! docs/coverage-guide.md, "FSMs".
//!
//! Recognition runs on the elaborated always blocks, before the other kinds
//! instrument them. A state register is a variable of 2 to 64 bits that a
//! single edge-triggered always block (the state process) assigns as a
//! whole, where
//! - that block, or the combinational block computing the value it loads
//!   (`state <= next_state`), tests the variable in a `case` or an `if`, and
//! - every assignment of the variable (and of the next-state variable)
//!   yields constants: parameters, enum members, literals, the variable
//!   itself, an expression of the variable under a branch that fixes its
//!   value (`2'd1: c <= c + 1`), or a `?:` of those.
//!
//! The states are the values those assignments yield and the constants the
//! tests compare with. A transition is an assignment that moves the variable
//! from one state to another under the branches around it; an assignment
//! outside every test of the variable (a reset, a `next_state = IDLE`
//! default) moves it from every state. These follow the commercial
//! simulator.
//!
//! Counting: the state process gets two kinds of marker, `$__xz_cov` calls
//! with ids from `FSM_MARK` up. One where its body starts notes whether
//! anything the block reads (or the state variable) changed since its
//! previous clock run; one in front of each assignment of the state variable
//! notes the write. At the end of the time slot the variable's value counts
//! as a visit when the block ran with changed inputs, and as a transition
//! from the value the previous sample saw when the two differ. The commercial
//! simulator skips a clocked block whose inputs are unchanged, so a state
//! held by `state <= next_state` counts on the clock that enters it and the
//! next one, not on every clock; the input check reproduces that, with the
//! inputs also compared at the end of each time slot (a change that is undone
//! before the next edge still counts) and runs an asynchronous reset starts
//! kept out of the comparison's reference.
use super::code_cov::CodeCov;
use super::*;
use crate::ast::Span;
use std::collections::BTreeSet;

/// Coverage marker ids from here up are FSM markers, not counters:
/// `FSM_MARK + 2p` starts state process `p`, `FSM_MARK + 2f + 1` precedes an
/// assignment of FSM `f`'s state variable. `cov_count` hands them to
/// `fsm_mark`.
pub(crate) const FSM_MARK: u32 = 1 << 29;

/// More states than this and the variable is not taken for an FSM.
const MAX_STATES: usize = 256;

/// The states a branch leaves the variable in.
#[derive(Clone, Debug)]
enum Ctx {
    /// Any state but these.
    Any(BTreeSet<u64>),
    /// One of these.
    Set(BTreeSet<u64>),
}

impl Ctx {
    fn all() -> Ctx {
        Ctx::Any(BTreeSet::new())
    }

    fn one(v: u64) -> Ctx {
        Ctx::Set(BTreeSet::from([v]))
    }

    fn and(&self, o: &Ctx) -> Ctx {
        match (self, o) {
            (Ctx::Set(a), Ctx::Set(b)) => Ctx::Set(a.intersection(b).copied().collect()),
            (Ctx::Set(a), Ctx::Any(e)) | (Ctx::Any(e), Ctx::Set(a)) => {
                Ctx::Set(a.difference(e).copied().collect())
            }
            (Ctx::Any(a), Ctx::Any(b)) => Ctx::Any(a.union(b).copied().collect()),
        }
    }

    fn or(&self, o: &Ctx) -> Ctx {
        match (self, o) {
            (Ctx::Set(a), Ctx::Set(b)) => Ctx::Set(a.union(b).copied().collect()),
            (Ctx::Set(a), Ctx::Any(e)) | (Ctx::Any(e), Ctx::Set(a)) => {
                Ctx::Any(e.difference(a).copied().collect())
            }
            (Ctx::Any(a), Ctx::Any(b)) => Ctx::Any(a.intersection(b).copied().collect()),
        }
    }

    fn not(&self) -> Ctx {
        match self {
            Ctx::Set(a) => Ctx::Any(a.clone()),
            Ctx::Any(e) => Ctx::Set(e.clone()),
        }
    }

    /// Narrowed by some test of the variable (a reset or a default is not).
    fn specific(&self) -> bool {
        !matches!(self, Ctx::Any(e) if e.is_empty())
    }

    fn holds(&self, v: u64) -> bool {
        match self {
            Ctx::Set(a) => a.contains(&v),
            Ctx::Any(e) => !e.contains(&v),
        }
    }
}

/// An assignment that yields `value` when the variable is in `ctx`.
#[derive(Clone)]
struct Rec {
    ctx: Ctx,
    value: u64,
    span: Span,
    /// In the state process itself, not in the next-state logic.
    direct: bool,
}

/// System functions of constants that are constants.
const PURE: [&str; 5] = ["$signed", "$unsigned", "$clog2", "$bits", "$size"];

/// The name of a whole-variable reference (`u.state`), `None` for anything
/// else (a select, a member, an expression).
fn whole(e: &Expression) -> Option<&str> {
    match &e.kind {
        ExprKind::Ident(h)
            if h.root.is_none() && h.path.len() == 1 && h.path[0].selects.is_empty() =>
        {
            Some(h.path[0].name.name.as_str())
        }
        _ => None,
    }
}

/// Every identifier `e` names, selects included.
fn idents<'a>(e: &'a Expression, out: &mut Vec<&'a str>) {
    match &e.kind {
        ExprKind::Ident(h) => {
            if let Some(s) = h.path.first() {
                out.push(s.name.name.as_str());
            }
            for s in &h.path {
                for x in &s.selects {
                    idents(x, out);
                }
            }
        }
        ExprKind::Unary { operand, .. } => idents(operand, out),
        ExprKind::Binary { left, right, .. } => {
            idents(left, out);
            idents(right, out);
        }
        ExprKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            idents(condition, out);
            idents(then_expr, out);
            idents(else_expr, out);
        }
        ExprKind::Concatenation(v) => v.iter().for_each(|x| idents(x, out)),
        ExprKind::Replication { count, exprs } => {
            idents(count, out);
            exprs.iter().for_each(|x| idents(x, out));
        }
        ExprKind::Call { args, .. } => args.iter().for_each(|x| idents(x, out)),
        ExprKind::SystemCall { name, args } => {
            // A cast's first argument names a type or a width.
            let skip = matches!(name.as_str(), "$__xz_named_cast" | "$__xz_size_cast") as usize;
            args.iter().skip(skip).for_each(|x| idents(x, out));
        }
        ExprKind::Index { expr, index } => {
            idents(expr, out);
            idents(index, out);
        }
        ExprKind::RangeSelect {
            expr, left, right, ..
        } => {
            idents(expr, out);
            idents(left, out);
            idents(right, out);
        }
        ExprKind::MemberAccess { expr, .. } | ExprKind::Paren(expr) => idents(expr, out),
        ExprKind::Inside { expr, ranges } => {
            idents(expr, out);
            ranges.iter().for_each(|x| idents(x, out));
        }
        ExprKind::Range(a, b) => {
            idents(a, out);
            idents(b, out);
        }
        _ => {}
    }
}

/// The variable references in `e`: identifiers, and selects of an
/// identifier (`rst[0]`, the port actual `.rst(rst[k])` leaves).
fn refs<'a>(e: &'a Expression, out: &mut Vec<&'a Expression>) {
    match &e.kind {
        ExprKind::Ident(h) => {
            out.push(e);
            for s in &h.path {
                for x in &s.selects {
                    refs(x, out);
                }
            }
        }
        ExprKind::Index { expr, index } if matches!(expr.kind, ExprKind::Ident(_)) => {
            out.push(e);
            refs(index, out);
        }
        ExprKind::RangeSelect {
            expr, left, right, ..
        } if matches!(expr.kind, ExprKind::Ident(_)) => {
            out.push(e);
            refs(left, out);
            refs(right, out);
        }
        ExprKind::Unary { operand, .. } => refs(operand, out),
        ExprKind::Binary { left, right, .. } => {
            refs(left, out);
            refs(right, out);
        }
        ExprKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            refs(condition, out);
            refs(then_expr, out);
            refs(else_expr, out);
        }
        ExprKind::Concatenation(v) => v.iter().for_each(|x| refs(x, out)),
        ExprKind::Replication { count, exprs } => {
            refs(count, out);
            exprs.iter().for_each(|x| refs(x, out));
        }
        ExprKind::Call { args, .. } => args.iter().for_each(|x| refs(x, out)),
        ExprKind::SystemCall { name, args } => {
            let skip = matches!(name.as_str(), "$__xz_named_cast" | "$__xz_size_cast") as usize;
            args.iter().skip(skip).for_each(|x| refs(x, out));
        }
        ExprKind::Index { expr, index } => {
            refs(expr, out);
            refs(index, out);
        }
        ExprKind::RangeSelect {
            expr, left, right, ..
        } => {
            refs(expr, out);
            refs(left, out);
            refs(right, out);
        }
        ExprKind::MemberAccess { expr, .. } | ExprKind::Paren(expr) => refs(expr, out),
        ExprKind::Inside { expr, ranges } => {
            refs(expr, out);
            ranges.iter().for_each(|x| refs(x, out));
        }
        ExprKind::Range(a, b) => {
            refs(a, out);
            refs(b, out);
        }
        _ => {}
    }
}

/// The expressions of a statement, timing controls aside.
fn stmt_exprs<'a>(s: &'a Statement, out: &mut Vec<&'a Expression>) {
    match &s.kind {
        StatementKind::SeqBlock { stmts, .. } | StatementKind::ParBlock { stmts, .. } => {
            stmts.iter().for_each(|x| stmt_exprs(x, out))
        }
        StatementKind::If {
            condition,
            then_stmt,
            else_stmt,
            ..
        } => {
            out.push(condition);
            stmt_exprs(then_stmt, out);
            if let Some(e) = else_stmt {
                stmt_exprs(e, out);
            }
        }
        StatementKind::Case { expr, items, .. } => {
            out.push(expr);
            for it in items {
                out.extend(it.patterns.iter());
                stmt_exprs(&it.stmt, out);
            }
        }
        StatementKind::For {
            condition, body, ..
        } => {
            if let Some(c) = condition {
                out.push(c);
            }
            stmt_exprs(body, out);
        }
        StatementKind::While { condition, body } | StatementKind::DoWhile { body, condition } => {
            out.push(condition);
            stmt_exprs(body, out);
        }
        StatementKind::Repeat { count, body } => {
            out.push(count);
            stmt_exprs(body, out);
        }
        StatementKind::Foreach { body, .. } | StatementKind::Forever { body } => {
            stmt_exprs(body, out)
        }
        StatementKind::TimingControl { stmt, .. } | StatementKind::Wait { stmt, .. } => {
            stmt_exprs(stmt, out)
        }
        StatementKind::BlockingAssign { lvalue, rvalue }
        | StatementKind::NonblockingAssign { lvalue, rvalue, .. } => {
            selects(lvalue, out);
            out.push(rvalue);
        }
        StatementKind::Expr(e) => out.push(e),
        _ => {}
    }
}

/// The expressions an assignment target reads: its indices and selects,
/// not the variable it writes.
fn selects<'a>(lv: &'a Expression, out: &mut Vec<&'a Expression>) {
    match &lv.kind {
        ExprKind::Ident(h) => {
            for s in &h.path {
                out.extend(s.selects.iter());
            }
        }
        ExprKind::Index { expr, index } => {
            selects(expr, out);
            out.push(index);
        }
        ExprKind::RangeSelect {
            expr, left, right, ..
        } => {
            selects(expr, out);
            out.push(left);
            out.push(right);
        }
        ExprKind::MemberAccess { expr, .. } | ExprKind::Paren(expr) => selects(expr, out),
        ExprKind::Concatenation(v) => v.iter().for_each(|x| selects(x, out)),
        _ => {}
    }
}

/// The whole-variable assignments of a statement (`full`) and the variables
/// it assigns in part (`part`).
fn targets<'a>(s: &'a Statement, full: &mut BTreeSet<&'a str>, part: &mut BTreeSet<&'a str>) {
    match &s.kind {
        StatementKind::SeqBlock { stmts, .. } | StatementKind::ParBlock { stmts, .. } => {
            stmts.iter().for_each(|x| targets(x, full, part))
        }
        StatementKind::If {
            then_stmt,
            else_stmt,
            ..
        } => {
            targets(then_stmt, full, part);
            if let Some(e) = else_stmt {
                targets(e, full, part);
            }
        }
        StatementKind::Case { items, .. } => {
            items.iter().for_each(|it| targets(&it.stmt, full, part))
        }
        StatementKind::For { body, .. }
        | StatementKind::Foreach { body, .. }
        | StatementKind::While { body, .. }
        | StatementKind::DoWhile { body, .. }
        | StatementKind::Repeat { body, .. }
        | StatementKind::Forever { body } => targets(body, full, part),
        StatementKind::TimingControl { stmt, .. } | StatementKind::Wait { stmt, .. } => {
            targets(stmt, full, part)
        }
        StatementKind::BlockingAssign { lvalue, .. }
        | StatementKind::NonblockingAssign { lvalue, .. } => match whole(lvalue) {
            Some(n) => {
                full.insert(n);
            }
            None => {
                let mut v = Vec::new();
                idents(lvalue, &mut v);
                if let Some(n) = v.first() {
                    part.insert(n);
                }
            }
        },
        _ => {}
    }
}

/// An always block's body below its event control, and whether that control
/// has an edge (a clocked block) or none (a combinational one). `None` for a
/// block that is neither, such as `always #5 clk = ~clk`.
fn body(ab: &AlwaysBlock) -> Option<(&Statement, bool)> {
    match (&ab.kind, &ab.stmt.kind) {
        (AlwaysKind::AlwaysComb, StatementKind::TimingControl { .. }) => None,
        (AlwaysKind::AlwaysComb, _) => Some((&ab.stmt, false)),
        (AlwaysKind::AlwaysLatch, _) => None,
        (
            _,
            StatementKind::TimingControl {
                control: TimingControl::Event(ev),
                stmt,
            },
        ) => {
            let edge = match ev {
                EventControl::EventExpr(items) => items.iter().any(|i| i.edge.is_some()),
                _ => false,
            };
            Some((&**stmt, edge))
        }
        _ => None,
    }
}

/// A constant: literals, parameters (a package's too, `pkg::K`) and enum
/// members, and operators and casts of those.
fn constant(sim: &Simulator, e: &Expression) -> bool {
    let params = &sim.module.parameters;
    let mut ok = true;
    let mut stack = vec![e];
    while let Some(x) = stack.pop() {
        match &x.kind {
            ExprKind::Ident(h) => {
                let segs: Vec<&str> = h.path.iter().map(|s| s.name.name.as_str()).collect();
                ok &= match segs.as_slice() {
                    [n] => params.contains_key(*n),
                    [p, n] => {
                        params.contains_key(format!("{}::{}", p, n).as_str())
                            || (params.contains_key(*n) && !sim.signal_name_to_id.contains_key(*p))
                    }
                    _ => false,
                };
                for s in &h.path {
                    stack.extend(s.selects.iter());
                }
            }
            ExprKind::Number(_) => {}
            ExprKind::Unary { operand, .. } => stack.push(operand),
            ExprKind::Binary { left, right, .. } => {
                stack.push(left);
                stack.push(right);
            }
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                stack.push(condition);
                stack.push(then_expr);
                stack.push(else_expr);
            }
            ExprKind::Paren(i) => stack.push(i),
            ExprKind::Concatenation(v) => stack.extend(v.iter()),
            ExprKind::Replication { count, exprs } => {
                stack.push(count);
                stack.extend(exprs.iter());
            }
            ExprKind::SystemCall { name, args } => match name.as_str() {
                // A cast's first argument names a type or a width.
                "$__xz_named_cast" | "$__xz_size_cast" | "$__xz_type_cast" => {
                    stack.extend(args.iter().skip(1))
                }
                n if PURE.contains(&n) => stack.extend(args.iter()),
                _ => ok = false,
            },
            _ => ok = false,
        }
    }
    ok
}

/// The variables `e` compares with something (`==`, `!=`, `===`, `!==`,
/// `inside`), by name under `prefix`.
fn tests(e: &Expression, prefix: &str, out: &mut HashSet<String>) {
    let mut stack = vec![e];
    while let Some(x) = stack.pop() {
        match &x.kind {
            ExprKind::Binary { op, left, right } => {
                if matches!(
                    op,
                    BinaryOp::Eq | BinaryOp::Neq | BinaryOp::CaseEq | BinaryOp::CaseNeq
                ) {
                    for side in [left, right] {
                        if let Some(n) = whole(strip(side)) {
                            out.insert(format!("{}{}", prefix, n));
                        }
                    }
                }
                stack.push(left);
                stack.push(right);
            }
            ExprKind::Inside { expr, ranges } => {
                if let Some(n) = whole(strip(expr)) {
                    out.insert(format!("{}{}", prefix, n));
                }
                stack.push(expr);
                stack.extend(ranges.iter());
            }
            ExprKind::Unary { operand, .. } => stack.push(operand),
            ExprKind::Paren(i) => stack.push(i),
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                stack.push(condition);
                stack.push(then_expr);
                stack.push(else_expr);
            }
            ExprKind::Concatenation(v) => stack.extend(v.iter()),
            ExprKind::SystemCall { args, .. } | ExprKind::Call { args, .. } => {
                stack.extend(args.iter())
            }
            _ => {}
        }
    }
}

fn strip(e: &Expression) -> &Expression {
    match &e.kind {
        ExprKind::Paren(i) => strip(i),
        _ => e,
    }
}

/// `tests` of a statement, and the variables it selects a `case` on.
fn stmt_tests(s: &Statement, out: &mut HashSet<String>) {
    let mut exprs = Vec::new();
    stmt_exprs(s, &mut exprs);
    for e in exprs {
        tests(e, "", out);
    }
    let mut stack = vec![s];
    while let Some(x) = stack.pop() {
        match &x.kind {
            StatementKind::Case { expr, items, .. } => {
                if let Some(n) = whole(strip(expr)) {
                    out.insert(n.to_string());
                }
                stack.extend(items.iter().map(|it| &it.stmt));
            }
            StatementKind::SeqBlock { stmts, .. } | StatementKind::ParBlock { stmts, .. } => {
                stack.extend(stmts.iter())
            }
            StatementKind::If {
                then_stmt,
                else_stmt,
                ..
            } => {
                stack.push(then_stmt);
                if let Some(e) = else_stmt {
                    stack.push(e);
                }
            }
            StatementKind::For { body, .. }
            | StatementKind::Foreach { body, .. }
            | StatementKind::While { body, .. }
            | StatementKind::DoWhile { body, .. }
            | StatementKind::Repeat { body, .. }
            | StatementKind::Forever { body }
            | StatementKind::TimingControl { stmt: body, .. }
            | StatementKind::Wait { stmt: body, .. } => stack.push(body),
            _ => {}
        }
    }
}

/// One walk of a block for the assignments of `target`, a state variable
/// `var` or its next-state variable.
struct Walk<'a> {
    sim: &'a mut Simulator,
    var: &'a str,
    width: u32,
    target: &'a str,
    /// `target <= other` is a load of the next-state variable `other` (the
    /// state process of a two-process FSM).
    loads: bool,
    recs: Vec<Rec>,
    /// Loads of another variable: (branch, variable).
    via: Vec<(Ctx, String)>,
    /// Case items and `if` branches that test the variable, in order.
    handlers: Vec<(Ctx, Span)>,
    /// Constants the variable is compared with.
    tested: BTreeSet<u64>,
    /// Names of constants, from the identifiers that gave them.
    names: HashMap<u64, String>,
    /// Some branch tests the variable.
    refined: bool,
    /// An assignment of the target yields something other than constants.
    bad: bool,
}

impl Walk<'_> {
    fn is_var(&self, e: &Expression) -> bool {
        match &e.kind {
            ExprKind::Paren(i) => self.is_var(i),
            _ => whole(e) == Some(self.var),
        }
    }

    fn mask(&self) -> u64 {
        if self.width >= 64 {
            u64::MAX
        } else {
            (1u64 << self.width) - 1
        }
    }

    fn constant(&self, e: &Expression) -> bool {
        constant(self.sim, e)
    }

    /// The value of a constant expression (no variable but the state
    /// variable, which `subst` has replaced), when fully known. A constant
    /// named by an identifier gives the state its name.
    fn value(&mut self, e: &Expression) -> Option<u64> {
        if !self.constant(e) {
            return None;
        }
        let v = self.sim.eval_expr(e);
        if v.has_xz() || v.is_real {
            return None;
        }
        let v = v.to_u64()?;
        let mut inner = e;
        loop {
            match &inner.kind {
                ExprKind::Paren(i) => inner = i,
                ExprKind::SystemCall { name, args }
                    if name == "$__xz_named_cast" && args.len() == 2 =>
                {
                    inner = &args[1]
                }
                _ => break,
            }
        }
        if let ExprKind::Ident(h) = &inner.kind {
            if let Some(leaf) = h.path.last() {
                let leaf = leaf.name.name.as_str();
                let leaf = leaf.rsplit(['.', ':']).next().unwrap_or(leaf).to_string();
                self.names.entry(v & self.mask()).or_insert(leaf);
            }
        }
        Some(v)
    }

    /// `e` with the state variable replaced by the constant `c`.
    fn subst(&self, e: &Expression, c: u64) -> Expression {
        if self.is_var(e) {
            return Expression::new(
                ExprKind::Number(NumberLiteral::Integer {
                    size: Some(self.width),
                    signed: false,
                    base: NumberBase::Decimal,
                    value: c.to_string(),
                    cached_val: Cell::new(None),
                }),
                e.span,
            );
        }
        let sub = |x: &Expression| Box::new(self.subst(x, c));
        let kind = match &e.kind {
            ExprKind::Unary { op, operand } => ExprKind::Unary {
                op: *op,
                operand: sub(operand),
            },
            ExprKind::Binary { op, left, right } => ExprKind::Binary {
                op: *op,
                left: sub(left),
                right: sub(right),
            },
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => ExprKind::Conditional {
                condition: sub(condition),
                then_expr: sub(then_expr),
                else_expr: sub(else_expr),
            },
            ExprKind::Paren(i) => ExprKind::Paren(sub(i)),
            ExprKind::Concatenation(v) => {
                ExprKind::Concatenation(v.iter().map(|x| self.subst(x, c)).collect())
            }
            ExprKind::SystemCall { name, args } => ExprKind::SystemCall {
                name: name.clone(),
                args: args.iter().map(|x| self.subst(x, c)).collect(),
            },
            k => k.clone(),
        };
        Expression::new(kind, e.span)
    }

    fn mentions(&self, e: &Expression) -> bool {
        let mut v = Vec::new();
        idents(e, &mut v);
        v.contains(&self.var)
    }

    /// The branch in which `cond` holds, when it is a test of the variable
    /// alone (`state == K`, `state != K`, and `&&`, `||`, `!` of those).
    fn pure(&mut self, cond: &Expression) -> Option<Ctx> {
        match &cond.kind {
            ExprKind::Paren(i) => self.pure(i),
            ExprKind::Unary {
                op: UnaryOp::LogNot,
                operand,
            } => self.pure(operand).map(|c| c.not()),
            ExprKind::Binary { op, left, right } => match op {
                BinaryOp::Eq | BinaryOp::CaseEq | BinaryOp::Neq | BinaryOp::CaseNeq => {
                    let k = if self.is_var(left) && !self.mentions(right) {
                        self.value(right)?
                    } else if self.is_var(right) && !self.mentions(left) {
                        self.value(left)?
                    } else {
                        return None;
                    };
                    self.tested.insert(k);
                    Some(if matches!(op, BinaryOp::Eq | BinaryOp::CaseEq) {
                        Ctx::one(k)
                    } else {
                        Ctx::Any(BTreeSet::from([k]))
                    })
                }
                BinaryOp::LogAnd => Some(self.pure(left)?.and(&self.pure(right)?)),
                BinaryOp::LogOr => Some(self.pure(left)?.or(&self.pure(right)?)),
                _ => None,
            },
            ExprKind::Inside { expr, ranges } if self.is_var(expr) => {
                let mut set = BTreeSet::new();
                for r in ranges {
                    if matches!(r.kind, ExprKind::Range(..)) {
                        return None;
                    }
                    set.insert(self.value(r)?);
                }
                self.tested.extend(set.iter().copied());
                Some(Ctx::Set(set))
            }
            _ => None,
        }
    }

    /// The branch in which `cond` holds as far as the variable goes: its
    /// `pure` test, or the tests among the terms of a `&&`.
    fn then(&mut self, cond: &Expression) -> Option<Ctx> {
        if let Some(c) = self.pure(cond) {
            return Some(c);
        }
        match &cond.kind {
            ExprKind::Paren(i) => self.then(i),
            ExprKind::Binary {
                op: BinaryOp::LogAnd,
                left,
                right,
            } => match (self.then(left), self.then(right)) {
                (Some(a), Some(b)) => Some(a.and(&b)),
                (a, b) => a.or(b),
            },
            _ => None,
        }
    }

    /// The values an assignment's right-hand side yields, each with the
    /// branch it yields it in; `None` stands for the variable itself (a
    /// hold). `Err` when it yields anything but constants.
    fn eval(&mut self, e: &Expression, ctx: &Ctx) -> Result<Vec<(Ctx, Option<u64>)>, ()> {
        if self.is_var(e) {
            return Ok(vec![(ctx.clone(), None)]);
        }
        match &e.kind {
            ExprKind::Paren(i) => self.eval(i, ctx),
            ExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let (t, f) = match (self.pure(condition), self.then(condition)) {
                    (Some(p), _) => {
                        self.refined = true;
                        (ctx.and(&p), ctx.and(&p.not()))
                    }
                    (None, Some(t)) => {
                        self.refined = true;
                        (ctx.and(&t), ctx.clone())
                    }
                    (None, None) => (ctx.clone(), ctx.clone()),
                };
                let mut v = self.eval(then_expr, &t)?;
                v.extend(self.eval(else_expr, &f)?);
                Ok(v)
            }
            _ if self.mentions(e) => {
                let Ctx::Set(set) = ctx else {
                    return Err(());
                };
                let mut v = Vec::new();
                for &c in set {
                    let x = self.subst(e, c);
                    let r = self.value(&x).ok_or(())?;
                    v.push((Ctx::one(c), Some(r & self.mask())));
                }
                Ok(v)
            }
            _ => {
                let r = self.value(e).ok_or(())?;
                Ok(vec![(ctx.clone(), Some(r & self.mask()))])
            }
        }
    }

    /// The label sets of a `case` on the variable, one per item (empty for
    /// `default`); `None` when a label is not a fully known constant.
    fn labels(&mut self, items: &[CaseItem]) -> Option<Vec<BTreeSet<u64>>> {
        let mut out = Vec::new();
        for it in items {
            if it.pattern.is_some() || it.guard.is_some() {
                return None;
            }
            let mut set = BTreeSet::new();
            for p in &it.patterns {
                if matches!(p.kind, ExprKind::Range(..)) || self.mentions(p) {
                    return None;
                }
                // A casez/casex wildcard label (x or z bits) is not one
                // value: `value` refuses it.
                set.insert(self.value(p)?);
            }
            out.push(set);
        }
        Some(out)
    }

    fn stmt(&mut self, s: &Statement, ctx: &Ctx) {
        match &s.kind {
            StatementKind::SeqBlock { stmts, .. } => {
                for x in stmts {
                    self.stmt(x, ctx);
                }
            }
            StatementKind::If {
                condition,
                then_stmt,
                else_stmt,
                ..
            } => {
                let pure = self.pure(condition);
                let then = pure.clone().or_else(|| self.then(condition));
                let t = match &then {
                    Some(c) => {
                        self.refined = true;
                        let t = ctx.and(c);
                        self.handlers.push((t.clone(), s.span));
                        t
                    }
                    None => ctx.clone(),
                };
                self.stmt(then_stmt, &t);
                if let Some(e) = else_stmt {
                    let f = match &pure {
                        Some(p) => ctx.and(&p.not()),
                        None => ctx.clone(),
                    };
                    self.stmt(e, &f);
                }
            }
            StatementKind::Case { expr, items, .. } => {
                let labels = if self.is_var(expr) {
                    self.labels(items)
                } else {
                    None
                };
                let Some(labels) = labels else {
                    for it in items {
                        self.stmt(&it.stmt, ctx);
                    }
                    return;
                };
                self.refined = true;
                let every: BTreeSet<u64> = labels.iter().flatten().copied().collect();
                self.tested.extend(every.iter().copied());
                let mut seen = BTreeSet::new();
                for (it, set) in items.iter().zip(labels) {
                    let c = if it.is_default {
                        ctx.and(&Ctx::Any(every.clone()))
                    } else {
                        ctx.and(&Ctx::Set(set.clone())).and(&Ctx::Any(seen.clone()))
                    };
                    seen.extend(set);
                    self.handlers.push((c.clone(), it.span));
                    self.stmt(&it.stmt, &c);
                }
            }
            StatementKind::For { body, .. }
            | StatementKind::Foreach { body, .. }
            | StatementKind::While { body, .. }
            | StatementKind::DoWhile { body, .. }
            | StatementKind::Repeat { body, .. }
            | StatementKind::Forever { body } => self.stmt(body, ctx),
            // A wait or a fork inside the block: not a synthesizable state
            // machine.
            StatementKind::TimingControl { .. }
            | StatementKind::Wait { .. }
            | StatementKind::ParBlock { .. } => self.bad = true,
            StatementKind::BlockingAssign { lvalue, rvalue }
            | StatementKind::NonblockingAssign { lvalue, rvalue, .. } => {
                if whole(lvalue) == Some(self.target) {
                    self.assign(rvalue, ctx, s.span);
                }
            }
            _ => {}
        }
    }

    /// An assignment of the target (a statement's, or a continuous one's).
    fn assign(&mut self, rvalue: &Expression, ctx: &Ctx, span: Span) {
        if self.loads {
            if let Some(n) = whole(rvalue)
                .filter(|n| *n != self.var && !self.sim.module.parameters.contains_key(*n))
            {
                self.via.push((ctx.clone(), n.to_string()));
                return;
            }
        }
        match self.eval(rvalue, ctx) {
            Ok(v) => {
                for (c, x) in v {
                    if let Some(value) = x {
                        self.recs.push(Rec {
                            ctx: c,
                            value,
                            span,
                            direct: self.loads,
                        });
                    }
                }
            }
            Err(()) => self.bad = true,
        }
    }
}

/// A recognised FSM: what recognition found and what the run counts.
pub(crate) struct Fsm {
    pub(super) scope: u32,
    /// The state variable's name within the scope.
    name: String,
    /// The next-state variable's, for a two-process FSM.
    next: Option<String>,
    sig: usize,
    width: u32,
    /// The state process (always block).
    span: Span,
    /// By value: value, name, the case item or branch that handles it.
    states: Vec<(u64, String, Span)>,
    /// By (from, to): state indices and the assignment.
    trans: Vec<(u32, u32, Span)>,
    /// `from * states + to` -> transition index, `u32::MAX` for none.
    tmap: Vec<u32>,
    reset: Vec<u32>,
    unreachable: Vec<u32>,
    proc: u32,
    hits: Vec<u64>,
    thits: Vec<u64>,
    /// Writes of a known value outside the state set.
    illegal: u64,
    /// The state the last sample saw.
    shadow: Option<u32>,
    pending: bool,
    counted: bool,
}

/// A state process: the variables (or constant selects of them, such as a
/// port connected to one bit of a vector) its body reads or writes, and
/// their values when it last ran.
struct Proc {
    inputs: Vec<Expression>,
    /// The variables `inputs` read.
    ids: Vec<usize>,
    /// An input changed after the last run (seen at the end of a time slot).
    stale: bool,
    /// The clock: the edge in the event control on a signal the body does not
    /// read (`posedge clk` of `@(posedge clk or negedge rst_n)`). A run is a
    /// clock run when the clock had that edge in the time slot and no other
    /// run of the slot was; only clock runs move the snapshot `words` (the
    /// commercial simulator compares a clock run's inputs with those of the
    /// previous clock run, not of a run an asynchronous reset started).
    clock: Option<(Expression, Edge)>,
    /// The clock's level at the end of the last time slot (`None`: x or z).
    level: Option<bool>,
    /// A clock run happened in this time slot.
    clocked: bool,
    words: Vec<u64>,
    seen: bool,
    ran: bool,
    dirty: bool,
}

pub(crate) struct FsmCov {
    pub(super) fsms: Vec<Fsm>,
    procs: Vec<Proc>,
    /// Always block index -> its process, and the FSMs it assigns.
    blocks: Vec<(usize, u32, Vec<(String, u32)>)>,
    pending: Vec<u32>,
    ran: Vec<u32>,
    scratch: Vec<u64>,
}

/// One FSM of a scope in the results.
#[derive(Clone)]
pub(super) struct FsmRow {
    name: String,
    next: Option<String>,
    file: String,
    line: u32,
    start: usize,
    /// name, value, line, count
    states: Vec<(String, u64, u32, u64)>,
    /// from, to, line, count
    trans: Vec<(String, String, u32, u64)>,
    reset: Vec<String>,
    unreachable: Vec<String>,
    illegal: u64,
}

impl FsmRow {
    /// (states covered, states, transitions covered, transitions)
    pub(super) fn tally(&self) -> [u64; 4] {
        [
            self.states.iter().filter(|s| s.3 > 0).count() as u64,
            self.states.len() as u64,
            self.trans.iter().filter(|t| t.3 > 0).count() as u64,
            self.trans.len() as u64,
        ]
    }

    pub(super) fn key(&self) -> (&str, usize, &str) {
        (&self.file, self.start, &self.name)
    }

    /// Add another instance's counts of the same FSM; false when the two
    /// differ in shape (another parameterization).
    pub(super) fn add(&mut self, o: &FsmRow) -> bool {
        let same = self.states.len() == o.states.len()
            && self.trans.len() == o.trans.len()
            && self.states.iter().zip(&o.states).all(|(a, b)| a.1 == b.1)
            && self
                .trans
                .iter()
                .zip(&o.trans)
                .all(|(a, b)| a.0 == b.0 && a.1 == b.1);
        if same {
            for (a, b) in self.states.iter_mut().zip(&o.states) {
                a.3 += b.3;
            }
            for (a, b) in self.trans.iter_mut().zip(&o.trans) {
                a.3 += b.3;
            }
            self.illegal += o.illegal;
        }
        same
    }

    pub(super) fn json(&self, out: &mut String) {
        let names = |v: &[String]| {
            v.iter()
                .map(|n| format!("\"{}\"", json_escape(n)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "{{\"variable\": \"{}\", \"next\": {}, \"file\": \"{}\", \"line\": {}, \"reset\": [{}], \"states\": [",
            json_escape(&self.name),
            match &self.next {
                Some(n) => format!("\"{}\"", json_escape(n)),
                None => "null".to_string(),
            },
            json_escape(&self.file),
            self.line,
            names(&self.reset)
        ));
        for (i, s) in self.states.iter().enumerate() {
            out.push_str(&format!(
                "{}{{\"name\": \"{}\", \"value\": {}, \"line\": {}, \"count\": {}}}",
                if i == 0 { "" } else { ", " },
                json_escape(&s.0),
                s.1,
                s.2,
                s.3
            ));
        }
        out.push_str("], \"transitions\": [");
        for (i, t) in self.trans.iter().enumerate() {
            out.push_str(&format!(
                "{}{{\"from\": \"{}\", \"to\": \"{}\", \"line\": {}, \"count\": {}}}",
                if i == 0 { "" } else { ", " },
                json_escape(&t.0),
                json_escape(&t.1),
                t.2,
                t.3
            ));
        }
        out.push_str(&format!(
            "], \"unreachable\": [{}], \"illegal\": {}}}",
            names(&self.unreachable),
            self.illegal
        ));
    }
}

/// Bit 0 of a value: `None` when it is x or z.
fn bit(v: &Value) -> Option<bool> {
    let (a, b) = v.raw_bits();
    (b & 1 == 0).then_some(a & 1 == 1)
}

/// The value bits of a signal, two words (value, x/z) per 64 bits.
fn words(v: &Value, width: u32, out: &mut Vec<u64>) {
    if width <= 64 {
        let m = if width >= 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        let (a, b) = v.raw_bits();
        out.push(a & m);
        out.push(b & m);
        return;
    }
    for w in 0..width.div_ceil(64) as usize {
        let n = (width as usize - w * 64).min(64);
        let (a, b) = v.slice_bits_swar(w * 64, n);
        out.push(a);
        out.push(b);
    }
}

/// A statement list as one statement, with `marks` in front.
fn marked(marks: Vec<Statement>, s: Statement) -> Statement {
    let span = s.span;
    let mut v = marks;
    v.push(s);
    Statement::new(
        StatementKind::SeqBlock {
            name: None,
            stmts: v,
        },
        span,
    )
}

fn mark(id: u32, span: Span) -> Statement {
    Statement::new(
        StatementKind::Expr(Expression::new(
            ExprKind::SystemCall {
                name: COV_HIT_TASK.to_string(),
                args: vec![Expression::new(
                    ExprKind::Number(NumberLiteral::Integer {
                        size: None,
                        signed: false,
                        base: NumberBase::Decimal,
                        value: (FSM_MARK + id).to_string(),
                        cached_val: Cell::new(None),
                    }),
                    span,
                )],
            },
            span,
        )),
        span,
    )
}

/// Put a write marker in front of every whole assignment of an FSM's
/// variable in `s`.
fn mark_writes(s: Statement, vars: &[(String, u32)]) -> Statement {
    let span = s.span;
    let re = |kind| Statement::new(kind, span);
    let sub = |b: Box<Statement>| Box::new(mark_writes(*b, vars));
    match s.kind {
        StatementKind::SeqBlock { name, stmts } => re(StatementKind::SeqBlock {
            name,
            stmts: stmts.into_iter().map(|x| mark_writes(x, vars)).collect(),
        }),
        StatementKind::If {
            unique_priority,
            condition,
            then_stmt,
            else_stmt,
        } => re(StatementKind::If {
            unique_priority,
            condition,
            then_stmt: sub(then_stmt),
            else_stmt: else_stmt.map(sub),
        }),
        StatementKind::Case {
            unique_priority,
            kind,
            expr,
            mut items,
        } => {
            for it in items.iter_mut() {
                let st = std::mem::replace(&mut it.stmt, Statement::new(StatementKind::Null, span));
                it.stmt = mark_writes(st, vars);
            }
            re(StatementKind::Case {
                unique_priority,
                kind,
                expr,
                items,
            })
        }
        StatementKind::For {
            init,
            condition,
            step,
            body,
        } => re(StatementKind::For {
            init,
            condition,
            step,
            body: sub(body),
        }),
        StatementKind::Foreach {
            array,
            vars: v,
            body,
        } => re(StatementKind::Foreach {
            array,
            vars: v,
            body: sub(body),
        }),
        StatementKind::While { condition, body } => re(StatementKind::While {
            condition,
            body: sub(body),
        }),
        StatementKind::DoWhile { body, condition } => re(StatementKind::DoWhile {
            body: sub(body),
            condition,
        }),
        StatementKind::Repeat { count, body } => re(StatementKind::Repeat {
            count,
            body: sub(body),
        }),
        StatementKind::Forever { body } => re(StatementKind::Forever { body: sub(body) }),
        kind @ (StatementKind::BlockingAssign { .. } | StatementKind::NonblockingAssign { .. }) => {
            let lv = match &kind {
                StatementKind::BlockingAssign { lvalue, .. }
                | StatementKind::NonblockingAssign { lvalue, .. } => whole(lvalue),
                _ => None,
            };
            let marks: Vec<Statement> = vars
                .iter()
                .filter(|(n, _)| Some(n.as_str()) == lv)
                .map(|(_, f)| mark(2 * f + 1, span))
                .collect();
            if marks.is_empty() {
                re(kind)
            } else {
                marked(marks, re(kind))
            }
        }
        kind => re(kind),
    }
}

impl Simulator {
    /// Find the FSMs of `always` (see the module docs). Runs on the blocks
    /// as elaborated, before the other kinds instrument them.
    pub(super) fn fsm_extract(
        &mut self,
        cov: &mut CodeCov,
        paths: &HashSet<String>,
        always: &[AlwaysBlock],
    ) -> FsmCov {
        let mut out = FsmCov {
            fsms: Vec::new(),
            procs: Vec::new(),
            blocks: Vec::new(),
            pending: Vec::new(),
            ran: Vec::new(),
            scratch: Vec::new(),
        };
        // Who assigns what: variable -> blocks assigning it whole, and the
        // variables some block assigns in part.
        let mut writers: HashMap<&str, Vec<usize>> = HashMap::default();
        let mut parts: HashMap<&str, Vec<usize>> = HashMap::default();
        let mut kinds: Vec<Option<bool>> = Vec::with_capacity(always.len());
        for (i, ab) in always.iter().enumerate() {
            let b = body(ab);
            kinds.push(b.map(|(_, edge)| edge));
            let mut full = BTreeSet::new();
            let mut part = BTreeSet::new();
            targets(&ab.stmt, &mut full, &mut part);
            for n in full {
                writers.entry(n).or_default().push(i);
            }
            for n in part {
                parts.entry(n).or_default().push(i);
            }
        }
        let top = format!("{}.", self.module.name);
        // Continuous assignments by target, and every variable a test
        // compares with something: one no test names is no state.
        let (assigns, mut tested) = self.fsm_assigns();
        for ab in always {
            stmt_tests(&ab.stmt, &mut tested);
        }
        for (i, ab) in always.iter().enumerate() {
            let Some((proc_body, true)) = body(ab) else {
                continue;
            };
            let mut full = BTreeSet::new();
            let mut part = BTreeSet::new();
            targets(proc_body, &mut full, &mut part);
            let mut found: Vec<(String, u32)> = Vec::new();
            for var in full {
                if writers.get(var).map(|w| w.as_slice()) != Some(&[i][..])
                    || parts.get(var).is_some_and(|p| p.iter().any(|&b| b != i))
                    || !tested.contains(var)
                    || var.contains("__xz")
                    || self.module.parameters.contains_key(var)
                {
                    continue;
                }
                let Some(&sig) = self.signal_name_to_id.get(var).or_else(|| {
                    self.signal_name_to_id
                        .get(format!("{}{}", top, var).as_str())
                }) else {
                    continue;
                };
                let width = self.signal_widths.get(sig).copied().unwrap_or(0);
                if !(2..=64).contains(&width) || self.signal_real.get(sig).copied().unwrap_or(true)
                {
                    continue;
                }
                let mut w = Walk {
                    sim: self,
                    var,
                    width,
                    target: var,
                    loads: true,
                    recs: Vec::new(),
                    via: Vec::new(),
                    handlers: Vec::new(),
                    tested: BTreeSet::new(),
                    names: HashMap::default(),
                    refined: false,
                    bad: false,
                };
                w.stmt(proc_body, &Ctx::all());
                if w.bad {
                    continue;
                }
                let reset: BTreeSet<u64> = w
                    .recs
                    .iter()
                    .filter(|r| !r.ctx.specific())
                    .map(|r| r.value)
                    .collect();
                let mut recs = std::mem::take(&mut w.recs);
                let mut handlers = std::mem::take(&mut w.handlers);
                let mut tested = std::mem::take(&mut w.tested);
                let mut names = std::mem::take(&mut w.names);
                let mut refined = w.refined;
                let via = std::mem::take(&mut w.via);
                // A two-process FSM: the block loads one next-state variable
                // that a combinational block (or this one) computes.
                let mut next = None;
                if let Some((_, n)) = via.first() {
                    if via.iter().any(|(_, m)| m != n) {
                        continue;
                    }
                    // Computed by one block, or else by one continuous
                    // assignment.
                    let block = match writers.get(n.as_str()).map(|w| w.as_slice()) {
                        Some(&[c]) => Some(c),
                        None => None,
                        _ => continue,
                    };
                    let comb = match block {
                        Some(c) => {
                            if parts
                                .get(n.as_str())
                                .is_some_and(|p| p.iter().any(|&b| b != c))
                                || !(c == i || kinds[c] == Some(false))
                            {
                                continue;
                            }
                            let Some((comb, _)) = body(&always[c]) else {
                                continue;
                            };
                            Some(comb)
                        }
                        None => None,
                    };
                    let assign = match comb {
                        Some(_) => None,
                        None => {
                            if parts.contains_key(n.as_str()) {
                                continue;
                            }
                            match self.fsm_assign(&assigns, n) {
                                Some(one) => Some(one),
                                None => continue,
                            }
                        }
                    };
                    let mut w = Walk {
                        sim: self,
                        var,
                        width,
                        target: n,
                        loads: false,
                        recs: Vec::new(),
                        via: Vec::new(),
                        handlers: Vec::new(),
                        tested: BTreeSet::new(),
                        names: HashMap::default(),
                        refined: false,
                        bad: false,
                    };
                    match (comb, &assign) {
                        (Some(comb), _) => w.stmt(comb, &Ctx::all()),
                        (None, Some((rhs, span))) => w.assign(rhs, &Ctx::all(), *span),
                        (None, None) => continue,
                    }
                    if w.bad {
                        continue;
                    }
                    for (vctx, _) in &via {
                        recs.extend(w.recs.iter().map(|r| Rec {
                            ctx: vctx.and(&r.ctx),
                            value: r.value,
                            span: r.span,
                            direct: false,
                        }));
                    }
                    // The next-state block's tests name the states.
                    let mut h = std::mem::take(&mut w.handlers);
                    h.append(&mut handlers);
                    handlers = h;
                    tested.append(&mut w.tested);
                    for (k, v) in w.names.drain() {
                        names.entry(k).or_insert(v);
                    }
                    refined |= w.refined;
                    next = Some(n.clone());
                }
                if !refined {
                    continue;
                }
                let limit = if width >= 64 { u64::MAX } else { 1u64 << width };
                let values: BTreeSet<u64> = recs
                    .iter()
                    .map(|r| r.value)
                    .chain(tested.iter().copied())
                    .filter(|&v| width >= 64 || v < limit)
                    .collect();
                if values.len() < 2 || values.len() > MAX_STATES {
                    continue;
                }
                let index: HashMap<u64, u32> = values
                    .iter()
                    .enumerate()
                    .map(|(k, &v)| (v, k as u32))
                    .collect();
                let n = values.len();
                // Transitions, each with the assignment that makes it, as
                // the commercial simulator picks it: one inside a test of the
                // variable over one outside (a reset, a default), then one in
                // the state process over one in the next-state logic, then
                // the last in the source.
                let mut best: BTreeMap<(u32, u32), ((bool, bool, usize), Span)> = BTreeMap::new();
                for r in &recs {
                    let Some(&to) = index.get(&r.value) else {
                        continue;
                    };
                    let rank = (r.ctx.specific(), r.direct, r.span.start);
                    for (&v, &from) in &index {
                        if v == r.value || !r.ctx.holds(v) {
                            continue;
                        }
                        let e = best.entry((from, to)).or_insert((rank, r.span));
                        if rank >= e.0 {
                            *e = (rank, r.span);
                        }
                    }
                }
                if best.is_empty() {
                    continue;
                }
                let trans: Vec<(u32, u32, Span)> =
                    best.iter().map(|(&(f, t), &(_, sp))| (f, t, sp)).collect();
                // A state's line is that of the first transition out of it
                // made inside a test of the variable, else of the first out
                // of it at all, else of the case item or branch handling it.
                let out_of = |k: u32, tested: bool| {
                    best.iter()
                        .filter(|((f, _), (rank, _))| *f == k && (rank.0 || !tested))
                        .map(|(_, (_, sp))| *sp)
                        .min_by_key(|sp| sp.start)
                };
                let mut tmap = vec![u32::MAX; n * n];
                for (k, (f, t, _)) in trans.iter().enumerate() {
                    tmap[*f as usize * n + *t as usize] = k as u32;
                }
                let anon = values.iter().all(|v| !names.contains_key(v));
                let states: Vec<(u64, String, Span)> = values
                    .iter()
                    .enumerate()
                    .map(|(k, &v)| {
                        let name = match names.get(&v) {
                            Some(s) if !anon => s.clone(),
                            _ => format!("st{}", k),
                        };
                        let span = out_of(k as u32, true)
                            .or_else(|| out_of(k as u32, false))
                            .or_else(|| handlers.iter().find(|(c, _)| c.holds(v)).map(|(_, s)| *s))
                            .or_else(|| recs.iter().find(|r| r.value == v).map(|r| r.span))
                            .unwrap_or(ab.stmt.span);
                        (v, name, span)
                    })
                    .collect();
                let reset: Vec<u32> = reset.iter().filter_map(|v| index.get(v).copied()).collect();
                // States no transition leads to from a reset state.
                let mut unreachable = Vec::new();
                if !reset.is_empty() {
                    let mut seen = vec![false; n];
                    let mut todo = reset.clone();
                    for &r in &reset {
                        seen[r as usize] = true;
                    }
                    while let Some(s) = todo.pop() {
                        for (f, t, _) in &trans {
                            if *f == s && !seen[*t as usize] {
                                seen[*t as usize] = true;
                                todo.push(*t);
                            }
                        }
                    }
                    unreachable = (0..n as u32).filter(|&k| !seen[k as usize]).collect();
                }
                let Some(scope) = Self::cov_instance_scope(&self.module, cov, paths, &ab.scope)
                else {
                    continue;
                };
                // The variable's name within its instance: below the
                // longest prefix of the block's scope that is an instance.
                let mut inst = ab.scope.as_str();
                while !inst.is_empty() && !paths.contains(inst) {
                    inst = inst.rsplit_once('.').map(|(p, _)| p).unwrap_or("");
                }
                let rel = format!("{}.", inst);
                let local = |v: &str| -> String {
                    match v.strip_prefix(rel.as_str()) {
                        Some(l) if !inst.is_empty() => l.to_string(),
                        _ => v.to_string(),
                    }
                };
                let proc = out.procs.len() as u32;
                let f = out.fsms.len() as u32;
                let shadow = {
                    let (v, x) = self.signal_table[sig].raw_bits();
                    let m = if width >= 64 { u64::MAX } else { limit - 1 };
                    (x & m == 0).then(|| index.get(&(v & m)).copied()).flatten()
                };
                out.fsms.push(Fsm {
                    scope,
                    name: local(var),
                    next: next.as_deref().map(local),
                    sig,
                    width,
                    span: ab.stmt.span,
                    hits: vec![0; n],
                    thits: vec![0; trans.len()],
                    states,
                    trans,
                    tmap,
                    reset,
                    unreachable,
                    proc,
                    illegal: 0,
                    shadow,
                    pending: false,
                    counted: false,
                });
                found.push((var.to_string(), f));
            }
            if found.is_empty() {
                continue;
            }
            let vars: Vec<&str> = found.iter().map(|(v, _)| v.as_str()).collect();
            let (inputs, ids) = self.fsm_inputs(proc_body, &vars, &top);
            // The clock: the one edge item whose signal the body never reads.
            let mut read = Vec::new();
            for e in &inputs {
                idents(e, &mut read);
            }
            let clocks: Vec<(Expression, Edge)> = match &ab.stmt.kind {
                StatementKind::TimingControl {
                    control: TimingControl::Event(EventControl::EventExpr(items)),
                    ..
                } => items
                    .iter()
                    .filter_map(|it| {
                        let edge = it.edge?;
                        let mut names = Vec::new();
                        idents(&it.expr, &mut names);
                        (!names.iter().any(|n| read.contains(n))).then(|| (it.expr.clone(), edge))
                    })
                    .collect(),
                _ => Vec::new(),
            };
            // An edge on a select (`posedge rst[0]`) is an internal net here;
            // with another candidate, that one is the clock.
            let named: Vec<&(Expression, Edge)> = clocks
                .iter()
                .filter(|(e, _)| whole(e).is_none_or(|n| !n.contains("__xz")))
                .collect();
            let clock = match (clocks.as_slice(), named.as_slice()) {
                ([one], _) => Some(one.clone()),
                (_, [one]) => Some((*one).clone()),
                _ => None,
            };
            let level = clock.as_ref().and_then(|(e, _)| bit(&self.eval_expr(e)));
            out.procs.push(Proc {
                inputs,
                ids,
                words: Vec::new(),
                seen: false,
                ran: false,
                dirty: false,
                stale: false,
                clock,
                level,
                clocked: false,
            });
            out.blocks.push((i, out.procs.len() as u32 - 1, found));
        }
        out
    }

    /// The design's continuous assignments of whole variables by target
    /// (`None`: an elaborated one, else the index of a lazy one), and the
    /// variables their right-hand sides compare with something. Lazy ones
    /// are only named here, not elaborated: `build_comb_entries` takes them
    /// later, one at a time.
    fn fsm_assigns(&self) -> (HashMap<String, Vec<Option<usize>>>, HashSet<String>) {
        let mut by: HashMap<String, Vec<Option<usize>>> = HashMap::default();
        let mut tested = HashSet::default();
        for ca in &self.module.continuous_assigns {
            if let (Some(n), Some(_)) = (whole(&ca.lhs), &ca.origin) {
                by.entry(n.to_string()).or_default().push(None);
            }
            tests(&ca.rhs, "", &mut tested);
        }
        for (k, p) in self.module.pending_cont_assign.iter().enumerate() {
            if let (Some(n), Some(_)) = (whole(&p.lhs_source), p.origin_span) {
                by.entry(format!("{}{}", p.ctx.prefix, n))
                    .or_default()
                    .push(Some(k));
            }
            tests(&p.rhs_source, &p.ctx.prefix, &mut tested);
        }
        (by, tested)
    }

    /// The right-hand side and span of the one continuous assignment of `n`,
    /// elaborating a lazy one on a copy.
    fn fsm_assign(
        &self,
        index: &HashMap<String, Vec<Option<usize>>>,
        n: &str,
    ) -> Option<(Expression, Span)> {
        match index.get(n)?.as_slice() {
            [None] => self
                .module
                .continuous_assigns
                .iter()
                .find(|ca| whole(&ca.lhs) == Some(n) && ca.origin.is_some())
                .map(|ca| (ca.rhs.clone(), ca.origin.as_ref().unwrap().0)),
            [Some(k)] => {
                let ca = self.module.pending_cont_assign[*k]
                    .clone()
                    .materialize(&self.module.parameters);
                (whole(&ca.lhs) == Some(n)).then_some(())?;
                let span = ca.origin.as_ref()?.0;
                Some((ca.rhs, span))
            }
            _ => None,
        }
    }

    /// What a state process's input check reads: each variable its body
    /// names, or the constant select of it the body names, once.
    fn fsm_inputs(
        &mut self,
        body: &Statement,
        vars: &[&str],
        top: &str,
    ) -> (Vec<Expression>, Vec<usize>) {
        let mut exprs = Vec::new();
        stmt_exprs(body, &mut exprs);
        let mut all = Vec::new();
        for e in exprs {
            refs(e, &mut all);
        }
        let own: Vec<Expression> = vars
            .iter()
            .map(|v| {
                Expression::new(
                    ExprKind::Ident(HierarchicalIdentifier {
                        root: None,
                        path: vec![HierPathSegment {
                            name: crate::ast::Identifier {
                                name: v.to_string(),
                                span: body.span,
                            },
                            selects: Vec::new(),
                        }],
                        span: body.span,
                        cached_signal_id: Cell::new(None),
                        cached_resolved_name: std::cell::OnceCell::new(),
                    }),
                    body.span,
                )
            })
            .collect();
        all.extend(own.iter());
        let mut keys = HashSet::default();
        let mut out = Vec::new();
        let mut ids = Vec::new();
        for r in all {
            let (h, sels): (&HierarchicalIdentifier, Vec<&Expression>) = match &r.kind {
                ExprKind::Ident(h) => (h, h.path.iter().flat_map(|s| s.selects.iter()).collect()),
                ExprKind::Index { expr, index } => match &expr.kind {
                    ExprKind::Ident(h) => (h, vec![&**index]),
                    _ => continue,
                },
                ExprKind::RangeSelect {
                    expr, left, right, ..
                } => match &expr.kind {
                    ExprKind::Ident(h) => (h, vec![&**left, &**right]),
                    _ => continue,
                },
                _ => continue,
            };
            let Some(seg) = h.path.first() else {
                continue;
            };
            let name = seg.name.name.as_str();
            // A port actual is rooted (`$root`): its name is absolute.
            if self.module.parameters.contains_key(name) {
                continue;
            }
            if !self.signal_name_to_id.contains_key(name)
                && !self
                    .signal_name_to_id
                    .contains_key(format!("{}{}", top, name).as_str())
            {
                continue;
            }
            // A constant select reads those bits; any other, or a member
            // path, the whole variable.
            let constant = h.path.len() == 1 && sels.iter().all(|x| constant(self, x));
            let (key, e) = if constant && !sels.is_empty() {
                let vals: Vec<String> = sels
                    .iter()
                    .map(|x| format!("{:?}", self.eval_expr(x).raw_bits()))
                    .collect();
                (format!("{}[{}]", name, vals.join(":")), r.clone())
            } else {
                let mut bare = h.clone();
                bare.path.truncate(1);
                bare.path[0].selects.clear();
                (
                    name.to_string(),
                    Expression::new(ExprKind::Ident(bare), r.span),
                )
            };
            if keys.insert(key) {
                // Resolved now, with no process scope, and cached in the
                // expression for the runs.
                self.eval_expr(&e);
                out.push(e);
                if let Some(&id) = self.signal_name_to_id.get(name).or_else(|| {
                    self.signal_name_to_id
                        .get(format!("{}{}", top, name).as_str())
                }) {
                    ids.push(id);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        (out, ids)
    }

    /// Put the FSM markers into the state processes (after the other kinds
    /// have instrumented them).
    pub(super) fn fsm_instrument(fsm: &FsmCov, always: &mut [AlwaysBlock]) {
        for (i, p, vars) in &fsm.blocks {
            let Some(ab) = always.get_mut(*i) else {
                continue;
            };
            let span = ab.stmt.span;
            let s = std::mem::replace(&mut ab.stmt, Statement::new(StatementKind::Null, span));
            ab.stmt = match s.kind {
                StatementKind::TimingControl { control, stmt } => {
                    let inner = mark_writes(*stmt, vars);
                    Statement::new(
                        StatementKind::TimingControl {
                            control,
                            stmt: Box::new(marked(vec![mark(2 * p, span)], inner)),
                        },
                        span,
                    )
                }
                kind => Statement::new(kind, span),
            };
        }
    }

    /// An FSM marker (see `FSM_MARK`): a state process starting, or an
    /// assignment of a state variable. Re-running a block in the same time
    /// slot (a two-state block handed to the four-state VM) repeats the
    /// same marks, so both are idempotent within the slot.
    #[inline(never)]
    pub(super) fn fsm_mark(&mut self, m: u32) {
        let Some(f) = self.code_cov.as_mut().and_then(|c| c.fsm.as_mut()) else {
            return;
        };
        let k = (m >> 1) as usize;
        if m & 1 == 0 {
            let Some(p) = f.procs.get_mut(k) else {
                return;
            };
            let inputs = std::mem::take(&mut p.inputs);
            let clock = p.clock.take();
            let mut w = std::mem::take(&mut f.scratch);
            w.clear();
            for e in &inputs {
                let v = self.eval_expr(e);
                words(&v, v.width, &mut w);
            }
            let now = clock.as_ref().map(|(e, _)| bit(&self.eval_expr(e)));
            let Some(f) = self.code_cov.as_mut().and_then(|c| c.fsm.as_mut()) else {
                return;
            };
            let p = &mut f.procs[k];
            p.inputs = inputs;
            // A run without its clock's edge, or after this slot's clock
            // run, is an asynchronous one (a reset).
            let clocked = match (&clock, now) {
                (Some((_, edge)), Some(now)) => {
                    let edge = match edge {
                        Edge::Posedge => now == Some(true) && p.level != Some(true),
                        Edge::Negedge => now == Some(false) && p.level != Some(false),
                        Edge::Edge => now != p.level,
                    };
                    edge && !p.clocked
                }
                _ => true,
            };
            p.clock = clock;
            let changed = !p.seen || p.stale || w != p.words;
            if clocked {
                p.clocked = true;
                p.stale = false;
                std::mem::swap(&mut p.words, &mut w);
                p.seen = true;
            }
            f.scratch = w;
            if p.ran {
                p.dirty |= changed;
            } else {
                p.ran = true;
                p.dirty = changed;
                f.ran.push(k as u32);
            }
        } else {
            let Some(fsm) = f.fsms.get_mut(k) else {
                return;
            };
            let dirty = f.procs.get(fsm.proc as usize).is_some_and(|p| p.dirty);
            fsm.counted |= dirty;
            if !fsm.pending {
                fsm.pending = true;
                f.pending.push(k as u32);
            }
        }
    }

    /// End of a time slot: count the state each FSM written in it is in,
    /// and the transition into it.
    pub(super) fn cov_fsm_sample(&mut self) {
        let table = &self.signal_table;
        let time = self.time;
        let Some(f) = self.code_cov.as_mut().and_then(|c| c.fsm.as_mut()) else {
            return;
        };
        let value = |fsm: &Fsm| -> Option<Result<u32, ()>> {
            let (v, x) = table.get(fsm.sig)?.raw_bits();
            let m = if fsm.width >= 64 {
                u64::MAX
            } else {
                (1u64 << fsm.width) - 1
            };
            if x & m != 0 {
                return None;
            }
            Some(
                fsm.states
                    .binary_search_by_key(&(v & m), |s| s.0)
                    .map(|i| i as u32)
                    .map_err(|_| ()),
            )
        };
        for k in std::mem::take(&mut f.pending) {
            let fsm = &mut f.fsms[k as usize];
            let cur = value(fsm);
            if fsm.counted {
                match cur {
                    Some(Ok(i)) => fsm.hits[i as usize] += 1,
                    Some(Err(())) => fsm.illegal += 1,
                    None => {}
                }
            }
            let cur = cur.and_then(|c| c.ok());
            if let (Some(p), Some(c)) = (fsm.shadow, cur) {
                let t = fsm.tmap[p as usize * fsm.states.len() + c as usize];
                if t != u32::MAX {
                    fsm.thits[t as usize] += 1;
                }
            }
            fsm.shadow = cur;
            fsm.pending = false;
            fsm.counted = false;
        }
        for k in std::mem::take(&mut f.ran) {
            f.procs[k as usize].ran = false;
        }
        // At time 0 the variable takes its initial value (a declaration's
        // initializer, an initial block); the first transition leaves it.
        if time == 0 {
            for fsm in f.fsms.iter_mut() {
                fsm.shadow = value(fsm).and_then(|c| c.ok());
            }
        }
        self.fsm_watch();
    }

    /// End of a time slot: note the state processes an input of which now
    /// differs from what their last run read, so their next run counts even
    /// when the input has changed back by then (the commercial simulator
    /// runs a block after any change of its inputs). Only processes with an
    /// input written in the slot are compared.
    fn fsm_watch(&mut self) {
        let active = self.dump_dirty_active;
        let marks = &self.dump_dirty_mark;
        let epoch = self.dump_dirty_epoch;
        let Some(f) = self.code_cov.as_mut().and_then(|c| c.fsm.as_mut()) else {
            return;
        };
        let check: Vec<usize> = f
            .procs
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.seen
                    && !p.stale
                    && (!active
                        || p.ids
                            .iter()
                            .any(|&id| marks.get(id).is_some_and(|&m| m == epoch)))
            })
            .map(|(k, _)| k)
            .collect();
        // The clocks' levels, for the next slot's edges.
        for k in 0..f.procs.len() {
            let Some(clock) = self
                .code_cov
                .as_mut()
                .and_then(|c| c.fsm.as_mut())
                .and_then(|f| f.procs[k].clock.take())
            else {
                continue;
            };
            let level = bit(&self.eval_expr(&clock.0));
            if let Some(f) = self.code_cov.as_mut().and_then(|c| c.fsm.as_mut()) {
                let p = &mut f.procs[k];
                p.clock = Some(clock);
                p.level = level;
                p.clocked = false;
            }
        }
        for k in check {
            let Some(inputs) = self
                .code_cov
                .as_mut()
                .and_then(|c| c.fsm.as_mut())
                .map(|f| std::mem::take(&mut f.procs[k].inputs))
            else {
                return;
            };
            let mut w = Vec::new();
            for e in &inputs {
                let v = self.eval_expr(e);
                words(&v, v.width, &mut w);
            }
            if let Some(f) = self.code_cov.as_mut().and_then(|c| c.fsm.as_mut()) {
                let p = &mut f.procs[k];
                p.inputs = inputs;
                p.stale = w != p.words;
            }
        }
    }

    /// The FSMs of each scope, located in the source.
    pub(super) fn fsm_rows(
        &self,
        fsm: &FsmCov,
        scopes: usize,
        locate: &mut dyn FnMut(Span, u32) -> (String, u32),
    ) -> Vec<Vec<FsmRow>> {
        let mut out: Vec<Vec<FsmRow>> = (0..scopes).map(|_| Vec::new()).collect();
        for f in &fsm.fsms {
            let (file, line) = locate(f.span, f.scope);
            let name = |i: u32| f.states[i as usize].1.clone();
            out[f.scope as usize].push(FsmRow {
                name: f.name.clone(),
                next: f.next.clone(),
                file,
                line,
                start: f.span.start,
                states: f
                    .states
                    .iter()
                    .zip(&f.hits)
                    .map(|((v, n, sp), &c)| (n.clone(), *v, locate(*sp, f.scope).1, c))
                    .collect(),
                trans: f
                    .trans
                    .iter()
                    .zip(&f.thits)
                    .map(|((a, b, sp), &c)| (name(*a), name(*b), locate(*sp, f.scope).1, c))
                    .collect(),
                reset: f.reset.iter().map(|&i| name(i)).collect(),
                unreachable: f.unreachable.iter().map(|&i| name(i)).collect(),
                illegal: f.illegal,
            });
        }
        out
    }
}
