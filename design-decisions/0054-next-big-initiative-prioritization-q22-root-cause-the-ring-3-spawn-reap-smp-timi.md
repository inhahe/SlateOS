## 54. Next-big-initiative prioritization (Q22) — root-cause the ring-3 spawn/reap SMP timing race first (option D)

**Date:** 2026-07-02
**Decided by:** Operator (Claude recommended A + D-when-reachable; operator chose D)

**Context.** With the editor merge-on-external-change request complete and a full
cross-phase roadmap survey showing the project extraordinarily mature, the only
substantial remaining work fell into two buckets: giant external ports (dev
toolchain, Chromium, GPU/Mesa, extra filesystems) and deferred-risky internal
kernel work (the ring-3 spawn/exec/reap SMP timing race + TD31 symmetric cgroup
accounting + TD32 mount-tree remainder). Q22 asked the operator which to green-
light next: **A** dev toolchain (gcc/CPython/fastpy), **B** Chromium/web stack,
**C** GPU drivers + Mesa, **D** root-cause the spawn/reap SMP race, **E** extra
filesystems / container mount-tree.

**Decision.** The operator chose **D**, with the stated rationale "I like all
bugs to be solved asap." This authorizes fully working the ring-3 spawn/exec/reap
SMP timing race in a supervised session — the one class of kernel work CLAUDE.md
otherwise warns not to destabilize unsupervised. The operator being reachable
satisfies the prior condition (sanity-check boot stability across several runs).

**Why D over the recommended A.** Claude recommended A (dev toolchain) as the
highest-leverage *port* with D done when the operator is reachable. The operator
prioritizes bug elimination over new capability: fixing the spawn/reap race
unblocks TD31 (symmetric cgroup nr_tasks accounting) and the deferred fork/wait
E2E self-test, and directly improves boot stability — clearing the deferred-risky
kernel-bug bucket before taking on a large port. Both are defensible; the operator
owns the product/prioritization call (which is exactly why it was reserved).

**Consequence / plan.** Establish a green boot-test baseline, study the spawn/
exec/reap + kill/reap paths and the prior TD31 patch, assess whether the
B-PREEMPT-SPINLOCK fix (2026-07-01, claimed true root cause) already resolves the
residual WATCH flakes (B-DASH-STDIN-FLAKE, B-PTHREAD-YIELDBUDGET), instrument any
remaining race, and re-attempt TD31 boot-testing >=3x for stability. The other
Q22 options (A/B/C/E) remain available for a future steer and are NOT closed by
this decision.
