//! `<linux/w1.h>` — 1-Wire bus protocol constants.
//!
//! The 1-Wire bus is a single-wire protocol (plus ground) for
//! communicating with low-speed peripherals like temperature sensors
//! (DS18B20), iButton/keys, and battery monitors. Each device has
//! a unique 64-bit ROM ID for addressing.

// ---------------------------------------------------------------------------
// 1-Wire ROM commands
// ---------------------------------------------------------------------------

/// Read ROM (get device's 64-bit ID, single device only).
pub const W1_CMD_READ_ROM: u8 = 0x33;
/// Match ROM (select a specific device by ID).
pub const W1_CMD_MATCH_ROM: u8 = 0x55;
/// Skip ROM (address all devices on bus).
pub const W1_CMD_SKIP_ROM: u8 = 0xCC;
/// Search ROM (enumerate devices on bus).
pub const W1_CMD_SEARCH_ROM: u8 = 0xF0;
/// Alarm search (enumerate devices with alarm flag set).
pub const W1_CMD_ALARM_SEARCH: u8 = 0xEC;
/// Overdrive skip ROM (enter high-speed mode, all devices).
pub const W1_CMD_OVERDRIVE_SKIP: u8 = 0x3C;
/// Overdrive match ROM (enter high-speed mode, one device).
pub const W1_CMD_OVERDRIVE_MATCH: u8 = 0x69;
/// Resume (continue communication with last addressed device).
pub const W1_CMD_RESUME: u8 = 0xA5;

// ---------------------------------------------------------------------------
// 1-Wire device family codes (high byte of ROM ID)
// ---------------------------------------------------------------------------

/// DS18S20 temperature sensor (parasite powered).
pub const W1_FAMILY_DS18S20: u8 = 0x10;
/// DS18B20 temperature sensor (programmable resolution).
pub const W1_FAMILY_DS18B20: u8 = 0x28;
/// DS2413 dual-channel addressable switch.
pub const W1_FAMILY_DS2413: u8 = 0x3A;
/// DS2431 1K EEPROM.
pub const W1_FAMILY_DS2431: u8 = 0x2D;
/// DS2438 battery monitor.
pub const W1_FAMILY_DS2438: u8 = 0x26;
/// DS2502 1K EPROM (iButton).
pub const W1_FAMILY_DS2502: u8 = 0x09;

// ---------------------------------------------------------------------------
// 1-Wire netlink message types
// ---------------------------------------------------------------------------

/// Master added event.
pub const W1_MASTER_ADD: u32 = 0;
/// Master removed event.
pub const W1_MASTER_REMOVE: u32 = 1;
/// Slave added event.
pub const W1_SLAVE_ADD: u32 = 2;
/// Slave removed event.
pub const W1_SLAVE_REMOVE: u32 = 3;
/// List masters request.
pub const W1_LIST_MASTERS: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rom_commands_distinct() {
        let cmds = [
            W1_CMD_READ_ROM, W1_CMD_MATCH_ROM, W1_CMD_SKIP_ROM,
            W1_CMD_SEARCH_ROM, W1_CMD_ALARM_SEARCH,
            W1_CMD_OVERDRIVE_SKIP, W1_CMD_OVERDRIVE_MATCH,
            W1_CMD_RESUME,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_family_codes_distinct() {
        let fams = [
            W1_FAMILY_DS18S20, W1_FAMILY_DS18B20, W1_FAMILY_DS2413,
            W1_FAMILY_DS2431, W1_FAMILY_DS2438, W1_FAMILY_DS2502,
        ];
        for i in 0..fams.len() {
            for j in (i + 1)..fams.len() {
                assert_ne!(fams[i], fams[j]);
            }
        }
    }

    #[test]
    fn test_netlink_messages_distinct() {
        let msgs = [
            W1_MASTER_ADD, W1_MASTER_REMOVE,
            W1_SLAVE_ADD, W1_SLAVE_REMOVE, W1_LIST_MASTERS,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }
}
