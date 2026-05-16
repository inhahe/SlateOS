//! `<linux/netfilter/nf_tables.h>` — nftables constants.
//!
//! nftables is the modern Linux packet filtering framework, replacing
//! iptables. It uses a virtual machine with expressions/statements
//! to evaluate and classify network packets.

// ---------------------------------------------------------------------------
// NFT message types (relative to NFNL_SUBSYS_NFTABLES << 8)
// ---------------------------------------------------------------------------

/// New table.
pub const NFT_MSG_NEWTABLE: u32 = 0;
/// Get table.
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
pub const NFT_MSG_NEWGEN: u32 = 16;
/// Get generation.
pub const NFT_MSG_GETGEN: u32 = 17;
/// New object.
pub const NFT_MSG_NEWOBJ: u32 = 18;
/// Get object.
pub const NFT_MSG_GETOBJ: u32 = 19;
/// Delete object.
pub const NFT_MSG_DELOBJ: u32 = 20;
/// New flowtable.
pub const NFT_MSG_NEWFLOWTABLE: u32 = 24;
/// Get flowtable.
pub const NFT_MSG_GETFLOWTABLE: u32 = 25;
/// Delete flowtable.
pub const NFT_MSG_DELFLOWTABLE: u32 = 26;

// ---------------------------------------------------------------------------
// Table attributes (NFTA_TABLE_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NFTA_TABLE_UNSPEC: u16 = 0;
/// Table name.
pub const NFTA_TABLE_NAME: u16 = 1;
/// Table flags.
pub const NFTA_TABLE_FLAGS: u16 = 2;
/// Use count.
pub const NFTA_TABLE_USE: u16 = 3;
/// Handle.
pub const NFTA_TABLE_HANDLE: u16 = 4;
/// User data.
pub const NFTA_TABLE_USERDATA: u16 = 5;
/// Owner.
pub const NFTA_TABLE_OWNER: u16 = 6;

// ---------------------------------------------------------------------------
// Chain attributes (NFTA_CHAIN_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NFTA_CHAIN_UNSPEC: u16 = 0;
/// Chain name.
pub const NFTA_CHAIN_TABLE: u16 = 1;
/// Chain handle.
pub const NFTA_CHAIN_HANDLE: u16 = 2;
/// Chain name.
pub const NFTA_CHAIN_NAME: u16 = 3;
/// Chain hook.
pub const NFTA_CHAIN_HOOK: u16 = 4;
/// Chain policy.
pub const NFTA_CHAIN_POLICY: u16 = 5;
/// Use count.
pub const NFTA_CHAIN_USE: u16 = 6;
/// Chain type.
pub const NFTA_CHAIN_TYPE: u16 = 7;
/// Counters.
pub const NFTA_CHAIN_COUNTERS: u16 = 8;
/// Chain flags.
pub const NFTA_CHAIN_FLAGS: u16 = 9;
/// Chain ID.
pub const NFTA_CHAIN_ID: u16 = 10;
/// User data.
pub const NFTA_CHAIN_USERDATA: u16 = 11;

// ---------------------------------------------------------------------------
// Rule attributes (NFTA_RULE_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NFTA_RULE_UNSPEC: u16 = 0;
/// Table name.
pub const NFTA_RULE_TABLE: u16 = 1;
/// Chain name.
pub const NFTA_RULE_CHAIN: u16 = 2;
/// Rule handle.
pub const NFTA_RULE_HANDLE: u16 = 3;
/// Expressions (nested).
pub const NFTA_RULE_EXPRESSIONS: u16 = 4;
/// Compatibility.
pub const NFTA_RULE_COMPAT: u16 = 5;
/// Position (before this handle).
pub const NFTA_RULE_POSITION: u16 = 6;
/// User data.
pub const NFTA_RULE_USERDATA: u16 = 7;
/// Rule ID.
pub const NFTA_RULE_ID: u16 = 8;

