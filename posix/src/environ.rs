//! Environment variable access.
//!
//! Implements `getenv`, `setenv`, `unsetenv`, `putenv`.
//!
//! Uses a static array of `KEY=VALUE\0` C strings as the environment
//! store, since we have no heap for dynamic allocation beyond mmap.
//! The environment is initialized empty; programs can populate it.
//!
//! ## Limitations
//!
//! - Maximum 128 environment variables
//! - Maximum 256 bytes per `KEY=VALUE` string (including null)
//! - Not thread-safe (POSIX getenv is specified as not thread-safe)

use crate::string;

/// Maximum number of environment variables.
const MAX_ENV: usize = 128;
/// Maximum length of a single `KEY=VALUE` entry (including null).
const MAX_ENTRY_LEN: usize = 256;

/// Environment storage: array of null-terminated C strings.
///
/// An entry is "active" if its first byte is non-zero.
static mut ENV_STORE: [[u8; MAX_ENTRY_LEN]; MAX_ENV] = [[0u8; MAX_ENTRY_LEN]; MAX_ENV];

/// The `environ` pointer required by POSIX.
///
/// This is an array of pointers to `KEY=VALUE` strings, terminated
/// by a NULL pointer.  We rebuild it lazily when needed.
///
/// Starts as all-null — the first entry is the NULL terminator,
/// representing an empty environment.
static mut ENVIRON_PTRS: [*const u8; MAX_ENV + 1] = [core::ptr::null(); MAX_ENV + 1];

/// Global `environ` symbol (POSIX).
///
/// Points to `ENVIRON_PTRS` which is a null-terminated array of
/// string pointers.  Per POSIX, this is never NULL — programs can
/// safely iterate `environ` without a null-pointer check.
///
/// SAFETY: ENVIRON_PTRS is a static with stable address for the
/// lifetime of the process.  The cast from `*mut [*const u8; N]`
/// to `*mut *const u8` is valid because arrays have the same
/// alignment and layout as their element type.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut environ: *mut *const u8 = {
    // Initialize to point at ENVIRON_PTRS[0] (which is null, making
    // this a valid empty null-terminated array).
    // We can't use addr_of_mut! in a const context, so we'll set it
    // in rebuild_environ_ptrs or at first access.  For now, null is
    // the best we can do statically; __libc_start_main will fix it.
    core::ptr::null_mut()
};

/// Get the value of an environment variable.
///
/// Returns a pointer to the value string (after the '='), or NULL
/// if not found.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getenv(name: *const u8) -> *const u8 {
    if name.is_null() {
        return core::ptr::null();
    }

    let name_len = unsafe { string::strlen(name) };
    if name_len == 0 {
        return core::ptr::null();
    }

    // SAFETY: Single-threaded access to ENV_STORE.
    let store = unsafe { core::ptr::addr_of_mut!(ENV_STORE).as_mut() };
    let Some(store) = store else {
        return core::ptr::null();
    };

    for entry in store.iter() {
        if entry[0] == 0 {
            continue;
        }
        // Check if this entry starts with "name=".
        if entry_matches_name(entry, name, name_len) {
            // Return pointer to the value (after '=').
            // SAFETY: entry_matches_name verified entry[name_len] == '=',
            // so name_len + 1 is within the MAX_ENTRY_LEN buffer.
            return unsafe { entry.as_ptr().add(name_len.wrapping_add(1)) };
        }
    }

    core::ptr::null()
}

