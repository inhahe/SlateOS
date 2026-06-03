//! `<linux/nf_tables.h>` — Further nftables constants.
//!
//! Further nftables constants covering object types,
//! table/chain flags, and data types.

// ---------------------------------------------------------------------------
// Nftables object types
// ---------------------------------------------------------------------------

/// Unspec.
pub const NFT_OBJECT_UNSPEC: u32 = 0;
/// Counter.
pub const NFT_OBJECT_COUNTER: u32 = 1;
/// Quota.
pub const NFT_OBJECT_QUOTA: u32 = 2;
/// Connection limit.
pub const NFT_OBJECT_CT_HELPER: u32 = 3;
/// Limit.
pub const NFT_OBJECT_LIMIT: u32 = 4;
/// Conntrack timeout.
pub const NFT_OBJECT_CT_TIMEOUT: u32 = 5;
/// Security mark.
pub const NFT_OBJECT_SECMARK: u32 = 6;
/// Conntrack expectation.
pub const NFT_OBJECT_CT_EXPECT: u32 = 7;
/// Synproxy.
pub const NFT_OBJECT_SYNPROXY: u32 = 8;

// ---------------------------------------------------------------------------
// Nftables table flags
// ---------------------------------------------------------------------------

/// Dormant table.
pub const NFT_TABLE_F_DORMANT: u32 = 1 << 0;
/// Owner table.
pub const NFT_TABLE_F_OWNER: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Nftables chain flags
// ---------------------------------------------------------------------------

/// Base chain.
pub const NFT_CHAIN_BASE: u32 = 1 << 0;
/// Hardware offload chain.
pub const NFT_CHAIN_HW_OFFLOAD: u32 = 1 << 1;
/// Binding chain.
pub const NFT_CHAIN_BINDING: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Nftables data types
// ---------------------------------------------------------------------------

/// Value.
pub const NFT_DATA_VALUE: u32 = 0;
/// Verdict.
pub const NFT_DATA_VERDICT: u32 = 0xffffff00;

// ---------------------------------------------------------------------------
// Nftables generic netlink message types
// ---------------------------------------------------------------------------

/// Get tables.
pub const NFT_MSG_NEWTABLE: u32 = 0;
/// Get tables.
pub const NFT_MSG_GETTABLE: u32 = 1;
/// Delete table.
pub const NFT_MSG_DELTABLE: u32 = 2;
/// New chain.
pub const NFT_MSG_NEWCHAIN: u32 = 3;
/// Get chain.
pub const NFT_MSG_GETCHAIN: u32 = 4;
/// Delete chain.
pub const NFT_MSG_DELCHAIN: u32 = 5;
/// New rule.
pub const NFT_MSG_NEWRULE: u32 = 6;
/// Get rule.
pub const NFT_MSG_GETRULE: u32 = 7;
/// Delete rule.
pub const NFT_MSG_DELRULE: u32 = 8;
/// New set.
pub const NFT_MSG_NEWSET: u32 = 9;
/// Get set.
pub const NFT_MSG_GETSET: u32 = 10;
/// Delete set.
pub const NFT_MSG_DELSET: u32 = 11;
/// New set element.
pub const NFT_MSG_NEWSETELEM: u32 = 12;
/// Get set element.
pub const NFT_MSG_GETSETELEM: u32 = 13;
/// Delete set element.
pub const NFT_MSG_DELSETELEM: u32 = 14;
/// New generation.
pub const NFT_MSG_NEWGEN: u32 = 15;
/// Get generation.
pub const NFT_MSG_GETGEN: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_types_distinct() {
        let types = [
            NFT_OBJECT_UNSPEC,
            NFT_OBJECT_COUNTER,
            NFT_OBJECT_QUOTA,
            NFT_OBJECT_CT_HELPER,
            NFT_OBJECT_LIMIT,
            NFT_OBJECT_CT_TIMEOUT,
            NFT_OBJECT_SECMARK,
            NFT_OBJECT_CT_EXPECT,
            NFT_OBJECT_SYNPROXY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_table_flags_no_overlap() {
        assert_eq!(NFT_TABLE_F_DORMANT & NFT_TABLE_F_OWNER, 0);
    }

    #[test]
    fn test_table_flags_power_of_two() {
        assert!(NFT_TABLE_F_DORMANT.is_power_of_two());
        assert!(NFT_TABLE_F_OWNER.is_power_of_two());
    }

    #[test]
    fn test_chain_flags_no_overlap() {
        let flags = [NFT_CHAIN_BASE, NFT_CHAIN_HW_OFFLOAD, NFT_CHAIN_BINDING];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_data_types_distinct() {
        assert_ne!(NFT_DATA_VALUE, NFT_DATA_VERDICT);
    }

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            NFT_MSG_NEWTABLE,
            NFT_MSG_GETTABLE,
            NFT_MSG_DELTABLE,
            NFT_MSG_NEWCHAIN,
            NFT_MSG_GETCHAIN,
            NFT_MSG_DELCHAIN,
            NFT_MSG_NEWRULE,
            NFT_MSG_GETRULE,
            NFT_MSG_DELRULE,
            NFT_MSG_NEWSET,
            NFT_MSG_GETSET,
            NFT_MSG_DELSET,
            NFT_MSG_NEWSETELEM,
            NFT_MSG_GETSETELEM,
            NFT_MSG_DELSETELEM,
            NFT_MSG_NEWGEN,
            NFT_MSG_GETGEN,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }
}
