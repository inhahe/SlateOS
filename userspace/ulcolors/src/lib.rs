//! util-linux's `lib/colors.c` and `lib/color-names.c`, ported: whether a
//! util-linux program colours its output, and with what.
//!
//! The decision has more inputs than it looks:
//!
//! - **`--color[=WHEN]`**, parsed by [`colormode_or_err`]: `auto`, `never`,
//!   `always` -- case-insensitively, after one leading `=`.
//! - **Standard output.** Anything but `always` is `never` when it is not a
//!   terminal.
//! - **The terminal.** `auto` colours only a terminal whose terminfo entry
//!   says it has more than one colour (`setupterm` and `tigetnum ("colors")`,
//!   as util-linux is built against ncurses on the reference): no `TERM`, or
//!   no terminfo entry for it, is no colours. See [`tinfo`].
//! - **`terminal-colors.d`**, read only when no `--color` was given: files
//!   named `[util][@term].{enable,disable,scheme}` in
//!   `$XDG_CONFIG_HOME/terminal-colors.d` (else `~/.config/…`), and then
//!   `/etc/terminal-colors.d`, scored 1, +20 for naming this program, +10 for
//!   naming this terminal; a better `disable` than `enable` turns colours off
//!   -- but only once a `scheme` file has been found somewhere, because
//!   upstream reads "no scheme" as "no configuration" and then takes the
//!   built-in default. Measured, as everything here is, through
//!   `scripts/hexdump-diff.sh`.
//! - **The default**, [`COLORS_BY_DEFAULT`]: on, as the reference's build has
//!   it (`colors are enabled by default`, its `--help` says).
//!
//! Colour names go through [`sequence_from_colorname`], which keeps
//! upstream's table exactly as it is written -- including the two defects
//! that make `lightgray` and `white` unknown names (see there).
//!
//! The functions return the bytes a program writes; writing them is the
//! program's, as is the stream they go to.

use std::path::{Path, PathBuf};

pub mod tinfo;

/// `UL_COLOR_RESET`.
pub const RESET: &[u8] = b"\x1b[0m";
/// `UL_COLOR_BOLD`.
pub const BOLD: &[u8] = b"\x1b[1m";
/// `UL_COLOR_HALFBRIGHT`.
pub const HALFBRIGHT: &[u8] = b"\x1b[2m";
/// `UL_COLOR_UNDERSCORE`.
pub const UNDERSCORE: &[u8] = b"\x1b[4m";
/// `UL_COLOR_BLINK`.
pub const BLINK: &[u8] = b"\x1b[5m";
/// `UL_COLOR_REVERSE`.
pub const REVERSE: &[u8] = b"\x1b[7m";
/// `UL_COLOR_BLACK`.
pub const BLACK: &[u8] = b"\x1b[30m";
/// `UL_COLOR_RED`.
pub const RED: &[u8] = b"\x1b[31m";
/// `UL_COLOR_GREEN`.
pub const GREEN: &[u8] = b"\x1b[32m";
/// `UL_COLOR_BROWN`.
pub const BROWN: &[u8] = b"\x1b[33m";
/// `UL_COLOR_BLUE`.
pub const BLUE: &[u8] = b"\x1b[34m";
/// `UL_COLOR_MAGENTA`.
pub const MAGENTA: &[u8] = b"\x1b[35m";
/// `UL_COLOR_CYAN`.
pub const CYAN: &[u8] = b"\x1b[36m";
/// `UL_COLOR_GRAY`.
pub const GRAY: &[u8] = b"\x1b[37m";
/// `UL_COLOR_DARK_GRAY`.
pub const DARK_GRAY: &[u8] = b"\x1b[1;30m";
/// `UL_COLOR_BOLD_RED`.
pub const BOLD_RED: &[u8] = b"\x1b[1;31m";
/// `UL_COLOR_BOLD_GREEN`.
pub const BOLD_GREEN: &[u8] = b"\x1b[1;32m";
/// `UL_COLOR_BOLD_YELLOW`.
pub const BOLD_YELLOW: &[u8] = b"\x1b[1;33m";
/// `UL_COLOR_BOLD_BLUE`.
pub const BOLD_BLUE: &[u8] = b"\x1b[1;34m";
/// `UL_COLOR_BOLD_MAGENTA`.
pub const BOLD_MAGENTA: &[u8] = b"\x1b[1;35m";
/// `UL_COLOR_BOLD_CYAN`.
pub const BOLD_CYAN: &[u8] = b"\x1b[1;36m";
/// `UL_COLOR_WHITE`.
pub const WHITE: &[u8] = b"\x1b[1;37m";

