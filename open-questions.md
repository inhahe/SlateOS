# Open Questions — Operator Decision Queue

Decisions that genuinely need the human operator: architectural forks,
user-visible policies, and tradeoffs with no obviously-correct answer that
Claude has **deferred** rather than resolved autonomously.

This file is distinct from:

- **`design-decisions.md`** — decisions already *made* (each marked with who
  decided it). When the operator answers a question here, move it there as a
  `Decided by: Operator` entry and delete it from this file.
- **`known-issues.md`** — bugs and accumulated technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.

Format for each entry:

- **Question** — the decision to be made.
- **Options** — each with its pros and cons.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

---

(The coreutils "which set is canonical?" question was resolved on 2026-06-12 —
standalone per-tool crates are canonical; see `design-decisions.md` §8.)

---

### Q1. Per-VMA mempolicy storage — and the `set_mempolicy_home_node` 0-vs-`-ENOENT` choice

- **Question** — Should the kernel implement real per-VMA NUMA mempolicy
  storage, or keep treating `mbind`/`set_mempolicy` as UMA no-ops? This drives
  the return value of `set_mempolicy_home_node` on a valid non-empty range.
- **Background** — We are a single-node (UMA) system, so NUMA policy has no
  functional effect; `mbind` currently accepts and drops the policy (returns
  0). Linux's `set_mempolicy_home_node` walks the range's VMAs and returns
  `-ENOENT` when none has an explicit `MPOL_BIND`/`MPOL_PREFERRED_MANY` policy,
  `-EOPNOTSUPP` for a wrong-mode policy, or 0 once a bind policy is found.
  Without per-VMA policy storage we can't distinguish these cases.
- **Options**
  - **(A) Keep UMA no-op, return 0** *(current)* — pro: matches the common
    real-world path (`mbind(MPOL_BIND)` then `set_mempolicy_home_node` → 0);
    libnuma/glibc see success. con: returns 0 where Linux returns `-ENOENT`
    for a default-policy range; not fully faithful.
  - **(B) Keep UMA no-op, return `-ENOENT`** — pro: matches the "no explicit
    policy" path literally. con: breaks the common post-`mbind` success path
    (we'd report failure for a sequence Linux accepts); glibc would log
    "kernel lacks home-node" warnings.
  - **(C) Implement per-VMA mempolicy storage** — pro: fully faithful errno
    discrimination for the whole mempolicy family. con: substantial machinery
    (per-VMA policy objects, mbind_range, mpol_dup) for zero functional effect
    on a UMA system.
- **Claude's recommendation** — Stay on **(A)** for now (done). Only pursue
  **(C)** if a real multi-node target appears or an app actually depends on the
  `-ENOENT` discrimination. Documented as `known-issues.md` TD7.
- **Where it bites** — `kernel/src/syscall/linux.rs`:
  `sys_set_mempolicy_home_node`, `sys_mbind`, `sys_set_mempolicy`,
  `sys_get_mempolicy` (the empty-mask/default-policy answers).
- **Status** — OPEN
