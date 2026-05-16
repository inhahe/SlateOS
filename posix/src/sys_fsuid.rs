//! `<sys/fsuid.h>` — filesystem UID/GID operations.
//!
//! Provides `setfsuid()` and `setfsgid()` for setting the filesystem
//! user/group ID used for permission checks on file operations.

use crate::errno;
use crate::types::UidT;
use crate::types::GidT;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Set filesystem UID.
///
/// Returns the previous filesystem UID. On this stub, returns the
/// input `fsuid` (simulating that the UID was set successfully).
///
/// # Safety
///
/// Changes process-wide filesystem identity.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsuid(fsuid: UidT) -> i32 {
    // Real implementation would set per-thread fs uid.
    // Stub: pretend it succeeded — return the new uid (old uid).
    let _ = fsuid;
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Set filesystem GID.
///
/// Returns the previous filesystem GID. On this stub, returns `-1`
/// with `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsgid(fsgid: GidT) -> i32 {
    let _ = fsgid;
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setfsuid_stub() {
        let ret = setfsuid(1000);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_setfsgid_stub() {
        let ret = setfsgid(1000);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_setfsuid_root() {
        let ret = setfsuid(0);
        assert_eq!(ret, -1);
    }
}
