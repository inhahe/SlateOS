//! `glob(3)`: the paths a pattern matches, as the C library finds them.
//!
//! Asked of the one library every program links (design-decisions §768), so
//! a pattern a program expands is expanded as every C program's `glob` would
//! expand it -- with the library's sorting, its `GLOB_BRACE` and
//! `GLOB_TILDE`, and its `GLOB_NOCHECK` answer for a pattern that matches
//! nothing. procps' `sysctl` is the first caller: it loads `-p` files through
//! `glob (filename, GLOB_NOCHECK | GLOB_BRACE | GLOB_TILDE, …)` and expands a
//! key with a wildcard in it through `glob (path, 0, …)`.
//!
//! The paths are handed to a callback one at a time, in the library's order,
//! and freed with `globfree` before [`glob`] returns -- so this stays without
//! an allocator of its own, like the rest of the crate.

use core::ffi::CStr;

/// `GLOB_ERR`: stop at the first directory that cannot be read.
pub const GLOB_ERR: i32 = 1 << 0;
/// `GLOB_MARK`: append a slash to each directory matched.
pub const GLOB_MARK: i32 = 1 << 1;
/// `GLOB_NOSORT`: in the order the directories gave them, unsorted.
pub const GLOB_NOSORT: i32 = 1 << 2;
/// `GLOB_NOCHECK`: a pattern that matches nothing is its own one result.
pub const GLOB_NOCHECK: i32 = 1 << 4;
/// `GLOB_NOESCAPE`: a backslash is an ordinary character.
pub const GLOB_NOESCAPE: i32 = 1 << 6;
/// `GLOB_PERIOD`: a leading `.` may be matched by a wildcard (glibc).
pub const GLOB_PERIOD: i32 = 1 << 7;
/// `GLOB_BRACE`: `{a,b}` stands for `a` and `b` (glibc).
pub const GLOB_BRACE: i32 = 1 << 10;
/// `GLOB_NOMAGIC`: `GLOB_NOCHECK`, but only for a pattern with no wildcard.
pub const GLOB_NOMAGIC: i32 = 1 << 11;
/// `GLOB_TILDE`: `~` and `~user` are home directories (glibc).
pub const GLOB_TILDE: i32 = 1 << 12;
/// `GLOB_ONLYDIR`: directories only, as a hint (glibc).
pub const GLOB_ONLYDIR: i32 = 1 << 13;
/// `GLOB_TILDE_CHECK`: `GLOB_TILDE`, and no match for an unknown user.
pub const GLOB_TILDE_CHECK: i32 = 1 << 14;

/// `GLOB_NOSPACE`: the library ran out of memory.
pub const GLOB_NOSPACE: i32 = 1;
/// `GLOB_ABORTED`: a directory could not be read, and `GLOB_ERR` was set.
pub const GLOB_ABORTED: i32 = 2;
/// `GLOB_NOMATCH`: nothing matched, and `GLOB_NOCHECK` was not set.
pub const GLOB_NOMATCH: i32 = 3;
/// What `glob` answers here when there is no C library to ask: the
/// implementation-defined `GLOB_NOSYS` of glibc.
pub const GLOB_NOSYS: i32 = 4;

/// `glob_t`, as glibc and SlateOS's C library both lay it out (musl's is the
/// same size, its tail private): the count, the vector, `gl_offs`, the flags,
/// and five function pointers for `GLOB_ALTDIRFUNC`, which is never asked for
/// here and so never read.
#[cfg(unix)]
#[repr(C)]
struct GlobT {
    gl_pathc: usize,
    gl_pathv: *mut *mut core::ffi::c_char,
    gl_offs: usize,
    gl_flags: i32,
    gl_altdirfunc: [usize; 5],
}

#[cfg(unix)]
mod sys {
    use core::ffi::{c_char, c_int};

    unsafe extern "C" {
        pub fn glob(
            pattern: *const c_char,
            flags: c_int,
            errfunc: Option<unsafe extern "C" fn(*const c_char, c_int) -> c_int>,
            pglob: *mut super::GlobT,
        ) -> c_int;
        pub fn globfree(pglob: *mut super::GlobT);
    }
}

