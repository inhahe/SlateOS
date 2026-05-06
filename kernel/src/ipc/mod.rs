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
//! - **Completion port**: unified wait on heterogeneous kernel objects
//!   (IOCP-style).
//! - **Service registry**: named service discovery + connection brokering.
//!
//! ## Performance Targets
//!
//! - Channel round-trip: < 2us (Fuchsia: 1-2us, L4: 0.5-1us)
//! - Eventfd wake: 0.5-1us
//! - Completion port ready events: sub-microsecond

pub mod channel;
pub mod completion;
pub mod eventfd;
pub mod futex;
pub mod io_ring;
pub mod pipe;
pub mod semaphore;
pub mod service;
pub mod shm;
pub mod stats;
pub mod timer;

// TODO: Synchronous (rendezvous) channel mode.
// TODO: Splice/vmsplice for pipes.
// TODO: Benchmark all IPC paths.
