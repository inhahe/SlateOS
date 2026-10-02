## §273 — A self-test the build cannot reach is a build failure, and "reachable from the kernel shell" is a third answer, not a pass

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel had 41 test functions that no code anywhere could
call. They compiled, they looked like coverage in commit messages, and not one
of them had ever run — some for three months. We now check, before every build,
that every `self_test` function can actually be reached from the kernel's
startup path, and refuse to build if one cannot. The check reports a third
category too: tests that only run when a human types a command into the kernel
shell. Those are not broken, but they are not coverage either, and calling them
"tested" would be the same lie in a quieter voice.

**What went wrong.** `evdev::self_test` was written and committed without a
caller. One commit later it was wired in, and the very first boot that executed
it failed on a real bug — `SYN_DROPPED`, the marker that tells an input client
"discard your state", was being delivered one record *after* the first stale
event, so a client could latch a key down forever
(`B-A-EVDEV-SYN-DROPPED-ARRIVES-ONE-RECORD-LATE`). The test found a genuine
defect the instant it ran. That prompted the obvious question — how many other
tests are sitting there unrun? — and the answer was 41.

**Why a script and not a code review.** Rust will not help here. An uncalled
`pub fn` in a library crate is not dead code by the compiler's reckoning, since
something outside the crate might call it; `#[allow(dead_code)]` was on several
of them anyway. Nothing in the toolchain distinguishes "exported for callers" from
"exported and forgotten". So the predicate has to be written down explicitly.

**Reachability, not mere mention — this is the part that matters.** The first
version of the audit asked "is this symbol named anywhere in the tree?" and
found 27. A fixpoint reachability analysis rooted at `main.rs` found 300. The
gap is entirely self-tests called *only from other self-tests that are
themselves unreachable*: whole islands of test code that reference each other
and are entered from nowhere. An audit that accepts any mention validates those
islands and reports the tree as almost clean. This is the same error as
treating a strongly-connected component of dead code as live because its
members have inbound edges.

**The third bucket, and why it is not a failure.** Of those 300, 258 turned out
to be reachable from `kshell.rs` — interactive `test` subcommands of the kernel
shell (`taskbar test`, `http test`). They are runnable. The boot test never
runs them. Lumping them in with genuinely dead code would have produced a
failure message that was factually wrong about 86% of what it listed, and a
guard that is wrong about most of its output is a guard that gets `--quiet`-ed
and then deleted. So the check reports three states — *runs at boot*, *manual
only*, *reachable from nothing* — and fails on the third alone. The 258 are
printed as a standing count, which is an honest measure of a real gap without
pretending it is the same gap.

*The alternative considered:* fail on the manual-only ones too, forcing every
kshell test into the boot path. Rejected for now — some are genuinely
interactive (they want a keypress, or a device a boot test has no way to
provide), and a 258-entry failure on day one would have been ignored rather
than fixed. The count being visible is what will make it shrink.

**Why re-exports are followed exactly rather than approximated.** `pub use
dispatch::self_test;` genuinely renames a symbol, and `main.rs` calls it as
`syscall::self_test()`. Without following that, the check reports a
well-wired test as dead. A false negative here costs an unrun test that we
would probably have found anyway; a false positive costs the credibility of the
whole check on its first run. There is exactly one such re-export in the tree,
so following them is cheap and bounded.

**Where the gate lives.** `scripts/boot-test.sh`, immediately after
`check_prerequisites` and *before* the build. It costs milliseconds; the build
costs ten minutes. A script that exits 2 because it could not run at all counts
as a failure, for the standing reason that a check which cannot fire must never
be indistinguishable from a check that passed.

**The allowlist is deliberately hostile to growth.** One entry today
(`hardlockup::self_test_fire`, which forces the watchdog to fire, costing a
~15 s stall on every boot and latching the one-shot NMI dump that a real catch
needs). Each entry must carry a reason that would satisfy someone who did not
write it. A growing allowlist is the signal that the check has stopped being
useful, and should be read that way rather than as a maintenance chore.
