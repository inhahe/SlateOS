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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpwuid(uid: UidT) -> *const Passwd {
    if uid == 0 {
        return &raw const ROOT_PASSWD;
    }
    core::ptr::null()
}

/// Rewind the password database to the beginning.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setpwent() {
    // SAFETY: Single-threaded access.
    unsafe { core::ptr::addr_of_mut!(PW_POS).write(0); }
}

/// Read the next entry from the password database.
///
/// Returns null when all entries have been read.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpwent() -> *const Passwd {
    let pos = unsafe { *core::ptr::addr_of!(PW_POS) };
    if pos == 0 {
        unsafe { core::ptr::addr_of_mut!(PW_POS).write(1); }
        return &raw const ROOT_PASSWD;
    }
    core::ptr::null()
}

/// Close the password database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endpwent() {
    unsafe { core::ptr::addr_of_mut!(PW_POS).write(0); }
}

// ---------------------------------------------------------------------------
// Group database lookups
// ---------------------------------------------------------------------------

/// Look up a group by name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgrgid(gid: GidT) -> *const Group {
    if gid == 0 {
        return &raw const ROOT_GROUP;
    }
    core::ptr::null()
}

/// Rewind the group database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setgrent() {
    unsafe { core::ptr::addr_of_mut!(GR_POS).write(0); }
}

/// Read the next entry from the group database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgrent() -> *const Group {
    let pos = unsafe { *core::ptr::addr_of!(GR_POS) };
    if pos == 0 {
        unsafe { core::ptr::addr_of_mut!(GR_POS).write(1); }
        return &raw const ROOT_GROUP;
    }
    core::ptr::null()
}

/// Close the group database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endgrent() {
    unsafe { core::ptr::addr_of_mut!(GR_POS).write(0); }
}

// ---------------------------------------------------------------------------
// getgrouplist / initgroups
// ---------------------------------------------------------------------------

/// Get list of groups to which a user belongs.
///
/// Fills `groups` with up to `*ngroups` GIDs.  Always includes `group`
/// (the caller-provided primary group).
///
/// Our stub OS only knows about GID 0 (root).  If the user is "root",
/// we return GID 0.  Otherwise we just return the supplied primary group.
///
/// Returns the total number of groups on success.  If `*ngroups` is too
/// small, `*ngroups` is set to the required count and -1 is returned.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgrouplist(
    _user: *const u8,
    group: GidT,
    groups: *mut GidT,
    ngroups: *mut i32,
) -> i32 {
    if ngroups.is_null() {
        return -1;
    }

    let max = unsafe { *ngroups };
    // We always have exactly 1 group (the primary).
    let needed = 1;

    if max < needed {
        unsafe { *ngroups = needed; }
        return -1;
    }

    if !groups.is_null() {
        unsafe { *groups = group; }
    }
    unsafe { *ngroups = needed; }
    needed
}

