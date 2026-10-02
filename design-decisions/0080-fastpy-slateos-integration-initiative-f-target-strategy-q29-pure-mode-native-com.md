## 80. fastpy → SlateOS integration (initiative F) target strategy (Q29) — **pure-mode native compile first (A); add the CPython bridge later as a superset (B)**

**Date:** 2026-07-21
**Decided by:** Operator (Claude recommended A-first-then-B; operator confirmed
"A at first but eventually B") — resolves open-question **Q29**, unblocking the
*start* of initiative F.

**Context.** fastpy is an AOT Python→LLVM-IR→native compiler that today targets the
**host** only and links a C runtime plus an embedded-CPython bridge (for programs
using unsupported stdlib). Making it emit **SlateOS** binaries needs: (1) an
`x86_64-slateos` LLVM target/data-layout (modest), (2) the C runtime ported to
SlateOS syscalls/libc (gated on the Phase 2.5 POSIX layer), and (3) a decision on
the CPython bridge — the crux Q29 asked.

**Decision.** Begin with **pure-mode only (A)**: on the SlateOS target, compile
only programs fastpy supports natively and **disable the CPython fallback**. Add
the CPython bridge later (**B**) as an *enhancement/superset* once CPython is
ported to SlateOS (a large, later effort) — B becomes a strict superset of A, not
a competing design. Sequencing: mature the POSIX layer enough to host the C
runtime → add the `x86_64-slateos` fastpy target + port the runtime in pure mode →
pick one real OS component (e.g. the package manager) as the first
fastpy-compiled SlateOS binary.

**Rationale.** *Pro:* A is the only path that both starts soon *and* delivers the
roadmap's stated goal (native components that run *on* SlateOS); no CPython port
needed up front; matches how OS components would actually be written (plain typed
Python). *Con:* until fastpy grows native support for a given stdlib module, a
component using it won't compile on SlateOS — "any valid Python is valid fastpy"
doesn't hold *on-target* until B lands; commits the project to a prerequisite
chain (POSIX layer → runtime port → first component).

**Operator's exact words.** "A at first but eventually B."

**Where it lives.** fastpy `compiler/toolchain.py` (target triple / data layout,
currently host-only via `llvm.Target.from_default_triple()`; `link_executable`
unconditionally links libpython — the bridge to gate off for the slateos target),
fastpy `runtime/*.c` (syscall/libc surface to port), SlateOS `posix/` (Phase 2.5
libc coverage the runtime links against); roadmap.md Phase 0 (the F task).
Supersedes Q29 in open-questions.md (now removed).
