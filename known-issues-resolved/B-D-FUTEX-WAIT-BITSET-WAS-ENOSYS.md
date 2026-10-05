### [D] B-D-FUTEX-WAIT-BITSET-WAS-ENOSYS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/linux_futex.rs` (`futex`, which `syscall(SYS_futex, …)`
routes to).

**What it was.** Rust's standard library puts a thread to sleep with
`futex(FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG, …, FUTEX_BITSET_MATCH_ANY)`
and an absolute `CLOCK_MONOTONIC` deadline -- in `Mutex`, `Condvar`,
`thread::park` and `Once`. Here that command was `ENOSYS`, which std reads as
"woken", so every contended Rust lock and every condition-variable wait on
this system spun at full speed instead of sleeping -- and on one CPU, spun
against the very thread it was waiting for.

**Fix.** `FUTEX_WAIT_BITSET` and `FUTEX_WAKE_BITSET` are served by the
kernel's plain futex wait and wake: the absolute deadline is turned into the
kernel's relative one (an expired one is `ETIMEDOUT`, after the value is
compared, as `futex_wait` orders it), and a bitset is treated as matching
everything -- at worst a spurious wake-up, which every futex caller must
already tolerate. A zero bitset is `EINVAL`.

**What remains.** The requeue and `WAKE_OP` commands, `FUTEX_TRYLOCK_PI` and
`FUTEX_LOCK_PI2` are still `ENOSYS`: the kernel has no futex requeue yet.
glibc and Rust's std use none of them for their locks.
