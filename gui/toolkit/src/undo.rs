//! An undo history that keeps what was undone: a tree, not a line.
//!
//! Not to be confused with [`crate::history`], which is a ring buffer of
//! samples for the graphs that plot one.
//!
//! # Why a tree
//!
//! A straight-line history -- one stack to undo from, one to redo from --
//! throws the redo stack away the moment anything new is done. Undo three steps
//! to look at an earlier version, type one letter, and the three steps are gone
//! for good, with whatever work was in them. The operator asked for a redo
//! *tree* wherever something can be undone (`design-decisions.md` §1416): doing
//! something new after undoing starts a branch beside the old one, and the old
//! one stays where it was, reachable.
//!
//! # The model
//!
//! Every recorded step is a node, and its parent is the step it was done after.
//! The history stands at one node -- the last step in effect -- or at the root,
//! before every step. [`UndoHistory::undo`] moves to the parent; [`UndoHistory::redo`]
//! moves down to a child. Where a node has several children, redo takes the
//! one the user was last on: the branch just undone out of, or the one just
//! made. [`UndoHistory::select_branch`] chooses another.
//!
//! [`UndoHistory::earlier`] and [`UndoHistory::later`] walk every state the document
//! has been in, in the order each was first reached, whichever branch it is on.
//! Two keys reach anything the tree holds without the user having to know its
//! shape -- the model Vim's `g-` and `g+` use. Because two states on different
//! branches are not a single step apart, each returns the whole way there: the
//! steps to take back, then the steps to do again.
//!
//! # Steps, not documents
//!
//! `E` is whatever one of a program's steps is -- a change to a text, a stroke,
//! a moved shape -- and applying or reverting one is the program's business.
//! The history hands back which steps, in which direction, and never touches
//! the document; so it keeps no copies of the document either.
//!
//! # Bounded
//!
//! At most [`UndoHistory::limit`] steps are kept. Past that the oldest go -- first
//! whole branches the user is not on, oldest first, then the oldest step of the
//! line the user is on, which from then on cannot be undone. A history that
//! grew without bound would keep every keystroke of a long session.

use core::num::NonZeroUsize;

/// Which way one step of a journey through the history goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Travel<E> {
    /// Take this step back: revert it.
    Undo(E),
    /// Do this step again: apply it.
    Redo(E),
}

/// A node's place in the arena. Stable for the node's life: pruning frees a
/// slot rather than moving the nodes after it, so an id held across a prune
/// still names the node it named, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NodeId(usize);

/// The root's id: always slot 0, never freed.
const ROOT: NodeId = NodeId(0);

/// One step, where it hangs, and where redo goes from it.
#[derive(Debug)]
struct Node<E> {
    /// The step. `None` only for the root, which is the state before every
    /// step the history still holds.
    edit: Option<E>,
    /// The step this one was done after; `None` for the root.
    parent: Option<NodeId>,
    /// The steps done after this one, oldest branch first.
    children: Vec<NodeId>,
    /// Which child [`UndoHistory::redo`] goes to: an index into `children`.
    redo: usize,
    /// When the state after this step was first reached, counted from 1 in
    /// the order steps were recorded. The root's is 0, the earliest of all.
    seq: u64,
}

/// An undo history shaped as a tree. See the module documentation.
///
/// # The line invariant
///
/// Every step on the line from the root to the step the document is at is its
/// parent's redo branch. [`record`](Self::record) makes the new step its
/// parent's redo branch, [`redo`](Self::redo) only ever walks down redo
/// branches, [`select_branch`](Self::select_branch) changes only the branch
/// below the current step, a journey makes each step it goes down through its
/// parent's redo branch, and pruning keeps the redo index on the same child.
/// Undo and a journey *up* to a fork then need to set nothing: the branch
/// redo takes from there is already the one just left.
#[derive(Debug)]
pub struct UndoHistory<E> {
    /// Every node, by id; `None` for a freed slot.
    nodes: Vec<Option<Node<E>>>,
    /// Freed slots, reused before the arena grows.
    free: Vec<usize>,
    /// The node the document is at.
    current: NodeId,
    /// The next `seq` to hand out.
    next_seq: u64,
    /// How many steps are held -- nodes other than the root.
    len: usize,
    /// The most steps held.
    limit: NonZeroUsize,
}