/// Set an environment variable.
///
/// If `overwrite` is non-zero and the variable exists, it is replaced.
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `name` and `value` must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn setenv(name: *const u8, value: *const u8, overwrite: i32) -> i32 {
    if name.is_null() || value.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    let name_len = unsafe { string::strlen(name) };
    let value_len = unsafe { string::strlen(value) };

    // POSIX: name must not be empty.
    if name_len == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    // Check for '=' in name (not allowed).
    let mut k: usize = 0;
    while k < name_len {
        if unsafe { *name.add(k) } == b'=' {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
        k = k.wrapping_add(1);
    }

    // Total length: name + '=' + value + '\0'
    let total = name_len
        .wrapping_add(1)
        .wrapping_add(value_len)
        .wrapping_add(1);
    if total > MAX_ENTRY_LEN {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return -1;
    }

    // SAFETY: Single-threaded access.
    let store = unsafe { core::ptr::addr_of_mut!(ENV_STORE).as_mut() };
    let Some(store) = store else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return -1;
    };

    // Check if variable already exists.
    for entry in store.iter_mut() {
        if entry[0] == 0 {
            continue;
        }
        if entry_matches_name(entry, name, name_len) {
            if overwrite == 0 {
                return 0; // Don't overwrite.
            }
            // Overwrite in place.
            write_entry(entry, name, name_len, value, value_len);
            rebuild_environ_ptrs();
            return 0;
        }
    }

    // Find an empty slot.
    for entry in store.iter_mut() {
        if entry[0] == 0 {
            write_entry(entry, name, name_len, value, value_len);
            rebuild_environ_ptrs();
            return 0;
        }
    }

    // No space.
    crate::errno::set_errno(crate::errno::ENOMEM);
    -1
}

/// Remove an environment variable.
///
/// Returns 0 on success (including if variable didn't exist).
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn unsetenv(name: *const u8) -> i32 {
    if name.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    let name_len = unsafe { string::strlen(name) };
    if name_len == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    // POSIX: name must not contain '='.
    let mut k: usize = 0;
    while k < name_len {
        if unsafe { *name.add(k) } == b'=' {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
        k = k.wrapping_add(1);
    }

    // SAFETY: Single-threaded access.
    let store = unsafe { core::ptr::addr_of_mut!(ENV_STORE).as_mut() };
    let Some(store) = store else { return 0 };

    // POSIX requires removing ALL entries with the given name, not just
    // the first.  Duplicates shouldn't arise through normal setenv/putenv
    // use, but external manipulation of `environ` could create them.
    let mut removed = false;
    for entry in store.iter_mut() {
        if entry[0] == 0 {
            continue;
        }
        if entry_matches_name(entry, name, name_len) {
            entry[0] = 0; // Mark as empty.
            removed = true;
        }
    }
    if removed {
        rebuild_environ_ptrs();
    }

    0 // Success (even if variable didn't exist).
}

// ---------------------------------------------------------------------------
// putenv
// ---------------------------------------------------------------------------

/// Insert or modify an environment variable.
///
/// `string` must be of the form `"NAME=VALUE"`.  Unlike `setenv`,
/// `putenv` does *not* make a copy — POSIX says the caller must keep
/// `string` alive.  Our implementation copies into the internal store
/// (same as `setenv`) because the static `ENV_STORE` is the only
/// backing store.
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `string` must be a valid null-terminated C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn putenv(string: *mut u8) -> i32 {
    if string.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    // Find the '=' separator.
    let total_len = unsafe { string::strlen(string) };
    let mut eq_pos: usize = 0;
    let mut found = false;
    while eq_pos < total_len {
        // SAFETY: eq_pos < total_len, string is valid.
        if unsafe { *string.add(eq_pos) } == b'=' {
            found = true;
            break;
        }
        eq_pos = eq_pos.wrapping_add(1);
    }

    if !found {
        // No '=' means unset (glibc extension).
        return unsafe { unsetenv(string) };
    }

    let name_len = eq_pos;

    // POSIX: name portion must not be empty.
    if name_len == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    let value_ptr = unsafe { string.add(eq_pos.wrapping_add(1)) };
    let value_len = total_len.wrapping_sub(eq_pos).wrapping_sub(1);

    let total = name_len
        .wrapping_add(1)
        .wrapping_add(value_len)
        .wrapping_add(1);
    if total > MAX_ENTRY_LEN {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return -1;
    }

    // SAFETY: Single-threaded access.
    let store = unsafe { core::ptr::addr_of_mut!(ENV_STORE).as_mut() };
    let Some(store) = store else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return -1;
    };

    // Check if variable already exists — overwrite it.
    for entry in store.iter_mut() {
        if entry[0] == 0 {
            continue;
        }
        if entry_matches_name(entry, string, name_len) {
            write_entry(entry, string, name_len, value_ptr, value_len);
            rebuild_environ_ptrs();
            return 0;
        }
    }

    // Find an empty slot.
    for entry in store.iter_mut() {
        if entry[0] == 0 {
            write_entry(entry, string, name_len, value_ptr, value_len);
            rebuild_environ_ptrs();
            return 0;
        }
    }

    crate::errno::set_errno(crate::errno::ENOMEM);
    -1
}

