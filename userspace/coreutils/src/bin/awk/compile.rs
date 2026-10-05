//! The compiler: a parsed program to instructions for `interp`'s loop.
//!
//! ## Why awk is compiled at all
//!
//! The interpreter used to walk the tree, and a tree walk recurses once per
//! awk function call on the native stack: a recursive function 3000 calls
//! deep overflowed it and the process died (exit 134), where gawk runs 30000
//! without noticing. The usual cure -- a thread with an enormous stack -- is
//! the wrong one on SlateOS, which commits anonymous memory when it is mapped
//! (`MAP_LAZY` is opt-in), so a 1 GiB stack would cost 1 GiB up front. gawk's
//! own answer is this one: instructions run by a loop whose call frames are on
//! the heap, so the depth of awk recursion is bounded by memory, not by the
//! stack the process happened to be given.
//!
//! ## Why the instructions are gawk's, line numbers and all
//!
//! gawk places a diagnostic on the line of the *instruction* that raised it,
//! and every instruction it executes updates that line (`interpret.h`: `if
//! (pc->source_line > 0) sourceline = pc->source_line`). An instruction takes
//! its line from the token gawk made it from -- the `/` of a division, the `~`
//! of a match, the name of a call, the `print` of a print -- and runs after its
//! operands. So `if (1 &&\n 1/z)` fails on line 2's `/`, and a file that fails
//! to open after a rule has run is placed on the last line that rule executed.
//! [`Instr::loc`] is that token's line, `None` for an instruction gawk makes
//! without one (a jump, a pop), and the order of instructions is gawk's: a
//! print's redirection before its arguments, an assignment's right side before
//! its target, a `getline` target before the read.

use crate::ast::{
    BinOp, Builtin, CmpOp, Expr, ExprKind, GetlineSrc, Loc, Lvalue, Pattern, Program, RedirMode,
    Stmt, V_FNR, V_NF, V_NR, VarRef,
};
use crate::value::Str;
use ere::Regex;
use std::rc::Rc;

/// One instruction, and the line of the token it was made from.
#[derive(Clone, Debug)]
pub struct Instr {
    pub op: Op,
    pub loc: Option<Loc>,
}

/// What an instruction does. Stack effects are written `[before] -> [after]`,
/// top of stack last.
#[derive(Clone, Debug)]
pub enum Op {
    // ---- values ---------------------------------------------------------
    /// `[] -> [n]`
    Num(f64),
    /// `[] -> [s]`
    Str(Rc<Str>),
    /// `[] -> [re]`: a regex literal as an operand of `~`, `match`, `split`,
    /// `sub` or `gsub`, which take it as the pattern itself.
    Re(Rc<Regex>),
    /// `[] -> [$0 ~ re]`: a regex literal anywhere else.
    MatchRecord(Rc<Regex>),
    /// `[] -> [v]`
    Var(VarRef),
    /// `[] -> [array]`: an array by reference, for a call or `length`.
    Arr(VarRef),
    /// `[subs...] -> [a[subs]]`, creating the element, as reading one does.
    Elem(VarRef, usize),
    /// `[n] -> [$n]`
    Field,

    // ---- assignment -----------------------------------------------------
    /// `[] -> [place]`
    PlaceVar(VarRef),
    /// `[subs...] -> [place]`
    PlaceElem(VarRef, usize),
    /// `[n] -> [place]`, refusing a negative field number.
    PlaceField,
    /// `[v place] -> [v]`
    Assign,
    /// `[r place] -> [place op r]`
    AugAssign(BinOp),
    /// `[place] -> [place + d]`, `++x` and `--x`.
    PreIncr(f64),
    /// `[place] -> [old]`, `x++` and `x--`.
    PostIncr(f64),

    // ---- operators ------------------------------------------------------
    /// `[l r] -> [l op r]`
    Arith(BinOp),
    /// `[v] -> [-v]`
    Neg,
    /// `[v] -> [+v]`, a numeric reading.
    Pos,
    /// `[v] -> [!v]`
    Not,
    /// `[l r] -> [l op r]`
    Cmp(CmpOp),
    /// `[l r] -> [l r]`
    Concat,
    /// `[s re-or-text] -> [s ~ re]`, negated for `!~`.
    Match(bool),
    /// `[subs...] -> [(subs) in a]`
    In(VarRef, usize),
    /// `[v] -> [0 or 1]`: the value of a `&&` or `||` that did not short-cut.
    Bool,

