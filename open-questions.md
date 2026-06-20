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

## Q9 — How should the kernel auto-classify a *bare* static ELF (Linux ABI vs SlateOS-native)?

**Status:** OPEN

**Question.** When `spawn`/`exec` loads an ELF, it must decide whether the
binary speaks the Linux x86_64 syscall ABI (route `syscall` through
`kernel::syscall::linux`) or the SlateOS-native ABI (native dispatch table,
native initial stack, no Linux fd table). Today `elf::ElfFile::detect_linux_abi`
keys off three markers: `EI_OSABI == ELFOSABI_GNU`, a Linux `PT_INTERP`, or a
`PT_GNU_PROPERTY` segment. A **bare static Linux binary** — e.g. the output of
`tcc -nostdlib -static`, or a hand-rolled static musl/asm program — has *none*
of these (OSABI is plain `SYSV`/0, no interpreter, no GNU property note), so it
is misclassified as Native and its Linux `write`/`exit` syscalls hit the wrong
dispatch table (observed: `write(1,…)` produced 0 bytes; this was the root cause
of the Path-Z tcc self-test failure). The only GNU-ish marker tcc emits is a
`PT_GNU_RELRO` segment, which `detect_linux_abi` deliberately rejects (FreeBSD
clang emits it too, and a SlateOS-native binary built with GNU/LLVM tooling
might emit it as well — so it can't safely imply "Linux"). The fundamental
problem: **a bare SYSV static ELF carrying only generic GNU-toolchain artifacts
is genuinely ambiguous** between "Linux binary" and "SlateOS-native binary built
with a GNU/LLVM toolchain." No automatic heuristic can separate them reliably;
disambiguation requires an explicit marker on one side.

**Options.**

- **A — Flip the default to Linux; mark native binaries explicitly.** Treat any
  unmarked bare ELF as Linux ABI, and require the SlateOS-native toolchain to
  stamp native binaries with a distinct marker (a SlateOS `EI_OSABI` value in the
  arch range 64–255, or a `.note.slateos` `PT_NOTE`). *Pro:* every real-world
  Linux static binary (tcc output, static musl, `-nostdlib` asm) "just works",
  which is the whole point of the Linux-ABI layer / Path Z. *Con:* user-visible
  policy flip; the SlateOS toolchain must emit the marker; existing bare native
  test ELFs (`build_test_elf`) need the marker added; a *truly* unmarked native
  binary would be mis-run as Linux.
- **B — Keep native default; flag Linux binaries explicitly.** Leave bare ELFs
  defaulting to Native; callers that know a binary is Linux declare it (the
  `spawn_process_with_abi` hook added for the self-test, or a future per-exec
  flag / interpreter convention). *Pro:* no global policy change; native stays
  the "home" ABI. *Con:* a Makefile that builds a tool with tcc and then `exec`s
  it cannot transparently work — `exec` re-detects from the ELF and would pick
  Native — which undermines the "host a real toolchain" Path-Z goal.
- **C — Add `NT_GNU_ABI_TAG` note-walking (the punted signal #4).** Walk
  `PT_NOTE` for a `GNU/0,major,minor` ABI tag. *Pro:* catches most real
  dynamically- and statically-linked GNU/Linux binaries automatically. *Con:*
  does **not** catch tcc `-nostdlib` output (no notes at all), so it doesn't fix
  this case; still leaves the bare-ELF ambiguity for the default.
- **D — Hybrid (recommended).** Do A (default bare → Linux) *and* C (note-walk
  as a positive Linux signal) *and* stamp native binaries with an explicit
  SlateOS OSABI/note. Native is the side we fully control and can always mark;
  Linux is the open-world default. The `spawn_process_with_abi` override stays as
  the belt-and-suspenders for callers that know the ABI.

**Claude's recommendation:** **D** (or A as the minimal form). Native binaries
are produced exclusively by our own toolchain, so marking them is cheap and
unambiguous, whereas Linux binaries arrive from the outside world unmarked — so
the open-world default should be Linux. *In the meantime* the Path-Z tcc
self-test is unblocked via the new `spawn::spawn_process_with_abi(elf, options,
AbiMode::Linux)` entry point (the test *knows* it just compiled a Linux binary),
so no auto-detection change is needed to make the test pass. This question only
governs the **general** wild-ELF case (e.g. a Makefile exec'ing a freshly-built
tool), which no current code path exercises yet.

**Where it bites:** `kernel/src/proc/elf.rs::detect_linux_abi` (the heuristic);
`kernel/src/proc/spawn.rs::spawn_process_inner` (spawn ABI decision) and the
`exec` path around `new_abi_mode` (~line 946); `build_test_elf` and the SlateOS
native toolchain (would need a marker under A/D).

---

## Q10 — Which video codec backs the fullscreen capture fallback?

**Status:** OPEN

**Question.** The remote-desktop "video-encoded capture fallback for fullscreen
games/video" (roadmap §4.5) handles surfaces the draw-command stream cannot:
DMA-BUF / buffer-backed windows (games, video players) have raw pixels, not
vector `RenderCommand`s, so they currently stream as empty command lists. To
forward them efficiently at high resolution/refresh you need real *inter-frame*
video compression — an intra-only codec (QOI/PNG/RLE per frame) cannot meet the
4K/high-fps bandwidth budget, so picking the codec is a genuine, costly-to-
reverse decision (it dictates a library port and/or a hardware-encoder driver).
This gates the §4.5 video fallback and the related §4.x compositor GPU/video
items, but **not** all forward progress.

**Options.**

- **A — H.264 via a software encoder port (x264).** *Pro:* universally
  supported by every RDP/VNC/browser client; mature; best quality-per-bitrate at
  this complexity. *Con:* patent/licensing encumbrance (MPEG-LA pool) —
  problematic for a from-scratch OS that elsewhere avoids encumbered formats;
  x264 is GPL (licensing friction for bundling).
- **B — VP9 (libvpx) or AV1 (rav1e/SVT-AV1), royalty-free.** *Pro:* no patent
  royalties; AV1 has rav1e (Rust-native, fits the codebase). *Con:* software
  AV1/VP9 encode is CPU-heavy (rav1e realtime is marginal at 4K); fewer
  lightweight clients decode AV1; bigger port.
- **C — Hardware-encoded only (VAAPI/NVENC-equivalent via the GPU driver).**
  *Pro:* the only realistic path to 4K/high-fps without burning the CPU; matches
  how real remote-desktop/streaming stacks work. *Con:* hard-blocked on a GPU
  driver with an encode engine (AMDGPU/i915 port, roadmap §4.x) — not available
  yet, so nothing ships near-term.
- **D — Defer the whole fallback until a GPU encode path exists.** *Pro:* avoids
  committing to a software-codec port that hardware encode would later obsolete.
  *Con:* leaves fullscreen game/video remoting unsupported indefinitely.

**Claude's recommendation.** Lean **C long-term, D near-term**: the proper home
for this is hardware encode via the GPU driver, so defer the heavy codec port
until the AMDGPU/i915 encode engine is up rather than sink time into a software
encoder that hardware will supersede. If a software fallback is wanted *before*
GPU encode lands, prefer **B/AV1 (rav1e)** for the royalty-free + Rust-native
fit, accepting it's CPU-bound and realtime-marginal at 4K. **In the meantime**
Claude is not building a stub encoder (that would be a band-aid); the
draw-command stream already covers the flat-shaded-desktop case, and Claude is
picking unblocked work elsewhere.

**Where it bites.** New code would live in `gui/compositor` (fullscreen pixel
capture from the scanout/DMA-BUF path + frame pacing + an `Encoder` trait) and a
new encoder crate; the IPC would extend `CompositorRequest`/`CompositorResponse`
alongside the existing `StreamStart`/`StreamCapture`/`StreamStop`. The capture
substrate is codec-agnostic, but the encoder backend choice (and whether to
build a software one at all now) is what's blocked here.

---

No further open questions remain beyond Q10. All earlier deferred operator
decisions (Q1–Q8) have been resolved — see the "Recently resolved" list below
and `design-decisions.md` for full rationale. New decisions that genuinely need
the operator should be appended above this line as `## Q11 …`.

---

Recently resolved (see `design-decisions.md` for the full rationale):

- The coreutils "which set is canonical?" question — resolved 2026-06-12;
  standalone per-tool crates are canonical (§8).
- Q1 `set_mempolicy_home_node` / NUMA mempolicy on UMA — resolved 2026-06-13,
  **operator-confirmed 2026-06-14**; keep the UMA no-op returning 0, option A
  (§10).
- Q2 `/proc/sys/vm/overcommit_memory` & memory-commit policy — resolved
  2026-06-13, **operator-confirmed 2026-06-14** (keep the shipped defaults:
  native strict/committed, Linux lazy/overcommit; both configurable); build the
  both-strategies model (Option 5); map the system-wide overcommit knob to a
  fine-grained native cap (`admin.memory_policy`), not `CAP_SYS_ADMIN` (§11).
- Q3 next major initiative — resolved 2026-06-13; terminal/dev before GUI,
  GCC/CMake/Make toolchain first, CPython then fastpy (§9).
- Q4 toolchain on Slate OS: run-prebuilt-Linux vs native-port — resolved
  2026-06-13; **Path Z** (run prebuilt Linux toolchain binaries on the Linux-ABI
  layer now, native-port selectively later), native-first/no-leak kept
  inviolate, clang green-lit for install (§12).
- Q5 file-backed `mmap` — how far to take the fix — resolved 2026-06-14
  (§22), then **REOPENED 2026-06-14** by the operator, then **RE-RESOLVED
  2026-06-14**: adopt **C-lite** (a unified *read-only* page cache for
  shared-library text dedup + de-double-caching), deferred until a concrete
  consumer appears (the dynamic linker is the likely first; stable VFS
  file-identity is the precursor); writable `MAP_SHARED` writeback stays declined
  / `ENOSYS` (§23). Deferral trigger logged in `todo.txt`.
- Q6 cross-process memory introspection — resolved 2026-06-14: keep
  channel/shared-memory IPC for *consensual* sharing; add a
  **debug-capability-gated** cross-address-space `process_vm_readv`/`writev`
  (`Rights::DEBUG` on a `Process` capability; `EPERM` without it). `ptrace`
  remains a deferred follow-up behind the same gate (§24).
- Q8 Path Z libc + rootfs — resolved 2026-06-14, **operator-delegated to
  Claude**: go straight to **glibc** on an **ext4** rootfs, no musl
  stepping-stone (§25). Claude reversed its own earlier musl-first recommendation
  per the operator's stated preference for hard-work-upfront over throwaway
  scaffolding, given the static-load path is already proven end-to-end.
- Q7 kernel-task-stack-vs-IRQ overflow (B-DF1) — resolved 2026-06-15,
  **operator-chosen option A** (Claude recommended A): per-CPU guard-page IRQ
  stack with a manual nesting-aware switch + deferred preemption, plus the
  `cli`/`sti` recursion guard the restructuring exposed (§26). Validated:
  `http_gzip_8KiB` no longer double-faults at the gzip→dashboard transition.

---
