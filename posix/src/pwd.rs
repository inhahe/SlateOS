//! POSIX user and group databases (`<pwd.h>`, `<grp.h>`): `/etc/passwd` and
//! `/etc/group`, read as glibc 2.39's `nss_files` reads them.
//!
//! On SlateOS `/etc/users.yaml` is the account store, and `/etc/passwd` and
//! `/etc/group` are generated from it on every change (design-decisions
//! §353). This is the C library's view of those two files, as a glibc
//! program expects it:
//!
//! - **Lookups** by name or id take the first matching line, skipping the NIS
//!   `+`/`-` lines, as `nss_files` does; enumeration returns every entry,
//!   those included. The line syntax is [`crate::nss_files`]'.
//! - **The `_r` functions** copy the entry into the caller's buffer (`ERANGE`
//!   when it does not fit) and return 0 both when they found it and when
//!   there is none -- `*result` tells them apart -- setting `errno` to what
//!   they return, as glibc's do. The others keep the entry in this process's
//!   storage, growing it as needed, and return NULL with `errno` 0 for "no
//!   such entry".
//! - **`getgrouplist`** is the primary group, then every group whose member
//!   list names the user, as `nss_files`' `initgroups_dyn` finds them.
//!
//! A database that does not exist is answered from a built-in `root` entry --
//! uid and gid 0, home `/`, shell `/bin/sh` -- so a system image without the
//! files still knows its one user (design-decisions §1113). One that exists
//! and cannot be read is an error. A NULL pointer glibc would dereference is
//! `EFAULT` (design-decisions §303).
//!
//! ## What changed on 2026-09-26
//!
//! The databases were that built-in entry and nothing else: a lookup of any
//! other user or group failed, whatever `/etc/passwd` and `/etc/group` held.
//! A reentrant group lookup assumed the caller's buffer was 8-byte aligned,
//! and `getpwent_r` and `getgrent_r` did not exist.
//! (`known-issues.md` -> `B-D-PWD-KNEW-ONLY-ROOT`.)

use crate::errno;
use crate::nss_files::{
    Cursor, Fields, Held, Room, Which, c_bytes, enumerated, hold, is_compat, is_space, lines,
    lookup_result, next_entry, reentrant, string_or_null, with_text,
};
use crate::perprocess::process_global;
use crate::types::*;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Password database entry (struct passwd).
#[repr(C)]
pub struct Passwd {
    /// Username.
    pub pw_name: *const u8,
    /// Encrypted password, or `x` for "in `/etc/shadow`".
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
    /// Encrypted group password.
    pub gr_passwd: *const u8,
    /// Group ID.
    pub gr_gid: GidT,
    /// Null-terminated array of member names.
    pub gr_mem: *const *const u8,
}

impl Passwd {
    const EMPTY: Self = Self {
        pw_name: core::ptr::null(),
        pw_passwd: core::ptr::null(),
        pw_uid: 0,
        pw_gid: 0,
        pw_gecos: core::ptr::null(),
        pw_dir: core::ptr::null(),
        pw_shell: core::ptr::null(),
    };
}

impl Group {
    const EMPTY: Self = Self {
        gr_name: core::ptr::null(),
        gr_passwd: core::ptr::null(),
        gr_gid: 0,
        gr_mem: core::ptr::null(),
    };
}

// ---------------------------------------------------------------------------
// The databases' built-in text
// ---------------------------------------------------------------------------

/// `/etc/passwd` when there is none: its one user.
const ROOT_PASSWD_TEXT: &[u8] = b"root:x:0:0:root:/:/bin/sh\n";

/// `/etc/group` when there is none: its one group, with no members.
const ROOT_GROUP_TEXT: &[u8] = b"root:x:0:\n";

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// One `/etc/passwd` entry, borrowed from its line. `None` fields are the
/// NULL pointers of a bare `+`/`-` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PwEntry<'a> {
    name: &'a [u8],
    passwd: Option<&'a [u8]>,
    uid: u32,
    gid: u32,
    gecos: Option<&'a [u8]>,
    dir: Option<&'a [u8]>,
    shell: Option<&'a [u8]>,
}

/// glibc's passwd `LINE_PARSER`: `None` for a line that is no entry.
fn parse_pw(line: &[u8]) -> Option<PwEntry<'_>> {
    let mut f = Fields::new(line);
    let name = f.string();
    let compat = is_compat(name);
    if compat && f.at_end() {
        return Some(PwEntry {
            name,
            passwd: None,
            uid: 0,
            gid: 0,
            gecos: None,
            dir: None,
            shell: None,
        });
    }
    let passwd = f.string();
    let (uid, gid) = if compat {
        let uid = f.int_or_empty().ok()?.unwrap_or(0);
        (uid, f.int_or_empty().ok()?.unwrap_or(0))
    } else {
        let uid = f.int()?;
        (uid, f.int()?)
    };
    let gecos = f.string();
    let dir = f.string();
    Some(PwEntry {
        name,
        passwd: Some(passwd),
        uid,
        gid,
        gecos: Some(gecos),
        dir: Some(dir),
        shell: Some(f.rest()),
    })
}

/// One `/etc/group` entry, borrowed from its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GrEntry<'a> {
    name: &'a [u8],
    passwd: Option<&'a [u8]>,
    gid: u32,
    /// The member list as written; [`members`] takes it apart.
    members: &'a [u8],
}

