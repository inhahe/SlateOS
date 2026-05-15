//! `<linux/membarrier.h>` — memory barrier definitions.
//!
//! Provides constants for the `membarrier()` system call, which
//! provides memory ordering guarantees across CPUs.

// ---------------------------------------------------------------------------
// membarrier commands
// ---------------------------------------------------------------------------

/// Query supported commands.
pub const MEMBARRIER_CMD_QUERY: i32 = 0;

/// Issue a global memory barrier (targets all threads/CPUs).
pub const MEMBARRIER_CMD_GLOBAL: i32 = 1;

/// Expedited global memory barrier (with IPI).
pub const MEMBARRIER_CMD_GLOBAL_EXPEDITED: i32 = 1 << 1;

/// Register intent to use expedited barrier.
pub const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: i32 = 1 << 4;

/// Expedited private memory barrier (same process only).
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED: i32 = 1 << 3;

/// Register intent to use private expedited barrier.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED: i32 = 1 << 4;

/// Private expedited sync-core barrier.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE: i32 = 1 << 5;

/// Register for sync-core private expedited.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE: i32 = 1 << 6;

/// Private expedited RSEQ barrier.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ: i32 = 1 << 7;

/// Register for RSEQ private expedited.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ: i32 = 1 << 8;

// ---------------------------------------------------------------------------
// Re-export membarrier function
// ---------------------------------------------------------------------------

pub use crate::process::membarrier;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_query_zero() {
        assert_eq!(MEMBARRIER_CMD_QUERY, 0);
    }

    #[test]
    fn test_cmd_global_value() {
        assert_eq!(MEMBARRIER_CMD_GLOBAL, 1);
    }

    #[test]
    fn test_cmds_nonzero() {
        let cmds = [
            MEMBARRIER_CMD_GLOBAL,
            MEMBARRIER_CMD_GLOBAL_EXPEDITED,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ,
        ];
        for &c in &cmds {
            assert_ne!(c, 0);
        }
    }

    #[test]
    fn test_membarrier_query_stub() {
        // membarrier with CMD_QUERY should return supported mask or -1.
        let ret = membarrier(MEMBARRIER_CMD_QUERY, 0, 0);
        // Stub returns -1 (ENOSYS).
        assert!(ret == -1 || ret >= 0);
    }
}
