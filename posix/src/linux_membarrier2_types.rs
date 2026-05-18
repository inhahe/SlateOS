//! `<linux/membarrier.h>` — Additional membarrier constants.
//!
//! Supplementary membarrier constants covering command types,
//! flag combinations, and registration commands.

// ---------------------------------------------------------------------------
// Membarrier commands (MEMBARRIER_CMD_*)
// ---------------------------------------------------------------------------

/// Query supported commands.
pub const MEMBARRIER_CMD_QUERY: u32 = 0;
/// Global barrier.
pub const MEMBARRIER_CMD_GLOBAL: u32 = 1 << 0;
/// Global barrier (expedited).
pub const MEMBARRIER_CMD_GLOBAL_EXPEDITED: u32 = 1 << 1;
/// Register for global expedited.
pub const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: u32 = 1 << 2;
/// Private barrier (expedited).
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED: u32 = 1 << 3;
/// Register for private expedited.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED: u32 = 1 << 4;
/// Private expedited sync core.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE: u32 = 1 << 5;
/// Register for private expedited sync core.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE: u32 = 1 << 6;
/// Private expedited RSEQ.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ: u32 = 1 << 7;
/// Register for private expedited RSEQ.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ: u32 = 1 << 8;
/// Get registrations.
pub const MEMBARRIER_CMD_GET_REGISTRATIONS: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Membarrier flags
// ---------------------------------------------------------------------------

/// No flags.
pub const MEMBARRIER_CMD_FLAG_NONE: u32 = 0;
/// CPU flag (target specific CPU).
pub const MEMBARRIER_CMD_FLAG_CPU: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_power_of_two() {
        let cmds = [
            MEMBARRIER_CMD_GLOBAL,
            MEMBARRIER_CMD_GLOBAL_EXPEDITED,
            MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ,
            MEMBARRIER_CMD_GET_REGISTRATIONS,
        ];
        for c in &cmds {
            assert!(c.is_power_of_two(), "0x{:04x} not power of two", c);
        }
    }

    #[test]
    fn test_cmds_no_overlap() {
        let cmds = [
            MEMBARRIER_CMD_GLOBAL,
            MEMBARRIER_CMD_GLOBAL_EXPEDITED,
            MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ,
            MEMBARRIER_CMD_GET_REGISTRATIONS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_eq!(cmds[i] & cmds[j], 0);
            }
        }
    }

    #[test]
    fn test_query_zero() {
        assert_eq!(MEMBARRIER_CMD_QUERY, 0);
    }

    #[test]
    fn test_flag_cpu() {
        assert!(MEMBARRIER_CMD_FLAG_CPU.is_power_of_two());
    }
}
