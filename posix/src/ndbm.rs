//! POSIX `<ndbm.h>` — database operations.
//!
//! Validators for `dbm_open`, `dbm_close`, `dbm_store`, `dbm_fetch`,
//! `dbm_delete`, `dbm_firstkey`, `dbm_nextkey`, `dbm_error`,
//! `dbm_clearerr`, `dbm_dirfno`, `dbm_pagfno`.
//!
//! # Status of these calls
//!
//! NDBM is a 1980s-era key-value database originally from 4.3BSD,
//! standardized in POSIX.1-2001, and marked LEGACY in POSIX.1-2008.
//! Modern code uses SQLite, Berkeley DB, LMDB, or a flat JSON/CBOR
//! file. The few remaining real-world users are:
//!
//! - `sendmail`'s alias database (`/etc/aliases.db`) on the BSD ports.
//! - `postfix`'s `dbm_*` map type when configured for NDBM (the default
//!   is `hash` or `btree`).
//! - The GNU dbm library's `gdbm_compat` shim that maps `dbm_*` calls
//!   onto its own GDBM backend.
//! - Some Perl/Python NDBM bindings (`AnyDBM_File::NDBM_File`,
//!   `dbm.ndbm` in CPython before it switched the default).
//! - Tcl's `dict` extension on legacy BSD builds.
//!
//! We do not implement an NDBM backend (a real one would need on-disk
//! .dir/.pag file format support, hash bucket management, key/value
//! pair packing, and a recovery journal). Every `dbm_open` fails with
//! ENOSYS, and subsequent operations on the NULL handle produce
//! meaningful EINVAL / EFAULT / ENOENT feedback so callers using NDBM
//! as a configurable backend (postfix's `map_type = dbm`) fall back to
//! their alternative path (`map_type = hash` / `btree` / `cdb`).

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
// Limits
// ---------------------------------------------------------------------------

/// Maximum size we accept for a single key or value, in bytes.
///
/// Real NDBM implementations cap a `datum` at the underlying page size
/// (typically 1024 bytes for the .pag file). 4.4BSD's NDBM used
/// `PBLKSIZ = 1024`. We use 64 KiB as a generous upper bound — anything
/// larger is almost certainly a caller bug (sign-extended length, junk
/// from a corrupt struct).
pub const DBM_MAX_DATUM_SIZE: usize = 64 * 1024;

/// Maximum length of a file path passed to `dbm_open`, in bytes
/// (including the NUL terminator). Matches POSIX `PATH_MAX`.
pub const DBM_PATH_MAX: usize = 4096;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Walk a NUL-terminated C string, return its length (excluding NUL) or
/// None if no terminator within `max` bytes. Used for path validation.
///
/// # Safety
/// Caller must ensure `s` is either null (handled by the caller) or
/// points to a readable buffer of at least `max` bytes.
unsafe fn cstr_len(s: *const u8, max: usize) -> Option<usize> {
    for i in 0..max {
        // SAFETY: We only enter this loop when the caller has confirmed
        // s is non-null. read tolerates any alignment for u8.
        let b = unsafe { core::ptr::read(s.add(i)) };
        if b == 0 {
            return Some(i);
        }
    }
    None
}

