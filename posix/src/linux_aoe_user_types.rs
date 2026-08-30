//! `<linux/aoe.h>` — ATA-over-Ethernet (aoe) blkdev constants.
//!
//! The aoe driver exposes Coraid/EtherDrive AoE shelves as block
//! devices. vblade, aoetools (aoe-stat, aoe-mkdevs), and any AoE
//! initiator consume the constants below for the on-the-wire frame
//! header and the configuration ioctls.

// ---------------------------------------------------------------------------
// Ethertype (set by aoe driver in sk_buff)
// ---------------------------------------------------------------------------

/// AoE Ethertype as registered by Coraid.
pub const ETH_P_AOE: u16 = 0x88a2;

// ---------------------------------------------------------------------------
// AoE protocol version (in flags byte of struct aoehdr)
// ---------------------------------------------------------------------------

/// Current AoE protocol version (mask in the top 4 bits).
pub const AOE_VERSION: u8 = 1;
/// Mask for the version nibble in the header flags byte.
pub const AOE_VERSION_MASK: u8 = 0xf0;
/// Shift to recover the version nibble.
pub const AOE_VERSION_SHIFT: u8 = 4;

// ---------------------------------------------------------------------------
// Flags byte (struct aoehdr.verfl)
// ---------------------------------------------------------------------------

/// Response (set on replies; clear on requests).
pub const AOE_FLAG_RESP: u8 = 1 << 3;
/// Error indicator (response carries an error byte).
pub const AOE_FLAG_ERR: u8 = 1 << 2;

// ---------------------------------------------------------------------------
// Commands (struct aoehdr.cmd)
// ---------------------------------------------------------------------------

/// ATA command issue / response.
pub const AOE_CMD_ATA: u8 = 0;
/// Query / configure target.
pub const AOE_CMD_CFG: u8 = 1;
/// Mac-list directive (used to authorise initiators).
pub const AOE_CMD_RES: u8 = 2;
/// Vendor-specific extension command.
pub const AOE_CMD_VEND: u8 = 0xf0;

// ---------------------------------------------------------------------------
// Error codes (struct aoehdr.err, when AOE_FLAG_ERR set)
// ---------------------------------------------------------------------------

/// Unrecognised command code.
pub const AOE_ERR_BADCMD: u8 = 1;
/// Bad command argument.
pub const AOE_ERR_BADARG: u8 = 2;
/// Target unavailable.
pub const AOE_ERR_UNAVAIL: u8 = 3;
/// Config string mismatch.
pub const AOE_ERR_CFG_BADCONFIG: u8 = 4;
/// Unsupported version.
pub const AOE_ERR_BADVER: u8 = 5;

// ---------------------------------------------------------------------------
// Config-query sub-commands (struct aoehdr_cfg.aoecmd)
// ---------------------------------------------------------------------------

/// Read the config string.
pub const AOE_CFG_READ: u8 = 0;
/// Test-then-set (write only if current empty).
pub const AOE_CFG_TEST: u8 = 1;
/// Set (always overwrite).
pub const AOE_CFG_TEST_PREFIX: u8 = 2;
/// Set without test.
pub const AOE_CFG_SET: u8 = 3;
/// Force set (clear test conditions).
pub const AOE_CFG_FORCE_SET: u8 = 4;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum payload of one AoE config string.
pub const AOE_CFG_STR_MAX: u32 = 1024;
/// "Broadcast" shelf number — used by aoe-discover.
pub const AOE_SHELF_BCAST: u16 = 0xffff;
/// "Broadcast" slot number.
pub const AOE_SLOT_BCAST: u8 = 0xff;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethertype_well_known() {
        // 0x88a2 was registered with IEEE and must remain stable.
        assert_eq!(ETH_P_AOE, 0x88a2);
    }

    #[test]
    fn test_version_in_high_nibble() {
        assert_eq!(AOE_VERSION_MASK, 0xf0);
        assert_eq!(AOE_VERSION_SHIFT, 4);
        // V1 << 4 fits the mask.
        assert_eq!((AOE_VERSION << AOE_VERSION_SHIFT) & AOE_VERSION_MASK, 0x10);
    }

    #[test]
    fn test_flag_bits_in_low_nibble() {
        // Flag bits live in the low nibble (so version+flags share a
        // single byte without collision).
        assert_eq!(AOE_FLAG_RESP & AOE_VERSION_MASK, 0);
        assert_eq!(AOE_FLAG_ERR & AOE_VERSION_MASK, 0);
        assert_ne!(AOE_FLAG_RESP, AOE_FLAG_ERR);
        assert!(AOE_FLAG_RESP.is_power_of_two());
        assert!(AOE_FLAG_ERR.is_power_of_two());
    }

    #[test]
    fn test_commands_distinct() {
        let c = [AOE_CMD_ATA, AOE_CMD_CFG, AOE_CMD_RES, AOE_CMD_VEND];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // ATA must be 0 — it's the default command and the most common.
        assert_eq!(AOE_CMD_ATA, 0);
    }

    #[test]
    fn test_errors_distinct_nonzero() {
        let e = [
            AOE_ERR_BADCMD,
            AOE_ERR_BADARG,
            AOE_ERR_UNAVAIL,
            AOE_ERR_CFG_BADCONFIG,
            AOE_ERR_BADVER,
        ];
        for &x in &e {
            assert!(x > 0); // 0 means "no error".
        }
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }

    #[test]
    fn test_cfg_subcmds_distinct() {
        let c = [
            AOE_CFG_READ,
            AOE_CFG_TEST,
            AOE_CFG_TEST_PREFIX,
            AOE_CFG_SET,
            AOE_CFG_FORCE_SET,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        assert_eq!(AOE_CFG_READ, 0);
    }

    #[test]
    fn test_broadcast_constants() {
        // Shelf+slot broadcast pair is used by aoe-discover.
        assert_eq!(AOE_SHELF_BCAST, 0xffff);
        assert_eq!(AOE_SLOT_BCAST, 0xff);
        assert_eq!(AOE_CFG_STR_MAX, 1024);
    }
}
