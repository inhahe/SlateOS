//! `write_entry.c`'s filesystem half, and `db_iterator.c`'s note of where
//! `tic` writes: the directory it settles on (`_nc_set_writedir`, which
//! changes into it) and each entry written there under its names
//! (`_nc_write_entry`) -- the first name a file, the others symbolic links
//! to it (`--enable-symlinks`), one directory per first character.
//!
//! A name written twice in one run is noticed by the file's time: anything
//! changed since the first file this run wrote is this run's. A file older
//! than that, under an alias, is left alone with a warning.

use std::ffi::OsString;
use std::io::Write;

use coreutils::errmsg;
use terminfo::compile::Abort;
use terminfo::compile::scan::{Scanner, cstr};
use terminfo::compile::write::write_object;
use terminfo::termtype::TermType;

/// `TERMINFO`, the compiled-in default directory.
const TERMINFO: &[u8] = b"/etc/terminfo";
/// `PATH_MAX`.
const PATH_MAX: usize = 4096;
/// `MAX_ENTRY_SIZE`: `write_file`'s buffer.
const MAX_ENTRY_SIZE: usize = 32768;
/// `MAX_TERMINFO_LENGTH`: `name_list`'s size.
const MAX_TERMINFO_LENGTH: usize = 4096;
/// `LEAF_LEN`.
const LEAF_LEN: usize = 1;
/// The subdirectories an entry may go in.
const DIRNAMES: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// `R_OK`, `W_OK`, `X_OK`.
pub const R_OK: i32 = 4;
pub const W_OK: i32 = 2;
pub const X_OK: i32 = 1;

/// A file name's bytes as the `OsString` the platform opens.
#[cfg(unix)]
pub fn os(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(cstr(b).to_vec())
}

/// A file name's bytes, as near as a host without byte paths comes.
#[cfg(not(unix))]
pub fn os(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(cstr(b)).into_owned())
}

#[cfg(unix)]
unsafe extern "C" {
    fn access(path: *const u8, mode: i32) -> i32;
}

/// `access (path, mode)`: `Ok`, or the errno it failed with.
#[cfg(unix)]
pub fn sys_access(path: &[u8], mode: i32) -> Result<(), i32> {
    let mut c = cstr(path).to_vec();
    c.push(0);
    // SAFETY: `c` is NUL-terminated with no interior NUL and outlives the
    // call, which only reads it.
    let rc = unsafe { access(c.as_ptr(), mode) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
    }
}

/// `access` on a host without a POSIX permission model: existence, and the
/// read-only attribute for writing.
#[cfg(not(unix))]
pub fn sys_access(path: &[u8], mode: i32) -> Result<(), i32> {
    match std::fs::metadata(os(path)) {
        Ok(m) if mode & W_OK != 0 && m.permissions().readonly() => Err(13),
        Ok(_) => Ok(()),
        Err(_) => Err(2),
    }
}

/// `ENOENT`.
const ENOENT: i32 = 2;
/// `EEXIST`.
const EEXIST: i32 = 17;
/// `EPERM`.
const EPERM: i32 = 1;

/// `_nc_access (path, mode)`: `access`, except that a file to be written that
/// does not exist yet is judged by its directory.
pub fn nc_access(path: &[u8], mode: i32) -> bool {
    match sys_access(path, mode) {
        Ok(()) => true,
        Err(e) if mode & W_OK != 0 && e == ENOENT && cstr(path).len() < PATH_MAX => {
            let path = cstr(path);
            let leaf = path
                .iter()
                .rposition(|&c| c == b'/')
                .map_or(0, |p| p.wrapping_add(1));
            let head: &[u8] = if leaf == 0 {
                b"."
            } else {
                path.get(..leaf).unwrap_or(b".")
            };
            sys_access(head, R_OK | W_OK | X_OK).is_ok()
        }
        Err(_) => false,
    }
}

/// `stat (path)`'s modification time and whether it is a directory.
fn stat(path: &[u8]) -> Option<(i64, bool)> {
    let m = std::fs::metadata(os(path)).ok()?;
    Some((mtime(&m), m.is_dir()))
}

