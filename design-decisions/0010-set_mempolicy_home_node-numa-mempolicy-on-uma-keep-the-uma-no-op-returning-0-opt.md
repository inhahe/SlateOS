## 10. set_mempolicy_home_node / NUMA mempolicy on UMA — keep the UMA no-op returning 0 (option A)

**Date:** 2026-06-13

**Decided by:** Operator (this was `open-questions.md` Q1; Claude recommended
option A and laid out the UMA/NUMA/VMA tradeoff; the operator chose A).
**Re-confirmed by the operator 2026-06-14** ("go with your recommendation") when
the standing Q1 confirm was put to them — option A stands.

**Context:**
SlateOS is a single-node **UMA** system (all CPUs reach all RAM at equal latency —
the desktop hardware we target). Linux's NUMA mempolicy family
(`mbind`/`set_mempolicy`/`set_mempolicy_home_node`) lets a program request that
specific regions of its address space be backed by specific NUMA *nodes*. On UMA
there is exactly one node, so any such policy is functionally a no-op. The
question was what `set_mempolicy_home_node` should return on a valid non-empty
range when we keep `mbind`/`set_mempolicy` as no-ops:
- **(A)** return 0 (success) *(current)*,
- **(B)** return `-ENOENT` (Linux's literal answer for a default-policy range),
- **(C)** implement real per-VMA mempolicy storage so the errno can be
  discriminated faithfully (per-VMA policy objects, `mbind_range`, `mpol_dup` on
  fork — substantial machinery for zero functional effect on UMA).

**Decision — option A: keep the UMA no-op and return 0.**
`set_mempolicy_home_node` on a valid non-empty range returns 0;
`mbind`/`set_mempolicy` continue to accept-and-drop the policy. No per-VMA
policy storage is built.

**Rationale:**
- **Negligible stakes on UMA.** Only programs that call `set_mempolicy_home_node`
  (a NUMA-tuning syscall, Linux 5.17+) are affected — server software tuned for
  multi-socket boxes plus `numactl`/`libnuma`. That's **<0.1% of programs and
  ~0% of desktop programs**; native SlateOS programs are unaffected entirely (NUMA
  mempolicy is a Linux-ABI construct).
- **A maximizes Linux-app compatibility.** The common real sequence is
  `mbind(MPOL_BIND)` then `set_mempolicy_home_node`; returning 0 keeps that path
  succeeding, which is what glibc/libnuma expect. Option B would report failure
  for a sequence Linux accepts (triggering "kernel lacks home-node" warnings or
  degraded fallback paths). Neither A nor B can crash a program or stop it
  starting — the difference is at most a warning log on B.
- **C is real, fragile code for no benefit.** Per-VMA policy means every VMA
  split/merge (`mmap`/`munmap`/`mprotect`/`madvise`/`mremap`) and `fork` must
  carry/dup the policy — meaningful complexity whose entire payoff is faithful
  errnos on syscalls almost nothing calls, with zero effect on what any program
  computes or how fast it runs (one node).

**Alternatives considered:**
- **(B) return `-ENOENT`** — rejected: "more literal" only for a case that has no
  practical consequence on UMA, and it breaks the common post-`mbind` success
  path.
- **(C) per-VMA mempolicy storage** — rejected for now: substantial, bug-prone
  machinery for zero UMA benefit. **The correct trigger to revisit is SlateOS ever
  targeting real multi-node (multi-socket) hardware** — at which point C should
  be implemented *properly* (real page placement, not just errno cosmetics), and
  the faithful errnos come for free.

**Where it lives:**
- `kernel/src/syscall/linux.rs`: `sys_set_mempolicy_home_node`, `sys_mbind`,
  `sys_set_mempolicy`, `sys_get_mempolicy` (the empty-mask/default-policy
  answers).
- `known-issues.md` TD7 (the UMA no-op tech-debt note).

**How to reverse:**
- If a multi-node target appears: implement per-VMA mempolicy + node-aware
  allocation, then make `set_mempolicy_home_node` walk real per-VMA policies and
  return `-ENOENT`/`-EOPNOTSUPP`/0 per Linux. Until then, A stands.
