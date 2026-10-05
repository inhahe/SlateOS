## 771. A pipe-backed SSH session buffers nothing, refuses to run on a descriptor that can block, and ends at end-of-file rather than at the child's exit

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `ssh host 'command'`, `ssh -T host` and every subsystem (`sftp`,
when it exists) run a program with three ordinary pipes attached — its input,
its output, and its error output — instead of a terminal. The daemon has to
carry those three back and forth over one connection while never blocking,
because it serves every session on one thread. Three choices fell out of that,
and each of them has a cheaper wrong answer that looks fine until something
goes slightly wrong.

**What forced the choice.** The previous implementation called
`child.wait_with_output()`: run the command, collect everything it prints, send
it when it exits. That is a correct blocking primitive and a hopeless streaming
one — `ssh host 'tail -f log'` printed nothing, ever, and
`ssh host 'find /'` held the entire listing in daemon memory first. Replacing it
means the pump watches the pipes the same way it already watches a pty master.
A pty is one descriptor carrying both directions; pipes are three descriptors
that must be watched together, and that difference is where all three decisions
below come from.

### 1. The client's window bounds the read; nothing is buffered on this side

Every read from the child is capped at the channel's `remote_window`, and a
window of zero reads *nothing at all*.

| Option | *What changes* |
|---|---|
| **Read only what the window allows** (chosen) | A client that stops reading eventually stops the remote program, which blocks on a full pipe. Daemon memory per session is one 8 KiB stack buffer, always. |
| Read what is available, queue the excess | The remote program never blocks — and the daemon's memory grows without bound whenever a client is slower than the program. One `ssh host 'yes'` from a paused client is an out-of-memory condition. |

The chosen option is not a limitation being accepted; it is the SSH window
doing the job it exists for. Unread bytes stay in the kernel's pipe buffer,
that buffer fills, and the *process* blocks on its next `write`. The
back-pressure travels from the client all the way to the program producing the
output, with no queue anywhere in between. A daemon-side queue would break that
chain at exactly the point where it matters.

### 2. A descriptor that cannot be made non-blocking means the session is refused

`Pipes::take` puts all three descriptors into non-blocking mode with `fcntl`,
and returns `None` — killing the child and answering `CHANNEL_FAILURE` — if any
of them will not go.

| Option | *What changes* |
|---|---|
| **Refuse the session** (chosen) | One client gets an honest "no" for a session that could not be run safely. |
| Run it anyway, blocking | That one session works — until the program stops reading its input, at which point one `write` inside the daemon blocks and *every other connection on the machine* stops until it returns. |

Readiness polling is not a substitute here, and that is the subtle part. POSIX
says `POLLOUT` means *some* data may be written without blocking — not the
whole slice. A 32 KiB channel payload aimed at a pipe with 100 bytes of room
blocks in `write` despite a perfectly clean poll. Only `O_NONBLOCK` makes the
short write short instead of blocking, which is why the flag is a precondition
rather than an optimisation. On SlateOS the flag is load-bearing twice over:
`posix`'s `write` dispatches to the non-blocking `SYS_PIPE_TRY_WRITE` *only*
when `O_NONBLOCK` is set, and to the blocking `SYS_PIPE_WRITE` otherwise.

The corresponding `EAGAIN` from `write` is reported as "zero bytes accepted",
not as an error: the pipe filling up is the normal case this design creates on
purpose, and treating it as a failure would tear down healthy sessions under
exactly the load the design is meant to handle.

### 3. The session is over at end-of-file, not at the child's exit

A channel closes when `output_finished()` is true *and* the child has been
reaped. `output_finished()` means both output pipes reported end-of-file (or,
for a terminal, that the master reported `EIO`) — not that a read happened to
come back empty.

| Option | *What changes* |
|---|---|
| **Close at end-of-file on the streams** (chosen) | Every byte a command wrote is delivered. A command that leaves a background process holding its output pipe keeps the session open until that process exits too. |
| Close when the child exits | `ssh host 'echo hi &'` returns promptly in every case — and a command's last line can be cut off, because the child can exit between the final read and the `try_wait` that notices. |
| Close when a read comes back empty | Simplest, and wrong the first time a program pauses mid-output: an empty read means "nothing at this instant", which is the normal state of an interactive session. |

The middle option's failure is a race, which is the argument against it: it
would work in testing and truncate output in production. The chosen option's
cost — a session held open by a grandchild — is deterministic, visible, and
exactly what OpenSSH does, for the same reason: while a descriptor to the
program's output still exists, output is still reachable, so the session is
honestly not over.

**One consequence worth stating separately:** whether a session is finished is
a fact about its *streams*, and must not be read off the client's window. A
command whose final write consumed the last of the window leaves
`remote_window` at zero permanently, because a client that has everything it
asked for has no reason to send another `WINDOW_ADJUST`. An earlier draft of
the pump reported "not finished" whenever the window was closed, which left
exactly those sessions open until the connection dropped. The zero-window path
now reports the streams' real state and only declines to *read*.

**Cost accepted.** A session whose child spawns a grandchild that inherits
stdout stays open until the grandchild exits — `ssh host 'sleep 60 &'` does not
return immediately. This is the documented OpenSSH behaviour and the honest
one, but it does surprise people, and the workaround (redirect the background
process's output) has to be the user's, not ours.

**Trigger to revisit:** if a kernel-side multi-object wait appears (see §770 and
`TD-B-POLL-SELECT-AND-EPOLL-ARE-A-10MS-SPIN-LOOP-BECAUSE-THERE-IS-NO-KERNEL-WAIT`),
the polling in this pump collapses into it. None of the three decisions above
changes: they are about buffering, blocking and end-of-stream, not about how
readiness is discovered.
