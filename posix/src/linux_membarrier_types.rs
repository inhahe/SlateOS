//! `<linux/membarrier.h>` — membarrier() command constants.
//!
//! membarrier() provides memory barrier semantics across CPUs without
//! requiring each thread to execute its own barrier instruction. One
//! thread issues a "heavy" barrier; the kernel ensures all other
//! threads in the process observe a matching barrier. Used by RCU
//! implementations, JIT compilers, and lock-free data structures.

// ---------------------------------------------------------------------------
// membarrier commands
// ---------------------------------------------------------------------------

/// Query supported commands (returns bitmask).
pub const MEMBARRIER_CMD_QUERY: u32 = 0;
/// Issue global memory barrier (all threads).
pub const MEMBARRIER_CMD_GLOBAL: u32 = 1 << 0;
/// Expedited global barrier (IPI-based, faster).
pub const MEMBARRIER_CMD_GLOBAL_EXPEDITED: u32 = 1 << 1;
/// Register intent to receive expedited barriers.
pub const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: u32 = 1 << 2;
/// Private expedited barrier (same process only).
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED: u32 = 1 << 3;
/// Register for private expedited barriers.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED: u32 = 1 << 4;
/// Private expedited sync-core (flush pipeline).
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE: u32 = 1 << 5;
/// Register for sync-core barriers.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE: u32 = 1 << 6;
/// Private expedited RSEQ (restart restartable sequences).
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ: u32 = 1 << 7;
/// Register for RSEQ barriers.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// membarrier flags (for cmd argument)
// ---------------------------------------------------------------------------

/// Target a specific CPU (requires cpu_id argument).
pub const MEMBARRIER_CMD_FLAG_CPU: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// membarrier RSEQ flags
// ---------------------------------------------------------------------------

/// Restart rseq on specified CPU only.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ_FLAG_CPU: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_no_overlap() {
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
        ];
        for i in 0..cmds.len() {
            assert!(cmds[i].is_power_of_two());
            for j in (i + 1)..cmds.len() {
                assert_eq!(cmds[i] & cmds[j], 0);
            }
        }
    }

    #[test]
    fn test_query_is_zero() {
        assert_eq!(MEMBARRIER_CMD_QUERY, 0);
    }

    #[test]
    fn test_global_is_first_bit() {
        assert_eq!(MEMBARRIER_CMD_GLOBAL, 1);
    }

    #[test]
    fn test_flag_cpu() {
        assert_eq!(MEMBARRIER_CMD_FLAG_CPU, 1);
        assert_eq!(
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ_FLAG_CPU,
            MEMBARRIER_CMD_FLAG_CPU
        );
    }

    #[test]
    fn test_register_follows_cmd() {
        // Each register command is the next power of two after its cmd.
        assert_eq!(
            MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED,
            MEMBARRIER_CMD_GLOBAL_EXPEDITED << 1
        );
        assert_eq!(
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED << 1
        );
        assert_eq!(
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE << 1
        );
        assert_eq!(
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ,
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ << 1
        );
    }
}
