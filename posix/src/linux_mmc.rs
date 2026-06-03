//! `<linux/mmc/ioctl.h>` + `<linux/mmc/core.h>` — MMC/SD/eMMC constants.
//!
//! The MMC subsystem handles SD cards, eMMC, and SDIO devices.
//! Userspace accesses raw MMC commands via ioctls on `/dev/mmcblkN`.

// ---------------------------------------------------------------------------
// MMC commands (CMD indices)
// ---------------------------------------------------------------------------

/// GO_IDLE_STATE — reset card.
pub const MMC_GO_IDLE_STATE: u32 = 0;
/// SEND_OP_COND — ask operating condition.
pub const MMC_SEND_OP_COND: u32 = 1;
/// ALL_SEND_CID — all cards send CID.
pub const MMC_ALL_SEND_CID: u32 = 2;
/// SET_RELATIVE_ADDR — assign relative address.
pub const MMC_SET_RELATIVE_ADDR: u32 = 3;
/// SELECT_CARD — select/deselect card.
pub const MMC_SELECT_CARD: u32 = 7;
/// SEND_EXT_CSD — send extended CSD.
pub const MMC_SEND_EXT_CSD: u32 = 8;
/// SEND_CSD — send card specific data.
pub const MMC_SEND_CSD: u32 = 9;
/// STOP_TRANSMISSION — stop multiple block.
pub const MMC_STOP_TRANSMISSION: u32 = 12;
/// SEND_STATUS — card status.
pub const MMC_SEND_STATUS: u32 = 13;
/// READ_SINGLE_BLOCK.
pub const MMC_READ_SINGLE_BLOCK: u32 = 17;
/// READ_MULTIPLE_BLOCK.
pub const MMC_READ_MULTIPLE_BLOCK: u32 = 18;
/// WRITE_BLOCK.
pub const MMC_WRITE_BLOCK: u32 = 24;
/// WRITE_MULTIPLE_BLOCK.
pub const MMC_WRITE_MULTIPLE_BLOCK: u32 = 25;
/// APP_CMD — next command is application-specific.
pub const MMC_APP_CMD: u32 = 55;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// No response.
pub const MMC_RSP_NONE: u32 = 0;
/// R1 response (48 bits, with status).
pub const MMC_RSP_R1: u32 = 1;
/// R1b response (R1 with busy).
pub const MMC_RSP_R1B: u32 = 2;
/// R2 response (136 bits, CID/CSD).
pub const MMC_RSP_R2: u32 = 3;
/// R3 response (48 bits, OCR).
pub const MMC_RSP_R3: u32 = 4;
/// R4 response (fast I/O).
pub const MMC_RSP_R4: u32 = 5;
/// R5 response (SDIO).
pub const MMC_RSP_R5: u32 = 6;
/// R6 response (published RCA).
pub const MMC_RSP_R6: u32 = 7;
/// R7 response (card interface condition).
pub const MMC_RSP_R7: u32 = 8;

// ---------------------------------------------------------------------------
// Card state (bits in R1 status)
// ---------------------------------------------------------------------------

/// Card is ready.
pub const MMC_STATUS_RDY_FOR_DATA: u32 = 1 << 8;
/// Current state mask.
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
// MMC VDD voltage ranges (OCR bits)
// ---------------------------------------------------------------------------

/// 2.7V–2.8V.
pub const MMC_VDD_27_28: u32 = 1 << 15;
/// 2.8V–2.9V.
pub const MMC_VDD_28_29: u32 = 1 << 16;
/// 2.9V–3.0V.
pub const MMC_VDD_29_30: u32 = 1 << 17;
/// 3.0V–3.1V.
pub const MMC_VDD_30_31: u32 = 1 << 18;
/// 3.1V–3.2V.
pub const MMC_VDD_31_32: u32 = 1 << 19;
/// 3.2V–3.3V.
pub const MMC_VDD_32_33: u32 = 1 << 20;
/// 3.3V–3.4V.
pub const MMC_VDD_33_34: u32 = 1 << 21;
/// 3.4V–3.5V.
pub const MMC_VDD_34_35: u32 = 1 << 22;
/// 3.5V–3.6V.
pub const MMC_VDD_35_36: u32 = 1 << 23;

