//! `<linux/clk.h>` — clk_rate_request layout and CCF default rates.
//!
//! The `struct clk_rate_request` passes a desired rate plus min/max
//! bounds down the clock tree so that providers can negotiate the best
//! rate. This module covers the field offsets and a set of standard
//! reference rates used by oscillators.

// ---------------------------------------------------------------------------
// struct clk_rate_request field offsets (kernel layout, 64-bit)
// ---------------------------------------------------------------------------

/// `unsigned long rate` — the target rate.
pub const CLK_RATE_REQ_OFF_RATE: usize = 0;
/// `unsigned long min_rate` — lower bound.
pub const CLK_RATE_REQ_OFF_MIN_RATE: usize = 8;
/// `unsigned long max_rate` — upper bound.
pub const CLK_RATE_REQ_OFF_MAX_RATE: usize = 16;
/// `unsigned long best_parent_rate` — selected parent's rate.
pub const CLK_RATE_REQ_OFF_BEST_PARENT_RATE: usize = 24;
/// `struct clk_hw *best_parent_hw` — selected parent.
pub const CLK_RATE_REQ_OFF_BEST_PARENT_HW: usize = 32;
/// `struct clk_core *core` — the core requesting.
pub const CLK_RATE_REQ_OFF_CORE: usize = 40;
/// Total struct size (six 8-byte fields).
pub const CLK_RATE_REQ_SIZE: usize = 48;

// ---------------------------------------------------------------------------
// Standard oscillator reference rates (Hz)
// ---------------------------------------------------------------------------

/// 32.768 kHz watch crystal (RTC reference).
pub const CLK_RATE_32K768: u64 = 32_768;
/// 1 MHz.
pub const CLK_RATE_1MHZ: u64 = 1_000_000;
/// 24 MHz (common oscillator on ARM SoCs).
pub const CLK_RATE_24MHZ: u64 = 24_000_000;
/// 25 MHz (gigabit-Ethernet PHY reference).
pub const CLK_RATE_25MHZ: u64 = 25_000_000;
/// 26 MHz (BCM, mobile SoCs).
pub const CLK_RATE_26MHZ: u64 = 26_000_000;
/// 27 MHz (legacy MPEG-2 / video).
pub const CLK_RATE_27MHZ: u64 = 27_000_000;
/// 48 MHz (USB 1.1 / 2.0 reference).
pub const CLK_RATE_48MHZ: u64 = 48_000_000;
/// 50 MHz (fast Ethernet RMII).
pub const CLK_RATE_50MHZ: u64 = 50_000_000;
/// 100 MHz (PCIe ref).
pub const CLK_RATE_100MHZ: u64 = 100_000_000;
/// 125 MHz (GMII / 1000BASE-T).
pub const CLK_RATE_125MHZ: u64 = 125_000_000;

// ---------------------------------------------------------------------------
// Maximum clock rate the framework supports
// ---------------------------------------------------------------------------

/// Maximum representable rate (unsigned long max on 64-bit).
pub const CLK_RATE_MAX: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_request_layout_consecutive_u64s() {
        let o = [
            CLK_RATE_REQ_OFF_RATE,
            CLK_RATE_REQ_OFF_MIN_RATE,
            CLK_RATE_REQ_OFF_MAX_RATE,
            CLK_RATE_REQ_OFF_BEST_PARENT_RATE,
            CLK_RATE_REQ_OFF_BEST_PARENT_HW,
            CLK_RATE_REQ_OFF_CORE,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * 8);
        }
        assert_eq!(CLK_RATE_REQ_SIZE, 6 * 8);
    }

    #[test]
    fn test_min_le_target_le_max_meaning() {
        // The min slot comes between rate and max — natural ordering.
        assert!(CLK_RATE_REQ_OFF_RATE < CLK_RATE_REQ_OFF_MIN_RATE);
        assert!(CLK_RATE_REQ_OFF_MIN_RATE < CLK_RATE_REQ_OFF_MAX_RATE);
    }

    #[test]
    fn test_oscillator_rates_increasing() {
        let rates = [
            CLK_RATE_32K768,
            CLK_RATE_1MHZ,
            CLK_RATE_24MHZ,
            CLK_RATE_25MHZ,
            CLK_RATE_26MHZ,
            CLK_RATE_27MHZ,
            CLK_RATE_48MHZ,
            CLK_RATE_50MHZ,
            CLK_RATE_100MHZ,
            CLK_RATE_125MHZ,
        ];
        for w in rates.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn test_standard_mhz_rates_are_exact_multiples() {
        assert_eq!(CLK_RATE_24MHZ / CLK_RATE_1MHZ, 24);
        assert_eq!(CLK_RATE_25MHZ / CLK_RATE_1MHZ, 25);
        assert_eq!(CLK_RATE_26MHZ / CLK_RATE_1MHZ, 26);
        assert_eq!(CLK_RATE_27MHZ / CLK_RATE_1MHZ, 27);
        assert_eq!(CLK_RATE_48MHZ / CLK_RATE_1MHZ, 48);
        assert_eq!(CLK_RATE_50MHZ / CLK_RATE_1MHZ, 50);
        assert_eq!(CLK_RATE_100MHZ / CLK_RATE_1MHZ, 100);
        assert_eq!(CLK_RATE_125MHZ / CLK_RATE_1MHZ, 125);
    }

    #[test]
    fn test_32k_watch_crystal_is_pow2() {
        // 32_768 = 2^15. The watch crystal is exactly 2^15 Hz.
        assert_eq!(CLK_RATE_32K768, 1 << 15);
    }

    #[test]
    fn test_rate_max_is_u64_max() {
        assert_eq!(CLK_RATE_MAX, u64::MAX);
    }
}
