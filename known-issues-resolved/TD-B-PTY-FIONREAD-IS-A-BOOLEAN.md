### TD-B-PTY-FIONREAD-IS-A-BOOLEAN -- FIXED 2026-08-24

**Where:** `posix/src/ioctl.rs`, `handle_fionread`, the
`PtyMaster | PtySlave` arm.

`SYS_PIPE_READABLE_BYTES`, `SYS_SOCKETPAIR_READABLE_BYTES` and
`SYS_UDP_RX_FRONT_BYTES` all exist; the pty ring has no counterpart. So
`ioctl(FIONREAD)` is answered from bit 0 of `SYS_PTY_POLL`, widened to 0
or 1.

`ENOTTY` was considered and rejected: a pty *does* support `FIONREAD` on
Linux, and the callers are terminal emulators, whose fallback for "not a
terminal" is worse than a low count. The degradation is bounded -- a caller
that sizes a read by it reads a byte at a time, which is slow and loses
nothing, because `read()` returns what is actually there regardless of what
`FIONREAD` said.

**The 0 case is exact**, which is what keeps this merely degraded: a caller
using `FIONREAD` only to test emptiness -- the common case, and what
`select`-less polling loops do -- is right every time.

**Proper fix:** a `SYS_PTY_READABLE_BYTES`, if the ring carries a cheap
count. If it does not, this entry should be closed "won't fix" rather than
left open forever; that is stated in the request to lane A.

**Fixed 2026-08-24 -- and not as the "won't fix" that was offered.** The ring
keeps `len` as a field, maintained by every write and read regardless, so the
count is O(1) and was free to expose. `SYS_PTY_READABLE_BYTES` = 869.

How exact the answer is depends on the end, and libc states it rather than
leaving callers to assume:

| End | Mode | Answer |
|---|---|---|
| master | -- | exact |
| slave | raw | exact |
| slave | canonical | upper bound |
| either | anything | **zero is exact** |

The master's count is of *post*-discipline bytes, i.e. after `ONLCR`, so a
four-byte slave write containing one newline reports 5 -- which is the number a
reader must size by to avoid stranding the `\r` to be misread as the start of
the next line. The canonical slave's is of *pre*-discipline bytes: the line
editor has not run, so an erase will consume a byte rather than deliver one,
and an unterminated line delivers nothing until its newline arrives. Counting
exactly would mean running the editor twice, and the second run would see
different input.

Only the upper bound is ever wrong and it is harmless, for the same reason the
old boolean was: `read()` returns what is actually there regardless. The
property that made the boolean merely degraded rather than broken -- **zero is
exact** -- survives intact.

A hung-up end with an empty buffer answers 0 rather than failing, which
deliberately differs from `SYS_PTY_POLL`, where hangup sets the readable bit:
"would a read return immediately" is yes there, but "how many bytes are there"
is none, and `FIONREAD`'s caller believes the number.

libc clamps a negative return to 0 rather than propagating it. `FIONREAD` has
no way to say "unknown", and a negative stored into a caller's unsigned length
is an enormous positive -- the one way this call could do real damage.

**Lane A also found and fixed a live hang while implementing it**, which is
worth knowing because it explains a class of symptom: a canonical line is
delivered as a unit, so a reader whose buffer is smaller than the line leaves
the remainder in the *device's* pending buffer rather than any ring, and
`pty::readable()` consulted only the ring. A slave holding four undelivered
bytes of `"hello\n"` reported not readable, and went on doing so forever if
the master sent nothing further. Any libc test that did a short `read` on a
slave and then polled was racing this. Nothing in libc needed to change.
