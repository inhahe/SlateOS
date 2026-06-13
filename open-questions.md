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

## Q4 — Toolchain on OuRoS (task 5031): *run prebuilt Linux* vs *native port*, + a missing prerequisite

- **Status:** OPEN  *(reframed 2026-06-13 after operator feedback — see "Reframing" below)*

- **Question:** You chose the GCC/CMake/Make toolchain as the next major
  initiative (design-decisions.md §9). The first real fork is **how
  gcc/make/cmake/cpython/bash actually run on OuRoS** — as *prebuilt Linux
  binaries on the kernel's Linux-ABI layer*, or as *native OuRoS binaries we
  port/recompile*. This shapes the whole effort and is costly/hard-to-reverse.

- **Reframing (operator's instinct: favor what gives the Linux compatibility we
  need anyway).** My first cut framed this as "clang-bootstrap vs build-a-gcc-
  cross-compiler" — but both of those produce a *native* toolchain and **neither
  advances Linux compatibility.** The option that *is* Linux compat is running
  prebuilt Linux binaries on the compat layer. The three real paths:

  - **Path X — Run prebuilt Linux toolchain binaries on the Linux-ABI layer.**
    The kernel's Linux ELF loader + `ld.so` loading + full Linux syscall table
    (just verified) exist to run *unmodified* Linux binaries. Drop a distro's
    prebuilt gcc/make/cmake + its glibc/`ld.so` onto the disk image; harden the
    compat layer until they run. The roadmap's "gcc … *(via POSIX layer)*"
    wording matches this.
    - *Pros:* **this is the Linux compatibility we need anyway**; least work to a
      *usable* toolchain (gcc/CPython are millions of lines — no recompile);
      needs **no host C cross-compiler**; directly hardens the Linux-ABI layer
      (reused by every future Linux app).
    - *Cons:* runs as Linux-ABI processes (not native syscalls / capability-
      native); depends on the compat layer being complete enough for demanding
      programs (gcc forks subprocesses, heavy file/mmap/`/proc` use) — iterative
      hardening; must obtain + place a real glibc/musl runtime on the image.
  - **Path Y — Native OuRoS toolchain (recompile/port against `posix`).** Build
    a gcc cross-compiler targeting `x86_64-ouros` + a native C library (extend
    `posix`, or port musl to native syscalls), producing native-syscall
    binaries. The "purity" path (microkernel/capability-native).
    - *Pros:* native syscalls + capability security; smaller, OuRoS-idiomatic
      binaries; eventual self-hosting.
    - *Cons:* **does not advance Linux compatibility**; enormous per-program
      effort for gcc/CPython (canadian cross + full native libc); slowest to a
      working toolchain; CLAUDE.md already prefers **fastpy** for native OS
      userspace, shrinking how much big C software we'd ever want native.
  - **Path Z — Hybrid (recommended): Path X now, Path Y selectively later.**
    Run prebuilt Linux tools to get a working dev environment + harden Linux
    compat now; native-port only the specific components where capability-native
    behavior matters, later.

- **The clang question (orthogonal — it's a *tool*, not a path):** clang is one
  binary that targets *both* `x86_64-linux-*` *and* `x86_64-ouros`. Installing it
  is **low-regret regardless of path**: under Path X it lets us compile real
  Linux C programs to *systematically stress and harden the Linux-ABI layer*
  (the gating work); under Path Y it can cross-compile native C. The earlier
  "fast hello-world" pitch undersold it — its real value is driving Linux-compat
  hardening.

- **The prerequisite (a real blocker either way):** *no C compiler on this
  machine can target OuRoS or build/test C for it.* Only `clang-format`/
  `clang-tidy` ship with VS; `cl.exe` targets Windows PE/COFF. We need a real
  compiler installed — **clang (recommended; one install serves both ABIs)**, or
  MSYS2 + a gcc cross-build for Path Y. Installing system software is an
  operator-level environment change, so I'm not doing it unprompted.

- **What already exists:** custom Rust target `toolchain/x86_64-ouros.json`; a
  comprehensive Rust-implemented native libc (`posix/`, ~2300 files →
  `toolchain/sysroot/lib/libc.a`) with C-runtime glue in `posix/src/crt.rs`;
  Rust `std` runs on it (§5036 chose the libc-shim approach). The full kernel
  Linux-ABI layer (Linux ELF loader, `ld.so` loading, Linux syscall table,
  procfs/sysfs) — the foundation Path X builds on.

- **Claude's recommendation:** **Path Z (X first), install clang.** It is the
  Linux compatibility we need anyway, the fastest route to a usable toolchain,
  and reuses the Linux-ABI investment. Native porting (Y) is a later, optional
  purity step. **To start I need:** your pick of X/Y/Z, and an LLVM/clang install
  (or your go-ahead to attempt it).

- **Where it bites:** *Path X* — Linux-ABI hardening across `kernel/src/syscall/
  linux.rs`, ELF loader, procfs/sysfs; disk-image work to place a glibc/`ld.so`
  runtime. *Path Y* — `toolchain/sysroot/include/` (C headers), `posix/`, a
  crt0, gcc cross-build wiring. roadmap.md task 5031; design-decisions.md (new
  entry once decided).

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