/// glibc's group `LINE_PARSER`: `None` for a line that is no entry.
fn parse_gr(line: &[u8]) -> Option<GrEntry<'_>> {
    let mut f = Fields::new(line);
    let name = f.string();
    let compat = is_compat(name);
    let (passwd, gid) = if compat && f.at_end() {
        (None, 0)
    } else {
        let passwd = f.string();
        let gid = if compat {
            f.int_or_empty().ok()?.unwrap_or(0)
        } else {
            f.int()?
        };
        (Some(passwd), gid)
    };
    Some(GrEntry {
        name,
        passwd,
        gid,
        members: f.rest(),
    })
}

/// `parse_list`'s members: split at `,`, each without its leading white
/// space, the empty ones dropped.
fn members(list: &[u8]) -> impl Iterator<Item = &[u8]> {
    list.split(|&b| b == b',').filter_map(|m| {
        let start = m.iter().position(|&b| !is_space(b))?;
        m.get(start..)
    })
}

// ---------------------------------------------------------------------------
// Filling the caller's structures
// ---------------------------------------------------------------------------

fn fill_pw(e: &PwEntry<'_>, room: &mut Room) -> Result<Passwd, i32> {
    Ok(Passwd {
        pw_name: room.string(e.name)?.cast_const(),
        pw_passwd: string_or_null(room, e.passwd)?,
        pw_uid: e.uid,
        pw_gid: e.gid,
        pw_gecos: string_or_null(room, e.gecos)?,
        pw_dir: string_or_null(room, e.dir)?,
        pw_shell: string_or_null(room, e.shell)?,
    })
}

fn fill_gr(e: &GrEntry<'_>, room: &mut Room) -> Result<Group, i32> {
    let name = room.string(e.name)?.cast_const();
    let passwd = string_or_null(room, e.passwd)?;
    let n = members(e.members).count();
    // The NULL-terminated vector, then each member's string.
    let vector = room.pointers(n.checked_add(1).ok_or(errno::ERANGE)?)?;
    for (i, m) in members(e.members).enumerate() {
        let s = room.string(m)?;
        // SAFETY: `vector` holds `n + 1` pointers and `i < n`.
        unsafe { vector.add(i).write(s.cast_const()) };
    }
    Ok(Group {
        gr_name: name,
        gr_passwd: passwd,
        gr_gid: e.gid,
        gr_mem: vector.cast_const(),
    })
}

/// The first entry of `which`'s text that `pick` chooses, filled into `room`
/// -- or `None`.
fn find_pw(room: &mut Room, pick: impl Fn(&PwEntry<'_>) -> bool) -> Result<Option<Passwd>, i32> {
    with_text(Which::Passwd, ROOT_PASSWD_TEXT, |text| {
        lines(text, 0)
            .filter_map(|(line, _)| parse_pw(line))
            .find(|e| !is_compat(e.name) && pick(e))
            .map(|e| fill_pw(&e, room))
            .transpose()
    })?
}

fn find_gr(room: &mut Room, pick: impl Fn(&GrEntry<'_>) -> bool) -> Result<Option<Group>, i32> {
    with_text(Which::Group, ROOT_GROUP_TEXT, |text| {
        lines(text, 0)
            .filter_map(|(line, _)| parse_gr(line))
            .find(|e| !is_compat(e.name) && pick(e))
            .map(|e| fill_gr(&e, room))
            .transpose()
    })?
}

// ---------------------------------------------------------------------------
// Reentrant lookups
// ---------------------------------------------------------------------------

/// Look up a user by name (reentrant).
///
/// Returns 0 with `*result = pwd` when found, 0 with `*result = NULL` when
/// not, `ERANGE` when `buf` is too small, `EFAULT` for a NULL argument, or
/// the error reading `/etc/passwd`; `errno` is set to the same.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpwnam_r(
    name: *const u8,
    pwd: *mut Passwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Passwd,
) -> i32 {
    if name.is_null() {
        // glibc reads the name before anything else.
        errno::set_errno(errno::EFAULT);
        return errno::EFAULT;
    }
    // SAFETY: `name` is non-null and the caller's NUL-terminated string.
    let name = unsafe { c_bytes(name) };
    // SAFETY: the other pointers are the caller's, as documented.
    unsafe {
        reentrant(pwd, buf, buflen, result, |room| {
            find_pw(room, |e| e.name == name)
        })
    }
}

/// Look up a user by UID (reentrant). Same results as [`getpwnam_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpwuid_r(
    uid: UidT,
    pwd: *mut Passwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Passwd,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented.
    unsafe {
        reentrant(pwd, buf, buflen, result, |room| {
            find_pw(room, |e| e.uid == uid)
        })
    }
}

/// Look up a group by name (reentrant). Same results as [`getpwnam_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getgrnam_r(
    name: *const u8,
    grp: *mut Group,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Group,
) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return errno::EFAULT;
    }
    // SAFETY: as in `getpwnam_r`.
    let name = unsafe { c_bytes(name) };
    // SAFETY: as in `getpwnam_r`.
    unsafe {
        reentrant(grp, buf, buflen, result, |room| {
            find_gr(room, |e| e.name == name)
        })
    }
}

/// Look up a group by GID (reentrant). Same results as [`getpwnam_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getgrgid_r(
    gid: GidT,
    grp: *mut Group,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Group,
) -> i32 {
    // SAFETY: as in `getpwnam_r`.
    unsafe {
        reentrant(grp, buf, buflen, result, |room| {
            find_gr(room, |e| e.gid == gid)
        })
    }
}

// ---------------------------------------------------------------------------
// This process's storage for the non-reentrant functions
// ---------------------------------------------------------------------------

