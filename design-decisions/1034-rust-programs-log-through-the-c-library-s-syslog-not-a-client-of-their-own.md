## 1034. Rust programs log through the C library's `syslog()`, not a client of their own

**Date:** 2026-09-26
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** a program that "logs to syslog" needs something to hand its
messages to. `ntpdate -s`, `crond` and `anacron` now hand them to the C
library's `syslog()` -- through `userspace/libcsyslog`, a thin wrapper --
exactly as their C counterparts do, instead of each carrying its own copy of
the socket-and-fallback logic `logger` has. The cost shows today: on SlateOS
the library still prints these messages on stderr, so they do not yet reach
`journalctl`. Lane D has been asked to change that, in the one place it
needs changing.

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **The libc's `syslog()`, via `libcsyslog` (chosen)** | on SlateOS today the messages print on stderr; when the libc sends them to the journal or `/dev/log`, every caller follows at once | one syslog client on the system, shared with every ported C program, so the frame, the socket and the fallback are decided in one place; faithful to ntpdate and cron, which call syslog(3) | reaching `journalctl` waits on lane D (`requests/b-d-libc-syslog-could-reach-journalctl-today.md`) |
| A Rust client of our own, with `logger`'s journal fallback | the messages reach `journalctl` today | no wait on another lane | a second syslog client beside the libc's, which C programs would never use: two places to change when `/dev/log` arrives, and a Rust daemon and a C daemon on one system logging differently |
| Each program's own stopgap, as before | `ntpdate -s` loses its messages; `crond` prints its own lines on stderr | nothing to do | the defect |

`logger` stays the exception: util-linux's `logger` speaks the protocol
itself, and a faithful port does too (§1033).

Two details of the wrapper are load-bearing. `openlog` keeps the pointer it
is given, not a copy, so `libcsyslog` keeps the identity alive in a
process-wide slot until the libc has been handed a replacement; and a message
is always passed as the argument to `"%s"`, never as the format, so a `%` in
it is printed rather than interpreted -- `syslog-client-check.sh` sends
`100% %s %n` to prove it.

**Where:** `userspace/libcsyslog`, `userspace/ntpd` (`open_syslog`, `-s`),
`userspace/crond` (`log_msg`, `open_syslog`). Checked by
`scripts/syslog-client-check.sh`.

**Revisit** if lane D declines the request: the journal fallback then belongs
in `libcsyslog`, the next-best single place.
