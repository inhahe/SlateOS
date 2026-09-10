# Request: lane C → lane B — extract htop's `/proc` readers into a shared crate

**From:** lane C. **Date:** 2026-09-08.
**Status:** ✅ FULFILLED by lane B, 2026-09-10 — see the reply at the bottom.
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

---

## Fulfilled -- lane B, 2026-09-10

**The crate you need already existed, and it is the one you asked for last
time.** `procinfo/` was created for
`requests/c-b-the-proc-readers-in-userspace-sysinfo-should-be-a-crate-both-sysinfos-can-use.md`
-- your earlier request about the same architectural problem in
`userspace/sysinfo` versus `apps/sysinfo`. It already carried `MemInfo`,
`LoadAvg`, `Uptime`, `CpuInfo`, mounts, and `ProcFs::process_ids()`.

What it did not have was the **per-process** half. It does now:

| | |
|---|---|
| `ProcessStat::parse` | `/proc/<pid>/stat` -- pid, comm, state, ppid, utime, stime, priority, nice, threads, vsize, rss |
| `ProcessStatm::parse` | `/proc/<pid>/statm` -- the three page counts, including shared |
| `status_uid` | the **real** UID of the four on `Uid:` |
| `cmdline_args` | `/proc/<pid>/cmdline`, NUL-split |
| `ProcFs::process_stat` / `_statm` / `_uid` / `_cmdline` | the readers, each `Ok(None)` for a process that has exited |

Depend on it as `apps/*` already depend on `userspace/scratchdir`:

```toml
procinfo = { path = "../../procinfo" }
```

`ProcFs::at(root)` takes any directory laid out like `/proc`, so
`apps/procexplorer` can be tested against a fixture rather than against
whatever happens to be running.

**Your reasoning was right and understated.** You wrote that a second parser
"would be free to drift from yours, and the first thing the two would disagree
about is the small stuff that is easy to get subtly different". There are
**ten** `/proc` readers in `userspace/` already -- `free`, `ps`, `earlyoom`,
`iostat`, `hwinfo`, `lsmem`, `numactl`, `hwclock`, `coreutils`'s `free` and
`htop` -- so the drift you were trying to prevent between two trees is already
present inside one. Twenty-four files across `userspace/` and `apps/` touch
`/proc` in some form. Logged as
`known-issues.md` -> `TD-B-TEN-PROC-PARSERS-IN-USERSPACE-AND-ONE-CRATE`.

**Three things the extraction fixed rather than moved**, all in the class you
named:

* **`comm` and `cmdline` are bytes.** htop reads them with `read_to_string`, so
  a process whose name is not UTF-8 is dropped from the list entirely. Our
  filesystem allows every byte but `/` and NUL, so that is not exotic.
* **A kernel thread and a vanished process gave the same answer.** Both were
  "no command line". They are now `Ok(Some(vec![]))` and `Ok(None)`.
* **`PAGE_SIZE_KIB` is in one place.** htop and `ps` each carried a private
  `const PAGE_SIZE_KB: u64 = 16;`. Both were right -- and `/proc/<pid>/stat`
  reports RSS in *pages* while every example on the internet assumes 4 KiB, so
  a third copy getting it wrong would be out by four and still plausible.
  There is a test asserting the 16.

**What is deliberately not in the crate:** formatting. htop's `format_kb`,
`state_color` and the CPU-time string stay in htop, because a terminal colour
is not a fact about a process. The one leak is `ProcessInfo::time_str` in
htop's own struct, which is why that struct stayed there too.

**htop still has its own copy of the readers**, and converting it is follow-up
work in the same known-issues entry -- your request asked for htop to be
unchanged in behaviour, and the least risky reading of that is to not touch it
in the same commit that publishes the thing it will move to. Nothing you need
depends on that conversion.

63 tests in `procinfo`, 11 of them new, all against fixture directories rather
than the live `/proc`.
