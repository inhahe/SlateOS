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

Two low-stakes confirmations are outstanding (from the Q1/Q2 follow-ups — full
reasoning in `operator-answers-2026-06-13.md`; neither blocks anything, both are
already the shipped behavior, so these are "say so if you want them changed"):

- **Q1 confirm:** keep returning success (option A) for the NUMA mempolicy
  syscalls on our single-node hardware? (recommended)
- **Q2 confirm:** keep the shipped commit-policy defaults — native
  strict-commit, Linux lazy/overcommit? (recommended)

No other questions are open — the operator's decision queue is otherwise empty.

---

Recently resolved (see `design-decisions.md` for the full rationale):

- The coreutils "which set is canonical?" question — resolved 2026-06-12;
  standalone per-tool crates are canonical (§8).
- Q1 `set_mempolicy_home_node` / NUMA mempolicy on UMA — resolved 2026-06-13;
  keep the UMA no-op returning 0, option A (§10).
- Q2 `/proc/sys/vm/overcommit_memory` & memory-commit policy — resolved
  2026-06-13; keep `vm/` omitted now, build the configurable both-strategies
  model (Option 5) as the end-state; do not add `CAP_SYS_ADMIN` as a native
  capability — map it to fine-grained native caps (new `admin.memory_policy`
  for the system-wide overcommit knob) (§11).
- Q3 next major initiative — resolved 2026-06-13; terminal/dev before GUI,
  GCC/CMake/Make toolchain first, CPython then fastpy (§9).
- Q4 toolchain on Slate OS: run-prebuilt-Linux vs native-port — resolved
  2026-06-13; **Path Z** (run prebuilt Linux toolchain binaries on the Linux-ABI
  layer now, native-port selectively later), native-first/no-leak kept
  inviolate, clang green-lit for install (§12).
