//! POSIX `<ndbm.h>` — database operations.
//!
//! Stubs for `dbm_open`, `dbm_close`, `dbm_store`, `dbm_fetch`,
//! `dbm_delete`, `dbm_firstkey`, `dbm_nextkey`, `dbm_error`,
//! `dbm_clearerr`, `dbm_dirfno`, `dbm_pagfno`.
//!
//! Our OS does not implement a DBM database backend.  `dbm_open`
//! always fails, and all other operations return appropriate errors.
//!
//! These stubs satisfy link-time references from programs that use
//! the NDBM API.

use crate::errno;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `datum` — a key or value in the database.
///
/// A simple pointer + length pair, matching the POSIX `datum` structure.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Datum {
    /// Pointer to data bytes.
    pub dptr: *mut u8,
    /// Length in bytes.
    pub dsize: usize,
}

/// Null datum constant — used for error returns.
const NULL_DATUM: Datum = Datum {
    dptr: core::ptr::null_mut(),
    dsize: 0,
};

/// `DBM` — opaque database handle.
///
/// Since we never successfully open a database, this type is only used
/// as a pointer type in function signatures.
#[repr(C)]
pub struct Dbm {
    _opaque: u8,
}

// ---------------------------------------------------------------------------
// Store flags
// ---------------------------------------------------------------------------

/// Replace existing entry.
pub const DBM_INSERT: i32 = 0;
/// Insert only if key doesn't exist.
pub const DBM_REPLACE: i32 = 1;

// ---------------------------------------------------------------------------
// dbm_open
// ---------------------------------------------------------------------------

/// `dbm_open` — open a database.
///
/// Always fails since we have no DBM implementation.
/// Returns null and sets errno to ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_open(
    _file: *const u8,
    _open_flags: i32,
    _file_mode: u32,
) -> *mut Dbm {
    errno::set_errno(errno::ENOSYS);
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// dbm_close
// ---------------------------------------------------------------------------

/// `dbm_close` — close a database.
///
/// No-op since no database is ever opened.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_close(_db: *mut Dbm) {
    // Nothing to do.
}

// ---------------------------------------------------------------------------
// dbm_store
// ---------------------------------------------------------------------------

/// `dbm_store` — store a key/value pair.
///
/// Always returns -1 since no database is open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_store(
    _db: *mut Dbm,
    _key: Datum,
    _content: Datum,
    _store_mode: i32,
) -> i32 {
    errno::set_errno(errno::EINVAL);
    -1
}

// ---------------------------------------------------------------------------
// dbm_fetch
// ---------------------------------------------------------------------------

/// `dbm_fetch` — retrieve a value by key.
///
/// Always returns a null datum since no database is open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_fetch(_db: *mut Dbm, _key: Datum) -> Datum {
    NULL_DATUM
}

// ---------------------------------------------------------------------------
// dbm_delete
// ---------------------------------------------------------------------------

/// `dbm_delete` — delete a key/value pair.
///
/// Always returns -1 since no database is open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_delete(_db: *mut Dbm, _key: Datum) -> i32 {
    errno::set_errno(errno::EINVAL);
    -1
}

// ---------------------------------------------------------------------------
// dbm_firstkey / dbm_nextkey
// ---------------------------------------------------------------------------

/// `dbm_firstkey` — return the first key in the database.
///
/// Always returns a null datum since no database is open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_firstkey(_db: *mut Dbm) -> Datum {
    NULL_DATUM
}

/// `dbm_nextkey` — return the next key in the database.
///
/// Always returns a null datum since no database is open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_nextkey(_db: *mut Dbm) -> Datum {
    NULL_DATUM
}

// ---------------------------------------------------------------------------
// dbm_error / dbm_clearerr
// ---------------------------------------------------------------------------

/// `dbm_error` — check database error state.
///
/// Always returns 0 (no error), since no database operations occur.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_error(_db: *mut Dbm) -> i32 {
    0
}

/// `dbm_clearerr` — clear database error state.
///
/// No-op since there is no error state to clear.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_clearerr(_db: *mut Dbm) {
    // Nothing to do.
}

// ---------------------------------------------------------------------------
// dbm_dirfno / dbm_pagfno (non-standard but common extensions)
// ---------------------------------------------------------------------------

/// `dbm_dirfno` — return the file descriptor for the directory file.
///
/// Returns -1 since no database is open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_dirfno(_db: *mut Dbm) -> i32 {
    -1
}

