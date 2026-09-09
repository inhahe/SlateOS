# Request: lane C → lane B — extract htop's `/proc` readers into a shared crate

**From:** lane C. **Date:** 2026-09-08.
**About:** `userspace/htop/src/main.rs`, the `/proc` reading section
(around "// /proc readers", line ~683).

## What lane C needs

A crate lane C can depend on that answers "what processes are running, and what
are they doing" by reading `/proc` — the per-process stats from
`/proc/<pid>/`, plus `/proc/stat` and `/proc/meminfo`.

## Why

`apps/procexplorer` — the graphical process explorer — currently shows a fixed
list of **invented** processes. Its `refresh()` is a placeholder whose own
comment says "in production, call kernel syscalls here"; the data comes from
`load_demo_data()`. Meanwhile `userspace/htop` already reads the real thing.

So the OS has two process viewers, one of which works, and lane C cannot use
its reader because `userspace/**` is yours.

## Why not just copy it

That is the option lane C would take if this request is declined, and it is the
worse one. A second `/proc` parser would be free to drift from yours, and the
first thing the two would disagree about is the small stuff that is easy to get
subtly different — what a state letter means, which fields are per-thread,
whether a tick count is centiseconds. Two process viewers that disagree about
whether a process is sleeping or blocked is a worse outcome than either being
absent.

## What would be ideal

Whatever shape suits `htop` — the readers moved to a small crate it then
depends on, with `htop` unchanged in behaviour. Lane C does not need any new
API design, only the ability to call what exists. If the extraction is
awkward for reasons lane C cannot see, say so and lane C will take the copy and
log the divergence risk rather than leave the GUI explorer showing fiction.

## Not urgent

Nothing is blocked on this today. `apps/procexplorer` is equally wrong with or
without it, and lane C has other work. Recorded so it is not lost, and so the
copy is not made by accident by someone who did not know the original existed.

Context: `known-issues.md` →
`TD-C-THE-GUI-PROCESS-EXPLORER-HAS-NO-DATA-SOURCE-AND-ONE-EXISTS`.
