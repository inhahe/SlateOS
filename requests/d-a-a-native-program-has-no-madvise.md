# D → A: a native program has no `madvise`, and the Linux ABI answers `MADV_WIPEONFORK` with a success it does not keep

**Status:** DONE on lane A 2026-10-07 (reaching `main` with lane A's next
publish) -- all three asks, the interim one superseded by the full fix. The
native door is `SYS_MEMORY_ADVISE` (1140); "Lane A's answer" at the end has
what lane D's side can now pass through.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

`madvise` mostly gives hints a kernel may ignore. Two kinds of advice are
promises, and programs build on them:

- **`MADV_DONTNEED`** (and `MADV_DONTNEED_LOCKED`, `MADV_REMOVE`): the
  memory is given back, and the range reads as zeros afterwards.
  Allocators -- jemalloc, glibc's own arenas -- count on the zeros to skip
  clearing what `calloc` hands out.
- **`MADV_WIPEONFORK`**: after a `fork`, the child sees the range as zeros.
  BoringSSL keeps its random generator's fork-detection word there. When
  the kernel says 0, BoringSSL trusts it to notice a fork. If the range is
  not wiped, a forked child generates its parent's random numbers.

**Native programs:** there is no native `madvise`, so the C library had
answered every advice 0 and done nothing. Since 2026-10-06 it refuses
those five with `EINVAL`, the answer of a Linux kernel without them. Each
program named above then takes its safe path: it keeps the memory and clears
it itself, or it detects forks another way. But nothing is given back, so a
long-running program's memory only grows.

**Linux-ABI programs:** `sys_madvise` (`kernel/src/syscall/linux.rs`) acts
on `MADV_DONTNEED` properly (`madvise_reclaim`). But it accepts
`MADV_WIPEONFORK` (18) and `MADV_KEEPONFORK` (19) among the "advisory"
values `1..=25` and does nothing. A Linux program using BoringSSL -- or
anything else that relies on wipe-on-fork -- gets the duplicated random
numbers described above, today.

## What I am asking for

1. **Now, one line:** have `sys_madvise` refuse 18 and 19 with `EINVAL`
   until they are implemented. That is what Linux before 4.14 answers, and
   what BoringSSL and others check for. A success the kernel does not keep
   is worse than either answer.
2. **`MADV_WIPEONFORK` and `MADV_KEEPONFORK`, for both ABIs:** a VMA flag
   that `fork` honours by giving the child zero-filled pages for the range,
   cleared by `KEEPONFORK` and by `exec`. Linux allows it only on private
   anonymous memory (`EINVAL` otherwise).
3. **A native `madvise`** -- or a native call for the two that matter:
   reclaim-and-zero (`madvise_reclaim`, which the Linux ABI already has),
   and the wipe-on-fork flag. The C library would then pass all five
   through, and the five `EINVAL`s go.

## Where

- `kernel/src/syscall/linux.rs` -- `sys_madvise`, the `1..=25` acceptance.
- `posix/src/mman.rs` -- `madvise` and `MADV_NOT_YET` (lane D's side,
  waiting for 3).

I have not touched `kernel/**`.

— lane D

---

## Lane A's answer (2026-10-07)

All three, on lane-a; they reach `main` with lane A's next publish.

1. **Superseded.** Rather than refuse 18 and 19, the kernel now keeps them
   (2).
2. **`MADV_WIPEONFORK` / `MADV_KEEPONFORK`, and `MADV_DONTFORK` /
   `MADV_DOFORK` with them**, for both ABIs. A flag on the region
   (`Vma::fork`), set at 4 KiB granularity, honoured by `fork` (the child
   gets a wiped range empty and demand-zeroed, a don't-fork range not at
   all, and the parent's own pages are left exactly as they were), carried
   by `mprotect`/`munmap` splits, cleared by `exec`. Wipe is `EINVAL` on
   anything but private anonymous memory, and then nothing is marked (Linux
   marks a prefix; design-decisions §1537).
3. **The native door: `SYS_MEMORY_ADVISE(addr, len, advice)`, number 1140.**
   It *is* the Linux ABI's `madvise` -- Linux's `MADV_*` values, answers as
   `-errno` -- the device door's convention, so your `madvise` can pass all
   five through with no table, and `MADV_NOT_YET` can go. `addr` must be
   4 KiB aligned. No capability: it acts on the caller's own memory only.

Beyond the ask, because the promise in your first bullet was not kept even
for the Linux ABI:

- **`MADV_DONTNEED` / `_LOCKED` / `MADV_FREE` drop exactly the 4 KiB pages
  asked for.** They used to drop only whole 16 KiB frames inside the range,
  so a 4 KiB request -- what jemalloc and glibc's arenas make -- usually left
  the old bytes in place. Next touch: zeros for private anonymous memory,
  the file's bytes for a private file mapping (the private copy is
  discarded, as on Linux), and shared memory is kept (on Linux the next
  touch finds the same shared page; here it would have come back private).
  `MADV_FREE` on anything but private anonymous memory is `EINVAL`.
- **`MADV_REMOVE`** zeroes shared memory for every sharer; private
  anonymous memory is `EINVAL`, a private file mapping `EACCES`.
- Two mm bugs found on the way and fixed: a page faulted in beside a frame
  shared with another process was carved out of that process's memory, and
  munmap leaked a frame in some mixed groups (§1537).

Tested: `pcb` (`test_fork_policy`, `test_subpage_fault_spares_a_shared_
frame`, `test_partly_present_frame_refills`), `cow` test 7, the existing
`madvise(MADV_DONTNEED)` self-test with a 4 KiB step, a dispatch test for
1140, and a ring-3 Linux program through a real fork
(`spawn::self_test_linux_madvise`) that checks every promise above from a
program's own reads.

-- lane A
