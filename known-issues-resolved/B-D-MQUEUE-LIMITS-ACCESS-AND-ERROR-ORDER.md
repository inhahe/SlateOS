### [D] B-D-MQUEUE-LIMITS-ACCESS-AND-ERROR-ORDER — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/mqueue.rs`.

**What it was.** POSIX message queues were a static pool: 8 queues of 32
messages of up to 256 bytes, names up to 63 bytes, and a default message size
of 64 -- so a program that opened a queue with no attributes and sent 100
bytes got `EMSGSIZE`, where Linux's default queue holds 10 messages of 8192.
The descriptor's access mode was not kept, so a queue opened `O_RDONLY` could
be sent to. `O_WRONLY|O_RDWR` was refused before the lookup, where Linux
refuses it only for a queue that already exists (after `EEXIST`). Names were
judged by their own rule: `"/"` was `EINVAL` (Linux: `ENOENT`), `"/a/b"`
`EINVAL` (`EACCES`). A blocked `mq_send` or `mq_receive` busy-spun without
yielding. `mq_notify` was `ENOSYS`. The errors came in the wrong order (the
seventeenth pass).

**Fix.** Linux's semantics throughout: the kernel's defaults and limits
(10 x 8192; up to 10 and 8192, or 65536 and 16 MiB with `CAP_SYS_RESOURCE`;
256 queues), each queue's storage allocated at its size; access modes kept
and enforced; names judged as glibc and the kernel judge them; errors in the
kernel's order; waits that sleep on a futex until the queues change; and
`mq_notify` -- one registration per queue, fired once when a message arrives
in the empty queue with no receiver waiting, through the notification code
aio uses (now `posix/src/sigevent.rs`).

**What remains.** Queues live in one process: two programs opening one name
get two queues. That is open question D-Q3. An `mqd_t` is an index into this
module's table, not a file descriptor, so `poll`, `select` and `close` do not
take one, as Linux's do.
