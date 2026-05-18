//! `<linux/pinctrl/pinmux.h>` — Pin multiplexing function constants.
//!
//! Pin multiplexing (pinmux) selects which function a SoC pin
//! performs (GPIO, UART, SPI, I2C, etc.). Each pin can serve multiple
//! functions depending on the mux configuration. The pinctrl
//! subsystem manages both muxing and electrical configuration.

// ---------------------------------------------------------------------------
// Pinmux function types
// ---------------------------------------------------------------------------

/// Pin is in GPIO mode.
pub const PINMUX_FUNC_GPIO: u32 = 0;
/// Pin is in alternate function 1 (device-specific).
pub const PINMUX_FUNC_ALT1: u32 = 1;
/// Pin is in alternate function 2.
pub const PINMUX_FUNC_ALT2: u32 = 2;
/// Pin is in alternate function 3.
pub const PINMUX_FUNC_ALT3: u32 = 3;
/// Pin is in alternate function 4.
pub const PINMUX_FUNC_ALT4: u32 = 4;
/// Pin is in alternate function 5.
pub const PINMUX_FUNC_ALT5: u32 = 5;
/// Pin is in analog mode (no digital function).
pub const PINMUX_FUNC_ANALOG: u32 = 6;

// ---------------------------------------------------------------------------
// Pinctrl configuration types (generic)
// ---------------------------------------------------------------------------

/// Configure as input.
pub const PIN_CONFIG_INPUT_ENABLE: u32 = 0;
/// Configure as output.
pub const PIN_CONFIG_OUTPUT_ENABLE: u32 = 1;
/// Enable bias pull-up.
pub const PIN_CONFIG_BIAS_PULL_UP: u32 = 2;
/// Enable bias pull-down.
pub const PIN_CONFIG_BIAS_PULL_DOWN: u32 = 3;
/// Disable bias (high-impedance/floating).
pub const PIN_CONFIG_BIAS_DISABLE: u32 = 4;
/// Set drive strength (mA).
pub const PIN_CONFIG_DRIVE_STRENGTH: u32 = 5;
/// Open-drain output mode.
pub const PIN_CONFIG_DRIVE_OPEN_DRAIN: u32 = 6;
/// Push-pull output mode.
pub const PIN_CONFIG_DRIVE_PUSH_PULL: u32 = 7;
/// Open-source output mode.
pub const PIN_CONFIG_DRIVE_OPEN_SOURCE: u32 = 8;
/// Set slew rate (speed).
pub const PIN_CONFIG_SLEW_RATE: u32 = 9;
/// Enable input schmitt trigger.
pub const PIN_CONFIG_INPUT_SCHMITT_ENABLE: u32 = 10;
/// Power source selection (voltage domain).
pub const PIN_CONFIG_POWER_SOURCE: u32 = 11;

// ---------------------------------------------------------------------------
// Pinctrl state names (common predefined states)
// ---------------------------------------------------------------------------

/// Default state (active operation).
pub const PINCTRL_STATE_DEFAULT: u32 = 0;
/// Idle state (device not in use).
pub const PINCTRL_STATE_IDLE: u32 = 1;
/// Sleep state (system suspend).
pub const PINCTRL_STATE_SLEEP: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinmux_funcs_distinct() {
        let funcs = [
            PINMUX_FUNC_GPIO, PINMUX_FUNC_ALT1, PINMUX_FUNC_ALT2,
            PINMUX_FUNC_ALT3, PINMUX_FUNC_ALT4, PINMUX_FUNC_ALT5,
            PINMUX_FUNC_ANALOG,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_pin_configs_distinct() {
        let cfgs = [
            PIN_CONFIG_INPUT_ENABLE, PIN_CONFIG_OUTPUT_ENABLE,
            PIN_CONFIG_BIAS_PULL_UP, PIN_CONFIG_BIAS_PULL_DOWN,
            PIN_CONFIG_BIAS_DISABLE, PIN_CONFIG_DRIVE_STRENGTH,
            PIN_CONFIG_DRIVE_OPEN_DRAIN, PIN_CONFIG_DRIVE_PUSH_PULL,
            PIN_CONFIG_DRIVE_OPEN_SOURCE, PIN_CONFIG_SLEW_RATE,
            PIN_CONFIG_INPUT_SCHMITT_ENABLE, PIN_CONFIG_POWER_SOURCE,
        ];
        for i in 0..cfgs.len() {
            for j in (i + 1)..cfgs.len() {
                assert_ne!(cfgs[i], cfgs[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(PINCTRL_STATE_DEFAULT, PINCTRL_STATE_IDLE);
        assert_ne!(PINCTRL_STATE_IDLE, PINCTRL_STATE_SLEEP);
    }
}
