//! The real system, through the linked C library and std.
//!
//! Files go through std, which makes the same system calls procmail does
//! (`open` with `O_CREAT|O_EXCL` and a mode, `link`, `unlink`, `lstat`,
//! `fstat`). The calls std has no spelling for -- the ids, `signal`, `sleep`
//! and `uname` -- are the C library's own, declared here.

use crate::{Stat, System};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGPIPE: i32 = 13;
const SIGTERM: i32 = 15;
const SIG_IGN: usize = 1;

/// The `struct utsname` fields: six of 65 bytes, glibc's and SlateOS's alike.
const UTSNAME_LEN: usize = 65;
const UTSNAME_FIELDS: usize = 6;

unsafe extern "C" {
    fn getuid() -> u32;
    fn geteuid() -> u32;
    fn setuid(uid: u32) -> i32;
    fn getgid() -> u32;
    fn getegid() -> u32;
    fn setgid(gid: u32) -> i32;
    fn signal(signum: i32, handler: usize) -> usize;
    fn sleep(seconds: u32) -> u32;
    fn uname(buf: *mut u8) -> i32;
}

/// procmail's `exitflag`, set from a signal handler.
static EXITFLAG: AtomicI32 = AtomicI32::new(0);
/// mcommon.c's `gotsig`: a signal that arrived while `qsignal` was deciding.
static GOTSIG: AtomicI32 = AtomicI32::new(0);

/// lockfile.c `failure`: "merely sets a flag". An atomic store is
/// async-signal-safe.
extern "C" fn failure(_signum: i32) {
    EXITFLAG.store(2, Ordering::SeqCst);
}

/// mcommon.c `fakehandler`.
extern "C" fn fakehandler(_signum: i32) {
    GOTSIG.store(1, Ordering::SeqCst);
}

/// mcommon.c `qsignal`: catch `signum` with `failure` -- unless it was being
/// ignored when lockfile started, in which case it stays ignored. A signal
/// arriving between the two calls is caught by `fakehandler` and acted on.
fn qsignal(signum: i32) {
    // `signal` takes the handler as an address; a function pointer is that.
    let fake: extern "C" fn(i32) = fakehandler;
    let real: extern "C" fn(i32) = failure;
    GOTSIG.store(0, Ordering::SeqCst);
    // SAFETY: `signal` installs a handler for a valid signal number; the
    // handler is an `extern "C" fn(i32)` that only stores to an atomic, which
    // is async-signal-safe, and lives for the whole program.
    let previous = unsafe { signal(signum, fake as usize) };
    if previous == SIG_IGN {
        // SAFETY: restoring SIG_IGN for a valid signal number.
        unsafe { signal(signum, SIG_IGN) };
    } else {
        // SAFETY: as above, with `failure`, which also only stores an atomic.
        unsafe { signal(signum, real as usize) };
        if GOTSIG.load(Ordering::SeqCst) != 0 {
            failure(signum);
        }
    }
}

fn path(bytes: &[u8]) -> &OsStr {
    OsStr::from_bytes(bytes)
}

fn errno(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(crate::EIO)
}

fn to_stat(m: &fs::Metadata) -> Stat {
    Stat {
        dev: m.dev(),
        ino: m.ino(),
        uid: m.uid(),
        gid: m.gid(),
        nlink: m.nlink(),
        size: m.size(),
        mtime: m.mtime(),
        mode: m.mode(),
    }
}

/// The running system.
pub(crate) struct Unix {
    users: Option<pwdb::Db>,
}

impl Unix {
    pub(crate) fn new() -> Self {
        Self { users: None }
    }

    fn users(&mut self) -> &pwdb::Db {
        self.users.get_or_insert_with(pwdb::Db::load)
    }
}

impl System for Unix {
    type File = File;

    fn elog(&mut self, bytes: &[u8]) {
        // write(STDERR, ...): unbuffered, and a failure is not lockfile's
        // to report -- there is nowhere left to report it.
        let _ = std::io::stderr().write_all(bytes);
    }

    fn getuid(&mut self) -> u32 {
        // SAFETY: getuid cannot fail and takes no arguments.
        unsafe { getuid() }
    }

