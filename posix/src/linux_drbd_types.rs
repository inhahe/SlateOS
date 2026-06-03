//! `<linux/drbd.h>` — DRBD (Distributed Replicated Block Device) constants.
//!
//! DRBD constants covering connection states, disk states,
//! roles, packet types, and replication protocols.

// ---------------------------------------------------------------------------
// DRBD connection states
// ---------------------------------------------------------------------------

/// Standalone.
pub const C_STANDALONE: u32 = 0;
/// Disconnecting.
pub const C_DISCONNECTING: u32 = 1;
/// Unconnected.
pub const C_UNCONNECTED: u32 = 2;
/// Timeout.
pub const C_TIMEOUT: u32 = 3;
/// Broken pipe.
pub const C_BROKEN_PIPE: u32 = 4;
/// Network failure.
pub const C_NETWORK_FAILURE: u32 = 5;
/// Protocol error.
pub const C_PROTOCOL_ERROR: u32 = 6;
/// Tear down.
pub const C_TEAR_DOWN: u32 = 7;
/// Wait for connection.
pub const C_WF_CONNECTION: u32 = 8;
/// Wait for report params.
pub const C_WF_REPORT_PARAMS: u32 = 9;
/// Connected.
pub const C_CONNECTED: u32 = 10;
/// Starting sync source.
pub const C_STARTING_SYNC_S: u32 = 11;
/// Starting sync target.
pub const C_STARTING_SYNC_T: u32 = 12;
/// WF bitmap source.
pub const C_WF_BITMAP_S: u32 = 13;
/// WF bitmap target.
pub const C_WF_BITMAP_T: u32 = 14;
/// WF sync UUID.
pub const C_WF_SYNC_UUID: u32 = 15;
/// Sync source.
pub const C_SYNC_SOURCE: u32 = 16;
/// Sync target.
pub const C_SYNC_TARGET: u32 = 17;
/// Verify source.
pub const C_VERIFY_S: u32 = 18;
/// Verify target.
pub const C_VERIFY_T: u32 = 19;
/// Paused sync source.
pub const C_PAUSED_SYNC_S: u32 = 20;
/// Paused sync target.
pub const C_PAUSED_SYNC_T: u32 = 21;
/// Ahead.
pub const C_AHEAD: u32 = 22;
/// Behind.
pub const C_BEHIND: u32 = 23;

// ---------------------------------------------------------------------------
// DRBD disk states
// ---------------------------------------------------------------------------

/// Diskless.
pub const D_DISKLESS: u32 = 0;
/// Attaching.
pub const D_ATTACHING: u32 = 1;
/// Failed.
pub const D_FAILED: u32 = 2;
/// Negotiating.
pub const D_NEGOTIATING: u32 = 3;
/// Inconsistent.
pub const D_INCONSISTENT: u32 = 4;
/// Outdated.
pub const D_OUTDATED: u32 = 5;
/// D-unknown.
pub const D_UNKNOWN: u32 = 6;
/// Consistent.
pub const D_CONSISTENT: u32 = 7;
/// Up to date.
pub const D_UP_TO_DATE: u32 = 8;

// ---------------------------------------------------------------------------
// DRBD roles
// ---------------------------------------------------------------------------

/// Unknown role.
pub const R_UNKNOWN: u32 = 0;
/// Primary.
pub const R_PRIMARY: u32 = 1;
/// Secondary.
pub const R_SECONDARY: u32 = 2;

// ---------------------------------------------------------------------------
// DRBD replication protocols
// ---------------------------------------------------------------------------

/// Protocol A (async).
pub const DRBD_PROT_A: u32 = 1;
/// Protocol B (semi-sync).
pub const DRBD_PROT_B: u32 = 2;
/// Protocol C (sync).
pub const DRBD_PROT_C: u32 = 3;

// ---------------------------------------------------------------------------
// DRBD flags
// ---------------------------------------------------------------------------

/// Bitmap IO.
pub const DRBD_FLAG_BITMAP_IO: u32 = 1 << 0;
/// Discard concurrent.
pub const DRBD_FLAG_DISCARD_CONCURRENT: u32 = 1 << 1;
/// Force detach.
pub const DRBD_FLAG_FORCE_DETACH: u32 = 1 << 2;
/// Skip initial sync.
pub const DRBD_FLAG_SKIP_INITIAL_SYNC: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// DRBD magic
// ---------------------------------------------------------------------------

/// DRBD magic cookie.
pub const DRBD_MAGIC: u32 = 0x83740267;
/// DRBD magic big (big-endian).
pub const DRBD_MAGIC_BIG: u32 = 0x835a_0267;
/// DRBD magic 100 (version).
pub const DRBD_MAGIC_100: u32 = 0x8620_ec00;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conn_states_distinct() {
        let states = [
            C_STANDALONE,
            C_DISCONNECTING,
            C_UNCONNECTED,
            C_TIMEOUT,
            C_BROKEN_PIPE,
            C_NETWORK_FAILURE,
            C_PROTOCOL_ERROR,
            C_TEAR_DOWN,
            C_WF_CONNECTION,
            C_WF_REPORT_PARAMS,
            C_CONNECTED,
            C_STARTING_SYNC_S,
            C_STARTING_SYNC_T,
            C_WF_BITMAP_S,
            C_WF_BITMAP_T,
            C_WF_SYNC_UUID,
            C_SYNC_SOURCE,
            C_SYNC_TARGET,
            C_VERIFY_S,
            C_VERIFY_T,
            C_PAUSED_SYNC_S,
            C_PAUSED_SYNC_T,
            C_AHEAD,
            C_BEHIND,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_disk_states_distinct() {
        let states = [
            D_DISKLESS,
            D_ATTACHING,
            D_FAILED,
            D_NEGOTIATING,
            D_INCONSISTENT,
            D_OUTDATED,
            D_UNKNOWN,
            D_CONSISTENT,
            D_UP_TO_DATE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_roles_distinct() {
        let roles = [R_UNKNOWN, R_PRIMARY, R_SECONDARY];
        for i in 0..roles.len() {
            for j in (i + 1)..roles.len() {
                assert_ne!(roles[i], roles[j]);
            }
        }
    }

    #[test]
    fn test_protocols() {
        assert_eq!(DRBD_PROT_A, 1);
        assert_eq!(DRBD_PROT_B, 2);
        assert_eq!(DRBD_PROT_C, 3);
    }

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            DRBD_FLAG_BITMAP_IO,
            DRBD_FLAG_DISCARD_CONCURRENT,
            DRBD_FLAG_FORCE_DETACH,
            DRBD_FLAG_SKIP_INITIAL_SYNC,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            DRBD_FLAG_BITMAP_IO,
            DRBD_FLAG_DISCARD_CONCURRENT,
            DRBD_FLAG_FORCE_DETACH,
            DRBD_FLAG_SKIP_INITIAL_SYNC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_magic_distinct() {
        let magics = [DRBD_MAGIC, DRBD_MAGIC_BIG, DRBD_MAGIC_100];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }
}
