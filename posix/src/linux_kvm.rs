//! `<linux/kvm.h>` — Kernel Virtual Machine ioctls.
//!
//! KVM is Linux's hardware-assisted virtualization API. These ioctls
//! are used to create and manage virtual machines via `/dev/kvm`.

// ---------------------------------------------------------------------------
// System ioctls (/dev/kvm fd)
// ---------------------------------------------------------------------------

/// Get KVM API version (returns 12).
pub const KVM_GET_API_VERSION: u64 = 0xAE00;
/// Create a virtual machine.
pub const KVM_CREATE_VM: u64 = 0xAE01;
/// Check extension support.
pub const KVM_CHECK_EXTENSION: u64 = 0xAE03;
/// Get vCPU mmap size.
pub const KVM_GET_VCPU_MMAP_SIZE: u64 = 0xAE04;
/// Get supported CPUIDs.
pub const KVM_GET_SUPPORTED_CPUID: u64 = 0xC008AE05;
/// Get MSR index list.
pub const KVM_GET_MSR_INDEX_LIST: u64 = 0xC004AE02;

// ---------------------------------------------------------------------------
// VM ioctls (VM fd)
// ---------------------------------------------------------------------------

/// Create a vCPU.
pub const KVM_CREATE_VCPU: u64 = 0xAE41;
/// Set user memory region.
pub const KVM_SET_USER_MEMORY_REGION: u64 = 0x4020AE46;
/// Create IRQ chip.
pub const KVM_CREATE_IRQCHIP: u64 = 0xAE60;
/// Set IRQ line level.
pub const KVM_IRQ_LINE: u64 = 0x4008AE61;
/// Create PIT (timer).
pub const KVM_CREATE_PIT2: u64 = 0x4040AE77;
/// Get clock.
pub const KVM_GET_CLOCK: u64 = 0x8030AE7C;
/// Set clock.
pub const KVM_SET_CLOCK: u64 = 0x4030AE7B;
/// Set TSS address.
pub const KVM_SET_TSS_ADDR: u64 = 0xAE47;
/// Set identity map address.
pub const KVM_SET_IDENTITY_MAP_ADDR: u64 = 0x4008AE48;
/// Create device.
pub const KVM_CREATE_DEVICE: u64 = 0xC00CAEE0;
/// Signal MSI.
pub const KVM_SIGNAL_MSI: u64 = 0x4020AEA5;

// ---------------------------------------------------------------------------
// vCPU ioctls (vCPU fd)
// ---------------------------------------------------------------------------

/// Run the vCPU.
pub const KVM_RUN: u64 = 0xAE80;
/// Get registers.
pub const KVM_GET_REGS: u64 = 0x8090AE81;
/// Set registers.
pub const KVM_SET_REGS: u64 = 0x4090AE82;
/// Get special registers.
pub const KVM_GET_SREGS: u64 = 0x8138AE83;
/// Set special registers.
pub const KVM_SET_SREGS: u64 = 0x4138AE84;
/// Get MSRs.
pub const KVM_GET_MSRS: u64 = 0xC008AE88;
/// Set MSRs.
pub const KVM_SET_MSRS: u64 = 0x4008AE89;
/// Set CPUID entries.
pub const KVM_SET_CPUID2: u64 = 0x4008AE90;
/// Get FPU state.
pub const KVM_GET_FPU: u64 = 0x81A0AE8C;
/// Set FPU state.
pub const KVM_SET_FPU: u64 = 0x41A0AE8D;
/// Interrupt the vCPU.
pub const KVM_INTERRUPT: u64 = 0x4004AE86;

// ---------------------------------------------------------------------------
// KVM exit reasons (kvm_run.exit_reason)
// ---------------------------------------------------------------------------

/// Hardware exit (VMX/SVM exit).
pub const KVM_EXIT_UNKNOWN: u32 = 0;
/// Exception or NMI.
pub const KVM_EXIT_EXCEPTION: u32 = 1;
/// I/O instruction (in/out).
pub const KVM_EXIT_IO: u32 = 2;
/// Hypercall.
pub const KVM_EXIT_HYPERCALL: u32 = 3;
/// Debug event.
pub const KVM_EXIT_DEBUG: u32 = 4;
/// HLT instruction.
pub const KVM_EXIT_HLT: u32 = 5;
/// MMIO access.
pub const KVM_EXIT_MMIO: u32 = 6;
/// IRQ window.
pub const KVM_EXIT_IRQ_WINDOW_OPEN: u32 = 7;
/// Shutdown.
pub const KVM_EXIT_SHUTDOWN: u32 = 8;
/// Entry failure.
pub const KVM_EXIT_FAIL_ENTRY: u32 = 9;
/// Internal error.
pub const KVM_EXIT_INTERNAL_ERROR: u32 = 17;
/// System event (reset, shutdown).
pub const KVM_EXIT_SYSTEM_EVENT: u32 = 24;
/// IOAPIC EOI.
pub const KVM_EXIT_IOAPIC_EOI: u32 = 26;

// ---------------------------------------------------------------------------
// KVM API version
// ---------------------------------------------------------------------------

/// Expected KVM API version.
pub const KVM_API_VERSION: i32 = 12;

// ---------------------------------------------------------------------------
// KVM memory flags
// ---------------------------------------------------------------------------

/// Memory region is read-only.
pub const KVM_MEM_READONLY: u32 = 1 << 1;
/// Log dirty pages.
pub const KVM_MEM_LOG_DIRTY_PAGES: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_version() {
        assert_eq!(KVM_API_VERSION, 12);
    }

    #[test]
    fn test_system_ioctls_distinct() {
        let cmds = [
            KVM_GET_API_VERSION,
            KVM_CREATE_VM,
            KVM_CHECK_EXTENSION,
            KVM_GET_VCPU_MMAP_SIZE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_vcpu_ioctls_distinct() {
        let cmds = [
            KVM_RUN,
            KVM_GET_REGS,
            KVM_SET_REGS,
            KVM_GET_SREGS,
            KVM_SET_SREGS,
            KVM_GET_MSRS,
            KVM_SET_MSRS,
            KVM_GET_FPU,
            KVM_SET_FPU,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_exit_reasons_distinct() {
        let exits = [
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
            KVM_EXIT_INTERNAL_ERROR,
            KVM_EXIT_SYSTEM_EVENT,
        ];
        for i in 0..exits.len() {
            for j in (i + 1)..exits.len() {
                assert_ne!(exits[i], exits[j]);
            }
        }
    }

    #[test]
    fn test_memory_flags() {
        assert_eq!(KVM_MEM_LOG_DIRTY_PAGES, 1);
        assert_eq!(KVM_MEM_READONLY, 2);
        assert_eq!(KVM_MEM_LOG_DIRTY_PAGES & KVM_MEM_READONLY, 0);
    }
}
