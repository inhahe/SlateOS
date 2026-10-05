### [E] The mutation harness scored every failure in a submodule's tests as a crash -- 2026-09-25
**Status:** FIXED (lane E, 2026-09-25) -- `scripts/mutation_harness.py`

**In short:** the tool that proves a test suite catches broken code read a
failing test's name only when the test lived in the crate's root module
(`tests::name`). A test in a module the root declares is listed as
`input::tests::name`, and went unread -- so for a crate tested that way, every
mutation looked like a crash and was scored "caught", whatever the tests had
said. A table could pass a crate whose tests caught nothing.

**How it was found.** The text editor's close question is tested in
`apps/editor/src/input.rs`; its first sweep reported all seven rows "caught by a
crash", and its `main.rs` rows were refused as naming "no such test", because
the table check looked for test functions only in the file being mutated.

**The fix, additive:** failures are read under any module path, and a table's
test names are looked up in every `.rs` file beside the mutated one. Tables
that mutate a file other than the crate root: `apps/editor` (now swept),
`apps/email` and `apps/mediaconvert` -- both never swept, so no recorded
result rests on the old reading; their sweeps are in lane E's queue.
