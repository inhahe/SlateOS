//! `<linux/kvm.h>` — Kernel Virtual Machine (KVM) constants.
//!
//! KVM is the Linux kernel's hardware virtualization infrastructure.
//! It exposes /dev/kvm for creating VMs, vCPUs, and managing memory
//! regions. These constants define exit reasons, capability IDs,
//! and memory slot flags.

// ---------------------------------------------------------------------------
// KVM exit reasons
// ---------------------------------------------------------------------------

/// Unknown exit.
pub const KVM_EXIT_UNKNOWN: u32 = 0;
/// MMIO access (needs emulation).
pub const KVM_EXIT_MMIO: u32 = 6;
/// I/O port access.
pub const KVM_EXIT_IO: u32 = 2;
/// Hypercall (vmcall/vmmcall).
pub const KVM_EXIT_HYPERCALL: u32 = 3;
/// Debug exception.
pub const KVM_EXIT_DEBUG: u32 = 4;
/// HLT instruction.
pub const KVM_EXIT_HLT: u32 = 5;
/// IRQ window open.
pub const KVM_EXIT_IRQ_WINDOW_OPEN: u32 = 7;
/// Shutdown (triple fault).
pub const KVM_EXIT_SHUTDOWN: u32 = 8;
/// Entry failure.
pub const KVM_EXIT_FAIL_ENTRY: u32 = 9;
/// Internal error.
pub const KVM_EXIT_INTERNAL_ERROR: u32 = 17;
/// System event (reset/shutdown request).
pub const KVM_EXIT_SYSTEM_EVENT: u32 = 24;
/// I/O in direction.
pub const KVM_EXIT_IO_IN: u8 = 0;
/// I/O out direction.
pub const KVM_EXIT_IO_OUT: u8 = 1;

// ---------------------------------------------------------------------------
// KVM capabilities
// ---------------------------------------------------------------------------

/// Extended CPUID support.
pub const KVM_CAP_EXT_CPUID: u32 = 7;
/// IRQ chip in-kernel.
pub const KVM_CAP_IRQCHIP: u32 = 0;
/// Hardware-assisted memory isolation.
pub const KVM_CAP_HLT: u32 = 1;
/// User memory regions.
pub const KVM_CAP_USER_MEMORY: u32 = 3;
/// MSI injection.
pub const KVM_CAP_SIGNAL_MSI: u32 = 77;
/// Multiple address spaces.
pub const KVM_CAP_MULTI_ADDRESS_SPACE: u32 = 118;
/// Dirty page ring.
pub const KVM_CAP_DIRTY_LOG_RING: u32 = 192;

// ---------------------------------------------------------------------------
// Memory region flags
// ---------------------------------------------------------------------------

/// Region is read-only.
pub const KVM_MEM_READONLY: u32 = 1 << 1;
/// Region generates logs (dirty page tracking).
pub const KVM_MEM_LOG_DIRTY_PAGES: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// vCPU states
// ---------------------------------------------------------------------------

/// vCPU is runnable.
pub const KVM_MP_STATE_RUNNABLE: u32 = 0;
/// vCPU is uninitialized.
pub const KVM_MP_STATE_UNINITIALIZED: u32 = 1;
/// vCPU waiting for INIT.
pub const KVM_MP_STATE_INIT_RECEIVED: u32 = 2;
/// vCPU halted.
pub const KVM_MP_STATE_HALTED: u32 = 3;
/// vCPU waiting for SIPI.
pub const KVM_MP_STATE_SIPI_RECEIVED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_reasons_distinct() {
        let exits: [u32; 11] = [
            KVM_EXIT_UNKNOWN,
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
    fn test_io_directions_distinct() {
        assert_ne!(KVM_EXIT_IO_IN, KVM_EXIT_IO_OUT);
    }

    #[test]
    fn test_mem_flags_no_overlap() {
        assert_eq!(KVM_MEM_READONLY & KVM_MEM_LOG_DIRTY_PAGES, 0);
    }

    #[test]
    fn test_mp_states_distinct() {
        let states = [
            KVM_MP_STATE_RUNNABLE,
            KVM_MP_STATE_UNINITIALIZED,
            KVM_MP_STATE_INIT_RECEIVED,
            KVM_MP_STATE_HALTED,
            KVM_MP_STATE_SIPI_RECEIVED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
