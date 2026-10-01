//! The tree, compiled into two Thompson NFAs -- one that reads the string
//! forwards and one that reads it backwards -- and the simulation that runs
//! either: a set of threads, one per instruction, advanced a byte at a time,
//! so that no pattern costs more than the program's length for each byte.
//!
//! Bounded repetitions are written out, as glibc writes them out:
//! `x{2,4}` is `xx(x(x)?)?`, each copy of `x` its own code. The forward
//! program finds where the leftmost-longest match is; the backward one, with
//! `Mark`s at the boundaries of every concatenation and repetition that holds
//! a group, is how `dissect.rs` finds, for every boundary at once, which
//! positions the rest of a subpattern can be matched from.
//!
//! A node's code is written once for every copy of it; `Info::fwd` and
//! `Info::rev` keep where its first copy starts and ends, which is the
//! fragment a query runs when it asks about that node alone. A fragment ends
//! at one instruction, its exit, which no instruction inside it jumps past.

use super::parse::{Assert, ByteSet, NONE, Node, Tree, is_word};
use crate::list::{List, NoMem};

/// The most instructions a program may have: past it, `regcomp` answers
/// REG_ESPACE. Two million, some 24 MiB, where glibc stops only when
/// `malloc` does -- `(a{1000}){1000}` is a million copies of `a`.
pub(super) const MAX_INSTS: usize = 1 << 21;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Inst {
    /// One byte of set `.0`, then the next instruction.
    Byte(u32),
    /// A zero-width test, then the next instruction.
    Assert(Assert),
    /// Both targets.
    Split(u32, u32),
    Jmp(u32),
    /// Boundary `.1` of node `.0`, which a query about that node watches;
    /// then the next instruction.
    Mark(u32, u32),
    /// The end of the program.
    Accept,
}

/// What is known of each node.
#[derive(Clone, Copy, Debug)]
pub(super) struct Info {
    /// The fewest and most bytes it can match; `usize::MAX` for no limit.
    pub(super) min_len: usize,
    pub(super) max_len: usize,
    /// Whether it holds a group, and so has submatches to find.
    pub(super) has_group: bool,
    /// Whether it holds a back-reference.
    pub(super) has_backref: bool,
    /// The largest group number inside it (a group's own, if none is).
    pub(super) max_group: u32,
    /// Its first copy's entry and exit in the forward program.
    pub(super) fwd: (u32, u32),
    /// And in the backward one.
    pub(super) rev: (u32, u32),
}

/// A compiled pattern, as `regex_t` holds it.
pub(super) struct Program {
    pub(super) tree: Tree,
    pub(super) info: List<Info>,
    pub(super) fwd: List<Inst>,
    pub(super) rev: List<Inst>,
    /// Whether it has a back-reference: then `backref.rs` matches it.
    pub(super) backrefs: bool,
    /// The bytes a match can begin with, and whether one can begin without
    /// consuming one -- if not, a search skips to the next such byte.
    pub(super) first: ByteSet,
    pub(super) may_be_empty: bool,
    /// RE_ICASE: a back-reference matches its group's text in either
    /// case.
    pub(super) icase: bool,
    /// glibc's `dfa->lock`: held by the GNU searches, which may change the
    /// pattern buffer (`regex.rs`'s `Held`).
    pub(super) lock: core::sync::atomic::AtomicI32,
    /// The set of every byte, what the forward program reads a
    /// back-reference as: any string at all. With it that program matches
    /// wherever the pattern could, which is where `backref.rs` need look.
    any: u32,
}

impl Program {
    pub(super) fn info(&self, n: u32) -> Info {
        self.info.get(n as usize).copied().unwrap_or(Info {
            min_len: 0,
            max_len: usize::MAX,
            has_group: false,
            has_backref: false,
            max_group: 0,
            fwd: (NONE, NONE),
            rev: (NONE, NONE),
        })
    }

    pub(super) fn node(&self, n: u32) -> Node {
        self.tree
            .nodes
            .get(n as usize)
            .copied()
            .unwrap_or(Node::Empty)
    }

    pub(super) fn kids(&self, first: u32, len: u32) -> &[u32] {
        let a = first as usize;
        self.tree
            .kids
            .get(a..a.saturating_add(len as usize))
            .unwrap_or(&[])
    }
}

