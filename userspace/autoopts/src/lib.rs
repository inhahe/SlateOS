//! GNU AutoGen's option library, libopts 41.1, as GNU sharutils 4.15.2
//! bundles it: the command-line, configuration-file and help machinery that
//! `uuencode` and `uudecode` share.
//!
//! sharutils does not parse its own options. Each program's `*-opts.def` is
//! turned by AutoGen into a table of option descriptors, and libopts drives
//! everything from that table: the three processing passes, the `$HOME/.sharrc`
//! file read between them, `--help`, `--more-help` through a pager,
//! `--version[=MODE]`, `--save-opts` writing the current options back out as
//! an rc file and `--load-opts` reading one in. None of it is visible in the
//! programs' sources, and all of it is visible to a user -- in the wording of
//! every error, in which argument a `-v` swallows, in what a stray line in
//! `~/.sharrc` does to a later `-m`. So it is ported as a library, function by
//! function, and each program supplies its descriptor table the way the
//! generated `*-opts.c` does.
//!
//! # The passes
//!
//! [`Options::process`] is `optionProcess`:
//!
//! 1. **Immediate.** The whole command line is scanned; every syntax error is
//!    reported here, before anything else happens. Options marked immediate
//!    run now, in order: `--help`, `--more-help` and `--version` (which exit),
//!    and `--no-load-opts`.
//! 2. **Presets.** Unless `--no-load-opts` was given, each `home_list` entry
//!    (`$HOME`) is read -- the rc file inside it if it is a directory, the
//!    entry itself if it is a file -- once in each direction. Errors in the
//!    file are silent. Options that may not be preset (the help and version
//!    family, `--save-opts`) are ignored there.
//! 3. **Regular.** The command line again, running everything that is not
//!    immediate. A program's own option procedures run here (uudecode's `-o`
//!    reopens standard output on the spot). Then `--save-opts` writes its
//!    file and exits.
//!
//! Processing stops at the first operand; `--` ends it and is consumed; `-`
//! alone is an operand. Long names may be abbreviated, compare without case,
//! and treat `-`, `_` and `^` as the same character.
//!
//! # What upstream does that is surprising, kept on purpose
//!
//! Each is measured against the Ubuntu 24.04 build of sharutils 4.15.2
//! (`scripts/uu-diff.sh`):
//!
//! * An optional argument is taken from the *next word* when it does not
//!   start with `-`: `uuencode -v file` reads `file` as the version mode and
//!   fails on its `f`.
//! * An rc file's last line is ignored unless it ends in a newline, and a
//!   trailing `\` does not continue a line -- the backslash is dropped and the
//!   next line is read as an entry of its own.
//! * In the XML-ish form, `<name>value</name>` loses the first byte of the
//!   value (`trim_xml_text` overwrites it with a space).
//! * A `<?program NAME>` directive for another program skips the rest of the
//!   file: later directives are never compared.
//! * In an rc file every option is handled in the second direction only, the
//!   presetting direction skipping everything, because `direction_ok` is
//!   given the state of the *line* rather than of the option.
//! * A `load-opts FILE` line inside `~/.sharrc` loads that file with its
//!   options counted as if typed, so a `base64` in it makes a later `-m` on the
//!   command line "one base64 option" too many.
//!
//! # Where this port departs
//!
//! * `--save-opts`' warnings pass one argument to a message with two `%s`
//!   (`save.c`); upstream prints whatever a register held -- nothing when
//!   `HOME` is unset, a stray byte elsewhere. This port prints nothing there.
//! * A file name or a word from the command line that reaches a diagnostic is
//!   [`shown`]: printable bytes as upstream prints them, an octal escape
//!   (`\012`) for each byte that is not, so no name can start a line of its
//!   own on stderr or drive the terminal. Upstream writes the raw bytes. For
//!   every printable name the text is upstream's (design-decisions.md §1033).
//! * No message catalogs: text is the C locale's, as everywhere in the tree.
//! * Only what sharutils' descriptors use: argument types none and string,
//!   optional arguments, disablement names. No equivalence classes, stacked
//!   arguments, environment presets, vendor or number options.