    // ---- control --------------------------------------------------------
    Jump(usize),
    /// `[c] -> []`, jumping when `c` is false.
    JumpFalse(usize),
    /// `[c] -> []`, jumping when `c` is true.
    JumpTrue(usize),
    /// `&&`: `[c] -> [0]` and jump when `c` is false, else `[c] -> []`.
    AndJump(usize),
    /// `||`: `[c] -> [1]` and jump when `c` is true, else `[c] -> []`.
    OrJump(usize),
    /// `[v] -> []`
    Pop,

    // ---- functions ------------------------------------------------------
    /// `[args...] -> [result]` once the callee returns.
    Call(usize, usize),
    /// `[v]` (if the flag) `-> ` the caller's `[v]`.
    Return(bool),

    // ---- built-ins ------------------------------------------------------
    /// `[args...] -> [result]`, for a built-in whose arguments are values.
    Builtin(Builtin, usize),
    /// `[] -> [length(a)]`
    LengthArr(VarRef),
    /// `[s fs-or-nothing] -> [n]`, into the array; the flag says whether the
    /// separator was given.
    Split(VarRef, bool),
    /// `[re repl place] -> [n]`; global for `gsub`.
    Sub(bool),
    /// `[s re] -> [RSTART]`
    MatchFn,
    /// `[fmt args...] -> [s]`
    Sprintf(usize),

    // ---- input and output -----------------------------------------------
    /// `[place?] -> [1|0|-1]`, plain `getline`; the flag says there is a target.
    GetlineMain(bool),
    /// `[file place?] -> [1|0|-1]`
    GetlineFile(bool),
    /// `[cmd place?] -> [1|0|-1]`
    GetlineCmd(bool),
    /// `[target? args...] -> []`; no arguments prints `$0`.
    Print(usize, Option<RedirMode>),
    /// `[target? fmt args...] -> []`
    Printf(usize, Option<RedirMode>),

    // ---- statements -----------------------------------------------------
    /// `[subs...] -> []`
    Delete(VarRef, usize),
    DeleteAll(VarRef),
    Next,
    NextFile,
    /// `[status?] -> `, the flag saying whether one was given.
    Exit(bool),
    /// Take the list of an array's indices for `for (k in a)`, in gawk's
    /// order, as gawk's `Op_arrayfor_init` does.
    ForInStart(VarRef),
    /// Assign the list's next index to the variable -- deleted since or not,
    /// as gawk's `Op_arrayfor_incr` does -- or jump to the target (where
    /// `ForInPop` is) when the list is done.
    ForInNext(VarRef, usize),
    ForInPop,
    /// `for (k in a) delete a[k]`, which gawk's parser turns into one
    /// instruction (`Op_K_delete_loop`): `k` gets the index the loop would
    /// have visited first, and the array is emptied at a stroke. The array,
    /// then the variable.
    DeleteLoop(VarRef, VarRef),
    /// A range pattern: jump to the target if range `id` is open.
    RangeOpen(usize, usize),
    /// `[end] -> []`: having just matched its start, open the range unless
    /// its end matched too.
    RangeStarted(usize),
    /// `[end] -> []`: close the range if its end matched.
    RangeEnded(usize),
    /// The end of a block of code: BEGIN, the rules, or END.
    Halt,
}

/// A compiled function.
#[derive(Clone, Debug)]
pub struct FuncCode {
    pub entry: usize,
    pub params: usize,
}

/// A whole program's instructions, and where each part begins.
#[derive(Debug, Default)]
pub struct Code {
    pub ops: Vec<Instr>,
    pub begin: usize,
    /// The rules, evaluated in order for each record.
    pub rules: usize,
    pub end: usize,
    pub funcs: Vec<FuncCode>,
}

/// Where `break` and `continue` go inside the innermost loop.
struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

struct Compiler<'p> {
    prog: &'p Program,
    code: Code,
    loops: Vec<Loop>,
    /// The function being compiled, for its parameters' array-ness.
    func: Option<usize>,
}

