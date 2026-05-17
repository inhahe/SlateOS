//! `<linux/irqdomain.h>` — IRQ domain/mapping constants.
//!
//! IRQ domains provide a mapping between hardware interrupt numbers
//! and Linux IRQ numbers. Each interrupt controller (PIC, IOAPIC,
//! GIC, MSI) gets its own domain. When a device specifies its
//! interrupt (via device tree, ACPI, or PCI), the domain translates
//! it to a Linux virq (virtual IRQ number). This abstraction allows
//! multiple interrupt controllers to coexist and enables hierarchical
//! interrupt routing.

// ---------------------------------------------------------------------------
// IRQ domain flags
// ---------------------------------------------------------------------------

/// Domain is hierarchical (has parent domain).
pub const IRQ_DOMAIN_FLAG_HIERARCHY: u32 = 0x0001;
/// Domain handles MSI (Message Signaled Interrupts).
pub const IRQ_DOMAIN_FLAG_MSI: u32 = 0x0002;
/// Domain handles MSI-X (extended MSI).
pub const IRQ_DOMAIN_FLAG_MSI_REMAP: u32 = 0x0004;
/// Domain automatically translates from firmware to Linux IRQs.
pub const IRQ_DOMAIN_FLAG_AUTO_TRANSLATE: u32 = 0x0008;
/// Domain uses IPI (inter-processor interrupt) delivery.
pub const IRQ_DOMAIN_FLAG_IPI: u32 = 0x0010;
/// Domain is non-mappable (internal only).
pub const IRQ_DOMAIN_FLAG_NONCORE: u32 = 0x0020;

// ---------------------------------------------------------------------------
// IRQ domain mapping types
// ---------------------------------------------------------------------------

/// Linear mapping (array indexed by hardware IRQ).
pub const IRQ_DOMAIN_MAP_LINEAR: u32 = 0;
/// Tree mapping (radix tree for sparse IRQ spaces).
pub const IRQ_DOMAIN_MAP_TREE: u32 = 1;
/// No mapping (1:1 hardware to Linux IRQ).
pub const IRQ_DOMAIN_MAP_NOMAP: u32 = 2;

// ---------------------------------------------------------------------------
// Hardware IRQ trigger types
// ---------------------------------------------------------------------------

/// Edge-triggered interrupt.
pub const IRQ_TYPE_EDGE_RISING: u32 = 0x01;
/// Falling edge triggered.
pub const IRQ_TYPE_EDGE_FALLING: u32 = 0x02;
/// Both edges triggered.
pub const IRQ_TYPE_EDGE_BOTH: u32 = 0x03;
/// Level-triggered high.
pub const IRQ_TYPE_LEVEL_HIGH: u32 = 0x04;
/// Level-triggered low.
pub const IRQ_TYPE_LEVEL_LOW: u32 = 0x08;
/// No specific trigger type.
pub const IRQ_TYPE_NONE: u32 = 0x00;

// ---------------------------------------------------------------------------
// IRQ allocation flags
// ---------------------------------------------------------------------------

/// Allocate a specific IRQ number.
pub const IRQ_ALLOC_SPECIFIC: u32 = 0x01;
/// Allocate from a contiguous range.
pub const IRQ_ALLOC_CONTIGUOUS: u32 = 0x02;
/// Activate IRQ immediately after allocation.
pub const IRQ_ALLOC_ACTIVATE: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_flags_no_overlap() {
        let flags = [
            IRQ_DOMAIN_FLAG_HIERARCHY, IRQ_DOMAIN_FLAG_MSI,
            IRQ_DOMAIN_FLAG_MSI_REMAP, IRQ_DOMAIN_FLAG_AUTO_TRANSLATE,
            IRQ_DOMAIN_FLAG_IPI, IRQ_DOMAIN_FLAG_NONCORE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_map_types_distinct() {
        let types = [
            IRQ_DOMAIN_MAP_LINEAR, IRQ_DOMAIN_MAP_TREE,
            IRQ_DOMAIN_MAP_NOMAP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_trigger_types() {
        // Edge both is OR of rising and falling
        assert_eq!(IRQ_TYPE_EDGE_BOTH, IRQ_TYPE_EDGE_RISING | IRQ_TYPE_EDGE_FALLING);
        // Level high and low don't overlap with edge
        assert_eq!(IRQ_TYPE_LEVEL_HIGH & IRQ_TYPE_EDGE_BOTH, 0);
    }

    #[test]
    fn test_alloc_flags_no_overlap() {
        let flags = [IRQ_ALLOC_SPECIFIC, IRQ_ALLOC_CONTIGUOUS, IRQ_ALLOC_ACTIVATE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
