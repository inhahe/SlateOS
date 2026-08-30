//! `<linux/idxd.h>` — Intel Data Streaming Accelerator (DSA/IAX).
//!
//! Sapphire Rapids and later server CPUs expose a hardware data-movement
//! engine: zero-buffer memmove, CRC offload, dedup hashing, in-memory
//! compression (IAX). Userspace submits descriptors through SVM-mapped
//! work queues; the `accel-config` tool and DPDK's `dmadev` driver
//! consume the opcodes and flags below.

// ---------------------------------------------------------------------------
// Descriptor opcodes (struct dsa_hw_desc.opcode)
// ---------------------------------------------------------------------------

pub const DSA_OPCODE_NOOP: u32 = 0x00;
pub const DSA_OPCODE_BATCH: u32 = 0x01;
pub const DSA_OPCODE_DRAIN: u32 = 0x02;
pub const DSA_OPCODE_MEMMOVE: u32 = 0x03;
pub const DSA_OPCODE_MEMFILL: u32 = 0x04;
pub const DSA_OPCODE_COMPARE: u32 = 0x05;
pub const DSA_OPCODE_COMPVAL: u32 = 0x06;
pub const DSA_OPCODE_CR_DELTA: u32 = 0x07;
pub const DSA_OPCODE_AP_DELTA: u32 = 0x08;
pub const DSA_OPCODE_DUALCAST: u32 = 0x09;
pub const DSA_OPCODE_CRCGEN: u32 = 0x10;
pub const DSA_OPCODE_COPY_CRC: u32 = 0x11;
pub const DSA_OPCODE_DIF_CHECK: u32 = 0x12;
pub const DSA_OPCODE_DIF_INS: u32 = 0x13;
pub const DSA_OPCODE_DIF_STRP: u32 = 0x14;
pub const DSA_OPCODE_DIF_UPDT: u32 = 0x15;
pub const DSA_OPCODE_CFLUSH: u32 = 0x20;

// ---------------------------------------------------------------------------
// Descriptor flags (struct dsa_hw_desc.flags)
// ---------------------------------------------------------------------------

/// Fence — wait for prior descriptors to drain.
pub const IDXD_OP_FLAG_FENCE: u32 = 0x0001;
/// Block on fault — pause work queue if a page fault occurs.
pub const IDXD_OP_FLAG_BOF: u32 = 0x0002;
/// Completion record valid.
pub const IDXD_OP_FLAG_CRAV: u32 = 0x0004;
/// Request completion interrupt.
pub const IDXD_OP_FLAG_RCI: u32 = 0x0008;
/// Request completion record.
pub const IDXD_OP_FLAG_RCR: u32 = 0x0010;
/// Cache control — destination address is non-temporal.
pub const IDXD_OP_FLAG_CC: u32 = 0x0100;
/// Address-translation cache control.
pub const IDXD_OP_FLAG_ADDR1_TCS: u32 = 0x0200;
/// Source address-translation cache control.
pub const IDXD_OP_FLAG_ADDR2_TCS: u32 = 0x0400;
/// Use destination read-back to ensure cache eviction.
pub const IDXD_OP_FLAG_STORD: u32 = 0x0800;
/// Strict ordering between work queues.
pub const IDXD_OP_FLAG_STRICT: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Work-queue mode (per-WQ config in sysfs)
// ---------------------------------------------------------------------------

pub const IDXD_WQ_MODE_SHARED: u32 = 0;
pub const IDXD_WQ_MODE_DEDICATED: u32 = 1;

// ---------------------------------------------------------------------------
// Device kinds
// ---------------------------------------------------------------------------

/// Generic DSA device.
pub const IDXD_TYPE_DSA: u32 = 0;
/// In-memory Analytics Accelerator (compression/decompression).
pub const IDXD_TYPE_IAX: u32 = 1;

// ---------------------------------------------------------------------------
// Status codes (completion-record .status)
// ---------------------------------------------------------------------------

pub const DSA_COMP_NONE: u8 = 0;
pub const DSA_COMP_SUCCESS: u8 = 0x01;
pub const DSA_COMP_SUCCESS_PRED: u8 = 0x02;
pub const DSA_COMP_PAGE_FAULT_NOBOF: u8 = 0x03;
pub const DSA_COMP_PAGE_FAULT_IR: u8 = 0x04;
pub const DSA_COMP_BATCH_FAIL: u8 = 0x05;
pub const DSA_COMP_BATCH_PAGE_FAULT: u8 = 0x06;
pub const DSA_COMP_DR_OFFSET_NOINC: u8 = 0x07;
pub const DSA_COMP_DR_OFFSET_ERANGE: u8 = 0x08;
pub const DSA_COMP_DIF_ERR: u8 = 0x09;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let o = [
            DSA_OPCODE_NOOP,
            DSA_OPCODE_BATCH,
            DSA_OPCODE_DRAIN,
            DSA_OPCODE_MEMMOVE,
            DSA_OPCODE_MEMFILL,
            DSA_OPCODE_COMPARE,
            DSA_OPCODE_COMPVAL,
            DSA_OPCODE_CR_DELTA,
            DSA_OPCODE_AP_DELTA,
            DSA_OPCODE_DUALCAST,
            DSA_OPCODE_CRCGEN,
            DSA_OPCODE_COPY_CRC,
            DSA_OPCODE_DIF_CHECK,
            DSA_OPCODE_DIF_INS,
            DSA_OPCODE_DIF_STRP,
            DSA_OPCODE_DIF_UPDT,
            DSA_OPCODE_CFLUSH,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
        // All opcodes fit in a single byte.
        for &v in &o {
            assert!(v <= 0xFF);
        }
    }

    #[test]
    fn test_flags_pow2_and_distinct() {
        let f = [
            IDXD_OP_FLAG_FENCE,
            IDXD_OP_FLAG_BOF,
            IDXD_OP_FLAG_CRAV,
            IDXD_OP_FLAG_RCI,
            IDXD_OP_FLAG_RCR,
            IDXD_OP_FLAG_CC,
            IDXD_OP_FLAG_ADDR1_TCS,
            IDXD_OP_FLAG_ADDR2_TCS,
            IDXD_OP_FLAG_STORD,
            IDXD_OP_FLAG_STRICT,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_wq_modes_distinct() {
        assert_ne!(IDXD_WQ_MODE_SHARED, IDXD_WQ_MODE_DEDICATED);
    }

    #[test]
    fn test_device_types_distinct() {
        assert_ne!(IDXD_TYPE_DSA, IDXD_TYPE_IAX);
        assert_eq!(IDXD_TYPE_DSA, 0);
        assert_eq!(IDXD_TYPE_IAX, 1);
    }

    #[test]
    fn test_status_codes_dense() {
        let s = [
            DSA_COMP_NONE,
            DSA_COMP_SUCCESS,
            DSA_COMP_SUCCESS_PRED,
            DSA_COMP_PAGE_FAULT_NOBOF,
            DSA_COMP_PAGE_FAULT_IR,
            DSA_COMP_BATCH_FAIL,
            DSA_COMP_BATCH_PAGE_FAULT,
            DSA_COMP_DR_OFFSET_NOINC,
            DSA_COMP_DR_OFFSET_ERANGE,
            DSA_COMP_DIF_ERR,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
