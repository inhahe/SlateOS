## B-POSIX-PRCTL-REFUSES-PR-SET-DUMPABLE (lane B, 2026-10-02; the fix is lane D's)

**Status:** FIXED 2026-10-05 by lane D -- `prctl` takes `PR_SET_DUMPABLE` and `PR_GET_DUMPABLE` over a flag this library keeps (`posix/src/unistd.rs`, `dumpable`), 1 from the start and again after `exec`, as Linux's is; the kernel's own copy needs a native call, asked in `requests/d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md`. The reply is in `requests/b-d-two-calls-gnu-timeout-makes-are-not-native-yet.md`; on main since `a0b0df297`. This entry was not updated with it.

**In short:** when a command run under `timeout` on SlateOS is killed by a
signal that was not the time limit, `timeout` prints
`timeout: warning: disabling core dumps failed: Invalid argument` and exits
with 128 plus the signal's number. GNU's `timeout` prints nothing and dies of
the same signal itself, so that whatever started it sees a death and not an
exit. The status number is the same either way; the warning is noise.

**Where.** `posix/src/unistd.rs`, `prctl`: the native library takes six
options and answers every other one `EINVAL`, `PR_SET_DUMPABLE` among them.
The kernel keeps the flag (`kernel/src/proc/pcb.rs`, `linux_dumpable`), and
its Linux ABI sets it; the native library has no route to it.

**Why `timeout` asks.** Before it raises the signal against itself, upstream
makes itself non-dumpable, so that it leaves no core file on top of the one
its command left. SlateOS writes no core files at all, so there is nothing to
prevent -- but the call must still succeed, because the caller cannot know
that and asks anyway. `libcall::process::disable_core_dumps` is the call;
`timeout.rs` treats a refusal exactly as upstream does, which is what
produces the warning.

**The proper fix (lane D).** `PR_SET_DUMPABLE` and `PR_GET_DUMPABLE` on the
native path, reaching the kernel's flag. Asked in
`requests/b-d-two-calls-gnu-timeout-makes-are-not-native-yet.md`.

**And then.** Even with the warning gone, the death `timeout` passes on would
reach its parent as an *exit* with status 128+N, because that is how SlateOS
records every death by signal --
`known-issues/B-AN-EXIT-STATUS-OF-128-TO-255-IS-REPORTED-AS-A-SIGNAL-DEATH.md`.
