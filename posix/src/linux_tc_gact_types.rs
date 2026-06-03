//! `<linux/tc_act/tc_gact.h>` — TC generic action constants.
//!
//! Traffic control generic action constants covering attribute types
//! and probability distribution modes.

// ---------------------------------------------------------------------------
// TC gact attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_GACT_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_GACT_TM: u32 = 1;
/// Parameters.
pub const TCA_GACT_PARMS: u32 = 2;
/// Probability.
pub const TCA_GACT_PROB: u32 = 3;

// ---------------------------------------------------------------------------
// TC gact probability modes
// ---------------------------------------------------------------------------

/// No probability.
pub const PGACT_NONE: u32 = 0;
/// Netrand.
pub const PGACT_NETRAND: u32 = 1;
/// Determ.
pub const PGACT_DETERM: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [TCA_GACT_UNSPEC, TCA_GACT_TM, TCA_GACT_PARMS, TCA_GACT_PROB];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_prob_modes_distinct() {
        let modes = [PGACT_NONE, PGACT_NETRAND, PGACT_DETERM];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
