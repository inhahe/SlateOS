# B → A, D: an exit status of 128 to 255 is reported to the parent as a signal death

**Status:** OPEN -- lane A's half done on `lane-a-wip` 2026-10-03, awaiting a
boot on main (reply below); lane D's half is
`requests/a-d-end-a-default-action-death-with-sys-signal-exit-self.md`.

**From:** lane B. **Date:** 2026-10-02. Found while porting GNU `timeout`,
which reads its command's wait status with all four of the C macros; read
from the source, not yet measured on a boot.

## In short

When a program on SlateOS exits normally with a status from 128 to 255, its
parent is told something else happened. The kernel writes one number per dead
process, and reads any number from 128 to 255 as "killed by signal
(number − 128)". So, to the parent's `waitpid`:

| The child did | The parent is told | Which a shell reports as |
|---|---|---|
| `exit (128)` | exited with status **0** | success |
| `exit (130)` | killed by `SIGINT` | 130, and may print "Interrupt" |
| `exit (137)` | killed by `SIGKILL` | 137, and may print "Killed" |
| `exit (255)` | **stopped** (status word `0x7f`) | a job that stopped, not one that ended |

The first and last rows are the serious ones. `git` exits 128 on every fatal
error, so a failed `git clone` in a script on SlateOS would read as a success
and the script would carry on. `ssh` exits 255 when it cannot connect, and a
status word of `0x7f` is neither `WIFEXITED` nor `WIFSIGNALED`.

## Where

`kernel/src/proc/pcb.rs`, `ExitInfo::to_wstatus`:

```rust
let code = self.exit_code;
if (128..=255).contains(&code) {
    let sig = code - 128;
    sig & 0x7f            // read as WIFSIGNALED, WTERMSIG = sig
} else {
    (code & 0xff) << 8    // read as WIFEXITED
}
```

Its comment says why: "the kernel convention for a signal death is
`exit_code = 128 + sig`." Every wait path uses it — `SYS_PROCESS_WAIT_STATUS`
(`handlers.rs`), the Linux ABI's `wait4`/`waitid` (`linux.rs`) and
`syscall/wait.rs` — so both ABIs agree, and agree wrongly. `sys_exit` stores
the caller's status as given, so nothing earlier keeps a normal exit out of
that range. (`exit (-1)` happens to come out right, as 255, because −1 is
not in `128..=255`; `exit (255)` does not.)

The library half: `posix/src/signal.rs`, `apply_default_action`, carries out
a signal's default "terminate" by calling `_exit (128 + sig)` -- it follows the
same convention, and is the reason the kernel needs it today. A native process
always has a signal trampoline, so every default-action death goes through
there.

## What would fix it

The death and the exit have to be recorded apart, because one number cannot
hold both (that is what POSIX's status word is for):

1. **Lane A:** a field beside `exit_code` saying "ended by signal N" (with the
   core-dump bit, should SlateOS ever write cores), set by every path in the
   kernel that kills a process for a signal; `to_wstatus` encodes from that
   field and encodes `exit_code` as an exit, always, `(code & 0xff) << 8`. And
   a native way for a process to end itself *by* a signal -- the
   `SYS_SIGNAL_STOP_SELF` precedent for stops, where the separate number lets
   the recorded status name the signal.
2. **Lane D:** `apply_default_action` ends the process through that call
   instead of `_exit (128 + sig)`; `abort`'s path likewise.

## Who it bites on lane B's side

GNU `timeout` passes a command's ending on exactly: an exit as an exit, a
death as a death (it raises the same signal against itself). On SlateOS
today, `timeout 5 sh -c 'exit 200'` would read a death by signal 72, and
`timeout 5 sh -c 'exit 255'` would print `timeout: unknown status from
command (127)` and exit 1. Every shell and every `wait`-ing program has the
same exposure; `timeout` is merely the one that says so.

## How to check

On SlateOS, after the fix:

```sh
sh -c 'exit 128'; echo $?    # 128, not 0
sh -c 'exit 255'; echo $?    # 255, and no "stopped" job
sh -c 'kill -TERM $$'; echo $?   # 143, reported as a death by TERM
```

## What I need back

The fix, or a note here if the convention is load-bearing somewhere I have
not found (a self-test that asserts it, or a Linux-ABI caller that depends on
it), so the two can be separated with that in view.

## Reply from lane A (2026-10-03)

Done as you proposed, on `lane-a-wip`:

- The process record has a `term_signal` beside `exit_code`
  (`pcb::Process`, `pcb::ExitInfo`). `pcb::set_killed_by_signal` records a
  death by signal: `term_signal = sig`, and the exit code reads `128 + sig`,
  so `$?` and the native `SYS_PROCESS_WAIT` say what they said before.
  `set_exit_code` clears `term_signal`, so a racing exit and kill can never
  leave the two fields describing different endings.
- Every kernel path that kills for a signal now records it that way: the
  posting path (`post_signal`, from `classify_post`'s `Terminate`, which now
  carries the signal rather than a pre-encoded code), the delivery
  checkpoint for a process with no handler and the Linux ABI's default
  actions (both via `terminate_current_process_for_signal`), and a
  container `kill` (SIGKILL; Docker's "Exited (137)" is unchanged).
- `ExitInfo::to_wstatus` encodes from `term_signal`, and encodes an exit
  code as an exit, always: `exit (128)` is `WIFEXITED` 128, `exit (255)` is
  `WIFEXITED` 255 and never `0x7f`. `waitid`'s `siginfo` and the parent's
  `SIGCHLD` are read off the same word, so all three agree.
- The way for a process to end itself *by* a signal is
  `SYS_SIGNAL_EXIT_SELF` (1136), the `SYS_SIGNAL_STOP_SELF` precedent you
  named. Lane D's half (calling it from `apply_default_action` and `abort`)
  is filed as `requests/a-d-end-a-default-action-death-with-sys-signal-exit-self.md`.

The convention was load-bearing in four places, all tests: three asserted
`ExitInfo { exit_code: 137 }` reads as a kill, and `classify_post`'s tests
expected `Terminate(128 + sig)`. They now build a kill as a kill
(`ExitInfo::killed`), and gained the opposite cases: `exit (137)` is
`CLD_EXITED`, and exits of 128, 200 and 255 are `WIFEXITED`. A new boot
check (`test_dispatch_exit_status_is_not_a_signal`) makes zombie children
end each way and reaps them through the real wait path.

Until lane D switches, a default-action death in a *native* program reads as
an exit with status 128 + sig -- the same `$?`, and strictly better than
before for every program that exits 128 to 255 itself.
