//! `<linux/netfilter/nfnetlink.h>` — Additional nfnetlink constants.
//!
//! Supplementary nfnetlink constants covering subsystem IDs,
//! message types, and batch flags.

// ---------------------------------------------------------------------------
// Nfnetlink subsystem IDs (NFNL_SUBSYS_*)
// ---------------------------------------------------------------------------

/// No subsystem.
pub const NFNL_SUBSYS_NONE: u32 = 0;
/// Conntrack subsystem.
pub const NFNL_SUBSYS_CTNETLINK: u32 = 1;
/// Conntrack expect subsystem.
pub const NFNL_SUBSYS_CTNETLINK_EXP: u32 = 2;
/// Queuing subsystem.
pub const NFNL_SUBSYS_QUEUE: u32 = 3;
/// Ulog subsystem.
pub const NFNL_SUBSYS_ULOG: u32 = 4;
/// OSF subsystem.
pub const NFNL_SUBSYS_OSF: u32 = 5;
/// IP sets subsystem.
pub const NFNL_SUBSYS_IPSET: u32 = 6;
/// Accounting subsystem.
pub const NFNL_SUBSYS_ACCT: u32 = 7;
/// Conntrack timeout subsystem.
pub const NFNL_SUBSYS_CTNETLINK_TIMEOUT: u32 = 8;
/// Conntrack helper subsystem.
pub const NFNL_SUBSYS_CTHELPER: u32 = 9;
/// Nftables subsystem.
pub const NFNL_SUBSYS_NFTABLES: u32 = 10;
/// NFT compat subsystem.
pub const NFNL_SUBSYS_NFT_COMPAT: u32 = 11;
/// Hook subsystem.
pub const NFNL_SUBSYS_HOOK: u32 = 12;

/// Number of subsystems.
pub const NFNL_SUBSYS_COUNT: u32 = 13;

// ---------------------------------------------------------------------------
// Nfnetlink message flags
// ---------------------------------------------------------------------------

/// Request is part of a batch.
pub const NLM_F_BATCH: u32 = 0x0400;
/// Create if not exists.
pub const NLM_F_NF_CREATE: u32 = 0x0800;
/// Exclusive create.
pub const NLM_F_NF_EXCL: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Nfnetlink batch message types
// ---------------------------------------------------------------------------

/// Begin batch.
pub const NFNL_MSG_BATCH_BEGIN: u32 = 0x0010;
/// End batch.
pub const NFNL_MSG_BATCH_END: u32 = 0x0011;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subsys_ids_distinct() {
        let ids = [
            NFNL_SUBSYS_NONE,
            NFNL_SUBSYS_CTNETLINK,
            NFNL_SUBSYS_CTNETLINK_EXP,
            NFNL_SUBSYS_QUEUE,
            NFNL_SUBSYS_ULOG,
            NFNL_SUBSYS_OSF,
            NFNL_SUBSYS_IPSET,
            NFNL_SUBSYS_ACCT,
            NFNL_SUBSYS_CTNETLINK_TIMEOUT,
            NFNL_SUBSYS_CTHELPER,
            NFNL_SUBSYS_NFTABLES,
            NFNL_SUBSYS_NFT_COMPAT,
            NFNL_SUBSYS_HOOK,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_subsys_count() {
        assert_eq!(NFNL_SUBSYS_COUNT, 13);
        assert_eq!(NFNL_SUBSYS_HOOK + 1, NFNL_SUBSYS_COUNT);
    }

    #[test]
    fn test_msg_flags_distinct() {
        let flags = [NLM_F_BATCH, NLM_F_NF_CREATE, NLM_F_NF_EXCL];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_batch_msgs_distinct() {
        assert_ne!(NFNL_MSG_BATCH_BEGIN, NFNL_MSG_BATCH_END);
    }

    #[test]
    fn test_none_is_zero() {
        assert_eq!(NFNL_SUBSYS_NONE, 0);
    }
}
