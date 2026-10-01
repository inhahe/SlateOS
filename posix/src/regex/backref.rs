//! Matching a pattern with back-references, which no automaton can: every
//! parse of the pattern is weighed, and the one the standard prefers kept.
//!
//! `parses(node, pos, env)` is the set of ways `node` can match from `pos`:
//! for each place it can end, and each set of spans it leaves the groups a
//! back-reference names (`env`), the preferred parse tree of that. Two
//! parses that end alike and leave `env` alike can be told apart only by
//! the parse trees themselves -- what follows cannot see the difference --
//! so keeping the preferred one of each is exact, and it is what makes this
//! polynomial rather than exponential in the string for every pattern that
//! is not built to defeat it. The trees are compared as `dissect.rs`'s
//! rule would choose between them (XBD 9.1 as Okui and Suzuki read it): in
//! pre-order, each position by the length it matched, -1 for one that did
//! not take part.
//!
//! A concatenation, or a repetition, is found forwards: a state is how far
//! through its parts a parse is, where, and with what env, and each keeps
//! only the best way there, since what follows a state is the same whatever
//! came before it and the order compares from the front. The ways are kept
//! as a trie of parts, shared, with jump pointers so that where two part
//! lists first differ is found in time logarithmic in their length; and
//! every parse tree is made once (hash-consed), so that two parts there are
//! told apart by the first thing about them that differs, not by walking
//! their rest. A node with neither a group nor a back-reference in it is a
//! leaf: its parses differ in nothing a report or a back-reference sees, and
//! where it can end comes from one run of the forward program over its
//! fragment.
//!
//! A back-reference matches what its group last matched in the parse --
//! nothing, if the group has not matched (glibc's answer too: the match
//! fails). Under REG_ICASE the two are compared without case.
//!
//! A node's children's parses are worked out when it first asks for them --
//! a recursion, but one level a level of the pattern's nesting, and only to
//! `NEST` deep. Past that a node whose children are not all known puts them
//! on a stack of work and is tried again after them, which costs repeated
//! work but no stack. Every node asks for all of its children's parses
//! before it builds any of its own, so working one out in the middle of
//! another disturbs nothing. The tables it keeps are bounded
//! (`MAX_ENTRIES`); a pattern and string that would need more are answered
//! as glibc answers exhaustion, REG_NOMATCH.

use super::parse::{NONE, Node};
use super::prog::{Positions, Program, Run, Subject, Vm};
use crate::list::{List, NoMem};

/// The most parse trees, trie nodes, results and table slots, each, one
/// `regexec` may make: some four million, about a hundred MiB at the worst.
const MAX_ENTRIES: usize = 1 << 22;

/// How deep children's parses are worked out as they are asked for: a few
/// hundred bytes of stack a level, so a few dozen levels at most.
const NEST: u32 = 32;

/// The spans of groups 1 to 9, as a back-reference sees them; `NONE` for a
/// group that has not matched.
type Env = [(u32, u32); 9];
const EMPTY_ENV: Env = [(NONE, NONE); 9];

/// A parse tree's node, made once for each distinct one.
#[derive(Clone, Copy, PartialEq, Eq)]
struct TreeNode {
    node: u32,
    start: u32,
    end: u32,
    /// Cat and Rep: the trie node ending its list of parts (`ROOT` for
    /// none); Alt: the branch taken; Group: the inner tree (`NONE` for
    /// `()`).
    a: u32,
    /// Alt: the branch's tree.
    b: u32,
}

/// A list of parts, as a node of the trie of all of them: its last part,
/// and the list before it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Pre {
    parent: u32,
    part: u32,
    depth: u32,
    /// An ancestor further up, at a depth fixed by this one's alone (Myers'
    /// skew-binary jump pointers): an ancestor at any depth is found in
    /// logarithmic steps.
    jump: u32,
}

/// The empty list.
const ROOT: u32 = 0;

/// One way a node matches: where it ends, the env it leaves, its tree.
#[derive(Clone, Copy)]
struct Parse {
    end: u32,
    env: u32,
    tree: u32,
}

/// An insert-only open-addressed map from 3 x u32 to u32, emptied in time
/// proportional to what it held, not to its size.
struct Map {
    keys: List<[u32; 3]>,
    vals: List<u32>,
    touched: List<usize>,
    len: usize,
}

const EMPTY_KEY: [u32; 3] = [NONE, NONE, NONE];

