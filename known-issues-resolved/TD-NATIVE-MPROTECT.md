### TD-NATIVE-MPROTECT. No native `SYS_MPROTECT` handler — native `mprotect()` returns ENOTSUP — 2026-07-21 — ✅ RESOLVED 2026-07-22

**What:** The posix libc's native syscall path exposes `SYS_MPROTECT = 22`
(kernel/src/syscall/number.rs), but no handler is registered for it in the
*native* dispatch table (kernel/src/syscall/dispatch.rs). A native
`mprotect()` therefore resolves to NotSupported → `ENOTSUP`. Only the
**Linux-ABI** mprotect (linux.rs, Linux syscall #10 — `sys_mprotect`) is
implemented, and it's fully functional for the Linux compatibility path.

**Proper fix:** Register a native mprotect handler at 22 that reuses the
Linux mprotect core (`sys_mprotect`'s VMA-coverage + per-4KiB PTE walk) but
returns `KernelError` codes rather than Linux errno, so posix's
`errno::translate` maps it uniformly with the rest of the native ABI.
Options: (a) factor the mprotect core into a `fn -> KernelResult` shared by
both the Linux and native handlers (cleanest, preferred); or (b) register the
existing `sys_mprotect` at 22 and switch posix's `mman::mprotect` to a
Linux-style errno translate (posix errno values already equal Linux errno
values). Tracked in todo.txt.

**Impact:** Low for current work — the fastpy/initiative-F binaries and the
crt TLS path do not call mprotect. glibc RELRO / pthread guard pages use the
Linux ABI path, which is unaffected.

**Fix (2026-07-22):** Took option (a) — the cleaner, DRY route. Factored the
mprotect implementation into an ABI-neutral core in `kernel/src/syscall/linux.rs`:
- `mprotect_validate_core(addr,len,prot) -> KernelResult<Option<u64>>` runs the
  Linux `do_mprotect_pkey` gate order (align/len==0/overflow/prot-bits/userspace)
  and now returns `KernelError` (`InvalidArgument`/`OutOfMemory`) instead of a
  Linux-errno `SyscallResult`. A thin `mprotect_validate_args` wrapper still
  produces the `MprotectValidation` form for `sys_pkey_mprotect` (unchanged).
- `pub(crate) fn mprotect_core(addr,len,prot) -> KernelResult<()>` holds all the
  page-table work (VMA-coverage check, `protect_vma_range` bookkeeping, per-4 KiB
  PTE `change_flags_4k`, single batched `mprotect_flush_range` shootdown), now
  propagating `KernelError` via `?` and returning `Ok(())` on success.
- `sys_mprotect` (Linux ABI, syscall #10) is now a 4-line wrapper mapping
  `Err(e) => linux_err(linux_errno_for(e))`.
- New native handler `syscall::handlers::sys_mprotect` calls the same
  `mprotect_core` and returns raw `KernelError` codes; registered at
  `SYS_MPROTECT` = 22 in `syscall::dispatch`. posix `mman::mprotect`'s
  `errno::translate(syscall3(SYS_MPROTECT,…))` now maps 0→success,
  `InvalidArgument`→EINVAL, `OutOfMemory`→ENOMEM, `NoSuchProcess`→ESRCH — no more
  ENOTSUP.

Both ABIs share one implementation of the actual work, so there is no drift.
Verified by a new native dispatch self-test `test_dispatch_mprotect_native`
(proves the handler is registered — not `NotSupported` — and that the gate order
returns native error codes for misalign/len0/bad-prot/overflow) plus the
pre-existing Linux `self_test_mprotect_validation`/`self_test_mprotect_flush_range`
which continue to exercise the shared core.
