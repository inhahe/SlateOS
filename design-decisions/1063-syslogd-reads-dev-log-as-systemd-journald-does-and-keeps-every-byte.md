## 1063. `syslogd` reads `/dev/log` as systemd-journald does, and keeps every byte

**Date:** 2026-10-07
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** programs on SlateOS will soon send their log messages to the
socket `/dev/log`, as every Unix program does, and `syslogd` is what receives
them and writes them into the journal `journalctl` shows. A message arrives
as one line of text with a small header -- its importance, the time, the
program's name and number. This decides how `syslogd` reads that header: the
way systemd-journald, which owns `/dev/log` on today's Linux, reads it, rather
than the more ambitious way a classic syslog daemon such as rsyslog does. It
also decides that a message containing bytes that are not valid text is
stored exactly, not "cleaned up".

### What it is

A datagram (one message, sent whole) is read as systemd 255's
`journald-syslog.c` reads it, measured against the journald running in WSL:

* whitespace at both ends of the datagram is removed;
* `<PRI>` is up to three digits, unchecked against any range (`<999>` is
  facility 124, level 7); absent or malformed, the message is `user.info`;
* the timestamp is exactly `Mmm dd hh:mm:ss ` -- three letters, a day that may
  be space-padded, the time, and a space; anything else is not a timestamp;
* the program's name is the first word if it ends in `:`, with an optional
  `[pid]` before the colon; one separator after the colon is consumed;
* everything left is the message.

Each part is filed in the journal record journald would have made, in this
journal's own five fields where it has them: `level` from the priority;
`service` the name -- or, with none, the sender's command name, which is what
`journalctl` on Linux shows in its place; `pid` the number when it is one,
else the sender's; `msg` the message. Everything else journald would keep is
kept beside them under journald's own names: `SYSLOG_FACILITY` (with
`facility`, the name `logger`'s records already carry), `SYSLOG_TIMESTAMP` as
sent, a `SYSLOG_PID` that is not a number, `SYSLOG_RAW` -- the whole datagram
-- wherever the header was not fully understood or whitespace was removed,
and the sender's `_PID`, `_UID`, `_GID` and `_COMM` from the socket's
credentials (`SO_PASSCRED`), the kernel vouching for them where the message
could say anything.

A value that is not valid UTF-8 (the standard encoding of text) is written as
a JSON array of its byte values -- the spelling `journalctl -o json` uses for
such a field on Linux -- and every other value as a JSON string, as before, a
control character included as its `\u` escape. So no writer's existing output
changes, and `journalctl` here reads both spellings. (journald's export also
uses the array for valid text with a control character in it; that is a rule
for `journalctl -o json`'s output, not for how the journal is stored.)

### Alternatives

**rsyslog's parsers (`pmrfc3164`, `pmrfc5424`).** They also take a hostname
after the timestamp and the whole RFC 5424 header (the newer syslog format,
with structured data). For: more of the header ends up in fields. Against:
they guess -- a message without a timestamp had its first word taken as a
hostname, measured -- and rsyslog rewrites what it keeps: a newline becomes
`#012`, an invalid byte becomes U+FFFD. Above all it is not what the owner of
`/dev/log` does on the Linux every port here is measured against, so neither a
script nor `journalctl` there sees what this would show.

**journald's, with the RFC 5424 header parsed as well.** For: `logger
--rfc5424` messages would file their parts. Against: it would be this
system's own invention, measurable against nothing; on Linux those messages
reach the journal with their header in the text, and they do here too.

**Replacing or dropping bytes that are not text.** Against: lossy decoding is
ruled out across this tree (`CLAUDE.md` checklist item 7), and a log is the
last place to lose evidence.

### Where

`userspace/syslogd` (the reader and the socket), `userspace/journalrec`
(the byte-safe values), `userspace/journalctl` (reading them), checked by
`scripts/syslogd-diff.sh` against WSL's journald.
