//! `pathfind` (libopts' `compat/pathfind.c`): where along `PATH` a program
//! named by a bare word lives. libopts calls it on `argv[0]` to fill in
//! `pzProgPath`, which several diagnostics print -- so
//! `uuencode -z` says `/usr/bin/uuencode: illegal option -- z`.
//!
//! A name with a `/` in it is never found (no directory entry can contain
//! one), and the caller then keeps `argv[0]` as it was. The directory is
//! read entry by entry rather than probed, the match must also be readable
//! and executable, and the result is normalised textually, not resolved.

use std::fs;

use crate::bytes_os;

/// `AG_PATH_MAX`: the longest `PATH` element used.
const PATH_MAX: usize = 4096;

/// `extract_colon_unit`: the next non-empty element of `path` from `*ix`.
fn colon_unit(path: &[u8], ix: &mut usize) -> Option<Vec<u8>> {
    if *ix >= path.len() {
        return None;
    }
    let mut p = *ix;
    while path.get(p) == Some(&b':') {
        p = p.saturating_add(1);
    }
    let mut unit = Vec::new();
    loop {
        match path.get(p) {
            None => break,
            Some(b':') => {
                p = p.saturating_add(1);
                break;
            }
            Some(&c) => {
                unit.push(c);
                p = p.saturating_add(1);
                if unit.len() >= PATH_MAX {
                    break;
                }
            }
        }
    }
    if unit.is_empty() {
        return None;
    }
    *ix = p;
    Some(unit)
}

/// `access(name, R_OK | X_OK) == 0`.
fn readable_and_executable(name: &[u8]) -> bool {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        unsafe extern "C" {
            fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
        }
        const R_OK: i32 = 4;
        const X_OK: i32 = 1;
        let Ok(c) = CString::new(name.to_vec()) else {
            return false;
        };
        // SAFETY: `c` is a valid NUL-terminated string that outlives the
        // call; `access` only reads it.
        unsafe { access(c.as_ptr(), R_OK | X_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        fs::metadata(bytes_os(name)).is_ok()
    }
}

/// Whether the directory `dir` has an entry named `name`, as a `readdir`
/// scan would find it (`.` and `..` included).
fn has_entry(dir: &[u8], name: &[u8]) -> Option<bool> {
    let entries = fs::read_dir(bytes_os(dir)).ok()?;
    if name == b"." || name == b".." {
        return Some(true);
    }
    Some(
        entries
            .flatten()
            .any(|e| crate::os_bytes(&e.file_name()) == name),
    )
}

/// `pathfind(path, name, "rx")`.
pub(crate) fn pathfind(path: Option<&[u8]>, name: &[u8]) -> Option<Vec<u8>> {
    let path = path?;
    let mut ix = 0usize;
    while let Some(dir) = colon_unit(path, &mut ix) {
        if has_entry(&dir, name) != Some(true) {
            continue;
        }
        let mut abs = if name.first() == Some(&b'/') {
            name.to_vec()
        } else {
            let mut a = dir.clone();
            if a.last() != Some(&b'/') {
                a.push(b'/');
            }
            a.extend_from_slice(name);
            a
        };
        if readable_and_executable(&abs) {
            abs = canonicalize_pathname(&abs);
            return Some(abs);
        }
    }
    None
}

/// `canonicalize_pathname`: `//`, `./` and `../` removed textually, as
/// pathfind.c does it -- including its way with a leading `..`, which
/// leaves a `/` where the removed text began.
pub(crate) fn canonicalize_pathname(path: &[u8]) -> Vec<u8> {
    let mut r = path.to_vec();
    let stub = if path.first() == Some(&b'/') {
        b'/'
    } else {
        b'.'
    };
    let get = |r: &Vec<u8>, i: usize| r.get(i).copied().unwrap_or(0);
    // `strcpy(result + dst, result + src)`.
    let shift = |r: &mut Vec<u8>, dst: usize, src: usize| {
        let tail: Vec<u8> = r.get(src..).unwrap_or_default().to_vec();
        r.truncate(dst);
        r.extend_from_slice(&tail);
    };
    let mut i: usize = 0;
    while get(&r, i) != 0 {
        while get(&r, i) != 0 && get(&r, i) != b'/' {
            i = i.saturating_add(1);
        }
        let start = i;
        i = i.saturating_add(1);
        if get(&r, start) == 0 {
            break;
        }
        while get(&r, i) == b'/' {
            i = i.saturating_add(1);
        }
        if start.saturating_add(1) != i {
            shift(&mut r, start.saturating_add(1), i);
            i = start.saturating_add(1);
        }
        if start > 0 && get(&r, start.saturating_sub(1)) == b'\\' {
            continue;
        }
        if (start != 0 && get(&r, i) == 0)
            || (get(&r, i) == b'.' && get(&r, i.saturating_add(1)) == 0)
        {
            i = i.saturating_sub(1);
            r.truncate(i);
            break;
        }
        if get(&r, i) == b'.' {
            if get(&r, i.saturating_add(1)) == b'/' {
                shift(&mut r, i, i.saturating_add(1));
                i = start;
                continue;
            }
            if get(&r, i.saturating_add(1)) == b'.'
                && (get(&r, i.saturating_add(2)) == b'/' || get(&r, i.saturating_add(2)) == 0)
            {
                // `while (--start > -1 && result[start] != '/')`.
                let mut s = isize::try_from(start).unwrap_or(0);
                loop {
                    s = s.saturating_sub(1);
                    if s <= -1 || get(&r, usize::try_from(s).unwrap_or(0)) == b'/' {
                        break;
                    }
                }
                let dst = usize::try_from(s.saturating_add(1)).unwrap_or(0);
                shift(&mut r, dst, i.saturating_add(2));
                i = usize::try_from(s).unwrap_or(0);
            }
        }
    }
    if r.is_empty() {
        r.push(stub);
    }
    r
}
