//! `<linux/netfilter/nf_log.h>` — netfilter logging constants.
//!
//! Netfilter logging targets record matched packets for debugging,
//! auditing, and intrusion detection. The LOG target writes to the
//! kernel log (dmesg/syslog), ULOG/NFLOG send packets to userspace
//! daemons (ulogd, nflog), and recent kernels support per-protocol
//! log backends.

// ---------------------------------------------------------------------------
// Log types (nf_log_type)
// ---------------------------------------------------------------------------

/// Unspecified log type.
pub const NF_LOG_TYPE_ULOG: u32 = 0;
/// LOG target (kernel printk).
pub const NF_LOG_TYPE_LOG: u32 = 1;
/// Number of log types.
pub const NF_LOG_TYPE_MAX: u32 = 2;

// ---------------------------------------------------------------------------
// Log flags (NF_LOG_*)
// ---------------------------------------------------------------------------

/// Log TCP sequence numbers.
pub const NF_LOG_TCPSEQ: u32 = 0x01;
/// Log TCP options.
pub const NF_LOG_TCPOPT: u32 = 0x02;
/// Log IP options.
pub const NF_LOG_IPOPT: u32 = 0x04;
/// Log UID/GID of socket owner.
pub const NF_LOG_UID: u32 = 0x08;
/// Log NFLOG sequence number.
pub const NF_LOG_NFLOG: u32 = 0x10;
/// Log MAC address.
pub const NF_LOG_MACDECODE: u32 = 0x20;
/// All log flags combined.
pub const NF_LOG_MASK: u32 = 0x3F;

// ---------------------------------------------------------------------------
// Default log level (severity)
// ---------------------------------------------------------------------------

/// Emergency: system is unusable.
pub const NF_LOG_EMERG: u32 = 0;
/// Alert: action must be taken immediately.
pub const NF_LOG_ALERT: u32 = 1;
/// Critical: critical conditions.
pub const NF_LOG_CRIT: u32 = 2;
/// Error: error conditions.
pub const NF_LOG_ERR: u32 = 3;
/// Warning: warning conditions.
pub const NF_LOG_WARNING: u32 = 4;
/// Notice: normal but significant condition.
pub const NF_LOG_NOTICE: u32 = 5;
/// Info: informational.
pub const NF_LOG_INFO: u32 = 6;
/// Debug: debug-level messages.
pub const NF_LOG_DEBUG: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_types_distinct() {
        assert_ne!(NF_LOG_TYPE_ULOG, NF_LOG_TYPE_LOG);
    }

    #[test]
    fn test_log_flags_no_overlap() {
        let flags = [
            NF_LOG_TCPSEQ, NF_LOG_TCPOPT, NF_LOG_IPOPT,
            NF_LOG_UID, NF_LOG_NFLOG, NF_LOG_MACDECODE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_log_mask_covers_all() {
        let combined = NF_LOG_TCPSEQ | NF_LOG_TCPOPT | NF_LOG_IPOPT
            | NF_LOG_UID | NF_LOG_NFLOG | NF_LOG_MACDECODE;
        assert_eq!(NF_LOG_MASK, combined);
    }

    #[test]
    fn test_severity_levels_ordered() {
        assert!(NF_LOG_EMERG < NF_LOG_ALERT);
        assert!(NF_LOG_ALERT < NF_LOG_CRIT);
        assert!(NF_LOG_CRIT < NF_LOG_ERR);
        assert!(NF_LOG_ERR < NF_LOG_WARNING);
        assert!(NF_LOG_WARNING < NF_LOG_NOTICE);
        assert!(NF_LOG_NOTICE < NF_LOG_INFO);
        assert!(NF_LOG_INFO < NF_LOG_DEBUG);
    }
}
