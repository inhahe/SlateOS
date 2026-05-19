//! `<linux/rose.h>` — Additional ROSE constants.
//!
//! Supplementary ROSE (X.25 over AX.25) networking constants covering
//! socket options, diagnostic codes, and protocol parameters.

// ---------------------------------------------------------------------------
// ROSE socket options
// ---------------------------------------------------------------------------

/// Defer.
pub const ROSE_DEFER: u32 = 1;
/// Timer T1.
pub const ROSE_T1: u32 = 2;
/// Timer T2.
pub const ROSE_T2: u32 = 3;
/// Timer T3.
pub const ROSE_T3: u32 = 4;
/// Idle timer.
pub const ROSE_IDLE: u32 = 5;
/// Queue length.
pub const ROSE_QBITINCL: u32 = 6;
/// Holdback timer.
pub const ROSE_HOLDBACK: u32 = 7;

// ---------------------------------------------------------------------------
// ROSE diagnostic codes
// ---------------------------------------------------------------------------

/// No diagnostic.
pub const ROSE_DTE_ORIGINATED: u32 = 0x00;
/// Number busy.
pub const ROSE_NUMBER_BUSY: u32 = 0x01;
/// Invalid facility.
pub const ROSE_INVALID_FACILITY: u32 = 0x03;
/// Network congestion.
pub const ROSE_NETWORK_CONGESTION: u32 = 0x05;
/// Out of order.
pub const ROSE_OUT_OF_ORDER: u32 = 0x09;
/// Access barred.
pub const ROSE_ACCESS_BARRED: u32 = 0x0B;
/// Not obtainable.
pub const ROSE_NOT_OBTAINABLE: u32 = 0x0D;
/// Remote procedure error.
pub const ROSE_REMOTE_PROCEDURE: u32 = 0x11;
/// Local procedure error.
pub const ROSE_LOCAL_PROCEDURE: u32 = 0x13;
/// Ship absent.
pub const ROSE_SHIP_ABSENT: u32 = 0x39;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sockopts_distinct() {
        let opts = [
            ROSE_DEFER, ROSE_T1, ROSE_T2, ROSE_T3,
            ROSE_IDLE, ROSE_QBITINCL, ROSE_HOLDBACK,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_diagnostics_distinct() {
        let diags = [
            ROSE_DTE_ORIGINATED, ROSE_NUMBER_BUSY,
            ROSE_INVALID_FACILITY, ROSE_NETWORK_CONGESTION,
            ROSE_OUT_OF_ORDER, ROSE_ACCESS_BARRED,
            ROSE_NOT_OBTAINABLE, ROSE_REMOTE_PROCEDURE,
            ROSE_LOCAL_PROCEDURE, ROSE_SHIP_ABSENT,
        ];
        for i in 0..diags.len() {
            for j in (i + 1)..diags.len() {
                assert_ne!(diags[i], diags[j]);
            }
        }
    }
}
