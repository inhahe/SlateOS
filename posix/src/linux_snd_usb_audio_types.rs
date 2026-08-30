//! `<sound/usb_audio.h>` — USB Audio Class constants.
//!
//! USB Audio Class (UAC) defines the protocol for USB audio devices
//! (headsets, microphones, DACs, audio interfaces). UAC 1.0 supports
//! up to 24-bit/96kHz; UAC 2.0 adds high-speed support for higher
//! sample rates and more channels; UAC 3.0 adds power management
//! and device classification. The Linux USB audio driver handles
//! device enumeration, format negotiation, clock management, and
//! isochronous streaming.

// ---------------------------------------------------------------------------
// USB Audio Class versions
// ---------------------------------------------------------------------------

/// USB Audio Class 1.0.
pub const UAC_VERSION_1: u32 = 0x0100;
/// USB Audio Class 2.0.
pub const UAC_VERSION_2: u32 = 0x0200;
/// USB Audio Class 3.0.
pub const UAC_VERSION_3: u32 = 0x0300;

// ---------------------------------------------------------------------------
// USB Audio interface subtypes
// ---------------------------------------------------------------------------

/// Header descriptor.
pub const UAC_SUBTYPE_HEADER: u32 = 0x01;
/// Input terminal.
pub const UAC_SUBTYPE_INPUT_TERMINAL: u32 = 0x02;
/// Output terminal.
pub const UAC_SUBTYPE_OUTPUT_TERMINAL: u32 = 0x03;
/// Mixer unit.
pub const UAC_SUBTYPE_MIXER_UNIT: u32 = 0x04;
/// Selector unit (input mux).
pub const UAC_SUBTYPE_SELECTOR_UNIT: u32 = 0x05;
/// Feature unit (volume, mute, etc.).
pub const UAC_SUBTYPE_FEATURE_UNIT: u32 = 0x06;
/// Effect unit (reverb, etc., UAC2+).
pub const UAC_SUBTYPE_EFFECT_UNIT: u32 = 0x07;
/// Processing unit.
pub const UAC_SUBTYPE_PROCESSING_UNIT: u32 = 0x08;
/// Extension unit.
pub const UAC_SUBTYPE_EXTENSION_UNIT: u32 = 0x09;
/// Clock source (UAC2+).
pub const UAC_SUBTYPE_CLOCK_SOURCE: u32 = 0x0A;
/// Clock selector (UAC2+).
pub const UAC_SUBTYPE_CLOCK_SELECTOR: u32 = 0x0B;
/// Clock multiplier (UAC2+).
pub const UAC_SUBTYPE_CLOCK_MULTIPLIER: u32 = 0x0C;

// ---------------------------------------------------------------------------
// USB Audio feature unit control selectors
// ---------------------------------------------------------------------------

/// Mute control.
pub const UAC_FU_MUTE: u32 = 0x01;
/// Volume control.
pub const UAC_FU_VOLUME: u32 = 0x02;
/// Bass control.
pub const UAC_FU_BASS: u32 = 0x03;
/// Treble control.
pub const UAC_FU_TREBLE: u32 = 0x05;
/// Automatic gain control.
pub const UAC_FU_AGC: u32 = 0x07;

// ---------------------------------------------------------------------------
// USB Audio endpoint attributes
// ---------------------------------------------------------------------------

/// Asynchronous endpoint (device is clock master).
pub const UAC_EP_ASYNC: u32 = 0x01;
/// Adaptive endpoint (device adapts to host clock).
pub const UAC_EP_ADAPTIVE: u32 = 0x02;
/// Synchronous endpoint (locked to SOF).
pub const UAC_EP_SYNC: u32 = 0x03;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_ordered() {
        assert!(UAC_VERSION_1 < UAC_VERSION_2);
        assert!(UAC_VERSION_2 < UAC_VERSION_3);
    }

    #[test]
    fn test_subtypes_distinct() {
        let types = [
            UAC_SUBTYPE_HEADER,
            UAC_SUBTYPE_INPUT_TERMINAL,
            UAC_SUBTYPE_OUTPUT_TERMINAL,
            UAC_SUBTYPE_MIXER_UNIT,
            UAC_SUBTYPE_SELECTOR_UNIT,
            UAC_SUBTYPE_FEATURE_UNIT,
            UAC_SUBTYPE_EFFECT_UNIT,
            UAC_SUBTYPE_PROCESSING_UNIT,
            UAC_SUBTYPE_EXTENSION_UNIT,
            UAC_SUBTYPE_CLOCK_SOURCE,
            UAC_SUBTYPE_CLOCK_SELECTOR,
            UAC_SUBTYPE_CLOCK_MULTIPLIER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_feature_controls_distinct() {
        let ctrls = [
            UAC_FU_MUTE,
            UAC_FU_VOLUME,
            UAC_FU_BASS,
            UAC_FU_TREBLE,
            UAC_FU_AGC,
        ];
        for i in 0..ctrls.len() {
            for j in (i + 1)..ctrls.len() {
                assert_ne!(ctrls[i], ctrls[j]);
            }
        }
    }

    #[test]
    fn test_endpoint_attrs_distinct() {
        let eps = [UAC_EP_ASYNC, UAC_EP_ADAPTIVE, UAC_EP_SYNC];
        for i in 0..eps.len() {
            for j in (i + 1)..eps.len() {
                assert_ne!(eps[i], eps[j]);
            }
        }
    }
}
