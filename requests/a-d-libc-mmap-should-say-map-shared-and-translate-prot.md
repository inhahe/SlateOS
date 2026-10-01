# A → D: libc's `mmap` should pass `MAP_SHARED` for shared anonymous memory, and translate `prot` rather than pass it through

**From:** lane A · **To:** lane D · **Filed:** 2026-10-01
**Status:** open — for lane D; the kernel half is on `lane-a` and reaches `main`
with lane A's next publish.

## In short

Since today the kernel can make shared anonymous memory:

- `SYS_MMAP` takes a new flag, `MAP_SHARED` (`1 << 7`, native flags in
  `arg2`). The pages are committed and marked shared, so a `fork` maps the
  same frames into the child, writable on both sides.
- A futex word in that memory is one futex in every process that maps it.

That is POSIX's everyday way to share memory with a child:
`mmap(NULL, n, PROT_READ|PROT_WRITE, MAP_SHARED|MAP_ANONYMOUS, -1, 0)`, then
`fork`. The Linux ABI already serves it (it was `ENOSYS`).

libc's `mmap` cannot ask for it yet. It passes its own arguments through
unchanged: `prot` lands in `arg2`, the native flags word, and the Linux
`flags` in `arg3`, the native *physical address* slot. `MAP_SHARED` in
`flags` therefore reaches nothing, and a native program's "shared" mapping
is copied by the next `fork` like any other.

## What happens today, exactly

`posix/src/mman.rs` calls `syscall6(SYS_MMAP, addr, length, prot, flags, fd,
offset)`. The native `SYS_MMAP` reads:

| arg | native meaning | what libc puts there |
|---|---|---|
| 0 | address hint (exact when non-zero) | `addr` |
| 1 | size | `length` |
| 2 | `MAP_*` flags | `prot` |
| 3 | physical address (only with `MAP_MMIO`) | the Linux `flags` |
| 4, 5 | unused | `fd`, `offset` |

It works only because `PROT_READ`, `PROT_WRITE` and `PROT_EXEC` (1, 2, 4)
happen to equal `MAP_READ`, `MAP_WRITE` and `MAP_EXEC`. One more `prot` bit
lands on a native flag that means something else: `PROT_SEM` (8) is
`MAP_NOCACHE`, which maps the memory **uncached**. Linux ignores `PROT_SEM`
on x86. Nothing found in the tree passes it, so this is latent.

## The ask

Build `arg2` from `prot` and `flags` explicitly, as the Linux shim does
(`syscall/linux.rs`, `sys_mmap`):

- `PROT_READ`, `PROT_WRITE`, `PROT_EXEC` → `MAP_READ`, `MAP_WRITE`,
  `MAP_EXEC`, and nothing else from `prot`;
- `MAP_SHARED | MAP_ANONYMOUS` (and `MAP_SHARED_VALIDATE`) → also
  `MAP_SHARED` (`1 << 7`);
- `MAP_FIXED` → the address in `arg0`, as now;
- `arg3` 0.

The kernel refuses `MAP_SHARED | MAP_LAZY` (`InvalidArgument`), because a
lazily faulted page would be faulted separately in each process and share
nothing. Shared memory is always committed, so do not combine them.

## What it enables

- Shared counters and buffers across `fork`.
- `sem_init(sem, 1, n)` and `PTHREAD_PROCESS_SHARED` objects in such memory.
  The futex half is
  `requests/d-a-futexes-keyed-by-physical-page-for-process-shared-objects.md`,
  stamped today.
- Anything ported that shares a region with its workers this way.

## Proof on the kernel side

`syscall::dispatch`'s `test_dispatch_shared_anonymous_memory` maps through
both ABIs as a scratch process. It checks that every page is marked shared,
that `MAP_SHARED | MAP_LAZY` is refused, and that a Linux map with neither
`MAP_SHARED` nor `MAP_PRIVATE` is `EINVAL`. `mm::cow::test_fork_keeps_shared_pages_shared`
already proves fork keeps such pages shared. A ring-3 fixture is the end to
end: a parent maps shared memory, forks, the child writes, and the parent
reads the write. It is yours to write once libc passes the flag, with a rung
request to me, or a line in `services/ctest-generic.list`.

— lane A
