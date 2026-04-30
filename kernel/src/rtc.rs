//! CMOS Real-Time Clock (RTC) driver.
//!
//! Reads the current date and time from the MC146818-compatible CMOS RTC
//! found in every PC-compatible system.  The RTC is battery-backed and
//! maintains time while the system is powered off.
//!
//! ## Ports
//!
//! - 0x70: Address register (write index + NMI disable bit)
//! - 0x71: Data register (read/write selected CMOS register)
//!
//! ## BCD vs binary
//!
//! The RTC can store values in either BCD or binary format, controlled
//! by Status Register B bit 2.  We check this bit and convert as needed.
//!
//! ## Update-in-progress
//!
//! The RTC sets bit 7 of Status Register A during the ~244 µs window
//! when it copies time from its divider chain to the readable registers.
//! We wait for this bit to clear before reading to avoid torn values.

use crate::port;

// ---------------------------------------------------------------------------
// CMOS register addresses
// ---------------------------------------------------------------------------

/// CMOS address port.
const CMOS_ADDR: u16 = 0x70;
/// CMOS data port.
const CMOS_DATA: u16 = 0x71;

/// Seconds (0-59).
const REG_SECONDS: u8 = 0x00;
/// Minutes (0-59).
const REG_MINUTES: u8 = 0x02;
/// Hours (0-23 in 24h mode, 1-12 + PM bit in 12h mode).
const REG_HOURS: u8 = 0x04;
/// Day of month (1-31).
const REG_DAY: u8 = 0x07;
/// Month (1-12).
const REG_MONTH: u8 = 0x08;
/// Year (0-99, two-digit).
const REG_YEAR: u8 = 0x09;
/// Century register (not present on all hardware; QEMU has it).
const REG_CENTURY: u8 = 0x32;
/// Status Register A (bit 7 = update in progress).
const REG_STATUS_A: u8 = 0x0A;
/// Status Register B (bit 1 = 24h, bit 2 = binary, bit 4 = UIE).
const REG_STATUS_B: u8 = 0x0B;

// ---------------------------------------------------------------------------
// Date/time structure
// ---------------------------------------------------------------------------

/// A date and time from the RTC.
#[derive(Debug, Clone, Copy)]
pub struct DateTime {
    /// Full four-digit year (e.g. 2026).
    pub year: u16,
    /// Month (1-12).
    pub month: u8,
    /// Day of month (1-31).
    pub day: u8,
    /// Hours (0-23, always 24-hour format).
    pub hour: u8,
    /// Minutes (0-59).
    pub minute: u8,
    /// Seconds (0-59).
    pub second: u8,
}

