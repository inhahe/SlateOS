//! `<linux/blkdev.h>` — block-layer request state and timeout
//! constants surfaced through sysfs and tracepoints.
//!
//! These are the user-visible knobs around the lifecycle of a single
//! block request: state codes (issued/completed/timeout), per-queue
//! timeout, retry counters, and command-flag bits read by `blktrace`
//! and `bpftrace`-style profilers.

// ---------------------------------------------------------------------------
// Request state (`enum req_state` / `MQ_RQ_*`)
// ---------------------------------------------------------------------------

pub const MQ_RQ_IDLE: u32 = 0;
pub const MQ_RQ_IN_FLIGHT: u32 = 1;
pub const MQ_RQ_COMPLETE: u32 = 2;

// ---------------------------------------------------------------------------
// Per-queue timeout / retry limits
// ---------------------------------------------------------------------------

/// Default per-request timeout (ms).
pub const BLK_DEFAULT_TIMEOUT_MS: u32 = 30_000;

/// Maximum per-request timeout (5 minutes).
pub const BLK_MAX_TIMEOUT_MS: u32 = 300_000;

/// Maximum software-retries the block layer attempts before giving up.
pub const BLK_MAX_RETRIES: u32 = 5;

// ---------------------------------------------------------------------------
// Request issue flags
// ---------------------------------------------------------------------------

pub const BLK_MQ_REQ_NOWAIT: u32 = 1 << 0;
pub const BLK_MQ_REQ_RESERVED: u32 = 1 << 1;
pub const BLK_MQ_REQ_PM: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Completion-status codes (`blk_status_t`)
// ---------------------------------------------------------------------------

pub const BLK_STS_OK: u32 = 0;
pub const BLK_STS_NOTSUPP: u32 = 1;
pub const BLK_STS_TIMEOUT: u32 = 2;
pub const BLK_STS_NOSPC: u32 = 3;
pub const BLK_STS_TRANSPORT: u32 = 4;
pub const BLK_STS_TARGET: u32 = 5;
pub const BLK_STS_NEXUS: u32 = 6;
pub const BLK_STS_MEDIUM: u32 = 7;
pub const BLK_STS_PROTECTION: u32 = 8;
pub const BLK_STS_RESOURCE: u32 = 9;
pub const BLK_STS_IOERR: u32 = 10;
pub const BLK_STS_AGAIN: u32 = 12;
pub const BLK_STS_DEV_RESOURCE: u32 = 13;
pub const BLK_STS_ZONE_RESOURCE: u32 = 14;
pub const BLK_STS_ZONE_OPEN_RESOURCE: u32 = 15;
pub const BLK_STS_ZONE_ACTIVE_RESOURCE: u32 = 16;
pub const BLK_STS_OFFLINE: u32 = 17;
pub const BLK_STS_DURATION_LIMIT: u32 = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rq_states_dense_0_to_2() {
        let s = [MQ_RQ_IDLE, MQ_RQ_IN_FLIGHT, MQ_RQ_COMPLETE];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // IDLE is the freshly-allocated state.
        assert_eq!(MQ_RQ_IDLE, 0);
    }

    #[test]
    fn test_timeout_relations() {
        assert_eq!(BLK_DEFAULT_TIMEOUT_MS, 30_000);
        assert_eq!(BLK_MAX_TIMEOUT_MS, 300_000);
        // Max timeout is exactly 10x the default.
        assert_eq!(BLK_MAX_TIMEOUT_MS / BLK_DEFAULT_TIMEOUT_MS, 10);
        // Five software retries before giving up.
        assert_eq!(BLK_MAX_RETRIES, 5);
    }

    #[test]
    fn test_req_issue_flags_single_bits() {
        let f = [BLK_MQ_REQ_NOWAIT, BLK_MQ_REQ_RESERVED, BLK_MQ_REQ_PM];
        let mut or = 0;
        for &v in &f {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0b111);
    }

    #[test]
    fn test_blk_sts_codes_distinct() {
        let s = [
            BLK_STS_OK,
            BLK_STS_NOTSUPP,
            BLK_STS_TIMEOUT,
            BLK_STS_NOSPC,
            BLK_STS_TRANSPORT,
            BLK_STS_TARGET,
            BLK_STS_NEXUS,
            BLK_STS_MEDIUM,
            BLK_STS_PROTECTION,
            BLK_STS_RESOURCE,
            BLK_STS_IOERR,
            BLK_STS_AGAIN,
            BLK_STS_DEV_RESOURCE,
            BLK_STS_ZONE_RESOURCE,
            BLK_STS_ZONE_OPEN_RESOURCE,
            BLK_STS_ZONE_ACTIVE_RESOURCE,
            BLK_STS_OFFLINE,
            BLK_STS_DURATION_LIMIT,
        ];
        for (i, &a) in s.iter().enumerate() {
            for &b in &s[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // OK is the success sentinel.
        assert_eq!(BLK_STS_OK, 0);
    }

    #[test]
    fn test_status_first_block_dense_0_to_10() {
        // The legacy codes (0..10) are contiguous.
        let dense = [
            BLK_STS_OK,
            BLK_STS_NOTSUPP,
            BLK_STS_TIMEOUT,
            BLK_STS_NOSPC,
            BLK_STS_TRANSPORT,
            BLK_STS_TARGET,
            BLK_STS_NEXUS,
            BLK_STS_MEDIUM,
            BLK_STS_PROTECTION,
            BLK_STS_RESOURCE,
            BLK_STS_IOERR,
        ];
        for (i, &v) in dense.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // 11 was historically deprecated — AGAIN starts at 12.
        assert_eq!(BLK_STS_AGAIN, 12);
    }

    #[test]
    fn test_zone_resource_family_clustered() {
        // Three zone resource codes form a consecutive run 14..16.
        assert_eq!(BLK_STS_ZONE_RESOURCE, 14);
        assert_eq!(BLK_STS_ZONE_OPEN_RESOURCE, 15);
        assert_eq!(BLK_STS_ZONE_ACTIVE_RESOURCE, 16);
        assert_eq!(
            BLK_STS_ZONE_OPEN_RESOURCE - BLK_STS_ZONE_RESOURCE,
            1
        );
        assert_eq!(
            BLK_STS_ZONE_ACTIVE_RESOURCE - BLK_STS_ZONE_OPEN_RESOURCE,
            1
        );
    }
}
