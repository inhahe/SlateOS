//! `<linux/acrn.h>` — ACRN hypervisor userspace API.
//!
//! ACRN is an automotive/IoT-class type-1 hypervisor. Its
//! Service-VM userspace (acrn-dm device model, acrn-manager) talks
//! to the kernel-side ACRN HSM driver through `/dev/acrn_hsm`
//! using the ioctls below.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// ACRN ioctl group letter ('A').
pub const ACRN_IOCTL_TYPE: u8 = 0xA2;

// ---------------------------------------------------------------------------
// VM lifecycle ioctls
// ---------------------------------------------------------------------------

/// `ACRN_IOCTL_CREATE_VM` — create a guest VM.
pub const ACRN_IOCTL_CREATE_VM: u32 = 0xC020_A210;
/// `ACRN_IOCTL_DESTROY_VM` — destroy a guest VM.
pub const ACRN_IOCTL_DESTROY_VM: u32 = 0x0000_A211;
/// `ACRN_IOCTL_START_VM` — start a stopped VM.
pub const ACRN_IOCTL_START_VM: u32 = 0x0000_A212;
/// `ACRN_IOCTL_PAUSE_VM` — pause a running VM.
pub const ACRN_IOCTL_PAUSE_VM: u32 = 0x0000_A213;
/// `ACRN_IOCTL_RESET_VM` — reset a VM.
pub const ACRN_IOCTL_RESET_VM: u32 = 0x0000_A215;
/// `ACRN_IOCTL_SET_VCPU_REGS` — write a vCPU's registers.
pub const ACRN_IOCTL_SET_VCPU_REGS: u32 = 0x4400_A216;

// ---------------------------------------------------------------------------
// Memory-mapping ioctls
// ---------------------------------------------------------------------------

/// `ACRN_IOCTL_SET_MEMSEG` — map host memory into the guest.
pub const ACRN_IOCTL_SET_MEMSEG: u32 = 0x4028_A241;
/// `ACRN_IOCTL_UNSET_MEMSEG` — unmap a guest memory range.
pub const ACRN_IOCTL_UNSET_MEMSEG: u32 = 0x4028_A242;

// ---------------------------------------------------------------------------
// I/O request notification ioctls
// ---------------------------------------------------------------------------

/// `ACRN_IOCTL_SET_IOREQ_BUFFER` — bind a userspace I/O request buffer.
pub const ACRN_IOCTL_SET_IOREQ_BUFFER: u32 = 0x4008_A270;
/// `ACRN_IOCTL_NOTIFY_REQUEST_FINISH` — tell HV the I/O response is ready.
pub const ACRN_IOCTL_NOTIFY_REQUEST_FINISH: u32 = 0x4008_A271;
/// `ACRN_IOCTL_CREATE_IOREQ_CLIENT` — create an emulator client.
pub const ACRN_IOCTL_CREATE_IOREQ_CLIENT: u32 = 0x0000_A272;
/// `ACRN_IOCTL_DESTROY_IOREQ_CLIENT` — destroy an emulator client.
pub const ACRN_IOCTL_DESTROY_IOREQ_CLIENT: u32 = 0x0000_A273;

// ---------------------------------------------------------------------------
// IRQ ioctls
// ---------------------------------------------------------------------------

/// `ACRN_IOCTL_INJECT_MSI` — inject an MSI into the guest.
pub const ACRN_IOCTL_INJECT_MSI: u32 = 0x4010_A223;
/// `ACRN_IOCTL_VM_INTR_MONITOR` — install an IRQ monitor.
pub const ACRN_IOCTL_VM_INTR_MONITOR: u32 = 0x4008_A224;
/// `ACRN_IOCTL_SET_IRQLINE` — drive a virtual IRQ line.
pub const ACRN_IOCTL_SET_IRQLINE: u32 = 0x4008_A225;

// ---------------------------------------------------------------------------
// I/O request types (struct acrn_io_request.type)
// ---------------------------------------------------------------------------

/// Port I/O.
pub const ACRN_IOREQ_TYPE_PORTIO: u32 = 0;
/// Memory-mapped I/O.
pub const ACRN_IOREQ_TYPE_MMIO: u32 = 1;
/// PCI configuration cycle.
pub const ACRN_IOREQ_TYPE_PCICFG: u32 = 2;
/// WBINVD or related cache instruction.
pub const ACRN_IOREQ_TYPE_WP: u32 = 3;

// ---------------------------------------------------------------------------
// I/O request directions
// ---------------------------------------------------------------------------

/// Guest is reading from device.
pub const ACRN_IOREQ_DIR_READ: u32 = 0;
/// Guest is writing to device.
pub const ACRN_IOREQ_DIR_WRITE: u32 = 1;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of vCPUs per ACRN VM.
pub const ACRN_MAX_VCPU: u32 = 8;
/// Maximum number of memory segments per VM.
pub const ACRN_MAX_MEMSEG: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_group_letter() {
        // ACRN reserves group 0xA2 in the ioctl number space.
        assert_eq!(ACRN_IOCTL_TYPE, 0xA2);
    }

    #[test]
    fn test_lifecycle_ioctls_distinct_and_in_group() {
        let ops = [
            ACRN_IOCTL_CREATE_VM,
            ACRN_IOCTL_DESTROY_VM,
            ACRN_IOCTL_START_VM,
            ACRN_IOCTL_PAUSE_VM,
            ACRN_IOCTL_RESET_VM,
            ACRN_IOCTL_SET_VCPU_REGS,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // ACRN type byte in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, ACRN_IOCTL_TYPE as u32);
        }
    }

    #[test]
    fn test_memory_ioctls_distinct() {
        assert_ne!(ACRN_IOCTL_SET_MEMSEG, ACRN_IOCTL_UNSET_MEMSEG);
        assert_eq!((ACRN_IOCTL_SET_MEMSEG >> 8) & 0xff, 0xA2);
        assert_eq!((ACRN_IOCTL_UNSET_MEMSEG >> 8) & 0xff, 0xA2);
    }

    #[test]
    fn test_ioreq_ioctls_distinct() {
        let ops = [
            ACRN_IOCTL_SET_IOREQ_BUFFER,
            ACRN_IOCTL_NOTIFY_REQUEST_FINISH,
            ACRN_IOCTL_CREATE_IOREQ_CLIENT,
            ACRN_IOCTL_DESTROY_IOREQ_CLIENT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_irq_ioctls_distinct() {
        let ops = [
            ACRN_IOCTL_INJECT_MSI,
            ACRN_IOCTL_VM_INTR_MONITOR,
            ACRN_IOCTL_SET_IRQLINE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_ioreq_types_and_dir_dense() {
        let t = [
            ACRN_IOREQ_TYPE_PORTIO,
            ACRN_IOREQ_TYPE_MMIO,
            ACRN_IOREQ_TYPE_PCICFG,
            ACRN_IOREQ_TYPE_WP,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // READ=0 / WRITE=1 mirrors POSIX read(2)/write(2) semantics.
        assert_eq!(ACRN_IOREQ_DIR_READ, 0);
        assert_eq!(ACRN_IOREQ_DIR_WRITE, 1);
    }

    #[test]
    fn test_limits_pow2() {
        assert!(ACRN_MAX_VCPU.is_power_of_two());
        assert!(ACRN_MAX_MEMSEG.is_power_of_two());
    }
}
