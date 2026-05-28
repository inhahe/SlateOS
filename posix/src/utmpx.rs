//! POSIX user accounting database stubs (`<utmpx.h>`).
//!
//! Our OS does not maintain a utmp/wtmp database.  These stubs allow
//! programs that enumerate logged-in users (e.g., `who`, `w`, `last`)
//! to compile and run without error — they simply see no entries.
//!
//! ## Stubbed Functions
//!
//! - `setutxent` — rewind to beginning of utmpx database
//! - `getutxent` — read next entry from utmpx database
//! - `getutxid` — find entry by id/type
//! - `getutxline` — find entry by terminal line
//! - `pututxline` — write entry to utmpx database
//! - `endutxent` — close utmpx database
//! - `utmpxname` — set alternate utmpx database path
//!
//! All "read" functions return NULL (no entries).  Write functions
//! silently succeed.

use crate::errno;

// ---------------------------------------------------------------------------
// utmpx entry types (Linux values)
// ---------------------------------------------------------------------------

/// Empty entry.
pub const EMPTY: i16 = 0;
/// Entry for a process that started a run level change.
pub const RUN_LVL: i16 = 1;
/// Entry for the system boot time.
pub const BOOT_TIME: i16 = 2;
/// Time after system clock changed.
pub const NEW_TIME: i16 = 3;
/// Time when system clock changed.
pub const OLD_TIME: i16 = 4;
/// Entry for a process started by `init`.
pub const INIT_PROCESS: i16 = 5;
/// Entry for a session leader (login process).
pub const LOGIN_PROCESS: i16 = 6;
/// Normal user process.
pub const USER_PROCESS: i16 = 7;
/// Terminated process.
pub const DEAD_PROCESS: i16 = 8;
/// Not defined on Linux, reserved for future use.
pub const ACCOUNTING: i16 = 9;

// ---------------------------------------------------------------------------
// struct utmpx
// ---------------------------------------------------------------------------

/// Size of the `ut_user` field.
pub const UT_NAMESIZE: usize = 32;
/// Size of the `ut_line` field.
pub const UT_LINESIZE: usize = 32;
/// Size of the `ut_host` field.
pub const UT_HOSTSIZE: usize = 256;
/// Size of the `ut_id` field.
pub const UT_IDSIZE: usize = 4;

/// Timeval for utmpx (32-bit fields for compatibility).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UtmpxTimeval {
    /// Seconds since epoch.
    pub tv_sec: i32,
    /// Microseconds.
    pub tv_usec: i32,
}

/// User accounting database entry.
///
/// Matches the POSIX `struct utmpx` layout (glibc-compatible).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Utmpx {
    /// Type of entry (EMPTY, USER_PROCESS, etc.).
    pub ut_type: i16,
    /// PID of login process.
    pub ut_pid: i32,
    /// Terminal device name (`/dev/ttyN`).
    pub ut_line: [u8; UT_LINESIZE],
    /// Identifier for the entry (typically terminal name suffix).
    pub ut_id: [u8; UT_IDSIZE],
    /// Username.
    pub ut_user: [u8; UT_NAMESIZE],
    /// Hostname for remote login.
    pub ut_host: [u8; UT_HOSTSIZE],
    /// Exit status for DEAD_PROCESS entries.
    pub ut_exit: [u8; 4], // struct exit_status { e_termination, e_exit }
    /// Session ID.
    pub ut_session: i32,
    /// Time entry was made.
    pub ut_tv: UtmpxTimeval,
    /// Internet address of remote host (IPv6).
    pub ut_addr_v6: [i32; 4],
    /// Reserved for future use.
    _reserved: [u8; 20],
}

// ---------------------------------------------------------------------------
// Stub functions
// ---------------------------------------------------------------------------

/// Rewind the utmpx database to the beginning.
///
/// Stub: no-op (no database to rewind).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setutxent() {
    // No-op: no utmpx database.
}

/// Read the next entry from the utmpx database.
///
/// Stub: always returns NULL (no entries).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutxent() -> *mut Utmpx {
    core::ptr::null_mut()
}

/// Find an entry in the utmpx database matching the given id/type.
///
/// Stub: always returns NULL (no entries).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutxid(_id: *const Utmpx) -> *mut Utmpx {
    core::ptr::null_mut()
}

/// Find an entry in the utmpx database matching the given terminal line.
///
/// Stub: always returns NULL (no entries).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutxline(_line: *const Utmpx) -> *mut Utmpx {
    core::ptr::null_mut()
}

/// Write an entry to the utmpx database.
///
/// Stub: returns the input pointer (pretend success).  No actual
/// database is written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pututxline(utmpx: *const Utmpx) -> *mut Utmpx {
    if utmpx.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // Return a "mutable" pointer to the same entry (pretend we wrote it).
    utmpx as *mut Utmpx
}

/// Close the utmpx database.
///
/// Stub: no-op (no database to close).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endutxent() {
    // No-op: no utmpx database.
}

/// Set the name of the utmpx database file.
///
/// Stub: always returns 0 (success).  We don't actually open any file.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utmpxname(_file: *const u8) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// glibc compatibility aliases
// ---------------------------------------------------------------------------

/// `setutent` — glibc alias for `setutxent`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setutent() {
    setutxent();
}

/// `getutent` — glibc alias for `getutxent`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutent() -> *mut Utmpx {
    getutxent()
}

/// `getutid` — glibc alias for `getutxid`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutid(id: *const Utmpx) -> *mut Utmpx {
    getutxid(id)
}

/// `getutline` — glibc alias for `getutxline`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutline(line: *const Utmpx) -> *mut Utmpx {
    getutxline(line)
}

