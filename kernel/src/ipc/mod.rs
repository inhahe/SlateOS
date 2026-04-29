//! Inter-Process Communication subsystem.
//!
//! ## IPC Mechanisms
//!
//! - **Channels** (primary): structured messages + capability transfer.
//!   Async (buffered) by default, sync (rendezvous) as option.
//! - **Pipes**: one-way byte streams with splice/vmsplice optimization.
//! - **Shared memory**: direct memory sharing with ring buffer support,
//!   futex signaling, and seqlocks.
//! - **Eventfd counters**: lightweight wake-up notifications.
//!
//! ## Performance Targets
//!
//! - Channel round-trip: < 2us (Fuchsia: 1-2us, L4: 0.5-1us)
//! - Eventfd wake: 0.5-1us

// TODO: Channel implementation (send, recv, capability transfer).
// TODO: Synchronous (rendezvous) channel mode.
// TODO: Pipe implementation.
// TODO: Shared memory regions.
// TODO: Eventfd-like counters.
// TODO: Benchmark all IPC paths.
