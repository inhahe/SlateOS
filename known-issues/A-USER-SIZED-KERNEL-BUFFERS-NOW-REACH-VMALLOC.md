### [A] A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC: 21 syscalls copy a whole user buffer into one kernel allocation, and that allocation can now be 1 GiB -- 2026-09-25
**Status:** OPEN, partly fixed. `epoll_wait` was fixed with §959. On
2026-09-26, classes 1, 2a, 2b, 2c and 3 were fixed on lane-a:
- class 1, sizes with a protocol maximum: the four channel sends are judged
  against `MAX_MESSAGE_SIZE`, and `sys_udp_send` against the UDP payload
  maximum, before a byte is read;
- class 2a, pipes and socketpairs in both directions, and class 2b, a pty
  master's read and write: every call copies at
  most what one call can move, after checking the caller's whole
  `(ptr, len)` claim as a user span;
- class 3, file data: streamed a bounded chunk at a time.

What remains is listed under **Progress** at the end of this entry.

**In short:** some system calls copy everything a program passes them into a
kernel buffer of the same size before doing anything with it — a 100 MB
`write` makes a 100 MB kernel copy. Until today the kernel could not make any
single allocation above 16 MB, so these calls failed on big buffers. They now
succeed, because large kernel allocations are mapped from vmalloc
(design-decisions.md §959). Nothing breaks, but one program can now make the
kernel hold up to 1 GiB (the vmalloc region's size) for the length of one call,
and several such calls at once can fill the region, after which other large
kernel allocations fail with `OutOfMemory` until they finish.

**Why it is bounded rather than an open hole.** Every one of these sites
reads its source with `copy_from_user` (through `read_user_vec`), so the caller
must actually have that much readable memory mapped — and memory here is
committed by default. The amplification is 1:1, not the 1:100 of the 16 MiB
case the 2026-09-21 `poll` entry describes. It is a resource problem, not a
crash: every site allocates fallibly and returns `ENOMEM`.

**The sites** — each copies `len` bytes taken straight from the syscall
arguments, with no cap before the copy (`read_user_vec(ptr, len, usize::MAX)`
or `alloc_zeroed_vec(len)`), line numbers as of 2026-09-25:

| where | calls |
|---|---|
| `syscall/handlers.rs` | `sys_channel_send`, `_send_timeout`, `_send_blocking`, `_send_caps` (1483, 1674, 1705, 1746); `sys_pipe_write`, `_try_write`, `_write_timeout` (2228, 2286, 2403); `sys_socketpair_send`, `_try_send`, `_send_timeout` (2497, 2542, 2598); `pty_master_write_common` (6031); `sys_fs_write_file`, `sys_fs_write`, `sys_fs_append` (9225, 10530, 12216); `sys_tcp_send`, `sys_udp_send` (12670, 12992); the three ELF-image spawns (3334, 3448, 8486) |
| `syscall/linux.rs` | `dispatch_memfd_write`, `dispatch_memfd_read` (4249, 4285) |

A channel message, a pipe write or a datagram has a natural size far below
this. `sys_fs_write_file` argues in a comment, correctly, that its copy is
proportional to memory the caller has already committed; what changed is only
that "proportional" now runs to 1 GiB where the allocator used to stop it at
16 MiB.

**Fixed alongside:** `epoll_wait` allocated `maxevents * 12` bytes up front, and
`maxevents` may be up to `EP_MAX_EVENTS` (`INT_MAX / 12`) whatever the set
holds, so one call on a three-fd epoll set could commit 1 GiB. It now sizes the
buffer by `min(maxevents, interest set)` — one record per registered fd is the
most a pass can produce.

**The proper fix** is Linux's: data syscalls stream in bounded chunks (a
page-cache-sized bounce buffer, or copying straight into the destination),
and never hold a kernel copy proportional to the request. The ELF-image
spawns are the exception that genuinely wants the whole image, and they are
the reason §959 exists; they should be bounded by a per-process limit rather
than by the allocator.

**Progress** (2026-09-26). The sites fall into classes by what bounds them,
and each class has its own proper fix:

| class | sites | bound | state |
|---|---|---|---|
| 1 | the four channel sends; `sys_udp_send` | a protocol maximum (`MAX_MESSAGE_SIZE`; 65,535 - 8) | **fixed**: judged before the copy; ring-3 probe "send-size gate" |
| 2a | pipe write, try_write, write_timeout, read, try_read, read_timeout, peek; socketpair send, try_send, send_timeout, recv, try_recv, recv_timeout | one buffer per call (1 MiB pipe maximum; 64 KiB ring) | **fixed**: `read_call_buffer`/`with_call_out_buf` span-check the whole claim and copy at most one call's worth; ring-3 probe "pipe/socketpair per-call copy bound" |
| 2b | `pty_master_write_common`, `pty_master_read_common` | the input queue (4 KiB) / the output ring (64 KiB) | **fixed**: through class 2a's helpers; the write's cap also bounds how much one call pushes through the line discipline under its locks; probe 0x5A-0x5C |
| 2c | `sys_tcp_send` -- the native call, served by the in-kernel stack (`net::tcp`), not the netstack daemon | `MAX_TX_BUFFER` (64 KiB): all the stack keeps for retransmission | **fixed**: `read_call_buffer` caps the copy there; a stream send may be short. (The Linux-ABI socket writes reach the daemon and already stage at most 4 KiB per call.) |
| 3 | `sys_fs_write_file`, `sys_fs_write`, `sys_fs_append`, and `sys_fs_read`'s out-buffer (both ABIs: Linux `read`/`write` on a file call these) | none -- a file can take any length | **fixed**: streamed through one 1 MiB bounce buffer; a part-way fault returns the count, the no-count calls validate the whole source first; a write past the vmalloc region's 1 GiB now works; probe "streamed file I/O" 0x61-0x68. Found alongside: `A-VFS-APPEND-RACES` |
| 4 | the three ELF-image spawns | the image | open -- a per-process limit, not the allocator |
| 3 | `dispatch_memfd_write`/`_read` (linux.rs) | the memfd | **fixed**: the same `stream_user_write`/`stream_user_read` loop; a seal stopping a later chunk returns the earlier chunks' count, as Linux's shmem does page by page |

Class 2a changed one behaviour, deliberately. A buffer whose unmapped tail
lies beyond what one call can move is no longer refused with
`InvalidAddress`: the kernel never needed those bytes, and Linux does not
refuse it either. A claim that runs past user space still is.
