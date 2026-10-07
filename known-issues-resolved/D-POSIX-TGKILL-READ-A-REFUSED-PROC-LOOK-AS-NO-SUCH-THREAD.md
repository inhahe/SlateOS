## D-POSIX-TGKILL-READ-A-REFUSED-PROC-LOOK-AS-NO-SUCH-THREAD — `tgkill` of another process's thread answered `ESRCH` in any process without a File capability, and the pgroup rung went red (lane D, 2026-09-30) — **Status: FIXED 2026-09-30**

**In short:** `tgkill` sends a signal to one thread of one process, and
refuses with "no such thread" when the thread is not that process's. For
another process, the C library checks by looking for
`/proc/<pid>/task/<tid>` -- and a process started without the right to look
at files (a File capability with METADATA rights) is refused that look. The
refusal was read as "no such thread", so such a process could never
`tgkill` a thread of its own child. The boot test's process-groups rung
starts its fixture with no capabilities, and its check 84 failed: the first
boot of commits 119-126 was red.

**Why:** `proc_task_exists` (`posix/src/signal.rs`) returned `false` for
any failing `access`, the capability refusal (`EACCES`) with the rest. The
host tests could not see it: the host has no `/proc/<pid>/task` at all, so
they only checked that a path that cannot exist is not found.

**The fix:** a refusal is not an answer about the thread. `tgkill` then
signals nothing and answers `EPERM` -- or `ESRCH` when
`SYS_PROCESS_IS_READY` says the process is not there at all -- rather than
guessing, since `tgkill` exists so that a thread id reused since the caller
learnt it is not hit. The fixture checks that branch when it may not look
(84-87) and the full checks when it may; 88 checks a missing process either
way. The check belongs in the kernel, capability-free: asked of lane A as
item 5 of `requests/d-a-put-each-signal-s-siginfo-in-the-native-frame.md`,
now with this case.

**Where:** `posix/src/signal.rs` (`tgkill`, `proc_task_exists`);
`services/ctest-pgroup/main.c` (checks 84-88).
