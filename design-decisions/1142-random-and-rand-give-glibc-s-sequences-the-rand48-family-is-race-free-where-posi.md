## 1142. `random` and `rand` give glibc's sequences; the `rand48` family is race-free where POSIX requires it, and its draws stay unlocked where POSIX exempts them

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a program that seeds the C library's random-number generator
with a fixed number -- a test suite comparing its output with a recorded
file, a simulation replaying a run -- gets a particular sequence, and which
one is the library's choice. This library now gives glibc's, number for
number, for `rand`, `random` and `rand_r` (the `rand48` family's POSIX
fixes outright). Before, `random` was the wrong kind of generator and
`rand`'s sequence was nobody's (known-issues
D-POSIX-RANDOM-WAS-AN-LCG-AND-INITSTATE-A-STUB). And POSIX requires more of
the `rand48` family than glibc gives -- the functions that set its state
must be safe to call from several threads at once -- which this library now
meets without slowing the functions that draw numbers, which POSIX exempts.

**The sequences.** POSIX fixes `random`'s kind of generator -- additive
feedback, 31 numbers of state by default, `initstate`'s five sizes -- but
not how a seed fills its table, and `rand`'s not at all:

| Option | *What changes:* |
|---|---|
| **glibc's** (taken) | A seed gives what it gives on Linux. `rand` is `random` there, so the two share one generator: one seed gives both one sequence, and a program calling both sees it split between them. |
| musl's | `rand` a 64-bit linear congruential generator, `random`'s table seeded musl's own way: what Alpine and zig's default target give, and not what programs are likely to have recorded output against. |
| our own | What no other system gives; no reason to. |

glibc's is what programs have been run against and their outputs recorded
on, and the library follows glibc wherever POSIX leaves the choice (§1141).

**Thread safety.** POSIX.1-2024 (XSH 2.9.1) exempts `rand`, `srand`,
`drand48`, `lrand48` and `mrand48` from thread safety, and not `random`,
`srandom`, `initstate`, `setstate`, `srand48`, `seed48`, `lcong48`,
`erand48`, `nrand48` or `jrand48`; and `rand` "shall avoid data races with
all functions other than non-thread-safe pseudo-random sequence generation
functions". So:

- `random` and its three take one lock, as glibc's and musl's do; `rand`,
  being `random`, takes it too.
- The `rand48` family's state and parameters are atomics -- the multiplier
  and addend in one word, so nothing reads one from one `lcong48` and the
  other from another -- and the three initializers take a lock among
  themselves. glibc's lock none of this family.
- `drand48`, `lrand48` and `mrand48` stay unlocked, as glibc's are. A lock
  would make every draw contend in the programs -- OpenMP ones above all --
  that share one generator among threads, in breach of POSIX, and get away
  with it on glibc. Unlocked, two threads drawing at once can draw one
  number twice, which POSIX allows; the state being atomic, it is never
  undefined behaviour.
- `seed48` returns the state it replaced in three words of the calling
  thread's own, where glibc's are one static every thread shares, so that
  another thread's `seed48` cannot overwrite them while they are read.
- `fork` holds both locks across the system call, so a child of a threaded
  parent never finds one held by a thread it does not have.

**Where it parts from glibc's**, on purpose: `setstate` refuses an array
whose recorded position is past its generator's end, where glibc's would
read and write past the array; `initstate`, `setstate` and the `_r` forms
refuse a NULL pointer, and `random_r` and `srandom_r` a `struct
random_data` never set up, with `EINVAL`, where glibc's follow the pointer;
`rand_r(NULL)` gives 0.

**Where:** `posix/src/prng.rs`; `posix/src/process.rs` (`fork`).
