"""Mutation test for statehistory's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The crate keeps an undo history of whole states as the toolkit's tree (C-Q24,
design-decisions §1416): each edit a step from the state before it to the one
after.  The table covers where undo, redo and the journeys land, the edit in
progress, and the one state the edits either side of it share.

What it cannot put back as a row is the fault the crate once had: an edit's
`before` taken from the state an undo had left rather than from the program --
a field that no longer exists.  `a_change_between_an_undo_and_the_next_edit_is_
kept` is that fault's test.

Usage:  python -u apps/statehistory/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

LINE = "undo_and_redo_go_back_and_forth_along_the_line"
OPEN = "the_edit_in_progress_is_the_first_undone"
BRANCH = "an_edit_after_an_undo_keeps_the_undone_branch"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "undo lands after the step",
        "        let step = self.tree.undo()?;\n        Some(Self::arrive(&step.before))",
        "        let step = self.tree.undo()?;\n        Some(Self::arrive(&step.after))",
        [LINE, OPEN],
    ),
    (
        "redo lands before the step",
        "        let step = self.tree.redo()?;\n        Some(Self::arrive(&step.after))",
        "        let step = self.tree.redo()?;\n        Some(Self::arrive(&step.before))",
        [LINE, OPEN],
    ),
    (
        "a journey ends where its first step does",
        "        let last = match steps.into_iter().last()? {",
        "        let last = match steps.into_iter().next()? {",
        [BRANCH],
    ),
    (
        "a journey's step back lands after it",
        "            Travel::Undo(step) => step.before,",
        "            Travel::Undo(step) => step.after,",
        [BRANCH],
    ),
    (
        "undo cannot see the edit in progress",
        "        self.open.is_some() || self.tree.can_undo()",
        "        self.tree.can_undo()",
        [OPEN],
    ),
    (
        "redo is offered while an edit is in progress",
        "        self.open.is_none() && self.tree.can_redo()",
        "        self.tree.can_redo()",
        [BRANCH],
    ),
    (
        "undo loses the edit in progress",
        "    pub fn undo(&mut self, now: S) -> Option<S> {\n        self.close(now);",
        "    pub fn undo(&mut self, now: S) -> Option<S> {\n        drop(now);",
        [OPEN],
    ),
    (
        "clearing keeps the edit in progress",
        "        self.tree.clear();\n        self.open = None;",
        "        self.tree.clear();",
        ["clearing_forgets_everything"],
    ),
    (
        "the state two edits share is copied",
        "        if let Some(before) = self.open.replace(Rc::clone(&now)) {",
        "        if let Some(before) = self.open.replace(Rc::new((*now).clone())) {",
        ["each_state_is_held_once"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "statehistory", timeout=300, only=sys.argv[1:] or None))
