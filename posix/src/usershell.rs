//! `getusershell`, `setusershell`, `endusershell` (`<unistd.h>`, from BSD):
//! the login shells `/etc/shells` lists, as glibc 2.39's `getusershell.c`
//! reads them.
//!
//! `chsh` asks whether a shell is one a user may choose, and `ftpd` and
//! `su` whether a user's shell is a real one. The list is read whole on the
//! first call (or on `setusershell`) and handed out one shell a call:
//!
//! - **A line's shell** starts at its first `/` and ends at white space, a
//!   `#` or the line's end. A line whose first `/` comes after a `#` -- or
//!   that has none -- lists nothing, and so does a last line that is only a
//!   `/` with no newline after it (glibc's `cp[1] == '\0'`). So
//!   `x/bin/ksh` lists `/bin/ksh`, and `/bin/a /bin/b` only `/bin/a`.
//! - **No file**, or one that cannot be read: `/bin/sh` and `/bin/csh`,
//!   glibc's and musl's fallback -- historical, and harmless, since a shell
//!   that is not installed cannot be run whether or not it is listed.
//!
//! The strings are this process's, good until `setusershell` or
//! `endusershell`. glibc sizes its pointer array from the file's length
//! divided by three and so overruns it for a file of very short lines; this
//! counts them.

use crate::nss_files::{Db, Which, is_space, read};
use crate::perprocess::process_global;

/// The shells when there is no `/etc/shells`: glibc's `okshells`.
const DEFAULT_SHELLS: [&core::ffi::CStr; 2] = [c"/bin/sh", c"/bin/csh"];

/// The list, and where the next `getusershell` is in it.
struct Shells {
    /// Read yet: glibc's `curshell != NULL`.
    loaded: bool,
    /// The file's shells, each NUL-terminated, one after another, in a
    /// `malloc` block; NULL when the defaults are the list.
    block: *mut u8,
    /// The block's length.
    len: usize,
    /// The next shell: an offset into `block`, or an index into
    /// [`DEFAULT_SHELLS`].
    next: usize,
}

impl Shells {
    const NONE: Self = Self {
        loaded: false,
        block: core::ptr::null_mut(),
        len: 0,
        next: 0,
    };

    /// Forget the list: its block freed, the next call reads the file again.
    fn clear(&mut self) {
        // SAFETY: `block` is NULL or this list's `malloc` block.
        unsafe { crate::malloc::free(self.block) };
        *self = Self::NONE;
    }

    /// Read the list afresh (`initshells`), from its first shell.
    fn load(&mut self) {
        self.clear();
        self.loaded = true;
        let Ok(Db::Text(text)) = read(Which::Shells) else {
            return; // the defaults
        };
        let text = text.bytes();
        // Each shell and its NUL: no more than the text, a byte a line more.
        let size = shells_of(text).fold(0usize, |n, s| n.saturating_add(s.len()).saturating_add(1));
        let block = crate::malloc::malloc(size.max(1));
        if block.is_null() {
            return; // glibc's answer to a failed malloc: the defaults
        }
        // SAFETY: a fresh block of at least `size` bytes, this list's alone.
        let dst = unsafe { core::slice::from_raw_parts_mut(block, size) };
        let mut at = 0usize;
        for shell in shells_of(text) {
            let end = at.saturating_add(shell.len());
            // `size` counted every shell and its NUL, so both land inside.
            if let Some(slot) = dst.get_mut(at..end) {
                slot.copy_from_slice(shell);
            }
            if let Some(nul) = dst.get_mut(end) {
                *nul = 0;
            }
            at = end.saturating_add(1);
        }
        self.block = block;
        self.len = size;
    }

    /// The next shell, or NULL after the last.
    fn next(&mut self) -> *const u8 {
        if self.block.is_null() {
            let Some(shell) = DEFAULT_SHELLS.get(self.next) else {
                return core::ptr::null();
            };
            self.next = self.next.saturating_add(1);
            return shell.as_ptr().cast();
        }
        if self.next >= self.len {
            return core::ptr::null();
        }
        // SAFETY: `next` is the offset of a shell inside the block.
        let shell = unsafe { self.block.add(self.next) };
        // SAFETY: every shell in the block is NUL-terminated.
        let n = unsafe { crate::string::strlen(shell) };
        self.next = self.next.saturating_add(n).saturating_add(1);
        shell
    }
}

