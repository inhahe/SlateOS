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

## Q17 real `container exec` semantics — replace the netns-debug facade? — OPEN

- **Question** — Docker's `docker exec <ctr> <cmd>` runs a **new program from
  the container's own rootfs** inside the running container's namespaces (PID,
  mount, net, user) and cgroup. Our current `container exec` (kshell ~68148) is a
  *different* thing: it switches into the container's **network namespace** and
  runs a **kshell builtin** there — a handy network-debugging facade, not a
  rootfs-binary launcher. Do we (a) replace the facade with real Docker-style
  exec, (b) keep the facade and add real exec under a new verb, or (c) keep the
  facade only?
- **Context / why it's an operator call** — This changes the behavior of an
  **existing, already-shipped command**. The netns-debug facade is genuinely
  useful (run `ip`/`ping`/socket builtins inside a container's network sandbox
  without a rootfs binary present) and real exec would *lose* that unless kept
  separately. It's user-visible, so it shouldn't be swapped unilaterally.
- **Options**
  - **A. Replace.** `container exec <id> <path> [args…]` spawns the rootfs binary
    in the container's namespaces+cgroup, reaping its exit code (reuse the proven
    `set_wait_task`→`block_current` join used by `container::wait`). *Pro:*
    matches Docker exactly; the single obvious meaning of "exec". *Con:* deletes
    the netns-debug facility; a foreground-blocking exec self-test risks tripping
    the documented flaky glibc-spawn/COW hang (B-PTHREAD-YIELDBUDGET family) in
    the boot test.
  - **B. Both, distinct verbs.** Keep `container exec` = netns-debug facade; add
    `container run-in <id> <path> [args…]` (or `exec --rootfs`) for the real
    rootfs-binary exec. *Pro:* no capability lost, Docker parity gained. *Con:*
    two verbs, and `exec` then diverges from Docker's meaning (confusing for
    Docker users; the `docker` delegate would have to map `exec`→`run-in`).
  - **C. Keep facade only.** *Pro:* zero risk, no new spawn/join surface. *Con:*
    no real exec — a visible gap vs Docker; `healthcheck`/`exec`-dependent
    features can't be built on it.
- **Claude's recommendation** — **B** short-term shading into **A** long-term:
  add real rootfs exec under an unambiguous verb now (no capability lost, testable
  in isolation), and once the glibc-spawn flakiness is root-caused, make the
  `docker exec` delegate route to the real path so Docker users get Docker
  semantics while `container exec` keeps the netns-debug meaning for our own
  tooling. In the meantime neither `exec` behavior changes, so nothing is blocked.
- **Where it bites** — `kernel/src/kshell.rs` (`container exec` arm ~68148 + a new
  arm / `docker` delegate map), `kernel/src/container.rs` (a new
  `exec(id, argv) -> KernelResult<i32>` that enters the container's ns+cgroup,
  spawns, and joins via `set_wait_task`), and the `healthcheck` feature that would
  consume it. See `known-issues.md` D-CONTAINER-EXEC-WAIT.
- **Only gates `docker build`'s `RUN`/`HEALTHCHECK` now (rest shipped).** The
  `docker build` capstone (Dockerfile → OCI image) is **built and shipping** for
  every instruction except `RUN` and `HEALTHCHECK`: `oci.rs` has an OCI image
  *writer* (`write_image` — uncompressed-tar layer blobs + sha256 digests +
  config/manifest/index/oci-layout JSON) and a full Dockerfile builder
  (`build_image`) wired to `oci build` / `docker build`. It supports
  FROM (`scratch` or a local OCI image dir, with base-layer + config
  inheritance), COPY/ADD (file + directory sources, with `.dockerignore`
  context filtering incl. `!` re-inclusion), ENV/CMD/ENTRYPOINT/WORKDIR/USER/
  EXPOSE/LABEL/VOLUME/STOPSIGNAL/SHELL/ONBUILD, ARG with
  `${VAR}`/`$VAR`/`${VAR:-default}` expansion + `--build-arg` overrides,
  **multi-stage builds** (`FROM … AS <name>`, `FROM <stage>` base inheritance,
  `COPY --from=<stage-name|index|image-dir>` cross-stage copies, and
  `--target <stage>` to output an intermediate stage), and
  OCI config `history[]` recording (surfaced via `oci`/`docker history`).
  `RUN <cmd>` — which executes a command inside the in-progress image's rootfs,
  *the same* rootfs-binary exec this question is about — and `HEALTHCHECK`
  (which references such a command) are the **only** unbuilt instructions:
  `build_image` rejects `RUN` with a precise `BuildError::RunUnsupported`
  diagnostic pointing at Q17. So resolving Q17 is now the sole remaining unlock
  for `RUN`/`HEALTHCHECK`.
