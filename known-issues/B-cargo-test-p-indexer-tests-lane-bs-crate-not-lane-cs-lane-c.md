## `cargo test -p indexer` tests lane B's crate, not lane C's (lane C)

**Status: OPEN 2026-08-15** (lane C, needs a cross-lane decision). Four crates
under `apps/` carry a package name that differs from their directory, because a
crate with the directory's name already existed under `userspace/`:

| directory | package | collides with |
|---|---|---|
| `apps/backup` | `backup-app` | `userspace/backup` |
| `apps/indexer` | `indexer-app` | `userspace/indexer` |
| `apps/sysinfo` | `sysinfo-app` | `userspace/sysinfo` |
| `apps/tmux` | `tmux-app` | `userspace/tmux` |

The hazard is that `-p <dir-name>` is not an error. `cargo build -p indexer`
and `cargo test -p indexer` both succeed, silently building **lane B's**
`userspace/indexer` — a different program. This was hit for real: an entire
edit-build cycle on `apps/indexer/src/main.rs` reported a clean build and
`test result: ok. 0 passed`, and the "0 passed" against a file containing 58
`#[test]` functions was the only visible symptom. A change with no tests of its
own would have produced an unqualified green.

**Detection:** if `cargo test -p X` reports a test count that does not match
`grep -c '#\[test\]'` in the crate you edited, you are testing a different
crate. `cargo test -p X -v 2>&1 | grep 'Running unittests'` prints the path.

**Proper fix** is a cross-lane rename so directory and package agree — but both
halves of each pair are real programs with overlapping purposes
(`userspace/tmux` vs `apps/tmux`), and deciding which survives, or what the
surviving names are, is not lane C's call to make alone. Filed here rather than
acted on; it wants a `requests/c-b-…` once there is a concrete proposal.