/// The tree, analysed and compiled.
pub(super) fn compile(tree: Tree, icase: bool) -> Result<Program, NoMem> {
    let n = tree.nodes.len();
    let mut info = List::with_capacity(n)?;
    let mut backrefs = false;
    // Children come before their parents, so one pass in order sees every
    // child's facts before its parent needs them.
    for id in 0..n {
        let node = tree.nodes.get(id).copied().unwrap_or(Node::Empty);
        let get = |k: u32, info: &List<Info>| info.get(k as usize).copied();
        let kid_list = |first: u32, len: u32| {
            let a = first as usize;
            tree.kids
                .get(a..a.saturating_add(len as usize))
                .unwrap_or(&[])
        };
        let mut i = Info {
            min_len: 0,
            max_len: 0,
            has_group: false,
            has_backref: false,
            max_group: 0,
            fwd: (NONE, NONE),
            rev: (NONE, NONE),
        };
        match node {
            Node::Empty | Node::Assert(_) => {}
            Node::Set(_) => {
                i.min_len = 1;
                i.max_len = 1;
            }
            Node::BackRef(_) => {
                backrefs = true;
                i.has_backref = true;
                i.max_len = usize::MAX;
            }
            Node::Group { index, body } => {
                i.has_group = true;
                i.max_group = index;
                if let Some(b) = get(body, &info) {
                    i.min_len = b.min_len;
                    i.max_len = b.max_len;
                    i.has_backref = b.has_backref;
                    i.max_group = i.max_group.max(b.max_group);
                }
            }
            Node::Cat { first, len } => {
                for &k in kid_list(first, len) {
                    if let Some(b) = get(k, &info) {
                        i.min_len = i.min_len.saturating_add(b.min_len);
                        i.max_len = i.max_len.saturating_add(b.max_len);
                        i.has_group |= b.has_group;
                        i.has_backref |= b.has_backref;
                        i.max_group = i.max_group.max(b.max_group);
                    }
                }
            }
            Node::Alt { first, len } => {
                i.min_len = usize::MAX;
                for &k in kid_list(first, len) {
                    if let Some(b) = get(k, &info) {
                        i.min_len = i.min_len.min(b.min_len);
                        i.max_len = i.max_len.max(b.max_len);
                        i.has_group |= b.has_group;
                        i.has_backref |= b.has_backref;
                        i.max_group = i.max_group.max(b.max_group);
                    }
                }
            }
            Node::Rep { body, min, max } => {
                if let Some(b) = get(body, &info) {
                    i.min_len = b.min_len.saturating_mul(min as usize);
                    i.max_len = if b.max_len == 0 {
                        0
                    } else if max == NONE {
                        usize::MAX
                    } else {
                        b.max_len.saturating_mul(max as usize)
                    };
                    i.has_group = b.has_group;
                    i.has_backref = b.has_backref;
                    i.max_group = b.max_group;
                }
            }
        }
        info.push(i)?;
    }
    let mut p = Program {
        tree,
        info,
        fwd: List::new(),
        rev: List::new(),
        backrefs,
        first: ByteSet::EMPTY,
        may_be_empty: true,
        icase,
        lock: core::sync::atomic::AtomicI32::new(0),
        any: NONE,
    };
    if backrefs {
        p.any = u32::try_from(p.tree.sets.len()).map_err(|_| NoMem)?;
        p.tree.sets.push(ByteSet([u64::MAX; 4]))?;
    }
    p.fwd = emit(&mut p, false)?;
    if !backrefs {
        // Only `dissect.rs` runs backwards, and only without back-references.
        p.rev = emit(&mut p, true)?;
    }
    let (first, may_be_empty) = first_bytes(&p)?;
    p.first = first;
    p.may_be_empty = may_be_empty;
    Ok(p)
}

/// The emitter's work: a stack of these rather than a recursion, so that a
/// pattern nested ten thousand deep costs heap, not stack.
#[derive(Clone, Copy)]
enum Task {
    Enter(u32),
    Leave { node: u32, pending: usize },
    Mark { node: u32, slot: u32 },
    AltBranch { node: u32, k: u32 },
    AltAfter { node: u32, k: u32, split: u32 },
    RepCopy { node: u32, c: u32 },
    RepCopyAfter { node: u32, c: u32 },
    RepTail { node: u32 },
    RepLoopEnd { split: u32, head: u32 },
    RepOpt { node: u32, c: u32 },
    RepOptAfter { node: u32, c: u32 },
}