fn mix(words: &[u32]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &x in words {
        h = (h ^ u64::from(x)).wrapping_mul(0x0100_0000_01B3);
        h ^= h >> 29;
    }
    h
}

impl Map {
    fn new() -> Self {
        Self {
            keys: List::new(),
            vals: List::new(),
            touched: List::new(),
            len: 0,
        }
    }

    fn clear(&mut self) {
        while let Some(i) = self.touched.pop() {
            if let Some(k) = self.keys.get_mut(i) {
                *k = EMPTY_KEY;
            }
        }
        self.len = 0;
    }

    fn slot(&self, k: &[u32; 3]) -> Option<usize> {
        let cap = self.keys.len();
        if cap == 0 {
            return None;
        }
        let mask = cap.wrapping_sub(1);
        let mut i = (mix(k) as usize) & mask;
        loop {
            match self.keys.get(i) {
                Some(x) if x == k => return Some(i),
                Some(x) if *x == EMPTY_KEY => return None,
                None => return None,
                _ => i = i.wrapping_add(1) & mask,
            }
        }
    }

    fn get(&self, k: &[u32; 3]) -> Option<u32> {
        self.slot(k).and_then(|i| self.vals.get(i).copied())
    }

    fn set(&mut self, k: [u32; 3], v: u32) -> Result<(), NoMem> {
        if let Some(i) = self.slot(&k) {
            if let Some(x) = self.vals.get_mut(i) {
                *x = v;
            }
            return Ok(());
        }
        if self.len.saturating_mul(2) >= self.keys.len() {
            self.grow()?;
        }
        let mask = self.keys.len().wrapping_sub(1);
        let mut i = (mix(&k) as usize) & mask;
        while self.keys.get(i).is_some_and(|x| *x != EMPTY_KEY) {
            i = i.wrapping_add(1) & mask;
        }
        if let (Some(x), Some(y)) = (self.keys.get_mut(i), self.vals.get_mut(i)) {
            *x = k;
            *y = v;
        }
        self.touched.push(i)?;
        self.len = self.len.wrapping_add(1);
        Ok(())
    }

    fn grow(&mut self) -> Result<(), NoMem> {
        let cap = self.keys.len().saturating_mul(2).max(64);
        if cap > MAX_ENTRIES {
            return Err(NoMem);
        }
        let old_keys = core::mem::replace(&mut self.keys, List::filled(cap, EMPTY_KEY)?);
        let old_vals = core::mem::replace(&mut self.vals, List::filled(cap, 0)?);
        self.touched.clear();
        self.len = 0;
        for (k, v) in old_keys.iter().zip(old_vals.iter()) {
            if *k != EMPTY_KEY {
                self.set(*k, *v)?;
            }
        }
        Ok(())
    }
}

/// `rec` made once: its id in `list`, found through `map` by its hash (a
/// collision told apart by comparing the records themselves).
fn intern<T: Copy + PartialEq>(
    map: &mut Map,
    list: &mut List<T>,
    rec: T,
    h: u64,
) -> Result<u32, NoMem> {
    let mut probe = 0u32;
    loop {
        let key = [h as u32, (h >> 32) as u32, probe];
        match map.get(&key) {
            Some(id) => {
                if list.get(id as usize) == Some(&rec) {
                    return Ok(id);
                }
                probe = probe.wrapping_add(1);
            }
            None => {
                if list.len() >= MAX_ENTRIES {
                    return Err(NoMem);
                }
                let id = u32::try_from(list.len()).map_err(|_| NoMem)?;
                list.push(rec)?;
                map.set(key, id)?;
                return Ok(id);
            }
        }
    }
}

/// A state of a concatenation's or a repetition's forward pass: `d` parts
/// done (for a repetition without limit, past `min` all one), at `pos`, with
/// `env`; the best way there, as the state it came from and its last part;
/// and that way's trie node, once the state is done with.
#[derive(Clone, Copy)]
struct State {
    d: u32,
    pos: u32,
    env: u32,
    from: u32,
    part: u32,
    trie: u32,
}

struct Engine<'p, 's> {
    p: &'p Program,
    sub: &'p Subject<'s>,
    icase: bool,
    /// Which groups a back-reference names: only their spans are in an env.
    named: [bool; 9],
    envs: List<Env>,
    env_ids: Map,
    trees: List<TreeNode>,
    tree_ids: Map,
    pres: List<Pre>,
    pre_ids: Map,
    /// Every memo entry's parses, contiguous: `spans[memo value]` is where.
    results: List<Parse>,
    spans: List<(u32, u32)>,
    memo: Map,
    /// Scratch for building one node's parses.
    scratch: List<Parse>,
    index: Map,
    missing: List<[u32; 3]>,
    /// How many children's parses are being worked out inside one another.
    depth: u32,
    /// For a node with neither a group nor a back-reference in it.
    vm: Vm,
    ends: Positions,
}

