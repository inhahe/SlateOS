//! `<sound/hda_verbs.h>` — Intel HD Audio (HDA) codec verb constants.
//!
//! Intel HD Audio is the standard audio interface on modern PCs.
//! The HDA controller communicates with audio codecs (on the HDA
//! link bus) using "verbs" — commands sent to codec nodes. Each
//! codec has a tree of widgets (DACs, ADCs, mixers, pin complexes)
//! connected in a configurable signal path. The driver queries the
//! codec topology and configures the signal routing at runtime.

// ---------------------------------------------------------------------------
// HDA verb GET commands
// ---------------------------------------------------------------------------

/// Get connection select (which input is active).
pub const HDA_VERB_GET_CONNECT_SEL: u32 = 0x0F01;
/// Get connection list entry.
pub const HDA_VERB_GET_CONNECT_LIST: u32 = 0x0F02;
/// Get converter format (sample rate, bits, channels).
pub const HDA_VERB_GET_CONV_FORMAT: u32 = 0x0A00;
/// Get amplifier gain/mute.
pub const HDA_VERB_GET_AMP_GAIN_MUTE: u32 = 0x0B00;
/// Get processing coefficient.
pub const HDA_VERB_GET_PROC_COEF: u32 = 0x0C00;
/// Get coefficient index.
pub const HDA_VERB_GET_COEF_INDEX: u32 = 0x0D00;
/// Get pin widget control.
pub const HDA_VERB_GET_PIN_WIDGET_CONTROL: u32 = 0x0F07;
/// Get pin sense (jack detect, impedance).
pub const HDA_VERB_GET_PIN_SENSE: u32 = 0x0F09;
/// Get EAPD/BTL enable.
pub const HDA_VERB_GET_EAPD_BTL: u32 = 0x0F0C;
/// Get power state.
pub const HDA_VERB_GET_POWER_STATE: u32 = 0x0F05;
/// Get configuration default (BIOS pin config).
pub const HDA_VERB_GET_CONFIG_DEFAULT: u32 = 0x0F1C;

// ---------------------------------------------------------------------------
// HDA verb SET commands
// ---------------------------------------------------------------------------

/// Set connection select.
pub const HDA_VERB_SET_CONNECT_SEL: u32 = 0x0701;
/// Set converter format.
pub const HDA_VERB_SET_CONV_FORMAT: u32 = 0x0200;
/// Set amplifier gain/mute.
pub const HDA_VERB_SET_AMP_GAIN_MUTE: u32 = 0x0300;
/// Set pin widget control.
pub const HDA_VERB_SET_PIN_WIDGET_CONTROL: u32 = 0x0707;
/// Set power state.
pub const HDA_VERB_SET_POWER_STATE: u32 = 0x0705;
/// Set EAPD/BTL enable.
pub const HDA_VERB_SET_EAPD_BTL: u32 = 0x070C;
/// Set coefficient index.
pub const HDA_VERB_SET_COEF_INDEX: u32 = 0x0500;
/// Set processing coefficient.
pub const HDA_VERB_SET_PROC_COEF: u32 = 0x0400;

// ---------------------------------------------------------------------------
// HDA widget types (node types in codec topology)
// ---------------------------------------------------------------------------

/// Audio output (DAC).
pub const HDA_WIDGET_AUDIO_OUTPUT: u32 = 0x0;
/// Audio input (ADC).
pub const HDA_WIDGET_AUDIO_INPUT: u32 = 0x1;
/// Audio mixer.
pub const HDA_WIDGET_AUDIO_MIXER: u32 = 0x2;
/// Audio selector (mux).
pub const HDA_WIDGET_AUDIO_SELECTOR: u32 = 0x3;
/// Pin complex (jack/speaker/mic).
pub const HDA_WIDGET_PIN_COMPLEX: u32 = 0x4;
/// Power widget.
pub const HDA_WIDGET_POWER: u32 = 0x5;
/// Volume knob.
pub const HDA_WIDGET_VOLUME_KNOB: u32 = 0x6;
/// Beep generator.
pub const HDA_WIDGET_BEEP: u32 = 0x7;
/// Vendor defined widget.
pub const HDA_WIDGET_VENDOR: u32 = 0xF;

// ---------------------------------------------------------------------------
// HDA pin control flags
// ---------------------------------------------------------------------------

/// Pin output enable.
pub const HDA_PIN_OUT_EN: u32 = 1 << 6;
/// Pin input enable.
pub const HDA_PIN_IN_EN: u32 = 1 << 5;
/// Pin headphone amp enable.
pub const HDA_PIN_HP_EN: u32 = 1 << 7;
/// Pin voltage reference: Hi-Z.
pub const HDA_PIN_VREF_HIZ: u32 = 0x00;
/// Pin voltage reference: 50%.
pub const HDA_PIN_VREF_50: u32 = 0x01;
/// Pin voltage reference: 80%.
pub const HDA_PIN_VREF_80: u32 = 0x04;
/// Pin voltage reference: 100%.
pub const HDA_PIN_VREF_100: u32 = 0x05;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_verbs_distinct() {
        let verbs = [
            HDA_VERB_GET_CONNECT_SEL, HDA_VERB_GET_CONNECT_LIST,
            HDA_VERB_GET_CONV_FORMAT, HDA_VERB_GET_AMP_GAIN_MUTE,
            HDA_VERB_GET_PROC_COEF, HDA_VERB_GET_COEF_INDEX,
            HDA_VERB_GET_PIN_WIDGET_CONTROL, HDA_VERB_GET_PIN_SENSE,
            HDA_VERB_GET_EAPD_BTL, HDA_VERB_GET_POWER_STATE,
            HDA_VERB_GET_CONFIG_DEFAULT,
        ];
        for i in 0..verbs.len() {
            for j in (i + 1)..verbs.len() {
                assert_ne!(verbs[i], verbs[j]);
            }
        }
    }

    #[test]
    fn test_widget_types_distinct() {
        let types = [
            HDA_WIDGET_AUDIO_OUTPUT, HDA_WIDGET_AUDIO_INPUT,
            HDA_WIDGET_AUDIO_MIXER, HDA_WIDGET_AUDIO_SELECTOR,
            HDA_WIDGET_PIN_COMPLEX, HDA_WIDGET_POWER,
            HDA_WIDGET_VOLUME_KNOB, HDA_WIDGET_BEEP,
            HDA_WIDGET_VENDOR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pin_direction_flags_no_overlap() {
        // OUT_EN and IN_EN are separate bits
        assert_eq!(HDA_PIN_OUT_EN & HDA_PIN_IN_EN, 0);
        assert_eq!(HDA_PIN_OUT_EN & HDA_PIN_HP_EN, 0);
        assert_eq!(HDA_PIN_IN_EN & HDA_PIN_HP_EN, 0);
    }
}
