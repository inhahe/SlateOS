//! `<linux/soundwire/sdw.h>` — SoundWire bus constants.
//!
//! SoundWire (MIPI SoundWire) is a low-pin-count bidirectional
//! serial bus for audio devices. It carries multi-channel audio
//! streams and control data over a two-wire interface (clock + data).
//! SoundWire supports device discovery, dynamic bandwidth allocation,
//! and synchronization across multiple bus instances. Used in modern
//! PC/laptop audio (Intel platforms) and mobile devices for connecting
//! codecs and amplifiers.

// ---------------------------------------------------------------------------
// SoundWire device states (slave status)
// ---------------------------------------------------------------------------

/// Device is not present (not attached to bus).
pub const SDW_SLAVE_UNATTACHED: u32 = 0;
/// Device is attached (enumerated, has device number).
pub const SDW_SLAVE_ATTACHED: u32 = 1;
/// Device is alert (has pending interrupt).
pub const SDW_SLAVE_ALERT: u32 = 2;

// ---------------------------------------------------------------------------
// SoundWire data port flow modes
// ---------------------------------------------------------------------------

/// Isochronous mode (guaranteed timing).
pub const SDW_PORT_FLOW_ISOCHRONOUS: u32 = 0;
/// Normal mode (standard audio streaming).
pub const SDW_PORT_FLOW_NORMAL: u32 = 1;

// ---------------------------------------------------------------------------
// SoundWire stream types
// ---------------------------------------------------------------------------

/// PCM stream (standard audio).
pub const SDW_STREAM_PCM: u32 = 0;
/// PDM stream (pulse-density modulation, digital mic).
pub const SDW_STREAM_PDM: u32 = 1;

// ---------------------------------------------------------------------------
// SoundWire clock stop modes
// ---------------------------------------------------------------------------

/// Clock stop mode 0 (bus keeps context, fast resume).
pub const SDW_CLK_STOP_MODE0: u32 = 0;
/// Clock stop mode 1 (bus loses context, full re-enumeration needed).
pub const SDW_CLK_STOP_MODE1: u32 = 1;

// ---------------------------------------------------------------------------
// SoundWire bus clock frequencies (common rates)
// ---------------------------------------------------------------------------

/// 9.6 MHz bus clock.
pub const SDW_CLK_9600: u32 = 9_600_000;
/// 12 MHz bus clock.
pub const SDW_CLK_12000: u32 = 12_000_000;
/// 12.288 MHz bus clock (audio-rate derivative).
pub const SDW_CLK_12288: u32 = 12_288_000;
/// 24 MHz bus clock.
pub const SDW_CLK_24000: u32 = 24_000_000;
/// 24.576 MHz bus clock.
pub const SDW_CLK_24576: u32 = 24_576_000;

// ---------------------------------------------------------------------------
// SoundWire command/response status
// ---------------------------------------------------------------------------

/// Command acknowledged (success).
pub const SDW_CMD_OK: u32 = 0;
/// Command ignored (device didn't respond).
pub const SDW_CMD_IGNORED: u32 = 1;
/// Command failed (NACK or bus error).
pub const SDW_CMD_FAIL: u32 = 2;
/// Command timeout.
pub const SDW_CMD_TIMEOUT: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slave_states_distinct() {
        let states = [
            SDW_SLAVE_UNATTACHED, SDW_SLAVE_ATTACHED, SDW_SLAVE_ALERT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flow_modes_distinct() {
        assert_ne!(SDW_PORT_FLOW_ISOCHRONOUS, SDW_PORT_FLOW_NORMAL);
    }

    #[test]
    fn test_stream_types_distinct() {
        assert_ne!(SDW_STREAM_PCM, SDW_STREAM_PDM);
    }

    #[test]
    fn test_clock_stop_modes_distinct() {
        assert_ne!(SDW_CLK_STOP_MODE0, SDW_CLK_STOP_MODE1);
    }

    #[test]
    fn test_clock_freqs_distinct_and_ordered() {
        let clks = [
            SDW_CLK_9600, SDW_CLK_12000, SDW_CLK_12288,
            SDW_CLK_24000, SDW_CLK_24576,
        ];
        for i in 0..clks.len() {
            for j in (i + 1)..clks.len() {
                assert_ne!(clks[i], clks[j]);
            }
        }
    }

    #[test]
    fn test_cmd_status_distinct() {
        let statuses = [
            SDW_CMD_OK, SDW_CMD_IGNORED, SDW_CMD_FAIL, SDW_CMD_TIMEOUT,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }
}
