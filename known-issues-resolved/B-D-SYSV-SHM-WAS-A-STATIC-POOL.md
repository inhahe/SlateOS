### [D] B-D-SYSV-SHM-WAS-A-STATIC-POOL — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/sysv_shm.rs` (rewritten), over the kernel's
shared-memory regions (`SYS_SHM_CREATE`/`MAP`/`UNMAP`/`CLOSE`, now named in
`posix/src/syscall.rs`).

**In short:** System V shared memory is how some programs share a block of
memory between their parts -- PostgreSQL, X11's shared-memory extension,
many older Unix programs. Ours held four blocks of at most 64 KiB each, gave
the same address to every attach, and checked no permissions, so anything
larger or more numerous failed, and a program comparing two attach addresses
was told they were one mapping. It is now Linux's, as the message queues and
semaphores already were.

**What was wrong, against Linux 6.6:**

| | was | now |
|---|---|---|
| sizes, counts | four segments, 64 KiB each | `SHMMIN` 1 byte to `SHMMAX`, `SHMALL` pages in all, `SHMMNI` 4096 segments -- Linux's defaults |
| memory | a static pool inside the library | a kernel shared-memory region per segment; every `shmat` maps it afresh, so two attaches are two addresses of the same bytes |
| permissions | stored, never checked | `ipcperms` for `shmget`, `shmat` (read, or read and write) and `IPC_STAT`; owner or creator (or `CAP_SYS_ADMIN`) for `IPC_SET` and `IPC_RMID`; `SHM_LOCK` the owner's, or `CAP_IPC_LOCK` |
| removal while attached | kept the key usable | `SHM_DEST` in the mode, the key made private, freed at the last detach -- the id still attaches, as on Linux |
| `shmget` on an existing key | -- | `EEXIST`, then a larger `size` `EINVAL` before the permission's `EACCES`, as `ipcget` orders them |
| `IPC_SET`/`IPC_STAT` with a NULL buffer | the segment looked up first | `IPC_SET` reads the buffer first; `IPC_STAT` writes it after the lookup and the permission |
| `IPC_INFO`, `SHM_INFO`, `SHM_STAT`, `SHM_STAT_ANY` | `EINVAL` | Linux's figures and ids |
| times, pids | 0 | `shm_atime`, `shm_dtime`, `shm_ctime`, `shm_cpid`, `shm_lpid` kept |

**Still not Linux's:**

- **An address the caller chooses** (`shmat(id, addr, …)` with `addr`
  non-NULL) is `EINVAL`: `SYS_SHM_MAP` picks the address itself. Asked of
  lane A in `requests/d-a-shm-map-at-an-address.md`.
- **`SHM_EXEC`** is `EACCES`: the kernel never maps shared memory
  executable.
- **Across `fork` and between programs** a segment is not shared: the table
  is this process's (open question D-Q3, as for the message queues and
  semaphores), and a child's `shmat` of an inherited id is refused by the
  kernel, which authorizes only a region's creator.
- **`SHM_HUGETLB`** is `ENOMEM`, as on a Linux system with no huge pages set
  aside.
