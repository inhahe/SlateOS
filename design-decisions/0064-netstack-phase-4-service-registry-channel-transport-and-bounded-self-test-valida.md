## 64. netstack Phase 4 — Service-Registry channel transport, and bounded-self-test validation because the raw-NIC claim is exclusive

**Date:** 2026-07-14
**Decided by:** Claude (operator-approved scope). Sub-implementation call under
§63's Path B; reviewable/reversible.

**Context.** Phase 4 turns socket syscalls (`SYS_DNS_RESOLVE`, `SYS_TCP_*`,
`SYS_UDP_*`) into IPC to the `netstack` daemon. Two questions had to be settled
before writing code: (a) *what transport* carries the app↔daemon request/reply,
and (b) *how to validate it* given the rest of the system.

**Transport decision: the kernel Service Registry (`kernel/src/ipc/service.rs`).**
It already provides exactly the primitives Phase 4 needs: `register(name)` for the
daemon to publish `net.stack`, `connect(name)` for the kernel-side syscall handler
to obtain a client channel endpoint, and `accept`/`try_accept` for the daemon to
take the server endpoint. Requests/replies ride `channel::Message` byte payloads
over that channel (`channel::send`/`recv`). It is kernel-mediated, name-based, and
namespace-isolated — no new IPC mechanism required. Rejected alternatives: a bespoke
syscall pair dedicated to netstack (redundant with the registry, more ABI surface);
raw shared memory only (needed later for the *bulk data* path, but overkill for the
one-shot request/reply control path — start with channel messages, add a shared-mem
data ring for TCP/UDP streaming as a later increment).

**Validation constraint (the important finding): the raw-NIC claim is exclusive.**
The Phase-1 `sys_net_raw_open` gives its owner an *exclusive* claim, and `net::poll()`
deliberately skips draining the physical NIC while a raw owner holds it (so the
kernel stack and a raw owner never both consume frames). Consequence: a *persistent*
netstack daemon that holds the NIC open forever would starve the kernel-resident
stack's RX. Since the kernel stack must stay live until Phase 5 cutover, Phase 4
**cannot** run an always-on daemon that owns the NIC and simultaneously forward
syscalls to it while the kernel stack still serves other traffic — they would fight
over the NIC.

**Decision: validate Phase 4 with bounded self-tests (Phase-2 style), defer
persistent cutover to Phase 5.** Each Phase 4 increment (starting with
`SYS_DNS_RESOLVE`) is proven by a self-test that: spins up the daemon, has it claim
the NIC, register its service, service exactly one request/reply, then release the
NIC — a bounded window, not a permanent takeover. The syscall-forwarding wiring is
built and exercised, but the kernel keeps its own stack as the live path until
Phase 5 flips every socket syscall to the daemon at once and deletes the kernel
stack (at which point the exclusive claim becomes correct, not a conflict).

**Where it lives.** `services/netstack/src/main.rs` (service registration +
request loop), kernel socket-syscall handlers (`kernel/src/syscall/handlers.rs`,
`connect`/`send`/`recv` to `net.stack`), a bounded self-test in the boot self-test
path. Schema + status tracked in `net-userspace-migration.md` Phase 4.
