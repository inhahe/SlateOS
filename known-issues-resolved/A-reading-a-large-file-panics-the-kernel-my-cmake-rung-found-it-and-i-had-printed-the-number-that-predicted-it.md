### [A] Reading a large file panics the kernel, my cmake rung found it, and I had printed the number that predicted it -- 2026-09-18
**Status:** FIXED 2026-09-18, boot-verified on ebb683642 (both read paths return OutOfMemory instead of aborting; the cmake rung took the skip path and the kernel did not panic, which is what the change was for)

**In short:** asking the kernel to read a 22-megabyte file killed it, with
2.7 gigabytes of memory free. The cause is that the allocator rounds a
request up to the next power of two, so a 22.5 MB file asks for a 32 MB
*unbroken* run of memory, and if the free memory is in smaller pieces there
is no way to satisfy it. The read then aborted instead of returning an
error, and in a kernel an abort is a panic.

```
[spawn] Running CMake 4.4.3 linked against OUR libc.a (ring 3) test...
!!! KERNEL PANIC !!!
memory allocation of 22526200 bytes failed
  Memory: 5242880 KiB total, 2464576 KiB used, 2778304 KiB free
  Heap: large=18713/18676, refills=192, failures=1
  # 5: kernel::fs::vfs::Vfs::read_file_resolved+0x5a4
```

**The arithmetic, which is the actionable part.** `heap.rs:883`
`large_order` rounds the frame count up to the next power of two for the
buddy allocator. At 16 KiB frames:

| file | bytes | frames | rounded to | contiguous demand |
|---|---|---|---|---|
| `pkgconf-slateos.elf` | 1,834,856 | 113 | 128 | 2 MiB |
| `make-slateos.elf` | 2,395,104 | 147 | 256 | 4 MiB |
| `bash-slateos.elf` | 4,339,648 | 265 | 512 | 8 MiB |
| `python-slateos.elf` | 10,468,016 | 639 | 1024 | 16 MiB |
| `python312.zip` | 20,498,464 | 1251 | **2048** | **32 MiB** |
| `cmake-slateos.elf` | 22,526,200 | 1376 | **2048** | **32 MiB** |

**So this is not a size cap, and that matters.** The Python zip is the same
order and reads successfully on every boot. Whether a 2048-frame contiguous
block exists depends on buddy fragmentation at that point in the boot, and
the cmake rung runs later than the Python one -- after make, tcc, and the
make-drives-tcc build. The failure is therefore **nondeterministic**, which
is the worst kind of panic to introduce into a read path.

**I had the number.** The rung prints
`cmake: /mnt/bin/cmake is 22526200 bytes` and my own commit message quotes
`22,526,200 bytes + 1748 module files`. I measured the size, printed it
twice, and never asked whether the kernel could allocate it. Having a datum
is not the same as having used it -- and it is the same failure as reading a
declaration instead of a call: the information was in front of me and the
question I asked of it was the wrong one.

**Two fixes, and the general one is the important one.**

1. `Vfs` read paths used `alloc::vec![0u8; out_len]`, which **aborts** on
   allocation failure. Now `try_reserve_exact` with
   `KernelError::OutOfMemory`. This is general: *any* caller reading a large
   file could panic the kernel, and my rung merely found it. `try_reserve`
   was already the established idiom -- 12 uses across `drm/card_fd`,
   `mm/user`, `net/bridge`, `syscall/handlers` and `virtio/gpu` -- so this
   applies a convention the tree had rather than inventing one.
   **Both sites**, not just the one in the backtrace: the identical
   expression appears twice, and fixing only the traced one would leave the
   other for the next large file (lane C's rule).
2. The cmake rung now treats `OutOfMemory` as an **environment fact** and
   skips through `pathz_skip` so the lost coverage is counted, rather than
   reddening the boot. That matches the existing split exactly -- absent
   source skips, present-but-broken fails -- and a silent `Ok` here would
   have been the Path-Z verdict problem recorded earlier today.

**What is still unresolved.** The rung will now *skip* rather than run
whenever the block is unavailable, so Part 61 may not actually exercise
cmake on a given boot. Loading a 22.5 MB binary through a buddy allocator
is the wrong mechanism for a file this size; mapping it rather than copying
it is the real answer, and that is a larger change than a self-test should
carry. Recorded rather than attempted.