impl<E: Clone> UndoHistory<E> {
    /// An empty history keeping at most `limit` steps.
    #[must_use]
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            nodes: vec![Some(Node {
                edit: None,
                parent: None,
                children: Vec::new(),
                redo: 0,
                seq: 0,
            })],
            free: Vec::new(),
            current: ROOT,
            next_seq: 1,
            len: 0,
            limit,
        }
    }

    /// The most steps this history keeps.
    #[must_use]
    pub const fn limit(&self) -> NonZeroUsize {
        self.limit
    }

    /// How many steps it holds, on every branch.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether it holds no steps at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Forget every step: the document as it is becomes the root.
    pub fn clear(&mut self) {
        *self = Self::new(self.limit);
    }

    fn node(&self, id: NodeId) -> Option<&Node<E>> {
        self.nodes.get(id.0).and_then(Option::as_ref)
    }

    fn node_mut(&mut self, id: NodeId) -> Option<&mut Node<E>> {
        self.nodes.get_mut(id.0).and_then(Option::as_mut)
    }

    /// The child redo goes to from `id`, if it has one.
    fn redo_child(&self, id: NodeId) -> Option<NodeId> {
        let node = self.node(id)?;
        node.children
            .get(node.redo)
            .or_else(|| node.children.last())
            .copied()
    }

    /// Record a step just taken.
    ///
    /// It hangs after the step the document is at, beside any it has already
    /// been undone out of -- which are kept, not discarded -- and redo from
    /// there goes to it, since it is the branch the user is now on.
    pub fn record(&mut self, edit: E) {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let node = Node {
            edit: Some(edit),
            parent: Some(self.current),
            children: Vec::new(),
            redo: 0,
            seq,
        };
        let id = match self.free.pop() {
            Some(slot) => {
                if let Some(entry) = self.nodes.get_mut(slot) {
                    *entry = Some(node);
                }
                NodeId(slot)
            }
            None => {
                self.nodes.push(Some(node));
                NodeId(self.nodes.len().saturating_sub(1))
            }
        };
        let current = self.current;
        if let Some(parent) = self.node_mut(current) {
            parent.children.push(id);
            parent.redo = parent.children.len().saturating_sub(1);
        }
        self.current = id;
        self.len = self.len.saturating_add(1);
        self.prune();
    }

    /// The step the document is at, to fold the next one into -- a run of
    /// typing kept as one step -- or `None` when that would be wrong.
    ///
    /// Wrong when there is no step (the root) and when the step has children:
    /// a step something was done after is the state those were done *from*,
    /// and changing it would leave them hanging from a state that never
    /// existed.
    pub fn last_mut(&mut self) -> Option<&mut E> {
        let current = self.current;
        let node = self.node_mut(current)?;
        if !node.children.is_empty() {
            return None;
        }
        node.edit.as_mut()
    }

    /// Whether there is a step to take back.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.current != ROOT
    }

    /// Whether there is a step to do again, on some branch.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.redo_child(self.current).is_some()
    }

    /// Take back the step the document is at: the step to revert, or `None`
    /// at the root. Redo from the step before goes back down this branch.
    ///
    /// Nothing to set for that: by the line invariant (see the struct) the
    /// step before already points its redo here. It used to be set here as
    /// well, and removing either that or a journey's own setting left every
    /// test passing -- two mechanisms, each hiding the other's absence. The
    /// invariant is the one a journey up to a fork depends on, so it is the
    /// one kept, and tested on its own.
    pub fn undo(&mut self) -> Option<E> {
        let node = self.node(self.current)?;
        let parent = node.parent?;
        let edit = node.edit.clone()?;
        self.current = parent;
        Some(edit)
    }

    /// Do again the step redo goes to from here: the step to apply, or `None`
    /// when nothing was ever undone from here.
    pub fn redo(&mut self) -> Option<E> {
        let child = self.redo_child(self.current)?;
        let edit = self.node(child)?.edit.clone()?;
        self.current = child;
        Some(edit)
    }

    /// How many branches redo could go down from here -- more than one when
    /// something new was done after undoing.
    #[must_use]
    pub fn branches(&self) -> usize {
        self.node(self.current).map_or(0, |n| n.children.len())
    }

    /// Which of them redo takes, counted from the oldest.
    #[must_use]
    pub fn branch(&self) -> usize {
        self.node(self.current)
            .map_or(0, |n| n.redo.min(n.children.len().saturating_sub(1)))
    }

    /// Make redo take branch `index`, counted from the oldest. Returns whether
    /// there is such a branch.
    pub fn select_branch(&mut self, index: usize) -> bool {
        let current = self.current;
        match self.node_mut(current) {
            Some(node) if index < node.children.len() => {
                node.redo = index;
                true
            }
            _ => false,
        }
    }

    /// Go to the state the document was in just before this one was first
    /// reached, on whichever branch: the steps to take, in order. Empty at the
    /// earliest state held.
    pub fn earlier(&mut self) -> Vec<Travel<E>> {
        let here = self.seq_of(self.current);
        let target = self
            .live()
            .filter(|(_, n)| n.seq < here)
            .max_by_key(|(_, n)| n.seq)
            .map(|(id, _)| id);
        target.map_or_else(Vec::new, |t| self.travel_to(t))
    }

    /// Go to the state first reached just after this one, on whichever
    /// branch: the steps to take, in order. Empty at the latest.
    pub fn later(&mut self) -> Vec<Travel<E>> {
        let here = self.seq_of(self.current);
        let target = self
            .live()
            .filter(|(_, n)| n.seq > here)
            .min_by_key(|(_, n)| n.seq)
            .map(|(id, _)| id);
        target.map_or_else(Vec::new, |t| self.travel_to(t))
    }

    fn seq_of(&self, id: NodeId) -> u64 {
        self.node(id).map_or(0, |n| n.seq)
    }

    /// Every node there is, with its id.
    fn live(&self) -> impl Iterator<Item = (NodeId, &Node<E>)> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|n| (NodeId(i), n)))
    }

    /// `id` and every step above it, from `id` up to the root.
    fn line_up(&self, id: NodeId) -> Vec<NodeId> {
        let mut line = vec![id];
        let mut at = id;
        // Bounded by the node count, so a cycle -- which `record` cannot make,
        // but which a corrupted arena would -- ends rather than hangs.
        for _ in 0..self.nodes.len() {
            match self.node(at).and_then(|n| n.parent) {
                Some(parent) => {
                    line.push(parent);
                    at = parent;
                }
                None => break,
            }
        }
        line
    }

    /// Move to `target`: up from here to the step both lines share, then down
    /// to it. Each step on the way down becomes the branch redo takes, so a
    /// plain redo afterwards carries on down the branch just travelled.
    fn travel_to(&mut self, target: NodeId) -> Vec<Travel<E>> {
        let up = self.line_up(self.current);
        let down = self.line_up(target);
        let Some(meet) = up.iter().copied().find(|id| down.contains(id)) else {
            return Vec::new();
        };
        let mut steps = Vec::new();
        for id in up.iter().copied().take_while(|id| *id != meet) {
            if let Some(edit) = self.node(id).and_then(|n| n.edit.clone()) {
                steps.push(Travel::Undo(edit));
            }
        }
        let descent: Vec<NodeId> = down.iter().copied().take_while(|id| *id != meet).collect();
        for id in descent.iter().rev().copied() {
            if let Some(parent) = self.node(id).and_then(|n| n.parent)
                && let Some(up_node) = self.node_mut(parent)
                && let Some(index) = up_node.children.iter().position(|c| *c == id)
            {
                up_node.redo = index;
            }
            if let Some(edit) = self.node(id).and_then(|n| n.edit.clone()) {
                steps.push(Travel::Redo(edit));
            }
        }
        self.current = target;
        steps
    }

    /// Free `id` and everything below it.
    fn drop_subtree(&mut self, id: NodeId) {
        let mut pending = vec![id];
        while let Some(at) = pending.pop() {
            let Some(node) = self.nodes.get_mut(at.0).and_then(Option::take) else {
                continue;
            };
            pending.extend(node.children);
            self.free.push(at.0);
            self.len = self.len.saturating_sub(1);
        }
    }

    /// Bring the history back within its limit. See the module documentation.
    fn prune(&mut self) {
        while self.len > self.limit.get() {
            let line = self.line_up(self.current);
            // Whole branches off the user's line go first, oldest first: a
            // child of a step on the line that is not itself on it, anywhere
            // along the line, and everything after it.
            let beside = line
                .iter()
                .filter_map(|id| self.node(*id))
                .flat_map(|n| n.children.iter().copied())
                .filter(|c| !line.contains(c))
                .min_by_key(|c| self.seq_of(*c));
            if let Some(branch) = beside {
                let parent = self.node(branch).and_then(|n| n.parent);
                self.drop_subtree(branch);
                if let Some(up) = parent.and_then(|p| self.node_mut(p)) {
                    detach(up, branch);
                }
                continue;
            }
            // Then the oldest step of the user's own line: the root's one
            // remaining child. Its children hang from the root now, and it
            // can no longer be undone -- the state after it is the earliest
            // there is.
            let Some(oldest) = self.node(ROOT).and_then(|n| n.children.first().copied()) else {
                break;
            };
            let Some(node) = self.nodes.get_mut(oldest.0).and_then(Option::take) else {
                break;
            };
            self.free.push(oldest.0);
            self.len = self.len.saturating_sub(1);
            for child in &node.children {
                if let Some(c) = self.node_mut(*child) {
                    c.parent = Some(ROOT);
                }
            }
            if let Some(root) = self.node_mut(ROOT) {
                root.children = node.children;
                root.redo = node.redo;
            }
            if self.current == oldest {
                self.current = ROOT;
            }
        }
    }
}

