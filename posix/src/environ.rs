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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
    let Some(store) = store else { return core::ptr::null() };

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
#[unsafe(no_mangle)]
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
    let total = name_len.wrapping_add(1).wrapping_add(value_len).wrapping_add(1);
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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

    let total = name_len.wrapping_add(1).wrapping_add(value_len).wrapping_add(1);
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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

/// Rebuild the ENVIRON_PTRS array from ENV_STORE.
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
    unsafe {
        core::ptr::addr_of_mut!(environ).write(
            core::ptr::addr_of_mut!(ENVIRON_PTRS).cast::<*const u8>()
        );
    }
}