struct Emitter<'p> {
    p: &'p mut Program,
    code: List<Inst>,
    reverse: bool,
    /// Splits and jumps whose open target is the end of a node still being
    /// written, innermost last.
    pending: List<u32>,
    tasks: List<Task>,
}

fn emit(p: &mut Program, reverse: bool) -> Result<List<Inst>, NoMem> {
    let root = p.tree.root;
    let mut e = Emitter {
        p,
        code: List::new(),
        reverse,
        pending: List::new(),
        tasks: List::new(),
    };
    e.tasks.push(Task::Enter(root))?;
    while let Some(t) = e.tasks.pop() {
        e.step(t)?;
    }
    e.put(Inst::Accept)?;
    Ok(e.code)
}

impl Emitter<'_> {
    fn pc(&self) -> u32 {
        // `put` keeps the program under MAX_INSTS, far below u32::MAX.
        self.code.len() as u32
    }

    fn put(&mut self, i: Inst) -> Result<u32, NoMem> {
        if self.code.len() >= MAX_INSTS {
            return Err(NoMem);
        }
        let pc = self.pc();
        self.code.push(i)?;
        Ok(pc)
    }

    /// The open target of the split or jump at `at`, closed onto `to`.
    fn patch(&mut self, at: u32, to: u32) {
        if let Some(i) = self.code.get_mut(at as usize) {
            match i {
                Inst::Split(_, b) => *b = to,
                Inst::Jmp(t) => *t = to,
                _ => {}
            }
        }
    }

    /// Whether node `n`'s backward code carries boundary marks: a
    /// concatenation or repetition with a group in it, which `dissect.rs`
    /// will need to take apart.
    fn marks(&self, n: u32) -> bool {
        self.reverse && self.p.info(n).has_group
    }

    fn fragment(&mut self, n: u32) -> Option<&mut (u32, u32)> {
        let reverse = self.reverse;
        self.p
            .info
            .get_mut(n as usize)
            .map(|i| if reverse { &mut i.rev } else { &mut i.fwd })
    }

    fn step(&mut self, t: Task) -> Result<(), NoMem> {
        match t {
            Task::Enter(n) => {
                let pc = self.pc();
                if let Some(f) = self.fragment(n)
                    && f.0 == NONE
                {
                    f.0 = pc;
                }
                let pending = self.pending.len();
                self.tasks.push(Task::Leave { node: n, pending })?;
                match self.p.node(n) {
                    Node::Empty => {}
                    Node::BackRef(_) => {
                        // Any string: what the forward program is for with
                        // back-references, finding where a match could be.
                        let head = self.pc();
                        let split = self.put(Inst::Split(head.wrapping_add(1), NONE))?;
                        self.put(Inst::Byte(self.p.any))?;
                        self.put(Inst::Jmp(head))?;
                        let end = self.pc();
                        self.patch(split, end);
                    }
                    Node::Set(s) => {
                        self.put(Inst::Byte(s))?;
                    }
                    Node::Assert(a) => {
                        self.put(Inst::Assert(a))?;
                    }
                    Node::Group { body, .. } => {
                        if body != NONE {
                            self.tasks.push(Task::Enter(body))?;
                        }
                    }
                    Node::Cat { first, len } => {
                        let marks = self.marks(n);
                        let kids = self.p.kids(first, len);
                        let count = kids.len();
                        for k in 0..count {
                            // Pushed so as to run in order forwards, and last
                            // first backwards, with the boundary before
                            // element k marked once elements k.. are read.
                            let (idx, mark) = if self.reverse {
                                (k, marks && k > 0)
                            } else {
                                (count.wrapping_sub(1).wrapping_sub(k), false)
                            };
                            let kid = self.p.kids(first, len).get(idx).copied().unwrap_or(NONE);
                            if mark {
                                self.tasks.push(Task::Mark {
                                    node: n,
                                    slot: idx as u32,
                                })?;
                            }
                            self.tasks.push(Task::Enter(kid))?;
                        }
                    }
                    Node::Alt { .. } => {
                        self.tasks.push(Task::AltBranch { node: n, k: 0 })?;
                    }
                    Node::Rep { min, max, .. } => {
                        if self.marks(n) && !(max == NONE && min == 0) {
                            self.put(Inst::Mark(n, 0))?;
                        }
                        if min > 0 {
                            self.tasks.push(Task::RepCopy { node: n, c: 1 })?;
                        } else {
                            self.tasks.push(Task::RepTail { node: n })?;
                        }
                    }
                }
            }
            Task::Leave { node, pending } => {
                let end = self.pc();
                while self.pending.len() > pending {
                    if let Some(at) = self.pending.pop() {
                        self.patch(at, end);
                    }
                }
                if let Some(f) = self.fragment(node)
                    && f.1 == NONE
                {
                    f.1 = end;
                }
            }
            Task::Mark { node, slot } => {
                self.put(Inst::Mark(node, slot))?;
            }
            Task::AltBranch { node, k } => {
                let Node::Alt { first, len } = self.p.node(node) else {
                    return Ok(());
                };
                let kid = self
                    .p
                    .kids(first, len)
                    .get(k as usize)
                    .copied()
                    .unwrap_or(NONE);
                let split = if k.wrapping_add(1) < len {
                    let pc = self.pc();
                    self.put(Inst::Split(pc.wrapping_add(1), NONE))?
                } else {
                    NONE
                };
                self.tasks.push(Task::AltAfter { node, k, split })?;
                self.tasks.push(Task::Enter(kid))?;
            }
            Task::AltAfter { node, k, split } => {
                if split != NONE {
                    let jmp = self.put(Inst::Jmp(NONE))?;
                    self.pending.push(jmp)?;
                    let next = self.pc();
                    self.patch(split, next);
                    self.tasks.push(Task::AltBranch {
                        node,
                        k: k.wrapping_add(1),
                    })?;
                }
            }
            Task::RepCopy { node, c } => {
                let Node::Rep { body, .. } = self.p.node(node) else {
                    return Ok(());
                };
                self.tasks.push(Task::RepCopyAfter { node, c })?;
                self.tasks.push(Task::Enter(body))?;
            }
            Task::RepCopyAfter { node, c } => {
                let Node::Rep { min, max, .. } = self.p.node(node) else {
                    return Ok(());
                };
                // Past the last mandatory copy of an unbounded one, the loop
                // head carries the mark.
                if self.marks(node) && !(max == NONE && c == min) {
                    self.put(Inst::Mark(node, c))?;
                }
                if c < min {
                    self.tasks.push(Task::RepCopy {
                        node,
                        c: c.wrapping_add(1),
                    })?;
                } else {
                    self.tasks.push(Task::RepTail { node })?;
                }
            }
            Task::RepTail { node } => {
                let Node::Rep { body, min, max } = self.p.node(node) else {
                    return Ok(());
                };
                if max == NONE {
                    let head = self.pc();
                    if self.marks(node) {
                        self.put(Inst::Mark(node, min))?;
                    }
                    let pc = self.pc();
                    let split = self.put(Inst::Split(pc.wrapping_add(1), NONE))?;
                    self.tasks.push(Task::RepLoopEnd { split, head })?;
                    self.tasks.push(Task::Enter(body))?;
                } else if max > min {
                    self.tasks.push(Task::RepOpt { node, c: 1 })?;
                }
            }
            Task::RepLoopEnd { split, head } => {
                self.put(Inst::Jmp(head))?;
                let end = self.pc();
                self.patch(split, end);
            }
            Task::RepOpt { node, c } => {
                let Node::Rep { body, .. } = self.p.node(node) else {
                    return Ok(());
                };
                let pc = self.pc();
                let split = self.put(Inst::Split(pc.wrapping_add(1), NONE))?;
                self.pending.push(split)?;
                self.tasks.push(Task::RepOptAfter { node, c })?;
                self.tasks.push(Task::Enter(body))?;
            }
            Task::RepOptAfter { node, c } => {
                let Node::Rep { min, max, .. } = self.p.node(node) else {
                    return Ok(());
                };
                let slot = min.wrapping_add(c);
                if self.marks(node) {
                    self.put(Inst::Mark(node, slot))?;
                }
                if slot < max {
                    self.tasks.push(Task::RepOpt {
                        node,
                        c: c.wrapping_add(1),
                    })?;
                }
            }
        }
        Ok(())
    }
}

