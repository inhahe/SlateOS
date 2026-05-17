//! `<linux/interrupt.h>` (softirq subset) — Soft interrupt constants.
//!
//! Softirqs are deferred interrupt handlers that run at a lower
//! priority than hardware interrupts but higher than process context.
//! They're used for high-frequency work that must complete quickly
//! but doesn't need to happen in hard IRQ context: network packet
//! processing (NET_TX/NET_RX), block I/O completion, timer callbacks,
//! scheduler balancing, and RCU processing. Softirqs run on the CPU
//! that raised them, with interrupts enabled.

// ---------------------------------------------------------------------------
// Softirq vectors (priorities, lower number = higher priority)
// ---------------------------------------------------------------------------

/// High-priority tasklets.
pub const HI_SOFTIRQ: u32 = 0;
/// Timer expiration processing.
pub const TIMER_SOFTIRQ: u32 = 1;
/// Network TX completion.
pub const NET_TX_SOFTIRQ: u32 = 2;
/// Network RX processing (NAPI).
pub const NET_RX_SOFTIRQ: u32 = 3;
/// Block device I/O completion.
pub const BLOCK_SOFTIRQ: u32 = 4;
/// IRQ poll (block device polling).
pub const IRQ_POLL_SOFTIRQ: u32 = 5;
/// Normal-priority tasklets.
pub const TASKLET_SOFTIRQ: u32 = 6;
/// Scheduler load balancing.
pub const SCHED_SOFTIRQ: u32 = 7;
/// High-resolution timer expiration.
pub const HRTIMER_SOFTIRQ: u32 = 8;
/// RCU (Read-Copy-Update) callback processing.
pub const RCU_SOFTIRQ: u32 = 9;
/// Number of softirq vectors.
pub const NR_SOFTIRQS: u32 = 10;

// ---------------------------------------------------------------------------
// Softirq processing limits
// ---------------------------------------------------------------------------

/// Maximum softirqs to process before deferring to ksoftirqd.
pub const MAX_SOFTIRQ_RESTART: u32 = 10;
/// Maximum time to spend in softirq processing (ms).
pub const MAX_SOFTIRQ_TIME_MS: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_softirq_vectors_distinct() {
        let vectors = [
            HI_SOFTIRQ, TIMER_SOFTIRQ, NET_TX_SOFTIRQ,
            NET_RX_SOFTIRQ, BLOCK_SOFTIRQ, IRQ_POLL_SOFTIRQ,
            TASKLET_SOFTIRQ, SCHED_SOFTIRQ, HRTIMER_SOFTIRQ,
            RCU_SOFTIRQ,
        ];
        assert_eq!(vectors.len(), NR_SOFTIRQS as usize);
        for i in 0..vectors.len() {
            for j in (i + 1)..vectors.len() {
                assert_ne!(vectors[i], vectors[j]);
            }
        }
    }

    #[test]
    fn test_limits_positive() {
        assert!(MAX_SOFTIRQ_RESTART > 0);
        assert!(MAX_SOFTIRQ_TIME_MS > 0);
    }
}