/// `enum colortmode`: `--color`'s values, and "not given".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    /// `UL_COLORMODE_AUTO`.
    Auto,
    /// `UL_COLORMODE_NEVER`.
    Never,
    /// `UL_COLORMODE_ALWAYS`.
    Always,
    /// `UL_COLORMODE_UNDEF`: no `--color` at all.
    Undef,
}

/// `USE_COLORS_BY_DEFAULT`, which the reference's build defines.
pub const COLORS_BY_DEFAULT: bool = true;

/// `UL_COLORMODE_DEFAULT`.
const DEFAULT_MODE: ColorMode = if COLORS_BY_DEFAULT {
    ColorMode::Auto
} else {
    ColorMode::Never
};

/// `USAGE_COLORS_DEFAULT`: the line `--help` prints under `--color`.
#[must_use]
pub fn usage_colors_default() -> &'static str {
    if COLORS_BY_DEFAULT {
        "colors are enabled by default"
    } else {
        "colors are disabled by default"
    }
}

/// `colormode_from_string`: `auto`, `never` or `always`, in any case.
#[must_use]
pub fn colormode_from_string(s: &[u8]) -> Option<ColorMode> {
    [
        (&b"auto"[..], ColorMode::Auto),
        (b"never", ColorMode::Never),
        (b"always", ColorMode::Always),
    ]
    .into_iter()
    .find(|(name, _)| !s.is_empty() && s.eq_ignore_ascii_case(name))
    .map(|(_, mode)| mode)
}

/// `colormode_or_err`: the mode named by `arg` less one leading `=`.
///
/// # Errors
///
/// The text that named no mode -- `arg` less that `=` -- for the caller to
/// word as upstream does: `ERRMSG: 'TEXT'`.
pub fn colormode_or_err(arg: &[u8]) -> Result<ColorMode, Vec<u8>> {
    let p = arg.strip_prefix(b"=").unwrap_or(arg);
    colormode_from_string(p).ok_or_else(|| p.to_vec())
}

/// glibc's `bsearch`: the binary search exactly as it probes, so a table that
/// is not sorted -- as upstream's colour names are not -- misses what it
/// misses.
fn bsearch<'a, T>(table: &'a [T], key: &[u8], name: impl Fn(&T) -> &[u8]) -> Option<&'a T> {
    let (mut l, mut u) = (0usize, table.len());
    while l < u {
        let idx = l.saturating_add(u) / 2;
        let item = table.get(idx)?;
        match key.cmp(name(item)) {
            std::cmp::Ordering::Less => u = idx,
            std::cmp::Ordering::Greater => l = idx.saturating_add(1),
            std::cmp::Ordering::Equal => return Some(item),
        }
    }
    None
}

/// `basic_schemes`, as upstream writes it -- in this order, which is not
/// sorted (`yellow` before `white`), and with `"lightgray,"`, comma and all.
const COLOR_NAMES: &[(&[u8], &[u8])] = &[
    (b"black", BLACK),
    (b"blink", BLINK),
    (b"blue", BLUE),
    (b"bold", BOLD),
    (b"brown", BROWN),
    (b"cyan", CYAN),
    (b"darkgray", DARK_GRAY),
    (b"gray", GRAY),
    (b"green", GREEN),
    (b"halfbright", HALFBRIGHT),
    (b"lightblue", BOLD_BLUE),
    (b"lightcyan", BOLD_CYAN),
    (b"lightgray,", GRAY),
    (b"lightgreen", BOLD_GREEN),
    (b"lightmagenta", BOLD_MAGENTA),
    (b"lightred", BOLD_RED),
    (b"magenta", MAGENTA),
    (b"red", RED),
    (b"reset", RESET),
    (b"reverse", REVERSE),
    (b"yellow", BOLD_YELLOW),
    (b"white", WHITE),
];

