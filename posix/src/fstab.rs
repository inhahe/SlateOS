//! `<fstab.h>` — the BSD file-system table interface over `/etc/fstab`, as
//! glibc 2.39's misc/fstab.c has it: [`crate::mntent`]'s entries, each with
//! a type taken from its options.
//!
//! `getfsent` reads the table's next entry, opening it first if it is not
//! open; `getfsspec` and `getfsfile` read it again from the start for the
//! first entry with that device or that mount point -- leaving the table
//! just past it, so a `getfsent` after one goes on from there; `setfsent`
//! opens the table or rewinds it (1, or 0 when it cannot be opened);
//! `endfsent` closes it.  An entry's `fs_type` is the first of `rw`, `rq`,
//! `ro`, `sw` and `xx` its options hold (`hasmntopt`'s reading, so `rw=1`
//! counts and `norw` does not), else `??`.
//!
//! There is one table state per process, as in glibc: the functions are not
//! reentrant, and the entry each returns is overwritten by the next call.
//! glibc 2.39's answers are `fsttys_oracle.txt`
//! (`posix/tools/oracle/fsttys_harness.py`), which the tests replay.

use crate::mntent::{Mntent, endmntent, getmntent_r, hasmntopt};
use crate::perprocess::process_global;

/// The table (glibc's `_PATH_FSTAB`).
const PATH_FSTAB: &core::ffi::CStr = c"/etc/fstab";

/// glibc's buffer for one entry's strings.
const BUFFER_SIZE: usize = 0x1fc0;

/// A file-system table entry (`struct fstab`), glibc's layout.
#[repr(C)]
pub struct Fstab {
    /// The device.
    pub fs_spec: *mut u8,
    /// Where it is mounted.
    pub fs_file: *mut u8,
    /// Its file system's type (`ext4`, `nfs` ...).
    pub fs_vfstype: *mut u8,
    /// Its mount options.
    pub fs_mntops: *mut u8,
    /// `rw`, `rq`, `ro`, `sw`, `xx` or `??`, from the options.
    pub fs_type: *const u8,
    /// How often it is dumped, in days.
    pub fs_freq: i32,
    /// Its place in `fsck`'s order.
    pub fs_passno: i32,
}

/// What `fstab_init` keeps between calls: glibc's `struct fstab_state`.
struct State {
    /// The table's stream, or NULL.
    fp: *mut u8,
    /// [`BUFFER_SIZE`] bytes, `malloc`ed once, for the entry's strings.
    buffer: *mut u8,
    mnt: Mntent,
    ret: Fstab,
}

process_global! {
    fn state() -> State = State {
        fp: core::ptr::null_mut(),
        buffer: core::ptr::null_mut(),
        mnt: Mntent {
            mnt_fsname: core::ptr::null_mut(),
            mnt_dir: core::ptr::null_mut(),
            mnt_type: core::ptr::null_mut(),
            mnt_opts: core::ptr::null_mut(),
            mnt_freq: 0,
            mnt_passno: 0,
        },
        ret: Fstab {
            fs_spec: core::ptr::null_mut(),
            fs_file: core::ptr::null_mut(),
            fs_vfstype: core::ptr::null_mut(),
            fs_mntops: core::ptr::null_mut(),
            fs_type: core::ptr::null(),
            fs_freq: 0,
            fs_passno: 0,
        },
    };
}

/// The table, opened: `setmntent(_PATH_FSTAB, "r")`.
fn open_table() -> *mut u8 {
    #[cfg(test)]
    return test_table::open();
    #[cfg(not(test))]
    // SAFETY: two C strings.
    unsafe {
        crate::mntent::setmntent(PATH_FSTAB.as_ptr().cast(), c"r".as_ptr().cast())
    }
}

/// glibc's `fstab_init`: the state with its buffer and the table open --
/// rewound, with `rewind` -- or NULL when either cannot be had.
fn init(rewind: bool) -> Option<&'static mut State> {
    // SAFETY: this process's state; these functions are not reentrant, as
    // glibc's are not.
    let st = unsafe { &mut *state() };
    if st.buffer.is_null() {
        st.buffer = crate::malloc::malloc(BUFFER_SIZE);
        if st.buffer.is_null() {
            return None;
        }
    }
    if st.fp.is_null() {
        st.fp = open_table();
        if st.fp.is_null() {
            return None;
        }
    } else if rewind {
        crate::stdio::rewind(st.fp);
    }
    Some(st)
}

/// The table's next entry, into the state: glibc's `fstab_fetch`.
fn fetch(st: &mut State) -> bool {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    const LEN: i32 = BUFFER_SIZE as i32;
    // SAFETY: an open stream, the state's entry and its buffer of LEN bytes.
    !unsafe { getmntent_r(st.fp, &raw mut st.mnt, st.buffer, LEN) }.is_null()
}

