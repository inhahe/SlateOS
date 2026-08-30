//! `<linux/pkt_sched.h>` — packet scheduling (traffic control) constants.
//!
//! These constants are used with the `tc` tool and RTM_NEWQDISC/
//! RTM_NEWTCLASS netlink messages to configure traffic control
//! queuing disciplines (qdiscs), classes, and filters.

// ---------------------------------------------------------------------------
// Qdisc types (TCA_* attributes)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_UNSPEC: u16 = 0;
/// Qdisc kind string.
pub const TCA_KIND: u16 = 1;
/// Qdisc options.
pub const TCA_OPTIONS: u16 = 2;
/// Statistics.
pub const TCA_STATS: u16 = 3;
/// Rate estimation.
pub const TCA_RATE: u16 = 5;
/// Extended statistics.
pub const TCA_STATS2: u16 = 7;
/// Chain info.
pub const TCA_CHAIN: u16 = 11;
/// Hardware offload status.
pub const TCA_HW_OFFLOAD: u16 = 12;

// ---------------------------------------------------------------------------
// Well-known qdisc identifiers
// ---------------------------------------------------------------------------

/// pfifo (simple FIFO).
pub const TC_PRIO_MAX: u32 = 15;
/// Number of priority bands.
pub const TC_PRIO_BESTEFFORT: u32 = 0;
/// Filler priority.
pub const TC_PRIO_FILLER: u32 = 1;
/// Bulk priority.
pub const TC_PRIO_BULK: u32 = 2;
/// Interactive bulk.
pub const TC_PRIO_INTERACTIVE_BULK: u32 = 4;
/// Interactive.
pub const TC_PRIO_INTERACTIVE: u32 = 6;
/// Control traffic.
pub const TC_PRIO_CONTROL: u32 = 7;

// ---------------------------------------------------------------------------
// FQ_CODEL parameters (modern default qdisc)
// ---------------------------------------------------------------------------

/// FQ-CoDel target (microseconds).
pub const FQ_CODEL_DEFAULT_TARGET: u32 = 5000;
/// FQ-CoDel interval (microseconds).
pub const FQ_CODEL_DEFAULT_INTERVAL: u32 = 100_000;
/// FQ-CoDel quantum (bytes).
pub const FQ_CODEL_DEFAULT_QUANTUM: u32 = 1514;
/// FQ-CoDel queue limit (packets).
pub const FQ_CODEL_DEFAULT_LIMIT: u32 = 10240;
/// FQ-CoDel flows.
pub const FQ_CODEL_DEFAULT_FLOWS: u32 = 1024;

// ---------------------------------------------------------------------------
// HTB parameters
// ---------------------------------------------------------------------------

/// TCA_HTB_INIT.
pub const TCA_HTB_INIT: u16 = 2;
/// TCA_HTB_PARMS.
pub const TCA_HTB_PARMS: u16 = 3;
/// TCA_HTB_CTAB.
pub const TCA_HTB_CTAB: u16 = 4;
/// TCA_HTB_RTAB.
pub const TCA_HTB_RTAB: u16 = 5;
/// TCA_HTB_DIRECT_QLEN.
pub const TCA_HTB_DIRECT_QLEN: u16 = 6;
/// TCA_HTB_RATE64.
pub const TCA_HTB_RATE64: u16 = 7;
/// TCA_HTB_CEIL64.
pub const TCA_HTB_CEIL64: u16 = 8;

// ---------------------------------------------------------------------------
// Handle macros
// ---------------------------------------------------------------------------

/// Major number from handle.
pub const fn tc_h_maj(h: u32) -> u32 {
    h & 0xFFFF0000
}

/// Minor number from handle.
pub const fn tc_h_min(h: u32) -> u32 {
    h & 0x0000FFFF
}

/// Root qdisc handle.
pub const TC_H_ROOT: u32 = 0xFFFFFFFF;
/// Ingress qdisc handle.
pub const TC_H_INGRESS: u32 = 0xFFFFFFF1;
/// Clsact qdisc handle.
pub const TC_H_CLSACT: u32 = TC_H_INGRESS;
/// Unspec handle.
pub const TC_H_UNSPEC: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tca_attrs() {
        assert_eq!(TCA_UNSPEC, 0);
        assert_eq!(TCA_KIND, 1);
        assert_eq!(TCA_OPTIONS, 2);
        assert_eq!(TCA_STATS, 3);
    }

    #[test]
    fn test_priorities_ordered() {
        assert!(TC_PRIO_BESTEFFORT < TC_PRIO_FILLER);
        assert!(TC_PRIO_FILLER < TC_PRIO_BULK);
        assert!(TC_PRIO_BULK < TC_PRIO_INTERACTIVE_BULK);
        assert!(TC_PRIO_INTERACTIVE_BULK < TC_PRIO_INTERACTIVE);
        assert!(TC_PRIO_INTERACTIVE < TC_PRIO_CONTROL);
    }

    #[test]
    fn test_handle_macros() {
        let handle: u32 = 0x00010002;
        assert_eq!(tc_h_maj(handle), 0x00010000);
        assert_eq!(tc_h_min(handle), 0x00000002);
    }

    #[test]
    fn test_fq_codel_defaults() {
        assert_eq!(FQ_CODEL_DEFAULT_TARGET, 5000);
        assert_eq!(FQ_CODEL_DEFAULT_INTERVAL, 100_000);
    }

    #[test]
    fn test_special_handles() {
        assert_ne!(TC_H_ROOT, TC_H_INGRESS);
        assert_eq!(TC_H_CLSACT, TC_H_INGRESS);
        assert_eq!(TC_H_UNSPEC, 0);
    }
}
