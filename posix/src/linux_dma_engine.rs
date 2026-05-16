//! `<linux/dmaengine.h>` — DMA engine framework constants.
//!
//! The DMA engine framework provides a common API for offload
//! engines (DMA controllers) that can perform memory-to-memory,
//! device-to-memory, and memory-to-device data transfers without
//! CPU involvement.

// ---------------------------------------------------------------------------
// DMA transfer direction
// ---------------------------------------------------------------------------

/// Memory to memory.
pub const DMA_MEM_TO_MEM: u32 = 0;
/// Memory to device.
pub const DMA_MEM_TO_DEV: u32 = 1;
/// Device to memory.
pub const DMA_DEV_TO_MEM: u32 = 2;
/// Device to device.
pub const DMA_DEV_TO_DEV: u32 = 3;

// ---------------------------------------------------------------------------
// DMA transaction types (capabilities)
// ---------------------------------------------------------------------------

/// Asynchronous memcpy.
pub const DMA_MEMCPY: u32 = 1 << 0;
/// XOR (RAID5 parity).
pub const DMA_XOR: u32 = 1 << 1;
/// PQ (RAID6 parity).
pub const DMA_PQ: u32 = 1 << 2;
/// XOR validate.
pub const DMA_XOR_VAL: u32 = 1 << 3;
/// PQ validate.
pub const DMA_PQ_VAL: u32 = 1 << 4;
/// Memset.
pub const DMA_MEMSET: u32 = 1 << 5;
/// Scatter-gather memcpy.
pub const DMA_SG: u32 = 1 << 6;
/// Interleaved DMA.
pub const DMA_INTERLEAVE: u32 = 1 << 7;
/// Cyclic DMA.
pub const DMA_CYCLIC: u32 = 1 << 8;
/// Slave/peripheral DMA.
pub const DMA_SLAVE: u32 = 1 << 9;
/// Completion interrupt.
pub const DMA_COMPLETION_NO_ORDER: u32 = 1 << 10;
/// Repeat transfer.
pub const DMA_REPEAT: u32 = 1 << 11;
/// Load EOT (end of transfer event).
pub const DMA_LOAD_EOT: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// DMA control commands
// ---------------------------------------------------------------------------

/// Terminate all pending transfers.
pub const DMA_TERMINATE_ALL: u32 = 0;
/// Pause channel.
pub const DMA_PAUSE: u32 = 1;
/// Resume channel.
pub const DMA_RESUME: u32 = 2;

// ---------------------------------------------------------------------------
// Transfer status
// ---------------------------------------------------------------------------

/// Transfer complete.
pub const DMA_COMPLETE: u32 = 0;
/// Transfer in progress.
pub const DMA_IN_PROGRESS: u32 = 1;
/// Transfer paused.
pub const DMA_PAUSED: u32 = 2;
/// Transfer error.
pub const DMA_ERROR: u32 = 3;

// ---------------------------------------------------------------------------
// DMA descriptor flags
// ---------------------------------------------------------------------------

/// Generate interrupt on completion.
pub const DMA_PREP_INTERRUPT: u32 = 1 << 0;
/// Fence — don't reorder past this.
pub const DMA_PREP_FENCE: u32 = 1 << 1;
/// PQ disable result.
pub const DMA_PREP_PQ_DISABLE_P: u32 = 1 << 2;
/// PQ disable Q result.
pub const DMA_PREP_PQ_DISABLE_Q: u32 = 1 << 3;
/// Continue previous operation.
pub const DMA_PREP_CONTINUE: u32 = 1 << 4;
/// CMD — execute command, not data transfer.
pub const DMA_PREP_CMD: u32 = 1 << 5;
/// Repeat the transfer.
pub const DMA_PREP_REPEAT: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directions_distinct() {
        let dirs = [DMA_MEM_TO_MEM, DMA_MEM_TO_DEV, DMA_DEV_TO_MEM, DMA_DEV_TO_DEV];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_tx_types_powers_of_two() {
        let types = [
            DMA_MEMCPY, DMA_XOR, DMA_PQ, DMA_XOR_VAL, DMA_PQ_VAL,
            DMA_MEMSET, DMA_SG, DMA_INTERLEAVE, DMA_CYCLIC,
            DMA_SLAVE, DMA_COMPLETION_NO_ORDER, DMA_REPEAT, DMA_LOAD_EOT,
        ];
        for t in &types {
            assert!(t.is_power_of_two(), "0x{:x}", t);
        }
    }

    #[test]
    fn test_tx_types_no_overlap() {
        let types = [
            DMA_MEMCPY, DMA_XOR, DMA_PQ, DMA_XOR_VAL, DMA_PQ_VAL,
            DMA_MEMSET, DMA_SG, DMA_INTERLEAVE, DMA_CYCLIC,
            DMA_SLAVE, DMA_COMPLETION_NO_ORDER, DMA_REPEAT, DMA_LOAD_EOT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_control_cmds_distinct() {
        let cmds = [DMA_TERMINATE_ALL, DMA_PAUSE, DMA_RESUME];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_status_distinct() {
        let statuses = [DMA_COMPLETE, DMA_IN_PROGRESS, DMA_PAUSED, DMA_ERROR];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_prep_flags_powers_of_two() {
        let flags = [
            DMA_PREP_INTERRUPT, DMA_PREP_FENCE,
            DMA_PREP_PQ_DISABLE_P, DMA_PREP_PQ_DISABLE_Q,
            DMA_PREP_CONTINUE, DMA_PREP_CMD, DMA_PREP_REPEAT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }
}
