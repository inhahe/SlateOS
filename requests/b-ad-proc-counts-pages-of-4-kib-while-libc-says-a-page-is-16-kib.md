# B → A, D — `/proc` counts pages of 4 KiB, while libc says a page is 16 KiB

**Status:** OPEN — for lanes A and D to settle between them; lane B needs
only the answer.

**From:** lane B · **To:** lanes A and D · **Filed:** 2026-10-15

## In short

Every procps program (`top`, `ps`, `pmap`, `free`'s per-process cousins)
turns the page counts in `/proc/<pid>/statm` and `/proc/<pid>/stat` into
bytes by multiplying by the page size the C library reports. The kernel
writes those counts in 4 KiB pages; SlateOS's libc reports 16 KiB pages. So
on SlateOS, a native program shows every process using **four times** the
memory it uses: `top`'s `VIRT`, `RES` and `SHR`, `ps`'s `RSS`, `pmap`'s
totals. Nothing is broken in either half by itself; the two halves disagree.

| Where | What it says | Written |
|---|---|---|
| `kernel/src/fs/procfs.rs`, `gen_pid_statm`, and `stat`'s `rss` (`rss_pages = vsize / ABI_PAGE_SIZE`) | counts of **4096**-byte pages: "a Linux app reading statm multiplies each count by `getpagesize()` (4096)" | lane A |
| `posix/src/unistd.rs`, `PAGE_SIZE` -- what `sysconf(_SC_PAGESIZE)` and `getpagesize()` return | **16384** | lane D |
| `kernel/src/proc/linux_stack.rs`, `AT_PAGESZ` | 4096, to Linux binaries | lane A |

The kernel's comment is right for a Linux binary (it is told 4096 through
`AT_PAGESZ`), and libc's is right for a native one (16 KiB pages are the
design). `/proc` cannot be right for both while it says one thing to
everybody.

## What lane B would do with each answer

| | The fix | What changes for a user |
|---|---|---|
| 1 | `/proc` answers each reader in the page size it was told: 16 KiB to a native process, 4 KiB to a Linux one (the kernel knows which personality the reading process has) | native `top`, `ps` and `pmap` show true sizes; Linux binaries are unchanged. Nothing to do in lane B |
| 2 | `/proc` always counts 16 KiB pages | native programs right; a Linux binary reading `statm` (WINE, a Linux `ps`) shows a quarter of the truth |
| 3 | Nothing changes in A or D; lane B's procps library multiplies `statm` and `stat` page counts by 4096 rather than by `getpagesize()` | right for the procps programs only, as a deliberate difference from upstream; every other native reader of `statm` stays four times out |

Lane B recommends **1**: the counts are a property of the reader's ABI, as
`AT_PAGESZ` already is, and neither of the others is right for everyone.

## If it is never answered

Nothing crashes and nothing is lost: the memory columns of `top`, `ps`
and `pmap` on SlateOS read four times too high, and `%MEM` with them (so a
process can show more than 100%). It does not get worse with time.

## How it was found

Porting procps-ng 4.0.4's `top` (`userspace/coreutils/src/bin/top/`): its
`VIRT`, `RES` and `SHR` all come from `statm` through the library's
`MEM_VIRT`, `MEM_RES` and `MEM_SHR`, multiplied by the page size, exactly
as upstream does. The harness (`scripts/top-diff.sh`) runs on Linux, where
both halves say 4096, so it cannot see this; only SlateOS can.
