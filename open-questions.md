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

## Q31 — Where does the SlateOS *native* ABI set up main-thread ELF TLS (thread pointer / `%fs` base) for compiler-emitted `__thread`?

**Context / how this surfaced.** Initiative F's toolchain is complete: fastpy now
cross-compiles a real program to a ~2.9 MB SlateOS `ET_EXEC` ELF that links
cleanly against the sysroot `libc.a` (posix crate) with zero undefined symbols
(`design.md` initiative-F checklist; `tests/test_cross_target.py`). The next
milestone is booting a fastpy-built binary under the kernel ("first real
component"). Inspecting the produced ELF shows a **`PT_TLS` segment**
(filesz 0x10, memsz 0x1b1a0): fastpy's C runtime uses compiler thread-locals
(`runtime/threading.h`: `#define FPY_THREAD_LOCAL __thread`). The compiler lowers
`__thread` to `%fs:offset` accesses (x86-64 TLS variant II), which **require an
`%fs` base (thread pointer) to be established** before any such access — there is
no alternative lowering.

But on SlateOS today, a *native* static binary gets **no thread pointer**:
- The kernel deliberately **resets `fs_base` to 0 on exec** and expects userspace
  to set it up (`kernel/src/proc/spawn.rs` ~L1189-1198, "execve replaces the
  address space … userspace sets it up") — the Linux/libc model.
- Linux-ABI binaries do set it up: their crt/ld.so reads the **aux vector**
  (`AT_PHDR`/`AT_PHNUM`) off the SysV stack the kernel builds and calls
  `arch_prctl(ARCH_SET_FS)` (wired in `kernel/src/syscall/linux.rs`).
- **Native binaries have no aux vector** (the posix crt fetches argv/fds via
  dedicated native syscalls `SYS_PROCESS_GET_ARGS`/`_GET_INITIAL_FDS`, not off the
  stack), and the posix crt's `__libc_start_main` (`posix/src/crt.rs`) does
  **not** set up TLS at all. So `fs_base` stays 0 and the first `__thread` access
  in a fastpy binary faults.

Note this is **additive**, not a reversal of the posix crate's deliberate
"tid-keyed pthread TSD instead of FS/GS TLS" choice (`posix/src/pthread.rs`
module doc): that choice is about *library* TSD (`pthread_setspecific`), a
different mechanism from compiler `__thread`. Supporting compiler `__thread`
(mandatory to run essentially any real C/C++ on SlateOS — fastpy today, ported C
libraries later) requires an `%fs` base regardless.

**Options.**

- **Option A — posix crt sets up main-thread TLS (userspace).** In
  `__libc_start_main`, find the program's `PT_TLS` via the linker-defined
  `__ehdr_start` symbol (available in static non-PIE links — no aux vector
  needed), allocate a variant-II TLS block + TCB self-pointer, copy the init
  image, and set `fs_base`. Setting `fs_base` needs either the existing Linux
  `arch_prctl` number routed into the native dispatch, or a small **new native
  syscall** (`SYS_SET_FS_BASE`, calling the kernel's existing
  `set_current_task_fs_base`).
  - *Pros:* keeps TLS setup in userspace, matching the kernel's existing "reset
    to 0, userspace sets it up" design; kernel loader stays minimal; consistent
    with the native crt already doing its own startup (args/fds/environ/signals/
    constructors). Child-thread TLS (`pthread_create`) naturally lives in the
    same userspace layer later.
  - *Cons:* implements variant-II TLS layout in the posix crate; needs a native
    `fs_base` setter exposed; more moving parts than C.

- **Option C — kernel ELF loader sets up main-thread TLS.** During spawn, parse
  `PT_TLS`, allocate a TLS block in the new address space, lay out the TCB, and
  set the initial thread's `fs_base` directly (instead of leaving it 0).
  - *Pros:* fewest moving parts, fully self-contained in the loader (already
    PHDR-aware and already builds the initial stack), no new syscall, no posix
    change — unblocks immediately.
  - *Cons:* puts x86-64 TLS-ABI layout knowledge in the microkernel loader
    (arguably a libc concern) and slightly contradicts the current "userspace
    sets `fs_base`" comment; child-thread TLS still needs a userspace path later,
    so responsibility ends up split across kernel + userspace anyway.

- **Option B — fastpy drops `__thread` in SlateOS pure mode** (`FPY_THREAD_LOCAL`
  → empty). *Pros:* trivial, fastpy-side only, no OS change. *Cons:* a
  correctness trap — multithreaded fastpy-on-SlateOS programs would silently
  share "thread-local" state (data races); doesn't solve the general problem
  (other C `__thread` code still can't run). Recommended **against**.

**Claude's recommendation.** **Option A** (userspace/posix-crt via `__ehdr_start`
+ a native `fs_base` setter). It matches the kernel's existing design intent,
keeps the microkernel loader minimal, and generalizes cleanly to child-thread TLS
in the same layer. **Option C** is a defensible simpler alternative if you'd
rather keep all process-image setup in the loader. Either A or C is correct;
B is not. In the meantime the toolchain deliverable (Q30) is fully done and
committed; only on-target *execution* is gated on this. Not proceeding
autonomously because it's a foundational, costly-to-reverse native-ABI choice.

**Where it bites.** `posix/src/crt.rs` (`__libc_start_main`), `posix/src/syscall.rs`
(a possible `SYS_SET_FS_BASE`), `kernel/src/proc/spawn.rs` (fs_base reset ~L1189;
or the TLS-setup site for C), `kernel/src/proc/elf.rs` (`PT_TLS`, currently
`#[allow(dead_code)]`), `kernel/src/sched/mod.rs` (`set_current_task_fs_base`),
and fastpy `runtime/threading.h` (`FPY_THREAD_LOCAL`). After it's set up, rebuild
the sysroot `libc.a`, rebuild the fastpy binary, embed + spawn it (mirror
`services/hello`), and boot-test.

**Status:** OPEN.

---

Earlier deferred operator decisions (Q1–Q30) have been
resolved — see the "Recently resolved" list below and `design-decisions.md` for
full rationale. New decisions should be appended above this line as `## Q31 …`.

---

Recently resolved (see `design-decisions.md` for the full rationale):

- Q30 C cross-toolchain for fastpy's SlateOS runtime (initiative F) — resolved
  2026-07-21 (§81): **option A (a clang cross-toolchain to musl), realized via
  `zig cc --target=x86_64-linux-musl`** — a self-contained, portable clang +
  bundled musl headers + musl libc, so no heavyweight system-wide LLVM install
  and no separately vendored musl headers were needed (sidesteps both cons of
  A). Operator said "do A"; Claude picked zig as the concrete mechanism. The
  pure-mode runtime now cross-compiles and a real fastpy program links to a
  ~2.9 MB SlateOS ET_EXEC ELF with zero undefined symbols.

- Q29 fastpy → SlateOS target strategy (initiative F) — resolved 2026-07-21
  (§80): **pure-mode native compile first (A); add the CPython bridge later as a
  superset (B)** — "A at first but eventually B." Unblocks *starting* initiative
  F. Sequencing: mature the POSIX layer → add the `x86_64-slateos` fastpy target
  + port the C runtime in pure mode → compile one real OS component. Claude
  recommended A-first-then-B; operator confirmed.

- Q28 `osh` `$EUID`/`$UID` identity — resolved 2026-07-21 (§79): **default root
  (`0`/`0`) [option A], made per-user configurable** via `OSH_UID`/`OSH_EUID`.
  Seeded as real readonly-integer vars (readonly-enforced, bash-faithful
  listings). Claude recommended A; operator accepted and added the
  default-plus-per-user-override framing. Implemented; known-issues
  TD-OILS-IDVARS updated.

- Q27 `osh` advertising as bash (`$BASH_VERSION`/`$BASH_VERSINFO`) — resolved
  2026-07-21 (§78): **option A (advertise), as a per-user toggle
  (`OSH_BASH_COMPAT`) defaulting on** — mirrors upstream Oils' own `bash_compat`
  flag (which defaults on for `osh`, off for `ysh`; upstream sets
  `BASH_VERSION='5.3'`). osh keeps its level at 5.2 (never claims a 5.3-only
  feature). Claude recommended A + proposed the toggle; operator chose A and
  asked for the per-user-default framing.

- Q26 Oils (OSH) port strategy confirmed — resolved 2026-07-21 (§77): **finish
  the Rust reimplementation (A) now; keep A as a permanent user option even if a
  faithful C++ `oils-for-unix` port (B) lands later.** Claude recommended
  finishing A; operator confirmed and added that B is an additive future option,
  not a replacement.

- Q25 next large initiative + fixed ordering — resolved 2026-07-18 (§69):
  **Option A** (the interactive-shell userland) first, with the explicit
  clarification that the shell is **Oils (OSH)** — a bash-*superset* shell —
  **not bash itself** (roadmap-detailed.md §2.7). Fixed initiative order recorded
  durably so it need not be re-asked: **A → F → B → C → D → E** (1. Oils/OSH +
  coreutils, 2. fastpy build-system integration, 3. Mesa/GPU userspace [gated by
  Q18/virgl], 4. Chromium, 5. WINE, 6. additional filesystems). Claude recommended
  A-then-F; operator set the full ordering.

- Q24 raw `spin::Mutex` holder-preemption — reactive vs. proactive audit —
  resolved 2026-07-18 (§70): **Option B** (proactive kernel-wide audit/conversion)
  — "no technical debt, do it the right way." Not a blind sed: the heap and other
  deliberately-raw locks stay raw + manual-preempt; hot leaf locks move to a
  preempt-aware `PreemptSpinMutex`; contended non-leaf locks move to
  `crate::sync::Mutex` (lockdep); conversion is incremental and validated with
  `wedge-soak.sh` green. Claude recommended A (reactive) with C as escalation;
  operator overruled and chose the full proactive sweep.

- Q23 session model for daemon-backed AF_INET **server** sockets — resolved
  2026-07-18 (§71): **Option A** (shared, refcounted session; no daemon-ABI
  change) for the interim, since the whole per-op synchronous socket path is a
  stepping stone to the async socket server that will replace the ring-per-op
  model wholesale. Standing operator guideline recorded: **do not gold-plate
  interim/throwaway netstack infrastructure** — server sockets get A only; the
  concurrency limitation is documented and temporary. Claude recommended A;
  operator confirmed A.

- Q22 netstack Phase 5 cutover — deletion scope + cutover strategy — resolved
  2026-07-14 (§66): **Q22a → Option C** (phased deletion — L2–L4 core first, app
  protocols re-homed to userspace individually) and **Q22b → (ii) staged**
  (persistent daemon + socket-forwarding behind a default-off boot switch; prove
  parity in QEMU, flip the default, then delete). Claude recommended both; operator
  approved both.

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
- Q9 bare-ELF ABI auto-classification — resolved 2026-06-24, **operator-chosen
  option D** (Claude recommended D): default unmarked bare ELF → Linux ABI, add
  `NT_GNU_ABI_TAG` note-walk as a positive Linux signal, stamp native binaries
  with an explicit SlateOS marker; `spawn_process_with_abi` override kept (§33).
- Q10 fullscreen-capture video codec — resolved 2026-06-24, **operator deferred
  to Claude's recommendation**: hardware encode via the GPU driver long-term
  (option C), defer the software-codec port near-term (option D), no stub
  encoder meanwhile; if a software path is ever needed first, AV1/`rav1e` over
  H.264 (§34).
- Q11 zero-copy page-flipping for large channel messages — resolved 2026-06-24,
  **operator-chosen option B** (Claude recommended B): explicit opt-in
  `MSG_ZEROCOPY`-style flag + caller-provided page-aligned landing region; copy
  path stays the default. Compiler follow-up: keep it programmer/library-
  controlled (library-level auto-threshold helper), the compiler does not
  auto-insert the flag (§35).
- Q12 next large initiative — resolved 2026-06-24, **operator-chosen option E**:
  build the C-lite read-only page cache now; lifts the §23 "not now" hold (§36).
- Q13 de-double-cache file data — resolved 2026-06-30, **operator-chosen option A**
  (Claude recommended A): page-cache-primary — the page cache is the single cache
  for regular-file data, the buffer cache caches only filesystem metadata (§38).
- Q14 connect the two cgroup subsystems — resolved 2026-06-30, **operator-chosen
  option A** (Claude recommended A): cgroupfs as the frontend,
  `kernel/src/cgroup.rs` as the enforcement engine; fork/clone/spawn inherit
  `cgroup_id` (§39).
- Q15 next focus — resolved 2026-06-30, **operator-chosen option A then C/D**:
  execute Q13 + Q14 first, then a large initiative — C (GPU accel) or D (Docker /
  container-runtime port) in operator-indifferent order; this is the explicit
  go-ahead for the Docker port (§40).
- Q16 `container diff` baseline semantics — resolved 2026-07-01, **Claude
  autonomous (operator-approved Docker-port scope)**: implemented **option A**
  (overlay-only diff). See `design-decisions.md` §41.
- Q17 `container exec` semantics — resolved 2026-07-14, **operator-chosen
  option B** (Claude recommended B): keep the netns-debug `container exec` facade
  AND add real rootfs-binary exec under a distinct verb (`container run-in` /
  `exec --rootfs`); the `docker exec` delegate + `docker build` `RUN`/`HEALTHCHECK`
  route to the real path (§58).
- Q18 GPU acceleration scope — resolved 2026-07-14, **operator-chosen option B**
  (Claude recommended C): build the kernel-side virtio-gpu render-ioctl dispatch
  now with honest "no-3D" reporting (GETPARAM `3D_FEATURES=0`, no capsets, correct
  errno on 3D ioctls); defer the Mesa port until a virgl test environment exists
  (§59).
- Q19 container network model — resolved 2026-07-14, **operator-chosen option B**
  (Claude recommended B): generalise to N-interface multi-network membership
  (Docker parity) as its own dedicated increment (§60).
- Q20 hard-lockup (BSP-dead) detector — resolved 2026-07-14, **operator-chosen
  option A** (Claude recommended A): build the `i6300esb` watchdog + inject-nmi
  detector, opt-in behind the existing `boot-test.sh --hard-lockup-watchdog` flag
  (§61).
- Q21 `nft`/`iptables` compat tooling — resolved 2026-07-14, **operator-chosen
  option C** (Claude recommended C): keep `nft`/`iptables` as an explicit
  parser/pretty-printer only, fix the docs, steer users to `fw`; defer full/minimal
  kernel wiring (§62).
