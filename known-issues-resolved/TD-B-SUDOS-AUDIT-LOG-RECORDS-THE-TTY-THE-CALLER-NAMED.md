## TD-B-SUDOS-AUDIT-LOG-RECORDS-THE-TTY-THE-CALLER-NAMED (lane B, 2026-09-11) -- FIXED 2026-09-11

**FIXED** with `ttyname(0)`, which is what the entry named as the proper
fix and what real sudo uses. The bytes are not put through UTF-8: the old
comment's argument for `var_os` over `var` survives the change, because a
tty name is a path under `/dev` and this OS allows any byte but `/` and
NUL in one.

One thing the entry did not anticipate: `ttyname` returning null means
*descriptor 0 is not a terminal*, which is a real answer and a different
one from *there is a terminal and I could not name it*. The record says
`none` for the first rather than reusing `unknown` for both.

**Found again by the gate built for this family the previous tick**
(`check-env-identity`, gate 23). It was in the baseline as a known site,
and fixing it turned the pin stale, which is the ratchet working in the
direction it is meant to move.


**In short:** `sudo`'s audit log has a `TTY=` field, and the value comes from
the caller's `$TTY` environment variable. A user can therefore choose what
terminal the log says they ran the command from.

**Where.** `userspace/sudo/src/main.rs`, `current_tty`:

    env::var_os("TTY").unwrap_or_else(|| OsString::from("unknown"))

used only by `log_command`, at four call sites.

**Why it is bounded, and why it is still worth fixing.** The value is escaped
where it is written, so it cannot forge whole log lines — only the contents of
one field. And it is an audit record, not an authorization input: nothing
branches on it. So this is log integrity, not privilege escalation, which is
why it is an entry rather than a same-day fix.

It is still the same family as the two that *were* escalation:
`current_username` (`$USER` chose which sudoers rules applied and which
credential cache was consulted) and `current_hostname` (`$HOSTNAME` chose the
host half of the rules), both fixed 2026-09-11. An audit log is read precisely
when someone is working out what happened, and a field the subject could set is
one that says nothing at exactly that moment.

**The proper fix** is `ttyname(0)`, which is what real sudo uses and what
`posix` already exposes — not `$TTY`, which no other Unix consults for this.
The existing docstring's care about `var_os` versus `var` stays relevant: a tty
name is a path under `/dev` and may not be UTF-8.
