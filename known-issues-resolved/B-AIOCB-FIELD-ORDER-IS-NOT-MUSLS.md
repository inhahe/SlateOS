## B-AIOCB-FIELD-ORDER-IS-NOT-MUSLS (lane B, 2026-09-09) -- FIXED the same day

**In short:** the structure used to queue an asynchronous read or write has its
fields in a different order from the C library's, and is smaller.

| | ours | musl |
|---|---|---|
| `sizeof(struct aiocb)` | 136 | **168** |
| `aio_offset` | 8 | 128 |
| `aio_reqprio` | 32 | 8 |
| `aio_sigevent` | 36 | 32 |
| `aio_lio_opcode` | 100 | 4 |

musl orders it `aio_fildes, aio_lio_opcode, aio_reqprio, aio_buf, aio_nbytes,
aio_sigevent`, then 32 bytes of its own private state, then `aio_offset` at
128 and more private state to 168. Ours opens `aio_fildes, aio_offset, aio_buf`.
`aio_sigevent` is also an opaque `[u8; 64]` placeholder here rather than a real
`struct sigevent` — which is at least the right *size*, measured.

**Where.** `posix/src/aio.rs`.

**Why it is recorded rather than fixed today.** Nothing in the tree calls the
POSIX aio family, and the fix is a reorder plus giving `aio_sigevent` a real
type -- which is a second change (`Sigevent` itself is only three fields here
against musl's union). Doing both properly is worth its own commit rather than
a tail-end of a gate-shrinking one.

**Fixed** by reordering to musl's, with musl's two runs of private state
carried as opaque padding so the public fields either side of them are at the
right offsets. `aio_sigevent` stays an opaque `[u8; 64]` -- the right *size*,
measured -- rather than a real `Sigevent`, which is a separate improvement.

**The doc comment said "matches the POSIX `struct aiocb` layout".** POSIX
cannot settle that: it names the members and leaves the order to the
implementation, so there is no *the* POSIX layout to match, only a particular C
library's. A claim that cannot be true or false is worse than a wrong one --
nothing can contradict it.
