## TD-B-NSENTER-RAN-THE-COMMAND-IN-THE-WRONG-NAMESPACE — 2026-09-15 — FIXED by refusing

**In short:** `nsenter` checked that the target's namespace files existed, did
**not** call `setns`, and then **ran the command anyway** — in the caller's own
namespaces, reporting nothing unusual. `nsenter -t <container> -m -- rm -rf
/data` deleted the host's `/data`.

**Why this was worse than any inert flag in this tree.** Everywhere else, an
unread field meant a requested behaviour did not happen: `patch -l` matched
strictly, `tee -i` died on `^C`, `lscpu -p` printed the wrong table. Here the
command was not skipped — it was **performed, in the wrong place**. The user
saw it run normally and had no way to tell where.

Three comments in the file said so out loud — *"in real implementation, this
would use setns(2)"*, *"For simulation, just verify accessibility"*, *"Execute
command (in real implementation, this would happen after setns)"*. All three
were true, none reached the user, and the program between them did the
dangerous thing. A truthful comment beside a false behaviour is the shape
design-decisions §1022 catalogues.

**Fixed by refusing, not by wiring.** There is nothing to wire to:
`posix::setns` validates its arguments and its `CAP_SYS_ADMIN` gate and then
returns `ENOSYS` — "namespace subsystem not implemented". This is not the
`curl`/`tee` pattern where the capability existed and the call site was
missing; the capability genuinely is absent. `nsenter` now reports that and
exits 1, and the execution path is **deleted** rather than guarded —
`Command::new` no longer appears in the file, so it cannot run a command by
any route.

**This entry ends the day a namespace subsystem lands.** At that point `setns`
stops returning `ENOSYS`, and the refusal should become a real `setns` call in
the loop that currently only checks the files.

**Also moot until then:** `-W/--wdns` (`wd_fd`), `-F/--no-fork` and
`--preserve-credentials` are parsed and read by nothing. They describe what to
do *after* entering a namespace, so there is nothing for them to modify while
entry is refused. Listed so the next reader does not count them as separate
defects.

**Where it lives:** `userspace/nsenter/src/main.rs`.