/// `color_sequence_from_colorname`: a colour's escape sequence by its name.
///
/// Upstream's table is searched with `bsearch` and is not sorted, and one of
/// its names has a stray comma. So `white` is not found (the search turns
/// away from it at `yellow`), and `lightgray` is not either -- `lightgray,`
/// is. Kept: these are what every util-linux program answers, and a scheme
/// file that names them gets the same (unknown) result here.
#[must_use]
pub fn sequence_from_colorname(name: &[u8]) -> Option<&'static [u8]> {
    bsearch(COLOR_NAMES, name, |e| e.0).map(|e| e.1)
}

/// `color_is_sequence`: an escape sequence already, `ESC [ digit … m`.
#[must_use]
pub fn is_sequence(color: &[u8]) -> bool {
    color.len() >= 4
        && color.first() == Some(&0x1b)
        && color.get(1) == Some(&b'[')
        && color.get(2).is_some_and(u8::is_ascii_digit)
        && color.last() == Some(&b'm')
}

/// `color_get_sequence`: a scheme file's colour made an escape sequence.
///
/// A name (anything starting with a letter) is looked up, and kept as it is
/// if unknown -- so an unknown name is written out as text, as upstream
/// writes it. Anything else is `xx;yy` wrapped as `ESC [ xx;yy m`, with the
/// backslash escapes `\a \b \e \f \n \r \t \v \\ \_` (a space) `\#` and `\?`
/// replaced; another escaped character is kept with its backslash.
#[must_use]
pub fn get_sequence(color: &[u8]) -> Vec<u8> {
    if color.first() != Some(&b'\\') && color.first().is_some_and(u8::is_ascii_alphabetic) {
        return sequence_from_colorname(color).unwrap_or(color).to_vec();
    }
    let mut seq = Vec::with_capacity(color.len().saturating_add(3));
    seq.extend_from_slice(b"\x1b[");
    seq.extend_from_slice(color);
    seq.push(b'm');
    let mut out = Vec::with_capacity(seq.len());
    let mut i = 0usize;
    while let Some(&c) = seq.get(i) {
        if c != b'\\' {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        match seq.get(i.saturating_add(1)).copied() {
            Some(b'a') => out.push(0x07),
            Some(b'b') => out.push(0x08),
            Some(b'e') => out.push(0x1b),
            Some(b'f') => out.push(0x0c),
            Some(b'n') => out.push(b'\n'),
            Some(b'r') => out.push(b'\r'),
            Some(b't') => out.push(b'\t'),
            Some(b'v') => out.push(0x0b),
            Some(b'\\') => out.push(b'\\'),
            Some(b'_') => out.push(b' '),
            Some(b'#') => out.push(b'#'),
            Some(b'?') => out.push(b'?'),
            // `*out++ = *in; *out++ = *(in + 1);` -- the backslash and the
            // byte after it, which for a trailing backslash is the string's
            // end: nothing more.
            Some(other) => {
                out.push(b'\\');
                out.push(other);
            }
            None => out.push(b'\\'),
        }
        i = i.saturating_add(2);
    }
    out
}

/// What [`Colors::init`] reads from the environment, gathered so that a test
/// can give it one of its own.
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// `$TERM`.
    pub term: Option<Vec<u8>>,
    /// `$HOME`.
    pub home: Option<Vec<u8>>,
    /// `$XDG_CONFIG_HOME`.
    pub xdg_config_home: Option<Vec<u8>>,
    /// `$TERMINFO` and `$TERMINFO_DIRS`, for [`tinfo`].
    pub terminfo: Option<Vec<u8>>,
    pub terminfo_dirs: Option<Vec<u8>>,
    /// `/etc/terminal-colors.d`, as the system's.
    pub system_dir: PathBuf,
}

