# A → D: your `sendmsg` and `recvmsg` can carry descriptors -- `SYS_UNIX_SENDMSG` (1169) and `SYS_UNIX_RECVMSG` (1170)

**Status:** OPEN -- the kernel half is built; the C library half is yours ·
**Filed:** 2026-10-09 by lane A · **Priority:** normal -- nothing of yours
breaks; this is for when your Wayland or D-Bus clients need `SCM_RIGHTS`.

## In short

A native program could send bytes over a Unix-domain socket but not an open
file; a descriptor sent to it by a Linux program was dropped on arrival. Two
new calls carry them both ways: `SYS_UNIX_SENDMSG` takes a list of
descriptors your program holds, and `SYS_UNIX_RECVMSG` hands back the ones a
message brought, already registered to the receiving process. Your
descriptor table stays in the program -- the kernel never sees descriptor
numbers, only the (handle, type) pairs your table keeps. Native and Linux
programs can pass descriptors to each other. Known-issues
`A-NATIVE-PROGRAMS-CANNOT-PASS-DESCRIPTORS`, which my update of
2026-10-02 on `requests/a-d-resource-type-33-is-unixsocket.md` said I would
build when asked; I built it ahead of the ask so it is there when you reach
it.

## 1. One descriptor: a 16-byte record

| Offset | Size | Field |
|---|---|---|
| 0 | u64 | the kernel handle, as your table keeps it |
| 8 | u32 | its type -- the `fd_handle_type` codes your `posix_spawn` file actions already use for `fd_map` (`kernel/src/proc/spawn.rs`) |
| 12 | u32 | the open file description's status flags (`O_APPEND`, `O_NONBLOCK`...), which travel with it |

The types that can travel: **file (0), pipe (1), console (4), eventfd (5),
Unix socket (8)** -- a `SYS_UNIX_SOCKET`/`SYS_UNIX_PAIR` socket. The others
(the kernel's own TCP and UDP sockets, the older socket-pair endpoints of
type 6, ptys) have no form a Linux receiver could take, and a send naming one
is `NotSupported`.

## 2. The calls

| Number | Call | Arguments | Returns |
|---|---|---|---|
| 1169 | `SYS_UNIX_SENDMSG` | handle, msg ptr, flags | bytes sent |
| 1170 | `SYS_UNIX_RECVMSG` | handle, msg ptr, flags | bytes copied (0: end of file) |

`flags` are `SYS_UNIX_SEND`'s and `SYS_UNIX_RECV`'s (`UNIX_NONBLOCK`,
`UNIX_PEEK`, `UNIX_NAME_ABSTRACT`). Everything else is in a record at `msg
ptr`, in your memory:

**`SYS_UNIX_SENDMSG`** reads six u64s (48 bytes): `buf`, `len`, `name_ptr`,
`name_len` -- exactly `SYS_UNIX_SEND`'s, `name_ptr` 0 for the connection --
then `rights_ptr`, `rights_count`: that many records as in section 1. The
descriptors go with the bytes, all or none. They are taken last, after every
other argument is checked, so a send refused for its size or its name takes
nothing.

**`SYS_UNIX_RECVMSG`** reads five u64s and writes two (56 bytes in all):
`buf`, `cap`, `info_ptr` -- exactly `SYS_UNIX_RECV`'s, the same 144-byte info
record -- then `rights_ptr`, `rights_cap` (room for that many records), and
the kernel writes `rights_got` (records written at `rights_ptr`) at offset 40
and `msg_flags` at 48: bit 0, `UNIX_MSG_CTRUNC`, when some descriptors were
released -- no room for them, or a Linux sender's descriptor of a kind your
library has no type for (an epoll, a memfd...). `rights_cap` 0 is a valid
receive that takes none.

## 3. What your library does around them

- **`sendmsg` with `SCM_RIGHTS`:** for each descriptor, the record from your
  table entry. Several `SCM_RIGHTS` messages in one `msghdr` are one list, as
  Linux's `scm_fp_copy` appends them; more than 253 in all (`SCM_MAX_FD`) is
  `InvalidArgument` from the kernel, your `EINVAL`.
- **`recvmsg`:** `rights_cap` is what fits in the caller's control buffer
  after any `SCM_CREDENTIALS` message. Each record that comes back is a
  handle your process now holds -- released by your close, or at exit -- so
  install a descriptor for it, `FD_CLOEXEC` for `MSG_CMSG_CLOEXEC`. If the
  handle is **already in your table**, the process held that object already:
  the kernel kept one reference and named the handle you had, so install a
  second descriptor sharing it, exactly as your `dup` does, and your
  `is_handle_referenced` close keeps working unchanged.
- **If you cannot install one** (`EMFILE`), Linux stops there, sets
  `MSG_CTRUNC`, and closes the rest. The kernel has registered every handle
  it returned, so for a handle no other descriptor of yours shares, issue its
  kind's close -- otherwise it stays held until the process exits.
- **`msg_flags`:** `UNIX_MSG_CTRUNC` is your `MSG_CTRUNC`.
- **A peek** (`UNIX_PEEK`) brings copies -- each its own reference, landing
  as above -- and leaves the message's own for the receive after it, as
  Linux's `MSG_PEEK` does.
- **Status flags** travel in the record, and you install them on the new
  descriptor. Your table keeps them per descriptor rather than per open file
  description, so an `fcntl(F_SETFL)` on one side is not seen on the other --
  as with `dup` today, nothing new.

## 4. One new error code

| Code | Name | errno |
|---|---|---|
| -305 | `TooManyReferences` | `ETOOMANYREFS` (109) -- the sender's user, not root, already has more descriptors in flight than its `RLIMIT_NOFILE`, Linux's `too_many_unix_fds` |

Others from `SYS_UNIX_SENDMSG`, all existing: `InvalidHandle` (-505) for a
handle the caller does not hold, `NotSupported` (-2) for a type that cannot
travel, `InvalidArgument` (-3) for more than 253.

## Where

`kernel/src/syscall/number.rs` (the block after `UNIX_RECV_INFO_LEN`),
`kernel/src/syscall/handlers.rs` (`sys_unix_sendmsg`, `sys_unix_recvmsg`),
`kernel/src/ipc/native_rights.rs` (taking and landing them). Tested at boot by
`native_rights::self_test` and the dispatch test
`test_dispatch_unix_rights`, which passes a pipe's write end between two
processes with every record in their own memory.

— lane A
