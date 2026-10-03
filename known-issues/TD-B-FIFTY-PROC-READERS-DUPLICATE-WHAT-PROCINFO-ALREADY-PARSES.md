## TD-B-FIFTY-PROC-READERS-DUPLICATE-WHAT-PROCINFO-ALREADY-PARSES (lane B, 2026-09-10)

> **Renamed and corrected 2026-09-10.** This was
> `TD-B-TEN-PROC-PARSERS-IN-USERSPACE-AND-ONE-CRATE`, and **ten was wrong by a
> factor of five.** The correction is at the bottom, under "The count was
> wrong, and how", because the way it was wrong matters more than the number.

**In short:** ten programs in `userspace/` each parse `/proc` themselves. There
is now one crate that does it properly, and nine of them still do not use it.

**Found while** fulfilling
`requests/c-b-extract-htops-proc-readers-into-a-shared-crate.md`. Lane C's
argument for the crate was that *two* parsers would drift; the count inside
`userspace/` alone is ten, and twenty-four files across `userspace/` and
`apps/` touch `/proc` in some form.

**The ten:** `htop`, `ps`, `free`, `coreutils`'s `free`, `earlyoom`, `iostat`,
`hwinfo`, `lsmem`, `numactl`, `hwclock`.

### `/proc/diskstats` was a fourth family, and the copies disagreed (2026-09-12)

Not on the list above, because that list was built from `/proc/<pid>` and
`/proc/meminfo` readers. Three programs parsed `/proc/diskstats` privately --
`iostat`, `sysstat`, `vmstat` -- and the thing they disagreed about was the
size of a sector:

| | conversion | verdict |
|---|---|---|
| `sysstat` | `sectors * 0.5` -> kB | 512 bytes, hardcoded, **right** |
| `iostat` | `sectors * read_sector_size(dev)` | the DEVICE's sector size -- **8x over-report on a 4K drive** |
| `vmstat` | prints raw sectors | never wrong about the unit, because it never converted |

The block layer accounts in fixed 512-byte units whatever the device does, so
`/sys/block/<dev>/queue/hw_sector_size` -- 4096 on a great many modern drives
-- is not the unit these counters are in. **Latent on the development host,
where every device reports 512**, which is exactly why it survived.

Worth noting how it was found: not by hunting for the bug, but by asking which
programs still parse `/proc` themselves. The disagreement was provable without
leaving the repository -- two programs in one tree giving different byte counts
for one kernel counter, and one of them matching upstream sysstat.

They also disagreed about the WIDTH in a way that shows the drift directly:
`iostat`'s comment said kernels since 4.18 append columns and treated 14 as a
minimum; `vmstat`'s said "at least 14" and required exactly that. One format,
two copies, two different amounts of knowledge about it.

All three now use `procinfo::DiskStats` and `procinfo::DISKSTATS_SECTOR_BYTES`.

**Two of `vmstat`'s tests were testing `str::split_whitespace`.** They split a
line inside the test and asserted on the resulting `Vec`, never calling the
program's parser; the short-line one asserted that its own literal had fewer
than 14 fields and said in a comment that such a line "should be skipped",
with nothing checking that anything skipped it. Both now go through the parser
that runs, and a third covers a modern kernel's extra columns.

**Why it matters more than tidiness.** The things these disagree about are not
cosmetic:

* `/proc/<pid>/stat` reports RSS in **pages**, and SlateOS pages are 16 KiB
  where every published example assumes 4. `htop` and `ps` each carry a private
  `const PAGE_SIZE_KB: u64 = 16;`. Both are right today and nothing makes them
  stay right.
* The second field of `stat` is the command name in parentheses and may contain
  spaces *and* parentheses, so it must be found by the **last** `)`. A reader
  that splits on whitespace mis-numbers every field after it, for exactly the
  processes worth a second look.
* A command name is bytes. Reading it as UTF-8 silently drops processes.

**The fix** is to move each onto `procinfo`, which now covers memory, load,
uptime, cpuinfo, mounts and the per-process family. Not one change: each
program has its own output format and its own idea of which fields it needs,
and the useful unit is one program per commit.

