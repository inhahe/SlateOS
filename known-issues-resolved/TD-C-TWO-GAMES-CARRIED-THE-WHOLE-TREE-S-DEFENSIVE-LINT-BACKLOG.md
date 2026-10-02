## TD-C-TWO-GAMES-CARRIED-THE-WHOLE-TREE-S-DEFENSIVE-LINT-BACKLOG -- FIXED 2026-09-13

**Date:** 2026-09-13. **Lane:** C.
**Where:** `apps/match3/src/main.rs` (127) and `apps/pinball/src/main.rs` (64).

**In short:** two of this lane's 144 application crates held **every one** of
the tree's remaining warnings about array indexing that could crash and
arithmetic that could overflow. The other 142 were at zero. Both are now at
zero too, and all 240 of their tests still pass.

**How it was found, which is the part worth keeping.** Not by a sweep -- by
lane A's correction that two different gates share the name `cfg-unix` and
check different populations. The one the pre-push hook runs compiles only the
62 crates that *contain* a `#[cfg(unix)]` block; the one in `boot-test.sh`
lints the whole workspace on a unix target. Everything this lane had verified
that day was `--target x86_64-pc-windows-gnu`, so running the boot test's
actual command was the first look at that population, and 196 warnings fell
out of it.

**What each crate's defect actually was** -- and they were different, which is
why a mechanical sweep would have been the wrong tool:

* **`match3` already had the right accessors.** `get_gem` and `set_gem` existed
  with bounds checks, labelled `(for testing)`, sitting beside **forty** direct
  `self.board[row][col]` subscripts. That arrangement is the defect: a bounds
  test written next to each subscript is a test somebody forgets. They are the
  board's only bounds reasoning now, over `.get()`/`.get_mut()`, with a
  `debug_assert` on writes -- a write off the board is worth failing a test
  over and worth surviving in a released game.
* **`pinball`'s test module was simply missing the `#![allow(...)]` block**
  every other app carries, with the reason attached. That is the whole reason
  its test code was in the tree's total.

**Six sites got real fixes rather than mechanical ones.** `GEM_COLORS[self.index()]`
-- an array subscripted by an enum's discriminant -- became a `match`, so
adding a gem colour now fails to *compile* instead of panicking the first time
that colour is drawn. `m.positions[m.length / 2]` became `get`, because
`length` and `positions.len()` are equal only by construction and the
construction is in another function. The rest of the arithmetic became
saturating, which in every case wrote down a promise the surrounding guard was
already making.

**One warning introduced and removed.** Wrapping both arms of a branch in
`if let Some(ball)` produced a collapsible `else { if .. }`; the two arms
differed only in a sign, so collapsing them left the code better than it
started.

**Five are left in the tree and they are not lane C's to fix.**
`civildate/src/lib.rs` has five in the test oracle its own doc comment
describes -- a `zeller` implementation kept deliberately separate from the
code it grades. The fix is the same `#![allow(...)]` block, and `civildate/`
is outside this lane's globs.