// ---------------------------------------------------------------------------
// Verdicts
// ---------------------------------------------------------------------------

/// Continue processing.
pub const NFT_CONTINUE: i32 = -1;
/// Break from current rule.
pub const NFT_BREAK: i32 = -2;
/// Jump to chain.
pub const NFT_JUMP: i32 = -3;
/// Go to chain (no return).
pub const NFT_GOTO: i32 = -4;
/// Return from chain.
pub const NFT_RETURN: i32 = -5;

/// Accept packet.
pub const NF_DROP: u32 = 0;
/// Drop packet.
pub const NF_ACCEPT: u32 = 1;

// ---------------------------------------------------------------------------
// Register numbers
// ---------------------------------------------------------------------------

/// Verdict register.
pub const NFT_REG_VERDICT: u32 = 0;
/// 128-bit register 1.
pub const NFT_REG_1: u32 = 1;
/// 128-bit register 2.
pub const NFT_REG_2: u32 = 2;
/// 128-bit register 3.
pub const NFT_REG_3: u32 = 3;
/// 128-bit register 4.
pub const NFT_REG_4: u32 = 4;
/// First 32-bit register.
pub const NFT_REG32_00: u32 = 8;

// ---------------------------------------------------------------------------
// Table flags
// ---------------------------------------------------------------------------

/// Dormant table (not active).
pub const NFT_TABLE_F_DORMANT: u32 = 0x1;
/// Table owned by process.
pub const NFT_TABLE_F_OWNER: u32 = 0x2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_grouped() {
        // Table messages: 0-2
        assert_eq!(NFT_MSG_NEWTABLE, 0);
        assert_eq!(NFT_MSG_DELTABLE, 2);
        // Chain messages: 3-5
        assert_eq!(NFT_MSG_NEWCHAIN, 3);
        assert_eq!(NFT_MSG_DELCHAIN, 5);
        // Rule messages: 6-8
        assert_eq!(NFT_MSG_NEWRULE, 6);
        assert_eq!(NFT_MSG_DELRULE, 8);
    }

    #[test]
    fn test_table_attrs_sequential() {
        assert_eq!(NFTA_TABLE_UNSPEC, 0);
        assert_eq!(NFTA_TABLE_NAME, 1);
        assert_eq!(NFTA_TABLE_FLAGS, 2);
        assert_eq!(NFTA_TABLE_OWNER, 6);
    }

    #[test]
    fn test_chain_attrs_distinct() {
        let attrs = [
            NFTA_CHAIN_UNSPEC, NFTA_CHAIN_TABLE, NFTA_CHAIN_HANDLE,
            NFTA_CHAIN_NAME, NFTA_CHAIN_HOOK, NFTA_CHAIN_POLICY,
            NFTA_CHAIN_USE, NFTA_CHAIN_TYPE, NFTA_CHAIN_COUNTERS,
            NFTA_CHAIN_FLAGS, NFTA_CHAIN_ID, NFTA_CHAIN_USERDATA,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_verdicts() {
        assert_eq!(NFT_CONTINUE, -1);
        assert_eq!(NFT_BREAK, -2);
        assert_eq!(NFT_JUMP, -3);
        assert_eq!(NFT_GOTO, -4);
        assert_eq!(NFT_RETURN, -5);
    }

    #[test]
    fn test_registers() {
        assert_eq!(NFT_REG_VERDICT, 0);
        assert_eq!(NFT_REG_1, 1);
        assert_eq!(NFT_REG32_00, 8);
    }

    #[test]
    fn test_nf_actions() {
        assert_eq!(NF_DROP, 0);
        assert_eq!(NF_ACCEPT, 1);
    }

    #[test]
    fn test_table_flags() {
        assert_eq!(NFT_TABLE_F_DORMANT, 0x1);
        assert_eq!(NFT_TABLE_F_OWNER, 0x2);
    }
}