/// A back-referencing pattern's matcher for one subject: every start it is
/// asked about shares what the others have worked out.
pub(super) struct Matcher<'p, 's> {
    e: Engine<'p, 's>,
    empty: u32,
}

impl<'p, 's> Matcher<'p, 's> {
    pub(super) fn new(p: &'p Program, sub: &'p Subject<'s>) -> Result<Self, NoMem> {
        if sub.end() >= NONE as usize {
            return Err(NoMem);
        }
        let mut named = [false; 9];
        for node in p.tree.nodes.iter() {
            if let Node::BackRef(g) = *node
                && let Some(slot) = named.get_mut((g as usize).wrapping_sub(1))
            {
                *slot = true;
            }
        }
        let mut e = Engine {
            p,
            sub,
            icase: p.icase,
            named,
            envs: List::new(),
            env_ids: Map::new(),
            trees: List::new(),
            tree_ids: Map::new(),
            pres: List::new(),
            pre_ids: Map::new(),
            results: List::new(),
            spans: List::new(),
            memo: Map::new(),
            scratch: List::new(),
            index: Map::new(),
            missing: List::new(),
            depth: 0,
            vm: Vm::new(p.fwd.len())?,
            ends: Positions::new(),
        };
        e.pres.push(Pre {
            parent: NONE,
            part: NONE,
            depth: 0,
            jump: ROOT,
        })?;
        let empty = e.intern_env(&EMPTY_ENV)?;
        Ok(Self { e, empty })
    }

    /// The preferred match beginning at `first`, into `pm[0..=nsub]`:
    /// `false` if there is none there.
    pub(super) fn at(&mut self, first: usize, pm: &mut [(isize, isize)]) -> Result<bool, NoMem> {
        let e = &mut self.e;
        let key = [e.p.tree.root, first as u32, self.empty];
        e.solve(key)?;
        let Some((a, len)) = e.lookup(&key) else {
            return Ok(false);
        };
        let mut best: Option<Parse> = None;
        for k in 0..len {
            let Some(&c) = e.results.get((a as usize).wrapping_add(k as usize)) else {
                continue;
            };
            best = match best {
                None => Some(c),
                Some(b) if c.end > b.end || (c.end == b.end && e.compare(c.tree, b.tree)? > 0) => {
                    Some(c)
                }
                keep => keep,
            };
        }
        let Some(b) = best else {
            return Ok(false);
        };
        for slot in pm.iter_mut() {
            *slot = (-1, -1);
        }
        if let Some(slot) = pm.get_mut(0) {
            *slot = (first.cast_signed(), (b.end as usize).cast_signed());
        }
        e.report(b.tree, pm)?;
        Ok(true)
    }
}

/// Where two part lists first differ.
enum Differ {
    /// The same list.
    Same,
    /// One is a prefix of the other: +1 if the first is the longer.
    Prefix(i32),
    /// Their first different parts.
    Parts(u32, u32),
}

impl Engine<'_, '_> {
    fn intern_env(&mut self, env: &Env) -> Result<u32, NoMem> {
        let mut words = [0u32; 18];
        for (k, &(a, b)) in env.iter().enumerate() {
            if let Some(w) = words.get_mut(k.wrapping_mul(2)) {
                *w = a;
            }
            if let Some(w) = words.get_mut(k.wrapping_mul(2).wrapping_add(1)) {
                *w = b;
            }
        }
        intern(&mut self.env_ids, &mut self.envs, *env, mix(&words))
    }

    fn env(&self, id: u32) -> Env {
        self.envs.get(id as usize).copied().unwrap_or(EMPTY_ENV)
    }

    /// `env` with group `g` at `[a, b)`, if a back-reference can see it.
    fn bind(&mut self, env: u32, g: u32, a: u32, b: u32) -> Result<u32, NoMem> {
        let k = (g as usize).wrapping_sub(1);
        if !self.named.get(k).copied().unwrap_or(false) {
            return Ok(env);
        }
        let mut e = self.env(env);
        if let Some(slot) = e.get_mut(k) {
            *slot = (a, b);
        }
        self.intern_env(&e)
    }