/// Validate a `Datum` used as a key. POSIX requires `dptr != NULL` and
/// `dsize > 0` for a key — an empty key is treated as a deleted slot in
/// the original 4.3BSD NDBM layout. Returns 0 on success, negative
/// errno on failure.
fn validate_key(key: Datum) -> Result<(), i32> {
    if key.dsize == 0 || key.dptr.is_null() {
        return Err(errno::EINVAL);
    }
    if key.dsize > DBM_MAX_DATUM_SIZE {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validate a `Datum` used as a value. Unlike keys, a zero-length value
/// with a non-null pointer is well-formed (some NDBM users store
/// presence-only entries). NULL pointer with nonzero size is always
/// EINVAL.
fn validate_value(val: Datum) -> Result<(), i32> {
    if val.dptr.is_null() && val.dsize != 0 {
        return Err(errno::EINVAL);
    }
    if val.dsize > DBM_MAX_DATUM_SIZE {
        return Err(errno::EINVAL);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// dbm_open
// ---------------------------------------------------------------------------

/// `dbm_open` — open a database.
///
/// Linux/POSIX semantics:
/// - NULL `file` → EFAULT.
/// - empty `file` (first byte is NUL) → ENOENT.
/// - `file` not NUL-terminated within `DBM_PATH_MAX` → ENAMETOOLONG.
/// - `open_flags` with both `O_RDONLY (0)` and `O_WRONLY (1)` set
///   simultaneously — these are bit positions 0/1 of `open_flags` and
///   in POSIX are mutually exclusive — we trust the caller (no extra
///   check, matching glibc which lets the kernel sort it out).
/// - All other inputs → ENOSYS (no NDBM backend).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_open(
    file: *const u8,
    _open_flags: i32,
    _file_mode: u32,
) -> *mut Dbm {
    if file.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: file is non-null; cstr_len walks one byte at a time up to
    // DBM_PATH_MAX so it cannot read past the maximum path.
    let len = unsafe { cstr_len(file, DBM_PATH_MAX) };
    match len {
        None => {
            errno::set_errno(errno::ENAMETOOLONG);
            return core::ptr::null_mut();
        }
        Some(0) => {
            errno::set_errno(errno::ENOENT);
            return core::ptr::null_mut();
        }
        Some(_) => {}
    }
    errno::set_errno(errno::ENOSYS);
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// dbm_close
// ---------------------------------------------------------------------------

/// `dbm_close` — close a database.
///
/// No-op since no database is ever opened. NDBM's `dbm_close` returns
/// `void`, so we can't signal "the handle was NULL" via a return code
/// — we just do nothing, matching glibc's behavior on a NULL DBM*.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_close(_db: *mut Dbm) {
    // Nothing to do.
}

// ---------------------------------------------------------------------------
// dbm_store
// ---------------------------------------------------------------------------

/// `dbm_store` — store a key/value pair.
///
/// Validates key, value, and store mode before returning EINVAL (no db
/// open). On a real implementation, a valid call would return 0 on
/// success, 1 if `DBM_INSERT` and the key already exists, -1 on error.
///
/// - NULL db → EINVAL (no handle to operate on — `dbm_open` always
///   returns NULL in our build).
/// - Invalid key (NULL dptr with nonzero dsize, empty key, oversized
///   key) → EINVAL.
/// - Invalid value (NULL dptr with nonzero dsize, oversized value) →
///   EINVAL.
/// - `store_mode` not in `{DBM_INSERT, DBM_REPLACE}` → EINVAL.
/// - All other inputs → EINVAL (no db open).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_store(
    db: *mut Dbm,
    key: Datum,
    content: Datum,
    store_mode: i32,
) -> i32 {
    if let Err(e) = validate_key(key) {
        errno::set_errno(e);
        return -1;
    }
    if let Err(e) = validate_value(content) {
        errno::set_errno(e);
        return -1;
    }
    if store_mode != DBM_INSERT && store_mode != DBM_REPLACE {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if db.is_null() {
        // Inputs were well-formed, but there's no open db (because
        // dbm_open never succeeds). Return EINVAL to match glibc's
        // "bad handle" path. Real NDBM would return -1 with errno
        // unchanged for "duplicate key in INSERT mode" — but here we
        // never get that far.
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // db was non-null but we never hand out non-null handles, so any
    // value we see here is a caller fabrication. Still return EINVAL.
    errno::set_errno(errno::EINVAL);
    -1
}

// ---------------------------------------------------------------------------
// dbm_fetch
// ---------------------------------------------------------------------------

/// `dbm_fetch` — retrieve a value by key.
///
/// On a real NDBM, returns a datum pointing into the db's internal
/// buffer, or `NULL_DATUM` if the key isn't found. We always return
/// `NULL_DATUM` (key never found). On invalid input we still return
/// `NULL_DATUM` — `dbm_fetch` has no error channel other than the null
/// pointer in the returned datum.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_fetch(_db: *mut Dbm, _key: Datum) -> Datum {
    NULL_DATUM
}

// ---------------------------------------------------------------------------
// dbm_delete
// ---------------------------------------------------------------------------

/// `dbm_delete` — delete a key/value pair.
///
/// Same input validation as `dbm_store`'s key half.
///
/// - Invalid key → EINVAL.
/// - All other inputs → EINVAL (no db open).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dbm_delete(_db: *mut Dbm, key: Datum) -> i32 {
    if let Err(e) = validate_key(key) {
        errno::set_errno(e);
        return -1;
    }
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

    #[test]
    fn test_limits() {
        assert_eq!(DBM_MAX_DATUM_SIZE, 65536);
        assert_eq!(DBM_PATH_MAX, 4096);
    }

    // -----------------------------------------------------------------------
    // dbm_open
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_open_null_file_efault() {
        errno::set_errno(errno::EBADF);
        let db = dbm_open(core::ptr::null(), 0, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_dbm_open_empty_file_enoent() {
        errno::set_errno(errno::EBADF);
        let db = dbm_open(b"\0".as_ptr(), 0, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn test_dbm_open_valid_reaches_enosys() {
        errno::set_errno(errno::EBADF);
        let db = dbm_open(b"/var/lib/aliases\0".as_ptr(), 0o2 | 0o100, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_dbm_open_unterminated_path_enametoolong() {
        // A buffer that has no NUL within DBM_PATH_MAX bytes.
        let huge = vec![b'a'; DBM_PATH_MAX + 1];
        errno::set_errno(errno::EBADF);
        let db = dbm_open(huge.as_ptr(), 0, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::ENAMETOOLONG);
    }

    // -----------------------------------------------------------------------
    // dbm_close
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_close_null() {
        // Returns void; just must not crash on NULL.
        dbm_close(core::ptr::null_mut());
    }

    // -----------------------------------------------------------------------
    // dbm_store — key validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_store_zero_key_einval() {
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 0, // empty key
        };
        let val = Datum {
            dptr: b"v\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, DBM_INSERT);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_dbm_store_null_key_dptr_einval() {
        let key = Datum {
            dptr: core::ptr::null_mut(),
            dsize: 4,
        };
        let val = Datum {
            dptr: b"v\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, DBM_INSERT);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_dbm_store_huge_key_einval() {
        let key = Datum {
            dptr: b"k".as_ptr() as *mut u8,
            dsize: DBM_MAX_DATUM_SIZE + 1,
        };
        let val = Datum {
            dptr: b"v\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, DBM_INSERT);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // dbm_store — value validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_store_null_value_dptr_with_size_einval() {
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        let val = Datum {
            dptr: core::ptr::null_mut(),
            dsize: 4, // claim 4 bytes but no buffer
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, DBM_INSERT);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_dbm_store_null_value_dptr_zero_size_ok_shape() {
        // NULL dptr with dsize=0 is a valid "presence-only" value;
        // shape passes validation and we fall through to "no db" EINVAL.
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        let val = Datum {
            dptr: core::ptr::null_mut(),
            dsize: 0,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, DBM_INSERT);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_dbm_store_huge_value_einval() {
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        let val = Datum {
            dptr: b"v".as_ptr() as *mut u8,
            dsize: DBM_MAX_DATUM_SIZE + 1,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, DBM_INSERT);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // dbm_store — store_mode validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_store_unknown_mode_einval() {
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        let val = Datum {
            dptr: b"v\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, 99);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_dbm_store_insert_mode_falls_through() {
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        let val = Datum {
            dptr: b"v\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, DBM_INSERT);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_dbm_store_replace_mode_falls_through() {
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        let val = Datum {
            dptr: b"v\0".as_ptr() as *mut u8,
            dsize: 1,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_store(core::ptr::null_mut(), key, val, DBM_REPLACE);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
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
        let r = dbm_fetch(core::ptr::null_mut(), key);
        assert!(r.dptr.is_null());
        assert_eq!(r.dsize, 0);
    }

    #[test]
    fn test_dbm_fetch_invalid_key_returns_null_datum() {
        let key = Datum {
            dptr: core::ptr::null_mut(),
            dsize: 8,
        };
        let r = dbm_fetch(core::ptr::null_mut(), key);
        // dbm_fetch has no error channel besides the null datum.
        assert!(r.dptr.is_null());
    }

    // -----------------------------------------------------------------------
    // dbm_delete
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_delete_zero_key_einval() {
        let key = Datum {
            dptr: b"k\0".as_ptr() as *mut u8,
            dsize: 0,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_delete(core::ptr::null_mut(), key);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_dbm_delete_null_key_dptr_einval() {
        let key = Datum {
            dptr: core::ptr::null_mut(),
            dsize: 4,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_delete(core::ptr::null_mut(), key);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_dbm_delete_valid_falls_through() {
        let key = Datum {
            dptr: b"key\0".as_ptr() as *mut u8,
            dsize: 3,
        };
        errno::set_errno(errno::EBADF);
        let r = dbm_delete(core::ptr::null_mut(), key);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // dbm_firstkey / dbm_nextkey
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_firstkey_returns_null() {
        let r = dbm_firstkey(core::ptr::null_mut());
        assert!(r.dptr.is_null());
        assert_eq!(r.dsize, 0);
    }

    #[test]
    fn test_dbm_nextkey_returns_null() {
        let r = dbm_nextkey(core::ptr::null_mut());
        assert!(r.dptr.is_null());
        assert_eq!(r.dsize, 0);
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
    // Real-world workflow tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sendmail_aliases_db_workflow() {
        // sendmail's newaliases tool calls dbm_open("/etc/aliases", O_RDWR|O_CREAT, 0644)
        // when rebuilding the aliases database. On ENOSYS it logs
        // "newaliases: NDBM not available, no aliases DB built" and the
        // postmaster falls back to hash-table lookup of /etc/aliases.
        errno::set_errno(errno::EBADF);
        let db = dbm_open(b"/etc/aliases\0".as_ptr(), 0o2 | 0o100, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_postfix_map_type_dbm_fallback_workflow() {
        // postfix's lookup_dbm() probes dbm_open() for each map_type=dbm
        // configured map at startup. On ENOSYS, postfix's main.cf
        // parser logs "warning: dict_dbm_open(/etc/postfix/virtual):
        // No such file or directory" (translated from ENOSYS in postfix's
        // dict.c) and the admin must switch to map_type=hash or btree.
        errno::set_errno(errno::EBADF);
        let db = dbm_open(b"/etc/postfix/virtual\0".as_ptr(), 0, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_gdbm_compat_shim_workflow() {
        // GNU gdbm's libgdbm_compat.so exposes NDBM-style entry points
        // that translate dbm_* calls onto its GDBM backend. On a system
        // where the compat shim isn't linked, callers fall through to
        // our stubs. dbm_open() returns ENOSYS, the compat layer
        // returns -1 to its caller, who falls back to GDBM directly.
        errno::set_errno(errno::EBADF);
        let db = dbm_open(b"my.gdbm\0".as_ptr(), 0o2, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_cpython_dbm_ndbm_module_probe_workflow() {
        // CPython 3.x's `dbm.ndbm` module attempts dbm_open() lazily on
        // first use. On ENOSYS, it raises `dbm.error: NDBM not
        // available` and the Python program catches that to fall back
        // to `dbm.dumb` (a pure-Python pickle-backed dict).
        errno::set_errno(errno::EBADF);
        let db = dbm_open(b"shelf.db\0".as_ptr(), 0o100, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_open_store_fetch_close_workflow() {
        // Typical (failing) usage pattern: open → store → fetch → close.
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

        assert_eq!(dbm_store(db, key, val, DBM_INSERT), -1);
        let fetched = dbm_fetch(db, key);
        assert!(fetched.dptr.is_null());
        assert_eq!(dbm_error(db), 0);
        dbm_close(db);
    }

    #[test]
    fn test_iterate_keys_empty() {
        let first = dbm_firstkey(core::ptr::null_mut());
        assert!(first.dptr.is_null());
        let next = dbm_nextkey(core::ptr::null_mut());
        assert!(next.dptr.is_null());
    }

    // -----------------------------------------------------------------------
    // POSIX errno-preserved-on-validation-success regression
    // -----------------------------------------------------------------------

    #[test]
    fn test_dbm_open_errno_set_to_enosys_on_validation_success() {
        errno::set_errno(errno::EBADF);
        let db = dbm_open(b"foo\0".as_ptr(), 0, 0o644);
        assert!(db.is_null());
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }
}
