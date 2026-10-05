//! `diff` — compare files line by line.
//!
//! A port of GNU diffutils 3.10's `diff`, file by file: `diff.c` (here), and
//! `analyze.c` with gnulib's `diffseq.h`, `io.c`, `util.c`, `context.c`,
//! `normal.c`, `ed.c`, `ifdef.c`, `side.c` and `dir.c` beside it -- the same
//! options, messages and exit statuses, and above all the same edit script.
//!
//! # Why the edit script is the point
//!
//! Two files generally have many shortest edit scripts, and every one of
//! them is a correct diff. The one GNU prints comes out of a particular
//! pipeline: lines with no match in the other file are set aside first
//! (`discard_confusing_lines`), Myers' algorithm runs on what is left,
//! splitting at the middle snake and giving up early with a heuristic when
//! a comparison gets expensive, and then every run of changes is slid as far
//! as identical lines allow (`shift_boundaries`). The hand-written `diff`
//! this replaced computed an LCS and printed a different, equally valid
//! script for about half of all random inputs -- different hunks for
//! `patch`, and different text for anyone comparing outputs.
//!
//! # What no reading of `--help` suggests
//!
//! **The comparison only sees the middle.** The common prefix and suffix are
//! cut at line boundaries, keeping `--horizon-lines` (at least the context)
//! lines of each, and only the lines between are compared -- so the
//! discarding thresholds, and where a run of changes may slide, depend on
//! where the cuts fall.
//!
//! **A binary file is one with a NUL in its first block** -- `st_blksize`
//! bytes, 4096 on most file systems. A NUL further in does not count.
//!
//! **`-e` and `-f` cannot say a file lacks its last newline,** so they print
//! the diff as though it had one, then say `No newline at end of file` on
//! standard error and exit 2.
//!
//! **Digits are options:** `-3` is `-C 3`, and `-1 -2` is `-C 12`, because
//! the digits of consecutive digit options accumulate.
//!
//! # Deliberate differences
//!
//! `--help` omits the bug-reporting block and `--version` names SlateOS. No
//! signal handlers are installed to reset the terminal's colours on an
//! interrupt: SlateOS does not deliver Unix signals for process control.
//! Column widths are this system's table (`charwidth`), which gives a soft
//! hyphen no width where glibc gives it one (design-decisions §1042).
//!
//! # Reference
//!
//! Measured against GNU diff 3.10 through WSL; where measurement could not
//! settle a rule, the diffutils 3.10 sources settled it. `scripts/diff-diff.sh`
//! is the executable form of every claim here.

mod analyze;
mod context;
mod dir;
mod ifdef;
mod io;
mod normal;
mod side;
#[cfg(test)]
mod tests;
mod util;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes};
use coreutils::stdfd;
use io::{Desc, FileData, Lin, Stat};
use std::ffi::OsString;
use std::process::ExitCode;
use std::rc::Rc;
use util::Out;

coreutils::guard_std_fds!();

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_FAILURE: i32 = 1;
pub const EXIT_TROUBLE: i32 = 2;

/// `diff -Z; echo $?` is 2.
const DIFF: Program = Program::new("diff", EXIT_TROUBLE);

/// Upstream's `shortopts`, verbatim.
const SHORT_OPTIONS: &str = "0123456789abBcC:dD:eEfF:hHiI:lL:nNpPqrsS:tTuU:vwW:x:X:yZ";

/// Upstream's `longopts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("binary", Takes::Nothing),
    ("brief", Takes::Nothing),
    ("changed-group-format", Takes::Required),
    ("color", Takes::Optional),
    ("context", Takes::Optional),
    ("ed", Takes::Nothing),
    ("exclude", Takes::Required),
    ("exclude-from", Takes::Required),
    ("expand-tabs", Takes::Nothing),
    ("forward-ed", Takes::Nothing),
    ("from-file", Takes::Required),
    ("help", Takes::Nothing),
    ("horizon-lines", Takes::Required),
    ("ifdef", Takes::Required),
    ("ignore-all-space", Takes::Nothing),
    ("ignore-blank-lines", Takes::Nothing),
    ("ignore-case", Takes::Nothing),
    ("ignore-file-name-case", Takes::Nothing),
    ("ignore-matching-lines", Takes::Required),
    ("ignore-space-change", Takes::Nothing),
    ("ignore-tab-expansion", Takes::Nothing),
    ("ignore-trailing-space", Takes::Nothing),
    ("inhibit-hunk-merge", Takes::Nothing),
    ("initial-tab", Takes::Nothing),
    ("label", Takes::Required),
    ("left-column", Takes::Nothing),
    ("line-format", Takes::Required),
    ("minimal", Takes::Nothing),
    ("new-file", Takes::Nothing),
    ("new-group-format", Takes::Required),
    ("new-line-format", Takes::Required),
    ("no-dereference", Takes::Nothing),
    ("no-ignore-file-name-case", Takes::Nothing),
    ("normal", Takes::Nothing),
    ("old-group-format", Takes::Required),
    ("old-line-format", Takes::Required),
    ("paginate", Takes::Nothing),
    ("palette", Takes::Required),
    ("rcs", Takes::Nothing),
    ("recursive", Takes::Nothing),
    ("report-identical-files", Takes::Nothing),
    ("sdiff-merge-assist", Takes::Nothing),
    ("show-c-function", Takes::Nothing),
    ("show-function-line", Takes::Required),
    ("side-by-side", Takes::Nothing),
    ("speed-large-files", Takes::Nothing),
    ("starting-file", Takes::Required),
    ("strip-trailing-cr", Takes::Nothing),
    ("suppress-blank-empty", Takes::Nothing),
    ("suppress-common-lines", Takes::Nothing),
    ("tabsize", Takes::Required),
    ("text", Takes::Nothing),
    ("to-file", Takes::Required),
    ("unchanged-group-format", Takes::Required),
    ("unchanged-line-format", Takes::Required),
    ("unidirectional-new-file", Takes::Nothing),
    ("unified", Takes::Optional),
    ("version", Takes::Nothing),
    ("width", Takes::Required),
    ("-no-directory", Takes::Nothing),
    ("-presume-output-tty", Takes::Nothing),
];

/// `GUTTER_WIDTH_MINIMUM`.
const GUTTER_WIDTH_MINIMUM: usize = 3;

/// `CONTEXT_MAX`: so that `2 * CONTEXT + 1` cannot overflow.
const CONTEXT_MAX: Lin = (Lin::MAX - 1) / 2;

/// `enum output_style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStyle {
    Unspecified,
    Normal,
    Context,
    Unified,
    Ed,
    ForwardEd,
    Rcs,
    Ifdef,
    Sdiff,
}

/// `enum DIFF_white_space`, in upstream's order: the comparisons between
/// these are upstream's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WhiteSpace {
    None,
    TabExpansion,
    TrailingSpace,
    TabExpansionAndTrailingSpace,
    SpaceChange,
    AllSpace,
}

impl WhiteSpace {
    /// The `IGNORE_TRAILING_SPACE` bit.
    pub fn trailing(self) -> bool {
        matches!(
            self,
            WhiteSpace::TrailingSpace | WhiteSpace::TabExpansionAndTrailingSpace
        )
    }
    /// The `IGNORE_TAB_EXPANSION` bit.
    pub fn tab_expansion(self) -> bool {
        matches!(
            self,
            WhiteSpace::TabExpansion | WhiteSpace::TabExpansionAndTrailingSpace
        )
    }
    /// `ignore_white_space |= bit` for `-E` and `-Z`, below `-b`.
    fn or(self, tab: bool, trailing: bool) -> WhiteSpace {
        if self >= WhiteSpace::SpaceChange {
            return self;
        }
        match (self.tab_expansion() || tab, self.trailing() || trailing) {
            (false, false) => WhiteSpace::None,
            (true, false) => WhiteSpace::TabExpansion,
            (false, true) => WhiteSpace::TrailingSpace,
            (true, true) => WhiteSpace::TabExpansionAndTrailingSpace,
        }
    }
}