use std::ffi::OsStr;
use std::io;

mod charmap;
mod config;
mod cook;
mod find;
mod out;
mod pathfind;
mod save;
mod usage;

#[cfg(test)]
mod tests;

/// Option state bits (`OPTST_*`), numbered as `options.h` numbers them, since
/// descriptor tables are written in them.
pub mod st {
    /// Set by the program itself.
    pub const SET: u32 = 0x000_0001;
    /// Set by an rc file.
    pub const PRESET: u32 = 0x000_0002;
    /// Set on the command line.
    pub const DEFINED: u32 = 0x000_0004;
    /// Reset (`--reset-option`; not used by sharutils).
    pub const RESET: u32 = 0x000_0008;
    /// Selected under its disablement name (`--no-load-opts`).
    pub const DISABLED: u32 = 0x000_0020;
    /// May not be preset from an rc file.
    pub const NO_INIT: u32 = 0x000_0100;
    /// The argument may be omitted.
    pub const ARG_OPTIONAL: u32 = 0x001_0000;
    /// Handled in the immediate pass when enabled.
    pub const IMM: u32 = 0x002_0000;
    /// Handled in the immediate pass when disabled.
    pub const DISABLE_IMM: u32 = 0x004_0000;
    /// Compiled out.
    pub const OMITTED: u32 = 0x008_0000;
    /// A documentation-only entry.
    pub const DOCUMENT: u32 = 0x020_0000;
    /// Handled again in the regular pass when enabled.
    pub const TWICE: u32 = 0x040_0000;
    /// Handled again in the regular pass when disabled.
    pub const DISABLE_TWICE: u32 = 0x080_0000;
    /// Not accepted on the command line.
    pub const NO_COMMAND: u32 = 0x200_0000;
    /// How the option was set: one of `SET`, `PRESET`, `DEFINED`, `RESET`.
    pub const SET_MASK: u32 = 0x000_000F;
    /// The bits a descriptor keeps however it is set.
    pub const PERSISTENT_MASK: u32 = 0xFFF_FF00;
    /// Set on the command line or by the program.
    pub const SELECTED_MASK: u32 = 0x000_0005;
    /// Compiled out or documentation: never matched.
    pub const IMMUTABLE_MASK: u32 = 0x028_0000;
    /// Never written by `--save-opts`.
    pub const DO_NOT_SAVE_MASK: u32 = 0x028_0100;
    /// Never shown in usage.
    pub const NO_USAGE_MASK: u32 = 0x608_0000;
    /// The argument type, in bits 12-15.
    pub const ARG_TYPE_MASK: u32 = 0x000_F000;
    /// Argument type: a string (`OPTST_SET_ARGTYPE(OPARG_TYPE_STRING)`).
    pub const ARG_STRING: u32 = 0x000_1000;
}

/// Option-processing bits (`OPTPROC_*`).
pub mod pr {
    /// Long options are recognised.
    pub const LONGOPT: u32 = 0x00_0001;
    /// Short (flag character) options are recognised.
    pub const SHORTOPT: u32 = 0x00_0002;
    /// Errors are reported and exit; cleared while reading rc files.
    pub const ERRSTOP: u32 = 0x00_0004;
    /// No option is required.
    pub const NO_REQ_OPT: u32 = 0x00_0010;
    /// Options accept a leading number (`-#`).
    pub const NUM_OPT: u32 = 0x00_0020;
    /// Presets have been done.
    pub const INITDONE: u32 = 0x00_0040;
    /// Environment variables preset options.
    pub const ENVIRON: u32 = 0x00_0100;
    /// Operands and options may be intermixed.
    pub const REORDER: u32 = 0x00_0800;
    /// Usage is formatted the GNU way.
    pub const GNUUSAGE: u32 = 0x00_1000;
    /// A usage error prints the short usage, not the full one.
    pub const MISUSE: u32 = 0x00_4000;
    /// The immediate pass is running.
    pub const IMMEDIATE: u32 = 0x00_8000;
    /// Vendor (`-W`) options.
    pub const VENDOR_OPT: u32 = 0x04_0000;
    /// rc files are being read.
    pub const PRESETTING: u32 = 0x08_0000;
    /// Usage is computed from the descriptors, not printed from the text.
    pub const COMPUTE: u32 = 0x10_0000;
}

