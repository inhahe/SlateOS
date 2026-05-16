//! `<linux/pinctrl.h>` — Pin control subsystem constants.
//!
//! The pinctrl subsystem manages SoC pin multiplexing and
//! electrical configuration (pull-up/down, drive strength,
//! input enable, etc.). Drivers register pin controllers;
//! device tree or ACPI bindings select pin functions.

// ---------------------------------------------------------------------------
// Pin config types (PIN_CONFIG_*)
// ---------------------------------------------------------------------------

/// Bias: bus hold.
pub const PIN_CONFIG_BIAS_BUS_HOLD: u32 = 0;
/// Bias: disable (high-Z).
pub const PIN_CONFIG_BIAS_DISABLE: u32 = 1;
/// Bias: high impedance.
pub const PIN_CONFIG_BIAS_HIGH_IMPEDANCE: u32 = 2;
/// Bias: pull-down.
pub const PIN_CONFIG_BIAS_PULL_DOWN: u32 = 3;
/// Bias: pull-pin default.
pub const PIN_CONFIG_BIAS_PULL_PIN_DEFAULT: u32 = 4;
/// Bias: pull-up.
pub const PIN_CONFIG_BIAS_PULL_UP: u32 = 5;
/// Drive: open drain.
pub const PIN_CONFIG_DRIVE_OPEN_DRAIN: u32 = 6;
/// Drive: open source.
pub const PIN_CONFIG_DRIVE_OPEN_SOURCE: u32 = 7;
/// Drive: push-pull.
pub const PIN_CONFIG_DRIVE_PUSH_PULL: u32 = 8;
/// Drive strength (mA).
pub const PIN_CONFIG_DRIVE_STRENGTH: u32 = 9;
/// Drive strength (uA).
pub const PIN_CONFIG_DRIVE_STRENGTH_UA: u32 = 10;
/// Input debounce (usec).
pub const PIN_CONFIG_INPUT_DEBOUNCE: u32 = 11;
/// Input enable.
pub const PIN_CONFIG_INPUT_ENABLE: u32 = 12;
/// Input Schmitt enable.
pub const PIN_CONFIG_INPUT_SCHMITT: u32 = 13;
/// Input Schmitt trigger enable.
pub const PIN_CONFIG_INPUT_SCHMITT_ENABLE: u32 = 14;
/// Low power mode.
pub const PIN_CONFIG_MODE_LOW_POWER: u32 = 15;
/// PWM mode.
pub const PIN_CONFIG_MODE_PWM: u32 = 16;
/// Output.
pub const PIN_CONFIG_OUTPUT: u32 = 17;
/// Output enable.
pub const PIN_CONFIG_OUTPUT_ENABLE: u32 = 18;
/// Output impedance.
pub const PIN_CONFIG_OUTPUT_IMPEDANCE_OHMS: u32 = 19;
/// Persist state across sleep.
pub const PIN_CONFIG_PERSIST_STATE: u32 = 20;
/// Power source (voltage rail).
pub const PIN_CONFIG_POWER_SOURCE: u32 = 21;
/// Skew delay.
pub const PIN_CONFIG_SKEW_DELAY: u32 = 22;
/// Sleep hardware state.
pub const PIN_CONFIG_SLEEP_HARDWARE_STATE: u32 = 23;
/// Slew rate.
pub const PIN_CONFIG_SLEW_RATE: u32 = 24;
/// End marker.
pub const PIN_CONFIG_END: u32 = 0x7F;
/// Config max.
pub const PIN_CONFIG_MAX: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Pin mux function types
// ---------------------------------------------------------------------------

/// GPIO function.
pub const PINMUX_GPIO: u32 = 0;
/// Alternate function base.
pub const PINMUX_FUNC_BASE: u32 = 1;

// ---------------------------------------------------------------------------
// Pinctrl states
// ---------------------------------------------------------------------------

