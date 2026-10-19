//! Unresolved hierarchical references (IEEE 1800-2023 §23.6, §23.8).
//!
//! A hierarchical name has to resolve at elaboration: its first component is
//! found downward from the scope that holds the reference or upward through
//! the enclosing scopes (§23.8), and every later component has to be declared
//! in the scope the previous one named. A reference that does not resolve is
//! an error. The simulator used to accept one silently, so a testbench that
//! wrote to a guessed internal name (`dut.mem_data[3] = ...`) ran as if the
//! write had happened, and a read of one returned x.
//!
//! The check runs on the elaborated design, so it sees exactly the generate
//! blocks and instances elaboration selected: a reference inside a generate
//! branch a constant rules out is never checked, as in the reference
//! simulator. It reports a name only when it is certain the name is missing:
//! the reference has to reach a scope whose declarations are all known (the
//! top, a module or interface instance, an instance-array element or a
//! generate block), and the next component has to name nothing declared in
//! it. A first component that names nothing at all is left to the
//! undeclared-identifier check; a component after a variable selects a member
//! of it (a struct field, a class property or method, an interface member).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use xezim_core::SourceDefinition;
use xezim_core::ast::Span;
use xezim_core::ast::decl::ModuleItem;
use xezim_core::ast::expr::{AssignmentPatternItem, ExprKind, Expression};
use xezim_core::ast::stmt::{
    EventControl, ForInit, ProceduralContinuous, Statement, StatementKind, TimingControl,
};
use xezim_core::elaborate::{ElaboratedModule, RewriteCtx};

/// One component of a hierarchical name: its identifier and the flat-key
/// rendering of its selects (`[1]`, empty for none), or `None` when a select
/// is not a literal.
struct Seg {
    name: String,
    sel: Option<String>,
}

impl Seg {
    fn show(&self) -> String {
        match &self.sel {
            Some(s) => format!("{}{}", self.name, s),
            None => format!("{}[...]", self.name),
        }
    }
}

/// A hierarchical reference: its components and where it is written.
struct Ref {
    segs: Vec<Seg>,
    span: Span,
}

/// A body's references and the scope they resolve from.
struct Body<'a> {
    refs: Rc<Vec<Ref>>,
    /// Flat scope of the body: `""` for the top, `u_a.u_b` or `u_a.g[1]`
    /// below it.
    scope: String,
    /// Definition whose source text the spans index (`None`: the one the
    /// scope's instance instantiates).
    owner: Option<&'a str>,
    /// The instance's port bindings for a body not yet rewritten into the
    /// instance: a name through a port is a member of the connected object.
    ctx: Option<&'a RewriteCtx>,
}

/// What one component names, seen from a scope.
enum Step {
    /// A declaration: what follows selects members of it.
    Item,
    /// A scope, under its flat name.
    Scope(String),
    Missing,
}

/// Names of the elaborated design, keyed the way elaboration flattens them.
struct Names<'a> {
    elab: &'a ElaboratedModule,
    /// Every flat name a declaration produced.
    items: HashSet<&'a str>,
    /// The same names plus every instance path, sorted for prefix queries.
    sorted: Vec<&'a str>,
    /// Instance path -> definition name.
    insts: HashMap<&'a str, &'a str>,
    /// Labels of every generate block in the design.
    gens: HashSet<String>,
    /// Names this check does not judge: packages, classes, interface and
    /// typedef names, modports, and instances added by `bind`.
    opaque: HashSet<String>,
    /// Named blocks of processes (`u.blk`): their locals are not all in the
    /// tables, so a name through one is not checked.
    blocks: HashSet<String>,
}