/// What part a descriptor plays in usage (`AOUSE_*`): the help option is
/// found by it for "Try '... --help'".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// One of the program's own options.
    User,
    /// `--version`.
    Version,
    /// `--help`.
    Help,
    /// `--more-help`.
    MoreHelp,
    /// `--save-opts`.
    SaveOpts,
    /// `--load-opts`.
    LoadOpts,
}

/// The procedure an option runs when handled (`pOptProc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// None: the option is only recorded.
    None,
    /// `optionPrintVersion`.
    PrintVersion,
    /// `doUsageOpt`: full usage to stdout, exit 0.
    Usage,
    /// `optionPagedUsage`.
    PagedUsage,
    /// `optionLoadOpt`.
    LoadOpt,
    /// The program's own, through [`Callbacks::option`].
    User,
}

/// One option descriptor (`tOptDesc`), as the generated table spells it.
#[derive(Debug)]
pub struct Desc {
    /// The flag character.
    pub value: u8,
    /// The long name.
    pub name: &'static str,
    /// The disablement name (`no-load-opts`), if any.
    pub disable_name: Option<&'static str>,
    /// The one-line description, for computed usage.
    pub text: &'static str,
    /// The compiled-in state: [`st`] bits and the argument type.
    pub flags: u32,
    /// How often the option must appear on the command line.
    pub min: u16,
    /// How often it may; [`NOLIMIT`] for no limit.
    pub max: u16,
    /// Its part in usage.
    pub role: Role,
    /// Its procedure.
    pub action: Action,
}

/// `NOLIMIT`: an option that may appear any number of times.
pub const NOLIMIT: u16 = u16::MAX;

/// The five descriptors AutoGen appends to every sharutils program's own, in
/// the order the generated tables list them: `--version[=MODE]`, `--help`,
/// `--more-help`, `--save-opts[=FILE]` and `--load-opts=FILE`
/// (`--no-load-opts`). The flags are those of a build with a working `fork`
/// and optional option arguments, which is every build that matters.
pub const STANDARD_DESCS: [Desc; 5] = [
    Desc {
        value: b'v',
        name: "version",
        disable_name: None,
        text: "output version information and exit",
        flags: st::ARG_STRING | st::ARG_OPTIONAL | st::IMM | st::NO_INIT,
        min: 0,
        max: 1,
        role: Role::Version,
        action: Action::PrintVersion,
    },
    Desc {
        value: b'h',
        name: "help",
        disable_name: None,
        text: "display extended usage information and exit",
        flags: st::IMM | st::NO_INIT,
        min: 0,
        max: 1,
        role: Role::Help,
        action: Action::Usage,
    },
    Desc {
        value: b'!',
        name: "more-help",
        disable_name: None,
        text: "extended usage information passed thru pager",
        flags: st::IMM | st::NO_INIT,
        min: 0,
        max: 1,
        role: Role::MoreHelp,
        action: Action::PagedUsage,
    },
    Desc {
        value: b'R',
        name: "save-opts",
        disable_name: None,
        text: "save the option state to a config file",
        flags: st::ARG_STRING | st::ARG_OPTIONAL | st::NO_INIT,
        min: 0,
        max: 1,
        role: Role::SaveOpts,
        action: Action::None,
    },
    Desc {
        value: b'r',
        name: "load-opts",
        disable_name: Some("no-load-opts"),
        text: "load options from a config file",
        flags: st::ARG_STRING | st::DISABLE_IMM,
        min: 0,
        max: NOLIMIT,
        role: Role::LoadOpts,
        action: Action::LoadOpt,
    },
];

