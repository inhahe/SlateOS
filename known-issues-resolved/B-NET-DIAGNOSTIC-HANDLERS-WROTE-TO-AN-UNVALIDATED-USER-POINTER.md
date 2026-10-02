### B-NET-DIAGNOSTIC-HANDLERS-WROTE-TO-AN-UNVALIDATED-USER-POINTER. Five uncapability-gated syscalls were a write-what-where primitive — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/syscall/handlers.rs`)

**What.** `sys_tcp_list`, `sys_tcp_listener_list`, `sys_net_if_info`,
`sys_arp_table` and `sys_dns_cache_stats` each wrote their output records
straight through `args.arg0 as *mut u8`, having checked only that the argument
was non-zero:

```rust
if buf_ptr == 0 { return SyscallResult::err(KernelError::InvalidArgument); }
...
// SAFETY: buf_ptr is a userspace pointer validated by the caller;
// written < max_records ensures dst stays within the buffer.
let dst = (buf_ptr + written * RECORD_SIZE) as *mut u8;
unsafe { core::ptr::copy_nonoverlapping(record.as_ptr(), dst, RECORD_SIZE); }
```

Nothing validated it. `validate_user_write` was never called, and the SAFETY
comments asserted a precondition that no code established — "validated by the
caller" (there is no such caller; this is the syscall boundary), or in
`sys_dns_cache_stats`' case `buf_len >= STATS_SIZE`, which bounds the *length*
and says nothing about the *address*.

**Why it mattered.** None of the five requires a capability. So any process at
all could pass an arbitrary kernel virtual address and have the kernel write
attacker-influenced bytes over it: the TCP connection table (remote IPs and
ports the attacker chooses by opening connections), the interface config, the
ARP cache, or the DNS counters. That is a write-what-where primitive with
partial content control — enough to corrupt page tables, a task struct, or a
capability table. It was reachable from an unprivileged process with no
capability held.

`sys_net_route_list` and `sys_tcp_info`, in the same file and the same style,
*did* call `validate_user_write`, which is what made the omission easy to miss
on a read-through.

**Fix.** All five now pack their records into a kernel-owned buffer and deliver
it with a single `copy_to_user`, which performs the validation the comments
assumed (and brackets the store with STAC/CLAC for SMAP). Sizing the scratch
buffer by the number of records that actually exist, rather than by the
caller's advertised `buf_len`, additionally stops a caller demanding an
arbitrary kernel allocation.

**How it was found.** Not by looking for it — by working through
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` below and reading
every SAFETY comment on a user-pointer access to check whether the invariant it
claimed actually held. Five did not. **The lesson worth keeping: a SAFETY
comment that names a precondition without pointing at the code that establishes
it is not evidence, and in this file it was wrong five times out of five.**
