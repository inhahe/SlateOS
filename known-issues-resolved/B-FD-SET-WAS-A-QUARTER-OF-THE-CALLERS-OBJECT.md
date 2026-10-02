## B-FD-SET-WAS-A-QUARTER-OF-THE-CALLERS-OBJECT (lane B, 2026-09-09) -- FIXED the same day

**In short:** the set of file descriptors a program hands to `select()` is 128
bytes in C and was 32 bytes to us, so every `select()` read and wrote the front
quarter of the caller's own variable and left the rest as it found it.

**Where.** `posix/src/poll.rs`, `struct FdSet`.

**Cause, and it is instructive.** `FD_SET_WORDS` was derived from `FD_SETSIZE`,
which is 256 here because `fdtable::MAX_FDS` is 256. That reasoning is correct
about *this system's fd limit* and wrong about *the C library's structure* --
two facts that happen to be the same number in glibc and musl (1024 both ways)
and are not the same fact. A C caller writes `fd_set r;` on its stack and gets
128 bytes whatever our fdtable can hold.

**Effect.** `select()`'s write-back covered 32 of 128 bytes, so a caller reusing
a set across calls kept stale bits in the 96 bytes we never touched. No fd at or
above 256 can exist here, so nothing could read a *wrong* bit -- but nothing
guaranteed that either, and any C program that memsets its own set and then
inspects it after `select` was reading three quarters uninitialised.

**Fixed** by splitting the two facts: `FD_SET_BITS` (1024, the ABI) sizes the
structure and `FD_SETSIZE` (256, the policy) still bounds `FD_SET`/`FD_ISSET`.
Found by `scripts/check-libc-abi.py`.

**Two tests could not have caught it.** One asserted
`size_of::<FdSet>() > 0`, true of every struct that has ever existed; the other
asserted `FD_SET_WORDS == 4 // 256 / 64`, restating the derivation under test.
Both now assert 128 against musl's own number.
