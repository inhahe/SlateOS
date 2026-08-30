//! `<linux/mmc/mmc.h>` — MMC/SD/eMMC card constants.
//!
//! MMC (MultiMediaCard), SD (Secure Digital), and eMMC (embedded MMC)
//! share a common protocol for flash-based storage. The Linux MMC
//! subsystem handles card detection, initialization, and I/O for all
//! these card types via host controller drivers.

// ---------------------------------------------------------------------------
// MMC commands
// ---------------------------------------------------------------------------

/// GO_IDLE_STATE (reset, CMD0).
pub const MMC_CMD_GO_IDLE_STATE: u8 = 0;
/// SEND_OP_COND (CMD1, MMC only).
pub const MMC_CMD_SEND_OP_COND: u8 = 1;
/// ALL_SEND_CID (CMD2).
pub const MMC_CMD_ALL_SEND_CID: u8 = 2;
/// SET_RELATIVE_ADDR (CMD3).
pub const MMC_CMD_SET_RELATIVE_ADDR: u8 = 3;
/// SELECT_CARD (CMD7).
pub const MMC_CMD_SELECT_CARD: u8 = 7;
/// SEND_CSD (CMD9).
pub const MMC_CMD_SEND_CSD: u8 = 9;
/// STOP_TRANSMISSION (CMD12).
pub const MMC_CMD_STOP_TRANSMISSION: u8 = 12;
/// SEND_STATUS (CMD13).
pub const MMC_CMD_SEND_STATUS: u8 = 13;
/// SET_BLOCKLEN (CMD16).
pub const MMC_CMD_SET_BLOCKLEN: u8 = 16;
/// READ_SINGLE_BLOCK (CMD17).
pub const MMC_CMD_READ_SINGLE_BLOCK: u8 = 17;
/// READ_MULTIPLE_BLOCK (CMD18).
pub const MMC_CMD_READ_MULTIPLE_BLOCK: u8 = 18;
/// WRITE_BLOCK (CMD24).
pub const MMC_CMD_WRITE_BLOCK: u8 = 24;
/// WRITE_MULTIPLE_BLOCK (CMD25).
pub const MMC_CMD_WRITE_MULTIPLE_BLOCK: u8 = 25;
/// ERASE (CMD38).
pub const MMC_CMD_ERASE: u8 = 38;
/// APP_CMD (CMD55, prefix for ACMD).
pub const MMC_CMD_APP_CMD: u8 = 55;

// ---------------------------------------------------------------------------
// Card states (R1 response bits 12:9)
// ---------------------------------------------------------------------------

/// Idle state.
pub const MMC_STATE_IDLE: u8 = 0;
/// Ready state.
pub const MMC_STATE_READY: u8 = 1;
/// Identification state.
pub const MMC_STATE_IDENT: u8 = 2;
/// Standby state.
pub const MMC_STATE_STBY: u8 = 3;
/// Transfer state.
pub const MMC_STATE_TRAN: u8 = 4;
/// Sending data state.
pub const MMC_STATE_DATA: u8 = 5;
/// Receive data state.
pub const MMC_STATE_RCV: u8 = 6;
/// Programming state.
pub const MMC_STATE_PRG: u8 = 7;
/// Disconnect state.
pub const MMC_STATE_DIS: u8 = 8;

// ---------------------------------------------------------------------------
// Bus speed modes
// ---------------------------------------------------------------------------

/// Default speed (25 MHz, SD) / 26 MHz (MMC).
pub const MMC_TIMING_LEGACY: u8 = 0;
/// High speed (50 MHz).
pub const MMC_TIMING_SD_HS: u8 = 1;
/// UHS-I SDR12.
pub const MMC_TIMING_UHS_SDR12: u8 = 2;
/// UHS-I SDR25.
pub const MMC_TIMING_UHS_SDR25: u8 = 3;
/// UHS-I SDR50.
pub const MMC_TIMING_UHS_SDR50: u8 = 4;
/// UHS-I SDR104.
pub const MMC_TIMING_UHS_SDR104: u8 = 5;
/// UHS-I DDR50.
pub const MMC_TIMING_UHS_DDR50: u8 = 6;
/// eMMC HS200.
pub const MMC_TIMING_MMC_HS200: u8 = 7;
/// eMMC HS400.
pub const MMC_TIMING_MMC_HS400: u8 = 8;

// ---------------------------------------------------------------------------
// Card type flags
// ---------------------------------------------------------------------------

/// MMC card.
pub const MMC_TYPE_MMC: u8 = 0;
/// SD card.
pub const MMC_TYPE_SD: u8 = 1;
/// SDIO card.
pub const MMC_TYPE_SDIO: u8 = 2;
/// SD combo (memory + SDIO).
pub const MMC_TYPE_SD_COMBO: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            MMC_CMD_GO_IDLE_STATE,
            MMC_CMD_SEND_OP_COND,
            MMC_CMD_ALL_SEND_CID,
            MMC_CMD_SET_RELATIVE_ADDR,
            MMC_CMD_SELECT_CARD,
            MMC_CMD_SEND_CSD,
            MMC_CMD_STOP_TRANSMISSION,
            MMC_CMD_SEND_STATUS,
            MMC_CMD_SET_BLOCKLEN,
            MMC_CMD_READ_SINGLE_BLOCK,
            MMC_CMD_READ_MULTIPLE_BLOCK,
            MMC_CMD_WRITE_BLOCK,
            MMC_CMD_WRITE_MULTIPLE_BLOCK,
            MMC_CMD_ERASE,
            MMC_CMD_APP_CMD,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            MMC_STATE_IDLE,
            MMC_STATE_READY,
            MMC_STATE_IDENT,
            MMC_STATE_STBY,
            MMC_STATE_TRAN,
            MMC_STATE_DATA,
            MMC_STATE_RCV,
            MMC_STATE_PRG,
            MMC_STATE_DIS,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_timings_distinct() {
        let timings = [
            MMC_TIMING_LEGACY,
            MMC_TIMING_SD_HS,
            MMC_TIMING_UHS_SDR12,
            MMC_TIMING_UHS_SDR25,
            MMC_TIMING_UHS_SDR50,
            MMC_TIMING_UHS_SDR104,
            MMC_TIMING_UHS_DDR50,
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
    fn test_card_types_distinct() {
        let types = [MMC_TYPE_MMC, MMC_TYPE_SD, MMC_TYPE_SDIO, MMC_TYPE_SD_COMBO];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
