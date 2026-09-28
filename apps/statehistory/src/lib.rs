//! An undo history of whole states, kept as the toolkit's tree.
//!
//! [`guitk::undo::UndoHistory`] keeps *steps*: a change the program knows
//! how to apply and to take back. Three programs keep *states* instead -- a
//! copy of the whole picture, diagram or deck, taken just before each edit
//! (`apps/paint`, `apps/diagram`, `apps/slides`). Their stacks threw the
//! undone states away at the next edit; the operator asked for a redo tree
//! wherever something can be undone (C-Q24, `design-decisions.md` §1416).
//!
//! # How
//!
//! Each edit is kept as a step whose ends are two states: the one before it
//! and the one after. Undoing a step is going to its `before`, redoing it is
//! going to its `after`, and the tree does the rest -- branches, and walking
//! every state in the order each was reached ([`StateHistory::earlier`],
//! [`StateHistory::later`]).
//!
//! **The state after one edit is the state before the next**, so it is held
//! once, in an `Rc` both steps share: the history costs a copy per edit, as
//! the stacks did, not two.
//!
//! # What a program does
//!
//! What it did for its stack: say so before each edit begins, with the state
//! it is in ([`StateHistory::begin`]). The state *after* an edit is known
//! only when the next begins, or when the history is walked -- which is why
//! [`undo`](StateHistory::undo), [`redo`](StateHistory::redo) and the
//! journeys take the state the program is in, as the stacks' `undo` and
//! `redo` did.
//!
//! Each returns the state to put in place, or `None` when there is nowhere
//! to go; the program replaces what it holds with it.

use core::num::NonZeroUsize;
use std::rc::Rc;

use guitk::undo::{Travel, UndoHistory};

/// One edit: the state before it and the state after it.
#[derive(Debug)]
struct Step<S> {
    before: Rc<S>,
    after: Rc<S>,
}

/// Cloned by its two `Rc`s: the tree clones a step to hand it back, and the
/// states themselves are never copied for that.
impl<S> Clone for Step<S> {
    fn clone(&self) -> Self {
        Self {
            before: Rc::clone(&self.before),
            after: Rc::clone(&self.after),
        }
    }
}

/// An undo history of whole states, kept as a tree. See the crate
/// documentation.
#[derive(Debug)]
pub struct StateHistory<S> {
    tree: UndoHistory<Step<S>>,
    /// The state before the edit in progress. It becomes a step -- with the
    /// state after the edit -- when the next edit begins, or when the history
    /// is walked.
    open: Option<Rc<S>>,
    /// The state the program is in, when the history put it there: after an
    /// undo, a redo or a journey, until the next edit begins. The next step's
    /// `before` is then this `Rc` itself rather than a second copy of it.
    known: Option<Rc<S>>,
}

