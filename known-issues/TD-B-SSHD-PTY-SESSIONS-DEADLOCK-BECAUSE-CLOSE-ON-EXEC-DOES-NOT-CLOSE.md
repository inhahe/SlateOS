## TD-B-SSHD-PTY-SESSIONS-DEADLOCK-BECAUSE-CLOSE-ON-EXEC-DOES-NOT-CLOSE (lane B, 2026-09-24)

**Status:** OPEN — waiting on the platform fix in
`requests/b-ad-close-on-exec-does-not-close-on-a-native-exec.md` (lanes A, D).

**In short:** an SSH login that asks for a terminal would never reach a
prompt on SlateOS. `sshd` starts the shell with std's `Command` and a
`pre_exec(login_tty)` closure, which makes std fork and then wait for
end-of-file on a close-on-exec pipe to learn that `exec` succeeded. On a native
SlateOS process close-on-exec hides a descriptor from the new program but does
not close its kernel handle, so that end-of-file arrives only when the shell
*exits*. `spawn()` does not return, nothing reads the pty master, and the
shell blocks as soon as its output fills the terminal.

**Where:** `userspace/sshd/src/lib.rs` — `shell_command` (the `pre_exec`) and
`Pty::open`. Found reading the path while writing `libcall::pty` for
`apps/terminal`; never observed, because no boot rung starts an `sshd` session
with a pty (`kernel/src/main.rs` has none).

**A second, smaller defect in the same place:** `openpty` does not set
`FD_CLOEXEC` on the master, and `sshd` does not either, so the login shell
inherits its own terminal's master. When the client disconnects and `sshd`
closes its copy, the master is still open in the shell and the kernel's hangup
(`SIGHUP` to the foreground group) never fires. Marking it close-on-exec would
not help today, for the reason above; once the platform fix lands it becomes
a one-line `fcntl(F_SETFD, FD_CLOEXEC)` after `openpty`, and should be made
then.

**Why not moved onto `libcall::pty::spawn`:** it does not change identity, and `sshd` must: its `Command` carries the user's uid, gid and groups (`authlib::identity`). Nor would it avoid the wait: the `spawn` in the tree (lane E's, design-decisions §1200) reports a failed `execve` through a close-on-exec pipe, as `std` does, so on SlateOS it waits for the child to exit in the same way (`requests/b-e-libcall-pty-spawn-waits-for-the-shell-to-exit-on-slateos.md`). The platform fix is the fix for all of them.

**Fix:** land the request (lane A releases a native `exec`'s close-on-exec
handles; lane D names them), then set `FD_CLOEXEC` on the master in
`Pty::open`, and add a boot rung that opens an `sshd` pty session and reads a
prompt back.