**One down, nine to go.** `htop` moved completely on 2026-09-10, in three
steps: per-process reading (where the crate's per-process half came from), then
CPU, then memory, uptime and load. Its private `PAGE_SIZE_KB`, its `CpuStat`
and its `MemInfo` are gone with them. **`htop` no longer opens anything under
`/proc`** -- its one remaining `read_file` reads `/etc/passwd`.

Two things the crate had to grow to absorb it, both worth having anyway:

* `CpuTimes::since`, the subtraction a viewer needs before it divides. Its
  first consumer was htop's bar fix; `apps/procexplorer` needs the same.
* `MemInfo`'s swap fields, and a **second** used-memory figure.
  `used_kib` is `total - free`, the kernel's bookkeeping; `used_excluding_cache_kib`
  subtracts buffers and cache, which is the number `htop` and `free` show a
  person. Neither is wrong and every program that computed it privately picked
  one silently -- which is the whole shape of this entry.

**Two down, eight to go.** `ps` moved on 2026-09-10 and, like `htop`, no
longer opens anything under `/proc`. **No program in the tree now carries a
private `PAGE_SIZE_KB`.**

The crate grew three more things to absorb it:

* four more `/proc/<pid>/stat` fields -- `pgrp`, `session`, `tty_nr`,
  `starttime_ticks`;
* `ProcessStatus`, which **replaced** the `status_uid` free function rather
  than joining it. `ps` needs the GID, the supplementary groups and the `Vm*`
  figures from the same file, and a `status_gid`/`status_groups`/… beside
  `status_uid` would have been four scans of one file and four places to
  disagree about what `Uid:`'s four columns mean;
* `display_bytes`, moved out of `htop`. That one is the lesson of this tick:
  see below.

**`ps` was one edit away from a fourth private answer to the same question.**
Converted mechanically, its `comm` came out as `String::from_utf8_lossy`, which
`CLAUDE.md` item 7 forbids and which `htop` had already been given a correct
answer for a tick earlier. Two callers, two different renderings of the same
bytes, one day apart -- inside the crate that exists to stop exactly that.
`display_bytes` now lives in `procinfo` with its five tests, and its doc says
why a crate that disclaims formatting owns one formatter: the crate hands out
**bytes** on purpose, so every caller inherits the same problem, and one
correct answer is the point.

**Three down.** `coreutils`'s own `ps` moved on 2026-09-10 -- a reader the
original list did not contain at all, which is how the miscount came to light.
It gained two things it could not do for itself: the **real UID** (it was
`uid: 0` with a comment saying "would need `/proc/<pid>/status`", so `ps -f`
showed every process as root) and a `comm` that is not UTF-8 (`read_to_string`
failed and the process was skipped by `continue`).

### The count was wrong, and how

The original entry said **ten** readers. The real numbers, derived rather than
grepped for:

| | |
|---|---|
| files under `userspace/` and `apps/` that open a `/proc` path and do not use `procinfo` | **95** |
| of those, files opening something `procinfo` already parses | **50** |

**49 as of 2026-09-10.** `userspace/vmstat` is converted. It was worth taking
on contact rather than in a sweep, because it had the whole shape of the
problem in one file: its own `CpuTimes` struct, its own `/proc/stat` parser
beside it, and its own `cpu_total` and `cpu_delta` which were
`procinfo::CpuTimes::total` and `::since` field for field. Removing it needed
three new fields in `procinfo` (`intr`, `ctxt`, `btime`) and a `ProcFs::stat`
that returns the CPU lines and the counters from **one** read -- the two
existing accessors would have sampled the file twice per interval, so the CPU
delta and the context-switch delta would have described different instants.

**47 by the end of the same tick.** `userspace/uptime` and `userspace/hwclock`
each opened `/proc/stat` for `btime` alone, and `hwclock` hand-parsed
`/proc/uptime` beside it, so converting the two removed three parsers.

**46 with `userspace/pgrep`**, which was the one worth seeking out rather than
taking on contact: it read `/proc/<pid>/stat` through `read_to_string`, so a
process whose name is not UTF-8 was dropped from the listing entirely -- and
`pgrep` and `pkill` are the same binary, so such a process could not be
signalled by name at all. `userspace/top` followed the same day; `userspace/pstree` followed the same
day, which closes the set.

~~**Five more programs still read `/proc/<pid>/stat` by hand** -- `kill`,
`lsof`, `strace`, `sysstat` and `who`~~ -- found by running the grep rather than
assuming the three were all of them. **All converted 2026-09-10, and it was four,
not five.**

`strace` was on that list wrongly: its only reference to `/proc/<pid>/stat` is
`fs::metadata(&path).is_err()`, an existence check that never opens the file.
The grep that produced the list of five answered "which files mention this
path"; it was read as "which files parse it". The *same* narrowing that
produced the original "ten readers" figure this entry corrects, two paragraphs
up, made by the person who wrote that correction.

**Derived, not decremented:** no file under `userspace/` or `apps/` hand-parses
`/proc/<pid>/stat` any more -- checked by looking for the `find('(')`/`rfind(')')`
pair rather than for the path.

`kill`'s was the same shape as `pgrep`'s: `killall <name>` could not see a
process whose name is not UTF-8, so it could not be killed by name. `who -u`
lost the **idle time and PID columns** for such a login, not just the command
name -- those numbers were readable all along, on the same line as a name that
would not decode. `lsof` dropped the process *and every open file it held*,
which is the one thing `lsof` exists to report. Seven further hits were `/proc/<pid>/status`, a
different file: "stat" being a prefix of "status" is a trap for exactly this
kind of sweep.

**And the running count in this entry is a running count, not a measurement.**
The 50 above was derived; every figure since has been that number minus one per
conversion, and nothing re-derived it. What *is* measured, and re-measured each time
it is quoted: **82** files under `userspace/` and `apps/` open a `/proc` path
without `procinfo`, down from the 95 recorded above (86 and 83 earlier the same
day). The "opens something `procinfo` already parses"
subset has not been re-derived since, and should be before anyone quotes it.

**`pstree`'s version was the worst of the three.** A tree is assembled by
matching each process's `ppid` against a parent that has to be present, so one
process dropped for having an unreadable name took **every descendant with
it** -- an arbitrarily large subtree missing, with nothing to say so. In
`pgrep` and `top` the same defect loses one row.

`top` carried two more things worth naming. Its `COMMAND` column was
`&p.name[..16]` on a `String` -- **a panic** whenever byte 16 falls inside a
multi-byte character, so any process whose name held one non-ASCII character in
the wrong place would have killed the viewer as it drew its own list. And the
crate had **no tests at all**: a process viewer with a column rule, a sort and
a `/proc` parser, none of it asserted, which is how that slice sat there. It
has five now, covering the cut, the panic, a name that is not UTF-8, and a
terminal escape in a name -- which matters more in `top` than elsewhere because
it redraws every second, so an unescaped one is re-applied forever.

They also showed why "each program has its own copy" is not a neutral
arrangement even when every copy works. `uptime` stripped `"btime "` with the
trailing space; `hwclock` stripped `"btime"` without it, so a line named
`btimefoo` would have matched one and not the other. Neither is wrong on any
real `/proc/stat`. Two spellings of one rule, with nothing that could make them
disagree loudly enough for anyone to look.

Ten came from `grep -rln "/proc/stat\|/proc/meminfo"` -- a command that answers
*"which files mention these two paths"* -- and the answer was written down as
*"which files parse `/proc`"*. Every reader that touches only
`/proc/<pid>/...` was invisible to it, which is most of them, and
`coreutils`'s `ps` -- one of the two `ps` implementations in this tree -- was
among the missing.

**This is the same defect the entry is about, one level up.** A number derived
from a pattern that answered a narrower question than the one being asked, and
reported at the width of the question. It also travelled: the figure went into
a reply on lane C's request, so they were told ten as well. That reply is
corrected.

### What the real number changes

"One program per commit" is a plan for ten. For fifty it is not a plan, and
saying so is the useful part of the correction. What actually follows:

* **Convert on contact.** A file that is being edited for another reason moves;
  nobody schedules fifty commits.
* **The ones worth seeking out** are those whose parsing is *load-bearing and
  subtle* -- anything reading `/proc/<pid>/stat` (the `comm` field mis-numbers
  every field after it if split naively) or converting RSS pages (16 KiB here,
  4 KiB in every published example). `top`, `pgrep`, `pstree`, `w`, `who` and
  `vmstat` are in that set.
* **The other 45** mostly open one path for one purpose -- `/proc/self/exe`,
  `/proc/mounts`, `/proc/net/*` -- and are not duplicating `procinfo` at all.
  They are in the 95 and not in the 50, and lumping them together is what made
  the first number meaningless in the other direction.

Remaining: `ps`, `free`, `coreutils`'s `free`, `earlyoom`, `iostat`, `hwinfo`,
`lsmem`, `numactl`, `hwclock`. `ps` is the one that still carries its own
`const PAGE_SIZE_KB: u64 = 16;`.

**Not urgent, and worth saying why.** Every one of the ten works today. This is
the debt of ten right answers with nothing keeping them right, not a list of
bugs.
