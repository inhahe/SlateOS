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

## Q26 — Oils (OSH) port strategy: Rust reimplementation now vs. faithful C++ `oils-for-unix` cross-compile later

**Status:** OPEN (proceeding on the prerequisite-forced default; heads-up only).

**Question.** You committed (§69/Q25→A) to porting **Oils (OSH)** — a bash-superset
shell — as the first large initiative. *How* should it be ported?

**Options.**
- **(A) Rust reimplementation in-tree (`userspace/oils`)** — a real Rust shell
  implementing the OSH language that forks/execs on SlateOS, matching the
  coreutils reimplementation pattern.
  - *Pro:* only path that builds/runs **today**; consistent with existing
    userspace; honest use of the Q24 fork/exec de-risking. *Con:* not bit-for-bit
    upstream OSH; reimplementation effort + fidelity risk on obscure bash corners.
- **(B) Faithful C++ `oils-for-unix` cross-compile** — build the real upstream.
  - *Pro:* exact OSH semantics, no reimplementation. *Con:* **blocked** — no
    C/C++→`x86_64-slateos` toolchain or libc/CRT exists yet; standing that up is a
    separate massive initiative. Delivers nothing runnable in the near term.

**Claude's recommendation / what I'm doing meanwhile.** Proceeding with **(A)** —
it is the only prerequisite-satisfiable path and the reversal cost is local (the
crate is an isolated userspace binary; if a C++/slateos toolchain later lands via
the Mesa/Chromium/WINE work, swap in the real `oils-for-unix`). Full rationale in
`design-decisions.md §72`. If you'd rather gate this behind first building the
C++ toolchain, say so and I'll re-order.

**Where it bites.** `userspace/oils/` (new crate); roadmap.md:1494; the existing
minimal `userspace/coreutils/src/bin/sh.rs` stays a small POSIX baseline.

---

## Q27 — Should `osh` advertise itself as bash via `$BASH_VERSION` / `$BASH_VERSINFO`?

**Status:** OPEN (heads-up only; not currently set).

**Question.** Many real-world scripts feature-detect bash by testing
`[ -n "$BASH_VERSION" ]` or gate on `${BASH_VERSINFO[0]} -ge N`. `osh` is an
in-tree Rust reimplementation of the OSH *language* (a bash superset), not bash
itself, and today sets **neither** variable. Should it set them — and if so, to
what value?

**Options.**
- **(A) Set both, reporting a bash-compatible version** (e.g. `BASH_VERSION="5.2.0(1)-release"`,
  `BASH_VERSINFO=(5 2 0 1 release x86_64-slateos)`).
  - *Pro:* the widest set of "is this bash?" scripts take their bash code path and
    run — the whole point of a bash-superset shell. *Con:* it's a lie: a script may
    then assume a specific bash's behavior/feature that `osh` implements slightly
    differently or not at all, producing a subtler failure than an honest "not bash".
- **(B) Set them to an honest `osh`-branded value** (e.g. `BASH_VERSION` unset but a
  new `OSH_VERSION`/`OILS_VERSION` set, mirroring upstream Oils).
  - *Pro:* truthful; scripts that specifically want Oils can detect it. *Con:* the
    common `[ -n "$BASH_VERSION" ]` guard fails, so bash-only script branches are
    skipped even though `osh` could run them.
- **(C) Leave both unset (status quo).**
  - *Pro:* zero risk of masquerade-induced misbehavior. *Con:* bash-detecting
    scripts silently fall back to a POSIX-sh code path (or bail), losing bash
    features `osh` actually supports.

**Claude's recommendation / meanwhile.** Leaning **(A)** — the crate's stated goal
is to be a drop-in bash-superset replacement, and the dominant real-world use of
`$BASH_VERSION` is exactly the "run the bash branch" gate we *want* to satisfy;
upstream Oils itself sets `$BASH_VERSION` for this reason. But it's a user-visible
compatibility *policy* (deliberately claiming to be bash) with a real downside, so
I've deferred rather than decided. Meanwhile both stay unset.

**Where it bites.** `interp.rs` `param_value` (~4619) and shell init (`new()`
~454) — where read-only dynamic vars like `BASHPID`/`EPOCHSECONDS` are produced and
where seed vars would be inserted.

---

## Q28 — What user identity should `osh` report via `$EUID`/`$UID` (and `$HOSTNAME`)?

**Status:** OPEN (heads-up + genuinely blocks these vars; not currently set).

**Question.** bash always defines readonly `$EUID` and `$UID` (numeric effective
/real user id) and `$HOSTNAME`. Real scripts lean on them constantly — the
canonical root check `if [ "$EUID" -ne 0 ]; then echo "run as root"; fi`, and
many installers gate on `$UID`. `osh` currently leaves all three unset, so those
comparisons error on an empty operand. To set `$EUID`/`$UID` we must decide *what
identity `osh` reports* — and there is no SlateOS `getuid`-equivalent wired into
either the host or target build yet, so it can't just read a real credential.

**Options.**
- **(A) Report root (`EUID=UID=0`) as the default.**
  - *Pro:* matches the reality that early SlateOS bring-up / init runs as the
    system with full authority; root checks that *gate* privileged actions will
    (correctly, for now) see root. *Con:* a "are you root? then it's safe to nuke
    the system" script would proceed where a real multi-user system wouldn't; masks
    the absence of a privilege model.
- **(B) Report a fixed non-root user (e.g. `EUID=UID=1000`).**
  - *Pro:* the safer default — privileged-action scripts refuse rather than run
    with assumed root; models the eventual "shell runs in a user session" norm.
    *Con:* bring-up utilities that legitimately need root would wrongly bail.
- **(C) Leave unset until a real `getuid`/`geteuid` syscall exists (status quo),**
  then wire them dynamically to the actual process credential.
  - *Pro:* never lies about identity. *Con:* keeps breaking the very common
    `$EUID`/`$UID` script idioms in the meantime.

**Claude's recommendation / meanwhile.** Mild lean to **(A)** for the current
single-user, pre-privilege-model bring-up state (the shell genuinely *is* the
all-powerful system right now, and root-gated scripts should take their root
path), switching to a real credential read once SlateOS exposes `getuid`. But
this is a user-visible security-adjacent policy with a real footgun, so I've
deferred rather than decided; all three vars stay unset for now. `$HOSTNAME`'s
default (`localhost`/`slateos`) is a low-stakes naming choice that can ride along
with whatever's decided here.

**Where it bites.** `interp.rs` `param_value` (dynamic-var arm near
`BASHPID`/`BASH_SUBSHELL`) and `seed_shell_vars`; tracked in `known-issues.md`
TD-OILS-IDVARS.

---

Earlier deferred operator decisions (Q1–Q25) have been
resolved — see the "Recently resolved" list below and `design-decisions.md` for
full rationale. New decisions should be appended above this line as `## Q29 …`.

---

Recently resolved (see `design-decisions.md` for the full rationale):

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
