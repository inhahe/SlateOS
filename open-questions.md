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

## Q22 — netstack userspace migration Phase 5: deletion scope + cutover strategy

**Status:** OPEN

**Context.** §63 (Path B) is settled: the TCP/IP protocol stack moves to the
userspace `netstack` daemon behind the thin capability-gated kernel NIC shim
(`kernel/src/net/raw.rs` + `SYS_NET_RAW_*`). Phases 1–4 are done — the daemon can
already claim the NIC, register `net.stack`, and serve real DNS/TCP/UDP ops over
the channel + zero-copy SHM ring (bounded self-test model, §64: the exclusive NIC
claim would starve the still-live in-kernel stack, so the daemon claims → serves →
releases → exits rather than running persistently). Phase 5 is the cutover: make
the daemon persistent, forward the POSIX/Linux socket syscalls to it, and delete
the kernel-resident stack. Two forks in Phase 5 have no obviously-correct answer
and are costly/irreversible (deleting a ~60 K-line subsystem), so I'm flagging
them rather than deciding unilaterally. **I am not blocked** — the daemon must
grow a persistent multi-connection socket server under *either* answer, so I'm
building that prerequisite now (see "Where it bites"); these questions only gate
the final deletion + wiring step.

**Q22a — deletion scope.** `kernel/src/net/` is ~60 K lines / ~48 modules. Only a
subset (`ethernet, arp, ipv4/ipv6, icmp/icmpv6, tcp, udp, dns, dhcp, frag,
interface, ndisc`) is the L2–L4 stack the daemon replaces. The rest are
*application-level* protocol servers/clients that happen to live in-kernel today:
`ssh` (2713 lines), `httpd`/`http`, `ftp`, `smtp`, `snmp`, `telnet`, `tftp`,
`ntp`, `dhcpd`, `syslog`, `socks`, `upnp`, `mdns`, `lldp`, `iperf`, `netcat`,
`traceroute`, `tls`, `websocket`, `nat`, `firewall`, `bridge`, `veth`, `vlan`,
`qos`, `igmp`, `mld`, `wol`, `pcap`, `dashboard`, `netstat`. The migration doc
says literally "remove `kernel/src/net/` protocol modules; keep only the thin NIC
shim" — taken at face value that deletes *all* of the above, silently dropping
every one of those features.

- **Option A — L2–L4 only.** Delete just the core stack the daemon replaces;
  keep the app-protocol modules compiling in-kernel for now, migrate/delete them
  in later dedicated tasks. *Pros:* no feature regression; smallest blast radius;
  each app protocol gets a real userspace re-home instead of vanishing. *Cons:*
  `kernel/src/net/` still large; those app modules still call the L2–L4 APIs
  being deleted, so they'd need to be rewired onto the daemon's socket API too —
  which means they can't actually stay as-is (they depend on in-kernel `tcp`/
  `udp`). This may not be cleanly separable.
- **Option B — delete everything, accept regression.** Follow the doc literally:
  delete all of `kernel/src/net/` except the NIC shim; the app protocols come
  back later as userspace daemons/tools. *Pros:* clean, matches the stated Phase-5
  goal, forces the microkernel end-state. *Cons:* large, temporary functional
  regression (ssh/http/ftp/… servers gone until re-homed); irreversible in one
  step.
- **Option C — phased deletion.** Persistent daemon reaches L4 parity; forward
  socket syscalls; delete the L2–L4 core first (Option A's scope), then delete
  each app module in its own follow-up as it's re-homed to userspace. *Pros:*
  incremental, always-buildable, no big regression window. *Cons:* longest
  calendar span; a transitional period where the kernel hosts app protocols over
  a daemon-provided socket API (added coupling).

**Q22b — cutover mechanism (given §64 exclusive claim).** Once the daemon holds
the NIC persistently, the in-kernel stack cannot reach the uplink, so socket
syscalls must forward to the daemon the moment it goes persistent — there is no
clean dual-stack. Options: **(i) big-bang** — one commit flips persistence +
forwarding + deletion together (matches the doc, but a huge untestable step); or
**(ii) staged** — land a persistent daemon + socket-forwarding path behind a
boot flag / config default while the kernel stack remains the compiled fallback,
flip the default once the daemon proves parity in QEMU, then delete. Staged is
far more testable but needs a temporary "which stack owns the NIC" switch that
§64 says can't be a true concurrent dual-stack (only one owns the NIC at a time,
selected at boot).

**Claude's recommendation.** **Q22a → Option C** (phased: core first, app modules
re-homed individually) and **Q22b → (ii) staged** (persistent daemon + forwarding
behind a default-off switch, flip after QEMU parity, then delete). This keeps
every step buildable and boot-testable and avoids a giant irreversible regression,
at the cost of more increments. **Meanwhile** (unblocked under any answer) I'm
building the daemon's persistent multi-connection socket server — evolving the
one-shot ring batch (`ring_tcp_process`, single `Option<TcpConn>`) into a
`conn_id`-keyed connection table so send/recv/close SQEs act on a live connection
across separate submissions and multiple sockets coexist. That is the shared
prerequisite for the socket-syscall forwarders regardless of scope/mechanism.

**Where it bites.**
- Daemon: `services/netstack/src/main.rs` (`ring_tcp_process`, `run_dns_service`,
  `TcpConn`, the NIC-claim lifecycle in `main`).
- Kernel shim: `kernel/src/net/raw.rs`, `SYS_NET_RAW_*`.
- Socket forwarders: `kernel/src/syscall/linux.rs` (`sys_socket`/`sys_connect`/
  `sys_sendto`/`sys_recvfrom`/`sys_bind`/`sys_listen`/`sys_accept`/… at
  ~35139–36546), which today dispatch into `kernel/src/net/{tcp,udp,...}`.
- Deletion target: `kernel/src/net/` (`mod.rs` + the module list above).
- Persistent-spawn path: how init/the service manager launches the daemon at boot
  (today it's spawned only by the bounded kernel self-test in
  `kernel/src/proc/spawn.rs`).

---

No further open questions beyond Q22. Earlier deferred operator decisions (Q1–Q21)
have been resolved — see the "Recently resolved" list below and
`design-decisions.md` for full rationale. New decisions should be appended above
this line as `## Q23 …`.

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
