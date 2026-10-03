## 65. netstack bulk data path — io_uring-style SPSC shared-memory ring (fixed 32B SQE / 16B CQE, cache-line-separated indices, separate data area)

**Date:** 2026-07-14
**Decided by:** Claude (autonomous). Sub-implementation call under §63/§64;
reviewable/reversible — the ABI is versioned (`RING_VERSION`) so it can be
revised before anything depends on it long-term.

**Context.** §64 settled the *control* path (one-shot request/reply over a
Service-Registry channel) but explicitly deferred the *bulk* path: streaming
`send`/`recv` on a TCP/UDP socket cannot ride per-call `channel::Message` copies
without blowing the < 2 µs IPC round-trip and the throughput targets (a per-byte
kernel↔daemon copy per stream chunk is exactly the anti-pattern CLAUDE.md's perf
section calls out — "IPC channels should move pages, not copy contents"). Phase 4
needs a zero-copy transport for socket data before the daemon can host persistent
per-connection state machines.

**Decision: an io_uring-style pair of SPSC rings in one shared-memory region.**
Modeled directly on Linux io_uring (the reference the perf table cites for
submission cost). One `SYS_SHM_CREATE` region holds: a header, a **submission
queue** (SQ — kernel produces, daemon consumes: connect/send/recv/close), a
**completion queue** (CQ — daemon produces, kernel consumes: result + echoed
`user_data`), and a **separate bulk data area**. SQE/CQE carry only a
`(data_off, data_len)` window into the data area, so message bytes are never
copied across the channel — the kernel writes send-data straight into shared
memory and the daemon reads it in place (and vice-versa for recv).

**Sub-choices and why:**
- *Fixed 32-byte SQE / 16-byte CQE* (not variable-length). Fixed stride makes
  `slot = index & (entries-1)` a single mask, keeps entries cache-friendly, and
  avoids a length-parsing step on the hot path. 32B holds op + conn_id +
  (data_off,data_len) + user_data + an 8-byte `aux` (endpoint pack for connect);
  16B holds user_data + result + flags. Chosen over io_uring's larger 64B SQE
  because we don't carry its full opcode surface.
- *Free-running u32 indices, power-of-two entry counts.* Wrapping monotonic
  indices give unambiguous empty (`head==tail`) / full (`tail-head==entries`)
  without a wasted slot, and the mask replaces a modulo.
- *Four indices on separate cache lines* (SQ head/tail, CQ head/tail — header is
  5 cache lines, `HEADER_LEN=320`). Producer and consumer touch different lines,
  so no false sharing on the hottest words. Straight from the per-CPU /
  cache-line-alignment guidance in CLAUDE.md's perf patterns.
- *Pure, mapping-agnostic module.* `netipc/src/ring.rs` defines only the byte
  layout, entry (de)serialization, and index arithmetic — no atomics, no
  mapping. That keeps the shared crate `no_std`, dependency-free, and
  `#![forbid(unsafe_code)]`. Both sides link the one module, so the ring ABI
  can't drift — same rationale as §64's shared schema.
- *Shared atomic driver in a separate `netring` crate (not duplicated at each
  integration site).* The acquire/release atomic index accesses and the
  `push`/`pop`/`write_data`/`read_data` bounds logic are the one genuinely
  subtle, `unsafe`, easy-to-get-wrong part of the ring. Writing them **once** in
  a `no_std` `netring` crate (which depends on `netipc` for the ABI) — rather
  than hand-rolling the Acquire/Release dance separately in the kernel forwarder
  and again in the daemon — means the memory-ordering correctness is written,
  reviewed, and *host-tested* exactly once, then linked verbatim into both
  sides. This directly answers the CLAUDE.md unsafe-policy rule ("isolate
  `unsafe`, wrap it in a safe abstraction as close to the operation as
  possible"): after `Ring::init`/`Ring::attach`, every hot-path method is safe
  and bounds-checked against the length-validated geometry. `netipc` stays
  `#![forbid(unsafe_code)]`; `netring` is the single audited home for the ring's
  `unsafe`. The alternative — open-coding the atomics at both integration sites —
  was rejected because two independent copies of Acquire/Release logic is exactly
  where a subtle ordering bug (visible only under concurrent cross-address-space
  contention, i.e. nearly untestable in situ) would hide.

**Deferred sub-choice (flagged, not yet decided): recv/notification blocking.**
When a `recv` SQE has no data yet, *how* the waiter is parked and woken — futex
on a shared word, an eventfd-style handle, or a channel-signal — is left open;
the ring itself is notification-neutral (a consumer can poll it). This will be
settled when the kernel/daemon integration lands, and is a candidate for an
`open-questions.md` entry if it turns out to have a real tradeoff (polling vs.
futex latency vs. CPU burn). Logged here so the ring ABI isn't mistaken for
having answered it.

**Rejected alternatives:** (a) a single bidirectional ring — conflates the two
producers and needs locking; SPSC pairs are lock-free. (b) Copying stream data
inside `channel::Message` — the thing this whole decision exists to avoid.
(c) Variable-length SQEs — parsing cost + harder slot math for no real gain at
our opcode count.

**Where it lives.** `netipc/src/ring.rs` (ABI + 10 host tests) and the
`netring` crate (`netring/src/lib.rs` — the atomic driver + 9 host tests,
including an end-to-end kernel-init → daemon-attach echo through the ring). Both
are workspace-`exclude`d (built for `x86_64-unknown-none` as deps of the kernel
and daemon; host-testable with an explicit `--target`). Wiring the ring into the
kernel forwarder and the daemon (SHM region + `Ring::init`/`Ring::attach` +
a ring-echo control op) comes in following Phase 4 increments; tracked in
`net-userspace-migration.md`. Streaming-only limitations still noted under
`known-issues.md` `D-NETSTACK-TCP-MINIMAL` until the ring is wired end-to-end.