/// `pututline` — glibc alias for `pututxline`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pututline(utmpx: *const Utmpx) -> *mut Utmpx {
    pututxline(utmpx)
}

/// `endutent` — glibc alias for `endutxent`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endutent() {
    endutxent();
}

/// `utmpname` — glibc alias for `utmpxname`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utmpname(file: *const u8) -> i32 {
    utmpxname(file)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Entry type constants --

    #[test]
    fn test_entry_type_values() {
        assert_eq!(EMPTY, 0);
        assert_eq!(RUN_LVL, 1);
        assert_eq!(BOOT_TIME, 2);
        assert_eq!(NEW_TIME, 3);
        assert_eq!(OLD_TIME, 4);
        assert_eq!(INIT_PROCESS, 5);
        assert_eq!(LOGIN_PROCESS, 6);
        assert_eq!(USER_PROCESS, 7);
        assert_eq!(DEAD_PROCESS, 8);
        assert_eq!(ACCOUNTING, 9);
    }

    #[test]
    fn test_entry_types_distinct() {
        let types = [
            EMPTY, RUN_LVL, BOOT_TIME, NEW_TIME, OLD_TIME,
            INIT_PROCESS, LOGIN_PROCESS, USER_PROCESS, DEAD_PROCESS,
            ACCOUNTING,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j], "types[{i}] == types[{j}]");
            }
        }
    }

    // -- Size constants --

    #[test]
    fn test_size_constants() {
        assert_eq!(UT_NAMESIZE, 32);
        assert_eq!(UT_LINESIZE, 32);
        assert_eq!(UT_HOSTSIZE, 256);
        assert_eq!(UT_IDSIZE, 4);
    }

    // -- Struct layout --

    #[test]
    fn test_utmpx_size() {
        let size = core::mem::size_of::<Utmpx>();
        // Should be large enough to hold all fields.
        assert!(size >= 380, "Utmpx should be at least 380 bytes, got {size}");
    }

    #[test]
    fn test_utmpx_alignment() {
        assert!(core::mem::align_of::<Utmpx>() >= 4,
            "Utmpx should be aligned to at least 4 bytes");
    }

    #[test]
    fn test_utmpx_timeval_size() {
        assert_eq!(core::mem::size_of::<UtmpxTimeval>(), 8);
    }

    // -- setutxent / endutxent (no-ops) --

    #[test]
    fn test_setutxent_no_crash() {
        setutxent();
    }

    #[test]
    fn test_endutxent_no_crash() {
        endutxent();
    }

    #[test]
    fn test_set_end_cycle() {
        setutxent();
        endutxent();
        setutxent();
        endutxent();
    }

    // -- getutxent --

    #[test]
    fn test_getutxent_returns_null() {
        setutxent();
        assert!(getutxent().is_null());
    }

    #[test]
    fn test_getutxent_repeated_null() {
        setutxent();
        assert!(getutxent().is_null());
        assert!(getutxent().is_null());
    }

    // -- getutxid --

    #[test]
    fn test_getutxid_returns_null() {
        assert!(getutxid(core::ptr::null()).is_null());
    }

    #[test]
    fn test_getutxid_with_entry() {
        let entry = unsafe { core::mem::zeroed::<Utmpx>() };
        assert!(getutxid(&entry).is_null());
    }

    // -- getutxline --

    #[test]
    fn test_getutxline_returns_null() {
        assert!(getutxline(core::ptr::null()).is_null());
    }

    #[test]
    fn test_getutxline_with_entry() {
        let entry = unsafe { core::mem::zeroed::<Utmpx>() };
        assert!(getutxline(&entry).is_null());
    }

    // -- pututxline --

    #[test]
    fn test_pututxline_null_returns_null() {
        errno::set_errno(0);
        assert!(pututxline(core::ptr::null()).is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_pututxline_returns_input() {
        let entry = unsafe { core::mem::zeroed::<Utmpx>() };
        let result = pututxline(&entry);
        assert!(!result.is_null());
        assert_eq!(result as *const Utmpx, &entry as *const Utmpx);
    }

    // -- utmpxname --

    #[test]
    fn test_utmpxname_succeeds() {
        assert_eq!(utmpxname(b"/var/run/utmpx\0".as_ptr()), 0);
    }

    #[test]
    fn test_utmpxname_null() {
        assert_eq!(utmpxname(core::ptr::null()), 0);
    }

    // -- glibc alias tests --

    #[test]
    fn test_setutent_no_crash() {
        setutent();
    }

    #[test]
    fn test_getutent_returns_null() {
        assert!(getutent().is_null());
    }

    #[test]
    fn test_endutent_no_crash() {
        endutent();
    }

    #[test]
    fn test_getutid_returns_null() {
        assert!(getutid(core::ptr::null()).is_null());
    }

    #[test]
    fn test_getutline_returns_null() {
        assert!(getutline(core::ptr::null()).is_null());
    }

    #[test]
    fn test_pututline_null() {
        assert!(pututline(core::ptr::null()).is_null());
    }

    #[test]
    fn test_pututline_returns_input() {
        let entry = unsafe { core::mem::zeroed::<Utmpx>() };
        let result = pututline(&entry);
        assert!(!result.is_null());
    }

    #[test]
    fn test_utmpname_succeeds() {
        assert_eq!(utmpname(b"/var/run/utmp\0".as_ptr()), 0);
    }

    // -- Full enumeration pattern --

    #[test]
    fn test_full_enumeration_pattern() {
        // Typical usage: setutxent(); while (getutxent()) {...}; endutxent();
        setutxent();
        let mut count = 0;
        loop {
            let entry = getutxent();
            if entry.is_null() {
                break;
            }
            count += 1;
        }
        endutxent();
        assert_eq!(count, 0);
    }
}