/// `enum colors_style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorsStyle {
    Never,
    Auto,
    Always,
}

/// `-x` and `-X` patterns: gnulib's `exclude` with `EXCLUDE_WILDCARDS`.
#[derive(Debug, Default)]
pub struct Exclude {
    /// Each glob, and whether `--ignore-file-name-case` was in force when it
    /// was added.
    patterns: Vec<(Vec<u8>, bool)>,
}

impl Exclude {
    fn add(&mut self, pattern: &[u8], casefold: bool) {
        self.patterns.push((pattern.to_vec(), casefold));
    }

    /// `excluded_file_name`: a match of the whole name, or of any part after
    /// a `/`.
    pub fn excludes(&self, name: &[u8]) -> bool {
        use coreutils::fnmatch::{Flags, fnmatch};
        self.patterns.iter().any(|(glob, casefold)| {
            let flags = if *casefold {
                Flags::CASEFOLD
            } else {
                Flags::NONE
            };
            if fnmatch(glob, name, flags) {
                return true;
            }
            name.iter().enumerate().any(|(i, &b)| {
                b == b'/'
                    && name.get(i.saturating_add(1)) != Some(&b'/')
                    && fnmatch(
                        glob,
                        name.get(i.saturating_add(1)..).unwrap_or_default(),
                        flags,
                    )
            })
        })
    }
}

/// Every option, once parsed: upstream's file-scope variables.
pub struct Opts {
    pub output_style: OutputStyle,
    pub colors_style: ColorsStyle,
    pub no_diff_means_no_output: bool,
    pub context: Lin,
    pub text: bool,
    pub horizon_lines: Lin,
    pub ignore_white_space: WhiteSpace,
    pub ignore_blank_lines: bool,
    pub files_can_be_treated_as_binary: bool,
    pub ignore_case: bool,
    pub ignore_file_name_case: bool,
    pub no_dereference_symlinks: bool,
    pub file_label: [Option<Vec<u8>>; 2],
    pub function_regexp: Option<ere::Regex>,
    pub ignore_regexp: Option<ere::Regex>,
    pub brief: bool,
    pub expand_tabs: bool,
    pub tabsize: usize,
    pub initial_tab: bool,
    pub suppress_blank_empty: bool,
    pub strip_trailing_cr: bool,
    pub starting_file: Option<Vec<u8>>,
    pub paginate: bool,
    pub group_format: [Vec<u8>; 4],
    pub line_format: [Vec<u8>; 3],
    pub sdiff_merge_assist: bool,
    pub left_column: bool,
    pub suppress_common_lines: bool,
    pub sdiff_half_width: usize,
    pub sdiff_column2_offset: usize,
    pub switch_string: Vec<u8>,
    pub speed_large_files: bool,
    pub excluded: Exclude,
    pub minimal: bool,
    pub time_format: &'static [u8],
    pub zone: localtime::Zone,
    pub recursive: bool,
    pub new_file: bool,
    pub unidirectional_new_file: bool,
    pub report_identical_files: bool,
    pub no_directory: bool,
    pub presume_output_tty: bool,
}

/// Whether `re` matches anywhere in `text`: `re_search (..) >= 0`.
pub fn regex_matches(re: &ere::Regex, text: &[u8]) -> bool {
    re.is_match(text).unwrap_or(false)
}

/// The run: the options and the output side.
pub struct Diff {
    pub opts: Rc<Opts>,
    pub out: Out,
}

impl Diff {
    /// `error (0, 0, ...)`.
    pub fn error(&mut self, msg: &[u8]) {
        stdfd::diag_bytes(&[b"diff: ".as_slice(), msg, b"\n"].concat());
    }

    /// `perror_with_name`.
    pub fn perror_with_name(&mut self, name: &[u8], e: &std::io::Error) {
        let reason = coreutils::errmsg::strerror(e);
        stdfd::diag_bytes(&[b"diff: ".as_slice(), name, b": ", reason.as_bytes(), b"\n"].concat());
    }
}

/// Leave with status 2, as upstream's `die (EXIT_TROUBLE, ...)` does after
/// its message: the messages `-l` held back first, then standard output's
/// buffer, whose failure no longer matters.
pub fn exit_trouble(out: &mut Out) -> ! {
    out.print_message_queue();
    out.flush();
    stdfd::exit_now(2, 2)
}

/// One pair of names being compared, and its parent pair under `-r`.
pub struct Comparison<'a> {
    pub names: [Vec<u8>; 2],
    pub stat: [Stat; 2],
    pub nonexistent: [bool; 2],
    pub parent: Option<&'a Comparison<'a>>,
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 2)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (opts, operands, from_file, to_file, palette) = match parse_args(&args) {
        Ok(Parsed::Run(p)) => *p,
        Ok(Parsed::Help) => {
            let mut out = stdfd::Stream::stdout();
            let _ = std::io::Write::write_all(&mut out, HELP.as_bytes());
            return check_stdout(out, ExitCode::SUCCESS);
        }
        Ok(Parsed::Version) => {
            let mut out = stdfd::Stream::stdout();
            let _ = std::io::Write::write_all(&mut out, b"diff (SlateOS diffutils) 0.1.0\n");
            return check_stdout(out, ExitCode::SUCCESS);
        }
        Err(Refusal { message, referral }) => {
            if let Some(m) = message {
                stdfd::diag_bytes(&[b"diff: ".as_slice(), &m, b"\n"].concat());
            }
            if referral {
                stdfd::diag_line("diff: Try 'diff --help' for more information.");
            }
            return ExitCode::from(2);
        }
    };

    let mut d = Diff {
        opts: Rc::new(opts),
        out: Out::new(palette),
    };
    let mut exit_status = EXIT_SUCCESS;
    match (from_file, to_file) {
        (Some(_), Some(_)) => fatal(&mut d, "--from-file and --to-file both specified"),
        (Some(from), None) => {
            for op in &operands {
                let s = compare_files(&mut d, None, Some(&from), Some(op));
                exit_status = exit_status.max(s);
            }
        }
        (None, Some(to)) => {
            for op in &operands {
                let s = compare_files(&mut d, None, Some(op), Some(&to));
                exit_status = exit_status.max(s);
            }
        }
        (None, None) => {
            if operands.len() != 2 {
                let msg = if operands.len() < 2 {
                    // `argv[argc - 1]` after getopt's permutation: the last
                    // operand if there is one, else the last option word.
                    // With no arguments at all that is `argv[0]`, the name the
                    // program was run by.
                    let last = operands
                        .last()
                        .cloned()
                        .or_else(|| args.last().map(|a| os_bytes(a).into_owned()))
                        .or_else(|| {
                            std::env::args_os()
                                .next()
                                .map(|a| os_bytes(&a).into_owned())
                        })
                        .unwrap_or_default();
                    [b"missing operand after '".as_slice(), &last, b"'"].concat()
                } else {
                    [
                        b"extra operand '".as_slice(),
                        operands.get(2).map_or(&[][..], Vec::as_slice),
                        b"'",
                    ]
                    .concat()
                };
                stdfd::diag_bytes(&[b"diff: ".as_slice(), &msg, b"\n"].concat());
                stdfd::diag_line("diff: Try 'diff --help' for more information.");
                return ExitCode::from(2);
            }
            exit_status = compare_files(
                &mut d,
                None,
                operands.first().map(Vec::as_slice),
                operands.get(1).map(Vec::as_slice),
            );
        }
    }

    d.out.print_message_queue();
    let code = ExitCode::from(u8::try_from(exit_status).unwrap_or(2));
    check_stdout(d.out.into_stdout(), code)
}

