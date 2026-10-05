## B-AT-RANDOM-WAS-RE-ROLLED-UNDER-A-RACE — two threads could both fill the stack-canary buffer

**Status:** fixed 2026-08-22 · `posix/src/crt.rs`

**In short:** every program gets a small random number at startup that the
compiler uses to detect a stack overrun — if the number changes underneath a
running function, the program kills itself reporting an attack that never
happened. Our code that produced that number could be run by two threads at
once, and each would generate a *different* number and overwrite the other's.
A program doing nothing wrong could therefore abort at random. It is fixed;
this entry records what was wrong and why the old comment said it was fine.

`getauxval(AT_RANDOM)` returns a pointer to a 16-byte buffer that glibc and
musl both use to seed `__stack_chk_guard` — the "stack canary", a value written
below a function's local variables on entry and checked on exit, on the theory
that an overrun would have to clobber it. `ensure_at_random_initialized()`
filled that buffer on first use, gated on an `AtomicBool`, with this comment:

> The buffer is owned by this module and only ever written here. A race in
> single-process userspace is harmless: both racers fill the same buffer, and
> the second write simply overwrites the first.

That is wrong twice. It is a data race in the language sense — a 16-byte write
concurrent with the 16-byte read a third thread is doing through the pointer
`getauxval` just handed it. And "both racers fill the same buffer" is the
failure, not the excuse for it: `fill_random` gives each racer *different*
bytes, so a thread that re-rolls the buffer after another has already cached a
canary makes that thread's next function epilogue compare a new guard against
an old one and abort a process that was never smashed.

The fix is a three-state latch (`UNINIT` / `FILLING` / `READY`) instead of a
bool. `compare_exchange(UNINIT → FILLING)` admits exactly one filler; every
other caller spins on `Acquire` until `READY` rather than writing, which is
correct here because the critical section is one `fill_random` call, it runs
at most once per process, and the futex machinery a blocking wait would need
is itself a posix facility that may not be initialized this early. The
`Release` store pairs with those `Acquire` loads so a thread that sees `READY`
sees all sixteen bytes. On the `abort()` path the latch is deliberately left
at `FILLING`: `abort()` does not return and takes the process with it, so
there is no thread left to spin.

`AT_RANDOM_BYTES` is now in the detector's `IGNORE` table rather than the
baseline, with that reasoning attached — it is reachable from ten tests but
executes in one, and the entry records a fixed bug rather than an excused one.

**How it was found:** not by a test. The raced-globals detector flagged it, and
the flag was only readable because the entry had to be *read before it was
fixed* — the caveat written into this file one pass earlier. A tool that only
listed line numbers would have produced a lock here, which would have been the
wrong fix and would have left the target build racing.

It is scoped to `posix/` and `userspace/` for the same reason gate 2 is scoped
to `userspace/`: a lane A or C push must never pay for, or be blocked by, a lane
B check. Bypass is `ALLOW_RACED_GLOBAL=1`, separate from the other two gates'.

Validated against the five known instances: the detector's per-global test
counts match the hand analysis exactly (`DL_ERROR` 15, `HTAB` 7, `SAVED` 3,
`UMASK_VALUE` 3), and all four fixed globals now classify as *serialised* —
which is what demonstrates that both halves work, reachability and guard
detection. The macro-based globals above remain unaudited, but they are now
*visible*: they sit in the baseline rather than being invisible until they flake.

One caveat on the backlog's meaning, learned from the `NO_NEW_PRIVS` entry: a
baselined line says "two or more tests reach this with no lock", **not** "this is
a bug". Reachability here is a regex over a one-hop call graph, so a global
touched only on a branch no test takes still counts. Each entry has to be read
before it is fixed, and some will end in `IGNORE` rather than in a lock.

### Verification

`cargo test -p posix --lib` — 20515 passed, 0 failed. Then the pairing that
exposed it: `cargo test -p coreutils -p posix`, which is the load condition
under which the original failure appeared.
