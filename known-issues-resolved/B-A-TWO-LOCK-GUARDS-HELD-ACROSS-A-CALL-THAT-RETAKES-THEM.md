## B-A-TWO-LOCK-GUARDS-HELD-ACROSS-A-CALL-THAT-RETAKES-THEM (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** Same fatal shape as the counter bug — a thread asking for a lock it
already holds — but split across two functions, so no single line mentions the
lock twice and no text search can see it. Found by a new script rather than by
booting into them.

**Found by** `scripts/check-recursive-locks.py`, added here. It parses each file,
builds a same-file call graph, works out which locks each function may acquire
transitively, and reports any guard bound to a named local that is still live
when a call to a re-acquiring function runs. Two findings across 799 files.

**Finding 1 — `ipc/service.rs`, `test_socket_activation`.** Held
`SOCKET_ACTIVATIONS` across `unregister_socket_activation(name)`, which takes the
same lock. Only on the *failure* paths, which is the worst possible placement:
the test would hang the machine precisely when a check had failed, instead of
printing which expectation was wrong. (`channel::close` was called under the
same guard too — a different lock, so an ordering hazard rather than a
self-deadlock.) Fixed by copying the two observed facts out under the lock and
judging them after release.

**Finding 2 — `svcstart.rs`, `boot_services`.** Held `STATE` and called `init()`,
which takes `STATE`. Guarded by `if !state.initialized`, so the normal path never
hit it — `initproc` calls `svcstart::init()` before `boot_services()`. But the
kernel shell's `boot` command (`kshell.rs:45788`) calls `boot_services()`
directly, and that is exactly the caller for which `initialized` is false. Fixed
by hoisting the check into a bound local so the release is a visible statement
rather than an inference about temporary lifetimes.

**Limits of the checker.** Deliberately a within-file heuristic: it resolves
calls only to functions defined in the same file and locks only via ALL-CAPS
static receivers. That covers the module-private `static STATE: Mutex<_>` pattern
this kernel uses everywhere, and needs no import or trait-dispatch resolution. It
has false negatives by construction (a call into another module that reaches back
is invisible). `try_lock()` is never reported — it returns `None` rather than
spinning. Two reports over 799 files, both genuine, so the false-positive rate is
low enough to be worth running.

**Severity.** Finding 2 is a live hang reachable by typing `boot` at the kernel
shell. Finding 1 is a test that converts a legible failure into a freeze.
