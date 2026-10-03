### D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE — 2026-08-13 — TECH DEBT (blocked enabling SMAP) — ✅ FIXED 2026-08-13 (CR4.SMAP is on)

**What.** Roughly 100 syscall handlers in `kernel/src/syscall/handlers.rs`,
`kernel/src/syscall/linux.rs` and `kernel/src/ipc/io_ring.rs` follow this shape:

```rust
if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) { ... }
// SAFETY: Buffer validated above — in user space, mapped, writable.
let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) };
match pipe::read(handle, buf) { ... }
```

That is, they validate a user pointer and then construct a Rust slice *over the
user virtual address itself* and pass it into arbitrary kernel code. `mm::user`
is not involved beyond validation.

**Why this blocks SMAP.** With CR4.SMAP set, every one of those accesses faults:
they are supervisor-mode reads/writes of user pages performed outside any
STAC/CLAC window. This is what `smep_smap::USER_ACCESSES_ANNOTATED = false`
stood for, and it is why `smap_enable_blocker()` kept refusing to set the bit
even after `B-AC-INHERITED-AT-KERNEL-ENTRY` was fixed.

**Why the obvious fix is wrong.** Wrapping each site in `stac()`/`clac()` would
make it *compile and boot*, and would be a serious bug. `pipe::read` blocks: it
registers a waiter and reschedules with the slice still live. So a `stac()` held
across it would (a) leave AC = 1 in the task's saved RFLAGS, so SMAP stays
disabled for that task across the context switch and the scheduler itself runs
with the override on, and (b) hold the window open for an unbounded time. The
STAC window must stay inside a single non-blocking copy, which is exactly what
`mm::user::copy_{from,to}_user` already does.

**The independent bug underneath.** Even with SMAP off, holding a raw user slice
across a blocking call is a TOCTOU use-after-free: another thread in the same
process can `munmap`/`mremap` the range while the caller sleeps, and the kernel
then writes through a stale user mapping — into whatever now owns that physical
page. SMAP would merely convert this from silent corruption into a fault. So
this is worth fixing on its own merits, independent of SMAP.

**Proper fix.** Handlers must not hand user virtual addresses to kernel
subsystems. Either:

1. **Bounce through a kernel buffer** — `copy_from_user` into kernel memory,
   call the subsystem, `copy_to_user` the result back. Correct everywhere,
   costs a copy on paths that currently have none.
2. **Use the `_as` accessors** (`mm::user::copy_{from,to}_user_as`) which
   resolve the user VA to a physical frame and access it through the HHDM.
   Those are supervisor mappings, so SMAP does not apply and no STAC window is
   needed at all — but the frame must be pinned for the duration, which is the
   part that does not exist yet.

(2) is the better end state for large transfers; (1) is right for the many
small ones. Either way this is a mechanical but wide refactor across ~100 call
sites, and it wants a pinning primitive before (2) is available.

**Where.** `kernel/src/syscall/handlers.rs` (~60 sites — grep
`from_raw_parts`), `kernel/src/syscall/linux.rs`, `kernel/src/ipc/io_ring.rs`;
gate at `kernel/src/smep_smap.rs::USER_ACCESSES_ANNOTATED`.

**Approach taken.** Option (1), the bounce, everywhere. Option (2) still wants
a frame-pinning primitive that does not exist; revisit it for the large
transfers once one does. Four shapes recur:

- **write** — `read_user_vec(ptr, len, MAX)` into a kernel `Vec`, then call the
  subsystem with `&data`.
- **read** — `with_user_out_buf(ptr, cap, MAX, |buf| subsystem_read(buf))`.
- **recv-then-copy** — keep the up-front `validate_user_write` so a bad
  destination costs the caller an error rather than a *dequeued* message with
  nowhere to go, then `copy_to_user`, which re-validates at the moment of the
  store. The second check is the one that matters: the dequeue blocks.