/// `st_mtime`.
#[cfg(unix)]
fn mtime(m: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    m.mtime()
}

/// `st_mtime`, as near as the host has it.
#[cfg(not(unix))]
fn mtime(m: &std::fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// `strerror (errno)`.
fn strerror(errno: i32) -> String {
    errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
}

/// `perror (s)`.
fn perror(s: &[u8], errno: i32) {
    let mut m = s.to_vec();
    m.extend_from_slice(format!(": {}\n", strerror(errno)).as_bytes());
    ulclosestream::stderr_write(&m);
}

/// The errno of a failed operation.
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(0)
}

/// Where `tic` writes, and what it has written.
pub struct Writer {
    /// `TicDirectory`.
    tic_dir: Option<Vec<u8>>,
    /// `HaveTicDirectory`.
    have: bool,
    /// `KeepTicDirectory`.
    keep: bool,
    /// The first file's time: what this run wrote is newer.
    start_time: i64,
    call_count: u32,
    /// `check_writeable`'s note of the subdirectories made or found.
    verified: [bool; 62],
    /// `total_written`.
    pub total_written: u32,
    /// `_nc_user_definable`.
    pub user_definable: bool,
    /// The environment, for `$TERMINFO` and `$HOME`.
    pub env: terminfo::Env,
}

impl Writer {
    /// A writer that has written nothing.
    pub fn new(env: terminfo::Env, user_definable: bool) -> Self {
        Self {
            tic_dir: None,
            have: false,
            keep: false,
            start_time: 0,
            call_count: 0,
            verified: [false; 62],
            total_written: 0,
            user_definable,
            env,
        }
    }

    /// `_nc_tic_dir (path)`: the directory written to, `path` from now on
    /// unless it is fixed already; with none, `$TERMINFO` once, else the
    /// compiled-in default.
    pub fn tic_dir(&mut self, path: Option<&[u8]>) -> Vec<u8> {
        if !self.keep {
            match path {
                Some(p) => {
                    self.tic_dir = Some(p.to_vec());
                    self.have = true;
                }
                None => {
                    if !self.have
                        && self.env.trusted
                        && let Some(t) = self.env.terminfo.clone()
                    {
                        return self.tic_dir(Some(&t));
                    }
                }
            }
        }
        self.tic_dir.clone().unwrap_or_else(|| TERMINFO.to_vec())
    }

    /// `_nc_keep_tic_dir (path)`.
    fn keep_tic_dir(&mut self, path: &[u8]) {
        self.tic_dir(Some(path));
        self.keep = true;
    }

    /// The directory, as it is now: `_nc_tic_dir (NULL)` without its first
    /// look at `$TERMINFO`'s side effect where it has already happened.
    pub fn current_dir(&mut self) -> Vec<u8> {
        self.tic_dir(None)
    }

    /// `_nc_home_terminfo ()`: `$HOME/.terminfo`.
    pub fn home_terminfo(&self) -> Option<Vec<u8>> {
        if !self.env.trusted {
            return None;
        }
        let home = self.env.home.as_ref()?;
        let mut v = home.clone();
        v.extend_from_slice(b"/.terminfo");
        Some(v)
    }