/// `check_stdout`: `write failed` for an earlier failed write, `standard
/// output: REASON` for a failed final one; both exit 2.
fn check_stdout(out: stdfd::Stream, code: ExitCode) -> ExitCode {
    if out.errored() {
        if out.error().is_some_and(|e| stdfd::reader_gone(&e)) {
            return code;
        }
        stdfd::diag_line("diff: write failed");
        return ExitCode::from(2);
    }
    match out.finish() {
        Ok(()) => code,
        Err(e) if stdfd::reader_gone(&e) => code,
        Err(e) => {
            stdfd::diag_line(&format!(
                "diff: standard output: {}",
                coreutils::errmsg::strerror(&e)
            ));
            ExitCode::from(2)
        }
    }
}

/// `fatal`: a message after whatever `-l` held back, then status 2.
fn fatal(d: &mut Diff, msg: &str) -> ! {
    d.out.print_message_queue();
    stdfd::diag_line(&format!("diff: {msg}"));
    exit_trouble(&mut d.out)
}

/// A command line refused: the message (if any), and whether the `Try`
/// referral follows it.
pub struct Refusal {
    message: Option<Vec<u8>>,
    referral: bool,
}

fn try_help(message: Option<Vec<u8>>) -> Refusal {
    Refusal {
        message,
        referral: true,
    }
}

type Run = (
    Opts,
    Vec<Vec<u8>>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
);

enum Parsed {
    Help,
    Version,
    Run(Box<Run>),
}

/// Upstream's `strtoimax (optarg, &numend, 10)` followed by `*numend` being
/// the only test of the whole string: white space may lead, a sign may too,
/// and nothing may trail. `Some` is the value, saturated as `strtoimax`
/// saturates; `None` is a string with a trailing byte or no digits -- except
/// the *empty* string, which `strtoimax` answers with 0 and an `numend` that
/// already points at the terminating NUL, so it reads as zero.
fn strtoimax_whole(text: &[u8]) -> Option<i64> {
    if text.is_empty() {
        return Some(0);
    }
    let start = text
        .iter()
        .position(|&c| !io::is_space(c))
        .unwrap_or(text.len());
    let body = text.get(start..).unwrap_or_default();
    let (negative, digits) = match body.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, body),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let magnitude = digits.iter().fold(0u64, |v, &d| {
        v.saturating_mul(10)
            .saturating_add(u64::from(d.wrapping_sub(b'0')))
            .min(i64::MAX.unsigned_abs())
    });
    let value = i64::try_from(magnitude).unwrap_or(i64::MAX);
    Some(if negative {
        value.saturating_neg()
    } else {
        value
    })
}