/// The bytes a match can begin with, and whether one can begin without
/// consuming any -- an assertion counted as passable, which can only make
/// the set larger than it need be.
fn first_bytes(p: &Program) -> Result<(ByteSet, bool), NoMem> {
    let mut first = ByteSet::EMPTY;
    let mut empty = false;
    let mut seen = List::filled(p.fwd.len(), false)?;
    let mut stack = List::new();
    stack.push(p.info(p.tree.root).fwd.0)?;
    while let Some(pc) = stack.pop() {
        let Some(s) = seen.get_mut(pc as usize) else {
            continue;
        };
        if *s {
            continue;
        }
        *s = true;
        match p.fwd.get(pc as usize) {
            Some(Inst::Byte(set)) => {
                if let Some(bs) = p.tree.sets.get(*set as usize) {
                    first.union(bs);
                }
            }
            Some(Inst::Assert(_) | Inst::Mark(..)) => stack.push(pc.wrapping_add(1))?,
            Some(Inst::Split(a, b)) => {
                stack.push(*b)?;
                stack.push(*a)?;
            }
            Some(Inst::Jmp(t)) => stack.push(*t)?,
            Some(Inst::Accept) | None => empty = true,
        }
    }
    Ok((first, empty))
}

// ---------------------------------------------------------------------------
// The subject and its contexts
// ---------------------------------------------------------------------------

