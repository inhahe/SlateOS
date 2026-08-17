//! Deciding which names are arrays, before the program runs.
//!
//! ## Why this pass has to exist
//!
//! ```awk
//! function fill(a) { a[1] = "x" }
//! BEGIN { fill(v); print v[1] }
//! ```
//!
//! must print `x`. Arrays are passed to functions **by reference**, so `fill`
//! has to be given the caller's `v` and not a copy — but at the call site `v`
//! has never been touched, so there is nothing about its *value* that says it is
//! an array. The only evidence is how the callee uses it, which is somewhere
//! else in the program.
//!
//! An interpreter that decides at run time gets this wrong: it sees an
//! untouched variable, passes it by value, and `print v[1]` prints nothing. The
//! bug is silent, and the idiom is common — it is how awk programs return more
//! than one value.
//!
//! ## How it decides
//!
//! Two rules, applied until nothing changes:
//!
//! 1. A name subscripted, deleted, iterated with `in`, or given to `split` is
//!    an array. A name assigned, read, incremented, or given to any other
//!    built-in is a scalar.
//! 2. If a plain variable is passed as argument *i* of function `f`, then it is
//!    an array exactly when `f`'s parameter *i* is — and a scalar exactly when
//!    that parameter is — in either direction. That is the rule that carries the
//!    evidence from `fill`'s body back to the call site, and through chains of
//!    calls.
//!
//! The fixed point is reached quickly because each round can only turn `false`
//! into `true`, and there are finitely many names.
//!
//! ## The conflict
//!
//! A name that ends up marked both ways — `x[1] = 1; y = x` — is refused. awk
//! has no value that is both, so there is no execution that could be right:
//! either the subscript or the plain use is a typo. Catching it here means the
//! diagnostic arrives before any output has been produced, where gawk's
//! run-time check can arrive halfway through a report.
//!
//! Two uses that *look* scalar are exempt, because they ask a question about an
//! array rather than about a value: the bare name in `length(a)`, and `a` in
//! `split(s, a)` — which is the array-marking use itself.

use crate::ast::{Builtin, Expr, Lvalue, Pattern, Program, Stmt, VarRef, V_ARGV, V_ENVIRON};

/// One `f(…, v, …)` argument, recorded so the two ends can agree later.
struct Link {
    /// The function whose body contains the call, or `None` for a rule.
    caller: Option<usize>,
    callee: usize,
    param: usize,
    var: VarRef,
}

/// The two ways a name can be used. A name that collects both is the error this
/// pass exists to report.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    Array,
    Scalar,
}

/// What a name has been seen doing. Both flags can be set; that is the conflict.
#[derive(Clone, Copy, Default)]
struct Flags {
    array: bool,
    scalar: bool,
}

impl Flags {
    fn has(self, u: Use) -> bool {
        match u {
            Use::Array => self.array,
            Use::Scalar => self.scalar,
        }
    }

    /// Set one flag. Returns whether this changed anything, which is what drives
    /// the fixed point.
    fn set(&mut self, u: Use) -> bool {
        let slot = match u {
            Use::Array => &mut self.array,
            Use::Scalar => &mut self.scalar,
        };
        let was = *slot;
        *slot = true;
        !was
    }
}

/// A name used both ways, recorded at the moment the second use is seen so the
/// message can say where the two uses were rather than only that they existed.
///
/// Kept as slots rather than as a formatted string because the names live in the
/// `Program`, which the walk does not carry.
enum Conflict {
    /// One name, used both ways within its own scope.
    Direct { ctx: Option<usize>, var: VarRef },
    /// A variable and the parameter it is passed to. Neither is wrong on its
    /// own; it is the call that puts them together. `var_use` is the variable's
    /// side, so the message can name the two ends in the right order.
    Passed { ctx: Option<usize>, var: VarRef, callee: usize, param: usize, var_use: Use },
}