impl<'a> Names<'a> {
    fn new(defs: &[&SourceDefinition], elab: &'a ElaboratedModule) -> Self {
        let mut items: HashSet<&'a str> = HashSet::new();
        let keys: [&mut dyn Iterator<Item = &'a String>; 36] = [
            &mut elab.signals.keys(),
            &mut elab.parameters.keys(),
            &mut elab.arrays.keys(),
            &mut elab.associative_arrays.keys(),
            &mut elab.arrays_2d.keys(),
            &mut elab.arrays_nd.keys(),
            &mut elab.dynamic_arrays.iter(),
            &mut elab.queue_vars.iter(),
            &mut elab.functions.as_map().keys(),
            &mut elab.tasks.keys(),
            &mut elab.dpi_imports.keys(),
            &mut elab.sequences.iter(),
            &mut elab.property_decls.keys(),
            &mut elab.clocking_blocks.keys(),
            &mut elab.covergroups.keys(),
            &mut elab.lets.keys(),
            &mut elab.typedefs.as_map().keys(),
            &mut elab.typedef_types.keys(),
            &mut elab.classes.keys(),
            &mut elab.var_decl_types.keys(),
            &mut elab.struct_members.keys(),
            &mut elab.packed_struct_fields.keys(),
            &mut elab.packed_signal_elem_widths.keys(),
            &mut elab.packed_full_dims.keys(),
            &mut elab.unpacked_decl_dims.keys(),
            &mut elab.string_signals.iter(),
            &mut elab.nets.iter(),
            &mut elab.resolved_net_kinds.keys(),
            &mut elab.checker_decls.keys(),
            &mut elab.modport_views.keys(),
            &mut elab.clocking_signal_dirs.keys(),
            &mut elab.class_type_args.keys(),
            &mut elab.array_elem_class.keys(),
            &mut elab.pkg_var_decls.keys(),
            &mut elab.port_aliases.keys(),
            &mut elab.port_aliases.values(),
        ];
        for it in keys {
            items.extend(it.map(|k| k.as_str()));
        }
        let insts: HashMap<&'a str, &'a str> = elab
            .instances
            .iter()
            .map(|i| (i.path.as_str(), i.def_name.as_str()))
            .collect();
        let mut sorted: Vec<&'a str> = items.iter().chain(insts.keys()).copied().collect();
        sorted.sort_unstable();
        sorted.dedup();
        let mut gens = HashSet::new();
        let mut opaque: HashSet<String> = elab.packages.iter().cloned().collect();
        opaque.extend(elab.classes.keys().cloned());
        opaque.extend(elab.interfaces.iter().cloned());
        for def in defs {
            match def {
                SourceDefinition::Module(m) => labels(&m.items, &mut gens, &mut opaque),
                SourceDefinition::Interface(m) => labels(&m.items, &mut gens, &mut opaque),
                SourceDefinition::Program(m) => labels(&m.items, &mut gens, &mut opaque),
                SourceDefinition::Class(c) => {
                    opaque.insert(c.name.name.clone());
                }
                SourceDefinition::Package(p) => {
                    opaque.insert(p.name.name.clone());
                }
                SourceDefinition::Typedef(t) => {
                    opaque.insert(t.name.name.clone());
                }
                SourceDefinition::Udp(_) => {}
            }
        }
        Names {
            elab,
            items,
            sorted,
            insts,
            gens,
            opaque,
            blocks: HashSet::new(),
        }
    }

    /// A declaration has this flat name.
    fn item(&self, n: &str) -> bool {
        self.items.contains(n)
            || (!self.elab.elided_port_hashes.is_empty()
                && self
                    .elab
                    .elided_port_hashes
                    .binary_search(&xezim_core::elaborate::elided_port_hash(n))
                    .is_ok())
    }

    /// Some flat name starts with `p`.
    fn under(&self, p: &str) -> bool {
        let i = self.sorted.partition_point(|k| *k < p);
        self.sorted.get(i).is_some_and(|k| k.starts_with(p))
    }

    /// `n` names a scope: an instance, a generate or named block, or
    /// anything else that holds declarations of its own.
    fn scope(&self, n: &str) -> bool {
        self.insts.contains_key(n) || self.blocks.contains(n) || self.under(&format!("{n}."))
    }

    /// `n` is the top, an instance or a generate block: a scope whose every
    /// declaration is in the tables, so a name missing from them is
    /// certainly not declared in it.
    fn closed(&self, n: &str) -> bool {
        if n.is_empty() || self.insts.contains_key(n) {
            return true;
        }
        if self.item(n) || self.blocks.contains(n) {
            return false;
        }
        let leaf = n.rsplit('.').next().unwrap_or(n);
        let base = leaf.split('[').next().unwrap_or(leaf);
        self.gens.contains(base) || implicit(base)
    }

