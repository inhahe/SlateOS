//! Secure Boot's lists, as a native program changes and asks them:
//! `<slateos/secureboot.h>`'s `secureboot_enroll`, `secureboot_remove` and
//! `secureboot_verify`, over lane A's three calls (1082-1084,
//! design-decisions §978).
//!
//! Linux keeps such lists in the firmware's variables, which `mokutil` and
//! `efi-updatevar` change; here the kernel keeps them (`fs::secureboot`),
//! publishes them in `/proc/secureboot`, and `sbctl` changes them. Enrolling
//! and removing need the right to (`(Process, ENROLL_SECUREBOOT)`); asking
//! needs none. The kernel never parses a certificate: the caller gives each
//! entry's SHA-256 fingerprint, `SHA256:` and 64 hex digits, or the digits
//! alone.

use crate::errno::{self, native};

/// The `PK` list: the platform key's fingerprint.
pub const SECUREBOOT_PK: i32 = 0;
/// The `KEK` list: key-exchange keys.
pub const SECUREBOOT_KEK: i32 = 1;
/// The `db` list: what may run.
pub const SECUREBOOT_DB: i32 = 2;
/// The `dbx` list: what may not, whatever `db` says.
pub const SECUREBOOT_DBX: i32 = 3;
/// The `MOK` list: the machine owner's keys, which may run.
pub const SECUREBOOT_MOK: i32 = 4;

/// A verdict's `listing`: the image is in `db` or `MOK`.
pub const SECUREBOOT_ALLOWED: u32 = 0;
/// ... in `dbx`.
pub const SECUREBOOT_FORBIDDEN: u32 = 1;
/// ... in neither.
pub const SECUREBOOT_UNLISTED: u32 = 2;

/// What `secureboot_verify` says of an image: its `listing`
/// (`SECUREBOOT_ALLOWED`, `_FORBIDDEN` or `_UNLISTED`), the id of the `entry`
/// that decided it (0 when unlisted), whether the state `enforced`, and
/// whether the image `may_run` -- each 0 or 1. The kernel's four
/// little-endian words, in their order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SecurebootVerdict {
    /// Which list the image is in.
    pub listing: u32,
    /// The deciding entry's id; 0 when unlisted.
    pub entry: u32,
    /// 1 when the state enforces.
    pub enforced: u32,
    /// 1 when the image may run.
    pub may_run: u32,
}

/// The errno for a refusal by the Secure Boot calls: `EPERM` without the
/// right to change the lists, `ENOSPC` for a full table, `EEXIST` for an
/// entry of the same list and fingerprint, `ENOENT` for an id that names
/// none, `EINVAL` for a list, subject or fingerprint that is not one,
/// `EFAULT` for an output that cannot be written; `ENOSYS` from a kernel
/// without the calls.
fn refusal(ret: i64) -> i32 {
    match ret {
        native::PERMISSION_DENIED => errno::EPERM,
        native::RESOURCE_EXHAUSTED => errno::ENOSPC,
        _ if crate::syscall::lacks_call(ret) => errno::ENOSYS,
        _ => errno::errno_for(ret),
    }
}

/// Enrol an entry in list `list` (`SECUREBOOT_PK` .. `SECUREBOOT_MOK`):
/// `subject`, its UTF-8 name for people, 1 to 256 bytes, and `fingerprint`,
/// its SHA-256. The new entry's id, or -1 with `errno` set ([`refusal`]).
///
/// # Safety
///
/// `subject` and `fingerprint` must be readable for their lengths.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn secureboot_enroll(
    list: i32,
    subject: *const u8,
    subject_len: usize,
    fingerprint: *const u8,
    fingerprint_len: usize,
) -> i64 {
    let ret = crate::syscall::syscall5(
        crate::syscall::SYS_SECUREBOOT_ENROLL,
        u64::from(list.cast_unsigned()),
        subject as u64,
        subject_len as u64,
        fingerprint as u64,
        fingerprint_len as u64,
    );
    if ret < 0 {
        errno::set_errno(refusal(ret));
        return -1;
    }
    ret
}