- **Status** — `OPEN` (deferred; not blocking — the image writer + non-RUN
  builder are done; only `RUN` and the `container exec` semantics await the
  operator's fork choice).

---

## Q18 GPU acceleration — how far to invest, given the virgl/Mesa ceiling? — OPEN

- **Question** — Q15 gave the go-ahead for the GPU-acceleration initiative
  (roadmap §4582 "Vulkan loader and basic GPU command submission", §4583
  "OpenGL via Mesa"). The **foundation** is now built and tested (see "Where it
  bites"), but the **headline payoff — real 3D rendering — is gated on two
  prerequisites that are genuine operator calls**, not just effort:
  1. **A virgl-capable test environment.** Our headless CI (`-display none`,
     `virtio-gpu-pci`) is 2D-only: plain virtio-gpu offers **no
     `VIRTIO_GPU_F_VIRGL`** (observed device features `0x30000002` — EDID bit
     only). Real 3D needs `virtio-gpu-gl-pci` **plus** a host GL/display backend
     **plus** host `virglrenderer`. On the Windows dev box that means QEMU built
     with virgl (ANGLE/OpenGL) and a non-headless display — none of which the
     boot-test can currently provide. Without it, every 3D code path is
     **unfalsifiable** (buildable + self-testable only, never integration-tested
     against a GPU).
  2. **The Mesa port itself (§4583).** The only consumer of the virtio-gpu
     render ioctls is Mesa's virgl (OpenGL) / venus (Vulkan) drivers — a **large
     external C port** (Mesa + its Vulkan loader). Per CLAUDE.md, a giant
     external port needs operator go-ahead on prerequisites/prioritization
     before starting. Building the kernel-side render-ioctl dispatch *before*
     there is any client makes it speculative infrastructure with nothing to
     validate it.
- **Options**
  - **A. Invest in the virgl test env + commit to the Mesa port now.** *Pro:*
    unlocks genuine, testable GPU acceleration end-to-end. *Con:* requires
    provisioning a virgl-capable QEMU+display on the dev box (may be
    non-trivial/impossible headlessly), and commits to a large multi-part C
    port whose validation depends on (1).
  - **B. Build the kernel-side virtio-gpu render-ioctl dispatch now with honest
    "no-3D" reporting, defer Mesa.** GETPARAM reports `3D_FEATURES=0`,
    GET_CAPS returns no capsets, 3D-requiring ioctls return the correct errno;
    verified by a new ring-3 self-test that opens `renderD128` and issues the
    ioctls. *Pro:* real, testable ABI plumbing on real hardware; correct
    behaviour for any future client. *Con:* the reporting is necessarily
    "unsupported" until (1)+(2) land, so it delivers no *acceleration* — it just
    makes the render node answer virtio-gpu ioctls correctly.
  - **C. Treat the foundation as a good stopping point for GPU accel and pick up
    other roadmap work** until (1)/(2) are resolved. *Pro:* no speculative
    infrastructure; keeps delivering fully-testable features elsewhere. *Con:*
    GPU accel pauses short of any rendering.
- **Claude's recommendation** — **C now, with B available on request.** The
  foundation trilogy below is complete and tested; the next *acceleration* step
  genuinely needs a decision on (1) the test environment and (2) the Mesa-port
  commitment. In the meantime I'm continuing with other unblocked roadmap tasks
  (not idling), and can do **B** whenever you want the render-node ioctls
  answered correctly ahead of the Mesa port.
- **Where it bites / what's already done** —
  - `kernel/src/drm/virtgpu_uapi.rs` — pure `virtgpu_drm.h` uAPI ABI layer
    (structs + ioctl numbers + `param_value` policy + self-test). *Done.*
  - `scripts/boot-test.sh` — `-device virtio-gpu-pci` so the 2D device path is
    exercised (DRM device 1 = virtio-gpu, promoted to primary). *Done.*
  - `kernel/src/drm/mod.rs` (`primary_device`) + `kernel/src/syscall/linux.rs`
    (`try_open_drm`) — `/dev/dri/card0`+`renderD128` bound to the primary GPU.
    *Done.*
  - Option B would land in `kernel/src/syscall/linux.rs` `drm_card_ioctl` (a new
    `DRM_COMMAND_BASE`-range arm routing to `drm::virtgpu_uapi`), plus a ring-3
    `renderD128` ioctl self-test.
- **Status** — `OPEN` (deferred; **not blocking** — the foundation is done and
  other roadmap work continues; only the *acceleration* payoff awaits the fork).

---

All deferred operator decisions (Q1–Q15) have been resolved — see the
"Recently resolved" list below and `design-decisions.md` for full rationale. New
decisions should be appended above this line as `## Q17 …`.

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
  (overlay-only diff). `Container` now records its `OverlayId` at `oci run` time;
  `container::diff(id)` enumerates the overlay upper (Added/Changed via
  `which_layer`) + whiteouts (Deleted), sorted; plain bind-rootfs containers
  return `InvalidArgument` ("no overlay rootfs"). No band-aid, matches Docker.
  Where: `kernel/src/container.rs`, `kernel/src/fs/overlay.rs` (`upper_path`/
  `whiteouts`), `kernel/src/kshell.rs` (`container diff` arm). See
  `design-decisions.md` §41.

---
