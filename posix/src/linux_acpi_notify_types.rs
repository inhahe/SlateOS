//! `<linux/acpi.h>` — ACPI system-level notification constants.
//!
//! Beyond device-specific events, ACPI provides system-wide
//! notifications for power state transitions, processor performance
//! changes, memory/device hot-add events, and global system events
//! that affect multiple subsystems.

// ---------------------------------------------------------------------------
// System notification types (GPE-based)
// ---------------------------------------------------------------------------

/// System power state change notification.
pub const ACPI_SYSTEM_NOTIFY_POWER: u32 = 0x00;
/// System thermal notification.
pub const ACPI_SYSTEM_NOTIFY_THERMAL: u32 = 0x01;
/// System memory change (hot-add/remove).
pub const ACPI_SYSTEM_NOTIFY_MEMORY: u32 = 0x02;
/// System processor change (hot-add/remove).
pub const ACPI_SYSTEM_NOTIFY_PROCESSOR: u32 = 0x03;
/// System docking event.
pub const ACPI_SYSTEM_NOTIFY_DOCK: u32 = 0x04;

// ---------------------------------------------------------------------------
// GPE (General Purpose Event) types
// ---------------------------------------------------------------------------

/// Edge-triggered GPE.
pub const ACPI_GPE_EDGE_TRIGGERED: u32 = 0;
/// Level-triggered GPE.
pub const ACPI_GPE_LEVEL_TRIGGERED: u32 = 1;

/// GPE dispatch type: not used.
pub const ACPI_GPE_DISPATCH_NONE: u32 = 0;
/// GPE dispatch type: handler function.
pub const ACPI_GPE_DISPATCH_HANDLER: u32 = 1;
/// GPE dispatch type: AML method (_Lxx/_Exx).
pub const ACPI_GPE_DISPATCH_METHOD: u32 = 2;
/// GPE dispatch type: notify list.
pub const ACPI_GPE_DISPATCH_NOTIFY: u32 = 3;

// ---------------------------------------------------------------------------
// ACPI global lock states
// ---------------------------------------------------------------------------

/// Global lock is free.
pub const ACPI_GLOCK_FREE: u32 = 0;
/// Global lock is owned by OSPM.
pub const ACPI_GLOCK_OWNED: u32 = 1;
/// Global lock is pending (firmware waiting).
pub const ACPI_GLOCK_PENDING: u32 = 2;

// ---------------------------------------------------------------------------
// ACPI address space IDs
// ---------------------------------------------------------------------------

/// System memory address space.
pub const ACPI_ADR_SPACE_SYSTEM_MEMORY: u8 = 0;
/// System I/O address space.
pub const ACPI_ADR_SPACE_SYSTEM_IO: u8 = 1;
/// PCI configuration space.
pub const ACPI_ADR_SPACE_PCI_CONFIG: u8 = 2;
/// Embedded controller address space.
pub const ACPI_ADR_SPACE_EC: u8 = 3;
/// SMBus address space.
pub const ACPI_ADR_SPACE_SMBUS: u8 = 4;
/// CMOS address space.
pub const ACPI_ADR_SPACE_CMOS: u8 = 5;
/// PCI BAR target address space.
pub const ACPI_ADR_SPACE_PCI_BAR_TARGET: u8 = 6;
/// IPMI address space.
pub const ACPI_ADR_SPACE_IPMI: u8 = 7;
/// GPIO address space.
pub const ACPI_ADR_SPACE_GPIO: u8 = 8;
/// Generic serial bus address space.
pub const ACPI_ADR_SPACE_GSBUS: u8 = 9;
/// Platform communications channel.
pub const ACPI_ADR_SPACE_PCC: u8 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_notify_distinct() {
        let notifs = [
            ACPI_SYSTEM_NOTIFY_POWER, ACPI_SYSTEM_NOTIFY_THERMAL,
            ACPI_SYSTEM_NOTIFY_MEMORY, ACPI_SYSTEM_NOTIFY_PROCESSOR,
            ACPI_SYSTEM_NOTIFY_DOCK,
        ];
        for i in 0..notifs.len() {
            for j in (i + 1)..notifs.len() {
                assert_ne!(notifs[i], notifs[j]);
            }
        }
    }

    #[test]
    fn test_gpe_trigger_types() {
        assert_ne!(ACPI_GPE_EDGE_TRIGGERED, ACPI_GPE_LEVEL_TRIGGERED);
    }

    #[test]
    fn test_gpe_dispatch_distinct() {
        let disps = [
            ACPI_GPE_DISPATCH_NONE, ACPI_GPE_DISPATCH_HANDLER,
            ACPI_GPE_DISPATCH_METHOD, ACPI_GPE_DISPATCH_NOTIFY,
        ];
        for i in 0..disps.len() {
            for j in (i + 1)..disps.len() {
                assert_ne!(disps[i], disps[j]);
            }
        }
    }

    #[test]
    fn test_glock_states_distinct() {
        assert_ne!(ACPI_GLOCK_FREE, ACPI_GLOCK_OWNED);
        assert_ne!(ACPI_GLOCK_OWNED, ACPI_GLOCK_PENDING);
        assert_ne!(ACPI_GLOCK_FREE, ACPI_GLOCK_PENDING);
    }

    #[test]
    fn test_address_space_ids_sequential() {
        assert_eq!(ACPI_ADR_SPACE_SYSTEM_MEMORY, 0);
        assert_eq!(ACPI_ADR_SPACE_PCC, 10);
    }

    #[test]
    fn test_address_space_ids_distinct() {
        let spaces: [u8; 11] = [
            ACPI_ADR_SPACE_SYSTEM_MEMORY, ACPI_ADR_SPACE_SYSTEM_IO,
            ACPI_ADR_SPACE_PCI_CONFIG, ACPI_ADR_SPACE_EC,
            ACPI_ADR_SPACE_SMBUS, ACPI_ADR_SPACE_CMOS,
            ACPI_ADR_SPACE_PCI_BAR_TARGET, ACPI_ADR_SPACE_IPMI,
            ACPI_ADR_SPACE_GPIO, ACPI_ADR_SPACE_GSBUS,
            ACPI_ADR_SPACE_PCC,
        ];
        for i in 0..spaces.len() {
            for j in (i + 1)..spaces.len() {
                assert_ne!(spaces[i], spaces[j]);
            }
        }
    }
}