struct Pass {
    global: Vec<Flags>,
    param: Vec<Vec<Flags>>,
    links: Vec<Link>,
    changed: bool,
    /// The first conflict seen. First, not all of them: the second is usually
    /// the first one's echo through another call, and awk stops at the first
    /// anyway.
    conflict: Option<Conflict>,
}

/// Work out which globals and which function parameters are arrays.
///
/// Writes the answer into `prog`. Reports the one case that cannot be resolved:
/// a name used both ways.
///
/// # Errors
/// Returns a diagnostic if a name is used as both a scalar and an array, which
/// awk cannot represent and which is always a bug in the program.
pub fn resolve(prog: &mut Program) -> Result<(), String> {
    let mut p = Pass {
        global: vec![Flags::default(); prog.globals],
        param: prog.funcs.iter().map(|f| vec![Flags::default(); f.params.len()]).collect(),
        links: Vec::new(),
        changed: true,
        conflict: None,
    };
    // The two built-in arrays are known without looking.
    for special in [V_ENVIRON, V_ARGV] {
        if let Some(slot) = p.global.get_mut(special) {
            slot.array = true;
        }
    }

    for rule in &prog.rules {
        match &rule.pattern {
            Pattern::Always => {}
            Pattern::Expr(e) => p.expr(None, e),
            Pattern::Range(a, b, _) => {
                p.expr(None, a);
                p.expr(None, b);
            }
        }
        if let Some(body) = &rule.action {
            p.stmts(None, body);
        }
    }
    p.stmts(None, &prog.begin);
    p.stmts(None, &prog.end);
    for (i, f) in prog.funcs.iter().enumerate() {
        p.stmts(Some(i), &f.body);
    }

    // Rule 2, to a fixed point.
    let mut rounds = 0usize;
    while p.changed {
        p.changed = false;
        rounds = rounds.saturating_add(1);
        // Each round can only set flags, never clear them, so this terminates;
        // the bound is belt and braces against a future edit that breaks that.
        if rounds > p.links.len().saturating_add(2) {
            break;
        }
        for i in 0..p.links.len() {
            let Some(link) = p.links.get(i) else { continue };
            let (caller, callee, param, var) = (link.caller, link.callee, link.param, link.var);
            let at_callee = p.param.get(callee).and_then(|v| v.get(param)).copied().unwrap_or_default();
            let at_caller = p.flags(caller, var);
            // The argument and the parameter are the same storage, so each end
            // of the link teaches the other — in both directions and for both
            // kinds of use.
            for u in [Use::Array, Use::Scalar] {
                if at_callee.has(u) && !at_caller.has(u) {
                    // The callee says `u`; if the caller's variable already says
                    // the opposite, the call is what put them together, and
                    // saying so is more use than naming either end alone.
                    if at_caller.has(other(u)) && p.conflict.is_none() {
                        p.conflict = Some(Conflict::Passed {
                            ctx: caller,
                            var,
                            callee,
                            param,
                            var_use: other(u),
                        });
                    }
                    p.mark(caller, var, u);
                }
                if at_caller.has(u) && !at_callee.has(u) {
                    if at_callee.has(other(u)) && p.conflict.is_none() {
                        p.conflict =
                            Some(Conflict::Passed { ctx: caller, var, callee, param, var_use: u });
                    }
                    if let Some(slot) = p.param.get_mut(callee).and_then(|v| v.get_mut(param))
                        && slot.set(u)
                    {
                        p.changed = true;
                    }
                }
            }
        }
    }

    if let Some(c) = &p.conflict {
        return Err(describe(prog, c));
    }
    prog.global_is_array = p.global.iter().map(|f| f.array).collect();
    prog.param_is_array = p.param.iter().map(|fs| fs.iter().map(|f| f.array).collect()).collect();
    Ok(())
}

fn other(u: Use) -> Use {
    match u {
        Use::Array => Use::Scalar,
        Use::Scalar => Use::Array,
    }
}

