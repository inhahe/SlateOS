//! `<linux/mptcp.h>` — Multipath TCP socket and netlink ABI.
//!
//! MPTCP (RFC 8684) lets a single logical TCP connection use multiple
//! subflows over different paths. Linux 5.6 added upstream support;
//! mobile carriers and datacenter operators use it for transparent
//! WiFi/cellular bonding and for multi-NIC server bonding. `mptcpd`
//! and `iproute2`'s `ip mptcp` configure paths via the genetlink
//! family below.

// ---------------------------------------------------------------------------
// IPPROTO selector
// ---------------------------------------------------------------------------

/// `IPPROTO_MPTCP` — used as the third argument to `socket(2)`.
pub const IPPROTO_MPTCP: u32 = 262;

// ---------------------------------------------------------------------------
// SOL_MPTCP socket options
// ---------------------------------------------------------------------------

pub const SOL_MPTCP: u32 = 284;

pub const MPTCP_INFO: u32 = 1;
pub const MPTCP_TCPINFO: u32 = 2;
pub const MPTCP_SUBFLOW_ADDRS: u32 = 3;
pub const MPTCP_FULL_INFO: u32 = 4;

// ---------------------------------------------------------------------------
// genetlink family ("mptcp_pm")
// ---------------------------------------------------------------------------

pub const MPTCP_PM_NAME: &str = "mptcp_pm";
pub const MPTCP_PM_VER: u32 = 1;
pub const MPTCP_PM_EV_GRP_NAME: &str = "mptcp_pm_events";

// ---------------------------------------------------------------------------
// Path-manager commands
// ---------------------------------------------------------------------------

pub const MPTCP_PM_CMD_UNSPEC: u32 = 0;
pub const MPTCP_PM_CMD_ADD_ADDR: u32 = 1;
pub const MPTCP_PM_CMD_DEL_ADDR: u32 = 2;
pub const MPTCP_PM_CMD_GET_ADDR: u32 = 3;
pub const MPTCP_PM_CMD_FLUSH_ADDRS: u32 = 4;
pub const MPTCP_PM_CMD_SET_LIMITS: u32 = 5;
pub const MPTCP_PM_CMD_GET_LIMITS: u32 = 6;
pub const MPTCP_PM_CMD_SET_FLAGS: u32 = 7;
pub const MPTCP_PM_CMD_ANNOUNCE: u32 = 8;
pub const MPTCP_PM_CMD_REMOVE: u32 = 9;
pub const MPTCP_PM_CMD_SUBFLOW_CREATE: u32 = 10;
pub const MPTCP_PM_CMD_SUBFLOW_DESTROY: u32 = 11;

// ---------------------------------------------------------------------------
// Address flags
// ---------------------------------------------------------------------------

pub const MPTCP_PM_ADDR_FLAG_SIGNAL: u32 = 1 << 0;
pub const MPTCP_PM_ADDR_FLAG_SUBFLOW: u32 = 1 << 1;
pub const MPTCP_PM_ADDR_FLAG_BACKUP: u32 = 1 << 2;
pub const MPTCP_PM_ADDR_FLAG_FULLMESH: u32 = 1 << 3;
pub const MPTCP_PM_ADDR_FLAG_IMPLICIT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Path-manager event types
// ---------------------------------------------------------------------------

pub const MPTCP_EVENT_CREATED: u32 = 1;
pub const MPTCP_EVENT_ESTABLISHED: u32 = 2;
pub const MPTCP_EVENT_CLOSED: u32 = 3;
pub const MPTCP_EVENT_ANNOUNCED: u32 = 6;
pub const MPTCP_EVENT_REMOVED: u32 = 7;
pub const MPTCP_EVENT_SUB_ESTABLISHED: u32 = 10;
pub const MPTCP_EVENT_SUB_CLOSED: u32 = 11;
pub const MPTCP_EVENT_SUB_PRIORITY: u32 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipproto_and_sol() {
        assert_eq!(IPPROTO_MPTCP, 262);
        assert_eq!(SOL_MPTCP, 284);
    }

    #[test]
    fn test_sockopts_dense_1_to_4() {
        let o = [MPTCP_INFO, MPTCP_TCPINFO, MPTCP_SUBFLOW_ADDRS, MPTCP_FULL_INFO];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_pm_genl_family() {
        assert_eq!(MPTCP_PM_NAME, "mptcp_pm");
        assert_eq!(MPTCP_PM_VER, 1);
        assert_eq!(MPTCP_PM_EV_GRP_NAME, "mptcp_pm_events");
    }

    #[test]
    fn test_pm_commands_dense_0_to_11() {
        let c = [
            MPTCP_PM_CMD_UNSPEC,
            MPTCP_PM_CMD_ADD_ADDR,
            MPTCP_PM_CMD_DEL_ADDR,
            MPTCP_PM_CMD_GET_ADDR,
            MPTCP_PM_CMD_FLUSH_ADDRS,
            MPTCP_PM_CMD_SET_LIMITS,
            MPTCP_PM_CMD_GET_LIMITS,
            MPTCP_PM_CMD_SET_FLAGS,
            MPTCP_PM_CMD_ANNOUNCE,
            MPTCP_PM_CMD_REMOVE,
            MPTCP_PM_CMD_SUBFLOW_CREATE,
            MPTCP_PM_CMD_SUBFLOW_DESTROY,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_addr_flags_dense_single_bit() {
        let f = [
            MPTCP_PM_ADDR_FLAG_SIGNAL,
            MPTCP_PM_ADDR_FLAG_SUBFLOW,
            MPTCP_PM_ADDR_FLAG_BACKUP,
            MPTCP_PM_ADDR_FLAG_FULLMESH,
            MPTCP_PM_ADDR_FLAG_IMPLICIT,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // Five dense bits.
        assert_eq!(f.iter().fold(0u32, |a, b| a | b), 0x1F);
    }

    #[test]
    fn test_events_distinct() {
        let e = [
            MPTCP_EVENT_CREATED,
            MPTCP_EVENT_ESTABLISHED,
            MPTCP_EVENT_CLOSED,
            MPTCP_EVENT_ANNOUNCED,
            MPTCP_EVENT_REMOVED,
            MPTCP_EVENT_SUB_ESTABLISHED,
            MPTCP_EVENT_SUB_CLOSED,
            MPTCP_EVENT_SUB_PRIORITY,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
        // Connection events use 1..7, subflow events use 10..13.
        assert!(MPTCP_EVENT_REMOVED < MPTCP_EVENT_SUB_ESTABLISHED);
    }
}
