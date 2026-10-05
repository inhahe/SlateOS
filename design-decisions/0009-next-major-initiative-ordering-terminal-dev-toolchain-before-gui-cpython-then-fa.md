## 9. Next major initiative ordering — terminal/dev toolchain before GUI; CPython then fastpy (fastpy depends on CPython)

**Date:** 2026-06-13 (corrected same day — see CPython-dependency note)

**Decided by:** Operator (Claude surveyed the roadmap, found bounded work
exhausted, and put the strategic ordering to the operator as `open-questions.md`
Q3 with options A–E and a recommendation of "bash first"; the operator chose a
different ordering — toolchain before bash, terminal/dev before GUI, and Python
via fastpy. The operator subsequently corrected a factual error in Claude's
write-up: fastpy is **not** an alternative to CPython but **depends on** it, so
the ordering is CPython *then* fastpy, not "fastpy instead of CPython").

**Context:**
An autonomous-loop survey (2026-06-13) confirmed every readily-actionable
roadmap surface was already mature (procfs/`/proc/sys`, sysfs, sysctlfs, the
full Linux syscall table, the POSIX layer, the container runtime, the ALSA
shim, the DRM/KMS shim). The only remaining roadmap work is large, multi-day
*ports*, each a costly and hard-to-reverse commitment with no obviously-correct
ordering — so the direction was put to the operator rather than picked
autonomously. The candidates were: (A) bash, (B) GCC/CMake/Make toolchain +
CPython, (C) GPU drivers → Mesa → Vulkan/OpenGL, (D) WINE, (E) Chromium.

**Decision:**
- **Terminal / developer environment comes before the GUI stack.** Build out a
  usable command-line dev environment first; defer the GPU/Mesa/compositor app
  vision (options C/D/E) until that's in place.
- **Port the GCC/CMake/Make toolchain (roadmap task 5031) before bash (task
  1491).** The toolchain is the prioritized next initiative.
- **Port CPython (task 5033) *first*, then integrate fastpy (tasks 24 + 5034) on
  top of it.** fastpy is the preferred *fast* execution path for SlateOS userspace
  Python (it AOT-compiles Python to native code and is many times faster than
  CPython, and is maintained to be CPython-3.14-compatible). **But fastpy is not
  a standalone replacement for CPython — it depends on the CPython runtime/DLL as
  a bridge** for a set of operations it does not implement natively, most notably
  **importing binary/compiled Python extension modules** (the C-API extension
  ecosystem). So CPython must be ported *before* fastpy can run, and CPython
  stays resident as fastpy's bridge — it is a **prerequisite and a runtime
  dependency**, not an alternative we skip. **Status check:** neither is ported
  yet — task 5033 (CPython) is `[ ]`, and tasks 24 & 5034 (fastpy) are `[ ]`
  (unstarted) in `roadmap.md`.

**Rationale:**
- A working dev toolchain is the foundation for self-hosting and for building
  everything downstream; it rides the already-mature POSIX layer and has **no
  GPU dependency**, making it the least-blocked big initiative. Doing it before
  the GUI is the intuitive ordering (you build the tools before the storefront).
- fastpy gives CPython-3.14 compatibility at much higher performance, and the
  project's own guidance already prefers "Python via fastpy" for userspace
  components (CLAUDE.md). But because fastpy bridges to the CPython runtime/DLL
  for binary-extension imports and other unimplemented operations, CPython is a
  hard prerequisite — porting CPython is not optional work we can defer in favor
  of fastpy; it is step one, with fastpy layered on top as the fast path.

**Honest nuance recorded at decision time (toolchain ↔ shell bootstrap
co-dependency):** the operator's reasoning was "porting bash will be easier once
the toolchain exists." The dependency is *mostly* the other way around —
GCC/Make are built and driven *by* a shell (`configure` scripts, recipe command
lines invoke `/bin/sh`). In practice neither strictly blocks the other here
because SlateOS already has a kernel shell (`kshell`) and a coreutils set, and the
toolchain itself is **cross-built on the dev host**, not self-hosted on SlateOS
initially — so we don't need bash-on-SlateOS to *produce* the toolchain binaries.
The conclusion (toolchain first) stands; the ordering is fine because the
host-side cross-build sidesteps the circular dependency. A full `make` driving
`configure` scripts *on SlateOS* will eventually want a real `/bin/sh`, at which
point bash (or a smaller POSIX sh) becomes the natural follow-on.

**Alternatives considered:**
- **(A) bash first** — Claude's original recommendation (least-blocked, highly
  decomposable, high leverage). Not chosen: the operator preferred the toolchain
  first; the bootstrap nuance above shows bash-first isn't *required* for the
  toolchain, so toolchain-first is a valid ordering.
- **(C/D/E) GPU/Mesa, WINE, Chromium** — deferred: these are the GUI/app long
  pole, the most hardware-dependent, and (D)/(E) are gated behind the GPU/Mesa
  work. The operator explicitly wants terminal/dev before GUI.
- **"fastpy instead of CPython" (skip the CPython port)** — Claude's original
  write-up framed it this way; **corrected and rejected** by the operator:
  fastpy depends on the CPython runtime/DLL (binary-extension imports etc.), so
  CPython can't be skipped. The relationship is CPython-then-fastpy, with CPython
  remaining resident as the bridge.

**Where it lives:**
- `roadmap.md`: task 5031 (gcc/cmake/make/pkg-config — next), task 5033 (CPython
  — prerequisite for fastpy, port first), tasks 24 & 5034 (fastpy integration &
  compiler — layered on CPython), task 1491 (bash — follow-on after toolchain).
- New top-level work; entry points emerge as the toolchain port begins
  (build wiring under the workspace + `pkg/`/`userspace/` as needed).

**How to reverse:**
- Re-prioritize by reordering the roadmap tasks. The CPython→fastpy dependency is
  not a preference but a technical fact (fastpy's bridge), so it can't be
  reordered unless fastpy gains a native binary-extension loader that removes the
  CPython dependency. If GUI work becomes more urgent than dev tooling, start
  option (C) instead — but per this decision, terminal/dev leads.