/// What a name is called, in a form that reads inside a sentence.
fn name_of(prog: &Program, ctx: Option<usize>, v: VarRef) -> String {
    match v {
        VarRef::Global(s) => prog.global_names.get(s).cloned().unwrap_or_else(|| "?".to_string()),
        VarRef::Local(s) => {
            let Some(f) = ctx.and_then(|f| prog.funcs.get(f)) else { return "?".to_string() };
            let name = f.params.get(s).map_or("?", String::as_str);
            format!("{}'s parameter {name}", f.name)
        }
    }
}

fn describe(prog: &Program, c: &Conflict) -> String {
    match c {
        Conflict::Direct { ctx, var } => {
            format!("{} is used both as an array and as a scalar", name_of(prog, *ctx, *var))
        }
        Conflict::Passed { ctx, var, callee, param, var_use } => {
            let (was, becomes) = match var_use {
                Use::Array => ("an array", "a scalar"),
                Use::Scalar => ("a scalar", "an array"),
            };
            let callee_name = prog.funcs.get(*callee).map_or("?", |f| f.name.as_str());
            let param_name = prog
                .funcs
                .get(*callee)
                .and_then(|f| f.params.get(*param))
                .map_or("?", String::as_str);
            format!(
                "{} is {was}, but it is passed to {callee_name} as {param_name}, which is {becomes}",
                name_of(prog, *ctx, *var)
            )
        }
    }
}

impl Pass {
    fn flags(&self, ctx: Option<usize>, v: VarRef) -> Flags {
        match v {
            VarRef::Global(s) => self.global.get(s).copied().unwrap_or_default(),
            VarRef::Local(s) => ctx
                .and_then(|f| self.param.get(f))
                .and_then(|ps| ps.get(s))
                .copied()
                .unwrap_or_default(),
        }
    }

    fn mark(&mut self, ctx: Option<usize>, v: VarRef, u: Use) {
        // A use that contradicts one already recorded for this name is the
        // conflict, and this is the earliest point at which it is visible. The
        // walk is in source order, so the first one found is the first the
        // reader would have written.
        if self.conflict.is_none() && self.flags(ctx, v).has(other(u)) {
            self.conflict = Some(Conflict::Direct { ctx, var: v });
        }
        let slot = match v {
            VarRef::Global(s) => self.global.get_mut(s),
            VarRef::Local(s) => ctx.and_then(|f| self.param.get_mut(f)).and_then(|ps| ps.get_mut(s)),
        };
        if let Some(slot) = slot
            && slot.set(u)
        {
            self.changed = true;
        }
    }

    fn stmts(&mut self, ctx: Option<usize>, body: &[Stmt]) {
        for s in body {
            self.stmt(ctx, s);
        }
    }

