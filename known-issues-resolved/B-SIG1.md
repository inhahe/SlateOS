### B-SIG1. dash's `wait` builtin (background-job reap) livelocked: no SIGCHLD on child exit + `rt_sigsuspend` was a stub — FIXED 2026-06-16

**RESOLVED 2026-06-16.** A real glibc `dash` running `/bin/emit > file &
wait` (Path-Z self-test `self_test_linux_real_glibc_shell_bgjob`) hung the
boot thread to a timeout. dash's `wait` builtin uses
`dowait(DOWAIT_BLOCK|DOWAIT_WAITCMD)`, whose `waitproc` computes
`flags = WNOHANG` (because `DOWAIT_WAITCMD` makes `block != DOWAIT_BLOCK`),
then loops `while (!gotsigchld && !pending_sig) sigsuspend(&oldmask)` —
relying on SIGCHLD delivery (its handler sets `gotsigchld`). The
synchronous pipe/loop/cmdsub waits use blocking `waitpid` (flags 0) and
never needed SIGCHLD, which is why those parts passed.

Two kernel gaps caused the livelock, both fixed properly:

1. **SIGCHLD was never posted to the parent on child exit.**
   `kernel/src/proc/thread.rs::on_thread_exit` now posts SIGCHLD to the
   parent when a child becomes a zombie — via the Linux-ABI disposition
   path (`signal::set_pending_info`, delivered by
   `deliver_linux_signal` → `linux_disposition`) for Linux parents, and
   `classify_post_info` for native parents (SIGCHLD's default action is
   ignore, so a no-handler parent correctly drops it). This is distinct
   from the existing `wait4()` waiter wakeups, which target a thread parked
   in `wait4()`, not the signal path.

2. **`sys_rt_sigsuspend` was a stub returning EINTR immediately.** This
   made dash busy-spin (`sigsuspend` → EINTR → re-loop → …), starving the
   boot thread. It is now a real park loop modeled on `sys_pause`
   (`kernel/src/syscall/linux.rs`): it installs the temporary mask, parks
   on the signalfd wait-queue until a signal deliverable under that mask
   arrives, and restores the original mask correctly via a Linux
   `saved_sigmask`/`TIF_RESTORE_SIGMASK` mechanism — `emit_linux_rt_frame`
   writes the saved pre-suspend mask into the handler frame's `uc_sigmask`
   (so `rt_sigreturn` restores it), and the no-handler tail of
   `deliver_linux_signal` restores it directly. The contextless
   (in-kernel, `caller_pid()==None`) case still returns EINTR immediately
   so the existing rt_sigsuspend self-test is unaffected.

**Verify:** boot test reaches `BOOT_OK`; the bgjob self-test logs "read
back 16 bytes == expected, exit 0: OK".
