//! The loop that runs compiled instructions.
//!
//! [`crate::compile`] turns the program into one vector of [`Instr`]s, and this
//! runs a stretch of it -- BEGIN, the rules for one record, END -- to its
//! `Halt`. A call pushes a [`Frame`] and jumps; a return pops one and jumps
//! back. Nothing here recurses per awk call, so a recursion is as deep as
//! memory allows, as gawk's is, rather than as deep as the native stack.
//!
//! Before each instruction, its line becomes the interpreter's line
//! ([`Interp::loc`]) -- gawk's `sourceline` -- which is what every diagnostic
//! raised while it runs names.

use super::null_redirect;
use super::{Array, Cell, Fatal, Flow, Fs, Interp, MainRead, R, count_chars, expand_ampersand};
use super::{index_of, new_array, seconds_since_epoch, split_with};
use crate::ast::{BinOp, Builtin, RedirMode, V_NR, V_RLENGTH, V_RSTART, V_SUBSEP, VarRef};
use crate::compile::{Code, Instr, Op};
use crate::value::{Str, Value, c_long, calc_exp, compare};
use ere::{Regex, ch};
use std::rc::Rc;

/// What the instructions work on.
pub(super) enum Item {
    Val(Value),
    /// An array, passed by reference.
    Arr(Array),
    /// Something about to be assigned to.
    Place(Place),
    /// A regex literal given as a pattern operand.
    Re(Rc<Regex>),
}

/// An lvalue, resolved once: its subscripts or field number already
/// evaluated. Reading and then writing through it -- `a[i++] += 1` -- touches
/// one element, which a second evaluation of the subscript would not. An
/// element's subscript is kept as the value it was, which finds the same
/// element every time it is looked up.
pub(super) enum Place {
    Var(VarRef),
    Field(usize),
    Elem(Array, Value),
}

/// A call in progress.
pub(super) struct Frame {
    pub(super) locals: Vec<Cell>,
    /// Where to go on return.
    ret: usize,
    /// The stack and the `for`-loop iterators as they were at the call,
    /// restored on the way out however the function ends.
    stack_base: usize,
    iter_base: usize,
}

/// A `for (k in a)` loop in progress: the indices as they were when it
/// began. The body may delete from the array, and gawk visits a deleted index
/// all the same -- the list is the loop's, not the array's.
pub(super) struct ForIter {
    keys: Vec<Value>,
    next: usize,
}

/// Which code is running, for `next` and `nextfile`, which gawk allows only
/// while a record is being processed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ctx {
    Begin,
    Rule,
    End,
}

impl Ctx {
    fn name(self) -> &'static str {
        match self {
            Ctx::Begin => "BEGIN",
            Ctx::Rule => "Rule",
            Ctx::End => "END",
        }
    }
}

/// An instruction stream that broke its own invariants: the compiler emitted
/// something the loop cannot run. It is a bug here, never the program's.
fn internal(what: &str) -> Fatal {
    Fatal::Said(format!("fatal: internal error: {what}"))
}

impl Interp {
    /// Run the code from `entry` to its `Halt`, or until control leaves it.
    ///
    /// Whatever happens -- a fatal, or `next` or `exit` from inside a function
    /// -- the stack, the frames and the loop iterators are put back as they
    /// were, so the next record starts clean.
    pub(super) fn run_code(&mut self, entry: usize, ctx: Ctx) -> R<Flow> {
        let code = Rc::clone(&self.code);
        let base = (self.stack.len(), self.frames.len(), self.iters.len());
        let flow = self.exec(&code, entry, ctx);
        self.stack.truncate(base.0);
        self.frames.truncate(base.1);
        self.iters.truncate(base.2);
        flow
    }

