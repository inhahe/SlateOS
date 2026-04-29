//! Scheduler subsystem.
//!
//! Implements the scheduler trait interface and the default priority
//! round-robin scheduler.
//!
//! ## Design
//!
//! - Trait-based: `Scheduler` trait with `pick_next_task`, `enqueue`,
//!   `dequeue`, `task_tick`, `balance_load`.
//! - Default: priority round-robin with 32-64 levels, per-CPU queues,
//!   work stealing, priority inheritance, interactive task detection.
//! - Alternative schedulers can be added behind the same trait.
//!
//! ## Performance Targets
//!
//! - `pick_next_task`: O(1) or O(log n), never O(n)
//! - Context switch: < 5us (Linux: 1-3us)

// TODO: Define Scheduler trait.
// TODO: Implement priority round-robin scheduler.
// TODO: Per-CPU run queues.
// TODO: Work stealing.
// TODO: Priority inheritance.
// TODO: Interactive task detection.
