## TD-B-POSIX-TEST-BINARY-CRASHED-WITH-0xC0000005-ONCE (lane B, 2026-08-21)

**Status:** OPEN — observed once, not reproduced.

**In short:** one run of `cargo test -p posix --target x86_64-pc-windows-gnu`
exited with `0xC0000005` (a Windows access violation — the process read or
wrote memory it does not own) and printed *no test output at all*: not a
`running N tests` banner, not a single dot. An immediate re-run of the exact
same command passed 20,500 tests. Nothing else in the tree changed between the
two runs.

**Why zero output matters.** A crash inside a test would still have printed the
banner and the tests that ran before it. Printing nothing means the process
died before the harness got going — during process start-up, static
initialisation, or while the loader was still mapping the image. That points
away from a bug in any particular test and towards something about the binary
or its environment at launch.

**The untested hypothesis: a concurrent build against the shared `target/`.**
Lane B's worktree has one `target/` directory, and at the time of the crash a
second cargo invocation was running against it. Cargo relinks test binaries in
place; a process launched from a path that is being rewritten underneath it is
exactly the shape of a load-time access violation, and exactly the shape of a
failure that vanishes on re-run. This is a hypothesis, not a diagnosis — it was
not instrumented.

**How to act on it.** If it recurs, capture it under `cdb` rather than
re-running: `cdb -g -G -o cargo test -p posix ...` will stop on the first-chance
AV and give the faulting module and address, which distinguishes "the loader
was reading a half-written image" from "a static initialiser dereferenced
null". Until then, avoid running two cargo commands against the same `target/`
concurrently, which is good practice for unrelated reasons (it also invalidates
the fingerprint cache and forces rebuilds).

**Not a blocker.** The suite is green on every other run, and this has been seen
exactly once. It is logged so that a *second* sighting is recognised as a
pattern rather than re-diagnosed from scratch.