    #[allow(clippy::too_many_lines)]
    fn exec(&mut self, code: &Code, entry: usize, ctx: Ctx) -> R<Flow> {
        let mut pc = entry;
        loop {
            let Some(Instr { op, loc }) = code.ops.get(pc) else {
                return Err(internal("ran off the end of the code"));
            };
            if let Some(l) = loc {
                self.loc = Some(*l);
            }
            pc = pc.saturating_add(1);
            match op {
                Op::Num(n) => self.push(Value::Num(*n)),
                Op::Str(s) => self.push(Value::Str(Rc::clone(s))),
                Op::Re(re) => self.stack.push(Item::Re(Rc::clone(re))),
                Op::MatchRecord(re) => {
                    let rec = self.record().clone();
                    let m = re.is_match(&rec)?;
                    self.push(Value::Num(f64::from(u8::from(m))));
                }
                Op::Var(v) => {
                    let val = self.get_var(*v)?;
                    self.push(val);
                }
                Op::Arr(v) => {
                    let a = self.array_of(*v)?;
                    self.stack.push(Item::Arr(a));
                }
                Op::Elem(v, n) => {
                    let subs = self.pop_subscript(*n)?;
                    let a = self.array_of(*v)?;
                    let convfmt = self.convfmt();
                    // gawk's `Op_subscript` takes the subscript's text when the
                    // element is not there yet -- for its diagnostics -- which
                    // settles an index from `for (k in a)` into a string.
                    if a.borrow().get(&subs, &convfmt).is_none() {
                        let _ = subs.to_str(&convfmt);
                    }
                    // Referring to `a[k]` *creates* it, which is why `if (a[k]
                    // == "") ...` makes `k in a` true afterwards. Every awk does
                    // this and programs test for it.
                    let val = a.borrow_mut().lookup(&subs, &convfmt).clone();
                    self.push(val);
                }
                Op::Field => {
                    let n = self.pop_val()?;
                    let n = self.field_number(&n)?;
                    let val = self.get_field(n)?;
                    self.push(val);
                }

                Op::PlaceVar(v) => self.stack.push(Item::Place(Place::Var(*v))),
                Op::PlaceElem(v, n) => {
                    let subs = self.pop_subscript(*n)?;
                    let a = self.array_of(*v)?;
                    // The element is made here, as gawk's `Op_subscript_lhs`
                    // makes it, so `getline a[k]` at end of file and a `sub`
                    // that matches nothing still leave `k in a` true.
                    let convfmt = self.convfmt();
                    let _ = a.borrow_mut().lookup(&subs, &convfmt);
                    self.stack.push(Item::Place(Place::Elem(a, subs)));
                }
                Op::PlaceField => {
                    let n = self.pop_val()?;
                    let n = self.field_number(&n)?;
                    self.stack.push(Item::Place(Place::Field(n)));
                }
                Op::Assign => {
                    let place = self.pop_place()?;
                    let val = self.pop_val()?;
                    self.store(&place, val.clone())?;
                    self.push(val);
                }
                Op::AugAssign(op) => {
                    let place = self.pop_place()?;
                    let r = self.pop_val()?.to_num();
                    let l = self.load(&place)?.to_num();
                    let val = Value::Num(arith_assign(*op, l, r)?);
                    self.store(&place, val.clone())?;
                    self.push(val);
                }
                Op::PreIncr(d) => {
                    let place = self.pop_place()?;
                    let v = self.load(&place)?.to_num() + d;
                    self.store(&place, Value::Num(v))?;
                    self.push(Value::Num(v));
                }
                Op::PostIncr(d) => {
                    let place = self.pop_place()?;
                    let old = self.load(&place)?.to_num();
                    self.store(&place, Value::Num(old + d))?;
                    self.push(Value::Num(old));
                }

                Op::Arith(op) => {
                    let r = self.pop_val()?.to_num();
                    let l = self.pop_val()?.to_num();
                    self.push(Value::Num(arith(*op, l, r)?));
                }
                Op::Neg => {
                    let v = self.pop_val()?.to_num();
                    self.push(Value::Num(-v));
                }
                Op::Pos => {
                    let v = self.pop_val()?.to_num();
                    self.push(Value::Num(v));
                }
                Op::Not => {
                    let v = self.pop_val()?;
                    self.push(Value::Num(f64::from(u8::from(!v.truthy()))));
                }
                Op::Cmp(op) => {
                    let r = self.pop_val()?;
                    let l = self.pop_val()?;
                    let convfmt = self.convfmt();
                    let yes = compare(&l, &r, *op, &convfmt);
                    self.push(Value::Num(f64::from(u8::from(yes))));
                }
                Op::Concat => {
                    let r = self.pop_val()?;
                    let l = self.pop_val()?;
                    let mut s = self.to_str(&l).as_ref().clone();
                    s.extend_from_slice(&self.to_str(&r));
                    self.push(Value::str(s));
                }
                Op::Match(neg) => {
                    let re = self.pop_regex()?;
                    let subject = self.pop_val()?;
                    let subject = self.to_str(&subject);
                    let m = re.is_match(&subject)?;
                    self.push(Value::Num(f64::from(u8::from(m != *neg))));
                }
                Op::In(arr, n) => {
                    let subs = self.pop_subscript(*n)?;
                    let a = self.array_of(*arr)?;
                    let convfmt = self.convfmt();
                    let present = a.borrow().get(&subs, &convfmt).is_some();
                    self.push(Value::Num(f64::from(u8::from(present))));
                }
                Op::Bool => {
                    let v = self.pop_val()?;
                    self.push(Value::Num(f64::from(u8::from(v.truthy()))));
                }

                Op::Jump(t) => pc = *t,
                Op::JumpFalse(t) => {
                    if !self.pop_val()?.truthy() {
                        pc = *t;
                    }
                }
                Op::JumpTrue(t) => {
                    if self.pop_val()?.truthy() {
                        pc = *t;
                    }
                }
                Op::AndJump(t) => {
                    let v = self.pop_val()?;
                    if !v.truthy() {
                        self.push(Value::Num(0.0));
                        pc = *t;
                    }
                }
                Op::OrJump(t) => {
                    let v = self.pop_val()?;
                    if v.truthy() {
                        self.push(Value::Num(1.0));
                        pc = *t;
                    }
                }
                Op::Pop => {
                    self.pop()?;
                }

                Op::Call(f, nargs) => pc = self.call(code, *f, *nargs, pc)?,
                Op::Return(has) => {
                    let v = if *has { self.pop_val()? } else { Value::Uninit };
                    let Some(frame) = self.frames.pop() else {
                        return Err(internal("return outside a function"));
                    };
                    self.stack.truncate(frame.stack_base);
                    self.iters.truncate(frame.iter_base);
                    self.push(v);
                    pc = frame.ret;
                }

                Op::Builtin(b, n) => {
                    let args = self.pop_vals(*n)?;
                    let v = self.builtin(*b, &args)?;
                    self.push(v);
                }
                Op::LengthArr(v) => {
                    let a = self.array_of(*v)?;
                    let n = a.borrow().len();
                    self.push(Value::Num(f64::from(u32::try_from(n).unwrap_or(u32::MAX))));
                }
                Op::Split(arr, has_sep) => {
                    let sep = if *has_sep { Some(self.pop()?) } else { None };
                    let s = self.pop_val()?;
                    let n = self.split(&s, *arr, sep)?;
                    self.push(n);
                }
                Op::Sub(global) => {
                    let place = self.pop_place()?;
                    let repl = self.pop_val()?;
                    let re = self.pop_regex()?;
                    let n = self.substitute(*global, &re, &repl, &place)?;
                    self.push(n);
                }
                Op::MatchFn => {
                    let re = self.pop_regex()?;
                    let s = self.pop_val()?;
                    let v = self.match_fn(&s, &re)?;
                    self.push(v);
                }
                Op::Sprintf(n) => {
                    let vals = self.pop_vals(*n)?;
                    let Some((fmt, rest)) = vals.split_first() else {
                        return Err(internal("sprintf with no format"));
                    };
                    let fmt = self.to_str(fmt);
                    let convfmt = self.string_of(crate::ast::V_CONVFMT);
                    let text = crate::fmt::sprintf(&fmt, rest, &convfmt).map_err(Fatal::Said)?;
                    self.push(Value::str(text));
                }

                Op::GetlineMain(into) => {
                    let place = if *into { Some(self.pop_place()?) } else { None };
                    let v = self.getline_main(place.as_ref())?;
                    self.push(v);
                }
                Op::GetlineFile(into) => {
                    let place = if *into { Some(self.pop_place()?) } else { None };
                    let file = self.pop_val()?;
                    let v = self.getline_file(&file, place.as_ref())?;
                    self.push(v);
                }
                Op::GetlineCmd(into) => {
                    let place = if *into { Some(self.pop_place()?) } else { None };
                    let cmd = self.pop_val()?;
                    let v = self.getline_cmd(&cmd, place.as_ref())?;
                    self.push(v);
                }
                Op::Print(n, mode) => {
                    let vals = self.pop_vals(*n)?;
                    let target = self.pop_target(*mode)?;
                    self.print_values(&vals, target)?;
                }
                Op::Printf(n, mode) => {
                    let vals = self.pop_vals(*n)?;
                    let target = self.pop_target(*mode)?;
                    self.printf_values(&vals, target)?;
                }

                Op::Delete(arr, n) => {
                    let subs = self.pop_subscript(*n)?;
                    let a = self.array_of(*arr)?;
                    let convfmt = self.convfmt();
                    // gawk's `do_delete`: an element that is not there is
                    // not an error, but its subscript's text is taken.
                    let present = a.borrow().get(&subs, &convfmt).is_some();
                    if present {
                        a.borrow_mut().remove(&subs, &convfmt);
                    } else {
                        let _ = subs.to_str(&convfmt);
                    }
                }
                Op::DeleteAll(arr) => {
                    let a = self.array_of(*arr)?;
                    a.borrow_mut().clear();
                }
                Op::DeleteLoop(arr, var) => {
                    let a = self.array_of(*arr)?;
                    let first = a.borrow().first_index();
                    if let Some(k) = first {
                        a.borrow_mut().clear();
                        // Straight into the variable, as gawk's
                        // `do_delete_loop` assigns it: no special
                        // variable's hook runs (`NR`, `FNR` and `NF` never
                        // get here; see `compile::delete_loop`).
                        self.set_cell(*var, Cell::Val(k));
                    }
                }
                Op::Next => {
                    if ctx != Ctx::Rule {
                        return Err(Fatal::Said(format!(
                            "fatal: `next' cannot be called from a `{}' rule",
                            ctx.name()
                        )));
                    }
                    return Ok(Flow::Next);
                }
                Op::NextFile => {
                    if ctx != Ctx::Rule {
                        return Err(Fatal::Said(format!(
                            "fatal: `nextfile' cannot be called from a `{}' rule",
                            ctx.name()
                        )));
                    }
                    return Ok(Flow::NextFile);
                }
                Op::Exit(has) => {
                    self.exited = true;
                    if *has {
                        // gawk's `(int) get_number_si(v)`: the C conversion,
                        // so `exit 1e30` is 0, not a saturated 255.
                        #[allow(clippy::cast_possible_truncation)]
                        let status = c_long(self.pop_val()?.to_num()) as i32;
                        self.exit_code = Some(status);
                    } else if self.exit_code.is_none() {
                        self.exit_code = Some(0);
                    }
                    return Ok(Flow::Exit);
                }
                Op::ForInStart(arr) => {
                    let array = self.array_of(*arr)?;
                    let keys = array.borrow().indices();
                    self.iters.push(ForIter { keys, next: 0 });
                }
                Op::ForInNext(var, exit) => {
                    let Some(it) = self.iters.last_mut() else {
                        return Err(internal("for-in without a loop"));
                    };
                    let key = it.keys.get(it.next).cloned();
                    it.next = it.next.saturating_add(1);
                    match key {
                        Some(k) => self.set_var(*var, k)?,
                        None => pc = *exit,
                    }
                }
                Op::ForInPop => {
                    self.iters.pop();
                }
                Op::RangeOpen(id, t) => {
                    if self.ranges.get(*id).copied().unwrap_or(false) {
                        pc = *t;
                    }
                }
                Op::RangeStarted(id) => {
                    let ended = self.pop_val()?.truthy();
                    if !ended && let Some(slot) = self.ranges.get_mut(*id) {
                        *slot = true;
                    }
                }
                Op::RangeEnded(id) => {
                    let ended = self.pop_val()?.truthy();
                    if ended && let Some(slot) = self.ranges.get_mut(*id) {
                        *slot = false;
                    }
                }
                Op::Halt => return Ok(Flow::Normal),
            }
        }
    }

