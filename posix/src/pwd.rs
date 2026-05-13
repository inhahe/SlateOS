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
// Reentrant password/group lookups
// ---------------------------------------------------------------------------

/// Look up a user by name (reentrant).
///
/// Copies the result into caller-provided `pwd` and `buf`.  On success,
/// `*result` is set to `pwd`; if the user is not found, `*result` is
/// NULL and 0 is returned.
///
/// Returns 0 on success, or an error code (`ERANGE` if `buflen` is
/// too small).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpwnam_r(
    name: *const u8,
    pwd: *mut Passwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Passwd,
) -> i32 {
    if result.is_null() || pwd.is_null() || buf.is_null() {
        return crate::errno::EINVAL;
    }

    // Default: not found.
    unsafe { *result = core::ptr::null(); }

    if name.is_null() {
        return 0;
    }

    let len = unsafe { crate::string::strlen(name) };
    if len == 4 && matches_root(name) {
        return fill_passwd_r(pwd, buf, buflen, result);
    }

    0
}

/// Look up a user by UID (reentrant).
///
/// Same semantics as `getpwnam_r`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpwuid_r(
    uid: UidT,
    pwd: *mut Passwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Passwd,
) -> i32 {
    if result.is_null() || pwd.is_null() || buf.is_null() {
        return crate::errno::EINVAL;
    }

    unsafe { *result = core::ptr::null(); }

    if uid == 0 {
        return fill_passwd_r(pwd, buf, buflen, result);
    }

    0
}

/// Look up a group by name (reentrant).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getgrnam_r(
    name: *const u8,
    grp: *mut Group,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Group,
) -> i32 {
    if result.is_null() || grp.is_null() || buf.is_null() {
        return crate::errno::EINVAL;
    }

    unsafe { *result = core::ptr::null(); }

    if name.is_null() {
        return 0;
    }

    let len = unsafe { crate::string::strlen(name) };
    if len == 4 && matches_root(name) {
        return fill_group_r(grp, buf, buflen, result);
    }

    0
}

/// Look up a group by GID (reentrant).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getgrgid_r(
    gid: GidT,
    grp: *mut Group,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Group,
) -> i32 {
    if result.is_null() || grp.is_null() || buf.is_null() {
        return crate::errno::EINVAL;
    }

    unsafe { *result = core::ptr::null(); }

    if gid == 0 {
        return fill_group_r(grp, buf, buflen, result);
    }

    0
}

/// Fill in a Passwd struct with root user data, copying strings into `buf`.
///
/// String layout in buf: "root\0x\0root\0/\0/bin/sh\0" = 24 bytes.
fn fill_passwd_r(
    pwd: *mut Passwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Passwd,
) -> i32 {
    // Strings: "root\0" (5) + "x\0" (2) + "root\0" (5) + "/\0" (2) + "/bin/sh\0" (8) = 22
    const NEEDED: usize = 22;
    if buflen < NEEDED {
        return crate::errno::ERANGE;
    }

    // Copy strings into buf.
    let strings: &[u8] = b"root\0x\0root\0/\0/bin/sh\0";
    let mut i: usize = 0;
    while i < NEEDED {
        // SAFETY: i < NEEDED <= buflen, buf is valid.  i < NEEDED = 22
        // which is less than the byte-string length.
        let byte = strings.get(i).copied().unwrap_or(0);
        unsafe { *buf.add(i) = byte; }
        i = i.wrapping_add(1);
    }

    // Fill the struct with pointers into buf.
    // SAFETY: pwd is non-null (checked by caller).
    unsafe {
        (*pwd).pw_name = buf;                     // "root" at offset 0
        (*pwd).pw_passwd = buf.add(5);            // "x" at offset 5
        (*pwd).pw_uid = 0;
        (*pwd).pw_gid = 0;
        (*pwd).pw_gecos = buf.add(7);             // "root" at offset 7
        (*pwd).pw_dir = buf.add(12);              // "/" at offset 12
        (*pwd).pw_shell = buf.add(14);            // "/bin/sh" at offset 14
        *result = pwd;
    }

    0
}

/// Fill in a Group struct with root group data, copying strings into `buf`.
///
/// String layout: "root\0x\0" + null pointer for gr_mem = 7 + 8 = 15 bytes.
fn fill_group_r(
    grp: *mut Group,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Group,
) -> i32 {
    // Strings: "root\0" (5) + "x\0" (2) = 7 bytes for strings.
    // Plus 8 bytes for a null pointer (gr_mem entry), aligned to 8 bytes.
    // We need strings + alignment padding + one null pointer.
    const STR_BYTES: usize = 7;
    // Align up to 8 for the pointer.
    const ALIGN: usize = 8;
    const PTR_START: usize = (STR_BYTES + ALIGN - 1) & !(ALIGN - 1); // = 8
    const NEEDED: usize = PTR_START + ALIGN; // 8 + 8 = 16

    if buflen < NEEDED {
        return crate::errno::ERANGE;
    }

    // Copy strings.
    let strings: &[u8] = b"root\0x\0";
    let mut i: usize = 0;
    while i < STR_BYTES {
        let byte = strings.get(i).copied().unwrap_or(0);
        unsafe { *buf.add(i) = byte; }
        i = i.wrapping_add(1);
    }

    // Write a null pointer for gr_mem (empty member list).
    // Zero the padding and pointer bytes.
    i = STR_BYTES;
    while i < NEEDED {
        unsafe { *buf.add(i) = 0; }
        i = i.wrapping_add(1);
    }

    unsafe {
        (*grp).gr_name = buf;                     // "root" at offset 0
        (*grp).gr_passwd = buf.add(5);            // "x" at offset 5
        (*grp).gr_gid = 0;
        // gr_mem points to the null pointer we wrote at PTR_START.
        // SAFETY: buf is from caller's buffer; at PTR_START it's 8-byte
        // aligned because PTR_START = 8 and buf came from the stack or
        // a caller-allocated buffer.  We wrote all-zeroes at that offset,
        // forming a valid null pointer for gr_mem's array.
        #[allow(clippy::cast_ptr_alignment)]
        { (*grp).gr_mem = buf.add(PTR_START).cast::<*const u8>(); }
        *result = grp;
    }

    0
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