/// Remove the entry with id `id`. 0, or -1 with `errno` set ([`refusal`]).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn secureboot_remove(id: u64) -> i32 {
    let ret = crate::syscall::syscall1(crate::syscall::SYS_SECUREBOOT_REMOVE, id);
    if ret < 0 {
        errno::set_errno(refusal(ret));
        return -1;
    }
    0
}

/// Whether the image `name` -- 0 to 256 bytes of any value, a path or not,
/// recorded with the verdict -- whose SHA-256 is `hash` may run: the
/// verdict in `out`. 0, or -1 with `errno` set ([`refusal`]).
///
/// # Safety
///
/// `name` and `hash` must be readable for their lengths, and `out` writable
/// for a [`SecurebootVerdict`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn secureboot_verify(
    name: *const u8,
    name_len: usize,
    hash: *const u8,
    hash_len: usize,
    out: *mut SecurebootVerdict,
) -> i32 {
    // Before the kernel is asked, which records each verdict it gives.
    if out.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let mut words = [0u8; 16];
    let ret = crate::syscall::syscall5(
        crate::syscall::SYS_SECUREBOOT_VERIFY,
        name as u64,
        name_len as u64,
        hash as u64,
        hash_len as u64,
        words.as_mut_ptr() as u64,
    );
    if ret < 0 {
        errno::set_errno(refusal(ret));
        return -1;
    }
    // SAFETY: the caller's contract; non-null, checked above.
    unsafe { out.write_unaligned(verdict_of(&words)) };
    0
}

/// The verdict in the kernel's record: four little-endian `u32`s.
fn verdict_of(words: &[u8; 16]) -> SecurebootVerdict {
    let word = |i: usize| {
        words
            .get(i..i.saturating_add(4))
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0, u32::from_le_bytes)
    };
    SecurebootVerdict {
        listing: word(0),
        entry: word(4),
        enforced: word(8),
        may_run: word(12),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kernel's record, word by word, in its order.
    #[test]
    fn a_verdict_is_the_kernels_four_words() {
        let mut words = [0u8; 16];
        words[..4].copy_from_slice(&1u32.to_le_bytes());
        words[4..8].copy_from_slice(&77u32.to_le_bytes());
        words[12..].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            verdict_of(&words),
            SecurebootVerdict {
                listing: SECUREBOOT_FORBIDDEN,
                entry: 77,
                enforced: 0,
                may_run: 1,
            }
        );
    }

    /// The refusals a program can act on are told apart.
    #[test]
    fn the_refusals_are_told_apart() {
        assert_eq!(refusal(native::PERMISSION_DENIED), errno::EPERM);
        assert_eq!(refusal(native::RESOURCE_EXHAUSTED), errno::ENOSPC);
        assert_eq!(refusal(native::ALREADY_EXISTS), errno::EEXIST);
        assert_eq!(refusal(native::NOT_FOUND), errno::ENOENT);
        assert_eq!(refusal(native::INVALID_ARGUMENT), errno::EINVAL);
        assert_eq!(refusal(native::INVALID_ADDRESS), errno::EFAULT);
    }

    /// On the host there is no kernel: each call says so.
    #[test]
    fn without_the_calls_each_is_enosys() {
        let fp = b"SHA256:00";
        let mut out = SecurebootVerdict::default();
        errno::set_errno(0);
        // SAFETY: the slices are live for their lengths.
        assert_eq!(
            unsafe { secureboot_enroll(SECUREBOOT_DB, b"x".as_ptr(), 1, fp.as_ptr(), fp.len()) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        assert_eq!(secureboot_remove(1), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        // SAFETY: as above, and `out` is a live verdict.
        assert_eq!(
            unsafe { secureboot_verify(b"a".as_ptr(), 1, fp.as_ptr(), fp.len(), &raw mut out) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }
}
