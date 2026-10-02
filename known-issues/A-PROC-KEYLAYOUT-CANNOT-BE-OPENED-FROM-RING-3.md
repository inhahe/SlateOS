## A-PROC-KEYLAYOUT-CANNOT-BE-OPENED-FROM-RING-3 (lane A, 2026-09-16) — **Status: WITHDRAWN**, the fault was in my rung

`ctest-keylayout` exited **1**: cannot open `/proc/keylayout`. Checked rather than assumed -- `keylayout` IS in `procfs::ROOT_FILES` (top-level `/proc`), `generate()` serves it, `/proc` is mounted rw, and kernel-side self-tests read `/proc/version` and `/proc/sys/kernel/*` successfully in the same boot. So the failure is specific to a **ring-3 open**, not to the node's existence or the mount.

**WITHDRAWN. `/proc/keylayout` is fine; my rung spawned the fixture
holding nothing.** `self_test_ctest_keylayout` passed
`capabilities: &[]` with `parent: 0`, so the process is not a fork of
init and holds no capability at all. Without `(File, READ)` it cannot
open anything, which is precisely exit 1. Fixed by granting
`(File, 0, READ)` and `(Process, 0, SET_KEYLAYOUT)`, modelled on
`self_test_ctest_hostname` -- a rung three hundred lines away whose
docstring says it exists to make a grant exist, for the identical reason.

**Why this entry is corrected rather than deleted.** The paragraph above
listed five verifications and described them as "checked rather than
assumed". All five were true: `keylayout` is in `ROOT_FILES`, the parser
maps it to `RootFile`, `generate()` serves it, `/proc` is mounted rw, and
kernel-side self-tests read `/proc/version` and `/proc/sys/kernel/*` in
the same boot. **None of them was about the thing that failed.** Every one
was about procfs; the failure was about the caller.

Lane B's framing of the distinction is the part worth keeping, because it
says what to do: their `awk` near-miss was four confirmations sharing one
blind *spot*, and a blind spot is a gap you can go looking for -- the
defence is more checks. This was five confirmations sharing one *subject*,
and **there is no number of procfs checks that finds a capability bug**. A
wrong subject does not look like a gap; it looks like thoroughness. So the
question when several checks agree is not "is there another one" but **are
they all about the same subject, and is that the subject that failed.**

It also refutes a guess of lane B's that I declined to act on -- that this
shared a cause with `ctest-pty`'s 45, a path working inside the kernel and
not outside. The resemblance was genuine and described both symptoms
exactly. The cause was unrelated. That is a better argument for waiting on
a measurement than any case where the resemblance was weak, because a weak
resemblance is easy to resist.
