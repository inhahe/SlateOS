//! ls — list directory contents.
//!
//! A port of GNU `ls` (coreutils **9.5**), written against its source rather
//! than against a memory of its behaviour, because almost every rule in it is
//! arbitrary in the precise sense: it cannot be re-derived, only copied. The
//! column allocator, the six-month recency window, the rule that a name's
//! *suffix* decides `-v` order, and the fact that `--time-style=nosuch` is not
//! an error unless `-l` is also given are all of that kind.
//!
//! The version matters and is not a detail: 9.5 changed how *every* width in
//! the program is measured — text that cannot be measured now widens no column
//! and is padded to no column, where 9.4 counted it at roughly a column per
//! byte. Some comments below cite measurements taken from the 9.4 that ships in
//! WSL; where they do, they say so, and the behaviour reproduced is 9.5's. See
//! `design-decisions.md` §366.
//!
//! # What it replaces
//!
//! The previous `ls` declared four options — `-l -a -h -1` — and did the rest
//! by hand. It held paths as [`String`], so a file whose name is not UTF-8
//! could not be listed at all; it treated every `--long-option` as a *file
//! name*, so `ls --color=never` tried to open a file called `--color=never`;
//! it had no column layout, so `ls` in a terminal printed one name per line;
//! it rendered the mode string itself, disagreeing with `chmod`'s; it printed
//! numeric uids because it could not read `/etc/passwd`; and it formatted
//! timestamps with a hand-rolled civil-from-days that assumed UTC.
//!
//! Every one of those is now somebody else's code: `coreutils::getopt`,
//! `coreutils::quote`, `modechange::permission_string`, `pwdb`, `localtime`,
//! `coreutils::human` and `coreutils::vercmp`. That is the point of the
//! rewrite rather than a side effect of it — `ls -v` and `sort -V` must agree,
//! `ls -l`'s owner column and `id`'s must agree, `ls -l`'s clock and `date`'s
//! must agree, and `ls -h`'s `1.5G` and `df -h`'s must agree.
//!
//! # The four rules that are not guessable
//!
//! **A file's *suffix* is cut off before `-v` compares it.** That is why `ls -v`
//! puts `a.b` before `a-b`, and it lives in `coreutils::vercmp` because
//! `sort -V` needs the identical answer.
//!
//! **The name column is laid out by a search, not a formula.** GNU tries every
//! column count from 1 to `line_length / 3 + 1` at once, tracking the widest
//! name in each cell of each candidate layout, and then takes the largest count
//! that still fits.
//!
//! **A timestamp older than six months prints its year instead of its clock**,
//! and "six months" is `31556952 / 2` seconds — half a mean Gregorian year —
//! measured against the time `ls` started, with the window *open* at both ends.
//!
//! **`--dired` is byte offsets into `ls`'s own output**, which is why output is
//! accumulated in memory rather than streamed: the offsets are only knowable
//! once the surrounding text has been written.
//!
//! Built only on unix-family targets — our `x86_64-slateos` presents as
//! `linux-musl`, so `cfg(unix)` matches.

#![cfg_attr(not(unix), allow(dead_code))]

use charwidth::char_width;
use coreutils::errmsg::strerror;
use coreutils::fnmatch::{Flags, fnmatch};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::human::{Opts, default_block_size, human_readable};
use coreutils::pathname::{base_len, last_component, last_component_offset};
use coreutils::quote::{Mb, Style, next_mb, os_bytes, quote, quoteaf, quotef};
use coreutils::vercmp::version;
use coreutils::xnum::{self, Status, strtol_fatal};
use modechange::{S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG, S_IFSOCK};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

/// `ls`'s usage status is **2**, not the 1 that almost every other utility
/// uses: 1 is already spent on "something went wrong with a file", which `ls`
/// reports while still producing a listing.
const LS: Program = Program::new("ls", 2);

/// GNU's own short-option string, copied verbatim from `decode_switches`.
///
/// It is copied rather than derived because it must also list the letters this
/// port handles differently — a letter missing from it turns its argument into
/// an operand, and `ls -w 80 .` would try to open a file called `80`.
const SHORT_OPTIONS: &str = "abcdfghiklmnopqrstuvw:xABCDFGHI:LNQRST:UXZ1";

/// GNU's long table, **in declaration order**, which is observable: a bad
/// prefix lists the possibilities in this order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("escape", Takes::Nothing),
    ("directory", Takes::Nothing),
    ("dired", Takes::Nothing),
    ("full-time", Takes::Nothing),
    ("group-directories-first", Takes::Nothing),
    ("human-readable", Takes::Nothing),
    ("inode", Takes::Nothing),
    ("kibibytes", Takes::Nothing),
    ("numeric-uid-gid", Takes::Nothing),
    ("no-group", Takes::Nothing),
    ("hide-control-chars", Takes::Nothing),
    ("reverse", Takes::Nothing),
    ("size", Takes::Nothing),
    ("width", Takes::Required),
    ("almost-all", Takes::Nothing),
    ("ignore-backups", Takes::Nothing),
    ("classify", Takes::Optional),
    ("file-type", Takes::Nothing),
    ("si", Takes::Nothing),
    ("dereference-command-line", Takes::Nothing),
    ("dereference-command-line-symlink-to-dir", Takes::Nothing),
    ("hide", Takes::Required),
    ("ignore", Takes::Required),
    ("indicator-style", Takes::Required),
    ("dereference", Takes::Nothing),
    ("literal", Takes::Nothing),
    ("quote-name", Takes::Nothing),
    ("quoting-style", Takes::Required),
    ("recursive", Takes::Nothing),
    ("format", Takes::Required),
    ("show-control-chars", Takes::Nothing),
    ("sort", Takes::Required),
    ("tabsize", Takes::Required),
    ("time", Takes::Required),
    ("time-style", Takes::Required),
    ("zero", Takes::Nothing),
    ("color", Takes::Optional),
    ("hyperlink", Takes::Optional),
    ("block-size", Takes::Required),
    ("context", Takes::Nothing),
    ("author", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Upstream's `MIN_COLUMN_WIDTH`: a name, plus the two spaces that separate it
/// from the next column.
const MIN_COLUMN_WIDTH: usize = 3;

// ------------------------------------------------------------------ enums ---

/// How the names are arranged on the page. GNU's `enum format`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    /// `-l`: one file per line, with mode, owner, size and time.
    Long,
    /// `-1`.
    OnePerLine,
    /// `-C`: columns, filled downwards.
    ManyPerLine,
    /// `-x`: columns, filled across.
    Horizontal,
    /// `-m`: comma-separated, wrapped.
    WithCommas,
}

/// GNU's `enum sort_type`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sort {
    Name,
    Extension,
    Width,
    Size,
    Version,
    Time,
    None,
}

/// Which of the four timestamps `-l` shows and `-t` sorts by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TimeType {
    Mtime,
    Ctime,
    Atime,
    Btime,
}

/// GNU's `ignore_mode`: how much of a directory is hidden by default.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IgnoreMode {
    /// Everything beginning with `.`, plus `--hide` and `--ignore`.
    Default,
    /// `-A`: only `.` and `..`, plus `--ignore`.
    DotAndDotDot,
    /// `-a`: only `--ignore`.
    Minimal,
}

/// The trailing character `-F`, `-p` and `--file-type` append.
///
/// [`Ord`] is load-bearing: GNU writes the "does this style mark anything but
/// directories" test as `file_type <= indicator_style`, and uses the same
/// ordering to index the string `"*=>@|"`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Indicator {
    None,
    Slash,
    FileType,
    Classify,
}

/// GNU's `enum dereference_symlink`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Deref {
    /// No option said; the default depends on the output format and is
    /// resolved once, after the whole command line has been read.
    Undefined,
    Never,
    CommandLineArguments,
    CommandLineSymlinkToDir,
    Always,
}

/// The `always`/`never`/`auto` argument shared by `--color`, `--classify` and
/// `--hyperlink`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum When {
    Always,
    Never,
    IfTty,
}

/// GNU's `enum filetype`. The order is the order of `filetype_letter`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FileType {
    Unknown,
    Fifo,
    Chardev,
    Directory,
    Blockdev,
    Normal,
    SymbolicLink,
    Sock,
    /// A union-mount tombstone: the entry that hides a file of the same name
    /// in a lower layer. `-l` prints it as `w`.
    ///
    /// Never constructed here, and it cannot be: upstream builds it from
    /// `readdir`'s `DT_WHT`, and `std::fs::FileType` exposes no such
    /// predicate and no way to reach the raw `d_type`. The variant is kept
    /// because it holds this enum's discriminants in `filetype_letter`'s
    /// order, and dropping it would silently move `arg_directory`'s letter.
    #[cfg_attr(unix, expect(dead_code, reason = "std exposes no DT_WHT"))]
    Whiteout,
    /// A directory named on the command line and being listed *as a file* —
    /// `ls -d`, or the header line a recursive listing prints for it.
    ArgDirectory,
}

/// GNU's `filetype_letter`, the first column of `-l`'s mode string.
///
/// Upstream spells it as the string `"?pcdb-lswd"` indexed by the enum, with a
/// `static_assert` tying the two lengths together. Note its last two
/// characters: [`FileType::ArgDirectory`] is a directory and takes `d`, the
/// *same* letter as [`FileType::Directory`] — the two differ in where the
/// listing puts them, not in what they are. Only [`FileType::Unknown`] takes
/// the `?`.
///
/// This is reached only for a file whose `stat` failed; anything stated gets
/// the letter from its mode instead. So the case it decides is `ls -ld` on a
/// directory that could not be stated, which prints `d?????????`.
const fn filetype_letter(kind: FileType) -> u8 {
    match kind {
        FileType::Unknown => b'?',
        FileType::Fifo => b'p',
        FileType::Chardev => b'c',
        FileType::Directory | FileType::ArgDirectory => b'd',
        FileType::Blockdev => b'b',
        FileType::Normal => b'-',
        FileType::SymbolicLink => b'l',
        FileType::Sock => b's',
        FileType::Whiteout => b'w',
    }
}

// -------------------------------------------------------- argmatch tables ---

/// `--format`. Seven words for five values, which is why `--format=h` resolves
/// (`horizontal` and `across` agree) while `--format=v` does not.
const FORMAT_ARGS: &[(&str, Format)] = &[
    ("verbose", Format::Long),
    ("long", Format::Long),
    ("commas", Format::WithCommas),
    ("horizontal", Format::Horizontal),
    ("across", Format::Horizontal),
    ("vertical", Format::ManyPerLine),
    ("single-column", Format::OnePerLine),
];

/// `--sort`. Note the absence of `name`: there is no word for the default.
const SORT_ARGS: &[(&str, Sort)] = &[
    ("none", Sort::None),
    ("time", Sort::Time),
    ("size", Sort::Size),
    ("extension", Sort::Extension),
    ("version", Sort::Version),
    ("width", Sort::Width),
];

/// `--time`. Nine words for four values.
const TIME_ARGS: &[(&str, TimeType)] = &[
    ("atime", TimeType::Atime),
    ("access", TimeType::Atime),
    ("use", TimeType::Atime),
    ("ctime", TimeType::Ctime),
    ("status", TimeType::Ctime),
    ("mtime", TimeType::Mtime),
    ("modification", TimeType::Mtime),
    ("birth", TimeType::Btime),
    ("creation", TimeType::Btime),
];

/// `--color`, `--classify`, `--hyperlink`. `force` and `none` are there for
/// compatibility with a different `color-ls`, and are why `--color=n` is
/// ambiguous rather than `never`.
const WHEN_ARGS: &[(&str, When)] = &[
    ("always", When::Always),
    ("yes", When::Always),
    ("force", When::Always),
    ("never", When::Never),
    ("no", When::Never),
    ("none", When::Never),
    ("auto", When::IfTty),
    ("tty", When::IfTty),
    ("if-tty", When::IfTty),
];

/// `--indicator-style`.
const INDICATOR_STYLE_ARGS: &[(&str, Indicator)] = &[
    ("none", Indicator::None),
    ("slash", Indicator::Slash),
    ("file-type", Indicator::FileType),
    ("classify", Indicator::Classify),
];

/// The four words `--time-style` takes, minus the two spellings that are not
/// words: a `posix-` prefix and a leading `+`. Both are handled before this
/// table is consulted.
const TIME_STYLE_ARGS: &[(&str, TimeStyle)] = &[
    ("full-iso", TimeStyle::FullIso),
    ("long-iso", TimeStyle::LongIso),
    ("iso", TimeStyle::Iso),
    ("locale", TimeStyle::Locale),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TimeStyle {
    FullIso,
    LongIso,
    Iso,
    Locale,
}

// ------------------------------------------------------------------ flags ---

/// One option, whichever way it was spelled.
///
/// GNU gets this for free — `getopt_long` returns the same `int` for `-a` and
/// `--all` because the table says so — and a port that matched on the two
/// spellings separately would be two option loops that have to agree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flag {
    All,
    Escape,
    Ctime,
    Directory,
    NoSortAll,
    LongNoOwner,
    HumanReadable,
    Inode,
    Kibibytes,
    LongFormat,
    WithCommas,
    NumericUidGid,
    LongNoGroup,
    Slash,
    HideControlChars,
    Reverse,
    Size,
    SortTime,
    Atime,
    SortVersion,
    Width,
    Horizontal,
    AlmostAll,
    IgnoreBackups,
    ManyPerLine,
    Dired,
    Classify,
    NoGroup,
    DerefCommandLine,
    Ignore,
    Dereference,
    Literal,
    QuoteName,
    Recursive,
    SortSize,
    Tabsize,
    SortNone,
    SortExtension,
    Context,
    OnePerLine,
    FileType,
    DerefCommandLineSymlinkToDir,
    Hide,
    IndicatorStyle,
    QuotingStyle,
    Format,
    Sort,
    Time,
    TimeStyle,
    FullTime,
    GroupDirectoriesFirst,
    ShowControlChars,
    Si,
    Color,
    Hyperlink,
    BlockSize,
    Author,
    Zero,
    Help,
    Version,
}

/// The short letter → [`Flag`] map. Every letter of [`SHORT_OPTIONS`] appears.
fn short_flag(letter: u8) -> Option<Flag> {
    Some(match letter {
        b'a' => Flag::All,
        b'b' => Flag::Escape,
        b'c' => Flag::Ctime,
        b'd' => Flag::Directory,
        b'f' => Flag::NoSortAll,
        b'g' => Flag::LongNoOwner,
        b'h' => Flag::HumanReadable,
        b'i' => Flag::Inode,
        b'k' => Flag::Kibibytes,
        b'l' => Flag::LongFormat,
        b'm' => Flag::WithCommas,
        b'n' => Flag::NumericUidGid,
        b'o' => Flag::LongNoGroup,
        b'p' => Flag::Slash,
        b'q' => Flag::HideControlChars,
        b'r' => Flag::Reverse,
        b's' => Flag::Size,
        b't' => Flag::SortTime,
        b'u' => Flag::Atime,
        b'v' => Flag::SortVersion,
        b'w' => Flag::Width,
        b'x' => Flag::Horizontal,
        b'A' => Flag::AlmostAll,
        b'B' => Flag::IgnoreBackups,
        b'C' => Flag::ManyPerLine,
        b'D' => Flag::Dired,
        b'F' => Flag::Classify,
        b'G' => Flag::NoGroup,
        b'H' => Flag::DerefCommandLine,
        b'I' => Flag::Ignore,
        b'L' => Flag::Dereference,
        b'N' => Flag::Literal,
        b'Q' => Flag::QuoteName,
        b'R' => Flag::Recursive,
        b'S' => Flag::SortSize,
        b'T' => Flag::Tabsize,
        b'U' => Flag::SortNone,
        b'X' => Flag::SortExtension,
        b'Z' => Flag::Context,
        b'1' => Flag::OnePerLine,
        _ => return None,
    })
}

/// The long name → [`Flag`] map. The name arrives already resolved by
/// `getopt`, so every arm here is an exact entry of [`LONG_OPTIONS`].
fn long_flag(name: &str) -> Option<Flag> {
    Some(match name {
        "all" => Flag::All,
        "escape" => Flag::Escape,
        "directory" => Flag::Directory,
        "dired" => Flag::Dired,
        "full-time" => Flag::FullTime,
        "group-directories-first" => Flag::GroupDirectoriesFirst,
        "human-readable" => Flag::HumanReadable,
        "inode" => Flag::Inode,
        "kibibytes" => Flag::Kibibytes,
        "numeric-uid-gid" => Flag::NumericUidGid,
        "no-group" => Flag::NoGroup,
        "hide-control-chars" => Flag::HideControlChars,
        "reverse" => Flag::Reverse,
        "size" => Flag::Size,
        "width" => Flag::Width,
        "almost-all" => Flag::AlmostAll,
        "ignore-backups" => Flag::IgnoreBackups,
        "classify" => Flag::Classify,
        "file-type" => Flag::FileType,
        "si" => Flag::Si,
        "dereference-command-line" => Flag::DerefCommandLine,
        "dereference-command-line-symlink-to-dir" => Flag::DerefCommandLineSymlinkToDir,
        "hide" => Flag::Hide,
        "ignore" => Flag::Ignore,
        "indicator-style" => Flag::IndicatorStyle,
        "dereference" => Flag::Dereference,
        "literal" => Flag::Literal,
        "quote-name" => Flag::QuoteName,
        "quoting-style" => Flag::QuotingStyle,
        "recursive" => Flag::Recursive,
        "format" => Flag::Format,
        "show-control-chars" => Flag::ShowControlChars,
        "sort" => Flag::Sort,
        "tabsize" => Flag::Tabsize,
        "time" => Flag::Time,
        "time-style" => Flag::TimeStyle,
        "zero" => Flag::Zero,
        "color" => Flag::Color,
        "hyperlink" => Flag::Hyperlink,
        "block-size" => Flag::BlockSize,
        "context" => Flag::Context,
        "author" => Flag::Author,
        "help" => Flag::Help,
        "version" => Flag::Version,
        _ => return None,
    })
}

// ----------------------------------------------------------------- config ---

/// Everything the command line decides, once it has all been read.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Config {
    format: Format,
    sort: Sort,
    sort_reverse: bool,
    time_type: TimeType,
    ignore_mode: IgnoreMode,
    hide_patterns: Vec<Vec<u8>>,
    ignore_patterns: Vec<Vec<u8>>,
    print_inode: bool,
    print_block_size: bool,
    print_owner: bool,
    print_group: bool,
    print_author: bool,
    print_scontext: bool,
    numeric_ids: bool,
    immediate_dirs: bool,
    recursive: bool,
    directories_first: bool,
    indicator_style: Indicator,
    dereference: Deref,
    dired: bool,
    eolbyte: u8,
    line_length: usize,
    max_idx: usize,
    tabsize: usize,
    qmark_funny_chars: bool,
    quoting_style: Style,
    /// gnulib's `filename_quoting_options`' `quote_these_too` — the extra
    /// bytes a *file name* singles out. See [`Style::quote_with`], and
    /// [`DIRNAME_EXTRA`] for the header line's own, different set.
    filename_extra: Vec<u8>,
    /// The word `--dired` echoes in its `//DIRED-OPTIONS//` line. It is the
    /// *name* of the style rather than the style, because two of the ten words
    /// name one value and only the first of them is ever printed.
    quoting_style_name: &'static str,
    align_variable_outer_quotes: bool,
    human_output_opts: Opts,
    output_block_size: u64,
    file_human_output_opts: Opts,
    file_output_block_size: u64,
    /// `[non-recent, recent]`, GNU's `long_time_format`.
    long_time_format: [Vec<u8>; 2],
    format_needs_stat: bool,
    format_needs_type: bool,
    check_symlink_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            format: Format::OnePerLine,
            sort: Sort::Name,
            sort_reverse: false,
            time_type: TimeType::Mtime,
            ignore_mode: IgnoreMode::Default,
            hide_patterns: Vec::new(),
            ignore_patterns: Vec::new(),
            print_inode: false,
            print_block_size: false,
            print_owner: true,
            print_group: true,
            print_author: false,
            print_scontext: false,
            numeric_ids: false,
            immediate_dirs: false,
            recursive: false,
            directories_first: false,
            indicator_style: Indicator::None,
            dereference: Deref::Undefined,
            dired: false,
            eolbyte: b'\n',
            line_length: 80,
            max_idx: 27,
            tabsize: 8,
            qmark_funny_chars: false,
            quoting_style: Style::Literal,
            filename_extra: Vec::new(),
            quoting_style_name: "literal",
            align_variable_outer_quotes: false,
            human_output_opts: Opts::NONE,
            // What `finish` computes with no options and an empty environment.
            // Upstream leaves a 0 here to mean "not yet chosen" and fills it in
            // after the option loop; that marker now lives in
            // `Settings::block_size_specified`, so this field is a block size at
            // every moment and `human_readable` can never divide by it.
            output_block_size: 1024,
            file_human_output_opts: Opts::NONE,
            file_output_block_size: 1,
            long_time_format: [b"%b %e  %Y".to_vec(), b"%b %e %H:%M".to_vec()],
            format_needs_stat: false,
            format_needs_type: false,
            check_symlink_mode: false,
        }
    }
}

/// The environment `ls` reads, gathered up so that parsing stays a pure
/// function of `(argv, env)` and can be unit-tested without exporting anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Environment {
    columns: Option<Vec<u8>>,
    tabsize: Option<Vec<u8>>,
    quoting_style: Option<Vec<u8>>,
    time_style: Option<Vec<u8>>,
    ls_block_size: Option<Vec<u8>>,
    block_size: Option<Vec<u8>>,
    posixly_correct: bool,
    stdout_isatty: bool,
    /// `hard_locale (LC_TIME)`: false only for exactly `C` and `POSIX`.
    ///
    /// It decides two separate things — whether `--time-style=posix-…` strips
    /// its prefix or gives up, and whether `locale` looks its formats up in the
    /// message catalogue — and it is *true* under `LC_ALL=C.UTF-8`, which is
    /// the trap: `C.UTF-8` is not `C`.
    hard_locale_time: bool,
}

/// What the command line asked for, once it has been understood.
enum Request {
    Help,
    Version,
    Run(Box<Config>, Vec<Vec<u8>>),
}

/// A command line that will not run, and everything to print about it.
#[derive(Debug, PartialEq, Eq)]
struct Refusal {
    /// Complete stderr lines, prefixed where GNU prefixes them.
    lines: Vec<String>,
    /// Whether `Try 'ls --help' for more information.` follows.
    referral: bool,
    status: i32,
}

impl Refusal {
    fn from_getopt(error: &getopt::Error) -> Self {
        Self {
            lines: vec![format!("ls: {}", error.sentence)],
            referral: error.referral.is_some(),
            status: error.status,
        }
    }

    /// One sentence and no referral, at `ls`'s fatal status. This is the shape
    /// of upstream's bare `error (LS_FAILURE, 0, …)` — `-w`, `-T` and
    /// `--block-size` all take it, and measured, none prints a referral.
    fn fatal(sentence: String) -> Self {
        Self {
            lines: vec![format!("ls: {sentence}")],
            referral: false,
            status: 2,
        }
    }

    fn print(&self, err: &mut dyn Write) {
        for line in &self.lines {
            // A diagnostic that cannot be written has nowhere left to be
            // reported, so the failure is deliberately dropped here.
            let _ = writeln!(err, "{line}");
        }
        if self.referral {
            let _ = writeln!(err, "Try 'ls --help' for more information.");
        }
    }
}

// ------------------------------------------------------------- small bits ---

/// GNU's `decode_line_length`: `None` if the spec is not a number, `Some(0)` —
/// meaning *no limit* — if it is too large to be one.
///
/// The base is 0, so `-w 0x50` is 80. The suffix list is empty rather than
/// absent, which is a distinction gnulib makes and this one keeps: `-w 1K` is
/// an invalid *suffix* rather than an invalid number, and both are rejected.
fn decode_line_length(spec: &[u8]) -> Option<u64> {
    let (value, status) = xnum::xstrtoumax_base(spec, 0, Some(b""));
    match status {
        // Upstream clamps at `MIN (PTRDIFF_MAX, SIZE_MAX)` and treats anything
        // above it as 0, i.e. as infinity.
        Status::Ok if value <= i64::MAX.unsigned_abs() => Some(value),
        Status::Ok | Status::Overflow => Some(0),
        Status::Invalid | Status::InvalidSuffix | Status::InvalidSuffixWithOverflow => None,
    }
}

/// gnulib's `human_options` with upstream's environment fallback restored.
///
/// `human_options (nullptr, …)` does not mean "no block size": gnulib looks at
/// `BLOCK_SIZE` from inside `humblock`, so `ls` reaches that variable without
/// naming it. Splitting the lookup out makes the chain visible at the call
/// site, where the fact that `LS_BLOCK_SIZE` wins over `BLOCK_SIZE` is a rule
/// of `ls` rather than of gnulib.
fn block_size_options(spec: Option<&[u8]>, posixly_correct: bool) -> (u64, Opts) {
    match spec {
        Some(text) => {
            let (size, opts, _) = coreutils::human::human_options(text, posixly_correct);
            (size, opts)
        }
        None => (default_block_size(posixly_correct), Opts::NONE),
    }
}

/// The `//DIRED-OPTIONS//` spelling of a style. The inverse of `Style::WORDS`,
/// which cannot simply be searched backwards by value in general — two words
/// name one value — but whose *first* match is the one upstream prints.
fn style_word(style: Style) -> &'static str {
    Style::WORDS
        .iter()
        .find(|(_, value)| *value == style)
        .map_or("literal", |(word, _)| *word)
}

/// The first line of an argmatch sentence — its "invalid argument … for …"
/// half, without the list of valid words that follows it.
fn first_line(sentence: &str) -> &str {
    sentence.split('\n').next().unwrap_or(sentence)
}

// ------------------------------------------------------------ option loop ---

/// The option loop's own locals, handed to [`finish`] in one piece.
struct Settings {
    kibibytes_specified: bool,
    /// Whether `-h`, `--si` or `--block-size=…` chose a block size. Upstream
    /// spells this `! output_block_size`, using zero in the field itself as a
    /// "not yet chosen" marker; here the marker lives outside the field so that
    /// a `Config` can never hold a block size that `human_readable` would
    /// divide by. Nothing else can produce the marker: GNU rejects
    /// `--block-size=0` outright (`ls: invalid --block-size argument '0'`).
    block_size_specified: bool,
    format_opt: Option<Format>,
    hide_control_chars_opt: Option<bool>,
    quoting_style_opt: Option<Style>,
    sort_opt: Option<Sort>,
    tabsize_opt: Option<u64>,
    width_opt: Option<u64>,
    time_style_option: Option<Vec<u8>>,
    print_hyperlink: bool,
}