    // ---- the stack ------------------------------------------------------

    fn push(&mut self, v: Value) {
        self.stack.push(Item::Val(v));
    }

    fn pop(&mut self) -> R<Item> {
        self.stack.pop().ok_or_else(|| internal("stack underflow"))
    }

    fn pop_val(&mut self) -> R<Value> {
        match self.pop()? {
            Item::Val(v) => Ok(v),
            _ => Err(internal("expected a value")),
        }
    }

    fn pop_place(&mut self) -> R<Place> {
        match self.pop()? {
            Item::Place(p) => Ok(p),
            _ => Err(internal("expected a place")),
        }
    }

    /// The top `n` values, in the order they were pushed.
    fn pop_vals(&mut self, n: usize) -> R<Vec<Value>> {
        let at = self
            .stack
            .len()
            .checked_sub(n)
            .ok_or_else(|| internal("stack underflow"))?;
        self.stack
            .drain(at..)
            .map(|item| match item {
                Item::Val(v) => Ok(v),
                _ => Err(internal("expected a value")),
            })
            .collect()
    }

    /// The top `n` values as one subscript: one value as it is -- the array
    /// decides what it makes of it -- and several joined with `SUBSEP` into a
    /// string (gawk's `concat_exp`), how awk gives a one-dimensional map
    /// two-dimensional syntax.
    fn pop_subscript(&mut self, n: usize) -> R<Value> {
        let mut vals = self.pop_vals(n)?;
        if vals.len() == 1
            && let Some(one) = vals.pop()
        {
            return Ok(one);
        }
        let sep = self.string_of(V_SUBSEP);
        let mut out = Str::new();
        for (i, v) in vals.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(&sep);
            }
            out.extend_from_slice(&self.to_str(v));
        }
        Ok(Value::str(out))
    }

    /// A pattern operand: a regex literal as it is, or text compiled as a
    /// dynamic regex -- on first use, then from the cache, because the usual
    /// shape is a pattern held in a variable and used on every record.
    fn pop_regex(&mut self) -> R<Rc<Regex>> {
        match self.pop()? {
            Item::Re(re) => Ok(re),
            Item::Val(v) => {
                let text = self.to_str(&v).as_ref().clone();
                self.cached_regex(text)
            }
            _ => Err(internal("expected a pattern")),
        }
    }

    fn cached_regex(&mut self, text: Str) -> R<Rc<Regex>> {
        if let Some(re) = self.re_cache.get(&text) {
            return Ok(Rc::clone(re));
        }
        let re = Rc::new(self.dynamic_regex(&text)?);
        // The cache is per distinct pattern text; a program that builds a new
        // pattern from every record would otherwise grow it without bound.
        if self.re_cache.len() < 1000 {
            self.re_cache.insert(text, Rc::clone(&re));
        }
        Ok(re)
    }

    /// A print's redirection target, which gawk evaluated before the list.
    fn pop_target(&mut self, mode: Option<RedirMode>) -> R<Option<(RedirMode, Str)>> {
        match mode {
            None => Ok(None),
            Some(mode) => {
                let v = self.pop_val()?;
                Ok(Some((mode, self.to_str(&v).as_ref().clone())))
            }
        }
    }

    // ---- places ---------------------------------------------------------

    /// A field number as gawk takes one: C's `(long)` of the value, and a
    /// negative one refused where the `$` is.
    fn field_number(&self, v: &Value) -> R<usize> {
        let n = c_long(v.to_num());
        if n < 0 {
            return Err(Fatal::Said(format!("fatal: attempt to access field {n}")));
        }
        let i = usize::try_from(n).unwrap_or(usize::MAX);
        // A field number this large is a typo, not a record; without the bound
        // `$1000000000 = "x"` allocates until the machine gives up. gawk has
        // no such bound, and would try.
        if i > 16_000_000 {
            return Err(Fatal::Said(format!(
                "fatal: field {i} is beyond any plausible record"
            )));
        }
        Ok(i)
    }

    fn load(&mut self, p: &Place) -> R<Value> {
        match p {
            Place::Var(v) => self.get_var(*v),
            Place::Field(n) => self.get_field(*n),
            Place::Elem(a, subs) => {
                let convfmt = self.convfmt();
                Ok(a.borrow_mut().lookup(subs, &convfmt).clone())
            }
        }
    }

    fn store(&mut self, p: &Place, v: Value) -> R<()> {
        match p {
            Place::Var(var) => self.set_var(*var, v),
            Place::Field(n) => self.set_field(*n, v),
            Place::Elem(a, subs) => {
                let convfmt = self.convfmt();
                // An element keeps its own copy of a field, as a variable does.
                *a.borrow_mut().lookup(subs, &convfmt) = v.unfield();
                Ok(())
            }
        }
    }

    // ---- calls ----------------------------------------------------------

    /// Enter function `f` with the `nargs` arguments on the stack, returning
    /// where to continue: the function's first instruction.
    fn call(&mut self, code: &Code, f: usize, nargs: usize, ret: usize) -> R<usize> {
        let Some(func) = code.funcs.get(f) else {
            return Err(internal("call of a function that was never compiled"));
        };
        let params = func.params;
        let entry = func.entry;
        if nargs > params {
            // gawk warns, on every such call, and drops the extras -- which
            // it has already evaluated, side effects and all.
            let name = self.prog.funcs.get(f).map_or("?", |fd| fd.name.as_str());
            let message =
                format!("function `{name}' called with more arguments than declared").into_bytes();
            self.warning(&message);
        }
        let at = self
            .stack
            .len()
            .checked_sub(nargs)
            .ok_or_else(|| internal("stack underflow"))?;
        let args: Vec<Item> = self.stack.drain(at..).collect();
        let is_array = self.prog.param_is_array.get(f).cloned().unwrap_or_default();
        let mut locals: Vec<Cell> = Vec::with_capacity(params);
        let mut args = args.into_iter();
        for i in 0..params {
            let cell = match args.next() {
                Some(Item::Val(v)) => Cell::Val(v.for_call()),
                Some(Item::Arr(a)) => Cell::Arr(a),
                Some(_) => return Err(internal("expected an argument")),
                // A parameter the caller did not pass is a local: an empty
                // array if the function uses it as one -- that is how awk
                // programs declare local arrays -- else uninitialised.
                None if is_array.get(i).copied().unwrap_or(false) => Cell::Arr(new_array()),
                None => Cell::Val(Value::Uninit),
            };
            locals.push(cell);
        }
        self.frames.push(Frame {
            locals,
            ret,
            stack_base: self.stack.len(),
            iter_base: self.iters.len(),
        });
        Ok(entry)
    }

    // ---- getline --------------------------------------------------------

    /// Plain `getline`: the next record of the main input, moving `NR` and
    /// `FNR`. A file that cannot be opened is gawk's fatal error here as in
    /// the main loop; one that cannot be read is -1.
    fn getline_main(&mut self, into: Option<&Place>) -> R<Value> {
        let rec = match self.next_main_record()? {
            MainRead::Record(r) => r,
            MainRead::Eof => return Ok(Value::Num(0.0)),
            MainRead::Failed(_) => return Ok(Value::Num(-1.0)),
        };
        self.bump(V_NR);
        self.bump(crate::ast::V_FNR);
        match into {
            // Into a variable: NR and FNR move, but the fields do not, because
            // `$0` was not touched.
            Some(p) => self.store(p, Value::from_input(rec))?,
            None => self.set_record(rec),
        }
        Ok(Value::Num(1.0))
    }

    fn getline_file(&mut self, file: &Value, into: Option<&Place>) -> R<Value> {
        let name = self.to_str(file).as_ref().clone();
        if name.is_empty() {
            return Err(null_redirect("<"));
        }
        let rs = self.rs.clone();
        let rec = match self.inputs.file(&name).and_then(|r| r.next(&rs)) {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(Value::Num(0.0)),
            // A file that will not open is -1, not a fatal error: that is what
            // lets `while ((getline < f) > 0)` be written against a file that
            // may not be there.
            Err(_) => return Ok(Value::Num(-1.0)),
        };
        match into {
            Some(p) => self.store(p, Value::from_input(rec))?,
            None => self.set_record(rec),
        }
        Ok(Value::Num(1.0))
    }

    fn getline_cmd(&mut self, cmd: &Value, into: Option<&Place>) -> R<Value> {
        let name = self.to_str(cmd).as_ref().clone();
        if name.is_empty() {
            return Err(null_redirect("|"));
        }
        let rs = self.rs.clone();
        // Nothing is flushed first: the command writes into our pipe, not to
        // our standard output, and gawk's `gawk_popen` does not flush either.
        let rec = match self.inputs.command(&name).and_then(|r| r.next(&rs)) {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(Value::Num(0.0)),
            Err(_) => return Ok(Value::Num(-1.0)),
        };
        self.bump(V_NR);
        match into {
            Some(p) => self.store(p, Value::from_input(rec))?,
            None => self.set_record(rec),
        }
        Ok(Value::Num(1.0))
    }

    // ---- built-in functions ---------------------------------------------

    /// The built-ins whose arguments are plain values.
    #[allow(clippy::too_many_lines)]
    fn builtin(&mut self, b: Builtin, args: &[Value]) -> R<Value> {
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Uninit);
        let num = |i: usize| arg(i).to_num();
        Ok(match b {
            Builtin::Length => {
                let s = self.to_str(&arg(0));
                Value::Num(count_chars(&s))
            }
            Builtin::Substr => {
                let s = self.to_str(&arg(0));
                let chars: Vec<ch::Ch> = ch::chars(&s).collect();
                Value::str(substr(&chars, num(1), (args.len() > 2).then(|| num(2))))
            }
            Builtin::Index => {
                let hay = self.to_str(&arg(0));
                let needle = self.to_str(&arg(1));
                Value::Num(index_of(&hay, &needle))
            }
            Builtin::Sin => Value::Num(num(0).sin()),
            Builtin::Cos => Value::Num(num(0).cos()),
            Builtin::Atan2 => Value::Num(num(0).atan2(num(1))),
            Builtin::Exp => {
                let d = num(0);
                let res = d.exp();
                // gawk warns when C's `exp` sets ERANGE: a finite argument
                // whose result overflowed to infinity or underflowed to 0.
                // (A subnormal result is not one: `exp(-710)` is quiet.)
                if d.is_finite() && (res.is_infinite() || res == 0.0) {
                    let shown = crate::fmt::sprintf_one_number(b"%g", d);
                    let mut message = b"exp: argument ".to_vec();
                    message.extend_from_slice(&shown);
                    message.extend_from_slice(b" is out of range");
                    self.warning(&message);
                }
                Value::Num(res)
            }
            Builtin::Log | Builtin::Sqrt => {
                let d = num(0);
                // `-0` is not negative here, as it is not to C's `<`.
                if d < 0.0 {
                    let name = if b == Builtin::Log { "log" } else { "sqrt" };
                    let shown = crate::fmt::sprintf_one_number(b"%g", d);
                    let mut message = format!("{name}: received negative argument ").into_bytes();
                    message.extend_from_slice(&shown);
                    self.warning(&message);
                }
                Value::Num(if b == Builtin::Log { d.ln() } else { d.sqrt() })
            }
            Builtin::Int => Value::Num(num(0).trunc()),
            Builtin::Rand => Value::Num(self.next_random()),
            Builtin::Srand => {
                let previous = self.seed;
                let new = if args.is_empty() {
                    seconds_since_epoch()
                } else {
                    num(0)
                };
                self.seed = new;
                let bits = new.to_bits();
                // Any odd, non-zero state will do; xorshift is dead at zero.
                self.rng = bits ^ 0x9e37_79b9_7f4a_7c15 | 1;
                Value::Num(previous)
            }
            Builtin::Tolower | Builtin::Toupper => {
                let s = self.to_str(&arg(0));
                let mut out = Str::new();
                for c in ch::chars(&s) {
                    let mapped = if b == Builtin::Tolower {
                        c.to_lowercase()
                    } else {
                        c.to_uppercase()
                    };
                    for m in mapped {
                        m.push_to(&mut out);
                    }
                }
                Value::str(out)
            }
            Builtin::System => {
                // Everything buffered is written first, every redirection
                // too (gawk's `flush_io`), or the command's output and ours
                // come out in the wrong order.
                self.flush_io()?;
                let cmd = self.to_str(&arg(0));
                // An empty command runs nothing and is 0.
                if cmd.is_empty() {
                    return Ok(Value::Num(0.0));
                }
                match crate::io::shell(&cmd).status() {
                    Ok(s) => Value::Num(f64::from(raw_status(s))),
                    Err(_) => Value::Num(-1.0),
                }
            }
            Builtin::Close => {
                let name = self.to_str(&arg(0));
                // gawk syncs standard output first; a failure there is not
                // this close's to report.
                let _ = self.out.flush_stdout();
                if self.inputs.close(&name) {
                    return Ok(Value::Num(0.0));
                }
                match self.out.close(&name) {
                    // Under `--posix` a successful close is 0, whatever a
                    // command exited with.
                    Some(Ok(())) => Value::Num(0.0),
                    Some(Err(e)) => {
                        return Err(Fatal::Said(format!(
                            "fatal: flush to \"{}\" failed: {}",
                            String::from_utf8_lossy(&name),
                            coreutils::errmsg::strerror(&e)
                        )));
                    }
                    None => Value::Num(-1.0),
                }
            }
            Builtin::Fflush => {
                let name = args.first().map(|v| self.to_str(v).as_ref().clone());
                match name {
                    // `fflush()` and `fflush("")` both flush everything.
                    None => {
                        self.flush_io()?;
                        Value::Num(0.0)
                    }
                    Some(n) if n.is_empty() => {
                        self.flush_io()?;
                        Value::Num(0.0)
                    }
                    Some(n) => self.fflush_one(&n)?,
                }
            }
            // These take arrays, patterns or targets, and have instructions
            // of their own.
            Builtin::Split | Builtin::Sub | Builtin::Gsub | Builtin::Match | Builtin::Sprintf => {
                return Err(internal("a built-in with its own instruction"));
            }
        })
    }

    /// `fflush(name)`, as gawk's `do_fflush` answers it.
    fn fflush_one(&mut self, name: &[u8]) -> R<Value> {
        let shown = String::from_utf8_lossy(name).into_owned();
        if let Some(pipe) = self.inputs.open_as(name) {
            let what = if pipe { "pipe" } else { "file" };
            let message =
                format!("fflush: cannot flush: {what} `{shown}' opened for reading, not writing");
            self.warning(message.as_bytes());
            return Ok(Value::Num(-1.0));
        }
        match self.out.flush_one(name) {
            Some(Ok(())) => return Ok(Value::Num(0.0)),
            Some(Err(e)) if coreutils::stdfd::reader_gone(&e) && name == b"/dev/stdout" => {
                return Err(Fatal::ReaderGone);
            }
            Some(Err(e)) => {
                return Err(Fatal::Said(format!(
                    "fatal: fflush: cannot flush file `{shown}': {}",
                    coreutils::errmsg::strerror(&e)
                )));
            }
            None => {}
        }
        // The standard streams flush by name whether or not anything was
        // redirected to them (`stdfile`).
        if name == b"/dev/stdout" {
            return match self.out.flush_stdout() {
                Ok(()) => Ok(Value::Num(0.0)),
                Err(e) if coreutils::stdfd::reader_gone(&e) => Err(Fatal::ReaderGone),
                Err(e) => Err(Fatal::Said(format!(
                    "fatal: fflush: cannot flush standard output: {}",
                    coreutils::errmsg::strerror(&e)
                ))),
            };
        }
        if name == b"/dev/stderr" {
            // Standard error is not buffered here; there is nothing to flush.
            return Ok(Value::Num(0.0));
        }
        let message = format!("fflush: `{shown}' is not an open file, pipe or co-process");
        self.warning(message.as_bytes());
        Ok(Value::Num(-1.0))
    }

    /// `split(s, arr [, sep])`: `sep` a regex literal, text, or the current
    /// `FS` when not given.
    fn split(&mut self, s: &Value, arr: VarRef, sep: Option<Item>) -> R<Value> {
        let s = self.to_str(s).as_ref().clone();
        let fs = match sep {
            None => None,
            Some(Item::Re(re)) => Some(Fs::Regex(re)),
            Some(Item::Val(v)) => {
                let text = self.to_str(&v);
                Some(self.make_fs(&text)?)
            }
            Some(_) => return Err(internal("expected a separator")),
        };
        let parts = match &fs {
            None => self.split_record(&s)?,
            Some(f) => split_with(f, &s, false)?,
        };
        let a = self.array_of(arr)?;
        {
            // gawk's `do_split`: the array emptied, then each piece stored
            // under the number of its place, so the array is an integer one.
            let mut m = a.borrow_mut();
            m.clear();
            for (i, p) in parts.iter().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let k = Value::Num(i.saturating_add(1) as f64);
                *m.lookup(&k, b"%.6g") = Value::from_input(p.clone());
            }
        }
        Ok(Value::Num(f64::from(
            u32::try_from(parts.len()).unwrap_or(u32::MAX),
        )))
    }

    /// `sub` and `gsub`.
    ///
    /// The replacement's `&` stands for the matched text and `\&` for a
    /// literal ampersand -- the one piece of syntax in awk where a backslash
    /// has to be interpreted at *substitution* time rather than when the
    /// string was read.
    fn substitute(&mut self, global: bool, re: &Regex, repl: &Value, target: &Place) -> R<Value> {
        let repl = self.to_str(repl).as_ref().clone();
        let sv = self.load(target)?;
        let hay = self.to_str(&sv).as_ref().clone();

        let mut out = Str::new();
        let mut last = 0usize;
        let mut count = 0u32;
        for span in re.find_iter(&hay) {
            let (s, e) = span?;
            out.extend_from_slice(hay.get(last..s).unwrap_or_default());
            expand_ampersand(&repl, hay.get(s..e).unwrap_or_default(), &mut out);
            last = e;
            count = count.saturating_add(1);
            if !global {
                break;
            }
        }
        if count == 0 {
            return Ok(Value::Num(0.0));
        }
        out.extend_from_slice(hay.get(last..).unwrap_or_default());
        self.store(target, Value::str(out))?;
        Ok(Value::Num(f64::from(count)))
    }

    /// `match(s, re)`, setting `RSTART` and `RLENGTH`.
    fn match_fn(&mut self, s: &Value, re: &Regex) -> R<Value> {
        let s = self.to_str(s);
        match re.find(&s)? {
            Some((a, b)) => {
                // RSTART and RLENGTH are in characters, so the byte offsets the
                // engine gives have to be converted.
                let start = count_chars(s.get(..a).unwrap_or_default()) + 1.0;
                let len = count_chars(s.get(a..b).unwrap_or_default());
                self.set_global(V_RSTART, Value::Num(start));
                self.set_global(V_RLENGTH, Value::Num(len));
                Ok(Value::Num(start))
            }
            None => {
                self.set_global(V_RSTART, Value::Num(0.0));
                self.set_global(V_RLENGTH, Value::Num(-1.0));
                Ok(Value::Num(0.0))
            }
        }
    }
}

