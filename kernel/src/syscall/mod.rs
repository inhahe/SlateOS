//! Syscall dispatch.
//!
//! Entry point for all userspace-to-kernel transitions via the `syscall`
//! instruction.  Dispatches to subsystem handlers based on syscall number.
//!
//! ## Syscall Number Ranges
//!
//! | Range   | Owner           |
//! |---------|-----------------|
//! | 0-199   | kernel-core     |
//! | 200-399 | kernel-ipc      |
//! | 400-499 | kernel-security |
//! | 500-599 | kernel-process  |
//! | 600-799 | filesystem      |
//! | 800-999 | networking      |
//!
//! ## Design
//!
//! - Versioned syscall tables for ABI stability.
//! - Many specialized syscalls (Linux style).
//! - io_uring-style batched submission as an additional path.
//!
//! ## Performance Target
//!
//! Trivial syscall round-trip: < 200ns (Linux getpid: ~100ns).

// TODO: syscall entry/exit assembly (switch stacks, save/restore regs).
// TODO: Syscall dispatch table.
// TODO: Versioned syscall tables.
// TODO: io_uring submission path.