// ---------------------------------------------------------------------------
// Bus widths
// ---------------------------------------------------------------------------

/// 1-bit bus.
pub const MMC_BUS_WIDTH_1: u32 = 0;
/// 4-bit bus.
pub const MMC_BUS_WIDTH_4: u32 = 2;
/// 8-bit bus.
pub const MMC_BUS_WIDTH_8: u32 = 3;

// ---------------------------------------------------------------------------
// Timing modes
// ---------------------------------------------------------------------------

/// Legacy timing.
pub const MMC_TIMING_LEGACY: u32 = 0;
/// MMC high-speed.
pub const MMC_TIMING_MMC_HS: u32 = 1;
/// SD high-speed.
pub const MMC_TIMING_SD_HS: u32 = 2;
/// UHS SDR12.
pub const MMC_TIMING_UHS_SDR12: u32 = 3;
/// UHS SDR25.
pub const MMC_TIMING_UHS_SDR25: u32 = 4;
/// UHS SDR50.
pub const MMC_TIMING_UHS_SDR50: u32 = 5;
/// UHS SDR104.
pub const MMC_TIMING_UHS_SDR104: u32 = 6;
/// UHS DDR50.
pub const MMC_TIMING_UHS_DDR50: u32 = 7;
/// MMC DDR52.
pub const MMC_TIMING_MMC_DDR52: u32 = 8;
/// MMC HS200.
pub const MMC_TIMING_MMC_HS200: u32 = 9;
/// MMC HS400.
pub const MMC_TIMING_MMC_HS400: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            MMC_GO_IDLE_STATE,
            MMC_SEND_OP_COND,
            MMC_ALL_SEND_CID,
            MMC_SET_RELATIVE_ADDR,
            MMC_SELECT_CARD,
            MMC_SEND_EXT_CSD,
            MMC_SEND_CSD,
            MMC_STOP_TRANSMISSION,
            MMC_SEND_STATUS,
            MMC_READ_SINGLE_BLOCK,
            MMC_READ_MULTIPLE_BLOCK,
            MMC_WRITE_BLOCK,
            MMC_WRITE_MULTIPLE_BLOCK,
            MMC_APP_CMD,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_response_types_distinct() {
        let rsps = [
            MMC_RSP_NONE,
            MMC_RSP_R1,
            MMC_RSP_R1B,
            MMC_RSP_R2,
            MMC_RSP_R3,
            MMC_RSP_R4,
            MMC_RSP_R5,
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
    fn test_vdd_ranges_are_powers_of_two() {
        let vdds = [
            MMC_VDD_27_28,
            MMC_VDD_28_29,
            MMC_VDD_29_30,
            MMC_VDD_30_31,
            MMC_VDD_31_32,
            MMC_VDD_32_33,
            MMC_VDD_33_34,
            MMC_VDD_34_35,
            MMC_VDD_35_36,
        ];
        for vdd in &vdds {
            assert!(vdd.is_power_of_two(), "0x{:x} is not a power of two", vdd);
        }
    }

    #[test]
    fn test_timing_distinct() {
        let timings = [
            MMC_TIMING_LEGACY,
            MMC_TIMING_MMC_HS,
            MMC_TIMING_SD_HS,
            MMC_TIMING_UHS_SDR12,
            MMC_TIMING_UHS_SDR25,
            MMC_TIMING_UHS_SDR50,
            MMC_TIMING_UHS_SDR104,
            MMC_TIMING_UHS_DDR50,
            MMC_TIMING_MMC_DDR52,
            MMC_TIMING_MMC_HS200,
            MMC_TIMING_MMC_HS400,
        ];
        for i in 0..timings.len() {
            for j in (i + 1)..timings.len() {
                assert_ne!(timings[i], timings[j]);
            }
        }
    }

    #[test]
    fn test_bus_widths() {
        assert_eq!(MMC_BUS_WIDTH_1, 0);
        assert_eq!(MMC_BUS_WIDTH_4, 2);
        assert_eq!(MMC_BUS_WIDTH_8, 3);
    }
}
