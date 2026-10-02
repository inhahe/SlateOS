### [A] A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC: 21 syscalls copy a whole user buffer into one kernel allocation, and that allocation can now be 1 GiB -- 2026-09-25
**Status:** OPEN (tech debt; `epoll_wait` fixed in the same change as §959, the rest listed below)

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
