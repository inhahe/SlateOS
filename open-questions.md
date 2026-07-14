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

## Q23 — Session model for daemon-backed AF_INET **server** sockets (accepted-connection independence)

**Status:** OPEN (logged 2026-07-14). Not blocking: the daemon+ring listen/accept
layer is done and boot-validated (see `net-userspace-migration.md`, "Listen/accept
server sockets over the daemon"); this question only gates the final AF_INET
**socket-fd** wiring (`sys_bind`/`sys_listen`/`sys_accept4` +
`net::socket::SockState::Listening`). While it is open, Claude is working the other
remaining pre-5.7 gap — **IPv6 connect** in the daemon — which has no such fork.

**Background.** In the daemon, a session == one SHM ring (one `RingConns` table +
its listeners). `OP_ACCEPT` installs the newly-established connection into the
**listener's own session**, under a new conn_id on the *same* ring. So a listening
socket and every connection it accepts physically share one ring. Linux, by
contrast, gives every accepted fd a fully independent socket whose lifetime is
decoupled from the listener's.

**Question.** How should the socket-fd layer model accepted connections so their
lifetime/independence matches Linux, given the daemon co-locates them with the
listener?

**Options.**

- **A — Shared, refcounted session (no daemon-ABI change).** The listening
  `SocketInner` owns the session; each accepted socket is a new fd that holds an
  `Arc` on the same session and carries its own conn_id. Per-connection `close`
  sends `OP_CLOSE` for that conn_id; the session's `OP_STOP` fires only when the
  last reference (listener or any accepted socket) drops — so closing the listener
  no longer kills already-accepted connections (Linux-correct lifetime).
  - *Pros:* no daemon protocol change; reuses everything already built; smallest
    diff; matches the migration doc's "interim synchronous model, to be replaced by
    the async socket server" framing.
  - *Cons:* all connections under one listener funnel through **one ring guarded by
    one lock** — a *blocking* op on one accepted conn stalls every other conn on the
    same listener until its deadline. (Mitigated in practice: servers that use
    `accept`+`poll`+non-blocking I/O only serialize per round-trip, not per slow
    client. It is real for naively-blocking multi-client servers.)

- **B — Accept-into-a-fresh-ring (daemon-ABI change).** Extend accept so the kernel
  hands the daemon a *new* ring handle and the daemon migrates the established
  `TcpConn` out of the listener's session into a new single-connection session on
  that ring. Each accepted socket then owns its own ring exactly like a client
  socket.
  - *Pros:* true per-connection independence and concurrency (one slow client can't
    stall others); accepted sockets are structurally identical to client sockets.
  - *Cons:* new/extended accept ABI (SQE carries a ring handle; daemon must
    `OP_RING_TCP`-attach it and move connection state between session tables); more
    moving parts and a costlier-to-reverse protocol commitment.

**Claude's recommendation:** **Option A** for the interim. The whole per-op
synchronous socket path is explicitly a stepping stone to the async, always-on
socket server (see `known-issues.md` D-NETSOCK-SYNC and the migration doc), which
will replace the ring-per-op model wholesale — so paying for B's ABI complexity now,
only to rework it at the async cutover, is poor value. A fixes the Linux *lifetime*
semantics (the correctness-critical part) with zero protocol change; the
concurrency limitation is real but documented and temporary, and is a non-issue for
the poll-driven server pattern. If the operator wants genuine per-connection
concurrency before the async server lands, choose B.

**Where it bites:** `kernel/src/net/socket.rs` (`SockState`, `SocketInner`,
`SOCKET_TABLE`; a shared `Arc<Mutex<Session>>` for A vs. a per-socket ring for B),
`kernel/src/net/netstack_client.rs` (a `Session` abstraction hosting multiple
conn_ids vs. the current single-conn `NetstackConn`), `kernel/src/syscall/linux.rs`
(`sys_bind`/`sys_listen`/`sys_accept4` routing), and — for B only —
`services/netstack/src/main.rs` (accept-into-new-ring) + `netipc/src/ring.rs`
(accept SQE ring-handle field).

---

Earlier deferred operator decisions (Q1–Q22) have been
resolved — see the "Recently resolved" list below and `design-decisions.md` for
full rationale. New decisions should be appended above this line as `## Q24 …`.

---

Recently resolved (see `design-decisions.md` for the full rationale):

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
