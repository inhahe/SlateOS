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

## Q11 — Zero-copy page-flipping for large channel messages: what ABI does the receiver see?

**Status:** OPEN

**Question.** Channel IPC today copies the message payload (sender userspace
buffer → kernel `Message.data: Vec<u8>` → receiver userspace buffer). For large
messages this is the dominant cost — a freshly-measured kernel-internal baseline
puts a 64 KiB round-trip at **~343 µs min** under QEMU-TCG, virtually all of it
in the `Message::from_bytes` allocation+copy (`bench_ipc_channel_large`,
`bench/baselines.toml [ipc_channel_roundtrip_64k]`). The design spec calls for
**zero-copy page flipping** for large messages (move the sender's pages into the
receiver's address space instead of copying). Implementing that requires deciding
*what the receiver observes*, which is a user-visible ABI/policy fork — not an
internal optimization that can be made transparently, because page granularity
(16 KiB) and address placement are visible to userspace.

**Options.**
- **A — Transparent, kernel-chosen mapping.** On `recv`, the kernel maps the
  flipped pages somewhere in the receiver's address space and hands back a
  pointer+len. *Pro:* simplest caller model; no pre-arranged buffer. *Con:* the
  receive buffer address is now kernel-chosen (callers can't `recv` into a fixed
  buffer); the receiver must later unmap; payloads round up to 16 KiB pages so
  sub-page tails need a length field; sender loses the pages (move semantics)
  which changes `send` ownership rules.
- **B — Opt-in flag + caller-provided page-aligned region.** A `MSG_ZEROCOPY`-style
  send flag; receiver pre-registers a page-aligned landing region. *Pro:* explicit,
  predictable, matches `io_uring`/`vmsplice` mental model; copy path stays the
  default so nothing existing changes. *Con:* more API surface; only helps callers
  that adopt it.
- **C — Threshold-automatic: copy below N, flip at/above N, always transparent.**
  *Pro:* best-case perf with no caller change. *Con:* combines A's ownership/teardown
  complexity with a silent behavior change at the threshold (a `send` that used to
  copy now moves the sender's pages); surprising and hard to reason about.
- **D — Defer; keep copy-only.** *Pro:* the copy path is correct and the 64 KiB cap
  bounds the worst case; no new ABI to commit to. *Con:* leaves the spec's zero-copy
  goal unmet; large-message IPC stays copy-bound.

**Claude's recommendation.** Lean **B** (explicit opt-in + caller-provided aligned
region): it keeps the existing copy path as the zero-risk default, matches the
zero-copy idioms callers already know, and avoids silently changing `send`
ownership semantics. **In the meantime** Claude has built only the benchmark
groundwork (the baseline above) — *not* a stub flip path (that would be a
band-aid) — and is picking unblocked work elsewhere. This is logged now because
the implementation can't proceed without the ABI choice, and the choice is
user-visible enough to want operator input.

**Where it bites.** `kernel/src/ipc/channel.rs` (`Message`, `send`/`recv`,
`MAX_MESSAGE_SIZE`, the module's "Future Optimizations" note), the MM page-transfer
mechanism (move/remap pages between address spaces — new in `kernel/src/mm`), and
the Linux/native syscall glue that marshals channel messages. The benchmark to
measure any implementation against already exists (`kernel/src/bench.rs::bench_ipc_channel_large`).

## Q12 — Which large initiative comes next? (bounded in-context work is exhausted)

**Status:** OPEN

**Question.** As of 2026-06-20 a full sweep of the bounded, in-deep-context work
queues (fs-admin / TD18, the Linux event-fd family, the POSIX libc layer, and the
entire `todo.txt` deferred/follow-up changelog) confirms that **every concrete,
clearly-unblocked, non-boot-risky increment is already implemented** — repeatedly,
the "owed follow-up" notes turned out to describe already-shipped code (file-backed
mmap, the per-PCB Linux fd table, fork/exec/wait, the ChaCha20 CSPRNG behind
`getrandom`, the SA_RESTART sentinel taxonomy, and the canonical line-buffered
console `read` are all done; the stale notes have been corrected). What remains is
**all large-grained**, and each remaining vein is gated on something only the
operator can resolve — a prioritization call, an architectural fork, or an unmet
prerequisite. The decision needed: **which large initiative should the next block
of autonomous work target?**

**Options (the remaining large veins, with what gates each).**

- **A — TCP/IP stack → userspace service migration** (roadmap §"Move to userspace
  service", ~line 1125). *Gate:* this is an **architectural fork** (keep the stack
  in-kernel vs. re-home it behind a channel-IPC network service) that is costly to
  reverse and changes a user-visible service boundary — reserved for operator
  input per the standing rule.
- **B — Compositor GPU acceleration / video-capture fallback** (roadmap §4.x,
  §4.5). *Gate:* hard-blocked on **prerequisites** — a real GPU driver
  (AMDGPU/i915 port) with a 3D/encode engine — and on the **Q10** codec fork
  already open. Software-only stopgaps were judged band-aids.
- **C — POSIX libc-layer expansion.** *Gate:* **no concrete gap found.** The layer
  is mature and the project already runs *real glibc* on the Linux-ABI path (Path
  Z), so broad libc work risks low-value churn without a specific missing-function
  driver. Would need the operator to name a target workload that exposes a gap.
- **D — A large external port** (bash, Mesa/Vulkan, AMDGPU/i915, CPython, the Rust
  toolchain, Btrfs/F2FS/NTFS, Chromium, WINE — all roadmap `[ ]`). *Gate:*
  **operator prioritization + prerequisites.** The standing rule asks for operator
  go-ahead before a *giant* external port (it's a prioritization/sequencing call,
  not an effort-cost one), and several depend on infrastructure not yet built.
- **E — Build the operator-pre-approved C-lite read-only page cache now**
  (design-decisions §23 / Q5). *Gate:* the operator explicitly said "implement
  **LATER, not now**", with a trigger (first real cross-process `.so` `.text`
  dedup consumer). Its precursor — stable VFS file identity — *is* now met
  (`FileMeta.ino` is populated for ext4/FAT/memfs), so only the operator's
  "not now" and the not-yet-firing consumer trigger hold it back. Starting it
  would also touch the boot-critical fault/mmap path unsupervised.

**Claude's recommendation.** No autonomous default is clearly correct here: A and
the §4.x items are operator-reserved forks/prerequisite-blocked, C risks low-value
churn without a named gap, D needs the operator's prioritization, and E is
explicitly operator-deferred. The most useful thing the operator can supply is a
**direction**: either (i) pick which large vein to open (A/B/C/D), (ii) lift the
"not now" on E if the dynamic-linker dedup payoff is now wanted, or (iii) name a
concrete workload/target (a specific program to run, a specific port to attempt)
that turns one of these into a bounded, gap-driven task. **In the meantime**, per
the state-(3) rule and the "stop scheduling idle heartbeats" rule, Claude is
ending the autonomous loop rather than firing further no-op ticks; it will resume
when the operator answers.

**Where it bites.** Project-wide / prioritization — not a single file. The
specific technical forks remain Q9 (ELF ABI default), Q10 (capture codec), Q11
(zero-copy channel ABI); E's home is `kernel/src/mm` + the `mmap`/fault path.

---

No further open questions remain beyond Q12. All earlier deferred operator
decisions (Q1–Q8) have been resolved — see the "Recently resolved" list below
and `design-decisions.md` for full rationale. New decisions that genuinely need
the operator should be appended above this line as `## Q13 …`.

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