/// GNU's `decode_switches`, up to the end of its `while` loop.
///
/// `err` is where warnings go — the three "ignoring invalid …" messages that
/// do **not** stop the parse. They are written as they are found rather than
/// collected, because a later option can be fatal and upstream's ordering puts
/// the warning first: `TABSIZE=x ls -C -w nan` prints the tab-size warning and
/// then the line-width error.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per option, in upstream's order; splitting it would hide that order"
)]
fn parse_args(
    argv: &[OsString],
    env: &Environment,
    err: &mut dyn Write,
) -> Result<Request, Refusal> {
    let mut cfg = Config::default();
    let mut operands: Vec<Vec<u8>> = Vec::new();

    // Upstream's "false or -1 unless a switch says otherwise" locals. They are
    // separate from `cfg` because "not set" is a distinct state that decides
    // what the environment and the tty are allowed to contribute.
    let mut set = Settings {
        kibibytes_specified: false,
        block_size_specified: false,
        format_opt: None,
        hide_control_chars_opt: None,
        quoting_style_opt: None,
        sort_opt: None,
        tabsize_opt: None,
        width_opt: None,
        time_style_option: None,
        print_hyperlink: false,
    };
    // Recognised, validated, and then dropped: see `known-issues.md`
    // → `TD-B-LS-ACCEPTS-COLOUR-AND-HYPERLINK-WITHOUT-EMITTING-EITHER`.
    let mut print_with_color = false;

    for item in LS.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let item = item.map_err(|error| Refusal::from_getopt(&error))?;
        let (flag, value, spelling): (Flag, Option<OsString>, String) = match item {
            Opt::Operand(word) => {
                operands.push(os_bytes(word).into_owned());
                continue;
            }
            Opt::Short(letter, value) => {
                let Some(flag) = short_flag(letter) else {
                    // Unreachable while `SHORT_OPTIONS` and `short_flag` agree;
                    // getopt has already rejected every other letter.
                    return Err(Refusal::from_getopt(&LS.invalid_option(letter)));
                };
                (flag, value, format!("-{}", char::from(letter)))
            }
            Opt::Long(name, value) => {
                let Some(flag) = long_flag(name) else {
                    return Err(Refusal::from_getopt(
                        &LS.unrecognized_option(format!("--{name}").as_bytes()),
                    ));
                };
                (flag, value, format!("--{name}"))
            }
        };
        let raw = value.as_deref().map(|v| os_bytes(v).into_owned());
        let arg = raw.as_deref().unwrap_or_default();

        match flag {
            Flag::Help => return Ok(Request::Help),
            Flag::Version => return Ok(Request::Version),
            Flag::All => cfg.ignore_mode = IgnoreMode::Minimal,
            Flag::AlmostAll => cfg.ignore_mode = IgnoreMode::DotAndDotDot,
            Flag::Escape => set.quoting_style_opt = Some(Style::Escape),
            Flag::Ctime => cfg.time_type = TimeType::Ctime,
            Flag::Atime => cfg.time_type = TimeType::Atime,
            Flag::Directory => cfg.immediate_dirs = true,
            // `-f` is five options at once, and the one that is *not* obvious
            // is that it un-sets `-s`: `ls -s -f` prints no block sizes.
            Flag::NoSortAll => {
                cfg.ignore_mode = IgnoreMode::Minimal;
                set.sort_opt = Some(Sort::None);
                if set.format_opt == Some(Format::Long) {
                    set.format_opt = None;
                }
                print_with_color = false;
                set.print_hyperlink = false;
                cfg.print_block_size = false;
            }
            Flag::FileType => cfg.indicator_style = Indicator::FileType,
            Flag::LongNoOwner => {
                set.format_opt = Some(Format::Long);
                cfg.print_owner = false;
            }
            Flag::LongNoGroup => {
                set.format_opt = Some(Format::Long);
                cfg.print_group = false;
            }
            Flag::NoGroup => cfg.print_group = false,
            Flag::HumanReadable => {
                cfg.human_output_opts = Opts::AUTOSCALE | Opts::SI | Opts::BASE_1024;
                cfg.file_human_output_opts = cfg.human_output_opts;
                cfg.output_block_size = 1;
                set.block_size_specified = true;
                cfg.file_output_block_size = 1;
            }
            Flag::Si => {
                cfg.human_output_opts = Opts::AUTOSCALE | Opts::SI;
                cfg.file_human_output_opts = cfg.human_output_opts;
                cfg.output_block_size = 1;
                set.block_size_specified = true;
                cfg.file_output_block_size = 1;
            }
            Flag::Inode => cfg.print_inode = true,
            Flag::Kibibytes => set.kibibytes_specified = true,
            Flag::LongFormat => set.format_opt = Some(Format::Long),
            Flag::WithCommas => set.format_opt = Some(Format::WithCommas),
            Flag::ManyPerLine => set.format_opt = Some(Format::ManyPerLine),
            Flag::Horizontal => set.format_opt = Some(Format::Horizontal),
            Flag::NumericUidGid => {
                cfg.numeric_ids = true;
                set.format_opt = Some(Format::Long);
            }
            Flag::Slash => cfg.indicator_style = Indicator::Slash,
            Flag::HideControlChars => set.hide_control_chars_opt = Some(true),
            Flag::ShowControlChars => set.hide_control_chars_opt = Some(false),
            Flag::Reverse => cfg.sort_reverse = true,
            Flag::Size => cfg.print_block_size = true,
            Flag::SortTime => set.sort_opt = Some(Sort::Time),
            Flag::SortSize => set.sort_opt = Some(Sort::Size),
            Flag::SortVersion => set.sort_opt = Some(Sort::Version),
            Flag::SortExtension => set.sort_opt = Some(Sort::Extension),
            Flag::SortNone => set.sort_opt = Some(Sort::None),
            Flag::Sort => {
                set.sort_opt = Some(
                    LS.argmatch(arg, &spelling, SORT_ARGS)
                        .map_err(|e| Refusal::from_getopt(&e))?,
                );
            }
            Flag::Time => {
                cfg.time_type = LS
                    .argmatch(arg, &spelling, TIME_ARGS)
                    .map_err(|e| Refusal::from_getopt(&e))?;
            }
            Flag::Format => {
                set.format_opt = Some(
                    LS.argmatch(arg, &spelling, FORMAT_ARGS)
                        .map_err(|e| Refusal::from_getopt(&e))?,
                );
            }
            Flag::IndicatorStyle => {
                cfg.indicator_style = LS
                    .argmatch(arg, &spelling, INDICATOR_STYLE_ARGS)
                    .map_err(|e| Refusal::from_getopt(&e))?;
            }
            Flag::QuotingStyle => {
                set.quoting_style_opt = Some(
                    LS.argmatch(arg, &spelling, Style::WORDS)
                        .map_err(|e| Refusal::from_getopt(&e))?,
                );
            }
            Flag::Width => {
                let Some(width) = decode_line_length(arg) else {
                    return Err(Refusal::fatal(format!(
                        "invalid line width: {}",
                        quote(arg)
                    )));
                };
                set.width_opt = Some(width);
            }
            Flag::Tabsize => {
                set.tabsize_opt = Some(
                    xnum::xnumtoumax(
                        arg,
                        0,
                        0,
                        i64::MAX.unsigned_abs(),
                        Some(b""),
                        "invalid tab size",
                    )
                    .map_err(Refusal::fatal)?,
                );
            }
            Flag::IgnoreBackups => {
                // Two patterns, not one: without the second, `ls -aB` would
                // still list `.foo~`.
                cfg.ignore_patterns.push(b"*~".to_vec());
                cfg.ignore_patterns.push(b".*~".to_vec());
            }
            Flag::Ignore => cfg.ignore_patterns.push(arg.to_vec()),
            Flag::Hide => cfg.hide_patterns.push(arg.to_vec()),
            Flag::Dired => {
                // `-D` is three switches in one, and the first two are not
                // documented in its `--help` line: it also selects the long
                // format and cancels `--hyperlink`. Measured, GNU ls 9.5, on
                // a directory of three files:
                //
                // ```text
                // ls --dired              ->  total 8 / drwxr-xr-x … a / …
                //                             //DIRED// 57 58 …
                // ls --dired --hyperlink  ->  the same rows, with no indent,
                //                             and no //DIRED// line at all
                // ```
                //
                // The first shows `-D` turning on the long format by itself
                // — nothing else on that command line asked for it. The
                // second shows that the cancelling is only a *default*: a
                // later `--hyperlink` sets the flag again, and `finish`'s
                // `dired && !print_hyperlink` then withdraws `dired`.
                set.format_opt = Some(Format::Long);
                set.print_hyperlink = false;
                cfg.dired = true;
            }
            Flag::Classify => {
                let when = match raw.as_deref() {
                    // `--classify` with no argument means `--classify=always`;
                    // `-F` can never carry one.
                    None => When::Always,
                    Some(text) => LS
                        .argmatch(text, "--classify", WHEN_ARGS)
                        .map_err(|e| Refusal::from_getopt(&e))?,
                };
                if when == When::Always || (when == When::IfTty && env.stdout_isatty) {
                    cfg.indicator_style = Indicator::Classify;
                }
            }
            Flag::Color => {
                let when = match raw.as_deref() {
                    None => When::Always,
                    Some(text) => LS
                        .argmatch(text, "--color", WHEN_ARGS)
                        .map_err(|e| Refusal::from_getopt(&e))?,
                };
                print_with_color =
                    when == When::Always || (when == When::IfTty && env.stdout_isatty);
            }
            Flag::Hyperlink => {
                let when = match raw.as_deref() {
                    None => When::Always,
                    Some(text) => LS
                        .argmatch(text, "--hyperlink", WHEN_ARGS)
                        .map_err(|e| Refusal::from_getopt(&e))?,
                };
                set.print_hyperlink =
                    when == When::Always || (when == When::IfTty && env.stdout_isatty);
            }
            Flag::DerefCommandLine => cfg.dereference = Deref::CommandLineArguments,
            Flag::DerefCommandLineSymlinkToDir => {
                cfg.dereference = Deref::CommandLineSymlinkToDir;
            }
            Flag::Dereference => cfg.dereference = Deref::Always,
            Flag::Literal => set.quoting_style_opt = Some(Style::Literal),
            Flag::QuoteName => set.quoting_style_opt = Some(Style::C),
            Flag::Recursive => cfg.recursive = true,
            // `-1` has no effect after `-l`, which is why `ls -l -1` is still a
            // long listing while `ls -1 -l` obviously is.
            Flag::OnePerLine => {
                if set.format_opt != Some(Format::Long) {
                    set.format_opt = Some(Format::OnePerLine);
                }
            }
            Flag::Author => cfg.print_author = true,
            Flag::Context => cfg.print_scontext = true,
            Flag::GroupDirectoriesFirst => cfg.directories_first = true,
            Flag::FullTime => {
                set.format_opt = Some(Format::Long);
                set.time_style_option = Some(b"full-iso".to_vec());
            }
            Flag::TimeStyle => set.time_style_option = Some(arg.to_vec()),
            Flag::BlockSize => {
                let (size, opts, status) =
                    coreutils::human::human_options(arg, env.posixly_correct);
                if let Some(sentence) = strtol_fatal(status, &spelling, arg) {
                    return Err(Refusal::fatal(sentence));
                }
                cfg.human_output_opts = opts;
                cfg.output_block_size = size;
                set.block_size_specified = true;
                cfg.file_human_output_opts = opts;
                cfg.file_output_block_size = size;
            }
            Flag::Zero => {
                cfg.eolbyte = 0;
                set.hide_control_chars_opt = Some(false);
                if set.format_opt != Some(Format::Long) {
                    set.format_opt = Some(Format::OnePerLine);
                }
                print_with_color = false;
                set.quoting_style_opt = Some(Style::Literal);
            }
        }
    }

    let _ = print_with_color;
    finish(cfg, env, err, set).map(|cfg| Request::Run(Box::new(cfg), operands))
}

/// Everything `decode_switches` does after its `while` loop, plus the four
/// derivations `main` makes from the result.
///
/// It is separate from [`parse_args`] only because the loop is already long;
/// the two are one function upstream, and the split is at upstream's own
/// `if (! output_block_size)` — spelled here as
/// `if !set.block_size_specified`, which is the same test on a flag rather
/// than on a zero stored in the field itself.
#[expect(
    clippy::too_many_lines,
    reason = "upstream's post-loop block, in upstream's order; the order is load-bearing"
)]
fn finish(
    mut cfg: Config,
    env: &Environment,
    err: &mut dyn Write,
    set: Settings,
) -> Result<Config, Refusal> {
    // The block-size chain. `LS_BLOCK_SIZE` wins over `BLOCK_SIZE`; either of
    // them being *set* also moves the `-l` size column onto the same footing,
    // which `-h` alone does not do; and `-k` overrides the pair, but only for
    // the block-count columns and not for the file sizes.
    if !set.block_size_specified {
        let spec = env.ls_block_size.as_deref().or(env.block_size.as_deref());
        let (size, opts) = block_size_options(spec, env.posixly_correct);
        cfg.output_block_size = size;
        cfg.human_output_opts = opts;
        if spec.is_some() {
            cfg.file_human_output_opts = opts;
            cfg.file_output_block_size = size;
        }
        if set.kibibytes_specified {
            cfg.human_output_opts = Opts::NONE;
            cfg.output_block_size = 1024;
        }
    }

    cfg.format = set.format_opt.unwrap_or(if env.stdout_isatty {
        Format::ManyPerLine
    } else {
        Format::OnePerLine
    });

    // The width is only *asked for* when it could matter, which is why
    // `COLUMNS=x ls -l` is silent and `COLUMNS=x ls -C` warns.
    let mut linelen = set.width_opt;
    if matches!(
        cfg.format,
        Format::ManyPerLine | Format::Horizontal | Format::WithCommas
    ) && linelen.is_none()
    {
        // Upstream's `TIOCGWINSZ` branch comes first and is absent here: our
        // terminal exports `COLUMNS` and has no window-size ioctl yet.
        if let Some(text) = env.columns.as_deref().filter(|text| !text.is_empty()) {
            match decode_line_length(text) {
                Some(width) => linelen = Some(width),
                None => {
                    let _ = writeln!(
                        err,
                        "ls: ignoring invalid width in environment variable COLUMNS: {}",
                        quote(text)
                    );
                }
            }
        }
    }
    cfg.line_length = usize::try_from(linelen.unwrap_or(80)).unwrap_or(usize::MAX);

    // The most columns the page could hold: every column is at least three
    // cells wide, and the first one carries no separator.
    cfg.max_idx = (cfg.line_length / MIN_COLUMN_WIDTH).saturating_add(usize::from(
        !cfg.line_length.is_multiple_of(MIN_COLUMN_WIDTH),
    ));

    if matches!(
        cfg.format,
        Format::ManyPerLine | Format::Horizontal | Format::WithCommas
    ) {
        match set.tabsize_opt {
            Some(size) => cfg.tabsize = usize::try_from(size).unwrap_or(usize::MAX),
            None => {
                cfg.tabsize = 8;
                if let Some(text) = env.tabsize.as_deref() {
                    match xnum::xstrtoumax_base(text, 0, Some(b"")) {
                        (value, Status::Ok) => {
                            cfg.tabsize = usize::try_from(value).unwrap_or(usize::MAX);
                        }
                        _ => {
                            let _ = writeln!(
                                err,
                                "ls: ignoring invalid tab size in environment variable TABSIZE: {}",
                                quote(text)
                            );
                        }
                    }
                }
            }
        }
    }

    cfg.qmark_funny_chars = set.hide_control_chars_opt.unwrap_or(env.stdout_isatty);

    // The style comes from the option, then from `QUOTING_STYLE`, then from
    // whether stdout is a terminal. A `QUOTING_STYLE` that does not resolve is
    // a warning and is then ignored, not an error.
    let mut style = set.quoting_style_opt;
    if style.is_none()
        && let Some(text) = env.quoting_style.as_deref()
    {
        match LS.argmatch(text, "--quoting-style", Style::WORDS) {
            Ok(value) => style = Some(value),
            Err(_) => {
                let _ = writeln!(
                    err,
                    "ls: ignoring invalid value of environment variable QUOTING_STYLE: {}",
                    quote(text)
                );
            }
        }
    }
    cfg.quoting_style = style.unwrap_or(if env.stdout_isatty {
        Style::ShellEscape
    } else {
        Style::Literal
    });
    cfg.quoting_style_name = style_word(cfg.quoting_style);
    cfg.filename_extra = filename_extra(cfg.quoting_style, cfg.indicator_style);

    // Only the three styles whose quotes are *conditional* need the padding
    // column, and only in a format that aligns anything.
    cfg.align_variable_outer_quotes = (cfg.format == Format::Long
        || (matches!(cfg.format, Format::ManyPerLine | Format::Horizontal)
            && cfg.line_length != 0))
        && matches!(
            cfg.quoting_style,
            Style::Shell | Style::ShellEscape | Style::CMaybe
        );

    // `--dired` is meaningful only with `-l` and without `--hyperlink`;
    // upstream drops it silently otherwise, and only *then* checks it against
    // `--zero` — so `ls --dired --zero` is fine and `ls -l --dired --zero` is
    // the error.
    cfg.dired = cfg.dired && cfg.format == Format::Long && !set.print_hyperlink;
    if cfg.eolbyte == 0 && cfg.dired {
        return Err(Refusal::fatal(
            "--dired and --zero are incompatible".to_string(),
        ));
    }

    // `-u` alone sorts by atime; `-lu` shows atime but sorts by name. The
    // distinction is the `format != long_format` here and nowhere else.
    cfg.sort = set.sort_opt.unwrap_or(
        if cfg.format != Format::Long
            && matches!(
                cfg.time_type,
                TimeType::Ctime | TimeType::Atime | TimeType::Btime
            )
        {
            Sort::Time
        } else {
            Sort::Name
        },
    );

    if cfg.format == Format::Long {
        cfg.long_time_format = time_formats(set.time_style_option.as_deref(), env)?;
    }

    // ---- `main`'s own derivations, which depend on the whole command line ---

    if cfg.directories_first {
        cfg.check_symlink_mode = true;
    }

    if cfg.dereference == Deref::Undefined {
        cfg.dereference = if cfg.immediate_dirs
            || cfg.indicator_style == Indicator::Classify
            || cfg.format == Format::Long
        {
            Deref::Never
        } else {
            Deref::CommandLineSymlinkToDir
        };
    }

    cfg.format_needs_stat = cfg.sort == Sort::Time
        || cfg.sort == Sort::Size
        || cfg.format == Format::Long
        || cfg.print_scontext
        || cfg.print_block_size;
    cfg.format_needs_type = !cfg.format_needs_stat
        && (cfg.recursive || cfg.indicator_style != Indicator::None || cfg.directories_first);

    Ok(cfg)
}

/// GNU's `--time-style` block: the `posix-` prefix, the `+FORMAT` spelling, and
/// the four words, in that order.
///
/// It runs **only when `-l` is in force**, which is why `ls --time-style=nosuch`
/// succeeds and `ls -l --time-style=nosuch` does not. That is not a shortcut
/// here either — the block is literally inside upstream's
/// `if (format == long_format)`.
fn time_formats(option: Option<&[u8]>, env: &Environment) -> Result<[Vec<u8>; 2], Refusal> {
    let default: [Vec<u8>; 2] = [b"%b %e  %Y".to_vec(), b"%b %e %H:%M".to_vec()];
    let mut style: &[u8] = option.or(env.time_style.as_deref()).unwrap_or(b"locale");

    // `posix-iso` means "iso, but only if the locale is not C" — and it says so
    // by *returning* from the whole function, leaving the default formats in
    // place rather than falling through to the word after the prefix.
    while style.starts_with(b"posix-") {
        if !env.hard_locale_time {
            return Ok(default);
        }
        style = style.get(6..).unwrap_or_default();
    }

    if style.first() == Some(&b'+') {
        let body = style.get(1..).unwrap_or_default();
        return match body.iter().position(|&byte| byte == b'\n') {
            None => Ok([body.to_vec(), body.to_vec()]),
            Some(cut) => {
                let head = body.get(..cut).unwrap_or_default().to_vec();
                let tail = body.get(cut.saturating_add(1)..).unwrap_or_default();
                if tail.contains(&b'\n') {
                    return Err(Refusal::fatal(format!(
                        "invalid time style format {}",
                        quote(head.as_slice())
                    )));
                }
                Ok([head, tail.to_vec()])
            }
        };
    }

    let chosen = LS
        .argmatch(style, "time style", TIME_STYLE_ARGS)
        .map_err(|error| Refusal {
            // Upstream does not use `XARGMATCH` here, because that would print
            // neither the `posix-` variants nor the `+FORMAT` line. It prints
            // the invalid-argument sentence, then this hand-built list, then
            // `usage (LS_FAILURE)` — so unlike every other argmatch failure in
            // `ls` this one carries a referral and exits 2 rather than 1.
            lines: std::iter::once(format!("ls: {}", first_line(&error.sentence)))
                .chain(std::iter::once("Valid arguments are:".to_string()))
                .chain(
                    TIME_STYLE_ARGS
                        .iter()
                        .map(|(word, _)| format!("  - [posix-]{word}")),
                )
                .chain(std::iter::once(
                    "  - +FORMAT (e.g., +%H:%M) for a 'date'-style format".to_string(),
                ))
                .collect(),
            referral: true,
            status: 2,
        })?;

    Ok(match chosen {
        TimeStyle::FullIso => {
            let format = b"%Y-%m-%d %H:%M:%S.%N %z".to_vec();
            [format.clone(), format]
        }
        TimeStyle::LongIso => {
            let format = b"%Y-%m-%d %H:%M".to_vec();
            [format.clone(), format]
        }
        TimeStyle::Iso => [b"%Y-%m-%d ".to_vec(), b"%m-%d %H:%M".to_vec()],
        // Upstream looks the two formats up in the message catalogue when the
        // locale is hard. We have no catalogue, so both cases are the default.
        TimeStyle::Locale => default,
    })
}

// ------------------------------------------------------ names and widths ---

/// gnulib's `quote_these_too` for the **directory header** line — `dir:`.
///
/// Only the colon, and never the space or the indicator characters, because
/// the header is not a name in a column and nothing is appended to it. That
/// asymmetry is measurable: `ls -b` prints a file called `a b` as `a\ b` and a
/// directory called `d e` as the header `d e:`.
const DIRNAME_EXTRA: &[u8] = b":";

/// gnulib's `quote_these_too` for a **file name**, as `decode_switches` builds
/// it: the space under `escape`, then the indicator characters this style can
/// append.
///
/// `&"*=>@|"[indicator_style - file_type]` is upstream's spelling of the
/// second half, which is why [`Indicator`] is [`Ord`]: `--file-type` (which
/// appends `/=>@|`) must quote `*` as well as the rest, while `-F` — one step
/// further along — appends `*` too but quotes only `=>@|`. The `/` a directory
/// gets is in neither set, because a `/` cannot appear in a name.
fn filename_extra(style: Style, indicator: Indicator) -> Vec<u8> {
    let mut out = Vec::new();
    if style == Style::Escape {
        out.push(b' ');
    }
    match indicator {
        Indicator::None | Indicator::Slash => {}
        Indicator::FileType => out.extend_from_slice(b"*=>@|"),
        Indicator::Classify => out.extend_from_slice(b"=>@|"),
    }
    out
}

/// The screen width of `text`, or `None` where GNU's `mbsnwidth` returns -1.
///
/// This is gnulib's `mbsnwidth` under `ls`'s `MBSWIDTH_FLAGS`, which is
/// `MBSW_REJECT_INVALID | MBSW_REJECT_UNPRINTABLE` — so it is not a width
/// function that happens to fail, it is one that **refuses the whole string**
/// the moment it meets a byte that is not valid UTF-8, a truncated sequence at
/// the end, or a character with no width. One unprintable character does not
/// cost its own width; it costs the width of the name.
///
/// The -1 does *not* become a zero at the call site — see [`display_width`] for
/// what it becomes instead, and why.
///
/// The one place we knowingly differ from GNU is the unprintable test:
/// `c32width` asks glibc's `iswprint`, a table generated from a particular
/// Unicode release, and `charwidth::char_width` asks a rule that cannot drift.
/// They agree on every assigned character and part company on unassigned ones.
/// See `design-decisions.md` §357.
fn mbs_width(text: &[u8]) -> Option<usize> {
    let mut width = 0usize;
    let mut rest = text;
    while let Some(&first) = rest.first() {
        // gnulib's printable-ASCII fast path, which is exactly 0x20..=0x7e.
        // `\x7f` is deliberately not in it and takes the slow path, where it
        // is rejected as unprintable.
        if (0x20..0x7f).contains(&first) {
            width = width.saturating_add(1);
            rest = rest.get(1..).unwrap_or_default();
            continue;
        }
        match next_mb(rest) {
            // Unreachable: `rest` is not empty.
            None => break,
            Some(Mb::Invalid | Mb::Incomplete) => return None,
            Some(Mb::Char(c, len)) => {
                width = width.saturating_add(char_width(c)?);
                rest = rest.get(len..).unwrap_or_default();
            }
        }
    }
    Some(width)
}

/// [`mbs_width`] with GNU's `MAX (0, …)` applied — which does nothing, so a
/// name it refuses is [`usize::MAX`] columns wide.
///
/// Upstream writes
///
/// ```c
/// size_t displayed_width IF_LINT ( = 0);
/// …
/// displayed_width = mbsnwidth (buf, len, MBSWIDTH_FLAGS);
/// displayed_width = MAX (0, displayed_width);
/// ```
///
/// The clamp is plainly meant to turn the -1 into a 0, and it cannot: the
/// variable is a `size_t`, so the -1 has already become `SIZE_MAX` by the time
/// `MAX` sees it, and `MAX (0, SIZE_MAX)` is `SIZE_MAX`. Every width the layout
/// code then computes from it wraps.
///
/// **This is reproduced deliberately, and it is observable.** Measured on a
/// directory holding `\abell`, `AAAA`, `BBBB` and `CCCC`, laid out `-CU` in
/// that order:
///
/// ```text
/// ls 9.4  ->  \abell··AAAA··BBBB··CCCC      (24 bytes)
/// ls 9.5  ->  \abellAAAA··BBBB··CCCC        (22 bytes)
/// ```
///
/// and with the refused name second instead of first, 9.5 emits a tab and a
/// space where the column asks for three spaces. 9.4 measured `\abell` as four
/// columns (its `mbsnwidth` flags let a control character cost nothing);
/// 9.5 refuses the name outright and then underflows. It is not a passing
/// regression either — coreutils master still has both lines verbatim, checked
/// 2026-08-22 — so this is GNU's behaviour from 9.5 onwards, not a slip in one
/// release. See `design-decisions.md` §367.
///
/// The blast radius is small: it takes a name holding a control character or a
/// byte that is not valid UTF-8, printed in a column format to something that
/// is not a terminal. On a terminal `ls` defaults to `-q`, which replaces those
/// bytes with `?` before any of this is asked.
fn display_width(text: &[u8]) -> usize {
    // `MAX (0, …)` on a `size_t`, spelled honestly.
    mbs_width(text).unwrap_or(usize::MAX)
}

/// `-q`: replace every character that will not print with a single `?`.
///
/// Returns the substituted bytes and their width, which is *not* the width of
/// the input — the point of the pass is that the result has one.
///
/// The unit is the character, not the byte, and the difference is the whole
/// reason this exists rather than a byte loop: a two-byte character that will
/// not print becomes **one** `?`, while a two-byte sequence that is not a
/// character at all becomes one `?` per byte. `caf\xc3\xa9` and `caf\xc3\xa9`
/// truncated to `caf\xc3` are three characters and one `?` apart.
///
/// GNU has a second, unibyte implementation of this for `MB_CUR_MAX == 1`,
/// which replaces every non-`isprint` *byte*. It is unreachable here: it needs
/// a locale whose charset is not UTF-8, and SlateOS has one charset.
fn qmark(text: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(text.len());
    let mut width = 0usize;
    let mut rest = text;
    while let Some(&first) = rest.first() {
        if (0x20..0x7f).contains(&first) {
            out.push(first);
            width = width.saturating_add(1);
            rest = rest.get(1..).unwrap_or_default();
            continue;
        }
        let (eat, keep) = match next_mb(rest) {
            // Unreachable: `rest` is not empty.
            None => break,
            // One byte skipped, one `?` — so a run of stray bytes becomes a
            // run of question marks rather than a single one.
            Some(Mb::Invalid) => (1, None),
            // A truncated sequence at the end is one `?` however long it is,
            // because there is no way to tell how much of it was meant.
            Some(Mb::Incomplete) => (rest.len(), None),
            Some(Mb::Char(c, len)) => (len, char_width(c).map(|w| (len, w))),
        };
        match keep {
            Some((len, w)) => {
                out.extend_from_slice(rest.get(..len).unwrap_or_default());
                width = width.saturating_add(w);
            }
            None => {
                out.push(b'?');
                width = width.saturating_add(1);
            }
        }
        rest = rest.get(eat..).unwrap_or_default();
    }
    (out, width)
}

/// GNU's `needs_quoting`: whether `style` would change `name` at all.
///
/// Upstream renders into a two-byte buffer and compares the first byte and the
/// length, which is not a shortcut — `quotearg_buffer` returns the length it
/// *would* have written — but it is the same predicate as this, and cheaper
/// than the full rendering it is deciding whether to skip.
///
/// The answer feeds two separate things: whether a name may bypass the general
/// quoting path entirely, and whether the *column* it sits in needs a leading
/// space so that quoted and unquoted names in it line up. See [`Rendered`].
fn needs_quoting(style: Style, extra: &[u8], name: &[u8]) -> bool {
    let rendered = style.quote_with(name, extra);
    rendered.first() != name.first() || rendered.len() != name.len()
}

/// A name as it will appear: the bytes, the columns they occupy, and whether a
/// space goes in front of them.
struct Rendered {
    bytes: Vec<u8>,
    width: usize,
    /// GNU's `pad`. Under a style whose quotes are *conditional*
    /// (`shell`, `shell-escape`, `c-maybe`), a listing where some names are
    /// quoted and others are not would have its columns off by one; the
    /// unquoted ones get a leading space so that the quote marks sit outside
    /// the column rather than inside it. It is set only when some name in this
    /// directory really was quoted — one space in front of every name in a
    /// directory that has none would be a bug, not an alignment.
    pad: bool,
}

/// GNU's `quote_name_buf`: render one name, measure it, and decide its
/// padding.
///
/// `general` is upstream's `needs_general_quoting`, which is `f->quoted`, and
/// its three values are three different things:
///
/// * `Some(false)` — this name was *measured* not to need quoting, so the
///   rendering is the name and the quoting pass is skipped outright.
/// * `Some(true)` — measured to need it. Render.
/// * `None` — upstream's `-1`, "not measured". Render. Every name gets this
///   once some earlier name in the directory has been found to need quoting,
///   because the only reason to measure was to find that out.
///
/// `cwd_some_quoted` is whether any name in *this directory* needed quoting;
/// it is what makes padding a property of the listing rather than of the name.
fn quote_name(
    cfg: &Config,
    extra: &[u8],
    cwd_some_quoted: bool,
    name: &[u8],
    general: Option<bool>,
) -> Rendered {
    // The `-q` pass runs on top of the three styles that do not escape
    // anything themselves. Under the other seven an unprintable byte has
    // already become a visible escape, so there is nothing left to replace.
    let needs_further = cfg.qmark_funny_chars
        && matches!(
            cfg.quoting_style,
            Style::Shell | Style::ShellAlways | Style::Literal
        );

    let (mut bytes, quoted) = if general == Some(false) {
        (name.to_vec(), false)
    } else {
        let out = cfg.quoting_style.quote_with(name, extra);
        let quoted = out.first() != name.first() || out.len() != name.len();
        (out, quoted)
    };

    let width = if needs_further {
        let (substituted, width) = qmark(&bytes);
        bytes = substituted;
        width
    } else {
        display_width(&bytes)
    };

    Rendered {
        bytes,
        width,
        pad: cfg.align_variable_outer_quotes && cwd_some_quoted && !quoted,
    }
}

// ------------------------------------------------------------ file records ---

/// One timestamp, as `ls` holds and compares it.
///
/// Nanoseconds are carried rather than dropped because `-t` genuinely uses
/// them: two files written in the same second by the same `cp` sort by the
/// sub-second part, and a listing that ordered them by name instead would be
/// wrong in the one case `-t` exists for.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
struct Ts {
    sec: i64,
    nsec: i64,
}

impl Ts {
    /// The value GNU leaves in the birth-time field when the filesystem has no
    /// birth time — not a real instant, and it is compared as one on purpose:
    /// `-t --time=birth` on such a filesystem sorts every file equally and the
    /// name tie-break decides, which is the ordering a user sees.
    const UNKNOWN: Self = Self { sec: -1, nsec: -1 };

    const fn is_known(self) -> bool {
        !(self.sec == -1 && self.nsec == -1)
    }
}

/// The parts of `struct stat` that `ls` reads.
///
/// It is a struct of our own rather than [`std::fs::Metadata`] because `ls`
/// needs fields that `Metadata`'s portable surface does not expose (`rdev` for
/// a device's major:minor, `blocks` for `-s`, the birth time for
/// `--time=birth`), and because a *failed* stat still has to leave a record
/// behind — `ls -l` prints a line for a dangling symlink, with `?` in every
/// column that the failed stat would have filled.
#[derive(Clone, Copy, Default, Debug)]
struct Stat {
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    size: i64,
    blocks: u64,
    rdev: u64,
    ino: u64,
    dev: u64,
    atime: Ts,
    mtime: Ts,
    ctime: Ts,
    btime: Ts,
}

/// GNU's `struct fileinfo`: one row of the listing.
#[derive(Clone, Debug)]
struct FileInfo {
    /// The name as it will be printed — the bare entry name inside a
    /// directory, but the whole operand for a file named on the command line,
    /// which is why `ls /etc/passwd` prints the path and `ls /etc` does not.
    name: Vec<u8>,
    /// The path `stat` was given, kept because a recursive listing has to
    /// reach an entry through its directory rather than through the name it
    /// prints.
    full_name: Vec<u8>,
    /// Where a symlink points, read only when something will show it: `-l`, or
    /// a classification that needs to know whether the target is a directory.
    linkname: Option<Vec<u8>>,
    /// Whether the `stat` succeeded. Every numeric field below is meaningless
    /// when this is false, and `-l` prints `?` for each of them.
    stat_ok: bool,
    stat: Stat,
    filetype: FileType,
    /// The target's mode when the link was followed, `0` otherwise. It is
    /// separate from `stat.mode` because a symlink to a directory must sort
    /// with the directories under `--group-directories-first` while still
    /// printing as a symlink.
    link_mode: u32,
    /// Whether a symlink's target could be reached. `-F` marks a dangling link
    /// with `@` and a live one by what it points at, so the two cases differ
    /// in the output and not only in an error.
    link_ok: bool,
    /// GNU's tri-state `f->quoted`: [`None`] is upstream's `-1`, "not
    /// measured". See [`quote_name`].
    quoted: Option<bool>,
    /// The cached screen width, filled in before the sort when the layout or
    /// `-U`-less width sorting will ask for it more than once.
    width: Option<usize>,
}

impl Default for FileInfo {
    fn default() -> Self {
        Self {
            name: Vec::new(),
            full_name: Vec::new(),
            linkname: None,
            stat_ok: false,
            stat: Stat {
                btime: Ts::UNKNOWN,
                ..Stat::default()
            },
            filetype: FileType::Unknown,
            link_mode: 0,
            link_ok: false,
            quoted: None,
            width: None,
        }
    }
}

// ------------------------------------------------------------------ sorting ---

/// GNU's `is_linked_directory`: what `--group-directories-first` counts as a
/// directory.
///
/// A symlink to a directory counts — but only once the link has been followed,
/// which `ls` does for this purpose and not for the printed type. That is why
/// the test reads [`FileInfo::link_mode`] as well as the file's own type.
fn is_linked_directory(f: &FileInfo) -> bool {
    matches!(f.filetype, FileType::Directory | FileType::ArgDirectory)
        || f.link_mode & S_IFMT == S_IFDIR
}

/// The extension `-X` sorts on: the last `.` and everything after it.
///
/// A name with no dot sorts as the empty string and therefore first — and so
/// does a *dot file*, because `strrchr` finds the leading dot and the
/// extension of `.bashrc` is the whole name. That is upstream's behaviour, not
/// an accident of this transcription: `ls -X` really does group `.bashrc` with
/// the `.bashrc`-extensioned files rather than with the extensionless ones.
fn extension(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b'.') {
        Some(at) => name.get(at..).unwrap_or_default(),
        None => b"",
    }
}

/// The cached width, or the width computed now — GNU's `fileinfo_name_width`.
///
/// The `+ pad` wraps rather than saturates because upstream's is a `size_t`
/// addition and the width it is added to may be [`usize::MAX`]; see
/// [`display_width`]. Saturating here would quietly repair GNU's underflow in
/// one place and not the eight others, which is worse than either answer.
fn name_width(cfg: &Config, cwd_some_quoted: bool, f: &FileInfo) -> usize {
    if let Some(width) = f.width {
        return width;
    }
    let out = quote_name(cfg, &cfg.filename_extra, cwd_some_quoted, &f.name, f.quoted);
    out.width.wrapping_add(usize::from(out.pad))
}

