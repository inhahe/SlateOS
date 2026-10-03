## B-AN-EXIT-STATUS-OF-128-TO-255-IS-REPORTED-AS-A-SIGNAL-DEATH (lane B, 2026-10-02; the fix is lanes A's and D's)

**Status:** OPEN -- lanes A's and D's to fix; asked in `requests/b-ad-an-exit-status-of-128-to-255-is-reported-as-a-signal-death.md`.

**In short:** on SlateOS, a program that ends with `exit (N)` for N from 128
to 255 is reported to its parent as something else. `exit (128)` arrives as a
clean exit with status **0** -- success -- `exit (130)` as a death by `SIGINT`,
and `exit (255)` as a *stopped* process. `git` exits 128 on every fatal error,
so a script that checks `git`'s status would carry on after a failure; `ssh`
exits 255 when it cannot connect.

**Where.** `kernel/src/proc/pcb.rs`, `ExitInfo::to_wstatus`, which every wait
path of both ABIs uses. The kernel keeps one number per dead process, and its
convention is that a death by signal N is recorded as `128 + N`; so it reads
any number from 128 to 255 back as a death, `(code - 128) & 0x7f` in the low
bits of the status word. A normal exit with such a number is indistinguishable
from a death, and is reported as one -- 128 becomes signal 0, which is the
status word of a clean exit, and 255 becomes `0x7f`, the word for "stopped".

The library depends on the convention: `posix/src/signal.rs`'s
`apply_default_action` carries out a signal's "terminate" default by calling
`_exit (128 + sig)`, which is how every native process dies of a signal, since
every native process has a signal trampoline.

**Found** reading the wait status the way GNU `timeout` does -- all four C
macros -- for its port. Read from the source, not yet measured on a boot. On
SlateOS today `timeout 5 sh -c 'exit 255'` would print `timeout: unknown status
from command (127)` and exit 1, and `timeout 5 sh -c 'exit 200'` would take the
command for one killed by signal 72.

**The proper fix.** Record a death apart from an exit -- one number cannot
hold both, which is what POSIX's status word is for. Lane A: a field for "ended
by signal N", set by every kernel path that kills a process for a signal, a
native call by which a process ends *itself* by a signal (as
`SYS_SIGNAL_STOP_SELF` exists for stops), and `to_wstatus` encoding an exit
always as an exit. Lane D: `apply_default_action` and `abort` end the process
through that call instead of `_exit (128 + sig)`. Asked in
`requests/b-ad-an-exit-status-of-128-to-255-is-reported-as-a-signal-death.md`.