    /// `make_db_root (path)`: the directory made if it is not there; 0, or
    /// -1 with the errno, when it cannot be written in.
    fn make_db_root(&mut self, path: &[u8]) -> Result<(), i32> {
        // `make_db_path`: the name must fit, made absolute or not.
        let top = self.current_dir();
        let fits = if path == top.as_slice() || path.first() == Some(&b'/') {
            path.len() < PATH_MAX
        } else {
            top.len().saturating_add(path.len()).saturating_add(6) <= PATH_MAX
        };
        if !fits {
            return Err(0);
        }
        match std::fs::metadata(os(path)) {
            Err(_) => {
                #[cfg(unix)]
                let b = {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut b = std::fs::DirBuilder::new();
                    b.mode(0o777);
                    b
                };
                #[cfg(not(unix))]
                let b = std::fs::DirBuilder::new();
                b.create(os(path)).map_err(|e| errno_of(&e))
            }
            Ok(m) => {
                if let Err(e) = sys_access(path, R_OK | W_OK | X_OK) {
                    Err(e)
                } else if !m.is_dir() {
                    Err(20)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// `_nc_set_writedir (dir)`: the directory written to settled, made,
    /// and made the working directory -- `$TERMINFO`, or the default, or
    /// `$HOME/.terminfo` when neither can be written in.
    ///
    /// # Errors
    ///
    /// Upstream's abort, its message written.
    pub fn set_writedir(
        &mut self,
        scan: &mut Scanner<'_>,
        dir: Option<&[u8]>,
    ) -> Result<(), Abort> {
        let specific = dir.is_some();
        let mut dir = dir.map(<[u8]>::to_vec);
        if !specific && self.env.trusted {
            dir = self.env.terminfo.clone();
        }
        if let Some(d) = &dir {
            self.tic_dir(Some(d));
        }
        let mut destination = self.current_dir();
        if let Err(errno) = self.make_db_root(&destination) {
            let mut success = false;
            let mut errno = errno;
            if !specific && let Some(home) = self.home_terminfo() {
                destination = home;
                match self.make_db_root(&destination) {
                    Ok(()) => success = true,
                    Err(e) => errno = e,
                }
            }
            if !success {
                let mut m = destination.clone();
                m.extend_from_slice(format!(": permission denied (errno {errno})").as_bytes());
                return Err(scan.err_abort(&m));
            }
        }
        let dest = self.tic_dir(Some(&destination));
        let actual = std::env::set_current_dir(os(&dest))
            .ok()
            .and_then(|()| std::env::current_dir().ok());
        let Some(actual) = actual else {
            let mut m = destination.clone();
            m.extend_from_slice(b": not a directory");
            return Err(scan.err_abort(&m));
        };
        let actual = coreutils::quote::os_bytes(actual.as_os_str()).into_owned();
        self.keep_tic_dir(&actual);
        Ok(())
    }

    /// `check_writeable (code)`: the subdirectory for names starting with
    /// `code`, made if need be.
    fn check_writeable(&mut self, scan: &mut Scanner<'_>, code: u8) -> Result<(), Abort> {
        let Some(at) = DIRNAMES
            .iter()
            .position(|&c| c == code)
            .filter(|_| code != 0)
        else {
            let mut m = b"Illegal terminfo subdirectory \"".to_vec();
            m.push(code);
            m.push(b'"');
            return Err(scan.err_abort(&m));
        };
        if !self.verified.get(at).copied().unwrap_or(false) {
            if self.make_db_root(&[code]).is_err() {
                let mut m = self.current_dir();
                m.push(b'/');
                m.push(code);
                m.extend_from_slice(b": permission denied");
                return Err(scan.err_abort(&m));
            }
            if let Some(v) = self.verified.get_mut(at) {
                *v = true;
            }
        }
        Ok(())
    }

    /// `write_file (filename, tp)`.
    fn write_file(
        &mut self,
        scan: &mut Scanner<'_>,
        filename: &[u8],
        tp: &TermType,
    ) -> Result<(), Abort> {
        let Some(buffer) = write_object(tp, self.user_definable, MAX_ENTRY_SIZE) else {
            scan.warning(format!("entry is larger than {MAX_ENTRY_SIZE} bytes").as_bytes());
            return Ok(());
        };
        let opened = if nc_access(filename, W_OK) {
            std::fs::File::create(os(filename)).map_err(|e| errno_of(&e))
        } else {
            Err(sys_access(filename, W_OK).err().unwrap_or(13))
        };
        let mut fp = match opened {
            Ok(f) => f,
            Err(errno) => {
                perror(filename, errno);
                let mut m = b"cannot open ".to_vec();
                m.extend_from_slice(&self.current_dir());
                m.push(b'/');
                m.extend_from_slice(filename);
                return Err(scan.syserr_abort(&m));
            }
        };
        self.total_written = self.total_written.saturating_add(1);
        if let Err(e) = fp.write_all(&buffer) {
            let mut m = b"error writing ".to_vec();
            m.extend_from_slice(&self.current_dir());
            m.push(b'/');
            m.extend_from_slice(filename);
            m.extend_from_slice(format!(": {}", strerror(errno_of(&e))).as_bytes());
            return Err(scan.syserr_abort(&m));
        }
        Ok(())
    }

    /// `_nc_write_entry (tp)`: the entry written under its first name, and
    /// linked under its aliases.
    ///
    /// # Errors
    ///
    /// Upstream's abort, its message written.
    #[allow(clippy::too_many_lines, reason = "upstream's _nc_write_entry")]
    pub fn write_entry(&mut self, scan: &mut Scanner<'_>, tp: &TermType) -> Result<(), Abort> {
        let term_names = cstr(&tp.term_names).to_vec();
        if term_names.is_empty() {
            return Err(scan.syserr_abort(b"no terminal name found."));
        } else if term_names.len() >= MAX_TERMINFO_LENGTH - 1 {
            let mut m = b"terminal name too long: ".to_vec();
            m.extend_from_slice(&term_names);
            return Err(scan.syserr_abort(&m));
        }

        // The first name, and the aliases between it and the description.
        let mut name_list = term_names.clone();
        let first_end;
        let mut others: Vec<u8> = Vec::new();
        match name_list
            .iter()
            .rposition(|&c| c == b'|')
            .filter(|&p| p != 0)
        {
            Some(last_bar) => {
                name_list.truncate(last_bar);
                match name_list.iter().position(|&c| c == b'|') {
                    Some(bar) => {
                        others = name_list
                            .get(bar.wrapping_add(1)..)
                            .unwrap_or_default()
                            .to_vec();
                        first_end = bar;
                    }
                    None => first_end = name_list.len(),
                }
            }
            None => first_end = name_list.len(),
        }
        let first_name_v = name_list.get(..first_end).unwrap_or_default().to_vec();
        scan.set_type(&first_name_v);

        if self.call_count == 0 {
            self.start_time = 0;
        }
        self.call_count = self.call_count.saturating_add(1);

        let limit2 = PATH_MAX - (2 + LEAF_LEN);
        let mut first = first_name_v.clone();
        if first.len() >= limit2 {
            scan.warning(b"terminal name too long.");
            first.truncate(limit2);
        }
        let first_char = first.first().copied().unwrap_or(0);
        let mut filename = vec![first_char, b'/'];
        filename.extend_from_slice(
            first
                .get(..first.len().min(PATH_MAX - (LEAF_LEN + 2)))
                .unwrap_or(&first),
        );
        let filename = cstr(&filename).to_vec();

        // "Has this primary name been written since the first call to
        // write_entry()?"
        if self.start_time > 0
            && let Some((mt, _)) = stat(&filename)
            && mt >= self.start_time
        {
            scan.warning(b"name multiply defined.");
        }

        self.check_writeable(scan, first_char)?;
        self.write_file(scan, &filename, tp)?;

        if self.start_time == 0 {
            match stat(&filename) {
                Some((mt, _)) if mt != 0 => self.start_time = mt,
                _ => {
                    let mut m = b"error obtaining time from ".to_vec();
                    m.extend_from_slice(&self.current_dir());
                    m.push(b'/');
                    m.extend_from_slice(&filename);
                    return Err(scan.syserr_abort(&m));
                }
            }
        }

        // Upstream's walk: each alias starts with whatever byte follows the
        // last `|` -- for an empty alias, the next `|` itself.
        let mut i = 0usize;
        while i < others.len() {
            let start = i;
            i = i.wrapping_add(1);
            while i < others.len() && others.get(i) != Some(&b'|') {
                i = i.wrapping_add(1);
            }
            let alias = others.get(start..i).unwrap_or_default().to_vec();
            if i < others.len() {
                i = i.wrapping_add(1);
            }
            let alias = alias.as_slice();
            if alias.len() > PATH_MAX - (2 + LEAF_LEN) {
                let mut m = b"terminal alias ".to_vec();
                m.extend_from_slice(alias);
                m.extend_from_slice(b" too long.");
                scan.warning(&m);
                continue;
            }
            if alias.contains(&b'/') {
                let mut m = b"cannot link alias ".to_vec();
                m.extend_from_slice(alias);
                m.push(b'.');
                scan.warning(&m);
                continue;
            }
            let alias_first = alias.first().copied().unwrap_or(0);
            self.check_writeable(scan, alias_first)?;
            let mut linkname = vec![alias_first, b'/'];
            linkname.extend_from_slice(alias);
            let linkname = cstr(&linkname).to_vec();

            if filename == linkname {
                scan.warning(b"self-synonym ignored");
            } else if let Some((mt, _)) = stat(&linkname)
                && mt < self.start_time
            {
                let mut m = b"alias ".to_vec();
                m.extend_from_slice(alias);
                m.extend_from_slice(b" multiply defined.");
                scan.warning(&m);
            } else if nc_access(&linkname, W_OK) {
                let symlinkname: Vec<u8> = if Some(&first_char) == linkname.first() {
                    first.clone()
                } else {
                    let mut v = b"../".to_vec();
                    v.extend_from_slice(&filename);
                    v
                };
                let code = match std::fs::remove_file(os(&linkname)) {
                    Ok(()) => 0,
                    Err(e) if errno_of(&e) == ENOENT => 0,
                    Err(_) => -1,
                };
                if let Err(e) = symlink(&symlinkname, &linkname) {
                    let errno = errno_of(&e);
                    if code == 0 && errno == EEXIST {
                        let mut m = b"can't link ".to_vec();
                        m.extend_from_slice(&filename);
                        m.extend_from_slice(b" to ");
                        m.extend_from_slice(&linkname);
                        scan.warning(&m);
                    } else if code == 0 && (errno == EPERM || errno == ENOENT) {
                        self.write_file(scan, &linkname, tp)?;
                    } else {
                        let mut m = b"cannot link ".to_vec();
                        m.extend_from_slice(&filename);
                        m.extend_from_slice(b" to ");
                        m.extend_from_slice(&linkname);
                        m.extend_from_slice(format!(" (errno={errno})").as_bytes());
                        scan.warning(&m);
                    }
                }
            }
        }
        Ok(())
    }
}

/// `symlink (target, linkname)`.
#[cfg(unix)]
fn symlink(target: &[u8], linkname: &[u8]) -> std::io::Result<()> {
    std::os::unix::fs::symlink(os(target), os(linkname))
}

/// No symbolic links on this host: the entry is copied instead, as
/// upstream does where `symlink` fails with `EPERM`.
#[cfg(not(unix))]
fn symlink(_target: &[u8], _linkname: &[u8]) -> std::io::Result<()> {
    Err(std::io::Error::from_raw_os_error(EPERM))
}

/// `valid_db_path (nominal)`: the directory if `tic` could write there --
/// it is a directory it may write in, or it is not there and its parent is.
pub fn valid_db_path(nominal: &[u8]) -> Option<Vec<u8>> {
    match stat(nominal) {
        Some((_, is_dir)) => {
            (is_dir && sys_access(nominal, R_OK | W_OK | X_OK).is_ok()).then(|| nominal.to_vec())
        }
        None => {
            let leaf = nominal
                .iter()
                .rposition(|&c| c == b'/')
                .map_or(0, |p| p.wrapping_add(1));
            if leaf == 0 {
                return None;
            }
            let parent = nominal.get(..leaf).unwrap_or_default();
            match stat(parent) {
                Some((_, true)) if sys_access(parent, R_OK | W_OK | X_OK).is_ok() => {
                    Some(nominal.to_vec())
                }
                _ => None,
            }
        }
    }
}
