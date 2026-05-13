//! POSIX user/group database lookups.
//!
//! Implements `getpwnam`, `getpwuid`, `getgrnam`, `getgrgid` and related
//! functions.
//!
//! ## Implementation
//!
//! Our OS doesn't have a real user database.  All lookups return a
//! single "root" user with UID/GID 0 and home directory "/".  This
//! satisfies programs that do mandatory user lookups (bash, Python,
//! coreutils `id`/`whoami`).
//!
//! ## Limitations
//!
//! - Only one user ("root", UID 0) and one group ("root", GID 0).
//! - `getpwent`/`setpwent`/`endpwent` enumeration works but only
//!   yields the single root entry.
//! - Not thread-safe (uses static storage, matching POSIX spec).

use crate::types::*;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Password database entry (struct passwd).
#[repr(C)]
pub struct Passwd {
    /// Username.
    pub pw_name: *const u8,
    /// Encrypted password (not used).
    pub pw_passwd: *const u8,
    /// User ID.
    pub pw_uid: UidT,
    /// Group ID.
    pub pw_gid: GidT,
    /// Real name / comment (GECOS field).
    pub pw_gecos: *const u8,
    /// Home directory.
    pub pw_dir: *const u8,
    /// Login shell.
    pub pw_shell: *const u8,
}

/// Group database entry (struct group).
#[repr(C)]
pub struct Group {
    /// Group name.
    pub gr_name: *const u8,
    /// Encrypted group password (not used).
    pub gr_passwd: *const u8,
    /// Group ID.
    pub gr_gid: GidT,
    /// Null-terminated array of member names.
    pub gr_mem: *const *const u8,
}

// SAFETY: Passwd and Group contain raw pointers to static byte strings.
// The data they point to is 'static and immutable, so sharing across
// threads is safe.
unsafe impl Sync for Passwd {}
unsafe impl Sync for Group {}

// ---------------------------------------------------------------------------
// Static entries
// ---------------------------------------------------------------------------

/// The single "root" user entry.
static ROOT_PASSWD: Passwd = Passwd {
    pw_name: c"root".as_ptr().cast::<u8>(),
    pw_passwd: c"x".as_ptr().cast::<u8>(),
    pw_uid: 0,
    pw_gid: 0,
    pw_gecos: c"root".as_ptr().cast::<u8>(),
    pw_dir: c"/".as_ptr().cast::<u8>(),
    pw_shell: c"/bin/sh".as_ptr().cast::<u8>(),
};

/// Empty null-terminated member list for `gr_mem`.
///
/// POSIX requires `gr_mem` to point to a null-terminated array of
/// member name pointers.  A null pointer for the array itself would
/// crash programs that iterate `grp->gr_mem[i]` without null-checking.
/// Wrapper is needed because `*const u8` is not `Sync`.
#[repr(transparent)]
struct SyncMemberList([*const u8; 1]);

// SAFETY: Contains only a null pointer — immutable, no data to race on.
unsafe impl Sync for SyncMemberList {}

static EMPTY_MEM: SyncMemberList = SyncMemberList([core::ptr::null()]);

/// The single "root" group entry.
static ROOT_GROUP: Group = Group {
    gr_name: c"root".as_ptr().cast::<u8>(),
    gr_passwd: c"x".as_ptr().cast::<u8>(),
    gr_gid: 0,
    gr_mem: EMPTY_MEM.0.as_ptr(),
};

/// Enumeration position for getpwent/getgrent.
static mut PW_POS: i32 = 0;
static mut GR_POS: i32 = 0;

// ---------------------------------------------------------------------------
// Password database lookups
// ---------------------------------------------------------------------------

/// Look up a user by name.
///
/// Returns a pointer to a static Passwd struct, or null if not found.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpwnam(name: *const u8) -> *const Passwd {
    if name.is_null() {
        return core::ptr::null();
    }

    // Check if the name matches "root".
    let len = unsafe { crate::string::strlen(name) };
    if len == 4 && matches_root(name) {
        return &raw const ROOT_PASSWD;
    }

    core::ptr::null()
}

