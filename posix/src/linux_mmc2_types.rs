//! `<linux/mmc/mmc.h>` — Additional MMC/SD card constants.
//!
//! Supplementary MMC constants covering command indices,
//! response types, card states, and bus width modes.

// ---------------------------------------------------------------------------
// MMC commands (MMC_*)
// ---------------------------------------------------------------------------

/// GO_IDLE_STATE (CMD0).
pub const MMC_GO_IDLE_STATE: u32 = 0;
/// SEND_OP_COND (CMD1).
pub const MMC_SEND_OP_COND: u32 = 1;
/// ALL_SEND_CID (CMD2).
pub const MMC_ALL_SEND_CID: u32 = 2;
/// SET_RELATIVE_ADDR (CMD3).
pub const MMC_SET_RELATIVE_ADDR: u32 = 3;
/// SWITCH (CMD6).
pub const MMC_SWITCH: u32 = 6;
/// SELECT_CARD (CMD7).
pub const MMC_SELECT_CARD: u32 = 7;
/// SEND_EXT_CSD (CMD8).
pub const MMC_SEND_EXT_CSD: u32 = 8;
/// SEND_CSD (CMD9).
pub const MMC_SEND_CSD: u32 = 9;
/// SEND_CID (CMD10).
pub const MMC_SEND_CID: u32 = 10;
/// STOP_TRANSMISSION (CMD12).
pub const MMC_STOP_TRANSMISSION: u32 = 12;
/// SEND_STATUS (CMD13).
pub const MMC_SEND_STATUS: u32 = 13;
/// SET_BLOCKLEN (CMD16).
pub const MMC_SET_BLOCKLEN: u32 = 16;
/// READ_SINGLE_BLOCK (CMD17).
pub const MMC_READ_SINGLE_BLOCK: u32 = 17;
/// READ_MULTIPLE_BLOCK (CMD18).
pub const MMC_READ_MULTIPLE_BLOCK: u32 = 18;
/// WRITE_BLOCK (CMD24).
pub const MMC_WRITE_BLOCK: u32 = 24;
/// WRITE_MULTIPLE_BLOCK (CMD25).
pub const MMC_WRITE_MULTIPLE_BLOCK: u32 = 25;
/// APP_CMD (CMD55).
pub const MMC_APP_CMD: u32 = 55;

// ---------------------------------------------------------------------------
// MMC response types
// ---------------------------------------------------------------------------

/// No response.
pub const MMC_RSP_NONE: u32 = 0;
/// R1 (normal response).
pub const MMC_RSP_R1: u32 = 1;
/// R1b (normal + busy).
pub const MMC_RSP_R1B: u32 = 2;
/// R2 (CID/CSD register).
pub const MMC_RSP_R2: u32 = 3;
/// R3 (OCR register).
pub const MMC_RSP_R3: u32 = 4;
/// R4 (fast IO).
pub const MMC_RSP_R4: u32 = 5;
/// R5 (IRQ response).
pub const MMC_RSP_R5: u32 = 6;
/// R6 (RCA response).
pub const MMC_RSP_R6: u32 = 7;
/// R7 (card interface condition).
pub const MMC_RSP_R7: u32 = 8;

// ---------------------------------------------------------------------------
// MMC card states
// ---------------------------------------------------------------------------

/// Idle state.
pub const MMC_STATE_IDLE: u32 = 0;
/// Ready state.
pub const MMC_STATE_READY: u32 = 1;
/// Identification state.
pub const MMC_STATE_IDENT: u32 = 2;
/// Stand-by state.
pub const MMC_STATE_STBY: u32 = 3;
/// Transfer state.
pub const MMC_STATE_TRAN: u32 = 4;
/// Sending data state.
pub const MMC_STATE_DATA: u32 = 5;
/// Receive data state.
pub const MMC_STATE_RCV: u32 = 6;
/// Programming state.
pub const MMC_STATE_PRG: u32 = 7;
/// Disconnect state.
pub const MMC_STATE_DIS: u32 = 8;
/// Bus test state.
pub const MMC_STATE_BTST: u32 = 9;
/// Sleep state.
pub const MMC_STATE_SLP: u32 = 10;

// ---------------------------------------------------------------------------
// MMC bus width
// ---------------------------------------------------------------------------

/// 1-bit bus width.
pub const MMC_BUS_WIDTH_1: u32 = 0;
/// 4-bit bus width.
pub const MMC_BUS_WIDTH_4: u32 = 1;
/// 8-bit bus width.
pub const MMC_BUS_WIDTH_8: u32 = 2;

// ---------------------------------------------------------------------------
// MMC timing modes
// ---------------------------------------------------------------------------

/// Legacy timing.
pub const MMC_TIMING_LEGACY: u32 = 0;
/// High speed.
pub const MMC_TIMING_MMC_HS: u32 = 1;
/// SD high speed.
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
    fn test_commands_distinct() {
        let cmds = [
            MMC_GO_IDLE_STATE,
            MMC_SEND_OP_COND,
            MMC_ALL_SEND_CID,
            MMC_SET_RELATIVE_ADDR,
            MMC_SWITCH,
            MMC_SELECT_CARD,
            MMC_SEND_EXT_CSD,
            MMC_SEND_CSD,
            MMC_SEND_CID,
            MMC_STOP_TRANSMISSION,
            MMC_SEND_STATUS,
            MMC_SET_BLOCKLEN,
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
    fn test_card_states_distinct() {
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
            MMC_STATE_BTST,
            MMC_STATE_SLP,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_bus_widths_distinct() {
        let widths = [MMC_BUS_WIDTH_1, MMC_BUS_WIDTH_4, MMC_BUS_WIDTH_8];
        for i in 0..widths.len() {
            for j in (i + 1)..widths.len() {
                assert_ne!(widths[i], widths[j]);
            }
        }
    }

    #[test]
    fn test_timing_modes_distinct() {
        let modes = [
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
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
