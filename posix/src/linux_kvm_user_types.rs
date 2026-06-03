//! `<linux/kvm.h>` — KVM hypervisor user ABI.
//!
//! QEMU/KVM, crosvm, Firecracker, kvmtool, and every cloud-hypervisor
//! style VMM open `/dev/kvm`, issue `KVM_CREATE_VM`, then a series of
//! `KVM_CREATE_VCPU` calls, then drive each vCPU with `KVM_RUN`.
//! The constants below are the bedrock of that protocol.

// ---------------------------------------------------------------------------
// Device path
// ---------------------------------------------------------------------------

/// Character device every KVM userspace opens first.
pub const KVM_DEV_PATH: &str = "/dev/kvm";

// ---------------------------------------------------------------------------
// System-fd ioctls (issued on /dev/kvm)
// ---------------------------------------------------------------------------

pub const KVM_GET_API_VERSION: u32 = 0xAE00;
pub const KVM_CREATE_VM: u32 = 0xAE01;
pub const KVM_GET_MSR_INDEX_LIST: u32 = 0xC004_AE02;
pub const KVM_CHECK_EXTENSION: u32 = 0xAE03;
pub const KVM_GET_VCPU_MMAP_SIZE: u32 = 0xAE04;
pub const KVM_GET_SUPPORTED_CPUID: u32 = 0xC008_AE05;

// ---------------------------------------------------------------------------
// VM-fd ioctls
// ---------------------------------------------------------------------------

pub const KVM_CREATE_VCPU: u32 = 0xAE41;
pub const KVM_GET_DIRTY_LOG: u32 = 0x4010_AE42;
pub const KVM_SET_NR_MMU_PAGES: u32 = 0xAE44;
pub const KVM_GET_NR_MMU_PAGES: u32 = 0xAE45;
pub const KVM_SET_USER_MEMORY_REGION: u32 = 0x4020_AE46;
pub const KVM_SET_TSS_ADDR: u32 = 0xAE47;
pub const KVM_SET_IDENTITY_MAP_ADDR: u32 = 0x4008_AE48;
pub const KVM_CREATE_IRQCHIP: u32 = 0xAE60;
pub const KVM_IRQ_LINE: u32 = 0x4008_AE61;
pub const KVM_GET_IRQCHIP: u32 = 0xC208_AE62;
pub const KVM_SET_IRQCHIP: u32 = 0x8208_AE63;
pub const KVM_CREATE_PIT2: u32 = 0x4040_AE77;

// ---------------------------------------------------------------------------
// VCPU-fd ioctls
// ---------------------------------------------------------------------------

pub const KVM_RUN: u32 = 0xAE80;
pub const KVM_GET_REGS: u32 = 0x8090_AE81;
pub const KVM_SET_REGS: u32 = 0x4090_AE82;
pub const KVM_GET_SREGS: u32 = 0x8138_AE83;
pub const KVM_SET_SREGS: u32 = 0x4138_AE84;
pub const KVM_TRANSLATE: u32 = 0xC018_AE85;
pub const KVM_INTERRUPT: u32 = 0x4004_AE86;

// ---------------------------------------------------------------------------
// `kvm_run.exit_reason` codes
// ---------------------------------------------------------------------------

pub const KVM_EXIT_UNKNOWN: u32 = 0;
pub const KVM_EXIT_EXCEPTION: u32 = 1;
pub const KVM_EXIT_IO: u32 = 2;
pub const KVM_EXIT_HYPERCALL: u32 = 3;
pub const KVM_EXIT_DEBUG: u32 = 4;
pub const KVM_EXIT_HLT: u32 = 5;
pub const KVM_EXIT_MMIO: u32 = 6;
pub const KVM_EXIT_IRQ_WINDOW_OPEN: u32 = 7;
pub const KVM_EXIT_SHUTDOWN: u32 = 8;
pub const KVM_EXIT_FAIL_ENTRY: u32 = 9;
pub const KVM_EXIT_INTR: u32 = 10;
pub const KVM_EXIT_SET_TPR: u32 = 11;
pub const KVM_EXIT_TPR_ACCESS: u32 = 12;
pub const KVM_EXIT_S390_SIEIC: u32 = 13;
pub const KVM_EXIT_S390_RESET: u32 = 14;
pub const KVM_EXIT_DCR: u32 = 15;
pub const KVM_EXIT_NMI: u32 = 16;
pub const KVM_EXIT_INTERNAL_ERROR: u32 = 17;

// ---------------------------------------------------------------------------
// API version baseline
// ---------------------------------------------------------------------------

/// KVM API version (stable since 2.6.20).
pub const KVM_API_VERSION: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_path() {
        assert_eq!(KVM_DEV_PATH, "/dev/kvm");
    }

    #[test]
    fn test_system_ioctls_use_ae_magic() {
        // KVM ioctls use the magic byte 0xAE.
        for c in [
            KVM_GET_API_VERSION,
            KVM_CREATE_VM,
            KVM_CHECK_EXTENSION,
            KVM_GET_VCPU_MMAP_SIZE,
        ] {
            assert_eq!((c >> 8) & 0xFF, 0xAE);
        }
    }

    #[test]
    fn test_vm_ioctls_use_ae_magic() {
        for c in [
            KVM_CREATE_VCPU,
            KVM_SET_NR_MMU_PAGES,
            KVM_GET_NR_MMU_PAGES,
            KVM_SET_TSS_ADDR,
            KVM_CREATE_IRQCHIP,
        ] {
            assert_eq!((c >> 8) & 0xFF, 0xAE);
        }
    }

    #[test]
    fn test_vcpu_ioctls_use_ae_magic() {
        for c in [
            KVM_RUN,
            KVM_GET_REGS,
            KVM_SET_REGS,
            KVM_GET_SREGS,
            KVM_SET_SREGS,
            KVM_TRANSLATE,
            KVM_INTERRUPT,
        ] {
            assert_eq!((c >> 8) & 0xFF, 0xAE);
        }
    }

    #[test]
    fn test_exit_reasons_dense_0_to_17() {
        let e = [
            KVM_EXIT_UNKNOWN,
            KVM_EXIT_EXCEPTION,
            KVM_EXIT_IO,
            KVM_EXIT_HYPERCALL,
            KVM_EXIT_DEBUG,
            KVM_EXIT_HLT,
            KVM_EXIT_MMIO,
            KVM_EXIT_IRQ_WINDOW_OPEN,
            KVM_EXIT_SHUTDOWN,
            KVM_EXIT_FAIL_ENTRY,
            KVM_EXIT_INTR,
            KVM_EXIT_SET_TPR,
            KVM_EXIT_TPR_ACCESS,
            KVM_EXIT_S390_SIEIC,
            KVM_EXIT_S390_RESET,
            KVM_EXIT_DCR,
            KVM_EXIT_NMI,
            KVM_EXIT_INTERNAL_ERROR,
        ];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_api_version_is_12() {
        // KVM_GET_API_VERSION has returned 12 since Linux 2.6.20 (2007).
        assert_eq!(KVM_API_VERSION, 12);
    }
}
