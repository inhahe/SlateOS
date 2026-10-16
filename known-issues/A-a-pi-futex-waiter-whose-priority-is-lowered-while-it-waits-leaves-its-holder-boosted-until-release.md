### [A] A PI-futex waiter whose priority is lowered while it waits leaves its holder boosted until release -- 2026-10-07

**Status:** OPEN (deliberate simplification; low impact)

**What happens.** When a thread blocked on a priority-inheritance futex is
given a *higher* priority (`sched_setscheduler`, a nice change), the new level
is lent on to the holder and down the chain at once
(`ipc::futex::pi_waiter_priority_changed`, Linux's `rt_mutex_adjust_pi`).
When it is given a *lower* one, nothing is taken back: the holder keeps the
larger loan until it releases the futex, when its inheritance is worked out
afresh from the waiters still queued (`futex_unlock_pi`'s recalculation).

**Why it was left.** The raise is the direction that matters -- an
un-propagated raise is a priority inversion, a real-time thread stuck behind
an ordinary holder. The lowering only over-boosts the holder, for no longer
than it holds the lock, which a PI lock's holder is supposed to keep short
anyway. Linux recomputes in both directions.

**Where the fix goes.** `pi_waiter_priority_changed` would recompute the
holder's loan as the maximum over the futex's remaining waiters -- the
calculation `futex_unlock_pi` already makes (`recalc`), factored out -- and
call `sched::set_inherited_priority` with it, then walk the chain the same
way, each link recomputed rather than boosted. The chain walker
(`sched::pi_chain_boost`) only boosts today, so it needs a recompute variant.

**How to see it.** Kernel self-test shape: holder L at level 20, waiter H made
`SCHED_FIFO` 90 while blocked (L lent level 0), then H set back to
`SCHED_OTHER`: L's effective level stays 0 until it unlocks, where Linux's
would return to H's ordinary level at once (16, still a loan above L's 20).
