//! `<linux/cachefiles.h>` — CacheFiles daemon interface constants.
//!
//! CacheFiles provides persistent local caching of network
//! filesystems (NFS, AFS, CIFS) via a cache on a local filesystem.
//! These constants define daemon commands, object types, and modes.

// ---------------------------------------------------------------------------
// CacheFiles daemon commands
// ---------------------------------------------------------------------------

/// Bind cache to directory.
pub const CACHEFILES_CMD_BIND: u32 = 0;
/// Unbind cache.
pub const CACHEFILES_CMD_UNBIND: u32 = 1;
/// Cull objects.
pub const CACHEFILES_CMD_CULL: u32 = 2;
/// Debug mode.
pub const CACHEFILES_CMD_DEBUG: u32 = 3;
/// Secure label.
pub const CACHEFILES_CMD_SECCTX: u32 = 4;
/// Tag cache.
pub const CACHEFILES_CMD_TAG: u32 = 5;

// ---------------------------------------------------------------------------
// CacheFiles on-demand mode commands
// ---------------------------------------------------------------------------

/// Open request from kernel.
pub const CACHEFILES_OP_OPEN: u32 = 0;
/// Close request from kernel.
pub const CACHEFILES_OP_CLOSE: u32 = 1;
/// Read request from kernel.
pub const CACHEFILES_OP_READ: u32 = 2;

// ---------------------------------------------------------------------------
// CacheFiles IOCTL
// ---------------------------------------------------------------------------

/// On-demand read IOCTL.
pub const CACHEFILES_IOC_READ_COMPLETE: u32 = 0x01;

// ---------------------------------------------------------------------------
// Fscache object states
// ---------------------------------------------------------------------------

/// Object looking up.
pub const FSCACHE_OBJECT_LOOKING_UP: u32 = 0;
/// Object creating.
pub const FSCACHE_OBJECT_CREATING: u32 = 1;
/// Object available.
pub const FSCACHE_OBJECT_AVAILABLE: u32 = 2;
/// Object active.
pub const FSCACHE_OBJECT_ACTIVE: u32 = 3;
/// Object invalidating.
pub const FSCACHE_OBJECT_INVALIDATING: u32 = 4;
/// Object updating.
pub const FSCACHE_OBJECT_UPDATING: u32 = 5;
/// Object dying.
pub const FSCACHE_OBJECT_DYING: u32 = 6;
/// Object dead.
pub const FSCACHE_OBJECT_DEAD: u32 = 7;

// ---------------------------------------------------------------------------
// Fscache cookie type
// ---------------------------------------------------------------------------

/// Index cookie (directory).
pub const FSCACHE_COOKIE_TYPE_INDEX: u32 = 0;
/// Data cookie (file).
pub const FSCACHE_COOKIE_TYPE_DATAFILE: u32 = 1;
/// Other cookie.
pub const FSCACHE_COOKIE_TYPE_OTHER: u32 = 2;

// ---------------------------------------------------------------------------
// Fscache cookie flags
// ---------------------------------------------------------------------------

/// Cookie looking up.
pub const FSCACHE_COOKIE_LOOKING_UP: u32 = 0;
/// Cookie creating.
pub const FSCACHE_COOKIE_CREATING: u32 = 1;
/// Cookie no data yet.
pub const FSCACHE_COOKIE_NO_DATA_TO_READ: u32 = 2;

// ---------------------------------------------------------------------------
// Cache culling limits
// ---------------------------------------------------------------------------

/// Default block cache limit (percent).
pub const CACHEFILES_BRUN_PERCENT: u32 = 7;
/// Default block cull limit (percent).
pub const CACHEFILES_BCULL_PERCENT: u32 = 6;
/// Default block stop limit (percent).
pub const CACHEFILES_BSTOP_PERCENT: u32 = 1;
/// Default file run percent.
pub const CACHEFILES_FRUN_PERCENT: u32 = 7;
/// Default file cull percent.
pub const CACHEFILES_FCULL_PERCENT: u32 = 6;
/// Default file stop percent.
pub const CACHEFILES_FSTOP_PERCENT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_values_sequential() {
        assert_eq!(CACHEFILES_CMD_BIND, 0);
        assert_eq!(CACHEFILES_CMD_UNBIND, 1);
        assert_eq!(CACHEFILES_CMD_CULL, 2);
        assert_eq!(CACHEFILES_CMD_DEBUG, 3);
        assert_eq!(CACHEFILES_CMD_SECCTX, 4);
        assert_eq!(CACHEFILES_CMD_TAG, 5);
    }

    #[test]
    fn test_op_values_sequential() {
        assert_eq!(CACHEFILES_OP_OPEN, 0);
        assert_eq!(CACHEFILES_OP_CLOSE, 1);
        assert_eq!(CACHEFILES_OP_READ, 2);
    }

    #[test]
    fn test_object_states_sequential() {
        assert_eq!(FSCACHE_OBJECT_LOOKING_UP, 0);
        assert_eq!(FSCACHE_OBJECT_CREATING, 1);
        assert_eq!(FSCACHE_OBJECT_AVAILABLE, 2);
        assert_eq!(FSCACHE_OBJECT_DEAD, 7);
    }

    #[test]
    fn test_object_states_distinct() {
        let states = [
            FSCACHE_OBJECT_LOOKING_UP,
            FSCACHE_OBJECT_CREATING,
            FSCACHE_OBJECT_AVAILABLE,
            FSCACHE_OBJECT_ACTIVE,
            FSCACHE_OBJECT_INVALIDATING,
            FSCACHE_OBJECT_UPDATING,
            FSCACHE_OBJECT_DYING,
            FSCACHE_OBJECT_DEAD,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_cookie_types_sequential() {
        assert_eq!(FSCACHE_COOKIE_TYPE_INDEX, 0);
        assert_eq!(FSCACHE_COOKIE_TYPE_DATAFILE, 1);
        assert_eq!(FSCACHE_COOKIE_TYPE_OTHER, 2);
    }

    #[test]
    fn test_culling_limits() {
        assert!(CACHEFILES_BSTOP_PERCENT < CACHEFILES_BCULL_PERCENT);
        assert!(CACHEFILES_BCULL_PERCENT < CACHEFILES_BRUN_PERCENT);
    }

    #[test]
    fn test_ioctl() {
        assert_eq!(CACHEFILES_IOC_READ_COMPLETE, 0x01);
    }
}
