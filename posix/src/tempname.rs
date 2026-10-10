//! Temporary names: glibc's `__gen_tempname` (sysdeps/posix/tempname.c) and
//! `__path_search` (stdio-common/tmpdir.c), which every function that names
//! or makes a temporary file is written over -- `mkstemp` and its `o`, `s`
//! and large-file forms, `mkdtemp`, `mktemp` and `tmpfile` (stdlib.rs),
//! `tmpnam` and `tempnam` (stdio.rs), `tmpnam_r` (unistd.rs).
//!
//! # A name
//!
//! A template's last six bytes before its suffix must be `X`s -- else
//! `EINVAL`, and the template is left as it was.  They become letters and
//! digits, drawn from `arc4random` without bias, glibc's 62 in glibc's
//! order; the name is then made a file (`O_RDWR | O_CREAT | O_EXCL`, with
//! the caller's flags less an access mode, mode 0600 before the umask), a
//! directory (0700), or neither -- checked with `lstat` to be free, for
//! `mktemp`, `tmpnam` and `tempnam`.  A name that is taken (`EEXIST`) is
//! drawn again, up to 62 cubed times, glibc's `ATTEMPTS_MIN`; any other
//! failure is the answer, with the template holding the name that failed.
//! A success leaves `errno` as it was.
//!
//! # A directory
//!
//! `tempnam` puts its name in `$TMPDIR` if that is a directory, else in its
//! argument if that is one, else in `/tmp` (`P_tmpdir`); `tmpnam`,
//! `tmpnam_r` and `tmpfile` in `/tmp`.  No directory at all is `ENOENT`.
//! Trailing slashes are dropped (all but a lone `/`), and at most five bytes
//! of the prefix are kept (`file` if there is none, `tmpf` for `tmpfile`).
//!
//! One difference from glibc: its directory test is a `stat` whose failure
//! it leaves in `errno`, so a `tempnam` that succeeds after passing over a
//! missing directory answers `ENOENT` in it -- undoing, one layer up, the
//! care `__gen_tempname` takes to leave `errno` alone.  Here the test leaves
//! it alone too (design-decisions.md §1151).
//!
//! glibc 2.39's answers -- every template shape through every function,
//! the flags and modes files are made with, the directories and prefixes --
//! are `tempfile_oracle.txt` (posix/tools/oracle/tempfile_harness.py),
//! which the tests below replay through the functions themselves, over a
//! filesystem in memory.

use crate::errno;

/// How many names are drawn before `EEXIST`: glibc's `ATTEMPTS_MIN`, 62
/// cubed (and its `TMP_MAX`).
pub(crate) const ATTEMPTS: u32 = 62 * 62 * 62;

/// What a name's `X`s become, in glibc's order.
const LETTERS: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// The `X`s a template ends in, before its suffix.
const X_LEN: usize = 6;

/// Base-62 digits one fair draw gives: 62^10 is below 2^64, 62^11 is not.
const DIGITS_PER_DRAW: u32 = 10;

/// 62^10: a draw at or above the largest multiple of it below 2^64 would
/// favour the low digits, and is drawn again.
const BASE_62_POWER: u64 = 839_299_365_868_340_224;

/// The mode a file is made with, before the umask.
pub(crate) const FILE_MODE: u32 = 0o600;

/// The mode a directory is made with, before the umask.
pub(crate) const DIR_MODE: u32 = 0o700;

/// What becomes of a free name: glibc's `__GT_FILE`, `__GT_DIR` and
/// `__GT_NOCREATE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A file, opened with [`file_flags`] of these and [`FILE_MODE`]: the
    /// descriptor is the answer.
    File(i32),
    /// A directory, [`DIR_MODE`]: 0.
    Dir,
    /// Nothing, once `lstat` says the name is free: 0.
    NoCreate,
}

/// The flags a temporary file is opened with: glibc's `try_file`, the
/// caller's with their access mode replaced by `O_RDWR`, and
/// `O_CREAT | O_EXCL`.
pub(crate) fn file_flags(flags: i32) -> i32 {
    use crate::fcntl::{O_ACCMODE, O_CREAT, O_EXCL, O_RDWR};
    (flags & !O_ACCMODE) | O_RDWR | O_CREAT | O_EXCL
}

/// One name tried: glibc's `try_file`, `try_dir` or `try_nocreate`.  The
/// descriptor or 0, or -1 with `errno` -- `EEXIST` for a name that is
/// taken.
fn attempt(name: *const u8, kind: Kind) -> i32 {
    #[cfg(test)]
    if let Some(r) = fake::attempt(name, kind) {
        return r;
    }
    match kind {
        Kind::File(flags) => crate::file::open(name, file_flags(flags), FILE_MODE),
        Kind::Dir => crate::file::mkdir(name, DIR_MODE),
        Kind::NoCreate => {
            // SAFETY: an all-zero `Stat` is a valid output buffer.
            let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
            if crate::file::lstat(name, &raw mut st) == 0 || errno::get_errno() == errno::EOVERFLOW
            {
                errno::set_errno(errno::EEXIST);
                return -1;
            }
            if errno::get_errno() == errno::ENOENT {
                0
            } else {
                -1
            }
        }
    }
}