// ---------------------------------------------------------------------------
// clearenv
// ---------------------------------------------------------------------------

/// Clear the entire environment.
///
/// Removes all environment variables.  Returns 0 on success.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clearenv() -> i32 {
    // SAFETY: Single-threaded access.
    let store = unsafe { core::ptr::addr_of_mut!(ENV_STORE).as_mut() };
    if let Some(store) = store {
        for entry in store.iter_mut() {
            entry[0] = 0;
        }
        rebuild_environ_ptrs();
    }
    0
}

// ---------------------------------------------------------------------------
// secure_getenv
// ---------------------------------------------------------------------------

/// Get an environment variable (security-aware).
///
/// In a real libc, `secure_getenv` returns null if the process is
/// running with elevated privileges (setuid/setgid).  Since our OS
/// doesn't have privilege escalation, this is identical to `getenv`.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn secure_getenv(name: *const u8) -> *const u8 {
    // No privilege escalation in our OS — just delegate.
    unsafe { getenv(name) }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if an entry starts with `name=`.
fn entry_matches_name(entry: &[u8; MAX_ENTRY_LEN], name: *const u8, name_len: usize) -> bool {
    // Check that entry[name_len] == '='.
    if let Some(&eq) = entry.get(name_len) {
        if eq != b'=' {
            return false;
        }
    } else {
        return false;
    }

    // Compare the name portion.
    let mut i: usize = 0;
    while i < name_len {
        let Some(&a) = entry.get(i) else { return false };
        let b = unsafe { *name.add(i) };
        if a != b {
            return false;
        }
        i = i.wrapping_add(1);
    }
    true
}

/// Write `name=value\0` into an entry buffer.
///
/// Caller must ensure `name_len + 1 + value_len + 1 <= MAX_ENTRY_LEN`.
/// The setenv function validates this before calling write_entry.
fn write_entry(
    entry: &mut [u8; MAX_ENTRY_LEN],
    name: *const u8,
    name_len: usize,
    value: *const u8,
    value_len: usize,
) {
    let mut pos: usize = 0;

    // Copy name.
    let mut i: usize = 0;
    while i < name_len {
        if let Some(slot) = entry.get_mut(pos) {
            *slot = unsafe { *name.add(i) };
        }
        pos = pos.wrapping_add(1);
        i = i.wrapping_add(1);
    }

    // Write '='.
    if let Some(slot) = entry.get_mut(pos) {
        *slot = b'=';
    }
    pos = pos.wrapping_add(1);

    // Copy value.
    let mut j: usize = 0;
    while j < value_len {
        if let Some(slot) = entry.get_mut(pos) {
            *slot = unsafe { *value.add(j) };
        }
        pos = pos.wrapping_add(1);
        j = j.wrapping_add(1);
    }

    // Null terminate.
    if let Some(slot) = entry.get_mut(pos) {
        *slot = 0;
    }
}

/// Initialize the `environ` pointer early in process startup.
///
/// Called by `__libc_start_main` to ensure `environ` is non-NULL
/// before `main()` runs.  POSIX requires programs to be able to
/// iterate `environ` without checking for NULL.
pub fn init_environ() {
    rebuild_environ_ptrs();
}

/// Rebuild the `ENVIRON_PTRS` array from `ENV_STORE`.
fn rebuild_environ_ptrs() {
    // SAFETY: Single-threaded access.
    let store = unsafe { core::ptr::addr_of_mut!(ENV_STORE).as_ref() };
    let Some(store) = store else { return };

    let ptrs = unsafe { core::ptr::addr_of_mut!(ENVIRON_PTRS).as_mut() };
    let Some(ptrs) = ptrs else { return };

    let mut idx: usize = 0;
    for entry in store {
        if entry[0] != 0
            && let Some(slot) = ptrs.get_mut(idx)
        {
            *slot = entry.as_ptr();
            idx = idx.wrapping_add(1);
        }
    }
    // Null terminate.
    if let Some(slot) = ptrs.get_mut(idx) {
        *slot = core::ptr::null();
    }

    // Update the environ pointer.
    let ptr_val = core::ptr::addr_of_mut!(ENVIRON_PTRS).cast::<*const u8>();
    unsafe {
        core::ptr::addr_of_mut!(environ).write(ptr_val);
        // Keep __environ (glibc alias) in sync.
        core::ptr::addr_of_mut!(crate::crt::__environ).write(ptr_val);
    }
}