/// Look up a user by UID.
///
/// Returns a pointer to a static Passwd struct, or null if not found.
#[unsafe(no_mangle)]
pub extern "C" fn getpwuid(uid: UidT) -> *const Passwd {
    if uid == 0 {
        return &raw const ROOT_PASSWD;
    }
    core::ptr::null()
}

/// Rewind the password database to the beginning.
#[unsafe(no_mangle)]
pub extern "C" fn setpwent() {
    // SAFETY: Single-threaded access.
    unsafe { core::ptr::addr_of_mut!(PW_POS).write(0); }
}

/// Read the next entry from the password database.
///
/// Returns null when all entries have been read.
#[unsafe(no_mangle)]
pub extern "C" fn getpwent() -> *const Passwd {
    let pos = unsafe { *core::ptr::addr_of!(PW_POS) };
    if pos == 0 {
        unsafe { core::ptr::addr_of_mut!(PW_POS).write(1); }
        return &raw const ROOT_PASSWD;
    }
    core::ptr::null()
}

/// Close the password database.
#[unsafe(no_mangle)]
pub extern "C" fn endpwent() {
    unsafe { core::ptr::addr_of_mut!(PW_POS).write(0); }
}

// ---------------------------------------------------------------------------
// Group database lookups
// ---------------------------------------------------------------------------

/// Look up a group by name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getgrnam(name: *const u8) -> *const Group {
    if name.is_null() {
        return core::ptr::null();
    }

    let len = unsafe { crate::string::strlen(name) };
    if len == 4 && matches_root(name) {
        return &raw const ROOT_GROUP;
    }

    core::ptr::null()
}

/// Look up a group by GID.
#[unsafe(no_mangle)]
pub extern "C" fn getgrgid(gid: GidT) -> *const Group {
    if gid == 0 {
        return &raw const ROOT_GROUP;
    }
    core::ptr::null()
}

/// Rewind the group database.
#[unsafe(no_mangle)]
pub extern "C" fn setgrent() {
    unsafe { core::ptr::addr_of_mut!(GR_POS).write(0); }
}

/// Read the next entry from the group database.
#[unsafe(no_mangle)]
pub extern "C" fn getgrent() -> *const Group {
    let pos = unsafe { *core::ptr::addr_of!(GR_POS) };
    if pos == 0 {
        unsafe { core::ptr::addr_of_mut!(GR_POS).write(1); }
        return &raw const ROOT_GROUP;
    }
    core::ptr::null()
}

/// Close the group database.
#[unsafe(no_mangle)]
pub extern "C" fn endgrent() {
    unsafe { core::ptr::addr_of_mut!(GR_POS).write(0); }
}

// ---------------------------------------------------------------------------
// Login name
// ---------------------------------------------------------------------------

/// Get the login name.
///
/// Returns "root" (our only user).
#[unsafe(no_mangle)]
pub extern "C" fn getlogin() -> *const u8 {
    c"root".as_ptr().cast::<u8>()
}

/// Get the login name into a buffer.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn getlogin_r(buf: *mut u8, bufsize: usize) -> i32 {
    if buf.is_null() || bufsize < 5 {
        crate::errno::set_errno(crate::errno::ERANGE);
        return -1;
    }

    // Write "root\0".
    let name = b"root\0";
    let mut i: usize = 0;
    while i < 5 {
        if let Some(&b) = name.get(i) {
            // SAFETY: i < 5 <= bufsize.
            unsafe { *buf.add(i) = b; }
        }
        i = i.wrapping_add(1);
    }

    0
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a 4-byte string is "root".
fn matches_root(s: *const u8) -> bool {
    unsafe {
        *s == b'r' && *s.add(1) == b'o' && *s.add(2) == b'o' && *s.add(3) == b't'
    }
}