/// The comparison for one sort key, before `-r` and
/// `--group-directories-first` are applied.
///
/// Two of the keys run *backwards* — `-S` puts the largest first and `-t` the
/// newest — and that is a property of the key rather than of `-r`, which is
/// why `ls -Sr` gives smallest-first and not something else. Every key falls
/// back to the name, so the order is total and a listing does not depend on
/// the order the directory happened to be read in.
///
/// `strcoll` is a byte comparison here. Under a locale whose collation is
/// codepoint order — which `C.UTF-8` is, and SlateOS has no other — it is
/// exactly `strcmp`, so GNU's whole `use_strcmp` fallback (it re-sorts with
/// `strcmp` if `strcoll` sets `errno`) has nothing to fall back *from* and is
/// not reproduced.
fn key_cmp(cfg: &Config, cwd_some_quoted: bool, a: &FileInfo, b: &FileInfo) -> Ordering {
    let by_name = || a.name.cmp(&b.name);
    let time = |f: &FileInfo| match cfg.time_type {
        TimeType::Mtime => f.stat.mtime,
        TimeType::Ctime => f.stat.ctime,
        TimeType::Atime => f.stat.atime,
        TimeType::Btime => f.stat.btime,
    };
    match cfg.sort {
        Sort::Name | Sort::None => by_name(),
        Sort::Extension => extension(&a.name)
            .cmp(extension(&b.name))
            .then_with(by_name),
        // Upstream is `int diff = fileinfo_name_width (a) -
        // fileinfo_name_width (b); return diff ? diff : cmp (…)`, and both
        // conversions in that first line are load-bearing: the subtraction
        // wraps in `size_t`, and the 64-bit result is then narrowed to a
        // 32-bit `int`. For ordinary widths the pair is just a subtraction,
        // but a name [`display_width`] refused is `usize::MAX` wide, and
        // `SIZE_MAX - 4` narrows to -5 — so the *widest* name in the listing
        // sorts first. Measured, GNU ls 9.5, on `ab`, `abc`, `一一` and
        // `a\177bcd`:
        //
        // ```text
        // $ ls --sort=width -1
        // a\177bcd
        // ab
        // abc
        // 一一
        // ```
        Sort::Width => {
            let diff = name_width(cfg, cwd_some_quoted, a).wrapping_sub(name_width(
                cfg,
                cwd_some_quoted,
                b,
            ));
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "C's implicit size_t -> int narrowing, reproduced deliberately"
            )]
            let diff = diff as u32 as i32;
            if diff == 0 { by_name() } else { diff.cmp(&0) }
        }
        // Largest first, newest first: the key is compared the other way
        // round, not the pair.
        Sort::Size => b.stat.size.cmp(&a.stat.size).then_with(by_name),
        Sort::Time => time(b).cmp(&time(a)).then_with(by_name),
        // `filevercmp` answers `Equal` for names that are the same version but
        // not the same string — `f009` and `f9` — so the tie-break is not
        // optional. It is `strcmp` and not `strcoll` because `filevercmp` is
        // locale-independent and a locale-dependent secondary could disagree
        // with it.
        Sort::Version => version(&a.name, &b.name).then_with(by_name),
    }
}

/// The full comparison: directories first, then the key, then `-r`.
///
/// The order of the three matters and is upstream's. `-r` reverses the *key*
/// and not the directory grouping — `ls -r --group-directories-first` still
/// lists directories first — because upstream applies `dirfirst_check` to the
/// unswapped pair and hands it an already-reversed key comparator.
fn file_cmp(cfg: &Config, cwd_some_quoted: bool, a: &FileInfo, b: &FileInfo) -> Ordering {
    if cfg.directories_first {
        let grouped = is_linked_directory(b).cmp(&is_linked_directory(a));
        if grouped != Ordering::Equal {
            return grouped;
        }
    }
    let (x, y) = if cfg.sort_reverse { (b, a) } else { (a, b) };
    key_cmp(cfg, cwd_some_quoted, x, y)
}

/// GNU's `sort_files`, including the `update_current_files_info` pass that
/// caches every name's width first.
///
/// The cache is not only a saving: [`name_width`] is `O(n)` in the name and
/// the comparator is called `O(n log n)` times, so measuring inside the
/// comparator would make sorting a large directory quadratic in the *bytes*.
/// GNU caches it under exactly the conditions that ask for it twice.
fn sort_files(cfg: &Config, cwd_some_quoted: bool, files: &mut [FileInfo]) {
    if cfg.sort == Sort::Width
        || (cfg.line_length > 0 && matches!(cfg.format, Format::ManyPerLine | Format::Horizontal))
    {
        for f in files.iter_mut() {
            let out = quote_name(cfg, &cfg.filename_extra, cwd_some_quoted, &f.name, f.quoted);
            // Wrapping, to match [`name_width`]: the cache must hold exactly
            // what the uncached path would have computed.
            f.width = Some(out.width.wrapping_add(usize::from(out.pad)));
        }
    }
    if cfg.sort == Sort::None {
        return;
    }
    // A stable sort, where GNU uses a merge sort for the same reason: the
    // comparator is a total order only because every key falls back to the
    // name, and an unstable sort would still be free to reorder genuine ties.
    files.sort_by(|a, b| file_cmp(cfg, cwd_some_quoted, a, b));
}

// ------------------------------------------------- selecting and naming ---

/// GNU's `patterns_match`: does any `--hide`/`--ignore` pattern cover `name`?
///
/// `FNM_PERIOD` is the flag that makes `-I '*'` leave the dot files alone,
/// which is what lets `ls -a -I '*'` print `.`, `..` and `.hidden` and nothing
/// else. Measured, GNU ls 9.4.
fn patterns_match(patterns: &[Vec<u8>], name: &[u8]) -> bool {
    patterns.iter().any(|p| fnmatch(p, name, Flags::PERIOD))
}

/// GNU's `file_ignored`: is this directory entry hidden before it is ever
/// stat'd?
///
/// The three clauses answer to `-a`/`-A`, `--hide` and `--ignore` in that
/// order. Upstream writes the middle test as `! name[1 + (name[1] == '.')]`,
/// which reads as "the name is exactly `.` or exactly `..`" — under `-A` those
/// two are the only entries hidden.
///
/// `--hide` is consulted only in the default mode, so `-a` and `-A` both
/// cancel it while neither cancels `--ignore`. Measured: `ls -a --hide='*'`
/// prints everything, `ls --hide='d*'` drops `dirA` and `dirZ`.
fn file_ignored(cfg: &Config, name: &[u8]) -> bool {
    let dot_special = cfg.ignore_mode != IgnoreMode::Minimal
        && name.first() == Some(&b'.')
        && (cfg.ignore_mode == IgnoreMode::Default || name == b"." || name == b"..");
    dot_special
        || (cfg.ignore_mode == IgnoreMode::Default && patterns_match(&cfg.hide_patterns, name))
        || patterns_match(&cfg.ignore_patterns, name)
}

/// GNU's `attach`: the path `stat` is given for an entry of `dirname`.
///
/// Two special cases, both upstream's and both visible in the error messages a
/// failed listing prints. A `dirname` of exactly `.` contributes nothing, so
/// `ls .` reports `cannot access 'x'` and not `cannot access './x'`; and a
/// `dirname` that already ends in `/` does not get a second one, so `ls /`
/// reports `/x` rather than `//x`.
fn attach(dirname: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dirname.len().saturating_add(name.len()).saturating_add(1));
    if dirname != b"." {
        out.extend_from_slice(dirname);
        if !dirname.is_empty() && dirname.last() != Some(&b'/') {
            out.push(b'/');
        }
    }
    out.extend_from_slice(name);
    out
}

/// The full name `gobble_file` stats: `name` itself when there is nothing to
/// prepend, [`attach`]'s join otherwise.
///
/// An absolute `name` is used as-is even inside a directory listing, which is
/// how `ls -R /` reaches `/etc` without building `///etc`.
fn full_name_for(dirname: &[u8], name: &[u8]) -> Vec<u8> {
    if name.first() == Some(&b'/') || dirname.is_empty() {
        name.to_vec()
    } else {
        attach(dirname, name)
    }
}

/// gnulib's `file_name_concat`, which is *not* [`attach`].
///
/// `ls` joins a directory to an entry two different ways on purpose, and the
/// difference is visible: `attach` builds the path handed to `stat`, and drops
/// a `dirname` of `.` so an error reads `cannot access 'x'`; this one builds
/// the name a recursive listing will *print*, and keeps it, so `ls -R .`
/// heads the subdirectory `./dirA`. Measured, GNU ls 9.4.
///
/// The dir is truncated to the end of its last component, so a trailing run of
/// slashes collapses: `ls -R 'dirA//'` heads `dirA//` — the operand, printed
/// verbatim — but its children `dirA/sub1`. The `.` separator is gnulib's
/// answer to joining the root to an absolute base: `/` + `/foo` is `/./foo`,
/// because `//foo` names a different file on some POSIX systems.
fn file_name_concat(dir: &[u8], base: &[u8]) -> Vec<u8> {
    let dirbase_at = last_component_offset(dir);
    let dirbaselen = base_len(dir.get(dirbase_at..).unwrap_or_default());
    let dirlen = dirbase_at.saturating_add(dirbaselen);
    let sep = if dirbaselen == 0 {
        // The dir is a filesystem root.
        (base.first() == Some(&b'/')).then_some(b'.')
    } else {
        let ends_in_slash = dirlen
            .checked_sub(1)
            .and_then(|last| dir.get(last))
            .is_some_and(|&c| c == b'/');
        (!ends_in_slash && base.first() != Some(&b'/')).then_some(b'/')
    };
    let mut out = dir.get(..dirlen).unwrap_or_default().to_vec();
    out.extend(sep);
    out.extend_from_slice(base);
    out
}

/// GNU's `basename_is_dot_or_dotdot`, the guard that stops `-R` recursing
/// through `./././.` forever.
fn basename_is_dot_or_dotdot(name: &[u8]) -> bool {
    let base = last_component(name);
    base == b"." || base == b".."
}

/// Whether an entry has to be `stat`ed, GNU's condition at `ls.c:3400`.
///
/// The point of the condition is that a plain `ls` of a large directory makes
/// *no* `stat` calls at all: `readdir` on Linux supplies the inode and the
/// type, and nothing in the default output needs more than the name. Every
/// clause below names an option that does need more.
///
/// Three of upstream's clauses are absent, all of them `--color`'s: colouring
/// a directory needs its mode for the sticky and other-writable cases, and
/// colouring a regular file needs it for the executable and set-id cases.
/// `ls` here accepts `--color` and emits nothing for it (see
/// `known-issues.md`), so a `stat` driven by it could only change which errors
/// are printed, never the listing. `--hyperlink` is absent for the same
/// reason.
fn needs_stat(cfg: &Config, kind: FileType, inode_known: bool, command_line_arg: bool) -> bool {
    command_line_arg
        || cfg.format_needs_stat
        // Dereferencing changes both the inode and the type, and `readdir`
        // reports the link's, not the target's.
        || ((cfg.print_inode || cfg.format_needs_type)
            && matches!(kind, FileType::SymbolicLink | FileType::Unknown)
            && (cfg.dereference == Deref::Always || cfg.check_symlink_mode))
        || (cfg.print_inode && !inode_known)
        || (cfg.format_needs_type
            && (kind == FileType::Unknown
                || command_line_arg
                // `-F` marks an executable with `*`, and only the mode says
                // whether a regular file is one.
                || (kind == FileType::Normal && cfg.indicator_style == Indicator::Classify)))
}

/// A directory waiting to be listed. GNU's `struct pending`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pending {
    /// The path to open, or [`None`] for the marker that says a recursive
    /// listing has finished with `realname` and may stop guarding against
    /// loops through it.
    name: Option<Vec<u8>>,
    /// The name to print in the `dir:` header when it differs from `name` —
    /// a symlink named on the command line is opened through its target but
    /// announced under the name the user typed.
    realname: Option<Vec<u8>>,
    command_line_arg: bool,
}

/// GNU's `extract_dirs_from_files`: move the directories out of the listing
/// and into the queue.
///
/// `dirname` is [`None`] for the command-line pass, where `.` and `..` are
/// real operands to be listed, and `Some` inside a recursive one, where they
/// are the entries that would make the walk cycle.
///
/// Only `arg_directory` entries are *removed*; a plain `directory` found by
/// `-R` is both queued and left in the parent's listing, which is why a
/// recursive listing shows a subdirectory as a row under its parent and again
/// as a heading of its own.
///
/// The queue is a **stack**, not a FIFO, and that is what makes `-R`
/// depth-first: the children `print_dir` discovers are taken up before the
/// siblings queued alongside their parent. Upstream gets this from a
/// singly-linked list that `queue_directory` prepends to and the driver drains
/// from the head; here `pending` is a `Vec` pushed and popped at the back,
/// which is the same discipline. So the walk over the files is backwards and
/// the marker goes on *first* — exactly upstream's order — and the pops come
/// back out as the marker last, after every child it is marking the end of.
///
/// ```text
/// $ ls -R          ->  .  a  a/x  a/y  b  b/z
/// ```
///
/// A queue drained from the front would give `. a b a/x a/y b/z` instead.
fn extract_dirs_from_files(
    dirname: Option<&[u8]>,
    command_line_arg: bool,
    loop_detect: bool,
    files: &mut Vec<FileInfo>,
    pending: &mut Vec<Pending>,
) {
    if let Some(dir) = dirname
        && loop_detect
    {
        // The marker that says `dir` is done. It carries no name, so the
        // listing loop knows to pop it rather than open it.
        pending.push(Pending {
            name: None,
            realname: Some(dir.to_vec()),
            command_line_arg: false,
        });
    }

    for f in files.iter().rev() {
        let is_dir = matches!(f.filetype, FileType::Directory | FileType::ArgDirectory);
        if !is_dir || (dirname.is_some() && basename_is_dot_or_dotdot(&f.name)) {
            continue;
        }
        let name = match dirname {
            Some(dir) if f.name.first() != Some(&b'/') => file_name_concat(dir, &f.name),
            _ => f.name.clone(),
        };
        pending.push(Pending {
            name: Some(name),
            realname: f.linkname.clone(),
            command_line_arg,
        });
    }

    files.retain(|f| f.filetype != FileType::ArgDirectory);
}

// -------------------------------------------- the columns of a long line ---

/// The `/etc/passwd` and `/etc/group` lookups the owner and group columns do,
/// and `-n`'s decision not to do them.
struct Names {
    db: pwdb::Db,
    /// `-n`. It is held here rather than read from the [`Config`] because it
    /// is the *only* thing about the config these lookups depend on, and
    /// because it turns every lookup off — the files are then not read at all.
    numeric: bool,
}

impl Names {
    /// [`None`] means "print the number instead", which is what `-n` asks for
    /// and also what an id with no entry in the file gets.
    fn user(&self, uid: u32) -> Option<&[u8]> {
        (!self.numeric)
            .then(|| self.db.user_by_uid(uid))
            .flatten()
            .map(|u| u.name.as_slice())
    }

    fn group(&self, gid: u32) -> Option<&[u8]> {
        (!self.numeric)
            .then(|| self.db.group_by_gid(gid))
            .flatten()
            .map(|g| g.name.as_slice())
    }
}

/// GNU's `format_user_or_group`: one owner or group column, with its trailing
/// separator.
///
/// The two branches align *differently*, which is upstream's and is visible in
/// any listing of a directory with mixed owners:
///
/// ```text
/// $ ls -l  /var/lib   ->  drwxr-xr-x 3 root      root      ...
/// $ ls -ln /var/lib   ->  drwxr-xr-x 3    0   0            ...
/// ```
///
/// A name is padded on the right, a number on the left. So a single id with no
/// `/etc/passwd` entry does not merely print as a number — it prints
/// right-aligned in a column of left-aligned names.
///
/// Both branches emit one space *beyond* `width`; upstream's `do … while
/// (pad--)` runs once more than `pad`, and the numeric branch has the space in
/// its format string. That space is the field separator, so the caller does
/// not add one.
///
/// A name whose width [`mbs_width`] refuses is padded by **nothing** — not by
/// the full column, as measuring it at zero would give. That is upstream's
/// `width_gap = name_width < 0 ? 0 : width - name_width`, and it is the
/// visible half of the 9.4-to-9.5 change recorded in `design-decisions.md`
/// §366. Measured against both binaries, with a group column seven wide and a
/// group named `g\002bad`:
///
/// ```text
/// 9.4: … root sp g 002 b a d sp sp sp sp 0 …   padded as though four wide
/// 9.5: … root sp g 002 b a d sp 0 …            padded as though unmeasurable
/// ```
fn format_user_or_group(out: &mut Vec<u8>, name: Option<&[u8]>, id: u64, width: usize) {
    match name {
        Some(name) => {
            let pad = mbs_width(name).map_or(0, |w| width.saturating_sub(w));
            out.extend_from_slice(name);
            out.resize(out.len().saturating_add(pad).saturating_add(1), b' ');
        }
        None => {
            let text = id.to_string();
            let pad = width.saturating_sub(text.len());
            out.resize(out.len().saturating_add(pad), b' ');
            out.extend_from_slice(text.as_bytes());
            out.push(b' ');
        }
    }
}

/// The width [`format_user_or_group`] will take, not counting the separator,
/// or [`None`] for a name that cannot be measured.
///
/// The `None` is kept rather than clamped because the two callers want
/// different things from it: the column *accumulator* treats it as no
/// contribution, which a zero also achieves, while [`format_user_or_group`]
/// treats it as no padding, which a zero does not.
fn format_user_or_group_width(name: Option<&[u8]>, id: u64) -> Option<usize> {
    name.map_or_else(|| Some(id.to_string().len()), mbs_width)
}

/// GNU's `format_inode`: the `-i` column.
///
/// `?` where the number is not known — a file whose `stat` failed, and a
/// filesystem whose `readdir` gives no inode. The second case is spelled as
/// the number zero: `system.h`'s `NOT_AN_INODE_NUMBER = 0` is what
/// `gobble_file` is handed for a command-line argument and what `D_INO`
/// expands to where `struct dirent` has no `d_ino`, so upstream's test is
/// `f->stat_ok && f->stat.st_ino != NOT_AN_INODE_NUMBER` rather than
/// `stat_ok` alone. A real filesystem has no inode 0, so nothing is lost by
/// spending the value as a marker.
///
/// The `?` is a *byte*, not a number, so it does not widen the column — but
/// note that [`Widths::observe_inode`] deliberately measures the raw
/// `st_ino`, so a zero inode does contribute a width of 1 even though it
/// prints as one character anyway.
fn format_inode(f: &FileInfo) -> Vec<u8> {
    if f.stat_ok && f.stat.ino != 0 {
        f.stat.ino.to_string().into_bytes()
    } else {
        b"?".to_vec()
    }
}

/// The per-directory running maxima that make `-l`'s columns line up.
///
/// They are *per directory*, not per listing: [`clear_files`](Widths::default)
/// resets them between directories, which is why `ls -lR` can give two
/// directories different column widths. Each is the widest value **seen so
/// far**, so they are complete only once every file has been gobbled — which
/// is why `ls -l` cannot start printing until it has read the whole directory.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Widths {
    inode: usize,
    block_size: usize,
    nlink: usize,
    owner: usize,
    group: usize,
    author: usize,
    scontext: usize,
    major_device: usize,
    minor_device: usize,
    file_size: usize,
}

impl Widths {
    /// The columns that need a successful `stat`, observed in upstream's
    /// order.
    ///
    /// A device gets `major, minor` where a file gets its size, and the
    /// *size* column has to be wide enough for both — upstream widens
    /// `file_size_width` to `major + 2 + minor` for a device, counting the
    /// `, ` between them. That is why one character device in a directory
    /// widens every regular file's size column.
    fn observe_stated(&mut self, cfg: &Config, names: &Names, f: &FileInfo, blocks: u64) {
        if cfg.format == Format::Long || cfg.print_block_size {
            let text = human_readable(
                blocks,
                cfg.human_output_opts,
                ST_NBLOCKSIZE,
                cfg.output_block_size,
            );
            // A width `mbs_width` refuses contributes nothing, which is what
            // upstream's `if (block_size_width < len)` does with a -1 — so
            // `unwrap_or(0)` against a maximum that starts at zero is the same
            // test, not a clamp. (`human_readable` output is digits and a unit
            // letter, so this can only refuse under a hostile `--block-size`.)
            self.block_size = self.block_size.max(mbs_width(text.as_bytes()).unwrap_or(0));
        }

        if cfg.format == Format::Long {
            if cfg.print_owner {
                let w = format_user_or_group_width(names.user(f.stat.uid), u64::from(f.stat.uid));
                self.owner = self.owner.max(w.unwrap_or(0));
            }
            if cfg.print_group {
                let w = format_user_or_group_width(names.group(f.stat.gid), u64::from(f.stat.gid));
                self.group = self.group.max(w.unwrap_or(0));
            }
            if cfg.print_author {
                // GNU/Hurd's `st_author`. There is no such field on Linux or
                // on SlateOS, and upstream's `fstat-nofollow` shim defines it
                // as `st_uid`, so `--author` repeats the owner column.
                let w = format_user_or_group_width(names.user(f.stat.uid), u64::from(f.stat.uid));
                self.author = self.author.max(w.unwrap_or(0));
            }
        }

        if cfg.print_scontext {
            self.scontext = self.scontext.max(SCONTEXT_UNKNOWN.len());
        }

        if cfg.format == Format::Long {
            self.nlink = self.nlink.max(f.stat.nlink.to_string().len());

            let kind = f.stat.mode & S_IFMT;
            if kind == S_IFCHR || kind == S_IFBLK {
                let major = major_of(f.stat.rdev).to_string().len();
                let minor = minor_of(f.stat.rdev).to_string().len();
                self.major_device = self.major_device.max(major);
                self.minor_device = self.minor_device.max(minor);
                let together = self
                    .major_device
                    .saturating_add(2)
                    .saturating_add(self.minor_device);
                self.file_size = self.file_size.max(together);
            } else {
                let text = human_readable(
                    unsigned_file_size(f.stat.size),
                    cfg.file_human_output_opts,
                    1,
                    cfg.file_output_block_size,
                );
                self.file_size = self.file_size.max(mbs_width(text.as_bytes()).unwrap_or(0));
            }
        }
    }

    /// The inode column, observed for every file that survives to the end of
    /// `gobble_file`.
    ///
    /// It is outside the stat branch, so `ls -i` widens this from the inode
    /// `readdir` supplied without stating anything; but a file whose `stat`
    /// *failed* returns early and never reaches here, so a `?` does not widen
    /// it either.
    fn observe_inode(&mut self, cfg: &Config, f: &FileInfo) {
        if cfg.print_inode {
            self.inode = self.inode.max(f.stat.ino.to_string().len());
        }
    }
}

/// `ST_NBLOCKSIZE`: the unit `st_blocks` counts in, fixed at 512 bytes by
/// POSIX regardless of the filesystem's own block size.
const ST_NBLOCKSIZE: u64 = 512;

/// The security context of a file whose label could not be read — and, here,
/// of every file, since SlateOS has no SELinux. `ls -Z` prints it.
const SCONTEXT_UNKNOWN: &[u8] = b"?";

/// The major device number, Linux's encoding of `dev_t`.
const fn major_of(dev: u64) -> u64 {
    ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)
}

/// The minor device number, Linux's encoding of `dev_t`.
const fn minor_of(dev: u64) -> u64 {
    (dev & 0xff) | ((dev >> 12) & !0xff)
}

/// GNU's `unsigned_file_size`: POSIX requires a size to print without a sign.
///
/// A negative `st_size` is assumed to be a positive one that wrapped, so it is
/// reinterpreted rather than clamped — which is what `as u64` does here, and
/// what upstream's `size + (size < 0) * (OFF_T_MAX - OFF_T_MIN + 1)` does
/// there.
#[expect(
    clippy::cast_sign_loss,
    reason = "the reinterpretation is the specified behaviour, not an accident"
)]
const fn unsigned_file_size(size: i64) -> u64 {
    size as u64
}

// ----------------------------------------------- the time column of a line ---

/// Half a mean Gregorian year, in seconds: the window inside which a timestamp
/// is *recent* and prints a clock time rather than a year.
///
/// A Gregorian year averages 365.2425 days — 31 556 952 seconds — and upstream
/// writes the halving as `31556952 / 2` rather than the quotient so that the
/// year stays visible in the source. Spelled the same way here for the same
/// reason.
const SIX_MONTHS: i64 = 31_556_952 / 2;

/// The clock, the zone and the fallback width — everything the time column
/// needs that is fixed for a whole run.
///
/// It exists as a struct because upstream's three pieces are a mutable global
/// (`current_time`), a process-wide `localtz`, and a function-local `static`
/// cache inside `long_time_expected_width`. Gathering them makes the column a
/// function of its inputs and lets a test hand it a clock that does not move.
struct Times {
    zone: localtime::Zone,
    /// GNU's `current_time`. It starts *before every possible timestamp* —
    /// upstream's `TYPE_MINIMUM (time_t)` with `tv_nsec = -1` — so that the
    /// first file listed always reads the real clock.
    now: Ts,
    read_clock: fn() -> Ts,
    /// [`long_time_expected_width`], computed once because upstream caches it
    /// in a function-local `static`.
    fallback_width: usize,
}

/// GNU's `long_time_expected_width`: how wide the time column's `?` is.
///
/// It is the **non-recent** format rendered at the epoch — the one instant that
/// is always available — measured strictly, and zero if that measurement
/// refuses. Nothing else is padded to it: a timestamp that renders is emitted
/// at whatever width it comes out, which is why two `--time-style` formats of
/// different widths really do misalign the column:
///
/// ```text
/// $ ls -l --time-style=+"$(printf '%Y\n%Y-%m-%d')"
/// -rw-r--r-- 1 u u 0 2026       future
/// -rw-r--r-- 1 u u 0 2026-02-21 inside
/// ```
fn long_time_expected_width(cfg: &Config, zone: &localtime::Zone) -> usize {
    let format = cfg.long_time_format.first().map_or(&[][..], Vec::as_slice);
    let text = localtime::strftime(format, &zone.local(0, 0));
    mbs_width(&text).unwrap_or(0)
}

impl Times {
    fn new(cfg: &Config, zone: localtime::Zone, read_clock: fn() -> Ts) -> Self {
        let fallback_width = long_time_expected_width(cfg, &zone);
        Self {
            zone,
            now: Ts {
                sec: i64::MIN,
                nsec: -1,
            },
            read_clock,
            fallback_width,
        }
    }

    /// Whether `when` falls in the past six months, re-reading the clock first
    /// if the file appears to be in the future.
    ///
    /// The re-read is upstream's, and its comment gives the reason: a file may
    /// have been modified since the last time we looked at the clock, so a
    /// timestamp ahead of `current_time` is more likely a stale clock than a
    /// file from the future. It is also why the initial `current_time` is the
    /// minimum — the first file always triggers one real read.
    ///
    /// Both comparisons are strict, so a file stamped *exactly* now is not
    /// recent, and neither is one in the future.
    fn is_recent(&mut self, when: Ts) -> bool {
        if self.now < when {
            self.now = (self.read_clock)();
        }
        let six_months_ago = Ts {
            sec: self.now.sec.saturating_sub(SIX_MONTHS),
            nsec: self.now.nsec,
        };
        six_months_ago < when && when < self.now
    }

    /// The time column: the rendered timestamp and its trailing space, or the
    /// right-aligned `?` that stands in for a timestamp there is none of.
    ///
    /// Three cases, and upstream distinguishes them with one test on a sentinel
    /// byte (`if (s || !*p)`) that is worth spelling out:
    ///
    /// | Case | Output |
    /// |---|---|
    /// | the format rendered something | that, then one space |
    /// | the format is empty (`--time-style=+`) | one space, and no `?` |
    /// | there is no timestamp to render | `?` right-aligned in [`Self::fallback_width`], then one space |
    ///
    /// The second case is not a curiosity — it is why the test is written on
    /// the buffer rather than on the return value, since `nstrftime` returns 0
    /// both for "wrote nothing" and for "did not fit".
    ///
    /// The third case is reached by a failed `stat` (a dangling symlink under
    /// `-lL`, which prints `?` in every stat-derived column) and by
    /// `--time=birth` on a filesystem that has no birth time. Upstream has a
    /// fourth trigger — `localtime_rz` failing — which prints the raw seconds
    /// instead of `?`; that cannot happen here, because [`localtime::Tm`]
    /// carries a full `i64` year and so has no `struct tm`-style year overflow
    /// to fail on. The difference is confined to instants no filesystem can
    /// store: ext4 caps out in the year 2446.
    fn format(&mut self, out: &mut Vec<u8>, cfg: &Config, stat_ok: bool, when: Ts) {
        if stat_ok && when.is_known() {
            let recent = self.is_recent(when);
            // A negative `tv_nsec` is the birth-time sentinel, which
            // `is_known` has already excluded; anything else out of range is
            // a corrupt stat, and zero is what upstream's `int ns` would
            // print for it too.
            let nanos = u32::try_from(when.nsec).unwrap_or(0);
            let tm = self.zone.local(when.sec, nanos);
            let format = cfg
                .long_time_format
                .get(usize::from(recent))
                .map_or(&[][..], Vec::as_slice);
            out.extend_from_slice(&localtime::strftime(format, &tm));
            out.push(b' ');
            return;
        }
        let pad = self.fallback_width.saturating_sub(1);
        out.resize(out.len().saturating_add(pad), b' ');
        out.extend_from_slice(b"? ");
    }
}

/// The wall clock, as [`Times`] reads it. `gettime` in upstream.
///
/// A clock before the epoch is not an error here — it is what a machine with a
/// dead RTC reports, and `ls` has no business refusing to list a directory
/// because of it — so the pre-epoch case is carried as a negative second count
/// rather than clamped.
fn system_clock() -> Ts {
    let now = std::time::SystemTime::now();
    match now.duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => Ts {
            sec: i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
            nsec: i64::from(since.subsec_nanos()),
        },
        Err(before) => {
            let d = before.duration();
            Ts {
                sec: i64::try_from(d.as_secs())
                    .unwrap_or(i64::MAX)
                    .saturating_neg(),
                nsec: i64::from(d.subsec_nanos()),
            }
        }
    }
}

// ------------------------------------------------------------ the long line ---

/// GNU's `stdout` plus its two `--dired` obstacks, kept together because the
/// offsets in the obstacks are indices into the bytes.
///
/// Upstream tracks the position in a global `dired_pos` that it increments by
/// hand at every write, and gets away with it because every write goes through
/// one of the four `dired_*` helpers. Here the bytes are accumulated in a
/// [`Vec`] for the whole run — which `--dired` forces anyway, since an offset
/// is only knowable once the text in front of it exists — so the position is
/// [`Vec::len`] and cannot drift from the output it describes.
#[derive(Default, Debug)]
struct Out {
    buf: Vec<u8>,
    /// GNU's `dired_obstack`: the begin and end offset of every **file name**,
    /// printed as the `//DIRED//` line.
    dired: Vec<usize>,
    /// GNU's `subdired_obstack`: the same for the `dir:` **header** lines that
    /// `-R` prints, printed as `//SUBDIRED//`. They are separate because an
    /// editor following the output wants to descend into the second kind and
    /// visit the first.
    subdired: Vec<usize>,
}

/// Which `--dired` list a name's offsets belong in, or neither.
///
/// Upstream passes the obstack itself, and a null pointer for "do not record" —
/// which is what `print_long_format` passes for a symlink *target*, since the
/// target is not a file in this listing and an editor must not offer to open
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dired {
    No,
    Names,
    Headers,
}

impl Out {
    /// GNU's `dired_indent`: `--dired` shifts every long line right by two, so
    /// that the offsets it reports are not the offsets of an ordinary listing.
    fn indent(&mut self, cfg: &Config) {
        if cfg.dired {
            self.buf.extend_from_slice(b"  ");
        }
    }

    /// GNU's `push_current_dired_pos`, which is a no-op without `--dired` —
    /// the position is still tracked, but nothing asks for it.
    fn mark(&mut self, cfg: &Config, which: Dired) {
        if !cfg.dired {
            return;
        }
        let pos = self.buf.len();
        match which {
            Dired::No => {}
            Dired::Names => self.dired.push(pos),
            Dired::Headers => self.subdired.push(pos),
        }
    }
}