    fn tree(&mut self, t: TreeNode) -> Result<u32, NoMem> {
        let h = mix(&[t.node, t.start, t.end, t.a, t.b]);
        intern(&mut self.tree_ids, &mut self.trees, t, h)
    }

    fn t(&self, id: u32) -> TreeNode {
        self.trees.get(id as usize).copied().unwrap_or(TreeNode {
            node: NONE,
            start: 0,
            end: 0,
            a: NONE,
            b: NONE,
        })
    }

    fn pre(&self, id: u32) -> Pre {
        self.pres.get(id as usize).copied().unwrap_or(Pre {
            parent: NONE,
            part: NONE,
            depth: 0,
            jump: ROOT,
        })
    }

    /// The list `parent` with `part` after it.
    fn extend(&mut self, parent: u32, part: u32) -> Result<u32, NoMem> {
        let u = self.pre(parent);
        let j = self.pre(u.jump);
        let jump = if u.depth.wrapping_sub(j.depth) == j.depth.wrapping_sub(self.pre(j.jump).depth)
        {
            j.jump
        } else {
            parent
        };
        let rec = Pre {
            parent,
            part,
            depth: u.depth.wrapping_add(1),
            jump,
        };
        let h = mix(&[parent, part]);
        intern(&mut self.pre_ids, &mut self.pres, rec, h)
    }

    /// `v`'s ancestor at depth `d` (at most its own).
    fn ancestor(&self, mut v: u32, d: u32) -> u32 {
        loop {
            let x = self.pre(v);
            if x.depth <= d {
                return v;
            }
            v = if self.pre(x.jump).depth >= d {
                x.jump
            } else {
                x.parent
            };
        }
    }

    /// Two different lists as deep as each other: their first different
    /// parts' trie nodes.
    fn diverge(&self, mut u: u32, mut v: u32) -> (u32, u32) {
        loop {
            let (a, b) = (self.pre(u), self.pre(v));
            if a.parent == b.parent {
                return (u, v);
            }
            if a.jump == b.jump {
                u = a.parent;
                v = b.parent;
            } else {
                u = a.jump;
                v = b.jump;
            }
        }
    }

    /// Where lists `x` and `y` first differ.
    fn differ(&self, x: u32, y: u32) -> Differ {
        if x == y {
            return Differ::Same;
        }
        let (dx, dy) = (self.pre(x).depth, self.pre(y).depth);
        let (xu, yu) = if dx > dy {
            (self.ancestor(x, dy), y)
        } else {
            (x, self.ancestor(y, dx))
        };
        if xu == yu {
            return Differ::Prefix(if dx > dy { 1 } else { -1 });
        }
        let (a, b) = self.diverge(xu, yu);
        Differ::Parts(self.pre(a).part, self.pre(b).part)
    }

    fn norm(&self, t: u32) -> u32 {
        let n = self.t(t);
        n.end.wrapping_sub(n.start)
    }

    /// Two parts in the same place of two lists: +1 if `a` is preferred.
    fn part_cmp(&mut self, a: u32, b: u32) -> Result<i32, NoMem> {
        if a == b {
            return Ok(0);
        }
        let (na, nb) = (self.norm(a), self.norm(b));
        if na != nb {
            return Ok(if na > nb { 1 } else { -1 });
        }
        self.compare(a, b)
    }

    /// List `x` with part `a` after it against `y` with `b`: +1 if the first
    /// is preferred.
    fn extended_cmp(&mut self, x: u32, a: u32, y: u32, b: u32) -> Result<i32, NoMem> {
        match self.differ(x, y) {
            Differ::Same => self.part_cmp(a, b),
            Differ::Parts(pa, pb) => self.part_cmp(pa, pb),
            Differ::Prefix(longer) => {
                // The shorter list's new part meets the longer's next one.
                let (dx, dy) = (self.pre(x).depth, self.pre(y).depth);
                let r = if longer > 0 {
                    let next = self.pre(self.ancestor(x, dy.wrapping_add(1))).part;
                    self.part_cmp(next, b)?
                } else {
                    let next = self.pre(self.ancestor(y, dx.wrapping_add(1))).part;
                    self.part_cmp(a, next)?
                };
                // Alike there, the longer list has a part where the other
                // has ended: present beats absent.
                Ok(if r == 0 { longer } else { r })
            }
        }
    }

    fn lookup(&self, key: &[u32; 3]) -> Option<(u32, u32)> {
        self.memo
            .get(key)
            .and_then(|v| self.spans.get(v as usize).copied())
    }

