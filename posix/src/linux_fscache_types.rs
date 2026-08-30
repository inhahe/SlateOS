//! `<linux/fscache.h>` — FS-Cache (filesystem caching) constants.
//!
//! FS-Cache provides a local on-disk cache for network filesystems
//! (NFS, AFS, CIFS, 9P). When a file is read from the network, the
//! data is stored in the cache backend (CacheFiles, which uses a
//! local filesystem as backing store). Subsequent reads of the same
//! data hit the local cache instead of going over the network.
//! FS-Cache handles cache coherency, space management, and indexing.

// ---------------------------------------------------------------------------
// FS-Cache object types
// ---------------------------------------------------------------------------

/// Volume index (top-level server/share identifier).
pub const FSCACHE_OBJ_VOLUME: u32 = 0;
/// Cookie (individual cached file/object).
pub const FSCACHE_OBJ_COOKIE: u32 = 1;

// ---------------------------------------------------------------------------
// FS-Cache cookie states
// ---------------------------------------------------------------------------

/// Cookie is looking up (checking if data exists in cache).
pub const FSCACHE_COOKIE_LOOKING_UP: u32 = 0;
/// Cookie has no backing data (cache miss).
pub const FSCACHE_COOKIE_NO_DATA: u32 = 1;
/// Cookie has data available in cache.
pub const FSCACHE_COOKIE_CACHED: u32 = 2;
/// Cookie is being invalidated (data is stale).
pub const FSCACHE_COOKIE_INVALIDATING: u32 = 3;
/// Cookie is being retired (final cleanup).
pub const FSCACHE_COOKIE_RETIRING: u32 = 4;

// ---------------------------------------------------------------------------
// FS-Cache access modes
// ---------------------------------------------------------------------------

/// Cache is read-only (no writes to cache).
pub const FSCACHE_ACCESS_RO: u32 = 0;
/// Cache is read-write (writes update cache).
pub const FSCACHE_ACCESS_RW: u32 = 1;

// ---------------------------------------------------------------------------
// CacheFiles culling states (space management)
// ---------------------------------------------------------------------------

/// Cull: no pressure (sufficient space).
pub const FSCACHE_CULL_NONE: u32 = 0;
/// Cull: low space (start culling old objects).
pub const FSCACHE_CULL_LOW: u32 = 1;
/// Cull: critical (aggressively free space).
pub const FSCACHE_CULL_CRITICAL: u32 = 2;
/// Cull: full (cache is full, reject new objects).
pub const FSCACHE_CULL_FULL: u32 = 3;

// ---------------------------------------------------------------------------
// FS-Cache coherency states
// ---------------------------------------------------------------------------

/// Data is coherent (cache matches server).
pub const FSCACHE_COHERENT_OK: u32 = 0;
/// Data needs revalidation (check with server).
pub const FSCACHE_COHERENT_NEEDS_CHECK: u32 = 1;
/// Data is stale (known to be outdated).
pub const FSCACHE_COHERENT_STALE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_types_distinct() {
        assert_ne!(FSCACHE_OBJ_VOLUME, FSCACHE_OBJ_COOKIE);
    }

    #[test]
    fn test_cookie_states_distinct() {
        let states = [
            FSCACHE_COOKIE_LOOKING_UP,
            FSCACHE_COOKIE_NO_DATA,
            FSCACHE_COOKIE_CACHED,
            FSCACHE_COOKIE_INVALIDATING,
            FSCACHE_COOKIE_RETIRING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_access_modes_distinct() {
        assert_ne!(FSCACHE_ACCESS_RO, FSCACHE_ACCESS_RW);
    }

    #[test]
    fn test_cull_states_ordered() {
        assert!(FSCACHE_CULL_NONE < FSCACHE_CULL_LOW);
        assert!(FSCACHE_CULL_LOW < FSCACHE_CULL_CRITICAL);
        assert!(FSCACHE_CULL_CRITICAL < FSCACHE_CULL_FULL);
    }

    #[test]
    fn test_coherency_distinct() {
        let states = [
            FSCACHE_COHERENT_OK,
            FSCACHE_COHERENT_NEEDS_CHECK,
            FSCACHE_COHERENT_STALE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
