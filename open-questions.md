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

## Q4 — Toolchain port (task 5031): bootstrap-compiler strategy + a missing prerequisite

- **Status:** OPEN

- **Question:** You chose the GCC/CMake/Make toolchain port as the next major
  initiative (design-decisions.md §9). Starting it surfaced a hard prerequisite
  and an architectural fork that I'd rather not resolve unilaterally, because
  it's a costly, multi-day, hard-to-reverse commitment and it requires changing
  the dev environment (installing a compiler).

- **The prerequisite (a real blocker):** *there is currently no C compiler on
  this machine that can cross-compile to OuRoS.* I checked: no `clang.exe`
  (only `clang-format`/`clang-tidy` ship with VS), no `gcc`, no `zig`. The only
  C/C++ compiler present is MSVC `cl.exe`, which targets Windows PE/COFF and
  cannot emit `x86_64-ouros` ELF binaries. So *nothing C-related for the
  toolchain can be built or tested until a cross-capable C compiler is
  installed.* Installing system software / changing the dev toolchain is an
  operator-level environment change, so I'm not doing it on my own.

- **What already exists (so we're not starting from zero):** a custom Rust
  target `toolchain/x86_64-ouros.json`; a comprehensive Rust-implemented libc
  (`posix/`, ~2300 files) that exports C-ABI symbols backed by native syscalls,
  built into `toolchain/sysroot/lib/libc.a`; C-runtime startup glue in
  `posix/src/crt.rs` (`__libc_start_main`, `atexit`, C++ ABI stubs). Rust `std`
  already runs against this shim (design-decisions.md §5036 deliberately chose
  the "libc-compatible shim" approach over forking std/libc). **What's missing
  for compiling *C* against it is a C header tree** (`<stdio.h>`, `<stdlib.h>`,
  …) declaring those symbols — plus a compiler to use them.

- **Options:**
  - **(A) Install LLVM/clang standalone; bootstrap with clang.** clang is a
    turnkey cross-compiler (one binary targets any triple via `--target`). We'd
    extend the existing `posix` shim into a full *C* libc by writing a header
    tree under `toolchain/sysroot/include/`, then cross-compile C against
    `clang --target=x86_64-... -isystem sysroot/include` + `libc.a` + a crt0.
    First milestone: a hand-written hello-world C program that runs on OuRoS
    (proves the C → libc → native-syscall path, the way I just proved the
    dynamic-linker path). clang can later build GCC itself.
    - *Pros:* leverages everything already built (custom target, posix shim,
      sysroot); least work to first running C binary; matches the project's
      established shim philosophy (§5036); clang installs cleanly on Windows.
    - *Cons:* clang ≠ gcc, so "port gcc" (the literal task) becomes a *later*
      phase (build gcc with clang once the C runtime is solid); some autoconf
      projects assume `gcc`/`cc` exists (mitigated by symlinking `cc`→clang).
  - **(B) Build a real GCC cross-compiler from source.** The literal "port gcc"
    path: set up MSYS2 (or WSL), build `binutils` + `gcc` targeting
    `x86_64-ouros`, and port a real C library for the target
    (musl/newlib) rather than using the Rust `posix` shim for C.
    - *Pros:* end-state is exactly a native gcc toolchain; a ported musl brings
      its own complete, battle-tested headers + implementation.
    - *Cons:* much heavier (multi-stage "canadian cross"); needs a Unix-y build
      env on Windows (MSYS2/WSL); porting musl/newlib to native syscalls
      duplicates a lot of what `posix` already does; slowest path to a first
      running C binary; partially abandons the existing shim investment.
  - **(C) Hybrid:** clang-bootstrap now (Option A) to get a working C runtime +
    self-tests quickly, then build gcc-on-OuRoS with clang as a later phase
    (Option B's end-state, reached via A). This is really "A first, B later."

- **Claude's recommendation:** **Option A / C** — install LLVM/clang, extend
  `posix` into a full C libc with a header tree, prove a hello-world C binary
  end-to-end, then layer gcc on top later. It reuses the substantial existing
  investment (custom target + posix shim + sysroot), matches §5036's chosen
  philosophy, and reaches a *testable* first milestone fastest. The only thing I
  need from you to start: **agreement on Option A and an LLVM/clang install**
  (or your go-ahead for me to attempt the install). If you prefer B, I'll set up
  MSYS2 + the cross-build instead.

- **Where it bites:** new `toolchain/sysroot/include/` (C headers), `posix/`
  (any C-only shims the headers expose), a crt0 object, build wiring under
  `toolchain/`; roadmap.md task 5031; design-decisions.md (new entry once
  decided).

---

Also awaiting your word (low-stakes confirmations from the Q1/Q2 follow-ups —
full reasoning in `operator-answers-2026-06-13.md`; neither blocks anything,
both are already the shipped behavior):

- **Q1 confirm:** keep returning success (option A) for the NUMA mempolicy
  syscalls on our single-node hardware? (recommended)
- **Q2 confirm:** keep the shipped commit-policy defaults — native
  strict-commit, Linux lazy/overcommit? (recommended)

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
