//! `<linux/msi.h>` — Message-Signaled Interrupts userspace bindings.
//!
//! MSI/MSI-X is how every modern PCIe device delivers interrupts —
//! the device writes a 32-bit value to a memory-mapped address instead
//! of asserting a wire. VFIO exposes the per-device MSI vector table
//! to userspace so qemu/KVM can route guest interrupts directly to the
//! hardware. The constants below come from `<linux/msi.h>` and the
//! VFIO/IRQFD ABI.

// ---------------------------------------------------------------------------
// Per-device limits
// ---------------------------------------------------------------------------

/// Hardware maximum MSI vectors per function (32, single dword field).
pub const MSI_MAX_VECTORS: u32 = 32;
/// Hardware maximum MSI-X table entries per function (PCIe 5.0).
pub const MSIX_MAX_VECTORS: u32 = 2048;

/// MSI-X table entry size (4 × u32: addr_low, addr_high, data, control).
pub const MSIX_ENTRY_SIZE: usize = 16;
/// MSI-X PBA entry size (1 bit per vector, accessed in u64 chunks).
pub const MSIX_PBA_BITS_PER_ENTRY: usize = 64;

// ---------------------------------------------------------------------------
// MSI-X control register bits (PCIe spec)
// ---------------------------------------------------------------------------

/// Mask bit in the per-vector control word.
pub const MSIX_VEC_CTRL_MASKBIT: u32 = 0x0000_0001;

/// MSI-X table BIR (BAR Indicator Register) mask in MSI-X capability.
pub const MSIX_TABLE_BIR_MASK: u32 = 0x0000_0007;
/// MSI-X PBA BIR mask.
pub const MSIX_PBA_BIR_MASK: u32 = 0x0000_0007;
/// Offset mask (high bits, 32-byte aligned).
pub const MSIX_OFFSET_MASK: u32 = 0xFFFF_FFF8;

/// MSI-X global enable bit in the message-control field.
pub const MSIX_FLAGS_ENABLE: u16 = 1 << 15;
/// Function-mask bit in the message-control field.
pub const MSIX_FLAGS_MASKALL: u16 = 1 << 14;

// ---------------------------------------------------------------------------
// VFIO PCI MSI interrupt index (passed in `vfio_irq_info.index`)
// ---------------------------------------------------------------------------

pub const VFIO_PCI_INTX_IRQ_INDEX: u32 = 0;
pub const VFIO_PCI_MSI_IRQ_INDEX: u32 = 1;
pub const VFIO_PCI_MSIX_IRQ_INDEX: u32 = 2;
pub const VFIO_PCI_ERR_IRQ_INDEX: u32 = 3;
pub const VFIO_PCI_REQ_IRQ_INDEX: u32 = 4;
pub const VFIO_PCI_NUM_IRQS: u32 = 5;

// ---------------------------------------------------------------------------
// `KVM_IRQ_ROUTING_MSI` constants
// ---------------------------------------------------------------------------

pub const KVM_IRQ_ROUTING_IRQCHIP: u32 = 1;
pub const KVM_IRQ_ROUTING_MSI: u32 = 2;
pub const KVM_IRQ_ROUTING_S390_ADAPTER: u32 = 3;
pub const KVM_IRQ_ROUTING_HV_SINT: u32 = 4;
pub const KVM_IRQ_ROUTING_XEN_EVTCHN: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_caps() {
        // MSI: 32 vectors max per function.
        assert_eq!(MSI_MAX_VECTORS, 32);
        assert!(MSI_MAX_VECTORS.is_power_of_two());
        // MSI-X: 2048 vectors max per function.
        assert_eq!(MSIX_MAX_VECTORS, 2048);
        assert!(MSIX_MAX_VECTORS.is_power_of_two());
        assert!(MSIX_MAX_VECTORS > MSI_MAX_VECTORS);
    }

    #[test]
    fn test_table_entry_layout() {
        // 4 dwords per table entry.
        assert_eq!(MSIX_ENTRY_SIZE, 16);
        // PBA accessed in u64 chunks per the PCIe spec.
        assert_eq!(MSIX_PBA_BITS_PER_ENTRY, 64);
    }

    #[test]
    fn test_msix_control_bits() {
        assert!(MSIX_VEC_CTRL_MASKBIT.is_power_of_two());
        // BIR field is the low 3 bits.
        assert_eq!(MSIX_TABLE_BIR_MASK, 7);
        assert_eq!(MSIX_PBA_BIR_MASK, 7);
        // Offset and BIR cover the entire 32-bit field (8-byte alignment).
        assert_eq!(MSIX_OFFSET_MASK | MSIX_TABLE_BIR_MASK, 0xFFFF_FFFF);
        // ENABLE is bit 15, MASKALL bit 14.
        assert_eq!(MSIX_FLAGS_ENABLE, 0x8000);
        assert_eq!(MSIX_FLAGS_MASKALL, 0x4000);
    }

    #[test]
    fn test_vfio_pci_irq_indices_dense_0_to_4() {
        let v = [
            VFIO_PCI_INTX_IRQ_INDEX,
            VFIO_PCI_MSI_IRQ_INDEX,
            VFIO_PCI_MSIX_IRQ_INDEX,
            VFIO_PCI_ERR_IRQ_INDEX,
            VFIO_PCI_REQ_IRQ_INDEX,
        ];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
        assert_eq!(VFIO_PCI_NUM_IRQS, 5);
    }

    #[test]
    fn test_kvm_routing_types_distinct() {
        let r = [
            KVM_IRQ_ROUTING_IRQCHIP,
            KVM_IRQ_ROUTING_MSI,
            KVM_IRQ_ROUTING_S390_ADAPTER,
            KVM_IRQ_ROUTING_HV_SINT,
            KVM_IRQ_ROUTING_XEN_EVTCHN,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }
}