impl Env {
    /// The process's own environment.
    #[must_use]
    pub fn from_process() -> Self {
        let var = |name: &str| std::env::var_os(name).map(|v| bytes_of(&v));
        Self {
            term: var("TERM"),
            home: var("HOME"),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            terminfo: var("TERMINFO"),
            terminfo_dirs: var("TERMINFO_DIRS"),
            system_dir: PathBuf::from(SYSTEM_DIR),
        }
    }
}

/// `_PATH_TERMCOLORS_DIR`.
pub const SYSTEM_DIR: &str = "/etc/terminal-colors.d";
/// `_PATH_TERMCOLORS_DIRNAME`.
const DIRNAME: &str = "terminal-colors.d";

/// An `OsStr`'s bytes.
#[cfg(unix)]
fn bytes_of(s: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

/// An `OsStr`'s bytes, as near as a host without them comes.
#[cfg(not(unix))]
fn bytes_of(s: &std::ffi::OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
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

/// `UL_COLORFILE_*`: the three kinds of file in `terminal-colors.d`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileType {
    Disable = 0,
    Enable = 1,
    Scheme = 2,
}

/// A `terminal-colors.d` file name taken apart: `[name][@term].type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tokens<'a> {
    /// The program the file is for; `None` for every program.
    name: Option<&'a [u8]>,
    /// The terminal it is for; `None` for every terminal.
    term: Option<&'a [u8]>,
    ty: FileType,
}

/// `filename_to_tokens`: a `terminal-colors.d` file name as `[name][@term].type`
/// -- `None` for a name that is not one (unknown type, empty, a dot first).
fn filename_to_tokens(s: &[u8]) -> Option<Tokens<'_>> {
    if s.is_empty() || s.first() == Some(&b'.') {
        return None;
    }
    let dot = s.iter().rposition(|&c| c == b'.');
    let type_start = dot.map_or(0, |d| d.saturating_add(1));
    let ty = match s.get(type_start..)? {
        b"disable" => FileType::Disable,
        b"enable" => FileType::Enable,
        b"scheme" => FileType::Scheme,
        _ => return None,
    };
    if type_start == 0 {
        return Some(Tokens {
            name: None,
            term: None,
            ty,
        });
    }
    let at = s.iter().position(|&c| c == b'@');
    let mut term = None;
    if let Some(at) = at {
        let term_start = at.saturating_add(1);
        // `*termsz = type_start - term_start - 1`: what lies between.
        term = Some(
            s.get(term_start..type_start.saturating_sub(1))
                .unwrap_or_default(),
        );
        if at == 0 {
            return Some(Tokens {
                name: None,
                term,
                ty,
            });
        }
    }
    let end = at.unwrap_or_else(|| type_start.saturating_sub(1));
    Some(Tokens {
        name: Some(s.get(..end).unwrap_or_default()),
        term,
        ty,
    })
}

/// `struct ul_color_ctl`: one program's colour state.
#[derive(Clone, Debug)]
pub struct Colors {
    utilname: Vec<u8>,
    env: Env,
    /// `sfile`: the best scheme file found.
    sfile: Option<PathBuf>,
    /// `schemes`, sorted by name.
    schemes: Vec<(Vec<u8>, Vec<u8>)>,
    mode: ColorMode,
    has_colors: bool,
    disabled: bool,
    cs_configured: bool,
    configured: bool,
    /// `scores`, by [`FileType`].
    scores: [i32; 3],
}

/// What a failed configuration read leaves `rc` as: `-errno` in upstream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadErr {
    /// No scheme file was found (`-ENOENT`), or the directory is missing.
    NotFound,
    /// The directory could not be read for another reason.
    Other(i32),
}

impl Colors {
    /// `colors_init (mode, name)`, for a standard output that is or is not
    /// a terminal, under the process's environment.
    #[must_use]
    pub fn init(mode: ColorMode, utilname: &[u8], stdout_is_tty: bool) -> Self {
        Self::init_with(mode, utilname, stdout_is_tty, Env::from_process())
    }