/// `substr(s, m [, n])` over `s`'s characters, as gawk 5.2.1's `do_substr`
/// computes it: the start and the length are *truncated*, not rounded --
/// `substr("hello", 1.5, 2.4)` is `he` -- a start below 1 (or NaN) is 1 with
/// the length kept, so `substr("hello", 0, 2)` is `he` too, and a length
/// below 1 (or NaN) is the empty string.
///
/// (This rounded once, on the belief that gawk did. Its source says
/// otherwise, and so does running it.)
fn substr(chars: &[ch::Ch], start: f64, length: Option<f64>) -> Str {
    // `! (d >= 1)`, so NaN takes the branch, as in gawk.
    let start = if start >= 1.0 { start } else { 1.0 };
    let length = match length {
        Some(n) if n >= 1.0 => Some(n),
        Some(_) => return Str::new(),
        None => None,
    };
    // C's conversion to `size_t`: truncation, saturating where gawk clamps.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let to_size = |d: f64| -> usize {
        if d >= usize::MAX as f64 {
            usize::MAX
        } else {
            d as usize
        }
    };
    let indx = to_size(start - 1.0);
    let len = chars.len();
    if indx >= len {
        return Str::new();
    }
    let rest = len.saturating_sub(indx);
    let n = length.map_or(rest, |n| to_size(n).min(rest));
    let mut out = Str::new();
    for c in chars.get(indx..indx.saturating_add(n)).unwrap_or_default() {
        c.push_to(&mut out);
    }
    out
}