/// Upstream's option loop and everything after it in `main` up to the
/// comparisons.
#[allow(clippy::too_many_lines)]
fn parse_args(args: &[OsString]) -> Result<Parsed, Refusal> {
    let mut output_style = OutputStyle::Unspecified;
    let mut ocontext: Lin = -1;
    let mut context: Lin = 0;
    let mut explicit_context = false;
    let mut width: usize = 0;
    let mut tabsize: usize = 0;
    let mut show_c_function = false;
    let mut from_file: Option<Vec<u8>> = None;
    let mut to_file: Option<Vec<u8>> = None;
    let mut text = false;
    let mut ignore_white_space = WhiteSpace::None;
    let mut ignore_blank_lines = false;
    let mut minimal = false;
    let mut group_format: [Option<Vec<u8>>; 4] = [None, None, None, None];
    let mut line_format: [Option<Vec<u8>>; 3] = [None, None, None];
    let mut function_res: Vec<Vec<u8>> = Vec::new();
    let mut ignore_res: Vec<Vec<u8>> = Vec::new();
    let mut speed_large_files = false;
    let mut ignore_case = false;
    let mut paginate = false;
    let mut file_label: [Option<Vec<u8>>; 2] = [None, None];
    let mut new_file = false;
    let mut unidirectional_new_file = false;
    let mut brief = false;
    let mut recursive = false;
    let mut report_identical_files = false;
    let mut starting_file: Option<Vec<u8>> = None;
    let mut expand_tabs = false;
    let mut initial_tab = false;
    let mut excluded = Exclude::default();
    let mut ignore_file_name_case = false;
    let mut horizon_lines: Lin = 0;
    let mut left_column = false;
    let mut no_dereference_symlinks = false;
    let mut sdiff_merge_assist = false;
    let mut strip_trailing_cr = false;
    let mut suppress_blank_empty = false;
    let mut suppress_common_lines = false;
    let mut colors_style = ColorsStyle::Never;
    let mut palette: Option<Vec<u8>> = None;
    let mut no_directory = false;
    let mut presume_output_tty = false;
    let mut operand_words: Vec<usize> = Vec::new();
    let mut prev_digit = false;

    fn specify_style(style: &mut OutputStyle, want: OutputStyle) -> Result<(), Refusal> {
        if *style != want {
            if *style != OutputStyle::Unspecified {
                return Err(try_help(Some(b"conflicting output style options".to_vec())));
            }
            *style = want;
        }
        Ok(())
    }
    fn specify_value(var: &mut Option<Vec<u8>>, value: &[u8], option: &str) -> Result<(), Refusal> {
        if var.as_deref().is_some_and(|v| v != value) {
            return Err(try_help(Some(
                [
                    format!("conflicting {option} option value '").as_bytes(),
                    value,
                    b"'",
                ]
                .concat(),
            )));
        }
        *var = Some(value.to_vec());
        Ok(())
    }

    for item in DIFF.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        let item = item.map_err(|e| Refusal {
            message: Some(e.sentence.into_bytes()),
            referral: true,
        })?;
        let mut this_digit = false;
        let value = |v: &Option<OsString>| {
            v.as_ref()
                .map(|s| os_bytes(s).into_owned())
                .unwrap_or_default()
        };
        match &item {
            Opt::Short(c @ b'0'..=b'9', _) => {
                let digit = Lin::from(c.saturating_sub(b'0'));
                ocontext = if !prev_digit {
                    digit
                } else if ocontext.saturating_sub(Lin::from(digit <= CONTEXT_MAX % 10))
                    < CONTEXT_MAX / 10
                {
                    ocontext.saturating_mul(10).saturating_add(digit)
                } else {
                    CONTEXT_MAX
                };
                this_digit = true;
            }
            Opt::Short(b'a', _) | Opt::Long("text", _) => text = true,
            Opt::Short(b'b', _) | Opt::Long("ignore-space-change", _) => {
                if ignore_white_space < WhiteSpace::SpaceChange {
                    ignore_white_space = WhiteSpace::SpaceChange;
                }
            }
            Opt::Short(b'Z', _) | Opt::Long("ignore-trailing-space", _) => {
                ignore_white_space = ignore_white_space.or(false, true);
            }
            Opt::Short(b'B', _) | Opt::Long("ignore-blank-lines", _) => ignore_blank_lines = true,
            Opt::Short(c @ (b'C' | b'U'), v) => {
                handle_context(
                    &mut output_style,
                    &mut context,
                    &mut explicit_context,
                    *c == b'U',
                    v.as_ref(),
                )?;
            }
            Opt::Long(name @ ("context" | "unified"), v) => {
                handle_context(
                    &mut output_style,
                    &mut context,
                    &mut explicit_context,
                    *name == "unified",
                    v.as_ref(),
                )?;
            }
            Opt::Short(b'c', _) => {
                specify_style(&mut output_style, OutputStyle::Context)?;
                context = context.max(3);
            }
            Opt::Short(b'd', _) | Opt::Long("minimal", _) => minimal = true,
            Opt::Short(b'D', v) | Opt::Long("ifdef", v) => {
                specify_style(&mut output_style, OutputStyle::Ifdef)?;
                let name = value(v);
                let formats: [&[u8]; 4] = [
                    b"%=",
                    b"#ifndef @\n%<#endif /* ! @ */\n",
                    b"#ifdef @\n%>#endif /* @ */\n",
                    b"#ifndef @\n%<#else /* @ */\n%>#endif /* @ */\n",
                ];
                for (slot, f) in group_format.iter_mut().zip(formats) {
                    let mut built = Vec::new();
                    for &b in f {
                        if b == b'@' {
                            built.extend_from_slice(&name);
                        } else {
                            built.push(b);
                        }
                    }
                    specify_value(slot, &built, "-D")?;
                }
            }
            Opt::Short(b'e', _) | Opt::Long("ed", _) => {
                specify_style(&mut output_style, OutputStyle::Ed)?;
            }
            Opt::Short(b'E', _) | Opt::Long("ignore-tab-expansion", _) => {
                ignore_white_space = ignore_white_space.or(true, false);
            }
            Opt::Short(b'f', _) | Opt::Long("forward-ed", _) => {
                specify_style(&mut output_style, OutputStyle::ForwardEd)?;
            }
            Opt::Short(b'F', v) | Opt::Long("show-function-line", v) => {
                let p = value(v);
                compile_regex(&p)?;
                function_res.push(p);
            }
            Opt::Short(b'h', _) => {}
            Opt::Short(b'H', _) | Opt::Long("speed-large-files", _) => speed_large_files = true,
            Opt::Short(b'i', _) | Opt::Long("ignore-case", _) => ignore_case = true,
            Opt::Short(b'I', v) | Opt::Long("ignore-matching-lines", v) => {
                let p = value(v);
                compile_regex(&p)?;
                ignore_res.push(p);
            }
            Opt::Short(b'l', _) | Opt::Long("paginate", _) => paginate = true,
            Opt::Short(b'L', v) | Opt::Long("label", v) => {
                let l = value(v);
                if file_label[0].is_none() {
                    file_label[0] = Some(l);
                } else if file_label[1].is_none() {
                    file_label[1] = Some(l);
                } else {
                    return Err(Refusal {
                        message: Some(b"too many file label options".to_vec()),
                        referral: false,
                    });
                }
            }
            Opt::Short(b'n', _) | Opt::Long("rcs", _) => {
                specify_style(&mut output_style, OutputStyle::Rcs)?;
            }
            Opt::Short(b'N', _) | Opt::Long("new-file", _) => new_file = true,
            Opt::Short(b'p', _) | Opt::Long("show-c-function", _) => {
                show_c_function = true;
                function_res.push(b"^[[:alpha:]$_]".to_vec());
            }
            Opt::Short(b'P', _) | Opt::Long("unidirectional-new-file", _) => {
                unidirectional_new_file = true;
            }
            Opt::Short(b'q', _) | Opt::Long("brief", _) => brief = true,
            Opt::Short(b'r', _) | Opt::Long("recursive", _) => recursive = true,
            Opt::Short(b's', _) | Opt::Long("report-identical-files", _) => {
                report_identical_files = true;
            }
            Opt::Short(b'S', v) | Opt::Long("starting-file", v) => {
                specify_value(&mut starting_file, &value(v), "-S")?;
            }
            Opt::Short(b't', _) | Opt::Long("expand-tabs", _) => expand_tabs = true,
            Opt::Short(b'T', _) | Opt::Long("initial-tab", _) => initial_tab = true,
            Opt::Short(b'u', _) => {
                specify_style(&mut output_style, OutputStyle::Unified)?;
                context = context.max(3);
            }
            Opt::Short(b'v', _) | Opt::Long("version", _) => return Ok(Parsed::Version),
            Opt::Short(b'w', _) | Opt::Long("ignore-all-space", _) => {
                ignore_white_space = WhiteSpace::AllSpace;
            }
            Opt::Short(b'x', v) | Opt::Long("exclude", v) => {
                excluded.add(&value(v), ignore_file_name_case);
            }
            Opt::Short(b'X', v) | Opt::Long("exclude-from", v) => {
                let name = value(v);
                match read_exclude_file(&name) {
                    Ok(patterns) => {
                        for p in patterns {
                            excluded.add(&p, ignore_file_name_case);
                        }
                    }
                    Err(e) => {
                        return Err(Refusal {
                            message: Some(
                                [
                                    name.as_slice(),
                                    b": ",
                                    coreutils::errmsg::strerror(&e).as_bytes(),
                                ]
                                .concat(),
                            ),
                            referral: false,
                        });
                    }
                }
            }
            Opt::Short(b'y', _) | Opt::Long("side-by-side", _) => {
                specify_style(&mut output_style, OutputStyle::Sdiff)?;
            }
            Opt::Short(b'W', v) | Opt::Long("width", v) => {
                let t = value(v);
                let n = strtoimax_whole(&t)
                    .filter(|&n| n > 0)
                    .and_then(|n| usize::try_from(n).ok());
                let Some(n) = n else {
                    return Err(try_help(Some(
                        [b"invalid width '".as_slice(), &t, b"'"].concat(),
                    )));
                };
                if width != n {
                    if width != 0 {
                        return Err(Refusal {
                            message: Some(b"conflicting width options".to_vec()),
                            referral: false,
                        });
                    }
                    width = n;
                }
            }
            Opt::Long("binary", _) => {}
            Opt::Long("from-file", v) => specify_value(&mut from_file, &value(v), "--from-file")?,
            Opt::Long("help", _) => return Ok(Parsed::Help),
            Opt::Long("horizon-lines", v) => {
                let t = value(v);
                let Some(n) = strtoimax_whole(&t).filter(|&n| n >= 0) else {
                    return Err(try_help(Some(
                        [b"invalid horizon length '".as_slice(), &t, b"'"].concat(),
                    )));
                };
                horizon_lines = horizon_lines.max(Lin::try_from(n).unwrap_or(Lin::MAX));
            }
            Opt::Long("ignore-file-name-case", _) => ignore_file_name_case = true,
            Opt::Long("inhibit-hunk-merge", _) => {}
            Opt::Long("left-column", _) => left_column = true,
            Opt::Long("line-format", v) => {
                specify_style(&mut output_style, OutputStyle::Ifdef)?;
                for slot in &mut line_format {
                    specify_value(slot, &value(v), "--line-format")?;
                }
            }
            Opt::Long("no-dereference", _) => no_dereference_symlinks = true,
            Opt::Long("no-ignore-file-name-case", _) => ignore_file_name_case = false,
            Opt::Long("normal", _) => specify_style(&mut output_style, OutputStyle::Normal)?,
            Opt::Long("sdiff-merge-assist", _) => {
                specify_style(&mut output_style, OutputStyle::Sdiff)?;
                sdiff_merge_assist = true;
            }
            Opt::Long("strip-trailing-cr", _) => strip_trailing_cr = true,
            Opt::Long("suppress-blank-empty", _) => suppress_blank_empty = true,
            Opt::Long("suppress-common-lines", _) => suppress_common_lines = true,
            Opt::Long("tabsize", v) => {
                let t = value(v);
                let n = strtoimax_whole(&t)
                    .filter(|&n| n > 0)
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|&n| n <= usize::MAX - GUTTER_WIDTH_MINIMUM);
                let Some(n) = n else {
                    return Err(try_help(Some(
                        [b"invalid tabsize '".as_slice(), &t, b"'"].concat(),
                    )));
                };
                if tabsize != n {
                    if tabsize != 0 {
                        return Err(Refusal {
                            message: Some(b"conflicting tabsize options".to_vec()),
                            referral: false,
                        });
                    }
                    tabsize = n;
                }
            }
            Opt::Long("to-file", v) => specify_value(&mut to_file, &value(v), "--to-file")?,
            Opt::Long(
                name @ ("unchanged-line-format" | "old-line-format" | "new-line-format"),
                v,
            ) => {
                specify_style(&mut output_style, OutputStyle::Ifdef)?;
                let i = match *name {
                    "unchanged-line-format" => 0,
                    "old-line-format" => 1,
                    _ => 2,
                };
                if let Some(slot) = line_format.get_mut(i) {
                    specify_value(slot, &value(v), &format!("--{name}"))?;
                }
            }
            Opt::Long(
                name @ ("unchanged-group-format"
                | "old-group-format"
                | "new-group-format"
                | "changed-group-format"),
                v,
            ) => {
                specify_style(&mut output_style, OutputStyle::Ifdef)?;
                let i = match *name {
                    "unchanged-group-format" => 0,
                    "old-group-format" => 1,
                    "new-group-format" => 2,
                    _ => 3,
                };
                if let Some(slot) = group_format.get_mut(i) {
                    specify_value(slot, &value(v), &format!("--{name}"))?;
                }
            }
            Opt::Long("color", v) => {
                colors_style = match v.as_ref().map(|s| os_bytes(s).into_owned()).as_deref() {
                    None | Some(b"auto") => ColorsStyle::Auto,
                    Some(b"always") => ColorsStyle::Always,
                    Some(b"never") => ColorsStyle::Never,
                    Some(other) => {
                        return Err(try_help(Some(
                            [b"invalid color '".as_slice(), other, b"'"].concat(),
                        )));
                    }
                };
            }
            Opt::Long("palette", v) => palette = Some(value(v)),
            Opt::Long("-no-directory", _) => no_directory = true,
            Opt::Long("-presume-output-tty", _) => presume_output_tty = true,
            Opt::Operand(word) => {
                if let Some(i) = args.iter().position(|a| std::ptr::eq(a, *word)) {
                    operand_words.push(i);
                }
            }
            Opt::Short(..) | Opt::Long(..) => {}
        }
        prev_digit = this_digit;
    }

    if colors_style == ColorsStyle::Auto && std::env::var_os("TERM").is_some_and(|t| t == "dumb") {
        colors_style = ColorsStyle::Never;
    }
    if output_style == OutputStyle::Unspecified {
        if show_c_function {
            output_style = OutputStyle::Context;
            if ocontext < 0 {
                context = 3;
            }
        } else {
            output_style = OutputStyle::Normal;
        }
    }
    let time_format: &'static [u8] = if output_style != OutputStyle::Context
        || coreutils::locale::hard_locale(coreutils::locale::Category::Time)
    {
        b"%Y-%m-%d %H:%M:%S.%N %z"
    } else {
        b"%a %b %e %T %Y"
    };
    if 0 <= ocontext
        && matches!(output_style, OutputStyle::Context | OutputStyle::Unified)
        && (context < ocontext || (ocontext < context && !explicit_context))
    {
        context = ocontext;
    }
    if tabsize == 0 {
        tabsize = 8;
    }
    if width == 0 {
        width = 130;
    }
    // The half line width, then the gutter width, as large as they can be.
    let t = if expand_tabs { 1 } else { tabsize };
    let w = width;
    let t_plus_g = t.saturating_add(GUTTER_WIDTH_MINIMUM);
    let unaligned_off = (w >> 1)
        .saturating_add(t_plus_g >> 1)
        .saturating_add(w & t_plus_g & 1);
    let off = unaligned_off.saturating_sub(unaligned_off.checked_rem(t).unwrap_or(0));
    let sdiff_half_width = if off <= GUTTER_WIDTH_MINIMUM || w <= off {
        0
    } else {
        off.saturating_sub(GUTTER_WIDTH_MINIMUM)
            .min(w.saturating_sub(off))
    };
    let sdiff_column2_offset = if sdiff_half_width != 0 { off } else { w };
    if horizon_lines < context {
        horizon_lines = context;
    }

    let function_regexp = summarize(&function_res)?;
    let ignore_regexp = summarize(&ignore_res)?;

    let mut group_format_v: [Vec<u8>; 4] = Default::default();
    let mut line_format_v: [Vec<u8>; 3] = Default::default();
    if output_style == OutputStyle::Ifdef {
        for (dst, src) in line_format_v.iter_mut().zip(line_format.iter()) {
            *dst = src.clone().unwrap_or_else(|| b"%l\n".to_vec());
        }
        let changed = group_format[3].clone();
        group_format_v[1] = group_format[1]
            .clone()
            .or_else(|| changed.clone())
            .unwrap_or_else(|| b"%<".to_vec());
        group_format_v[2] = group_format[2]
            .clone()
            .or_else(|| changed.clone())
            .unwrap_or_else(|| b"%>".to_vec());
        group_format_v[0] = group_format[0].clone().unwrap_or_else(|| b"%=".to_vec());
        group_format_v[3] =
            changed.unwrap_or_else(|| [group_format_v[1].as_slice(), &group_format_v[2]].concat());
    }
    let no_diff_means_no_output = if output_style == OutputStyle::Ifdef {
        group_format_v[0].is_empty() || (group_format_v[0] == b"%=" && line_format_v[0].is_empty())
    } else {
        output_style != OutputStyle::Sdiff || suppress_common_lines
    };
    let binary = true;
    let files_can_be_treated_as_binary = brief
        && binary
        && !(ignore_blank_lines
            || ignore_case
            || strip_trailing_cr
            || !ignore_res.is_empty()
            || ignore_white_space != WhiteSpace::None);

    // The option words, in their order: everything not an operand.
    let mut switch_string = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if operand_words.contains(&i) {
            continue;
        }
        switch_string.push(b' ');
        switch_string.extend(quoting::Style::Shell.quote_with(&os_bytes(a), b""));
    }
    let operands: Vec<Vec<u8>> = operand_words
        .iter()
        .filter_map(|&i| args.get(i))
        .map(|a| os_bytes(a).into_owned())
        .collect();

    let opts = Opts {
        output_style,
        colors_style,
        no_diff_means_no_output,
        context,
        text,
        horizon_lines,
        ignore_white_space,
        ignore_blank_lines,
        files_can_be_treated_as_binary,
        ignore_case,
        ignore_file_name_case,
        no_dereference_symlinks,
        file_label,
        function_regexp,
        ignore_regexp,
        brief,
        expand_tabs,
        tabsize,
        initial_tab,
        suppress_blank_empty,
        strip_trailing_cr,
        starting_file,
        paginate,
        group_format: group_format_v,
        line_format: line_format_v,
        sdiff_merge_assist,
        left_column,
        suppress_common_lines,
        sdiff_half_width,
        sdiff_column2_offset,
        switch_string,
        speed_large_files,
        excluded,
        minimal,
        time_format,
        zone: localtime::Zone::from_env(),
        recursive,
        new_file,
        unidirectional_new_file,
        report_identical_files,
        no_directory,
        presume_output_tty,
    };
    Ok(Parsed::Run(Box::new((
        opts, operands, from_file, to_file, palette,
    ))))
}