/// Default state name.
pub const PINCTRL_STATE_DEFAULT: &str = "default";
/// Init state name.
pub const PINCTRL_STATE_INIT: &str = "init";
/// Idle state name.
pub const PINCTRL_STATE_IDLE: &str = "idle";
/// Sleep state name.
pub const PINCTRL_STATE_SLEEP: &str = "sleep";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_types_distinct() {
        let configs = [
            PIN_CONFIG_BIAS_BUS_HOLD, PIN_CONFIG_BIAS_DISABLE,
            PIN_CONFIG_BIAS_HIGH_IMPEDANCE, PIN_CONFIG_BIAS_PULL_DOWN,
            PIN_CONFIG_BIAS_PULL_PIN_DEFAULT, PIN_CONFIG_BIAS_PULL_UP,
            PIN_CONFIG_DRIVE_OPEN_DRAIN, PIN_CONFIG_DRIVE_OPEN_SOURCE,
            PIN_CONFIG_DRIVE_PUSH_PULL, PIN_CONFIG_DRIVE_STRENGTH,
            PIN_CONFIG_DRIVE_STRENGTH_UA, PIN_CONFIG_INPUT_DEBOUNCE,
            PIN_CONFIG_INPUT_ENABLE, PIN_CONFIG_INPUT_SCHMITT,
            PIN_CONFIG_INPUT_SCHMITT_ENABLE, PIN_CONFIG_MODE_LOW_POWER,
            PIN_CONFIG_MODE_PWM, PIN_CONFIG_OUTPUT,
            PIN_CONFIG_OUTPUT_ENABLE, PIN_CONFIG_OUTPUT_IMPEDANCE_OHMS,
            PIN_CONFIG_PERSIST_STATE, PIN_CONFIG_POWER_SOURCE,
            PIN_CONFIG_SKEW_DELAY, PIN_CONFIG_SLEEP_HARDWARE_STATE,
            PIN_CONFIG_SLEW_RATE,
        ];
        for i in 0..configs.len() {
            for j in (i + 1)..configs.len() {
                assert_ne!(configs[i], configs[j]);
            }
        }
    }

    #[test]
    fn test_config_types_sequential() {
        assert_eq!(PIN_CONFIG_BIAS_BUS_HOLD, 0);
        assert_eq!(PIN_CONFIG_SLEW_RATE, 24);
    }

    #[test]
    fn test_config_end_and_max() {
        assert_eq!(PIN_CONFIG_END, 0x7F);
        assert_eq!(PIN_CONFIG_MAX, 0xFF);
        assert!(PIN_CONFIG_END < PIN_CONFIG_MAX);
    }

    #[test]
    fn test_all_configs_below_end() {
        let configs = [
            PIN_CONFIG_BIAS_BUS_HOLD, PIN_CONFIG_BIAS_DISABLE,
            PIN_CONFIG_BIAS_HIGH_IMPEDANCE, PIN_CONFIG_BIAS_PULL_DOWN,
            PIN_CONFIG_BIAS_PULL_PIN_DEFAULT, PIN_CONFIG_BIAS_PULL_UP,
            PIN_CONFIG_DRIVE_OPEN_DRAIN, PIN_CONFIG_DRIVE_OPEN_SOURCE,
            PIN_CONFIG_DRIVE_PUSH_PULL, PIN_CONFIG_DRIVE_STRENGTH,
            PIN_CONFIG_DRIVE_STRENGTH_UA, PIN_CONFIG_INPUT_DEBOUNCE,
            PIN_CONFIG_INPUT_ENABLE, PIN_CONFIG_INPUT_SCHMITT,
            PIN_CONFIG_INPUT_SCHMITT_ENABLE, PIN_CONFIG_MODE_LOW_POWER,
            PIN_CONFIG_MODE_PWM, PIN_CONFIG_OUTPUT,
            PIN_CONFIG_OUTPUT_ENABLE, PIN_CONFIG_OUTPUT_IMPEDANCE_OHMS,
            PIN_CONFIG_PERSIST_STATE, PIN_CONFIG_POWER_SOURCE,
            PIN_CONFIG_SKEW_DELAY, PIN_CONFIG_SLEEP_HARDWARE_STATE,
            PIN_CONFIG_SLEW_RATE,
        ];
        for c in &configs {
            assert!(*c < PIN_CONFIG_END, "{}", c);
        }
    }

    #[test]
    fn test_pinmux_values() {
        assert_eq!(PINMUX_GPIO, 0);
        assert_eq!(PINMUX_FUNC_BASE, 1);
    }

    #[test]
    fn test_state_names_distinct() {
        let states = [
            PINCTRL_STATE_DEFAULT, PINCTRL_STATE_INIT,
            PINCTRL_STATE_IDLE, PINCTRL_STATE_SLEEP,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
