//! `<linux/hdreg.h>` — IDE/ATA hard drive interface.
//!
//! Provides ioctl constants for querying and controlling IDE/ATA
//! disk drives.

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Get device identity (struct hd_driveid).
pub const HDIO_GET_IDENTITY: u64 = 0x030D;
/// Get 32-bit I/O flag.
pub const HDIO_GET_32BIT: u64 = 0x0309;
/// Set 32-bit I/O flag.
pub const HDIO_SET_32BIT: u64 = 0x0324;
/// Get multcount.
pub const HDIO_GET_MULTCOUNT: u64 = 0x0304;
/// Set multcount.
pub const HDIO_SET_MULTCOUNT: u64 = 0x0321;
/// Get WCACHE flag.
pub const HDIO_GET_WCACHE: u64 = 0x030E;
/// Set WCACHE flag.
pub const HDIO_SET_WCACHE: u64 = 0x032B;
/// Get DMA flag.
pub const HDIO_GET_DMA: u64 = 0x030B;
/// Set DMA flag.
pub const HDIO_SET_DMA: u64 = 0x0326;
/// Get unmaskirq flag.
pub const HDIO_GET_UNMASKINTR: u64 = 0x0302;
/// Set unmaskirq flag.
pub const HDIO_SET_UNMASKINTR: u64 = 0x0322;
/// Get keep settings flag.
pub const HDIO_GET_KEEPSETTINGS: u64 = 0x0308;
/// Set keep settings flag.
pub const HDIO_SET_KEEPSETTINGS: u64 = 0x0323;
/// Get nice flags.
pub const HDIO_GET_NICE: u64 = 0x030C;
/// Set nice flags.
pub const HDIO_SET_NICE: u64 = 0x0329;
/// Get acoustic setting.
pub const HDIO_GET_ACOUSTIC: u64 = 0x030F;
/// Set acoustic setting.
pub const HDIO_SET_ACOUSTIC: u64 = 0x032C;
/// Get busstate.
pub const HDIO_GET_BUSSTATE: u64 = 0x031A;
/// Set busstate.
pub const HDIO_SET_BUSSTATE: u64 = 0x032D;
/// Issue ATA taskfile command.
pub const HDIO_DRIVE_TASKFILE: u64 = 0x031D;
/// Issue ATA command directly.
pub const HDIO_DRIVE_CMD: u64 = 0x031F;
/// Issue ATA task command.
pub const HDIO_DRIVE_TASK: u64 = 0x031E;
/// Reset controller.
pub const HDIO_DRIVE_RESET: u64 = 0x031C;

// ---------------------------------------------------------------------------
// Bus states
// ---------------------------------------------------------------------------

/// Bus is off.
pub const BUSSTATE_OFF: i32 = 0;
/// Bus is on.
pub const BUSSTATE_ON: i32 = 1;
/// Bus is in tristate.
pub const BUSSTATE_TRISTATE: i32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            HDIO_GET_IDENTITY,
            HDIO_GET_32BIT,
            HDIO_SET_32BIT,
            HDIO_GET_MULTCOUNT,
            HDIO_SET_MULTCOUNT,
            HDIO_GET_DMA,
            HDIO_SET_DMA,
            HDIO_DRIVE_CMD,
            HDIO_DRIVE_TASK,
            HDIO_DRIVE_RESET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_get_set_pairs() {
        // Get and set should differ.
        assert_ne!(HDIO_GET_32BIT, HDIO_SET_32BIT);
        assert_ne!(HDIO_GET_DMA, HDIO_SET_DMA);
        assert_ne!(HDIO_GET_WCACHE, HDIO_SET_WCACHE);
        assert_ne!(HDIO_GET_MULTCOUNT, HDIO_SET_MULTCOUNT);
    }

    #[test]
    fn test_busstate_values() {
        assert_eq!(BUSSTATE_OFF, 0);
        assert_eq!(BUSSTATE_ON, 1);
        assert_eq!(BUSSTATE_TRISTATE, 2);
    }

    #[test]
    fn test_identity_ioctl() {
        assert_eq!(HDIO_GET_IDENTITY, 0x030D);
    }
}