/// `-C`/`-U` and `--context`/`--unified`.
fn handle_context(
    style: &mut OutputStyle,
    context: &mut Lin,
    explicit: &mut bool,
    unified: bool,
    v: Option<&OsString>,
) -> Result<(), Refusal> {
    let numval = match v {
        Some(s) => {
            let t = os_bytes(s).into_owned();
            match strtoimax_whole(&t) {
                Some(n) if n >= 0 => Lin::try_from(n).unwrap_or(Lin::MAX).min(CONTEXT_MAX),
                _ => {
                    return Err(try_help(Some(
                        [b"invalid context length '".as_slice(), &t, b"'"].concat(),
                    )));
                }
            }
        }
        None => 3,
    };
    let want = if unified {
        OutputStyle::Unified
    } else {
        OutputStyle::Context
    };
    if *style != want {
        if *style != OutputStyle::Unspecified {
            return Err(try_help(Some(b"conflicting output style options".to_vec())));
        }
        *style = want;
    }
    if *context < numval {
        *context = numval;
    }
    *explicit = true;
    Ok(())
}

/// Compile one `-F`/`-I` expression, as `add_regexp` does to report it.
fn compile_regex(p: &[u8]) -> Result<ere::Regex, Refusal> {
    ere::bre::compile_syntax(p, false, ere::bre::BreSyntax::GREP).map_err(|e| Refusal {
        message: Some([p, b": ", e.message().as_bytes()].concat()),
        referral: false,
    })
}

