## 1033. `logger` is util-linux's, and where `/dev/log` cannot exist it writes the journal

**Date:** 2026-09-26
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `logger` -- the command scripts use to put a line into the
system log -- is now a function-by-function port of util-linux 2.39.3's,
checked against the real one by `scripts/logger-diff.sh` (138 cases).
Upstream hands each message to a log daemon through a special socket file,
`/dev/log`, and silently drops it when that cannot be reached. SlateOS cannot
have that socket yet, so copying upstream exactly would make every script's
logging vanish without a word; in exactly that situation, and no other, the
port writes the message into the log `journalctl` reads instead. Two smaller
differences: a name in an error message has its control characters escaped
rather than printed raw, and `-S 0` on piped input no longer sends empty
messages forever.

### Where a message goes when `/dev/log` cannot exist

Why it cannot: the platform has no Unix-domain sockets bound to a path
(`socket(AF_UNIX, ...)` fails with `EAFNOSUPPORT`, "address family not
supported"); known-issues TD-B-NOTHING-RECEIVES-SYSLOG-MESSAGES and its
request to lanes A and D.

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **A journal record, only when every attempt on `/dev/log` fails with `EAFNOSUPPORT` (chosen)** | `logger hi` on SlateOS shows up in `journalctl`; on Linux nothing changes | messages reach the log today; the branch stops running by itself once the sockets exist, with no change to `logger`; the harness still measures pure upstream behaviour, since Linux never takes it | a second writer of `/var/log/syslog.jsonl` beside the future daemon, whose record (level, facility, tag, PID) the port has to shape as a daemon would |
| Upstream exactly: drop the message | `logger hi` succeeds and nothing is recorded | byte for byte upstream | every script's logging is silently lost until the sockets land -- the defect this port was written to fix |
| Keep the old program's own file, `/var/log/syslog` as RFC 3164 text | text lines `journalctl` reports as "not a journal record" | no new format | neither upstream nor readable by `journalctl` |
| Fall back on any failure, "no daemon listening" included | on Linux, `logger` with no syslog daemon writes a SlateOS log file | never loses a message | changes upstream's behaviour where upstream's is well defined, on the host the harness runs on |

The branch is narrow on purpose. `-u PATH` names a socket the caller chose,
so only `/dev/log` is ever redirected. And `EAFNOSUPPORT` means the platform
has no such sockets at all, which is not "no daemon" (`ENOENT`,
`ECONNREFUSED`): upstream handles that its own way, and the port still does.
`--journald` exists, as it does in every distribution's build (util-linux
built with libsystemd), and writes the same journal, there being no journald;
its `MESSAGE`, `PRIORITY` and `SYSLOG_IDENTIFIER` become the record's own
fields and every other field is kept beside them (`journalrec::Record::extra`).

### Names in diagnostics

Upstream pastes arguments into its messages -- `unknown facility name: %s` --
or wraps them in its own `'%s'`. A name holding a newline can then print a
line `logger` never wrote (§370). The tree's usual remedy, `quotef`/`quoteaf`,
renders the name the way a shell would read it back, which changes ordinary
text too: the first harness run caught `x="y"` printed as `'x="y"'` and the
empty name as `''`. The port instead prints printable text exactly as
upstream does and octal-escapes only what is not printable -- `\012` for a
newline, `\033` for the escape that starts a terminal control sequence.
Forging a line needs a control byte; nothing else changes.

| Option | *What changes:* (`-p $'a\nb.info'`, `--sd-id "a'b"` twice) |
|---|---|
| **Upstream's text, unprintables escaped (chosen)** | `unknown facility name: a\012b`; `structured data ID 'a'b' is not unique`, as upstream |
| `quotef`/`quoteaf`, the tree's GNU convention | `'a'$'\n''b'`; `"a'b"` -- and every name with a space, a quote or nothing in it changes too |
| Upstream exactly | a second line, `b`, that `logger` did not mean to print |

Upstream's own `'%s'` is rendered by `quoting::escaped_in_quotes`, added for
this: upstream's quote marks around escaped contents. It is not `quoteaf`,
which would print `it's` as `"it's"`; the marks are decoration here, not a
delimiter, and upstream does not escape an `'` inside them either. The
harness pins three escapes as expected differences. B-Q12 asks the same
question of `osh` and now lists this as an option.

### `-S 0`

With `-S 0` and input on stdin, 2.39.3's read loop can store nothing, so it
never consumes the byte it stopped on and sends empty messages forever (still
so on util-linux master). The port sends one empty message per line, which
is what `-S 0` makes of a word given as an argument. The harness does not
compare it: upstream would never finish.

### Kept as upstream, and how it was checked

`SCM_CREDENTIALS` -- root's `--id=PID` naming another live process as a local
message's sender -- is sent as upstream sends it, a `sendmsg` carrying the
credentials. The harness runs it as root in a user namespace (`unshare -r`):
the kernel refuses the claim in the host's PID namespace, which that root does
not own ("send message failed: Operation not permitted" from both sides), and
accepts it in a PID namespace of its own, where a listener with `SO_PASSCRED`
sees the claimed PID from both sides.

**Where:** `userspace/logger/src/deliver.rs` (the journal branch), `main.rs`
("What is not upstream's", `shown`), `input.rs` (`-S 0`), `sys.rs`
(`send_as`, `may_claim`); `userspace/quoting/src/lib.rs`
(`escaped_in_quotes`). Checked by `scripts/logger-diff.sh`.

**Revisit** when path-bound Unix-domain sockets land: the journal branch
should then never run on SlateOS either -- confirm `logger` reaches `syslogd`
through `/dev/log`, then consider deleting the branch. And if B-Q12's answer
sets a tree-wide policy for upstream message text.