    /// What `seg` names in scope `cur`.
    fn step(&self, cur: &str, seg: &Seg) -> Step {
        let bare = join(cur, &seg.name);
        // An instance is a scope even where elaboration also keeps a signal
        // under its name (an interface instance's handle, say).
        if let Some(s) = &seg.sel {
            let key = format!("{bare}{s}");
            if self.insts.contains_key(key.as_str()) {
                return Step::Scope(key);
            }
        }
        if self.item(&bare) {
            return Step::Item;
        }
        match &seg.sel {
            Some(s) => {
                let key = format!("{bare}{s}");
                if !s.is_empty() && self.item(&key) {
                    return Step::Item;
                }
                if self.scope(&key) {
                    return Step::Scope(key);
                }
                Step::Missing
            }
            // A select that is not a literal: whichever element it names, an
            // array of scopes by that name is declared; nothing below it can
            // be checked.
            None if self.under(&format!("{bare}[")) => Step::Item,
            None => Step::Missing,
        }
    }

    /// The definition whose text a body in `scope` comes from.
    fn owner(&self, scope: &str) -> &'a str {
        let mut cur = scope;
        loop {
            if let Some(d) = self.insts.get(cur) {
                return d;
            }
            match parent(cur) {
                Some(p) => cur = p,
                None => return self.elab.name.as_str(),
            }
        }
    }

    /// A flat scope as the user would write it, from the top.
    fn show(&self, scope: &str) -> String {
        let top = self.elab.name.as_str();
        match (scope.is_empty(), top == xezim_core::MULTI_TOP_WRAPPER) {
            (true, true) => "$root".to_string(),
            (true, false) => top.to_string(),
            (false, true) => scope.to_string(),
            (false, false) => format!("{top}.{scope}"),
        }
    }

    /// Resolve the components after the first from scope `cur`.
    fn walk(&self, mut cur: String, segs: &[Seg]) -> Result<(), Miss> {
        for seg in segs {
            match self.step(&cur, seg) {
                Step::Item => return Ok(()),
                Step::Scope(s) => cur = s,
                // §27.6: an unnamed generate block's implicit name. Its
                // declarations are flattened into the enclosing scope.
                Step::Missing if implicit(&seg.name) => {}
                Step::Missing => {
                    // A DPI import declared in a module is a member of each
                    // instance (§35.5.1), registered under its bare name.
                    if self.opaque.contains(&seg.name)
                        || self.elab.dpi_imports.contains_key(&seg.name)
                        || !self.closed(&cur)
                    {
                        return Ok(());
                    }
                    return Err(Miss {
                        scope: cur,
                        name: seg.name.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Resolve a reference from `scope` (§23.8). `Ok` when it resolves, or
    /// when its first component names nothing hierarchical (a variable, a
    /// package, a class, an undeclared name); `Err` when it reaches a scope
    /// that does not declare its next component.
    fn resolve(&self, scope: &str, segs: &[Seg]) -> Result<(), Miss> {
        let head = &segs[0];
        if head.name == "$root" {
            // `$root.<top>`: the top is the root scope itself, unless several
            // tops sit under the synthetic root as instances.
            let rest = match segs.get(1) {
                Some(t) if t.name == self.elab.name => &segs[2..],
                Some(_) => &segs[1..],
                None => return Ok(()),
            };
            return self.walk(String::new(), rest);
        }
        if self.opaque.contains(&head.name)
            || matches!(head.name.as_str(), "this" | "super" | "local" | "std")
            || head.name.starts_with('$')
        {
            return Ok(());
        }
        // Where the first component can resolve, nearest first: a child of
        // the scope or of an enclosing one, an enclosing instance named by
        // its instance or definition name, or the top by its name.
        let mut starts: Vec<String> = Vec::new();
        let mut level = Some(scope);
        while let Some(l) = level {
            match self.step(l, head) {
                Step::Item => return Ok(()),
                Step::Scope(s) => starts.push(s),
                Step::Missing => {}
            }
            if l.is_empty() {
                if head.name == self.elab.name {
                    starts.push(String::new());
                }
            } else {
                let leaf = l.rsplit('.').next().unwrap_or(l);
                let named = head
                    .sel
                    .as_ref()
                    .is_some_and(|s| *leaf == format!("{}{s}", head.name));
                if named || self.insts.get(l).is_some_and(|d| *d == head.name) {
                    starts.push(l.to_string());
                }
            }
            level = parent(l);
        }
        let mut miss = None;
        for s in starts {
            match self.walk(s, &segs[1..]) {
                Ok(()) => return Ok(()),
                Err(m) => {
                    miss.get_or_insert(m);
                }
            }
        }
        miss.map_or(Ok(()), Err)
    }
}

/// How a reference failed: the scope it reached and the component that is
/// not declared there.
struct Miss {
    scope: String,
    name: String,
}

/// Generate-block labels and modport names of a definition's items.
fn labels(items: &[ModuleItem], gens: &mut HashSet<String>, opaque: &mut HashSet<String>) {
    for it in items {
        match it {
            ModuleItem::GenerateRegion(g) => labels(&g.items, gens, opaque),
            ModuleItem::GenerateIf(g) => {
                gens.extend(g.branch_labels.iter().flatten().cloned());
                for (_, b) in &g.branches {
                    labels(b, gens, opaque);
                }
            }
            ModuleItem::GenerateFor(g) => {
                gens.extend(g.name.iter().cloned());
                labels(&g.items, gens, opaque);
            }
            ModuleItem::GenerateCase(g) => {
                for arm in &g.arms {
                    gens.extend(arm.label.iter().cloned());
                    labels(&arm.items, gens, opaque);
                }
            }
            ModuleItem::ModportDeclaration(m) => {
                opaque.extend(m.items.iter().map(|i| i.name.name.clone()));
            }
            // An instance a `bind` adds: whether it lands where the user
            // expects is the bind support's business, not this check's.
            ModuleItem::Bind(b) => {
                opaque.extend(
                    b.instantiation
                        .instances
                        .iter()
                        .map(|i| i.name.name.clone()),
                );
            }
            _ => {}
        }
    }
}

/// `genblk<N>`, the implicit name of an unnamed generate block (§27.6).
fn implicit(name: &str) -> bool {
    name.strip_prefix("genblk")
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

/// The enclosing scope of a flat scope name; `None` for the top.
fn parent(scope: &str) -> Option<&str> {
    if scope.is_empty() {
        return None;
    }
    Some(scope.rsplit_once('.').map_or("", |(p, _)| p))
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        return name.to_string();
    }
    format!("{scope}.{name}")
}

/// The value of a literal select, as elaboration renders it in a flat name.
fn literal(e: &Expression) -> Option<u64> {
    use xezim_core::ast::expr::{NumberBase, NumberLiteral};
    match &e.kind {
        ExprKind::Paren(x) => literal(x),
        ExprKind::Number(NumberLiteral::Integer { value, base, .. }) => {
            let radix = match base {
                NumberBase::Binary => 2,
                NumberBase::Octal => 8,
                NumberBase::Hex => 16,
                NumberBase::Decimal => 10,
            };
            let digits: String = value.chars().filter(|c| *c != '_').collect();
            u64::from_str_radix(&digits, radix).ok()
        }
        _ => None,
    }
}

fn select(seg: &mut Seg, e: &Expression) {
    seg.sel = seg
        .sel
        .take()
        .zip(literal(e))
        .map(|(s, v)| format!("{s}[{v}]"));
}

fn seg(name: &str) -> Seg {
    Seg {
        name: name.to_string(),
        sel: Some(String::new()),
    }
}

/// The components of a dotted name with their selects, or `None` for
/// anything else (a call result, a cast, a concatenation...). A name the
/// inliner pinned to the parent scope (a port connection pasted into the
/// child) is not the child's to resolve.
fn segments(e: &Expression) -> Option<Vec<Seg>> {
    match &e.kind {
        ExprKind::Ident(h) => {
            if h.root.is_some() || h.path.is_empty() {
                return None;
            }
            let mut out = Vec::with_capacity(h.path.len() + 1);
            for s in &h.path {
                let mut x = seg(&s.name.name);
                for sel in &s.selects {
                    select(&mut x, sel);
                }
                out.push(x);
            }
            Some(out)
        }
        ExprKind::MemberAccess { expr, member } => {
            let mut out = segments(expr)?;
            out.push(seg(&member.name));
            Some(out)
        }
        ExprKind::Index { expr, index } => {
            let mut out = segments(expr)?;
            select(out.last_mut()?, index);
            Some(out)
        }
        _ => None,
    }
}

/// Where a dotted name's first component starts.
fn start(e: &Expression) -> usize {
    match &e.kind {
        ExprKind::Ident(h) => h.path.first().map_or(e.span.start, |s| s.name.span.start),
        ExprKind::MemberAccess { expr, .. } | ExprKind::Index { expr, .. } => start(expr),
        _ => e.span.start,
    }
}

/// Visit the select expressions inside a dotted name.
fn selects(e: &Expression, f: &mut dyn FnMut(&Expression)) {
    match &e.kind {
        ExprKind::Ident(h) => h.path.iter().flat_map(|s| &s.selects).for_each(f),
        ExprKind::MemberAccess { expr, .. } => selects(expr, f),
        ExprKind::Index { expr, index } => {
            selects(expr, f);
            f(index);
        }
        _ => {}
    }
}

/// System tasks whose arguments name scopes rather than values (waveform
/// dumps, assertion and coverage control, SDF): see the core's
/// undeclared-identifier check, which skips the same ones.
fn scope_args(name: &str) -> bool {
    [
        "$dump",
        "$fsdb",
        "$vcdplus",
        "$shm",
        "$record",
        "$probe",
        "$xm",
        "$indago",
        "$vwdb",
        "$sdf",
        "$assert",
        "$coverage",
        "$get_coverage",
        "$set_coverage",
        "$load_coverage",
        "$printtimescale",
        "$timeformat",
        "$scope",
        "$list",
        "$show",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

/// Collect the hierarchical references of an expression: its dotted names
/// of two or more components.
fn expr_refs(e: &Expression, out: &mut Vec<Ref>) {
    if let Some(segs) = segments(e) {
        if segs.len() > 1 {
            // An index node's span can start at its `[`; report from the
            // name's first component.
            let span = Span {
                start: start(e).min(e.span.start),
                end: e.span.end,
            };
            out.push(Ref { segs, span });
        }
        selects(e, &mut |x| expr_refs(x, out));
        return;
    }
    let mut each = |x: &Expression| expr_refs(x, out);
    match &e.kind {
        ExprKind::Unary { operand, .. } => each(operand),
        ExprKind::Binary { left, right, .. } | ExprKind::Range(left, right) => {
            each(left);
            each(right);
        }
        ExprKind::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            each(condition);
            each(then_expr);
            each(else_expr);
        }
        ExprKind::Concatenation(xs) => xs.iter().for_each(each),
        ExprKind::Replication { count, exprs } => {
            each(count);
            exprs.iter().for_each(each);
        }
        ExprKind::AssignmentPattern(items) => {
            for it in items {
                match it {
                    AssignmentPatternItem::Ordered(x)
                    | AssignmentPatternItem::Named(_, x)
                    | AssignmentPatternItem::Typed(_, x)
                    | AssignmentPatternItem::Default(x) => each(x),
                    AssignmentPatternItem::Keyed(k, x) => {
                        each(k);
                        each(x);
                    }
                }
            }
        }
        ExprKind::Call { func, args } => {
            each(func);
            args.iter().for_each(each);
        }
        ExprKind::SystemCall { name, args } if !scope_args(name) => args.iter().for_each(each),
        ExprKind::NamedArg { expr: Some(x), .. }
        | ExprKind::Paren(x)
        | ExprKind::Matches { expr: x, .. }
        | ExprKind::MemberAccess { expr: x, .. }
        | ExprKind::RandomizeWith { call: x, .. }
        | ExprKind::Tagged { inner: Some(x), .. }
        | ExprKind::ShallowCopy { source: x } => each(x),
        ExprKind::Inside { expr, ranges } => {
            each(expr);
            ranges.iter().for_each(each);
        }
        ExprKind::Index { expr, index } => {
            each(expr);
            each(index);
        }
        ExprKind::RangeSelect {
            expr, left, right, ..
        } => {
            each(expr);
            each(left);
            each(right);
        }
        ExprKind::WithClause { expr, filter } => {
            each(expr);
            each(filter);
        }
        ExprKind::AssignExpr { lvalue, rvalue } => {
            each(lvalue);
            each(rvalue);
        }
        ExprKind::StreamOp {
            slice_size, exprs, ..
        } => {
            slice_size.iter().for_each(|x| each(x));
            exprs.iter().for_each(each);
        }
        ExprKind::SvaClocked {
            clock, iff, body, ..
        } => {
            each(clock);
            iff.iter().for_each(|x| each(x));
            each(body);
        }
        _ => {}
    }
}

fn control_refs(c: &TimingControl, out: &mut Vec<Ref>) {
    match c {
        TimingControl::Delay(d) => expr_refs(d, out),
        TimingControl::Event(EventControl::HierIdentifier(e)) => expr_refs(e, out),
        TimingControl::Event(EventControl::EventExpr(evs)) => {
            for ev in evs {
                expr_refs(&ev.expr, out);
                ev.iff.iter().for_each(|x| expr_refs(x, out));
            }
        }
        TimingControl::Event(_) => {}
    }
}

/// Collect the hierarchical references of a statement, and the names it
/// declares (`locals`), which shadow any scope of the same name.
fn stmt_refs(s: &Statement, out: &mut Vec<Ref>, locals: &mut HashSet<String>) {
    match &s.kind {
        StatementKind::Expr(e) | StatementKind::Return(Some(e)) => expr_refs(e, out),
        StatementKind::BlockingAssign { lvalue, rvalue } => {
            expr_refs(lvalue, out);
            expr_refs(rvalue, out);
        }
        StatementKind::NonblockingAssign {
            lvalue,
            delay,
            rvalue,
        } => {
            expr_refs(lvalue, out);
            delay.iter().for_each(|x| expr_refs(x, out));
            expr_refs(rvalue, out);
        }
        StatementKind::If {
            condition,
            then_stmt,
            else_stmt,
            ..
        } => {
            expr_refs(condition, out);
            stmt_refs(then_stmt, out, locals);
            else_stmt.iter().for_each(|x| stmt_refs(x, out, locals));
        }
        StatementKind::Case { expr, items, .. } => {
            expr_refs(expr, out);
            for it in items {
                it.patterns.iter().for_each(|x| expr_refs(x, out));
                it.guard.iter().for_each(|x| expr_refs(x, out));
                stmt_refs(&it.stmt, out, locals);
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
                    ForInit::VarDecl { name, init, .. } => {
                        locals.insert(name.name.clone());
                        expr_refs(init, out);
                    }
                    ForInit::Assign { lvalue, rvalue } => {
                        expr_refs(lvalue, out);
                        expr_refs(rvalue, out);
                    }
                }
            }
            condition.iter().for_each(|x| expr_refs(x, out));
            step.iter().for_each(|x| expr_refs(x, out));
            stmt_refs(body, out, locals);
        }
        StatementKind::Foreach { array, vars, body } => {
            expr_refs(array, out);
            locals.extend(vars.iter().flatten().map(|v| v.name.clone()));
            stmt_refs(body, out, locals);
        }
        StatementKind::While { condition, body } | StatementKind::DoWhile { body, condition } => {
            expr_refs(condition, out);
            stmt_refs(body, out, locals);
        }
        StatementKind::Repeat { count, body } => {
            expr_refs(count, out);
            stmt_refs(body, out, locals);
        }
        StatementKind::Forever { body }
        | StatementKind::ForeverTail { body }
        | StatementKind::RsAction { body }
        | StatementKind::ForeachTail { body, .. } => stmt_refs(body, out, locals),
        StatementKind::SeqBlock { stmts, .. } | StatementKind::ParBlock { stmts, .. } => {
            stmts.iter().for_each(|x| stmt_refs(x, out, locals));
        }
        StatementKind::TimingControl { control, stmt } => {
            control_refs(control, out);
            stmt_refs(stmt, out, locals);
        }
        StatementKind::EventTrigger {
            target: Some(t), ..
        } => expr_refs(t, out),
        StatementKind::Wait { condition, stmt } => {
            expr_refs(condition, out);
            stmt_refs(stmt, out, locals);
        }
        StatementKind::Assertion(a) => {
            expr_refs(&a.expr, out);
            a.inferred_clock
                .iter()
                .for_each(|c| expr_refs(&c.expr, out));
            for x in a.action.iter().chain(a.else_action.iter()) {
                stmt_refs(x, out, locals);
            }
        }
        StatementKind::ProceduralContinuous(p) => match p {
            ProceduralContinuous::Assign { lvalue, rvalue }
            | ProceduralContinuous::Force { lvalue, rvalue } => {
                expr_refs(lvalue, out);
                expr_refs(rvalue, out);
            }
            ProceduralContinuous::Deassign(x) | ProceduralContinuous::Release(x) => {
                expr_refs(x, out)
            }
        },
        StatementKind::VarDecl { declarators, .. } => {
            for d in declarators {
                locals.insert(d.name.name.clone());
                d.init.iter().for_each(|x| expr_refs(x, out));
            }
        }
        StatementKind::RandCase { items } => {
            for (w, x) in items {
                expr_refs(w, out);
                stmt_refs(x, out, locals);
            }
        }
        StatementKind::WaitOrder { pass, fail, .. } => {
            for x in pass.iter().chain(fail.iter()) {
                stmt_refs(x, out, locals);
            }
        }
        _ => {}
    }
}

/// The references of a body, without those whose first component is one of
/// the body's own declarations.
fn body_refs(stmts: &[&Statement], mut locals: HashSet<String>) -> Rc<Vec<Ref>> {
    let mut refs = Vec::new();
    for s in stmts {
        stmt_refs(s, &mut refs, &mut locals);
    }
    refs.retain(|r| !locals.contains(&r.segs[0].name));
    Rc::new(refs)
}

/// Flat names of the named blocks in a statement.
fn block_names(s: &Statement, scope: &str, out: &mut HashSet<String>) {
    match &s.kind {
        StatementKind::SeqBlock { name, stmts } | StatementKind::ParBlock { name, stmts, .. } => {
            if let Some(n) = name {
                out.insert(join(scope, &n.name));
            }
            stmts.iter().for_each(|x| block_names(x, scope, out));
        }
        StatementKind::If {
            then_stmt,
            else_stmt,
            ..
        } => {
            block_names(then_stmt, scope, out);
            else_stmt.iter().for_each(|x| block_names(x, scope, out));
        }
        StatementKind::Case { items, .. } => {
            items
                .iter()
                .for_each(|it| block_names(&it.stmt, scope, out));
        }
        StatementKind::For { body, .. }
        | StatementKind::Foreach { body, .. }
        | StatementKind::While { body, .. }
        | StatementKind::DoWhile { body, .. }
        | StatementKind::Repeat { body, .. }
        | StatementKind::Forever { body }
        | StatementKind::TimingControl { stmt: body, .. }
        | StatementKind::Wait { stmt: body, .. } => block_names(body, scope, out),
        _ => {}
    }
}

/// Check every hierarchical reference of the elaborated design. The errors,
/// one line per unresolved reference, as one entry (none when all resolve).
pub fn check(defs: &[&SourceDefinition], elab: &ElaboratedModule) -> Vec<String> {
    let mut bodies: Vec<Body> = Vec::new();
    let mut blocks = HashSet::new();
    let mut add = |refs: Rc<Vec<Ref>>, scope: String, owner, ctx| {
        if !refs.is_empty() {
            bodies.push(Body {
                refs,
                scope,
                owner,
                ctx,
            });
        }
    };
    // Bodies elaboration has already rewritten into their instance.
    for (stmt, scope) in elab
        .always_blocks
        .iter()
        .map(|b| (&b.stmt, &b.scope))
        .chain(
            elab.initial_blocks
                .iter()
                .chain(&elab.final_blocks)
                .chain(&elab.static_init_blocks)
                .chain(&elab.program_initial_blocks)
                .map(|b| (&b.stmt, &b.scope)),
        )
    {
        block_names(stmt, scope, &mut blocks);
        add(
            body_refs(&[stmt], HashSet::new()),
            scope.clone(),
            None,
            None,
        );
    }
    for ca in &elab.continuous_assigns {
        // Only assignments the source spells out; port connections and
        // gates are elaboration's own.
        if let Some((_, scope)) = &ca.origin {
            let mut refs = Vec::new();
            expr_refs(&ca.lhs, &mut refs);
            expr_refs(&ca.rhs, &mut refs);
            add(Rc::new(refs), scope.clone(), None, None);
        }
    }
    let subroutines = elab
        .tasks
        .iter()
        .map(|(k, t)| (k, &t.ports, &t.items))
        .chain(
            elab.functions
                .as_map()
                .iter()
                .map(|(k, f)| (k, &f.ports, &f.items)),
        );
    for (key, ports, items) in subroutines {
        // A package subroutine cannot name the design hierarchy (§26.2).
        if key.contains("::")
            || elab.pkg_subr_owner.contains_key(key)
            || elab.func_decl_scope.contains_key(key)
        {
            continue;
        }
        let formals = ports.iter().map(|p| p.name.name.clone()).collect();
        let stmts: Vec<&Statement> = items.iter().collect();
        add(
            body_refs(&stmts, formals),
            parent(key).unwrap_or("").to_string(),
            None,
            None,
        );
    }
    for (key, body) in &elab.property_decls {
        let mut refs = Vec::new();
        expr_refs(body, &mut refs);
        let formals = elab.property_params.get(key);
        refs.retain(|r| !formals.is_some_and(|f| f.contains(&r.segs[0].name)));
        add(
            Rc::new(refs),
            parent(key).unwrap_or("").to_string(),
            None,
            None,
        );
    }
    // Bodies still in their definition's text, with the instance's
    // bindings. Sibling instances share the text: collect it once.
    let mut memo: HashMap<*const Statement, Rc<Vec<Ref>>> = HashMap::new();
    let pending = elab
        .pending_always
        .iter()
        .map(|p| (&p.source, &p.ctx, p.scope()))
        .chain(elab.pending_initial.iter().map(|p| {
            let base = p.ctx.prefix.trim_end_matches('.');
            let scope = match p.gen_scope.as_str() {
                "" => base.to_string(),
                g => join(base, g),
            };
            (&p.source, &p.ctx, scope)
        }));
    for (source, ctx, scope) in pending {
        block_names(source, &scope, &mut blocks);
        let refs = memo
            .entry(Rc::as_ptr(source))
            .or_insert_with(|| body_refs(&[source.as_ref()], HashSet::new()))
            .clone();
        add(refs, scope, Some(ctx.owner.as_str()), Some(ctx.as_ref()));
    }
    for p in &elab.pending_cont_assign {
        if p.origin_span.is_some() {
            let mut refs = Vec::new();
            expr_refs(&p.lhs_source, &mut refs);
            expr_refs(&p.rhs_source, &mut refs);
            let scope = p.ctx.prefix.trim_end_matches('.').to_string();
            add(
                Rc::new(refs),
                scope,
                Some(p.ctx.owner.as_str()),
                Some(p.ctx.as_ref()),
            );
        }
    }
    if bodies.is_empty() {
        return Vec::new();
    }
    let mut names = Names::new(defs, elab);
    names.blocks = blocks;
    let debug = std::env::var_os("XEZIM_HIER_REF_DEBUG").is_some();
    let mut seen = HashSet::new();
    let mut errs = Vec::new();
    for b in &bodies {
        for r in b.refs.iter() {
            let head = r.segs[0].name.as_str();
            if b.ctx.is_some_and(|c| {
                c.port_map.contains_key(head) || c.interface_map.contains_key(head)
            }) {
                continue;
            }
            let res = names.resolve(&b.scope, &r.segs);
            let full: Vec<String> = r.segs.iter().map(Seg::show).collect();
            if debug {
                let what = match &res {
                    Ok(()) => "resolves".to_string(),
                    Err(m) => format!("'{}' missing in '{}'", m.name, m.scope),
                };
                eprintln!("[HIER-REF] {:?}: {} {}", b.scope, full.join("."), what);
            }
            let Err(m) = res else { continue };
            let owner = b.owner.unwrap_or_else(|| names.owner(&b.scope));
            let msg = format!(
                "hierarchical name '{}' does not resolve: '{}' is not declared in '{}' \
                 (searched from '{}'; IEEE 1800-2023 §23.8)",
                full.join("."),
                m.name,
                names.show(&m.scope),
                names.show(&b.scope),
            );
            let err = match xezim_core::elaborate::span_location_of(elab, r.span, owner) {
                Some(l) => format!("{l}: error: {msg}"),
                None => format!("error: {msg}"),
            };
            // One report per name and place: a macro expands to the same
            // location, and sibling instances share their text.
            if seen.insert(err.clone()) {
                errs.push((owner, r.span.start, err));
            }
        }
    }
    if errs.is_empty() {
        return Vec::new();
    }
    // In source order, one per line.
    errs.sort();
    vec![
        errs.into_iter()
            .map(|(_, _, e)| e)
            .collect::<Vec<_>>()
            .join("\n"),
    ]
}
