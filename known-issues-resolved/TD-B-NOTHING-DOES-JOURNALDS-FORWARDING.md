## TD-B-NOTHING-DOES-JOURNALDS-FORWARDING (lane B, 2026-10-09) — RESOLVED

**Status:** RESOLVED 2026-10-09

**Resolved the same day.** journald's default forwarding is ported: an `emerg`
message is broadcast to every logged-in user's terminal (`ForwardToWall`).
The new crate `userspace/journalfwd` holds systemd 255's `wall()` and
journald's `server_forward_wall`. `syslogd` calls it for each `/dev/log`
datagram and `systemd-cat`'s helper for each line, before the record is
filed, as journald orders it. Both now also name a writer as journald's
`pid_get_comm` does, escaped. `scripts/journalfwd-diff.sh` holds the
broadcast to systemd's own `wall()` from `libsystemd-shared`, each side in
namespaces whose terminals are the harness's own: 31 cases agree.

**What is left** is the three switches a stream's header can turn on: copying
lines to a syslog daemon, the kernel log and the console. They are now
`TD-B-A-STREAMS-FORWARDING-SWITCHES-ARE-READ-AND-IGNORED`. The entry below is
kept as written.

**In short:** on Linux, systemd-journald does more with a message than store
it. With its default settings it also writes any message at the most urgent
level, `emerg`, to every logged-in terminal, as `wall` would ("Broadcast
message from systemd-journald@host"). And a program can ask, per stream, for
its lines to be copied to the kernel log, the console, or a classic syslog
daemon. SlateOS has no journald: its journal is a file that `syslogd` (for
`/dev/log`) and `systemd-cat`'s helper (for output streams) append to. Neither
does any of that forwarding, so `echo down | systemd-cat -p emerg` files the
line and nobody's terminal shows it.

**Where:**

- `userspace/syslogd/src/daemon.rs` — files each datagram, forwards nothing.
- `userspace/coreutils/src/bin/systemd-cat.rs`, module `journald` — reads the
  three forwarding switches a stream's header carries (`Stream::forward`,
  because the protocol has them) and acts on none.

**What journald does, systemd 255, Ubuntu's defaults** (`journald.conf`:
`ForwardToWall=yes`, `MaxLevelWall=emerg`; `ForwardToSyslog`, `ForwardToKMsg`,
`ForwardToConsole` all `no`, with `MaxLevelKMsg=notice`,
`MaxLevelConsole=info`, `MaxLevelSyslog=debug`):

- `server_forward_wall` (`journald-wall.c`): a message at or under
  `MaxLevelWall` goes to `utmp_wall`, as `IDENT[PID]: MESSAGE` -- the
  identifier, or failing it the sender's command name -- under the banner
  `Broadcast message from systemd-journald@HOST (DATE):`.
- A stream's own switches (`stdout_stream_log`): `server_forward_syslog` to
  `/run/systemd/journal/syslog`, `server_forward_kmsg` to `/dev/kmsg` as
  `<PRI>IDENT[PID]: MESSAGE`, `server_forward_console` to `/dev/console`.
  `systemd-cat` always sends them off; only an identifier with newlines in it
  can turn them on (`scripts/systemd-cat-diff.sh`, "a header written into the
  identifier").

**Reproduce:** on Linux, `echo test | systemd-cat -p emerg` prints the broadcast
on every terminal of a logged-in user. On SlateOS nothing is printed.

**The fix:** one implementation of journald's forwarding -- the wall broadcast
first, since it is the one on by default -- shared by `syslogd` and the
`systemd-cat` helper, measured against WSL's journald for the banner, the
line, and which terminals are written to (`utmp_wall`'s rules, which differ
from util-linux `wall`'s). Then the per-stream switches, which need the same
writers.