/// A draw that gives [`DIGITS_PER_DRAW`] base-62 digits without bias.
fn draw() -> u64 {
    let unfair_min = u64::MAX - u64::MAX % BASE_62_POWER;
    loop {
        let v =
            (u64::from(crate::random::arc4random()) << 32) | u64::from(crate::random::arc4random());
        if v < unfair_min {
            return v;
        }
    }
}

/// glibc's `__gen_tempname(tmpl, suffixlen, flags, kind)`: the template's
/// six `X`s before its last `suffixlen` bytes made a free name, and made
/// what `kind` says.  The descriptor (a file) or 0, or -1 with `errno`.
///
/// # Safety
///
/// `tmpl` must be a writable C string.
pub(crate) unsafe fn gen_tempname(tmpl: *mut u8, suffixlen: usize, kind: Kind) -> i32 {
    // SAFETY: the caller's contract.
    unsafe { gen_tempname_with(tmpl, suffixlen, &mut |name| attempt(name, kind)) }
}

/// `mkstemp`, `mkostemp`, `mkstemps` and `mkostemps`, and their large-file
/// names: a file made from the template with its last `suffixlen` bytes
/// kept, opened with `flags` as [`file_flags`] makes them.  A NULL
/// template (where glibc's faults) and a negative `suffixlen` (glibc's own
/// check) are `EINVAL`.
///
/// # Safety
///
/// `template` must be NULL or a writable C string.
pub(crate) unsafe fn make_file(template: *mut u8, suffixlen: i32, flags: i32) -> i32 {
    let Ok(suffix) = usize::try_from(suffixlen) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if template.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: a writable C string, the caller's.
    unsafe { gen_tempname(template, suffix, Kind::File(flags)) }
}