    /// Every parse `key` needs, computed, then its own.
    fn solve(&mut self, key: [u32; 3]) -> Result<(), NoMem> {
        if self.lookup(&key).is_some() {
            return Ok(());
        }
        let mut stack: List<[u32; 3]> = List::new();
        stack.push(key)?;
        while let Some(&top) = stack.last() {
            if self.lookup(&top).is_some() {
                stack.pop();
                continue;
            }
            self.missing.clear();
            if self.compute(top)? {
                stack.pop();
            } else {
                while let Some(m) = self.missing.pop() {
                    stack.push(m)?;
                }
            }
        }
        Ok(())
    }

    /// A child's parses: known, worked out now, or -- nested too deep for
    /// that -- noted as missing, for the caller to be tried again after.
    fn child(&mut self, node: u32, pos: u32, env: u32) -> Result<Option<(u32, u32)>, NoMem> {
        let key = [node, pos, env];
        if let Some(r) = self.lookup(&key) {
            return Ok(Some(r));
        }
        if self.depth < NEST {
            self.depth = self.depth.wrapping_add(1);
            let solved = self.solve(key);
            self.depth = self.depth.wrapping_sub(1);
            solved?;
            if let Some(r) = self.lookup(&key) {
                return Ok(Some(r));
            }
        }
        self.missing.push(key)?;
        Ok(None)
    }

    fn result(&self, span: (u32, u32), k: u32) -> Parse {
        self.results
            .get((span.0 as usize).wrapping_add(k as usize))
            .copied()
            .unwrap_or(Parse {
                end: 0,
                env: 0,
                tree: NONE,
            })
    }

    /// `c` offered as a parse of the node being computed: kept if it is the
    /// first to end where it ends with its env, or preferred to the one that
    /// was.
    fn offer(&mut self, c: Parse) -> Result<(), NoMem> {
        let k = [c.end, c.env, 0];
        match self.index.get(&k) {
            Some(i) => {
                let old = self.scratch.get(i as usize).copied();
                if let Some(old) = old
                    && self.compare(c.tree, old.tree)? > 0
                    && let Some(slot) = self.scratch.get_mut(i as usize)
                {
                    *slot = c;
                }
            }
            None => {
                let i = self.scratch.len() as u32;
                self.scratch.push(c)?;
                self.index.set(k, i)?;
            }
        }
        Ok(())
    }

    fn leaf(&mut self, n: u32, start: u32, end: u32, env: u32) -> Result<(), NoMem> {
        let t = self.tree(TreeNode {
            node: n,
            start,
            end,
            a: NONE,
            b: NONE,
        })?;
        self.offer(Parse { end, env, tree: t })
    }

    /// The parses on `scratch`, stored as `key`'s.
    fn record(&mut self, key: [u32; 3]) -> Result<(), NoMem> {
        if self.results.len().saturating_add(self.scratch.len()) > MAX_ENTRIES {
            return Err(NoMem);
        }
        let first = self.results.len() as u32;
        let len = self.scratch.len() as u32;
        let scratch = core::mem::take(&mut self.scratch);
        self.results.extend_from_slice(&scratch)?;
        self.scratch = scratch;
        let id = self.spans.len() as u32;
        self.spans.push((first, len))?;
        self.memo.set(key, id)
    }

    /// The node being computed starts building its parses: after the last of
    /// its children's has been worked out, since working one out builds in
    /// the same place.
    fn fresh(&mut self) {
        self.scratch.clear();
        self.index.clear();
    }

