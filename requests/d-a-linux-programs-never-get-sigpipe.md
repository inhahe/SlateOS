# D → A — a Linux program writing to a broken pipe is never sent SIGPIPE

**Filed:** 2026-10-06 by lane D.
**Status:** OPEN -- for lane A (`kernel/src/syscall/linux.rs`). Not urgent.

**In short:** on Linux, a program that writes into a pipe or connection
nobody reads any more is sent a signal, `SIGPIPE`. Unless it arranged
otherwise, the signal ends it quietly, which is how `yes | head -1` stops
`yes`. Since today, SlateOS's own C library does the same for native
programs (design-decisions §1176). Programs run through the Linux
personality get only an error code from the kernel and keep going. So a
Linux build of a shell loop such as `while :; do echo x; done | head -1`
never ends. This asks the kernel's Linux ABI to send the signal where Linux
does.

## What happens now

`kernel/src/syscall/linux.rs` maps `KernelError::BrokenPipe` and a closed
channel to `EPIPE`. Its own comments say it never raises `SIGPIPE`:
- line ~1067: "`MSG_NOSIGNAL` (0x4000) is a no-op because we never raise
  `SIGPIPE`";
- line ~39626: "a broken pipe surfaces as EPIPE".

## What Linux does (measured on 6.6, WSL, with the calls below)

| Call | Answer | `SIGPIPE` |
|---|---|---|
| `write`/`writev` to a pipe with no reader | `EPIPE` | yes |
| `write`/`send` on a unix stream socket whose peer closed, or after `shutdown(SHUT_WR)` | `EPIPE` | yes |
| the same with `MSG_NOSIGNAL` | `EPIPE` | no |
| `write`/`send` on a TCP socket never connected, or listening | `EPIPE` | yes |
| `tee`/`splice` into a pipe with no reader | `EPIPE` | yes |
| `send` on a UDP socket after `shutdown(SHUT_WR)` | `EPIPE` | no |
| any of these with `SIGPIPE` ignored | `EPIPE` | -- |

The signal is `SI_USER`, from the process itself (`send_sig(SIGPIPE,
current, 0)`). It is sent before the call returns, so a handler runs
before the `EPIPE` is seen, and the default action ends the process
before the call returns. A TCP send after the peer's orderly close is
`EPIPE` and `SIGPIPE` too (`tcp_sendmsg` → `sk_stream_error`). After a
reset it is `ECONNRESET` and no signal: the socket's pending error
replaces the `EPIPE` before the signal is decided.

## The ask

Wherever `linux.rs` turns a broken pipe or a closed stream into `EPIPE`
for `write`, `writev`, `send`, `sendto`, `sendmsg`, `splice`, `tee`,
`vmsplice` or `sendfile`:
- post `SIGPIPE` to the calling thread, as the Linux ABI's other
  self-directed signals are posted, unless the call carried `MSG_NOSIGNAL`;
- keep returning `EPIPE`.

Nothing for ttys: Linux sends no `SIGPIPE` for a terminal.

## How lane D checks its half

`services/ctest-sigpipe` (native) runs eight checks with exit 42:
- the default action ends a child;
- the ignored, handled, `MSG_NOSIGNAL` and blocked cases;
- `tee`, `SHUT_WR`, and a TCP socket never connected.

It waits for the generic fixture rung (`requests/d-a-one-rung-for-every-c-fixture.md`).
A Linux-ABI copy of the same program, built against glibc, would check
yours the same way.
