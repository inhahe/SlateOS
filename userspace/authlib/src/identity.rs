//! Assuming the identity this crate has just verified.
//!
//! # Why this is in `authlib` and not in each program
//!
//! Five programs authenticate a user and then run something as them: `su`,
//! `doas`, `sudo`, `sshd` and `login`. Until 2026-09-10 none of them changed
//! any credential the operating system checks -- they set `HOME`, `USER` and
//! `SHELL` and ran the target's shell under *the caller's* identity.
//!
//! The two lines that fix that are short enough to copy into all five, and
//! copying them would be the wrong call, because **the code is not the thing
//! being shared -- the future edit is.** Exactly one piece of the drop is
//! missing (see below), and when the kernel grows the syscall it needs adding
//! in one place, not in five places one of which will be missed. That is the
//! same argument `authlib` itself was created on: three programs each
//! implemented the hash format separately and disagreed
//! (`design-decisions.md` §329).
//!
//! # The order is required, not chosen
//!
//! Supplementary groups, then gid, then uid. Each step drops the privilege the
//! previous one needed: lowering the uid first leaves the process unable to
//! lower its gid, so it ends up in the target's user identity and the caller's
//! group -- a half-drop that looks like a drop. `std` performs them in this
//! order in the child between fork and exec, which is why this is expressed as
//! settings on a [`Command`] rather than as calls made here.
//!
//! # Supplementary groups are not reset, and that is not an oversight
//!
//! [`become_user`] sets gid and uid and deliberately does not ask for groups.
//! `posix::setgroups` returns `ENOSYS` on purpose -- the kernel implements it
//! only in the Linux-ABI table and `posix/src/syscall.rs` has no native number
//! for native libc to call, filed as
//! `requests/b-a-no-syscall-sets-supplementary-groups-changes-root-or-changes-directory.md`.
//! `std` would call it in the child, get `ENOSYS`, and abort before exec, so
//! requesting it does not produce a more complete drop; it produces a program
//! that cannot start a shell at all.
//!
//! Leaving them is the textbook privilege leak: a process holding the caller's
//! supplementary groups that lowers only its uid still has every group the
//! caller was in. It leaks nothing *today*, because `posix::getgroups` reports
//! none and there is nothing to retain -- a fact about the current kernel and
//! not a guarantee. Tracked in `known-issues.md` as
//! TD-B-USER-SWITCHING-PROGRAMS-CANNOT-RESET-SUPPLEMENTARY-GROUPS. **When that
//! request is answered, this file is the one that changes.**

use std::process::Command;

/// Arrange for `cmd` to run as `uid`:`gid`.
///
/// The credentials are applied by the child between fork and exec, so this
/// only records the intent on the [`Command`]; nothing about the calling
/// process changes. That matters for the caller's own error reporting -- a
/// program that has called this can still write to its own stderr afterwards.
///
/// Callers should resolve both numbers from the account record *before*
/// building the command, and refuse if the record cannot name a uid: a session
/// whose owner cannot be named must not start. Passing a sentinel such as
/// `u32::MAX` here would start a shell owned by an identity belonging to
/// nobody.
///
/// Supplementary groups are not reset. See the module documentation; that is
/// the whole of what is missing, and it is missing everywhere at once rather
/// than per-caller, which is why this function exists.
///
/// # Platforms
///
/// A no-op off unix, because `CommandExt` does not exist there. The real
/// target is unix; the host build exists to run the test suite, and the only
/// alternative -- making every caller write `#[cfg(unix)]` around the call --
/// puts the platform question in five places to save it in one.
pub fn become_user(cmd: &mut Command, uid: u32, gid: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // gid first: see the module doc. `std` will apply them in the child.
        cmd.gid(gid);
        cmd.uid(uid);
    }
    #[cfg(not(unix))]
    {
        // The host build compiles this arm. Consuming the arguments keeps the
        // signature honest without an `_` prefix, which would tell the next
        // reader they are unused on the target, where they are the point.
        let _ = (cmd, uid, gid);
    }
}
