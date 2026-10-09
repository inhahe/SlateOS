//! ncurses 6.4's terminfo layer -- `libtinfo` -- as the programs here ask
//! it about the terminal they write to.
//!
//! - **Is the terminal known?** [`setupterm`] answers as `setupterm` and
//!   `tgetent` do: found, not found, or no database at all
//!   ([`TGETENT_YES`], [`TGETENT_NO`], [`TGETENT_ERR`]), with the complaint
//!   `setupterm` prints before it exits when it was given nowhere to put
//!   the answer -- including its verdicts on generic and hard-copy
//!   terminals.
//! - **What can it do?** An [`Entry`]: the standard capabilities by their
//!   index in `term.h` ([`boolean`], [`number`], [`string`]), the extended
//!   ones by name.
//! - **What does `tgetstr` answer?** A [`Termcap`] from [`tgetent`], whose
//!   `sgr0` is the one `tgetent` trims for termcap callers ([`trim_sgr0`]).
//! - **How does a parameterised capability expand?** [`Tparm`].
//! - **What does `tputs` write?** [`tputs`] -- and [`Termcap::tputs`],
//!   which pads as upstream pads on the terminal `setupterm` found: at its
//!   output speed, with its pad character.
//! - **What is in a compiled entry, exactly?** [`termtype::TermType`]: every
//!   value raw, an absent capability and a cancelled one told apart, as
//!   `tic`, `infocmp` and `toe` need it ([`read_file_entry`],
//!   [`read_entry_file`]).
//! - **How is a source compiled?** [`compile`]: `tic`'s library, the
//!   scanner and parser through `use=` resolution to the compiled file, and
//!   back out to source.
//!
//! Ported from Ubuntu 24.04's ncurses 6.4+20240113, built as Debian builds
//! it: no termcap fallback, no hashed database, the compiled-in search list
//! `/etc/terminfo:/lib/terminfo:/usr/share/terminfo` and the default
//! directory `/etc/terminfo`.
//!
//! # Finding an entry
//!
//! [`search_list`] is `_nc_first_db`: `$TERMINFO`, `$HOME/.terminfo`, each
//! element of `$TERMINFO_DIRS` and then the compiled-in list -- the three
//! variables only for a program not running set-user-id or set-group-id --
//! an empty element standing for `/etc/terminfo`, duplicates dropped, and
//! then everything that is not a directory, a non-empty file or a
//! quick-dump (`hex:...`, `b64:...`, an entry spelled out in place of a
//! directory), and everything that is the same directory as one before it.
//! In each, the entry for `xterm` is `x/xterm`. A name that is empty, `.`,
//! `..`, or holds `/` or `:` names no terminal; a list with nothing in it
//! is "no database".
//!
//! # One entry built in
//!
//! Where the database has nothing for a name, `setupterm` tries the entries
//! ncurses was built with (`--with-fallbacks`). The reference's build has
//! none; this one has `xterm-256color` -- the reference's own compiled entry
//! -- because that is what SlateOS's terminal sets `TERM` to, and an image
//! without a database should still know its own terminal (design-decisions
//! §1066). It never stands in front of the database: a file wins.

use std::path::PathBuf;

pub mod captab;
pub mod compile;
mod entry;
mod fallback_data;
pub mod names;
mod sgr0;
pub mod termtype;
mod tparm;
mod tputs;

pub use entry::{
    ABSENT_NUMERIC, BOOLCOUNT, CANCELLED_NUMERIC, Entry, NUMCOUNT, STRCOUNT, TiString,
};

/// A capability's type -- `BOOLEAN`, `NUMBER`, `STRING`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A flag.
    Boolean,
    /// A number.
    Number,
    /// A string, perhaps with parameters.
    String,
}

pub use sgr0::trim_sgr0;
pub use tparm::{Analysis, Arg, NUM_PARM, Tparm, analyze};
pub use tputs::{Outc, Padding, baudrate, putp_to, tputs, tputs_affcnt, tputs_to, tputs_with};

/// `TGETENT_YES`: the terminal was found.
pub const TGETENT_YES: i32 = 1;
/// `TGETENT_NO`: it was not.
pub const TGETENT_NO: i32 = 0;
/// `TGETENT_ERR`: there was nowhere to look, or no name to look for.
pub const TGETENT_ERR: i32 = -1;

/// Boolean capabilities, by their index in `term.h`.
pub mod boolean {
    /// `gn`: a generic line type, not a terminal.
    pub const GENERIC_TYPE: usize = 6;
    /// `hc`: a hard-copy terminal.
    pub const HARD_COPY: usize = 7;
    /// `xon`: the terminal uses XON/XOFF handshaking.
    pub const XON_XOFF: usize = 20;
    /// `npc`: the terminal has no pad character; delays are pauses.
    pub const NO_PAD_CHAR: usize = 25;
}

/// Numeric capabilities, by their index in `term.h`.
pub mod number {
    /// `colors`.
    pub const MAX_COLORS: usize = 13;
}

