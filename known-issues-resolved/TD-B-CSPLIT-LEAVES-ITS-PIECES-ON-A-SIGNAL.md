## TD-B-CSPLIT-LEAVES-ITS-PIECES-ON-A-SIGNAL (lane B, 2026-10-08)

**Status:** RESOLVED 2026-10-08 (lane B). `coreutils::cleanup` holds the
machinery `sort` brought in, and `csplit` registers each piece in the
critical section that creates it (nothing under `-k`); its error cleanup is
upstream's `delete_all_files` too -- the pieces last first, a failure to
remove one that is not `ENOENT` said, where it was ignored. Measured before
the fix, `seq 1 3000 | csplit -n 4 - 2 '{*}' | true` left 1501 pieces where
GNU leaves none; `csplit-diff.sh` holds that case and its `-k` twin.

**In short:** interrupt GNU `csplit` -- Ctrl-C, a `kill`, its reader gone --
and it deletes the pieces it has written so far, as it does when it fails,
unless `-k` asked it to keep them. Ours leaves them. So a `csplit` stopped
half way leaves `xx00`, `xx01`, ... behind where GNU's leaves nothing, and
the next run's pieces mix with the last run's.

**Upstream:** `csplit.c`'s `main` installs `interrupt_handler` for
`SIGALRM`, `SIGHUP`, `SIGINT`, `SIGPIPE`, `SIGQUIT`, `SIGTERM` (and
`SIGPOLL`, `SIGPROF`, `SIGVTALRM`, `SIGXCPU`, `SIGXFSZ` where they exist),
each only if not inherited ignored; the handler calls `delete_all_files
(true)` -- nothing under `-k` -- then restores the default action and
re-raises.

**Where:** `userspace/coreutils/src/bin/csplit.rs`, which registers no
handler; its `Sink::remove_all` runs only on a failure it returns from.

**The fix:** the same machinery `sort` has had since 2026-10-08
(`sort/external.rs`: a registry changed only with the caught signals
blocked, a handler that unlinks every registered path and re-raises,
`libcall::signal::is_ignored` and `set_handler_masked`). Move it into a
`coreutils` library module both programs use -- a `Cleanup` registry with
`install(signals)`, `add(path)`, `remove(path)` and `remove_all()` -- then
register each piece as `csplit` creates it (not under `-k`), and add a
`csplit-diff.sh` case that interrupts both programs mid-split and compares
what is left.
