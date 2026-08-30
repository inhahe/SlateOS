//! `<asm/kvm.h>` (arm64) — KVM ARM-specific vCPU and device constants.
//!
//! ARM/ARM64-only KVM uapi values used by qemu/kvmtool when creating
//! AArch64 guests: vCPU feature bits, target identifiers, exception
//! types, and PSCI/GIC device control codes. These mirror the
//! definitions in the kernel's `asm/kvm.h` on aarch64.

// ---------------------------------------------------------------------------
// KVM_ARM_VCPU_INIT.features bits
// ---------------------------------------------------------------------------

/// CPU is powered off on creation (wait for PSCI start).
pub const KVM_ARM_VCPU_POWER_OFF: u32 = 0;
/// Emulate EL1 32-bit (AArch32) on a 64-bit host.
pub const KVM_ARM_VCPU_EL1_32BIT: u32 = 1;
/// Enable PMUv3 emulation.
pub const KVM_ARM_VCPU_PMU_V3: u32 = 3;
/// Enable Scalable Vector Extension (SVE).
pub const KVM_ARM_VCPU_SVE: u32 = 4;
/// Enable Pointer Authentication (address keys).
pub const KVM_ARM_VCPU_PTRAUTH_ADDRESS: u32 = 5;
/// Enable Pointer Authentication (generic key).
pub const KVM_ARM_VCPU_PTRAUTH_GENERIC: u32 = 6;
/// vCPU is a System Suspended (SUSPENDED on creation).
pub const KVM_ARM_VCPU_HAS_EL2: u32 = 7;

// ---------------------------------------------------------------------------
// Target CPU identifiers (KVM_ARM_PREFERRED_TARGET → kvm_vcpu_init.target)
// ---------------------------------------------------------------------------

/// Cortex-A15 32-bit guest target.
pub const KVM_ARM_TARGET_CORTEX_A15: u32 = 0;
/// Cortex-A7 32-bit guest target.
pub const KVM_ARM_TARGET_CORTEX_A7: u32 = 1;
/// Cortex-A53 64-bit guest target.
pub const KVM_ARM_TARGET_CORTEX_A53: u32 = 4;
/// Cortex-A57 64-bit guest target.
pub const KVM_ARM_TARGET_CORTEX_A57: u32 = 2;
/// Generic v8 64-bit target (default since 5.10).
pub const KVM_ARM_TARGET_GENERIC_V8: u32 = 5;
/// "Unknown CPU" sentinel returned when the host cannot match.
pub const KVM_ARM_TARGET_NONE: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Exception types passed in `kvm_vcpu_events`
// ---------------------------------------------------------------------------

/// Serror (SError interrupt) pending.
pub const KVM_ARM_VCPU_EXCEPT_AA64_ELx_SYNC: u32 = 0;
/// IRQ pending injection (asynchronous external abort).
pub const KVM_ARM_VCPU_EXCEPT_AA64_ELx_IRQ: u32 = 1;
/// FIQ pending injection.
pub const KVM_ARM_VCPU_EXCEPT_AA64_ELx_FIQ: u32 = 2;
/// SError pending injection.
pub const KVM_ARM_VCPU_EXCEPT_AA64_ELx_SERR: u32 = 3;

// ---------------------------------------------------------------------------
// GIC device-control groups (passed to KVM_DEV_ARM_VGIC_GRP_* ioctls)
// ---------------------------------------------------------------------------

/// GICv2 dist/cpu interface address.
pub const KVM_DEV_ARM_VGIC_GRP_ADDR: u32 = 0;
/// Register access — distributor/redistributor registers.
pub const KVM_DEV_ARM_VGIC_GRP_DIST_REGS: u32 = 1;
/// Per-CPU interface register access.
pub const KVM_DEV_ARM_VGIC_GRP_CPU_REGS: u32 = 2;
/// Number of IRQs supported.
pub const KVM_DEV_ARM_VGIC_GRP_NR_IRQS: u32 = 3;
/// vGIC control (init, save/restore signalling).
pub const KVM_DEV_ARM_VGIC_GRP_CTRL: u32 = 4;
/// Redistributor register access (GICv3).
pub const KVM_DEV_ARM_VGIC_GRP_REDIST_REGS: u32 = 5;
/// CPU interface system-register access (ICC_*).
pub const KVM_DEV_ARM_VGIC_GRP_CPU_SYSREGS: u32 = 6;
/// Hard-coded LPI translation tables.
pub const KVM_DEV_ARM_VGIC_GRP_LEVEL_INFO: u32 = 7;
/// ITS register block (GICv3 LPI).
pub const KVM_DEV_ARM_VGIC_GRP_ITS_REGS: u32 = 8;