- **record packing** — fill a kernel buffer sized by what the kernel will
  actually emit (never by the caller's advertised capacity), then one
  `copy_to_user` for the batch. Storing records one at a time meant a fault
  partway through left the caller with a partial answer it could not detect.

**Progress (2026-08-13).** `handlers.rs` is done. Verified by grep, not by
recollection — the first time this entry claimed "done" the grep immediately
disproved it, which is why the check is recorded here:

```
grep -nE 'as \*const u8|as \*mut u8|as \*mut u64|as \*mut u32|as \*mut i32|copy_nonoverlapping|ptr::write|write_bytes|read_volatile|write_volatile|from_raw_parts' kernel/src/syscall/handlers.rs
```

now yields six hits, all benign: five are prose inside comments describing the
code that *used* to be there, and `handlers.rs:3655` takes a slice over a
`SpawnArgsHeader` living on the **kernel** stack, which is not a user access at
all. So: no `from_raw_parts` over a user address, no
`core::ptr::write`/`copy_nonoverlapping` through a user pointer, and no
raw-pointer locals derived from a syscall argument.

The last batch was the six net handlers, which all *did* validate first and so
were easy to skim past — validation is necessary but it is not the property
being restored here. Four inbound record readers (`sys_net_if_config`,
`sys_net_route_add`, `sys_net_route_del`, `sys_net_fw_add_rule`) became
`read_user_value::<[u8; REC_SIZE]>` — a `[u8; N]` has alignment 1, so the
fixed-size ABI record decodes through the bounce with no cast and no indexing.
`sys_tcp_info` became a single `copy_to_user`. `sys_net_route_list` was the only
one needing real restructuring: it stored records one at a time through
`buf_ptr as *mut u8`, so a fault on record 5 of 9 left the caller holding a
partial table *and* a return value claiming all nine — now packed in the kernel
and delivered as one copy, with the count reported from what was actually
packed.

`kernel/src/ipc/io_ring.rs` is done too, and was a different animal: not one of
its fifteen user accesses validated *anything*, so the conversion was a security
fix rather than a SMAP preparation — see
`B-IO-RING-SUBMISSION-PATH-WAS-UNGATED-AND-UNVALIDATED` above, which also covers
the three unrelated holes reading the file closely turned up (blocking with a
global spinlock held, no ownership checks, an HHDM address published to ring 3).
The lesson generalises: the files still to convert should be read as *audits*,
not as find-and-replace.

`kernel/src/syscall/linux.rs` and `kernel/src/drm/syscall.rs` are done as of
2026-08-13. `linux.rs` was much smaller than the raw grep suggested — of 23
hits, most were HHDM addresses in self-test code or prose inside comments, and
only eight were genuine user accesses. They fell into three groups:

- **wait/rusage** (`sys_waitid`, `sys_wait4`). Every one of these carried a
  SAFETY comment of the form *"validated as a writable user range before the
  wait began and the address space has not changed"* — wrong twice over, since
  the wait is exactly what blocks and a peer thread can `munmap` the range
  while the caller sleeps. Now `write_user_value` / a shared
  `clear_user_rusage`. The encoder was split out of `write_waitid_siginfo` as a
  pure `waitid_siginfo` so the byte-layout self-test — which fed it a *kernel*
  stack buffer — no longer drives the user-delivery path at all.
- **`rt_sigreturn`** read the whole `LinuxUcontext` off the user stack with
  `read_unaligned`; now `read_user_value::<LinuxUcontext>`, which also lands it
  in an aligned kernel local so the user-side alignment stops mattering.
- **`emit_linux_rt_frame`** wrote `pretcode`, `ucontext` and `siginfo` as three
  separate stores straight onto the user stack. Now packed into one
  `RT_SIGFRAME_SIZE` kernel array (the layout is contiguous by construction)
  and delivered with a single `copy_to_user`. This one needed care: the write
  can now *fail*, where before it could only fault, and by that point
  `take_saved_sigmask` has already consumed the pending `sigsuspend` mask — so
  the failure path puts it back, keeping the documented contract that a `None`
  return means nothing happened and the caller may retry.

`drm/syscall.rs` had a single site, and it was the worst-annotated one in the
tree: `sys_drm_atomic_commit` built a slice over the user buffer with *no
validation whatsoever* — `"SAFETY: The caller is responsible for passing a
valid buffer. In the current kernel-mode testing setup, all addresses are
valid."` — and then parsed record counts out of it. Besides the missing
validation that is a double-fetch: the counts and the records were read from a
live user mapping the submitting process can rewrite between reads. Now
`read_user_vec` with a 64 KiB cap.

`kernel/src/ipc/futex.rs` was **not** on the original file list because it uses
none of the grep's patterns: it casts the user address to `*const AtomicU32`
instead. Fourteen sites, and the one place where the bounce is *not* the right
answer, so it needed its own primitive — see
`D-FUTEX-ATOMICS-OPERATE-DIRECTLY-ON-USER-WORDS` below (now fixed).

A final sweep after that — grepping the *whole* kernel for
`from_raw_parts`/`as *const`/`as *mut`/`unsafe { &*(` and triaging each hit as
HHDM, kernel-local or user address — turned up three more genuinely-unconverted
user accesses, all in `handlers.rs`:

- **`sys_cp_wait` / `sys_cp_try_wait`** delivered completion events by storing
  them one 24-byte record at a time through `args.arg1 as *mut CpEventRaw`,
  after a `validate_user_write` that ran *before* `completion::wait` blocked.
  Now packed into a kernel `Vec` and delivered with a single
  `write_user_items`, which makes delivery all-or-nothing as well.
- **`sys_exception_return_with_frame` / `sys_signal_return_with_frame`** read
  the saved register context field-by-field through `&*(frame.arg0 as *const
  ExceptionContext)` — a double-fetch as well as an unbracketed access. Now
  `read_user_value`. Reading these two closely is also what surfaced
  `B-FRAME-REWRITING-RETURNS-INSTALLED-UNSANITISED-USER-STATE` (above).

**Done — CR4.SMAP is on (2026-08-13).** `USER_ACCESSES_ANNOTATED` is `true`,
`smap_enable_blocker()` returns `None`, and a boot under
`-cpu qemu64,+smep,+smap,+umip` reaches BOOT_OK (135 s) with CR4 = `0x300e20`
and no self-test failures.

The very first boot with the bit set did **not** get that far, and the failure
is the most useful result of this whole entry: a fatal kernel #PF writing to a
*user* stack address from `idt::try_dispatch_user_exception`, which builds the
SEH `ExceptionContext` on the user stack. That site was missed because every
sweep above enumerated code taking a user address **out of a syscall argument**;
this one takes it out of the *ring-3 interrupt frame's RSP*. Worse, the address
was never checked against `USER_SPACE_END` at all, making it an arbitrary kernel
write for any process that had registered an exception handler — see
`B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP` above.

So the closing lesson of this entry, which cost three separate "surely we're
done now" moments to learn: **a grep proves nothing, and neither does a careful
reading; only turning the enforcement on is the audit.** Anything derived from a
saved ring-3 register — an interrupt frame's `rsp`/`rip`, any `SavedRegisters`
GPR — is user input exactly as much as a syscall argument is.

The gate constant stays in `smep_smap.rs` rather than being deleted: it is the
one documented place to turn SMAP back off if a fourth missed path turns up, and
the blocker string still reaches the serial log.

**Bugs found while doing it.** The refactor was worth far more than the SMAP
unblock — reading each site closely turned up a long list of live defects,
every one of which predates this work:

- **`B-NET-DIAGNOSTIC-HANDLERS-WROTE-TO-AN-UNVALIDATED-USER-POINTER`** (above)
  — five handlers, arbitrary kernel write, no capability required. The most
  serious find.
- **A kernel-panic vector in the xattr handlers.** They validated *one* byte
  and then scanned up to 256 looking for a NUL. An unterminated string at the
  end of a mapping walks the scan into an unmapped page, and the fault is taken
  in supervisor mode with no exception-table entry to recover from — so any
  process could panic the kernel with one unterminated buffer. Fixed by
  `mm::user::read_user_cstr`, which copies forward in page-bounded chunks.
- **Alignment UB in `mm::user::read_user`/`write_user`.** Typed
  `core::ptr::read`/`write` through a user-supplied pointer, behind a safety
  contract requiring the caller to "ensure it is properly aligned" — which a
  syscall ABI cannot enforce. Both deleted in favour of the byte-wise
  `read_user_value`/`write_user_value`.
- **Alignment UB in `sys_fs_metadata` for *every* caller.** `attributes` sits
  at ABI offset 58, two bytes past a `u16`, so `out_ptr.add(58) as *mut u32`
  was a misaligned typed store by construction — not merely reachable by a
  hostile caller.
- **A self-deadlock in `sys_log_read`.** `klog::read_logs` formats JSON-lines
  *while holding the log-ring spinlock*, straight into the caller's buffer, so
  a demand-paging fault on an untouched user page is taken with that lock held
  — and the fault path itself logs.
- **Six silent truncations**, each of which changed *which object* the syscall
  operated on rather than merely shortening a result: `sys_dns_resolve` (a
  clipped name resolves a different host), `sys_fs_symlink` (the link points
  elsewhere), `sys_fs_set_xattr` and `sys_fs_append` (clipped the data and
  returned *success* — silent corruption), `sys_cap_request` (clipped the
  human-facing reason string, so a request could be made to read as more
  innocuous than it is), and `ns_bind`/`ns_unbind`/`ns_hide` (a truncated
  prefix installs a sandbox rule over a *broader* subtree — failure in exactly
  the wrong direction). All now reject with `InvalidArgument`. The rule
  adopted: a length cap is legitimate **only** where the handler returns the
  number of bytes it consumed, making it a `write(2)`-style short write the
  caller loops on (`sys_debug_print`, console write, `readlink`).
- **`sys_fs_handle_path` reported the truncated length**, so a caller that
  filled its buffer exactly could not distinguish a fit from a clipped path.
  Now returns the full length — the `snprintf` contract.
- **Infallible allocations sized by a syscall argument** (`vec![0u8; buf_cap]`
  in `sys_fs_read` and elsewhere): on exhaustion these call the allocation
  error handler, i.e. a userspace-triggerable kernel abort. Now
  `mm::user::alloc_zeroed_vec`, which returns `OutOfMemory`.
- **`sys_getrandom` validated its destination *after* running the CSPRNG**, so
  a bad pointer consumed entropy for nothing.
- **Two false SAFETY comments on blocking paths** — the `waitpid` status write
  claimed "the address space cannot have changed since" on a path that sleeps,
  and `sys_process_crash_info` called a userspace buffer "always valid kernel
  memory".
- **Several handlers wrote multi-field records one field at a time**
  (`sys_net_stat`'s six counters, `sys_fs_watch_read`, `sys_process_get_args`,
  `sys_fs_journal_read`), so a fault partway through left the caller with a
  half-updated record — and in the `watch_read` and `get_args` cases the data
  had *already* been dequeued/consumed, so it was unrecoverably lost.

**Related.** `B-AC-INHERITED-AT-KERNEL-ENTRY` (fixed) was the *other*
prerequisite for SMAP. design-decisions §122 records why SMAP stays behind a
gate rather than being enabled optimistically.
