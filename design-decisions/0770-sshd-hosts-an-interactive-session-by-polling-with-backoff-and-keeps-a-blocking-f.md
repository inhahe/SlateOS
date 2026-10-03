## 770. sshd hosts an interactive session by polling with backoff, and keeps a blocking fast path for every connection that has no session

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** an SSH daemon hosting a login shell has to watch two things at
once — the network socket, in case the user typed something, and the terminal
the shell is running on, in case the shell printed something. Neither can be
waited on in a way that also wakes for the other, so the loop has to *ask* each
of them "anything yet?" in turn and sleep a little when both say no. Sleeping
costs latency; not sleeping costs a burned CPU core. The decision is how long
to sleep, and — more importantly — that connections with no shell running keep
blocking properly and pay nothing.

**What forced the choice.** A readiness wait that covers a TCP handle *and* a
pty fd does not exist. The socket is a raw kernel handle (`SYS_TCP_POLL_STATUS`
takes a handle); the terminal is a libc file descriptor, because the only way a
child can adopt a terminal as its *controlling* terminal is `login_tty`, which
is an fd operation. There is no call that turns a raw pty handle into an fd, and
no `poll` that accepts both kinds. So one loop, two readiness primitives, and
nothing to block on that covers both.

**The alternatives:**

| Option | *What changes* |
|---|---|
| **Poll both, sleep on idle** (chosen) | An idle session wakes a few times a second and does nothing. Keystroke latency is one sleep interval in the worst case. |
| A thread per direction, each blocking | No polling at all, no added latency — but the target spec sets `has-thread-local: false`, and a thread blocked in `read` cannot be woken to shut down cleanly when the other direction ends. |
| Spin without sleeping | Zero latency, one CPU core burned per open session. |
| Give the socket a file descriptor and `poll()` both together | One readiness call instead of two — but a *worse* one; see below. |
| Wait for a unified readiness syscall | Correct in the long run, and nothing works until it exists. |

**Why not just put the socket in the fd table and `poll()` both?** It looks like
the obvious fix — `posix` already knows `HandleKind::TcpStream`, so a socket
obtained through libc `accept` is pollable alongside the pty master, and a
single `poll` would cover both. It is the wrong trade *today*, because `posix`'s
`poll` is itself a userspace loop that sleeps 10 ms between checks
(`posix/src/poll.rs`; `select` and `epoll_wait` are the same loop). Routing this
through it would replace a 0.5 ms floor with a fixed 10 ms one — twenty times
the worst-case keystroke latency — while waking no less often, and would on top
of that require rewriting sshd's entire transport onto file descriptors. The
unification is only worth having once the wait underneath it is a real one,
which is the same condition as the revisit trigger below.

**The interval: 0.5 ms doubling to 20 ms.** The floor is set by what a person
can perceive — half a millisecond is far below the threshold where added
keystroke latency is noticeable, so a session that is actively being typed into
feels immediate. The ceiling is set by what an *idle* session should cost: at
20 ms a connection left open overnight wakes 50 times a second, which is
negligible, and 20 ms is still under the ~50 ms at which a delay starts to read
as sluggishness when typing resumes. Doubling in between means the interval is
short exactly when the session is busy and long exactly when it is not, without
either bound needing to be a compromise. The backoff resets to the floor on any
productive pass.

**The part that matters more than the interval: the blocking fast path.** When
no channel has a child running — which is *all* of version exchange, key
exchange and authentication, and the entirety of every `exec`-only session — the
loop blocks on `recv_packet` exactly as it always did. Polling is entered only
when there is genuinely a second thing to watch. This is why the change adds no
latency and no wake-ups to the paths that were already correct, and it is the
reason the tradeoff above is acceptable: it is not paid by connections that do
not need it.

**Cost accepted.** An interactive session that is idle wakes the daemon ~50
times a second per connection. On a machine with many idle SSH sessions that is
real, if small. The measurement that would change this: if idle-session wake-ups
ever show up in a profile, the answer is not a longer sleep — it is the unified
readiness wait, which removes the wake-ups entirely rather than trading them
against latency.

**Trigger to revisit:** when a readiness primitive that accepts both TCP handles
and file descriptors exists. At that point this loop should become a single
blocking wait over both, the backoff constants should be deleted rather than
tuned, and the fast path becomes unnecessary because the slow path is no longer
slow.
