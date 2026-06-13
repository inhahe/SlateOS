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

---

### Q2. Should `/proc/sys/vm/overcommit_memory` (and the `vm/` tree) be exposed, and at what value?

- **Question** — The new `/proc/sys` sysctl tree (procfs.rs, task 5092)
  deliberately omits the `vm/` subtree. The first candidate is
  `vm/overcommit_memory`. Should we expose it, and if so report which value?
- **Background** — `design.txt`/CLAUDE.md mandate "Committed memory by default,
  lazy allocation opt-in. No silent overcommit." That policy maps cleanly onto
  Linux's `vm/overcommit_memory = 2` (strict accounting: total commit may not
  exceed swap + RAM·ratio), **not** the Linux default `0` (heuristic
  overcommit). So the *honest* value reflecting our design is `2`. The hesitation
  is purely about second-order app behavior: some Linux apps read this file and
  change strategy (e.g. Go/JVM/Electron/WINE allocate large sparse mappings
  expecting lazy backing; on seeing strict accounting they may shrink arenas or
  refuse to start). Our `/proc/sys` is read-only, so an app that tries to *write*
  it (to request overcommit) gets a write error — which Linux apps generally
  tolerate (the write needs CAP_SYS_ADMIN anyway).
- **Options**
  - **(A) Expose `vm/overcommit_memory = 2`** — pro: honest reflection of the
    "no silent overcommit" design; apps that respect it allocate within real
    limits. con: a minority of apps tuned for the Linux default-`0` world may
    behave conservatively or warn; read-only means they can't flip it.
  - **(B) Expose `vm/overcommit_memory = 0`** (advertise heuristic overcommit) —
    pro: matches what most Linux desktop apps assume, maximizing drop-in
    compatibility. con: a *lie* — we don't actually overcommit, so an app that
    trusts `0` and over-allocates would hit commit failures our design intends
    to surface up front; contradicts the design and the "never fabricate" rule.
  - **(C) Keep `vm/` omitted** *(current)* — pro: an absent file makes glibc/apps
    fall back to their built-in default assumptions rather than acting on a
    value we're unsure about; no fabrication. con: some readers treat a missing
    sysctl as an error or log noise; we forgo signalling our real policy.
- **Claude's recommendation** — Lean **(A)** (`= 2`) on the merits — it's the
  honest, design-faithful value and read-only exposure is harmless — but this is
  a user-visible compatibility/behavior tradeoff, so deferring to the operator
  rather than guessing. Staying on **(C)** (omitted) until decided. If (A) is
  chosen, `vm/overcommit_ratio` (default 50) and `vm/overcommit_kbytes` (0)
  would naturally follow for completeness.
- **Where it bites** — `kernel/src/fs/procfs.rs`: `SYS_FILES`/`SYS_DIRS`
  (add `"vm"` dir + `"vm/overcommit_memory"`), `gen_sys` (the value), and the
  procfs self-test.
- **Status** — OPEN

---

### Q3. Which major initiative comes next? (the autonomous loop has run out of *bounded* roadmap work)

- **Question** — An autonomous-loop survey (2026-06-13) confirmed that every
  readily-actionable surface is already mature, so the only remaining roadmap
  work is large multi-day ports. Which should be prioritized? This is a strategic
  direction call with a costly, hard-to-reverse commitment (days of work each)
  and no obviously-correct ordering, so it's being put to the operator rather
  than picked autonomously.
- **What's already done (why there's no bounded increment left to grab):**
  - `/proc` + `/proc/sys` (procfs.rs) — exhaustive; further sysctl entries are
    blocked on **Q2** or lack honest backing.
  - sysfs (`sysfs.rs`), sysctlfs (`sysctlfs.rs`) — present.
  - Linux syscall table (`syscall/linux.rs`) — every named syscall has a handler
    (down to historical no-ops like `nfsservctl`/`tuxcall`/`vserver`); the
    "syscall-by-syscall audit" (task 5089) yields no edits — coverage is complete.
  - POSIX layer (`posix/src/`, ~2294 files) — no `todo!`/`unimplemented!` stubs;
    extraordinarily complete.
  - Container runtime (task 5223) — complete except "Port Docker".
  - ALSA shim (task 5095) — complete except STATUS ioctl (**TD10**, blocked on the
    time64 timespec-ABI decision) and a real hardware audio backend (a driver task).
  - DRM/KMS Linux-ABI shim — recent; swept clean; open debt is **TD11/TD12**.
  - Most recent bug-hunt find: **F12** (alsa_pcm mixer-slot leak), now fixed.
- **Options** (each is a large initiative; dependency notes in parens)
  - **(A) Port bash** (task 1491) — gateway to a real interactive userspace.
    Depends on the POSIX libc layer (1184/1430), which is already very mature, so
    this is plausibly the *least-blocked* big task. Best if the goal is a usable
    shell / dev environment.
  - **(B) Port the GCC/CMake/Make toolchain** (5031) + **CPython** (5033) — a
    self-hosting dev environment; also rides the mature POSIX layer.
  - **(C) GPU drivers → Mesa → Vulkan/OpenGL** (4554/4582) — unblocks the GUI
    stack and is a prerequisite for (D)/(E). Largest and most hardware-dependent.
  - **(D) WINE** (5096) — Windows-app support; needs Mesa + audio (audio shim is
    in place; Mesa is not), so effectively gated behind (C).
  - **(E) Chromium** (5025) — needs POSIX + GPU + audio + networking; the heaviest
    single port; gated behind (C).
- **Claude's recommendation** — **(A) bash** as the immediate next step: it's the
  least-blocked (rides the mature POSIX layer, no GPU dependency), it's
  decomposable into small tested increments (the ALSA shim showed this pattern
  works well for the autonomous loop), and a working shell is high leverage for
  everything downstream. (C) is the long pole for the GUI/app vision and should
  start in parallel when the operator is available to steer hardware/driver
  choices. **The autonomous loop is stopping here** rather than guessing a
  multi-day direction or spinning on no-edit sweeps; resume it (or point it at a
  specific target, e.g. "get bash building") when you're back.
- **Where it bites** — new top-level work; entry points depend on the choice
  (`userspace/` for a bash port, `drivers/`/`gui/gpu/` for GPU/Mesa).
- **Status** — OPEN
