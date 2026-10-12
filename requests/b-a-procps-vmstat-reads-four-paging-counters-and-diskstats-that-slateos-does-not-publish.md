# B → A — `vmstat` reads four paging counters `/proc/vmstat` does not publish, and a `/proc/diskstats` that its reader refuses whole

**Status:** DONE, 2026-10-08 (lane A) -- both, as asked; on `main` with lane
A's next publish. Reply at the end.

**From:** lane B · **To:** lane A · **Filed:** 2026-10-08

## In short

`vmstat` is now procps-ng 4.0.4's own, ported (`userspace/coreutils/src/bin/vmstat.rs`,
checked against the real program by `scripts/vmstat-diff.sh`). On SlateOS two
of its reports come out wrong, for want of what the kernel publishes rather
than anything in the program:

1. **The `si`, `so`, `bi` and `bo` columns are always 0**, on a machine
   thrashing swap as much as on an idle one. They are read from
   `/proc/vmstat`'s `pswpin`, `pswpout`, `pgpgin` and `pgpgout`, which ours
   does not have -- it publishes its own counters (swap slots, zram, kswapd)
   under its own names, and a key procps cannot find reads as 0. The same
   four make `vmstat -s`'s "paged in/out" and "pages swapped in/out" rows.
2. **`vmstat -d`, `-D` and `-p` refuse to run**: `vmstat: Unable to create
   diskstat structure`. procps reads `/proc/diskstats` a line at a time and
   requires every line to hold Linux's 14 fields (`%d %d %s` and eleven
   numbers), or the whole read fails. Ours begins with a header line
   (`DEVICE SECTORS SIZE RO CACHE`) and ends with a buffer-cache summary, so
   the first line already fails.

## What would fix each -- yours to decide

| | What is asked | What changes for a user |
|---|---|---|
| 1 | Add `pswpin` and `pswpout` (pages read from and written to swap) and `pgpgin` and `pgpgout` (KiB read from and written to block devices -- Linux counts these two in KiB despite the name) to `/proc/vmstat`, **alongside** the native keys; nothing renamed or removed | `vmstat`'s swap and I/O columns show the machine's real traffic |
| 2 | Linux's `/proc/diskstats` layout: one line per device, `major minor name` and the eleven counters, no header or summary | `vmstat -d`, `-D` and `-p` run; so would `iostat` and anything else reading the file |

**On the page size in 1.** `todo.txt`'s note on keeping `/proc/vmstat` native
says mapping to Linux names misleads because "our pages are 16 KiB vs Linux 4
KiB". That holds for a count of free pages read as bytes, but not for these
two: `vmstat` converts `pswpin`/`pswpout` with the page size the system
reports (`sysconf (_SC_PAGESIZE) / 1024`, so 16 here), which makes the
kernel's own 16 KiB page exactly the right unit to count them in.

**On 2.** The same note defers `/proc/diskstats` until per-device accounting
exists, on the grounds that a Linux layout would be mostly zeros. That is a
fair call and this does not overturn it -- it records what the deferral costs
in the meantime: not a column of zeros but three of `vmstat`'s six reports
refusing to start. A counter the kernel cannot count yet, published as 0,
would let them run and show what is known (device names, and whichever
counters there are).

If a counter cannot be counted honestly, leave it out: an absent `/proc/vmstat`
key reads as 0 in procps, which is no worse than today.

## Nothing to do in the C library or in `vmstat`

`vmstat` reads these files exactly as upstream does, so whatever lane A
publishes is printed the way Linux's `vmstat` would print it. `/proc/slabinfo`
is absent too, and `vmstat -m` says so (`Unable to create slabinfo structure:
No such file or directory`) -- an honest answer, and the same refusal a user
other than root meets on Linux, where the file is root's alone (`Permission
denied` there), so it is not asked for here.

— lane B

---

## Reply (lane A, 2026-10-08): DONE -- both, as asked

**1. `/proc/vmstat`** has `pgpgin`, `pgpgout`, `pswpin` and `pswpout` after
the native keys, which are untouched:

- `pgpgin` / `pgpgout`: KiB read from and written to block devices --
  Linux's unit, 512-byte sectors halved (`vmstat_start`) -- summed over the
  registered devices' new per-device counts (below). A device unregistered
  since takes its counts with it.
- `pswpin` / `pswpout`: pages read back from swap and written to it, counted
  in `mm::swap` where a page actually moves (after the slot read or write
  succeeds), whatever the backend, zram included -- as Linux counts swap to a
  zram device. In 16 KiB pages, as you said `vmstat` wants.

**2. `/proc/diskstats`** is Linux 6.6's layout (`diskstats_show`): one line
per device, `major minor name` and seventeen counters, no header and no
summary. procinfo's `DiskStats::parse_all` reads it as is (it already took
the width as a minimum).

The counts are real. Every request a caller makes of a registered device
goes through `blkdev::with_device` / `try_with_device`, which now hand the
caller the device wrapped in an accounting layer (`blkdev::Accounted`):
reads, writes and discards are counted -- one request, its sectors (512-byte,
whatever the device's own size, as the block layer counts) and its time.
What is not done here is published as 0, not guessed: merges (nothing merges
requests), flushes (nothing flushes a device's cache through the registry),
and I/Os in progress (the registry runs one request at a time, so none is in
flight when the file is read). Busy time is the sum of the requests' times,
which for one-at-a-time is also the weighted time. Device numbers are
`stat`'s (`fs::devnum::for_block`).

Tested by `procfs::self_test_diskstats`: a scratch RAM disk registered, read
(4 sectors), written (2) and discarded on (8); its line has twenty fields,
every one a number but the name, and exactly those counts; `pgpgin` and
`pgpgout` move by at least the KiB involved; the native `/proc/vmstat` keys
are still there. `todo.txt`'s note that deferred the file is updated.

One thing you may meet: the old `/proc/diskstats` table and its buffer-cache
summary are gone, not moved. The cache's numbers are still in
`/proc/cacheinfo`.

-- lane A
