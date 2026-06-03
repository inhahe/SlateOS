//! `<linux/dvb/ca.h>` — DVB Conditional Access userspace API.
//!
//! Linux DVB exposes Conditional-Access modules (smartcards used
//! for pay-TV decryption) at `/dev/dvb/adapterN/caM`. dvb-apps
//! (`ca_zap`, `dvbsnoop`, vdr-plugins, TVHeadend) issue the ioctls
//! and CA-PMT commands below.

// ---------------------------------------------------------------------------
// CA slot type and flags (struct ca_slot_info)
// ---------------------------------------------------------------------------

/// Smartcard slot (typical CAM module).
pub const CA_CI: u32 = 1;
/// CI-link layer slot.
pub const CA_CI_LINK: u32 = 2;
/// CI physical layer slot.
pub const CA_CI_PHYS: u32 = 4;
/// Old-style DESCR module.
pub const CA_DESCR: u32 = 8;
/// Slot supports session-control protocol.
pub const CA_SC: u32 = 128;

/// Module is present in the slot.
pub const CA_CI_MODULE_PRESENT: u32 = 1;
/// Module is initialised and ready.
pub const CA_CI_MODULE_READY: u32 = 2;

// ---------------------------------------------------------------------------
// CA descrambler types (struct ca_descr_info)
// ---------------------------------------------------------------------------

/// DVB Common Scrambling Algorithm.
pub const CA_ECD: u32 = 1;
/// Conax / NDS / generic.
pub const CA_NDS: u32 = 2;
/// DSS scrambling.
pub const CA_DSS: u32 = 4;

// ---------------------------------------------------------------------------
// Descrambler parity (struct ca_descr.parity)
// ---------------------------------------------------------------------------

/// Even parity control word.
pub const CA_PARITY_EVEN: u32 = 0;
/// Odd parity control word.
pub const CA_PARITY_ODD: u32 = 1;

// ---------------------------------------------------------------------------
// Userspace CA-PMT list-management commands
// (struct ca_msg + CA_SEND_MSG; first byte of CA-PMT)
// ---------------------------------------------------------------------------

/// More CA-PMT entries follow.
pub const CA_LIST_MGMT_MORE: u8 = 0x00;
/// First and only CA-PMT entry.
pub const CA_LIST_MGMT_FIRST: u8 = 0x01;
/// Last CA-PMT entry.
pub const CA_LIST_MGMT_LAST: u8 = 0x02;
/// Only one CA-PMT entry (default).
pub const CA_LIST_MGMT_ONLY: u8 = 0x03;
/// Add another service to an existing list.
pub const CA_LIST_MGMT_ADD: u8 = 0x04;
/// Update a service in the list.
pub const CA_LIST_MGMT_UPDATE: u8 = 0x05;

// ---------------------------------------------------------------------------
// CA-PMT cmd_id (selects what the CAM should do)
// ---------------------------------------------------------------------------

/// OK to descramble.
pub const CA_PMT_CMD_OK_DESCRAMBLING: u8 = 0x01;
/// OK to MMI (interact with user).
pub const CA_PMT_CMD_OK_MMI: u8 = 0x02;
/// Query CAM, don't start descrambling yet.
pub const CA_PMT_CMD_QUERY: u8 = 0x03;
/// Tear down.
pub const CA_PMT_CMD_NOT_SELECTED: u8 = 0x04;

// ---------------------------------------------------------------------------
// ioctl numbers (type 'o' base 0x80)
// ---------------------------------------------------------------------------

/// `CA_RESET` — reset the CAM.
pub const CA_RESET: u32 = 0x0000_6f80;
/// `CA_GET_CAP` — query CA capabilities.
pub const CA_GET_CAP: u32 = 0x8000_6f81;
/// `CA_GET_SLOT_INFO` — query CA slot info.
pub const CA_GET_SLOT_INFO: u32 = 0x8000_6f82;
/// `CA_GET_DESCR_INFO` — query descrambler info.
pub const CA_GET_DESCR_INFO: u32 = 0x8000_6f83;
/// `CA_GET_MSG` — read a CA APDU from the CAM.
pub const CA_GET_MSG: u32 = 0x8000_6f84;
/// `CA_SEND_MSG` — send a CA APDU to the CAM.
pub const CA_SEND_MSG: u32 = 0x4000_6f85;
/// `CA_SET_DESCR` — set a descrambler control word.
pub const CA_SET_DESCR: u32 = 0x4000_6f86;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_types_distinct_pow2() {
        let s = [CA_CI, CA_CI_LINK, CA_CI_PHYS, CA_DESCR, CA_SC];
        for &b in &s {
            assert!(b.is_power_of_two());
        }
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_module_flags_distinct() {
        assert!(CA_CI_MODULE_PRESENT.is_power_of_two());
        assert!(CA_CI_MODULE_READY.is_power_of_two());
        assert_ne!(CA_CI_MODULE_PRESENT, CA_CI_MODULE_READY);
    }

    #[test]
    fn test_descr_types_pow2() {
        for &b in &[CA_ECD, CA_NDS, CA_DSS] {
            assert!(b.is_power_of_two());
        }
        assert_ne!(CA_ECD, CA_NDS);
        assert_ne!(CA_NDS, CA_DSS);
    }

    #[test]
    fn test_parity_even_zero() {
        // CA_PARITY_EVEN==0 lets userspace zero-initialise the parity
        // field to a sensible default.
        assert_eq!(CA_PARITY_EVEN, 0);
        assert_eq!(CA_PARITY_ODD, 1);
    }

    #[test]
    fn test_list_mgmt_distinct() {
        let l = [
            CA_LIST_MGMT_MORE,
            CA_LIST_MGMT_FIRST,
            CA_LIST_MGMT_LAST,
            CA_LIST_MGMT_ONLY,
            CA_LIST_MGMT_ADD,
            CA_LIST_MGMT_UPDATE,
        ];
        for i in 0..l.len() {
            for j in (i + 1)..l.len() {
                assert_ne!(l[i], l[j]);
            }
        }
    }

    #[test]
    fn test_pmt_cmds_distinct() {
        let c = [
            CA_PMT_CMD_OK_DESCRAMBLING,
            CA_PMT_CMD_OK_MMI,
            CA_PMT_CMD_QUERY,
            CA_PMT_CMD_NOT_SELECTED,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct_and_type_o() {
        let ops = [
            CA_RESET,
            CA_GET_CAP,
            CA_GET_SLOT_INFO,
            CA_GET_DESCR_INFO,
            CA_GET_MSG,
            CA_SEND_MSG,
            CA_SET_DESCR,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // DVB ioctls use type byte 'o' (0x6f).
            assert_eq!((ops[i] >> 8) & 0xff, b'o' as u32);
        }
    }
}
