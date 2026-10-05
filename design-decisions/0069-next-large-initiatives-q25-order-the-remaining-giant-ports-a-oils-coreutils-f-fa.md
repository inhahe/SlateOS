## 69. Next large initiatives (Q25) — order the remaining giant ports: **A(Oils+coreutils) → F(fastpy) → B(Mesa/GPU) → C(Chromium) → D(WINE) → E(filesystems)**

**Date:** 2026-07-18

**Decided by:** Operator (Claude recommended "A first, then F"; the operator
adopted that and fixed the full ordering of the remaining initiatives).

**Context.** With the self-hosting C toolchain (tcc on-target, glibc + `ld.so`
dynamic linking, ring-3 execution, the Path-Z self-test suite) and the POSIX
layer both comprehensive, the roadmap's entire remaining unchecked frontier is
"giant external ports." Picking the order among them has historically been the
operator's call (open-questions Q25).

**Decision.**
- **Do the interactive-shell userland first.** The item labeled "bash" in Q25
  option A is **not bash** — the shell we port is **Oils (OSH)**, the
  bash-compatible *superset* already on the roadmap ("Port Oils (bash-compatible,
  replaces bash for POSIX compatibility)", `roadmap-detailed.md` §2.7 Shells,
  ~line 861). OSH runs existing bash scripts (superset) and is the POSIX/bash
  compatibility shell; Nushell remains the default *interactive* shell. So Q25-A
  = **Oils + coreutils**, not a bash port.
- **Fixed order for the remaining giant initiatives** (so this need not be
  re-asked later):
  1. **A — Oils (OSH) + coreutils** (interactive shell userland).
  2. **F — fastpy build-system integration** (unblocks writing OS userspace
     tools in Python-via-fastpy: package manager, settings UI, file indexer,
     installer, etc.).
  3. **B — Mesa / GPU userspace** (3D; still gated by Q18 on a virgl test
     environment — see that item).
  4. **C — Chromium** (browser + "system web app"/Electron framework).
  5. **D — WINE** (Windows app compatibility).
  6. **E — Additional filesystems** (Btrfs / F2FS / NTFS).

**Rationale.** A is the smallest, highest-leverage next step and builds directly
on the just-proven tcc/glibc/`ld.so`/ring-3 path; a working shell + coreutils is
the natural foundation for everything else and is continuously shippable one tool
at a time. F then unlocks the Python userspace lane (a force-multiplier for the
many small system tools `CLAUDE.md` says to write in fastpy). B/C/D/E are larger
and either gated (B on Q18/virgl) or dependent on more maturity (C/D on
graphics+audio); E is self-contained and lowest immediate payoff, so it sorts
last.

**Alternatives considered.** Leading with B/C/D/E instead of A/F — rejected:
they are larger, some are gated, and none give the incremental
shell-plus-coreutils foundation that unblocks the most subsequent work. Doing F
before A — rejected: fastpy integration is valuable but the shell/coreutils
userland is the more universal unblocker and the smaller gap from what's proven.

**Where it lives.** `roadmap.md` (line ~1494 bash/Oils; line ~24 fastpy; lines
~5117–5119 filesystems; line ~5032 Chromium; line ~5114 WINE);
`roadmap-detailed.md` §2.7. The practical gates for A are the fork/exec WATCH
bugs in `known-issues.md` (B-FORKEXEC-BOOT-HANG, B-PTHREAD-TEARDOWN-PF).

**How to reverse.** Re-open Q25 and re-sequence; the ordering is guidance for
task-selection, not a code commitment, so reversing costs nothing but a new
decision.