/// GNU's `print_name_with_quoting`: write one name and return the bytes it
/// took, **including** the alignment space in front of it.
///
/// The return is a byte count and not a screen width. That is upstream's, and
/// its comment says why: the number only feeds `start_col`, which exists so the
/// colour code can tell whether a name straddles a line boundary, and a byte
/// count is always at least the width it bounds — so the cheap number is the
/// safe one. It is reproduced because [`print_long_format`] passes it on to the
/// symlink target.
///
/// `symlink_target` swaps the name for [`FileInfo::linkname`] and, with it, two
/// other things: the target gets no alignment space (upstream's
/// `allow_pad = !symlink_target`, since padding the target would put a space
/// after the `->`), and it is not recorded in `//DIRED//`.
///
/// The target is rendered under the *name*'s [`FileInfo::quoted`] measurement,
/// which would be wrong — a name needing no quoting could point at a target
/// that does — were it not for the repair in `gobble_file`: upstream sets
/// `f->quoted = -1` when the link name needs quoting and the name did not, so
/// the "skip the quoting pass" shortcut is withdrawn from both.
fn print_name_with_quoting(
    out: &mut Out,
    cfg: &Config,
    cwd_some_quoted: bool,
    f: &FileInfo,
    symlink_target: bool,
    stack: Dired,
) -> usize {
    let empty = Vec::new();
    let name = if symlink_target {
        f.linkname.as_ref().unwrap_or(&empty)
    } else {
        &f.name
    };
    let rendered = quote_name(cfg, &cfg.filename_extra, cwd_some_quoted, name, f.quoted);

    let pad = usize::from(rendered.pad && !symlink_target);
    if pad == 1 {
        out.buf.push(b' ');
    }
    out.mark(cfg, stack);
    out.buf.extend_from_slice(&rendered.bytes);
    out.mark(cfg, stack);

    rendered.bytes.len().saturating_add(pad)
}

/// `S_IXUGO`: the three execute bits, which are what `-F` marks with `*`.
const S_IXUGO: u32 = 0o111;

/// GNU's `get_type_indicator`: the character `-F`, `-p` or `--file-type`
/// appends, or `None` for the files that get nothing.
///
/// Every test is `stat_ok ? <mode test> : <type test>`, so a file whose stat
/// failed is still classified — from what `readdir` said it was. The order is
/// upstream's and is not arbitrary:
///
/// * A **regular file** is decided first and takes `*` only under `-F`, and
///   only if it is executable *and* was stated — a `readdir` type cannot know
///   about the execute bits, so an unstated regular file is never starred.
/// * A **directory** takes `/` under all three styles, which is why the `-p`
///   test sits *after* it: `-p` marks directories and nothing else, so it
///   returns here and not before.
/// * The rest — `@`, `|`, `=` — are reached only by `--file-type` and `-F`.
///
/// Solaris doors (`>`) are omitted: `S_ISDOOR` expands to a constant `0` on
/// every system that lacks them, which is every system we target.
fn get_type_indicator(cfg: &Config, stat_ok: bool, mode: u32, kind: FileType) -> Option<u8> {
    // Upstream's `stat_ok ? S_ISxxx (mode) : type == xxx`, once.
    let is = |m: u32, k: FileType| {
        if stat_ok {
            mode & S_IFMT == m
        } else {
            kind == k
        }
    };

    if is(S_IFREG, FileType::Normal) {
        return (stat_ok && cfg.indicator_style == Indicator::Classify && mode & S_IXUGO != 0)
            .then_some(b'*');
    }
    // `arg_directory` is a directory too, and only the non-stat branch can
    // ever see it — a stated one is `S_ISDIR`.
    if is(S_IFDIR, FileType::Directory) || (!stat_ok && kind == FileType::ArgDirectory) {
        return Some(b'/');
    }
    if cfg.indicator_style == Indicator::Slash {
        return None;
    }
    if is(S_IFLNK, FileType::SymbolicLink) {
        return Some(b'@');
    }
    if is(S_IFIFO, FileType::Fifo) {
        return Some(b'|');
    }
    if is(S_IFSOCK, FileType::Sock) {
        return Some(b'=');
    }
    None
}

/// GNU's `print_type_indicator`: [`get_type_indicator`], written out.
fn print_type_indicator(out: &mut Out, cfg: &Config, stat_ok: bool, mode: u32, kind: FileType) {
    if let Some(c) = get_type_indicator(cfg, stat_ok, mode, kind) {
        out.buf.push(c);
    }
}

/// GNU's mode string for one file: ten characters, or the type letter and nine
/// `?` when the stat failed.
///
/// Upstream builds eleven and then truncates: the eleventh is POSIX's
/// "optional alternate access method flag", which becomes `+` for a file with
/// an ACL and `.` for one with only a security context. It is truncated
/// whenever *no* file in the directory has either — and since SlateOS has
/// neither ACLs nor SELinux, `any_has_acl` is false for every listing and the
/// character is always cut. That is why this returns ten and
/// [`modechange::mode_string`] returns ten: the eleventh has no source to come
/// from.
fn mode_string(f: &FileInfo) -> Vec<u8> {
    if f.stat_ok {
        modechange::mode_string(f.stat.mode).into_bytes()
    } else {
        let mut s = vec![b'?'; 10];
        if let Some(first) = s.first_mut() {
            *first = filetype_letter(f.filetype);
        }
        s
    }
}

/// The timestamp `--time` selected. `--time=birth` is the only one that can be
/// absent, and it reports its absence as [`Ts::UNKNOWN`] — upstream's
/// `btime_ok`, which is exactly `!(tv_sec == -1 && tv_nsec == -1)`.
const fn chosen_time(cfg: &Config, f: &FileInfo) -> Ts {
    match cfg.time_type {
        TimeType::Ctime => f.stat.ctime,
        TimeType::Mtime => f.stat.mtime,
        TimeType::Atime => f.stat.atime,
        TimeType::Btime => f.stat.btime,
    }
}

/// A number right-aligned in `width` columns, followed by the field separator.
///
/// This is upstream's `sprintf (p, "%*s ", width, text)` for the columns whose
/// content is digits — the inode, the link count, the two halves of a device
/// number. They cannot fail to measure, so unlike the owner column there is no
/// unmeasurable case to skip.
fn pad_left(out: &mut Vec<u8>, width: usize, text: &[u8]) {
    let pad = width.saturating_sub(text.len());
    out.resize(out.len().saturating_add(pad), b' ');
    out.extend_from_slice(text);
    out.push(b' ');
}

/// A *measured* value right-aligned in `width` columns, followed by the field
/// separator — the block-size and file-size columns.
///
/// Distinct from [`pad_left`] because these are measured with [`mbs_width`] and
/// so can refuse, in which case upstream emits no padding at all:
/// `for (int pad = size_width < 0 ? 0 : file_size_width - size_width; ...)`.
/// See `design-decisions.md` §366.
fn pad_left_measured(out: &mut Vec<u8>, width: usize, text: &[u8]) {
    let pad = mbs_width(text).map_or(0, |w| width.saturating_sub(w));
    out.resize(out.len().saturating_add(pad), b' ');
    out.extend_from_slice(text);
    out.push(b' ');
}

/// GNU's `print_long_format`: one line of `ls -l`.
///
/// The columns, in the order they are written:
///
/// | Column | When | Alignment |
/// |---|---|---|
/// | inode | `-i` | right, in `w.inode` |
/// | blocks | `-s` | right, in `w.block_size`, measured |
/// | mode | always | fixed ten |
/// | links | always | right, in `w.nlink` |
/// | owner | unless `-g` | left if a name, right if a number |
/// | group | unless `-G`/`-o` | as owner |
/// | author | `--author` | as owner |
/// | context | `-Z` | as owner |
/// | size *or* `major, minor` | always | right, in `w.file_size`, measured |
/// | time | always | as rendered |
/// | name | always | left, and last |
///
/// `--dired`'s two-space [`Out::indent`] goes in **front of the inode**, at the
/// very start of the line. Upstream's `dired_indent ()` call sits several
/// columns further down — after the link count — but the columns above it are
/// `sprintf`'d into a local buffer that is not flushed until later, while
/// `dired_indent` writes straight to `stdout`. So the two spaces overtake them
/// in the stream. Measured, GNU ls 9.5:
///
/// ```text
/// $ ls -lis --dired --time-style=long-iso plain
///   636229 4 -rw-r--r-- 1 inhahe inhahe 5 2023-11-14 22:13 plain
/// //DIRED// 57 62
/// ```
///
/// A file whose `stat` failed prints `?` in every column above that came from
/// the stat — but keeps its inode's width, its name and its indicator, because
/// those did not.
fn print_long_format(
    out: &mut Out,
    cfg: &Config,
    names: &Names,
    times: &mut Times,
    w: &Widths,
    cwd_some_quoted: bool,
    f: &FileInfo,
) {
    let when = chosen_time(cfg, f);

    out.indent(cfg);

    if cfg.print_inode {
        pad_left(&mut out.buf, w.inode, &format_inode(f));
    }

    if cfg.print_block_size {
        let blocks = if f.stat_ok {
            human_readable(
                f.stat.blocks,
                cfg.human_output_opts,
                ST_NBLOCKSIZE,
                cfg.output_block_size,
            )
        } else {
            "?".to_owned()
        };
        pad_left_measured(&mut out.buf, w.block_size, blocks.as_bytes());
    }

    out.buf.extend_from_slice(&mode_string(f));
    out.buf.push(b' ');
    let nlink = if f.stat_ok {
        f.stat.nlink.to_string()
    } else {
        "?".to_owned()
    };
    pad_left(&mut out.buf, w.nlink, nlink.as_bytes());

    // A failed stat sends every one of these through the *name* branch with
    // the literal `?`, so they are left-aligned like a name and not
    // right-aligned like the id they stand in for.
    let failed: Option<&[u8]> = (!f.stat_ok).then_some(b"?");
    if cfg.print_owner {
        let name = failed.or_else(|| names.user(f.stat.uid));
        format_user_or_group(&mut out.buf, name, u64::from(f.stat.uid), w.owner);
    }
    if cfg.print_group {
        let name = failed.or_else(|| names.group(f.stat.gid));
        format_user_or_group(&mut out.buf, name, u64::from(f.stat.gid), w.group);
    }
    if cfg.print_author {
        // GNU/Hurd's `st_author`, which is `st_uid` everywhere else.
        let name = failed.or_else(|| names.user(f.stat.uid));
        format_user_or_group(&mut out.buf, name, u64::from(f.stat.uid), w.author);
    }
    if cfg.print_scontext {
        // Upstream passes `f->scontext` as the *name* and a zero id, so this
        // column can never take the numeric branch.
        format_user_or_group(&mut out.buf, Some(SCONTEXT_UNKNOWN), 0, w.scontext);
    }

    let kind = f.stat.mode & S_IFMT;
    if f.stat_ok && (kind == S_IFCHR || kind == S_IFBLK) {
        // The size column has to hold `major, minor` too, so the major half
        // absorbs whatever slack the size column has over the pair — which is
        // what keeps a device line's *name* aligned with a plain file's.
        let major = major_of(f.stat.rdev).to_string();
        let minor = minor_of(f.stat.rdev).to_string();
        let together = w
            .major_device
            .saturating_add(2)
            .saturating_add(w.minor_device);
        let blanks = w.file_size.saturating_sub(together);
        pad_left_zero_sep(
            &mut out.buf,
            w.major_device.saturating_add(blanks),
            major.as_bytes(),
        );
        pad_left(&mut out.buf, w.minor_device, minor.as_bytes());
    } else {
        let size = if f.stat_ok {
            human_readable(
                unsigned_file_size(f.stat.size),
                cfg.file_human_output_opts,
                1,
                cfg.file_output_block_size,
            )
        } else {
            "?".to_owned()
        };
        pad_left_measured(&mut out.buf, w.file_size, size.as_bytes());
    }

    times.format(&mut out.buf, cfg, f.stat_ok, when);

    print_name_with_quoting(out, cfg, cwd_some_quoted, f, false, Dired::Names);

    if f.filetype == FileType::SymbolicLink {
        // A link whose target could not be *read* prints as a bare name: there
        // is nothing to put after the arrow, so upstream prints no arrow. It
        // also prints no indicator, which is why this is not an `else`.
        if f.linkname.is_some() {
            out.buf.extend_from_slice(b" -> ");
            print_name_with_quoting(out, cfg, cwd_some_quoted, f, true, Dired::No);
            if cfg.indicator_style != Indicator::None {
                // The indicator describes the *target*, and is chosen from the
                // target's mode with `unknown` as the fallback type — so a
                // dangling link that was still readable gets nothing.
                print_type_indicator(out, cfg, true, f.link_mode, FileType::Unknown);
            }
        }
    } else if cfg.indicator_style != Indicator::None {
        print_type_indicator(out, cfg, f.stat_ok, f.stat.mode, f.filetype);
    }
}

/// The major half of a device number: right-aligned in `width`, then `, `
/// rather than the single space [`pad_left`] appends.
fn pad_left_zero_sep(out: &mut Vec<u8>, width: usize, text: &[u8]) {
    let pad = width.saturating_sub(text.len());
    out.resize(out.len().saturating_add(pad), b' ');
    out.extend_from_slice(text);
    out.extend_from_slice(b", ");
}

// ------------------------------------------------------- the column layouts ---

/// GNU's `print_file_name_and_frills`: one name, with whatever `-i`, `-s` and
/// `-Z` put in front of it and whatever `-F` puts behind it.
///
/// This is the short-format counterpart of [`print_long_format`], and its three
/// prefix columns are the same three, laid out the same way — except under
/// `-m`, where every one of them collapses to its own natural width. That is
/// upstream's `format == with_commas ? 0 : <width>`, and it is what makes `-m`
/// a *flowing* list rather than a table: there are no columns to align to.
///
/// Upstream's `start_col` parameter and return value both exist only for
/// colour — the first tells the escape code whether the name straddles a line
/// boundary, the second is that name's byte length so the caller can work the
/// first out. Neither is reproduced, because neither has a consumer here: the
/// callers below all track position from
/// [`length_of_file_name_and_frills`] instead.
fn print_file_name_and_frills(
    out: &mut Out,
    cfg: &Config,
    w: &Widths,
    cwd_some_quoted: bool,
    f: &FileInfo,
) {
    let flowing = cfg.format == Format::WithCommas;

    if cfg.print_inode {
        let width = if flowing { 0 } else { w.inode };
        pad_left(&mut out.buf, width, &format_inode(f));
    }
    if cfg.print_block_size {
        let blocks = if f.stat_ok {
            human_readable(
                f.stat.blocks,
                cfg.human_output_opts,
                ST_NBLOCKSIZE,
                cfg.output_block_size,
            )
        } else {
            "?".to_owned()
        };
        let width = if flowing { 0 } else { w.block_size };
        // `printf("%*s ", …)`, which counts *bytes* — unlike the long
        // format's block column, which is measured. The two disagree only for
        // a block size whose unit letter is not ASCII, which no `--block-size`
        // can produce.
        pad_left(&mut out.buf, width, blocks.as_bytes());
    }
    if cfg.print_scontext {
        let width = if flowing { 0 } else { w.scontext };
        pad_left(&mut out.buf, width, SCONTEXT_UNKNOWN);
    }

    print_name_with_quoting(out, cfg, cwd_some_quoted, f, false, Dired::Names);

    if cfg.indicator_style != Indicator::None {
        print_type_indicator(out, cfg, f.stat_ok, f.stat.mode, f.filetype);
    }
}

/// GNU's `length_of_file_name_and_frills`: what
/// [`print_file_name_and_frills`] will occupy, computed before it is called.
///
/// The column allocator needs this for every file *before* it can decide how
/// many columns there are, which is why the layout formats cannot start
/// printing until the whole directory has been read.
///
/// The inode's contribution under `-m` is the width of the raw `st_ino` and
/// not of what [`format_inode`] would print — so a file with no inode is
/// measured as `0` and printed as `?`. Both are one column, so the discrepancy
/// is invisible; it is reproduced rather than tidied because it is upstream's
/// `strlen (umaxtostr (f->stat.st_ino, buf))`.
fn length_of_file_name_and_frills(
    cfg: &Config,
    w: &Widths,
    cwd_some_quoted: bool,
    f: &FileInfo,
) -> usize {
    let flowing = cfg.format == Format::WithCommas;
    let mut len = 0usize;

    if cfg.print_inode {
        let own = if flowing {
            f.stat.ino.to_string().len()
        } else {
            w.inode
        };
        len = len.saturating_add(1).saturating_add(own);
    }
    if cfg.print_block_size {
        let own = if flowing {
            if f.stat_ok {
                human_readable(
                    f.stat.blocks,
                    cfg.human_output_opts,
                    ST_NBLOCKSIZE,
                    cfg.output_block_size,
                )
                .len()
            } else {
                1
            }
        } else {
            w.block_size
        };
        len = len.saturating_add(1).saturating_add(own);
    }
    if cfg.print_scontext {
        let own = if flowing {
            SCONTEXT_UNKNOWN.len()
        } else {
            w.scontext
        };
        len = len.saturating_add(1).saturating_add(own);
    }

    // Wrapping, not saturating: the name's width is `usize::MAX` when GNU's
    // `mbsnwidth` refused it, and every length derived from it wraps in
    // upstream's `size_t` arithmetic. See [`display_width`].
    len = len.wrapping_add(name_width(cfg, cwd_some_quoted, f));

    if cfg.indicator_style != Indicator::None
        && get_type_indicator(cfg, f.stat_ok, f.stat.mode, f.filetype).is_some()
    {
        // Wrapping for the same reason: `-F` on a name of width `usize::MAX`
        // gives width 0, which is what GNU's `len += (c != 0)` gives.
        len = len.wrapping_add(1);
    }
    len
}

/// One candidate layout: `i + 1` columns wide, in GNU's `column_info`.
#[derive(Clone, Debug)]
struct ColumnInfo {
    /// Whether this many columns still fits. Once it stops fitting it is never
    /// reconsidered — a column can only ever grow — so this doubles as a
    /// short-circuit for the rest of the scan.
    valid_len: bool,
    /// The width of the whole line under this layout, separators included.
    line_len: usize,
    /// The width of each column, which is why the search is quadratic: a
    /// layout is not `n` equal columns but `n` independently-sized ones.
    col_arr: Vec<usize>,
}

/// GNU's `calculate_columns`: the column count, found by trying every one of
/// them at once.
///
/// There is no formula. The number of columns that fits depends on *which*
/// names land in which column, which depends on the number of columns — so
/// upstream evaluates all `max_cols` candidate layouts in a single pass over
/// the files, growing each layout's per-column maxima as it goes, and then
/// takes the widest layout that never overflowed. It is `O(files × columns)`,
/// which is the reason for the `max_idx` cap on `max_cols`.
///
/// `by_columns` is the difference between `-C` and `-x`, and it is only this
/// line: which column a file lands in. Down the page,
/// `filesno / ceil(n / cols)`; across it, `filesno % cols`.
///
/// The `+ 2` is [`MIN_COLUMN_WIDTH`]'s two separating spaces, charged to every
/// column but the last — which is why a listing can be exactly `line_length`
/// wide without wrapping.
///
/// Returns the count and that layout's column widths.
fn calculate_columns(cfg: &Config, lengths: &[usize], by_columns: bool) -> (usize, Vec<usize>) {
    let n = lengths.len();
    // Upstream's "normally the screen decides, but few files can decide too".
    let max_cols = if cfg.max_idx > 0 && cfg.max_idx < n {
        cfg.max_idx
    } else {
        n
    };
    if max_cols == 0 {
        return (1, vec![MIN_COLUMN_WIDTH]);
    }

    let mut info: Vec<ColumnInfo> = (0..max_cols)
        .map(|i| ColumnInfo {
            valid_len: true,
            line_len: i.saturating_add(1).saturating_mul(MIN_COLUMN_WIDTH),
            col_arr: vec![MIN_COLUMN_WIDTH; i.saturating_add(1)],
        })
        .collect();

    for (filesno, &name_length) in lengths.iter().enumerate() {
        for (i, candidate) in info.iter_mut().enumerate() {
            if !candidate.valid_len {
                continue;
            }
            let cols = i.saturating_add(1);
            let idx = if by_columns {
                // `(n + i) / (i + 1)` is `ceil(n / cols)`: the rows this
                // layout needs. It cannot be zero, because `max_cols <= n`.
                let rows = n.saturating_add(i) / cols;
                filesno / rows.max(1)
            } else {
                filesno % cols
            };
            // Wrapping, twice, because upstream's is `size_t` arithmetic on a
            // width that may be `usize::MAX` — see [`display_width`]. It is
            // this line that produces the visible effect: a refused name in a
            // non-final column has `real_length` 1, which never beats
            // `MIN_COLUMN_WIDTH`, so its column stays three wide while the name
            // itself is measured as `usize::MAX` when the time comes to pad it.
            let real_length = name_length.wrapping_add(if idx == i { 0 } else { 2 });
            let Some(slot) = candidate.col_arr.get_mut(idx) else {
                continue;
            };
            if *slot < real_length {
                candidate.line_len = candidate
                    .line_len
                    .wrapping_add(real_length.wrapping_sub(*slot));
                *slot = real_length;
                candidate.valid_len = candidate.line_len < cfg.line_length;
            }
        }
    }

    let mut cols = max_cols;
    while cols > 1 {
        if info
            .get(cols.saturating_sub(1))
            .is_some_and(|c| c.valid_len)
        {
            break;
        }
        cols = cols.saturating_sub(1);
    }
    let widths = info
        .get(cols.saturating_sub(1))
        .map(|c| c.col_arr.clone())
        .unwrap_or_else(|| vec![MIN_COLUMN_WIDTH]);
    (cols, widths)
}

/// GNU's `indent`: move the cursor from column `from` to column `to`, using a
/// tab wherever one lands exactly where a run of spaces would.
///
/// The test is `to / tabsize > (from + 1) / tabsize`, which is not the obvious
/// "is there a tab stop between here and there". The `+ 1` makes it refuse to
/// emit a tab that would save only a single space, so `ls` never turns one
/// space into a tab — a tab that renders as one column on the terminal it was
/// measured for renders as eight somewhere else, and a listing that is only
/// correct at one tab width is worse than one that is a byte longer.
///
/// `tabsize` of zero is `-T0`, which asks for spaces only.
fn indent(out: &mut Vec<u8>, tabsize: usize, mut from: usize, to: usize) {
    while from < to {
        if tabsize != 0 && to / tabsize > from.saturating_add(1) / tabsize {
            out.push(b'\t');
            // Advance to the next tab stop, not by a whole `tabsize`.
            from = from.saturating_add(tabsize.saturating_sub(from % tabsize));
        } else {
            out.push(b' ');
            from = from.saturating_add(1);
        }
    }
}

/// GNU's `print_many_per_line` (`-C`): names down the page, then across.
///
/// The row loop stops at the *file* count and not at the column count, so the
/// last column is short rather than the last row — which is the whole visible
/// difference from `-x`.
fn print_many_per_line(
    out: &mut Out,
    cfg: &Config,
    w: &Widths,
    cwd_some_quoted: bool,
    files: &[&FileInfo],
    lengths: &[usize],
) {
    let n = files.len();
    let (cols, col_arr) = calculate_columns(cfg, lengths, true);
    let rows = n / cols + usize::from(!n.is_multiple_of(cols));

    for row in 0..rows {
        let mut col = 0usize;
        let mut filesno = row;
        let mut pos = 0usize;
        while let (Some(f), Some(&name_length)) = (files.get(filesno), lengths.get(filesno)) {
            let max_name_length = col_arr.get(col).copied().unwrap_or(MIN_COLUMN_WIDTH);
            col = col.saturating_add(1);
            print_file_name_and_frills(out, cfg, w, cwd_some_quoted, f);

            filesno = filesno.saturating_add(rows);
            if filesno >= n {
                break;
            }
            // `pos + name_length` wraps: a name GNU's `mbsnwidth` refused is
            // `usize::MAX` wide, so this lands one column *before* `pos` and
            // [`indent`] pads one more than the column asks — or, at `pos` 0,
            // lands past `to` and pads nothing at all. See [`display_width`].
            indent(
                &mut out.buf,
                cfg.tabsize,
                pos.wrapping_add(name_length),
                pos.saturating_add(max_name_length),
            );
            pos = pos.saturating_add(max_name_length);
        }
        out.buf.push(cfg.eolbyte);
    }
}

/// GNU's `print_horizontal` (`-x`): names across the page, then down.
fn print_horizontal(
    out: &mut Out,
    cfg: &Config,
    w: &Widths,
    cwd_some_quoted: bool,
    files: &[&FileInfo],
    lengths: &[usize],
) {
    let (cols, col_arr) = calculate_columns(cfg, lengths, false);
    let mut pos = 0usize;
    let mut name_length = lengths.first().copied().unwrap_or(0);
    let mut max_name_length = col_arr.first().copied().unwrap_or(MIN_COLUMN_WIDTH);

    let Some(first) = files.first() else { return };
    print_file_name_and_frills(out, cfg, w, cwd_some_quoted, first);

    for (filesno, f) in files.iter().enumerate().skip(1) {
        let col = filesno % cols;
        if col == 0 {
            out.buf.push(cfg.eolbyte);
            pos = 0;
        } else {
            // Wrapping for the same reason as in `print_many_per_line`.
            indent(
                &mut out.buf,
                cfg.tabsize,
                pos.wrapping_add(name_length),
                pos.saturating_add(max_name_length),
            );
            pos = pos.saturating_add(max_name_length);
        }
        print_file_name_and_frills(out, cfg, w, cwd_some_quoted, f);
        name_length = lengths.get(filesno).copied().unwrap_or(0);
        max_name_length = col_arr.get(col).copied().unwrap_or(MIN_COLUMN_WIDTH);
    }
    out.buf.push(cfg.eolbyte);
}

/// GNU's `print_with_separator`: names separated by `sep` and a space, wrapped
/// at the line width.
///
/// It serves two formats. `-m` passes a comma; `-C` and `-x` pass a *space*
/// when there is no line width to wrap at (`-w0`), because with no width there
/// are no columns to compute and a single flowing line is all that is left.
///
/// The wrap test is `pos + len + 2 < line_length`, strictly — so a name that
/// would end exactly at the last column still wraps. The separator that starts
/// the next line is the `sep` *and* the newline, in that order: `-m` output
/// ends its lines with a comma.
fn print_with_separator(
    out: &mut Out,
    cfg: &Config,
    w: &Widths,
    cwd_some_quoted: bool,
    files: &[&FileInfo],
    lengths: &[usize],
    sep: u8,
) {
    let mut pos = 0usize;
    for (filesno, f) in files.iter().enumerate() {
        let len = if cfg.line_length == 0 {
            0
        } else {
            lengths.get(filesno).copied().unwrap_or(0)
        };

        if filesno != 0 {
            // Upstream's guard is
            //
            // ```c
            // (pos + len + 2 < line_length) && (pos <= SIZE_MAX - len - 2)
            // ```
            //
            // and both halves are wrapping `size_t` arithmetic, so the second
            // is not the overflow check it reads as: a `len` of `usize::MAX`
            // (see [`display_width`]) makes `SIZE_MAX - len - 2` wrap to
            // `SIZE_MAX - 1`, which almost every `pos` is below, while the
            // first half becomes `pos + 1 < line_length`. The pair therefore
            // *passes* for a refused name where a real overflow check would
            // fail it, and the line does not wrap. Translated literally rather
            // than repaired, for the reason given in [`display_width`].
            let fits = cfg.line_length == 0
                || (pos.wrapping_add(len).wrapping_add(2) < cfg.line_length
                    && pos <= usize::MAX.wrapping_sub(len).wrapping_sub(2));
            let separator = if fits {
                pos = pos.wrapping_add(2);
                b' '
            } else {
                pos = 0;
                cfg.eolbyte
            };
            out.buf.push(sep);
            out.buf.push(separator);
        }

        print_file_name_and_frills(out, cfg, w, cwd_some_quoted, f);
        pos = pos.wrapping_add(len);
    }
    out.buf.push(cfg.eolbyte);
}

/// GNU's `print_current_files`: the whole of one directory's listing, in
/// whichever of the five arrangements was asked for.
///
/// `-C` and `-x` fall back to [`print_with_separator`] with a space when
/// `-w0` removed the width they lay out against. `-m` uses it always.
fn print_current_files(
    out: &mut Out,
    cfg: &Config,
    names: &Names,
    times: &mut Times,
    w: &Widths,
    cwd_some_quoted: bool,
    files: &[&FileInfo],
) {
    if files.is_empty() {
        return;
    }
    let lengths: Vec<usize> = files
        .iter()
        .map(|f| length_of_file_name_and_frills(cfg, w, cwd_some_quoted, f))
        .collect();

    match cfg.format {
        Format::OnePerLine => {
            for f in files {
                print_file_name_and_frills(out, cfg, w, cwd_some_quoted, f);
                out.buf.push(cfg.eolbyte);
            }
        }
        Format::ManyPerLine => {
            if cfg.line_length == 0 {
                print_with_separator(out, cfg, w, cwd_some_quoted, files, &lengths, b' ');
            } else {
                print_many_per_line(out, cfg, w, cwd_some_quoted, files, &lengths);
            }
        }
        Format::Horizontal => {
            if cfg.line_length == 0 {
                print_with_separator(out, cfg, w, cwd_some_quoted, files, &lengths, b' ');
            } else {
                print_horizontal(out, cfg, w, cwd_some_quoted, files, &lengths);
            }
        }
        Format::WithCommas => {
            print_with_separator(out, cfg, w, cwd_some_quoted, files, &lengths, b',');
        }
        Format::Long => {
            for f in files {
                print_long_format(out, cfg, names, times, w, cwd_some_quoted, f);
                out.buf.push(cfg.eolbyte);
            }
        }
    }
}

// ------------------------------------------------------ reading the files ---

/// The filesystem, behind a trait so that the walk can be tested without one.
///
/// The methods are named for the calls upstream makes, and they are the only
/// four it makes: `ls` opens no file. `stat_for_mode` is not separate because
/// it is [`Tree::stat`] — upstream distinguishes them only to ask `statx` for
/// fewer fields.
trait Tree {
    fn stat(&self, path: &[u8]) -> std::io::Result<Stat>;
    fn lstat(&self, path: &[u8]) -> std::io::Result<Stat>;
    fn read_link(&self, path: &[u8]) -> std::io::Result<Vec<u8>>;
    /// One directory's entries, in the order the filesystem gives them.
    ///
    /// An **iterator** and not a `Vec`, for two reasons that are both
    /// upstream's. The outer [`Result`] is `opendir` failing and the inner one
    /// is `readdir` failing part-way through, and `ls` prints a different
    /// sentence for each — `cannot open directory` against
    /// `reading directory` — so collapsing them would lose a message. And the
    /// one-name-at-a-time case in [`Listing::print_dir`] exists precisely to
    /// list a directory of millions of entries in constant memory, which a
    /// `Vec` of every entry would defeat.
    fn read_dir<'t>(&'t self, path: &[u8]) -> std::io::Result<DirIter<'t>>;
}

/// What [`Tree::read_dir`] hands back: `readdir` until it stops.
type DirIter<'t> = Box<dyn Iterator<Item = std::io::Result<Entry>> + 't>;

/// One `readdir` result: `(name, type, inode)`.
///
/// The type is `d_type`, which is [`FileType::Unknown`] on a filesystem that
/// does not supply one; the inode is `d_ino`, which is [`NOT_AN_INODE_NUMBER`]
/// when unavailable. Both are what decide whether a `stat` happens at all, so
/// both are carried rather than resolved by the reader.
type Entry = (Vec<u8>, FileType, u64);

/// `NOT_AN_INODE_NUMBER`: the value `ls` treats as "there is no inode here",
/// printed as `?` by [`format_inode`].
///
/// Zero rather than a sentinel because no filesystem uses inode 0, and because
/// a field that starts zeroed is then already "unknown".
const NOT_AN_INODE_NUMBER: u64 = 0;