/// The processing flags both sharutils programs are generated with:
/// errors stop, flags and long names both work, no option is required, GNU
/// usage layout, and a usage error prints the short usage.
pub const SHARUTILS_FLAGS: u32 =
    pr::ERRSTOP | pr::SHORTOPT | pr::LONGOPT | pr::NO_REQ_OPT | pr::GNUUSAGE | pr::MISUSE;

/// `AO_EXIT_REQ_USAGE`: a usage request that exits 0 with the full usage.
pub const EXIT_REQ_USAGE: i32 = 10064;

/// One program's option set (`tOptions`), as its generated `*-opts.c` fills
/// it in.
#[derive(Debug)]
pub struct Program {
    /// `program_name`: the name in "NAME fatal error:" and "NAME usage error:".
    pub name: &'static str,
    /// `pzPROGNAME`: the upper-cased name, for `[NAME]` rc sections.
    pub upper: &'static str,
    /// The rc file's name inside a home-list directory.
    pub rc_name: &'static str,
    /// `pzCopyright`: the version line and short copyright.
    pub copyright: &'static str,
    /// `pzCopyNotice`: the license notice.
    pub copy_notice: &'static str,
    /// `pzFullVersion`.
    pub full_version: &'static str,
    /// Where rc files live, lowest priority first.
    pub home_list: &'static [&'static str],
    /// `pzUsageTitle`, with one `%s` for the program name.
    pub usage_title: &'static str,
    /// `pzExplain`: printed after the options in computed usage.
    pub explain: Option<&'static str>,
    /// `pzDetail`: printed at the end of computed full usage.
    pub detail: Option<&'static str>,
    /// Where bug reports go.
    pub bug_addr: &'static str,
    /// The descriptors, the program's own first.
    pub descs: &'static [Desc],
    /// How many of `descs` are the program's own (`presetOptCt`).
    pub preset_ct: usize,
    /// The index of `--save-opts`; `--load-opts` follows it.
    pub save_opts: usize,
    /// The precomputed full usage text.
    pub full_usage: &'static str,
    /// The precomputed short usage text.
    pub short_usage: &'static str,
    /// The initial [`pr`] bits.
    pub proc_flags: u32,
    /// The exit code of a usage error (`NAME_EXIT_USAGE_ERROR`).
    pub usage_error: i32,
}

/// An option's run-time state: `fOptState`, `optOccCt` and `optArg`.
#[derive(Clone, Debug)]
pub struct OptState {
    /// [`st`] bits.
    pub flags: u32,
    /// Command-line occurrences.
    pub occ: u32,
    /// The argument, as last given.
    pub arg: Option<Vec<u8>>,
}

/// The process should exit with this status (`exit(3)`'s argument; its low
/// byte is what a parent sees).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit(pub i32);

impl Exit {
    /// The status a parent sees.
    #[must_use]
    pub fn status(self) -> u8 {
        (self.0 & 0xff) as u8
    }
}

/// A program's own option procedures.
pub trait Callbacks {
    /// Option `index` (whose [`Desc::action`] is [`Action::User`]) was just
    /// handled -- from the command line, an rc file or a `--load-opts` file.
    /// Its argument is `opts.arg(index)`.
    ///
    /// # Errors
    ///
    /// An [`Exit`], when the procedure ends the program.
    fn option(&mut self, opts: &Options, index: usize) -> Result<(), Exit>;
}

/// No procedures of the program's own.
#[derive(Debug, Default)]
pub struct NoCallbacks;

