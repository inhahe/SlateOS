### [D] B-D-SYSV-MSG-LIMITS-PERMISSIONS-AND-ERROR-ORDER — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/sysv_msg.rs`.

**What it was.** System V message queues were a static pool: 8 queues, 32
messages each, 256 bytes a message, 8192 bytes a queue. `msgsnd` refused a
message over 256 bytes, where Linux takes 8192, and `msgrcv` refused any
*buffer* over 256 bytes with `EINVAL` -- so the ordinary receive into a
`char mtext[8192]` failed however small the message. A blocked call spun on
the CPU without yielding. Permissions were stored and never checked;
`IPC_SET` clamped `msg_qbytes` silently where Linux refuses it without
`CAP_SYS_RESOURCE`, and let any caller change the owner; the timestamps and
pids `IPC_STAT` reports were always 0; and `IPC_INFO`, `MSG_INFO` and
`MSG_STAT` -- what `ipcs -q` reads -- were `EINVAL`. A NULL buffer was
`EFAULT` before the id, the queue or the message had been looked at, and
`IPC_SET` looked the queue up before reading its buffer.

**Fix.** Linux 6.6's ipc/msg.c, inside one process: its limits (8192-byte
messages; 16384-byte queues, holding as many messages as bytes; 32000
queues), each message allocated at its size; `ipcperms`, and the owner test
for `IPC_SET` and `IPC_RMID`; the error orders of `ksys_msgsnd`,
`do_msgrcv` and `ksys_msgctl`; `IPC_INFO`, `MSG_INFO`, `MSG_STAT` and
`MSG_STAT_ANY`; the timestamps and pids; futex waits; and `EIDRM` for a call
blocked on a queue that is removed.

**What remains.** The queues are one process's, as the POSIX queues are
(D-Q3). The blocking paths are tested on the host with the waiting step
played by the test; no ring-3 fixture has yet run two threads against one
queue.
