## TD-B-SCRATCH-PATH-HELPERS-NOT-YET-ON-SCRATCHDIR (lane B, 2026-08-21)

**In short:** Seven more crates build their own throwaway-file paths in their
tests instead of using the shared `scratchdir` crate. None of them is
authentication code, which is why the five that were went first. Two of them can
still collide the way the original bug did.

**Where.** Two shapes, with different exposure:

| Shape | Sites | Can collide |
|---|---|---|
| Clock-derived, pid, no counter | `userspace/oils/src/interp.rs` — lines ~71986, ~72009, ~85697, ~85787, ~85824, ~97026, ~106665 | **Yes**, between threads of one run — the original mechanism, 13% per colliding pair |
| Fixed name, no pid and no counter | `crond2` (`crond2_test_anacron`, `…2`, `…3`, `…_ts`), `du` (`du_test_walk`, `du_test_apparent`, `du_test_exclude`), `firejail` (eight `firejail_test_*`), `wc` (`wc-width-<pid>`) | Not between tests of one run (the names differ), but **yes between two concurrent runs** of the suite — e.g. the workspace gate and a `cargo test -p du` in another window |
| Clock **and** counter | `coreutils/src/bin/touch.rs`, `coreutils/src/bin/realpath.rs`, `oils/src/interp.rs:64490` | No — the counter already makes these correct. They leak on a panicking test, and the clock in the name is dead weight, but neither is a correctness bug |

**`oils` already has the fix and its seven sites simply do not use it.**
`interp.rs:68598` defines a local `ScratchDir` with a local `uniq_name`
(`:68577`) that is *correct*: pid + a process-wide `AtomicU64` sequence, a
`Drop` that removes the tree, and a clock stamp kept only so a leftover
directory's age is readable — never as the uniqueness. It was written for
`TD-OILS-TEST-SCRATCH-NAME-COLLISION`, the same bug under a different name, and
it even uses `create_dir` rather than `create_dir_all` so a collision would fail
loudly instead of silently sharing. The seven sites in the table above are
hand-rolled clock paths that were never converted when the crate fixed itself;
they are not a competing design, they are code the existing design never
reached. An earlier revision of this entry called the local type "a fourth
partial reimplementation" — that was wrong, and it mattered, because it made the
remedy look like a redesign when it is a mechanical retarget.

**Why it wasn't done in the same change.** The five auth crates
(`authlib`, `ftpd`, `sshd`, `doas`, `logind`) are the ones where a fixture
collision produces a *false green over password checking*, which is a different
severity from a flaky `du` test. They were converted, tested and merged first
rather than held behind a mechanical sweep of eight more crates.

**The proper fix.** Add `scratchdir = { path = "../scratchdir" }` to each
crate's `[dev-dependencies]` and replace each helper with
`ScratchDir::new("<crate>_test")` + `dir.path(name)`, deleting the manual
cleanup tails — `Drop` covers the panicking case they never could. For `oils`
the seven sites can move to the local `ScratchDir` that is already there, which
is a smaller change than adding a dependency to a crate that does not need one;
replacing the local type with the shared crate afterwards is a tidy-up, not part
of the fix. See
design-decisions.md §349 for why this is a crate rather than a corrected copy in
each place, and `scratchdir`'s module docs for why the clock cannot be made to
work.

**If never fixed:** `oils` carries the same intermittent-red risk the auth
crates had, in a suite big enough that an occasional unexplained failure will be
attributed to the shell rather than the fixture. The fixed-name group is benign
until someone runs two suites at once, at which point it deletes the other run's
files mid-test.