/// Initialize the supplementary group access list.
///
/// Sets the supplementary group IDs for the calling process to the
/// groups that `user` belongs to, plus `group`.
///
/// Stub: always succeeds (single-user OS, groups not enforced).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn initgroups(_user: *const u8, _group: GidT) -> i32 {
    0
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpwnam_r(
    name: *const u8,
    pwd: *mut Passwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Passwd,
) -> i32 {
    if result.is_null() || pwd.is_null() || buf.is_null() {
        return crate::errno::EFAULT;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpwuid_r(
    uid: UidT,
    pwd: *mut Passwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Passwd,
) -> i32 {
    if result.is_null() || pwd.is_null() || buf.is_null() {
        return crate::errno::EFAULT;
    }

    unsafe { *result = core::ptr::null(); }

    if uid == 0 {
        return fill_passwd_r(pwd, buf, buflen, result);
    }

    0
}

/// Look up a group by name (reentrant).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getgrnam_r(
    name: *const u8,
    grp: *mut Group,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Group,
) -> i32 {
    if result.is_null() || grp.is_null() || buf.is_null() {
        return crate::errno::EFAULT;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getgrgid_r(
    gid: GidT,
    grp: *mut Group,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Group,
) -> i32 {
    if result.is_null() || grp.is_null() || buf.is_null() {
        return crate::errno::EFAULT;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getlogin() -> *const u8 {
    c"root".as_ptr().cast::<u8>()
}

/// Get the login name into a buffer.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

// ---------------------------------------------------------------------------
// Unit tests (run with `cargo test -p posix -- --test-threads=1`)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errno;

    // Helper: reset global enumeration state between tests.
    fn reset_state() {
        unsafe {
            core::ptr::addr_of_mut!(PW_POS).write(0);
            core::ptr::addr_of_mut!(GR_POS).write(0);
        }
    }

    // -----------------------------------------------------------------------
    // getpwnam
    // -----------------------------------------------------------------------

    #[test]
    fn getpwnam_root_found() {
        reset_state();
        let pw = unsafe { getpwnam(b"root\0".as_ptr()) };
        assert!(!pw.is_null());
        let pw = unsafe { &*pw };
        assert_eq!(pw.pw_uid, 0);
        assert_eq!(pw.pw_gid, 0);
        // Verify the name is "root".
        let name = unsafe { core::ffi::CStr::from_ptr(pw.pw_name.cast()) };
        assert_eq!(name.to_bytes(), b"root");
    }

    #[test]
    fn getpwnam_nonroot_returns_null() {
        reset_state();
        let pw = unsafe { getpwnam(b"nobody\0".as_ptr()) };
        assert!(pw.is_null());
    }

    #[test]
    fn getpwnam_null_name_returns_null() {
        reset_state();
        let pw = unsafe { getpwnam(core::ptr::null()) };
        assert!(pw.is_null());
    }

    // -----------------------------------------------------------------------
    // getpwuid
    // -----------------------------------------------------------------------

    #[test]
    fn getpwuid_zero_returns_root() {
        reset_state();
        let pw = getpwuid(0);
        assert!(!pw.is_null());
        let pw = unsafe { &*pw };
        assert_eq!(pw.pw_uid, 0);
        assert_eq!(pw.pw_gid, 0);
    }

    #[test]
    fn getpwuid_nonzero_returns_null() {
        reset_state();
        assert!(getpwuid(1).is_null());
        assert!(getpwuid(1000).is_null());
        assert!(getpwuid(u32::MAX).is_null());
    }

    // -----------------------------------------------------------------------
    // getpwent / setpwent / endpwent enumeration
    // -----------------------------------------------------------------------

    #[test]
    fn pwent_enumeration() {
        reset_state();
        setpwent();

        // First call returns root.
        let pw = getpwent();
        assert!(!pw.is_null());
        let pw = unsafe { &*pw };
        assert_eq!(pw.pw_uid, 0);

        // Second call returns null (only one entry).
        assert!(getpwent().is_null());

        // setpwent resets enumeration.
        setpwent();
        let pw2 = getpwent();
        assert!(!pw2.is_null());
        let pw2 = unsafe { &*pw2 };
        assert_eq!(pw2.pw_uid, 0);

        // endpwent also resets.
        endpwent();
        let pw3 = getpwent();
        assert!(!pw3.is_null());
    }

    // -----------------------------------------------------------------------
    // getgrnam
    // -----------------------------------------------------------------------

    #[test]
    fn getgrnam_root_found() {
        reset_state();
        let gr = unsafe { getgrnam(b"root\0".as_ptr()) };
        assert!(!gr.is_null());
        let gr = unsafe { &*gr };
        assert_eq!(gr.gr_gid, 0);
        let name = unsafe { core::ffi::CStr::from_ptr(gr.gr_name.cast()) };
        assert_eq!(name.to_bytes(), b"root");
    }

    #[test]
    fn getgrnam_nonroot_returns_null() {
        reset_state();
        let gr = unsafe { getgrnam(b"wheel\0".as_ptr()) };
        assert!(gr.is_null());
    }

    #[test]
    fn getgrnam_null_name_returns_null() {
        reset_state();
        let gr = unsafe { getgrnam(core::ptr::null()) };
        assert!(gr.is_null());
    }

    // -----------------------------------------------------------------------
    // getgrgid
    // -----------------------------------------------------------------------

    #[test]
    fn getgrgid_zero_returns_root() {
        reset_state();
        let gr = getgrgid(0);
        assert!(!gr.is_null());
        let gr = unsafe { &*gr };
        assert_eq!(gr.gr_gid, 0);
    }

    #[test]
    fn getgrgid_nonzero_returns_null() {
        reset_state();
        assert!(getgrgid(1).is_null());
        assert!(getgrgid(100).is_null());
        assert!(getgrgid(u32::MAX).is_null());
    }

    // -----------------------------------------------------------------------
    // getgrent / setgrent / endgrent enumeration
    // -----------------------------------------------------------------------

    #[test]
    fn grent_enumeration() {
        reset_state();
        setgrent();

        // First call returns root group.
        let gr = getgrent();
        assert!(!gr.is_null());
        let gr = unsafe { &*gr };
        assert_eq!(gr.gr_gid, 0);

        // Second call returns null.
        assert!(getgrent().is_null());

        // setgrent resets.
        setgrent();
        let gr2 = getgrent();
        assert!(!gr2.is_null());

        // endgrent also resets.
        endgrent();
        let gr3 = getgrent();
        assert!(!gr3.is_null());
    }

    // -----------------------------------------------------------------------
    // getpwnam_r
    // -----------------------------------------------------------------------

    #[test]
    fn getpwnam_r_root_found() {
        reset_state();
        let mut pwd: Passwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 128];
        let mut result: *const Passwd = core::ptr::null();

        let ret = unsafe {
            getpwnam_r(
                b"root\0".as_ptr(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };

        assert_eq!(ret, 0);
        assert!(!result.is_null());
        assert_eq!(pwd.pw_uid, 0);
        assert_eq!(pwd.pw_gid, 0);

        // Verify strings were copied into buf.
        let name = unsafe { core::ffi::CStr::from_ptr(pwd.pw_name.cast()) };
        assert_eq!(name.to_bytes(), b"root");
        let shell = unsafe { core::ffi::CStr::from_ptr(pwd.pw_shell.cast()) };
        assert_eq!(shell.to_bytes(), b"/bin/sh");
        let dir = unsafe { core::ffi::CStr::from_ptr(pwd.pw_dir.cast()) };
        assert_eq!(dir.to_bytes(), b"/");
        let gecos = unsafe { core::ffi::CStr::from_ptr(pwd.pw_gecos.cast()) };
        assert_eq!(gecos.to_bytes(), b"root");
    }

    #[test]
    fn getpwnam_r_nonroot_result_null() {
        reset_state();
        let mut pwd: Passwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 128];
        let mut result: *const Passwd = core::ptr::null();

        let ret = unsafe {
            getpwnam_r(
                b"nobody\0".as_ptr(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };

        assert_eq!(ret, 0);
        assert!(result.is_null());
    }

    #[test]
    fn getpwnam_r_buffer_too_small() {
        reset_state();
        let mut pwd: Passwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 4]; // Way too small (need 22 bytes).
        let mut result: *const Passwd = core::ptr::null();

        let ret = unsafe {
            getpwnam_r(
                b"root\0".as_ptr(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };

        assert_eq!(ret, errno::ERANGE);
        assert!(result.is_null());
    }

    #[test]
    fn getpwnam_r_null_args_efault() {
        reset_state();
        let mut pwd: Passwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 128];
        let mut result: *const Passwd = core::ptr::null();

        // Null result pointer.
        let ret = unsafe {
            getpwnam_r(
                b"root\0".as_ptr(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(ret, errno::EFAULT);

        // Null pwd pointer.
        let ret = unsafe {
            getpwnam_r(
                b"root\0".as_ptr(),
                core::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(ret, errno::EFAULT);

        // Null buf pointer.
        let ret = unsafe {
            getpwnam_r(
                b"root\0".as_ptr(),
                &mut pwd,
                core::ptr::null_mut(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn getpwnam_r_null_name_not_found() {
        reset_state();
        let mut pwd: Passwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 128];
        let mut result: *const Passwd = core::ptr::null();

        let ret = unsafe {
            getpwnam_r(
                core::ptr::null(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };

        // Null name is not an error, just "not found".
        assert_eq!(ret, 0);
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // getpwuid_r
    // -----------------------------------------------------------------------

    #[test]
    fn getpwuid_r_zero_found() {
        reset_state();
        let mut pwd: Passwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 128];
        let mut result: *const Passwd = core::ptr::null();

        let ret = unsafe {
            getpwuid_r(0, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result)
        };

        assert_eq!(ret, 0);
        assert!(!result.is_null());
        assert_eq!(pwd.pw_uid, 0);
        assert_eq!(pwd.pw_gid, 0);
    }

    #[test]
    fn getpwuid_r_nonzero_result_null() {
        reset_state();
        let mut pwd: Passwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 128];
        let mut result: *const Passwd = core::ptr::null();

        let ret = unsafe {
            getpwuid_r(999, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result)
        };

        assert_eq!(ret, 0);
        assert!(result.is_null());
    }

    #[test]
    fn getpwuid_r_buffer_too_small() {
        reset_state();
        let mut pwd: Passwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 2];
        let mut result: *const Passwd = core::ptr::null();

        let ret = unsafe {
            getpwuid_r(0, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result)
        };

        assert_eq!(ret, errno::ERANGE);
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // getgrnam_r
    // -----------------------------------------------------------------------

    #[test]
    fn getgrnam_r_root_found() {
        reset_state();
        let mut grp: Group = unsafe { core::mem::zeroed() };
        // Need 16 bytes (7 string bytes + 1 padding + 8 null pointer).
        let mut buf = [0u8; 128];
        let mut result: *const Group = core::ptr::null();

        let ret = unsafe {
            getgrnam_r(
                b"root\0".as_ptr(),
                &mut grp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };

        assert_eq!(ret, 0);
        assert!(!result.is_null());
        assert_eq!(grp.gr_gid, 0);
        let name = unsafe { core::ffi::CStr::from_ptr(grp.gr_name.cast()) };
        assert_eq!(name.to_bytes(), b"root");
    }

    #[test]
    fn getgrnam_r_buffer_too_small() {
        reset_state();
        let mut grp: Group = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 4]; // Need 16 bytes.
        let mut result: *const Group = core::ptr::null();

        let ret = unsafe {
            getgrnam_r(
                b"root\0".as_ptr(),
                &mut grp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };

        assert_eq!(ret, errno::ERANGE);
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // getgrgid_r
    // -----------------------------------------------------------------------

    #[test]
    fn getgrgid_r_zero_found() {
        reset_state();
        let mut grp: Group = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 128];
        let mut result: *const Group = core::ptr::null();

        let ret = unsafe {
            getgrgid_r(0, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result)
        };

        assert_eq!(ret, 0);
        assert!(!result.is_null());
        assert_eq!(grp.gr_gid, 0);
    }

    #[test]
    fn getgrgid_r_buffer_too_small() {
        reset_state();
        let mut grp: Group = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 2];
        let mut result: *const Group = core::ptr::null();

        let ret = unsafe {
            getgrgid_r(0, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result)
        };

        assert_eq!(ret, errno::ERANGE);
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // getlogin / getlogin_r
    // -----------------------------------------------------------------------

    #[test]
    fn getlogin_returns_root() {
        let login = getlogin();
        assert!(!login.is_null());
        let name = unsafe { core::ffi::CStr::from_ptr(login.cast()) };
        assert_eq!(name.to_bytes(), b"root");
    }

    #[test]
    fn getlogin_r_writes_root() {
        let mut buf = [0u8; 32];
        let ret = getlogin_r(buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, 0);
        // Verify "root\0" was written.
        assert_eq!(&buf[..5], b"root\0");
    }

    #[test]
    fn getlogin_r_buffer_too_small() {
        let mut buf = [0u8; 3]; // Need 5 bytes for "root\0".
        let ret = getlogin_r(buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
    }

    #[test]
    fn getlogin_r_null_buf_returns_error() {
        let ret = getlogin_r(core::ptr::null_mut(), 100);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // ROOT_PASSWD static
    // -----------------------------------------------------------------------

    #[test]
    fn root_passwd_static_fields() {
        let pw = &ROOT_PASSWD;
        assert_eq!(pw.pw_uid, 0);
        assert_eq!(pw.pw_gid, 0);

        let name = unsafe { core::ffi::CStr::from_ptr(pw.pw_name.cast()) };
        assert_eq!(name.to_bytes(), b"root");

        let passwd = unsafe { core::ffi::CStr::from_ptr(pw.pw_passwd.cast()) };
        assert_eq!(passwd.to_bytes(), b"x");

        let gecos = unsafe { core::ffi::CStr::from_ptr(pw.pw_gecos.cast()) };
        assert_eq!(gecos.to_bytes(), b"root");

        let dir = unsafe { core::ffi::CStr::from_ptr(pw.pw_dir.cast()) };
        assert_eq!(dir.to_bytes(), b"/");

        let shell = unsafe { core::ffi::CStr::from_ptr(pw.pw_shell.cast()) };
        assert_eq!(shell.to_bytes(), b"/bin/sh");
    }

    // -----------------------------------------------------------------------
    // ROOT_GROUP static
    // -----------------------------------------------------------------------

    #[test]
    fn root_group_static_fields() {
        let gr = &ROOT_GROUP;
        assert_eq!(gr.gr_gid, 0);

        let name = unsafe { core::ffi::CStr::from_ptr(gr.gr_name.cast()) };
        assert_eq!(name.to_bytes(), b"root");

        let passwd = unsafe { core::ffi::CStr::from_ptr(gr.gr_passwd.cast()) };
        assert_eq!(passwd.to_bytes(), b"x");

        // gr_mem is non-null and first element is null (empty member list).
        assert!(!gr.gr_mem.is_null());
        let first_member = unsafe { *gr.gr_mem };
        assert!(first_member.is_null());
    }

    // -----------------------------------------------------------------------
    // Struct layout (size checks for x86_64)
    // -----------------------------------------------------------------------

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn passwd_struct_size() {
        // 5 pointers (8 bytes each) + 2 u32 fields (packed together) = 48 bytes.
        assert_eq!(core::mem::size_of::<Passwd>(), 48);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn group_struct_size() {
        // 2 pointers (8 each) + 1 u32 + 4 padding + 1 pointer (8) = 32 bytes.
        assert_eq!(core::mem::size_of::<Group>(), 32);
    }

    // -------------------------------------------------------------------
    // Password database enumeration
    // -------------------------------------------------------------------

    #[test]
    fn test_setpwent_getpwent_endpwent() {
        // Reset enumeration state.
        setpwent();

        // First call returns the root entry.
        let p1 = getpwent();
        assert!(!p1.is_null());
        let uid = unsafe { (*p1).pw_uid };
        assert_eq!(uid, 0); // root

        // Second call returns null (only one entry).
        let p2 = getpwent();
        assert!(p2.is_null());

        // Close and re-open: should get root again.
        endpwent();
        setpwent();
        let p3 = getpwent();
        assert!(!p3.is_null());
        assert_eq!(unsafe { (*p3).pw_uid }, 0);

        endpwent();
    }

    // -------------------------------------------------------------------
    // Group database enumeration
    // -------------------------------------------------------------------

    #[test]
    fn test_setgrent_getgrent_endgrent() {
        setgrent();

        let g1 = getgrent();
        assert!(!g1.is_null());
        assert_eq!(unsafe { (*g1).gr_gid }, 0); // root

        let g2 = getgrent();
        assert!(g2.is_null());

        endgrent();
        setgrent();
        let g3 = getgrent();
        assert!(!g3.is_null());
        assert_eq!(unsafe { (*g3).gr_gid }, 0);

        endgrent();
    }

    // -- getgrouplist --

    #[test]
    fn test_getgrouplist_basic() {
        let mut groups = [0i32; 4];
        let mut ngroups: i32 = 4;
        let ret = getgrouplist(
            b"root\0".as_ptr(),
            0,
            groups.as_mut_ptr() as *mut GidT,
            &mut ngroups,
        );
        assert_eq!(ret, 1);
        assert_eq!(ngroups, 1);
        assert_eq!(groups[0], 0);
    }

    #[test]
    fn test_getgrouplist_nonroot_user() {
        let mut groups = [0i32; 4];
        let mut ngroups: i32 = 4;
        let ret = getgrouplist(
            b"nobody\0".as_ptr(),
            1000,
            groups.as_mut_ptr() as *mut GidT,
            &mut ngroups,
        );
        assert_eq!(ret, 1);
        assert_eq!(ngroups, 1);
        assert_eq!(groups[0], 1000);
    }

    #[test]
    fn test_getgrouplist_buffer_too_small() {
        let mut ngroups: i32 = 0;
        let ret = getgrouplist(
            b"root\0".as_ptr(),
            0,
            core::ptr::null_mut(),
            &mut ngroups,
        );
        assert_eq!(ret, -1);
        assert_eq!(ngroups, 1);
    }

    #[test]
    fn test_getgrouplist_null_ngroups() {
        let ret = getgrouplist(
            b"root\0".as_ptr(),
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
    }

    // -- initgroups --

    #[test]
    fn test_initgroups_succeeds() {
        assert_eq!(initgroups(b"root\0".as_ptr(), 0), 0);
    }

    #[test]
    fn test_initgroups_nonroot() {
        assert_eq!(initgroups(b"nobody\0".as_ptr(), 1000), 0);
    }

    #[test]
    fn test_initgroups_null_user() {
        assert_eq!(initgroups(core::ptr::null(), 0), 0);
    }
}
