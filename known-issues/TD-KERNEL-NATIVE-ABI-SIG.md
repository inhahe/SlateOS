### TD-KERNEL-NATIVE-ABI-SIG_IGN-IS-INVISIBLE-TO-THE-KERNEL. Terminal-access job control cannot honour a native-ABI process's `SIG_IGN`, so a native shell must *block* `SIGTTOU` where bash *ignores* it — 2026-08-12

**Where:** `kernel/src/syscall/handlers.rs::signal_ignored_or_blocked`, used by
`tty_job_control_decide` (the `SIGTTIN`/`SIGTTOU` policy added in
`design-decisions.md` §115). The table it cannot reach is
`kernel/src/syscall/linux.rs::linux_sigaction_table` (private, Linux-ABI only).

**The gap.** POSIX says a background terminal access whose signal is *ignored*
behaves as if the signal had been sent and discarded: a read fails `EIO`, a
write proceeds. `signal_ignored_or_blocked` can answer that from three sources,
and only two are exact for both ABIs:

- the **blocked mask** — kernel-owned, exact for native and Linux alike;
- **no trampoline registered**, in which case the kernel's own default-action
  table *is* the disposition — exact;
- an explicit **`SIG_IGN`**, which lives in `linux.rs`'s per-pid sigaction
  table and is therefore visible only when `pcb::get_abi_mode(pid)` is
  `AbiMode::Linux`.

So a native-ABI process that registers a signal trampoline and then sets
`SIGTTOU` to `SIG_IGN` in its own userspace disposition table is indistinguishable,
from the kernel's side, from one that left it at the default. It gets signalled
where a Linux-ABI process would have been let through.

**Why it is not simply a bug.** `kernel/src/proc/signal.rs` states the standing
architecture: the kernel tracks the pending set, the blocked mask and the
trampoline address; **userspace owns the native per-signal disposition table**.
`SYS_SIGNAL_STOP_SELF` (1062) exists precisely because of that asymmetry —
userspace *reports* an already-resolved disposition instead of asking the
kernel to re-derive one. Inventing a kernel-side native sigaction table solely
to serve this predicate would duplicate state userspace already owns, which is
the exact bug shape §113 and §114 were spent removing.

**Reproduce.** A native-ABI program that is in the background of its
controlling terminal, has a trampoline installed, and has set `SIGTTOU` to
`SIG_IGN`: `tcsetpgrp(0, pgrp)` signals it (or returns `EIO` if its group is
orphaned) instead of proceeding.

**Practical consequence.** A job-control shell written against our native libc
must `sigprocmask(SIG_BLOCK, {SIGTTOU})` around `tcsetpgrp` rather than
`signal(SIGTTOU, SIG_IGN)` the way bash does. `services/ctest-ctty/main.c`
checks 81-86 demonstrate the blocking form, so the limitation is executable
documentation rather than a comment.

**Proper fix.** Give the native ABI a kernel-visible disposition for the
*Ignore* case only — either a native `sigaction` syscall recording into a
per-pid table promoted out of `linux.rs` into an ABI-neutral module, or a
narrow "report this signal as ignored" call shaped like `SYS_SIGNAL_STOP_SELF`.
**Trigger:** when a native-ABI job-control shell is written, or sooner if any
native program needs POSIX-exact ignore semantics. Also tracked in `todo.txt`
item 1a of the controlling-terminal section.
