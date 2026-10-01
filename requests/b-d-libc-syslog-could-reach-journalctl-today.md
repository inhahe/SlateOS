# B → D: libc `syslog()` could reach `journalctl` today, the way `logger` now does

**Status:** OPEN. Nothing breaks without it; messages keep going to stderr.

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
