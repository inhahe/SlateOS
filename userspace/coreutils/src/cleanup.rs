//! Files a program must not leave behind if it is killed: GNU `sort`'s
//! temporary files, GNU `csplit`'s pieces. Both programs catch "the usual
//! suspects" -- `SIGALRM`, `SIGHUP`, `SIGINT`, `SIGPIPE`, `SIGQUIT`,
//! `SIGTERM`, `SIGPOLL`, `SIGPROF`, `SIGVTALRM`, `SIGXCPU`, `SIGXFSZ`, each
//! unless it was inherited ignored -- remove their files in the handler, and
//! die of the signal as if it had never been caught. This is that, once.
//!
//! # The list the handler reads
//!
//! A program registers each file as it makes it, in the same critical
//! section -- signals blocked -- so that no signal can arrive between the
//! two and leave the file behind; and forgets it as it removes it. The list
//! is changed only with the caught signals blocked, so the handler, which is
//! the only other reader, never sees a change half made; and these programs
//! run one thread. Around a `fork` the list is emptied (`without`), as GNU
//! `sort`'s `pipe_fork` empties its `temphead`: a child the handler runs in
//! before it `exec`s must not remove its parent's files.
//!
//! What happens at an ordinary exit is the program's choice: `sort` removes
//! whatever is left ([`remove_all`]); `csplit`'s pieces are its output, and
//! stay.

use std::cell::UnsafeCell;
use std::ffi::CString;
use std::path::Path;

use crate::quote::os_bytes;

/// The registered paths.
struct Registry(UnsafeCell<Vec<CString>>);

// SAFETY: see the module documentation. Every access is either inside
// `change`, with the caught signals blocked, or the handler's own read, which
// cannot run while one of those is in progress; and the programs that use
// this run one thread.
unsafe impl Sync for Registry {}

static REGISTRY: Registry = Registry(UnsafeCell::new(Vec::new()));

/// The signals caught -- those of [`SIGNALS`] not inherited ignored.
#[cfg(unix)]
static CAUGHT: std::sync::OnceLock<libcall::signal::SigSet> = std::sync::OnceLock::new();

/// "The usual suspects", in Linux's numbers: `SIGALRM`, `SIGHUP`, `SIGINT`,
/// `SIGPIPE`, `SIGQUIT`, `SIGTERM`, `SIGPOLL`, `SIGPROF`, `SIGVTALRM`,
/// `SIGXCPU`, `SIGXFSZ`.
#[cfg(unix)]
const SIGNALS: [i32; 11] = [14, 1, 2, 13, 3, 15, 29, 27, 26, 24, 25];

/// Catch the usual suspects, unless they were inherited ignored, with a
/// handler that removes every registered file and then dies of the signal.
/// Call it once, before the first file is made -- after the inherited
/// dispositions are back (`stdfd::restore`), so that one the parent ignored
/// stays ignored.
pub fn install() {
    #[cfg(unix)]
    {
        let mut caught = libcall::signal::SigSet::empty();
        for sig in SIGNALS {
            if !libcall::signal::is_ignored(sig).unwrap_or(true) {
                // Unchecked: every number here is a signal.
                let _ = caught.add(sig);
            }
        }
        for sig in SIGNALS {
            if caught.contains(sig) {
                // Unchecked, as upstream's `sigaction` calls are.
                let _ = libcall::signal::set_handler_masked(sig, handler, &caught);
            }
        }
        let _ = CAUGHT.set(caught);
    }
}

/// Run `f` on the list with the caught signals blocked -- upstream's
/// `cs_enter`/`cs_leave`.
pub fn change<T>(f: impl FnOnce(&mut Vec<CString>) -> T) -> T {
    #[cfg(unix)]
    let before = CAUGHT
        .get()
        .and_then(|set| libcall::signal::block(set).ok());
    // SAFETY: the caught signals are blocked, so the handler cannot read the
    // list while it changes, and no other thread exists.
    let result = f(unsafe { &mut *REGISTRY.0.get() });
    #[cfg(unix)]
    if let (Some(set), Some(before)) = (CAUGHT.get(), before) {
        // Unblock only what this blocked: a signal blocked before stays so.
        let mut newly = libcall::signal::SigSet::empty();
        for sig in SIGNALS {
            if set.contains(sig) && !before.contains(sig) {
                let _ = newly.add(sig);
            }
        }
        // Unchecked: unblocking a valid set cannot fail.
        let _ = libcall::signal::unblock(&newly);
    }
    result
}

