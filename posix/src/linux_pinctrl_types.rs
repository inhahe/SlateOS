//! `<linux/pinctrl/pinctrl.h>` — Pin control subsystem constants.
//!
//! The pinctrl framework manages pin multiplexing (selecting which
//! function a SoC pin performs) and pin configuration (pull-up/down,
//! drive strength, slew rate). Nearly every embedded SoC has a pin
//! controller that determines GPIO vs. UART vs. SPI vs. I2C, etc.

// ---------------------------------------------------------------------------
// Pin function types
// ---------------------------------------------------------------------------

/// GPIO function.
pub const PINCTRL_FUNC_GPIO: u8 = 0;
/// UART function.
pub const PINCTRL_FUNC_UART: u8 = 1;
/// I2C function.
pub const PINCTRL_FUNC_I2C: u8 = 2;
/// SPI function.
pub const PINCTRL_FUNC_SPI: u8 = 3;
/// PWM function.
pub const PINCTRL_FUNC_PWM: u8 = 4;
/// I2S (audio) function.
pub const PINCTRL_FUNC_I2S: u8 = 5;

// ---------------------------------------------------------------------------
// Pin configuration types (PIN_CONFIG_*)
// ---------------------------------------------------------------------------

/// Enable bias pull-up.
pub const PIN_CONFIG_BIAS_PULL_UP: u16 = 0;
/// Enable bias pull-down.
pub const PIN_CONFIG_BIAS_PULL_DOWN: u16 = 1;
/// Disable bias (high-Z).
pub const PIN_CONFIG_BIAS_DISABLE: u16 = 2;
/// High impedance.
pub const PIN_CONFIG_BIAS_HIGH_IMPEDANCE: u16 = 3;
/// Set drive strength (mA).
pub const PIN_CONFIG_DRIVE_STRENGTH: u16 = 4;
/// Open drain output.
pub const PIN_CONFIG_DRIVE_OPEN_DRAIN: u16 = 5;
/// Open source output.
pub const PIN_CONFIG_DRIVE_OPEN_SOURCE: u16 = 6;
/// Push-pull output.
pub const PIN_CONFIG_DRIVE_PUSH_PULL: u16 = 7;
/// Input enable.
pub const PIN_CONFIG_INPUT_ENABLE: u16 = 8;
/// Output enable.
pub const PIN_CONFIG_OUTPUT_ENABLE: u16 = 9;
/// Output value (high/low).
pub const PIN_CONFIG_OUTPUT: u16 = 10;
/// Slew rate.
pub const PIN_CONFIG_SLEW_RATE: u16 = 11;
/// Input debounce (microseconds).
pub const PIN_CONFIG_INPUT_DEBOUNCE: u16 = 12;
/// Power source voltage (millivolts).
pub const PIN_CONFIG_POWER_SOURCE: u16 = 13;

// ---------------------------------------------------------------------------
// Pin control states
// ---------------------------------------------------------------------------

/// Default state name.
pub const PINCTRL_STATE_DEFAULT: &str = "default";
/// Idle state (low-power pin config).
pub const PINCTRL_STATE_IDLE: &str = "idle";
/// Sleep state (suspend pin config).
pub const PINCTRL_STATE_SLEEP: &str = "sleep";
/// Init state (during probe).
pub const PINCTRL_STATE_INIT: &str = "init";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_func_types_distinct() {
        let funcs = [
            PINCTRL_FUNC_GPIO, PINCTRL_FUNC_UART, PINCTRL_FUNC_I2C,
            PINCTRL_FUNC_SPI, PINCTRL_FUNC_PWM, PINCTRL_FUNC_I2S,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_config_types_distinct() {
        let configs = [
            PIN_CONFIG_BIAS_PULL_UP, PIN_CONFIG_BIAS_PULL_DOWN,
            PIN_CONFIG_BIAS_DISABLE, PIN_CONFIG_BIAS_HIGH_IMPEDANCE,
            PIN_CONFIG_DRIVE_STRENGTH, PIN_CONFIG_DRIVE_OPEN_DRAIN,
            PIN_CONFIG_DRIVE_OPEN_SOURCE, PIN_CONFIG_DRIVE_PUSH_PULL,
            PIN_CONFIG_INPUT_ENABLE, PIN_CONFIG_OUTPUT_ENABLE,
            PIN_CONFIG_OUTPUT, PIN_CONFIG_SLEW_RATE,
            PIN_CONFIG_INPUT_DEBOUNCE, PIN_CONFIG_POWER_SOURCE,
        ];
        for i in 0..configs.len() {
            for j in (i + 1)..configs.len() {
                assert_ne!(configs[i], configs[j]);
            }
        }
    }

    #[test]
    fn test_state_names_distinct() {
        let states = [
            PINCTRL_STATE_DEFAULT, PINCTRL_STATE_IDLE,
            PINCTRL_STATE_SLEEP, PINCTRL_STATE_INIT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