    fn stmt(&mut self, ctx: Option<usize>, s: &Stmt) {
        match s {
            Stmt::Expr(e) => self.expr(ctx, e),
            Stmt::Print(args, r) | Stmt::Printf(args, r) => {
                for a in args {
                    self.expr(ctx, a);
                }
                if let Some(r) = r {
                    self.expr(ctx, &r.target);
                }
            }
            Stmt::Block(b) => self.stmts(ctx, b),
            Stmt::If(c, t, e) => {
                self.expr(ctx, c);
                self.stmt(ctx, t);
                if let Some(e) = e {
                    self.stmt(ctx, e);
                }
            }
            Stmt::While(c, b) => {
                self.expr(ctx, c);
                self.stmt(ctx, b);
            }
            Stmt::DoWhile(b, c) => {
                self.stmt(ctx, b);
                self.expr(ctx, c);
            }
            Stmt::For { init, cond, step, body } => {
                if let Some(s) = init {
                    self.stmt(ctx, s);
                }
                if let Some(c) = cond {
                    self.expr(ctx, c);
                }
                if let Some(s) = step {
                    self.stmt(ctx, s);
                }
                self.stmt(ctx, body);
            }
            Stmt::ForIn { var, array, body } => {
                self.lvalue(ctx, var);
                self.mark(ctx, *array, Use::Array);
                self.stmt(ctx, body);
            }
            Stmt::Exit(e) | Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(ctx, e);
                }
            }
            Stmt::Delete(arr, subs) => {
                self.mark(ctx, *arr, Use::Array);
                for s in subs {
                    self.expr(ctx, s);
                }
            }
            Stmt::Next | Stmt::NextFile | Stmt::Break | Stmt::Continue | Stmt::Nop => {}
        }
    }

    fn lvalue(&mut self, ctx: Option<usize>, lv: &Lvalue) {
        match lv {
            Lvalue::Var(v) => self.mark(ctx, *v, Use::Scalar),
            Lvalue::Field(e) => self.expr(ctx, e),
            Lvalue::Index(v, subs) => {
                self.mark(ctx, *v, Use::Array);
                for s in subs {
                    self.expr(ctx, s);
                }
            }
        }
    }

    fn expr(&mut self, ctx: Option<usize>, e: &Expr) {
        match e {
            Expr::Num(_) | Expr::Str(_) | Expr::Regex(_) => {}
            Expr::Get(lv) => self.lvalue(ctx, lv),
            Expr::Assign(lv, rhs) => {
                self.lvalue(ctx, lv);
                self.expr(ctx, rhs);
            }
            Expr::AugAssign(lv, _, rhs) => {
                self.lvalue(ctx, lv);
                self.expr(ctx, rhs);
            }
            Expr::Cond(a, b, c) => {
                self.expr(ctx, a);
                self.expr(ctx, b);
                self.expr(ctx, c);
            }
            Expr::Or(a, b) | Expr::And(a, b) | Expr::Concat(a, b) | Expr::Bin(_, a, b) | Expr::Cmp(_, a, b) => {
                self.expr(ctx, a);
                self.expr(ctx, b);
            }
            Expr::Match { lhs, rhs, .. } => {
                self.expr(ctx, lhs);
                self.expr(ctx, rhs);
            }
            Expr::In(subs, arr) => {
                for s in subs {
                    self.expr(ctx, s);
                }
                self.mark(ctx, *arr, Use::Array);
            }
            Expr::Neg(a) | Expr::Pos(a) | Expr::Not(a) => self.expr(ctx, a),
            Expr::PreIncr(lv, _) | Expr::PostIncr(lv, _) => self.lvalue(ctx, lv),
            Expr::Call(f, args) => {
                for (i, a) in args.iter().enumerate() {
                    // Only a *bare* variable can be an array argument; anything
                    // else is an expression and therefore a scalar.
                    if let Expr::Get(Lvalue::Var(v)) = a {
                        self.links.push(Link { caller: ctx, callee: *f, param: i, var: *v });
                    } else {
                        self.expr(ctx, a);
                    }
                }
            }
            Expr::Builtin(b, args) => {
                for (i, a) in args.iter().enumerate() {
                    if *b == Builtin::Split && i == 1 {
                        if let Expr::Get(Lvalue::Var(v)) = a {
                            self.mark(ctx, *v, Use::Array);
                        }
                        continue;
                    }
                    // `length(a)` is the one place a bare name may be either: on
                    // an array it is the element count, on a scalar the string
                    // length. It is therefore no evidence at all, so it must not
                    // be allowed to record a scalar use — that would make
                    // `split(s, a); print length(a)` a conflict.
                    if *b == Builtin::Length && matches!(a, Expr::Get(Lvalue::Var(_))) {
                        continue;
                    }
                    self.expr(ctx, a);
                }
            }
            Expr::Getline(g) => {
                if let Some(lv) = &g.into {
                    self.lvalue(ctx, lv);
                }
                match &g.src {
                    crate::ast::GetlineSrc::Main => {}
                    crate::ast::GetlineSrc::File(e) | crate::ast::GetlineSrc::Cmd(e) => self.expr(ctx, e),
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::resolve;
    use crate::parse::parse;

    /// Run the pass, and report which globals it decided are arrays, by name.
    fn arrays(src: &str) -> Result<Vec<String>, String> {
        let mut prog = parse(src.as_bytes())?;
        resolve(&mut prog)?;
        Ok(prog
            .global_names
            .iter()
            .enumerate()
            .filter(|(i, _)| prog.global_is_array.get(*i).copied().unwrap_or(false))
            .map(|(_, n)| n.clone())
            .collect())
    }

    #[test]
    fn a_callees_use_of_a_parameter_types_the_callers_variable() {
        // The whole reason the pass exists: at the call site `v` has never been
        // touched, so only `fill`'s body says it is an array.
        let a = arrays("function fill(a) {a[1] = \"x\"} BEGIN {fill(v); print v[1]}").unwrap();
        assert!(a.contains(&"v".to_string()), "{a:?}");
    }

    #[test]
    fn the_evidence_travels_through_a_chain_of_calls() {
        let a = arrays("function inner(x) {x[1] = 1} function outer(y) {inner(y)} BEGIN {outer(g)}")
            .unwrap();
        assert!(a.contains(&"g".to_string()), "{a:?}");
    }

    #[test]
    fn a_name_used_both_ways_is_refused() {
        let e = arrays("BEGIN {x[1] = 1; y = x}").unwrap_err();
        assert!(e.contains('x'), "{e}");
        assert!(e.contains("both"), "{e}");
    }

    #[test]
    fn the_conflict_is_found_across_a_call_boundary() {
        // `g` is an array here and a scalar in `f`'s body; neither end alone
        // knows that, which is what makes the fixed point necessary — and what
        // makes naming only one of them an unhelpful diagnostic.
        let e = arrays("function f(p) {return p + 1} BEGIN {g[1] = 1; print f(g)}").unwrap_err();
        assert_eq!(e, "g is an array, but it is passed to f as p, which is a scalar");
    }

    #[test]
    fn a_parameter_used_both_ways_names_its_function() {
        let e = arrays("function f(p) {p[1] = 1; p = 2} BEGIN {f(q)}").unwrap_err();
        assert_eq!(e, "f's parameter p is used both as an array and as a scalar");
    }

    #[test]
    fn length_of_an_array_is_not_a_scalar_use() {
        // Otherwise the commonest way to count what `split` produced would be
        // rejected as a conflict.
        let a = arrays("BEGIN {n = split(\"a:b\", p, \":\"); print length(p)}").unwrap();
        assert!(a.contains(&"p".to_string()), "{a:?}");
    }

    #[test]
    fn a_name_used_only_one_way_is_left_alone() {
        let a = arrays("BEGIN {s = \"x\"; t = s s}").unwrap();
        assert!(!a.contains(&"s".to_string()), "{a:?}");
        assert!(!a.contains(&"t".to_string()), "{a:?}");
    }

    #[test]
    fn a_parameter_shadowing_a_global_does_not_type_it() {
        // `a` the parameter is an array; `a` the global is a scalar. They are
        // different variables, so this is not a conflict.
        let a = arrays("function f(a) {a[1] = 1} BEGIN {a = 3; f(b)}").unwrap();
        assert!(a.contains(&"b".to_string()), "{a:?}");
        assert!(!a.contains(&"a".to_string()), "{a:?}");
    }

    #[test]
    fn the_two_builtin_arrays_are_arrays_without_being_used() {
        let a = arrays("BEGIN {print 1}").unwrap();
        assert!(a.contains(&"ENVIRON".to_string()), "{a:?}");
        assert!(a.contains(&"ARGV".to_string()), "{a:?}");
    }

    #[test]
    fn assigning_a_builtin_array_to_a_scalar_is_the_conflict() {
        let e = arrays("BEGIN {x = ARGV}").unwrap_err();
        assert!(e.contains("ARGV"), "{e}");
    }

    #[test]
    fn a_recursive_function_reaches_a_fixed_point() {
        // A self-link would loop forever without the "only false to true" rule.
        let a = arrays("function r(a, i) {if (i > 2) return; a[i] = i; r(a, i + 1)} BEGIN {r(q, 0)}")
            .unwrap();
        assert!(a.contains(&"q".to_string()), "{a:?}");
    }
}
