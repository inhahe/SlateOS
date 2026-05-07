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
static mut ENVIRON_PTRS: [*const u8; MAX_ENV + 1] = [core::ptr::null(); MAX_ENV + 1];

/// Global `environ` symbol (POSIX).
#[unsafe(no_mangle)]
pub static mut environ: *mut *const u8 = core::ptr::null_mut();

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

    // SAFETY: Single-threaded access.
    let store = unsafe { core::ptr::addr_of_mut!(ENV_STORE).as_mut() };
    let Some(store) = store else { return 0 };

    for entry in store.iter_mut() {
        if entry[0] == 0 {
            continue;
        }
        if entry_matches_name(entry, name, name_len) {
            entry[0] = 0; // Mark as empty.
            rebuild_environ_ptrs();
            return 0;
        }
    }

    0 // Variable didn't exist — not an error.
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