/// The string being matched, and what regexec's flags make of its ends.
pub(super) struct Subject<'s> {
    /// From the string's first byte: with REG_STARTEND a match begins no
    /// earlier than `pmatch[0].rm_so`, but what precedes it is still what
    /// `^` and `\b` there see. It ends where a match must: for
    /// `re_search_2`, at its `stop`, which can be short of the string's end.
    pub(super) s: &'s [u8],
    /// The byte after `s`, when the string goes on past where a match may
    /// end: what `$` and `\b` at the end of `s` see.
    pub(super) after: Option<u8>,
    pub(super) notbol: bool,
    pub(super) noteol: bool,
    /// `^` and `$` match at a newline: the pattern buffer's
    /// `newline_anchor` (REG_NEWLINE's, and always `re_compile_pattern`'s).
    pub(super) newline: bool,
}

impl Subject<'_> {
    pub(super) fn end(&self) -> usize {
        self.s.len()
    }

    /// Whether assertion `a` holds at position `p`, as glibc's contexts
    /// have it: before the string, no word and (unless REG_NOTBOL) a line's
    /// start; after it -- the string's end, not `s`'s if `after` says it
    /// goes on -- no word and (unless REG_NOTEOL) a line's end; a newline
    /// is a line's end or start only under `newline`.
    pub(super) fn holds(&self, a: Assert, p: usize) -> bool {
        let (pw, pl, pb) = if p == 0 {
            (false, !self.notbol, true)
        } else {
            let c = self.s.get(p.wrapping_sub(1)).copied().unwrap_or(0);
            (is_word(c), self.newline && c == b'\n', false)
        };
        let next = if p >= self.s.len() {
            self.after
        } else {
            self.s.get(p).copied()
        };
        let (nw, nl, nb) = match next {
            None => (false, !self.noteol, true),
            Some(c) => (is_word(c), self.newline && c == b'\n', false),
        };
        match a {
            Assert::Bol => pl,
            Assert::Eol => nl,
            Assert::BufStart => pb,
            Assert::BufEnd => nb,
            Assert::WordStart => !pw && nw,
            Assert::WordEnd => pw && !nw,
            Assert::WordBoundary => pw != nw,
            Assert::NotWordBoundary => pw == nw,
        }
    }
}

// ---------------------------------------------------------------------------
// The simulation
// ---------------------------------------------------------------------------

/// A set of instructions, in the order they were added, each with a value:
/// where the match its thread belongs to began.
struct Threads {
    dense: List<u32>,
    vals: List<usize>,
    sparse: List<u32>,
    len: usize,
}

impl Threads {
    fn new(n: usize) -> Result<Self, NoMem> {
        Ok(Self {
            dense: List::filled(n, 0)?,
            vals: List::filled(n, 0)?,
            sparse: List::filled(n, 0)?,
            len: 0,
        })
    }

    fn contains(&self, pc: u32) -> bool {
        let Some(&i) = self.sparse.get(pc as usize) else {
            return true;
        };
        (i as usize) < self.len && self.dense.get(i as usize) == Some(&pc)
    }

