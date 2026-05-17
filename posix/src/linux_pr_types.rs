//! `<linux/pr.h>` — Persistent Reservations (SCSI PR) constants.
//!
//! Persistent Reservations allow multiple hosts (in a cluster) to
//! coordinate access to shared storage devices. A host can reserve
//! a LUN exclusively or share it with specific other hosts. PRs
//! survive host reboots and path failovers (unlike SCSI-2 Reserve).
//! Used by cluster filesystems (GFS2, OCFS2), HA databases, and
//! VM live migration with shared storage.

// ---------------------------------------------------------------------------
// PR IOCTLs
// ---------------------------------------------------------------------------

/// Register a PR key (establish presence on the device).
pub const IOC_PR_REGISTER: u32 = 0x01;
/// Reserve the device with a specific type.
pub const IOC_PR_RESERVE: u32 = 0x02;
/// Release a reservation.
pub const IOC_PR_RELEASE: u32 = 0x03;
/// Preempt another host's reservation.
pub const IOC_PR_PREEMPT: u32 = 0x04;
/// Preempt and abort (forcefully remove another host).
pub const IOC_PR_PREEMPT_ABORT: u32 = 0x05;
/// Clear all registrations and reservations.
pub const IOC_PR_CLEAR: u32 = 0x06;

// ---------------------------------------------------------------------------
// PR reservation types
// ---------------------------------------------------------------------------

/// Write Exclusive (only holder can write).
pub const PR_TYPE_WRITE_EXCLUSIVE: u32 = 1;
/// Exclusive Access (only holder can read or write).
pub const PR_TYPE_EXCLUSIVE_ACCESS: u32 = 3;
/// Write Exclusive — Registrants Only (registered hosts can read, holder writes).
pub const PR_TYPE_WRITE_EXCLUSIVE_REG_ONLY: u32 = 5;
/// Exclusive Access — Registrants Only (only registered hosts access).
pub const PR_TYPE_EXCLUSIVE_ACCESS_REG_ONLY: u32 = 6;
/// Write Exclusive — All Registrants (all registered can write).
pub const PR_TYPE_WRITE_EXCLUSIVE_ALL_REGS: u32 = 7;
/// Exclusive Access — All Registrants.
pub const PR_TYPE_EXCLUSIVE_ACCESS_ALL_REGS: u32 = 8;

// ---------------------------------------------------------------------------
// PR flags
// ---------------------------------------------------------------------------

/// Ignore existing key (for register).
pub const PR_FL_IGNORE_KEY: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// PR status (reservation holder info)
// ---------------------------------------------------------------------------

/// No reservation held.
pub const PR_STATUS_NONE: u32 = 0;
/// Reservation is held by this host.
pub const PR_STATUS_HELD: u32 = 1;
/// Reservation is held by another host.
pub const PR_STATUS_OTHER: u32 = 2;

// ---------------------------------------------------------------------------
// PR scope
// ---------------------------------------------------------------------------

/// LU (Logical Unit) scope — entire device.
pub const PR_SCOPE_LU: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            IOC_PR_REGISTER, IOC_PR_RESERVE, IOC_PR_RELEASE,
            IOC_PR_PREEMPT, IOC_PR_PREEMPT_ABORT, IOC_PR_CLEAR,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_reservation_types_distinct() {
        let types = [
            PR_TYPE_WRITE_EXCLUSIVE, PR_TYPE_EXCLUSIVE_ACCESS,
            PR_TYPE_WRITE_EXCLUSIVE_REG_ONLY,
            PR_TYPE_EXCLUSIVE_ACCESS_REG_ONLY,
            PR_TYPE_WRITE_EXCLUSIVE_ALL_REGS,
            PR_TYPE_EXCLUSIVE_ACCESS_ALL_REGS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_status_distinct() {
        let statuses = [PR_STATUS_NONE, PR_STATUS_HELD, PR_STATUS_OTHER];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_flag_value() {
        assert_eq!(PR_FL_IGNORE_KEY, 1);
        assert!(PR_FL_IGNORE_KEY.is_power_of_two());
    }
}