/// String capabilities, by their index in `term.h`.
pub mod string {
    /// `clear`.
    pub const CLEAR_SCREEN: usize = 5;
    /// `cmdch`: the prototype command character.
    pub const COMMAND_CHARACTER: usize = 9;
    /// `cup`.
    pub const CURSOR_ADDRESS: usize = 10;
    /// `cud1`.
    pub const CURSOR_DOWN: usize = 11;
    /// `home`.
    pub const CURSOR_HOME: usize = 12;
    /// `smacs`.
    pub const ENTER_ALT_CHARSET_MODE: usize = 25;
    /// `bold`: termcap's `md`.
    pub const ENTER_BOLD_MODE: usize = 27;
    /// `rmacs`.
    pub const EXIT_ALT_CHARSET_MODE: usize = 38;
    /// `sgr0`: termcap's `me`.
    pub const EXIT_ATTRIBUTE_MODE: usize = 39;
    /// `pad`: the pad character.
    pub const PAD_CHAR: usize = 104;
    /// `sgr`.
    pub const SET_ATTRIBUTES: usize = 131;
    /// `tsl`.
    pub const TO_STATUS_LINE: usize = 135;
    /// `acsc`.
    pub const ACS_CHARS: usize = 146;
    /// `setf`.
    pub const SET_FOREGROUND: usize = 302;
    /// `setb`.
    pub const SET_BACKGROUND: usize = 303;
    /// `setaf`.
    pub const SET_A_FOREGROUND: usize = 359;
    /// `setab`.
    pub const SET_A_BACKGROUND: usize = 360;
}

/// The compiled-in `TERMINFO_DIRS`.
const CFG_LIST: &[u8] = b"/etc/terminfo:/lib/terminfo:/usr/share/terminfo";
/// The compiled-in `TERMINFO`: what an empty element stands for.
const CFG_ONCE: &[u8] = b"/etc/terminfo";
/// `PATH_MAX`, which bounds an entry's file name.
const PATH_MAX: usize = 4096;

/// What ncurses reads from the environment to find its database.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env {
    /// `$TERMINFO`.
    pub terminfo: Option<Vec<u8>>,
    /// `$HOME`, for `$HOME/.terminfo`.
    pub home: Option<Vec<u8>>,
    /// `$TERMINFO_DIRS`.
    pub terminfo_dirs: Option<Vec<u8>>,
    /// `_nc_env_access ()`: whether the three may be used at all -- not by
    /// a program running set-user-id or set-group-id.
    pub trusted: bool,
    /// `$CC`: a command character that, when it is one byte long, replaces
    /// a terminal's `cmdch` in the strings of its [`legacy_copy`]. Read
    /// whoever is running -- upstream does not ask `_nc_env_access` about
    /// this one.
    pub cc: Option<Vec<u8>>,
}

impl Env {
    /// The process's own.
    #[must_use]
    pub fn from_process() -> Self {
        let var = |name: &str| std::env::var_os(name).map(|v| bytes_of(&v));
        Self {
            terminfo: var("TERMINFO"),
            home: var("HOME"),
            terminfo_dirs: var("TERMINFO_DIRS"),
            trusted: env_access(),
            cc: var("CC"),
        }
    }
}

/// An environment value's bytes.
#[cfg(unix)]
fn bytes_of(v: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    v.as_bytes().to_vec()
}

/// An environment value's bytes, as near as a host without them comes.
#[cfg(not(unix))]
fn bytes_of(v: &std::ffi::OsStr) -> Vec<u8> {
    v.to_string_lossy().into_owned().into_bytes()
}

/// Bytes as a path.
#[cfg(unix)]
fn path_of(b: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(b))
}

/// Bytes as a path, as near as a host without them comes.
#[cfg(not(unix))]
fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(b).into_owned())
}

#[cfg(unix)]
unsafe extern "C" {
    fn getuid() -> u32;
    fn geteuid() -> u32;
    fn getgid() -> u32;
    fn getegid() -> u32;
}

/// `_nc_env_access ()`: false when the real and effective user or group
/// differ -- a set-id program, which may not be steered by `$TERMINFO`.
#[cfg(unix)]
#[must_use]
pub fn env_access() -> bool {
    // SAFETY: four POSIX getters with no arguments, which cannot fail.
    unsafe { getuid() == geteuid() && getgid() == getegid() }
}

/// `_nc_env_access ()` on a host with no set-id programs.
#[cfg(not(unix))]
#[must_use]
pub fn env_access() -> bool {
    true
}

/// `quick_prefix`: an element that is an entry spelled out.
fn quick_prefix(s: &[u8]) -> bool {
    s.starts_with(b"b64:") || s.starts_with(b"hex:")
}

/// `trim_formatting`: newlines (and a backslash before one) and tabs out,
/// for a quick-dump written across lines.
fn trim_formatting(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&ch) = s.get(i) {
        i = i.saturating_add(1);
        if ch == b'\\' && s.get(i) == Some(&b'\n') {
            continue;
        }
        if ch == b'\n' || ch == b'\t' {
            continue;
        }
        out.push(ch);
    }
    out
}

/// What `check_existence` learns of an element: whether it is usable, and
/// which file it is, to know it again under another name.
fn existence(element: &[u8]) -> Option<Vec<u8>> {
    if quick_prefix(element) {
        // Upstream leaves its `stat` buffer untouched for these: zeros.
        return Some(b"0:0".to_vec());
    }
    let path = path_of(element);
    let meta = std::fs::metadata(&path).ok()?;
    if meta.is_dir() || (meta.is_file() && meta.len() > 0) {
        Some(identity(&path, &meta))
    } else {
        None
    }
}

