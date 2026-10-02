## B-HOST-MALLOC-NEVER-WORKED-SO-EVERY-ALLOCATING-TEST-PASSED-ON-ITS-OOM-PATH — 2026-08-21 — lane B — FIXED

**In short:** on host builds (`cargo test`), our `malloc` returned NULL for
every single call, always, and had done for as long as it existed. NULL is a
*legal* `malloc` result, so nothing looked broken: every libc function that
allocates quietly took its out-of-memory branch, returned the error POSIX says
it should, and its test asserted that error and passed. `free`, `realloc` and
`malloc_usable_size` were never reached at all on the host. The test suite was
green and was testing the wrong half of every allocating function.

**Where.** `posix/src/malloc.rs`; root cause in `posix/src/syscall.rs` ~line 414.

**Why.** `syscallN()` deliberately returns `HOST_ENOSYS` (-38) on
`not(target_os = "none")` — see its "Host-build safety gate" comment, and it is
correct to do so: a raw `SYSCALL` instruction on a Windows host dispatches into
NT and would do something arbitrary. But `malloc` is built directly on
`mman::mmap`, so the gate made every `malloc` fail rather than making it
unavailable. The gate was doing its job; the allocator had no host path.

**How it stayed hidden for so long.** Three tests had written the failure down
as the *expected* result — `malloc_small_returns_null_in_test`,
`malloc_page_size_returns_null_in_test` and `reallocarray_one_one_null`, the
last two even carrying comments explaining that mmap fails in test mode. A test
that asserts a component does not work will never report that the component does
not work. This is the same shape as §355's stamp gate and as the per-directory
`.gitignore` rules: a rule that documents its own exception stops being a rule.

**Found by.** Not by anything looking for it. Fixing
`BUG-POSIX-SPAWN-FILE-ACTIONS-IS-4624-BYTES-IN-AN-80-BYTE-SLOT` above moved the
file-actions array onto the heap, and eleven spawn tests failed with `ENOMEM` at
once. It took adding an allocation to a path that previously had none to make
the allocator's absence observable.

**Fix.** `map_region`/`unmap_region` now abstract the backing store, `mman::mmap`
on target and `std::alloc::alloc_zeroed` at `REGION_ALIGN` on the host — the
same host-shim shape already used by `host_clock` in syscall.rs and
`host_eventfd_sim` in epoll.rs. The three tests now assert real round-trips
(allocate, write both ends, `malloc_usable_size`, `free`).

**Consequence to keep in mind.** Every allocating function in `posix/` is now
being tested on a code path it had never executed on the host. The full suite
passes, but "passed before and passes now" means something different for these
than for the rest of the tree: before, most of them never got past the first
`malloc`.
