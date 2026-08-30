//! `<linux/sock_diag.h>` — Additional socket diagnostics constants.
//!
//! Supplementary socket diagnostics constants covering attribute types,
//! request types, and shutdown state values.

// ---------------------------------------------------------------------------
// Socket diagnostics request types
// ---------------------------------------------------------------------------

/// By family.
pub const SOCK_DIAG_BY_FAMILY2: u32 = 20;
/// Socket destroy.
pub const SOCK_DESTROY2: u32 = 21;

// ---------------------------------------------------------------------------
// Socket diagnostics attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const SK_DIAG_ATTR_UNSPEC: u32 = 0;
/// Memory info.
pub const SK_DIAG_ATTR_MEMINFO: u32 = 1;
/// Shutdown state.
pub const SK_DIAG_ATTR_SHUTDOWN: u32 = 2;
/// Protocol.
pub const SK_DIAG_ATTR_PROTOCOL: u32 = 3;

// ---------------------------------------------------------------------------
// Socket diagnostic memory info fields
// ---------------------------------------------------------------------------

/// Receive memory allocated.
pub const SK_MEMINFO_RMEM_ALLOC: u32 = 0;
/// Receive buffer size.
pub const SK_MEMINFO_RCVBUF: u32 = 1;
/// Write memory allocated.
pub const SK_MEMINFO_WMEM_ALLOC: u32 = 2;
/// Send buffer size.
pub const SK_MEMINFO_SNDBUF: u32 = 3;
/// Forward alloc.
pub const SK_MEMINFO_FWD_ALLOC: u32 = 4;
/// Write memory queued.
pub const SK_MEMINFO_WMEM_QUEUED: u32 = 5;
/// Opt memory.
pub const SK_MEMINFO_OPTMEM: u32 = 6;
/// Backlog.
pub const SK_MEMINFO_BACKLOG: u32 = 7;
/// Drops.
pub const SK_MEMINFO_DROPS: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_req_types_distinct() {
        assert_ne!(SOCK_DIAG_BY_FAMILY2, SOCK_DESTROY2);
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            SK_DIAG_ATTR_UNSPEC,
            SK_DIAG_ATTR_MEMINFO,
            SK_DIAG_ATTR_SHUTDOWN,
            SK_DIAG_ATTR_PROTOCOL,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_meminfo_fields_distinct() {
        let fields = [
            SK_MEMINFO_RMEM_ALLOC,
            SK_MEMINFO_RCVBUF,
            SK_MEMINFO_WMEM_ALLOC,
            SK_MEMINFO_SNDBUF,
            SK_MEMINFO_FWD_ALLOC,
            SK_MEMINFO_WMEM_QUEUED,
            SK_MEMINFO_OPTMEM,
            SK_MEMINFO_BACKLOG,
            SK_MEMINFO_DROPS,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }
}