process_global! {
    fn held_pw() -> Held<Passwd> = Held::new(Passwd::EMPTY);
    fn held_gr() -> Held<Group> = Held::new(Group::EMPTY);
    fn held_pwent() -> Held<Passwd> = Held::new(Passwd::EMPTY);
    fn held_grent() -> Held<Group> = Held::new(Group::EMPTY);
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

/// Look up a user by name: the entry, or NULL -- `errno` 0 for no such user.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpwnam(name: *const u8) -> *const Passwd {
    // SAFETY: `name` is passed on as given; the rest is this process's.
    lookup_result(hold(held_pw(), |p, b, l, r| unsafe {
        getpwnam_r(name, p, b, l, r)
    }))
}

/// Look up a user by UID: the entry, or NULL -- `errno` 0 for no such user.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpwuid(uid: UidT) -> *const Passwd {
    // SAFETY: this process's storage.
    lookup_result(hold(held_pw(), |p, b, l, r| unsafe {
        getpwuid_r(uid, p, b, l, r)
    }))
}

/// Look up a group by name: the entry, or NULL -- `errno` 0 for none.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getgrnam(name: *const u8) -> *const Group {
    // SAFETY: as in `getpwnam`.
    lookup_result(hold(held_gr(), |p, b, l, r| unsafe {
        getgrnam_r(name, p, b, l, r)
    }))
}

/// Look up a group by GID: the entry, or NULL -- `errno` 0 for none.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgrgid(gid: GidT) -> *const Group {
    // SAFETY: as in `getpwuid`.
    lookup_result(hold(held_gr(), |p, b, l, r| unsafe {
        getgrgid_r(gid, p, b, l, r)
    }))
}

// ---------------------------------------------------------------------------
// Enumeration
// ---------------------------------------------------------------------------

process_global! {
    fn pw_cursor() -> Cursor = Cursor::CLOSED;
    fn gr_cursor() -> Cursor = Cursor::CLOSED;
}

/// Rewind the password database: the next [`getpwent`] reads it afresh.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setpwent() {
    // SAFETY: this process's cursor.
    unsafe { *pw_cursor() = Cursor::CLOSED };
}

/// The next password entry into the caller's buffer: 0, `ENOENT` at the end,
/// or `ERANGE` (the same entry comes next time).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpwent_r(
    pwd: *mut Passwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Passwd,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented.
    unsafe {
        next_entry(
            pw_cursor(),
            Which::Passwd,
            ROOT_PASSWD_TEXT,
            |line, room| parse_pw(line).map(|e| fill_pw(&e, room)),
            pwd,
            buf,
            buflen,
            result,
        )
    }
}

/// The next password entry, or NULL at the end (`errno` untouched there).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpwent() -> *const Passwd {
    // SAFETY: this process's storage.
    enumerated(hold(held_pwent(), |p, b, l, r| unsafe {
        getpwent_r(p, b, l, r)
    }))
}

/// Close the password database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endpwent() {
    // SAFETY: this process's cursor.
    unsafe { *pw_cursor() = Cursor::CLOSED };
}

/// Rewind the group database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setgrent() {
    // SAFETY: this process's cursor.
    unsafe { *gr_cursor() = Cursor::CLOSED };
}

/// The next group entry into the caller's buffer; as [`getpwent_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getgrent_r(
    grp: *mut Group,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Group,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented.
    unsafe {
        next_entry(
            gr_cursor(),
            Which::Group,
            ROOT_GROUP_TEXT,
            |line, room| parse_gr(line).map(|e| fill_gr(&e, room)),
            grp,
            buf,
            buflen,
            result,
        )
    }
}

/// The next group entry, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgrent() -> *const Group {
    // SAFETY: this process's storage.
    enumerated(hold(held_grent(), |p, b, l, r| unsafe {
        getgrent_r(p, b, l, r)
    }))
}

/// Close the group database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endgrent() {
    // SAFETY: this process's cursor.
    unsafe { *gr_cursor() = Cursor::CLOSED };
}

// ---------------------------------------------------------------------------
// getgrouplist / initgroups
// ---------------------------------------------------------------------------

/// The groups `user` belongs to: `group` first, then each group whose member
/// list names `user` -- other than `group` itself -- in `/etc/group`'s order,
/// as `nss_files`' `initgroups_dyn` finds them.
///
/// Up to `*ngroups` of them are written to `groups`, and `*ngroups` becomes
/// how many there are. Returns that count, or -1 when `*ngroups` was too
/// small for it (glibc's `getgrouplist`). A group database that cannot be
/// read contributes nothing, as an unavailable service does. A NULL `user`
/// is a member of no group; a NULL `ngroups`, or a NULL `groups` that would
/// be written to, is -1 with `EFAULT`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getgrouplist(
    user: *const u8,
    group: GidT,
    groups: *mut GidT,
    ngroups: *mut i32,
) -> i32 {
    if ngroups.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: `ngroups` is non-null and the caller's.
    let room = usize::try_from(unsafe { *ngroups }).unwrap_or(0);
    if groups.is_null() && room > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let user: Option<&[u8]> = if user.is_null() {
        None
    } else {
        // SAFETY: the caller's NUL-terminated string.
        Some(unsafe { c_bytes(user) })
    };
    let mut total = 0usize;
    let mut add = |gid: GidT| {
        if total < room {
            // SAFETY: `groups` holds `room` entries (the caller's `*ngroups`).
            unsafe { groups.add(total).write(gid) };
        }
        total = total.saturating_add(1);
    };
    add(group);
    // An unreadable database contributes nothing: `initgroups_dyn` is then
    // an unavailable service, and glibc goes on with what it has.
    let _ = with_text(Which::Group, ROOT_GROUP_TEXT, |text| {
        for (line, _) in lines(text, 0) {
            let Some(e) = parse_gr(line) else { continue };
            if e.gid != group && user.is_some_and(|u| members(e.members).any(|m| m == u)) {
                add(e.gid);
            }
        }
    });
    let count = i32::try_from(total).unwrap_or(i32::MAX);
    // SAFETY: as above.
    unsafe { *ngroups = count };
    if total > room { -1 } else { count }
}