/// Take `child` out of `node`'s children, keeping redo on the branch it was on
/// -- or, if that was the one taken, on the newest branch left.
fn detach<E>(node: &mut Node<E>, child: NodeId) {
    let Some(at) = node.children.iter().position(|c| *c == child) else {
        return;
    };
    node.children.remove(at);
    node.redo = match node.redo.cmp(&at) {
        core::cmp::Ordering::Greater => node.redo.saturating_sub(1),
        core::cmp::Ordering::Equal => node.children.len().saturating_sub(1),
        core::cmp::Ordering::Less => node.redo,
    };
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// A document the tests edit: a string, and steps that append or remove a
    /// word, so every state can be checked by what it reads.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Push(&'static str);

    fn history(limit: usize) -> UndoHistory<Push> {
        UndoHistory::new(NonZeroUsize::new(limit).unwrap())
    }

    /// The document, as a program keeping it would: apply what the history
    /// hands back.
    #[derive(Default)]
    struct Doc(Vec<&'static str>);

    impl Doc {
        fn apply(&mut self, travel: Travel<Push>) {
            match travel {
                Travel::Redo(Push(word)) => self.0.push(word),
                Travel::Undo(Push(word)) => {
                    assert_eq!(self.0.pop(), Some(word), "reverted a step not in effect");
                }
            }
        }
        fn read(&self) -> String {
            self.0.join(" ")
        }
    }

    fn doing(h: &mut UndoHistory<Push>, doc: &mut Doc, word: &'static str) {
        doc.0.push(word);
        h.record(Push(word));
    }

    fn undoing(h: &mut UndoHistory<Push>, doc: &mut Doc) -> bool {
        h.undo().map(|e| doc.apply(Travel::Undo(e))).is_some()
    }

    fn redoing(h: &mut UndoHistory<Push>, doc: &mut Doc) -> bool {
        h.redo().map(|e| doc.apply(Travel::Redo(e))).is_some()
    }

    #[test]
    fn a_line_of_steps_undoes_and_redoes_like_a_stack() {
        let mut h = history(100);
        let mut doc = Doc::default();
        assert!(!h.can_undo() && !h.can_redo());
        for w in ["a", "b", "c"] {
            doing(&mut h, &mut doc, w);
        }
        assert!(undoing(&mut h, &mut doc));
        assert!(undoing(&mut h, &mut doc));
        assert_eq!(doc.read(), "a");
        assert!(redoing(&mut h, &mut doc));
        assert_eq!(doc.read(), "a b");
        assert!(redoing(&mut h, &mut doc));
        assert!(!redoing(&mut h, &mut doc), "nothing more to redo");
        assert_eq!(doc.read(), "a b c");
        for _ in 0..3 {
            assert!(undoing(&mut h, &mut doc));
        }
        assert!(!undoing(&mut h, &mut doc), "past the first step");
        assert_eq!(doc.read(), "");
    }

    /// The point of the tree: doing something new after undoing keeps what was
    /// undone. A straight-line history would have thrown "b c" away at "x".
    #[test]
    fn doing_something_new_after_undoing_keeps_what_was_undone() {
        let mut h = history(100);
        let mut doc = Doc::default();
        for w in ["a", "b", "c"] {
            doing(&mut h, &mut doc, w);
        }
        undoing(&mut h, &mut doc);
        undoing(&mut h, &mut doc);
        doing(&mut h, &mut doc, "x");
        assert_eq!(doc.read(), "a x");
        assert!(!h.can_redo(), "the new branch has nothing after it yet");

        // Back to "a", where there are two branches now; redo takes the new.
        undoing(&mut h, &mut doc);
        assert_eq!(h.branches(), 2);
        assert_eq!(h.branch(), 1, "the branch just undone out of");
        // Choose the old one, and it is all still there.
        assert!(h.select_branch(0));
        assert!(redoing(&mut h, &mut doc));
        assert!(redoing(&mut h, &mut doc));
        assert_eq!(doc.read(), "a b c");
        assert_eq!(h.len(), 4, "a, b, c and x: nothing was lost");
    }

    /// Redo goes down the branch last undone out of, so undo-undo-redo-redo
    /// comes back to where it started even where the tree forks.
    #[test]
    fn redo_follows_the_branch_last_undone_out_of() {
        let mut h = history(100);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a");
        doing(&mut h, &mut doc, "b");
        undoing(&mut h, &mut doc);
        doing(&mut h, &mut doc, "x");
        // Now onto the old branch, and out of it again.
        undoing(&mut h, &mut doc);
        h.select_branch(0);
        redoing(&mut h, &mut doc);
        assert_eq!(doc.read(), "a b");
        undoing(&mut h, &mut doc);
        assert_eq!(h.branch(), 0, "the branch just left, not the newest");
        redoing(&mut h, &mut doc);
        assert_eq!(doc.read(), "a b");
    }

    #[test]
    fn a_branch_that_does_not_exist_is_not_selected() {
        let mut h = history(100);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a");
        undoing(&mut h, &mut doc);
        assert!(!h.select_branch(1));
        assert_eq!(h.branch(), 0);
    }

    /// Earlier and later walk every state in the order it was first reached,
    /// across branches, and each hands back the whole way there.
    #[test]
    fn earlier_and_later_walk_every_state_in_time_order() {
        let mut h = history(100);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a"); // 1: "a"
        doing(&mut h, &mut doc, "b"); // 2: "a b"
        undoing(&mut h, &mut doc);
        doing(&mut h, &mut doc, "x"); // 3: "a x"
        doing(&mut h, &mut doc, "y"); // 4: "a x y"

        let mut seen = vec![doc.read()];
        loop {
            let steps = h.earlier();
            if steps.is_empty() {
                break;
            }
            for step in steps {
                doc.apply(step);
            }
            seen.push(doc.read());
        }
        assert_eq!(seen, vec!["a x y", "a x", "a b", "a", ""]);

        let mut forth = vec![doc.read()];
        loop {
            let steps = h.later();
            if steps.is_empty() {
                break;
            }
            for step in steps {
                doc.apply(step);
            }
            forth.push(doc.read());
        }
        assert_eq!(forth, vec!["", "a", "a b", "a x", "a x y"]);
    }

    /// Crossing branches is a journey: from "a b" to "a x" is one step back
    /// and one on, in that order.
    #[test]
    fn crossing_a_branch_takes_back_before_it_does_again() {
        let mut h = history(100);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a");
        doing(&mut h, &mut doc, "b");
        undoing(&mut h, &mut doc);
        doing(&mut h, &mut doc, "x");
        // At "a x" (3); earlier is "a b" (2).
        let steps = h.earlier();
        assert_eq!(
            steps,
            vec![Travel::Undo(Push("x")), Travel::Redo(Push("b"))]
        );
        // And a plain redo after arriving goes on down the branch arrived on:
        // there is nothing after "b", so undo then redo returns to "a b".
        for step in steps {
            doc.apply(step);
        }
        undoing(&mut h, &mut doc);
        redoing(&mut h, &mut doc);
        assert_eq!(doc.read(), "a b");
    }

    /// A step folds into the last one only where nothing was done after it.
    #[test]
    fn the_last_step_folds_only_at_the_tip() {
        let mut h = history(100);
        let mut doc = Doc::default();
        assert!(h.last_mut().is_none(), "the root is not a step");
        doing(&mut h, &mut doc, "a");
        assert!(h.last_mut().is_some());
        doing(&mut h, &mut doc, "b");
        undoing(&mut h, &mut doc);
        assert!(
            h.last_mut().is_none(),
            "\"a\" has \"b\" after it; changing it would orphan \"b\""
        );
    }

    /// Past the limit, whole branches beside the user's line go first, oldest
    /// first, and only then the oldest step of the line itself.
    #[test]
    fn past_the_limit_old_branches_go_before_the_line_being_worked_on() {
        let mut h = history(4);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a");
        undoing(&mut h, &mut doc); // back to the root: "a" is a branch
        doing(&mut h, &mut doc, "p");
        doing(&mut h, &mut doc, "q");
        doing(&mut h, &mut doc, "r");
        assert_eq!(h.len(), 4);
        doing(&mut h, &mut doc, "s"); // five steps: "a" goes, not "p"
        assert_eq!(h.len(), 4);
        assert_eq!(doc.read(), "p q r s");
        for _ in 0..4 {
            assert!(undoing(&mut h, &mut doc));
        }
        assert_eq!(doc.read(), "", "the line was kept whole");
        assert_eq!(h.branches(), 1, "\"a\" is gone");

        // Now only the line is left: the next step past the limit costs its
        // oldest step, which can no longer be undone.
        for _ in 0..4 {
            redoing(&mut h, &mut doc);
        }
        doing(&mut h, &mut doc, "t");
        assert_eq!(h.len(), 4);
        let mut undone = 0;
        while undoing(&mut h, &mut doc) {
            undone += 1;
        }
        assert_eq!(undone, 4);
        assert_eq!(doc.read(), "p", "\"p\" can no longer be taken back");
    }

    /// A new branch from the root, past the limit, costs the old branch --
    /// all of it -- and not the new one.
    #[test]
    fn a_new_branch_past_the_limit_costs_the_old_branch() {
        let mut h = history(2);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a");
        doing(&mut h, &mut doc, "b");
        undoing(&mut h, &mut doc); // at "a", "b" below
        undoing(&mut h, &mut doc); // at the root
        doing(&mut h, &mut doc, "x"); // a third step: the branch "a b" goes
        assert_eq!(h.len(), 1);
        assert_eq!(h.branches(), 0);
        assert!(undoing(&mut h, &mut doc));
        assert_eq!(h.branches(), 1, "only \"x\" is left to redo");
    }

    #[test]
    fn clearing_forgets_every_step() {
        let mut h = history(10);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a");
        undoing(&mut h, &mut doc);
        doing(&mut h, &mut doc, "b");
        h.clear();
        assert!(h.is_empty());
        assert!(!h.can_undo() && !h.can_redo());
        assert!(h.earlier().is_empty() && h.later().is_empty());
    }

    /// A branch deep in the tree goes before the oldest step of the line the
    /// user is on: the policy is "whole branches the user is not on first",
    /// wherever along the line they hang, not only at the root.
    #[test]
    fn a_branch_deep_in_the_tree_goes_before_the_lines_oldest_step() {
        let mut h = history(4);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a");
        doing(&mut h, &mut doc, "b");
        doing(&mut h, &mut doc, "old");
        undoing(&mut h, &mut doc); // at "a b", with "old" below
        doing(&mut h, &mut doc, "c"); // four steps
        doing(&mut h, &mut doc, "d"); // five: "old" goes, not "a"
        assert_eq!(h.len(), 4);
        let mut undone = 0;
        while undoing(&mut h, &mut doc) {
            undone += 1;
        }
        assert_eq!(undone, 4, "\"a\" can still be taken back");
        assert_eq!(doc.read(), "");
        // And at "a b" there is only the one branch now.
        redoing(&mut h, &mut doc);
        redoing(&mut h, &mut doc);
        assert_eq!(h.branches(), 1);
    }

    /// Taking a branch out keeps redo on the branch it was on.
    #[test]
    fn detaching_a_branch_keeps_redo_where_it_was() {
        let mut h = history(100);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a");
        undoing(&mut h, &mut doc);
        doing(&mut h, &mut doc, "b");
        undoing(&mut h, &mut doc);
        doing(&mut h, &mut doc, "c");
        undoing(&mut h, &mut doc);
        // At the root: branches a, b, c; redo on c.
        assert!(h.select_branch(1));
        let root = h.node_mut(ROOT).unwrap();
        let first = root.children[0];
        detach(root, first);
        assert_eq!(h.branch(), 0, "still on b, which moved down one");
        redoing(&mut h, &mut doc);
        assert_eq!(doc.read(), "b");
    }

    /// A journey *up* to a fork -- no undo on the way -- leaves redo on the
    /// branch it came from, which only the line invariant provides: the steps
    /// a journey goes up through are not touched.
    #[test]
    fn a_journey_up_to_a_fork_leaves_redo_on_the_branch_it_came_from() {
        let mut h = history(100);
        let mut doc = Doc::default();
        doing(&mut h, &mut doc, "a"); // 1
        undoing(&mut h, &mut doc);
        doing(&mut h, &mut doc, "b"); // 2, beside "a"
        // At "b". Back in time: "a" (1), across the fork.
        for step in h.earlier() {
            doc.apply(step);
        }
        assert_eq!(doc.read(), "a");
        // Back again: the root (0), straight up from "a".
        for step in h.earlier() {
            doc.apply(step);
        }
        assert_eq!(doc.read(), "");
        assert_eq!(h.branch(), 0, "redo is not on the branch just left");
        assert!(redoing(&mut h, &mut doc));
        assert_eq!(doc.read(), "a");
    }

    /// Every operation, in a long random mix, keeps the line invariant and
    /// keeps the document what the history says it is.
    ///
    /// The document is rebuilt from the history after every step -- the words
    /// on the line from the root to where the history stands -- and compared
    /// with the one the handed-back steps built. A wrong step anywhere, a lost
    /// branch or a redo pointing off the line shows up as the two disagreeing.
    #[test]
    fn a_long_random_session_keeps_the_line_and_the_document_true() {
        use randrange::{RandomSource, SeededRng};
        const WORDS: [&str; 5] = ["a", "b", "c", "d", "e"];
        let mut rng = SeededRng::new(0x9E37_79B9_7F4A_7C15);
        let mut h = history(12);
        let mut doc = Doc::default();
        for round in 0..5000 {
            match rng.below(7) {
                0 | 1 => doing(&mut h, &mut doc, WORDS[rng.below(WORDS.len())]),
                2 => {
                    undoing(&mut h, &mut doc);
                }
                3 => {
                    redoing(&mut h, &mut doc);
                }
                4 => {
                    for step in h.earlier() {
                        doc.apply(step);
                    }
                }
                5 => {
                    for step in h.later() {
                        doc.apply(step);
                    }
                }
                _ => {
                    let branches = h.branches();
                    if branches > 0 {
                        h.select_branch(rng.below(branches));
                    }
                }
            }
            // The line, from where the history stands up to the root.
            let line = h.line_up(h.current);
            for pair in line.windows(2) {
                let (child, parent) = (pair[0], pair[1]);
                let node = h.node(parent).unwrap();
                assert_eq!(
                    node.children.get(node.redo),
                    Some(&child),
                    "round {round}: a step on the line is not its parent's redo branch"
                );
            }
            // The document the history implies: every word on the line.
            let words: Vec<&str> = line
                .iter()
                .rev()
                .filter_map(|id| h.node(*id).and_then(|n| n.edit.as_ref()))
                .map(|Push(w)| *w)
                .collect();
            // The oldest steps may have been pruned: what they did stays in the
            // document and can no longer be undone, so the history's words are
            // the document's last ones.
            assert!(
                doc.0.ends_with(&words),
                "round {round}: the document {:?} does not end with the line {words:?}",
                doc.0
            );
            assert!(h.len() <= 12, "round {round}: past the limit");
        }
    }

    /// Freed slots are reused, so a long session at the limit does not grow the
    /// arena without bound.
    #[test]
    fn a_long_session_at_the_limit_reuses_its_slots() {
        let mut h = history(8);
        let mut doc = Doc::default();
        for _ in 0..1000 {
            doing(&mut h, &mut doc, "w");
        }
        assert_eq!(h.len(), 8);
        assert!(h.nodes.len() <= 10, "the arena grew to {}", h.nodes.len());
    }
}
