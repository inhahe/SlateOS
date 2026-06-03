//! `<linux/remoteproc_cdev.h>` — remoteproc user character-device API.
//!
//! remoteproc manages auxiliary CPUs/DSPs on SoCs (Cortex-M, Hexagon,
//! Cadence, etc.). Newer kernels expose a per-rproc character device
//! at `/dev/remoteproc<N>` so userspace can load firmware and
//! start/stop the remote without sysfs. Constants below cover the
//! ioctl numbers, state values, and firmware-name length limit.

// ---------------------------------------------------------------------------
// ioctl group letter / numbers
// ---------------------------------------------------------------------------

/// Magic byte for /dev/remoteproc ioctls (`RPROC_MAGIC`).
pub const RPROC_MAGIC: u8 = 0xb7;

/// `RPROC_SET_SHUTDOWN_ON_RELEASE` — request the remote be shut down
/// when the cdev is closed.
pub const RPROC_SET_SHUTDOWN_ON_RELEASE: u32 = 1;
/// `RPROC_GET_SHUTDOWN_ON_RELEASE` — query the current setting.
pub const RPROC_GET_SHUTDOWN_ON_RELEASE: u32 = 2;

// ---------------------------------------------------------------------------
// Remote processor states (sysfs / cdev "state" string)
// ---------------------------------------------------------------------------

/// Initial state — not yet probed.
pub const RPROC_OFFLINE: u32 = 0;
/// Suspended (held in low-power mode).
pub const RPROC_SUSPENDED: u32 = 1;
/// Running.
pub const RPROC_RUNNING: u32 = 2;
/// Crashed; awaiting recovery.
pub const RPROC_CRASHED: u32 = 3;
/// Reset / shutting down.
pub const RPROC_DELETED: u32 = 4;
/// Recovering after a crash.
pub const RPROC_ATTACHED: u32 = 5;
/// Detached (cdev closed, firmware retained).
pub const RPROC_DETACHED: u32 = 6;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum length of a firmware-name string.
pub const RPROC_MAX_FW_NAME: u32 = 128;
/// Maximum length of a remoteproc name string.
pub const RPROC_MAX_NAME_LEN: u32 = 64;

// ---------------------------------------------------------------------------
// Crash report reasons (sent via uevent)
// ---------------------------------------------------------------------------

/// MMU fault from the remote CPU.
pub const RPROC_MMUFAULT: u32 = 0;
/// Internal-watchdog fired.
pub const RPROC_WATCHDOG: u32 = 1;
/// Fatal error reported by firmware.
pub const RPROC_FATAL_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_byte() {
        // 0xb7 was chosen by the rproc cdev addition (5.13) to avoid
        // conflict with existing ioctl groups.
        assert_eq!(RPROC_MAGIC, 0xb7);
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(
            RPROC_SET_SHUTDOWN_ON_RELEASE,
            RPROC_GET_SHUTDOWN_ON_RELEASE
        );
    }

    #[test]
    fn test_states_distinct() {
        let s = [
            RPROC_OFFLINE,
            RPROC_SUSPENDED,
            RPROC_RUNNING,
            RPROC_CRASHED,
            RPROC_DELETED,
            RPROC_ATTACHED,
            RPROC_DETACHED,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
        // OFFLINE must be 0 so a zeroed control block reads as
        // not-yet-loaded.
        assert_eq!(RPROC_OFFLINE, 0);
    }

    #[test]
    fn test_name_limits_sane() {
        assert!(RPROC_MAX_FW_NAME.is_power_of_two());
        assert!(RPROC_MAX_NAME_LEN.is_power_of_two());
        assert!(RPROC_MAX_NAME_LEN < RPROC_MAX_FW_NAME);
    }

    #[test]
    fn test_crash_reasons_distinct() {
        let r = [RPROC_MMUFAULT, RPROC_WATCHDOG, RPROC_FATAL_ERROR];
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
    }
}