    fn insert(&mut self, pc: u32, val: usize) {
        let i = self.len;
        if let (Some(d), Some(v), Some(s)) = (
            self.dense.get_mut(i),
            self.vals.get_mut(i),
            self.sparse.get_mut(pc as usize),
        ) {
            *d = pc;
            *v = val;
            *s = i as u32;
            self.len = i.wrapping_add(1);
        }
    }

    fn get(&self, i: usize) -> (u32, usize) {
        (
            self.dense.get(i).copied().unwrap_or(NONE),
            self.vals.get(i).copied().unwrap_or(0),
        )
    }

    /// Only the threads whose value is at most `v`, in their order.
    fn keep_at_most(&mut self, v: usize) {
        let mut w = 0;
        for r in 0..self.len {
            let (pc, val) = self.get(r);
            if val <= v {
                if let (Some(d), Some(x), Some(s)) = (
                    self.dense.get_mut(w),
                    self.vals.get_mut(w),
                    self.sparse.get_mut(pc as usize),
                ) {
                    *d = pc;
                    *x = val;
                    *s = w as u32;
                }
                w = w.wrapping_add(1);
            }
        }
        self.len = w;
    }
}

/// What a closure met besides threads: the fragment's exit, or one of the
/// marks being watched.
pub(super) enum Hit {
    Exit(usize),
    Mark(u32),
}

/// The simulation's memory: two thread sets and a stack, each the program's
/// size, made once for a `regexec` and used for every query it asks.
pub(super) struct Vm {
    a: Threads,
    b: Threads,
    stack: List<(u32, usize)>,
}

/// A program and one fragment of it to run over a subject: from `entry`,
/// done at `exit`; `owner`'s marks noted as met.
#[derive(Clone, Copy)]
pub(super) struct Run<'p> {
    pub(super) code: &'p [Inst],
    pub(super) sets: &'p [ByteSet],
    pub(super) entry: u32,
    pub(super) exit: u32,
    pub(super) owner: u32,
    pub(super) sub: &'p Subject<'p>,
}

impl Vm {
    pub(super) fn new(n: usize) -> Result<Self, NoMem> {
        Ok(Self {
            a: Threads::new(n)?,
            b: Threads::new(n)?,
            stack: List::with_capacity(16)?,
        })
    }

    /// Every instruction reachable from `pc` at position `pos` without
    /// consuming a byte, added to set `a` (or `b`), each with `val`.
    fn closure(
        &mut self,
        into_b: bool,
        r: Run<'_>,
        pc: u32,
        pos: usize,
        val: usize,
        hit: &mut impl FnMut(Hit),
    ) -> Result<(), NoMem> {
        self.stack.clear();
        self.stack.push((pc, val))?;
        while let Some((pc, val)) = self.stack.pop() {
            if pc == r.exit {
                hit(Hit::Exit(val));
                continue;
            }
            let set = if into_b { &mut self.b } else { &mut self.a };
            if set.contains(pc) {
                continue;
            }
            set.insert(pc, val);
            match r.code.get(pc as usize) {
                Some(Inst::Assert(a)) => {
                    if r.sub.holds(*a, pos) {
                        self.stack.push((pc.wrapping_add(1), val))?;
                    }
                }
                Some(Inst::Split(x, y)) => {
                    self.stack.push((*y, val))?;
                    self.stack.push((*x, val))?;
                }
                Some(Inst::Jmp(t)) => self.stack.push((*t, val))?,
                Some(Inst::Mark(owner, slot)) => {
                    if *owner == r.owner {
                        hit(Hit::Mark(*slot));
                    }
                    self.stack.push((pc.wrapping_add(1), val))?;
                }
                Some(Inst::Byte(_) | Inst::Accept) | None => {}
            }
        }
        Ok(())
    }

    /// The threads of set `a` that can take byte `c`, moved on into set `b`
    /// at `pos` (the position after `c` forwards, before it backwards).
    fn step(
        &mut self,
        r: Run<'_>,
        c: u8,
        pos: usize,
        hit: &mut impl FnMut(Hit),
    ) -> Result<(), NoMem> {
        self.b.len = 0;
        for i in 0..self.a.len {
            let (pc, val) = self.a.get(i);
            if let Some(Inst::Byte(s)) = r.code.get(pc as usize)
                && r.sets.get(*s as usize).is_some_and(|set| set.contains(c))
            {
                self.closure(true, r, pc.wrapping_add(1), pos, val, hit)?;
            }
        }
        core::mem::swap(&mut self.a, &mut self.b);
        Ok(())
    }

