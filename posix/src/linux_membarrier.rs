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

/// Register intent to use expedited global barrier.
pub const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: i32 = 1 << 2;

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
    fn test_membarrier_query_reports_supported_bitmask() {
        // CMD_QUERY returns the bitmask of supported commands.  At
        // least CMD_GLOBAL and CMD_PRIVATE_EXPEDITED must be supported.
        let mask = membarrier(MEMBARRIER_CMD_QUERY, 0, 0);
        assert!(mask > 0);
        assert_ne!(mask & MEMBARRIER_CMD_GLOBAL, 0);
        assert_ne!(mask & MEMBARRIER_CMD_PRIVATE_EXPEDITED, 0);
    }

    #[test]
    fn test_membarrier_register_global_expedited_distinct_from_private() {
        // Regression: these used to collide on the same bit.
        assert_ne!(
            MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED,
        );
    }
}
