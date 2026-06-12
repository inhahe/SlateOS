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

## 1. coreutils crate vs. standalone per-tool crates — which set is canonical?

**Status:** OPEN

**Question:**
There is **duplication** between the `coreutils` crate's bundled binaries
(`coreutils/src/bin/{tr,dd,chown,df,…}`) and the standalone per-tool workspace
crates (`userspace/tr`, `userspace/dd`, `userspace/chown`, `userspace/df`, …).
Both implement the same tools. Which set is the canonical one that ships in the
OS image, and what happens to the other? The two can — and will — drift.

**Background:**
- The kernel-embedding/deployment request points at `coreutils/target` for
  deployment.
- But the **standalone crates** are the ones that are workspace members, have
  unit tests, and received the syscall-ABI / `std`-migration fixes. They are
  the more correct and better-maintained set today.
- Related (and now largely resolved) context: userspace tools should use `std`
  (`std::fs`/`std::time`/`std::process`), which routes through the `posix`
  crate's `extern "C"` libc shims to native OuRoS syscalls — not hand-rolled
  raw syscall numbers. Whichever set is chosen as canonical should be on `std`.

**Options:**
- **(a) Standalone crates are canonical; retire the `coreutils` duplicates.**
  - Pros: keeps the tested, fixed, workspace-member versions; one tool = one
    crate is simple to reason about; matches where the maintenance has gone.
  - Cons: the image build currently points at `coreutils/target` — the build
    + deployment wiring must be repointed at the standalone crates; larger
    build graph (many small crates).
- **(b) `coreutils` bundle is canonical; fold the standalone fixes into it and
  retire the standalone crates.**
  - Pros: one multi-call binary (busybox-style) is smaller on disk and matches
    the existing deployment target; fewer crates to build.
  - Cons: must port every syscall-ABI / `std` fix from the standalone crates
    into the bundle; loses the per-tool unit tests unless those are migrated
    too; multi-call binaries complicate per-tool dependency isolation.
- **(c) Keep both, with one generated from the other.**
  - Pros: no immediate deletion.
  - Cons: needs real machinery to keep them in lockstep; until that exists this
    is the status quo that *causes* the drift — not really a resolution.

**Claude's recommendation:**
Option (a) — make the tested standalone crates canonical and repoint the image
build at them, retiring the `coreutils/src/bin` duplicates. This preserves the
work and tests that already exist and avoids a fix-porting pass. But this is a
build/deployment-policy call with real consequences, so it's left for the
operator. In the meantime Claude continues to maintain/fix the **standalone**
crates (the tested set) and does not invest in the `coreutils` bundle.

**Where it bites:**
- `coreutils/src/bin/*` (bundled binaries) vs. `userspace/<tool>/` (standalone
  crates).
- The OS image build / kernel-embedding wiring that currently targets
  `coreutils/target`.
- `todo.txt`: the "DUPLICATION between the coreutils crate's bins …" judgment
  call (2026-05-31) and the "USE STD" audit note.
