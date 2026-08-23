//! ls — list directory contents.
//!
//! A port of GNU `ls` (coreutils 9.4), written against its source rather than
//! against a memory of its behaviour, because almost every rule in it is
//! arbitrary in the precise sense: it cannot be re-derived, only copied. The
//! column allocator, the six-month recency window, the rule that a name's
//! *suffix* decides `-v` order, and the fact that `--time-style=nosuch` is not
//! an error unless `-l` is also given are all of that kind.
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
use coreutils::fnmatch::{Flags, fnmatch};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::human::{Opts, default_block_size};
use coreutils::pathname::{base_len, last_component, last_component_offset};
use coreutils::quote::{Mb, Style, next_mb, os_bytes, quote};
use coreutils::vercmp::version;
use coreutils::xnum::{self, Status, strtol_fatal};
use modechange::{S_IFDIR, S_IFMT};
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
    Whiteout,
    /// A directory named on the command line and being listed *as a file* —
    /// `ls -d`, or the header line a recursive listing prints for it.
    ArgDirectory,
}

/// GNU's `filetype_letter`, the first column of `-l`'s mode string.
const fn filetype_letter(kind: FileType) -> u8 {
    match kind {
        FileType::Unknown | FileType::ArgDirectory => b'?',
        FileType::Fifo => b'p',
        FileType::Chardev => b'c',
        FileType::Directory => b'd',
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
            output_block_size: 0,
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
                cfg.file_output_block_size = 1;
            }
            Flag::Si => {
                cfg.human_output_opts = Opts::AUTOSCALE | Opts::SI;
                cfg.file_human_output_opts = cfg.human_output_opts;
                cfg.output_block_size = 1;
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
            Flag::Dired => cfg.dired = true,
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
/// `if (! output_block_size)`.
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
    if cfg.output_block_size == 0 {
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
/// Every caller in `ls` immediately clamps the -1 to zero — see
/// [`display_width`] — so a name holding a stray byte is laid out as if it
/// were empty. That is GNU's behaviour and it is why `ls` in a terminal
/// defaults to `-q`, which replaces such bytes before this is ever asked.
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

/// [`mbs_width`] with GNU's `MAX (0, …)` applied: a name it refuses occupies
/// no columns at all.
fn display_width(text: &[u8]) -> usize {
    mbs_width(text).unwrap_or(0)
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
fn name_width(cfg: &Config, cwd_some_quoted: bool, f: &FileInfo) -> usize {
    if let Some(width) = f.width {
        return width;
    }
    let out = quote_name(cfg, &cfg.filename_extra, cwd_some_quoted, &f.name, f.quoted);
    out.width.saturating_add(usize::from(out.pad))
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
        Sort::Width => name_width(cfg, cwd_some_quoted, a)
            .cmp(&name_width(cfg, cwd_some_quoted, b))
            .then_with(by_name),
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
            f.width = Some(out.width.saturating_add(usize::from(out.pad)));
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
/// Upstream walks the files backwards and prepends to the queue, so the two
/// reversals cancel and the queue ends up in listing order. Pushing forwards
/// onto a `Vec` we then drain from the front is the same order, so the
/// backwards walk is not reproduced.
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

    for f in files.iter() {
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

fn main() -> ExitCode {
    ExitCode::SUCCESS
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "a test that cannot build its own fixture should fail loudly"
)]
mod tests {
    use super::*;

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
        // …and every caller clamps that to zero rather than to the name.
        assert_eq!(display_width(b"n\xffame"), 0);
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
        // …and with the pass off, the raw byte survives and costs the whole
        // name its width.
        let raw = Config {
            quoting_style: Style::Literal,
            ..Config::default()
        };
        let out = quote_name(&raw, b"", false, b"n\xffame", None);
        assert_eq!(out.bytes, b"n\xffame");
        assert_eq!(out.width, 0);
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
    /// name holding a two-column character is wider than its character count
    /// and a name holding an unprintable byte has no width at all.
    #[test]
    fn width_order_measures_the_rendering_and_not_the_bytes() {
        let cfg = Config {
            sort: Sort::Width,
            ..Config::default()
        };
        let files = vec![
            file("abc"),
            // Five characters, one of them DEL: `mbsnwidth` refuses the whole
            // name and `ls` clamps that to zero, so it sorts as the narrowest
            // thing here despite being the longest.
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
        assert_eq!(pending.first().unwrap().name.as_deref(), Some(&b"dirA"[..]));

        let mut files = vec![file("x"), dir("dirA"), dir("dirZ")];
        let mut pending = Vec::new();
        extract_dirs_from_files(Some(b"."), false, false, &mut files, &mut pending);
        assert_eq!(files.len(), 3, "-R leaves the subdirectory in the listing");
        let queued: Vec<&[u8]> = pending.iter().filter_map(|p| p.name.as_deref()).collect();
        assert_eq!(queued, [&b"./dirA"[..], &b"./dirZ"[..]]);
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
        let queued: Vec<&[u8]> = pending.iter().filter_map(|p| p.name.as_deref()).collect();
        assert_eq!(
            queued,
            [&b"top/sub"[..]],
            "a/.. ends in .. and is a cycle too"
        );

        let mut files = subdirs();
        let mut pending = Vec::new();
        extract_dirs_from_files(None, true, false, &mut files, &mut pending);
        assert_eq!(pending.len(), 4);
    }

    /// `-R` puts a marker in the queue ahead of a directory's children, so the
    /// loop that reads the queue knows when the directory is finished and can
    /// stop guarding against a cycle through it. It carries no name, which is
    /// how it is told apart from a directory to open.
    #[test]
    fn recursion_marks_the_end_of_a_directory_in_the_queue_itself() {
        let mut files = vec![dir("sub")];
        let mut pending = Vec::new();
        extract_dirs_from_files(Some(b"top"), false, true, &mut files, &mut pending);
        assert_eq!(pending.first().unwrap().name, None);
        assert_eq!(
            pending.first().unwrap().realname.as_deref(),
            Some(&b"top"[..])
        );
        assert_eq!(
            pending.get(1).unwrap().name.as_deref(),
            Some(&b"top/sub"[..])
        );

        // Without `-R` there is no cycle to detect and no marker.
        let mut files = vec![dir("sub")];
        let mut pending = Vec::new();
        extract_dirs_from_files(Some(b"top"), false, false, &mut files, &mut pending);
        assert_eq!(pending.len(), 1);
    }
}
