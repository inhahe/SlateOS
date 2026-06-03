//! `<linux/ip_vs.h>` — IP Virtual Server (IPVS) constants.
//!
//! IPVS provides transport-layer load balancing.  These
//! constants define scheduling algorithms, forwarding
//! methods, connection flags, and netlink attribute types.

// ---------------------------------------------------------------------------
// IPVS service/destination forwarding methods
// ---------------------------------------------------------------------------

/// NAT forwarding (masquerading).
pub const IP_VS_CONN_F_MASQ: u32 = 0;
/// Local node (local delivery).
pub const IP_VS_CONN_F_LOCALNODE: u32 = 1;
/// Tunneling (IP-in-IP encapsulation).
pub const IP_VS_CONN_F_TUNNEL: u32 = 2;
/// Direct routing (DR).
pub const IP_VS_CONN_F_DROUTE: u32 = 3;
/// Bypass (no translation).
pub const IP_VS_CONN_F_BYPASS: u32 = 4;

// ---------------------------------------------------------------------------
// IPVS connection flags
// ---------------------------------------------------------------------------

/// Forwarding method mask.
pub const IP_VS_CONN_F_FWD_MASK: u32 = 0x0007;
/// No client port set.
pub const IP_VS_CONN_F_NOOUTPUT: u32 = 0x0008;
/// Inactive connection.
pub const IP_VS_CONN_F_INACTIVE: u32 = 0x0010;
/// Hashed.
pub const IP_VS_CONN_F_HASHED: u32 = 0x0040;
/// One packet scheduler.
pub const IP_VS_CONN_F_ONE_PACKET: u32 = 0x2000;
/// Sync connection (from sync daemon).
pub const IP_VS_CONN_F_SYNC: u32 = 0x0020;
/// Template connection.
pub const IP_VS_CONN_F_TEMPLATE: u32 = 0x0400;

// ---------------------------------------------------------------------------
// IPVS scheduler names (numeric IDs)
// ---------------------------------------------------------------------------

/// Round-robin.
pub const IP_VS_SCHED_RR: u32 = 0;
/// Weighted round-robin.
pub const IP_VS_SCHED_WRR: u32 = 1;
/// Least connection.
pub const IP_VS_SCHED_LC: u32 = 2;
/// Weighted least connection.
pub const IP_VS_SCHED_WLC: u32 = 3;
/// Locality-based least connection.
pub const IP_VS_SCHED_LBLC: u32 = 4;
/// Locality-based least connection with replication.
pub const IP_VS_SCHED_LBLCR: u32 = 5;
/// Destination hashing.
pub const IP_VS_SCHED_DH: u32 = 6;
/// Source hashing.
pub const IP_VS_SCHED_SH: u32 = 7;
/// Shortest expected delay.
pub const IP_VS_SCHED_SED: u32 = 8;
/// Never queue.
pub const IP_VS_SCHED_NQ: u32 = 9;
/// Overflow connection.
pub const IP_VS_SCHED_OVF: u32 = 10;
/// Maglev consistent hashing.
pub const IP_VS_SCHED_MH: u32 = 11;
/// Fair-queue (FQ).
pub const IP_VS_SCHED_FO: u32 = 12;
/// Weighted failover.
pub const IP_VS_SCHED_TWOS: u32 = 13;

// ---------------------------------------------------------------------------
// IPVS generic netlink commands
// ---------------------------------------------------------------------------

/// New service.
pub const IPVS_CMD_NEW_SERVICE: u32 = 1;
/// Set service.
pub const IPVS_CMD_SET_SERVICE: u32 = 2;
/// Delete service.
pub const IPVS_CMD_DEL_SERVICE: u32 = 3;
/// Get service.
pub const IPVS_CMD_GET_SERVICE: u32 = 4;
/// New destination.
pub const IPVS_CMD_NEW_DEST: u32 = 5;
/// Set destination.
pub const IPVS_CMD_SET_DEST: u32 = 6;
/// Delete destination.
pub const IPVS_CMD_DEL_DEST: u32 = 7;
/// Get destination.
pub const IPVS_CMD_GET_DEST: u32 = 8;
/// Get daemon.
pub const IPVS_CMD_NEW_DAEMON: u32 = 9;
/// Delete daemon.
pub const IPVS_CMD_DEL_DAEMON: u32 = 10;
/// Get daemon.
pub const IPVS_CMD_GET_DAEMON: u32 = 11;
/// Flush all.
pub const IPVS_CMD_FLUSH: u32 = 12;
/// Get info.
pub const IPVS_CMD_GET_INFO: u32 = 14;
/// Zero counters.
pub const IPVS_CMD_ZERO: u32 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fwd_methods_distinct() {
        let methods = [
            IP_VS_CONN_F_MASQ,
            IP_VS_CONN_F_LOCALNODE,
            IP_VS_CONN_F_TUNNEL,
            IP_VS_CONN_F_DROUTE,
            IP_VS_CONN_F_BYPASS,
        ];
        for i in 0..methods.len() {
            for j in (i + 1)..methods.len() {
                assert_ne!(methods[i], methods[j]);
            }
        }
    }

    #[test]
    fn test_schedulers_distinct() {
        let scheds = [
            IP_VS_SCHED_RR,
            IP_VS_SCHED_WRR,
            IP_VS_SCHED_LC,
            IP_VS_SCHED_WLC,
            IP_VS_SCHED_LBLC,
            IP_VS_SCHED_LBLCR,
            IP_VS_SCHED_DH,
            IP_VS_SCHED_SH,
            IP_VS_SCHED_SED,
            IP_VS_SCHED_NQ,
            IP_VS_SCHED_OVF,
            IP_VS_SCHED_MH,
            IP_VS_SCHED_FO,
            IP_VS_SCHED_TWOS,
        ];
        for i in 0..scheds.len() {
            for j in (i + 1)..scheds.len() {
                assert_ne!(scheds[i], scheds[j]);
            }
        }
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            IPVS_CMD_NEW_SERVICE,
            IPVS_CMD_SET_SERVICE,
            IPVS_CMD_DEL_SERVICE,
            IPVS_CMD_GET_SERVICE,
            IPVS_CMD_NEW_DEST,
            IPVS_CMD_SET_DEST,
            IPVS_CMD_DEL_DEST,
            IPVS_CMD_GET_DEST,
            IPVS_CMD_NEW_DAEMON,
            IPVS_CMD_DEL_DAEMON,
            IPVS_CMD_GET_DAEMON,
            IPVS_CMD_FLUSH,
            IPVS_CMD_GET_INFO,
            IPVS_CMD_ZERO,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_masq_is_zero() {
        assert_eq!(IP_VS_CONN_F_MASQ, 0);
    }

    #[test]
    fn test_rr_is_zero() {
        assert_eq!(IP_VS_SCHED_RR, 0);
    }

    #[test]
    fn test_fwd_mask() {
        assert_eq!(IP_VS_CONN_F_FWD_MASK, 0x0007);
    }
}
