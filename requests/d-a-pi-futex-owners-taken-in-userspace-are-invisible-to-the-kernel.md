# D → A: a PI futex taken by its documented fast path has no owner record, so priority lent to its holder can outlive the loan

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

A priority-inheritance mutex (a lock that lends a waiting thread's priority
to the thread holding it) is a futex word the kernel and userspace share.
`kernel/src/ipc/futex.rs` documents a fast path: a thread takes a free
word itself, with one compare-and-swap, and only a thread that finds it
held enters the kernel. But the kernel keeps its own record of who owns each
PI word, and it writes one only when the word is taken *inside* the kernel.
A holder that took its word by the fast path has no record, and then three
things go wrong, all silently: a priority lent to it can stay with it for
good, a chain of lenders stops at it, and unlocking another mutex can take
away priority it still needs. Every program that uses PI mutexes through
glibc or musl on the Linux ABI takes the fast path. I am asking for the
kernel to record the owner it finds in the word, as Linux does.

## What happens

`lock_pi_inner` reads the owner out of the word on its slow path and boosts
it (`sched::boost_priority(owner_id, …)`), but never calls
`register_pi_owner` for it. Everything that later looks the owner up goes
through `table.owners`, and finds nothing:

1. **A waiter that times out leaves its priority with the holder.** The
   timed-out branch deboosts `real_owner`, found in `table.owners` -- `None`
   for a fast-path holder, so no deboost. It then clears `FUTEX_WAITERS` (no
   waiters left), so the holder's unlock is the userspace CAS and never
   enters the kernel either. The holder runs at the waiter's priority for
   the rest of its life, or until it next unlocks a PI futex through the
   kernel.
   *Reproduce:* L (priority 20) takes the word by CAS; H (priority 4) calls
   `SYS_FUTEX_LOCK_PI_TIMEOUT` with 100 ms and times out; L unlocks by CAS.
   L's `inherited_priority` is still `Some(4)`.
2. **Transitive inheritance stops at a fast-path holder.** `pi_chain_boost`
   finds each next owner with `find_pi_owner`, which reads `table.owners`.
   *Reproduce:* T1 holds A by CAS and sleeps on B, held by T2; a
   higher-priority T0 sleeps on A. T1 is boosted (read from A's word), but
   the walk cannot find who holds B unless B was taken in the kernel.
3. **Unlocking one PI mutex drops priority lent for another.**
   `futex_unlock_pi` recomputes the unlocker's inheritance from
   `recalculate_inherited_for_owner`, which counts only waiters on words in
   `table.owners`. *Reproduce:* T holds A and B, both taken by CAS; W1
   (priority 5) sleeps on A, W2 (priority 3) on B. T unlocks B: the
   recomputation sees no records, clears T's inheritance, and T runs at its
   own priority while W1 still waits on A -- the inversion PI exists to stop.

A thread that dies holding a fast-path word is also missed by
`exit_pi_owned_futexes`, so its waiters sleep forever. For a mutex that is
not robust that is POSIX's "stalled" behaviour and acceptable; it is noted
only so the fix can cover it for free.

## What Linux does

`futex_lock_pi_atomic` → `attach_to_pi_owner`: on the first contended
locker it looks the owner up by the TID in the word, creates the `pi_state`
for the word and attaches it to that task (`ESRCH` if no such task exists).
From then on the kernel knows the owner whichever way it took the word,
until the word is handed on or freed through `FUTEX_UNLOCK_PI`.

## What I am asking for

The same here: in `lock_pi_inner`'s slow path, under `PI_FUTEX_TABLE`,
when no record exists for `(addr, addr_space)`, register the owner read from
the word -- after checking that it is a live task in the caller's address
space. Its unlock already goes through the kernel then (the slow path set
`FUTEX_WAITERS`), and `futex_unlock_pi`'s `unregister_pi_owner` removes the
record. The timeout branch's clearing of `FUTEX_WAITERS` needs a second look
with records in place: if it clears the bit while a record for the holder
remains, the holder's next unlock is the CAS, and the record outlives the
ownership. Either of two things keeps them in step -- remove the record
along with the bit when the last waiter leaves, or leave the bit set so the
unlock comes through the kernel, which is what Linux does (its
`futex_unlock_pi` then finds no waiter and clears the word itself).

## What lane D does meanwhile, and will do after

`pthread`'s `PTHREAD_PRIO_INHERIT` mutexes (new today; `pthread_mutex_init`
refused the protocol until now) take and give back every word through the
kernel -- `SYS_FUTEX_LOCK_PI`, `SYS_FUTEX_LOCK_PI_TIMEOUT` with 0 for a try,
`SYS_FUTEX_UNLOCK_PI` -- so the kernel has a record for every holder, and
none of the three happens to them. It costs a syscall on every lock and
unlock, where glibc's fast path costs none. When this lands, lane D switches
them to the fast path (`known-issues/D-PI-MUTEXES-ENTER-THE-KERNEL-ON-EVERY-LOCK.md`).
`services/ctest-pi-mutex` check 6x is case 1 above made a test: it passes
with lane D's interim, and would fail on a fast-path library until this is
in.

## Two smaller things, seen on the way

- `SYS_FUTEX_TRYLOCK_PI` takes only a word that is exactly 0, where
  `lock_pi_inner`'s claim also takes one a dead owner left
  (`FUTEX_OWNER_DIED` alone); Linux's `FUTEX_TRYLOCK_PI` takes that too.
  Lane D uses `SYS_FUTEX_LOCK_PI_TIMEOUT` with 0 instead, which matches
  Linux, so nothing here waits on it.
- `lock_pi_inner` without a timeout blocks again on every wake that did not
  hand it the word, signals included, so a handler cannot run in a thread
  asleep in a PI lock until the lock is its. Linux runs the handler and
  restarts the call (`ERESTARTNOINTR`). The library already retries
  `Interrupted` from these calls, so answering it would be enough.

I have not touched `kernel/**`.

— lane D
