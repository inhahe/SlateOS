### [D] B-D-IO-URING-WAS-NOT-LINUXS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/linux_io_uring.rs` -- `io_uring_setup`,
`io_uring_enter`, `io_uring_register`, and the `IORING_SETUP_*` constants.

**In short:** SlateOS has no io_uring, and programs that can use it check
for it at startup and fall back when it is missing. They tell "missing" from
"called wrongly" by the error, so the error has to be Linux's. Ours checked
arguments in an order of its own, refused combinations Linux accepts and
accepted flags Linux 6.6 does not have -- and one flag constant had another
flag's value.

**What was wrong, against Linux 6.6:**

| | was | now |
|---|---|---|
| `IORING_SETUP_SINGLE_ISSUER` | `1 << 8` -- the same bit as `IORING_SETUP_COOP_TASKRUN` | `1 << 12`, Linux's (the three other modules defining it had it right) |
| accepted setup flags | `HYBRID_IOPOLL` (6.13's) in, `NO_MMAP` and `REGISTERED_FD_ONLY` (6.5's) out | bits 0 to 16, 6.6's set |
| accepted enter flags, register opcodes | 6.12's `ABS_TIMER` and 6.13's `EXT_ARG_REG`; opcodes up to 31 | 6.6's: five flags, opcodes below 26 |
| `SQPOLL` with `IOPOLL` | `EINVAL` | accepted (then `ENOSYS`) |
| `SQPOLL` without `CAP_SYS_NICE` | `EPERM` | no capability is asked for, as in 6.6 |
| `SQPOLL` with `COOP_TASKRUN`, `TASKRUN_FLAG` or `DEFER_TASKRUN`; `TASKRUN_FLAG` without `COOP_TASKRUN` or `DEFER_TASKRUN`; a CQ smaller than the SQ | accepted -- and the CQ compared before rounding, so a CQ of 5 for 8 entries was refused | `EINVAL` as Linux refuses them, the CQ compared after both round up to a power of two |
| `io_uring_enter` | `min_complete`, `sig` and `sigsz` judged before the descriptor; every descriptor `EBADF` | flags, then the ring: not open `EBADF`, open `EOPNOTSUPP`, a registered-ring index `EINVAL` |
| `io_uring_register` | per-operation argument shapes (and an invented `E2BIG`) before the descriptor | opcode, then the ring, as for `io_uring_enter` |

`io_uring_setup` stops where Linux would allocate the rings and answers
`ENOSYS`; the checks Linux makes afterwards (`ATTACH_WQ`'s descriptor,
`SQ_AFF`'s CPU) are about rings that cannot exist here.