/// Set the calling process's supplementary groups to the ones `user`
/// belongs to -- `group`, then each group `/etc/group` names `user` in:
/// [`getgrouplist`]'s list, handed to
/// [`setgroups`](crate::unistd::setgroups), as glibc's `initgroups` does.
///
/// Until 2026-09-27 this returned 0 and set nothing, on the premise that "the
/// kernel keeps no supplementary groups yet" -- no longer so once
/// `SYS_PROCESS_SETGROUPS` existed and `setgroups` reached it.  `login`,
/// `su` and every daemon that drops from root call `initgroups` and then
/// `setuid`; with the stub they kept root's supplementary groups while the
/// return value they checked said the new ones were in place -- the false
/// success `setgroups`'s own documentation calls the one that breaks worst.
///
/// glibc's loop is kept: when the kernel refuses the list as too long
/// (`EINVAL`), it is tried one group shorter, until a length is taken or none
/// is left.  Returns 0, or -1 with `setgroups`'s errno -- `EPERM` without
/// `CAP_SETGID` -- or with `ENOMEM` if the list cannot be held.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn initgroups(user: *const u8, group: GidT) -> i32 {
    initgroups_with(user, group, |list| {
        crate::unistd::setgroups(list.len(), list.as_ptr())
    })
}

/// [`initgroups`] with the call that installs the list passed in, so the
/// tests can see the list and play the kernel's refusals.
fn initgroups_with(user: *const u8, group: GidT, mut set: impl FnMut(&[GidT]) -> i32) -> i32 {
    let Some(list) = GroupList::of(user, group) else {
        errno::set_errno(errno::ENOMEM);
        return -1;
    };
    let groups = list.as_slice();
    let mut len = groups.len();
    loop {
        let r = set(groups.get(..len).unwrap_or(groups));
        // glibc: `while (result == -1 && errno == EINVAL && --ngroups > 0)`.
        if r == -1 && errno::get_errno() == errno::EINVAL && len > 1 {
            len = len.saturating_sub(1);
            continue;
        }
        return r;
    }
}

/// The groups [`getgrouplist`] finds for a user, in a block of their own.
struct GroupList {
    ptr: *mut GidT,
    len: usize,
}

impl GroupList {
    /// The whole list, however long: asked for its length, then filled, and
    /// asked again if `/etc/group` grew in between.  `None` when memory runs
    /// out, or the file keeps growing faster than it can be read.
    fn of(user: *const u8, group: GidT) -> Option<Self> {
        let mut n: i32 = 0;
        // The count query: no room, a NULL vector; -1 with the count.
        let _ = getgrouplist(user, group, core::ptr::null_mut(), &mut n);
        for _ in 0..8 {
            let room = usize::try_from(n).ok()?.max(1);
            let bytes = room.checked_mul(core::mem::size_of::<GidT>())?;
            let ptr = crate::malloc::malloc(bytes).cast::<GidT>();
            if ptr.is_null() {
                return None;
            }
            // Owned from here: every way out of this iteration frees it,
            // except the one that returns it.
            let mut block = Self { ptr, len: 0 };
            let mut got = i32::try_from(room).ok()?;
            if getgrouplist(user, group, block.ptr, &mut got) >= 0 {
                block.len = usize::try_from(got).ok()?.min(room);
                return Some(block);
            }
            // It grew between the two reads: `got` is the new count, and
            // `block` is freed as the loop goes round.
            n = got;
        }
        None
    }

    fn as_slice(&self) -> &[GidT] {
        // SAFETY: `ptr` is this list's own block, and its first `len`
        // entries were written by `getgrouplist`.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for GroupList {
    fn drop(&mut self) {
        // SAFETY: `ptr` is this list's own `malloc` block.
        unsafe { crate::malloc::free(self.ptr.cast()) };
    }
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
        errno::set_errno(errno::ERANGE);
        return -1;
    }
    // SAFETY: `buf` holds at least 5 bytes.
    unsafe { core::ptr::copy_nonoverlapping(b"root\0".as_ptr(), buf, 5) };
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::nss_files::set_test_text;
    use std::vec::Vec;

    /// A C string's bytes.
    fn s(p: *const u8) -> Vec<u8> {
        assert!(!p.is_null(), "expected a string, got NULL");
        // SAFETY: the library hands out NUL-terminated strings.
        unsafe { c_bytes(p) }.to_vec()
    }

    /// The entry behind `p`, which must not be NULL.
    fn found<T>(p: *const T) -> &'static T {
        assert!(
            !p.is_null(),
            "expected an entry, got NULL (errno {})",
            errno::get_errno()
        );
        // SAFETY: non-null, and the library's entry.
        unsafe { &*p }
    }

    /// `gr_mem`, as names.
    fn mem(g: &Group) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            // SAFETY: `gr_mem` is a NULL-terminated vector.
            let p = unsafe { *g.gr_mem.add(i) };
            if p.is_null() {
                return out;
            }
            out.push(s(p));
            i += 1;
        }
    }

    const PASSWD: &[u8] = b"\
# the accounts
root:x:0:0:root:/root:/bin/bash
  daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin

