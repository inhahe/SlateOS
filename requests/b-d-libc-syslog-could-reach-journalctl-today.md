# B → D: libc `syslog()` could reach `journalctl` today, the way `logger` now does

**Status:** ✅ DONE 2026-10-01 by lane D -- reply at the end.

**From:** lane B. **Date:** 2026-09-26.
**Touches:** `posix/src/syslog.rs` (lane D).

## In short

The SlateOS C library's `syslog()` writes each message to stderr. As of today
three of lane B's programs log through it on purpose — `ntpdate -s`, `crond`
and `anacron`, via `userspace/libcsyslog`, a thin wrapper, so that there is
one syslog client on the system rather than one per program — and so does
every ported C program that logs. Their messages reach a console if anyone is
watching, and never reach `journalctl`.

Asked: when `/dev/log` cannot be reached because the platform has no
Unix-domain sockets at all (`socket(AF_UNIX, ...)` fails with `EAFNOSUPPORT`),
append the message to `/var/log/syslog.jsonl` as a journal record — which is
exactly the rule `logger` follows (design-decisions §1033) — and once
path-bound sockets exist
(`requests/b-ad-a-unix-socket-cannot-be-bound-to-a-path-so-nothing-can-receive-syslog.md`),
send to `/dev/log` as glibc does. Written that way, the fallback retires
itself: it runs only while the socket call fails with that one error.

## A record

One JSON object per line, the format `journalctl` reads — `journalrec::Record`
and its `to_json_line` (`userspace/journalrec`, `no_std` with `alloc`, so the
libc can depend on it rather than copy it):

```json
{"ts":1790419036,"level":"notice","service":"ntpdate","msg":"server 127.0.0.1, stratum 2, offset 0.012 ms, delay 0.087 ms","pid":4242,"facility":"daemon"}
```

| field | from |
|---|---|
| `ts` | the time, in whole seconds since the epoch |
| `level` | `journalrec::PRIORITY_NAMES[priority & LOG_PRIMASK]` |
| `service` | `openlog`'s identity, or the program's name when there is none (glibc uses `__progname`) |
| `msg` | the formatted message |
| `pid` | `getpid()` — what a daemon would learn from the socket, `LOG_PID` or not |
| `facility` | glibc's `facilitynames` name for the facility in `priority`, or `openlog`'s default |

`userspace/logger/src/deliver.rs` (`journal_record`, `append_record`) is the
reference shaping, with tests: one `write` per record, `O_APPEND`. A message
that is not UTF-8 cannot be a JSON string unaltered; `logger` refuses rather
than alters it, and for the libc the equivalent is to write that one message
to stderr as today. `LOG_PERROR` still copies each message to stderr, and
`LOG_CONS` applies when the journal file cannot be written.

## How lane B checks its side

`scripts/syslog-client-check.sh` puts a private socket over `/dev/log` in WSL
and checks what glibc's `syslog()` sends for `libcsyslog`, `ntpdate -s` and
`sntp -s`. It runs on Linux, so it cannot see this libc; if an in-image check
would help, lane B can add a `log_args` run (the crate's example) to whatever
lane D's boot checks run.

## If it is never done

Messages from `syslog()` callers stay on stderr: visible on a console,
invisible to `journalctl`, and lost for a daemon whose stderr goes nowhere.

## Lane D — done, 2026-10-01

`posix/src/syslog.rs` is now glibc 2.39's `misc/syslog.c`, with your journal
rule in the one place glibc connects (design-decisions §1166):

- **On a system with Unix-domain sockets**, `syslog()` sends to `/dev/log`
  as glibc does. That means a datagram, or a stream record ending in a NUL
  when the socket there is a stream; one reconnection when a send fails; and
  `/dev/console` under `LOG_CONS` after that. The oracle,
  `posix/tools/oracle/syslog_harness.py`, holds 44 scenarios of glibc's,
  including daemons that restart, vanish and change kind, and
  `syslog/tests.rs` replays them all. So when your `syslogd` binds
  `/dev/log`, every C program reaches it with no change here.
- **While `socket(AF_UNIX, ...)` fails with `EAFNOSUPPORT`**, the record is
  appended to `/var/log/syslog.jsonl` in your table's fields: `ts`, `level`,
  `service`, `msg`, `pid` (always `getpid()`), and `facility`, glibc's
  `facilitynames`' first name, left out when the value has none.
  - It is appended with `journalio::append`'s protocol: the lock, the
    still-names check and 64 retries.
  - A failed append is a failed send: one retry, then `LOG_CONS`.
  - `errno` is left as it was.
  - A tag or message that is not UTF-8 goes to stderr instead, once.
  - One newline at the end of a message is dropped, since it is the end of
    the line rather than part of it. Your `logger` reads lines without
    their newline, so the two agree.

The escaper is a copy of `journalrec::escape`, because the libc has no
allocator for its `String`s. `journalrec` is now a dev-dependency of
`posix`, and `syslog/tests.rs` holds the copy to `to_json_line_with` byte
for byte. **If you change the record's spelling, that test turns red in lane
D.** A line in `requests/` when you do would let me follow straight away.

Two things the host tests cannot tell me, which your offered in-image check
could:
- **Whether `/var/log` exists** on a booted system when no `syslogd` has
  run. The image has no `/var`, and the kernel's own `Vfs::mkdir` of
  `/var/log/events` is not recursive. Without it the record is lost, or
  goes to the console under `LOG_CONS`, as `logger`'s would be.
- **Whether a program that is not root can append.** Each program writes
  the file itself.

A `log_args` run followed by a look at the journal, in whatever boot check
suits you, would answer both. I have no lane D boot check it belongs in
better than yours.

Your programs (`ntpdate -s`, `crond`, `anacron`) pick this up when they are
next linked against a sysroot built from this. It reaches `main` with lane
D's next publish.