impl Callbacks for NoCallbacks {
    fn option(&mut self, _opts: &Options, _index: usize) -> Result<(), Exit> {
        Ok(())
    }
}

/// How an rc entry's value is processed (`tOptionLoadMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoadMode {
    /// Whitespace trimmed, quotes and escapes processed.
    Cooked,
    /// Whitespace trimmed, quotes processed.
    Uncooked,
    /// Everything kept.
    Keep,
}

/// The option set in use: the descriptor table, each option's state, and
/// where processing is on the command line.
#[derive(Debug)]
pub struct Options {
    prog: &'static Program,
    state: Vec<OptState>,
    /// `fOptSet`.
    set: u32,
    /// `pzProgName`: argv\[0\]'s last component.
    prog_name: Vec<u8>,
    /// `pzProgPath`: argv\[0\] found along `PATH`, or argv\[0\].
    prog_path: Vec<u8>,
    /// `origArgVect`.
    args: Vec<Vec<u8>>,
    /// `curOptIdx`.
    cur_idx: usize,
    /// `pzCurOpt`: a word and a byte offset into it.
    cur_opt: Option<(usize, usize)>,
    /// `option_load_mode`.
    load_mode: LoadMode,
}

impl Options {
    /// `validate_struct`: the program's options, for this command line.
    /// `args[0]` names the program, as `argv[0]`.
    #[must_use]
    pub fn new(prog: &'static Program, args: Vec<Vec<u8>>) -> Self {
        let argv0 = args.first().cloned().unwrap_or_default();
        let prog_name = match argv0.iter().rposition(|&b| b == b'/') {
            Some(slash) => argv0
                .get(slash.saturating_add(1)..)
                .unwrap_or_default()
                .to_vec(),
            None => argv0.clone(),
        };
        let path = std::env::var_os("PATH");
        let prog_path =
            pathfind::pathfind(path.as_deref().map(os_bytes).as_deref(), &argv0).unwrap_or(argv0);
        Options {
            prog,
            state: prog
                .descs
                .iter()
                .map(|d| OptState {
                    flags: d.flags,
                    occ: 0,
                    arg: None,
                })
                .collect(),
            set: prog.proc_flags,
            prog_name,
            prog_path,
            args,
            cur_idx: 0,
            cur_opt: None,
            load_mode: LoadMode::Uncooked,
        }
    }

    /// `optionProcess`: all three passes. Returns the index of the first
    /// operand in the argument vector.
    ///
    /// # Errors
    ///
    /// An [`Exit`] when processing ends the program: help, version,
    /// `--save-opts`, or an error already reported.
    pub fn process(&mut self, cb: &mut dyn Callbacks) -> Result<usize, Exit> {
        if !self.initialize(cb)? {
            return Ok(0);
        }
        if self.cur_idx == 0 {
            self.cur_idx = 1;
            self.cur_opt = None;
        }
        if self.regular_opts(cb)? == find::Res::Failure {
            return Ok(self.args.len());
        }
        let save = self.prog.save_opts;
        if self
            .state
            .get(save)
            .is_some_and(|s| s.flags & st::SELECTED_MASK != 0)
        {
            self.save_file();
            return Err(Exit(0));
        }
        Ok(self.cur_idx)
    }

    /// `HAVE_OPT`: the option was set in any way.
    #[must_use]
    pub fn have(&self, index: usize) -> bool {
        self.state
            .get(index)
            .is_some_and(|s| s.flags & st::SET_MASK != 0)
    }

    /// `ENABLED_OPT`: the option is not in its disabled state.
    #[must_use]
    pub fn enabled(&self, index: usize) -> bool {
        self.state
            .get(index)
            .is_some_and(|s| s.flags & st::DISABLED == 0)
    }

    /// `OPT_ARG`: the option's argument.
    #[must_use]
    pub fn arg(&self, index: usize) -> Option<&[u8]> {
        self.state.get(index).and_then(|s| s.arg.as_deref())
    }

