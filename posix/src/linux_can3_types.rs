//! `<linux/can.h>` — Additional CAN bus constants.
//!
//! Supplementary CAN constants covering error class flags,
//! CAN FD flags, and CAN XL frame constants.

// ---------------------------------------------------------------------------
// CAN error class flags (in can_id)
// ---------------------------------------------------------------------------

/// TX timeout.
pub const CAN_ERR_TX_TIMEOUT: u32 = 0x00000001;
/// Lost arbitration.
pub const CAN_ERR_LOSTARB: u32 = 0x00000002;
/// Controller problems.
pub const CAN_ERR_CRTL: u32 = 0x00000004;
/// Protocol violation.
pub const CAN_ERR_PROT: u32 = 0x00000008;
/// Transceiver status.
pub const CAN_ERR_TRX: u32 = 0x00000010;
/// No ACK on transmission.
pub const CAN_ERR_ACK: u32 = 0x00000020;
/// Bus off.
pub const CAN_ERR_BUSOFF: u32 = 0x00000040;
/// Bus error.
pub const CAN_ERR_BUSERROR: u32 = 0x00000080;
/// Controller restarted.
pub const CAN_ERR_RESTARTED: u32 = 0x00000100;
/// CAN ID counter overflow.
pub const CAN_ERR_CNT: u32 = 0x00000200;

// ---------------------------------------------------------------------------
// CAN FD flags (in canfd_frame.flags)
// ---------------------------------------------------------------------------

/// Bit Rate Switch — second bitrate for data phase.
pub const CANFD_BRS: u8 = 0x01;
/// Error State Indicator.
pub const CANFD_ESI: u8 = 0x02;
/// FD frame.
pub const CANFD_FDF: u8 = 0x04;

// ---------------------------------------------------------------------------
// CAN XL frame constants
// ---------------------------------------------------------------------------

/// XL frame flag.
pub const CANXL_XLF: u32 = 0x80;
/// XL security flag.
pub const CANXL_SEC: u32 = 0x01;
/// XL minimum data length.
pub const CANXL_MIN_DLEN: u32 = 1;
/// XL maximum data length.
pub const CANXL_MAX_DLEN: u32 = 2048;
/// XL maximum DLC.
pub const CANXL_MAX_DLC: u32 = 2048;
/// XL priority mask.
pub const CANXL_PRIO_MASK: u32 = 0x7FF;

// ---------------------------------------------------------------------------
// CAN frame lengths
// ---------------------------------------------------------------------------

/// Classic CAN maximum data length.
pub const CAN_MAX_DLEN: u32 = 8;
/// CAN FD maximum data length.
pub const CANFD_MAX_DLEN: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_err_flags_power_of_two() {
        let flags = [
            CAN_ERR_TX_TIMEOUT, CAN_ERR_LOSTARB, CAN_ERR_CRTL,
            CAN_ERR_PROT, CAN_ERR_TRX, CAN_ERR_ACK,
            CAN_ERR_BUSOFF, CAN_ERR_BUSERROR, CAN_ERR_RESTARTED,
            CAN_ERR_CNT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_err_flags_no_overlap() {
        let flags = [
            CAN_ERR_TX_TIMEOUT, CAN_ERR_LOSTARB, CAN_ERR_CRTL,
            CAN_ERR_PROT, CAN_ERR_TRX, CAN_ERR_ACK,
            CAN_ERR_BUSOFF, CAN_ERR_BUSERROR, CAN_ERR_RESTARTED,
            CAN_ERR_CNT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fd_flags_no_overlap() {
        let flags = [CANFD_BRS, CANFD_ESI, CANFD_FDF];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_frame_lengths() {
        assert_eq!(CAN_MAX_DLEN, 8);
        assert_eq!(CANFD_MAX_DLEN, 64);
        assert!(CANXL_MAX_DLEN > CANFD_MAX_DLEN);
    }

    #[test]
    fn test_xl_dlen_range() {
        assert!(CANXL_MIN_DLEN < CANXL_MAX_DLEN);
    }
}
