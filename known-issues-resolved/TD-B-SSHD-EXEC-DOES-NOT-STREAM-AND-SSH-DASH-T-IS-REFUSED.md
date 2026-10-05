## TD-B-SSHD-EXEC-DOES-NOT-STREAM-AND-SSH-DASH-T-IS-REFUSED — 2026-09-05 — FIXED 2026-09-05 (lane B)

**Fixed.** `exec` streams as the command produces output, `ssh -T` runs a login
shell on plain pipes, subsystems spawn, and `exec`'s stdin is a real pipe, so
`ssh host 'wc -l' < file` counts the file instead of reporting 0. What follows
is the original entry, unchanged; the account of the fix is at the end of it.

**In short:** `ssh host` (a normal interactive login) works. Two narrower things
do not. `ssh host 'some command'` waits for the command to finish before sending
any output, so `ssh host 'tail -f /var/log/x'` prints nothing, ever. And
`ssh -T host` — asking for a shell *without* a terminal, which is how scripts
pipe data through a remote shell — is refused outright. Both are the same
missing piece, and neither loses data or misreports anything; they just cannot
do the thing.

**Where it lives:** `userspace/sshd/src/main.rs` — `run_exec_request`, and the
`shell` arm of `handle_channel_request`.

### Why they are one problem

The session pump (`pump_sessions`) can move bytes between a client and a
*pseudo-terminal*, because a pty is a single fd carrying both directions. A
command without a terminal has three ordinary pipes instead — stdin, stdout,
stderr — which have to be watched together. `run_exec_request` sidesteps that
with `child.wait_with_output()`, the standard-library call that drains stdout
and stderr concurrently and returns when the process exits. That is genuinely
the right primitive for a blocking one-shot; reading two pipes one after another
from a single thread deadlocks the moment the child fills the one not being
read. It is simply not a streaming primitive.

The fix is to teach the pump about pipe-backed sessions as well as pty-backed
ones: poll the child's stdout and stderr alongside the socket, forward what is
ready, and forward client `CHANNEL_DATA` into the child's stdin. `ssh -T` then
becomes the same code path with the user's shell as the command, and `exec`'s
stdin — currently `/dev/null`, which is why `ssh host 'wc -l' < file` reports
0 — becomes a real pipe as a side effect.

*Cost while unfixed:* a command that never exits produces nothing; a command
with very large output is buffered in RAM (`ssh host 'find /'` holds the whole
listing before sending it); `ssh -T host` fails with a refusal the client
reports honestly. Interactive use, which is the common case, is unaffected.

**Trigger:** do this next time sshd is opened, or sooner if something needs
`sftp` — an sftp subsystem is a pipe-backed session with no terminal, so it
needs exactly this machinery and nothing else new.

**If never fixed:** SSH remains excellent for interactive logins and for
commands that finish, and unusable for streaming ones and for `sftp`.

### How it was fixed — 2026-09-05 (lane B)

`wait_with_output()` is gone. A session's standard streams are now a
`SessionIo` — `None`, `Terminal(Pty)`, or `Pipes` — held on the channel, so
"terminal *or* three pipes, never both and never one of each" is a fact the
type system enforces rather than a pair of `Option` fields that could disagree.
The pump that already carried a pty's bytes now carries a pipe-backed session's
in the same pass, so all four session kinds run on one code path:

| Request | Before | After |
|---|---|---|
| `shell` **with** `pty-req` | worked | unchanged |
| `shell` **without** `pty-req` (`ssh -T`) | refused | login shell on three pipes |
| `exec` | buffered until exit, stdin `/dev/null` | streamed, stdin is a real pipe |
| `subsystem` | refused | spawns the configured command on pipes |

Three things the fix had to get right, each recorded in full in
`design-decisions.md` §771:

- **Nothing is buffered on the daemon side.** Each read is capped at the
  channel's `remote_window`, and a zero window reads nothing, so the client's
  back-pressure reaches the remote process through the kernel's pipe buffer
  instead of accumulating in daemon memory. `ssh host 'yes'` from a paused
  client costs one 8 KiB stack buffer, not a growing queue.
- **The descriptors are made non-blocking, or the session is refused.**
  `poll` is not sufficient on the write side: POSIX only promises `POLLOUT`
  means *some* data may be written, so a large payload aimed at a nearly-full
  pipe would block in `write` despite a clean poll — and one blocked write in
  this single-threaded daemon stops every other connection on the machine. On
  SlateOS the flag also selects the syscall: `posix`'s `write` reaches the
  non-blocking `SYS_PIPE_TRY_WRITE` only when `O_NONBLOCK` is set.
- **A session ends at end-of-file, not at the child's exit.** Closing on exit
  races with the child's last write; closing on an empty read is wrong the
  first time a program pauses mid-output. Both output pipes reaching EOF (or
  `EIO` on a pty master) is the only unambiguous signal, and it is what
  licenses the close.

One defect found by review while writing the above and fixed with it: the pump
originally reported "not finished" whenever the send window was closed, which
left a session hanging when a command's final write happened to consume the
last of the window — a client that has everything it asked for has no reason to
send another `WINDOW_ADJUST`. Whether a session is finished is a fact about its
streams; the window only decides whether to *read*.

**Verified:** 164 tests pass on the Windows host and again under the WSL linux
half (`scripts/coreutils-check.sh --only linux --dir userspace/sshd`), which is
the build where `cfg(unix)` is true and therefore the only one that compiles
the descriptor code at all. Thirteen of those tests are new and cover the
stream bookkeeping directly, including a regression test for the zero-window
defect above. Host and linux clippy are both clean.

**Still open in sshd**, tracked separately: `env` requests are answered SUCCESS
and discarded
(`TD-B-SSHD-TELLS-EVERY-CLIENT-ITS-ENVIRONMENT-VARIABLES-WERE-ACCEPTED-AND-THROWS-THEM-AWAY`).
And the `sftp` subsystem now has the machinery it needs, but no server: the
configured `/usr/lib/sftp-server` does not exist in this tree, and
`userspace/sftp` is a *client* that speaks its own protocol over raw TCP.