    /// The leftmost-longest match of the whole program beginning at `from`
    /// or after it but not after `last`, as (start, end) -- or with `any`,
    /// the first match found, which is all a caller asking for no submatches
    /// needs to know exists.
    pub(super) fn search(
        &mut self,
        p: &Program,
        sub: &Subject<'_>,
        from: usize,
        last: usize,
        any: bool,
    ) -> Result<Option<(usize, usize)>, NoMem> {
        let root = p.info(p.tree.root);
        let r = Run {
            code: &p.fwd,
            sets: &p.tree.sets,
            entry: root.fwd.0,
            exit: root.fwd.1,
            owner: NONE,
            sub,
        };
        let end = sub.end();
        let mut best: Option<(usize, usize)> = None;
        let mut pos = from;
        self.a.len = 0;
        loop {
            if best.is_none() && pos <= last {
                if self.a.len == 0 && !p.may_be_empty {
                    // Nothing under way, and a match must begin with one of
                    // `first`: skip to the next.
                    let skip = sub
                        .s
                        .get(pos..)
                        .and_then(|rest| rest.iter().position(|&c| p.first.contains(c)));
                    match skip {
                        Some(k) if pos.wrapping_add(k) <= last => pos = pos.wrapping_add(k),
                        _ => return Ok(None),
                    }
                }
                let at = pos;
                self.closure(false, r, r.entry, pos, pos, &mut |h| {
                    if let Hit::Exit(start) = h {
                        better(&mut best, start, at);
                    }
                })?;
            }
            if any && best.is_some() {
                return Ok(best);
            }
            if self.a.len == 0 {
                // No thread alive: done if a match was found, or the string
                // is, or no match may begin further on; otherwise one may
                // begin at the next byte.
                if best.is_some() || pos >= end || pos >= last {
                    break;
                }
                pos = pos.wrapping_add(1);
                continue;
            }
            if pos >= end {
                break;
            }
            let c = sub.s.get(pos).copied().unwrap_or(0);
            let at = pos.wrapping_add(1);
            self.step(r, c, at, &mut |h| {
                if let Hit::Exit(start) = h {
                    better(&mut best, start, at);
                }
            })?;
            pos = at;
            if let Some((s, _)) = best {
                // A thread that began later can only find a match further
                // right than the one found.
                self.a.keep_at_most(s);
            }
        }
        Ok(best)
    }

    /// The positions in `[i, j]` where fragment `r`, begun at `i`, can end.
    pub(super) fn ends(
        &mut self,
        r: Run<'_>,
        i: usize,
        j: usize,
        out: &mut Positions,
    ) -> Result<(), NoMem> {
        out.reset(i, j);
        self.a.len = 0;
        let mut pos = i;
        let mut hit_end = false;
        self.closure(false, r, r.entry, pos, 0, &mut |h| {
            if let Hit::Exit(_) = h {
                hit_end = true;
            }
        })?;
        if hit_end {
            out.set(i)?;
        }
        while pos < j && self.a.len > 0 {
            let c = r.sub.s.get(pos).copied().unwrap_or(0);
            let at = pos.wrapping_add(1);
            hit_end = false;
            self.step(r, c, at, &mut |h| {
                if let Hit::Exit(_) = h {
                    hit_end = true;
                }
            })?;
            if hit_end {
                out.set(at)?;
            }
            pos = at;
        }
        Ok(())
    }

    /// Fragment `r` of the backward program run from `j` back to `i`: at
    /// each position, which of `r.owner`'s marks it met, in `out`.
    pub(super) fn marks(
        &mut self,
        r: Run<'_>,
        i: usize,
        j: usize,
        out: &mut MarkTable,
    ) -> Result<(), NoMem> {
        self.a.len = 0;
        let mut pos = j;
        self.closure(false, r, r.entry, pos, 0, &mut |h| {
            if let Hit::Mark(slot) = h {
                out.set(j, slot);
            }
        })?;
        while pos > i && self.a.len > 0 {
            let at = pos.wrapping_sub(1);
            let c = r.sub.s.get(at).copied().unwrap_or(0);
            self.step(r, c, at, &mut |h| {
                if let Hit::Mark(slot) = h {
                    out.set(at, slot);
                }
            })?;
            pos = at;
        }
        Ok(())
    }
}

