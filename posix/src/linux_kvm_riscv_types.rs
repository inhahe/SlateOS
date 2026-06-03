//! `<asm/kvm.h>` (riscv) — KVM RISC-V-specific vCPU constants.
//!
//! KVM gained RISC-V H-extension support in 5.16. Userspace
//! (qemu-system-riscv, kvmtool) creates RISC-V guests using these
//! ISA-extension identifiers, register-set selectors, and SBI
//! extension IDs.

// ---------------------------------------------------------------------------
// vCPU register classes — top 8 bits of the KVM_REG_RISCV reg id
// ---------------------------------------------------------------------------

/// General-purpose configuration register class.
pub const KVM_REG_RISCV_CONFIG: u64 = 0x01 << 24;
/// Core register class (PC, x0..x31).
pub const KVM_REG_RISCV_CORE: u64 = 0x02 << 24;
/// CSR class.
pub const KVM_REG_RISCV_CSR: u64 = 0x03 << 24;
/// Timer (HVIP/STIMECMP) class.
pub const KVM_REG_RISCV_TIMER: u64 = 0x04 << 24;
/// Floating-point F-extension class.
pub const KVM_REG_RISCV_FP_F: u64 = 0x05 << 24;
/// Floating-point D-extension class.
pub const KVM_REG_RISCV_FP_D: u64 = 0x06 << 24;
/// ISA-extension bitmap class.
pub const KVM_REG_RISCV_ISA_EXT: u64 = 0x07 << 24;
/// SBI experimental register class.
pub const KVM_REG_RISCV_SBI_EXT: u64 = 0x08 << 24;
/// Vector V-extension class.
pub const KVM_REG_RISCV_VECTOR: u64 = 0x09 << 24;

// ---------------------------------------------------------------------------
// ISA extension identifiers (KVM_RISCV_ISA_EXT_*)
// ---------------------------------------------------------------------------

/// Atomic (A).
pub const KVM_RISCV_ISA_EXT_A: u32 = 0;
/// Compressed (C).
pub const KVM_RISCV_ISA_EXT_C: u32 = 1;
/// Double float (D).
pub const KVM_RISCV_ISA_EXT_D: u32 = 2;
/// Single float (F).
pub const KVM_RISCV_ISA_EXT_F: u32 = 3;
/// Hypervisor (H).
pub const KVM_RISCV_ISA_EXT_H: u32 = 4;
/// Integer base (I).
pub const KVM_RISCV_ISA_EXT_I: u32 = 5;
/// Mul/div (M).
pub const KVM_RISCV_ISA_EXT_M: u32 = 6;
/// Supervisor-mode SSTC (Sstc).
pub const KVM_RISCV_ISA_EXT_SSTC: u32 = 7;
/// SVINVAL (Svinval).
pub const KVM_RISCV_ISA_EXT_SVINVAL: u32 = 8;
/// SVPBMT (Svpbmt).
pub const KVM_RISCV_ISA_EXT_SVPBMT: u32 = 9;
/// Zbb (basic bit-manipulation).
pub const KVM_RISCV_ISA_EXT_ZBB: u32 = 10;
/// Zicbom (cache-block management).
pub const KVM_RISCV_ISA_EXT_ZICBOM: u32 = 11;
/// Zicboz (cache-block zero).
pub const KVM_RISCV_ISA_EXT_ZICBOZ: u32 = 12;

// ---------------------------------------------------------------------------
// SBI extension IDs (KVM_RISCV_SBI_EXT_*)
// ---------------------------------------------------------------------------

/// SBI legacy v0.1.
pub const KVM_RISCV_SBI_EXT_V01: u32 = 0;
/// SBI Time extension.
pub const KVM_RISCV_SBI_EXT_TIME: u32 = 1;
/// SBI IPI extension.
pub const KVM_RISCV_SBI_EXT_IPI: u32 = 2;
/// SBI RFENCE extension.
pub const KVM_RISCV_SBI_EXT_RFENCE: u32 = 3;
/// SBI SRST (system-reset) extension.
pub const KVM_RISCV_SBI_EXT_SRST: u32 = 4;
/// SBI HSM (hart state management) extension.
pub const KVM_RISCV_SBI_EXT_HSM: u32 = 5;
/// SBI PMU extension.
pub const KVM_RISCV_SBI_EXT_PMU: u32 = 6;
/// Experimental SBI extension.
pub const KVM_RISCV_SBI_EXT_EXPERIMENTAL: u32 = 7;
/// Vendor-defined SBI extension.
pub const KVM_RISCV_SBI_EXT_VENDOR: u32 = 8;

// ---------------------------------------------------------------------------
// Exit reasons specific to RISC-V SBI (KVM_EXIT_RISCV_SBI subcodes)
// ---------------------------------------------------------------------------

/// SBI extension not handled by the host (return to user-mode SBI handler).
pub const KVM_RISCV_EXIT_SBI_UNHANDLED: u32 = 0;
/// SBI call must be forwarded to user space.
pub const KVM_RISCV_EXIT_SBI_FORWARD: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reg_classes_distinct_and_in_upper_byte() {
        let c = [
            KVM_REG_RISCV_CONFIG,
            KVM_REG_RISCV_CORE,
            KVM_REG_RISCV_CSR,
            KVM_REG_RISCV_TIMER,
            KVM_REG_RISCV_FP_F,
            KVM_REG_RISCV_FP_D,
            KVM_REG_RISCV_ISA_EXT,
            KVM_REG_RISCV_SBI_EXT,
            KVM_REG_RISCV_VECTOR,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
            // Class bits live in byte 3 (shift 24..32).
            assert_eq!(c[i] & 0x00ff_ffff, 0);
            assert_eq!(c[i] >> 32, 0);
        }
    }

    #[test]
    fn test_isa_ext_ids_distinct() {
        let e = [
            KVM_RISCV_ISA_EXT_A,
            KVM_RISCV_ISA_EXT_C,
            KVM_RISCV_ISA_EXT_D,
            KVM_RISCV_ISA_EXT_F,
            KVM_RISCV_ISA_EXT_H,
            KVM_RISCV_ISA_EXT_I,
            KVM_RISCV_ISA_EXT_M,
            KVM_RISCV_ISA_EXT_SSTC,
            KVM_RISCV_ISA_EXT_SVINVAL,
            KVM_RISCV_ISA_EXT_SVPBMT,
            KVM_RISCV_ISA_EXT_ZBB,
            KVM_RISCV_ISA_EXT_ZICBOM,
            KVM_RISCV_ISA_EXT_ZICBOZ,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }

    #[test]
    fn test_sbi_ext_ids_distinct() {
        let s = [
            KVM_RISCV_SBI_EXT_V01,
            KVM_RISCV_SBI_EXT_TIME,
            KVM_RISCV_SBI_EXT_IPI,
            KVM_RISCV_SBI_EXT_RFENCE,
            KVM_RISCV_SBI_EXT_SRST,
            KVM_RISCV_SBI_EXT_HSM,
            KVM_RISCV_SBI_EXT_PMU,
            KVM_RISCV_SBI_EXT_EXPERIMENTAL,
            KVM_RISCV_SBI_EXT_VENDOR,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_exit_subcodes_distinct() {
        assert_ne!(KVM_RISCV_EXIT_SBI_UNHANDLED, KVM_RISCV_EXIT_SBI_FORWARD);
    }
}
