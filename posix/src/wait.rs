//! POSIX wait status macros as C-callable functions.
//!
//! Provides `WIFEXITED`, `WEXITSTATUS`, `WIFSIGNALED`, `WTERMSIG`,
//! `WIFSTOPPED`, `WSTOPSIG`, `WCOREDUMP` as exported functions.
//!
//! The underlying logic is in `process.rs`; this module re-exports
//! them with the standard POSIX uppercase names for C compatibility.
//!
//! ## Wait status encoding (Linux-compatible)
//!
//! - Normal exit: bits 15:8 = exit code, bits 7:0 = 0
//! - Signal death: bits 7:0 = signal number (non-zero, not 0x7f)
//! - Stopped: bits 15:8 = stop signal, bits 7:0 = 0x7f

// ---------------------------------------------------------------------------
// Wait option flags
// ---------------------------------------------------------------------------

/// Don't block if no child has exited.
pub const WNOHANG: i32 = 1;
/// Also report stopped children.
pub const WUNTRACED: i32 = 2;
/// Also report continued children.
pub const WCONTINUED: i32 = 8;

// ---------------------------------------------------------------------------
// Wait status inspection functions (C-callable)
// ---------------------------------------------------------------------------

/// True if child terminated normally (via `exit()`).
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "C" fn WIFEXITED(status: i32) -> i32 {
    i32::from(crate::process::wifexited(status))
}

/// Return the exit status of the child.
///
/// Only meaningful if `WIFEXITED(status)` is true.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "C" fn WEXITSTATUS(status: i32) -> i32 {
    crate::process::wexitstatus(status)
}

/// True if child was terminated by a signal.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "C" fn WIFSIGNALED(status: i32) -> i32 {
    i32::from(crate::process::wifsignaled(status))
}

/// Return the signal number that terminated the child.
///
/// Only meaningful if `WIFSIGNALED(status)` is true.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "C" fn WTERMSIG(status: i32) -> i32 {
    crate::process::wtermsig(status)
}

/// True if child is currently stopped.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "C" fn WIFSTOPPED(status: i32) -> i32 {
    // Stopped: low 8 bits = 0x7f.
    i32::from((status & 0xFF) == 0x7F)
}

/// Return the signal that stopped the child.
///
/// Only meaningful if `WIFSTOPPED(status)` is true.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn WSTOPSIG(status: i32) -> i32 {
    (status >> 8) & 0xFF
}

/// True if child produced a core dump (non-POSIX but widely available).
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "C" fn WCOREDUMP(status: i32) -> i32 {
    i32::from(status & 0x80 != 0)
}