/// `best` replaced by (start, end) if that is further left, or as far left
/// and longer.
fn better(best: &mut Option<(usize, usize)>, start: usize, end: usize) {
    match *best {
        Some((s, e)) if s < start || (s == start && e >= end) => {}
        _ => *best = Some((start, end)),
    }
}

/// A set of positions in `[lo, hi]`, its storage only as long as the
/// highest one held needs: a run whose threads all die a byte in costs a
/// word, however far `hi` is.
pub(super) struct Positions {
    lo: usize,
    hi: usize,
    bits: List<u64>,
}

impl Positions {
    pub(super) const fn new() -> Self {
        Self {
            lo: 0,
            hi: 0,
            bits: List::new(),
        }
    }

    fn reset(&mut self, lo: usize, hi: usize) {
        self.lo = lo;
        self.hi = hi;
        self.bits.clear();
    }

    fn set(&mut self, p: usize) -> Result<(), NoMem> {
        if p < self.lo || p > self.hi {
            return Ok(());
        }
        let k = p.wrapping_sub(self.lo);
        let word = k / 64;
        if word >= self.bits.len() {
            self.bits.resize(word.wrapping_add(1), 0)?;
        }
        if let Some(w) = self.bits.get_mut(word) {
            *w |= 1u64 << (k % 64);
        }
        Ok(())
    }

    pub(super) fn has(&self, p: usize) -> bool {
        if p < self.lo || p > self.hi {
            return false;
        }
        let k = p.wrapping_sub(self.lo);
        self.bits
            .get(k / 64)
            .is_some_and(|w| w & (1u64 << (k % 64)) != 0)
    }

    /// The highest position that can be held: the last word's last bit.
    pub(super) fn top(&self) -> usize {
        let span = self.bits.len().saturating_mul(64);
        self.hi.min(self.lo.saturating_add(span).saturating_sub(1))
    }

    /// The largest position held that `ok` accepts.
    pub(super) fn max_where(&self, mut ok: impl FnMut(usize) -> bool) -> Option<usize> {
        if self.bits.is_empty() {
            return None;
        }
        let mut p = self.top();
        loop {
            if self.has(p) && ok(p) {
                return Some(p);
            }
            if p <= self.lo {
                return None;
            }
            p = p.wrapping_sub(1);
        }
    }
}

/// For each position of `[lo, hi]`, which of a node's `slots` marks were met
/// there.
pub(super) struct MarkTable {
    lo: usize,
    hi: usize,
    stride: usize,
    bits: List<u64>,
}

impl MarkTable {
    pub(super) fn new(lo: usize, hi: usize, slots: usize) -> Result<Self, NoMem> {
        let stride = slots.wrapping_add(63) / 64;
        let rows = hi.saturating_sub(lo).checked_add(1).ok_or(NoMem)?;
        let words = rows.checked_mul(stride.max(1)).ok_or(NoMem)?;
        Ok(Self {
            lo,
            hi,
            stride: stride.max(1),
            bits: List::filled(words, 0)?,
        })
    }

    fn set(&mut self, p: usize, slot: u32) {
        if p < self.lo || p > self.hi {
            return;
        }
        let s = slot as usize;
        let at = p
            .wrapping_sub(self.lo)
            .wrapping_mul(self.stride)
            .wrapping_add(s / 64);
        if s / 64 < self.stride
            && let Some(w) = self.bits.get_mut(at)
        {
            *w |= 1u64 << (s % 64);
        }
    }

    /// Whether any slot in `[a, b]` was met at `p`.
    pub(super) fn any(&self, p: usize, a: usize, b: usize) -> bool {
        if p < self.lo || p > self.hi || a > b {
            return false;
        }
        let row = p.wrapping_sub(self.lo).wrapping_mul(self.stride);
        let b = b.min(self.stride.wrapping_mul(64).wrapping_sub(1));
        let mut s = a;
        while s <= b {
            let word = self
                .bits
                .get(row.wrapping_add(s / 64))
                .copied()
                .unwrap_or(0);
            if s % 64 == 0 && s.wrapping_add(63) <= b {
                if word != 0 {
                    return true;
                }
                s = s.wrapping_add(64);
                continue;
            }
            if word & (1u64 << (s % 64)) != 0 {
                return true;
            }
            s = s.wrapping_add(1);
        }
        false
    }
}