/// Everything the walk accumulates that is not the output: the files of the
/// directory being read, the column widths they imply, and the two flags that
/// are set once and read everywhere.
#[derive(Default)]
struct Cwd {
    files: Vec<FileInfo>,
    widths: Widths,
    /// GNU's `cwd_some_quoted`: whether *any* name in this directory came out
    /// quoted, which decides whether the unquoted ones are padded to line their
    /// opening quotes up. It is per-directory and is cleared with the files.
    some_quoted: bool,
}

/// The exit status, which `ls` raises but never lowers.
///
/// The two failures are distinct: a file named on the command line that cannot
/// be reached is `2`, the same status as a bad option, while one found inside a
/// directory is `1`. Upstream's `set_exit_status (command_line_arg)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Exit(u8);

impl Exit {
    fn fail(&mut self, command_line_arg: bool) {
        self.0 = self.0.max(if command_line_arg { 2 } else { 1 });
    }
}

/// GNU's `gobble_file`: read one file into the listing, and widen every column
/// it turns out to need.
///
/// Returns the file's block count, which the caller adds to the `total` line.
/// A file that was not stated contributes zero — not because its blocks are
/// zero but because they are unknown, and upstream's `blocks` local is only
/// assigned inside the stat branch.
///
/// Three of its rules are visible in output and none is guessable:
///
/// * **A command-line argument that cannot be stated is dropped**, where one
///   found inside a directory is kept and printed with `?` in every column.
///   `ls nosuch` lists nothing at all; a dangling symlink inside a listed
///   directory still gets a row.
/// * **The inode column is widened outside the stat branch**, so `ls -i`
///   widens it from what `readdir` supplied without stating anything — but a
///   file whose stat *failed* returned before reaching it, so a `?` widens
///   nothing.
/// * **`f.quoted` is measured once for the name and then reused for the link
///   target**, and a link target that needs quoting resets it to "not
///   measured" rather than to "quoted". That keeps the target off
///   `cwd_some_quoted`: a target is not in the name column and cannot affect
///   its alignment.
#[expect(
    clippy::too_many_arguments,
    reason = "upstream's five parameters plus the four pieces of global state this port threads explicitly"
)]
fn gobble_file(
    tree: &dyn Tree,
    cfg: &Config,
    names: &Names,
    cwd: &mut Cwd,
    status: &mut Exit,
    err: &mut dyn Write,
    name: &[u8],
    kind: FileType,
    inode: u64,
    command_line_arg: bool,
    dirname: &[u8],
) -> u64 {
    let mut f = FileInfo {
        name: name.to_vec(),
        filetype: kind,
        stat: Stat {
            ino: inode,
            btime: Ts::UNKNOWN,
            ..Stat::default()
        },
        quoted: None,
        ..FileInfo::default()
    };

    // Only `--quoting-style`s with outer quotes ask this, and only until the
    // first quoted name answers it: once one name is quoted the padding is
    // settled for the whole directory.
    if !cwd.some_quoted && cfg.align_variable_outer_quotes {
        let quoted = needs_quoting(cfg.quoting_style, &cfg.filename_extra, name);
        f.quoted = Some(quoted);
        if quoted {
            cwd.some_quoted = true;
        }
    }

    let mut blocks = 0u64;

    if needs_stat(cfg, kind, inode != NOT_AN_INODE_NUMBER, command_line_arg) {
        // The name to reach the file by, which is the printed name only at the
        // top level.
        let full_name = full_name_for(dirname, name);
        f.full_name.clone_from(&full_name);

        // Which of `stat` and `lstat` runs. `-H` and
        // `--dereference-command-line-symlink-to-dir` both start with `stat`
        // and fall back to `lstat`, but on different conditions.
        //
        // Upstream also keeps a `do_deref` out of this, to tell `getfilecon`
        // which link to label. There is no `getfilecon` here — `-Z` prints `?`
        // for every file — so the flag has no consumer and is not carried.
        let result = match cfg.dereference {
            Deref::Always => tree.stat(&full_name),
            Deref::CommandLineArguments | Deref::CommandLineSymlinkToDir if command_line_arg => {
                let first = tree.stat(&full_name);
                let need_lstat = match &first {
                    // `ENOENT` or `ELOOP` from a *stat* means the link is
                    // broken or circular, and a broken link is still a row.
                    Err(e) => is_enoent_or_eloop(e),
                    Ok(st) => st.mode & S_IFMT != S_IFDIR,
                };
                if cfg.dereference == Deref::CommandLineArguments || !need_lstat {
                    first
                } else {
                    tree.lstat(&full_name)
                }
            }
            _ => tree.lstat(&full_name),
        };

        let stat = match result {
            Ok(stat) => stat,
            Err(error) => {
                let _ = writeln!(
                    err,
                    "ls: cannot access {}: {}",
                    quoteaf(&full_name),
                    strerror(&error)
                );
                status.fail(command_line_arg);
                if command_line_arg {
                    // An operand that cannot be reached leaves no row at all.
                    return 0;
                }
                cwd.files.push(f);
                return 0;
            }
        };
        f.stat = stat;
        f.stat_ok = true;

        if f.stat.mode & S_IFMT == S_IFLNK && (cfg.format == Format::Long || cfg.check_symlink_mode)
        {
            match tree.read_link(&full_name) {
                Ok(target) => f.linkname = Some(target),
                Err(error) => {
                    let _ = writeln!(
                        err,
                        "ls: cannot read symbolic link {}: {}",
                        quoteaf(&full_name),
                        strerror(&error)
                    );
                    status.fail(command_line_arg);
                }
            }

            // "Not measured" and not "quoted": the target takes the slower
            // quoting path without joining `cwd_some_quoted`, because the
            // target is not what the name column aligns.
            if let Some(target) = &f.linkname
                && f.quoted == Some(false)
                && needs_quoting(cfg.quoting_style, &cfg.filename_extra, target)
            {
                f.quoted = None;
            }

            // The target is followed only when something will show what it is:
            // an indicator that distinguishes types, or a sort that groups
            // directories. `-p` is below the threshold and does not follow.
            if f.linkname.is_some()
                && (Indicator::FileType <= cfg.indicator_style || cfg.check_symlink_mode)
                && let Ok(target_stat) = tree.stat(&full_name)
            {
                f.link_ok = true;
                f.link_mode = target_stat.mode;
            }
        }

        // The printed type now comes from the stat and not from `readdir`. A
        // directory named on the command line becomes `arg_directory`, which
        // is what later moves it out of the listing and into its own heading —
        // unless `-d` asked for it to stay.
        f.filetype = if f.stat.mode & S_IFMT == S_IFLNK {
            FileType::SymbolicLink
        } else if f.stat.mode & S_IFMT == S_IFDIR {
            if command_line_arg && !cfg.immediate_dirs {
                FileType::ArgDirectory
            } else {
                FileType::Directory
            }
        } else {
            FileType::Normal
        };

        blocks = f.stat.blocks;
        cwd.widths.observe_stated(cfg, names, &f, blocks);
    }

    cwd.widths.observe_inode(cfg, &f);
    cwd.files.push(f);
    blocks
}

/// Whether an error is one of the two `stat` failures that mean "this may still
/// be a symlink worth showing": the target does not exist, or the chain of
/// links is circular.
///
/// `ErrorKind::FilesystemLoop` is unstable, so `ELOOP` is matched by its raw
/// number. Both are Linux's; SlateOS uses the same values.
fn is_enoent_or_eloop(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound || error.raw_os_error() == Some(40)
}

// -------------------------------------------------------------- the walk ---

/// Everything the listing carries from one directory to the next: upstream's
/// file-scope variables, gathered so that a whole run is one value and a test
/// can drive it against a [`Tree`] that is not a filesystem.
///
/// `first`, `print_dir_name` and `pending` are the three that upstream keeps as
/// globals *and mutates from two different functions*, which is what makes the
/// `dir:` headings come out where they do.
struct Listing<'a> {
    tree: &'a dyn Tree,
    cfg: &'a Config,
    names: &'a Names,
    times: Times,
    out: Out,
    err: &'a mut dyn Write,
    status: Exit,
    /// The directory currently being read. GNU's `cwd_file` and its widths.
    cwd: Cwd,
    /// GNU's `pending_dirs`, a **stack** — see [`extract_dirs_from_files`].
    pending: Vec<Pending>,
    /// GNU's `active_dir_set`: the `(dev, ino)` of every directory on the path
    /// from the operand down to the one being read, so that a symlink pointing
    /// back up is caught rather than followed forever.
    ///
    /// A `Vec` rather than a hash table because it holds one entry per level of
    /// nesting, not one per file — a depth of thirty is a deep tree, and a
    /// linear scan of thirty pairs is faster than hashing one.
    active: Vec<(u64, u64)>,
    /// GNU's `static bool first` inside `print_dir`: whether any heading has
    /// been printed yet, which is what decides that the blank line goes
    /// *before* each heading but not before the first.
    first: bool,
    /// GNU's `print_dir_name`. It starts true, is cleared for the lone
    /// directory of a single-operand run, and is set again after every listed
    /// directory — so `ls dir` prints no heading but `ls dir1 dir2` prints two.
    print_dir_name: bool,
}

impl Listing<'_> {
    /// GNU's `LOOP_DETECT`, which is `!!active_dir_set`, and the set is
    /// allocated exactly when `-R` was asked for.
    const fn loop_detect(&self) -> bool {
        self.cfg.recursive
    }

    /// GNU's `file_failure`: one `ls: <sentence> <name>: <strerror>` line, and
    /// the exit status raised to match where the name came from.
    fn file_failure(
        &mut self,
        command_line_arg: bool,
        sentence: &str,
        name: &[u8],
        e: &std::io::Error,
    ) {
        // A diagnostic that cannot be written has nowhere left to be reported.
        let _ = writeln!(
            self.err,
            "ls: {sentence} {}: {}",
            quoteaf(name),
            strerror(e)
        );
        self.status.fail(command_line_arg);
    }

    /// GNU's `print_dir`: read one directory and print it.
    fn print_dir(&mut self, name: &[u8], realname: Option<&[u8]>, command_line_arg: bool) {
        let mut total_blocks = 0u64;

        let entries = match self.tree.read_dir(name) {
            Ok(entries) => entries,
            Err(e) => {
                self.file_failure(command_line_arg, "cannot open directory", name, &e);
                return;
            }
        };

        if self.loop_detect() {
            // Upstream stats the *open descriptor* and falls back to the path
            // only if `dirfd` failed. There is no descriptor to reach through
            // this trait, so the path is always used; the two differ only if
            // the directory is renamed between the open and the stat, which
            // upstream's own fallback path has the same hole in.
            let dir_stat = match self.tree.stat(name) {
                Ok(stat) => stat,
                Err(e) => {
                    let sentence = "cannot determine device and inode of";
                    self.file_failure(command_line_arg, sentence, name, &e);
                    return;
                }
            };
            let pair = (dir_stat.dev, dir_stat.ino);
            if self.active.contains(&pair) {
                // Not `file_failure`: there is no `errno` to report, so the
                // sentence stands alone and the name is quoted the *other*
                // way — `quotef`, which leaves a plain name unquoted.
                let _ = writeln!(
                    self.err,
                    "ls: {}: not listing already-listed directory",
                    quotef(name)
                );
                self.status.fail(true);
                return;
            }
            self.active.push(pair);
        }

        self.cwd = Cwd::default();

        if self.cfg.recursive || self.print_dir_name {
            if !self.first {
                self.out.buf.push(b'\n');
            }
            self.first = false;
            self.out.indent(self.cfg);
            // The heading quotes under `DIRNAME_EXTRA` rather than the file
            // set, is never measured for the shortcut (upstream's `-1`), and
            // is never padded — `clear_files` has just cleared
            // `cwd_some_quoted`, so there is nothing to align against.
            let rendered = quote_name(
                self.cfg,
                DIRNAME_EXTRA,
                false,
                realname.unwrap_or(name),
                None,
            );
            self.out.mark(self.cfg, Dired::Headers);
            self.out.buf.extend_from_slice(&rendered.bytes);
            self.out.mark(self.cfg, Dired::Headers);
            self.out.buf.extend_from_slice(b":\n");
        }

        // The one-name-at-a-time case: with nothing to sort, no widths to
        // agree on and no recursion, a name can be printed the moment it is
        // read, and a directory of millions of entries costs one row of
        // memory instead of millions. It is observable and not merely an
        // optimisation — the inode column is then padded to each name's own
        // width rather than to the directory's widest. Measured, GNU ls 9.5,
        // on `/dev`:
        //
        // ```text
        // ls -i  /dev  ->  .164 autofs   ..11 console
        // ls -iU /dev  ->  164 autofs    11 console
        // ```
        let one_at_a_time = self.cfg.format == Format::OnePerLine
            && self.cfg.sort == Sort::None
            && !self.cfg.print_block_size
            && !self.cfg.recursive;

        for entry in entries {
            let (child, kind, ino) = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    self.file_failure(command_line_arg, "reading directory", name, &e);
                    break;
                }
            };
            if file_ignored(self.cfg, &child) {
                continue;
            }
            total_blocks = total_blocks.saturating_add(gobble_file(
                self.tree,
                self.cfg,
                self.names,
                &mut self.cwd,
                &mut self.status,
                self.err,
                &child,
                kind,
                ino,
                false,
                name,
            ));
            if one_at_a_time {
                // `sort_files` still runs: even under `--sort=none` it is what
                // establishes the order the printer walks.
                sort_files(self.cfg, self.cwd.some_quoted, &mut self.cwd.files);
                self.print_current_files();
                self.cwd = Cwd::default();
            }
        }

        sort_files(self.cfg, self.cwd.some_quoted, &mut self.cwd.files);

        if self.cfg.recursive {
            extract_dirs_from_files(
                Some(name),
                false,
                self.loop_detect(),
                &mut self.cwd.files,
                &mut self.pending,
            );
        }

        if self.cfg.format == Format::Long || self.cfg.print_block_size {
            let total = human_readable(
                total_blocks,
                self.cfg.human_output_opts,
                ST_NBLOCKSIZE,
                self.cfg.output_block_size,
            );
            self.out.indent(self.cfg);
            self.out.buf.extend_from_slice(b"total ");
            self.out.buf.extend_from_slice(total.as_bytes());
            self.out.buf.push(self.cfg.eolbyte);
        }

        if !self.cwd.files.is_empty() {
            self.print_current_files();
        }
    }

    /// [`print_current_files`] against the directory currently held, which is
    /// the only way it is ever called. The caller has already sorted.
    fn print_current_files(&mut self) {
        let files: Vec<&FileInfo> = self.cwd.files.iter().collect();
        print_current_files(
            &mut self.out,
            self.cfg,
            self.names,
            &mut self.times,
            &self.cwd.widths,
            self.cwd.some_quoted,
            &files,
        );
    }

    /// The tail of GNU's `main`: gobble the operands, then drain the queue.
    fn run(&mut self, operands: &[Vec<u8>]) {
        if operands.is_empty() {
            if self.cfg.immediate_dirs {
                self.gobble_operand(b".", FileType::Directory);
            } else {
                self.pending.push(Pending {
                    name: Some(b".".to_vec()),
                    realname: None,
                    command_line_arg: true,
                });
            }
        } else {
            for operand in operands {
                self.gobble_operand(operand, FileType::Unknown);
            }
        }

        if !self.cwd.files.is_empty() {
            sort_files(self.cfg, self.cwd.some_quoted, &mut self.cwd.files);
            if !self.cfg.immediate_dirs {
                // `None`, so `.` and `..` are operands to be listed rather
                // than the entries that would make a walk cycle.
                extract_dirs_from_files(
                    None,
                    true,
                    self.loop_detect(),
                    &mut self.cwd.files,
                    &mut self.pending,
                );
            }
        }

        if self.cwd.files.is_empty() {
            // The single-directory case, which is the common one: `ls dir`
            // prints the contents with no `dir:` heading, but `ls dir1 dir2`
            // heads both. Upstream tests `pending_dirs->next == 0` — exactly
            // one entry — and not "one operand", because an operand that is a
            // *file* leaves a row behind and takes the branch above.
            if operands.len() <= 1 && self.pending.len() == 1 {
                self.print_dir_name = false;
            }
        } else {
            self.print_current_files();
            if !self.pending.is_empty() {
                self.out.buf.push(b'\n');
            }
        }

        while let Some(next) = self.pending.pop() {
            let Some(name) = next.name else {
                // The marker `extract_dirs_from_files` queued behind a
                // directory's children: that directory is finished, so it
                // stops being on the path and a later link to it is no longer
                // a cycle.
                self.active.pop();
                continue;
            };
            self.print_dir(&name, next.realname.as_deref(), next.command_line_arg);
            self.print_dir_name = true;
        }

        if self.cfg.dired {
            // An *empty* obstack prints no line at all — not an empty one.
            // `ls --dired` of a directory with no files prints only the
            // `//DIRED-OPTIONS//` line.
            let names = std::mem::take(&mut self.out.dired);
            let headers = std::mem::take(&mut self.out.subdired);
            dump_dired(&mut self.out.buf, b"//DIRED//", &names);
            dump_dired(&mut self.out.buf, b"//SUBDIRED//", &headers);
            self.out
                .buf
                .extend_from_slice(b"//DIRED-OPTIONS// --quoting-style=");
            self.out
                .buf
                .extend_from_slice(self.cfg.quoting_style_name.as_bytes());
            self.out.buf.push(b'\n');
        }
    }

    /// One command-line operand, which differs from a directory entry in three
    /// ways: it is stated with no `d_type` and no `d_ino` to save the call, its
    /// failures are status 2, and a directory among them becomes a *heading*
    /// rather than a row.
    fn gobble_operand(&mut self, name: &[u8], kind: FileType) {
        gobble_file(
            self.tree,
            self.cfg,
            self.names,
            &mut self.cwd,
            &mut self.status,
            self.err,
            name,
            kind,
            NOT_AN_INODE_NUMBER,
            true,
            b"",
        );
    }
}

/// GNU's `dired_dump_obstack`: `//DIRED// 12 17 30 34`, or nothing at all when
/// there is nothing to report.
fn dump_dired(out: &mut Vec<u8>, prefix: &[u8], offsets: &[usize]) {
    if offsets.is_empty() {
        return;
    }
    out.extend_from_slice(prefix);
    for offset in offsets {
        out.extend_from_slice(format!(" {offset}").as_bytes());
    }
    out.push(b'\n');
}

// ------------------------------------------------------------------- main ---

/// GNU 9.5's `--help`, verbatim, minus the four-line GNU-project footer that
/// none of these utilities carry (it points at `info` pages this system has
/// none of).
fn help_text() -> String {
    "\
Usage: ls [OPTION]... [FILE]...
List information about the FILEs (the current directory by default).
Sort entries alphabetically if none of -cftuvSUX nor --sort is specified.

Mandatory arguments to long options are mandatory for short options too.
  -a, --all                  do not ignore entries starting with .
  -A, --almost-all           do not list implied . and ..
      --author               with -l, print the author of each file
  -b, --escape               print C-style escapes for nongraphic characters
      --block-size=SIZE      with -l, scale sizes by SIZE when printing them;
                             e.g., '--block-size=M'; see SIZE format below

  -B, --ignore-backups       do not list implied entries ending with ~
  -c                         with -lt: sort by, and show, ctime (time of last
                             change of file status information);
                             with -l: show ctime and sort by name;
                             otherwise: sort by ctime, newest first

  -C                         list entries by columns
      --color[=WHEN]         color the output WHEN; more info below
  -d, --directory            list directories themselves, not their contents
  -D, --dired                generate output designed for Emacs' dired mode
  -f                         do not sort, enable -aU, disable -ls --color
  -F, --classify[=WHEN]      append indicator (one of */=>@|) to entries WHEN
      --file-type            likewise, except do not append '*'
      --format=WORD          across -x, commas -m, horizontal -x, long -l,
                             single-column -1, verbose -l, vertical -C

      --full-time            like -l --time-style=full-iso
  -g                         like -l, but do not list owner
      --group-directories-first
                             group directories before files;
                             can be augmented with a --sort option, but any
                             use of --sort=none (-U) disables grouping

  -G, --no-group             in a long listing, don't print group names
  -h, --human-readable       with -l and -s, print sizes like 1K 234M 2G etc.
      --si                   likewise, but use powers of 1000 not 1024
  -H, --dereference-command-line
                             follow symbolic links listed on the command line
      --dereference-command-line-symlink-to-dir
                             follow each command line symbolic link
                             that points to a directory

      --hide=PATTERN         do not list implied entries matching shell PATTERN
                             (overridden by -a or -A)

      --hyperlink[=WHEN]     hyperlink file names WHEN
      --indicator-style=WORD
                             append indicator with style WORD to entry names:
                             none (default), slash (-p),
                             file-type (--file-type), classify (-F)

  -i, --inode                print the index number of each file
  -I, --ignore=PATTERN       do not list implied entries matching shell PATTERN
  -k, --kibibytes            default to 1024-byte blocks for file system usage;
                             used only with -s and per directory totals

  -l                         use a long listing format
  -L, --dereference          when showing file information for a symbolic
                             link, show information for the file the link
                             references rather than for the link itself

  -m                         fill width with a comma separated list of entries
  -n, --numeric-uid-gid      like -l, but list numeric user and group IDs
  -N, --literal              print entry names without quoting
  -o                         like -l, but do not list group information
  -p, --indicator-style=slash
                             append / indicator to directories
  -q, --hide-control-chars   print ? instead of nongraphic characters
      --show-control-chars   show nongraphic characters as-is (the default,
                             unless program is 'ls' and output is a terminal)

  -Q, --quote-name           enclose entry names in double quotes
      --quoting-style=WORD   use quoting style WORD for entry names:
                             literal, locale, shell, shell-always,
                             shell-escape, shell-escape-always, c, escape
                             (overrides QUOTING_STYLE environment variable)

  -r, --reverse              reverse order while sorting
  -R, --recursive            list subdirectories recursively
  -s, --size                 print the allocated size of each file, in blocks
  -S                         sort by file size, largest first
      --sort=WORD            sort by WORD instead of name: none (-U), size (-S),
                             time (-t), version (-v), extension (-X), width

      --time=WORD            select which timestamp used to display or sort;
                               access time (-u): atime, access, use;
                               metadata change time (-c): ctime, status;
                               modified time (default): mtime, modification;
                               birth time: birth, creation;
                             with -l, WORD determines which time to show;
                             with --sort=time, sort by WORD (newest first)

      --time-style=TIME_STYLE
                             time/date format with -l; see TIME_STYLE below
  -t                         sort by time, newest first; see --time
  -T, --tabsize=COLS         assume tab stops at each COLS instead of 8
  -u                         with -lt: sort by, and show, access time;
                             with -l: show access time and sort by name;
                             otherwise: sort by access time, newest first

  -U                         do not sort; list entries in directory order
  -v                         natural sort of (version) numbers within text
  -w, --width=COLS           set output width to COLS.  0 means no limit
  -x                         list entries by lines instead of by columns
  -X                         sort alphabetically by entry extension
  -Z, --context              print any security context of each file
      --zero                 end each output line with NUL, not newline
  -1                         list one file per line
      --help        display this help and exit
      --version     output version information and exit

The SIZE argument is an integer and optional unit (example: 10K is 10*1024).
Units are K,M,G,T,P,E,Z,Y,R,Q (powers of 1024) or KB,MB,... (powers of 1000).
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.

The TIME_STYLE argument can be full-iso, long-iso, iso, locale, or +FORMAT.
FORMAT is interpreted like in date(1).  If FORMAT is FORMAT1<newline>FORMAT2,
then FORMAT1 applies to non-recent files and FORMAT2 to recent files.
TIME_STYLE prefixed with 'posix-' takes effect only outside the POSIX locale.
Also the TIME_STYLE environment variable sets the default style to use.

The WHEN argument defaults to 'always' and can also be 'auto' or 'never'.

Using color to distinguish file types is disabled both by default and
with --color=never.  With --color=auto, ls emits color codes only when
standard output is connected to a terminal.  The LS_COLORS environment
variable can change the settings.  Use the dircolors(1) command to set it.

Exit status:
 0  if OK,
 1  if minor problems (e.g., cannot access subdirectory),
 2  if serious trouble (e.g., cannot access command-line argument).
"
    .to_string()
}

#[cfg(not(unix))]
fn main() -> ExitCode {
    eprintln!("ls: unix-only utility; not supported on this platform");
    ExitCode::from(2)
}

/// The real filesystem.
#[cfg(unix)]
struct RealTree;

/// Only the real filesystem turns a byte path back into an `OsString`; every
/// other use of a path in this file stays in bytes.
#[cfg(unix)]
use coreutils::quote::os_from_bytes;

#[cfg(unix)]
impl Tree for RealTree {
    fn stat(&self, path: &[u8]) -> std::io::Result<Stat> {
        Ok(stat_of(&std::fs::metadata(os_from_bytes(path))?))
    }

    fn lstat(&self, path: &[u8]) -> std::io::Result<Stat> {
        Ok(stat_of(&std::fs::symlink_metadata(os_from_bytes(path))?))
    }

    fn read_link(&self, path: &[u8]) -> std::io::Result<Vec<u8>> {
        Ok(os_bytes(std::fs::read_link(os_from_bytes(path))?.as_os_str()).into_owned())
    }

    /// `readdir`, plus the two entries `std::fs::read_dir` filters out.
    ///
    /// `.` and `..` are real directory entries and `ls -a` lists them; std
    /// hides them because almost every other caller is walking a tree and
    /// would recurse forever. They are put back at the front — which is a
    /// **choice, and a wrong one on ext4**, because std does not report where
    /// the directory actually returned them and there is nothing left to
    /// recover it from.
    ///
    /// The position is observable under `-U`, `-f` and `--sort=none`, the
    /// three listings whose order is the directory's own. Measured on WSL's
    /// ext4, a raw `readdir(3)` loop over a directory of twelve entries and
    /// GNU ls 9.5 agree exactly, and both disagree with us:
    ///
    /// ```text
    /// readdir(3), and ls -f      ours -f
    /// y.tar.gz                   .
    /// ..                         ..
    /// x                          y.tar.gz
    /// …                          x
    /// .hidden                    …
    /// .                          .hidden
    /// ```
    ///
    /// ext4's hashed directory index puts `.` last here; a different directory
    /// puts it somewhere else again. Under every *sorted* listing — which is
    /// every listing but those three — the dots sort to the front regardless
    /// and the choice is invisible. See `known-issues.md`
    /// `TD-B-LS-INVENTS-A-POSITION-FOR-THE-DOT-ENTRIES`.
    fn read_dir<'t>(&'t self, path: &[u8]) -> std::io::Result<DirIter<'t>> {
        let entries = std::fs::read_dir(os_from_bytes(path))?;
        let dots = [b".".to_vec(), b"..".to_vec()]
            .into_iter()
            .map(|name| Ok((name, FileType::Directory, NOT_AN_INODE_NUMBER)));
        Ok(Box::new(dots.chain(entries.map(|entry| {
            let entry = entry?;
            let name = os_bytes(&entry.file_name()).into_owned();
            // `DirEntry::file_type` falls back to an `lstat` where `readdir`
            // returned `DT_UNKNOWN`, which GNU would instead carry through as
            // `unknown` and let `gobble_file` decide about. The listing is the
            // same either way; the difference is a syscall this port makes on
            // a filesystem — XFS without `ftype`, and some network ones — that
            // does not fill `d_type` in.
            let kind = entry.file_type().map_or(FileType::Unknown, |t| {
                if t.is_symlink() {
                    FileType::SymbolicLink
                } else if t.is_dir() {
                    FileType::Directory
                } else if t.is_file() {
                    FileType::Normal
                } else {
                    dirent_kind(&t)
                }
            });
            // The inode `readdir` reported is deliberately thrown away.
            // Upstream's `RELIABLE_D_INO` reduces to `NOT_AN_INODE_NUMBER`
            // unconditionally — `READDIR_LIES_ABOUT_MOUNTPOINT_D_INO`
            // defaults to 1 and nothing ever clears it — because for an entry
            // that is a mount point `d_ino` is the inode of the *covered*
            // directory, not of what is mounted there. So `ls -i` always
            // stats, and a `d_ino` passed on here would print a number that
            // disagrees with `stat` on exactly the entries a user is most
            // likely to be checking.
            Ok((name, kind, NOT_AN_INODE_NUMBER))
        }))))
    }
}

/// The four file types `std::fs::FileType` only exposes through its Unix
/// extension trait.
#[cfg(unix)]
fn dirent_kind(kind: &std::fs::FileType) -> FileType {
    use std::os::unix::fs::FileTypeExt;
    if kind.is_fifo() {
        FileType::Fifo
    } else if kind.is_char_device() {
        FileType::Chardev
    } else if kind.is_block_device() {
        FileType::Blockdev
    } else if kind.is_socket() {
        FileType::Sock
    } else {
        FileType::Unknown
    }
}

/// `struct stat` as `ls` reads it. The birth time is
/// [`Ts::UNKNOWN`] where the filesystem has none, which is what makes
/// `--time=birth` print `?` rather than the epoch.
#[cfg(unix)]
fn stat_of(meta: &std::fs::Metadata) -> Stat {
    use std::os::unix::fs::MetadataExt;
    let btime = meta.created().map_or(Ts::UNKNOWN, |t| {
        t.duration_since(std::time::UNIX_EPOCH)
            .map_or(Ts::UNKNOWN, |d| Ts {
                sec: i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
                nsec: i64::from(d.subsec_nanos()),
            })
    });
    Stat {
        mode: meta.mode(),
        nlink: meta.nlink(),
        uid: meta.uid(),
        gid: meta.gid(),
        size: meta.size().try_into().unwrap_or(i64::MAX),
        blocks: meta.blocks(),
        rdev: meta.rdev(),
        ino: meta.ino(),
        dev: meta.dev(),
        atime: Ts {
            sec: meta.atime(),
            nsec: meta.atime_nsec(),
        },
        mtime: Ts {
            sec: meta.mtime(),
            nsec: meta.mtime_nsec(),
        },
        ctime: Ts {
            sec: meta.ctime(),
            nsec: meta.ctime_nsec(),
        },
        btime,
    }
}

#[cfg(unix)]
fn main() -> ExitCode {
    use std::io::IsTerminal;

    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let var = |name: &str| std::env::var_os(name).map(|v| os_bytes(&v).into_owned());
    let env = Environment {
        columns: var("COLUMNS"),
        tabsize: var("TABSIZE"),
        quoting_style: var("QUOTING_STYLE"),
        time_style: var("TIME_STYLE"),
        ls_block_size: var("LS_BLOCK_SIZE"),
        block_size: var("BLOCK_SIZE"),
        posixly_correct: std::env::var_os("POSIXLY_CORRECT").is_some(),
        stdout_isatty: std::io::stdout().is_terminal(),
        // `hard_locale (LC_TIME)`: false for exactly `C` and `POSIX`, and the
        // three variables are consulted in the order the C library does.
        hard_locale_time: !matches!(
            var("LC_ALL")
                .or_else(|| var("LC_TIME"))
                .or_else(|| var("LANG"))
                .unwrap_or_default()
                .as_slice(),
            b"" | b"C" | b"POSIX"
        ),
    };

    let mut err = std::io::stderr();
    let request = match parse_args(&argv, &env, &mut err) {
        Ok(request) => request,
        Err(refusal) => {
            refusal.print(&mut err);
            return ExitCode::from(u8::try_from(refusal.status).unwrap_or(2));
        }
    };

    let (cfg, operands) = match request {
        Request::Help => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Request::Version => {
            println!("ls (SlateOS coreutils) 0.1.0");
            return ExitCode::SUCCESS;
        }
        Request::Run(cfg, operands) => (cfg, operands),
    };

    // The two lookups the long format needs. Both are skipped entirely when
    // nothing will ask them: `-n` never resolves an id to a name, and a
    // listing with no time column never resolves a zone.
    let names = Names {
        db: if cfg.numeric_ids || cfg.format != Format::Long {
            pwdb::Db::default()
        } else {
            pwdb::Db::load()
        },
        numeric: cfg.numeric_ids,
    };
    let times = Times::new(&cfg, localtime::Zone::from_env(), system_clock);

    let tree = RealTree;
    let mut listing = Listing {
        tree: &tree,
        cfg: &cfg,
        names: &names,
        times,
        out: Out::default(),
        err: &mut err,
        status: Exit::default(),
        cwd: Cwd::default(),
        pending: Vec::new(),
        active: Vec::new(),
        first: true,
        print_dir_name: true,
    };
    listing.run(&operands);
    let (bytes, status) = (listing.out.buf, listing.status);

    // One write for the whole listing. `--dired` forces the bytes to be held
    // anyway — an offset is only knowable once the text in front of it exists
    // — so there is no arrangement in which this streams, and buffering it
    // deliberately is cheaper than a `BufWriter` that would flush at
    // arbitrary points.
    let mut out = std::io::stdout().lock();
    if out.write_all(&bytes).is_err() || out.flush().is_err() {
        return ExitCode::from(2);
    }
    ExitCode::from(status.0)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "a test that cannot build its own fixture should fail loudly"
)]
mod tests {
    use super::*;
    use modechange::S_IFREG;

