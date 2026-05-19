//! `<linux/tc_act/tc_mirred.h>` — TC mirred action constants.
//!
//! Traffic control mirror/redirect action constants covering
//! attribute types and action commands.

// ---------------------------------------------------------------------------
// TC mirred action commands
// ---------------------------------------------------------------------------

/// Egress redirect.
pub const TCA_EGRESS_REDIR: u32 = 1;
/// Egress mirror.
pub const TCA_EGRESS_MIRROR: u32 = 2;
/// Ingress redirect.
pub const TCA_INGRESS_REDIR: u32 = 3;
/// Ingress mirror.
pub const TCA_INGRESS_MIRROR: u32 = 4;

// ---------------------------------------------------------------------------
// TC mirred attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_MIRRED_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_MIRRED_TM: u32 = 1;
/// Parameters.
pub const TCA_MIRRED_PARMS: u32 = 2;
/// Blockid.
pub const TCA_MIRRED_BLOCKID: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_cmds_distinct() {
        let cmds = [
            TCA_EGRESS_REDIR, TCA_EGRESS_MIRROR,
            TCA_INGRESS_REDIR, TCA_INGRESS_MIRROR,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_MIRRED_UNSPEC, TCA_MIRRED_TM,
            TCA_MIRRED_PARMS, TCA_MIRRED_BLOCKID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
