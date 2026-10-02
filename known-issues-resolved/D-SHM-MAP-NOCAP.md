### D-SHM-MAP-NOCAP. `SYS_SHM_MAP`/`SYS_SHM_SIZE`/`SYS_SHM_CLOSE` do not verify the caller owns the handle — RESOLVED 2026-07-14

**RESOLVED 2026-07-14 (option (b) — IPC provider-PID + `shm::authorize` grant).**
The three syscalls now enforce per-region authorization. Implementation:
 - `kernel/src/ipc/shm.rs`: `ShmRegion` gained an `authorized: Vec<u64>`
   list; new `shm::authorize(handle, pid)` (idempotent grant) and
   `shm::is_authorized(handle, pid)`.
 - `kernel/src/ipc/service.rs`: the service registry now records the
   registering process's `provider_pid`, exposed via
   `service::provider_pid(name) -> Option<u64>` — the missing identity
   plumbing called out below.
 - `kernel/src/net/netstack_client.rs::submit_round_on` and the five
   `kernel/src/proc/spawn.rs` netstack bootstraps (`shm_ping`, `ring_echo`,
   `ring_tcp`, …) call `shm::authorize(handle, service::provider_pid(b"net.stack"))`
   before handing the daemon a ring region.
 - `kernel/src/syscall/handlers.rs`: `sys_shm_map`/`sys_shm_size`/
   `sys_shm_close` now call `shm_check_authorized(handle)` — a userspace
   caller (`caller_pid()==Some(pid!=0)`) must be the region's creator or an
   authorized PID, else `PermissionDenied`; kernel context (`None`/PID 0)
   is the TCB and always allowed. `sys_shm_create` auto-authorizes the
   creating PID. Boot-validated with `net.userspace` on: the daemon
   (pid 227) completed all SHM-ring parity checks (TCP/UDP/DNS/nonblock/
   poll/listen-accept/connect6) with no permission errors; BOOT_OK at 120s.

Historical context (the original gap and the investigation that led to the
fix) is retained below.

---

`SYS_SHM_MAP` (kernel/src/syscall/handlers.rs `sys_shm_map`) maps a
shared-memory region into the caller's address space given only the
region's raw handle (which *is* the region ID — see `ShmHandle` in
kernel/src/ipc/shm.rs). It does **not** check that the calling process
created or was granted that region: any process that possesses (or
guesses — IDs are a small monotonic counter) a handle can map another
process's shared memory. `SYS_SHM_SIZE` and `SYS_SHM_CLOSE` have the same
gap (pre-existing). This is currently *by design* for the netstack Phase-4
bootstrap: the kernel creates the region and hands the handle to the
trusted `netstack` daemon over the `net.stack` control channel, so both
ends are trusted. **Proper fix:** gate SHM handles through the capability
system (unforgeable, per-process handle table with an explicit
grant/transfer op) before any *untrusted* process is allowed to use
`SYS_SHM_MAP` — i.e. before Phase 5 exposes socket data rings to arbitrary
apps. Until then, only kernel-mediated, trusted-daemon SHM sharing is
safe. Where it bites: any future userspace-to-userspace SHM use.

**Investigation 2026-07-14 (why this is not a quick turn):** the natural
enforcement — "a userspace `SYS_SHM_MAP`/`SIZE`/`CLOSE` caller must own or
have been granted the handle" — needs a way to *authorize the netstack
daemon* for the kernel-created ring regions it legitimately maps, or the
fix hard-breaks the working daemon. The daemon does **not** receive the
region as a tracked capability: the handle travels as *plain u64 payload*
inside the `net.stack` control-channel `Request` (see the daemon's
`shm_ping`/`ring_echo`/`ring_tcp` handlers in `services/netstack/src/main.rs`
and the kernel senders `netipc::encode_ring_tcp` in
`kernel/src/net/netstack_client.rs::submit_round_on` +
`kernel/src/proc/spawn.rs` self-test bootstraps). To authorize the daemon
at those handoff points the kernel must know the daemon's PID — but the
plumbing to derive it doesn't exist: `ipc/channel.rs` has **no** peer-PID
query (only `peer_side`), and `ipc/service.rs`'s `Listener`/registry does
**not** record the provider PID. So the real fix requires one of:
 (a) route the SHM handle through the existing **capability transfer**
     mechanism (channel `Message` cap slots → `ipc/mod.rs` already
     dispatches `ResourceType::SharedMemory` on cleanup) so receipt
     registers ownership in the daemon PCB via `pcb::register_ipc_handle`,
     then gate the three syscalls on `caller_pid()`-ownership (kernel
     context `caller_pid()==None` stays allowed as the TCB); **or**
 (b) add process-identity at the IPC boundary (channel peer-PID or service
     provider-PID) + an explicit `shm::authorize(handle, pid)` grant called
     at each kernel→daemon handoff, then gate the syscalls the same way.
Option (a) is architecturally cleaner (reuses caps, no new identity
plumbing) and is the recommended path. Either way it touches the working
daemon data path and needs full switch-on boot validation, so it is a
deliberate multi-step change rather than a drive-by — deferred until it
can be done carefully (still non-blocking: no untrusted process maps SHM
today).
