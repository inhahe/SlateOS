//! `<linux/hmm.h>` — Heterogeneous Memory Management page flags.
//!
//! HMM is the kernel layer that lets GPUs (notably NVIDIA via `nouveau`
//! and `amdgpu`) and other accelerators participate in the host's page
//! table — pages can migrate to device memory and remain mapped into
//! the CPU's address space as "device-private" entries that fault back
//! transparently. Userspace ROCm/CUDA stacks and Mesa SVM rely on the
//! flags below.

// ---------------------------------------------------------------------------
// `enum hmm_pfn_flags` — packed into the low bits of an unsigned long PFN
// ---------------------------------------------------------------------------

/// Page is valid (entry contains a meaningful PFN).
pub const HMM_PFN_VALID: u64 = 1 << 63;
/// Page is writable.
pub const HMM_PFN_WRITE: u64 = 1 << 62;
/// Page is on the device (not host RAM).
pub const HMM_PFN_ERROR: u64 = 1 << 61;
/// Compound (huge) page.
pub const HMM_PFN_ORDER_SHIFT: u32 = 56;
/// 5-bit order field below the shift.
pub const HMM_PFN_ORDER_MASK: u64 = 0x1F << HMM_PFN_ORDER_SHIFT;

// ---------------------------------------------------------------------------
// Required flags for hmm_range_fault() input
// ---------------------------------------------------------------------------

/// Caller requires the page to be present.
pub const HMM_PFN_REQ_FAULT: u64 = HMM_PFN_VALID;
/// Caller requires the page to be writable.
pub const HMM_PFN_REQ_WRITE: u64 = HMM_PFN_WRITE;

// ---------------------------------------------------------------------------
// Flag mask for clearing
// ---------------------------------------------------------------------------

/// Mask covering all HMM flag bits.
pub const HMM_PFN_FLAGS: u64 = HMM_PFN_VALID | HMM_PFN_WRITE | HMM_PFN_ERROR;

// ---------------------------------------------------------------------------
// Device-private migration types (struct migrate_vma.src/dst flags)
// ---------------------------------------------------------------------------

/// Source/destination is migration-eligible.
pub const MIGRATE_PFN_VALID: u64 = 1 << 0;
/// Page already migrated.
pub const MIGRATE_PFN_MIGRATE: u64 = 1 << 1;
/// Page is locked.
pub const MIGRATE_PFN_LOCKED: u64 = 1 << 2;
/// Page is write-protected.
pub const MIGRATE_PFN_WRITE: u64 = 1 << 3;
/// Bits below this shift are PFN payload.
pub const MIGRATE_PFN_SHIFT: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pfn_flag_bits_in_high_byte() {
        // VALID/WRITE/ERROR live in the high bits so they don't collide
        // with the PFN payload (which never reaches that high on x86_64).
        for &b in &[HMM_PFN_VALID, HMM_PFN_WRITE, HMM_PFN_ERROR] {
            assert!(b >= 1u64 << 56);
            assert!(b.is_power_of_two());
        }
        assert_ne!(HMM_PFN_VALID, HMM_PFN_WRITE);
        assert_ne!(HMM_PFN_WRITE, HMM_PFN_ERROR);
        assert_ne!(HMM_PFN_VALID, HMM_PFN_ERROR);
    }

    #[test]
    fn test_flags_aggregate() {
        // FLAGS mask must include each individual flag and nothing else.
        assert_eq!(
            HMM_PFN_FLAGS,
            HMM_PFN_VALID | HMM_PFN_WRITE | HMM_PFN_ERROR
        );
        assert_eq!(HMM_PFN_FLAGS & HMM_PFN_VALID, HMM_PFN_VALID);
        assert_eq!(HMM_PFN_FLAGS & HMM_PFN_WRITE, HMM_PFN_WRITE);
        assert_eq!(HMM_PFN_FLAGS & HMM_PFN_ERROR, HMM_PFN_ERROR);
    }

    #[test]
    fn test_req_flags_alias_state_flags() {
        // Input "require" flags reuse the output state flag bits.
        assert_eq!(HMM_PFN_REQ_FAULT, HMM_PFN_VALID);
        assert_eq!(HMM_PFN_REQ_WRITE, HMM_PFN_WRITE);
    }

    #[test]
    fn test_order_field_layout() {
        // 5-bit order field at the documented shift.
        assert_eq!(HMM_PFN_ORDER_SHIFT, 56);
        assert_eq!(HMM_PFN_ORDER_MASK >> HMM_PFN_ORDER_SHIFT, 0x1F);
        // Order must not overlap the high flag bits.
        assert_eq!(HMM_PFN_ORDER_MASK & HMM_PFN_VALID, 0);
        assert_eq!(HMM_PFN_ORDER_MASK & HMM_PFN_WRITE, 0);
        assert_eq!(HMM_PFN_ORDER_MASK & HMM_PFN_ERROR, 0);
    }

    #[test]
    fn test_migrate_flags_pow2_and_distinct() {
        let m = [
            MIGRATE_PFN_VALID,
            MIGRATE_PFN_MIGRATE,
            MIGRATE_PFN_LOCKED,
            MIGRATE_PFN_WRITE,
        ];
        for &b in &m {
            assert!(b.is_power_of_two());
        }
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
        // PFN payload starts above the flag bits.
        assert!((1u64 << MIGRATE_PFN_SHIFT) > MIGRATE_PFN_WRITE);
    }
}
