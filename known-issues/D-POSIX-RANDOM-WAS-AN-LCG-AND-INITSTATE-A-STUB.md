## D-POSIX-RANDOM-WAS-AN-LCG-AND-INITSTATE-A-STUB — `random()` was a linear congruential generator, `initstate` and `setstate` did nothing and returned the wrong array, `drand48`'s unseeded state was nobody's, and the `rand48` initializers raced (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 (`posix/src/prng.rs`)**

**In short:** POSIX specifies `random()` as a particular kind of generator
-- additive feedback over a table of 31 numbers, with `initstate` and
`setstate` to give it other tables and switch between them. Ours was a
simpler one, shared with `rand()`; `initstate` and `setstate` took a table
and ignored it, and both returned the table they were given instead of the
one they replaced, so a program that saved the generator and put it back
got the wrong one. And what a seed gave was nobody's sequence -- not
glibc's, not musl's -- so a program's recorded output (a test suite's
expected file, a replay) differed from Linux's for the same seed.

| What | Was | Is |
|---|---|---|
| `random`, `srandom` | `rand`'s generator: `x * 6364136223846793005 + 1` in 64 bits, bits 33 up returned, under a comment calling it glibc's | POSIX's additive feedback generator, glibc's sequences to the number |
| `initstate` | seeded that generator and returned its argument | lays the generator the size picks (8, 32, 64, 128, 256 bytes) out in the caller's array; returns the array it replaced, NULL under 8 bytes |
| `setstate` | returned its argument and did nothing else | takes up the generator in the array where it was left; returns the array it replaced, NULL for one no generator wrote |
| `rand`, `srand` | the generator above, unlocked | `random` and `srandom`, as glibc's are, and locked: POSIX requires `random` to be thread-safe and `rand` to avoid data races with it |
| `rand_r` | one step of a 32-bit generator | glibc's three-step form |
| the `rand48` family, unseeded | started from `0x330EABCD1234`, BSD's starting value with its words reversed | from 0, as glibc's |
| `srand48`, `seed48`, `lcong48`, `erand48`, `nrand48`, `jrand48` | unlocked reads and writes of three plain statics | race-free: POSIX exempts only `drand48`, `lrand48` and `mrand48` |
| `seed48`'s returned array | one static all threads shared | the calling thread's own |
| `erand48`, `nrand48`, `jrand48`, `seed48` | safe Rust functions dereferencing a caller's pointer | `unsafe`, their contracts stated |

Found writing the reentrant `_r` forms (D-POSIX-LIBC-LACKS-WHAT-GLIBCS-HEADERS-DECLARE),
which need a real generator to be reentrant forms of. Replayed against
glibc 2.39 (`posix/src/random_oracle.txt`: every state size with eight
seeds, `setstate`'s switches, all of the `_r` forms) and against POSIX's
own example on its `drand48` page. The choices -- glibc's sequences, and
which functions lock -- are design-decisions §1142.

**Where:** `posix/src/prng.rs`, moved out of `stdlib.rs`;
`posix/src/process.rs` (`fork` holds the two generators' locks across the
system call).