/// The shells `text` lists, by glibc's rules (see the module docs).
fn shells_of(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split_inclusive(|&b| b == b'\n').filter_map(|raw| {
        // `fgets` reads through a NUL, but the scan stops at it.
        let line = raw.split(|&b| b == 0).next().unwrap_or(&[]);
        let start = line.iter().position(|&b| b == b'/' || b == b'#')?;
        if line.get(start) == Some(&b'#') || line.len().checked_sub(start) == Some(1) {
            return None;
        }
        let rest = line.get(start..)?;
        let end = rest
            .iter()
            .position(|&b| is_space(b) || b == b'#')
            .unwrap_or(rest.len());
        rest.get(..end)
    })
}

process_global! {
    fn shells() -> Shells = Shells::NONE;
}

/// The next shell `/etc/shells` lists, or NULL after the last; reading the
/// list on the first call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getusershell() -> *mut u8 {
    // SAFETY: this process's list, used by one call at a time.
    let s = unsafe { &mut *shells() };
    if !s.loaded {
        s.load();
    }
    s.next().cast_mut()
}

/// Read the list again, from its first shell.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setusershell() {
    // SAFETY: as in `getusershell`.
    unsafe { &mut *shells() }.load();
}

/// Free the list; the next `getusershell` reads it again.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endusershell() {
    // SAFETY: as in `getusershell`.
    unsafe { &mut *shells() }.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nss_files::set_test_text;
    use std::vec::Vec;

    fn all() -> Vec<Vec<u8>> {
        endusershell();
        let mut out = Vec::new();
        loop {
            let p = getusershell();
            if p.is_null() {
                return out;
            }
            // SAFETY: a shell the list handed out, NUL-terminated.
            out.push(unsafe { crate::nss_files::c_bytes(p) }.to_vec());
        }
    }

    #[test]
    fn a_line_lists_what_follows_its_first_slash() {
        set_test_text(
            Which::Shells,
            Some(b"#c\n/bin/sh\n\n  /usr/bin/zsh  # t\nnot/a/shell\n/\nx/bin/ksh\na#/bin/no\n/bin/a /bin/b\n/bin/dash"),
        );
        assert_eq!(
            all(),
            [
                &b"/bin/sh"[..],
                b"/usr/bin/zsh",
                b"/a/shell",
                b"/",
                b"/bin/ksh",
                b"/bin/a",
                b"/bin/dash"
            ]
        );
    }

    #[test]
    fn a_last_line_of_just_a_slash_lists_nothing() {
        set_test_text(Which::Shells, Some(b"/bin/sh\n/"));
        assert_eq!(all(), [&b"/bin/sh"[..]]);
    }

    #[test]
    fn a_nul_ends_a_line_where_glibcs_scan_stops() {
        set_test_text(Which::Shells, Some(b"/bin/a\0/bin/b\nzz\0/bin/c\n/bin/d\n"));
        assert_eq!(all(), [&b"/bin/a"[..], b"/bin/d"]);
    }

    #[test]
    fn no_file_is_the_two_historical_shells() {
        set_test_text(Which::Shells, None);
        assert_eq!(all(), [&b"/bin/sh"[..], b"/bin/csh"]);
        crate::nss_files::set_test_error(Which::Shells, crate::errno::EACCES);
        assert_eq!(
            all(),
            [&b"/bin/sh"[..], b"/bin/csh"],
            "nor one that cannot be read"
        );
    }

    #[test]
    fn set_rewinds_and_end_forgets() {
        set_test_text(Which::Shells, Some(b"/bin/sh\n/bin/bash\n"));
        endusershell();
        let first = getusershell();
        assert!(!first.is_null());
        assert!(!getusershell().is_null());
        assert!(getusershell().is_null());
        assert!(getusershell().is_null(), "still at the end");
        setusershell();
        // SAFETY: a shell the list handed out.
        assert_eq!(
            unsafe { crate::nss_files::c_bytes(getusershell()) },
            b"/bin/sh"
        );
        set_test_text(Which::Shells, Some(b"/bin/new\n"));
        endusershell();
        // SAFETY: as above.
        assert_eq!(
            unsafe { crate::nss_files::c_bytes(getusershell()) },
            b"/bin/new",
            "read again after end"
        );
        endusershell();
    }
}
