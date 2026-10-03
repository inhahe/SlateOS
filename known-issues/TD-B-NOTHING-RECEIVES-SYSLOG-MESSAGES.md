## TD-B-NOTHING-RECEIVES-SYSLOG-MESSAGES (lane B, 2026-09-26) — **open**, waiting on lanes A, D

**Status:** OPEN — waiting on
`requests/b-ad-a-unix-socket-cannot-be-bound-to-a-path-so-nothing-can-receive-syslog.md`
(path-bound `AF_UNIX` sockets). Found 2026-09-26 while fixing `logger`'s
timestamps. Steps 3 and 4 below are done: `logger`'s messages reach
`journalctl` (413e56f1d), and `ntpdate -s`, `crond` and `anacron` log through
the libc's `syslog()` (the step-5 route) instead of losing their messages or
printing them to stderr themselves. Steps 1, 2 and 5 remain, with lanes A
and D.

**In short:** there is no system log on SlateOS in the sense a Unix program
means. A program that logs the POSIX way sends a datagram to `/dev/log` and
expects a daemon there to file it; here nothing listens on `/dev/log`, and
nothing *can*, because a Unix-domain socket cannot be bound to a path
(`socket(AF_UNIX, ...)` is `EAFNOSUPPORT`). So each writer does something
different, and `journalctl` sees almost none of it: with nothing under
`/var/log/journal/` it reads both `/var/log/syslog.jsonl` and `/var/log/syslog`,
but it parses only JSON-lines records and skips every other line without a
word (`read_all_entries` -> `JournalEntry::from_json_line`):

| writer | where its messages go |
|---|---|
| libc `syslog()` (lane D) | stderr (`posix/src/syslog.rs`, `let fd = 2`) |
| `logger` | a `journalrec` record in `/var/log/syslog.jsonl` -- since 2026-09-26; before, RFC 3164 text lines in `/var/log/syslog` that `journalctl` could not read |
| `ntpdate -s` | the libc's `syslog()` -- since 2026-09-26; before, nowhere: it `open`ed `/dev/log` as a file, which failed, and discarded the error |
| `crond`, `anacron` | the libc's `syslog()` -- since 2026-09-26; before, its own `crond2[PID]: ...` lines on stderr |
| `ntpd` (the daemon) | the libc's `syslog()`, or the file its `logfile` directive names -- since 2026-09-26; before, nothing at all outside `-d`, and its clock and drift-file failures were discarded |
| `systemd-cat` (`systemctl`) | a `journalrec` record in `/var/log/syslog.jsonl` |
| `syslogd log` | the same file |
| `syslogd daemon` | receives nothing (`cmd_daemon`: "the daemon sits idle") |

So `logger`'s lines, which were RFC 3164 text, were never shown by
`journalctl` at all (fixed by step 3), and `ntpdate -s`'s are simply lost. (Corrected
2026-09-26: this entry first said `journalctl` fell back to
`/var/log/syslog` only when the JSON-lines file yielded nothing. The code
reads both; it is the parser that drops the text lines.)

**The proper fix:**
1. Lanes A and D: path-bound `AF_UNIX` sockets (the request above).
2. `syslogd daemon` binds `/dev/log` (`SOCK_DGRAM`), parses each frame — the
   local form `<PRI>Mmm dd hh:mm:ss TAG[PID]: MSG`, RFC 3164 with a hostname,
   and RFC 5424 — and writes it as a `journalrec` record.
3. **DONE 2026-09-26 (413e56f1d, design-decisions §1033).** `logger` is a
   faithful port of util-linux 2.39.3's `logger.c`, sending to `/dev/log`
   exactly as upstream does, verified by `scripts/logger-diff.sh` (138 cases)
   against WSL's util-linux. It did not wait for step 1: where the platform
   has no Unix-domain sockets at all (`EAFNOSUPPORT` — not "no daemon
   listening", which upstream handles its own way), it appends a `journalrec`
   record instead, which is what a daemon would have written. On a host with
   sockets that branch never runs, so the harness compares pure upstream
   behaviour; on SlateOS it reaches `journalctl` today, and switches to
   `/dev/log` by itself once step 1 lands.
4. **DONE 2026-09-26.** `ntpdate -s` (and `sntp -s`) log through the C
   library's `syslog()` -- `openlog(name, LOG_PID, LOG_DAEMON)`, as ntpdate
   does -- by way of `userspace/libcsyslog`, a thin wrapper that keeps one
   syslog client on the system; `crond` and `anacron` too, at `LOG_CRON`,
   each message at its own severity. Checked end to end by
   `scripts/syslog-client-check.sh`, which puts a private socket over
   `/dev/log` in WSL. Where the messages go is now the libc's decision --
   step 5.
5. Lane D's libc `syslog()` sends to `/dev/log` as glibc does -- and until
   step 1 lands, appends a journal record when `socket(AF_UNIX, ...)` fails
   with `EAFNOSUPPORT`, as `logger` does. Asked in
   `requests/b-d-libc-syslog-could-reach-journalctl-today.md`; lane D's call.

Step 3's port needed a real `getopt_long` (permutation, abbreviated long
options, optional arguments). Only coreutils had one; it was extracted into
the shared `getoptlong` crate (c8e63a0bf), which `logger` uses and every
standalone util-linux port here can -- see
TD-B-STANDALONE-PORTS-MATCH-LONG-OPTIONS-WHOLE for the rest.