/// `st_dev` and `st_ino`.
#[cfg(unix)]
fn identity(_path: &std::path::Path, meta: &std::fs::Metadata) -> Vec<u8> {
    use std::os::unix::fs::MetadataExt;
    format!("{}:{}", meta.dev(), meta.ino()).into_bytes()
}

/// A host without device and inode numbers: the file's canonical name.
#[cfg(not(unix))]
fn identity(path: &std::path::Path, _meta: &std::fs::Metadata) -> Vec<u8> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical.to_string_lossy().into_owned().into_bytes()
}

/// `_nc_first_db`: the directories (and quick-dumps) an entry is looked
/// for in, in order.
#[must_use]
pub fn search_list(env: &Env) -> Vec<Vec<u8>> {
    search_list_tic(env, None)
}

/// `_nc_first_db` once `_nc_tic_dir` has named a directory: that one first.
#[must_use]
pub fn search_list_tic(env: &Env, tic_dir: Option<&[u8]>) -> Vec<Vec<u8>> {
    let mut values: Vec<Vec<u8>> = Vec::new();
    values.push(tic_dir.map(<[u8]>::to_vec).unwrap_or_default());
    if env.trusted {
        values.push(env.terminfo.clone().unwrap_or_default());
        // `PRIVATE_INFO`, "%s/.terminfo": an empty HOME is `/.terminfo`.
        values.push(
            env.home
                .as_ref()
                .map_or_else(Vec::new, |h| [h.as_slice(), b"/.terminfo"].concat()),
        );
        values.push(env.terminfo_dirs.clone().unwrap_or_default());
    }
    values.push(CFG_LIST.to_vec());
    values.push(CFG_ONCE.to_vec());

    // `add_to_blob`: the non-empty values, colon-separated.
    let mut blob: Vec<u8> = Vec::new();
    for v in values.iter().filter(|v| !v.is_empty()) {
        if !blob.is_empty() {
            blob.push(b':');
        }
        blob.extend_from_slice(v);
    }
    // Split at each colon -- except the one in a quick-dump's prefix.
    let mut list: Vec<Vec<u8>> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    for &c in &blob {
        if c == b':' && !(current.len() == 3 && quick_prefix(&[current.as_slice(), b":"].concat()))
        {
            list.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    list.push(current);

    // An empty element is the default directory; formatting is trimmed;
    // the first of two equal elements is kept.
    let mut named: Vec<Vec<u8>> = Vec::new();
    for e in list {
        let e = if e.is_empty() {
            CFG_ONCE.to_vec()
        } else {
            trim_formatting(&e)
        };
        if !named.contains(&e) {
            named.push(e);
        }
    }
    // Only what is there, and each directory once.
    let mut kept: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for e in named {
        if let Some(id) = existence(&e)
            && !kept.iter().any(|(_, k)| *k == id)
        {
            kept.push((e, id));
        }
    }
    kept.into_iter().map(|(e, _)| e).collect()
}

/// `_nc_read_file_entry`: the entry in the file `path`, its values raw,
/// or `None`.
#[must_use]
pub fn read_file_entry(path: &[u8], user_definable: bool) -> Option<termtype::TermType> {
    use std::io::Read;
    let file = std::fs::File::open(path_of(path)).ok()?;
    let mut buffer = Vec::new();
    let limit = u64::try_from(entry::MAX_ENTRY_SIZE.saturating_add(1)).unwrap_or(u64::MAX);
    file.take(limit).read_to_end(&mut buffer).ok()?;
    if buffer.is_empty() {
        return None;
    }
    termtype::read_termtype(&buffer, user_definable)
}

/// `_nc_read_tic_entry`: `name` in one element of the search list --
/// `filename` set, as upstream sets it, to what was read: `$TERMINFO` for an
/// entry spelled out in the list, else the file tried.
fn read_tic_entry(
    filename: &mut Vec<u8>,
    element: &[u8],
    name: &[u8],
    user_definable: bool,
) -> Option<termtype::TermType> {
    let dump = entry::decode_quickdump(element);
    if !dump.is_empty()
        && let Some(e) = termtype::read_termtype(&dump, user_definable)
        && entry::name_match(&e.term_names, name)
    {
        // "shorten name shown by infocmp"
        *filename = b"$TERMINFO".to_vec();
        return Some(e);
    }
    // `make_dir_filename`: DIR/<first byte>/NAME, if it fits.
    let first = *name.first()?;
    let need = 1usize
        .saturating_add(3)
        .saturating_add(element.len())
        .saturating_add(name.len());
    if need > PATH_MAX {
        return None;
    }
    let mut path = element.to_vec();
    path.push(b'/');
    path.push(first);
    path.push(b'/');
    path.extend_from_slice(name);
    *filename = path;
    read_file_entry(filename, user_definable)
}

/// `_nc_read_entry2`: the entry for `name`, or [`TGETENT_NO`] (an
/// impossible name, or none found) or [`TGETENT_ERR`] (nowhere to look).
pub fn read_entry(name: &[u8], env: &Env) -> Result<Entry, i32> {
    read_termtype_entry(name, env, None, true).map(Entry::from_termtype)
}

/// `_nc_read_entry2` as the compiler calls it: the description raw -- an
/// absent and a cancelled capability told apart -- looked for first in
/// `tic_dir` (`_nc_tic_dir`) if there is one, its extended capabilities
/// read only when `user_definable`.
pub fn read_termtype_entry(
    name: &[u8],
    env: &Env,
    tic_dir: Option<&[u8]>,
    user_definable: bool,
) -> Result<termtype::TermType, i32> {
    read_entry_file(name, env, tic_dir, user_definable).0
}

/// `_nc_read_entry2 (name, filename, tp)`: as [`read_termtype_entry`], and
/// the `filename` upstream leaves -- the file the entry came from, or the
/// last one tried, or the name itself when none was. `infocmp` prints it.
pub fn read_entry_file(
    name: &[u8],
    env: &Env,
    tic_dir: Option<&[u8]>,
    user_definable: bool,
) -> (Result<termtype::TermType, i32>, Vec<u8>) {
    let mut filename = name
        .get(..name.len().min(PATH_MAX - 1))
        .unwrap_or(name)
        .to_vec();
    if name.is_empty()
        || name == b"."
        || name == b".."
        || name.contains(&b'/')
        || name.contains(&b':')
    {
        return (Err(TGETENT_NO), filename);
    }
    let mut code = TGETENT_ERR;
    for element in search_list_tic(env, tic_dir) {
        match read_tic_entry(&mut filename, &element, name, user_definable) {
            Some(e) => return (Ok(e), filename),
            None => code = TGETENT_NO,
        }
    }
    (Err(code), filename)
}

/// `_nc_fallback2`: the entry built in under `name`, for a name the database
/// has nothing for -- ncurses' `--with-fallbacks`, carrying here the entry
/// for what SlateOS's own terminal sets `TERM` to, `xterm-256color`, so that
/// an image without a database still knows it (design-decisions §1066).
fn fallback(name: &[u8]) -> Option<Entry> {
    fallback_data::ALL
        .iter()
        .filter_map(|data| entry::read_termtype(data))
        .find(|e| entry::name_match(e.names(), name))
}

/// What `setupterm` learns: what it would store through `errret`, what it
/// would print without one (before `exit (EXIT_FAILURE)`), and the
/// terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// `*errret`: [`TGETENT_YES`], [`TGETENT_NO`] or [`TGETENT_ERR`].
    pub status: i32,
    /// The complaint; `None` exactly when `setupterm` returns `OK`. A
    /// generic terminal that is "not really generic", and a hard-copy one,
    /// are complained of with the terminal found ([`TGETENT_YES`]) --
    /// which is what `tgetent` goes by.
    pub complaint: Option<Vec<u8>>,
    /// The terminal, when [`Setup::status`] is [`TGETENT_YES`].
    pub entry: Option<Entry>,
    /// `_nc_baudrate (ospeed)`: the output speed of the terminal it was set
    /// up on, or 0 when its descriptor is no terminal, or nothing was found.
    pub baud: i32,
    /// `PC` as `set_curterm` sets it: the first byte of `pad`, or NUL.
    pub pad: u8,
    /// `setenv`s upstream makes to keep `LINES` and `COLUMNS` in step with
    /// the window (`use_tioctl` and `use_env` both on): the caller's to
    /// make, since a library cannot know it is safe to.
    pub env_updates: Vec<(&'static str, i32)>,
}

impl Setup {
    /// A failure, with nothing found.
    fn failure(status: i32, complaint: Vec<u8>) -> Self {
        Self {
            status,
            complaint: Some(complaint),
            entry: None,
            baud: 0,
            pad: 0,
            env_updates: Vec::new(),
        }
    }

    /// How `tputs` pads on the terminal set up: at its speed, with `PC`,
    /// or by pausing where it has `npc`. With no terminal, not at all.
    #[must_use]
    pub fn padding(&self) -> Padding {
        Padding {
            terminal: self.entry.is_some(),
            baud: self.baud,
            pad: self.pad,
            no_pad_char: self
                .entry
                .as_ref()
                .is_some_and(|e| e.flag(boolean::NO_PAD_CHAR)),
        }
    }
}

/// How `setupterm` sizes the screen, and on which descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// `Filedes`. Standard output that is no terminal is standard error
    /// instead, as upstream has it.
    pub fd: i32,
    /// `use_env`: `LINES` and `COLUMNS` override the window's size.
    pub use_env: bool,
    /// `use_tioctl`: the window's size is asked for even without
    /// `use_env`, and with it, `LINES` and `COLUMNS` are set to it.
    pub use_tioctl: bool,
}

impl Default for Options {
    /// Upstream's: standard output, `use_env` on, `use_tioctl` off.
    fn default() -> Self {
        Self {
            fd: 1,
            use_env: true,
            use_tioctl: false,
        }
    }
}

/// `'NAME': ` and a message: `ret_error1`.
fn named(name: &[u8], message: &[u8]) -> Vec<u8> {
    [b"'", name, b"': ", message].concat()
}

/// `setupterm (tname, 1, &errret)`, or -- with the name given -- the
/// `setupterm` inside `tgetent (buf, name)`. With `tname` `None` the name
/// is `term`, the value of `TERM`.
#[must_use]
pub fn setupterm(tname: Option<&[u8]>, term: Option<&[u8]>, env: &Env) -> Setup {
    setupterm_with(tname, term, env, &Options::default())
}

/// `isatty (fd)`, as `tcgetattr` answers it.
fn is_tty(fd: i32) -> bool {
    libcall::termios::get_attr(fd).is_ok()
}

/// `_nc_getenv_num (name)`: the variable as `strtol (s, &end, 0)` reads
/// all of it, or -1 for none, a negative, trailing bytes or more than an
/// `int`.
fn getenv_num(name: &str) -> i32 {
    let Some(value) = std::env::var_os(name).map(|v| bytes_of(&v)) else {
        return -1;
    };
    let (n, used) = cstrtol::strtol(&value, 0);
    if n < 0 || used == 0 || used != value.len() {
        return -1;
    }
    i32::try_from(n).unwrap_or(-1)
}

/// `_nc_get_screensize`, its answer put into the entry's `lines` and
/// `cols` "so tigetnum () and tgetnum () will do the right thing"; and the
/// `setenv`s it makes, returned.
fn screensize(entry: &mut Entry, fd: i32, opts: &Options) -> Vec<(&'static str, i32)> {
    const COLUMNS: usize = 0;
    const LINES: usize = 2;
    let mut updates = Vec::new();
    let mut linep = entry.number(LINES);
    let mut colp = entry.number(COLUMNS);
    if !(opts.use_env || opts.use_tioctl) {
        return updates;
    }
    // "try asking the OS"
    if is_tty(fd)
        && let Ok(size) = libcall::pty::window_size(fd)
    {
        linep = i32::from(size.rows);
        colp = i32::from(size.cols);
    }
    if opts.use_env {
        if opts.use_tioctl {
            // "If environment variables are used, update them."
            if getenv_num("LINES") > 0 {
                updates.push(("LINES", linep));
            }
            if getenv_num("COLUMNS") > 0 {
                updates.push(("COLUMNS", colp));
            }
        }
        let value = updated_num("LINES", &updates);
        if value > 0 {
            linep = value;
        }
        let value = updated_num("COLUMNS", &updates);
        if value > 0 {
            colp = value;
        }
        // `_nc_default_screensize`.
        if linep <= 0 {
            linep = entry.number(LINES);
        }
        if colp <= 0 {
            colp = entry.number(COLUMNS);
        }
        if linep <= 0 {
            linep = 24;
        }
        if colp <= 0 {
            colp = 80;
        }
    }
    // `lines = (NCURSES_INT2) (*linep)`, which with extended numbers is an
    // `int`: whole. (The `short` copy the `CUR` macros read is the
    // programs' to keep.)
    entry.set_number(LINES, linep);
    entry.set_number(COLUMNS, colp);
    updates
}

/// `_nc_getenv_num (name)` after the `setenv`s in `updates`.
fn updated_num(name: &str, updates: &[(&'static str, i32)]) -> i32 {
    updates
        .iter()
        .rev()
        .find(|(n, _)| *n == name)
        .map_or_else(|| getenv_num(name), |&(_, v)| v)
}

/// `setupterm (tname, opts.fd, &errret)`, its screen sized by `opts`.
#[must_use]
pub fn setupterm_with(
    tname: Option<&[u8]>,
    term: Option<&[u8]>,
    env: &Env,
    opts: &Options,
) -> Setup {
    let name = match tname {
        Some(n) => n,
        None => match term {
            Some(t) if !t.is_empty() => t,
            _ => {
                return Setup::failure(
                    TGETENT_ERR,
                    b"TERM environment variable not set.\n".to_vec(),
                );
            }
        },
    };
    if name.len() > entry::MAX_NAME_SIZE {
        return Setup::failure(
            TGETENT_ERR,
            format!(
                "TERM environment must be 1..{} characters.\n",
                entry::MAX_NAME_SIZE
            )
            .into_bytes(),
        );
    }
    let mut entry = match read_entry(name, env) {
        Ok(e) => e,
        // "try fallback list if entry on disk" -- whatever kept it off disk.
        Err(code) => match fallback(name) {
            Some(e) => e,
            None if code == TGETENT_ERR => {
                return Setup::failure(
                    TGETENT_ERR,
                    b"terminals database is inaccessible\n".to_vec(),
                );
            }
            None => return Setup::failure(code, named(name, b"unknown terminal type.\n")),
        },
    };
    // `set_curterm`: `PC = (VALID_STRING (pad_char) ? pad_char[0] : 0)`.
    // (`_nc_tinfo_cmdch` comes next, but rewrites only the copy `CUR`'s
    // macros read: see [`legacy_copy`].)
    let pad = entry
        .string(string::PAD_CHAR)
        .and_then(|p| p.first().copied())
        .unwrap_or(0);
    // "Allow output redirection. ... If stdout is directed to a file,
    // screen updates go to standard error."
    let fd = if opts.fd == 1 && !is_tty(1) {
        2
    } else {
        opts.fd
    };
    let baud = terminal_baud(fd);
    let env_updates = screensize(&mut entry, fd, opts);
    let found = |complaint: Option<Vec<u8>>, entry: Entry, env_updates| Setup {
        status: TGETENT_YES,
        complaint,
        entry: Some(entry),
        baud,
        pad,
        env_updates,
    };
    if entry.flag(boolean::GENERIC_TYPE) {
        // "BSD 4.3's termcap contains mis-typed "gn" for wy99. Do a sanity
        // check before giving up."
        let has = |i: usize| entry.string(i).is_some();
        if (has(string::CURSOR_ADDRESS) || (has(string::CURSOR_DOWN) && has(string::CURSOR_HOME)))
            && has(string::CLEAR_SCREEN)
        {
            return found(
                Some(named(name, b"terminal is not really generic.\n")),
                entry,
                env_updates,
            );
        }
        // The size was taken, and any `setenv` made, before this test.
        let mut failure = Setup::failure(
            TGETENT_NO,
            named(name, b"I need something more specific.\n"),
        );
        failure.env_updates = env_updates;
        return failure;
    }
    if entry.flag(boolean::HARD_COPY) {
        return found(
            Some(named(name, b"I can't handle hardcopy terminals.\n")),
            entry,
            env_updates,
        );
    }
    found(None, entry, env_updates)
}

/// `cur_term->type`, the copy of the terminal that `term.h`'s `CUR` macros
/// read -- `clear_screen`, `init_2string`, `key_backspace` in a program's
/// own code -- as `setupterm` leaves its strings.
///
/// ncurses keeps the terminal twice, each copy with strings of its own, and
/// `_nc_tinfo_cmdch` rewrites only this one: where the terminal has a
/// command character (`cmdch`) and `$CC` is one byte long, that byte takes
/// the command character's place in every string. `tigetstr` reads the
/// other copy, and sees the strings as the entry has them. Measured: with
/// `cmdch=@` and `CC=X`, `clear` sends `X[H` for `clear=@[H` while `tput
/// clear`'s `E3`, which it asks `tigetstr` for, stays `@E3`.
#[must_use]
pub fn legacy_copy(entry: &Entry, env: &Env) -> Entry {
    let mut copy = entry.clone();
    // "Only use the character if the string is a single character, since
    // it is fairly common for developers to set the C compiler name as an
    // environment variable - using the same symbol."
    if let Some(&proto) = entry
        .string(string::COMMAND_CHARACTER)
        .and_then(|c| c.first())
        && let Some([cc]) = env.cc.as_deref()
    {
        copy.replace_in_strings(proto, *cc);
    }
    copy
}

/// The output speed of the terminal on `fd` (`def_prog_mode`, then
/// `baudrate`), as `_nc_baudrate` gives it; 0 when it is no terminal.
fn terminal_baud(fd: i32) -> i32 {
    match libcall::termios::get_attr(fd) {
        Ok(t) => baudrate(libcall::termios::output_speed(&t)),
        Err(_) => 0,
    }
}

/// What `tgetent` leaves for the termcap functions: the terminal, the
/// `sgr0` it trimmed for them, and what `tputs` pads with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Termcap {
    entry: Entry,
    fix_sgr0: Option<Vec<u8>>,
    padding: Padding,
}

impl Termcap {
    /// What `setupterm` alone leaves the termcap functions: the terminal,
    /// padded as `setupterm` left it ([`Setup::padding`]), its `sgr0`
    /// untrimmed -- only `tgetent` trims it.
    #[must_use]
    pub fn untrimmed(entry: Entry, padding: Padding) -> Self {
        Self {
            entry,
            fix_sgr0: None,
            padding,
        }
    }

    /// `tputs (s, 1, putchar)` on this terminal: `s` with its padding made
    /// pad characters at the terminal's speed.
    #[must_use]
    pub fn tputs(&self, s: &[u8]) -> Vec<u8> {
        tputs(s, &self.padding)
    }

    /// The terminal.
    #[must_use]
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    /// `tgetstr` for the string capability at `index` (termcap's name for
    /// it is the caller's to know): as the entry has it, but `sgr0` --
    /// `me` -- as `tgetent` trimmed it.
    #[must_use]
    pub fn string(&self, index: usize) -> Option<&[u8]> {
        let s = self.entry.string(index)?;
        if index == string::EXIT_ATTRIBUTE_MODE
            && let Some(fixed) = &self.fix_sgr0
        {
            return Some(fixed);
        }
        Some(s)
    }
}

/// `tgetent (buf, name)`: the terminal, or what `tgetent` returns instead
/// -- [`TGETENT_NO`] or [`TGETENT_ERR`].
pub fn tgetent(name: &[u8], env: &Env) -> Result<Termcap, i32> {
    let setup = setupterm(Some(name), None, env);
    match setup.entry {
        Some(entry) if setup.status == TGETENT_YES => {
            let fix_sgr0 = trim_sgr0(&entry, &mut Tparm::new());
            // `if (pad_char != NULL) PC = pad_char[0];` -- again, and now
            // after a `$CC` may have rewritten it.
            let pad = entry
                .string(string::PAD_CHAR)
                .and_then(|p| p.first().copied())
                .unwrap_or(setup.pad);
            let padding = Padding {
                terminal: true,
                baud: setup.baud,
                pad,
                no_pad_char: entry.flag(boolean::NO_PAD_CHAR),
            };
            Ok(Termcap {
                entry,
                fix_sgr0,
                padding,
            })
        }
        _ => Err(setup.status),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// A scratch directory, removed when dropped.
    #[cfg(unix)]
    struct Scratch(PathBuf);

    #[cfg(unix)]
    impl Scratch {
        fn new(tag: &str) -> Self {
            let p =
                std::env::temp_dir().join(format!("terminfo-test-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn bytes(&self) -> Vec<u8> {
            bytes_of(self.0.as_os_str())
        }
        fn put(&self, name: &str, data: &[u8]) {
            let dir = self.0.join(&name[..1]);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), data).unwrap();
        }
    }

    #[cfg(unix)]
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn env_at(dir: &Scratch) -> Env {
        Env {
            terminfo: Some(dir.bytes()),
            home: None,
            terminfo_dirs: None,
            trusted: true,
            cc: None,
        }
    }

    fn compile(names: &[u8], bools: &[u8], strs: &[Option<&[u8]>]) -> Vec<u8> {
        entry::tests::compile(names, bools, &[], strs, false)
    }

    #[test]
    fn names_ncurses_refuses() {
        let env = Env::default();
        for name in [&b""[..], b".", b"..", b"a/b", b"a:b"] {
            assert_eq!(read_entry(name, &env).unwrap_err(), TGETENT_NO);
        }
    }

    #[test]
    fn the_built_in_entry_is_xterm_256color_by_any_of_its_names() {
        let e = fallback(b"xterm-256color").unwrap();
        assert_eq!(e.number(number::MAX_COLORS), 256);
        assert_eq!(e.string(string::ENTER_BOLD_MODE), Some(&b"\x1b[1m"[..]));
        assert_eq!(
            e.string(string::EXIT_ATTRIBUTE_MODE),
            Some(&b"\x1b(B\x1b[m"[..])
        );
        // The description is a name too, as `_nc_name_match` reads it.
        assert!(fallback(b"xterm with 256 colors").is_some());
        assert!(fallback(b"xterm").is_none());
        // Found whether the database has it or not; and a name it does not
        // know is still unknown.
        let env = Env {
            terminfo: Some(b"/nonexistent-terminfo".to_vec()),
            home: None,
            terminfo_dirs: None,
            trusted: false,
            cc: None,
        };
        let s = setupterm(Some(b"xterm-256color"), None, &env);
        assert_eq!((s.status, s.complaint), (TGETENT_YES, None));
        assert_ne!(
            setupterm(Some(b"zz-unknown"), None, &env).status,
            TGETENT_YES
        );
        assert_eq!(
            tgetent(b"xterm-256color", &env)
                .unwrap()
                .string(string::EXIT_ATTRIBUTE_MODE),
            Some(&b"\x1b[0m"[..])
        );
    }

    #[test]
    fn a_one_byte_cc_rewrites_the_legacy_copy_and_nothing_else() {
        let mut strs: Vec<Option<&[u8]>> = vec![None; string::CLEAR_SCREEN.max(9) + 1];
        strs[string::COMMAND_CHARACTER] = Some(b"@");
        strs[string::CLEAR_SCREEN] = Some(b"@[H@");
        let entry = entry::read_termtype(&compile(b"zz-cmdch|a test", &[], &strs)).unwrap();
        let with = |cc: &[u8]| Env {
            cc: Some(cc.to_vec()),
            ..Env::default()
        };
        let copy = legacy_copy(&entry, &with(b"X"));
        assert_eq!(copy.string(string::CLEAR_SCREEN), Some(&b"X[HX"[..]));
        assert_eq!(copy.string(string::COMMAND_CHARACTER), Some(&b"X"[..]));
        // The terminal itself -- what `tigetstr` reads -- keeps its own.
        assert_eq!(entry.string(string::CLEAR_SCREEN), Some(&b"@[H@"[..]));
        // Two bytes, or none, are a compiler's name: nothing changes.
        for cc in [&b"XY"[..], b""] {
            let copy = legacy_copy(&entry, &with(cc));
            assert_eq!(copy.string(string::CLEAR_SCREEN), Some(&b"@[H@"[..]));
        }
        assert_eq!(legacy_copy(&entry, &Env::default()), entry);
    }

    // A Windows path's drive colon is a search-list separator.
    #[cfg(unix)]
    #[test]
    fn an_entry_is_found_under_its_first_byte() {
        let dir = Scratch::new("found");
        let mut strs: Vec<Option<&[u8]>> = vec![None; string::ENTER_BOLD_MODE + 1];
        strs[string::ENTER_BOLD_MODE] = Some(b"\x1b[1m");
        dir.put("zz-test", &compile(b"zz-test|a test", &[], &strs));
        let setup = setupterm(Some(b"zz-test"), None, &env_at(&dir));
        assert_eq!(setup.status, TGETENT_YES);
        assert_eq!(setup.complaint, None);
        let entry = setup.entry.unwrap();
        assert_eq!(entry.string(string::ENTER_BOLD_MODE), Some(&b"\x1b[1m"[..]));
        // Untrusted, `$TERMINFO` is not looked in.
        let mut env = env_at(&dir);
        env.trusted = false;
        assert_eq!(setupterm(Some(b"zz-test"), None, &env).status, TGETENT_NO);
    }

    // A Windows path's drive colon is a search-list separator.
    #[cfg(unix)]
    #[test]
    fn setupterm_complains_as_upstream_does() {
        let dir = Scratch::new("complain");
        let env = env_at(&dir);
        let s = setupterm(None, None, &env);
        assert_eq!(
            (s.status, s.complaint.as_deref()),
            (
                TGETENT_ERR,
                Some(&b"TERM environment variable not set.\n"[..])
            )
        );
        let s = setupterm(None, Some(b"zz-nosuch"), &env);
        assert_eq!(
            (s.status, s.complaint.as_deref()),
            (
                TGETENT_NO,
                Some(&b"'zz-nosuch': unknown terminal type.\n"[..])
            )
        );
        let long = vec![b'x'; 513];
        let s = setupterm(Some(&long), None, &env);
        assert_eq!(
            (s.status, s.complaint.as_deref()),
            (
                TGETENT_ERR,
                Some(&b"TERM environment must be 1..512 characters.\n"[..])
            )
        );
        // Generic with nothing to address the cursor by: not found.
        let mut bools = vec![0u8; boolean::HARD_COPY + 1];
        bools[boolean::GENERIC_TYPE] = 1;
        dir.put("zz-generic", &compile(b"zz-generic", &bools, &[]));
        let s = setupterm(Some(b"zz-generic"), None, &env);
        assert_eq!(
            (s.status, s.complaint.as_deref()),
            (
                TGETENT_NO,
                Some(&b"'zz-generic': I need something more specific.\n"[..])
            )
        );
        // Hard copy: found, but complained of.
        let mut bools = vec![0u8; boolean::HARD_COPY + 1];
        bools[boolean::HARD_COPY] = 1;
        dir.put("zz-paper", &compile(b"zz-paper", &bools, &[]));
        let s = setupterm(Some(b"zz-paper"), None, &env);
        assert_eq!(s.status, TGETENT_YES);
        assert_eq!(
            s.complaint.as_deref(),
            Some(&b"'zz-paper': I can't handle hardcopy terminals.\n"[..])
        );
        assert!(tgetent(b"zz-paper", &env).is_ok());
    }

    // A Windows path's drive colon is a search-list separator.
    #[cfg(unix)]
    #[test]
    fn the_search_list_is_built_as_upstream_builds_it() {
        let a = Scratch::new("list-a");
        let b = Scratch::new("list-b");
        let env = Env {
            terminfo: Some(a.bytes()),
            home: None,
            terminfo_dirs: Some(
                [
                    b.bytes().as_slice(),
                    b"::",
                    a.bytes().as_slice(),
                    b":/nonexistent-terminfo",
                ]
                .concat(),
            ),
            trusted: true,
            cc: None,
        };
        let list = search_list(&env);
        assert_eq!(list.first(), Some(&a.bytes()));
        assert_eq!(list.get(1), Some(&b.bytes()));
        // `a` once, the missing directory not at all.
        assert_eq!(list.iter().filter(|e| **e == a.bytes()).count(), 1);
        assert!(
            !list
                .iter()
                .any(|e| e.as_slice() == b"/nonexistent-terminfo")
        );
        // A quick-dump is kept whole, its colon not a separator.
        let env = Env {
            terminfo: Some(b"hex:00".to_vec()),
            home: None,
            terminfo_dirs: None,
            trusted: true,
            cc: None,
        };
        assert_eq!(
            search_list(&env).first().map(Vec::as_slice),
            Some(&b"hex:00"[..])
        );
    }

    #[test]
    fn a_quick_dump_entry_is_read_from_the_variable() {
        let file = compile(b"qd|quick", &[], &[]);
        let hex: String = file.iter().fold(String::new(), |mut h, b| {
            use std::fmt::Write;
            let _ = write!(h, "{b:02x}");
            h
        });
        let env = Env {
            terminfo: Some(format!("hex:{hex}").into_bytes()),
            home: None,
            terminfo_dirs: None,
            trusted: true,
            cc: None,
        };
        assert!(read_entry(b"qd", &env).is_ok());
        assert!(read_entry(b"quick", &env).is_ok());
    }

    // A Windows path's drive colon is a search-list separator.
    #[cfg(unix)]
    #[test]
    fn tgetent_trims_sgr0_and_tgetstr_answers_with_it() {
        let dir = Scratch::new("tgetent");
        let mut strs: Vec<Option<&[u8]>> = vec![None; string::SET_ATTRIBUTES + 1];
        strs[string::ENTER_ALT_CHARSET_MODE] = Some(b"\x1b(0");
        strs[string::EXIT_ALT_CHARSET_MODE] = Some(b"\x1b(B");
        strs[string::EXIT_ATTRIBUTE_MODE] = Some(b"\x1b(B\x1b[m");
        strs[string::ENTER_BOLD_MODE] = Some(b"\x1b[1m");
        strs[string::SET_ATTRIBUTES] =
            Some(b"%?%p9%t\x1b(0%e\x1b(B%;\x1b[0%?%p6%t;1%;%?%p5%t;2%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p7%t;8%;m");
        dir.put("zz-xterm", &compile(b"zz-xterm", &[], &strs));
        let tc = tgetent(b"zz-xterm", &env_at(&dir)).unwrap();
        assert_eq!(
            tc.string(string::EXIT_ATTRIBUTE_MODE),
            Some(&b"\x1b[0m"[..])
        );
        assert_eq!(
            tc.entry().string(string::EXIT_ATTRIBUTE_MODE),
            Some(&b"\x1b(B\x1b[m"[..])
        );
        assert_eq!(tc.string(string::ENTER_BOLD_MODE), Some(&b"\x1b[1m"[..]));
        assert_eq!(tgetent(b"zz-none", &env_at(&dir)).unwrap_err(), TGETENT_NO);
    }
}