// ---------------------------------------------------------------------------
// PSCI versions (returned by KVM_ARM_VCPU_PSCI_VERSION)
// ---------------------------------------------------------------------------

/// PSCI v0.2 (32-bit major.minor packed).
pub const KVM_ARM_PSCI_0_2: u32 = (0 << 16) | 2;
/// PSCI v1.0.
pub const KVM_ARM_PSCI_1_0: u32 = (1 << 16) | 0;
/// PSCI v1.1.
pub const KVM_ARM_PSCI_1_1: u32 = (1 << 16) | 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_bits_distinct() {
        let feats = [
            KVM_ARM_VCPU_POWER_OFF,
            KVM_ARM_VCPU_EL1_32BIT,
            KVM_ARM_VCPU_PMU_V3,
            KVM_ARM_VCPU_SVE,
            KVM_ARM_VCPU_PTRAUTH_ADDRESS,
            KVM_ARM_VCPU_PTRAUTH_GENERIC,
            KVM_ARM_VCPU_HAS_EL2,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
            // Bit indices, must be < 32 to fit in the kernel's u32 mask.
            assert!(feats[i] < 32);
        }
    }

    #[test]
    fn test_targets_distinct() {
        let targs = [
            KVM_ARM_TARGET_CORTEX_A15,
            KVM_ARM_TARGET_CORTEX_A7,
            KVM_ARM_TARGET_CORTEX_A53,
            KVM_ARM_TARGET_CORTEX_A57,
            KVM_ARM_TARGET_GENERIC_V8,
            KVM_ARM_TARGET_NONE,
        ];
        for i in 0..targs.len() {
            for j in (i + 1)..targs.len() {
                assert_ne!(targs[i], targs[j]);
            }
        }
        assert_eq!(KVM_ARM_TARGET_NONE, u32::MAX);
    }

    #[test]
    fn test_except_types_distinct() {
        let ex = [
            KVM_ARM_VCPU_EXCEPT_AA64_ELx_SYNC,
            KVM_ARM_VCPU_EXCEPT_AA64_ELx_IRQ,
            KVM_ARM_VCPU_EXCEPT_AA64_ELx_FIQ,
            KVM_ARM_VCPU_EXCEPT_AA64_ELx_SERR,
        ];
        for i in 0..ex.len() {
            for j in (i + 1)..ex.len() {
                assert_ne!(ex[i], ex[j]);
            }
        }
    }

    #[test]
    fn test_vgic_groups_distinct() {
        let g = [
            KVM_DEV_ARM_VGIC_GRP_ADDR,
            KVM_DEV_ARM_VGIC_GRP_DIST_REGS,
            KVM_DEV_ARM_VGIC_GRP_CPU_REGS,
            KVM_DEV_ARM_VGIC_GRP_NR_IRQS,
            KVM_DEV_ARM_VGIC_GRP_CTRL,
            KVM_DEV_ARM_VGIC_GRP_REDIST_REGS,
            KVM_DEV_ARM_VGIC_GRP_CPU_SYSREGS,
            KVM_DEV_ARM_VGIC_GRP_LEVEL_INFO,
            KVM_DEV_ARM_VGIC_GRP_ITS_REGS,
        ];
        for i in 0..g.len() {
            for j in (i + 1)..g.len() {
                assert_ne!(g[i], g[j]);
            }
        }
    }

    #[test]
    fn test_psci_versions_ordered() {
        // Encoding packs major in high 16 bits, minor in low 16 bits.
        // Newer versions must compare strictly greater.
        assert!(KVM_ARM_PSCI_0_2 < KVM_ARM_PSCI_1_0);
        assert!(KVM_ARM_PSCI_1_0 < KVM_ARM_PSCI_1_1);
        assert_eq!(KVM_ARM_PSCI_0_2 & 0xffff, 2);
        assert_eq!(KVM_ARM_PSCI_1_1 >> 16, 1);
    }
}