/// The fetched entry as a `struct fstab`: glibc's `fstab_convert`.
fn convert(st: &mut State) -> *mut Fstab {
    let has = |opt: &core::ffi::CStr| {
        // SAFETY: the entry just fetched, and a C string.
        !unsafe { hasmntopt(&raw const st.mnt, opt.as_ptr().cast()) }.is_null()
    };
    let fs_type: &'static core::ffi::CStr = if has(c"rw") {
        c"rw"
    } else if has(c"rq") {
        c"rq"
    } else if has(c"ro") {
        c"ro"
    } else if has(c"sw") {
        c"sw"
    } else if has(c"xx") {
        c"xx"
    } else {
        c"??"
    };
    st.ret = Fstab {
        fs_spec: st.mnt.mnt_fsname,
        fs_file: st.mnt.mnt_dir,
        fs_vfstype: st.mnt.mnt_type,
        fs_mntops: st.mnt.mnt_opts,
        fs_type: fs_type.as_ptr().cast(),
        fs_freq: st.mnt.mnt_freq,
        fs_passno: st.mnt.mnt_passno,
    };
    &raw mut st.ret
}

/// Open the table, or rewind it: 1, or 0 when it cannot be opened.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setfsent() -> i32 {
    i32::from(init(true).is_some())
}

/// The table's next entry, or NULL at its end or when it cannot be opened.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getfsent() -> *mut Fstab {
    let Some(st) = init(false) else {
        return core::ptr::null_mut();
    };
    if fetch(st) {
        convert(st)
    } else {
        core::ptr::null_mut()
    }
}

/// The first entry whose field `pick` gives is `name`, read from the start.
///
/// # Safety
///
/// `name` is NULL or a C string.
unsafe fn find(name: *const u8, pick: fn(&Mntent) -> *mut u8) -> *mut Fstab {
    let Some(st) = init(true) else {
        return core::ptr::null_mut();
    };
    if name.is_null() {
        // Where glibc's `strcmp` faults: no entry has no name.
        return core::ptr::null_mut();
    }
    while fetch(st) {
        let field = pick(&st.mnt);
        // SAFETY: two C strings: the entry's field and the caller's name.
        if !field.is_null() && unsafe { crate::string::strcmp(field, name) } == 0 {
            return convert(st);
        }
    }
    core::ptr::null_mut()
}

/// The first entry for device `name`, the table read from its start; NULL
/// if there is none.
///
/// # Safety
///
/// `name` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getfsspec(name: *const u8) -> *mut Fstab {
    // SAFETY: the caller's contract.
    unsafe { find(name, |m| m.mnt_fsname) }
}

/// The first entry mounted at `name`, the table read from its start; NULL
/// if there is none.
///
/// # Safety
///
/// `name` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getfsfile(name: *const u8) -> *mut Fstab {
    // SAFETY: the caller's contract.
    unsafe { find(name, |m| m.mnt_dir) }
}

/// Close the table (nothing, when it is not open).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endfsent() {
    // SAFETY: this process's state.
    let st = unsafe { &mut *state() };
    if !st.fp.is_null() {
        // Nothing to report it to: endfsent returns nothing.
        let _ = endmntent(st.fp);
        st.fp = core::ptr::null_mut();
    }
}

/// The table the tests give: text in memory, or none.
#[cfg(test)]
pub(crate) mod test_table {
    use std::cell::Cell;

    std::thread_local! {
        static TEXT: Cell<Option<&'static [u8]>> = const { Cell::new(None) };
    }

    /// Make `text` this thread's `/etc/fstab`, or have none.
    pub(crate) fn set(text: Option<&'static [u8]>) {
        TEXT.with(|t| t.set(text));
    }

    /// A read-only stream over it, or NULL.
    pub(super) fn open() -> *mut u8 {
        let Some(text) = TEXT.with(Cell::get) else {
            return core::ptr::null_mut();
        };
        let bytes: &'static mut [u8] = std::boxed::Box::leak(text.to_vec().into_boxed_slice());
        // SAFETY: a leaked buffer of the length given, which outlives the
        // stream.
        unsafe {
            crate::stdio_mem::fmemopen(bytes.as_mut_ptr().cast(), bytes.len(), c"r".as_ptr().cast())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers (posix/tools/oracle/fsttys_harness.py).
    const ORACLE: &str = include_str!("fsttys_oracle.txt");

    /// A string as the harness's `str` writes it.
    fn show(p: *const u8) -> String {
        if p.is_null() {
            return "(null)".into();
        }
        // SAFETY: a C string of the entry's.
        let bytes = unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) };
        let mut s = String::new();
        for &b in bytes {
            match b {
                b'\\' => s.push_str("\\\\"),
                b'\n' => s.push_str("\\n"),
                b'\t' => s.push_str("\\t"),
                0x20..=0x7e => s.push(b as char),
                _ => s.push_str(&format!("\\x{b:02x}")),
            }
        }
        s
    }

