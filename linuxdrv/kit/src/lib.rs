//! lxkit -- the Linux driver layer's system-facing half (design-decisions
//! 1578 rule 3, 1580).
//!
//! A Linux driver process is Linux's own code, the layer's C half
//! (`linuxdrv/emul/`, which implements Linux's functions) and this crate,
//! which implements what the C half stands on -- the `slx_*` interface in
//! `linuxdrv/emul/slx.h` -- with the system's own calls. It never sees a
//! Linux header. It holds the process's entry point, its memory
//! ([`buddy`]), the console, the clock, its arguments ([`args`]), its tasks
//! -- Linux's kthreads and the like, coroutines on its one thread ([`task`],
//! and `sched`, which runs them) -- and how it ends.
//!
//! What can be tested on the host is a library ([`args`], [`buddy`],
//! [`task`]); the rest (`sched`, `sys`) is built only for the driver
//! process's own target.

// Bare only on the driver process's own target; on the host, std, for the
// tests.
#![cfg_attr(target_os = "none", no_std)]

pub mod args;
pub mod buddy;
pub mod task;

#[cfg(target_os = "none")]
mod sched;
#[cfg(target_os = "none")]
mod sys;

/// The exit codes a driver process ends with besides an initcall's own;
/// `slx.h` names the same, and the kernel's self-test reads them.
pub mod exit {
    /// Every initcall returned 0.
    pub const OK: i32 = 0;
    /// The system gave no clock.
    pub const NO_CLOCK: i32 = 120;
    /// Linux code called `BUG()`.
    pub const BUG: i32 = 121;
    /// The layer's Rust half panicked.
    pub const PANIC: i32 = 122;
    /// The system gave no random bytes.
    pub const NO_RANDOM: i32 = 123;
    /// Every task waits on something that will never happen.
    pub const DEADLOCK: i32 = 124;
    /// Linux code reached a function the layer does not implement.
    pub const STOP: i32 = 125;
    /// An initcall failed.
    pub const INIT_FAILED: i32 = 126;
    /// The process's memory could not be set up.
    pub const NO_MEMORY: i32 = 127;
}