    fn geteuid(&mut self) -> u32 {
        // SAFETY: geteuid cannot fail and takes no arguments.
        unsafe { geteuid() }
    }

    fn setuid(&mut self, uid: u32) -> bool {
        // SAFETY: setuid takes a plain integer and reports failure by status.
        unsafe { setuid(uid) == 0 }
    }

    fn getgid(&mut self) -> u32 {
        // SAFETY: getgid cannot fail and takes no arguments.
        unsafe { getgid() }
    }

    fn getegid(&mut self) -> u32 {
        // SAFETY: getegid cannot fail and takes no arguments.
        unsafe { getegid() }
    }

    fn setgid(&mut self, gid: u32) -> bool {
        // SAFETY: setgid takes a plain integer and reports failure by status.
        unsafe { setgid(gid) == 0 }
    }

    fn getpid(&mut self) -> u64 {
        u64::from(std::process::id())
    }

    fn time(&mut self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }

    fn nodename(&mut self) -> Vec<u8> {
        let mut buf = [0u8; UTSNAME_LEN * UTSNAME_FIELDS];
        // SAFETY: `buf` is a `struct utsname`'s size, six fields of
        // UTSNAME_LEN bytes, as glibc and SlateOS's C library define it, and
        // uname writes only within it.
        if unsafe { uname(buf.as_mut_ptr()) } != 0 {
            return Vec::new();
        }
        let field = buf.get(UTSNAME_LEN..UTSNAME_LEN * 2).unwrap_or(&[]);
        crate::cstr(field).to_vec()
    }

    fn logname(&mut self) -> Option<Vec<u8>> {
        std::env::var_os("LOGNAME").map(OsStringExt::into_vec)
    }

    fn user_by_name(&mut self, name: &[u8]) -> Option<(u32, Vec<u8>)> {
        self.users()
            .user_by_name(name)
            .map(|u| (u.uid, u.name.clone()))
    }

    fn user_by_uid(&mut self, uid: u32) -> Option<Vec<u8>> {
        self.users().user_by_uid(uid).map(|u| u.name.clone())
    }

    fn lstat(&mut self, p: &[u8]) -> Result<Stat, i32> {
        fs::symlink_metadata(path(p))
            .map(|m| to_stat(&m))
            .map_err(|e| errno(&e))
    }

    fn stat(&mut self, p: &[u8]) -> Result<Stat, i32> {
        fs::metadata(path(p))
            .map(|m| to_stat(&m))
            .map_err(|e| errno(&e))
    }

    fn create_excl(&mut self, p: &[u8], mode: u32) -> Result<File, i32> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path(p))
            .map_err(|e| errno(&e))
    }

    fn fstat(&mut self, file: &File) -> Result<Stat, i32> {
        file.metadata().map(|m| to_stat(&m)).map_err(|e| errno(&e))
    }

    fn write(&mut self, file: &mut File, bytes: &[u8]) {
        // rwrite(i, "0", 1): procmail does not look at the result either; a
        // lock whose `0` did not land is still a lock.
        let _ = file.write_all(bytes);
    }

    fn close(&mut self, file: File) {
        drop(file);
    }

    fn link(&mut self, old: &[u8], new: &[u8]) -> Result<(), i32> {
        fs::hard_link(path(old), path(new)).map_err(|e| errno(&e))
    }

    fn unlink(&mut self, p: &[u8]) -> Result<(), i32> {
        fs::remove_file(path(p)).map_err(|e| errno(&e))
    }

    fn sleep(&mut self, seconds: u32) {
        // SAFETY: sleep takes a plain integer; it returns early when a caught
        // signal arrives, which is the point of calling it rather than
        // std::thread::sleep, which would sleep on.
        unsafe { sleep(seconds) };
    }

    fn exitflag(&mut self) -> i32 {
        EXITFLAG.load(Ordering::SeqCst)
    }

    fn catch_signals(&mut self) {
        for signum in [SIGHUP, SIGINT, SIGQUIT, SIGTERM] {
            qsignal(signum);
        }
    }

    fn ignore_sigpipe(&mut self) {
        // SAFETY: setting SIG_IGN for a valid signal number.
        unsafe { signal(SIGPIPE, SIG_IGN) };
    }
}
