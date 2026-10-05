## B-POSIX-PRCTL-REFUSES-PR-SET-DUMPABLE (lane B, 2026-10-02; the fix is lane D's)

**Status:** OPEN -- lane D's to fix; asked in `requests/b-d-two-calls-gnu-timeout-makes-are-not-native-yet.md`.

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