/// `system`'s answer under `--posix`: the full wait status, as POSIX says
/// (`exit 3` is 768, death by signal 9 is 9), where gawk otherwise gives the
/// exit code.
#[cfg(unix)]
fn raw_status(s: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    s.into_raw()
}

/// Where there is no wait status to give, the exit code is all there is.
#[cfg(not(unix))]
fn raw_status(s: std::process::ExitStatus) -> i32 {
    s.code().unwrap_or(0)
}

/// `l op r`, with gawk's refusal of a zero divisor.
fn arith(op: BinOp, l: f64, r: f64) -> R<f64> {
    Ok(match op {
        BinOp::Add => l + r,
        BinOp::Sub => l - r,
        BinOp::Mul => l * r,
        // The wording is gawk's. `attempted` is not decoration: it says the
        // division did not happen, which is the difference between this and an
        // IEEE infinity, and awk is one of the few languages where that is a
        // hard error rather than a value.
        BinOp::Div => {
            if r == 0.0 {
                return Err(Fatal::Said("fatal: division by zero attempted".to_string()));
            }
            l / r
        }
        BinOp::Mod => {
            if r == 0.0 {
                return Err(Fatal::Said(
                    "fatal: division by zero attempted in `%'".to_string(),
                ));
            }
            // C's `fmod`, which keeps the sign of the left operand -- awk is
            // specified in terms of it, so `-7 % 3` is -1 and not 2.
            l % r
        }
        BinOp::Pow => calc_exp(l, r),
    })
}

