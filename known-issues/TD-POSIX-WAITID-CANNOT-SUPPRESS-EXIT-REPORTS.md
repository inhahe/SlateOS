### TD-POSIX-WAITID-CANNOT-SUPPRESS-EXIT-REPORTS. `waitid` without `WEXITED` still reports a child's exit, because the kernel primitive always does — LOGGED 2026-08-16 by lane B

**In short:** POSIX's `waitid()` lets a caller say *which kinds* of news it
wants about its children — "tell me if one exits", "tell me if one is
suspended", "tell me if one resumes" — by setting flags. Ours honours the
suspend and resume flags but cannot switch the *exit* one off: ask only about
suspends, and an exit will still be reported. Nothing we ship asks for that
combination, and the direction of the error is the safe one (a caller is told
more than it asked, never less), but a ported program that relies on the
filter will behave differently here than on Linux.

**Where it lives.** `posix/src/process.rs::waitid_kernel_options` — the
translation from `waitid`'s option word to the kernel's. `WSTOPPED`,
`WCONTINUED` and `WNOWAIT` each have a kernel bit; `WEXITED` has none, because
`SYS_PROCESS_WAIT_STATUS` reports a reapable exit unconditionally (that is what
`waitpid` means) and the two job-control classes are the *additions* selected by
`WUNTRACED`/`WCONTINUED`.

**How it would show.** `waitid(P_PID, child, &info, WSTOPPED)` on a child that
exits instead of stopping: Linux blocks (or returns `ECHILD` once the child is
its last), ours returns the exit as `CLD_EXITED` — **and reaps it**, so the
status is consumed by a call that did not ask for it. That last part is the
reason this is worth an entry rather than a comment: a caller that intended to
reap later has silently lost the exit status.

Note the fixture's check 100 deliberately sets `WSTOPPED | WEXITED` together for
exactly this reason — so a child that died instead of stopping is *reported*
rather than leaving the parent parked. That works today by accident of this bug
and by intent of the flags; it will keep working after a fix.

**Proper fix** (kernel-side, so lane A's): a `WNOEXITED` option bit — or,
better, invert it and make the primitive's exit reporting explicit under a bit
that every existing caller already sets by construction, the way `WPGID` and
`WINFO` were added without disturbing anyone. Not filed as a request yet: no
caller in the tree needs it, and it is a strictly larger change to the wait
primitive than the three that just landed. Promote it to `requests/` the first
time a port actually depends on the filter — glibc's `posix_spawn` and
`pthread_cancel` paths are the likeliest candidates.
