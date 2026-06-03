//! `<linux/membarrier.h>` — `membarrier()` syscall command codes.
//!
//! `membarrier()` lets userspace coordinate full memory barriers
//! across all threads of a process (or across CPUs) at low cost.
//! GNU libc rseq, the LLVM ORC runtime, and several JITs use it to
//! avoid placing barriers on the fast path. Constants below enumerate
//! the commands and the optional CPU-set flags introduced in 5.10.

// ---------------------------------------------------------------------------
// Command codes (passed in the `cmd` argument)
// ---------------------------------------------------------------------------

/// Query the bitmap of supported commands.
pub const MEMBARRIER_CMD_QUERY: u32 = 0;
/// System-wide full barrier on every CPU (requires
/// `MEMBARRIER_CMD_GLOBAL`).
pub const MEMBARRIER_CMD_GLOBAL: u32 = 1 << 0;
/// System-wide expedited full barrier (registered processes only).
pub const MEMBARRIER_CMD_GLOBAL_EXPEDITED: u32 = 1 << 1;
/// Register intent to use `GLOBAL_EXPEDITED`.
pub const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: u32 = 1 << 2;
/// Private process expedited full barrier.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED: u32 = 1 << 3;
/// Register intent to use `PRIVATE_EXPEDITED`.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED: u32 = 1 << 4;
/// Private SYNC_CORE expedited barrier (architectural icache flush).
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE: u32 = 1 << 5;
/// Register intent to use `PRIVATE_EXPEDITED_SYNC_CORE`.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE: u32 = 1 << 6;
/// Private expedited rseq-fence barrier.
pub const MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ: u32 = 1 << 7;
/// Register intent to use the rseq variant.
pub const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ: u32 = 1 << 8;
/// "Get registration bitmap" diagnostic command (5.18+).
pub const MEMBARRIER_CMD_GET_REGISTRATIONS: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Flag bits (3rd argument to membarrier())
// ---------------------------------------------------------------------------

/// Restrict the barrier to a single CPU specified in `cpu_id`.
pub const MEMBARRIER_CMD_FLAG_CPU: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Bit-mask covering all registration commands (used by glibc's
// `__pthread_membarrier_init` to know what to register up-front).
// ---------------------------------------------------------------------------

/// Combined mask of every `REGISTER_*` command.
pub const MEMBARRIER_CMD_REGISTER_ALL: u32 = MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE
    | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_are_single_bits_or_query_zero() {
        assert_eq!(MEMBARRIER_CMD_QUERY, 0);
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
        for &c in &cmds {
            // Each command is a single bit so QUERY can return a bitmap.
            assert!(c.is_power_of_two());
        }
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_distinct_and_single_bit() {
        assert!(MEMBARRIER_CMD_FLAG_CPU.is_power_of_two());
    }

    #[test]
    fn test_register_all_covers_register_commands() {
        // REGISTER_ALL must contain each REGISTER_* bit.
        let regs = [
            MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE,
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ,
        ];
        for &r in &regs {
            assert_eq!(MEMBARRIER_CMD_REGISTER_ALL & r, r);
        }
        // And not contain non-REGISTER bits.
        assert_eq!(MEMBARRIER_CMD_REGISTER_ALL & MEMBARRIER_CMD_GLOBAL, 0);
        assert_eq!(
            MEMBARRIER_CMD_REGISTER_ALL & MEMBARRIER_CMD_PRIVATE_EXPEDITED,
            0
        );
    }
}