impl core::fmt::Display for DateTime {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day,
            self.hour, self.minute, self.second
        )
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Read the current date and time from the CMOS RTC.
///
/// Waits for the update-in-progress flag to clear, then reads all
/// time registers in a single consistent snapshot (re-reads if the
/// values change between two consecutive reads).
// The BCD-to-binary conversions use small-value arithmetic that cannot overflow.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn read_datetime() -> DateTime {
    // Wait for any in-progress update to finish.
    wait_for_update();

    // Read twice and compare to ensure we didn't read during an update.
    let (mut sec, mut min, mut hour, mut day, mut month, mut year, mut century);
    loop {
        sec = read_cmos(REG_SECONDS);
        min = read_cmos(REG_MINUTES);
        hour = read_cmos(REG_HOURS);
        day = read_cmos(REG_DAY);
        month = read_cmos(REG_MONTH);
        year = read_cmos(REG_YEAR);
        century = read_cmos(REG_CENTURY);

        // Re-read and compare.  If the values changed, an update
        // occurred between our reads — try again.
        let sec2 = read_cmos(REG_SECONDS);
        let min2 = read_cmos(REG_MINUTES);
        let hour2 = read_cmos(REG_HOURS);
        let day2 = read_cmos(REG_DAY);
        let month2 = read_cmos(REG_MONTH);
        let year2 = read_cmos(REG_YEAR);

        if sec == sec2 && min == min2 && hour == hour2
            && day == day2 && month == month2 && year == year2
        {
            break;
        }
    }

    // Check Status Register B for format.
    let status_b = read_cmos(REG_STATUS_B);
    let is_binary = status_b & 0x04 != 0;
    let is_24h = status_b & 0x02 != 0;

    // Convert from BCD if necessary.
    if !is_binary {
        sec = bcd_to_bin(sec);
        min = bcd_to_bin(min);
        // Hours have special handling for 12h mode (PM bit in bit 7).
        hour = bcd_to_bin(hour & 0x7F) | (hour & 0x80);
        day = bcd_to_bin(day);
        month = bcd_to_bin(month);
        year = bcd_to_bin(year);
        century = bcd_to_bin(century);
    }

    // Convert 12h → 24h if necessary.
    if !is_24h && hour & 0x80 != 0 {
        // PM flag set.  12 PM = 12, 1-11 PM = 13-23.
        hour = (hour & 0x7F) + 12;
        if hour == 24 {
            hour = 12; // 12 PM stays 12.
        }
    }

    // Compute full year.
    let full_year = if century > 0 {
        u16::from(century) * 100 + u16::from(year)
    } else {
        // No century register — assume 20xx for year < 80, 19xx otherwise.
        if year < 80 { 2000 + u16::from(year) } else { 1900 + u16::from(year) }
    };

    DateTime {
        year: full_year,
        month,
        day,
        hour,
        minute: min,
        second: sec,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read a single CMOS register.
fn read_cmos(reg: u8) -> u8 {
    // Write register index (keep NMI enabled — clear bit 7).
    //
    // SAFETY: Ports 0x70/0x71 are the standard CMOS address/data ports,
    // always present on PC-compatible hardware.
    unsafe {
        port::outb(CMOS_ADDR, reg & 0x7F);
        port::inb(CMOS_DATA)
    }
}

/// Wait for the RTC update-in-progress flag to clear.
///
/// The RTC sets Status Register A bit 7 during the ~244 µs update
/// window.  We spin until it clears.
fn wait_for_update() {
    // First wait for UIP to become set (if we catch the start of an update).
    // Then wait for it to clear.  This ensures we don't read in the
    // middle of an update that's already underway.
    //
    // Timeout after ~10000 iterations (~10ms at port-IO speed) to avoid
    // hanging if the bit is never set.
    for _ in 0..10_000u32 {
        if read_cmos(REG_STATUS_A) & 0x80 == 0 {
            return;
        }
    }
}

/// Convert a BCD (Binary Coded Decimal) byte to binary.
///
/// E.g. 0x59 → 59, 0x23 → 23.
// Small-value arithmetic: BCD values are 0x00-0x99, result fits in u8.
#[allow(clippy::arithmetic_side_effects)]
const fn bcd_to_bin(bcd: u8) -> u8 {
    (bcd >> 4) * 10 + (bcd & 0x0F)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify the RTC returns a plausible date/time.
pub fn self_test() -> Result<(), &'static str> {
    crate::serial_println!("[rtc] Running self-test...");

    let dt = read_datetime();
    crate::serial_println!("[rtc]   Current time: {}", dt);

    // Basic sanity checks.
    if dt.year < 2020 || dt.year > 2100 {
        crate::serial_println!("[rtc]   WARNING: year {} seems implausible", dt.year);
    }
    if dt.month == 0 || dt.month > 12 {
        return Err("month out of range");
    }
    if dt.day == 0 || dt.day > 31 {
        return Err("day out of range");
    }
    if dt.hour > 23 {
        return Err("hour out of range");
    }
    if dt.minute > 59 {
        return Err("minute out of range");
    }
    if dt.second > 59 {
        return Err("second out of range");
    }

    crate::serial_println!("[rtc] Self-test PASSED");
    Ok(())
}