    /// [`Colors::init`] under a given environment.
    #[must_use]
    pub fn init_with(mode: ColorMode, utilname: &[u8], stdout_is_tty: bool, env: Env) -> Self {
        let mut cc = Self {
            utilname: utilname.to_vec(),
            env,
            sfile: None,
            schemes: Vec::new(),
            mode: ColorMode::Undef,
            has_colors: false,
            disabled: false,
            cs_configured: false,
            configured: false,
            scores: [0; 3],
        };
        cc.mode = if mode != ColorMode::Always && !stdout_is_tty {
            ColorMode::Never
        } else {
            mode
        };
        // `ready` is -1 until asked.
        let mut ready: Option<bool> = None;
        if cc.mode == ColorMode::Undef {
            let is_ready = cc.terminal_is_ready();
            ready = Some(is_ready);
            if is_ready {
                cc.mode = match cc.read_configuration() {
                    Err(_) => DEFAULT_MODE,
                    Ok(()) => {
                        let [disable, enable, _] = cc.scores;
                        if disable > enable {
                            ColorMode::Never
                        } else {
                            DEFAULT_MODE
                        }
                    }
                };
            }
        }
        cc.has_colors = match cc.mode {
            ColorMode::Auto => ready.unwrap_or_else(|| cc.terminal_is_ready()),
            ColorMode::Always => true,
            ColorMode::Never | ColorMode::Undef => false,
        };
        cc
    }

    /// `colors_terminal_is_ready`: `TERM`'s terminfo entry has more than one
    /// colour.
    fn terminal_is_ready(&self) -> bool {
        let Some(term) = &self.env.term else {
            return false;
        };
        let ncolors = tinfo::colors(
            term,
            self.env.terminfo.as_deref(),
            self.env.home.as_deref(),
            self.env.terminfo_dirs.as_deref(),
        )
        .unwrap_or(-1);
        ncolors > 1
    }

    /// `colors_get_homedir`: `$XDG_CONFIG_HOME/terminal-colors.d`, else
    /// `$HOME/.config/terminal-colors.d`.
    fn homedir(&self) -> Option<PathBuf> {
        if let Some(x) = &self.env.xdg_config_home {
            return Some(path_of(x).join(DIRNAME));
        }
        self.env
            .home
            .as_ref()
            .map(|h| path_of(h).join(".config").join(DIRNAME))
    }

    /// `colors_read_configuration`: the home directory's files, then -- if
    /// that found no scheme, or could not be read for want of permission --
    /// the system's, the scores carried across both.
    fn read_configuration(&mut self) -> Result<(), ReadErr> {
        let mut rc = Err(ReadErr::NotFound);
        if let Some(dir) = self.homedir() {
            rc = self.readdir(&dir);
        }
        if matches!(rc, Err(ReadErr::NotFound | ReadErr::Other(1 | 13))) {
            let system = self.env.system_dir.clone();
            rc = self.readdir(&system);
        }
        self.configured = true;
        rc
    }

