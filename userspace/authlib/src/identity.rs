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

/// Who is asking, as the kernel reports it.
///
/// `None` means this build cannot tell. **It does not mean root**, and a caller
/// that treats it as root has reintroduced the bug this function exists to
/// remove.
///
/// # The bug this replaces
///
/// Six programs decided whether their caller was root by reading the `UID`
/// *environment variable*:
///
/// ```ignore
/// fn current_uid() -> u32 {
///     env::var("UID").ok().and_then(|s| s.parse().ok()).unwrap_or(0)
/// }
/// ```
///
/// Two things are wrong with that and they compound. The environment belongs
/// to whoever starts the process, so `UID=0 passwd root` announces itself as
/// root and is believed. And `UID` is a *shell* variable, not an exported one
/// — this tree's own shell says so explicitly: "an inherited `UID=…` in the
/// environment neither wins nor becomes exported"
/// (`userspace/oils/src/interp.rs`). So the variable is normally **absent**,
/// the `unwrap_or(0)` fires, and the answer is root every single time. In
/// `passwd` that made `is_root()` unconditionally true, and the check that
/// stops one user changing another's password never ran at all. The spoof was
/// not even necessary.
///
/// `osh` had already reasoned this out for its own `$UID`, and reached the
/// opposite conclusion from the six: consulting an environment variable "on a
/// system that *can* [supply the credential] would let any parent process
/// redefine `$UID` by exporting a variable, which is precisely the spoofing
/// bash refuses when it ignores an inherited `UID=`". The shell refused what
/// the privileged programs accepted.
///
/// # Where the answer comes from
///
/// `getuid(2)`, via the C library. On SlateOS that is `posix::getuid`, which
/// reads the credential the kernel recorded at spawn; on the development host
/// it is the host's. Neither can be set by the process's parent, which is the
/// entire property being bought.
///
/// The *real* uid and not the effective one: these programs are asking "who is
/// the human running this?", which is what the real uid answers. A setuid
/// helper's effective uid says what it may do, not who asked.
///
/// # Off unix
///
/// `None`. The Windows host build exists to run the test suite and has no
/// answer to this question; inventing one is exactly how the defect above got
/// in. Callers must refuse the privileged path, not take it.
#[must_use]
pub fn caller_uid() -> Option<u32> {
    #[cfg(unix)]
    {
        // SAFETY: `getuid` is the POSIX libc function -- no arguments, no
        // side effects, no failure mode, returning `uid_t`, which is a 32-bit
        // unsigned integer on every platform this builds for. Declared here
        // rather than pulled from a crate for the same reason
        // `userspace/oils` declares it: it is one nullary symbol and the libc
        // it binds to on the real target is our own.
        unsafe extern "C" {
            fn getuid() -> u32;
        }
        // SAFETY: nullary call into libc, as above.
        Some(unsafe { getuid() })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The environment must not be able to answer this question.**
    ///
    /// The regression is exact: a `UID` in the environment used to be the
    /// whole answer. Setting it to something no real system would issue and
    /// requiring that it is not what comes back pins that, on both platforms
    /// -- off unix the answer is `None`, which is also not the sentinel.
    #[test]
    fn a_uid_in_the_environment_is_not_consulted() {
        const SPOOF: u32 = 4_242_424;

        // SAFETY: `set_var` is unsafe because another thread may be reading
        // the environment concurrently. Nothing else in this crate reads
        // `UID` -- that is the property under test -- and the value is a
        // sentinel no other test compares against.
        unsafe {
            std::env::set_var("UID", SPOOF.to_string());
        }
        let got = caller_uid();
        // SAFETY: as above; restoring the environment for anything that runs
        // after this test in the same process.
        unsafe {
            std::env::remove_var("UID");
        }

        assert_ne!(
            got,
            Some(SPOOF),
            "the caller's identity came from the environment, which the caller controls"
        );
    }

    /// Root is a value, absence is not.
    ///
    /// `None` must never be spelled the same as uid 0. The defect being fixed
    /// was precisely an `unwrap_or(0)` collapsing "I do not know" into "root",
    /// so the type has to keep them apart and this asserts that it does.
    #[test]
    fn not_knowing_is_not_the_same_as_being_root() {
        assert_ne!(None, Some(0u32));
        // And on this platform the answer is one of exactly those two shapes:
        // a real uid, or an honest refusal to guess.
        let got = caller_uid();
        #[cfg(unix)]
        assert!(got.is_some(), "unix can always answer getuid()");
        #[cfg(not(unix))]
        assert!(got.is_none(), "off unix there is no answer to invent");
    }
}