    // ------------------------------------------------- names and widths ---

    /// The indicator half of the set is `&"*=>@|"[indicator_style - file_type]`,
    /// so `-F` quotes one character *fewer* than `--file-type` even though it
    /// appends one more. Measured, GNU ls 9.4, on files named `a*b` and `a=b`:
    ///
    /// ```text
    /// ls -b --file-type  ->  a\*b  a\=b
    /// ls -b -F           ->  a*b   a\=b
    /// ```
    #[test]
    fn the_quoted_set_covers_only_indicators_this_style_can_append() {
        assert_eq!(filename_extra(Style::Literal, Indicator::None), b"");
        assert_eq!(filename_extra(Style::Escape, Indicator::None), b" ");
        // `/` is not in either set: a directory's `/` cannot be confused with
        // one in a name, because a name cannot hold one.
        assert_eq!(filename_extra(Style::Literal, Indicator::Slash), b"");
        assert_eq!(
            filename_extra(Style::Literal, Indicator::FileType),
            b"*=>@|"
        );
        assert_eq!(filename_extra(Style::Literal, Indicator::Classify), b"=>@|");
        assert_eq!(filename_extra(Style::Escape, Indicator::Classify), b" =>@|");
        // The header set never grows: a header has nothing appended to it.
        assert_eq!(DIRNAME_EXTRA, b":");
    }

    /// `MBSW_REJECT_INVALID | MBSW_REJECT_UNPRINTABLE` makes this all-or-
    /// nothing: one bad byte anywhere costs the width of the whole name, not
    /// its own width.
    ///
    /// Measured, GNU ls 9.4 — a 25-byte name holding one `\xff` is laid out as
    /// if it were empty, which is how five names fit in forty columns:
    ///
    /// ```text
    /// $ ls -N -C -w 40        # a b c d n<ff>ameXXXXXXXXXXXXXXXXXXXX
    /// a  b  c  d  n<ff>ameXXXXXXXXXXXXXXXXXXXX
    /// ```
    ///
    /// 9.5 refuses the *same* names and then hands the refusal on as
    /// `usize::MAX` rather than as zero, which is what [`display_width`]
    /// reproduces and what
    /// [`a_refused_name_is_laid_out_as_usize_max_columns_wide`] measures.
    #[test]
    fn a_name_with_one_unprintable_byte_has_no_width_at_all() {
        assert_eq!(mbs_width(b""), Some(0));
        assert_eq!(mbs_width(b"plain"), Some(5));
        // Two columns for a wide character, zero for a combining one.
        assert_eq!(mbs_width("\u{4e00}".as_bytes()), Some(2));
        assert_eq!(mbs_width("a\u{0301}".as_bytes()), Some(1));
        // A stray byte, a truncated tail, and a character with no width are
        // all the same answer.
        assert_eq!(mbs_width(b"n\xffame"), None);
        assert_eq!(mbs_width(b"caf\xc3"), None);
        assert_eq!(mbs_width(b"a\x7fb"), None);
        assert_eq!(mbs_width(b"a\tb"), None);
        // …and the caller's `MAX (0, …)` leaves it alone, because by then it
        // is a `size_t`.
        assert_eq!(display_width(b"n\xffame"), usize::MAX);
        assert_eq!(display_width(b"plain"), 5);
    }

    /// Every case measured, GNU ls 9.4 under `LC_ALL=C.UTF-8`:
    ///
    /// ```text
    /// $ ls -q -1
    /// a?b        # a \302\200 b   — a valid but unprintable character
    /// caf?       # caf \303       — a truncated sequence at the end
    /// café       # caf \303\251   — kept, because it prints
    /// del?end    # del \177 end   — DEL is not in the printable-ASCII range
    /// n?ame      # n \377 ame     — one stray byte
    /// two??here  # two \303\303 here — two stray bytes, two marks
    /// x?         # x \360\237\230 — three bytes of a four-byte character
    /// ```
    #[test]
    fn qmark_replaces_characters_not_bytes() {
        let mark = |name: &[u8]| {
            let (bytes, width) = qmark(name);
            (String::from_utf8(bytes).unwrap(), width)
        };
        assert_eq!(mark(b"plain"), ("plain".to_owned(), 5));
        assert_eq!(mark(b"a\xc2\x80b"), ("a?b".to_owned(), 3));
        assert_eq!(mark(b"del\x7fend"), ("del?end".to_owned(), 7));
        assert_eq!(mark(b"n\xffame"), ("n?ame".to_owned(), 5));
        // One `?` per stray byte, because each is skipped one byte at a time.
        assert_eq!(mark(b"two\xc3\xc3here"), ("two??here".to_owned(), 9));
        // But one `?` for a truncated tail however long it is: there is no way
        // to tell how much of the character was meant.
        assert_eq!(mark(b"caf\xc3"), ("caf?".to_owned(), 4));
        assert_eq!(mark(b"x\xf0\x9f\x98"), ("x?".to_owned(), 2));
        // A character that prints keeps its bytes and contributes its width,
        // which is not one per byte in either direction.
        assert_eq!(mark("caf\u{e9}".as_bytes()), ("caf\u{e9}".to_owned(), 4));
        assert_eq!(mark("\u{4e00}".as_bytes()), ("\u{4e00}".to_owned(), 2));
    }

    /// The predicate is "would the style change this name", which is why it
    /// depends on the set: a name is unquoted under one `-F` and quoted under
    /// the other.
    #[test]
    fn needs_quoting_asks_whether_the_style_would_change_the_name() {
        assert!(!needs_quoting(Style::Shell, b"", b"plain"));
        assert!(needs_quoting(Style::Shell, b"", b"a b"));
        assert!(!needs_quoting(Style::Literal, b"", b"a b"));
        // The set is what decides here, not the name: `@` is safe to a shell
        // and so goes unquoted, until `-F` puts it in the set because `-F`
        // appends one to a socket.
        assert!(!needs_quoting(Style::Shell, b"", b"a@b"));
        assert!(needs_quoting(Style::Shell, b"=>@|", b"a@b"));
        // `*` is quoted either way — a shell would glob it — which is why the
        // set is not the only reason a name gets quotes.
        assert!(needs_quoting(Style::Shell, b"", b"a*b"));
    }

    fn rendered(cfg: &Config, some_quoted: bool, name: &[u8]) -> (String, usize, bool) {
        let out = quote_name(cfg, &cfg.filename_extra, some_quoted, name, None);
        (String::from_utf8(out.bytes).unwrap(), out.width, out.pad)
    }

    /// The pad is a property of the *listing*, not of the name: it appears on
    /// unquoted names only once some other name in the same directory has been
    /// quoted, so that the quote marks hang outside the column.
    ///
    /// Measured, GNU ls 9.4, `--quoting-style=shell-escape -C -w 40`:
    ///
    /// ```text
    /// with a quoted name present:   'a b'   plain     # plain starts at col 8
    /// with it removed:              plain              # …and at col 0
    /// ```
    ///
    /// Column width is `max(5, 5+1) = 6` in the first case — the pad is what
    /// makes `plain` six wide — plus the two-space separator.
    #[test]
    fn outer_quotes_are_aligned_by_padding_the_names_that_lack_them() {
        let cfg = Config {
            quoting_style: Style::ShellEscape,
            align_variable_outer_quotes: true,
            ..Config::default()
        };
        assert_eq!(rendered(&cfg, true, b"a b"), ("'a b'".to_owned(), 5, false));
        assert_eq!(
            rendered(&cfg, true, b"plain"),
            ("plain".to_owned(), 5, true)
        );
        // Nothing in this directory was quoted, so nothing is padded.
        assert_eq!(
            rendered(&cfg, false, b"plain"),
            ("plain".to_owned(), 5, false)
        );
        // A style whose quotes are unconditional never pads, because its
        // columns were never uneven.
        let always = Config {
            quoting_style: Style::ShellAlways,
            ..Config::default()
        };
        assert_eq!(
            rendered(&always, true, b"plain"),
            ("'plain'".to_owned(), 7, false)
        );
    }

    /// `needs_general_quoting == Some(false)` is upstream's `f->quoted == 0`:
    /// the name was *measured* not to need quoting, so the rendering is
    /// skipped outright rather than performed and found to be a no-op. The two
    /// must agree, or the shortcut is a bug.
    #[test]
    fn a_name_measured_not_to_need_quoting_skips_the_quoting_pass() {
        let cfg = Config {
            quoting_style: Style::ShellEscape,
            align_variable_outer_quotes: true,
            ..Config::default()
        };
        let skipped = quote_name(&cfg, b"", true, b"plain", Some(false));
        let performed = quote_name(&cfg, b"", true, b"plain", None);
        assert_eq!(skipped.bytes, performed.bytes);
        assert_eq!(skipped.width, performed.width);
        assert_eq!(skipped.pad, performed.pad);
        assert!(skipped.pad);
    }

    /// `-q` runs only under the three styles that escape nothing themselves.
    /// Under the other seven the byte has already become a visible escape, so
    /// there is nothing left for a `?` to replace — and replacing it anyway
    /// would turn `\377` into `?`, losing the one thing the escape recorded.
    #[test]
    fn the_question_mark_pass_runs_only_where_nothing_else_escapes() {
        let literal = Config {
            qmark_funny_chars: true,
            quoting_style: Style::Literal,
            ..Config::default()
        };
        assert_eq!(rendered(&literal, false, b"n\xffame").0, "n?ame");
        let escape = Config {
            qmark_funny_chars: true,
            quoting_style: Style::Escape,
            ..Config::default()
        };
        assert_eq!(rendered(&escape, false, b"n\xffame").0, "n\\377ame");
        // …and with the pass off, the raw byte survives and takes the whole
        // name's width with it — to `usize::MAX`, not to zero, because the
        // clamp meant to catch it is a no-op. See [`display_width`].
        let raw = Config {
            quoting_style: Style::Literal,
            ..Config::default()
        };
        let out = quote_name(&raw, b"", false, b"n\xffame", None);
        assert_eq!(out.bytes, b"n\xffame");
        assert_eq!(out.width, usize::MAX);
    }

    // ------------------------------------------------------------ sorting ---

    /// A plain file with the given name.
    fn file(name: &str) -> FileInfo {
        FileInfo {
            name: name.as_bytes().to_vec(),
            stat_ok: true,
            filetype: FileType::Normal,
            ..FileInfo::default()
        }
    }

    fn dir(name: &str) -> FileInfo {
        FileInfo {
            filetype: FileType::Directory,
            ..file(name)
        }
    }

    /// A symlink whose target was followed and found to be a directory —
    /// the case `--group-directories-first` groups by the *target*.
    fn dirlink(name: &str) -> FileInfo {
        FileInfo {
            filetype: FileType::SymbolicLink,
            link_mode: S_IFDIR | 0o755,
            link_ok: true,
            ..file(name)
        }
    }

    fn order(cfg: &Config, mut files: Vec<FileInfo>) -> Vec<String> {
        sort_files(cfg, false, &mut files);
        files
            .into_iter()
            .map(|f| String::from_utf8(f.name).unwrap())
            .collect()
    }

    /// The fixture every ordering below was measured on, GNU ls 9.4 under
    /// `LC_ALL=C.UTF-8`, with `linkdir` a symlink to `dirA`.
    fn fixture() -> Vec<FileInfo> {
        vec![
            dir("."),
            dir(".."),
            file(".hidden"),
            file("a.txt"),
            file("b.tar.gz"),
            file("c"),
            dir("dirA"),
            dir("dirZ"),
            file("f009"),
            file("f10"),
            file("f2"),
            file("f9"),
            dirlink("linkdir"),
            file("zz"),
        ]
    }

    /// ```text
    /// $ ls -1 -a -X
    /// c dirA dirZ f009 f10 f2 f9 linkdir zz    # no extension at all
    /// . ..                                     # extension "."
    /// b.tar.gz  .hidden  a.txt                 # ".gz" ".hidden" ".txt"
    /// ```
    ///
    /// The two surprises are both `strrchr` doing exactly what it says: `..`
    /// has the extension `.`, and a dot file's extension is its whole name —
    /// so `-X` files `.hidden` under `h`, between `.gz` and `.txt`, and not
    /// with the extensionless names.
    #[test]
    fn extension_order_is_the_last_dot_onwards_including_a_leading_one() {
        let cfg = Config {
            sort: Sort::Extension,
            ..Config::default()
        };
        assert_eq!(
            order(&cfg, fixture()),
            [
                "c", "dirA", "dirZ", "f009", "f10", "f2", "f9", "linkdir", "zz", ".", "..",
                "b.tar.gz", ".hidden", "a.txt",
            ]
        );
    }

    /// ```text
    /// $ ls -1 -a -v
    /// . .. .hidden a.txt b.tar.gz c dirA dirZ f2 f009 f9 f10 linkdir zz
    /// ```
    ///
    /// `f2 f009 f9 f10` is the whole point: `f9` and `f10` are in version
    /// order rather than byte order, and `f009` sits before `f9` even though
    /// `filevercmp` calls them equal — that is the `strcmp` tie-break, without
    /// which the listing would depend on the order the directory was read in.
    #[test]
    fn version_order_falls_back_to_bytes_when_two_names_are_the_same_version() {
        let cfg = Config {
            sort: Sort::Version,
            ..Config::default()
        };
        assert_eq!(
            order(&cfg, fixture()),
            [
                ".", "..", ".hidden", "a.txt", "b.tar.gz", "c", "dirA", "dirZ", "f2", "f009", "f9",
                "f10", "linkdir", "zz",
            ]
        );
    }

    /// ```text
    /// $ ls -1 -a --group-directories-first
    /// . .. dirA dirZ linkdir  .hidden a.txt b.tar.gz c f009 f10 f2 f9 zz
    /// ```
    ///
    /// `linkdir` is a *symlink*, and it groups with the directories because
    /// the group is decided by what the link points at. `.` and `..` are
    /// directories too, which is why `-a --group-directories-first` does not
    /// start with the dot files.
    #[test]
    fn a_symlink_to_a_directory_is_grouped_with_the_directories() {
        let cfg = Config {
            directories_first: true,
            ..Config::default()
        };
        assert_eq!(
            order(&cfg, fixture()),
            [
                ".", "..", "dirA", "dirZ", "linkdir", ".hidden", "a.txt", "b.tar.gz", "c", "f009",
                "f10", "f2", "f9", "zz",
            ]
        );
    }

    /// ```text
    /// $ ls -1 -a -r --group-directories-first
    /// linkdir dirZ dirA .. .  zz f9 f2 f10 f009 c b.tar.gz a.txt .hidden
    /// ```
    ///
    /// `-r` reverses the *key* and not the grouping: the directories are still
    /// first, reversed among themselves. Upstream gets this by handing the
    /// dirs-first wrapper an already-reversed comparator rather than reversing
    /// the whole thing, and so does this.
    #[test]
    fn reversing_the_order_does_not_move_the_directories_to_the_bottom() {
        let cfg = Config {
            directories_first: true,
            sort_reverse: true,
            ..Config::default()
        };
        assert_eq!(
            order(&cfg, fixture()),
            [
                "linkdir", "dirZ", "dirA", "..", ".", "zz", "f9", "f2", "f10", "f009", "c",
                "b.tar.gz", "a.txt", ".hidden",
            ]
        );
    }

    /// `-S` and `-t` run backwards: largest and newest first. That is a
    /// property of the key, so `-Sr` is smallest-first — and a tie in the key
    /// still falls back to the name *forwards* under `-S` and *backwards*
    /// under `-Sr`, because the pair is swapped before the key is asked.
    #[test]
    fn size_and_time_put_the_largest_and_newest_first() {
        let sized = |name: &str, size: i64| FileInfo {
            stat: Stat {
                size,
                ..Stat::default()
            },
            ..file(name)
        };
        let files = || {
            vec![
                sized("a", 10),
                sized("b", 30),
                sized("c", 30),
                sized("d", 20),
            ]
        };
        let cfg = Config {
            sort: Sort::Size,
            ..Config::default()
        };
        assert_eq!(order(&cfg, files()), ["b", "c", "d", "a"]);
        let reversed = Config {
            sort_reverse: true,
            ..cfg
        };
        assert_eq!(order(&reversed, files()), ["a", "d", "c", "b"]);

        let timed = |name: &str, sec: i64, nsec: i64| FileInfo {
            stat: Stat {
                mtime: Ts { sec, nsec },
                ..Stat::default()
            },
            ..file(name)
        };
        // Same second, different nanoseconds — the case `-t` exists for, and
        // the one a seconds-only timestamp would decide by name instead.
        let cfg = Config {
            sort: Sort::Time,
            ..Config::default()
        };
        assert_eq!(
            order(
                &cfg,
                vec![timed("a", 5, 100), timed("b", 5, 900), timed("c", 4, 999)]
            ),
            ["b", "a", "c"]
        );
    }

    /// `-U` is not "some other order" — it is *no* sort at all, so the entries
    /// stay in the order the directory handed them over.
    #[test]
    fn no_sort_leaves_the_directory_order_alone() {
        let cfg = Config {
            sort: Sort::None,
            ..Config::default()
        };
        assert_eq!(order(&cfg, fixture()).first().unwrap(), ".");
        assert_eq!(order(&cfg, fixture()).last().unwrap(), "zz");
        assert_eq!(order(&cfg, vec![file("z"), file("a")]), ["z", "a"]);
    }

    /// `--sort=width` orders by the *screen* width of the rendered name, so a
    /// name holding a two-column character is wider than its character count —
    /// and a name holding an unprintable byte sorts first, even though its
    /// width is [`usize::MAX`].
    ///
    /// That last one is not a contradiction, it is a second truncation on top
    /// of the first: the comparator is `int diff = width (a) - width (b)`, so
    /// `SIZE_MAX - 2` comes back as -3. Measured, GNU ls 9.5:
    ///
    /// ```text
    /// $ ls --sort=width -1     # ab, abc, 一一, a\177bcd
    /// a\177bcd
    /// ab
    /// abc
    /// 一一
    /// ```
    #[test]
    fn width_order_measures_the_rendering_and_not_the_bytes() {
        let cfg = Config {
            sort: Sort::Width,
            ..Config::default()
        };
        let files = vec![
            file("abc"),
            // Five characters, one of them DEL: `mbsnwidth` refuses the whole
            // name, which makes it `usize::MAX` columns wide — and the
            // comparator's narrowing to `int` turns that into "narrowest".
            file("a\u{7f}bcd"),
            file("\u{4e00}\u{4e00}"),
            file("ab"),
        ];
        assert_eq!(
            order(&cfg, files),
            ["a\u{7f}bcd", "ab", "abc", "\u{4e00}\u{4e00}"]
        );
    }

    // --------------------------------------- selecting and naming entries ---

    /// The `--ignore` patterns are matched with `FNM_PERIOD`, so a `*` does not
    /// reach a dot file — which makes `-a -I '*'` print the three dot entries
    /// and nothing else. `--hide` is cancelled by `-a` and `-A`; `--ignore` is
    /// cancelled by neither. Measured, GNU ls 9.4:
    ///
    /// ```text
    /// $ ls -1 -a -I '*'        ->  .  ..  .hidden
    /// $ ls -1 -a --hide='*'    ->  (everything)
    /// $ ls -1 --hide='d*'      ->  everything but dirA and dirZ
    /// $ ls -1 -a -I '?'        ->  everything but c
    /// ```
    #[test]
    fn ignore_reaches_the_dot_files_only_by_naming_the_dot() {
        let survivors = |cfg: &Config| -> Vec<String> {
            fixture()
                .into_iter()
                .filter(|f| !file_ignored(cfg, &f.name))
                .map(|f| String::from_utf8(f.name).unwrap())
                .collect()
        };

        let all_ignore_star = Config {
            ignore_mode: IgnoreMode::Minimal,
            ignore_patterns: vec![b"*".to_vec()],
            ..Config::default()
        };
        assert_eq!(survivors(&all_ignore_star), [".", "..", ".hidden"]);

        let all_hide_star = Config {
            ignore_mode: IgnoreMode::Minimal,
            hide_patterns: vec![b"*".to_vec()],
            ..Config::default()
        };
        assert_eq!(survivors(&all_hide_star).len(), fixture().len());

        let hide_d = Config {
            hide_patterns: vec![b"d*".to_vec()],
            ..Config::default()
        };
        assert!(!survivors(&hide_d).iter().any(|n| n.starts_with('d')));
        // The default mode was already hiding these, so `--hide` is not what
        // removed them and the test above would pass without it.
        assert!(!survivors(&hide_d).contains(&".hidden".to_owned()));

        let all_ignore_one = Config {
            ignore_mode: IgnoreMode::Minimal,
            ignore_patterns: vec![b"?".to_vec()],
            ..Config::default()
        };
        assert!(!survivors(&all_ignore_one).contains(&"c".to_owned()));
        assert!(survivors(&all_ignore_one).contains(&".".to_owned()));
    }

    /// `-A` hides exactly two names, and upstream's
    /// `! name[1 + (name[1] == '.')]` is how it says so. A name that merely
    /// *starts* `..` — `...` or `..a` — is not one of them.
    #[test]
    fn almost_all_hides_the_two_directory_entries_and_no_other_dot_name() {
        let cfg = Config {
            ignore_mode: IgnoreMode::DotAndDotDot,
            ..Config::default()
        };
        assert!(file_ignored(&cfg, b"."));
        assert!(file_ignored(&cfg, b".."));
        assert!(!file_ignored(&cfg, b"..."));
        assert!(!file_ignored(&cfg, b"..a"));
        assert!(!file_ignored(&cfg, b".hidden"));

        let default = Config::default();
        assert!(file_ignored(&default, b"..."));
        assert!(file_ignored(&default, b".hidden"));
        assert!(!file_ignored(&default, b"a.hidden"));

        let all = Config {
            ignore_mode: IgnoreMode::Minimal,
            ..Config::default()
        };
        assert!(!all.hide_patterns.is_empty() || !file_ignored(&all, b"."));
    }

    /// The two joins are different functions and the difference shows.
    /// `attach` builds what `stat` is given and drops a `.`; `file_name_concat`
    /// builds what a recursive listing prints and keeps it. Measured:
    ///
    /// ```text
    /// $ ls -R .        ->  headings  .:  then  ./dirA
    /// $ ls -R 'dirA//' ->  headings  dirA//:  then  dirA/sub1
    /// ```
    #[test]
    fn the_path_that_is_stated_and_the_path_that_is_printed_are_joined_differently() {
        assert_eq!(attach(b".", b"x"), b"x");
        assert_eq!(file_name_concat(b".", b"x"), b"./x");

        assert_eq!(attach(b"dir", b"x"), b"dir/x");
        assert_eq!(attach(b"dir/", b"x"), b"dir/x");
        assert_eq!(attach(b"/", b"x"), b"/x");

        // The trailing run collapses because the dir is truncated to the end
        // of its last component.
        assert_eq!(file_name_concat(b"dirA//", b"sub1"), b"dirA/sub1");
        assert_eq!(file_name_concat(b"./dirA/", b"sub1"), b"./dirA/sub1");
        assert_eq!(file_name_concat(b"/", b"bin"), b"/bin");
        // gnulib joins a root to an absolute base with `.`, because `//foo`
        // is a different file from `/foo` on some POSIX systems.
        assert_eq!(file_name_concat(b"/", b"/foo"), b"/./foo");

        // An absolute entry name is used as it stands, whatever it is under.
        assert_eq!(full_name_for(b"dir", b"/abs"), b"/abs");
        assert_eq!(full_name_for(b"", b"rel"), b"rel");
        assert_eq!(full_name_for(b"dir", b"rel"), b"dir/rel");
    }

    /// A plain `ls` makes no `stat` calls at all, and the options that force
    /// one are exactly the ones that need more than `readdir` supplies. The
    /// clean way to see it is a *dangling* symlink, which only a stat can fail
    /// on. Measured, GNU ls 9.4, on a directory holding one:
    ///
    /// ```text
    /// ls -L   rc=0  (no error)          -- nothing wants the target
    /// ls -LF  rc=1  cannot access ...   -- -F needs the target's type
    /// ls -Li  rc=1  cannot access ...   -- -L moves the inode to the target
    /// ls -F   rc=0  (no error)          -- readdir already said "symlink"
    /// ls -i   rc=0  (no error)          -- readdir already gave the inode
    /// ls -l   rc=0  (no error)          -- lstat, and lstat succeeds
    /// ```
    #[test]
    fn nothing_is_stated_that_readdir_already_answered() {
        let link = FileType::SymbolicLink;
        let plain = Config::default();
        assert!(!needs_stat(&plain, link, true, false));

        // -L on its own: the dereference is real, but no output field asks
        // for anything the link itself cannot answer.
        let deref = Config {
            dereference: Deref::Always,
            ..Config::default()
        };
        assert!(!needs_stat(&deref, link, true, false));

        // -LF: `format_needs_type` plus the dereference.
        let deref_classify = Config {
            format_needs_type: true,
            indicator_style: Indicator::Classify,
            ..deref.clone()
        };
        assert!(needs_stat(&deref_classify, link, true, false));

        // -Li: the same clause, reached through the inode instead.
        let deref_inode = Config {
            print_inode: true,
            ..deref
        };
        assert!(needs_stat(&deref_inode, link, true, false));

        // -F without -L: readdir already said "symlink", and that is the
        // whole answer `@` needs.
        let classify = Config {
            format_needs_type: true,
            indicator_style: Indicator::Classify,
            ..Config::default()
        };
        assert!(!needs_stat(&classify, link, true, false));
        // But a *regular* file under -F needs its mode, to know whether to
        // mark it `*`.
        assert!(needs_stat(&classify, FileType::Normal, true, false));
        // And a filesystem whose readdir gives no type forces one regardless.
        assert!(needs_stat(&classify, FileType::Unknown, true, false));

        // -i without -L: the inode readdir gave is the one to print.
        let inode = Config {
            print_inode: true,
            ..Config::default()
        };
        assert!(!needs_stat(&inode, link, true, false));
        assert!(needs_stat(&inode, link, false, false));

        // -l stats, and the stat is an lstat, which is why it succeeds on a
        // dangling link.
        let long = Config {
            format: Format::Long,
            format_needs_stat: true,
            ..Config::default()
        };
        assert!(needs_stat(&long, link, true, false));

        // A command-line operand is always stated: it is how `ls nope` learns
        // there is nothing there to list.
        assert!(needs_stat(&plain, link, true, true));
    }

    /// `extract_dirs_from_files` queues the directories in listing order and
    /// removes only the ones that were named on the command line. Measured:
    ///
    /// ```text
    /// $ ls a.txt dirA   ->  a.txt  (blank)  dirA:  sub1 sub2 x
    /// $ ls -R .         ->  .:  ... dirA ...  (blank)  ./dirA:  ...
    /// ```
    ///
    /// The first shows the removal — `dirA` is not listed beside `a.txt`. The
    /// second shows that a directory found by `-R` is *not* removed: `dirA` is
    /// both a row under `.` and a heading of its own.
    #[test]
    fn only_a_directory_named_on_the_command_line_leaves_the_listing() {
        let mut files = vec![
            file("a.txt"),
            FileInfo {
                filetype: FileType::ArgDirectory,
                ..file("dirA")
            },
        ];
        let mut pending = Vec::new();
        extract_dirs_from_files(None, true, false, &mut files, &mut pending);
        assert_eq!(files.len(), 1);
        assert_eq!(files.first().unwrap().name, b"a.txt");
        assert_eq!(queue_order(&pending), [Some(&b"dirA"[..])]);

        let mut files = vec![file("x"), dir("dirA"), dir("dirZ")];
        let mut pending = Vec::new();
        extract_dirs_from_files(Some(b"."), false, false, &mut files, &mut pending);
        assert_eq!(files.len(), 3, "-R leaves the subdirectory in the listing");
        assert_eq!(
            queue_order(&pending),
            [Some(&b"./dirA"[..]), Some(&b"./dirZ"[..])]
        );
    }

    /// The order the driver will take entries out of the queue. `pending` is a
    /// stack, so that is the reverse of the order they went in.
    fn queue_order(pending: &[Pending]) -> Vec<Option<&[u8]>> {
        pending.iter().rev().map(|p| p.name.as_deref()).collect()
    }

    /// Inside a recursive listing `.` and `..` are the entries that would make
    /// the walk cycle, and they are dropped; on the command line they are
    /// operands the user asked for, and they are not.
    #[test]
    fn dot_and_dotdot_are_operands_at_the_top_and_cycles_below_it() {
        let subdirs = || vec![dir("."), dir(".."), dir("sub"), dir("a/..")];

        let mut files = subdirs();
        let mut pending = Vec::new();
        extract_dirs_from_files(Some(b"top"), false, false, &mut files, &mut pending);
        assert_eq!(
            queue_order(&pending),
            [Some(&b"top/sub"[..])],
            "a/.. ends in .. and is a cycle too"
        );

        let mut files = subdirs();
        let mut pending = Vec::new();
        extract_dirs_from_files(None, true, false, &mut files, &mut pending);
        assert_eq!(pending.len(), 4);
    }

    /// `-R` puts a marker in the queue *behind* a directory's children, so the
    /// loop that reads the queue knows when the directory is finished and can
    /// stop guarding against a cycle through it. It carries no name, which is
    /// how it is told apart from a directory to open.
    #[test]
    fn recursion_marks_the_end_of_a_directory_in_the_queue_itself() {
        let mut files = vec![dir("sub"), dir("tub")];
        let mut pending = Vec::new();
        extract_dirs_from_files(Some(b"top"), false, true, &mut files, &mut pending);
        assert_eq!(
            queue_order(&pending),
            [Some(&b"top/sub"[..]), Some(&b"top/tub"[..]), None],
            "both children come out before the marker that ends their parent"
        );
        assert_eq!(
            pending.first().unwrap().realname.as_deref(),
            Some(&b"top"[..])
        );

        // Without `-R` there is no cycle to detect and no marker.
        let mut files = vec![dir("sub")];
        let mut pending = Vec::new();
        extract_dirs_from_files(Some(b"top"), false, false, &mut files, &mut pending);
        assert_eq!(pending.len(), 1);
    }

    // ------------------------------------------ the columns of a long line ---

    fn no_names() -> Names {
        Names {
            db: pwdb::Db::from_bytes(b"", b""),
            numeric: false,
        }
    }

    fn named(passwd: &[u8], group: &[u8]) -> Names {
        Names {
            db: pwdb::Db::from_bytes(passwd, group),
            numeric: false,
        }
    }

    /// Linux's `makedev`, so a test can name a device the way `ls` prints it.
    const fn makedev(major: u64, minor: u64) -> u64 {
        ((major & 0xfff) << 8) | (minor & 0xff) | ((major & !0xfff) << 32) | ((minor & !0xff) << 12)
    }

