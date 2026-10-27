//! The evaluation stack, the registers' stacks and their arrays: `stack.c`
//! and `array.c`.
//!
//! Each level of a register's stack carries an array of its own: `S` pushes
//! a level with an empty array, `L` pops one and its array with it. Storing
//! into an array of an empty register makes a level with no value
//! (upstream's `DC_UNINITIALIZED`), which `l` then reports as a bug and `L`
//! as an empty stack -- upstream's own answers, kept.

use bcnumber::Num;

use crate::{Data, Dc, Level, show_id};

/// A binary operation: its result, or `None` for a failure already said.
pub type Op2 = fn(&mut Dc, &Num, &Num, i32) -> Option<Num>;

/// A binary operation with two results (`~`).
pub type Op2Pair = fn(&mut Dc, &Num, &Num, i32) -> Option<(Num, Num)>;

/// A ternary operation (`|`).
pub type Op3 = fn(&mut Dc, &Num, &Num, &Num, i32) -> Option<Num>;

impl Dc {
    /// `Empty_Stack`.
    fn empty_stack(&mut self) {
        self.complain(b"stack empty\n");
    }

    /// `non-numeric value`.
    fn non_numeric(&mut self) {
        self.complain(b"non-numeric value\n");
    }

    /// Whether the top `n` values are all numbers, having said what is
    /// wrong if they are not: too few values, or one not a number.
    fn numbers_on_top(&mut self, n: usize) -> bool {
        if self.stack.len() < n {
            self.empty_stack();
            return false;
        }
        let top = self
            .stack
            .get(self.stack.len().saturating_sub(n)..)
            .unwrap_or_default();
        if top.iter().any(|d| !matches!(d, Data::Num(_))) {
            self.non_numeric();
            return false;
        }
        true
    }

    /// Pop a value known to be a number.
    fn pop_num(&mut self) -> Option<std::rc::Rc<Num>> {
        match self.stack.pop() {
            Some(Data::Num(n)) => Some(n),
            Some(other) => {
                self.stack.push(other);
                None
            }
            None => None,
        }
    }

    /// `dc_binop (op, kscale)`: the top two numbers replaced by `op`'s
    /// result; left as they were if it fails.
    pub fn binop(&mut self, op: Op2, kscale: i32) {
        if !self.numbers_on_top(2) {
            return;
        }
        let (Some(b), Some(a)) = (self.pop_num(), self.pop_num()) else {
            return;
        };
        match op(self, &a, &b, kscale) {
            Some(r) => self.push(Data::num(r)),
            None => {
                self.push(Data::Num(a));
                self.push(Data::Num(b));
            }
        }
    }

    /// `dc_binop2 (op, kscale)`: the top two numbers replaced by `op`'s two
    /// results, the second on top.
    pub fn binop2(&mut self, op: Op2Pair, kscale: i32) {
        if !self.numbers_on_top(2) {
            return;
        }
        let (Some(b), Some(a)) = (self.pop_num(), self.pop_num()) else {
            return;
        };
        match op(self, &a, &b, kscale) {
            Some((r1, r2)) => {
                self.push(Data::num(r1));
                self.push(Data::num(r2));
            }
            None => {
                self.push(Data::Num(a));
                self.push(Data::Num(b));
            }
        }
    }

    /// `dc_triop (op, kscale)`: the top three numbers replaced by `op`'s
    /// result.
    pub fn triop(&mut self, op: Op3, kscale: i32) {
        if !self.numbers_on_top(3) {
            return;
        }
        let (Some(c), Some(b), Some(a)) = (self.pop_num(), self.pop_num(), self.pop_num()) else {
            return;
        };
        match op(self, &a, &b, &c, kscale) {
            Some(r) => self.push(Data::num(r)),
            None => {
                self.push(Data::Num(a));
                self.push(Data::Num(b));
                self.push(Data::Num(c));
            }
        }
    }

    /// `dc_cmpop ()`: the top two numbers popped and compared, the top one
    /// first: `bc_compare (top, next)`. 0 when there are not two numbers.
    pub fn cmpop(&mut self) -> i32 {
        if !self.numbers_on_top(2) {
            return 0;
        }
        let (Some(b), Some(a)) = (self.pop_num(), self.pop_num()) else {
            return 0;
        };
        bcnumber::compare(&b, &a)
    }

    /// `dc_clear_stack ()`.
    pub fn clear_stack(&mut self) {
        self.stack.clear();
    }

    /// `dc_push (value)`.
    pub fn push(&mut self, value: Data) {
        self.stack.push(value);
    }

    /// `dc_pop (&value)`: the top value, or `None` having said the stack is
    /// empty.
    pub fn pop(&mut self) -> Option<Data> {
        let v = self.stack.pop();
        if v.is_none() {
            self.empty_stack();
        }
        v
    }