/// Whether `for (var in array) body` is the loop gawk's parser rewrites into
/// one `delete` (awkgram.y, the `LEX_FOR '(' NAME LEX_IN` rule), and if so
/// where the `delete` was written.
///
/// gawk recognises it by the instructions the body compiled to: exactly one
/// `delete` of one subscript, that subscript a bare reference to the loop's
/// variable -- not parenthesised, which adds an instruction -- and the array
/// the loop's. Braces around it do not count, nor do empty `;` statements;
/// an empty `{}` does (it compiles to a no-op). The variable may not be one
/// gawk refreshes before reading (`NR`, `FNR`, `NF`).
fn delete_loop(var: VarRef, array: VarRef, body: &Stmt) -> Option<DeleteAt> {
    if matches!(var, VarRef::Global(V_NR | V_FNR | V_NF)) {
        return None;
    }
    let mut at = None;
    let mut s = body;
    loop {
        match s {
            Stmt::At(loc, inner) => {
                at = Some(*loc);
                s = inner;
            }
            Stmt::Block(list) if list.len() == 1 => s = list.first()?,
            Stmt::Delete {
                array: a,
                subs,
                bare: Some(v),
            } if *a == array && *v == var && subs.len() == 1 => return Some(DeleteAt(at)),
            _ => return None,
        }
    }
}

/// Where the `delete` of a loop [`delete_loop`] recognised was written, when
/// the parser recorded it: the line gawk's one instruction carries.
struct DeleteAt(Option<Loc>);

/// Compile `prog`, which [`crate::types::resolve`] has already typed.
#[must_use]
pub fn compile(prog: &Program) -> Code {
    let mut c = Compiler {
        prog,
        code: Code::default(),
        loops: Vec::new(),
        func: None,
    };

    c.code.begin = c.here();
    for s in &prog.begin {
        c.stmt(s, None);
    }
    c.emit(Op::Halt, None);

    c.code.rules = c.here();
    for rule in &prog.rules {
        c.rule(&rule.pattern, rule.action.as_deref());
    }
    c.emit(Op::Halt, None);

    c.code.end = c.here();
    for s in &prog.end {
        c.stmt(s, None);
    }
    c.emit(Op::Halt, None);

    for (i, f) in prog.funcs.iter().enumerate() {
        c.func = Some(i);
        let entry = c.here();
        for s in &f.body {
            c.stmt(s, None);
        }
        // Falling off the end returns the empty value.
        c.emit(Op::Return(false), None);
        c.code.funcs.push(FuncCode {
            entry,
            params: f.params.len(),
        });
    }
    c.code
}

