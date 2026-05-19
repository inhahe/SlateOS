//! `<linux/pkt_sched.h>` — Additional traffic control scheduler constants.
//!
//! Supplementary TC constants covering qdisc types,
//! HTB parameters, and FQ-CoDel parameters.

// ---------------------------------------------------------------------------
// Qdisc types (TCA_KIND values)
// ---------------------------------------------------------------------------

/// Pfifo (packet FIFO) qdisc type.
pub const TC_QDISC_PFIFO: u32 = 0;
/// Bfifo (byte FIFO) qdisc type.
pub const TC_QDISC_BFIFO: u32 = 1;
/// SFQ (Stochastic Fair Queuing) qdisc type.
pub const TC_QDISC_SFQ: u32 = 2;
/// RED (Random Early Detection) qdisc type.
pub const TC_QDISC_RED: u32 = 3;
/// TBF (Token Bucket Filter) qdisc type.
pub const TC_QDISC_TBF: u32 = 4;
/// HTB (Hierarchy Token Bucket) qdisc type.
pub const TC_QDISC_HTB: u32 = 5;
/// HFSC (Hierarchical Fair Service Curve) qdisc type.
pub const TC_QDISC_HFSC: u32 = 6;
/// FQ (Fair Queuing) qdisc type.
pub const TC_QDISC_FQ: u32 = 7;
/// FQ-CoDel qdisc type.
pub const TC_QDISC_FQ_CODEL: u32 = 8;
/// CAKE qdisc type.
pub const TC_QDISC_CAKE: u32 = 9;
/// ETS (Enhanced Transmission Selection) qdisc type.
pub const TC_QDISC_ETS: u32 = 10;

// ---------------------------------------------------------------------------
// HTB class modes
// ---------------------------------------------------------------------------

/// HTB: can send (not limited).
pub const TC_HTB_CAN_SEND: u32 = 0;
/// HTB: may borrow from parent.
pub const TC_HTB_MAY_BORROW: u32 = 1;
/// HTB: can't send (overlimit).
pub const TC_HTB_CANT_SEND: u32 = 2;

// ---------------------------------------------------------------------------
// FQ-CoDel parameters
// ---------------------------------------------------------------------------

/// Default target delay (5ms in microseconds).
pub const FQ_CODEL_TARGET_US: u32 = 5000;
/// Default interval (100ms in microseconds).
pub const FQ_CODEL_INTERVAL_US: u32 = 100000;
/// Default quantum (1514 bytes = typical MTU + ethernet header).
pub const FQ_CODEL_QUANTUM: u32 = 1514;
/// Default number of flows.
pub const FQ_CODEL_FLOWS: u32 = 1024;
/// Default ECN marking enabled.
pub const FQ_CODEL_ECN_ENABLED: u32 = 1;
/// Default ECN marking disabled.
pub const FQ_CODEL_ECN_DISABLED: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qdisc_types_distinct() {
        let types = [
            TC_QDISC_PFIFO, TC_QDISC_BFIFO, TC_QDISC_SFQ,
            TC_QDISC_RED, TC_QDISC_TBF, TC_QDISC_HTB,
            TC_QDISC_HFSC, TC_QDISC_FQ, TC_QDISC_FQ_CODEL,
            TC_QDISC_CAKE, TC_QDISC_ETS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_htb_modes_distinct() {
        let modes = [TC_HTB_CAN_SEND, TC_HTB_MAY_BORROW, TC_HTB_CANT_SEND];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_fq_codel_defaults() {
        assert_eq!(FQ_CODEL_TARGET_US, 5000);
        assert_eq!(FQ_CODEL_INTERVAL_US, 100000);
        assert_eq!(FQ_CODEL_FLOWS, 1024);
    }

    #[test]
    fn test_fq_codel_ecn_distinct() {
        assert_ne!(FQ_CODEL_ECN_ENABLED, FQ_CODEL_ECN_DISABLED);
    }

    #[test]
    fn test_fq_codel_target_less_than_interval() {
        assert!(FQ_CODEL_TARGET_US < FQ_CODEL_INTERVAL_US);
    }
}