/// `dbm_pagfno` — return the file descriptor for the page file.
///
/// Returns -1 since no database is open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_pagfno(_db: *mut Dbm) -> i32 {
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Datum
    // -----------------------------------------------------------------------

    #[test]
    fn test_datum_size() {
        // Datum is pointer + size — two pointer-sized fields.
        assert_eq!(
            core::mem::size_of::<Datum>(),
            2 * core::mem::size_of::<usize>()
        );
    }

    #[test]
    fn test_null_datum() {
        assert!(NULL_DATUM.dptr.is_null());
        assert_eq!(NULL_DATUM.dsize, 0);
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_store_flags() {
        assert_eq!(DBM_INSERT, 0);
        assert_eq!(DBM_REPLACE, 1);
    }

    // -----------------------------------------------------------------------
    // dbm_open
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_open_returns_null() {
        crate::errno::set_errno(0);
        let db = dbm_open(b"testdb\0".as_ptr(), 0, 0o644);
        assert!(db.is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_dbm_open_null_file() {
        let db = dbm_open(core::ptr::null(), 0, 0);
        assert!(db.is_null());
    }

    // -----------------------------------------------------------------------
    // dbm_close
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_close_null() {
        // Should not crash.
        dbm_close(core::ptr::null_mut());
    }

    // -----------------------------------------------------------------------
    // dbm_store
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_store_returns_error() {
        crate::errno::set_errno(0);
        let key = Datum {
            dptr: b"key\0".as_ptr() as *mut u8,
            dsize: 3,
        };
        let val = Datum {
            dptr: b"val\0".as_ptr() as *mut u8,
            dsize: 3,
        };
        let ret = dbm_store(core::ptr::null_mut(), key, val, DBM_INSERT);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_dbm_store_replace_mode() {
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        let val = Datum {
            dptr: b"v\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        let ret = dbm_store(core::ptr::null_mut(), key, val, DBM_REPLACE);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // dbm_fetch
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_fetch_returns_null_datum() {
        let key = Datum {
            dptr: b"key\0".as_ptr() as *mut u8,
            dsize: 3,
        };
        let result = dbm_fetch(core::ptr::null_mut(), key);
        assert!(result.dptr.is_null());
        assert_eq!(result.dsize, 0);
    }

    // -----------------------------------------------------------------------
    // dbm_delete
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_delete_returns_error() {
        crate::errno::set_errno(0);
        let key = Datum {
            dptr: b"key\0".as_ptr() as *mut u8,
            dsize: 3,
        };
        let ret = dbm_delete(core::ptr::null_mut(), key);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // dbm_firstkey / dbm_nextkey
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_firstkey_returns_null() {
        let result = dbm_firstkey(core::ptr::null_mut());
        assert!(result.dptr.is_null());
        assert_eq!(result.dsize, 0);
    }

    #[test]
    fn test_dbm_nextkey_returns_null() {
        let result = dbm_nextkey(core::ptr::null_mut());
        assert!(result.dptr.is_null());
        assert_eq!(result.dsize, 0);
    }

    // -----------------------------------------------------------------------
    // dbm_error / dbm_clearerr
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_error_returns_zero() {
        assert_eq!(dbm_error(core::ptr::null_mut()), 0);
    }

    #[test]
    fn test_dbm_clearerr_no_crash() {
        dbm_clearerr(core::ptr::null_mut());
    }

    // -----------------------------------------------------------------------
    // dbm_dirfno / dbm_pagfno
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_dirfno_returns_neg1() {
        assert_eq!(dbm_dirfno(core::ptr::null_mut()), -1);
    }

    #[test]
    fn test_dbm_pagfno_returns_neg1() {
        assert_eq!(dbm_pagfno(core::ptr::null_mut()), -1);
    }

    // -----------------------------------------------------------------------
    // Full workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_open_store_fetch_close_workflow() {
        // Typical usage pattern: open → store → fetch → close.
        let db = dbm_open(b"test\0".as_ptr(), 0o2 | 0o100, 0o644);
        assert!(db.is_null()); // open always fails

        let key = Datum {
            dptr: b"hello\0".as_ptr() as *mut u8,
            dsize: 5,
        };
        let val = Datum {
            dptr: b"world\0".as_ptr() as *mut u8,
            dsize: 5,
        };

        // Store fails.
        assert_eq!(dbm_store(db, key, val, DBM_INSERT), -1);
        // Fetch returns null.
        let fetched = dbm_fetch(db, key);
        assert!(fetched.dptr.is_null());
        // No error state (since nothing happened).
        assert_eq!(dbm_error(db), 0);
        // Close is a no-op.
        dbm_close(db);
    }

    #[test]
    fn test_iterate_keys_empty() {
        // Iterating over an empty/invalid database returns null immediately.
        let first = dbm_firstkey(core::ptr::null_mut());
        assert!(first.dptr.is_null());
        let next = dbm_nextkey(core::ptr::null_mut());
        assert!(next.dptr.is_null());
    }
}
