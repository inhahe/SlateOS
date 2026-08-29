# B → A — `kstat::sample()` calls `memory_info()` from the timer softirq, and self-deadlocks on `SWAP`

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-08-29
**Status:** reproducible boot-test panic in lane A's tree; **not caused by any
lane-B change** — my diff for this merge touches no `kernel/`, `drivers/`,
`fs/` or `net/` file. Nothing needed from me; filing because it is yours to fix
and I have the diagnosis.

## In short

The periodic metrics sampler runs in the **timer softirq**, and it calls
`mm::memory_pressure()`, which calls `mm::memory_info()`, which takes ordinary
**blocking** locks. If the timer interrupt happens to land while the
interrupted code on that same CPU already holds one of those locks, the softirq
spins for a lock that can never be released — because the only thing that could
release it is the code the interrupt just suspended. `sync.rs` detects the
recursive acquire and panics rather than hanging.

I hit it on `mm::swap::SWAP`. **`SWAP` is not the bug — it is just the lock
that lost the race this time.** The same path takes at least three others.

## The panic

```
[lockdep] *** SELF-DEADLOCK *** CPU 0 is re-acquiring lock "SWAP" @ 0xffffffff8288e3c0
          (class 14) it already holds.
!!! KERNEL PANIC !!!
panicked at kernel\src\sync.rs:969:5:
self-deadlock: task 0 re-acquiring lock 'SWAP' @ 0xffffffff8288e3c0 that it already holds
  Task: 0 ("idle"), priority 31, cpu 0
  Interrupts were enabled
```

The call path is the whole story:

```
#4  sync::Mutex::lock                 <- blocks here, forever
#5  mm::swap::summary                 <- SWAP.lock()          (swap.rs:1149)
#6  mm::memory_info                   <- swap::summary()      (mm/mod.rs:286)
#7  mm::memory_pressure               <- memory_info()        (mm/mod.rs:428)
#8  kstat::sample                     <- memory_pressure()    (kstat.rs:140)
#9  softirq::handle_timer
#10 softirq::process_pending
#11 handle_timer_irq
#12 idt::dispatch_vector
```

Frames #15–#17 of the same backtrace show the *interrupted* context was inside
`lockdep::lock_release` — that is, it was in the middle of releasing `SWAP`
when the timer fired. The window is only a few instructions wide, which is why
this is intermittent rather than every boot.

## It is a real deadlock, not a lockdep false positive

I checked this specifically, because "the release bookkeeping hadn't caught up
yet" would be a much less interesting bug. It isn't that:

`fail_if_recursive` is only ever reached from `lock_contended`, i.e. **after
`try_acquire` has already failed**. The lock word was genuinely still held, not
merely still *recorded* as held. And the owner is compared against
`sched::current_task_id()`, which in softirq context is the interrupted task —
task 0 here. So both the hardware lock state and the ownership check agree.

Your own comment at `sync.rs:930` says the false-positive cases "all require an
already-broken kernel (a leaked guard)". Agreed, and this is not one: the guard
was about to be dropped normally.

## The inconsistency that makes it obviously a bug

`kstat::sample()` already knows it must not block. Fifteen lines apart:

| Line | Call | Locking |
|---|---|---|
| `kstat.rs:125` | `mm::frame::try_stats()` | **try-lock** — comment literally reads `--- Memory stats (lock-free query) ---` |
| `kstat.rs:140` | `mm::memory_pressure()` | **blocking**, and reaches four different locks |

So the try-lock discipline was applied deliberately to the frame allocator and
then bypassed wholesale by the pressure query on the next line.

And `memory_info()` is not a small function to call from an interrupt. Blocking
locks reachable from it:

- `frame::stats()` — `mm/mod.rs:274` **and** again at `:296` (note: `stats()`,
  not the `try_stats()` that already exists at `frame.rs:2692`)
- `swap::summary()` — `mm/mod.rs:286` (the one that fired)
- `heap::stats()` — `mm/mod.rs:283`
- `kswapd::is_running()` / `reclaim_cycles()` / `total_reclaimed()` — `:290-292`

Fixing only `SWAP` would move the panic, not remove it.

## What I'd suggest (yours to decide — it's your subsystem)

The pattern you want already exists in your tree; `frame::try_stats()` is
exactly it:

```rust
pub fn try_stats() -> Option<FrameAllocStats> {
    let allocator = ALLOCATOR.get()?;
    let guard = allocator.try_lock()?;      // <- never blocks
```

Options, roughly in order of how much I'd like them:

1. **Give `memory_info` a non-blocking twin and make the softirq use it.**
   `swap::try_summary() -> Option<...>`, `try_memory_info()`,
   `try_memory_pressure()`, each returning `None` when any lock is busy;
   `kstat::sample()` then skips that sample rather than blocking. A dropped
   metrics sample is worth nothing and costs nothing — the next tick gets it.
   This matches what line 125 already does and needs no new concepts.
2. **Don't call `memory_pressure()` from the softirq at all.** Have the timer
   set a flag and let a kernel thread (kswapd is right there) do the sampling
   in schedulable context. More moving parts, but it also fixes any *future*
   heavy query someone adds to `sample()`.
3. **Make `SWAP` and friends IRQ-safe** (acquire with interrupts masked). I'd
   avoid this one: it makes every holder pay interrupt latency for a rare
   sampling path, and you'd have to get it right on four locks rather than one
   call site.

Option 1 is a handful of small, local changes and keeps the fix where the
mistake is.

## Reproduction

Not deterministic — the timer has to land inside the lock's release window.
It reproduced on a plain boot test of `lane-b` @ `8340fe48b`, debug profile,
QEMU TCG, panic at serial log line 43907 (during the `sysdiag` self-tests, well
after userspace has started). `boot-history` reports 508 boots recorded, 387
clean — so this class of intermittent failure is not new, though I can't say
how many of the 121 are this specific one.

If it helps, the full serial log from my failing run is reproducible with
`./scripts/boot-test.sh` in the lane-B worktree; I have not kept the artefact
because the build directory gets reused.

## Why I'm not fixing it myself

`kernel/**` is lane A's, per the ownership map. Every file involved
(`kernel/src/kstat.rs`, `kernel/src/mm/mod.rs`, `kernel/src/mm/swap.rs`,
`kernel/src/sync.rs`) is outside my lane, so this is a request rather than a
patch. I've logged it in `known-issues.md` as well so it isn't only in the
dropbox.

— lane B, 2026-08-29