    /// Every width in 9.5 is measured strictly, and text that refuses to be
    /// measured is *skipped* rather than guessed at: it widens no column and
    /// it is padded to no column. This is the whole of the 9.4 → 9.5 change
    /// recorded in `design-decisions.md` §366, so it gets a test of its own.
    ///
    /// Measured against a group literally named `g\002bad`, with `od -c` so
    /// that the space counts are byte-exact:
    ///
    /// ```text
    /// 9.4  ... root  sp  g 002   b   a   d  sp  sp  sp  sp   0 ...
    /// 9.5  ... root  sp  g 002   b   a   d  sp   0 ...
    /// ```
    ///
    /// 9.4 measured the name as four columns and padded it out to the
    /// column's seven; 9.5 measures nothing and pads nothing, so the size
    /// that follows moves left. Neither is "right" — but only one can be
    /// reproduced, and both binaries were on hand to choose between.
    #[test]
    fn a_width_that_cannot_be_measured_pads_nothing_rather_than_everything() {
        // The four ways a width goes unmeasured, all of them refusals now.
        assert_eq!(mbs_width(b"ab"), Some(2));
        assert_eq!(mbs_width(b"a\xffb"), None, "a byte no character starts");
        assert_eq!(mbs_width(b"a\xe4\xb8"), None, "a sequence cut short");
        assert_eq!(mbs_width(b"a\x7fb"), None, "a character with no glyph");
        assert_eq!(mbs_width("\u{4e00}".as_bytes()), Some(2), "and a wide one");

        // A measurable name is padded out to the column, as always.
        let mut out = Vec::new();
        format_user_or_group(&mut out, Some(b"root"), 0, 7);
        assert_eq!(
            out, b"root    ",
            "four columns, three of padding, separator"
        );

        // An unmeasurable one is not padded at all — just the separator.
        let mut out = Vec::new();
        format_user_or_group(&mut out, Some(b"g\x02bad"), 0, 7);
        assert_eq!(out, b"g\x02bad ");

        // And it contributed nothing to the width that column was computed
        // from in the first place, so it cannot have been the file that made
        // the column seven wide.
        assert_eq!(format_user_or_group_width(Some(b"g\x02bad"), 0), None);
        assert_eq!(format_user_or_group_width(Some(b"root"), 0), Some(4));

        // A *number* is not text and cannot refuse, so `-n` is unaffected.
        assert_eq!(format_user_or_group_width(None, 104), Some(3));
    }

    /// A name is padded on the right and a number on the left, so `-n` does not
    /// merely swap the text in the column — it swaps the alignment too.
    /// Measured, GNU ls 9.4:
    ///
    /// ```text
    /// $ ls -l  /var/lib  ->  drwxr-xr-x 3 root      root      4096 ...
    ///                        drwxr-xr-x 3 landscape landscape 4096 ...
    /// $ ls -ln /var/lib  ->  drwxr-xr-x 3   0   0            4096 ...
    ///                        drwxr-xr-x 3 104 105            4096 ...
    /// ```
    #[test]
    fn a_name_is_padded_on_the_right_and_an_id_on_the_left() {
        let mut out = Vec::new();
        format_user_or_group(&mut out, Some(b"root"), 0, 9);
        assert_eq!(out, b"root      ");
        assert_eq!(out.len(), 10, "nine columns plus the field separator");

        let mut out = Vec::new();
        format_user_or_group(&mut out, Some(b"landscape"), 104, 9);
        assert_eq!(out, b"landscape ");

        let mut out = Vec::new();
        format_user_or_group(&mut out, None, 0, 3);
        assert_eq!(out, b"  0 ");

        let mut out = Vec::new();
        format_user_or_group(&mut out, None, 104, 3);
        assert_eq!(out, b"104 ");

        // A name wider than the column is not truncated; it still gets its
        // separator, and the row it is on is simply longer than the others.
        let mut out = Vec::new();
        format_user_or_group(&mut out, Some(b"averylongname"), 0, 4);
        assert_eq!(out, b"averylongname ");
    }

    /// `-n` turns the lookups off, and so does an id with no entry in the
    /// file — the two are the same code path, which is why one unknown owner
    /// in a directory prints right-aligned among left-aligned names.
    #[test]
    fn an_id_with_no_entry_prints_exactly_as_dash_n_would() {
        let db = named(b"root:x:0:0:::\n", b"disk:x:6:\n");
        assert_eq!(db.user(0), Some(&b"root"[..]));
        assert_eq!(db.group(6), Some(&b"disk"[..]));
        assert_eq!(db.user(4000), None);
        assert_eq!(db.group(4001), None);

        let numeric = Names {
            db: pwdb::Db::from_bytes(b"root:x:0:0:::\n", b"disk:x:6:\n"),
            numeric: true,
        };
        assert_eq!(numeric.user(0), None);
        assert_eq!(numeric.group(6), None);

        assert_eq!(format_user_or_group_width(Some(b"root"), 0), Some(4));
        assert_eq!(format_user_or_group_width(None, 4000), Some(4));
        assert_eq!(format_user_or_group_width(None, 0), Some(1));
    }

    /// The size column has to hold both a size and a `major, minor`, so one
    /// device in a directory widens every regular file's size column.
    /// Measured, GNU ls 9.4, on `/dev` — where the widest major is `229` and
    /// the widest minor is `235`, giving `3 + 2 + 3 = 8`, and the widest plain
    /// size is `4096`:
    ///
    /// ```text
    /// crw-r--r-- 1 root root     10, 235 Aug 20 19:35 autofs
    /// drwxr-xr-x 2 root root        2940 Aug 20 19:38 char
    ///                          |-- 8 --|
    /// ```
    ///
    /// (The wider-looking gap in that listing is the *group* column, which
    /// `/dev` widens to 7 for `dialout`.)
    #[test]
    fn one_device_widens_the_size_column_for_every_file_beside_it() {
        let cfg = Config {
            format: Format::Long,
            ..Config::default()
        };
        let names = no_names();

        let device = |major: u64, minor: u64| FileInfo {
            stat_ok: true,
            stat: Stat {
                mode: S_IFCHR | 0o644,
                nlink: 1,
                rdev: makedev(major, minor),
                ..Stat::default()
            },
            ..file("dev")
        };
        let plain = |size: i64| FileInfo {
            stat_ok: true,
            stat: Stat {
                mode: S_IFREG | 0o644,
                nlink: 1,
                size,
                ..Stat::default()
            },
            ..file("f")
        };

        let mut w = Widths::default();
        w.observe_stated(&cfg, &names, &plain(4096), 8);
        assert_eq!(w.file_size, 4);
        w.observe_stated(&cfg, &names, &device(229, 0), 0);
        assert_eq!(w.major_device, 3);
        assert_eq!(w.minor_device, 1);
        w.observe_stated(&cfg, &names, &device(10, 235), 0);
        assert_eq!(w.minor_device, 3);
        assert_eq!(w.file_size, 8, "3 + 2 + 3, wider than any plain size here");

        // The encoding round-trips, which is the only reason the widths above
        // mean anything.
        assert_eq!(major_of(makedev(229, 0)), 229);
        assert_eq!(minor_of(makedev(10, 235)), 235);
        // Linux splits both numbers across the word, so a large minor is not
        // simply the low byte.
        assert_eq!(major_of(makedev(4095, 1048575)), 4095);
        assert_eq!(minor_of(makedev(4095, 1048575)), 1048575);
    }

    /// The maxima are per directory and each is the widest value *seen so
    /// far*, which is why `ls -l` cannot print a single row until it has read
    /// the whole directory — and why `ls -lR` can give two directories
    /// different column widths.
    #[test]
    fn every_column_is_as_wide_as_the_widest_thing_in_this_directory_alone() {
        let cfg = Config {
            format: Format::Long,
            print_inode: true,
            print_scontext: true,
            print_author: true,
            ..Config::default()
        };
        let names = named(b"root:x:0:0:::\nlandscape:x:104:105:::\n", b"disk:x:6:\n");

        let entry = |nlink: u64, uid: u32, gid: u32, size: i64, ino: u64| FileInfo {
            stat_ok: true,
            stat: Stat {
                mode: S_IFREG | 0o644,
                nlink,
                uid,
                gid,
                size,
                ino,
                ..Stat::default()
            },
            ..file("f")
        };

        let mut w = Widths::default();
        let a = entry(1, 0, 6, 7, 12);
        w.observe_stated(&cfg, &names, &a, 8);
        w.observe_inode(&cfg, &a);
        assert_eq!(
            w,
            Widths {
                inode: 2,
                block_size: 1,
                nlink: 1,
                owner: 4,
                group: 4,
                author: 4,
                scontext: 1,
                file_size: 1,
                ..Widths::default()
            }
        );

        let b = entry(1234, 104, 4001, 999_999, 3);
        w.observe_stated(&cfg, &names, &b, 2048);
        w.observe_inode(&cfg, &b);
        assert_eq!(w.nlink, 4);
        assert_eq!(w.owner, 9, "landscape");
        assert_eq!(w.group, 4, "gid 4001 has no entry, so its four digits");
        assert_eq!(w.file_size, 6);
        // 2048 blocks of 512 bytes, reported in the default 1 KiB unit.
        assert_eq!(w.block_size, 4);
        assert_eq!(w.inode, 2, "the narrower inode did not shrink the column");
    }

    /// `-i` widens its column from the inode `readdir` supplied, with no stat
    /// at all — but a file whose stat *failed* prints `?` and does not widen
    /// it, because upstream returns before the width is observed.
    ///
    /// There are two ways to have no inode and they arrive by different
    /// routes: a failed stat, and an inode of *zero*, which is
    /// `NOT_AN_INODE_NUMBER` — the value `gobble_file` is handed for a
    /// command-line argument and the value `D_INO` expands to where `struct
    /// dirent` carries no `d_ino`. Both print `?`.
    #[test]
    fn a_file_with_no_inode_prints_a_question_mark_and_widens_nothing() {
        let cfg = Config {
            print_inode: true,
            ..Config::default()
        };
        let known = FileInfo {
            stat_ok: true,
            stat: Stat {
                ino: 636_031,
                ..Stat::default()
            },
            ..file("a")
        };
        assert_eq!(format_inode(&known), b"636031");

        let failed = FileInfo {
            stat_ok: false,
            ..file("b")
        };
        assert_eq!(format_inode(&failed), b"?");

        let marker = FileInfo {
            stat_ok: true,
            stat: Stat {
                ino: 0,
                ..Stat::default()
            },
            ..file("c")
        };
        assert_eq!(format_inode(&marker), b"?", "zero is the no-inode marker");

        let mut w = Widths::default();
        w.observe_inode(&cfg, &known);
        assert_eq!(w.inode, 6);

        // The width is taken from the raw `st_ino` and not from what
        // `format_inode` would print (upstream line 3690 measures
        // `umaxtostr (f->stat.st_ino, buf)` directly), so the marker widens
        // the column as the string `0` rather than as the `?` it prints.
        // Both are one column, so the asymmetry never shows — but it is the
        // reason this stays a measurement of the number.
        let mut w = Widths::default();
        w.observe_inode(&cfg, &marker);
        assert_eq!(w.inode, 1);

        // And nothing is observed at all when `-i` was not asked for.
        let mut w = Widths::default();
        w.observe_inode(&Config::default(), &known);
        assert_eq!(w.inode, 0);
    }

    /// POSIX requires a size to print without a sign, so a negative `st_size`
    /// is read as the positive one that wrapped rather than clamped to zero.
    #[test]
    fn a_size_that_wrapped_prints_as_the_number_it_wrapped_from() {
        assert_eq!(unsigned_file_size(0), 0);
        assert_eq!(unsigned_file_size(4096), 4096);
        assert_eq!(unsigned_file_size(-1), u64::MAX);
        assert_eq!(unsigned_file_size(i64::MIN), 1 << 63);
    }

    // ------------------------------------------------------ the time column ---

    /// 2026-08-23 07:43:05 UTC — the instant the GNU measurements below were
    /// taken against, frozen so that the six-month window does not move.
    const FROZEN: i64 = 1_787_470_985;

    fn frozen_clock() -> Ts {
        Ts {
            sec: FROZEN,
            nsec: 0,
        }
    }

    fn times(cfg: &Config) -> Times {
        Times::new(cfg, localtime::Zone::utc(), frozen_clock)
    }