    /// `dc_top_of_stack (&value)`: the top value, left there.
    pub fn top(&mut self) -> Option<Data> {
        let v = self.stack.last().cloned();
        if v.is_none() {
            self.empty_stack();
        }
        v
    }

    /// `dc_stack_rotate (n)`: the top `|n|` values rotated -- the deepest
    /// of them brought to the top for a positive `n`, the top sent down to
    /// be the deepest for a negative one.
    pub fn rotate(&mut self, n: i32) {
        let depth = self.stack.len();
        let k = usize::try_from(n.unsigned_abs())
            .unwrap_or(usize::MAX)
            .min(depth);
        if k < 2 {
            return;
        }
        let at = depth.saturating_sub(k);
        if n > 0 {
            let v = self.stack.remove(at);
            self.stack.push(v);
        } else if let Some(v) = self.stack.pop() {
            self.stack.insert(at, v);
        }
    }

    /// `dc_tell_stackdepth ()`.
    #[must_use]
    pub fn depth(&self) -> i32 {
        i32::try_from(self.stack.len()).unwrap_or(i32::MAX)
    }

    /// `dc_tell_length (value)`: a number's digits, leading zeros not
    /// counted; a string's bytes.
    #[must_use]
    pub fn tell_length(value: &Data) -> i32 {
        match value {
            Data::Num(n) => crate::numeric::numlen(n),
            Data::Str(s) => i32::try_from(s.len()).unwrap_or(i32::MAX),
        }
    }

    /// `dc_printall (obase)`: every value, top first, a line each.
    pub fn print_all(&mut self) {
        let all: Vec<Data> = self.stack.iter().rev().cloned().collect();
        for v in &all {
            self.print(v, true);
        }
    }

    /// The top level of register `id`.
    fn top_level(&mut self, id: u8) -> Option<&mut Level> {
        self.registers
            .get_mut(usize::from(id))
            .and_then(|r| r.last_mut())
    }

    /// `dc_register_get (regid, &value)`: a copy of the register's top
    /// value, 0 for an empty register; `None` for a level with no value,
    /// having said so.
    pub fn register_get(&mut self, id: u8) -> Option<Data> {
        match self.top_level(id) {
            None => Some(Data::num(bcnumber::int2num(0))),
            Some(Level { value: Some(v), .. }) => Some(v.clone()),
            Some(Level { value: None, .. }) => {
                self.say(b"BUG: register ");
                self.check_stderr();
                let shown = show_id(id, b" exists but is uninitialized?\n");
                self.err.write(&shown);
                self.check_stderr();
                None
            }
        }
    }

    /// `dc_register_set (regid, value)`: the register's top value replaced
    /// (its array kept), or a level made for it.
    pub fn register_set(&mut self, id: u8, value: Data) {
        if let Some(level) = self.top_level(id) {
            level.value = Some(value);
        } else if let Some(r) = self.registers.get_mut(usize::from(id)) {
            r.push(Level {
                value: Some(value),
                ..Level::default()
            });
        }
    }

    /// `dc_register_push (stackid, value)`: a new level, with no array.
    pub fn register_push(&mut self, id: u8, value: Data) {
        if let Some(r) = self.registers.get_mut(usize::from(id)) {
            r.push(Level {
                value: Some(value),
                ..Level::default()
            });
        }
    }

    /// `dc_register_pop (stackid, &value)`: the top level popped, its array
    /// with it, and its value given; `None` for an empty register or a level
    /// with no value, having said the register is empty.
    pub fn register_pop(&mut self, id: u8) -> Option<Data> {
        let r = self.registers.get_mut(usize::from(id))?;
        if let Some(Level { value: Some(_), .. }) = r.last() {
            return r.pop().and_then(|l| l.value);
        }
        self.say(b"stack register ");
        self.check_stderr();
        let shown = show_id(id, b" is empty\n");
        self.err.write(&shown);
        self.check_stderr();
        None
    }

    /// `dc_array_set (array_id, index, value)`: into the array of the
    /// register's top level, making a level with no value if the register
    /// has none.
    pub fn array_set(&mut self, id: u8, index: i32, value: Data) {
        let Some(r) = self.registers.get_mut(usize::from(id)) else {
            return;
        };
        if r.is_empty() {
            r.push(Level::default());
        }
        if let Some(level) = r.last_mut() {
            level.array.insert(index, value);
        }
    }

    /// `dc_array_get (array_id, index)`: a copy of the element, 0 if it was
    /// never set.
    pub fn array_get(&mut self, id: u8, index: i32) -> Data {
        self.top_level(id)
            .and_then(|l| l.array.get(&index).cloned())
            .unwrap_or_else(|| Data::num(bcnumber::int2num(0)))
    }
}