    /// The harness's escapes undone.
    fn unescape(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' {
                match b[i + 1] {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'\\' => out.push(b'\\'),
                    b'x' => {
                        out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).unwrap());
                        i += 2;
                    }
                    other => panic!("escape {other}"),
                }
                i += 2;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    /// The oracle's input file `name`, leaked for the table to read.
    fn input(name: &str) -> &'static [u8] {
        let prefix = format!("input {name} = ");
        let line = ORACLE
            .lines()
            .find_map(|l| l.strip_prefix(prefix.as_str()))
            .unwrap();
        std::boxed::Box::leak(unescape(line).into_boxed_slice())
    }

    fn fs(what: &str, f: *mut Fstab) -> String {
        // SAFETY: the table's entry, if any.
        match unsafe { f.as_ref() } {
            None => format!("{what} = NULL"),
            Some(f) => format!(
                "{what} = {}|{}|{}|{}|{}|{}|{}",
                show(f.fs_spec),
                show(f.fs_file),
                show(f.fs_vfstype),
                show(f.fs_mntops),
                show(f.fs_type),
                f.fs_freq,
                f.fs_passno
            ),
        }
    }

    /// The harness's fstab half, call for call.
    fn fstab_run(run: &str) -> Vec<String> {
        let mut out = Vec::new();
        for n in 0..20 {
            let f = getfsent();
            out.push(fs(&format!("{run} getfsent #{n}"), f));
            if f.is_null() {
                break;
            }
        }
        out.push(format!("{run} setfsent = {}", setfsent()));
        out.push(fs(&format!("{run} getfsent after setfsent"), getfsent()));
        for spec in [
            "/dev/sda2",
            "/dev/sdb1",
            "none",
            "nothing",
            "/dev/z",
            "tmpfs",
            "/dev/v",
            "missing",
            "",
        ] {
            let c = std::ffi::CString::new(spec).unwrap();
            // SAFETY: a C string.
            let f = unsafe { getfsspec(c.as_ptr().cast()) };
            out.push(fs(&format!("{run} getfsspec({spec})"), f));
            out.push(fs(
                &format!("{run} getfsent after getfsspec({spec})"),
                getfsent(),
            ));
        }
        for file in [
            "/home",
            "/",
            "/z dir",
            "/z\\040dir",
            "none",
            "/var",
            "missing",
        ] {
            let c = std::ffi::CString::new(file).unwrap();
            // SAFETY: as above.
            let f = unsafe { getfsfile(c.as_ptr().cast()) };
            out.push(fs(&format!("{run} getfsfile({file})"), f));
        }
        endfsent();
        out.push(fs(&format!("{run} getfsent after endfsent"), getfsent()));
        endfsent();
        endfsent();
        out.push(format!("{run} endfsent twice = ok"));
        out
    }

    /// Every probe of the harness's fstab half, with /etc/fstab and without.
    #[test]
    fn the_table_is_read_as_glibc_reads_it() {
        for (run, text) in [("files", Some(input("fstab"))), ("none", None)] {
            test_table::set(text);
            let got = fstab_run(run);
            let want: Vec<_> = ORACLE
                .lines()
                .filter(|l| {
                    l.starts_with(&format!("{run} "))
                        && (l.contains(" getfs") || l.contains("fsent"))
                })
                .collect();
            assert_eq!(got.len(), want.len(), "{run}");
            for (g, w) in got.iter().zip(&want) {
                assert_eq!(g, w);
            }
            endfsent();
        }
        test_table::set(None);
        assert_eq!(PATH_FSTAB, c"/etc/fstab");
    }

    #[test]
    fn fstab_is_glibcs_layout() {
        assert_eq!(core::mem::size_of::<Fstab>(), 48);
        assert_eq!(core::mem::offset_of!(Fstab, fs_type), 32);
        assert_eq!(core::mem::offset_of!(Fstab, fs_freq), 40);
        assert_eq!(core::mem::offset_of!(Fstab, fs_passno), 44);
    }

    /// A NULL name, where glibc's `strcmp` faults, is no entry.
    #[test]
    fn a_null_name_is_no_entry() {
        test_table::set(Some(input("fstab")));
        // SAFETY: NULL is refused.
        unsafe {
            assert!(getfsspec(core::ptr::null()).is_null());
            assert!(getfsfile(core::ptr::null()).is_null());
        }
        endfsent();
        test_table::set(None);
    }
}