    fn time_field(cfg: &Config, stat_ok: bool, when: Ts) -> String {
        let mut out = Vec::new();
        times(cfg).format(&mut out, cfg, stat_ok, when);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn at(sec: i64) -> Ts {
        Ts { sec, nsec: 0 }
    }

    /// Measured, `TZ=UTC`, against files stamped 60 s inside and 60 s outside
    /// the window, plus one 60 s in the future:
    ///
    /// ```text
    /// $ ls -l --time-style="+$(printf 'OLD\nNEW')" | awk '{print $6, $7}'
    /// OLD future     NEW inside     NEW justnow     OLD outside
    /// ```
    ///
    /// A file in the future is *not* recent, which is the second half of
    /// upstream's `when < current_time` and is easy to lose.
    #[test]
    fn a_file_is_recent_only_inside_the_six_months_behind_the_clock() {
        let cfg = Config::default();
        let mut t = times(&cfg);

        assert!(t.is_recent(at(FROZEN - SIX_MONTHS + 60)));
        assert!(!t.is_recent(at(FROZEN - SIX_MONTHS - 60)));
        assert!(t.is_recent(at(FROZEN - 1)));
        assert!(!t.is_recent(at(FROZEN + 60)));

        // Both ends are strict: the boundary second itself is not recent, and
        // neither is a file stamped exactly now.
        assert!(!t.is_recent(at(FROZEN - SIX_MONTHS)));
        assert!(!t.is_recent(at(FROZEN)));
    }

    /// The clock is read once for the first file — `current_time` starts below
    /// every possible timestamp — and then again only for a file that appears
    /// to be in the future, because such a file is more likely evidence of a
    /// stale clock than of time travel.
    #[test]
    fn the_clock_is_read_again_only_for_a_file_that_looks_like_the_future() {
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
        static READS: AtomicUsize = AtomicUsize::new(0);
        fn counting_clock() -> Ts {
            READS.fetch_add(1, AtomicOrdering::Relaxed);
            Ts {
                sec: FROZEN,
                nsec: 0,
            }
        }

        READS.store(0, AtomicOrdering::Relaxed);
        let cfg = Config::default();
        let mut t = Times::new(&cfg, localtime::Zone::utc(), counting_clock);
        assert_eq!(READS.load(AtomicOrdering::Relaxed), 0);

        // The first file always reads it, whatever its timestamp.
        t.is_recent(at(FROZEN - 1_000_000));
        assert_eq!(READS.load(AtomicOrdering::Relaxed), 1);

        // Every subsequent file in the past reads nothing.
        t.is_recent(at(FROZEN - 2_000_000));
        t.is_recent(at(0));
        assert_eq!(READS.load(AtomicOrdering::Relaxed), 1);

        // One in the future reads it again — and, since the clock has not
        // actually moved, keeps reading it for every later future file.
        t.is_recent(at(FROZEN + 60));
        assert_eq!(READS.load(AtomicOrdering::Relaxed), 2);
    }

    /// ```text
    /// $ TZ=UTC ls -l
    /// -rw-r--r-- 1 u u 0 Jan  1  1970 epoch
    /// -rw-r--r-- 1 u u 0 Jan  1  2038 future
    /// -rw-r--r-- 1 u u 0 Aug  1 12:00 recent
    /// ```
    #[test]
    fn a_recent_file_shows_a_clock_and_an_old_one_shows_a_year() {
        let cfg = Config::default();
        assert_eq!(time_field(&cfg, true, at(0)), "Jan  1  1970 ");
        // 2026-08-01 12:00:00 UTC, three weeks before the frozen clock.
        assert_eq!(time_field(&cfg, true, at(1_785_585_600)), "Aug  1 12:00 ");
        // 2038-01-01, in the future and therefore not recent.
        assert_eq!(time_field(&cfg, true, at(2_145_916_800)), "Jan  1  2038 ");
    }

    /// The `?` is right-aligned in the width of the *non-recent* format
    /// rendered at the epoch, and nothing else is padded to that width.
    ///
    /// ```text
    /// $ TZ=UTC ls -lL .                          # `broken` is a dangling symlink
    /// l????????? ? ?      ?      ?            ? broken
    /// $ TZ=UTC ls -lL --time-style=full-iso .
    /// l????????? ? ?      ?      ?                                   ? broken
    /// $ TZ=UTC ls -lL --time-style=+%Y .
    /// l????????? ? ?      ?      ?    ? broken
    /// $ TZ=UTC ls -lL --time-style=+ .
    /// l????????? ? ?      ?      ? ? broken
    /// ```
    #[test]
    fn a_file_with_no_timestamp_prints_a_question_mark_in_the_epochs_width() {
        let with = |non_recent: &[u8]| Config {
            long_time_format: [non_recent.to_vec(), b"%b %e %H:%M".to_vec()],
            ..Config::default()
        };

        // "Jan  1  1970" — twelve columns.
        let cfg = Config::default();
        assert_eq!(times(&cfg).fallback_width, 12);
        assert_eq!(time_field(&cfg, false, at(0)), "           ? ");

        // "1970-01-01 00:00:00.000000000 +0000" — thirty-five.
        let cfg = with(b"%Y-%m-%d %H:%M:%S.%N %z");
        assert_eq!(times(&cfg).fallback_width, 35);
        assert_eq!(time_field(&cfg, false, at(0)).len(), 36);

        let cfg = with(b"%Y");
        assert_eq!(times(&cfg).fallback_width, 4);
        assert_eq!(time_field(&cfg, false, at(0)), "   ? ");

        // A width of zero pads nothing at all: `%0s` prints just the `?`.
        let cfg = with(b"");
        assert_eq!(times(&cfg).fallback_width, 0);
        assert_eq!(time_field(&cfg, false, at(0)), "? ");
    }

    /// An empty format is not a missing timestamp: it prints the separating
    /// space and no `?`. Upstream tells the two apart by the sentinel byte it
    /// left in the buffer, because `nstrftime` returns 0 for both.
    ///
    /// ```text
    /// $ ls -l --time-style=+
    /// -rw-r--r-- 1 u u 0  epoch
    /// ```
    #[test]
    fn an_empty_time_format_prints_a_space_and_not_a_question_mark() {
        let cfg = Config {
            long_time_format: [Vec::new(), Vec::new()],
            ..Config::default()
        };
        assert_eq!(time_field(&cfg, true, at(0)), " ");
        // …whereas a file with no timestamp still gets its `?`, now unpadded.
        assert_eq!(time_field(&cfg, false, at(0)), "? ");
    }

    /// `--time=birth` on a filesystem with no birth time reaches the same `?`
    /// as a failed `stat`, by way of the `(-1, -1)` sentinel.
    #[test]
    fn a_birth_time_the_filesystem_does_not_have_prints_the_same_question_mark() {
        let cfg = Config::default();
        assert_eq!(time_field(&cfg, true, Ts::UNKNOWN), "           ? ");
        assert_eq!(time_field(&cfg, false, at(0)), "           ? ");
    }

    /// A rendered timestamp is never padded, so two formats of different
    /// widths misalign — which is GNU's behaviour and not a defect here.
    ///
    /// ```text
    /// $ ls -l --time-style=+"$(printf '%Y\n%Y-%m-%d')"
    /// -rw-r--r-- 1 u u 0 2026       future
    /// -rw-r--r-- 1 u u 0 2026-02-21 inside
    /// ```
    #[test]
    fn a_rendered_timestamp_is_never_padded_to_the_other_formats_width() {
        let cfg = Config {
            long_time_format: [b"%Y".to_vec(), b"%Y-%m-%d".to_vec()],
            ..Config::default()
        };
        assert_eq!(time_field(&cfg, true, at(FROZEN + 60)), "2026 ");
        assert_eq!(time_field(&cfg, true, at(FROZEN - 60)), "2026-08-23 ");
    }

    /// The zone is the one `ls` resolved, not UTC — the same instant reads
    /// differently on either side of the dateline, and the column shows it.
    #[test]
    fn the_timestamp_is_rendered_in_the_zone_ls_resolved() {
        let cfg = Config::default();
        let mut out = Vec::new();
        let zone = localtime::Zone::resolve(
            Some(b"EST5EDT,M3.2.0,M11.1.0"),
            "",
            std::path::Path::new(""),
        );
        Times::new(&cfg, zone, frozen_clock).format(&mut out, &cfg, true, at(0));
        // The epoch was 19:00 on New Year's Eve in New York.
        assert_eq!(String::from_utf8_lossy(&out), "Dec 31  1969 ");
    }

    // --------------------------------------------------------- the long line ---

    /// `--time-style=long-iso`, which gives the two formats the same fixed
    /// sixteen columns and so lets a test assert a whole line without knowing
    /// when it ran.
    fn long_cfg() -> Config {
        Config {
            format: Format::Long,
            long_time_format: [b"%Y-%m-%d %H:%M".to_vec(), b"%Y-%m-%d %H:%M".to_vec()],
            ..Config::default()
        }
    }

    /// The `/etc/passwd` and `/etc/group` the measurements below were taken
    /// against.
    fn inhahe() -> Names {
        named(
            b"root:x:0:0:::\ninhahe:x:1000:1000:,,,:/home/inhahe:/bin/bash\n",
            b"root:x:0:\ndisk:x:6:\ninhahe:x:1000:\n",
        )
    }

    /// One long line, as a string, with the clock frozen so `is_recent` is
    /// deterministic. The format is fixed-width either way, so recency does
    /// not change the rendering — only the padding of a *missing* timestamp,
    /// which is what the failed-stat test looks at.
    fn long_line(cfg: &Config, names: &Names, w: &Widths, f: &FileInfo) -> String {
        let mut out = Out::default();
        let mut times = Times::new(cfg, localtime::Zone::utc(), frozen_clock);
        print_long_format(&mut out, cfg, names, &mut times, w, false, f);
        String::from_utf8_lossy(&out.buf).into_owned()
    }

    /// A file as `stat` leaves it: mode, links, ids, size and mtime, all of
    /// them the numbers the fixture in WSL actually had.
    fn stated(name: &str, mode: u32, kind: FileType) -> FileInfo {
        FileInfo {
            name: name.as_bytes().to_vec(),
            stat_ok: true,
            filetype: kind,
            stat: Stat {
                mode,
                nlink: 1,
                uid: 1000,
                gid: 1000,
                size: 5,
                blocks: 8,
                ino: 636_229,
                // 2023-11-14 22:13:20 UTC.
                mtime: at(1_700_000_000),
                btime: Ts::UNKNOWN,
                ..Stat::default()
            },
            ..FileInfo::default()
        }
    }

    /// The eleven columns, in the one order they are ever printed. Measured,
    /// GNU ls 9.5, on a five-byte file beside a directory (so the size column
    /// is four wide, from `4096`):
    ///
    /// ```text
    /// $ TZ=UTC ls -l --time-style=long-iso
    /// -rw-r--r-- 1 inhahe inhahe    5 2023-11-14 22:13 plain
    /// drwxr-xr-x 2 inhahe inhahe 4096 2023-11-14 22:13 sub
    /// ```
    #[test]
    fn a_long_line_is_its_columns_in_one_fixed_order() {
        let cfg = long_cfg();
        let w = Widths {
            nlink: 1,
            owner: 6,
            group: 6,
            file_size: 4,
            ..Widths::default()
        };
        let plain = stated("plain", S_IFREG | 0o644, FileType::Normal);
        assert_eq!(
            long_line(&cfg, &inhahe(), &w, &plain),
            "-rw-r--r-- 1 inhahe inhahe    5 2023-11-14 22:13 plain"
        );

        let base = stated("sub", S_IFDIR | 0o755, FileType::Directory);
        let sub = FileInfo {
            stat: Stat {
                nlink: 2,
                size: 4096,
                ..base.stat
            },
            ..base
        };
        assert_eq!(
            long_line(&cfg, &inhahe(), &w, &sub),
            "drwxr-xr-x 2 inhahe inhahe 4096 2023-11-14 22:13 sub"
        );
    }

    /// Everything the stat would have filled becomes `?` — and each `?` keeps
    /// its column's *alignment*, which is why the owner's is on the left and
    /// the inode's on the right. Measured, GNU ls 9.5, on a broken symlink
    /// listed with `-lLi` (so the stat of the target fails):
    ///
    /// ```text
    ///      ? l????????? ? ?      ?      ?                ? broken
    /// 636227 -rw-r--r-- 1 inhahe inhahe 2 2026-08-23 08:12 real
    /// ```
    ///
    /// Three things in that line are worth naming. The mode's first letter is
    /// `l` and not `?`, because it comes from what `readdir` said rather than
    /// from the stat. The owner and group `?` are left-aligned in six columns,
    /// because a `?` is passed as a *name*. And the sixteen spaces before the
    /// time's `?` are `long_time_expected_width` — the non-recent format
    /// rendered at the epoch — which is the one place that width is used.
    #[test]
    fn a_failed_stat_prints_a_question_mark_in_every_column_it_would_have_filled() {
        let cfg = Config {
            print_inode: true,
            ..long_cfg()
        };
        let w = Widths {
            inode: 6,
            nlink: 1,
            owner: 6,
            group: 6,
            file_size: 1,
            ..Widths::default()
        };
        let broken = FileInfo {
            name: b"broken".to_vec(),
            filetype: FileType::SymbolicLink,
            stat_ok: false,
            ..FileInfo::default()
        };
        assert_eq!(
            long_line(&cfg, &inhahe(), &w, &broken),
            "     ? l????????? ? ?      ?      ?                ? broken"
        );
    }

    /// A device spends the size column on `major, minor`, and the *major* half
    /// absorbs the slack — so the two halves stay put while the pair as a whole
    /// stays right-aligned with the sizes above it. Measured, GNU ls 9.5:
    ///
    /// ```text
    /// $ TZ=UTC ls -l --time-style=long-iso /dev/null /tmp/lfix/big
    /// crw-rw-rw- 1 root   root       1, 3 2026-08-20 23:35 /dev/null
    /// -rw-r--r-- 1 inhahe inhahe 12345678 2023-11-14 22:13 /tmp/lfix/big
    /// ```
    ///
    /// Four of the seven spaces between `root` and `1` are that slack:
    /// `file_size` is 8 and `major, minor` needs only 4.
    #[test]
    fn a_device_spends_the_size_column_on_a_major_and_a_minor() {
        let cfg = long_cfg();
        let w = Widths {
            nlink: 1,
            owner: 6,
            group: 6,
            file_size: 8,
            major_device: 1,
            minor_device: 1,
            ..Widths::default()
        };
        let base = stated("/dev/null", S_IFCHR | 0o666, FileType::Chardev);
        let null = FileInfo {
            stat: Stat {
                uid: 0,
                gid: 0,
                rdev: makedev(1, 3),
                ..base.stat
            },
            ..base
        };
        assert_eq!(
            long_line(&cfg, &inhahe(), &w, &null),
            "crw-rw-rw- 1 root   root       1, 3 2023-11-14 22:13 /dev/null"
        );

        // With no slack at all the major half is exactly its own width.
        let tight = Widths { file_size: 4, ..w };
        assert_eq!(
            long_line(&cfg, &inhahe(), &tight, &null),
            "crw-rw-rw- 1 root   root   1, 3 2023-11-14 22:13 /dev/null"
        );
    }

    /// A symlink prints its target after ` -> `, and the indicator that follows
    /// describes the **target** and not the link. Measured, GNU ls 9.5:
    ///
    /// ```text
    /// $ TZ=UTC ls -lF --time-style=long-iso
    /// lrwxrwxrwx 1 inhahe inhahe 3 2026-08-23 08:12 dlink -> sub/
    /// lrwxrwxrwx 1 inhahe inhahe 3 2026-08-23 08:12 elink -> exe*
    /// lrwxrwxrwx 1 inhahe inhahe 5 2023-11-14 22:13 link -> plain
    /// lrwxrwxrwx 1 inhahe inhahe 7 2023-11-14 22:13 dangle -> nowhere
    /// ```
    ///
    /// The last two get nothing: a plain target is unmarked, and a target that
    /// could not be reached leaves `link_mode` at zero, which is no file type
    /// at all.
    #[test]
    fn a_symlink_shows_its_target_and_the_targets_own_indicator() {
        let cfg = Config {
            indicator_style: Indicator::Classify,
            ..long_cfg()
        };
        let w = Widths {
            nlink: 1,
            owner: 6,
            group: 6,
            file_size: 1,
            ..Widths::default()
        };
        let link = |name: &str, target: &[u8], link_mode: u32| FileInfo {
            linkname: Some(target.to_vec()),
            link_mode,
            link_ok: link_mode != 0,
            stat: Stat {
                size: 3,
                ..stated(name, S_IFLNK | 0o777, FileType::SymbolicLink).stat
            },
            ..stated(name, S_IFLNK | 0o777, FileType::SymbolicLink)
        };
        let head = "lrwxrwxrwx 1 inhahe inhahe 3 2023-11-14 22:13 ";

        for (target, mode, tail) in [
            (&b"sub"[..], S_IFDIR | 0o755, "dlink -> sub/"),
            (&b"exe"[..], S_IFREG | 0o755, "elink -> exe*"),
            (&b"plain"[..], S_IFREG | 0o644, "link -> plain"),
            (&b"nowhere"[..], 0, "dangle -> nowhere"),
        ] {
            let name = tail.split(' ').next().unwrap_or_default();
            let f = link(name, target, mode);
            assert_eq!(long_line(&cfg, &inhahe(), &w, &f), format!("{head}{tail}"));
        }
    }

    /// `--dired`'s two spaces go in **front of the inode**, even though
    /// upstream's `dired_indent ()` call sits four columns later: the columns
    /// before it are still in a local buffer when it writes. And the offsets it
    /// records bracket the name alone. Measured, GNU ls 9.5:
    ///
    /// ```text
    /// $ TZ=UTC ls -lis --dired --time-style=long-iso plain
    ///   636229 4 -rw-r--r-- 1 inhahe inhahe 5 2023-11-14 22:13 plain
    /// //DIRED// 57 62
    /// ```
    #[test]
    fn dired_indents_in_front_of_the_inode_and_brackets_only_the_name() {
        let cfg = Config {
            dired: true,
            print_inode: true,
            print_block_size: true,
            ..long_cfg()
        };
        let w = Widths {
            inode: 6,
            block_size: 1,
            nlink: 1,
            owner: 6,
            group: 6,
            file_size: 1,
            ..Widths::default()
        };
        let plain = stated("plain", S_IFREG | 0o644, FileType::Normal);

        let mut out = Out::default();
        let mut times = Times::new(&cfg, localtime::Zone::utc(), frozen_clock);
        print_long_format(&mut out, &cfg, &inhahe(), &mut times, &w, false, &plain);

        assert_eq!(
            String::from_utf8_lossy(&out.buf),
            "  636229 4 -rw-r--r-- 1 inhahe inhahe 5 2023-11-14 22:13 plain"
        );
        assert_eq!(out.dired, vec![57, 62]);
        assert!(out.subdired.is_empty(), "a file is not a directory header");

        // Without `--dired` the same line is two columns narrower and nothing
        // is recorded at all.
        let plain_cfg = Config {
            dired: false,
            ..cfg
        };
        let mut out = Out::default();
        let mut times = Times::new(&plain_cfg, localtime::Zone::utc(), frozen_clock);
        print_long_format(
            &mut out,
            &plain_cfg,
            &inhahe(),
            &mut times,
            &w,
            false,
            &plain,
        );
        assert!(out.buf.starts_with(b"636229 "));
        assert!(out.dired.is_empty());
    }

    /// The indicator is chosen from the stat when there was one and from what
    /// `readdir` said when there was not, and the two paths must agree. The
    /// order of the tests is upstream's and decides two things: a regular file
    /// is settled before `-p` is consulted (so `-p` never stars one), and a
    /// directory is settled before it (so `-p` does mark one).
    #[test]
    fn an_indicator_comes_from_the_stat_or_from_readdir_and_never_from_neither() {
        let classify = Config {
            indicator_style: Indicator::Classify,
            ..Config::default()
        };
        let file_type = Config {
            indicator_style: Indicator::FileType,
            ..Config::default()
        };
        let slash = Config {
            indicator_style: Indicator::Slash,
            ..Config::default()
        };

        // Stated: the mode decides.
        let ind = |cfg: &Config, mode: u32| get_type_indicator(cfg, true, mode, FileType::Unknown);
        assert_eq!(ind(&classify, S_IFREG | 0o755), Some(b'*'));
        assert_eq!(
            ind(&file_type, S_IFREG | 0o755),
            None,
            "-F stars, --file-type does not"
        );
        assert_eq!(ind(&classify, S_IFREG | 0o644), None);
        assert_eq!(ind(&slash, S_IFDIR | 0o755), Some(b'/'));
        assert_eq!(
            ind(&slash, S_IFLNK | 0o777),
            None,
            "-p marks directories and nothing else"
        );
        assert_eq!(ind(&file_type, S_IFLNK | 0o777), Some(b'@'));
        assert_eq!(ind(&file_type, S_IFIFO | 0o644), Some(b'|'));
        assert_eq!(ind(&file_type, S_IFSOCK | 0o755), Some(b'='));
        assert_eq!(
            ind(&file_type, S_IFBLK | 0o660),
            None,
            "a device gets nothing"
        );

        // Unstated: the readdir type decides, and it cannot know about the
        // execute bits — so an unstated regular file is never starred, however
        // executable it turns out to be.
        let kind = |cfg: &Config, k: FileType| get_type_indicator(cfg, false, S_IFREG | 0o755, k);
        assert_eq!(kind(&classify, FileType::Normal), None);
        assert_eq!(kind(&classify, FileType::Directory), Some(b'/'));
        assert_eq!(
            kind(&classify, FileType::ArgDirectory),
            Some(b'/'),
            "a directory named on the command line is still a directory"
        );
        assert_eq!(kind(&classify, FileType::SymbolicLink), Some(b'@'));
        assert_eq!(kind(&slash, FileType::SymbolicLink), None);
        assert_eq!(kind(&classify, FileType::Unknown), None);
    }

    /// A directory that could not be stated keeps its `d`: upstream's
    /// `filetype_letter` is `"?pcdb-lswd"`, whose *last two* entries are both
    /// `d` — `directory` and `arg_directory` differ in where the listing puts
    /// them, not in what they are. Only a genuinely unknown type takes the `?`.
    #[test]
    fn the_mode_strings_first_letter_survives_a_failed_stat() {
        let unstated = |kind: FileType| {
            String::from_utf8_lossy(&mode_string(&FileInfo {
                filetype: kind,
                stat_ok: false,
                ..FileInfo::default()
            }))
            .into_owned()
        };
        assert_eq!(unstated(FileType::Unknown), "??????????");
        assert_eq!(unstated(FileType::SymbolicLink), "l?????????");
        assert_eq!(unstated(FileType::Directory), "d?????????");
        assert_eq!(unstated(FileType::ArgDirectory), "d?????????");
        assert_eq!(unstated(FileType::Fifo), "p?????????");
        assert_eq!(unstated(FileType::Chardev), "c?????????");
        assert_eq!(unstated(FileType::Blockdev), "b?????????");
        assert_eq!(unstated(FileType::Normal), "-?????????");
        assert_eq!(unstated(FileType::Sock), "s?????????");
        assert_eq!(unstated(FileType::Whiteout), "w?????????");

        // A stated file takes all ten from its mode, and never eleven: the
        // alternate-access flag has no source on a system with no ACLs.
        let stated_mode = mode_string(&FileInfo {
            stat_ok: true,
            stat: Stat {
                mode: S_IFREG | 0o4755,
                ..Stat::default()
            },
            ..FileInfo::default()
        });
        assert_eq!(String::from_utf8_lossy(&stated_mode), "-rwsr-xr-x");
        assert_eq!(stated_mode.len(), 10);
    }

    // ----------------------------------------------------- the column layouts ---

    /// A `Config` for one of the four non-long formats, with `max_idx`
    /// derived the way `main` derives it — `ceil(line_length / 3)`, the most
    /// columns a screen that wide could conceivably hold.
    fn layout_cfg(format: Format, line_length: usize) -> Config {
        Config {
            format,
            line_length,
            max_idx: (line_length / MIN_COLUMN_WIDTH)
                .saturating_add(usize::from(!line_length.is_multiple_of(MIN_COLUMN_WIDTH))),
            ..Config::default()
        }
    }

    /// The ten fixture names, laid out. They are the ones in `/tmp/lay` that
    /// every measurement in this section was taken from, and they are already
    /// in `ls`'s order.
    fn laid_out(cfg: &Config, w: &Widths, files: &[FileInfo]) -> String {
        let refs: Vec<&FileInfo> = files.iter().collect();
        let mut out = Out::default();
        let mut times = Times::new(cfg, localtime::Zone::utc(), frozen_clock);
        print_current_files(&mut out, cfg, &inhahe(), &mut times, w, false, &refs);
        String::from_utf8_lossy(&out.buf).into_owned()
    }

    /// `aaa bb c dddddddd ee fffff g hh iii jjjj` — widths 3 2 1 8 2 5 1 2 3 4,
    /// which is enough spread that the per-column maxima differ from each other
    /// and from the widest name.
    fn ten_names() -> Vec<FileInfo> {
        [
            "aaa", "bb", "c", "dddddddd", "ee", "fffff", "g", "hh", "iii", "jjjj",
        ]
        .into_iter()
        .map(|n| stated(n, S_IFREG | 0o644, FileType::Normal))
        .collect()
    }

    /// `-C` fills columns before rows and `-x` fills rows before columns, and
    /// that one difference re-sizes every column: the same ten names give
    /// different column widths under each. Measured, GNU ls 9.5, `-w40`
    /// (`.` for a space, `>` for a tab):
    ///
    /// ```text
    /// $ ls -C -w40      $ ls -x -w40
    /// aaa..c>.......ee.....g...iii    aaa..bb..c....dddddddd>ee..fffff
    /// bb...dddddddd..fffff..hh..jjjj  g....hh..iii..jjjj
    /// ```
    ///
    /// Note that `-C`'s first row is *not* the first three names: down-the-page
    /// order puts `aaa bb`, `c dddddddd`, … in the columns, so row one is the
    /// first name of each pair.
    #[test]
    fn down_the_page_and_across_it_size_their_columns_differently() {
        let w = Widths::default();
        let files = ten_names();

        assert_eq!(
            laid_out(&layout_cfg(Format::ManyPerLine, 40), &w, &files),
            "aaa  c\t       ee     g   iii\n\
             bb   dddddddd  fffff  hh  jjjj\n"
        );
        assert_eq!(
            laid_out(&layout_cfg(Format::Horizontal, 40), &w, &files),
            "aaa  bb  c    dddddddd\tee  fffff\n\
             g    hh  iii  jjjj\n"
        );
    }

    /// The gap between two columns is padded with tabs wherever a tab lands
    /// exactly, which makes the byte count depend on `-T` even though the
    /// rendered layout does not. `-T0` asks for spaces only; `-T4` moves every
    /// tab stop and so produces a third, different byte sequence for the same
    /// picture. Measured, GNU ls 9.5, `-C -w40`:
    ///
    /// ```text
    /// -T0   aaa..c.........ee.....g...iii
    /// -T4   aaa..c>>...ee>..g...iii
    /// ```
    ///
    /// The `(from + 1)` in the test is why `-T4` still spends two spaces after
    /// `bb`'s tab rather than a second tab: a tab that saves only one column is
    /// not worth the ambiguity of a tab.
    #[test]
    fn a_gap_is_paid_in_tabs_only_where_a_tab_stop_falls_inside_it() {
        let w = Widths::default();
        let files = ten_names();

        let spaces_only = Config {
            tabsize: 0,
            ..layout_cfg(Format::ManyPerLine, 40)
        };
        assert_eq!(
            laid_out(&spaces_only, &w, &files),
            "aaa  c         ee     g   iii\n\
             bb   dddddddd  fffff  hh  jjjj\n"
        );

        let four = Config {
            tabsize: 4,
            ..layout_cfg(Format::ManyPerLine, 40)
        };
        assert_eq!(
            laid_out(&four, &w, &files),
            "aaa  c\t\t   ee\t  g   iii\n\
             bb\t dddddddd  fffff  hh  jjjj\n"
        );
    }

    /// `-m` ends its lines with the separator, not before it: the comma belongs
    /// to the name it follows, and the newline is what replaces the space that
    /// would otherwise follow the comma. Measured, GNU ls 9.5, `-m -w40`:
    ///
    /// ```text
    /// aaa, bb, c, dddddddd, ee, fffff, g, hh,
    /// iii, jjjj
    /// ```
    ///
    /// The wrap test is `pos + len + 2 < line_length` — strict, and counting
    /// the separator that has not been emitted yet — so `hh,` sits at column 39
    /// of a 40-column screen and `iii` does not follow it.
    #[test]
    fn commas_end_the_line_they_wrap() {
        let w = Widths::default();
        let files = ten_names();
        assert_eq!(
            laid_out(&layout_cfg(Format::WithCommas, 40), &w, &files),
            "aaa, bb, c, dddddddd, ee, fffff, g, hh,\niii, jjjj\n"
        );
    }

    /// `-w0` says there is no screen to lay out against, which leaves `-C` and
    /// `-x` with nothing to compute: both fall through to the same flowing line
    /// the comma format uses, with a *space* in the comma's place.
    ///
    /// That gives **two** spaces between names, not one. The separator upstream
    /// prints is always two bytes — the `sep` it was passed and then either a
    /// space or a newline — so passing a space as the `sep` makes both of them
    /// spaces. Measured, GNU ls 9.5 (`.` for a space):
    ///
    /// ```text
    /// $ ls -C -w0
    /// aaa..bb..c..dddddddd..ee..fffff..g..hh..iii..jjjj
    /// ```
    #[test]
    fn no_width_at_all_leaves_one_flowing_line() {
        let w = Widths::default();
        let files = ten_names();
        let flat = "aaa  bb  c  dddddddd  ee  fffff  g  hh  iii  jjjj\n";
        assert_eq!(
            laid_out(&layout_cfg(Format::ManyPerLine, 0), &w, &files),
            flat
        );
        assert_eq!(
            laid_out(&layout_cfg(Format::Horizontal, 0), &w, &files),
            flat
        );

        // `-1` is not the same thing: it has a width, it just never uses it.
        assert_eq!(
            laid_out(&layout_cfg(Format::OnePerLine, 40), &w, &files),
            "aaa\nbb\nc\ndddddddd\nee\nfffff\ng\nhh\niii\njjjj\n"
        );
    }

    /// Every format but `-m` pads the inode and block-size prefixes to the
    /// widest in the listing; `-m` pads them to nothing, because a flowing list
    /// has no column for them to line up in. Measured, GNU ls 9.5, on a 200 KiB
    /// file beside an empty one:
    ///
    /// ```text
    /// $ ls -s -C          $ ls -s -m
    /// 200 big    0 small  200 big, 0 small
    /// ```
    #[test]
    fn a_flowing_list_pads_its_prefixes_to_nothing() {
        let w = Widths {
            block_size: 3,
            ..Widths::default()
        };
        let mut big = stated("big", S_IFREG | 0o644, FileType::Normal);
        big.stat.blocks = 400; // 400 × 512 B = 200 KiB, printed in KiB units.
        let mut small = stated("small", S_IFREG | 0o644, FileType::Normal);
        small.stat.blocks = 0;
        let files = vec![big, small];

        let columns = Config {
            print_block_size: true,
            ..layout_cfg(Format::ManyPerLine, 40)
        };
        assert_eq!(laid_out(&columns, &w, &files), "200 big    0 small\n");

        let commas = Config {
            print_block_size: true,
            ..layout_cfg(Format::WithCommas, 40)
        };
        assert_eq!(laid_out(&commas, &w, &files), "200 big, 0 small\n");
    }

    /// A layout is not `n` equal columns: each is sized independently, and a
    /// layout is rejected only when the *sum* overflows. The `+ 2` is charged
    /// to every column but the last, so a listing may be exactly `line_length`
    /// wide — `line_len < line_length` is tested against a total that does not
    /// include the trailing separator the last column never gets.
    #[test]
    fn a_layout_is_rejected_on_its_total_and_never_on_its_widest_column() {
        // Four names of four bytes: 4 + 2 per column, but only 4 for the last.
        let lengths = [4usize, 4, 4, 4];
        let cfg = |line_length: usize| layout_cfg(Format::ManyPerLine, line_length);

        // 6 + 6 + 6 + 4 = 22, which fits a 23-column screen and not a 22.
        assert_eq!(calculate_columns(&cfg(23), &lengths, true).0, 4);
        assert_eq!(calculate_columns(&cfg(22), &lengths, true).0, 3);

        // The widths come back per column, and the last one is short by the
        // two spaces it does not have to pay for.
        let (cols, widths) = calculate_columns(&cfg(23), &lengths, true);
        assert_eq!(cols, 4);
        assert_eq!(widths, vec![6, 6, 6, 4]);

        // `max_idx` caps the search, not the answer: with one name there is one
        // column however wide the screen is.
        assert_eq!(calculate_columns(&cfg(200), &[3], true).0, 1);
    }

    /// A name GNU's `mbsnwidth` refuses is [`usize::MAX`] columns wide, not
    /// zero (see [`display_width`]), and every width derived from it wraps.
    /// The result is not a subtle one column out — it is visible in the byte
    /// count of every format.
    ///
    /// Measured, GNU ls 9.5, `-U` on a directory holding `AAAA`, `BBBB`,
    /// `CCCC` and the two-name-long control-character names `\abell` and
    /// `\acell` (`.` for a space, `>` for a tab; byte counts from `wc -c`):
    ///
    /// ```text
    /// $ ls -C -U \abell AAAA BBBB CCCC     \abellAAAA..BBBB..CCCC          22
    /// $ ls -x -U \abell AAAA BBBB CCCC     \abellAAAA..BBBB..CCCC          22
    /// $ ls -m -U \abell AAAA BBBB CCCC     \abell,\nAAAA,.BBBB,.CCCC       24
    /// $ ls -m -w12 -U  (same order)        \abell,\nAAAA,.BBBB,\nCCCC      24
    /// $ ls -C -U AAAA \abell BBBB CCCC     AAAA..\abell>.BBBB..CCCC        24
    /// $ ls -C -U AAAA BBBB \abell CCCC     AAAA..BBBB..\abell....CCCC      26
    /// $ ls -C -U \abell \acell AAAA BBBB   \abell\acell....AAAA..BBBB      25
    /// ```
    ///
    /// Three separate consequences of the same underflow are visible there:
    ///
    /// * **A refused name in the first column is followed by nothing.**
    ///   `indent` is called with `from = pos + usize::MAX`, which at `pos == 0`
    ///   is `usize::MAX` — past the `to` it is padding towards — so it emits no
    ///   padding at all and `AAAA` butts straight up against `\abell`.
    /// * **A refused name anywhere else is followed by one column too many.**
    ///   `from` lands one column *before* `pos`, so the gap is three wide
    ///   where the column asks for two — a tab and a space in the second-column
    ///   case, four spaces in the third.
    /// * **`-m` breaks the line after it.** The wrap guard's second half,
    ///   `pos <= SIZE_MAX - len - 2`, is the one that fails: `pos` is already
    ///   `usize::MAX` from the refused name, and `SIZE_MAX - len - 2` wrapped
    ///   to `SIZE_MAX - 1`.
    ///
    /// The column *widths* are untouched by any of this, because
    /// `calculate_columns` charges the wrapped `real_length` of 1, which never
    /// beats `MIN_COLUMN_WIDTH`. Only the padding underflows.
    #[test]
    fn a_refused_name_is_laid_out_as_usize_max_columns_wide() {
        let w = Widths::default();
        let files = |names: &[&str]| -> Vec<FileInfo> {
            names
                .iter()
                .map(|n| stated(n, S_IFREG | 0o644, FileType::Normal))
                .collect()
        };

        // First column: the padding vanishes entirely.
        let first = files(&["\x07bell", "AAAA", "BBBB", "CCCC"]);
        assert_eq!(
            laid_out(&layout_cfg(Format::ManyPerLine, 80), &w, &first),
            "\x07bellAAAA  BBBB  CCCC\n"
        );
        assert_eq!(
            laid_out(&layout_cfg(Format::Horizontal, 80), &w, &first),
            "\x07bellAAAA  BBBB  CCCC\n"
        );

        // Second and third: one column of padding too many.
        assert_eq!(
            laid_out(
                &layout_cfg(Format::ManyPerLine, 80),
                &w,
                &files(&["AAAA", "\x07bell", "BBBB", "CCCC"])
            ),
            "AAAA  \x07bell\t BBBB  CCCC\n"
        );
        assert_eq!(
            laid_out(
                &layout_cfg(Format::ManyPerLine, 80),
                &w,
                &files(&["AAAA", "BBBB", "\x07bell", "CCCC"])
            ),
            "AAAA  BBBB  \x07bell    CCCC\n"
        );

        // Two of them in a row: the first eats its padding, the second
        // overpays, and the two effects do not cancel.
        assert_eq!(
            laid_out(
                &layout_cfg(Format::ManyPerLine, 80),
                &w,
                &files(&["\x07bell", "\x07cell", "AAAA", "BBBB"])
            ),
            "\x07bell\x07cell    AAAA  BBBB\n"
        );

        // `-m` wraps after it, on a screen it is nowhere near filling…
        assert_eq!(
            laid_out(&layout_cfg(Format::WithCommas, 80), &w, &first),
            "\x07bell,\nAAAA, BBBB, CCCC\n"
        );
        // …and goes on wrapping the rest of the line normally.
        assert_eq!(
            laid_out(&layout_cfg(Format::WithCommas, 12), &w, &first),
            "\x07bell,\nAAAA, BBBB,\nCCCC\n"
        );
    }

    // ------------------------------------------------------ reading the files ---

    /// A filesystem made of three maps: what `lstat` answers, where a symlink
    /// points, and what a directory holds. `stat` is `lstat` after following
    /// the links, which is the only thing the real one does that the maps do
    /// not say directly.
    #[derive(Default)]
    struct FakeTree {
        lstats: std::collections::HashMap<Vec<u8>, Stat>,
        targets: std::collections::HashMap<Vec<u8>, Vec<u8>>,
        dirs: std::collections::HashMap<Vec<u8>, Vec<Entry>>,
    }

    fn enoent() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::NotFound)
    }

    impl FakeTree {
        fn file(mut self, path: &str, stat: Stat) -> Self {
            self.lstats.insert(path.as_bytes().to_vec(), stat);
            self
        }

        /// A symlink: an `lstat` that reports one, and a target to follow.
        fn link(mut self, path: &str, target: &str) -> Self {
            self.lstats.insert(
                path.as_bytes().to_vec(),
                Stat {
                    mode: S_IFLNK | 0o777,
                    nlink: 1,
                    size: target.len() as i64,
                    ino: 100,
                    ..Stat::default()
                },
            );
            self.targets
                .insert(path.as_bytes().to_vec(), target.as_bytes().to_vec());
            self
        }
    }

    impl Tree for FakeTree {
        fn lstat(&self, path: &[u8]) -> std::io::Result<Stat> {
            self.lstats.get(path).copied().ok_or_else(enoent)
        }

        fn stat(&self, path: &[u8]) -> std::io::Result<Stat> {
            let mut at = path.to_vec();
            for _ in 0..8 {
                match self.targets.get(&at) {
                    Some(target) => at.clone_from(target),
                    None => break,
                }
            }
            self.lstats.get(&at).copied().ok_or_else(enoent)
        }

        fn read_link(&self, path: &[u8]) -> std::io::Result<Vec<u8>> {
            self.targets.get(path).cloned().ok_or_else(enoent)
        }

        fn read_dir<'t>(&'t self, path: &[u8]) -> std::io::Result<DirIter<'t>> {
            let entries = self.dirs.get(path).ok_or_else(enoent)?;
            Ok(Box::new(entries.iter().cloned().map(Ok)))
        }
    }

    /// One `gobble_file` call, with the diagnostics captured.
    struct Gobbled {
        cwd: Cwd,
        status: Exit,
        err: String,
        blocks: u64,
    }

    fn gobble(
        tree: &dyn Tree,
        cfg: &Config,
        name: &str,
        kind: FileType,
        inode: u64,
        command_line_arg: bool,
        dirname: &str,
    ) -> Gobbled {
        let mut cwd = Cwd::default();
        let mut status = Exit::default();
        let mut err = Vec::new();
        let blocks = gobble_file(
            tree,
            cfg,
            &inhahe(),
            &mut cwd,
            &mut status,
            &mut err,
            name.as_bytes(),
            kind,
            inode,
            command_line_arg,
            dirname.as_bytes(),
        );
        Gobbled {
            cwd,
            status,
            err: String::from_utf8_lossy(&err).into_owned(),
            blocks,
        }
    }

    /// `ls` in long format, which is the setting that makes every column and
    /// therefore every `stat` happen.
    fn stating_cfg() -> Config {
        Config {
            format_needs_stat: true,
            ..long_cfg()
        }
    }

    /// A file that cannot be reached is dropped when it was named on the
    /// command line and kept when it was found inside a directory — and the two
    /// take *different exit statuses*, 2 and 1. Measured, GNU ls 9.5, on a
    /// directory holding a dangling link and one real file:
    ///
    /// ```text
    /// $ ls nosuch                 status 2, no output at all
    /// $ ls -L -l .                status 1
    ///   total 0
    ///   l????????? ? ?      ?      ? ? dangle
    ///   -rw-r--r-- 1 inhahe inhahe 0 T real
    /// ```
    ///
    /// Both print `ls: cannot access 'x': No such file or directory`, and both
    /// name the file without a `./` — a `dirname` of exactly `.` contributes
    /// nothing to the path a diagnostic quotes.
    #[test]
    fn a_stat_that_failed_drops_an_operand_and_keeps_a_directory_entry() {
        let tree = FakeTree::default();
        let cfg = stating_cfg();

        let operand = gobble(&tree, &cfg, "nosuch", FileType::Unknown, 0, true, "");
        assert!(operand.cwd.files.is_empty(), "an operand leaves no row");
        assert_eq!(operand.status, Exit(2));
        assert_eq!(
            operand.err,
            "ls: cannot access 'nosuch': No such file or directory\n"
        );

        let entry = gobble(&tree, &cfg, "dangle", FileType::SymbolicLink, 7, false, ".");
        assert_eq!(entry.cwd.files.len(), 1, "an entry keeps its row");
        let f = entry.cwd.files.first().unwrap();
        assert!(!f.stat_ok);
        // The type `readdir` gave survives, which is what puts the `l` at the
        // front of `l?????????`.
        assert_eq!(f.filetype, FileType::SymbolicLink);
        assert_eq!(entry.status, Exit(1));
        assert_eq!(
            entry.err,
            "ls: cannot access 'dangle': No such file or directory\n"
        );
        assert_eq!(entry.blocks, 0, "unknown blocks are not counted as zero");
        // And it widened nothing: the accumulation is after the early return.
        assert_eq!(entry.cwd.widths, Widths::default());
    }

    /// `ls -i` prints the inode `readdir` supplied and never stats for it, so
    /// the column is widened outside the stat branch — which is also why a file
    /// whose stat *failed* does not widen it, having returned before reaching
    /// the accumulation. Measured, GNU ls 9.5:
    ///
    /// ```text
    /// $ ls -i .
    /// 636259 dangle
    /// 636260 real
    /// ```
    ///
    /// with no `stat` at all: `-i` alone needs neither the type nor the mode.
    #[test]
    fn an_inode_from_readdir_widens_its_column_with_nothing_stated() {
        let tree = FakeTree::default();
        let cfg = Config {
            print_inode: true,
            ..Config::default()
        };

        let got = gobble(
            &tree,
            &cfg,
            "dangle",
            FileType::SymbolicLink,
            636_259,
            false,
            ".",
        );
        assert_eq!(got.cwd.widths.inode, 6);
        assert!(
            !got.cwd.files.first().unwrap().stat_ok,
            "nothing was stated: the empty tree would have failed"
        );
        assert_eq!(got.status, Exit(0), "and so nothing failed either");
        assert!(got.err.is_empty());
    }

    /// A symlink's target is read for `-l`, and the `f->quoted` that was
    /// measured for the *name* is then reused to decide whether the target
    /// needs the slow path. A target that needs quoting resets it to "not
    /// measured" rather than to "quoted", and leaves `cwd_some_quoted` alone:
    /// the target is not in the name column, so it cannot change what that
    /// column aligns to.
    #[test]
    fn a_link_target_that_needs_quoting_unmeasures_the_name_without_aligning_it() {
        let tree = FakeTree::default().link("plain", "with space");
        let cfg = Config {
            quoting_style: Style::Shell,
            align_variable_outer_quotes: true,
            ..stating_cfg()
        };

        let got = gobble(&tree, &cfg, "plain", FileType::SymbolicLink, 0, false, "");
        let f = got.cwd.files.first().unwrap();
        assert_eq!(f.linkname.as_deref(), Some(b"with space".as_slice()));
        assert_eq!(
            f.quoted, None,
            "the name was measured as unquoted, then unmeasured by the target"
        );
        assert!(
            !got.cwd.some_quoted,
            "a target never joins the directory's alignment"
        );

        // A target that needs no quoting leaves the measurement standing.
        let tame = FakeTree::default().link("plain", "tame");
        let got = gobble(&tame, &cfg, "plain", FileType::SymbolicLink, 0, false, "");
        assert_eq!(got.cwd.files.first().unwrap().quoted, Some(false));

        // Whereas a *name* that needs quoting does join it, and is what makes
        // every later name in the directory take the slow path.
        let odd = FakeTree::default().link("with space", "tame");
        let got = gobble(
            &odd,
            &cfg,
            "with space",
            FileType::SymbolicLink,
            0,
            false,
            "",
        );
        assert_eq!(got.cwd.files.first().unwrap().quoted, Some(true));
        assert!(got.cwd.some_quoted);
    }

    /// The target is followed only when an indicator could tell the types
    /// apart. `-p` marks directories and nothing else, so it is *below* the
    /// threshold and does not follow; `--file-type` and `-F` are at or above it
    /// and do.
    #[test]
    fn a_target_is_followed_only_for_an_indicator_that_distinguishes_types() {
        let tree = FakeTree::default().link("l", "d").file(
            "d",
            Stat {
                mode: S_IFDIR | 0o755,
                nlink: 2,
                ..Stat::default()
            },
        );
        let followed = |indicator: Indicator| {
            let cfg = Config {
                indicator_style: indicator,
                ..stating_cfg()
            };
            let got = gobble(&tree, &cfg, "l", FileType::SymbolicLink, 0, false, "");
            let f = got.cwd.files.first().unwrap();
            (f.link_ok, f.link_mode)
        };
        assert_eq!(followed(Indicator::None), (false, 0));
        assert_eq!(followed(Indicator::Slash), (false, 0));
        assert_eq!(followed(Indicator::FileType), (true, S_IFDIR | 0o755));
        assert_eq!(followed(Indicator::Classify), (true, S_IFDIR | 0o755));

        // `--group-directories-first` follows it too, by a different route: it
        // needs the target's mode to know which group the link belongs in.
        let cfg = Config {
            check_symlink_mode: true,
            ..stating_cfg()
        };
        let got = gobble(&tree, &cfg, "l", FileType::SymbolicLink, 0, false, "");
        assert!(got.cwd.files.first().unwrap().link_ok);
    }

    /// A directory named on the command line becomes `arg_directory`, which is
    /// what later moves it out of the listing and into a heading of its own.
    /// `-d` is the option that stops it, and it stops it here rather than at
    /// the point of printing — which is why `ls -d dir` gives a row and `ls
    /// dir` gives a heading.
    #[test]
    fn a_directory_operand_becomes_a_heading_unless_d_asked_for_a_row() {
        let tree = FakeTree::default().file(
            "d",
            Stat {
                mode: S_IFDIR | 0o755,
                nlink: 2,
                blocks: 8,
                ..Stat::default()
            },
        );
        let kind = |cfg: &Config, command_line_arg: bool| {
            gobble(&tree, cfg, "d", FileType::Unknown, 0, command_line_arg, "")
                .cwd
                .files
                .first()
                .unwrap()
                .filetype
        };
        let cfg = stating_cfg();
        assert_eq!(kind(&cfg, true), FileType::ArgDirectory);
        assert_eq!(kind(&cfg, false), FileType::Directory);

        let immediate = Config {
            immediate_dirs: true,
            ..cfg
        };
        assert_eq!(kind(&immediate, true), FileType::Directory);
    }

    /// `-L` makes every stat a `stat`, so a dangling link fails where an
    /// `lstat` would have succeeded; `-H` does the same but only for operands.
    /// `--dereference-command-line-symlink-to-dir` starts with a `stat` too and
    /// falls back to `lstat` when the target turns out not to be a directory —
    /// so a link to a *file* prints as a link and a link to a directory prints
    /// as the directory.
    ///
    /// The fallback is also taken on `ENOENT` and `ELOOP`, which is the whole
    /// difference between the last two on a dangling link. Measured, GNU ls
    /// 9.5, with `dangle -> nowhere`, `tofile -> f` and `todir -> d`:
    ///
    /// ```text
    /// $ ls -l -H dangle                        status 2, cannot access 'dangle'
    /// $ ls -l -L dangle                        status 2, cannot access 'dangle'
    /// $ ls -l --dereference-…-to-dir dangle    status 0, lrwxrwxrwx … dangle -> nowhere
    /// $ ls -l --dereference-…-to-dir tofile    status 0, lrwxrwxrwx … tofile -> f
    /// $ ls -l --dereference-…-to-dir todir     status 0, total 0   (it became the heading)
    /// $ ls -l -H tofile                        status 0, -rw-r--r-- … tofile
    /// ```
    #[test]
    fn the_four_dereference_modes_differ_in_which_link_gets_stated() {
        let tree = FakeTree::default()
            .link("tofile", "f")
            .link("todir", "d")
            .link("dangle", "nowhere")
            .file(
                "f",
                Stat {
                    mode: S_IFREG | 0o644,
                    nlink: 1,
                    ..Stat::default()
                },
            )
            .file(
                "d",
                Stat {
                    mode: S_IFDIR | 0o755,
                    nlink: 2,
                    ..Stat::default()
                },
            );
        let kind = |deref: Deref, name: &str, command_line_arg: bool| {
            let cfg = Config {
                dereference: deref,
                ..stating_cfg()
            };
            let got = gobble(
                &tree,
                &cfg,
                name,
                FileType::Unknown,
                0,
                command_line_arg,
                "",
            );
            got.cwd
                .files
                .first()
                .map(|f| (f.stat_ok, f.filetype))
                .unwrap_or((false, FileType::Unknown))
        };

        // `-L`: the target decides, and a dangling one is not reachable at all.
        assert_eq!(
            kind(Deref::Always, "tofile", false),
            (true, FileType::Normal)
        );
        assert_eq!(
            kind(Deref::Always, "todir", false),
            (true, FileType::Directory)
        );
        assert_eq!(
            kind(Deref::Always, "dangle", true),
            (false, FileType::Unknown),
            "an unreachable operand leaves no row"
        );

        // `-H`: the same, but only for an operand.
        assert_eq!(
            kind(Deref::CommandLineArguments, "tofile", true),
            (true, FileType::Normal)
        );
        assert_eq!(
            kind(Deref::CommandLineArguments, "tofile", false),
            (true, FileType::SymbolicLink)
        );

        // `--dereference-command-line-symlink-to-dir`: a link to a directory is
        // the directory, a link to anything else stays a link.
        assert_eq!(
            kind(Deref::CommandLineSymlinkToDir, "todir", true),
            (true, FileType::ArgDirectory)
        );
        assert_eq!(
            kind(Deref::CommandLineSymlinkToDir, "tofile", true),
            (true, FileType::SymbolicLink)
        );
        // And a dangling one falls back to the `lstat`, which succeeds — so it
        // is listed rather than reported.
        assert_eq!(
            kind(Deref::CommandLineSymlinkToDir, "dangle", true),
            (true, FileType::SymbolicLink)
        );

        // `--dereference-command-line` has no such fallback: a dangling operand
        // is an error under it.
        assert_eq!(
            kind(Deref::CommandLineArguments, "dangle", true),
            (false, FileType::Unknown)
        );
    }
}