impl Compiler<'_> {
    fn here(&self) -> usize {
        self.code.ops.len()
    }

    fn emit(&mut self, op: Op, loc: Option<Loc>) -> usize {
        self.code.ops.push(Instr { op, loc });
        self.code.ops.len().saturating_sub(1)
    }

    /// Point the jump at `at` to `target`.
    fn patch(&mut self, at: usize, target: usize) {
        if let Some(instr) = self.code.ops.get_mut(at) {
            match &mut instr.op {
                Op::Jump(t)
                | Op::JumpFalse(t)
                | Op::JumpTrue(t)
                | Op::AndJump(t)
                | Op::OrJump(t)
                | Op::ForInNext(_, t)
                | Op::RangeOpen(_, t) => *t = target,
                _ => {}
            }
        }
    }

    /// Whether `v` holds an array where this code runs.
    fn is_array(&self, v: VarRef) -> bool {
        match v {
            VarRef::Global(s) => self.prog.global_is_array.get(s).copied().unwrap_or(false),
            VarRef::Local(s) => self
                .func
                .and_then(|f| self.prog.param_is_array.get(f))
                .and_then(|p| p.get(s))
                .copied()
                .unwrap_or(false),
        }
    }

    // ---- rules ----------------------------------------------------------

    fn rule(&mut self, pattern: &Pattern, action: Option<&[Stmt]>) {
        let mut skip = Vec::new();
        match pattern {
            Pattern::Always => {}
            Pattern::Expr(e) => {
                self.expr(e);
                skip.push(self.emit(Op::JumpFalse(0), None));
            }
            Pattern::Range(a, b, id) => {
                let open = self.emit(Op::RangeOpen(*id, 0), None);
                self.expr(a);
                skip.push(self.emit(Op::JumpFalse(0), None));
                self.expr(b);
                self.emit(Op::RangeStarted(*id), None);
                let to_action = self.emit(Op::Jump(0), None);
                let here = self.here();
                self.patch(open, here);
                self.expr(b);
                self.emit(Op::RangeEnded(*id), None);
                let here = self.here();
                self.patch(to_action, here);
            }
        }
        match action {
            Some(body) => {
                for s in body {
                    self.stmt(s, None);
                }
            }
            // The default action is `print`, which gawk makes without a line.
            None => {
                self.emit(Op::Print(0, None), None);
            }
        }
        let here = self.here();
        for at in skip {
            self.patch(at, here);
        }
    }

    // ---- statements -----------------------------------------------------

    /// Compile a statement. `at` is where it was written -- the statement's
    /// first token -- which is the line of the instruction a keyword statement
    /// makes from its keyword (`print`, `next`, `delete`...).
    fn stmt(&mut self, s: &Stmt, at: Option<Loc>) {
        match s {
            Stmt::At(loc, inner) => self.stmt(inner, Some(*loc)),
            Stmt::Expr(e) => {
                self.expr(e);
                self.emit(Op::Pop, None);
            }
            Stmt::Print(args, redirect) | Stmt::Printf(args, redirect) => {
                // gawk evaluates the redirection first, then the list.
                if let Some(r) = redirect {
                    self.expr(&r.target);
                }
                for a in args {
                    self.expr(a);
                }
                let mode = redirect.as_ref().map(|r| r.mode);
                let op = if matches!(s, Stmt::Printf(..)) {
                    Op::Printf(args.len(), mode)
                } else {
                    Op::Print(args.len(), mode)
                };
                self.emit(op, at);
            }
            Stmt::Block(body) => {
                for s in body {
                    self.stmt(s, None);
                }
            }
            Stmt::If(cond, then, other) => {
                self.expr(cond);
                let to_else = self.emit(Op::JumpFalse(0), None);
                self.stmt(then, None);
                match other {
                    Some(other) => {
                        let to_end = self.emit(Op::Jump(0), None);
                        let here = self.here();
                        self.patch(to_else, here);
                        self.stmt(other, None);
                        let here = self.here();
                        self.patch(to_end, here);
                    }
                    None => {
                        let here = self.here();
                        self.patch(to_else, here);
                    }
                }
            }
            Stmt::While(cond, body) => {
                let top = self.here();
                self.expr(cond);
                let to_end = self.emit(Op::JumpFalse(0), None);
                self.loop_body(body);
                self.emit(Op::Jump(top), None);
                let end = self.here();
                self.patch(to_end, end);
                self.close_loop(top, end);
            }
            Stmt::DoWhile(body, cond) => {
                let top = self.here();
                self.loop_body(body);
                let cont = self.here();
                self.expr(cond);
                self.emit(Op::JumpTrue(top), None);
                let end = self.here();
                self.close_loop(cont, end);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt(init, None);
                }
                let top = self.here();
                let to_end = cond.as_ref().map(|c| {
                    self.expr(c);
                    self.emit(Op::JumpFalse(0), None)
                });
                self.loop_body(body);
                let cont = self.here();
                if let Some(step) = step {
                    self.stmt(step, None);
                }
                self.emit(Op::Jump(top), None);
                let end = self.here();
                if let Some(at) = to_end {
                    self.patch(at, end);
                }
                self.close_loop(cont, end);
            }
            Stmt::ForIn { var, array, body } => {
                if let Some(DeleteAt(delete_at)) = delete_loop(*var, *array, body) {
                    self.emit(Op::DeleteLoop(*array, *var), delete_at.or(at));
                    return;
                }
                self.emit(Op::ForInStart(*array), at);
                let top = self.here();
                let next = self.emit(Op::ForInNext(*var, 0), at);
                self.loop_body(body);
                self.emit(Op::Jump(top), None);
                let exit = self.emit(Op::ForInPop, None);
                self.patch(next, exit);
                // `break` leaves through the pop; `continue` takes the next key.
                self.close_loop(top, exit);
            }
            Stmt::Next => {
                self.emit(Op::Next, at);
            }
            Stmt::NextFile => {
                self.emit(Op::NextFile, at);
            }
            Stmt::Exit(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
                self.emit(Op::Exit(e.is_some()), at);
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
                self.emit(Op::Return(e.is_some()), at);
            }
            Stmt::Break => {
                let j = self.emit(Op::Jump(0), at);
                if let Some(l) = self.loops.last_mut() {
                    l.breaks.push(j);
                }
            }
            Stmt::Continue => {
                let j = self.emit(Op::Jump(0), at);
                if let Some(l) = self.loops.last_mut() {
                    l.continues.push(j);
                }
            }
            Stmt::Delete {
                array: arr, subs, ..
            } => {
                if subs.is_empty() {
                    self.emit(Op::DeleteAll(*arr), at);
                } else {
                    for s in subs {
                        self.expr(s);
                    }
                    self.emit(Op::Delete(*arr, subs.len()), at);
                }
            }
            Stmt::Nop => {}
        }
    }

    /// A loop's body, with a fresh target list for its `break`s and
    /// `continue`s.
    fn loop_body(&mut self, body: &Stmt) {
        self.loops.push(Loop {
            breaks: Vec::new(),
            continues: Vec::new(),
        });
        self.stmt(body, None);
    }

    fn close_loop(&mut self, cont: usize, end: usize) {
        if let Some(l) = self.loops.pop() {
            for at in l.continues {
                self.patch(at, cont);
            }
            for at in l.breaks {
                self.patch(at, end);
            }
        }
    }

    // ---- expressions ----------------------------------------------------

    /// Compile `e`, leaving its value on the stack.
    fn expr(&mut self, e: &Expr) {
        let loc = Some(e.loc);
        match &e.kind {
            ExprKind::Num(n) => {
                self.emit(Op::Num(*n), loc);
            }
            ExprKind::Str(s) => {
                self.emit(Op::Str(Rc::clone(s)), loc);
            }
            ExprKind::Regex(re) => {
                self.emit(Op::MatchRecord(Rc::clone(re)), loc);
            }
            ExprKind::Get(lv) => match lv {
                Lvalue::Var(v) => {
                    self.emit(Op::Var(*v), loc);
                }
                Lvalue::Index(v, subs) => {
                    for s in subs {
                        self.expr(s);
                    }
                    self.emit(Op::Elem(*v, subs.len()), loc);
                }
                Lvalue::Field(n, at) => {
                    self.expr(n);
                    self.emit(Op::Field, Some(*at));
                }
            },
            ExprKind::Assign(lv, rhs) => {
                self.expr(rhs);
                self.place(lv);
                self.emit(Op::Assign, loc);
            }
            ExprKind::AugAssign(lv, op, rhs) => {
                self.expr(rhs);
                self.place(lv);
                self.emit(Op::AugAssign(*op), loc);
            }
            ExprKind::Cond(c, a, b) => {
                self.expr(c);
                // gawk frees the `?` and `:` tokens' instructions
                // (`mk_condition`), so the jumps carry no line.
                let to_else = self.emit(Op::JumpFalse(0), None);
                self.expr(a);
                let to_end = self.emit(Op::Jump(0), None);
                let here = self.here();
                self.patch(to_else, here);
                self.expr(b);
                let here = self.here();
                self.patch(to_end, here);
            }
            ExprKind::And(a, b) => {
                self.expr(a);
                let short = self.emit(Op::AndJump(0), loc);
                self.expr(b);
                self.emit(Op::Bool, None);
                let here = self.here();
                self.patch(short, here);
            }
            ExprKind::Or(a, b) => {
                self.expr(a);
                let short = self.emit(Op::OrJump(0), loc);
                self.expr(b);
                self.emit(Op::Bool, None);
                let here = self.here();
                self.patch(short, here);
            }
            ExprKind::In(subs, arr) => {
                for s in subs {
                    self.expr(s);
                }
                self.emit(Op::In(*arr, subs.len()), loc);
            }
            ExprKind::Match { neg, lhs, rhs } => {
                self.expr(lhs);
                self.regex_operand(rhs);
                self.emit(Op::Match(*neg), loc);
            }
            ExprKind::Cmp(op, a, b) => {
                self.expr(a);
                self.expr(b);
                self.emit(Op::Cmp(*op), loc);
            }
            ExprKind::Concat(a, b) => {
                self.expr(a);
                self.expr(b);
                // A concatenation has no token of its own, and gawk makes its
                // instruction without a line.
                self.emit(Op::Concat, None);
            }
            ExprKind::Bin(op, a, b) => {
                self.expr(a);
                self.expr(b);
                self.emit(Op::Arith(*op), loc);
            }
            ExprKind::Neg(a) => {
                self.expr(a);
                self.emit(Op::Neg, loc);
            }
            ExprKind::Pos(a) => {
                self.expr(a);
                self.emit(Op::Pos, loc);
            }
            ExprKind::Not(a) => {
                self.expr(a);
                self.emit(Op::Not, loc);
            }
            ExprKind::PreIncr(lv, d) => {
                self.place(lv);
                self.emit(Op::PreIncr(*d), loc);
            }
            ExprKind::PostIncr(lv, d) => {
                self.place(lv);
                self.emit(Op::PostIncr(*d), loc);
            }
            ExprKind::Call(f, args) => {
                let wants: Vec<bool> = self
                    .prog
                    .param_is_array
                    .get(*f)
                    .cloned()
                    .unwrap_or_default();
                for (i, a) in args.iter().enumerate() {
                    // An array goes by reference: a bare name the callee uses
                    // as an array, or -- past the declared parameters, where
                    // gawk evaluates an argument and drops it -- one that is
                    // an array here.
                    let by_ref = match &a.kind {
                        ExprKind::Get(Lvalue::Var(v)) => match wants.get(i) {
                            Some(w) => *w,
                            None => self.is_array(*v),
                        },
                        _ => false,
                    };
                    match (&a.kind, by_ref) {
                        (ExprKind::Get(Lvalue::Var(v)), true) => {
                            self.emit(Op::Arr(*v), Some(a.loc));
                        }
                        _ => self.expr(a),
                    }
                }
                self.emit(Op::Call(*f, args.len()), loc);
            }
            ExprKind::Builtin(b, args) => self.builtin(*b, args, loc),
            ExprKind::Getline(g) => {
                match &g.src {
                    GetlineSrc::Main => {}
                    GetlineSrc::File(x) | GetlineSrc::Cmd(x) => self.expr(x),
                }
                // gawk evaluates the target before it reads.
                if let Some(lv) = &g.into {
                    self.place(lv);
                }
                let into = g.into.is_some();
                let op = match &g.src {
                    GetlineSrc::Main => Op::GetlineMain(into),
                    GetlineSrc::File(_) => Op::GetlineFile(into),
                    GetlineSrc::Cmd(_) => Op::GetlineCmd(into),
                };
                self.emit(op, loc);
            }
        }
    }

    /// Something to assign to, left on the stack as a place.
    fn place(&mut self, lv: &Lvalue) {
        match lv {
            Lvalue::Var(v) => {
                self.emit(Op::PlaceVar(*v), None);
            }
            Lvalue::Index(v, subs) => {
                for s in subs {
                    self.expr(s);
                }
                self.emit(Op::PlaceElem(*v, subs.len()), None);
            }
            Lvalue::Field(n, at) => {
                self.expr(n);
                self.emit(Op::PlaceField, Some(*at));
            }
        }
    }

    /// The pattern operand of `~`, `match`, `split`, `sub` and `gsub`: a
    /// regex literal is the pattern itself, anything else is text to compile.
    fn regex_operand(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Regex(re) => {
                self.emit(Op::Re(Rc::clone(re)), Some(e.loc));
            }
            _ => self.expr(e),
        }
    }

    fn builtin(&mut self, b: Builtin, args: &[Expr], loc: Option<Loc>) {
        match b {
            Builtin::Length => match args.first() {
                // `length` alone is the record's.
                None => {
                    self.emit(Op::Num(0.0), loc);
                    self.emit(Op::Field, loc);
                    self.emit(Op::Builtin(Builtin::Length, 1), loc);
                }
                Some(Expr {
                    kind: ExprKind::Get(Lvalue::Var(v)),
                    ..
                }) if self.is_array(*v) => {
                    self.emit(Op::LengthArr(*v), loc);
                }
                Some(a) => {
                    self.expr(a);
                    self.emit(Op::Builtin(Builtin::Length, 1), loc);
                }
            },
            Builtin::Split => {
                if let Some(s) = args.first() {
                    self.expr(s);
                }
                let arr = match args.get(1).map(|a| &a.kind) {
                    Some(ExprKind::Get(Lvalue::Var(v))) => *v,
                    // The parser refuses anything else.
                    _ => VarRef::Global(0),
                };
                let sep = args.get(2);
                if let Some(sep) = sep {
                    self.regex_operand(sep);
                }
                self.emit(Op::Split(arr, sep.is_some()), loc);
            }
            Builtin::Sub | Builtin::Gsub => {
                if let Some(re) = args.first() {
                    self.regex_operand(re);
                }
                if let Some(repl) = args.get(1) {
                    self.expr(repl);
                }
                match args.get(2).map(|a| &a.kind) {
                    Some(ExprKind::Get(lv)) => self.place(lv),
                    // No target is `$0`.
                    _ => {
                        self.emit(Op::Num(0.0), loc);
                        self.emit(Op::PlaceField, loc);
                    }
                }
                self.emit(Op::Sub(b == Builtin::Gsub), loc);
            }
            Builtin::Match => {
                if let Some(s) = args.first() {
                    self.expr(s);
                }
                if let Some(re) = args.get(1) {
                    self.regex_operand(re);
                }
                self.emit(Op::MatchFn, loc);
            }
            Builtin::Sprintf => {
                for a in args {
                    self.expr(a);
                }
                self.emit(Op::Sprintf(args.len()), loc);
            }
            _ => {
                for a in args {
                    self.expr(a);
                }
                self.emit(Op::Builtin(b, args.len()), loc);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::source::SourceMap;

    fn code(src: &str) -> Code {
        let mut prog = crate::parse::parse(
            src.as_bytes(),
            &SourceMap::operand(src.as_bytes()),
            &mut ere::awk::Warnings::default(),
            &mut Vec::new(),
        )
        .unwrap_or_else(|e| panic!("{src:?}: {e}"));
        crate::types::resolve(&mut prog).unwrap();
        compile(&prog)
    }

    /// The operations of the BEGIN block, without their lines.
    fn begin_ops(src: &str) -> Vec<String> {
        let c = code(src);
        c.ops[c.begin..c.rules]
            .iter()
            .map(|i| format!("{:?}", i.op))
            .collect()
    }

    /// gawk's order for a print: the redirection, then the list, then the
    /// print itself.
    #[test]
    fn a_print_evaluates_its_redirection_first() {
        let ops = begin_ops(r#"BEGIN { print 1, 2 > "f" }"#);
        assert_eq!(
            ops,
            [
                "Str([102])",
                "Num(1.0)",
                "Num(2.0)",
                "Print(2, Some(Truncate))",
                "Halt"
            ]
        );
    }

    /// An assignment evaluates its right side, then its target.
    #[test]
    fn an_assignment_evaluates_its_right_side_first() {
        let ops = begin_ops("BEGIN { a[i++] = i }");
        // `i`, then the target's subscript `i++`, then the store.
        assert!(
            ops.first().is_some_and(|o| o.starts_with("Var(")),
            "{ops:?}"
        );
        let incr = ops.iter().position(|o| o.starts_with("PostIncr")).unwrap();
        let place = ops.iter().position(|o| o.starts_with("PlaceElem")).unwrap();
        let assign = ops.iter().position(|o| o == "Assign").unwrap();
        assert!(0 < incr && incr < place && place < assign, "{ops:?}");
    }

    /// The target of `getline` is evaluated before the read.
    #[test]
    fn a_getline_target_is_evaluated_before_the_read() {
        let ops = begin_ops(r#"BEGIN { getline a[i++] < "f" }"#);
        let target = ops.iter().position(|o| o.starts_with("PlaceElem")).unwrap();
        let read = ops
            .iter()
            .position(|o| o.starts_with("GetlineFile"))
            .unwrap();
        assert!(target < read, "{ops:?}");
    }

    /// Each instruction carries the line of its token.
    #[test]
    fn an_operator_carries_its_own_line() {
        let c = code("BEGIN {\n if (1 &&\n  1/z) print }");
        let div = c
            .ops
            .iter()
            .find(|i| matches!(i.op, Op::Arith(BinOp::Div)))
            .unwrap();
        assert_eq!(div.loc.map(|l| l.line), Some(3));
        let and = c
            .ops
            .iter()
            .find(|i| matches!(i.op, Op::AndJump(_)))
            .unwrap();
        assert_eq!(and.loc.map(|l| l.line), Some(2));
        // A jump is made without a line.
        let jf = c
            .ops
            .iter()
            .find(|i| matches!(i.op, Op::JumpFalse(_)))
            .unwrap();
        assert_eq!(jf.loc, None);
    }

    /// An array argument goes by reference; past the declared parameters an
    /// array still goes as an array, to be dropped.
    #[test]
    fn an_array_argument_goes_by_reference() {
        let c = code("function f(a) { a[1] = 2 } BEGIN { f(b); g(1, b) } function g(x) { }");
        let ops: Vec<String> = c.ops.iter().map(|i| format!("{:?}", i.op)).collect();
        assert!(
            ops.iter().filter(|o| o.starts_with("Arr(")).count() == 2,
            "{ops:?}"
        );
    }
}