/// `l op= r`: gawk names the assignment operator in its refusal.
fn arith_assign(op: BinOp, l: f64, r: f64) -> R<f64> {
    match op {
        BinOp::Div if r == 0.0 => Err(Fatal::Said(
            "fatal: division by zero attempted in `/='".to_string(),
        )),
        BinOp::Mod if r == 0.0 => Err(Fatal::Said(
            "fatal: division by zero attempted in `%='".to_string(),
        )),
        _ => arith(op, l, r),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn sub(s: &str, start: f64, length: Option<f64>) -> String {
        let chars: Vec<ch::Ch> = ch::chars(s.as_bytes()).collect();
        String::from_utf8(substr(&chars, start, length)).unwrap()
    }

    /// gawk 5.2.1's `do_substr`, case by case as gawk printed them.
    #[test]
    fn substr_truncates_its_arguments_as_gawk_does() {
        assert_eq!(sub("hello", 1.5, Some(2.4)), "he");
        assert_eq!(sub("hello", 2.7, None), "ello");
        assert_eq!(sub("hello", 0.9, Some(2.0)), "he");
        assert_eq!(sub("hello", 2.0, Some(0.9)), "");
        assert_eq!(sub("hello", 2.0, Some(1.9)), "e");
        // A start below 1 is 1, the length kept.
        assert_eq!(sub("hello", 0.0, Some(2.0)), "he");
        assert_eq!(sub("hello", -1.0, Some(3.0)), "hel");
        assert_eq!(sub("hello", -1e30, Some(1e30)), "hello");
        // Past the end, or from nothing, is empty.
        assert_eq!(sub("hello", 1e30, None), "");
        assert_eq!(sub("", 1.0, None), "");
        // NaN: a start that is not `>= 1` is 1; a length that is not is empty.
        assert_eq!(sub("hello", f64::NAN, None), "hello");
        assert_eq!(sub("hello", 2.0, Some(f64::NAN)), "");
        // Characters, not bytes.
        assert_eq!(sub("héllo", 2.0, Some(2.0)), "él");
    }

    /// The divisions gawk refuses at run time, worded as it words them.
    #[test]
    fn a_zero_divisor_names_its_operator() {
        let said = |r: R<f64>| match r {
            Err(Fatal::Said(m)) => m,
            _ => panic!("expected a refusal"),
        };
        assert_eq!(
            said(arith(BinOp::Div, 1.0, 0.0)),
            "fatal: division by zero attempted"
        );
        assert_eq!(
            said(arith(BinOp::Mod, 1.0, -0.0)),
            "fatal: division by zero attempted in `%'"
        );
        assert_eq!(
            said(arith_assign(BinOp::Div, 1.0, 0.0)),
            "fatal: division by zero attempted in `/='"
        );
        assert_eq!(
            said(arith_assign(BinOp::Mod, 1.0, 0.0)),
            "fatal: division by zero attempted in `%='"
        );
        assert!(arith_assign(BinOp::Add, 1.0, 0.0).is_ok());
        // `^` is gawk's, by squaring.
        assert!(matches!(arith(BinOp::Pow, 2.0, -1074.0), Ok(v) if v == 0.0));
    }
}