/// `summarize_regexp_list`: the disjunction of the expressions given.
fn summarize(list: &[Vec<u8>]) -> Result<Option<ere::Regex>, Refusal> {
    if list.is_empty() {
        return Ok(None);
    }
    let joined = list.join(b"\\|".as_slice());
    compile_regex(&joined).map(Some)
}

/// `add_exclude_file`: one pattern per line.
fn read_exclude_file(name: &[u8]) -> std::io::Result<Vec<Vec<u8>>> {
    let data = if name == b"-" {
        let mut v = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut v)?;
        v
    } else {
        std::fs::read(os_from_bytes(name))?
    };
    let mut out: Vec<Vec<u8>> = data.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect();
    if data.last() == Some(&b'\n') || data.is_empty() {
        out.pop();
    }
    Ok(out)
}

/// `stat` or `lstat` by name.
fn stat_name(name: &[u8], o: &Opts) -> std::io::Result<Stat> {
    let p = os_from_bytes(name);
    let m = if o.no_dereference_symlinks {
        std::fs::symlink_metadata(p)?
    } else {
        std::fs::metadata(p)?
    };
    Ok(Stat::of(&m))
}

/// The other of a pair: 1 for 0, 0 for 1.
fn other(f: usize) -> usize {
    usize::from(f == 0)
}

