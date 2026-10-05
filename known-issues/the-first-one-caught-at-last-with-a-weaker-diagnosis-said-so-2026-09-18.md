## The first one, caught at last -- with a weaker diagnosis, said so (2026-09-18)

**The original flake reproduced**, in the same workspace run discipline that
caught the second: `apps/explorer`, `426 passed; 1 failed`, which is this
crate's suite and matches the `424 passed; 1 failed` recorded above at its size
then. The failing test is
`the_icon_grid_wraps_and_survives_a_pane_narrower_than_a_cell`, panicking on
`text_y(&tree, "a.txt").expect("a")` -- the render did not contain a file the
test had just written.

**Same class as the second, and this crate is exposed the same way:** five
tests here call `settingsfile::testing::with_scratch_config`, which
`remove_var("HOME")`s and `set_var`s `XDG_CONFIG_HOME` under `ENV_LOCK`, while
`ScratchDir::new` -- reached by every scratch-using test through the local
`temp_dir` helper -- calls `std::env::temp_dir()`, which **reads** the
environment without taking that lock.

**This diagnosis is weaker than the second one and should be read as such.**
For `gui/desktop` the chain was complete and checkable: `HOME` absent gives two
icons instead of four, which is exactly the assertion that failed. Here the
chain is *plausible but unproven* -- a scratch directory resolved while another
thread rewrites the environment block could land somewhere other than where the
files were written, which would produce exactly this symptom, but **it has not
been observed doing so.** `std::env::set_var` is `unsafe` in Rust 2024 because
concurrent read-and-write of the environment is undefined, so "undefined" is
the honest description of the mechanism rather than a specific wrong value.

**The fix is right regardless of whether that is the mechanism**, which is why
it was applied: a reader of the environment in a binary that also writes it
should hold the same lock, and `temp_dir` is the one place every scratch-using
test in this crate passes through. If the flake recurs after this, the
hypothesis is wrong and the log will say so -- which is the point of keeping
them.