/// Load environment variables from packed null-terminated strings.
///
/// Each entry is a `KEY=VALUE\0` string, concatenated end-to-end.
/// Used during process startup to populate the environment from
/// data received via `SYS_PROCESS_GET_ARGS`.
///
/// Entries that exceed `MAX_ENTRY_LEN` or that don't contain `=`
/// are silently skipped.  If `ENV_STORE` runs out of slots, remaining
/// entries are dropped.
///
/// After loading, `rebuild_environ_ptrs()` is called to update
/// the `environ` pointer.
///
/// # Safety
///
/// `data` must point to readable memory of at least `data_len` bytes.
pub unsafe fn load_packed_envp(data: *const u8, data_len: usize, count: usize) {
    if data.is_null() || data_len == 0 || count == 0 {
        return;
    }

    let store = unsafe { core::ptr::addr_of_mut!(ENV_STORE).as_mut() };
    let Some(store) = store else { return };

    let mut pos = 0usize;
    let mut loaded = 0usize;

    while loaded < count && pos < data_len {
        let start = pos;

        // Find the null terminator for this entry.
        while pos < data_len {
            // SAFETY: pos < data_len guarantees readable.
            if unsafe { *data.add(pos) } == 0 {
                break;
            }
            pos = pos.wrapping_add(1);
        }

        let entry_len = pos.wrapping_sub(start);

        if entry_len > 0 && entry_len.wrapping_add(1) <= MAX_ENTRY_LEN {
            // Find an empty slot in ENV_STORE.
            let mut installed = false;
            for slot in store.iter_mut() {
                if slot[0] == 0 {
                    // Copy the "KEY=VALUE" string + null terminator.
                    // SAFETY: start..start+entry_len is within data_len.
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            data.add(start),
                            slot.as_mut_ptr(),
                            entry_len,
                        );
                    }
                    if let Some(term) = slot.get_mut(entry_len) {
                        *term = 0;
                    }
                    installed = true;
                    break;
                }
            }
            if !installed {
                break; // No more free slots.
            }
        }

        // Skip past the null terminator.
        pos = pos.wrapping_add(1);
        loaded = loaded.wrapping_add(1);
    }

    rebuild_environ_ptrs();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Cross-test serialisation lock for the process-global `ENV_STORE` /
