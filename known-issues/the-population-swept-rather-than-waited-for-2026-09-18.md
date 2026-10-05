## The population, swept rather than waited for (2026-09-18)

**Two of these were found by a test failing. The rest were looked for.** The
query is: which test binaries both WRITE the environment (any call to
`settingsfile::testing`) and READ it (`env::var_os`, `env::temp_dir`, or
`ScratchDir::new`, which calls `temp_dir` internally)? Five crates write it:

| crate | writers | readers | guards | state |
|---|---|---|---|---|
| `gui/desktop` | 59 | 6 | 8 | fixed tonight |
| `apps/settings` | 16 | 2 | 9 | already guarded |
| `gui/window` | 2 | 0 | 1 | no readers |
| `apps/fileassoc` | 1 | 3 | 0 | **fixed tonight** -- three `ScratchDir::new` calls with no guard at all |
| `apps/explorer` | 34 | 41 | 1 | **fully fixed 2026-09-18** -- see below |

**`apps/explorer` is now done (2026-09-18).** All 38 scratch-directory
creations across `main.rs`, `columns.rs`, `drives.rs`, `dropzone.rs`,
`fileops.rs` and `search.rs` are routed through one `#[cfg(test)]
pub(crate) guarded_scratch` in the crate root, which takes `config_turn()`
before calling `ScratchDir::new`. **One place, so the lock cannot be omitted by
a new test that copies an old one** -- which is how 35 of the 38 came to lack it
in the first place. Three now-unused `ScratchDir` imports were removed. 427
tests.

The paragraph below is what it said while the work was outstanding, kept
because the reasoning for stopping was sound and the note did its job:

**`apps/explorer` was left partly done, deliberately.** The failure that was
actually observed goes through `dir_of` -> `temp_dir`, which now holds the
lock, so the reproduced case is covered. **35 further `ScratchDir::new` calls
in `columns.rs`, `drives.rs`, `dropzone.rs`, `fileops.rs` and `search.rs`
bypass that helper** and are exposed to the same race. They want routing
through one guarded helper -- `#[cfg(test)] pub(crate)` in the crate root, so
the module test files can reach it -- which is a 38-site mechanical change
across six files, and not something to do in the minutes before a merge with
another lane mid-boot. It is a focused change with a clear shape, and this is
the note that says so rather than a silence that implies the work is finished.

**One mistake worth keeping from the `fileassoc` fix:** the helper was inserted
*before* the call sites were rewritten, so the rewrite caught the helper's own
call and made it call itself. `warning: function cannot return without
recursing` is a good compiler message and it cost a minute -- but the general
form is worth naming, because it will recur in any insert-and-rewrite edit:
**rewrite the call sites first, then add the definition.** A definition added
first is indistinguishable from a call site to a textual replace.

**The wider point, which applies beyond this test.** `std::env::set_var` is
`unsafe` in Rust 2024 precisely because the environment is process-global and
tests are threads. Any test that reads an environment variable is racing every
test that writes one, in the same binary, whether or not either knows about
the other -- and the failure surfaces as a wrong *value*, not a crash, so it
reads as a logic bug in whatever happened to be looking. **A lock only works
when both sides take it**, and the side that merely reads is the one that will
forget.
