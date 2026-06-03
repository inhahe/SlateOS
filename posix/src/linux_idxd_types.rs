//! `<linux/idxd.h>` — Intel Data Streaming/Accelerator constants.
//!
//! Constants for the Intel In-kernel Data Streaming Accelerator (IDXD)
//! userspace interface — descriptors for DSA (Data Streaming
//! Accelerator) and IAA (Intel Analytics Accelerator) command engines.

// ---------------------------------------------------------------------------
// Opcodes (struct dsa_hw_desc.opcode)
// ---------------------------------------------------------------------------

/// No-op descriptor (completion-only).
pub const DSA_OPCODE_NOOP: u32 = 0x00;
/// Batch — describes a list of sub-descriptors.
pub const DSA_OPCODE_BATCH: u32 = 0x01;
/// Drain — wait for in-flight descriptors to complete.
pub const DSA_OPCODE_DRAIN: u32 = 0x02;
/// Memmove (no overlap required to be reported).
pub const DSA_OPCODE_MEMMOVE: u32 = 0x03;
/// Memfill — fill a region with an 8-byte pattern.
pub const DSA_OPCODE_MEMFILL: u32 = 0x04;
/// Compare — byte-compare two regions.
pub const DSA_OPCODE_COMPARE: u32 = 0x05;
/// Compare-pattern — compare a region against a fixed 8-byte pattern.
pub const DSA_OPCODE_COMPVAL: u32 = 0x06;
/// CRC generation.
pub const DSA_OPCODE_CRCGEN: u32 = 0x10;
/// CRC copy.
pub const DSA_OPCODE_COPY_CRC: u32 = 0x11;
/// DIF check.
pub const DSA_OPCODE_DIF_CHECK: u32 = 0x12;
/// DIF insert.
pub const DSA_OPCODE_DIF_INS: u32 = 0x13;
/// DIF strip.
pub const DSA_OPCODE_DIF_STRP: u32 = 0x14;
/// DIF update.
pub const DSA_OPCODE_DIF_UPDT: u32 = 0x15;
/// Cache flush.
pub const DSA_OPCODE_CFLUSH: u32 = 0x20;

// ---------------------------------------------------------------------------
// IAA opcodes
// ---------------------------------------------------------------------------

/// IAA decompress.
pub const IAX_OPCODE_DECOMPRESS: u32 = 0x42;
/// IAA compress.
pub const IAX_OPCODE_COMPRESS: u32 = 0x43;
/// IAA CRC64.
pub const IAX_OPCODE_CRC64: u32 = 0x44;

// ---------------------------------------------------------------------------
// Descriptor flags (struct dsa_hw_desc.flags)
// ---------------------------------------------------------------------------

/// Fence — serialise with previous descriptors.
pub const IDXD_OP_FLAG_FENCE: u32 = 0x0001;
/// Block-on-fault.
pub const IDXD_OP_FLAG_BOF: u32 = 0x0002;
/// Completion record valid.
pub const IDXD_OP_FLAG_CRAV: u32 = 0x0004;
/// Request completion record.
pub const IDXD_OP_FLAG_RCR: u32 = 0x0008;
/// Request completion-interrupt.
pub const IDXD_OP_FLAG_RCI: u32 = 0x0010;
/// Cache-control destination.
pub const IDXD_OP_FLAG_CC: u32 = 0x0020;
/// Address-type bits select shadow / persistent / etc.
pub const IDXD_OP_FLAG_ADDR1_TCS: u32 = 0x0040;
/// Address-2 traffic-class select.
pub const IDXD_OP_FLAG_ADDR2_TCS: u32 = 0x0080;

// ---------------------------------------------------------------------------
// Completion status codes
// ---------------------------------------------------------------------------

/// Completion record status: success.
pub const DSA_COMP_SUCCESS: u32 = 0x01;
/// Compare/compval miscompare (pred fail).
pub const DSA_COMP_SUCCESS_PRED: u32 = 0x02;
/// Page fault on source/dest.
pub const DSA_COMP_PAGE_FAULT_NOBOF: u32 = 0x03;
/// Page-response error (BoF was set).
pub const DSA_COMP_PAGE_FAULT_IR: u32 = 0x04;
/// Bad opcode.
pub const DSA_COMP_BAD_OPCODE: u32 = 0x10;
/// Invalid flags.
pub const DSA_COMP_INVALID_FLAGS: u32 = 0x11;
/// Non-zero reserved field.
pub const DSA_COMP_NOZERO_RESERVE: u32 = 0x12;
/// Buffer overflow.
pub const DSA_COMP_XFER_ERR_READ_BUFF: u32 = 0x13;
/// Descriptor count exceeds limits.
pub const DSA_COMP_DESC_CNT_ERR: u32 = 0x14;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dsa_opcodes_distinct() {
        let opcodes = [
            DSA_OPCODE_NOOP,
            DSA_OPCODE_BATCH,
            DSA_OPCODE_DRAIN,
            DSA_OPCODE_MEMMOVE,
            DSA_OPCODE_MEMFILL,
            DSA_OPCODE_COMPARE,
            DSA_OPCODE_COMPVAL,
            DSA_OPCODE_CRCGEN,
            DSA_OPCODE_COPY_CRC,
            DSA_OPCODE_DIF_CHECK,
            DSA_OPCODE_DIF_INS,
            DSA_OPCODE_DIF_STRP,
            DSA_OPCODE_DIF_UPDT,
            DSA_OPCODE_CFLUSH,
            IAX_OPCODE_DECOMPRESS,
            IAX_OPCODE_COMPRESS,
            IAX_OPCODE_CRC64,
        ];
        for i in 0..opcodes.len() {
            for j in (i + 1)..opcodes.len() {
                assert_ne!(opcodes[i], opcodes[j]);
            }
        }
    }

    #[test]
    fn test_op_flags_distinct_bits() {
        let flags = [
            IDXD_OP_FLAG_FENCE,
            IDXD_OP_FLAG_BOF,
            IDXD_OP_FLAG_CRAV,
            IDXD_OP_FLAG_RCR,
            IDXD_OP_FLAG_RCI,
            IDXD_OP_FLAG_CC,
            IDXD_OP_FLAG_ADDR1_TCS,
            IDXD_OP_FLAG_ADDR2_TCS,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} is not a single bit");
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_completion_codes_distinct() {
        let codes = [
            DSA_COMP_SUCCESS,
            DSA_COMP_SUCCESS_PRED,
            DSA_COMP_PAGE_FAULT_NOBOF,
            DSA_COMP_PAGE_FAULT_IR,
            DSA_COMP_BAD_OPCODE,
            DSA_COMP_INVALID_FLAGS,
            DSA_COMP_NOZERO_RESERVE,
            DSA_COMP_XFER_ERR_READ_BUFF,
            DSA_COMP_DESC_CNT_ERR,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}
