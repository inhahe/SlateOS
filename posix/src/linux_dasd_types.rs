//! `<asm/dasd.h>` (s390) — IBM DASD (Direct Access Storage Device) ioctls.
//!
//! DASD is the s390 mainframe block-device driver. Userspace tools
//! (`dasdfmt`, `dasdview`, lsdasd) consume these ioctl numbers and
//! status flags to format, query, and control ECKD/FBA volumes on
//! Linux on Z.

// ---------------------------------------------------------------------------
// ioctl group magic
// ---------------------------------------------------------------------------

/// DASD ioctl base ('D').
pub const DASD_IOCTL_LETTER: u8 = b'D';

// ---------------------------------------------------------------------------
// Common ioctl numbers (4th-byte nr; full encoding adds direction/size)
// ---------------------------------------------------------------------------

/// `BIODASDDISABLE` — disable the device (offline).
pub const BIODASDDISABLE: u32 = 0x4400;
/// `BIODASDENABLE` — re-enable the device.
pub const BIODASDENABLE: u32 = 0x4401;
/// `BIODASDRSRV` — reserve the device.
pub const BIODASDRSRV: u32 = 0x4402;
/// `BIODASDRLSE` — release the device.
pub const BIODASDRLSE: u32 = 0x4403;
/// `BIODASDSLCK` — steal lock.
pub const BIODASDSLCK: u32 = 0x4404;
/// `BIODASDINFO` — get info (struct dasd_information_t).
pub const BIODASDINFO: u32 = 0x80108001;
/// `BIODASDFMT` — format the device.
pub const BIODASDFMT: u32 = 0x40308001;

// ---------------------------------------------------------------------------
// Device-state codes (dasd_information_t.status)
// ---------------------------------------------------------------------------

/// Device is new (no I/O yet).
pub const DASD_DEVICE_STATUS_NEW: u32 = 0;
/// Device known to the driver.
pub const DASD_DEVICE_STATUS_KNOWN: u32 = 1;
/// Device basic-functions enabled.
pub const DASD_DEVICE_STATUS_BASIC: u32 = 2;
/// Device unformatted.
pub const DASD_DEVICE_STATUS_UNFMT: u32 = 3;
/// Device ready (formatted, online).
pub const DASD_DEVICE_STATUS_READY: u32 = 4;
/// Device online and accepting I/O.
pub const DASD_DEVICE_STATUS_ONLINE: u32 = 5;

// ---------------------------------------------------------------------------
// Format intent codes (dasd_format_data_t.intensity)
// ---------------------------------------------------------------------------

/// Default-format intensity (whole device, write home addresses + R0).
pub const DASD_FMT_INT_FMT_R0: u32 = 1 << 0;
/// Format home addresses too.
pub const DASD_FMT_INT_FMT_HA: u32 = 1 << 1;
/// Write zero pattern.
pub const DASD_FMT_INT_INVAL: u32 = 1 << 2;
/// Compatible (mode 1) format.
pub const DASD_FMT_INT_COMPAT: u32 = 1 << 3;
/// Stop-on-error.
pub const DASD_FMT_INT_FMT_NOR0: u32 = 1 << 4;
/// Disable cache.
pub const DASD_FMT_INT_FEATURE_ERP: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Buffer / name sizes
// ---------------------------------------------------------------------------

/// Bus-ID buffer length used in dasd_information.
pub const DASD_BUS_ID_SIZE: u32 = 20;
/// Device-type buffer length.
pub const DASD_DEV_TYPE_SIZE: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_letter() {
        // 'D' is the historical letter for DASD ioctls on Linux/s390.
        assert_eq!(DASD_IOCTL_LETTER, b'D');
    }

    #[test]
    fn test_basic_ioctls_distinct() {
        let i = [
            BIODASDDISABLE,
            BIODASDENABLE,
            BIODASDRSRV,
            BIODASDRLSE,
            BIODASDSLCK,
            BIODASDINFO,
            BIODASDFMT,
        ];
        for x in 0..i.len() {
            for y in (x + 1)..i.len() {
                assert_ne!(i[x], i[y]);
            }
        }
    }

    #[test]
    fn test_device_states_monotonic() {
        // States move monotonically NEW -> KNOWN -> BASIC -> ... -> ONLINE.
        assert!(DASD_DEVICE_STATUS_NEW < DASD_DEVICE_STATUS_KNOWN);
        assert!(DASD_DEVICE_STATUS_KNOWN < DASD_DEVICE_STATUS_BASIC);
        assert!(DASD_DEVICE_STATUS_BASIC < DASD_DEVICE_STATUS_UNFMT);
        assert!(DASD_DEVICE_STATUS_UNFMT < DASD_DEVICE_STATUS_READY);
        assert!(DASD_DEVICE_STATUS_READY < DASD_DEVICE_STATUS_ONLINE);
    }

    #[test]
    fn test_format_intensity_bits_distinct_pow2() {
        let f = [
            DASD_FMT_INT_FMT_R0,
            DASD_FMT_INT_FMT_HA,
            DASD_FMT_INT_INVAL,
            DASD_FMT_INT_COMPAT,
            DASD_FMT_INT_FMT_NOR0,
            DASD_FMT_INT_FEATURE_ERP,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_buffer_sizes_sane() {
        assert!(DASD_BUS_ID_SIZE >= 8);
        assert_eq!(DASD_DEV_TYPE_SIZE, 4);
    }
}