    /// `colors_readdir`: the best match of each kind in `dir`, by score,
    /// a later file of an equal score replacing an earlier one.
    fn readdir(&mut self, dir: &Path) -> Result<(), ReadErr> {
        /// `EINVAL`, for a program with no name: not one of the failures
        /// that send the search on to the system directory.
        const EINVAL: i32 = 22;
        if self.utilname.is_empty() {
            return Err(ReadErr::Other(EINVAL));
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                return Err(match e.raw_os_error() {
                    Some(2) | None => ReadErr::NotFound,
                    Some(n) => ReadErr::Other(n),
                });
            }
        };
        let mut sfile: Option<Vec<u8>> = None;
        for entry in entries.map_while(Result::ok) {
            let name = bytes_of(&entry.file_name());
            if name.first() == Some(&b'.') {
                continue;
            }
            // `d_type`: a regular file, a link or "unknown" only.
            if let Ok(ft) = entry.file_type() {
                if !(ft.is_file() || ft.is_symlink()) {
                    continue;
                }
            }
            let Some(Tokens {
                name: tk_name,
                term: tk_term,
                ty,
            }) = filename_to_tokens(&name)
            else {
                continue;
            };
            let mut score = 1i32;
            if tk_name.is_some() {
                score = score.saturating_add(20);
            }
            if tk_term.is_some() {
                score = score.saturating_add(10);
            }
            let slot = ty as usize;
            if score < self.scores.get(slot).copied().unwrap_or(0) {
                continue;
            }
            if let Some(n) = tk_name {
                if !n.is_empty() && n != self.utilname.as_slice() {
                    continue;
                }
            }
            if let Some(t) = tk_term {
                if !t.is_empty() {
                    match &self.env.term {
                        Some(term) if !term.is_empty() && t == term.as_slice() => {}
                        _ => continue,
                    }
                }
            }
            if let Some(s) = self.scores.get_mut(slot) {
                *s = score;
            }
            if ty == FileType::Scheme {
                sfile = Some(name);
            }
        }
        match sfile {
            Some(name) => {
                self.sfile = Some(dir.join(path_of(&name)));
                Ok(())
            }
            None => Err(ReadErr::NotFound),
        }
    }

    /// `colors_read_schemes`: the scheme file's `name sequence` lines.
    fn read_schemes(&mut self) -> Result<(), ReadErr> {
        let mut rc = Ok(());
        if !self.configured {
            rc = self.read_configuration();
        }
        self.cs_configured = true;
        rc?;
        let Some(file) = self.sfile.clone() else {
            return Err(ReadErr::NotFound);
        };
        let text =
            std::fs::read(&file).map_err(|e| ReadErr::Other(e.raw_os_error().unwrap_or(5)))?;
        for line in text.split(|&c| c == b'\n') {
            let line = line
                .iter()
                .position(|&c| c == 0)
                .map_or(line, |n| line.get(..n).unwrap_or_default());
            let p = {
                let n = line
                    .iter()
                    .take_while(|&&c| c == b' ' || c == b'\t')
                    .count();
                line.get(n..).unwrap_or_default()
            };
            if p.is_empty() || p.first() == Some(&b'#') {
                continue;
            }
            // `sscanf (p, "%128[^ ] %128[^\n ]", cn, seq)`.
            let cn: Vec<u8> = p
                .iter()
                .take(128)
                .take_while(|&&c| c != b' ')
                .copied()
                .collect();
            let rest = p.get(cn.len()..).unwrap_or_default();
            let ws = rest
                .iter()
                .take_while(|&&c| matches!(c, b' ' | 0x09..=0x0d))
                .count();
            let rest = rest.get(ws..).unwrap_or_default();
            let seq: Vec<u8> = rest
                .iter()
                .take(128)
                .take_while(|&&c| c != b' ' && c != b'\n')
                .copied()
                .collect();
            // `%[` needs at least one byte; a name that filled its 128 and
            // went on leaves the rest for the second, which then starts with
            // a byte that is no blank -- the scan reads on from there.
            if cn.is_empty() || seq.is_empty() {
                continue;
            }
            let s = get_sequence(&seq);
            self.schemes.push((cn, s));
        }
        self.schemes.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(())
    }

    /// `colors_wanted`.
    #[must_use]
    pub fn wanted(&self) -> bool {
        self.has_colors
    }

    /// `colors_mode`.
    #[must_use]
    pub fn mode(&self) -> ColorMode {
        self.mode
    }

    /// `colors_off`.
    pub fn off(&mut self) {
        self.disabled = true;
    }

    /// `colors_on`.
    pub fn on(&mut self) {
        self.disabled = false;
    }

    /// `color_fenable (seq, f)`: what to write to turn `seq` on -- nothing
    /// when colours are off.
    #[must_use]
    pub fn enable<'s>(&self, seq: &'s [u8]) -> &'s [u8] {
        if !self.disabled && self.has_colors {
            seq
        } else {
            b""
        }
    }

    /// `color_fdisable`: what to write to turn colour off again.
    #[must_use]
    pub fn disable(&self) -> &'static [u8] {
        if !self.disabled && self.has_colors {
            RESET
        } else {
            b""
        }
    }

    /// `color_get_disable_sequence`.
    #[must_use]
    pub fn disable_sequence(&self) -> &'static [u8] {
        self.disable()
    }

    /// `color_scheme_get_sequence (name, dflt)`: the scheme's sequence for a
    /// logical name, or `dflt`; nothing at all when colours are off.
    pub fn scheme_get_sequence(&mut self, name: &[u8], dflt: Option<&[u8]>) -> Option<Vec<u8>> {
        if self.disabled || !self.has_colors {
            return None;
        }
        let found = self.get_scheme(name);
        match found {
            Some(seq) => Some(seq),
            None => dflt.map(<[u8]>::to_vec),
        }
    }

    /// `colors_get_scheme`.
    fn get_scheme(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        if name.is_empty() {
            return None;
        }
        if !self.cs_configured && self.read_schemes().is_err() {
            return None;
        }
        bsearch(&self.schemes, name, |e| e.0.as_slice()).map(|e| e.1.clone())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_name_table_keeps_upstreams_two_defects() {
        assert_eq!(sequence_from_colorname(b"red"), Some(RED));
        assert_eq!(sequence_from_colorname(b"yellow"), Some(BOLD_YELLOW));
        assert_eq!(sequence_from_colorname(b"lightblue"), Some(BOLD_BLUE));
        // The table is not sorted: the search turns away at `yellow`.
        assert_eq!(sequence_from_colorname(b"white"), None);
        // A stray comma in the table.
        assert_eq!(sequence_from_colorname(b"lightgray"), None);
        assert_eq!(sequence_from_colorname(b"lightgray,"), Some(GRAY));
        assert_eq!(sequence_from_colorname(b"nosuch"), None);
    }

    #[test]
    fn every_name_but_the_two_is_found() {
        for (name, seq) in COLOR_NAMES {
            if *name == b"white" {
                continue;
            }
            assert_eq!(sequence_from_colorname(name), Some(*seq), "{name:?}");
        }
    }

    #[test]
    fn a_sequence_from_a_scheme_line() {
        assert_eq!(get_sequence(b"red"), RED);
        assert_eq!(get_sequence(b"nosuch"), b"nosuch");
        assert_eq!(get_sequence(b"1;31"), b"\x1b[1;31m");
        assert_eq!(get_sequence(b"\\e\\_"), b"\x1b[\x1b m");
        assert_eq!(get_sequence(b"7\\x"), b"\x1b[7\\xm");
        assert!(is_sequence(b"\x1b[1m"));
        assert!(!is_sequence(b"\x1b[m"));
    }

    #[test]
    fn modes() {
        assert_eq!(colormode_or_err(b"Always"), Ok(ColorMode::Always));
        assert_eq!(colormode_or_err(b"=never"), Ok(ColorMode::Never));
        assert_eq!(colormode_or_err(b"==never"), Err(b"=never".to_vec()));
        assert_eq!(colormode_or_err(b""), Err(Vec::new()));
        assert_eq!(colormode_or_err(b"x"), Err(b"x".to_vec()));
    }

    #[test]
    fn file_names() {
        let t = |name: Option<&'static [u8]>, term: Option<&'static [u8]>, ty| {
            Some(Tokens { name, term, ty })
        };
        assert_eq!(
            filename_to_tokens(b"disable"),
            t(None, None, FileType::Disable)
        );
        assert_eq!(
            filename_to_tokens(b"hexdump.enable"),
            t(Some(b"hexdump"), None, FileType::Enable)
        );
        assert_eq!(
            filename_to_tokens(b"@xterm.scheme"),
            t(None, Some(b"xterm"), FileType::Scheme)
        );
        assert_eq!(
            filename_to_tokens(b"dmesg@xterm.disable"),
            t(Some(b"dmesg"), Some(b"xterm"), FileType::Disable)
        );
        assert_eq!(filename_to_tokens(b"hexdump.other"), None);
        assert_eq!(filename_to_tokens(b".enable"), None);
    }

    #[test]
    fn not_a_terminal_is_never_unless_always() {
        let env = Env::default();
        assert!(!Colors::init_with(ColorMode::Auto, b"x", false, env.clone()).wanted());
        assert!(Colors::init_with(ColorMode::Always, b"x", false, env.clone()).wanted());
        // A terminal with no TERM is not ready.
        assert!(!Colors::init_with(ColorMode::Auto, b"x", true, env).wanted());
    }
}
