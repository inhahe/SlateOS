## 975. A lock that has another lock taken under it is watched by the deadlock detector; a conversion that costs more than it is worth is reverted, by measurement

**Date:** 2026-09-27 · **Decided by:** Operator (Claude recommended this option) · **Lane:** A

Answering A-Q16. Relayed by lane F, 2026-09-27; verbatim:

> A, and I think you're also saying to "convert, read the arm, and revert any
> conversion that costs more than it is worth. The arm now exists to read," so
> do that.

**In short:** the kernel has a cheap kind of lock that the deadlock detector
does not watch. The stated reason was that nothing is ever locked while one is
held. That was measured false: it happens 1256 times per boot, across 89 pairs
of locks. The operator chose to convert those locks to the watched kind, then
measure what each conversion costs and undo the ones that cost more than they
are worth.

**What it obliges.**
1. **Convert the outer locks.** Every `PreemptSpinMutex` that §949's leaf check
   finds held while another lock is taken becomes `crate::sync::Mutex`. A lock
   taken in interrupt context must stay interrupt-safe under its new type, and
   that is checked per lock before converting, not discovered after.
2. **Price each conversion.** The price is acquisitions per boot, from the
   statistics `crate::sync::Mutex` keeps, times the extra cost per acquire,
   about 235 ns in `bench_lock_primitives`. A conversion that is material on a
   hot path (syscall entry, the scheduler, an interrupt) is reverted. Its
   lock order is then pinned another way: written down and checked by a
   targeted self-test, never left unwatched and undocumented again.
3. **End state.** The cheap type holds only true leaves. §949's check then
   reports nothing rather than 24 findings at its cap, so it can fail the boot
   on a new nesting instead of printing one more line. §70's reason is
   corrected to say so.
