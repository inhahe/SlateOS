//! `<asm/hwprobe.h>` — RISC-V hwprobe(2) syscall constants.
//!
//! The `riscv_hwprobe(2)` syscall lets userspace query the CPU
//! ISA-extension and microarchitectural feature set of each hart on
//! a RISC-V Linux system. glibc and libc++ feature-detection code
//! consume these.

// ---------------------------------------------------------------------------
// Probe key codes (riscv_hwprobe.key)
// ---------------------------------------------------------------------------

/// MVENDORID CSR value of the queried hart.
pub const RISCV_HWPROBE_KEY_MVENDORID: u32 = 0;
/// MARCHID CSR value.
pub const RISCV_HWPROBE_KEY_MARCHID: u32 = 1;
/// MIMPID CSR value.
pub const RISCV_HWPROBE_KEY_MIMPID: u32 = 2;
/// Base behavior set (whether RV64GC is supported).
pub const RISCV_HWPROBE_KEY_BASE_BEHAVIOR: u32 = 3;
/// IMA-extension bitmap (Z extensions etc.).
pub const RISCV_HWPROBE_KEY_IMA_EXT_0: u32 = 4;
/// Performance hint for misaligned scalar accesses.
pub const RISCV_HWPROBE_KEY_CPUPERF_0: u32 = 5;
/// Zicboz block size (cache-block ops).
pub const RISCV_HWPROBE_KEY_ZICBOZ_BLOCK_SIZE: u32 = 6;
/// Highest-userspace virtual-address bit.
pub const RISCV_HWPROBE_KEY_HIGHEST_VIRT_ADDRESS: u32 = 7;
/// Time-counter frequency (Hz).
pub const RISCV_HWPROBE_KEY_TIME_CSR_FREQ: u32 = 8;

// ---------------------------------------------------------------------------
// Base-behavior return-value bits (RISCV_HWPROBE_KEY_BASE_BEHAVIOR)
// ---------------------------------------------------------------------------

/// IMAFDC (RV64GC) baseline supported.
pub const RISCV_HWPROBE_BASE_BEHAVIOR_IMA: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// IMA-extension bitmap bits (RISCV_HWPROBE_KEY_IMA_EXT_0)
// ---------------------------------------------------------------------------

/// FD — single/double float.
pub const RISCV_HWPROBE_IMA_FD: u64 = 1 << 0;
/// C — compressed.
pub const RISCV_HWPROBE_IMA_C: u64 = 1 << 1;
/// V — vector.
pub const RISCV_HWPROBE_IMA_V: u64 = 1 << 2;
/// Zba bit-manipulation.
pub const RISCV_HWPROBE_EXT_ZBA: u64 = 1 << 3;
/// Zbb bit-manipulation.
pub const RISCV_HWPROBE_EXT_ZBB: u64 = 1 << 4;
/// Zbs single-bit ops.
pub const RISCV_HWPROBE_EXT_ZBS: u64 = 1 << 5;

// ---------------------------------------------------------------------------
// CPU-perf hint values (RISCV_HWPROBE_KEY_CPUPERF_0)
// ---------------------------------------------------------------------------

/// Misaligned access performance is unknown.
pub const RISCV_HWPROBE_MISALIGNED_UNKNOWN: u64 = 0;
/// Misaligned access is emulated (very slow).
pub const RISCV_HWPROBE_MISALIGNED_EMULATED: u64 = 1;
/// Misaligned access slower than aligned.
pub const RISCV_HWPROBE_MISALIGNED_SLOW: u64 = 2;
/// Misaligned access as fast as aligned.
pub const RISCV_HWPROBE_MISALIGNED_FAST: u64 = 3;
/// Misaligned access not supported (traps).
pub const RISCV_HWPROBE_MISALIGNED_UNSUPPORTED: u64 = 4;

/// Mask for the misaligned-perf sub-field of the cpuperf return.
pub const RISCV_HWPROBE_MISALIGNED_MASK: u64 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keys_distinct() {
        let keys = [
            RISCV_HWPROBE_KEY_MVENDORID,
            RISCV_HWPROBE_KEY_MARCHID,
            RISCV_HWPROBE_KEY_MIMPID,
            RISCV_HWPROBE_KEY_BASE_BEHAVIOR,
            RISCV_HWPROBE_KEY_IMA_EXT_0,
            RISCV_HWPROBE_KEY_CPUPERF_0,
            RISCV_HWPROBE_KEY_ZICBOZ_BLOCK_SIZE,
            RISCV_HWPROBE_KEY_HIGHEST_VIRT_ADDRESS,
            RISCV_HWPROBE_KEY_TIME_CSR_FREQ,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_ext_bits_distinct_powers_of_two() {
        let exts = [
            RISCV_HWPROBE_IMA_FD,
            RISCV_HWPROBE_IMA_C,
            RISCV_HWPROBE_IMA_V,
            RISCV_HWPROBE_EXT_ZBA,
            RISCV_HWPROBE_EXT_ZBB,
            RISCV_HWPROBE_EXT_ZBS,
        ];
        for &e in &exts {
            assert!(e.is_power_of_two());
        }
        for i in 0..exts.len() {
            for j in (i + 1)..exts.len() {
                assert_ne!(exts[i], exts[j]);
            }
        }
    }

    #[test]
    fn test_misaligned_values_within_mask() {
        let vals = [
            RISCV_HWPROBE_MISALIGNED_UNKNOWN,
            RISCV_HWPROBE_MISALIGNED_EMULATED,
            RISCV_HWPROBE_MISALIGNED_SLOW,
            RISCV_HWPROBE_MISALIGNED_FAST,
            RISCV_HWPROBE_MISALIGNED_UNSUPPORTED,
        ];
        // Every misaligned-performance code must fit the documented
        // 3-bit sub-field.
        for &v in &vals {
            assert!(v <= RISCV_HWPROBE_MISALIGNED_MASK);
        }
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_base_behavior_ima_set() {
        // The IMA baseline must be advertised; userspace relies on
        // this for any RV64GC inline assembly.
        assert!(RISCV_HWPROBE_BASE_BEHAVIOR_IMA.is_power_of_two());
    }
}
