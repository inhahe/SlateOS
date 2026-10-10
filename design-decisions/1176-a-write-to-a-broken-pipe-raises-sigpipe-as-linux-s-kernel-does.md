## 1176. A write to a broken pipe raises SIGPIPE, as Linux's kernel does -- §377's "no signal to restore" no longer holds

**Date:** 2026-10-06
**Decided by:** Claude (autonomous) -- revisiting §377, also Claude's
**Lane:** D

**In short:** on Linux, a program writing into a pipe whose reader has
gone, as in `producer | head -1` once `head` has its line, is ended by a
signal called `SIGPIPE`, quietly. On SlateOS that signal was never sent.
The writer got an error instead. A careful program printed "Broken pipe"
and stopped; a careless one went on writing. Under the shell,
`while true; do echo x; done | head -1` would loop forever, printing an
error each time. The C library now sends the signal where Linux's kernel
does, so C programs end as they do on Linux. Programs that ignore the
signal are unaffected and still get the error; Rust programs and Python
do that at startup.

**What changed and where (`posix/src/signal.rs` → `broken_pipe`):** the
C library raises `SIGPIPE` and then fails the call with `EPIPE` in these
cases:
- `write`, `writev`, `splice`, `sendfile` and `vmsplice` into a pipe with
  no reader;
- `tee` into one, raised even after a partial copy, as Linux's
  `link_pipe` does;
- `write`, `send` and `sendto` on a unix-domain stream socket whose peer
  has gone, or after our own `shutdown(SHUT_WR)`;
- the same calls on a TCP connection after an orderly close (a reset
  stays `ECONNRESET`, with no signal);
- the same calls on a TCP socket never connected, or listening.

`MSG_NOSIGNAL` gives `EPIPE` alone. The signal is `SI_USER` from this
process, the same as the kernel's `send_sig(SIGPIPE, current, 0)`. `errno`
is set after any handler has run. Terminals raise nothing, as on Linux.

Every answer but one was measured on Linux 6.6 with the same calls (WSL).
The exception is a TCP connection after an orderly close, which needs a
peer the probe did not set up; it follows Linux's source (`tcp_sendmsg`,
then `sk_stream_error`). Measuring corrected three more:
- `write`/`send` on a TCP socket never connected was `ENOTCONN`; Linux says
  `EPIPE` with `SIGPIPE`.
- On a listening socket, `write` was `EINVAL`, `send` and `sendto` were
  `ENOTSOCK`, and `read`, `recv` and `recvfrom` were `EINVAL`/`ENOTSOCK`.
  Linux says `EPIPE` with `SIGPIPE` for the sends and `ENOTCONN` for the
  receives.
- On a unix-domain stream socket, `sendto` and `recvfrom` were `ENOTSOCK`.
  Linux sends and receives as `send`/`recv` do. `sendto` answers `EISCONN`
  to a destination, and `EINVAL` to one longer than `sockaddr_storage`.

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. The C library raises it** (chosen) | `producer \| head -1` ends the producer as on Linux; a C program with no handler dies of `SIGPIPE` | Linux's behaviour for every C program, now that signals are delivered: handlers run, `kill` and `^C` reach them | a write in a program that never thought about it can now end the program, as it can on Linux |
| B. Keep §377: `EPIPE` alone | nothing | no program is ever ended by a write | a C program that ignores write errors loops forever behind a closed pipe; bash's `echo` in such a loop prints an error each time; nothing matches Linux |

**Why §377 does not stand in the way.** §377 kept `SIGPIPE` masked in
coreutils because "the target has no signal to restore". That was true in
August. Since then the C library delivers signals:
- `sigaction` handlers run through the kernel's trampoline;
- `kill` works;
- the terminal's `^C` reaches the foreground.

`design.txt`'s "no Unix signals for process control" is about stopping
programs, which is done over IPC. It is not about the compatibility layer's
duty to behave as Linux does for the programs written for it. §377's
outcome for coreutils is unchanged: Rust's runtime ignores `SIGPIPE`
before `main`, so those utilities still see `EPIPE` and stay quiet as
§377 decided. Whether to now restore the signal in coreutils, §377's
option A, is lane B's question; they are told
(`requests/d-b-sigpipe-is-raised-now.md`).

**Not here:**
- **Linux-ABI programs.** The kernel's `linux.rs` never raises `SIGPIPE`,
  and its `MSG_NOSIGNAL` is a no-op. That half is lane A's,
  `requests/d-a-linux-programs-never-get-sigpipe.md`.
- **The default action on the target.** The ring-3 check of the default
  action, `services/ctest-sigpipe`, waits for lane A's generic fixture rung
  with the others.
