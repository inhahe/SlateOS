## TD-B-A-STREAMS-FORWARDING-SWITCHES-ARE-READ-AND-IGNORED (lane B, 2026-10-09)

**Status:** OPEN

**In short:** a program sending its output to the journal through a stream
(the way `systemd-cat` does) can ask, in the stream's opening header, for each
line to be copied to three more places as well: a classic syslog daemon, the
kernel log, and the system console. journald honours those three switches;
SlateOS's stand-in for journald reads them and does nothing with them. In
practice it matters little. `systemd-cat` always sends all three switched off,
and only a crafted identifier containing newlines can turn one on. But a
program that asks for its lines on the console does not get them.

**What is already done:** journald's default forwarding, the broadcast of an
`emerg` message to every logged-in user's terminal (`ForwardToWall`), is in
`userspace/journalfwd` (2026-10-09). Both stand-ins call it: `syslogd` for
`/dev/log` and `systemd-cat`'s helper for streams. That closed
`TD-B-NOTHING-DOES-JOURNALDS-FORWARDING`, of which this is the remainder.

**Where:** `userspace/coreutils/src/bin/systemd-cat.rs`, module `journald`.
`Stream::forward` holds the three switches as the header gave them, and
nothing reads it.

**What journald does with each** (systemd 255, `journald-stream.c`'s
`stdout_stream_log`, called before the line is filed):

| switch | journald | where it goes |
|---|---|---|
| `forward_to_syslog` | `server_forward_syslog` | a datagram to `/run/systemd/journal/syslog`, for a classic syslog daemon |
| `forward_to_kmsg` | `server_forward_kmsg` | `/dev/kmsg`, as `<PRI>IDENT[PID]: MESSAGE` |
| `forward_to_console` | `server_forward_console` | `/dev/console` (or `TTYPath=`), prefixed with the time since boot |

**Reproduce:** write a stream header whose identifier carries the extra
lines that turn a switch on (`scripts/systemd-cat-diff.sh`, "a header written
into the identifier", builds one), then a line. On Linux the line also
appears on the console, in the kernel log, or at the syslog socket. Here it
is only filed.

**The fix:** one function per switch in `journalfwd`, beside
`forward_wall`, each ported from journald and measured the way
`scripts/journalfwd-diff.sh` measures the broadcast: against systemd's own
code, in a namespace where the destinations are the harness's own. Then
`journald::forward` calls each when its switch is on. `/dev/kmsg` and
`/dev/console` need lane A to say what SlateOS has at those paths before the
kmsg and console halves can be anything but a write that fails.
