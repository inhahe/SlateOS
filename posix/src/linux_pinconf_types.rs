//! `<linux/pinctrl/pinconf-generic.h>` — pin configuration parameter types.
//!
//! Pin configuration parameters control electrical characteristics of
//! SoC pins: bias (pull-up, pull-down), drive strength, slew rate,
//! schmitt trigger, input enable, etc. These are used by device tree
//! bindings to configure pins for their intended function.

// ---------------------------------------------------------------------------
// Pin configuration parameters (PIN_CONFIG_*)
// ---------------------------------------------------------------------------

/// Bias: pull-up resistor.
pub const PIN_CONFIG_BIAS_PULL_UP: u32 = 1;
/// Bias: pull-down resistor.
pub const PIN_CONFIG_BIAS_PULL_DOWN: u32 = 2;
/// Bias: disable (floating, high-impedance).
pub const PIN_CONFIG_BIAS_DISABLE: u32 = 3;
/// Bias: bus-hold (maintain last driven level).
pub const PIN_CONFIG_BIAS_BUS_HOLD: u32 = 4;
/// Bias: high impedance.
pub const PIN_CONFIG_BIAS_HIGH_IMPEDANCE: u32 = 5;
/// Drive: push-pull (standard CMOS output).
pub const PIN_CONFIG_DRIVE_PUSH_PULL: u32 = 6;
/// Drive: open-drain (wired-AND).
pub const PIN_CONFIG_DRIVE_OPEN_DRAIN: u32 = 7;
/// Drive: open-source (wired-OR).
pub const PIN_CONFIG_DRIVE_OPEN_SOURCE: u32 = 8;
/// Drive strength (in milliamps).
pub const PIN_CONFIG_DRIVE_STRENGTH: u32 = 9;
/// Drive strength (in microamps).
pub const PIN_CONFIG_DRIVE_STRENGTH_UA: u32 = 10;
/// Input enable.
pub const PIN_CONFIG_INPUT_ENABLE: u32 = 11;
/// Input debounce (in microseconds).
pub const PIN_CONFIG_INPUT_DEBOUNCE: u32 = 12;
/// Input Schmitt trigger enable.
pub const PIN_CONFIG_INPUT_SCHMITT_ENABLE: u32 = 13;
/// Input Schmitt trigger threshold level.
pub const PIN_CONFIG_INPUT_SCHMITT: u32 = 14;
/// Low power mode.
pub const PIN_CONFIG_LOW_POWER_MODE: u32 = 15;
/// Output enable.
pub const PIN_CONFIG_OUTPUT_ENABLE: u32 = 16;
/// Output value (high/low).
pub const PIN_CONFIG_OUTPUT: u32 = 17;
/// Power source (voltage level).
pub const PIN_CONFIG_POWER_SOURCE: u32 = 18;
/// Sleep hardware state (pin state during suspend).
pub const PIN_CONFIG_SLEEP_HARDWARE_STATE: u32 = 19;
/// Slew rate.
pub const PIN_CONFIG_SLEW_RATE: u32 = 20;
/// Skew delay.
pub const PIN_CONFIG_SKEW_DELAY: u32 = 21;
/// Persist state across suspend/resume.
pub const PIN_CONFIG_PERSIST_STATE: u32 = 22;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configs_distinct() {
        let configs = [
            PIN_CONFIG_BIAS_PULL_UP,
            PIN_CONFIG_BIAS_PULL_DOWN,
            PIN_CONFIG_BIAS_DISABLE,
            PIN_CONFIG_BIAS_BUS_HOLD,
            PIN_CONFIG_BIAS_HIGH_IMPEDANCE,
            PIN_CONFIG_DRIVE_PUSH_PULL,
            PIN_CONFIG_DRIVE_OPEN_DRAIN,
            PIN_CONFIG_DRIVE_OPEN_SOURCE,
            PIN_CONFIG_DRIVE_STRENGTH,
            PIN_CONFIG_DRIVE_STRENGTH_UA,
            PIN_CONFIG_INPUT_ENABLE,
            PIN_CONFIG_INPUT_DEBOUNCE,
            PIN_CONFIG_INPUT_SCHMITT_ENABLE,
            PIN_CONFIG_INPUT_SCHMITT,
            PIN_CONFIG_LOW_POWER_MODE,
            PIN_CONFIG_OUTPUT_ENABLE,
            PIN_CONFIG_OUTPUT,
            PIN_CONFIG_POWER_SOURCE,
            PIN_CONFIG_SLEEP_HARDWARE_STATE,
            PIN_CONFIG_SLEW_RATE,
            PIN_CONFIG_SKEW_DELAY,
            PIN_CONFIG_PERSIST_STATE,
        ];
        for i in 0..configs.len() {
            for j in (i + 1)..configs.len() {
                assert_ne!(configs[i], configs[j]);
            }
        }
    }

    #[test]
    fn test_bias_group() {
        assert!(PIN_CONFIG_BIAS_PULL_UP < PIN_CONFIG_BIAS_HIGH_IMPEDANCE);
    }

    #[test]
    fn test_configs_sequential() {
        assert_eq!(PIN_CONFIG_BIAS_PULL_UP, 1);
        assert_eq!(PIN_CONFIG_PERSIST_STATE, 22);
    }
}
