//! `<linux/dm-log-userspace.h>` — Device Mapper log-userspace constants.
//!
//! Used by device-mapper mirror targets to keep dirty-region logs
//! in userspace (via the dm-log-userspace daemon) rather than
//! on-disk. Part of the LVM/device-mapper infrastructure.

// ---------------------------------------------------------------------------
// DM log-userspace request types
// ---------------------------------------------------------------------------

/// Constructor.
pub const DM_ULOG_CTR: u32 = 1;
/// Destructor.
pub const DM_ULOG_DTR: u32 = 2;
/// Resume.
pub const DM_ULOG_RESUME: u32 = 3;
/// Suspend.
pub const DM_ULOG_SUSPEND: u32 = 4;
/// Get region size.
pub const DM_ULOG_GET_REGION_SIZE: u32 = 5;
/// Is clean.
pub const DM_ULOG_IS_CLEAN: u32 = 6;
/// In sync.
pub const DM_ULOG_IN_SYNC: u32 = 7;
/// Flush.
pub const DM_ULOG_FLUSH: u32 = 8;
/// Mark region.
pub const DM_ULOG_MARK_REGION: u32 = 9;
/// Clear region.
pub const DM_ULOG_CLEAR_REGION: u32 = 10;
/// Get resync work.
pub const DM_ULOG_GET_RESYNC_WORK: u32 = 11;
/// Set region sync.
pub const DM_ULOG_SET_REGION_SYNC: u32 = 12;
/// Get sync count.
pub const DM_ULOG_GET_SYNC_COUNT: u32 = 13;
/// Status info.
pub const DM_ULOG_STATUS_INFO: u32 = 14;
/// Status table.
pub const DM_ULOG_STATUS_TABLE: u32 = 15;
/// Is remote recovering.
pub const DM_ULOG_IS_REMOTE_RECOVERING: u32 = 16;

/// Maximum request type.
pub const DM_ULOG_REQUEST_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_types_sequential() {
        assert_eq!(DM_ULOG_CTR, 1);
        assert_eq!(DM_ULOG_DTR, 2);
        assert_eq!(DM_ULOG_FLUSH, 8);
        assert_eq!(DM_ULOG_IS_REMOTE_RECOVERING, 16);
    }

    #[test]
    fn test_request_types_distinct() {
        let types = [
            DM_ULOG_CTR, DM_ULOG_DTR, DM_ULOG_RESUME,
            DM_ULOG_SUSPEND, DM_ULOG_GET_REGION_SIZE,
            DM_ULOG_IS_CLEAN, DM_ULOG_IN_SYNC, DM_ULOG_FLUSH,
            DM_ULOG_MARK_REGION, DM_ULOG_CLEAR_REGION,
            DM_ULOG_GET_RESYNC_WORK, DM_ULOG_SET_REGION_SYNC,
            DM_ULOG_GET_SYNC_COUNT, DM_ULOG_STATUS_INFO,
            DM_ULOG_STATUS_TABLE, DM_ULOG_IS_REMOTE_RECOVERING,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_max() {
        assert_eq!(DM_ULOG_REQUEST_MAX, DM_ULOG_IS_REMOTE_RECOVERING);
    }
}
