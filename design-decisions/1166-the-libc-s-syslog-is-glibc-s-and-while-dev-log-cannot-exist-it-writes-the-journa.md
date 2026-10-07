## 1166. The libc's `syslog` is glibc's, and while `/dev/log` cannot exist it writes the journal

**Date:** 2026-10-01
**Decided by:** Claude (autonomous), on lane B's request
**Lane:** D

**In short:** when a C program on SlateOS called `syslog()`, the C library
printed the message on the program's standard error. `journalctl` never saw
it, and a daemon whose standard error goes nowhere lost it. The library now
does what glibc does, sending each message to the system log's socket at
`/dev/log`. SlateOS cannot have that socket yet, and while it cannot, each
message becomes a line in the journal file `journalctl` reads, as `logger`
already does (§1033). When the sockets arrive, the journal stops being used,
with no change to the library.

**What it is.** `posix/src/syslog.rs` is glibc 2.39's `misc/syslog.c`,
function for function, quirks included:
- the record is `<pri>Mmm dd hh:mm:ss tag[pid]: message`, with the timestamp
  spelled in the C locale;
- bits outside a priority and a facility get a complaint record of their own,
  filed at BSD's `INTERNALLOG` read as a priority (`<35>`);
- `openlog`'s facility applies even when it is 0, so `openlog(tag, 0, 0)`
  files later records as the kernel's;
- a datagram socket at first, and a stream (each record ending in a NUL) when
  the socket at `/dev/log` is one;
- one reconnection when a send fails, then the console under `LOG_CONS`;
- `LOG_PERROR`'s copy stops at a NUL and adds a newline only when the record
  lacks one;
- the NUL a stream record carries is decided before the first attempt, so a
  record resent on a connection that has just become a stream goes without
  one.

`posix/tools/oracle/syslog_harness.py` asked glibc 2.39 all of this, with
daemons of its own at `/dev/log` (datagram, stream, restarting, absent) and a
FIFO of its own at `/dev/console`, all inside a private mount namespace. That
is 44 scenarios and 345 lines; `syslog/tests.rs` replays every one through
the C entry points.

The journal is where glibc would have connected. When `socket(AF_UNIX, ...)`
fails with `EAFNOSUPPORT` -- the platform has no Unix-domain sockets, not "no
daemon listening" -- the logger counts as connected to the journal, and a
send is an append to `/var/log/syslog.jsonl`:
- **the record** is `journalrec`'s, as `logger` writes it: `ts`, `level`,
  `service` (the tag), `msg`, `pid` and `facility`. `pid` is `getpid()` with
  or without `LOG_PID`, since a daemon learns it from the socket.
  `facility` is the first name glibc's `facilitynames` gives the value
  (`auth`, not `security`), left out when the value has none;
- **one newline at the end of the message is dropped**, as rsyslog drops it
  and as glibc's own `LOG_PERROR` copy treats it: the end of the line, not
  part of it;
- **the append** is `journalio::append`'s protocol (§1037): `O_APPEND`, the
  exclusive `flock` (taken again after `EINTR`, skipped where there are no
  locks), the check that the path still names the file, 64 retries, and the
  whole record written before the file is closed;
- **a failed append is a failed send**, retried once on a new "connection"
  and then given to `LOG_CONS`, exactly as glibc treats a daemon that will
  not take a record;
- **`errno` is left as it was**, as a send that succeeds leaves it.

**Alternatives.**

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **The journal, on `EAFNOSUPPORT` only (chosen)** | `syslog()` reaches `journalctl` today | lane B's ask; `logger`'s rule, so the two writers agree; it retires itself | a second shape of the record to keep equal to `journalrec`'s -- held by a test against it |
| Standard error, as before | nothing in `journalctl` | no new code | the defect lane B asked about |
| The journal on any failure, `ECONNREFUSED` and `ENOENT` included | a system with sockets but no daemon logs to SlateOS's file | never loses a record | changes glibc where glibc's behaviour is well defined, and disagrees with `logger` |
| `journalrec` itself, at run time | one escaper instead of two | no copy | the libc has no allocator for its `String`s; giving it one for a forty-line formatter is out of proportion |

**A record that is not UTF-8** -- its tag or message -- cannot be a JSON
string without being altered. It is written to standard error instead, as
lane B suggested, which is where the library put every record before. It is
written once: not again when `LOG_PERROR` has already written it there.
Altering it to fit (replacing the bad bytes) would be silent corruption.

**Held to `journalrec`** by a dev-dependency. `journalrec` is a lane B crate
that only `posix`'s tests build, and `syslog/tests.rs` compares this
library's record with `Record::to_json_line_with`'s, byte for byte, over
quotes, backslashes, control characters, non-ASCII text, an embedded NUL and
trailing newlines. If lane B changes the spelling, that test fails in lane
D. `scripts/ctest-fixtures.py` reads every `path =` in `posix/Cargo.toml` as
a source of `libc.a`, so `journalrec` now counts towards the sysroot's
staleness although it is not linked into it. That over-counts in the safe
direction, and its comment says why it reads that broadly.

**Where not glibc, beyond the journal:**
- `errno` is set to the call's again before the second formatting pass, as
  it is before the first. glibc sets it once, so a `%m` that `malloc` had
  moved in between would make glibc drop the record (`cl != vl`); here it
  cannot.
- The flag of `__syslog_chk` asks for glibc's fortified formatting checks,
  which this library's printf family does not make (`fortify_printf.rs`). It
  is ignored here as it is there.

**What it does not do.** The journal is written by each program itself, so
a program needs write access to `/var/log/syslog.jsonl`, or to `/var/log` to
create it. A daemon would have needed neither. A program that has no such
access, or runs on a system without `/var/log`, loses the record, or sends
it to the console under `LOG_CONS`, as glibc does when its daemon is
unreachable. `logger` has the same limit. Both end when path-bound
Unix-domain sockets exist and `syslogd` listens
(`requests/b-ad-a-unix-socket-cannot-be-bound-to-a-path-so-nothing-can-receive-syslog.md`).
