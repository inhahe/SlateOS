//! `<linux/mmc/ioctl.h>` — MMC/SD/eMMC ioctl and protocol constants.
//!
//! These constants define the ioctl interface for sending MMC
//! commands to SD cards and eMMC devices, including command
//! opcodes, response types, and card status bits.

// ---------------------------------------------------------------------------
// MMC ioctl commands
// ---------------------------------------------------------------------------

/// Send a single MMC command.
pub const MMC_IOC_CMD: u32 = 0xC048_B300;
/// Send multiple MMC commands (batch).
pub const MMC_IOC_MULTI_CMD: u32 = 0xC008_B301;

// ---------------------------------------------------------------------------
// MMC command opcodes
// ---------------------------------------------------------------------------

/// GO_IDLE_STATE — reset card.
pub const MMC_GO_IDLE_STATE: u32 = 0;
/// SEND_OP_COND — MMC initialization.
pub const MMC_SEND_OP_COND: u32 = 1;
/// ALL_SEND_CID — get card identification.
pub const MMC_ALL_SEND_CID: u32 = 2;
/// SET_RELATIVE_ADDR — assign address.
pub const MMC_SET_RELATIVE_ADDR: u32 = 3;
/// SELECT/DESELECT_CARD.
pub const MMC_SELECT_CARD: u32 = 7;
/// SEND_CSD — card specific data.
pub const MMC_SEND_CSD: u32 = 9;
/// STOP_TRANSMISSION.
pub const MMC_STOP_TRANSMISSION: u32 = 12;
/// READ_SINGLE_BLOCK.
pub const MMC_READ_SINGLE_BLOCK: u32 = 17;
/// READ_MULTIPLE_BLOCK.
pub const MMC_READ_MULTIPLE_BLOCK: u32 = 18;
/// WRITE_BLOCK.
pub const MMC_WRITE_BLOCK: u32 = 24;
/// WRITE_MULTIPLE_BLOCK.
pub const MMC_WRITE_MULTIPLE_BLOCK: u32 = 25;
/// ERASE_GROUP_START.
pub const MMC_ERASE_GROUP_START: u32 = 35;
/// ERASE_GROUP_END.
pub const MMC_ERASE_GROUP_END: u32 = 36;
/// ERASE.
pub const MMC_ERASE: u32 = 38;
/// APP_CMD prefix.
pub const MMC_APP_CMD: u32 = 55;

// ---------------------------------------------------------------------------
// SD Application commands (ACMD, after CMD55)
// ---------------------------------------------------------------------------

/// Set bus width.
pub const SD_APP_SET_BUS_WIDTH: u32 = 6;
/// SD_STATUS.
pub const SD_APP_SD_STATUS: u32 = 13;
/// Send operating condition.
pub const SD_APP_SEND_OP_COND: u32 = 41;
/// Send SCR.
pub const SD_APP_SEND_SCR: u32 = 51;

// ---------------------------------------------------------------------------
// MMC response types
// ---------------------------------------------------------------------------

/// No response.
pub const MMC_RSP_NONE: u32 = 0;
/// R1 response (normal with status).
pub const MMC_RSP_R1: u32 = 1;
/// R1b response (R1 + busy).
pub const MMC_RSP_R1B: u32 = 2;
/// R2 response (CID/CSD).
pub const MMC_RSP_R2: u32 = 3;
/// R3 response (OCR).
pub const MMC_RSP_R3: u32 = 4;
/// R6 response (published RCA).
pub const MMC_RSP_R6: u32 = 6;
/// R7 response (card interface condition).
pub const MMC_RSP_R7: u32 = 7;

// ---------------------------------------------------------------------------
// Card status bits (R1 response)
// ---------------------------------------------------------------------------

/// Card is ready for data.
pub const MMC_STATUS_READY_FOR_DATA: u32 = 1 << 8;
/// Current state mask (4 bits at position 9-12).
pub const MMC_STATUS_CURRENT_STATE_MASK: u32 = 0xF << 9;
/// Error bit.
pub const MMC_STATUS_ERROR: u32 = 1 << 19;
/// CC error.
pub const MMC_STATUS_CC_ERROR: u32 = 1 << 20;
/// Card ECC failed.
pub const MMC_STATUS_CARD_ECC_FAILED: u32 = 1 << 21;
/// WP violation.
pub const MMC_STATUS_WP_VIOLATION: u32 = 1 << 26;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(MMC_IOC_CMD, MMC_IOC_MULTI_CMD);
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            MMC_GO_IDLE_STATE,
            MMC_SEND_OP_COND,
            MMC_ALL_SEND_CID,
            MMC_SET_RELATIVE_ADDR,
            MMC_SELECT_CARD,
            MMC_SEND_CSD,
            MMC_STOP_TRANSMISSION,
            MMC_READ_SINGLE_BLOCK,
            MMC_READ_MULTIPLE_BLOCK,
            MMC_WRITE_BLOCK,
            MMC_WRITE_MULTIPLE_BLOCK,
            MMC_ERASE_GROUP_START,
            MMC_ERASE_GROUP_END,
            MMC_ERASE,
            MMC_APP_CMD,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rsp_types_distinct() {
        let rsps = [
            MMC_RSP_NONE,
            MMC_RSP_R1,
            MMC_RSP_R1B,
            MMC_RSP_R2,
            MMC_RSP_R3,
            MMC_RSP_R6,
            MMC_RSP_R7,
        ];
        for i in 0..rsps.len() {
            for j in (i + 1)..rsps.len() {
                assert_ne!(rsps[i], rsps[j]);
            }
        }
    }

    #[test]
    fn test_go_idle_is_zero() {
        assert_eq!(MMC_GO_IDLE_STATE, 0);
    }

    #[test]
    fn test_status_ready() {
        assert_eq!(MMC_STATUS_READY_FOR_DATA, 1 << 8);
    }

    #[test]
    fn test_sd_acmds_distinct() {
        let acmds = [
            SD_APP_SET_BUS_WIDTH,
            SD_APP_SD_STATUS,
            SD_APP_SEND_OP_COND,
            SD_APP_SEND_SCR,
        ];
        for i in 0..acmds.len() {
            for j in (i + 1)..acmds.len() {
                assert_ne!(acmds[i], acmds[j]);
            }
        }
    }
}