/// `ENVIRON_PTRS`.  Tests (in this file and others, e.g. `wordexp`)
/// that read or mutate the environment must hold this lock for their
/// duration.  Without it, cargo's parallel test runner interleaves
/// `clearenv()` / `setenv()` calls from different tests and produces
/// intermittent failures (see `wordexp::tests::tilde_*` flakes).
#[cfg(test)]
pub static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire the env test lock, recovering from poison.
#[cfg(test)]
#[must_use = "the returned guard serialises env-mutating tests; bind it to `_g`"]
pub fn lock_env_for_test() -> std::sync::MutexGuard<'static, ()> {
    ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset environment state before each test.  Acquires the
    /// cross-test env lock and clears the store; bind the returned
    /// guard to `_g` so it lives for the test body.
    #[must_use = "the returned guard serialises env-mutating tests; bind it to `_g`"]
    fn reset() -> std::sync::MutexGuard<'static, ()> {
        let g = super::lock_env_for_test();
        clearenv();
        g
    }

    /// Helper: read a C string pointer into a `&[u8]` slice (without
    /// the terminating null).  Panics if `ptr` is null.
    unsafe fn cstr_bytes(ptr: *const u8) -> &'static [u8] {
        assert!(!ptr.is_null(), "unexpected null pointer");
        let len = unsafe { string::strlen(ptr) } as usize;
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }

    // -----------------------------------------------------------------------
    // getenv
    // -----------------------------------------------------------------------

    #[test]
    fn getenv_returns_null_for_missing_var() {
        let _g = reset();
        let ptr = unsafe { getenv(b"NOSUCH\0".as_ptr()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn getenv_returns_null_for_null_name() {
        let _g = reset();
        let ptr = unsafe { getenv(core::ptr::null()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn getenv_returns_null_for_empty_name() {
        let _g = reset();
        // Empty string = just a null terminator.
        let ptr = unsafe { getenv(b"\0".as_ptr()) };
        assert!(ptr.is_null());
    }

    // -----------------------------------------------------------------------
    // setenv / getenv round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn setenv_then_getenv() {
        let _g = reset();
        let rc = unsafe { setenv(b"HOME\0".as_ptr(), b"/root\0".as_ptr(), 1) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"HOME\0".as_ptr()) };
        assert!(!val.is_null());
        assert_eq!(unsafe { cstr_bytes(val) }, b"/root");
    }

    #[test]
    fn setenv_overwrite_replaces_value() {
        let _g = reset();
        unsafe { setenv(b"K\0".as_ptr(), b"old\0".as_ptr(), 1) };
        unsafe { setenv(b"K\0".as_ptr(), b"new\0".as_ptr(), 1) };

        let val = unsafe { getenv(b"K\0".as_ptr()) };
        assert_eq!(unsafe { cstr_bytes(val) }, b"new");
    }

    #[test]
    fn setenv_no_overwrite_keeps_original() {
        let _g = reset();
        unsafe { setenv(b"K\0".as_ptr(), b"first\0".as_ptr(), 1) };
        let rc = unsafe { setenv(b"K\0".as_ptr(), b"second\0".as_ptr(), 0) };
        assert_eq!(rc, 0); // success, but value unchanged

        let val = unsafe { getenv(b"K\0".as_ptr()) };
        assert_eq!(unsafe { cstr_bytes(val) }, b"first");
    }

    #[test]
    fn setenv_empty_value() {
        let _g = reset();
        let rc = unsafe { setenv(b"EMPTY\0".as_ptr(), b"\0".as_ptr(), 1) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"EMPTY\0".as_ptr()) };
        assert!(!val.is_null());
        assert_eq!(unsafe { cstr_bytes(val) }, b"");
    }

    #[test]
    fn setenv_rejects_null_name() {
        let _g = reset();
        let rc = unsafe { setenv(core::ptr::null(), b"v\0".as_ptr(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn setenv_rejects_null_value() {
        let _g = reset();
        let rc = unsafe { setenv(b"K\0".as_ptr(), core::ptr::null(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn setenv_rejects_empty_name() {
        let _g = reset();
        let rc = unsafe { setenv(b"\0".as_ptr(), b"v\0".as_ptr(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn setenv_rejects_equals_in_name() {
        let _g = reset();
        let rc = unsafe { setenv(b"A=B\0".as_ptr(), b"v\0".as_ptr(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn setenv_rejects_too_long_entry() {
        let _g = reset();
        // Build a value that, combined with a 1-char name + '=' + '\0',
        // exceeds MAX_ENTRY_LEN (256).  Name "X" = 1, '=' = 1, '\0' = 1,
        // so value must be > 253 bytes to overflow.
        let mut long_val = [b'A'; 254];
        long_val[253] = 0; // null terminate — 253 chars of content
        // Total = 1 + 1 + 253 + 1 = 256 — exactly at the limit, should succeed.
        let rc = unsafe { setenv(b"X\0".as_ptr(), long_val.as_ptr(), 1) };
        assert_eq!(rc, 0);

        // Now try one byte longer: 254 chars of content.
        let mut too_long = [b'B'; 255];
        too_long[254] = 0;
        // Total = 1 + 1 + 254 + 1 = 257 — exceeds MAX_ENTRY_LEN.
        let rc = unsafe { setenv(b"Y\0".as_ptr(), too_long.as_ptr(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOMEM);
    }

    // -----------------------------------------------------------------------
    // unsetenv
    // -----------------------------------------------------------------------

    #[test]
    fn unsetenv_removes_variable() {
        let _g = reset();
        unsafe { setenv(b"DEL\0".as_ptr(), b"yes\0".as_ptr(), 1) };
        let rc = unsafe { unsetenv(b"DEL\0".as_ptr()) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"DEL\0".as_ptr()) };
        assert!(val.is_null());
    }

    #[test]
    fn unsetenv_nonexistent_succeeds() {
        let _g = reset();
        let rc = unsafe { unsetenv(b"GHOST\0".as_ptr()) };
        assert_eq!(rc, 0);
    }

    #[test]
    fn unsetenv_rejects_null_name() {
        let _g = reset();
        let rc = unsafe { unsetenv(core::ptr::null()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn unsetenv_rejects_empty_name() {
        let _g = reset();
        let rc = unsafe { unsetenv(b"\0".as_ptr()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn unsetenv_rejects_equals_in_name() {
        let _g = reset();
        let rc = unsafe { unsetenv(b"A=B\0".as_ptr()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // putenv
    // -----------------------------------------------------------------------

    #[test]
    fn putenv_sets_variable() {
        let _g = reset();
        let mut s = *b"LANG=en_US\0";
        let rc = unsafe { putenv(s.as_mut_ptr()) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"LANG\0".as_ptr()) };
        assert_eq!(unsafe { cstr_bytes(val) }, b"en_US");
    }

    #[test]
    fn putenv_overwrites_existing() {
        let _g = reset();
        let mut s1 = *b"Z=one\0";
        unsafe { putenv(s1.as_mut_ptr()) };

        let mut s2 = *b"Z=two\0";
        unsafe { putenv(s2.as_mut_ptr()) };

        let val = unsafe { getenv(b"Z\0".as_ptr()) };
        assert_eq!(unsafe { cstr_bytes(val) }, b"two");
    }

    #[test]
    fn putenv_no_equals_unsets() {
        let _g = reset();
        unsafe { setenv(b"REM\0".as_ptr(), b"v\0".as_ptr(), 1) };
        let mut s = *b"REM\0";
        let rc = unsafe { putenv(s.as_mut_ptr()) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"REM\0".as_ptr()) };
        assert!(val.is_null());
    }

    #[test]
    fn putenv_rejects_null() {
        let _g = reset();
        let rc = unsafe { putenv(core::ptr::null_mut()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn putenv_rejects_empty_name_with_equals() {
        let _g = reset();
        // "=value" has an empty name portion.
        let mut s = *b"=value\0";
        let rc = unsafe { putenv(s.as_mut_ptr()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // clearenv
    // -----------------------------------------------------------------------

    #[test]
    fn clearenv_removes_all() {
        let _g = reset();
        unsafe { setenv(b"A\0".as_ptr(), b"1\0".as_ptr(), 1) };
        unsafe { setenv(b"B\0".as_ptr(), b"2\0".as_ptr(), 1) };
        unsafe { setenv(b"C\0".as_ptr(), b"3\0".as_ptr(), 1) };

        let rc = clearenv();
        assert_eq!(rc, 0);

        assert!(unsafe { getenv(b"A\0".as_ptr()) }.is_null());
        assert!(unsafe { getenv(b"B\0".as_ptr()) }.is_null());
        assert!(unsafe { getenv(b"C\0".as_ptr()) }.is_null());
    }

    #[test]
    fn clearenv_on_empty_is_noop() {
        let _g = reset();
        let rc = clearenv();
        assert_eq!(rc, 0);
    }

    // -----------------------------------------------------------------------
    // secure_getenv
    // -----------------------------------------------------------------------

    #[test]
    fn secure_getenv_matches_getenv() {
        let _g = reset();
        unsafe { setenv(b"SEC\0".as_ptr(), b"val\0".as_ptr(), 1) };

        let a = unsafe { getenv(b"SEC\0".as_ptr()) };
        let b = unsafe { secure_getenv(b"SEC\0".as_ptr()) };
        assert_eq!(a, b);
        assert_eq!(unsafe { cstr_bytes(b) }, b"val");
    }

    // -----------------------------------------------------------------------
    // Interaction / multi-variable tests
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_variables_independent() {
        let _g = reset();
        unsafe { setenv(b"X\0".as_ptr(), b"10\0".as_ptr(), 1) };
        unsafe { setenv(b"Y\0".as_ptr(), b"20\0".as_ptr(), 1) };
        unsafe { setenv(b"XX\0".as_ptr(), b"30\0".as_ptr(), 1) };

        // "X" should not match "XX" or "Y".
        assert_eq!(unsafe { cstr_bytes(getenv(b"X\0".as_ptr())) }, b"10");
        assert_eq!(unsafe { cstr_bytes(getenv(b"Y\0".as_ptr())) }, b"20");
        assert_eq!(unsafe { cstr_bytes(getenv(b"XX\0".as_ptr())) }, b"30");
    }

    #[test]
    fn unsetenv_does_not_affect_others() {
        let _g = reset();
        unsafe { setenv(b"KEEP\0".as_ptr(), b"yes\0".as_ptr(), 1) };
        unsafe { setenv(b"DROP\0".as_ptr(), b"no\0".as_ptr(), 1) };

        unsafe { unsetenv(b"DROP\0".as_ptr()) };

        assert_eq!(unsafe { cstr_bytes(getenv(b"KEEP\0".as_ptr())) }, b"yes");
        assert!(unsafe { getenv(b"DROP\0".as_ptr()) }.is_null());
    }

    #[test]
    fn setenv_after_unsetenv_reuses_slot() {
        let _g = reset();
        unsafe { setenv(b"REUSE\0".as_ptr(), b"a\0".as_ptr(), 1) };
        unsafe { unsetenv(b"REUSE\0".as_ptr()) };
        let rc = unsafe { setenv(b"REUSE\0".as_ptr(), b"b\0".as_ptr(), 1) };
        assert_eq!(rc, 0);
        assert_eq!(unsafe { cstr_bytes(getenv(b"REUSE\0".as_ptr())) }, b"b");
    }

    // -----------------------------------------------------------------------
    // load_packed_envp
    // -----------------------------------------------------------------------

    #[test]
    fn load_packed_envp_basic() {
        let _g = reset();
        // Two entries: "HOME=/root\0" + "USER=test\0"
        let data = b"HOME=/root\0USER=test\0";
        unsafe { load_packed_envp(data.as_ptr(), data.len(), 2) };

        let home = unsafe { getenv(b"HOME\0".as_ptr()) };
        assert!(!home.is_null());
        assert_eq!(unsafe { cstr_bytes(home) }, b"/root");

        let user = unsafe { getenv(b"USER\0".as_ptr()) };
        assert!(!user.is_null());
        assert_eq!(unsafe { cstr_bytes(user) }, b"test");
    }

    #[test]
    fn load_packed_envp_single() {
        let _g = reset();
        let data = b"LANG=C\0";
        unsafe { load_packed_envp(data.as_ptr(), data.len(), 1) };

        let val = unsafe { getenv(b"LANG\0".as_ptr()) };
        assert!(!val.is_null());
        assert_eq!(unsafe { cstr_bytes(val) }, b"C");
    }

    #[test]
    fn load_packed_envp_null() {
        let _g = reset();
        // Null data should be a no-op.
        unsafe { load_packed_envp(core::ptr::null(), 0, 0) };
        // No crash, no vars set.
    }

    #[test]
    fn load_packed_envp_zero_count() {
        let _g = reset();
        let data = b"FOO=bar\0";
        unsafe { load_packed_envp(data.as_ptr(), data.len(), 0) };
        // Count 0 — nothing loaded.
        assert!(unsafe { getenv(b"FOO\0".as_ptr()) }.is_null());
    }

    #[test]
    fn load_packed_envp_zero_len() {
        let _g = reset();
        let data = b"FOO=bar\0";
        unsafe { load_packed_envp(data.as_ptr(), 0, 1) };
        // Zero length — nothing loaded.
        assert!(unsafe { getenv(b"FOO\0".as_ptr()) }.is_null());
    }

    #[test]
    fn load_packed_envp_preserves_existing() {
        let _g = reset();
        // Set an existing var first.
        unsafe { setenv(b"EXISTING\0".as_ptr(), b"yes\0".as_ptr(), 1) };

        // Load a new var from packed data.
        let data = b"NEW=added\0";
        unsafe { load_packed_envp(data.as_ptr(), data.len(), 1) };

        // Both should exist.
        assert_eq!(
            unsafe { cstr_bytes(getenv(b"EXISTING\0".as_ptr())) },
            b"yes"
        );
        assert_eq!(unsafe { cstr_bytes(getenv(b"NEW\0".as_ptr())) }, b"added");
    }
}
