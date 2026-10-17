//! `chattr` and `lsattr`: change and list a file's attributes.
//!
//! e2fsprogs 1.47.0's `misc/chattr.c` and `misc/lsattr.c`, ported, as Ubuntu
//! 24.04 ships them, with the parts of `lib/e2p` and `lib/et` they use.
//!
//! One binary, run as either by the name it is called (the image installs
//! `lsattr` as a second name of `chattr`); each behaves as upstream's
//! program of that name, its messages naming it as `argv[0]` has it.
//!
//! The attributes are the inode's flags, read and set with the
//! `FS_IOC_GETFLAGS`/`FS_IOC_SETFLAGS` ioctls (`libcall::inode`), and its
//! generation and project with four more. On SlateOS none of them reaches
//! the kernel yet: the C library answers `ENOTTY` to them, which e2fsprogs
//! reports as `Operation not supported`, so both programs refuse every file
//! there until it forwards them
//! (`requests/b-ad-chattr-s-ioctls-do-not-reach-the-kernel.md`). The kernel
//! keeps two flags, immutable and append-only, for Linux-ABI programs today.

stdfdguard::guard_std_fds!();

#[cfg(unix)]
mod chattr;
#[cfg(unix)]
mod e2p;
#[cfg(unix)]
mod lsattr;

/// e2fsprogs' version and date, as `-V` prints them (`version.h`).
#[cfg(unix)]
const E2FSPROGS_VERSION: &str = "1.47.0";
#[cfg(unix)]
const E2FSPROGS_DATE: &str = "5-Feb-2023";

#[cfg(unix)]
fn main() {
    use std::os::unix::ffi::OsStringExt;
    // Before anything else: a standard descriptor the program was started
    // without is closed again, and SIGPIPE given back the disposition it
    // came with.
    stdfdguard::restore();
    let argv: Vec<Vec<u8>> = std::env::args_os().map(OsStringExt::into_vec).collect();
    let name = argv.first().map(|a| {
        let base = a.rsplit(|&b| b == b'/').next().unwrap_or(a);
        base.to_vec()
    });
    let code = if name.as_deref() == Some(b"lsattr") {
        lsattr::main(&argv)
    } else {
        chattr::main(&argv)
    };
    std::process::exit(code);
}

#[cfg(not(unix))]
fn main() {
    eprintln!("chattr: runs only on a Unix host");
    std::process::exit(1);
}
