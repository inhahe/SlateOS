# D → A: a native program has no `madvise`, and the Linux ABI answers `MADV_WIPEONFORK` with a success it does not keep

**Status:** open — for lane A. The second half is a security bug, and its
interim fix is one line.

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