impl<S: Clone> StateHistory<S> {
    /// An empty history keeping at most `limit` edits.
    #[must_use]
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            tree: UndoHistory::new(limit),
            open: None,
            known: None,
        }
    }

    /// An edit is about to begin, from `now`: what the program holds before
    /// it. The edit before this one, if there was one, becomes a step here,
    /// with `now` as the state it left.
    pub fn begin(&mut self, now: S) {
        let now = self.known.take().unwrap_or_else(|| Rc::new(now));
        if let Some(before) = self.open.replace(Rc::clone(&now)) {
            self.tree.record(Step { before, after: now });
        }
    }

    /// Whether there is an edit to take back: the one in progress, or one
    /// the history holds.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.open.is_some() || self.tree.can_undo()
    }

    /// Whether there is an undone edit to put back, on the branch the
    /// program is on. Never while an edit is in progress: it goes on the
    /// history first, as the newest step of its own branch.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.open.is_none() && self.tree.can_redo()
    }

    /// Take the last edit back: the state to put in place, or `None`. `now`
    /// is the state the program is in.
    pub fn undo(&mut self, now: S) -> Option<S> {
        self.close(now);
        let step = self.tree.undo()?;
        Some(self.arrive(step.before))
    }

    /// Put the last undone edit back, on the branch the program is on: the
    /// state to put in place, or `None`. `now` is the state the program is
    /// in.
    pub fn redo(&mut self, now: S) -> Option<S> {
        self.close(now);
        let step = self.tree.redo()?;
        Some(self.arrive(step.after))
    }

    /// Go to the state the program was in before this one was first reached,
    /// on whichever branch -- Alt+Z: the state to put in place, or `None` at
    /// the first. `now` is the state the program is in.
    pub fn earlier(&mut self, now: S) -> Option<S> {
        self.close(now);
        let steps = self.tree.earlier();
        self.journey(steps)
    }

    /// Go to the state first reached after this one, on whichever branch --
    /// Alt+Shift+Z: the state to put in place, or `None` at the newest. `now`
    /// is the state the program is in.
    pub fn later(&mut self, now: S) -> Option<S> {
        self.close(now);
        let steps = self.tree.later();
        self.journey(steps)
    }

    /// Forget everything: a new document, or one opened.
    pub fn clear(&mut self) {
        self.tree.clear();
        self.open = None;
        self.known = None;
    }

    /// The edit in progress, if there is one, becomes a step: `now` is the
    /// state it left.
    fn close(&mut self, now: S) {
        if let Some(before) = self.open.take() {
            let after = Rc::new(now);
            self.tree.record(Step {
                before,
                after: Rc::clone(&after),
            });
            self.known = Some(after);
        }
    }

    /// Where a journey ends: each state it passes through is whole, so only
    /// the last needs putting in place.
    fn journey(&mut self, steps: Vec<Travel<Step<S>>>) -> Option<S> {
        let last = match steps.into_iter().last()? {
            Travel::Undo(step) => step.before,
            Travel::Redo(step) => step.after,
        };
        Some(self.arrive(last))
    }

    /// The program is going to `state`: it is the one the history knows.
    fn arrive(&mut self, state: Rc<S>) -> S {
        let owned = (*state).clone();
        self.known = Some(state);
        owned
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// A "program" whose whole state is a string, driving a history as the
    /// three programs do.
    struct Doc {
        text: String,
        history: StateHistory<String>,
    }

    impl Doc {
        fn new() -> Self {
            Self {
                text: String::new(),
                history: StateHistory::new(NonZeroUsize::new(100).unwrap()),
            }
        }

        fn edit(&mut self, to: &str) {
            self.history.begin(self.text.clone());
            self.text = to.to_string();
        }

        fn undo(&mut self) -> bool {
            self.go(StateHistory::undo)
        }

        fn redo(&mut self) -> bool {
            self.go(StateHistory::redo)
        }

        fn earlier(&mut self) -> bool {
            self.go(StateHistory::earlier)
        }

        fn later(&mut self) -> bool {
            self.go(StateHistory::later)
        }

        fn go(&mut self, how: fn(&mut StateHistory<String>, String) -> Option<String>) -> bool {
            match how(&mut self.history, self.text.clone()) {
                Some(state) => {
                    self.text = state;
                    true
                }
                None => false,
            }
        }
    }

    #[test]
    fn undo_and_redo_go_back_and_forth_along_the_line() {
        let mut doc = Doc::new();
        doc.edit("a");
        doc.edit("ab");
        assert!(doc.undo());
        assert_eq!(doc.text, "a");
        assert!(doc.undo());
        assert_eq!(doc.text, "");
        assert!(!doc.undo(), "before the first edit");
        assert!(doc.redo());
        assert!(doc.redo());
        assert_eq!(doc.text, "ab");
        assert!(!doc.redo(), "past the newest");
    }

    /// The edit in progress -- begun, and not yet followed by another -- is
    /// the first undo takes back.
    #[test]
    fn the_edit_in_progress_is_the_first_undone() {
        let mut doc = Doc::new();
        assert!(!doc.history.can_undo());
        doc.edit("a");
        assert!(doc.history.can_undo());
        assert!(!doc.history.can_redo());
        assert!(doc.undo());
        assert_eq!(doc.text, "");
        assert!(doc.history.can_redo());
        assert!(doc.redo());
        assert_eq!(doc.text, "a");
    }

    /// **An edit after an undo starts a branch, and the undone one is kept**
    /// -- redo follows the new branch, and the journeys reach the old one in
    /// the order the states were made.
    #[test]
    fn an_edit_after_an_undo_keeps_the_undone_branch() {
        let mut doc = Doc::new();
        doc.edit("a");
        doc.edit("ab");
        doc.undo();
        doc.edit("ac");
        assert!(
            !doc.history.can_redo(),
            "redo would go onto the branch left"
        );
        assert!(!doc.redo());
        assert_eq!(doc.text, "ac");
        assert!(doc.earlier());
        assert_eq!(doc.text, "ab", "the undone branch was lost");
        assert!(doc.earlier());
        assert_eq!(doc.text, "a");
        assert!(doc.earlier());
        assert_eq!(doc.text, "");
        assert!(!doc.earlier(), "before the first state");
        for want in ["a", "ab", "ac"] {
            assert!(doc.later());
            assert_eq!(doc.text, want);
        }
        assert!(!doc.later(), "past the newest state");
    }

    /// Undo after a journey follows the branch the journey arrived on.
    #[test]
    fn undo_after_a_journey_follows_the_branch_arrived_on() {
        let mut doc = Doc::new();
        doc.edit("a");
        doc.edit("ab");
        doc.undo();
        doc.edit("ac");
        doc.earlier();
        assert_eq!(doc.text, "ab");
        assert!(doc.undo());
        assert_eq!(doc.text, "a");
        assert!(doc.redo());
        assert_eq!(doc.text, "ab", "redo left the branch it came up");
    }

    /// **The state after one edit is the state before the next, held once.**
    #[test]
    fn each_state_is_held_once() {
        let mut history = StateHistory::new(NonZeroUsize::new(10).unwrap());
        history.begin(String::from("0"));
        history.begin(String::from("1"));
        history.begin(String::from("2"));
        let first = history.tree.undo().expect("a step");
        let second = history.tree.undo().expect("a step");
        assert!(
            Rc::ptr_eq(&second.after, &first.before),
            "a state was copied"
        );
        // And the state an undo arrives at is the next edit's `before`
        // itself, not a copy of it.
        let mut history = StateHistory::new(NonZeroUsize::new(10).unwrap());
        history.begin(String::from("0"));
        let back = history.undo(String::from("1")).expect("undone");
        assert_eq!(back, "0");
        history.begin(back);
        let open = history.open.clone().expect("an edit is open");
        let undone = history.tree.redo().expect("the undone step");
        assert!(
            Rc::ptr_eq(&open, &undone.before),
            "the state an undo arrived at was copied"
        );
    }

    /// Clearing forgets the edit in progress too.
    #[test]
    fn clearing_forgets_everything() {
        let mut doc = Doc::new();
        doc.edit("a");
        doc.edit("ab");
        doc.history.clear();
        assert!(!doc.history.can_undo());
        assert!(!doc.undo());
        assert_eq!(doc.text, "ab");
    }

    /// Past the limit the oldest edits go.
    #[test]
    fn past_the_limit_the_oldest_edits_go() {
        let mut history = StateHistory::new(NonZeroUsize::new(3).unwrap());
        let mut text = String::new();
        for c in ['a', 'b', 'c', 'd', 'e'] {
            history.begin(text.clone());
            text.push(c);
        }
        let mut undone = 0;
        while let Some(state) = history.undo(text.clone()) {
            text = state;
            undone += 1;
        }
        assert_eq!(undone, 3);
        assert_eq!(
            text, "ab",
            "the history forgot the newest rather than the oldest"
        );
    }
}