/// `glob (pattern, flags, NULL, &g)`: each path the library found, in its
/// order, handed to `each` -- and then `globfree`.
///
/// A pattern that matches nothing is `Err(GLOB_NOMATCH)` unless `flags` has
/// [`GLOB_NOCHECK`], which makes the pattern itself the one path, as C's
/// does. With no error function, a directory that cannot be read is skipped
/// unless `flags` has [`GLOB_ERR`].
///
/// # Errors
///
/// `glob`'s own return value, not an `errno`: [`GLOB_NOMATCH`],
/// [`GLOB_ABORTED`] or [`GLOB_NOSPACE`]; and [`GLOB_NOSYS`] on a host with no
/// C library.
pub fn glob(pattern: &CStr, flags: i32, each: &mut dyn FnMut(&[u8])) -> Result<(), i32> {
    #[cfg(unix)]
    {
        let mut g = GlobT {
            gl_pathc: 0,
            gl_pathv: core::ptr::null_mut(),
            gl_offs: 0,
            gl_flags: 0,
            gl_altdirfunc: [0; 5],
        };
        // SAFETY: `pattern` is a NUL-terminated string that outlives the
        // call, no error function is passed, and `g` is a zeroed `glob_t` of
        // the library's layout that `glob` fills in. `GLOB_APPEND`,
        // `GLOB_DOOFFS` and `GLOB_ALTDIRFUNC` -- the flags that make it read
        // `g` -- are not ours to pass: they are masked off below.
        let rc = unsafe {
            sys::glob(
                pattern.as_ptr(),
                flags & !(GLOB_APPEND | GLOB_DOOFFS | GLOB_ALTDIRFUNC),
                None,
                &raw mut g,
            )
        };
        if rc == 0 {
            for k in 0..g.gl_pathc {
                // SAFETY: on success `gl_pathv` holds `gl_pathc` pointers
                // (after `gl_offs` NULLs, none here), each to a NUL-terminated
                // path, all live until `globfree` below.
                let path = unsafe { CStr::from_ptr(*g.gl_pathv.add(k)) };
                each(path.to_bytes());
            }
        }
        // SAFETY: `g` is the `glob_t` `glob` filled in (or left with no
        // vector, which `globfree` accepts); it is freed exactly once.
        unsafe { sys::globfree(&raw mut g) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }
    #[cfg(not(unix))]
    {
        let _ = (pattern, flags, each);
        Err(GLOB_NOSYS)
    }
}

/// `GLOB_APPEND`, `GLOB_DOOFFS` and `GLOB_ALTDIRFUNC`: the flags under which
/// `glob` reads the `glob_t` it is given rather than only writing it. [`glob`]
/// starts from an empty one every time, so it never passes them.
#[cfg(unix)]
const GLOB_DOOFFS: i32 = 1 << 3;
#[cfg(unix)]
const GLOB_APPEND: i32 = 1 << 5;
#[cfg(unix)]
const GLOB_ALTDIRFUNC: i32 = 1 << 9;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// A fresh directory under the system's temporary one, named for this
    /// process and `tag`, holding `files`.
    fn fixture(tag: &str, files: &[&str]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(std::format!("libcall-glob-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        dir
    }

    fn run(pattern: &str, flags: i32) -> Result<Vec<Vec<u8>>, i32> {
        let c = std::ffi::CString::new(pattern).unwrap();
        let mut got = Vec::new();
        glob(&c, flags, &mut |p| got.push(p.to_vec())).map(|()| got)
    }

    #[test]
    fn the_constants_agree_with_posix() {
        assert_eq!(GLOB_NOCHECK, posix::glob::GLOB_NOCHECK);
        assert_eq!(GLOB_BRACE, posix::glob::GLOB_BRACE);
        assert_eq!(GLOB_TILDE, posix::glob::GLOB_TILDE);
        assert_eq!(GLOB_TILDE_CHECK, posix::glob::GLOB_TILDE_CHECK);
    }

    #[test]
    fn matches_sorted_nocheck_and_braces() {
        let dir = fixture("m", &["b.conf", "a.conf", "c.txt"]);
        let d = dir.to_str().unwrap().replace('\\', "/");
        if cfg!(unix) {
            let conf = run(&std::format!("{d}/*.conf"), 0).unwrap();
            assert_eq!(
                conf,
                [
                    std::format!("{d}/a.conf").into_bytes(),
                    std::format!("{d}/b.conf").into_bytes()
                ]
            );
            assert_eq!(run(&std::format!("{d}/*.none"), 0), Err(GLOB_NOMATCH));
            let none = std::format!("{d}/*.none");
            assert_eq!(
                run(&none, GLOB_NOCHECK),
                Ok(std::vec![none.clone().into_bytes()])
            );
            let braced = run(&std::format!("{d}/{{c.txt,a.conf}}"), GLOB_BRACE).unwrap();
            assert_eq!(braced.len(), 2, "both alternatives, in brace order");
        } else {
            assert_eq!(run(&std::format!("{d}/*.conf"), 0), Err(GLOB_NOSYS));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
