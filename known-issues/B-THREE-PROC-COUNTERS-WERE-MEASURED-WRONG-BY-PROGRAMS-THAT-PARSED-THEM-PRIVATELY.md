## B-THREE-PROC-COUNTERS-WERE-MEASURED-WRONG-BY-PROGRAMS-THAT-PARSED-THEM-PRIVATELY (lane B, 2026-09-12) -- ALL FIXED

**In short:** asking *which programs still parse `/proc` themselves* -- not
looking for arithmetic errors -- turned up three wrong numbers. None of them is
visible reading one file. Each is a disagreement between two programs about one
kernel counter, so the comparison only exists across files, which is exactly
what a shared parser removes.

| Program | Counter | What it did | What a user saw |
|---|---|---|---|
| `iostat` | `/proc/diskstats` sectors | multiplied by `/sys/block/<dev>/queue/hw_sector_size` | **8x** the real throughput on a 4K-native drive |
| `sysstat` | `/proc/stat` cpu | added `guest` and `guest_nice` to the total | every percentage **too small** on a machine running VMs |
| `top` | `/proc/stat` cpu | never read `steal` at all | every percentage **too large**; idle 80% where the truth is 66.7% |

The last two are mirrors of each other. One added time the kernel had already
counted, inflating the denominator; the other omitted time the kernel does
publish, deflating it. Both came from a program deciding privately which
columns count.

**Each was provable without leaving the repository.** `sysstat` converted
diskstats sectors with `* 0.5` while `iostat` used the device's sector size --
two programs, one counter, different byte totals, and one of them had to be
wrong. `procinfo::CpuTimes` already documented "Running a guest. **Already
counted in `user`**" while `sysstat` added it anyway.

**Two of the three were defended by a test.** `sysstat`'s
`test_cpu_stat_total` asserted 1068 -- the eight real states summing to 1067,
plus guest a second time. `vmstat`'s two diskstats tests called
`split_whitespace` inside the test and asserted on the result, never touching
the program's parser; the short-line one asserted that its own literal had
fewer than 14 fields and said in a comment that such a line "should be
skipped", with nothing checking that anything skipped it.

**A fourth defect, found while converting.** `procinfo::CpuTimes::parse_line`
accepted any line beginning `cpu`, zero-filling absent columns. That leniency
is for columns the kernel ADDED later (`steal` in 2.6.11, `guest` in 2.6.24),
and it read `cpu 100` -- a truncated line -- as a CPU that is 100% user. Four
states is the fewest Linux has ever published, and is now the floor.

Three programs also disagreed about the diskstats WIDTH: `iostat` treated 14 as
a minimum and said in a comment that 4.18 appends columns; `vmstat` said "at
least 14" and required exactly that; `top` required seven `/proc/stat` columns
and would show no CPU row at all against an older kernel.

### What the sweep did NOT find, so nobody repeats it

* **`/proc/net/dev`** -- four private parsers (`ifconfig`, `ip`, `netstat`,
  `sysstat`), and all four agree that rx is field 0 and tx is field 8, which is
  the index `procinfo`'s own doc calls "the kind of index nobody re-derives".
  `netstat` indexes directly but guards `len() < 16` first.
* **`/proc/uptime`** -- ten readers, every one of them taking only the first
  field. The trap here is the second, which is idle time summed across ALL
  CPUs and can exceed uptime on a multi-core machine; nobody reads it.
* **`/proc/meminfo`** -- already consolidated; every reader goes through
  `procinfo`.

Consolidating those three families would be tidiness rather than repair, and
is not done for that reason.
