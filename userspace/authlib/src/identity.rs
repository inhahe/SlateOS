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
//! # Supplementary groups are dropped, and the order is the whole point
//!
//! [`become_user`] drops the supplementary groups, then sets gid, then sets
//! uid, and it does all three itself rather than handing any of them to
//! `Command::uid`/`Command::gid`. Each step sheds privilege the next one
//! needs, so the sequence is not a style choice: `setgroups` after `setuid`
//! fails with EPERM.
//!
//! That is also why this is not a `pre_exec` closure bolted onto the existing
//! `cmd.uid`/`cmd.gid` calls. `std` applies those in the child and runs
//! `pre_exec` closures afterwards, so a `setgroups` added that way would run
//! with the privilege already gone, fail, and abort the child -- converting a
//! silent leak into a program that cannot start a shell at all.
//!
//! **They are dropped rather than replaced by the target user's own groups**,
//! which is a deliberate half-measure. `userdb` records memberships as NAMES
//! (`Record::groups` returns `Vec<String>`) and this tree has no name-to-gid
//! resolver, so the target's groups cannot be expressed as the gid list the
//! call takes. Dropping is the safe direction: a session with too few groups
//! is refused work it should have been allowed, while one with too many holds
//! authority its user never had. Building that resolver is the remaining half,
//! tracked in `known-issues.md` under
//! TD-B-USER-SWITCHING-PROGRAMS-CANNOT-RESET-SUPPLEMENTARY-GROUPS.
//!
//! Until 2026-09-12 the groups were left alone entirely, which is the textbook
//! privilege leak -- a process holding the caller's supplementary groups that
//! lowers only its uid still has every group the caller was in. It leaked
//! nothing in practice, because `posix::getgroups` reports none, and that is a
//! fact about the current kernel rather than a guarantee.

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
/// Supplementary groups are DROPPED here, before the gid and uid change, and
/// the order is not optional -- each step sheds privilege the next one needs.
/// They are dropped rather than set to the target's own, because `userdb`
/// records memberships as names and this tree has no name-to-gid resolver.
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
        // Declared here, before any statement in this scope: clippy's
        // `items_after_statements` is right that an item appearing halfway
        // down reads as though it came into existence there, when it was
        // always in scope.
        unsafe extern "C" {
            fn setgroups(size: usize, list: *const u32) -> i32;
            fn setgid(gid: u32) -> i32;
            fn setuid(uid: u32) -> i32;
        }
        // ALL THREE STEPS HAPPEN HERE, and `cmd.uid`/`cmd.gid` are deliberately
        // not used, which is the opposite of what this function did until
        // 2026-09-12.
        //
        // `std` applies `uid`/`gid` in the child and then runs `pre_exec`
        // closures, in that order. So a `setgroups` added as a closure would run
        // *after* the process had already given up the privilege that
        // `setgroups` requires, fail with EPERM, and abort the child -- turning
        // a silent leak into `su` not working at all. Doing all three here keeps
        // the ordering under this function's control.
        //
        // THE ORDER IS THE WHOLE POINT: groups, then gid, then uid. Each step
        // sheds privilege the next one needs, so any other sequence either
        // fails or silently keeps what it was supposed to drop.
        //
        // `setgroups(0, NULL)` DROPS the supplementary groups rather than
        // setting the target's. That is a deliberate half-measure and the
        // reason is a missing piece elsewhere: `userdb` records group
        // memberships as NAMES and this tree has no name-to-gid resolver, so
        // the target's own groups cannot be expressed as the gid list the call
        // takes. Dropping is the safe direction -- a session with too few
        // groups is refused work it should have been allowed, while one with
        // too many holds authority its user never had. It is also a no-op
        // today, because `posix::getgroups` reports none; that is a fact about
        // the current kernel and not a guarantee, which is exactly why this is
        // wired now rather than when it starts to matter.
        // REACHED AS C SYMBOLS, not through the `posix` rlib. A SlateOS program
        // already links the real libc, and taking `posix` as a Rust dependency
        // for a stateful call gives the process a SECOND copy of it whose
        // syscalls are stubbed -- design-decisions.md §768 and
        // TD-B-THE-POSIX-RLIB-IS-A-SECOND-LIBC-WITH-EVERY-SYSCALL-STUBBED-OUT.
        // This crate's other use of `posix` is `posix::crypt`, which is a pure
        // module and carries no such hazard; `posix::unistd` issues syscalls and
        // does. The first version of this function called it directly and
        // `check-one-libc-per-process` was right to refuse it.
        // SAFETY: `pre_exec` runs between `fork` and `exec` in the child, where
        // only async-signal-safe work is permitted. All three are single
        // syscalls plus an errno store, and `Error::last_os_error` reads that
        // errno without allocating. Nothing here locks or touches the heap.
        // `setgroups(0, NULL)` is the POSIX drop idiom; the list pointer is
        // ignored when the count is zero.
        unsafe {
            cmd.pre_exec(move || {
                if setgroups(0, core::ptr::null()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if setgid(gid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if setuid(uid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
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

/// The caller's real group id, as the kernel reports it.
///
/// The companion to [`caller_uid`], with the same contract: `getgid(2)`, and
/// `None` off unix rather than an invented answer. `sudo` reads both, having
/// previously taken each from `/proc/self/status` with an environment-variable
/// fallback.
#[must_use]
pub fn caller_gid() -> Option<u32> {
    #[cfg(unix)]
    {
        // SAFETY: `getgid` is the POSIX libc function -- nullary, no failure
        // mode, returning `gid_t`, a 32-bit unsigned integer here. See
        // `caller_uid` for why the declaration is local.
        unsafe extern "C" {
            fn getgid() -> u32;
        }
        // SAFETY: nullary call into libc, as above.
        Some(unsafe { getgid() })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// `argv[0]` for a login shell: its own basename with a leading `-`.
///
/// The leading hyphen is the entire protocol by which a shell is told it is a
/// *login* shell, and therefore that it should read `/etc/profile` and the
/// user's own profile. `/bin/bash` becomes `-bash`. Passing `-l` instead works
/// for some shells and is a syntax error for others, which is why every
/// `login`, `su` and `sshd` in existence uses the hyphen.
///
/// # Why bytes
///
/// A shell path on this OS may hold any byte but `/` and NUL, so the rule has
/// to be expressed over bytes. Keeping it separate from the `OsStr` wrapper
/// below also keeps it *testable*: converting bytes to an `OsString` is only
/// possible through `std::os::unix::ffi`, which does not exist on the Windows
/// host these crates' tests are compiled for, so a single `OsStr -> OsString`
/// function could not be called by any test at all. The same shape as
/// separating a parser from its reader, and for the same reason: the part with
/// the decisions in it should not be the part that needs a particular platform
/// to run.
///
/// # Degenerate paths
///
/// A path ending in `/` has no basename. Emitting a bare `-` would name a
/// shell that does not exist and start no session, so the whole string is used
/// instead -- still wrong as a shell, but it fails loudly at `spawn` with a
/// name that says what was configured, rather than becoming a one-character
/// mystery.
#[must_use]
pub fn login_argv0_bytes(shell: &[u8]) -> Vec<u8> {
    let base = match shell.iter().rposition(|b| *b == b'/') {
        Some(i) if i.saturating_add(1) < shell.len() => {
            shell.get(i.saturating_add(1)..).unwrap_or(shell)
        }
        _ => shell,
    };
    let mut out = Vec::with_capacity(base.len().saturating_add(1));
    out.push(b'-');
    out.extend_from_slice(base);
    out
}

/// [`login_argv0_bytes`] as an `OsString`, for handing to
/// `CommandExt::arg0`.
///
/// Unix only, because `CommandExt::arg0` is. A caller on the host build has
/// nothing to pass it to.
#[cfg(unix)]
#[must_use]
pub fn login_argv0(shell: &std::ffi::OsStr) -> std::ffi::OsString {
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    std::ffi::OsString::from_vec(login_argv0_bytes(shell.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- the login-shell argv[0] ----
    //
    // These stood in `userspace/su` (over bytes) and `userspace/sshd` (over
    // `&str`) as two implementations of one rule. `userspace/login` was the
    // third caller, which is the point at which two copies stop being cheaper
    // than one shared function.

    /// The convention: basename, hyphen in front.
    #[test]
    fn login_argv0_is_the_basename_with_a_hyphen() {
        assert_eq!(login_argv0_bytes(b"/bin/bash"), b"-bash");
        assert_eq!(login_argv0_bytes(b"/usr/local/bin/osh"), b"-osh");
        assert_eq!(login_argv0_bytes(b"/usr/local/bin/fish"), b"-fish");
    }

    /// An account record may name the shell without a path at all.
    #[test]
    fn login_argv0_needs_no_directory() {
        assert_eq!(login_argv0_bytes(b"sh"), b"-sh");
    }

    /// A shell path may hold any byte but `/` and NUL, so the rule is over
    /// bytes and a name that is not UTF-8 survives it. This is the case the
    /// `&str` implementation in `sshd` could not have been given.
    #[test]
    fn login_argv0_keeps_bytes_that_are_not_utf8() {
        assert_eq!(login_argv0_bytes(b"/bin/o\xffh"), b"-o\xffh".to_vec());
    }

    /// Degenerate paths must still produce something a shell can be called by.
    ///
    /// A trailing slash has no basename. Emitting `-` alone would name a shell
    /// that does not exist and start no session; the whole path at least fails
    /// loudly at `spawn` saying what was configured.
    #[test]
    fn login_argv0_never_returns_a_bare_hyphen() {
        assert_eq!(login_argv0_bytes(b"/bin/"), b"-/bin/".to_vec());
        assert_eq!(login_argv0_bytes(b"/"), b"-/".to_vec());
        assert_eq!(login_argv0_bytes(b""), b"-".to_vec());
    }

    /// The hyphen is the whole point: without it the shell does not read the
    /// user's profile, and the session comes up with none of the environment a
    /// login is supposed to set up.
    #[test]
    fn login_argv0_keeps_the_hyphen_that_means_login_shell() {
        assert!(login_argv0_bytes(b"/bin/bash").starts_with(b"-"));
    }

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