/// `path` as the registry holds it; a path with a NUL byte in it cannot be
/// made, and so is never registered.
fn c_path(path: &Path) -> Option<CString> {
    CString::new(os_bytes(path.as_os_str()).into_owned()).ok()
}

/// Register `path` in a list already being changed -- for the caller that
/// makes the file inside [`change`], as it should.
pub fn add_to(list: &mut Vec<CString>, path: &Path) {
    if let Some(c) = c_path(path) {
        list.push(c);
    }
}

/// Remove `path` and forget it, in one critical section -- `sort`'s
/// `zaptemp`, `csplit`'s unlink of an elided piece.
///
/// # Errors
///
/// What `unlink` said; the path is forgotten either way.
pub fn remove(path: &Path) -> std::io::Result<()> {
    let c = c_path(path);
    change(|list| {
        let result = std::fs::remove_file(path);
        if let Some(c) = &c {
            list.retain(|p| p != c);
        }
        result
    })
}

/// Remove every registered file and forget them all: `sort`'s
/// `exit_cleanup`.
pub fn remove_all() {
    change(|list| {
        for path in list.iter() {
            sys::unlink(path);
        }
        list.clear();
    });
}

/// Run `spawn` with the list emptied, and put it back after: a child the
/// handler runs in before it `exec`s must not remove its parent's files.
pub fn without<T>(spawn: impl FnOnce() -> T) -> T {
    let saved = change(std::mem::take);
    let result = spawn();
    change(|list| {
        let mut saved = saved;
        saved.append(list);
        *list = saved;
    });
    result
}

/// The handler: remove every registered file, then die of the signal as if
/// it had never been caught -- upstream's `sighandler` and
/// `interrupt_handler`.
#[cfg(unix)]
extern "C" fn handler(sig: i32) {
    // SAFETY: the list is not being changed -- every change blocks this
    // signal -- and no other thread exists. `unlink` is async-signal-safe.
    let list = unsafe { &*REGISTRY.0.get() };
    for path in list {
        sys::unlink(path);
    }
    let _ = libcall::signal::set_default(sig);
    let _ = libcall::signal::raise(sig);
}

#[cfg(unix)]
mod sys {
    use std::ffi::CString;

    unsafe extern "C" {
        #[link_name = "unlink"]
        fn c_unlink(path: *const std::ffi::c_char) -> i32;
    }

    /// `unlink`, unchecked: a file already gone is what was wanted, and in a
    /// handler there is nowhere to report one that would not go.
    pub fn unlink(path: &CString) {
        // SAFETY: `path` is a NUL-terminated string that outlives the call,
        // which only reads it.
        let _ = unsafe { c_unlink(path.as_ptr()) };
    }
}

#[cfg(not(unix))]
mod sys {
    use std::ffi::CString;

    pub fn unlink(path: &CString) {
        if let Ok(text) = path.to_str() {
            // Unchecked: as the unix arm.
            let _ = std::fs::remove_file(text);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn registered(path: &Path) -> bool {
        let c = c_path(path).unwrap();
        change(|list| list.contains(&c))
    }

    #[test]
    fn a_registered_file_is_removed_by_remove_all_and_forgotten_by_remove() {
        let dir = std::env::temp_dir();
        let one = dir.join(format!("cleanup-test-one-{}", std::process::id()));
        let two = dir.join(format!("cleanup-test-two-{}", std::process::id()));
        for path in [&one, &two] {
            change(|list| {
                std::fs::write(path, b"x").unwrap();
                add_to(list, path);
            });
            assert!(registered(path));
        }
        remove(&one).unwrap();
        assert!(!one.exists());
        assert!(!registered(&one));
        // Spawning empties the list, and puts it back.
        let inside = without(|| registered(&two));
        assert!(!inside);
        assert!(registered(&two));
        remove_all();
        assert!(!two.exists());
        assert!(!registered(&two));
    }
}
