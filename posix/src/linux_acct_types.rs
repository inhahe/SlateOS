//! `<linux/acct.h>` — Process accounting constants.
//!
//! BSD-style process accounting records resource usage for every
//! process that exits. When enabled (via acct(2) syscall), the kernel
//! appends a fixed-size record to an accounting file for each exiting
//! process. Records include user/system CPU time, elapsed time,
//! memory usage, I/O counts, and exit status. Used for billing,
//! chargeback, capacity planning, and historical workload analysis
//! in multi-user systems.

// ---------------------------------------------------------------------------
// Accounting record flags (ac_flag)
// ---------------------------------------------------------------------------

/// Process forked but didn't exec.
pub const AFORK: u32 = 0x01;
/// Process used superuser privileges.
pub const ASU: u32 = 0x02;
/// Process ran on compatibility mode.
pub const ACOMPAT: u32 = 0x04;
/// Process dumped core.
pub const ACORE: u32 = 0x08;
/// Process was killed by a signal.
pub const AXSIG: u32 = 0x10;
/// Process had expanded memory.
pub const AEXPND: u32 = 0x20;

// ---------------------------------------------------------------------------
// Accounting versions
// ---------------------------------------------------------------------------

/// Accounting version 1 (original BSD).
pub const ACCT_VERSION_1: u32 = 1;
/// Accounting version 2 (extended, larger fields).
pub const ACCT_VERSION_2: u32 = 2;
/// Accounting version 3 (acct_v3, current Linux default).
pub const ACCT_VERSION_3: u32 = 3;

// ---------------------------------------------------------------------------
// Accounting file format constants
// ---------------------------------------------------------------------------

/// Size of command name field (ac_comm).
pub const ACCT_COMM: u32 = 16;

/// Comp_t encoding: mantissa bits.
pub const ACCT_COMP_MANTISSA: u32 = 13;
/// Comp_t encoding: exponent bits.
pub const ACCT_COMP_EXPONENT: u32 = 3;

// ---------------------------------------------------------------------------
// Accounting byte order
// ---------------------------------------------------------------------------

/// Little-endian accounting record.
pub const ACCT_BYTEORDER_LE: u32 = 0x00;
/// Big-endian accounting record.
pub const ACCT_BYTEORDER_BE: u32 = 0x80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [AFORK, ASU, ACOMPAT, ACORE, AXSIG, AEXPND];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_versions_distinct() {
        let vers = [ACCT_VERSION_1, ACCT_VERSION_2, ACCT_VERSION_3];
        for i in 0..vers.len() {
            for j in (i + 1)..vers.len() {
                assert_ne!(vers[i], vers[j]);
            }
        }
    }

    #[test]
    fn test_comm_size() {
        assert_eq!(ACCT_COMM, 16);
    }

    #[test]
    fn test_comp_t_encoding() {
        // comp_t is a 16-bit floating point: 13-bit mantissa + 3-bit exponent
        assert_eq!(ACCT_COMP_MANTISSA + ACCT_COMP_EXPONENT, 16);
    }

    #[test]
    fn test_byte_orders_distinct() {
        assert_ne!(ACCT_BYTEORDER_LE, ACCT_BYTEORDER_BE);
    }

    #[test]
    fn test_flags_are_powers_of_two() {
        let flags = [AFORK, ASU, ACOMPAT, ACORE, AXSIG, AEXPND];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }
}