    /// `COUNT_OPT`: command-line occurrences.
    #[must_use]
    pub fn count(&self, index: usize) -> u32 {
        self.state.get(index).map_or(0, |s| s.occ)
    }

    /// The argument vector, `argv[0]` first.
    #[must_use]
    pub fn args(&self) -> &[Vec<u8>] {
        &self.args
    }

    /// `pzProgName`.
    #[must_use]
    pub fn prog_name(&self) -> &[u8] {
        &self.prog_name
    }

    /// `pzProgPath`.
    #[must_use]
    pub fn prog_path(&self) -> &[u8] {
        &self.prog_path
    }

    /// `USAGE(code)`: `optionUsage`. The full usage to stdout for a request
    /// ([`EXIT_REQ_USAGE`] or 0), the short one to stderr otherwise.
    pub fn usage(&mut self, code: i32) -> Exit {
        self.option_usage(code)
    }

    /// `usage_message`: "NAME usage error:", the message, then the usage for
    /// the program's usage-error code.
    pub fn usage_message(&mut self, msg: &[u8]) -> Exit {
        ulclosestream::stderr_write(format!("{} usage error:\n", self.prog.name).as_bytes());
        ulclosestream::stderr_write(msg);
        self.option_usage(self.prog.usage_error)
    }

    /// `die`: "NAME fatal error:" and the message, on stderr; exit `code`.
    #[must_use]
    pub fn die(&self, code: i32, msg: &[u8]) -> Exit {
        ulclosestream::stderr_write(format!("{} fatal error:\n", self.prog.name).as_bytes());
        ulclosestream::stderr_write(msg);
        Exit(code)
    }

    /// `fserr`: `die` with "fserr ERRNO (REASON) performing 'OP' on NAME".
    /// The name is [`shown`]: upstream's bytes, unless they could forge a
    /// line.
    #[must_use]
    pub fn fserr(&self, code: i32, op: &str, fname: &[u8], err: &io::Error) -> Exit {
        let mut msg = format!(
            "fserr {} ({}) performing {} on ",
            err.raw_os_error().unwrap_or(0),
            errmsg::strerror(err),
            quoting::escaped_in_quotes(op.as_bytes())
        )
        .into_bytes();
        msg.extend_from_slice(&shown(fname));
        msg.push(b'\n');
        self.die(code, &msg)
    }
}

/// A file name or a word from the command line, as a diagnostic shows it:
/// the bytes upstream prints, where they are printable, and an octal escape
/// for each byte that is not -- so a name holding a newline cannot start a
/// line of its own on stderr, nor an escape sequence drive the terminal. For
/// every printable name this is upstream's text exactly (design-decisions.md
/// §1033, the rule `logger` set).
#[must_use]
pub fn shown(text: &[u8]) -> Vec<u8> {
    quoting::escape_unprintable(text).into_bytes()
}

/// [`shown`], inside the `'...'` upstream writes around it.
#[must_use]
pub fn shown_in_quotes(text: &[u8]) -> Vec<u8> {
    quoting::escaped_in_quotes(text).into_bytes()
}

/// An `OsStr` as the bytes the C program would have seen.
#[must_use]
pub fn os_bytes(s: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        s.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        s.to_string_lossy().into_owned().into_bytes()
    }
}

/// Bytes as an `OsStr`-backed path argument, for the file system calls.
#[must_use]
pub fn bytes_os(b: &[u8]) -> std::ffi::OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        OsStr::from_bytes(b).to_os_string()
    }
    #[cfg(not(unix))]
    {
        String::from_utf8_lossy(b).into_owned().into()
    }
}

/// `getenv`, as bytes.
pub(crate) fn getenv(name: &[u8]) -> Option<Vec<u8>> {
    if name.is_empty() || name.contains(&b'=') || name.contains(&0) {
        return None;
    }
    std::env::var_os(bytes_os(name)).map(|v| os_bytes(&v))
}