    /// The parses of `key`, recorded -- `false` if a child's are not known
    /// yet (they are on `missing`).
    fn compute(&mut self, key: [u32; 3]) -> Result<bool, NoMem> {
        let [n, pos, env] = key;
        let p = self.p;
        let node = p.node(n);
        let info = p.info(n);
        if !info.has_group
            && !info.has_backref
            && matches!(node, Node::Cat { .. } | Node::Alt { .. } | Node::Rep { .. })
        {
            // Nothing inside it a report or a back-reference sees: any parse
            // of it to a place is as good as another, so a leaf stands for
            // them all, and the places come from one run of its fragment.
            let r = Run {
                code: &p.fwd,
                sets: &p.tree.sets,
                entry: info.fwd.0,
                exit: info.fwd.1,
                owner: NONE,
                sub: self.sub,
            };
            let from = pos as usize;
            self.vm.ends(r, from, self.sub.end(), &mut self.ends)?;
            self.fresh();
            for e in from..=self.ends.top() {
                if self.ends.has(e) {
                    self.leaf(n, pos, e as u32, env)?;
                }
            }
            self.record(key)?;
            return Ok(true);
        }
        match node {
            Node::Empty => {
                self.fresh();
                self.leaf(n, pos, pos, env)?;
            }
            Node::Assert(a) => {
                self.fresh();
                if self.sub.holds(a, pos as usize) {
                    self.leaf(n, pos, pos, env)?;
                }
            }
            Node::Set(s) => {
                self.fresh();
                let c = self.sub.s.get(pos as usize).copied();
                if let (Some(c), Some(set)) = (c, p.tree.sets.get(s as usize))
                    && set.contains(c)
                {
                    self.leaf(n, pos, pos.wrapping_add(1), env)?;
                }
            }
            Node::BackRef(g) => {
                self.fresh();
                let span = self
                    .env(env)
                    .get((g as usize).wrapping_sub(1))
                    .copied()
                    .unwrap_or((NONE, NONE));
                if span.0 != NONE && self.same(span.0, span.1, pos) {
                    let end = pos.wrapping_add(span.1.wrapping_sub(span.0));
                    self.leaf(n, pos, end, env)?;
                }
            }
            Node::Group { index, body } => {
                if body == NONE {
                    self.fresh();
                    let e2 = self.bind(env, index, pos, pos)?;
                    let t = self.tree(TreeNode {
                        node: n,
                        start: pos,
                        end: pos,
                        a: NONE,
                        b: NONE,
                    })?;
                    self.offer(Parse {
                        end: pos,
                        env: e2,
                        tree: t,
                    })?;
                } else {
                    let Some(span) = self.child(body, pos, env)? else {
                        return Ok(false);
                    };
                    self.fresh();
                    for k in 0..span.1 {
                        let c = self.result(span, k);
                        let e2 = self.bind(c.env, index, pos, c.end)?;
                        let t = self.tree(TreeNode {
                            node: n,
                            start: pos,
                            end: c.end,
                            a: c.tree,
                            b: NONE,
                        })?;
                        self.offer(Parse {
                            end: c.end,
                            env: e2,
                            tree: t,
                        })?;
                    }
                }
            }
            Node::Alt { first, len } => {
                let kids = p.kids(first, len);
                let mut ready = true;
                for &b in kids {
                    if self.child(b, pos, env)?.is_none() {
                        ready = false;
                    }
                }
                if !ready {
                    return Ok(false);
                }
                self.fresh();
                for (i, &b) in kids.iter().enumerate() {
                    let Some(span) = self.child(b, pos, env)? else {
                        return Ok(false);
                    };
                    for k in 0..span.1 {
                        let c = self.result(span, k);
                        let t = self.tree(TreeNode {
                            node: n,
                            start: pos,
                            end: c.end,
                            a: i as u32,
                            b: c.tree,
                        })?;
                        self.offer(Parse {
                            end: c.end,
                            env: c.env,
                            tree: t,
                        })?;
                    }
                }
            }
            Node::Cat { first, len } => {
                if !self.seq(n, pos, env, p.kids(first, len), 0, NONE)? {
                    return Ok(false);
                }
            }
            Node::Rep { body, min, max } => {
                if !self.seq(n, pos, env, &[body], min, max)? {
                    return Ok(false);
                }
                if min == 0 {
                    // The sole empty iteration of a repetition that matched
                    // nothing, preferred to none.
                    let Some(span) = self.child(body, pos, env)? else {
                        return Ok(false);
                    };
                    for k in 0..span.1 {
                        let c = self.result(span, k);
                        if c.end == pos {
                            let list = self.extend(ROOT, c.tree)?;
                            let t = self.tree(TreeNode {
                                node: n,
                                start: pos,
                                end: pos,
                                a: list,
                                b: NONE,
                            })?;
                            self.offer(Parse {
                                end: pos,
                                env: c.env,
                                tree: t,
                            })?;
                        }
                    }
                }
            }
        }
        self.record(key)?;
        Ok(true)
    }

