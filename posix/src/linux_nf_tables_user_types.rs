//! `<linux/netfilter/nf_tables.h>` — nftables netlink ABI.
//!
//! nftables is the modern replacement for iptables — one consistent
//! framework across IPv4, IPv6, ARP, bridge, and netdev families.
//! `nft(8)`, `firewalld`, and `nftables.systemd-service` build rules
//! by composing the verdicts, message types, and attribute trees
//! defined here.

// ---------------------------------------------------------------------------
// `nfgenmsg` subsystem id
// ---------------------------------------------------------------------------

pub const NFNL_SUBSYS_NFTABLES: u32 = 10;

// ---------------------------------------------------------------------------
// nftables message types (`enum nf_tables_msg_types`)
// ---------------------------------------------------------------------------

pub const NFT_MSG_NEWTABLE: u32 = 0;
pub const NFT_MSG_GETTABLE: u32 = 1;
pub const NFT_MSG_DELTABLE: u32 = 2;
pub const NFT_MSG_NEWCHAIN: u32 = 3;
pub const NFT_MSG_GETCHAIN: u32 = 4;
pub const NFT_MSG_DELCHAIN: u32 = 5;
pub const NFT_MSG_NEWRULE: u32 = 6;
pub const NFT_MSG_GETRULE: u32 = 7;
pub const NFT_MSG_DELRULE: u32 = 8;
pub const NFT_MSG_NEWSET: u32 = 9;
pub const NFT_MSG_GETSET: u32 = 10;
pub const NFT_MSG_DELSET: u32 = 11;
pub const NFT_MSG_NEWSETELEM: u32 = 12;
pub const NFT_MSG_GETSETELEM: u32 = 13;
pub const NFT_MSG_DELSETELEM: u32 = 14;
pub const NFT_MSG_NEWGEN: u32 = 15;
pub const NFT_MSG_GETGEN: u32 = 16;
pub const NFT_MSG_TRACE: u32 = 17;
pub const NFT_MSG_NEWOBJ: u32 = 18;
pub const NFT_MSG_GETOBJ: u32 = 19;
pub const NFT_MSG_DELOBJ: u32 = 20;
pub const NFT_MSG_GETOBJ_RESET: u32 = 21;
pub const NFT_MSG_NEWFLOWTABLE: u32 = 22;
pub const NFT_MSG_GETFLOWTABLE: u32 = 23;
pub const NFT_MSG_DELFLOWTABLE: u32 = 24;
pub const NFT_MSG_GETRULE_RESET: u32 = 25;
pub const NFT_MSG_DESTROYTABLE: u32 = 26;
pub const NFT_MSG_DESTROYCHAIN: u32 = 27;
pub const NFT_MSG_DESTROYRULE: u32 = 28;
pub const NFT_MSG_DESTROYSET: u32 = 29;
pub const NFT_MSG_DESTROYSETELEM: u32 = 30;
pub const NFT_MSG_DESTROYOBJ: u32 = 31;
pub const NFT_MSG_DESTROYFLOWTABLE: u32 = 32;
pub const NFT_MSG_GETSETELEM_RESET: u32 = 33;

// ---------------------------------------------------------------------------
// nftables verdicts (returned from expression evaluation)
// ---------------------------------------------------------------------------

pub const NFT_CONTINUE: i32 = -1;
pub const NFT_BREAK: i32 = -2;
pub const NFT_JUMP: i32 = -3;
pub const NFT_GOTO: i32 = -4;
pub const NFT_RETURN: i32 = -5;

// ---------------------------------------------------------------------------
// Chain types (`enum nft_chain_types`)
// ---------------------------------------------------------------------------

pub const NFT_CHAIN_T_DEFAULT: u32 = 0;
pub const NFT_CHAIN_T_ROUTE: u32 = 1;
pub const NFT_CHAIN_T_NAT: u32 = 2;
pub const NFT_CHAIN_T_MAX: u32 = NFT_CHAIN_T_NAT;

// ---------------------------------------------------------------------------
// Identifier length limits
// ---------------------------------------------------------------------------

pub const NFT_NAME_MAXLEN: usize = 256;
pub const NFT_TABLE_MAXNAMELEN: usize = NFT_NAME_MAXLEN;
pub const NFT_CHAIN_MAXNAMELEN: usize = NFT_NAME_MAXLEN;
pub const NFT_SET_MAXNAMELEN: usize = NFT_NAME_MAXLEN;
pub const NFT_USERDATA_MAXLEN: usize = 256;
pub const NFT_OBJ_MAXNAMELEN: usize = NFT_NAME_MAXLEN;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subsys_id() {
        // nfnetlink subsystem id for nftables.
        assert_eq!(NFNL_SUBSYS_NFTABLES, 10);
    }

    #[test]
    fn test_msg_types_dense_0_to_33() {
        let m = [
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
            NFT_MSG_TRACE,
            NFT_MSG_NEWOBJ,
            NFT_MSG_GETOBJ,
            NFT_MSG_DELOBJ,
            NFT_MSG_GETOBJ_RESET,
            NFT_MSG_NEWFLOWTABLE,
            NFT_MSG_GETFLOWTABLE,
            NFT_MSG_DELFLOWTABLE,
            NFT_MSG_GETRULE_RESET,
            NFT_MSG_DESTROYTABLE,
            NFT_MSG_DESTROYCHAIN,
            NFT_MSG_DESTROYRULE,
            NFT_MSG_DESTROYSET,
            NFT_MSG_DESTROYSETELEM,
            NFT_MSG_DESTROYOBJ,
            NFT_MSG_DESTROYFLOWTABLE,
            NFT_MSG_GETSETELEM_RESET,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_verdicts_negative_and_distinct() {
        let v = [NFT_CONTINUE, NFT_BREAK, NFT_JUMP, NFT_GOTO, NFT_RETURN];
        for x in v {
            // All five nftables verdicts are negative — distinct from
            // the NF_* verdicts (which are 0..5).
            assert!(x < 0);
        }
        for i in 0..v.len() {
            for j in (i + 1)..v.len() {
                assert_ne!(v[i], v[j]);
            }
        }
        // CONTINUE..RETURN are -1..-5.
        assert_eq!(NFT_CONTINUE, -1);
        assert_eq!(NFT_RETURN, -5);
    }

    #[test]
    fn test_chain_types_dense() {
        assert_eq!(NFT_CHAIN_T_DEFAULT, 0);
        assert_eq!(NFT_CHAIN_T_ROUTE, 1);
        assert_eq!(NFT_CHAIN_T_NAT, 2);
        assert_eq!(NFT_CHAIN_T_MAX, 2);
    }

    #[test]
    fn test_max_name_lengths() {
        assert_eq!(NFT_NAME_MAXLEN, 256);
        assert_eq!(NFT_USERDATA_MAXLEN, 256);
        // All name-length caps share the same value.
        assert_eq!(NFT_TABLE_MAXNAMELEN, NFT_NAME_MAXLEN);
        assert_eq!(NFT_CHAIN_MAXNAMELEN, NFT_NAME_MAXLEN);
        assert_eq!(NFT_SET_MAXNAMELEN, NFT_NAME_MAXLEN);
        assert_eq!(NFT_OBJ_MAXNAMELEN, NFT_NAME_MAXLEN);
    }
}