alice:x:1000:1000:Alice Liddell,,,:/home/alice:/bin/sh
bad:x:notanumber:5:::
bob:x:1001:100:::/bin/sh:with:colons
+nis
alice:x:2000:2000:the second alice:/:/bin/false
";

    const GROUP: &[u8] = b"\
root:x:0:
wheel:x:10:root, alice
users:x:100:alice,bob,,  carol
alice:x:1000:
noise:x:x:alice
staff:x:50:bob,alice
+:::
";

    fn with_passwd(text: &'static [u8]) {
        set_test_text(Which::Passwd, Some(text));
    }

    fn with_group(text: &'static [u8]) {
        set_test_text(Which::Group, Some(text));
    }

    // -- parsing --

    #[test]
    fn parse_pw_takes_a_line_apart_as_glibc_does() {
        let e = parse_pw(b"alice:x:1000:1000:Alice:/home/alice:/bin/sh").unwrap();
        assert_eq!(
            (e.name, e.passwd, e.uid, e.gid, e.gecos, e.dir, e.shell),
            (
                &b"alice"[..],
                Some(&b"x"[..]),
                1000,
                1000,
                Some(&b"Alice"[..]),
                Some(&b"/home/alice"[..]),
                Some(&b"/bin/sh"[..])
            )
        );
        // The shell is the rest of the line, colons and all.
        assert_eq!(
            parse_pw(b"b:x:1:2:g:d:sh:x:y").unwrap().shell,
            Some(&b"sh:x:y"[..])
        );
        // Missing trailing fields are empty.
        let short = parse_pw(b"c:x:3:4").unwrap();
        assert_eq!(
            (short.gecos, short.dir, short.shell),
            (Some(&b""[..]), Some(&b""[..]), Some(&b""[..]))
        );
    }

    #[test]
    fn parse_pw_refuses_what_glibc_skips() {
        assert_eq!(
            parse_pw(b"x:x:nan:0::/:/bin/sh"),
            None,
            "a uid that is no number"
        );
        assert_eq!(parse_pw(b"x:x::0::/:/bin/sh"), None, "an empty uid");
        assert_eq!(parse_pw(b"x:x:5a:0::/:/bin/sh"), None, "junk after the uid");
        assert_eq!(parse_pw(b"justaname"), None);
        // Numbers are strtoul's: signs and clamping.
        assert_eq!(
            parse_pw(b"x:x:-1:+2:::").map(|e| (e.uid, e.gid)),
            Some((u32::MAX, 2))
        );
    }

    #[test]
    fn parse_pw_keeps_nis_compat_lines_as_glibc_does() {
        let bare = parse_pw(b"+").unwrap();
        assert_eq!(
            (bare.name, bare.passwd, bare.shell),
            (&b"+"[..], None, None)
        );
        // A compat line may leave its numbers empty.
        let e = parse_pw(b"-someone::::::").unwrap();
        assert_eq!((e.uid, e.gid, e.passwd), (0, 0, Some(&b""[..])));
    }

    #[test]
    fn parse_gr_and_its_members() {
        let e = parse_gr(b"users:x:100:alice,bob,,  carol").unwrap();
        assert_eq!(
            (e.name, e.passwd, e.gid),
            (&b"users"[..], Some(&b"x"[..]), 100)
        );
        assert_eq!(
            members(e.members).collect::<Vec<_>>(),
            [&b"alice"[..], b"bob", b"carol"]
        );
        assert_eq!(members(b"").count(), 0);
        assert_eq!(
            members(b" , ,").count(),
            0,
            "white space alone is no member"
        );
        assert_eq!(parse_gr(b"g:x:nan:a"), None);
        assert_eq!(parse_gr(b"+").map(|e| (e.passwd, e.gid)), Some((None, 0)));
    }

    // -- lookups against a database --

    #[test]
    fn getpwnam_reads_etc_passwd() {
        with_passwd(PASSWD);
        // SAFETY: a NUL-terminated name.
        let p = unsafe { getpwnam(c"alice".as_ptr().cast()) };
        assert!(!p.is_null());
        // SAFETY: the library's entry.
        let p = unsafe { &*p };
        assert_eq!(
            (p.pw_uid, p.pw_gid),
            (1000, 1000),
            "the first alice, not the second"
        );
        assert_eq!(s(p.pw_gecos), b"Alice Liddell,,,");
        assert_eq!(s(p.pw_dir), b"/home/alice");
        assert_eq!(s(p.pw_shell), b"/bin/sh");
        assert_eq!(errno::get_errno(), 0);
        // Leading white space is not part of the name.
        // SAFETY: as above.
        assert!(!unsafe { getpwnam(c"daemon".as_ptr().cast()) }.is_null());
    }

    #[test]
    fn a_user_not_there_is_null_with_errno_zero() {
        with_passwd(PASSWD);
        errno::set_errno(errno::EBADF);
        // SAFETY: a NUL-terminated name.
        assert!(unsafe { getpwnam(c"nobody".as_ptr().cast()) }.is_null());
        assert_eq!(errno::get_errno(), 0);
        errno::set_errno(errno::EBADF);
        assert!(getpwuid(4242).is_null());
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn lookups_skip_bad_lines_and_nis_entries() {
        with_passwd(PASSWD);
        // SAFETY: NUL-terminated names.
        unsafe {
            assert!(
                getpwnam(c"bad".as_ptr().cast()).is_null(),
                "its uid is no number"
            );
            assert!(getpwnam(c"+nis".as_ptr().cast()).is_null());
        }
        // SAFETY: the library's entry.
        let bob = found(getpwuid(1001));
        assert_eq!(s(bob.pw_shell), b"/bin/sh:with:colons");
    }

    #[test]
    fn getpwuid_takes_the_first_line_with_the_uid() {
        with_passwd(PASSWD);
        // SAFETY: the library's entry.
        let root = found(getpwuid(0));
        assert_eq!(s(root.pw_name), b"root");
        assert_eq!(
            s(root.pw_dir),
            b"/root",
            "the file's root, not the built-in one"
        );
        // SAFETY: as above.
        assert_eq!(s(found(getpwuid(2000)).pw_gecos), b"the second alice");
    }

    #[test]
    fn a_missing_database_is_the_builtin_root() {
        set_test_text(Which::Passwd, None);
        set_test_text(Which::Group, None);
        // SAFETY: the library's entries.
        let root = found(getpwuid(0));
        assert_eq!(
            (s(root.pw_name), s(root.pw_dir), s(root.pw_shell)),
            (b"root".to_vec(), b"/".to_vec(), b"/bin/sh".to_vec())
        );
        // SAFETY: a NUL-terminated name.
        assert!(!unsafe { getpwnam(c"root".as_ptr().cast()) }.is_null());
        assert!(getpwuid(1000).is_null());
        // SAFETY: the library's entry.
        let g = found(getgrgid(0));
        assert_eq!(s(g.gr_name), b"root");
        assert!(mem(g).is_empty());
    }

    #[test]
    fn getgrnam_and_getgrgid_read_etc_group() {
        with_group(GROUP);
        // SAFETY: the library's entry.
        let wheel = found(getgrgid(10));
        assert_eq!(s(wheel.gr_name), b"wheel");
        assert_eq!(mem(wheel), [b"root".to_vec(), b"alice".to_vec()]);
        // SAFETY: a NUL-terminated name; the library's entry.
        let users = found(unsafe { getgrnam(c"users".as_ptr().cast()) });
        assert_eq!(users.gr_gid, 100);
        assert_eq!(
            mem(users),
            [b"alice".to_vec(), b"bob".to_vec(), b"carol".to_vec()]
        );
        // SAFETY: a NUL-terminated name.
        assert!(
            unsafe { getgrnam(c"noise".as_ptr().cast()) }.is_null(),
            "its gid is no number"
        );
    }

    #[test]
    fn a_null_name_is_efault() {
        with_passwd(PASSWD);
        // SAFETY: NULL is what is being tested.
        assert!(unsafe { getpwnam(core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // SAFETY: as above.
        assert!(unsafe { getgrnam(core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- the reentrant forms --

    #[test]
    fn getpwnam_r_fills_the_callers_buffer() {
        with_passwd(PASSWD);
        let mut pwd = Passwd::EMPTY;
        let mut buf = [0u8; 256];
        let mut result: *const Passwd = core::ptr::null();
        // SAFETY: the caller's objects.
        let rc = unsafe {
            getpwnam_r(
                c"bob".as_ptr().cast(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(rc, 0);
        assert_eq!(result, &raw const pwd);
        assert_eq!(s(pwd.pw_name), b"bob");
        let range = buf.as_ptr_range();
        assert!(
            range.contains(&pwd.pw_shell),
            "strings live in the caller's buffer"
        );
    }

    #[test]
    fn getpwnam_r_not_found_is_zero_with_a_null_result() {
        with_passwd(PASSWD);
        let mut pwd = Passwd::EMPTY;
        let mut buf = [0u8; 256];
        let mut result: *const Passwd = &raw const pwd;
        // SAFETY: the caller's objects.
        let rc = unsafe {
            getpwnam_r(
                c"zed".as_ptr().cast(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!((rc, result), (0, core::ptr::null()));
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn a_short_buffer_is_erange_and_the_long_path_grows() {
        with_passwd(PASSWD);
        let mut pwd = Passwd::EMPTY;
        let mut buf = [0u8; 8];
        let mut result: *const Passwd = core::ptr::null();
        // SAFETY: the caller's objects.
        let rc = unsafe {
            getpwnam_r(
                c"alice".as_ptr().cast(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!((rc, result), (errno::ERANGE, core::ptr::null()));
        assert_eq!(errno::get_errno(), errno::ERANGE);
        // A GECOS field longer than glibc's 1024-byte first buffer.
        static LONG: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        let text = LONG.get_or_init(|| {
            let mut t = b"long:x:7:7:".to_vec();
            t.extend(core::iter::repeat_n(b'g', 5000));
            t.extend_from_slice(b":/:/bin/sh\n");
            t
        });
        with_passwd(text);
        // SAFETY: the library's entry.
        let e = found(getpwuid(7));
        assert_eq!(s(e.pw_gecos).len(), 5000);
    }

    #[test]
    fn getgrgid_r_aligns_the_member_vector_whatever_the_buffer() {
        with_group(GROUP);
        let mut backing = [0u64; 32];
        for skew in 0..8 {
            let mut grp = Group::EMPTY;
            let mut result: *const Group = core::ptr::null();
            let buf = backing.as_mut_ptr().cast::<u8>().wrapping_add(skew);
            // SAFETY: `buf` has 256 - skew writable bytes.
            let rc = unsafe { getgrgid_r(100, &mut grp, buf, 256 - skew, &mut result) };
            assert_eq!(rc, 0, "skew {skew}");
            assert_eq!(
                grp.gr_mem as usize % align_of::<*const u8>(),
                0,
                "skew {skew}"
            );
            assert_eq!(mem(&grp).len(), 3);
        }
    }

    #[test]
    fn the_r_forms_refuse_null_outputs() {
        with_passwd(PASSWD);
        let mut pwd = Passwd::EMPTY;
        let mut buf = [0u8; 64];
        let mut result: *const Passwd = core::ptr::null();
        let name = c"root".as_ptr().cast();
        // SAFETY: NULLs are what is being tested.
        unsafe {
            assert_eq!(
                getpwnam_r(
                    name,
                    core::ptr::null_mut(),
                    buf.as_mut_ptr(),
                    64,
                    &mut result
                ),
                errno::EFAULT
            );
            assert_eq!(
                getpwnam_r(name, &mut pwd, core::ptr::null_mut(), 64, &mut result),
                errno::EFAULT
            );
            assert_eq!(
                getpwnam_r(name, &mut pwd, buf.as_mut_ptr(), 64, core::ptr::null_mut()),
                errno::EFAULT
            );
            assert_eq!(
                getpwnam_r(
                    core::ptr::null(),
                    &mut pwd,
                    buf.as_mut_ptr(),
                    64,
                    &mut result
                ),
                errno::EFAULT
            );
            assert_eq!(
                getpwuid_r(0, core::ptr::null_mut(), buf.as_mut_ptr(), 64, &mut result),
                errno::EFAULT
            );
        }
    }

    // -- enumeration --

    #[test]
    fn getpwent_walks_every_entry_nis_lines_included() {
        with_passwd(PASSWD);
        setpwent();
        let mut names = Vec::new();
        loop {
            let p = getpwent();
            if p.is_null() {
                break;
            }
            // SAFETY: the library's entry.
            names.push(s(unsafe { &*p }.pw_name));
        }
        endpwent();
        let want: [&[u8]; 6] = [b"root", b"daemon", b"alice", b"bob", b"+nis", b"alice"];
        assert_eq!(names, want.map(<[u8]>::to_vec));
    }

    #[test]
    fn getpwent_r_gives_the_same_entry_again_after_erange() {
        with_passwd(PASSWD);
        setpwent();
        let mut pwd = Passwd::EMPTY;
        let mut small = [0u8; 4];
        let mut big = [0u8; 256];
        let mut result: *const Passwd = core::ptr::null();
        // SAFETY: the caller's objects.
        unsafe {
            assert_eq!(
                getpwent_r(&mut pwd, small.as_mut_ptr(), 4, &mut result),
                errno::ERANGE
            );
            assert_eq!(getpwent_r(&mut pwd, big.as_mut_ptr(), 256, &mut result), 0);
        }
        assert_eq!(s(pwd.pw_name), b"root", "the entry that did not fit");
        endpwent();
    }

    #[test]
    fn enumeration_ends_with_enoent_and_setpwent_rewinds() {
        with_passwd(b"one:x:1:1:::\ntwo:x:2:2:::\n");
        setpwent();
        let mut pwd = Passwd::EMPTY;
        let mut buf = [0u8; 128];
        let mut result: *const Passwd = core::ptr::null();
        // SAFETY: the caller's objects.
        unsafe {
            assert_eq!(getpwent_r(&mut pwd, buf.as_mut_ptr(), 128, &mut result), 0);
            assert_eq!(getpwent_r(&mut pwd, buf.as_mut_ptr(), 128, &mut result), 0);
            assert_eq!(
                getpwent_r(&mut pwd, buf.as_mut_ptr(), 128, &mut result),
                errno::ENOENT
            );
            assert!(result.is_null());
        }
        errno::set_errno(errno::EBADF);
        assert!(getpwent().is_null());
        assert_eq!(
            errno::get_errno(),
            errno::EBADF,
            "the end leaves errno alone"
        );
        setpwent();
        // SAFETY: the library's entry.
        assert_eq!(s(found(getpwent()).pw_name), b"one");
        endpwent();
    }

    #[test]
    fn getgrent_walks_the_groups() {
        with_group(GROUP);
        setgrent();
        let mut names = Vec::new();
        loop {
            let g = getgrent();
            if g.is_null() {
                break;
            }
            // SAFETY: the library's entry.
            names.push(s(unsafe { &*g }.gr_name));
        }
        endgrent();
        let want: [&[u8]; 6] = [b"root", b"wheel", b"users", b"alice", b"staff", b"+"];
        assert_eq!(names, want.map(<[u8]>::to_vec));
    }

    #[test]
    fn a_missing_database_enumerates_the_builtin_root() {
        set_test_text(Which::Passwd, None);
        setpwent();
        // SAFETY: the library's entry.
        assert_eq!(s(found(getpwent()).pw_name), b"root");
        assert!(getpwent().is_null());
        endpwent();
    }

    // -- getgrouplist --

    fn grouplist(user: &core::ffi::CStr, group: GidT, room: i32) -> (i32, i32, Vec<GidT>) {
        let mut groups = std::vec![GidT::MAX; usize::try_from(room.max(0)).unwrap()];
        let mut n = room;
        let r = getgrouplist(user.as_ptr().cast(), group, groups.as_mut_ptr(), &mut n);
        (r, n, groups)
    }

    #[test]
    fn getgrouplist_is_the_primary_then_each_group_naming_the_user() {
        with_group(GROUP);
        let (r, n, groups) = grouplist(c"alice", 1000, 8);
        assert_eq!((r, n), (4, 4));
        assert_eq!(
            groups[..4],
            [1000, 10, 100, 50],
            "file order; `alice` (1000) not twice"
        );
        let (r, _, groups) = grouplist(c"carol", 7, 8);
        assert_eq!((r, groups[..2].to_vec()), (2, std::vec![7, 100]));
    }

    #[test]
    fn getgrouplist_too_small_is_minus_one_with_the_count() {
        with_group(GROUP);
        let (r, n, groups) = grouplist(c"alice", 1000, 2);
        assert_eq!((r, n), (-1, 4));
        assert_eq!(groups, [1000, 10], "the first ones that fit");
        // The count-query idiom: no room at all, and a NULL vector.
        let mut n = 0;
        assert_eq!(
            getgrouplist(
                c"alice".as_ptr().cast(),
                1000,
                core::ptr::null_mut(),
                &mut n
            ),
            -1
        );
        assert_eq!(n, 4);
    }

    #[test]
    fn getgrouplist_null_arguments() {
        with_group(GROUP);
        let mut groups = [0; 4];
        errno::set_errno(0);
        assert_eq!(
            getgrouplist(
                c"alice".as_ptr().cast(),
                1,
                groups.as_mut_ptr(),
                core::ptr::null_mut()
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let mut n = 4;
        assert_eq!(
            getgrouplist(core::ptr::null(), 9, groups.as_mut_ptr(), &mut n),
            1,
            "in no group"
        );
        assert_eq!(groups[0], 9);
        let mut n = 4;
        assert_eq!(
            getgrouplist(c"alice".as_ptr().cast(), 1, core::ptr::null_mut(), &mut n),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- initgroups --

    /// `initgroups` installs [`getgrouplist`]'s list: the primary group, then
    /// each group naming the user, in the file's order.
    #[test]
    fn initgroups_installs_the_users_groups() {
        with_group(GROUP);
        let mut seen: Vec<Vec<GidT>> = Vec::new();
        let r = initgroups_with(c"alice".as_ptr().cast(), 1000, |g| {
            seen.push(g.to_vec());
            0
        });
        assert_eq!(r, 0);
        assert_eq!(seen, [std::vec![1000, 10, 100, 50]]);
        // No group file: the primary alone.
        set_test_text(Which::Group, None);
        seen.clear();
        assert_eq!(
            initgroups_with(c"alice".as_ptr().cast(), 7, |g| {
                seen.push(g.to_vec());
                0
            }),
            0
        );
        assert_eq!(seen, [std::vec![7]]);
    }

    /// glibc's loop: a list the kernel calls too long is tried one group
    /// shorter, down to one; any other refusal is the answer at once.
    #[test]
    fn initgroups_shortens_a_list_the_kernel_calls_too_long() {
        with_group(GROUP);
        let mut lens = Vec::new();
        let r = initgroups_with(c"alice".as_ptr().cast(), 1000, |g| {
            lens.push(g.len());
            if g.len() > 2 {
                errno::set_errno(errno::EINVAL);
                -1
            } else {
                0
            }
        });
        assert_eq!((r, lens.as_slice()), (0, [4, 3, 2].as_slice()));
        lens.clear();
        let r = initgroups_with(c"alice".as_ptr().cast(), 1000, |g| {
            lens.push(g.len());
            errno::set_errno(errno::EINVAL);
            -1
        });
        assert_eq!((r, errno::get_errno()), (-1, errno::EINVAL));
        assert_eq!(lens, [4, 3, 2, 1], "never an empty list");
        lens.clear();
        let r = initgroups_with(c"alice".as_ptr().cast(), 1000, |g| {
            lens.push(g.len());
            errno::set_errno(errno::EPERM);
            -1
        });
        assert_eq!((r, errno::get_errno(), lens.len()), (-1, errno::EPERM, 1));
    }

    /// The real call reaches `setgroups` -- on the host that is its `ENOSYS`,
    /// or `EPERM` while another test holds `CAP_SETGID` dropped -- where it
    /// used to return 0 having done nothing.
    #[test]
    fn initgroups_is_no_longer_a_silent_success() {
        with_group(GROUP);
        errno::set_errno(0);
        assert_eq!(initgroups(c"alice".as_ptr().cast(), 1000), -1);
        let e = errno::get_errno();
        assert!(e == errno::ENOSYS || e == errno::EPERM, "errno {e}");
    }

    #[test]
    fn getgrouplist_with_no_group_file_is_the_primary_alone() {
        set_test_text(Which::Group, None);
        let (r, n, groups) = grouplist(c"root", 0, 4);
        assert_eq!((r, n, groups[0]), (1, 1, 0));
    }

    // -- the rest --

    #[test]
    fn getlogin_returns_root() {
        assert_eq!(s(getlogin()), b"root");
        let mut buf = [0xAAu8; 8];
        assert_eq!(getlogin_r(buf.as_mut_ptr(), 8), 0);
        assert_eq!(&buf[..5], b"root\0");
        assert_eq!(getlogin_r(buf.as_mut_ptr(), 4), -1);
        assert_eq!(errno::get_errno(), errno::ERANGE);
        assert_eq!(getlogin_r(core::ptr::null_mut(), 8), -1);
    }

    #[test]
    fn passwd_and_group_are_the_c_structures() {
        assert_eq!(size_of::<Passwd>(), 48);
        assert_eq!(size_of::<Group>(), 32);
    }

    #[test]
    fn an_unreadable_database_is_its_error() {
        crate::nss_files::set_test_error(Which::Passwd, errno::EIO);
        // SAFETY: a NUL-terminated name.
        assert!(unsafe { getpwnam(c"root".as_ptr().cast()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EIO);
        let mut pwd = Passwd::EMPTY;
        let mut buf = [0u8; 64];
        let mut result: *const Passwd = core::ptr::null();
        // SAFETY: the caller's objects.
        assert_eq!(
            unsafe { getpwuid_r(0, &mut pwd, buf.as_mut_ptr(), 64, &mut result) },
            errno::EIO
        );
        setpwent();
        // SAFETY: as above.
        assert_eq!(
            unsafe { getpwent_r(&mut pwd, buf.as_mut_ptr(), 64, &mut result) },
            errno::ENOENT
        );
        endpwent();
        set_test_text(Which::Passwd, None);
    }

    #[test]
    fn getgrouplist_goes_on_without_an_unreadable_group_file() {
        crate::nss_files::set_test_error(Which::Group, errno::EACCES);
        let (r, n, groups) = grouplist(c"alice", 1000, 4);
        assert_eq!((r, n, groups[0]), (1, 1, 1000));
        set_test_text(Which::Group, None);
    }
}