    /// A concatenation of `items` (`min` and `max` unused), or a repetition
    /// of `items[0]` from `min` to `max` times (`NONE`: no limit), those past
    /// `min` non-empty: every way from `pos` to an end, the preferred list
    /// of parts for each (end, env), offered as node `n`'s parses. `false`
    /// if a part's parses are not known yet.
    fn seq(
        &mut self,
        n: u32,
        pos: u32,
        env: u32,
        items: &[u32],
        min: u32,
        max: u32,
    ) -> Result<bool, NoMem> {
        let rep = matches!(self.p.node(n), Node::Rep { .. });
        let steps = items.len() as u32;
        // Past `min` without a limit, one count stands for every count.
        let canon = |d: u32| if rep && max == NONE { d.min(min) } else { d };
        let last = |d: u32| {
            if rep {
                max != NONE && d >= max
            } else {
                d >= steps
            }
        };
        let done = |d: u32| if rep { d >= min } else { d >= steps };
        let item = |d: u32| {
            if rep {
                items.first().copied().unwrap_or(NONE)
            } else {
                items.get(d as usize).copied().unwrap_or(NONE)
            }
        };
        // Every state reachable, found first; every part's parses must be
        // known before any state can be settled.
        let mut states: List<State> = List::new();
        let mut at = Map::new();
        states.push(State {
            d: 0,
            pos,
            env,
            from: NONE,
            part: NONE,
            trie: NONE,
        })?;
        at.set([0, pos, env], 0)?;
        let mut i = 0usize;
        let mut ready = true;
        while let Some(&s) = states.get(i) {
            i = i.wrapping_add(1);
            if last(s.d) {
                continue;
            }
            let Some(span) = self.child(item(s.d), s.pos, s.env)? else {
                ready = false;
                continue;
            };
            for k in 0..span.1 {
                let c = self.result(span, k);
                if rep && c.end == s.pos && s.d >= min {
                    continue;
                }
                let key = [canon(s.d.wrapping_add(1)), c.end, c.env];
                if at.get(&key).is_none() {
                    at.set(key, states.len() as u32)?;
                    states.push(State {
                        d: key[0],
                        pos: c.end,
                        env: c.env,
                        from: NONE,
                        part: NONE,
                        trie: NONE,
                    })?;
                }
            }
        }
        if !ready {
            return Ok(false);
        }
        self.fresh();
        // A part leads to a greater count, or past the minimum without a
        // limit to a later position: so in (count, position) order each
        // state is settled before any it leads to.
        let mut order: List<u32> = List::with_capacity(states.len())?;
        for k in 0..states.len() {
            order.push(k as u32)?;
        }
        order
            .as_mut_slice()
            .sort_unstable_by_key(|&k| states.get(k as usize).map_or((0, 0), |s| (s.d, s.pos)));
        for &si in order.iter() {
            let Some(s) = states.get(si as usize).copied() else {
                continue;
            };
            let trie = if s.from == NONE {
                ROOT
            } else {
                let from = states.get(s.from as usize).map_or(ROOT, |f| f.trie);
                self.extend(from, s.part)?
            };
            if let Some(slot) = states.get_mut(si as usize) {
                slot.trie = trie;
            }
            if last(s.d) {
                continue;
            }
            let Some(span) = self.child(item(s.d), s.pos, s.env)? else {
                return Ok(false);
            };
            for k in 0..span.1 {
                let c = self.result(span, k);
                if rep && c.end == s.pos && s.d >= min {
                    continue;
                }
                let key = [canon(s.d.wrapping_add(1)), c.end, c.env];
                let Some(ti) = at.get(&key) else {
                    continue;
                };
                let Some(t) = states.get(ti as usize).copied() else {
                    continue;
                };
                let better = if t.from == NONE {
                    true
                } else {
                    let other = states.get(t.from as usize).map_or(ROOT, |f| f.trie);
                    self.extended_cmp(trie, c.tree, other, t.part)? > 0
                };
                if better && let Some(slot) = states.get_mut(ti as usize) {
                    slot.from = si;
                    slot.part = c.tree;
                }
            }
        }
        // The finished ways, the preferred one for each (end, env).
        let mut best = Map::new();
        let mut ends: List<(u32, u32, u32)> = List::new();
        for s in states.iter() {
            if !done(s.d) {
                continue;
            }
            let key = [s.pos, s.env, 0];
            match best.get(&key) {
                Some(w) => {
                    let Some(&(_, _, old)) = ends.get(w as usize) else {
                        continue;
                    };
                    if self.list_cmp(s.trie, old)? > 0
                        && let Some(slot) = ends.get_mut(w as usize)
                    {
                        *slot = (s.pos, s.env, s.trie);
                    }
                }
                None => {
                    best.set(key, ends.len() as u32)?;
                    ends.push((s.pos, s.env, s.trie))?;
                }
            }
        }
        for &(end, env2, list) in ends.iter() {
            let t = self.tree(TreeNode {
                node: n,
                start: pos,
                end,
                a: list,
                b: NONE,
            })?;
            self.offer(Parse {
                end,
                env: env2,
                tree: t,
            })?;
        }
        Ok(true)
    }

