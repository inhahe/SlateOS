### B-IO-RING-SUBMISSION-PATH-WAS-UNGATED-AND-UNVALIDATED. Four separate ring-3-reachable holes in `io_ring` — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/ipc/io_ring.rs`)

**Context.** `SYS_IO_RING_SETUP` (260) and `SYS_IO_RING_ENTER` (261) are
registered in the dispatch table with **no capability gate**, so every item
below was reachable from any unprivileged process. There are as yet no
userspace consumers — the only in-tree callers are `completion.rs`'s
`test_io_completion` and `bench.rs`'s `bench_io_ring_nop`, both of which submit
`IO_OP_NOP` with `addr: 0` — so nothing legitimate depended on the old
behaviour. That absence of users is also why this sat unnoticed: the subsystem
is fully wired to ring 3 but exercised only by two self-tests that never touch
the dangerous paths.

**(1) `sqe.addr` was dereferenced with no validation whatsoever.** Every
buffer-moving opcode did `sqe.addr as *const u8` / `as *mut u8` and built a
slice over it:

```rust
// SAFETY: Caller guarantees ptr is valid for len bytes.
let bytes = unsafe { core::slice::from_raw_parts(ptr, len.min(4096)) };
```

The "caller" is userspace filling a shared submission queue. No
`validate_user_read`/`validate_user_write` appeared anywhere in the file. This
gave, from an unprivileged process:

- **Kernel memory disclosure.** `IO_OP_CONSOLE_WRITE` with a kernel address
  printed up to 4 KiB of kernel memory to the console;
  `IO_OP_CHANNEL_SEND` / `IO_OP_PIPE_WRITE` / `IO_OP_FH_WRITE` /
  `IO_OP_FS_WRITE` exfiltrated it to a peer process or to a file.
- **Arbitrary kernel write.** `IO_OP_CHANNEL_RECV`, `IO_OP_PIPE_READ`,
  `IO_OP_FS_READ`, `IO_OP_FH_READ` and `IO_OP_FH_PREAD` copied
  attacker-influenced bytes to an address the attacker named.
- **Unbounded kernel allocation.** `exec_channel_send` and `exec_fs_write` took
  `len` straight from the SQE with no cap at all.

**(2) `execute_sqe` ran with the global `RING_TABLE` spinlock held.**
`enter()` held the lock across the whole processing loop. `IO_OP_SLEEP` parks
for up to 60 seconds and `IO_OP_TIMEOUT` for up to 10, and every buffer opcode
can block on filesystem I/O or a demand-paging fault. One process could
therefore hold a global kernel spinlock for a minute at a time, wedging every
other thread touching *any* ring.

**(3) No entry point checked ownership.** `owner_task` was stored and never
read. Handles come from `NEXT_RING_ID.fetch_add(1)` starting at 1, so they are
trivially guessable. Consequences:

- `destroy(other_ring)` freed the physical frames of another process's ring
  while that process still had them mapped — a **cross-process use-after-free
  of physical memory**, and the freed frames are then handed to somebody else.
- `set_cp(other_ring, 0)` silently detached a victim's ring from its completion
  port, so the victim's event loop would never wake again.
- `enter(other_ring)` executed a victim's queued SQEs in the *attacker's*
  address space and with the attacker's handle table.

**(4) `SYS_IO_RING_SETUP` returned an HHDM address to userspace.**
`io_ring::setup` returns `phys_frames[0] + hhdm`, a kernel direct-map pointer,
and the handler passed it back in `value2`. Ring 3 could not dereference it,
but simply *knowing* it discloses the direct-map base and so defeats kernel
address-space randomisation for any subsequent attack. The frames were also
never mapped into the caller at all, so the documented interface ("maps into
user space, returns the virtual address of the `IoRingHeader`") did not work.

**Fix.**

1. Every payload goes through `mm::user`. The length is a **rejection**
   threshold for the all-or-nothing opcodes (`CHANNEL_SEND`, `FS_WRITE`,
   `SERVICE_CONNECT` — where the CQE result is 0/error, so clamping would
   truncate the message/file/service name and report success) and a
   short-transfer **cap** only where the CQE result reports bytes consumed
   (`CONSOLE_WRITE`, `PIPE_*`, `FH_*`, `FS_READ`), which is the `write(2)`
   contract a caller already loops on.
2. `enter()` claims the ring with a `busy` flag under the lock, releases the
   lock, processes, then re-acquires to clear the claim. `busy` supplies the
   exclusion the lock used to: one entrant per ring, and `destroy` refuses
   (`WouldBlock`) while the processing loop is still reading SQEs out of those
   frames.
3. Ownership is by **process**, not task — the SQ/CQ are a mapping in one
   address space, so every thread of the owner is a legitimate driver and no
   thread outside it is. `enter`/`destroy`/`set_cp`/`attach_user_mapping` are
   owner-checked; `has_completions_ready` deliberately is not (it runs on
   whichever task polls a completion port and leaks only a bool), and says so
   in its doc comment.
4. `sys_io_ring_setup` maps the frames into the caller's address space
   read/write/no-execute with a per-frame `ref_inc` (the `sys_shm_map` model, so
   `destroy` dropping the ring's reference cannot pull memory out from under a
   live mapping), with full unwind on partial failure, and returns *that*
   address. `destroy` unmaps the recorded range first. `setup`'s doc comment now
   states in terms that the address it returns is HHDM and must not reach
   userspace.

**Also fixed in passing.** `exec_fh_pread`/`exec_fh_pwrite` restored the file
cursor with `let _ = seek(...)`, discarding the error. A failure there leaves
the handle parked at `offset + n`, silently corrupting every subsequent
sequential access — far worse than the pread itself failing. Both now report it.

**How it was found.** Working through
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` below: `io_ring.rs`
was next on the file list after `handlers.rs`, and the first grep for
`from_raw_parts` over a user address turned up fifteen sites, none validated.
