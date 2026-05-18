//! `<net/tcp.h>` (congestion subset) — TCP congestion control constants.
//!
//! TCP congestion control determines how fast a sender can transmit
//! data without overwhelming the network. Different algorithms trade
//! off throughput, latency, and fairness. The kernel supports pluggable
//! congestion control: each socket can use a different algorithm via
//! setsockopt(TCP_CONGESTION). The default (usually CUBIC or BBR)
//! can be set system-wide via sysctl.

// ---------------------------------------------------------------------------
// Congestion control algorithms
// ---------------------------------------------------------------------------

/// Reno (classic AIMD, the original TCP congestion control).
pub const TCP_CC_RENO: u32 = 0;
/// CUBIC (default on most Linux systems, optimized for high-BDP).
pub const TCP_CC_CUBIC: u32 = 1;
/// BBR (Bottleneck Bandwidth and RTT, model-based).
pub const TCP_CC_BBR: u32 = 2;
/// BBR v2 (improved fairness, lower queuing delay).
pub const TCP_CC_BBRV2: u32 = 3;
/// DCTCP (Data Center TCP, ECN-based).
pub const TCP_CC_DCTCP: u32 = 4;
/// Vegas (delay-based, proactive).
pub const TCP_CC_VEGAS: u32 = 5;
/// Westwood+ (bandwidth estimation, wireless-friendly).
pub const TCP_CC_WESTWOOD: u32 = 6;
/// HTCP (High-speed TCP for long fat networks).
pub const TCP_CC_HTCP: u32 = 7;
/// Illinois (delay+loss hybrid).
pub const TCP_CC_ILLINOIS: u32 = 8;
/// CDG (CAIA Delay-Gradient).
pub const TCP_CC_CDG: u32 = 9;

// ---------------------------------------------------------------------------
// Congestion events
// ---------------------------------------------------------------------------

/// Packet loss detected (duplicate ACKs or timeout).
pub const TCP_CA_EVENT_LOSS: u32 = 0;
/// ECN mark received (Explicit Congestion Notification).
pub const TCP_CA_EVENT_ECN: u32 = 1;
/// Fast recovery entered.
pub const TCP_CA_EVENT_FAST_RECOVERY: u32 = 2;
/// Congestion window reduced.
pub const TCP_CA_EVENT_CWND_RESTART: u32 = 3;
/// Slow start exited.
pub const TCP_CA_EVENT_SLOW_START_EXIT: u32 = 4;

// ---------------------------------------------------------------------------
// Congestion states
// ---------------------------------------------------------------------------

/// Open state (normal transmission).
pub const TCP_CA_OPEN: u32 = 0;
/// Disorder (received out-of-order packets, watching).
pub const TCP_CA_DISORDER: u32 = 1;
/// Congestion window reduced (ECN or loss).
pub const TCP_CA_CWR: u32 = 2;
/// Fast recovery (retransmitting lost packets).
pub const TCP_CA_RECOVERY: u32 = 3;
/// Loss recovery (RTO timeout, slow start).
pub const TCP_CA_LOSS: u32 = 4;

// ---------------------------------------------------------------------------
// TCP congestion control flags
// ---------------------------------------------------------------------------

/// Algorithm requires ECN support.
pub const TCP_CC_FLAG_ECN: u32 = 0x01;
/// Algorithm is delay-based (not loss-based).
pub const TCP_CC_FLAG_DELAY_BASED: u32 = 0x02;
/// Algorithm requires RTT samples.
pub const TCP_CC_FLAG_RTT_STAMP: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithms_distinct() {
        let algs = [
            TCP_CC_RENO, TCP_CC_CUBIC, TCP_CC_BBR, TCP_CC_BBRV2,
            TCP_CC_DCTCP, TCP_CC_VEGAS, TCP_CC_WESTWOOD,
            TCP_CC_HTCP, TCP_CC_ILLINOIS, TCP_CC_CDG,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            TCP_CA_EVENT_LOSS, TCP_CA_EVENT_ECN,
            TCP_CA_EVENT_FAST_RECOVERY, TCP_CA_EVENT_CWND_RESTART,
            TCP_CA_EVENT_SLOW_START_EXIT,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            TCP_CA_OPEN, TCP_CA_DISORDER, TCP_CA_CWR,
            TCP_CA_RECOVERY, TCP_CA_LOSS,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            TCP_CC_FLAG_ECN, TCP_CC_FLAG_DELAY_BASED,
            TCP_CC_FLAG_RTT_STAMP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