/// [`gen_tempname`] over another way of trying a name.
///
/// # Safety
///
/// `tmpl` must be a writable C string.
pub(crate) unsafe fn gen_tempname_with(
    tmpl: *mut u8,
    suffixlen: usize,
    try_name: &mut dyn FnMut(*const u8) -> i32,
) -> i32 {
    let saved = errno::get_errno();
    // SAFETY: a C string, the caller's.
    let len = unsafe { crate::string::strlen(tmpl) };
    let Some(start) = len
        .checked_sub(suffixlen)
        .and_then(|n| n.checked_sub(X_LEN))
    else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    for i in 0..X_LEN {
        // SAFETY: `start + i < start + X_LEN <= len`, inside the string.
        if unsafe { *tmpl.add(start.wrapping_add(i)) } != b'X' {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    let mut v = 0u64;
    let mut left = 0u32;
    for _ in 0..ATTEMPTS {
        for i in 0..X_LEN {
            if left == 0 {
                v = draw();
                left = DIGITS_PER_DRAW;
            }
            let pick = usize::try_from(v % 62).unwrap_or(0);
            let letter = LETTERS.get(pick).copied().unwrap_or(b'x');
            // SAFETY: as above -- one of the six `X`s' places.
            unsafe { *tmpl.add(start.wrapping_add(i)) = letter };
            v /= 62;
            left = left.wrapping_sub(1);
        }
        let r = try_name(tmpl);
        if r >= 0 {
            errno::set_errno(saved);
            return r;
        }
        if errno::get_errno() != errno::EEXIST {
            return -1;
        }
    }
    errno::set_errno(errno::EEXIST);
    -1
}

/// Whether `path` names a directory (glibc's `direxists`), `errno` left as
/// it was.
pub(crate) fn dir_exists(path: *const u8) -> bool {
    #[cfg(test)]
    if let Some(d) = fake::dir_exists(path) {
        return d;
    }
    let saved = errno::get_errno();
    // SAFETY: an all-zero `Stat` is a valid output buffer.
    let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
    let ok = crate::file::stat(path, &raw mut st) == 0 && st.st_mode & 0o170_000 == 0o040_000;
    errno::set_errno(saved);
    ok
}

/// glibc's `__path_search(buf, len, dir, pfx, try_tmpdir)`: `D/PPPPPXXXXXX`
/// into `buf`, NUL-terminated -- the directory `D` `$TMPDIR` (with
/// `try_tmpdir`, if a directory), else `dir` (if it is one, with
/// `try_tmpdir`; as given, without), else `/tmp` if a directory; and up to
/// five bytes of `pfx`, `file` if NULL or empty.  The length before the
/// `X`s, or `None` with `ENOENT` (no directory) or `EINVAL` (does not fit).
///
/// # Safety
///
/// `dir` and `pfx` are NULL or C strings.
pub(crate) unsafe fn path_search(
    buf: &mut [u8],
    dir: *const u8,
    pfx: *const u8,
    try_tmpdir: bool,
) -> Option<usize> {
    let mut d = dir;
    if try_tmpdir {
        // SAFETY: a C string.
        let env = unsafe { crate::environ::secure_lookup(c"TMPDIR".as_ptr().cast()) };
        if !env.is_null() && dir_exists(env) {
            d = env;
        } else if dir.is_null() || !dir_exists(dir) {
            d = core::ptr::null();
        }
    }
    if d.is_null() {
        let tmp = c"/tmp".as_ptr().cast::<u8>();
        if !dir_exists(tmp) {
            errno::set_errno(errno::ENOENT);
            return None;
        }
        d = tmp;
    }
    // SAFETY: C strings (the caller's, the environment's, or ours).
    let dir_bytes = unsafe { core::slice::from_raw_parts(d, crate::string::strlen(d)) };
    let mut dlen = dir_bytes.len();
    while dlen > 1 && dir_bytes.get(dlen.wrapping_sub(1)) == Some(&b'/') {
        dlen = dlen.wrapping_sub(1);
    }
    // SAFETY: as above.
    let pfx_bytes: &[u8] = if pfx.is_null() || unsafe { *pfx } == 0 {
        b"file"
    } else {
        // SAFETY: as above.
        unsafe { core::slice::from_raw_parts(pfx, crate::string::strlen(pfx)) }
    };
    let plen = pfx_bytes.len().min(5);
    let stem = dlen.saturating_add(1).saturating_add(plen);
    if stem.saturating_add(X_LEN).saturating_add(1) > buf.len() {
        errno::set_errno(errno::EINVAL);
        return None;
    }
    let parts: [&[u8]; 4] = [
        dir_bytes.get(..dlen).unwrap_or(&[]),
        b"/",
        pfx_bytes.get(..plen).unwrap_or(&[]),
        b"XXXXXX\0",
    ];
    let mut at = 0usize;
    for part in parts {
        for &b in part {
            if let Some(slot) = buf.get_mut(at) {
                *slot = b;
            }
            at = at.wrapping_add(1);
        }
    }
    Some(stem)
}

/// Remove the name a temporary file was made under (`tmpfile`'s, when its
/// stream is closed), `errno` left as it was: a name already gone, or one
/// that cannot be removed, is no failure of the close.
pub(crate) fn remove_name(name: *const u8) {
    #[cfg(test)]
    if fake::remove(name) {
        return;
    }
    let saved = errno::get_errno();
    // Deliberately unchecked: see above.
    let _ = crate::file::unlink(name);
    errno::set_errno(saved);
}

/// A filesystem in memory for the tests, consulted by [`attempt`],
/// [`dir_exists`] and [`remove_name`] while one is installed on the test's
/// thread: the replay of glibc's answers needs directories that exist and
/// do not, a file where a directory is expected, and names taken.
#[cfg(test)]
pub(crate) mod fake {
    use super::Kind;
    use crate::errno;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::vec::Vec;

    /// What a path is.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Node {
        Dir,
        File,
    }

    /// The filesystem: nodes by path (relative ones in the current
    /// directory), what was opened and removed, and the umask.
    #[derive(Debug, Default)]
    pub(crate) struct Fs {
        pub(crate) nodes: BTreeMap<Vec<u8>, (Node, u32)>,
        /// Each file opened: its name, flags and mode after the umask.
        pub(crate) opened: Vec<(Vec<u8>, i32, u32)>,
        pub(crate) removed: Vec<Vec<u8>>,
        pub(crate) umask: u32,
    }

    impl Fs {
        /// The harness's directory: `sub` a directory, `file` a file, and
        /// `/` and `/tmp`.
        pub(crate) fn harness() -> Self {
            let mut fs = Self::default();
            for (p, n) in [
                (&b"/"[..], Node::Dir),
                (b"/tmp", Node::Dir),
                (b"sub", Node::Dir),
                (b"file", Node::File),
            ] {
                fs.nodes.insert(p.to_vec(), (n, 0o755));
            }
            fs
        }

        /// A path's trailing slashes dropped (but a lone `/`).
        fn norm(path: &[u8]) -> &[u8] {
            let mut p = path;
            while p.len() > 1 && p.last() == Some(&b'/') {
                p = &p[..p.len() - 1];
            }
            p
        }

        /// Where a new entry at `path` would go: `Ok`, or the errno.
        fn parent_ok(&self, path: &[u8]) -> Result<(), i32> {
            let Some(cut) = path.iter().rposition(|&b| b == b'/') else {
                return Ok(());
            };
            let parent = if cut == 0 { &b"/"[..] } else { &path[..cut] };
            match self.nodes.get(Self::norm(parent)) {
                Some((Node::Dir, _)) => Ok(()),
                Some((Node::File, _)) => Err(errno::ENOTDIR),
                None => Err(errno::ENOENT),
            }
        }

        fn create(&mut self, path: &[u8], node: Node, mode: u32) -> Result<(), i32> {
            self.parent_ok(path)?;
            if self.nodes.contains_key(Self::norm(path)) {
                return Err(errno::EEXIST);
            }
            self.nodes
                .insert(Self::norm(path).to_vec(), (node, mode & !self.umask));
            Ok(())
        }
    }

    std::thread_local! {
        static FS: RefCell<Option<Fs>> = const { RefCell::new(None) };
    }

    /// Use `fs` on this thread until [`take`].
    pub(crate) fn install(fs: Fs) {
        FS.with(|c| *c.borrow_mut() = Some(fs));
    }

    /// Stop using the filesystem, and have it back.
    pub(crate) fn take() -> Option<Fs> {
        FS.with(|c| c.borrow_mut().take())
    }

    /// Look at the filesystem in place.
    pub(crate) fn with<R>(f: impl FnOnce(&mut Fs) -> R) -> Option<R> {
        FS.with(|c| c.borrow_mut().as_mut().map(f))
    }

    fn bytes(p: *const u8) -> Vec<u8> {
        // SAFETY: a C string (every caller's name is one).
        unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) }.to_vec()
    }

    /// [`super::attempt`] in memory: a file's descriptor is a real one of
    /// the host's table, so that a stream can be made on it.
    pub(crate) fn attempt(name: *const u8, kind: Kind) -> Option<i32> {
        let path = bytes(name);
        with(|fs| {
            let made = match kind {
                Kind::File(flags) => {
                    let mode = super::FILE_MODE & !fs.umask;
                    fs.create(&path, Node::File, super::FILE_MODE).map(|()| {
                        use crate::fcntl::{O_CREAT, O_EXCL};
                        let open = super::file_flags(flags);
                        fs.opened.push((path.clone(), open, mode));
                        // The status flags `fcntl(F_GETFL)` gives back, as
                        // the kernel keeps them: not the creation flags.
                        crate::fdtable::alloc_fd_with_flags(
                            crate::fdtable::HandleKind::File,
                            0,
                            open & !(O_CREAT | O_EXCL),
                        )
                        .unwrap_or(-1)
                    })
                }
                Kind::Dir => fs.create(&path, Node::Dir, super::DIR_MODE).map(|()| 0),
                // `lstat`: a missing directory on the way is as free a
                // name as a missing file (ENOENT); a file on the way is not
                // (ENOTDIR).
                Kind::NoCreate => match fs.parent_ok(&path) {
                    Err(errno::ENOENT) => Ok(0),
                    Err(e) => Err(e),
                    Ok(()) if fs.nodes.contains_key(Fs::norm(&path)) => Err(errno::EEXIST),
                    Ok(()) => Ok(0),
                },
            };
            made.unwrap_or_else(|e| {
                errno::set_errno(e);
                -1
            })
        })
    }

    /// [`super::dir_exists`] in memory.
    pub(crate) fn dir_exists(path: *const u8) -> Option<bool> {
        let p = bytes(path);
        with(|fs| matches!(fs.nodes.get(Fs::norm(&p)), Some((Node::Dir, _))))
    }

    /// [`super::remove_name`] in memory: `true` if a filesystem is installed.
    pub(crate) fn remove(name: *const u8) -> bool {
        let p = bytes(name);
        with(|fs| {
            fs.nodes.remove(Fs::norm(&p));
            fs.removed.push(p);
        })
        .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{self, Fs, Node};
    use super::*;
    use crate::fcntl;
    use std::format;
    use std::string::{String, ToString};
    use std::vec::Vec;

    /// glibc 2.39's answers (posix/tools/oracle/tempfile_harness.py).
    const ORACLE: &str = include_str!("tempfile_oracle.txt");

    /// The harness's `RUNS`: each template goes through a function this
    /// many times, and a position any run replaced is a random one.
    const RUNS: usize = 8;

    /// The harness's `errno` value before each call, and its word for it.
    const KEPT: i32 = 12345;

    fn errno_word(e: i32) -> String {
        match e {
            KEPT => "kept".into(),
            0 => "0".into(),
            crate::errno::EINVAL => "EINVAL".into(),
            crate::errno::ENOENT => "ENOENT".into(),
            crate::errno::EEXIST => "EEXIST".into(),
            crate::errno::ENOTDIR => "ENOTDIR".into(),
            crate::errno::EACCES => "EACCES".into(),
            crate::errno::EISDIR => "EISDIR".into(),
            crate::errno::ELOOP => "ELOOP".into(),
            crate::errno::ENAMETOOLONG => "ENAMETOOLONG".into(),
            crate::errno::EOPNOTSUPP => "EOPNOTSUPP".into(),
            crate::errno::EFAULT => "EFAULT".into(),
            e => format!("e{e}"),
        }
    }

    /// The oracle's lines as (probe, answer).
    fn oracle() -> Vec<(&'static str, &'static str)> {
        ORACLE
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once(" = "))
            .collect()
    }

    /// A C string of `bytes`, with room for nothing more.
    fn c(bytes: &[u8]) -> Vec<u8> {
        let mut v = bytes.to_vec();
        v.push(0);
        v
    }

    /// The C string at `p`.
    fn text(p: *const u8) -> Vec<u8> {
        // SAFETY: a C string (the functions' answers are).
        unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) }.to_vec()
    }

    /// One of the ten functions on `buf`: its answer as the harness words
    /// it (`fd`, `-1`, `tmpl`, `NULL`), with the descriptor closed.
    fn call(f: &str, buf: &mut [u8], suffix: i32) -> &'static str {
        use crate::stdlib::{
            mkdtemp, mkostemp, mkostemp64, mkostemps, mkostemps64, mkstemp, mkstemp64, mkstemps,
            mkstemps64, mktemp,
        };
        let t = buf.as_mut_ptr();
        // SAFETY: `buf` is a writable C string.
        let fd = unsafe {
            match f {
                "mkstemp" => mkstemp(t),
                "mkstemp64" => mkstemp64(t),
                "mkostemp" => mkostemp(t, 0),
                "mkostemp64" => mkostemp64(t, 0),
                "mkstemps" => mkstemps(t, suffix),
                "mkstemps64" => mkstemps64(t, suffix),
                "mkostemps" => mkostemps(t, suffix, 0),
                "mkostemps64" => mkostemps64(t, suffix, 0),
                "mkdtemp" => {
                    let r = mkdtemp(t);
                    return if r.is_null() {
                        "NULL"
                    } else if core::ptr::eq(r, t) {
                        "tmpl"
                    } else {
                        "other"
                    };
                }
                "mktemp" => {
                    let r = mktemp(t);
                    return if r.is_null() {
                        "NULL"
                    } else if core::ptr::eq(r, t) {
                        "tmpl"
                    } else {
                        "other"
                    };
                }
                other => panic!("{other}"),
            }
        };
        if fd < 0 {
            "-1"
        } else {
            let _ = crate::fdtable::close_fd(fd);
            "fd"
        }
    }

    /// What the harness writes for `f("tmpl", suffix)`, from this library.
    fn replay(f: &str, tmpl: &[u8], suffix: i32) -> String {
        fake::install(Fs::harness());
        let mut state = std::vec![0u8; tmpl.len()];
        let mut first: Option<String> = None;
        for _ in 0..RUNS {
            let mut buf = c(tmpl);
            crate::errno::set_errno(KEPT);
            let rc = call(f, &mut buf, suffix);
            let e = crate::errno::get_errno();
            let mut out = format!(" {rc} errno={}", errno_word(e));
            let name = buf[..tmpl.len()].to_vec();
            let node = fake::with(|fs| fs.nodes.get(&name).map(|n| n.0)).flatten();
            if rc == "fd" && node == Some(Node::File) {
                out.push_str(" file");
            } else if f == "mkdtemp" && rc == "tmpl" && node == Some(Node::Dir) {
                out.push_str(" dir");
            } else if f == "mktemp" && rc == "tmpl" && buf.first() != Some(&0) {
                out.push_str(if node.is_some() { " exists" } else { " absent" });
            }
            for (i, (&a, &b)) in buf.iter().zip(tmpl).enumerate() {
                let s = if a == 0 {
                    3
                } else if a == b {
                    0
                } else if a.is_ascii_alphanumeric() {
                    1
                } else {
                    2
                };
                state[i] = state[i].max(s);
            }
            match &first {
                None => first = Some(out),
                Some(f0) if *f0 != out => first = Some(" unstable".into()),
                Some(_) => {}
            }
        }
        fake::take();
        let shape: String = state
            .iter()
            .zip(tmpl)
            .map(|(&s, &b)| match s {
                0 => b as char,
                1 => '*',
                2 => '?',
                _ => '@',
            })
            .collect();
        format!("{} [{shape}]", first.unwrap_or_default())
    }

    /// Every template through every function, as glibc answered: which are
    /// refused, which bytes a name takes, what is made, and `errno`.
    #[test]
    fn templates_are_glibcs() {
        let mut n = 0;
        for (probe, want) in oracle() {
            if probe == "mkstemp(\"XXXXXX\") with names taken" {
                // Two names from one template: both made, and different.
                fake::install(Fs::harness());
                let (mut a, mut b) = (c(b"XXXXXX"), c(b"XXXXXX"));
                // SAFETY: writable C strings.
                let fds = unsafe {
                    [
                        crate::stdlib::mkstemp(a.as_mut_ptr()),
                        crate::stdlib::mkstemp(b.as_mut_ptr()),
                    ]
                };
                fake::take();
                let got = format!(
                    "{} {}",
                    if fds[0] >= 0 { "fd" } else { "-1" },
                    if fds[1] >= 0 && a != b {
                        "distinct"
                    } else {
                        "same"
                    }
                );
                assert_eq!(got, want);
                for fd in fds {
                    let _ = crate::fdtable::close_fd(fd);
                }
                continue;
            }
            let Some((f, rest)) = probe.split_once("(\"") else {
                continue;
            };
            let Some((tmpl, suffix)) = rest.rsplit_once("\", ") else {
                continue;
            };
            let suffix: i32 = suffix.trim_end_matches(')').parse().unwrap();
            let got = replay(f, tmpl.as_bytes(), suffix);
            assert_eq!(got.trim_start(), want, "{probe}");
            n += 1;
        }
        assert_eq!(n, 232, "the oracle's template probes");
    }

    /// The flags a name for `mkostemp(flags)` would describe, as the
    /// harness's `fd_flags` words a descriptor Linux opened with them.
    fn described(open_flags: i32) -> String {
        use crate::fcntl::{
            O_ACCMODE, O_APPEND, O_CLOEXEC, O_DSYNC, O_NOATIME, O_NONBLOCK, O_RDONLY, O_RDWR,
            O_SYNC, O_WRONLY,
        };
        let acc = open_flags & O_ACCMODE;
        let mut s = String::from(match acc {
            O_RDONLY => "rdonly",
            O_WRONLY => "wronly",
            O_RDWR => "rdwr",
            _ => "acc3",
        });
        if open_flags & O_APPEND != 0 {
            s.push_str("|append");
        }
        if open_flags & O_SYNC == O_SYNC {
            s.push_str("|sync");
        } else if open_flags & O_DSYNC != 0 {
            s.push_str("|dsync");
        }
        if open_flags & O_NONBLOCK != 0 {
            s.push_str("|nonblock");
        }
        if open_flags & O_NOATIME != 0 {
            s.push_str("|noatime");
        }
        if open_flags & O_ASYNC != 0 {
            s.push_str("|async");
        }
        if open_flags & O_CLOEXEC != 0 {
            s.push_str(" cloexec");
        }
        s
    }

    /// Linux's `O_ASYNC` (`FASYNC`), which crate::fcntl has no name for.
    const O_ASYNC: i32 = 0o20_000;

    fn flag_value(name: &str) -> i32 {
        use crate::fcntl::*;
        name.split('|')
            .map(|n| match n {
                "0" => 0,
                "O_WRONLY" => O_WRONLY,
                "O_RDWR" => O_RDWR,
                "O_ACCMODE" => O_ACCMODE,
                "O_APPEND" => O_APPEND,
                "O_CLOEXEC" => O_CLOEXEC,
                "O_SYNC" => O_SYNC,
                "O_DSYNC" => O_DSYNC,
                "O_NONBLOCK" => O_NONBLOCK,
                "O_NOCTTY" => O_NOCTTY,
                "O_TRUNC" => O_TRUNC,
                "O_CREAT" => O_CREAT,
                "O_EXCL" => O_EXCL,
                "O_NOFOLLOW" => O_NOFOLLOW,
                "O_DIRECTORY" => O_DIRECTORY,
                "O_PATH" => O_PATH,
                "O_TMPFILE" => O_TMPFILE,
                "O_NOATIME" => O_NOATIME,
                "O_ASYNC" => O_ASYNC,
                "-1" => -1,
                "0x40000000" => 0x4000_0000,
                other => panic!("{other}"),
            })
            .fold(0, |a, b| a | b)
    }

    /// The flags the `o` forms pass: the caller's, the access mode made
    /// `O_RDWR`, with `O_CREAT | O_EXCL` -- each open glibc's made one
    /// Linux described as the harness recorded, and each open Linux
    /// refused given the flag it refused, unfiltered.
    #[test]
    fn the_o_forms_open_with_glibcs_flags() {
        let mut n = 0;
        for (probe, want) in oracle() {
            let Some((f, flags)) = probe.split_once(" flags ") else {
                continue;
            };
            let flags = flag_value(flags);
            let mut fs = Fs::harness();
            fs.umask = 0o022;
            fake::install(fs);
            let mut buf = c(b"XXXXXX");
            let t = buf.as_mut_ptr();
            crate::errno::set_errno(KEPT);
            // SAFETY: a writable C string.
            let fd = unsafe {
                match f {
                    "mkostemp" => crate::stdlib::mkostemp(t, flags),
                    "mkostemp64" => crate::stdlib::mkostemp64(t, flags),
                    "mkostemps" => crate::stdlib::mkostemps(t, 0, flags),
                    "mkostemps64" => crate::stdlib::mkostemps64(t, 0, flags),
                    other => panic!("{other}"),
                }
            };
            assert!(fd >= 0, "{probe}: the fake makes every file");
            let _ = crate::fdtable::close_fd(fd);
            let fs = fake::take().unwrap();
            let [(_, open, mode)] = fs.opened[..] else {
                panic!("{probe}: one open")
            };
            assert_eq!(open, file_flags(flags), "{probe}");
            assert_eq!(mode, 0o600 & !0o022, "{probe}");
            // Everything the caller gave is passed on, but the access mode.
            assert_eq!(
                open & !fcntl::O_ACCMODE,
                (flags & !fcntl::O_ACCMODE) | fcntl::O_CREAT | fcntl::O_EXCL
            );
            match want.strip_prefix("fd errno=kept ") {
                Some(described_there) => assert_eq!(described(open), described_there, "{probe}"),
                None => assert!(want.starts_with("-1 errno="), "{probe}: {want}"),
            }
            n += 1;
        }
        assert_eq!(n, 92, "the oracle's flag probes");
    }

    /// Files are made 0600 and directories 0700, the umask applied after.
    #[test]
    fn modes_are_glibcs() {
        for (probe, want) in oracle() {
            let Some(umask) = probe.strip_prefix("modes under umask ") else {
                continue;
            };
            let umask = u32::from_str_radix(umask, 8).unwrap();
            let mut fs = Fs::harness();
            fs.umask = umask;
            fake::install(fs);
            let (mut a, mut b, mut cc, mut d) =
                (c(b"XXXXXX"), c(b"XXXXXX"), c(b"XXXXXX.c"), c(b"XXXXXX"));
            // SAFETY: writable C strings.
            let fds = unsafe {
                [
                    crate::stdlib::mkstemp(a.as_mut_ptr()),
                    crate::stdlib::mkostemp(b.as_mut_ptr(), fcntl::O_APPEND),
                    crate::stdlib::mkostemps(cc.as_mut_ptr(), 2, 0),
                ]
            };
            // SAFETY: as above.
            assert!(!unsafe { crate::stdlib::mkdtemp(d.as_mut_ptr()) }.is_null());
            for fd in fds {
                assert!(fd >= 0);
                let _ = crate::fdtable::close_fd(fd);
            }
            let fs = fake::take().unwrap();
            let mode = |name: &[u8]| fs.nodes[&name[..name.len() - 1]].1;
            let got = format!(
                "mkstemp {:04o} mkostemp {:04o} mkostemps {:04o} mkdtemp {:04o}",
                mode(&a),
                mode(&b),
                mode(&cc),
                mode(&d)
            );
            assert_eq!(got, want, "{probe}");
        }
    }

    /// `tempnam` looks where glibc's looks: `$TMPDIR`, the argument, `/tmp`
    /// -- each only if a directory -- and keeps five bytes of the prefix.
    /// Where glibc's answers `errno` `ENOENT` after a success (its
    /// directory test's, left behind), this one leaves `errno` alone
    /// (design-decisions.md §1151).
    #[test]
    fn tempnam_is_glibcs() {
        let mut n = 0;
        for (probe, want) in oracle() {
            let Some(rest) = probe.strip_prefix("tempnam(") else {
                continue;
            };
            let (args, env) = rest.split_once(") TMPDIR=").unwrap();
            let (dir, pfx) = args.split_once(", ").unwrap();
            let arg = |s: &str| (s != "NULL").then(|| c(s.as_bytes()));
            let (dir, pfx) = (arg(dir), arg(pfx));
            fake::install(Fs::harness());
            if env == "(unset)" {
                // SAFETY: a C string.
                unsafe { crate::environ::unsetenv(c"TMPDIR".as_ptr().cast()) };
            } else {
                let v = c(env.as_bytes());
                // SAFETY: C strings.
                unsafe { crate::environ::setenv(c"TMPDIR".as_ptr().cast(), v.as_ptr(), 1) };
            }
            let ptr = |o: &Option<Vec<u8>>| o.as_ref().map_or(core::ptr::null(), |v| v.as_ptr());
            crate::errno::set_errno(KEPT);
            // SAFETY: NULLs or C strings.
            let s = unsafe { crate::stdio::tempnam(ptr(&dir), ptr(&pfx)) };
            let e = crate::errno::get_errno();
            fake::take();
            let (glibc_head, glibc_name) = want.split_at(want.find(' ').unwrap());
            assert_eq!(glibc_head, "name", "{probe}: glibc made a name");
            assert!(!s.is_null(), "{probe}");
            let name = text(s);
            // SAFETY: tempnam's malloc'd answer, not used again.
            unsafe { crate::malloc::free(s) };
            let masked: String = {
                let cut = name.len() - 6;
                let mut m = String::from_utf8(name[..cut].to_vec()).unwrap();
                m.push_str("******");
                m
            };
            let glibc_errno = glibc_name.trim_start().split(' ').next().unwrap();
            let glibc_masked = glibc_name.trim_start().split(' ').nth(1).unwrap();
            assert_eq!(masked, glibc_masked, "{probe}");
            assert!(name[name.len() - 6..].iter().all(u8::is_ascii_alphanumeric));
            assert_eq!(errno_word(e), "kept", "{probe}");
            assert!(
                glibc_errno == "errno=kept" || glibc_errno == "errno=ENOENT",
                "{probe}: {glibc_errno}"
            );
            n += 1;
        }
        assert_eq!(n, 216, "the oracle's tempnam probes");
    }

    /// `tmpnam` and `tmpnam_r`: `/tmp/fileXXXXXX`, into the caller's buffer
    /// or the static one; `tmpnam_r` refuses NULL.
    #[test]
    fn tmpnam_is_glibcs() {
        let want: std::collections::BTreeMap<_, _> = oracle().into_iter().collect();
        assert_eq!(
            want["tmpnam(NULL)"],
            "name errno=kept /tmp/file****** len 15"
        );
        assert_eq!(
            want["tmpnam(buf)"],
            format!("buf L_tmpnam {}", crate::stdio::L_TMPNAM)
        );
        assert_eq!(want["tmpnam_r(NULL)"], "NULL errno=kept");
        assert_eq!(want["tmpnam_r(buf)"], "buf errno=kept /tmp/file******");
        assert_eq!(want["TMP_MAX"], ATTEMPTS.to_string());
        assert_eq!(want["P_tmpdir"], "/tmp");

        fake::install(Fs::harness());
        crate::errno::set_errno(KEPT);
        let s = crate::stdio::tmpnam(core::ptr::null_mut());
        assert!(!s.is_null());
        let name = text(s);
        assert_eq!((&name[..9], name.len()), (&b"/tmp/file"[..], 15));
        assert_eq!(crate::errno::get_errno(), KEPT);
        let mut buf = [0u8; crate::stdio::L_TMPNAM];
        assert_eq!(crate::stdio::tmpnam(buf.as_mut_ptr()), buf.as_mut_ptr());
        // SAFETY: NULL, and a buffer of L_tmpnam bytes.
        unsafe {
            assert!(crate::unistd::tmpnam_r(core::ptr::null_mut()).is_null());
            assert_eq!(crate::unistd::tmpnam_r(buf.as_mut_ptr()), buf.as_mut_ptr());
        }
        assert!(text(buf.as_ptr()).starts_with(b"/tmp/file"));
        assert_eq!(crate::errno::get_errno(), KEPT);
        fake::take();

        // With no /tmp, no name: ENOENT.
        let mut fs = Fs::harness();
        fs.nodes.remove(&b"/tmp"[..]);
        fake::install(fs);
        crate::errno::set_errno(0);
        assert!(crate::stdio::tmpnam(core::ptr::null_mut()).is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
        fake::take();
    }

    /// `tmpfile`: glibc's nameless file cannot be made here, so a named one,
    /// `/tmp/tmpfXXXXXX`, 0600, `O_RDWR` -- whatever `$TMPDIR` says -- and
    /// the name is removed when the stream is closed, not before.
    #[test]
    fn tmpfile_is_made_as_glibcs_and_removed_on_close() {
        let lines: Vec<_> = oracle()
            .into_iter()
            .filter(|(p, _)| p.starts_with("tmpfile"))
            .collect();
        assert_eq!(lines.len(), 4);
        for (probe, want) in &lines[..3] {
            // glibc's: nameless (link count 0) -- the one difference.
            assert_eq!(
                *want, "stream errno=kept rdwr mode 0600 nlink 0 rw",
                "{probe}"
            );
        }
        for env in [None, Some(&b"/nonexistent"[..]), Some(b"sub")] {
            let mut fs = Fs::harness();
            fs.umask = 0o022;
            fake::install(fs);
            match env {
                None => {
                    // SAFETY: a C string.
                    unsafe { crate::environ::unsetenv(c"TMPDIR".as_ptr().cast()) };
                }
                Some(v) => {
                    let v = c(v);
                    // SAFETY: C strings.
                    unsafe { crate::environ::setenv(c"TMPDIR".as_ptr().cast(), v.as_ptr(), 1) };
                }
            }
            crate::errno::set_errno(KEPT);
            let f = if env.is_some() {
                crate::stdlib::tmpfile64()
            } else {
                crate::stdlib::tmpfile()
            };
            assert!(!f.is_null());
            assert_eq!(crate::errno::get_errno(), KEPT, "errno kept");
            let (name, open, mode) = fake::with(|fs| fs.opened[0].clone()).unwrap();
            assert_eq!(&name[..9], b"/tmp/tmpf");
            assert_eq!(name.len(), 15);
            assert_eq!(open, fcntl::O_RDWR | fcntl::O_CREAT | fcntl::O_EXCL);
            assert_eq!(mode, 0o600 & !0o022);
            assert!(
                fake::with(|fs| fs.removed.is_empty()).unwrap(),
                "not while open"
            );
            // (The host's close of a descriptor the fake made fails -- its
            // system call is a stub -- and the stream is gone either way.)
            let _ = crate::stdio::fclose(f);
            assert_eq!(
                fake::with(|fs| fs.removed.clone()).unwrap(),
                std::slice::from_ref(&name)
            );
            assert!(!fake::take().unwrap().nodes.contains_key(&name));
        }
    }

    /// A name taken is drawn again; after [`ATTEMPTS`] draws, `EEXIST`,
    /// the template holding the last.  Any other failure ends it at once.
    #[test]
    fn names_taken_are_drawn_again_and_then_eexist() {
        let mut tries = 0u32;
        let mut buf = c(b"a-XXXXXX");
        crate::errno::set_errno(KEPT);
        // SAFETY: a writable C string.
        let r = unsafe {
            gen_tempname_with(buf.as_mut_ptr(), 0, &mut |_| {
                tries += 1;
                crate::errno::set_errno(crate::errno::EEXIST);
                -1
            })
        };
        assert_eq!((r, crate::errno::get_errno()), (-1, crate::errno::EEXIST));
        assert_eq!(tries, ATTEMPTS);
        assert!(buf[2..8].iter().all(u8::is_ascii_alphanumeric));

        let mut seen = std::collections::BTreeSet::new();
        let mut buf = c(b"XXXXXX");
        // SAFETY: as above.
        let r = unsafe {
            gen_tempname_with(buf.as_mut_ptr(), 0, &mut |p| {
                let name = text(p);
                assert!(seen.insert(name), "a name drawn twice");
                if seen.len() < 5 {
                    crate::errno::set_errno(crate::errno::EEXIST);
                    -1
                } else {
                    crate::errno::set_errno(crate::errno::EACCES);
                    -1
                }
            })
        };
        assert_eq!((r, crate::errno::get_errno()), (-1, crate::errno::EACCES));
        assert_eq!(seen.len(), 5);
    }

    /// The digits are the 62 letters and digits, every one reached, none
    /// favoured much: 62,000 draws of six, each letter near 6,000 times.
    #[test]
    fn names_use_every_letter_evenly() {
        let mut counts = [0u32; 256];
        for _ in 0..10_000 {
            let mut buf = c(b"XXXXXX");
            // SAFETY: a writable C string.
            let r = unsafe { gen_tempname_with(buf.as_mut_ptr(), 0, &mut |_| 0) };
            assert_eq!(r, 0);
            for &b in &buf[..6] {
                counts[usize::from(b)] += 1;
            }
        }
        for &l in LETTERS {
            let n = counts[usize::from(l)];
            assert!(
                (800..1200).contains(&n),
                "{} drawn {n} times of ~968",
                l as char
            );
        }
        let total: u32 = LETTERS.iter().map(|&l| counts[usize::from(l)]).sum();
        assert_eq!(total, 60_000);
    }

    /// A NULL template, where glibc's faults, is `EINVAL`, as a negative
    /// suffix is; `mktemp` answers NULL for it.
    #[test]
    fn null_and_negative_suffix_are_einval() {
        // SAFETY: NULL templates, refused before any is read.
        unsafe {
            for r in [
                crate::stdlib::mkstemp(core::ptr::null_mut()),
                crate::stdlib::mkstemp64(core::ptr::null_mut()),
                crate::stdlib::mkostemps(core::ptr::null_mut(), 0, 0),
                crate::stdlib::mkstemps64(core::ptr::null_mut(), -1),
            ] {
                assert_eq!(r, -1);
                assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            }
            crate::errno::set_errno(0);
            assert!(crate::stdlib::mkdtemp(core::ptr::null_mut()).is_null());
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            crate::errno::set_errno(0);
            assert!(crate::stdlib::mktemp(core::ptr::null_mut()).is_null());
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        }
    }
}