/// `compare_files`: compare two files or directories, `NAME0` and `NAME1`
/// within `parent`'s under `-r`. Returns 0, 1 or 2.
// Every index into `desc`, `stat`, `names`, `errno` and `nonexistent` is 0 or
// 1, into a pair -- the two files, as upstream's `cmp.file[f]` has them.
#[allow(clippy::too_many_lines, clippy::indexing_slicing)]
pub fn compare_files(
    d: &mut Diff,
    parent: Option<&Comparison<'_>>,
    name0: Option<&[u8]>,
    name1: Option<&[u8]>,
) -> i32 {
    let o = d.opts.clone();
    if !((name0.is_some() && name1.is_some())
        || (o.unidirectional_new_file && name1.is_some())
        || o.new_file)
    {
        let name = name0.or(name1).unwrap_or_default();
        let dir = parent
            .map(|p| p.names[usize::from(name0.is_none())].clone())
            .unwrap_or_default();
        d.out.message(
            &o,
            &[b"Only in ".as_slice(), &dir, b": ", name, b"\n"].concat(),
        );
        return EXIT_FAILURE;
    }

    let mut desc = [
        if name0.is_some() {
            Desc::Unopened
        } else {
            Desc::Nonexistent
        },
        if name1.is_some() {
            Desc::Unopened
        } else {
            Desc::Nonexistent
        },
    ];
    let n0 = name0.or(name1).unwrap_or_default();
    let n1 = name1.or(name0).unwrap_or_default();
    let mut names = match parent {
        None => [n0.to_vec(), n1.to_vec()],
        Some(p) => [
            coreutils::pathname::file_name_concat(&p.names[0], n0),
            coreutils::pathname::file_name_concat(&p.names[1], n1),
        ],
    };
    let mut stat = [Stat::default(), Stat::default()];
    let mut errno: [Option<std::io::Error>; 2] = [None, None];

    for f in 0..2 {
        if matches!(desc[f], Desc::Nonexistent) {
            continue;
        }
        if f == 1 && names[1] == names[0] {
            // One name twice: the first's descriptor and status, whatever
            // they are. Standard input is the one descriptor already open.
            desc[1] = match desc[0] {
                Desc::Nonexistent => Desc::Nonexistent,
                Desc::Stdin => Desc::Shared,
                Desc::Errno(e) => Desc::Errno(e),
                _ => Desc::Unopened,
            };
            stat[1] = stat[0];
            errno[1] = errno[0].as_ref().map(clone_error);
        } else if names[f] == b"-" {
            desc[f] = Desc::Stdin;
            match stdin_stat() {
                Ok(s) => stat[f] = s,
                Err(e) => {
                    errno[f] = Some(e);
                    desc[f] = Desc::Errno(0);
                }
            }
        } else {
            match stat_name(&names[f], &o) {
                Ok(s) => stat[f] = s,
                Err(e) => {
                    errno[f] = Some(e);
                    desc[f] = Desc::Errno(0);
                }
            }
        }
    }

    // -N and -P: an inaccessible empty regular file is absent, as `patch`
    // makes them; and so is a missing top-level operand whose counterpart
    // exists.
    for f in 0..2 {
        if !(o.new_file || (f == 0 && o.unidirectional_new_file)) {
            continue;
        }
        let other_present = matches!(desc[other(f)], Desc::Unopened | Desc::Stdin);
        let absent = match &desc[f] {
            Desc::Unopened => stat[f].is_reg() && stat[f].mode & 0o777 == 0 && stat[f].size == 0,
            Desc::Errno(_) => {
                errno[f]
                    .as_ref()
                    .is_some_and(|e| matches!(e.raw_os_error(), Some(2 | 9)))
                    && parent.is_none()
                    && other_present
            }
            _ => false,
        };
        if absent {
            desc[f] = Desc::Nonexistent;
            errno[f] = None;
        }
    }
    for f in 0..2 {
        if matches!(desc[f], Desc::Nonexistent) {
            stat[f] = Stat {
                mode: stat[other(f)].mode,
                ..Stat::default()
            };
        }
    }

    let mut status = EXIT_SUCCESS;
    for f in 0..2 {
        if let Some(e) = &errno[f] {
            if matches!(desc[f], Desc::Errno(_)) {
                d.perror_with_name(&names[f], e);
                status = EXIT_TROUBLE;
            }
        }
    }

    let dir_p = |s: &Stat| s.is_dir();
    if status == EXIT_SUCCESS
        && parent.is_none()
        && !o.no_directory
        && dir_p(&stat[0]) != dir_p(&stat[1])
    {
        // A directory and a file: the file of the same name in the directory.
        let fnm_arg = usize::from(dir_p(&stat[0]));
        let dir_arg = other(fnm_arg);
        let fnm = names[fnm_arg].clone();
        let filename = dir::find_dir_file_pathname(
            &names[dir_arg],
            coreutils::pathname::last_component(&fnm),
            &o,
        );
        names[dir_arg] = filename.clone();
        if fnm == b"-" {
            fatal(d, "cannot compare '-' to a directory");
        }
        match stat_name(&filename, &o) {
            Ok(s) => stat[dir_arg] = s,
            Err(e) => {
                d.perror_with_name(&filename, &e);
                status = EXIT_TROUBLE;
            }
        }
    }

    let label = |f: usize| o.file_label[f].clone().unwrap_or_else(|| names[f].clone());
    let nonexistent = [
        matches!(desc[0], Desc::Nonexistent),
        matches!(desc[1], Desc::Nonexistent),
    ];
    let same_files = !nonexistent[0]
        && !nonexistent[1]
        && io::same_file(&stat[0], &stat[1])
        && io::same_file_attributes(&stat[0], &stat[1]);

    if status != EXIT_SUCCESS
        || (nonexistent[0] && nonexistent[1])
        || (same_files && o.no_diff_means_no_output)
    {
        // Trouble already reported, nothing to compare, or one file twice.
    } else if dir_p(&stat[0]) && dir_p(&stat[1]) {
        if o.output_style == OutputStyle::Ifdef {
            fatal(d, "-D option not supported with directories");
        }
        let cmp = Comparison {
            names: names.clone(),
            stat,
            nonexistent,
            parent,
        };
        if parent.is_some() && !o.recursive {
            d.out.message(
                &o,
                &[
                    b"Common subdirectories: ".as_slice(),
                    &names[0],
                    b" and ",
                    &names[1],
                    b"\n",
                ]
                .concat(),
            );
        } else {
            status = dir::diff_dirs(d, &cmp);
        }
    } else if dir_p(&stat[0])
        || dir_p(&stat[1])
        || (parent.is_some()
            && !((stat[0].is_reg() || stat[0].is_lnk()) && (stat[1].is_reg() || stat[1].is_lnk())))
    {
        if nonexistent[0] || nonexistent[1] {
            if (dir_p(&stat[0]) || dir_p(&stat[1]))
                && o.recursive
                && (o.new_file || (o.unidirectional_new_file && nonexistent[0]))
            {
                let cmp = Comparison {
                    names: names.clone(),
                    stat,
                    nonexistent,
                    parent,
                };
                status = dir::diff_dirs(d, &cmp);
            } else {
                let dir = parent
                    .map(|p| p.names[usize::from(nonexistent[0])].clone())
                    .unwrap_or_default();
                d.out.message(
                    &o,
                    &[b"Only in ".as_slice(), &dir, b": ", n0, b"\n"].concat(),
                );
                status = EXIT_FAILURE;
            }
        } else {
            d.out.message(
                &o,
                &[
                    b"File ".as_slice(),
                    &label(0),
                    b" is a ",
                    io::file_type(&stat[0]).as_bytes(),
                    b" while file ",
                    &label(1),
                    b" is a ",
                    io::file_type(&stat[1]).as_bytes(),
                    b"\n",
                ]
                .concat(),
            );
            status = EXIT_FAILURE;
        }
    } else if stat[0].is_lnk() || stat[1].is_lnk() {
        if stat[0].is_lnk() && stat[1].is_lnk() {
            let mut values = Vec::new();
            for name in &names {
                match std::fs::read_link(os_from_bytes(name)) {
                    Ok(v) => values.push(os_bytes(v.as_os_str()).into_owned()),
                    Err(e) => {
                        d.perror_with_name(name, &e);
                        status = EXIT_TROUBLE;
                        break;
                    }
                }
            }
            if status == EXIT_SUCCESS && values.first() != values.get(1) {
                d.out.message(
                    &o,
                    &[
                        b"Symbolic links ".as_slice(),
                        &names[0],
                        b" and ",
                        &names[1],
                        b" differ\n",
                    ]
                    .concat(),
                );
                status = EXIT_FAILURE;
            }
        } else {
            d.out.message(
                &o,
                &[
                    b"File ".as_slice(),
                    &label(0),
                    b" is a ",
                    io::file_type(&stat[0]).as_bytes(),
                    b" while file ",
                    &label(1),
                    b" is a ",
                    io::file_type(&stat[1]).as_bytes(),
                    b"\n",
                ]
                .concat(),
            );
            status = EXIT_FAILURE;
        }
    } else if o.files_can_be_treated_as_binary
        && stat[0].is_reg()
        && stat[1].is_reg()
        && stat[0].size != stat[1].size
        && 0 < stat[0].size
        && 0 < stat[1].size
    {
        d.out.message(
            &o,
            &[
                b"Files ".as_slice(),
                &label(0),
                b" and ",
                &label(1),
                b" differ\n",
            ]
            .concat(),
        );
        status = EXIT_FAILURE;
    } else {
        // Both exist and neither is a directory: open and compare.
        for f in 0..2 {
            if !matches!(desc[f], Desc::Unopened) {
                continue;
            }
            if f == 1 && same_files {
                desc[1] = Desc::Shared;
                continue;
            }
            match std::fs::File::open(os_from_bytes(&names[f])) {
                Ok(file) => desc[f] = Desc::File(file),
                Err(e) => {
                    d.perror_with_name(&names[f], &e);
                    status = EXIT_TROUBLE;
                }
            }
        }
        if status == EXIT_SUCCESS {
            let [d0, d1] = desc;
            let mut files = [
                FileData::new(d0, names[0].clone()),
                FileData::new(d1, names[1].clone()),
            ];
            files[0].stat = stat[0];
            files[1].stat = stat[1];
            status = diff_2_files(d, &mut files, parent.is_some());
            for file in files {
                if let Desc::File(f) = file.desc
                    && let Err(e) = stdfd::close(f)
                {
                    d.perror_with_name(&file.name, &e);
                    status = EXIT_TROUBLE;
                }
            }
        }
    }

    if status == EXIT_SUCCESS {
        if o.report_identical_files && !dir_p(&stat[0]) {
            d.out.message(
                &o,
                &[
                    b"Files ".as_slice(),
                    &label(0),
                    b" and ",
                    &label(1),
                    b" are identical\n",
                ]
                .concat(),
            );
        }
    } else {
        // Flush, so that the differences are seen as they are found; a
        // failure is fatal here, as upstream's `pfatal_with_name`.
        d.out.flush();
        if let Some(e) = d.out.stdout_failure() {
            if stdfd::reader_gone(&e) {
                stdfd::exit_now(u8::try_from(status).unwrap_or(2), 2);
            }
            d.out.print_message_queue();
            stdfd::diag_line(&format!(
                "diff: standard output: {}",
                coreutils::errmsg::strerror(&e)
            ));
            stdfd::exit_now(2, 2);
        }
    }
    status
}

fn clone_error(e: &std::io::Error) -> std::io::Error {
    e.raw_os_error().map_or_else(
        || std::io::Error::new(e.kind(), e.to_string()),
        std::io::Error::from_raw_os_error,
    )
}

