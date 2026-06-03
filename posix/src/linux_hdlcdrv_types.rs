//! `<linux/hdlcdrv.h>` — HDLC driver userspace ioctl constants.
//!
//! Constants for the legacy HDLC (High-level Data Link Control)
//! driver userspace interface used by AX.25 packet radio modems
//! (e.g. baycom) on PC parallel-port hardware.

// ---------------------------------------------------------------------------
// ioctl command IDs (placed in the second word of the ifr_data union)
// ---------------------------------------------------------------------------

/// Get channel parameters.
pub const HDLCDRVCTL_GETCHANNELPAR: u32 = 0;
/// Set channel parameters.
pub const HDLCDRVCTL_SETCHANNELPAR: u32 = 1;
/// Get modem parameters.
pub const HDLCDRVCTL_GETMODEMPAR: u32 = 2;
/// Set modem parameters.
pub const HDLCDRVCTL_SETMODEMPAR: u32 = 3;
/// Get driver name string.
pub const HDLCDRVCTL_GETMODEMINFO: u32 = 4;
/// Calibrate (transmit unmodulated carrier for N seconds).
pub const HDLCDRVCTL_CALIBRATE: u32 = 5;
/// Get current driver statistics.
pub const HDLCDRVCTL_GETSTAT: u32 = 6;
/// Diagnostic: get raw PTT, DCD lines etc.
pub const HDLCDRVCTL_OLDGETSTAT: u32 = 7;
/// Get sample buffer (debug).
pub const HDLCDRVCTL_DRIVERNAME: u32 = 8;

// ---------------------------------------------------------------------------
// hdlcdrv_channel_params field ranges
// ---------------------------------------------------------------------------

/// Minimum TX delay (10 ms units).
pub const HDLCDRV_TXDELAY_MIN: u32 = 0;
/// Maximum TX delay.
pub const HDLCDRV_TXDELAY_MAX: u32 = 255;

/// Minimum P-persistence value (out of 255).
pub const HDLCDRV_PPERSIST_MIN: u32 = 0;
/// Maximum P-persistence value.
pub const HDLCDRV_PPERSIST_MAX: u32 = 255;

/// Minimum slot time (10 ms units).
pub const HDLCDRV_SLOTTIME_MIN: u32 = 0;
/// Maximum slot time.
pub const HDLCDRV_SLOTTIME_MAX: u32 = 255;

/// Minimum TX-tail time (10 ms units).
pub const HDLCDRV_TXTAIL_MIN: u32 = 0;
/// Maximum TX-tail time.
pub const HDLCDRV_TXTAIL_MAX: u32 = 255;

/// Full-duplex mode enabled.
pub const HDLCDRV_FULLDUPLEX: u32 = 1;
/// Half-duplex mode (default).
pub const HDLCDRV_HALFDUPLEX: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_ids_distinct() {
        let ids = [
            HDLCDRVCTL_GETCHANNELPAR,
            HDLCDRVCTL_SETCHANNELPAR,
            HDLCDRVCTL_GETMODEMPAR,
            HDLCDRVCTL_SETMODEMPAR,
            HDLCDRVCTL_GETMODEMINFO,
            HDLCDRVCTL_CALIBRATE,
            HDLCDRVCTL_GETSTAT,
            HDLCDRVCTL_OLDGETSTAT,
            HDLCDRVCTL_DRIVERNAME,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_param_ranges_sane() {
        assert!(HDLCDRV_TXDELAY_MIN < HDLCDRV_TXDELAY_MAX);
        assert!(HDLCDRV_PPERSIST_MIN < HDLCDRV_PPERSIST_MAX);
        assert!(HDLCDRV_SLOTTIME_MIN < HDLCDRV_SLOTTIME_MAX);
        assert!(HDLCDRV_TXTAIL_MIN < HDLCDRV_TXTAIL_MAX);
        // All AX.25 timing fields are unsigned bytes — max value 255.
        assert_eq!(HDLCDRV_TXDELAY_MAX, 255);
        assert_eq!(HDLCDRV_PPERSIST_MAX, 255);
        assert_eq!(HDLCDRV_SLOTTIME_MAX, 255);
        assert_eq!(HDLCDRV_TXTAIL_MAX, 255);
    }

    #[test]
    fn test_duplex_modes_distinct() {
        assert_ne!(HDLCDRV_FULLDUPLEX, HDLCDRV_HALFDUPLEX);
    }
}