    /// Two whole lists of parts over the same text: +1 if `x` is preferred.
    fn list_cmp(&mut self, x: u32, y: u32) -> Result<i32, NoMem> {
        match self.differ(x, y) {
            Differ::Same => Ok(0),
            Differ::Prefix(longer) => Ok(longer),
            Differ::Parts(a, b) => self.part_cmp(a, b),
        }
    }

    /// Whether `[a, b)` of the string reads again from `pos`.
    fn same(&self, a: u32, b: u32, pos: u32) -> bool {
        let s = self.sub.s;
        let len = b.wrapping_sub(a) as usize;
        let (Some(x), Some(y)) = (
            s.get(a as usize..(a as usize).wrapping_add(len)),
            s.get(pos as usize..(pos as usize).wrapping_add(len)),
        ) else {
            return false;
        };
        if self.icase {
            x.eq_ignore_ascii_case(y)
        } else {
            x == y
        }
    }

    /// The standard's order on two parses of the same node over the same
    /// text: +1 if `a` is preferred, -1 if `b`, 0 if alike. Two lists of
    /// parts are compared where they first differ: the trees being made
    /// once each, a part the lists have alike is the same part.
    fn compare(&mut self, a: u32, b: u32) -> Result<i32, NoMem> {
        let mut stack: List<(u32, u32)> = List::new();
        stack.push((a, b))?;
        while let Some((x, y)) = stack.pop() {
            if x == y {
                continue;
            }
            let (tx, ty) = (self.t(x), self.t(y));
            match self.p.node(tx.node) {
                Node::Cat { .. } | Node::Rep { .. } => match self.differ(tx.a, ty.a) {
                    Differ::Same => {}
                    Differ::Prefix(longer) => return Ok(longer),
                    Differ::Parts(pa, pb) => {
                        let (na, nb) = (self.norm(pa), self.norm(pb));
                        if na != nb {
                            return Ok(if na > nb { 1 } else { -1 });
                        }
                        stack.push((pa, pb))?;
                    }
                },
                Node::Group { .. } if tx.a != NONE && ty.a != NONE => {
                    stack.push((tx.a, ty.a))?;
                }
                Node::Alt { .. } => {
                    if tx.a != ty.a {
                        return Ok(if tx.a < ty.a { 1 } else { -1 });
                    }
                    stack.push((tx.b, ty.b))?;
                }
                _ => {}
            }
        }
        Ok(0)
    }

    /// The groups of parse `tree` into `pm`, as regexec reports them: each
    /// its last match, those inside a group only within its.
    fn report(&mut self, tree: u32, pm: &mut [(isize, isize)]) -> Result<(), NoMem> {
        let mut stack: List<u32> = List::new();
        // As in dissect.rs: a group met again clears the groups inside it
        // first; met once, it has nothing inside to clear yet.
        let mut seen = List::filled(pm.len(), false)?;
        let mut parts: List<u32> = List::new();
        stack.push(tree)?;
        while let Some(t) = stack.pop() {
            let tn = self.t(t);
            match self.p.node(tn.node) {
                Node::Group { index, .. } => {
                    let info = self.p.info(tn.node);
                    if let Some(s) = seen.get_mut(index as usize) {
                        if *s {
                            for g in index.wrapping_add(1)..=info.max_group {
                                if let Some(slot) = pm.get_mut(g as usize) {
                                    *slot = (-1, -1);
                                }
                            }
                        }
                        *s = true;
                    }
                    if let Some(slot) = pm.get_mut(index as usize) {
                        *slot = (
                            (tn.start as usize).cast_signed(),
                            (tn.end as usize).cast_signed(),
                        );
                    }
                    if tn.a != NONE {
                        stack.push(tn.a)?;
                    }
                }
                Node::Cat { .. } | Node::Rep { .. } => {
                    // The list's parts, gathered last first up the trie, go
                    // on the stack last first too, so the first comes off
                    // first.
                    parts.clear();
                    let mut v = tn.a;
                    while v != ROOT && v != NONE {
                        let x = self.pre(v);
                        parts.push(x.part)?;
                        v = x.parent;
                    }
                    for &part in parts.iter() {
                        stack.push(part)?;
                    }
                }
                Node::Alt { .. } => stack.push(tn.b)?,
                _ => {}
            }
        }
        Ok(())
    }
}
