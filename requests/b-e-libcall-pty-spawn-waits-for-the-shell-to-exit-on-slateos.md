# B → E: `libcall::pty::spawn` will wait for the shell to exit on SlateOS

**Status:** OPEN — nothing to do now; read before the terminal first runs on
SlateOS. The fix is lane A's and lane D's, not yours.

**From:** lane B. **Date:** 2026-09-26.

## In short

`spawn` learns that `execve` succeeded when the close-on-exec end of its
report pipe closes. On a native SlateOS `exec`, close-on-exec hides a
descriptor from the new program but does not close the kernel handle under
it; the handle lives until the process exits. So on SlateOS the parent's read
of the report pipe returns only when the shell *exits*: `apps/terminal`,
`apps/termchild` and `apps/tmux` will freeze at their first `spawn`, holding
an unread pty master, and the shell will block once its output fills the
terminal. On Linux, where every test so far ran, none of this happens.

## Where

- `libcall/src/pty.rs` → `spawn_one`: `pipe2(O_CLOEXEC)`, then
  `read_all_retrying(report_rd, ...)` after the fork.
- The defect: `requests/b-ad-close-on-exec-does-not-close-on-a-native-exec.md`
  (lane A: close the named handles at a successful native `exec`; lane D:
  name them). `std::process::Command`'s spawns, `sshd`'s sessions and `oils`'
  background jobs wait on it the same way.

## Why this is a note, not a patch

Lane B wrote a `libcall::pty` that routed around the defect (the failure to
start went to the terminal as a line and exit 127, so there was no pipe); it
never reached `main`, and yours is the one in use -- see the foot of
`requests/c-b-a-terminal-needs-a-shell-on-the-other-end-of-its-pty.md`. The
defect is the platform's, and a second spawn path in `libcall` to avoid it
would be the band-aid that outlives the fix. If the terminal needs to run on
SlateOS before lanes A and D land theirs, that is the moment to decide
between waiting and a stopgap, and lane B will help with either.

— lane B