/// `fstat (STDIN_FILENO)`, with the size reduced by the offset already read
/// and the modification time now, as POSIX asks of `-`.
fn stdin_stat() -> std::io::Result<Stat> {
    let Some(file) = coreutils::filekind::borrowed_stdin() else {
        return Err(std::io::Error::from_raw_os_error(9));
    };
    let mut s = Stat::of(&file.metadata()?);
    if s.is_reg() {
        use std::io::Seek as _;
        let pos = (&*file).stream_position()?;
        s.size = s
            .size
            .saturating_sub(i64::try_from(pos).unwrap_or(i64::MAX))
            .max(0);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    s.mtime_sec = i64::try_from(now.as_secs()).unwrap_or(i64::MAX);
    s.mtime_nsec = now.subsec_nanos();
    Ok(s)
}

/// `diff_2_files`: compare two opened files and print the differences.
// Every index into `files` and `file_label` is 0 or 1, into a pair.
#[allow(clippy::indexing_slicing)]
fn diff_2_files(d: &mut Diff, files: &mut [FileData; 2], recursive: bool) -> i32 {
    let o = d.opts.clone();
    let binary = match io::read_files(files, o.files_can_be_treated_as_binary, &o) {
        Ok(b) => b,
        Err((f, io::ReadError(e))) => {
            d.out.print_message_queue();
            d.perror_with_name(&files[f].name, &e);
            exit_trouble(&mut d.out);
        }
    };
    let label = |f: usize, files: &[FileData; 2]| {
        o.file_label[f]
            .clone()
            .unwrap_or_else(|| files[f].name.clone())
    };

    if binary {
        let changes = if files[0].stat.size != files[1].stat.size
            && 0 < files[0].stat.size
            && 0 < files[1].stat.size
            && (matches!(files[0].desc, Desc::Nonexistent) || files[0].stat.is_reg())
            && (matches!(files[1].desc, Desc::Nonexistent) || files[1].stat.is_reg())
        {
            true
        } else if matches!(files[1].desc, Desc::Shared) {
            false
        } else {
            match io::binary_differ(files) {
                Ok(c) => c,
                Err((f, io::ReadError(e))) => {
                    d.out.print_message_queue();
                    d.perror_with_name(&files[f].name, &e);
                    exit_trouble(&mut d.out);
                }
            }
        };
        if changes {
            let what: &[u8] = if o.brief { b"Files " } else { b"Binary files " };
            d.out.message(
                &o,
                &[
                    what,
                    &label(0, files),
                    b" and ",
                    &label(1, files),
                    b" differ\n",
                ]
                .concat(),
            );
        }
        return i32::from(changes);
    }

    analyze::compare(files, o.minimal, o.speed_large_files);
    let mut script = if o.output_style == OutputStyle::Ed {
        analyze::build_reverse_script(files)
    } else {
        analyze::build_script(files)
    };

    let changes = if o.ignore_blank_lines || o.ignore_regexp.is_some() {
        script
            .iter()
            .any(|c| util::analyze_hunk(std::slice::from_ref(c), files, &o).0 != 0)
    } else {
        !script.is_empty()
    };

    if o.brief {
        if changes {
            d.out.message(
                &o,
                &[
                    b"Files ".as_slice(),
                    &label(0, files),
                    b" and ",
                    &label(1, files),
                    b" differ\n",
                ]
                .concat(),
            );
        }
    } else if changes || !o.no_diff_means_no_output {
        d.out
            .setup_output(&label(0, files), &label(1, files), recursive);
        match o.output_style {
            OutputStyle::Context => {
                context::print_context_script(&mut d.out, &o, files, &mut script, false);
            }
            OutputStyle::Unified => {
                context::print_context_script(&mut d.out, &o, files, &mut script, true);
            }
            OutputStyle::Ed => normal::print_ed_script(&mut d.out, &o, files, &script),
            OutputStyle::ForwardEd => normal::pr_forward_ed_script(&mut d.out, &o, files, &script),
            OutputStyle::Rcs => normal::print_rcs_script(&mut d.out, &o, files, &script),
            OutputStyle::Ifdef => ifdef::print_ifdef_script(&mut d.out, &o, files, &script),
            OutputStyle::Sdiff => side::print_sdiff_script(&mut d.out, &o, files, &script),
            OutputStyle::Normal | OutputStyle::Unspecified => {
                normal::print_normal_script(&mut d.out, &o, files, &script);
            }
        }
        d.out.finish_output();
    }

    let mut status = i32::from(changes);
    if !io::robust_output_style(o.output_style) {
        for f in 0..2 {
            if files[f].missing_newline {
                d.error(&[label(f, files).as_slice(), b": No newline at end of file\n"].concat());
                status = EXIT_TROUBLE;
            }
        }
    }
    status
}

/// GNU's `--help`, minus the bug-reporting block.
const HELP: &str = "\
Usage: diff [OPTION]... FILES
Compare FILES line by line.

Mandatory arguments to long options are mandatory for short options too.
      --normal                  output a normal diff (the default)
  -q, --brief                   report only when files differ
  -s, --report-identical-files  report when two files are the same
  -c, -C NUM, --context[=NUM]   output NUM (default 3) lines of copied context
  -u, -U NUM, --unified[=NUM]   output NUM (default 3) lines of unified context
  -e, --ed                      output an ed script
  -n, --rcs                     output an RCS format diff
  -y, --side-by-side            output in two columns
  -W, --width=NUM               output at most NUM (default 130) print columns
      --left-column             output only the left column of common lines
      --suppress-common-lines   do not output common lines

  -p, --show-c-function         show which C function each change is in
  -F, --show-function-line=RE   show the most recent line matching RE
      --label LABEL             use LABEL instead of file name and timestamp
                                  (can be repeated)

  -t, --expand-tabs             expand tabs to spaces in output
  -T, --initial-tab             make tabs line up by prepending a tab
      --tabsize=NUM             tab stops every NUM (default 8) print columns
      --suppress-blank-empty    suppress space or tab before empty output lines
  -l, --paginate                pass output through 'pr' to paginate it

  -r, --recursive                 recursively compare any subdirectories found
      --no-dereference            don't follow symbolic links
  -N, --new-file                  treat absent files as empty
      --unidirectional-new-file   treat absent first files as empty
      --ignore-file-name-case     ignore case when comparing file names
      --no-ignore-file-name-case  consider case when comparing file names
  -x, --exclude=PAT               exclude files that match PAT
  -X, --exclude-from=FILE         exclude files that match any pattern in FILE
  -S, --starting-file=FILE        start with FILE when comparing directories
      --from-file=FILE1           compare FILE1 to all operands;
                                    FILE1 can be a directory
      --to-file=FILE2             compare all operands to FILE2;
                                    FILE2 can be a directory

  -i, --ignore-case               ignore case differences in file contents
  -E, --ignore-tab-expansion      ignore changes due to tab expansion
  -Z, --ignore-trailing-space     ignore white space at line end
  -b, --ignore-space-change       ignore changes in the amount of white space
  -w, --ignore-all-space          ignore all white space
  -B, --ignore-blank-lines        ignore changes where lines are all blank
  -I, --ignore-matching-lines=RE  ignore changes where all lines match RE

  -a, --text                      treat all files as text
      --strip-trailing-cr         strip trailing carriage return on input

  -D, --ifdef=NAME                output merged file with '#ifdef NAME' diffs
      --GTYPE-group-format=GFMT   format GTYPE input groups with GFMT
      --line-format=LFMT          format all input lines with LFMT
      --LTYPE-line-format=LFMT    format LTYPE input lines with LFMT
    These format options provide fine-grained control over the output
      of diff, generalizing -D/--ifdef.
    LTYPE is 'old', 'new', or 'unchanged'.  GTYPE is LTYPE or 'changed'.
    GFMT (only) may contain:
      %<  lines from FILE1
      %>  lines from FILE2
      %=  lines common to FILE1 and FILE2
      %[-][WIDTH][.[PREC]]{doxX}LETTER  printf-style spec for LETTER
        LETTERs are as follows for new group, lower case for old group:
          F  first line number
          L  last line number
          N  number of lines = L-F+1
          E  F-1
          M  L+1
      %(A=B?T:E)  if A equals B then T else E
    LFMT (only) may contain:
      %L  contents of line
      %l  contents of line, excluding any trailing newline
      %[-][WIDTH][.[PREC]]{doxX}n  printf-style spec for input line number
    Both GFMT and LFMT may contain:
      %%  %
      %c'C'  the single character C
      %c'\\OOO'  the character with octal code OOO
      C    the character C (other characters represent themselves)

  -d, --minimal            try hard to find a smaller set of changes
      --horizon-lines=NUM  keep NUM lines of the common prefix and suffix
      --speed-large-files  assume large files and many scattered small changes
      --color[=WHEN]       color output; WHEN is 'never', 'always', or 'auto';
                             plain --color means --color='auto'
      --palette=PALETTE    the colors to use when --color is active; PALETTE is
                             a colon-separated list of terminfo capabilities

      --help               display this help and exit
  -v, --version            output version information and exit

FILES are 'FILE1 FILE2' or 'DIR1 DIR2' or 'DIR FILE' or 'FILE DIR'.
If --from-file or --to-file is given, there are no restrictions on FILE(s).
If a FILE is '-', read standard input.
Exit status is 0 if inputs are the same, 1 if different, 2 if trouble.
";
