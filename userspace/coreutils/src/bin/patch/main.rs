//! Apply a diff to the files it names: GNU patch 2.7.6 as Ubuntu ships it, ported.
//!
//! The program is upstream's, file for file: `patch.c` here, the parser
//! (`pch.c`) in [`pch`], the input file (`inp.c`) in [`inp`], the helpers
//! (`util.c`) in [`util`], the symlink-safe path walk (`safe.c`) in [`safe`],
//! `--merge` (`merge.c`, with gnulib's `diffseq.h` and patch's
//! `bestmatch.h`) in [`merge`], and the backup names (gnulib's
//! `backupfile.c`, as patch bundles it) in [`backupfile`]. Each keeps its
//! upstream's names, so a difference can be traced to the line it came from.
//! Every global of upstream's is a field of [`Ctx`], passed where upstream
//! reached for the global.
//!
//! It is the patch Ubuntu ships -- 2.7.6 with Debian 2.7.6-7's fifteen
//! patches, every one of which is carried here: `-m` for `--merge`; the ed
//! script written to a temporary file and handed to `ed` on its standard
//! input rather than piped through a shell (CVE-2018-1000156), with a missing
//! input file allowed; no symlink followed when a file is opened unless
//! `--follow-symlinks` (CVE-2019-13636); a `cleanup` that runs once; no cap
//! on the directory cache under an unlimited `RLIMIT_NOFILE`; and the fixes
//! for a mangled rename, a `---` at the start of a context hunk, and `-o`
//! after a file that ended without a newline. Measured by
//! `scripts/patch-diff.sh` against Ubuntu's `patch`.
//!
//! # Deliberately different
//!
//! - `-v`/`--version` names this build.
//! - Rust stops on an allocation that fails rather than returning null, so
//!   upstream's "Ran out of memory using Plan A -- trying again" path, and
//!   the hunk-swap and `savebuf` failures it guards, cannot arise. Plan B --
//!   the input file kept in a temporary file in fixed-size records -- is
//!   still what `-x 16` asks for, as upstream.
//! - When a signal ends `patch`, its temporary files are removed and the
//!   signal raised again, as upstream; the patched files a git-style diff had
//!   queued are not put in place first. Upstream does that from inside the
//!   signal handler, through calls that are not safe there.
//! - The patch is read whole when it is opened. Upstream reads it through a
//!   stream as it goes, so a patch that rewrites its own file part-way through
//!   would read some of its later text after the rewrite.
//! - Two places where upstream's C is undefined get a defined answer. Plan B
//!   keeps a final unterminated line that is longer than its record, where
//!   upstream writes past its buffer ([`inp`]). And the merge search never
//!   gives up early, where upstream compares against a cost limit it never
//!   set ([`merge`]); 1,000 random merges agree with Ubuntu's binary.
//! - A hunk line of no bytes -- an empty last line with no newline -- is a
//!   write error, as upstream's `fwrite` of nothing makes it, reported with
//!   `errno` 0 (`Success`). That is what Ubuntu's binary prints in every case
//!   measured; upstream prints whatever an earlier call left in `errno`.
//! - **What cannot be written to standard output is reported.** Upstream
//!   checks no write there and has no `close_stdout`, so `patch < d >
//!   /dev/full` exits 0 having said nothing anywhere. Here the run ends as a
//!   GNU program that registers `close_stdout` ends: `patch: write error: No
//!   space left on device`, status 2, `patch`'s trouble ([`Ctx::exit`];
//!   design-decisions §1071, the operator's answer to B-Q25).

mod backupfile;
mod diffseq;
mod inp;
mod merge;
mod pch;
mod safe;
mod sys;
mod util;

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd;

use crate::backupfile::BackupType;
use crate::inp::Inp;
use crate::pch::Pch;
use crate::safe::Safe;
use crate::sys::{Stat, Timespec, errno};

coreutils::guard_std_fds!();

/// `lin`: a line number, signed.
pub type Lin = i64;

/// `enum diff`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Diff {
    No,
    Context,
    Normal,
    Ed,
    NewContext,
    Uni,
    GitBinary,
}

/// `verbosity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verbosity {
    Default,
    Silent,
    Verbose,
}

/// `enum conflict_style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictStyle {
    Merge,
    Diff3,
}

/// `read_only_behavior`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnly {
    Ignore,
    Warn,
    Fail,
}

/// The temporary files, by their slot in the registry a fatal signal reads.
pub const TMP_IN: usize = 0;
pub const TMP_OUT: usize = 1;
pub const TMP_PAT: usize = 2;
pub const TMP_REJ: usize = 3;
pub const TMP_ED: usize = 4;

/// `enum nametype`.
pub const OLD: usize = 0;
pub const NEW: usize = 1;
pub const INDEX: usize = 2;
pub const NONE: usize = 3;

/// `enum file_id_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileIdType {
    Unknown,
    Created,
    DeleteLater,
    Overwritten,
}

/// One entry of `file_id_table`.
#[derive(Clone, Copy, Debug)]
pub struct FileId {
    pub kind: FileIdType,
    pub queued_output: bool,
}

/// A temporary file's name and whether cleanup must remove it:
/// `TMPxxxNAME` and `TMPxxxNAME_needs_removal`.
#[derive(Clone, Debug, Default)]
pub struct Temp {
    pub name: Option<Vec<u8>>,
    pub needs_removal: bool,
}

/// `struct file_to_delete`.
#[derive(Clone, Debug)]
pub struct FileToDelete {
    pub name: Vec<u8>,
    pub st: Stat,
    pub backup: bool,
}

/// `struct file_to_output`.
#[derive(Clone, Debug)]
pub struct FileToOutput {
    pub from: Vec<u8>,
    pub from_st: Stat,
    pub to: Option<Vec<u8>>,
    pub mode: u32,
    pub backup: bool,
}

/// Where the patched file goes: a temporary file, `-o`'s file, or standard
/// output.
pub struct Output {
    pub w: std::io::BufWriter<std::fs::File>,
    pub fd: i32,
}

/// `struct outstate`.
pub struct OutState {
    pub ofp: Option<Output>,
    pub after_newline: bool,
    pub zero_output: bool,
}

impl OutState {
    /// `init_output`.
    pub fn new() -> Self {
        Self {
            ofp: None,
            after_newline: true,
            zero_output: true,
        }
    }
}

impl Default for OutState {
    fn default() -> Self {
        Self::new()
    }
}

/// Every global of upstream's `patch`.
pub struct Ctx {
    /// `program_name`: `argv[0]` as given.
    pub program_name: Vec<u8>,
    /// `buf`: the patch line just read, or the answer just typed.
    pub buf: Vec<u8>,
    /// `bufsize`: how big upstream's `buf` has grown -- by the longest patch
    /// line yet -- which is how much `ask` reads from the terminal at once.
    pub bufsize: usize,
    pub using_plan_a: bool,
    pub inname: Option<Vec<u8>>,
    pub outfile: Option<Vec<u8>>,
    pub inerrno: i32,
    pub invc: i32,
    pub instat: Stat,
    pub dry_run: bool,
    pub posixly_correct: bool,
    pub origprae: Option<Vec<u8>>,
    pub origbase: Option<Vec<u8>>,
    pub origsuff: Option<Vec<u8>>,
    pub tmpin: Temp,
    pub tmpout: Temp,
    pub tmppat: Temp,
    pub tmprej: Temp,
    /// `TMPEDNAME`: the ed script, written to a file for `ed` to read
    /// (Debian's fix for CVE-2018-1000156).
    pub tmped: Temp,
    pub debug: i32,
    pub force: bool,
    pub batch: bool,
    pub noreverse: bool,
    pub reverse: bool,
    pub verbosity: Verbosity,
    pub skip_rest_of_patch: bool,
    pub strippath: i32,
    pub canonicalize_ws: bool,
    pub patch_get: i32,
    pub set_time: bool,
    pub set_utc: bool,
    pub follow_symlinks: bool,
    pub diff_type: Diff,
    pub revision: Option<Vec<u8>>,
    pub no_strip_trailing_cr: bool,
    pub in_offset: Lin,
    pub out_offset: Lin,
    pub last_frozen_line: Lin,
    pub conflict_style: ConflictStyle,
    pub backup_type: BackupType,
    pub simple_backup_suffix: Vec<u8>,
    pub file_ids: HashMap<(u64, u64), FileId>,
    pub initial_time: Timespec,
    pub effective_ids: Option<(u32, u32)>,
    pub ttyfd: i32,
    pub quoting: quoting::Style,
    pub input_lines: Lin,
    pub inp: Inp,
    pub pch: Pch,
    pub safe: Safe,
    pub merge: bool,
    pub reject_format: Diff,
    pub make_backups: bool,
    pub backup_if_mismatch: bool,
    pub version_control: Option<Vec<u8>>,
    pub version_control_context: &'static str,
    pub remove_empty_files: bool,
    pub explicit_inname: bool,
    pub read_only_behavior: ReadOnly,
    pub reverse_flag_specified: bool,
    pub do_defines: Option<Vec<u8>>,
    pub rejfp: Option<std::io::BufWriter<std::fs::File>>,
    pub patchname: Option<Vec<u8>>,
    pub rejname: Option<Vec<u8>>,
    pub maxfuzz: Lin,
    pub files_to_delete: Vec<FileToDelete>,
    pub files_to_output: Vec<FileToOutput>,
    /// `merge_result`'s `last_what`.
    pub last_what: Option<&'static str>,
    /// Inside `cleanup` already: a failure there must not start it again.
    pub in_cleanup: bool,
}

/// The parser. Every complaint is `usage (stderr, 2)`.
const PATCH: Program = Program::new("patch", 2);

/// `shortopts`.
const SHORTOPTS: &str = "bB:cd:D:eEfF:g:i:lmnNo:p:r:RstTuvV:x:Y:z:Z";

/// `longopts`, in its order: abbreviations resolve against it.
const LONGOPTS: &[(&str, Takes)] = &[
    ("backup", Takes::Nothing),
    ("prefix", Takes::Required),
    ("context", Takes::Nothing),
    ("directory", Takes::Required),
    ("ifdef", Takes::Required),
    ("ed", Takes::Nothing),
    ("remove-empty-files", Takes::Nothing),
    ("force", Takes::Nothing),
    ("fuzz", Takes::Required),
    ("get", Takes::Required),
    ("input", Takes::Required),
    ("ignore-whitespace", Takes::Nothing),
    ("merge", Takes::Optional),
    ("normal", Takes::Nothing),
    ("forward", Takes::Nothing),
    ("output", Takes::Required),
    ("strip", Takes::Required),
    ("reject-file", Takes::Required),
    ("reverse", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("batch", Takes::Nothing),
    ("set-time", Takes::Nothing),
    ("unified", Takes::Nothing),
    ("version", Takes::Nothing),
    ("version-control", Takes::Required),
    ("debug", Takes::Required),
    ("basename-prefix", Takes::Required),
    ("suffix", Takes::Required),
    ("set-utc", Takes::Nothing),
    ("dry-run", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("binary", Takes::Nothing),
    ("help", Takes::Nothing),
    ("backup-if-mismatch", Takes::Nothing),
    ("no-backup-if-mismatch", Takes::Nothing),
    ("posix", Takes::Nothing),
    ("quoting-style", Takes::Required),
    ("reject-format", Takes::Required),
    ("read-only", Takes::Required),
    ("follow-symlinks", Takes::Nothing),
];

/// `option_help`.
const OPTION_HELP: &str = "\
Input options:

  -p NUM  --strip=NUM  Strip NUM leading components from file names.
  -F LINES  --fuzz LINES  Set the fuzz factor to LINES for inexact matching.
  -l  --ignore-whitespace  Ignore white space changes between patch and input.

  -c  --context  Interpret the patch as a context difference.
  -e  --ed  Interpret the patch as an ed script.
  -n  --normal  Interpret the patch as a normal difference.
  -u  --unified  Interpret the patch as a unified difference.

  -N  --forward  Ignore patches that appear to be reversed or already applied.
  -R  --reverse  Assume patches were created with old and new files swapped.

  -i PATCHFILE  --input=PATCHFILE  Read patch from PATCHFILE instead of stdin.

Output options:

  -o FILE  --output=FILE  Output patched files to FILE.
  -r FILE  --reject-file=FILE  Output rejects to FILE.

  -D NAME  --ifdef=NAME  Make merged if-then-else output using NAME.
  --merge  Merge using conflict markers instead of creating reject files.
  -E  --remove-empty-files  Remove output files that are empty after patching.

  -Z  --set-utc  Set times of patched files, assuming diff uses UTC (GMT).
  -T  --set-time  Likewise, assuming local time.

  --quoting-style=WORD   output file names using quoting style WORD.
    Valid WORDs are: literal, shell, shell-always, c, escape.
    Default is taken from QUOTING_STYLE env variable, or 'shell' if unset.

Backup and version control options:

  -b  --backup  Back up the original contents of each file.
  --backup-if-mismatch  Back up if the patch does not match exactly.
  --no-backup-if-mismatch  Back up mismatches only if otherwise requested.

  -V STYLE  --version-control=STYLE  Use STYLE version control.
\tSTYLE is either 'simple', 'numbered', or 'existing'.
  -B PREFIX  --prefix=PREFIX  Prepend PREFIX to backup file names.
  -Y PREFIX  --basename-prefix=PREFIX  Prepend PREFIX to backup file basenames.
  -z SUFFIX  --suffix=SUFFIX  Append SUFFIX to backup file names.

  -g NUM  --get=NUM  Get files from RCS etc. if positive; ask if negative.

Miscellaneous options:

  -t  --batch  Ask no questions; skip bad-Prereq patches; assume reversed.
  -f  --force  Like -t, but ignore bad-Prereq patches, and assume unreversed.
  -s  --quiet  --silent  Work silently unless an error occurs.
  --verbose  Output extra information about the work being done.
  --dry-run  Do not actually change any files; just print what would happen.
  --posix  Conform to the POSIX standard.

  -d DIR  --directory=DIR  Change the working directory to DIR first.
  --reject-format=FORMAT  Create 'context' or 'unified' rejects.
  --binary  Read and write data in binary mode.
  --read-only=BEHAVIOR  How to handle read-only input files: 'ignore' that they
                        are read-only, 'warn' (default), or 'fail'.

  -v  --version  Output version info.
  --help  Output this help.

Report bugs to <bug-patch@gnu.org>.
";

/// `if_defined`, `not_defined`, `else_defined`, `end_defined`: each begins
/// with a newline that is left off when the output already ends one
/// (`outstate->after_newline + ...`).
const IF_DEFINED: &[u8] = b"\n#ifdef ";
const NOT_DEFINED: &[u8] = b"\n#ifndef ";
const ELSE_DEFINED: &[u8] = b"\n#else\n";
const END_DEFINED: &[u8] = b"\n#endif\n";

/// `after_newline + s`: `s` without its first byte when the output is at the
/// start of a line.
fn skip_nl(after_newline: bool, s: &[u8]) -> &[u8] {
    if after_newline {
        s.get(1..).unwrap_or_default()
    } else {
        s
    }
}

/// `"s" + (n == 1)`: the plural ending.
fn plural(n: impl PartialEq<i64> + Copy) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `format_linenum`.
pub fn lin(n: Lin) -> String {
    n.to_string()
}

// Line numbers are upstream's `lin`, and so is their arithmetic: offsets
// and counts bounded by the hunk and the file, as the C bounds them.
#[allow(clippy::arithmetic_side_effects)]
impl Ctx {
    fn new(program_name: Vec<u8>) -> Self {
        Self {
            program_name,
            buf: Vec::with_capacity(pch::INITIAL_BUFSIZE),
            bufsize: pch::INITIAL_BUFSIZE,
            using_plan_a: false,
            inname: None,
            outfile: None,
            inerrno: 0,
            invc: 0,
            instat: Stat::default(),
            dry_run: false,
            posixly_correct: false,
            origprae: None,
            origbase: None,
            origsuff: None,
            tmpin: Temp::default(),
            tmpout: Temp::default(),
            tmppat: Temp::default(),
            tmprej: Temp::default(),
            tmped: Temp::default(),
            debug: 0,
            force: false,
            batch: false,
            noreverse: false,
            reverse: false,
            verbosity: Verbosity::Default,
            skip_rest_of_patch: false,
            strippath: -1,
            canonicalize_ws: false,
            patch_get: 0,
            set_time: false,
            set_utc: false,
            follow_symlinks: false,
            diff_type: Diff::No,
            revision: None,
            no_strip_trailing_cr: false,
            in_offset: 0,
            out_offset: 0,
            last_frozen_line: 0,
            conflict_style: ConflictStyle::Merge,
            backup_type: BackupType::NoBackups,
            simple_backup_suffix: b".orig".to_vec(),
            file_ids: HashMap::new(),
            initial_time: Timespec::default(),
            effective_ids: None,
            ttyfd: -2,
            quoting: quoting::Style::Shell,
            input_lines: 0,
            inp: Inp::default(),
            pch: Pch::default(),
            safe: Safe::new(),
            merge: false,
            reject_format: Diff::No,
            make_backups: false,
            backup_if_mismatch: false,
            version_control: None,
            version_control_context: "",
            remove_empty_files: false,
            explicit_inname: false,
            read_only_behavior: ReadOnly::Warn,
            reverse_flag_specified: false,
            do_defines: None,
            rejfp: None,
            patchname: None,
            rejname: None,
            maxfuzz: 2,
            files_to_delete: Vec::new(),
            files_to_output: Vec::new(),
            last_what: None,
            in_cleanup: false,
        }
    }

    /// `quotearg (name)`, in the C locale: upstream never calls
    /// `setlocale`, so a byte above 0177 is unprintable to it whatever the
    /// environment says.
    pub fn q(&self, name: &[u8]) -> Vec<u8> {
        self.quoting.quote_in(name, quoting::Charset::Ascii)
    }

    /// `usage (stream, status)`.
    fn usage(&mut self, to_stdout: bool, status: i32) -> ! {
        if status != 0 {
            let mut m = self.program_name.clone();
            m.extend_from_slice(b": Try '");
            m.extend_from_slice(&self.program_name);
            m.extend_from_slice(b" --help' for more information.\n");
            util::print_stderr(&m);
        } else {
            let mut m = b"Usage: ".to_vec();
            m.extend_from_slice(&self.program_name);
            m.extend_from_slice(b" [OPTION]... [ORIGFILE [PATCHFILE]]\n\n");
            m.extend_from_slice(OPTION_HELP.as_bytes());
            if to_stdout {
                util::print_stdout(&m);
            } else {
                util::print_stderr(&m);
            }
        }
        self.exit(status);
    }

    /// `numeric_string`.
    fn numeric_string(&mut self, string: &[u8], negative_allowed: bool, what: &str) -> i32 {
        let mut value: i32 = 0;
        let sign: i32 = if string.first() == Some(&b'-') { -1 } else { 1 };
        let mut p = usize::from(matches!(string.first(), Some(b'-' | b'+')));
        loop {
            let c = string.get(p).copied().unwrap_or(0);
            let digit = i32::from(c).wrapping_sub(i32::from(b'0'));
            if !(0..=9).contains(&digit) {
                let mut m = format!("{what} ").into_bytes();
                m.extend_from_slice(&self.q(string));
                m.extend_from_slice(b" is not a number");
                self.fatal(&m);
            }
            let next = value
                .checked_mul(10)
                .and_then(|v| v.checked_add(sign.wrapping_mul(digit)));
            let Some(next) = next else {
                let mut m = format!("{what} ").into_bytes();
                m.extend_from_slice(&self.q(string));
                m.extend_from_slice(b" is too large");
                self.fatal(&m);
            };
            value = next;
            p = p.saturating_add(1);
            if p >= string.len() {
                break;
            }
        }
        if value < 0 && !negative_allowed {
            let mut m = format!("{what} ").into_bytes();
            m.extend_from_slice(&self.q(string));
            m.extend_from_slice(b" is negative");
            self.fatal(&m);
        }
        value
    }

    /// `get_some_switches`.
    fn get_some_switches(&mut self, argv: &[OsString]) {
        self.rejname = None;
        let words = argv.get(1..).unwrap_or_default();
        if words.is_empty() {
            return;
        }
        let mut operands: Vec<Vec<u8>> = Vec::new();
        let mut parser = PATCH.parse(words, SHORTOPTS, LONGOPTS);
        while let Some(item) = parser.next() {
            let opt = match item {
                Ok(o) => o,
                Err(e) => {
                    let mut m = self.program_name.clone();
                    m.extend_from_slice(b": ");
                    m.extend_from_slice(e.sentence.as_bytes());
                    m.push(b'\n');
                    util::print_stderr(&m);
                    self.usage(false, 2);
                }
            };
            let arg = |v: &Option<OsString>| {
                v.as_ref()
                    .map(|v| os_bytes(v).into_owned())
                    .unwrap_or_default()
            };
            match opt {
                Opt::Short(b'b', _) | Opt::Long("backup", _) => {
                    self.make_backups = true;
                    // The backward-compatibility hack for CVS 1.9: '-b SUFFIX
                    // ORIGFILE PATCHFILE', the -b as its own word and the
                    // last three words, none an option, is '-b -z SUFFIX'.
                    let at = parser.optind();
                    let rest = words.get(at..).unwrap_or_default();
                    let is_dash_b = at
                        .checked_sub(1)
                        .and_then(|i| words.get(i))
                        .is_some_and(|w| os_bytes(w).as_ref() == b"-b");
                    let not_opt = |w: &OsString| {
                        let b = os_bytes(w);
                        !(b.first() == Some(&b'-') && b.len() > 1)
                    };
                    if rest.len() == 3
                        && is_dash_b
                        && rest.iter().all(not_opt)
                        && let Some(word) = parser.take_word()
                    {
                        let suffix = os_bytes(&word).into_owned();
                        if self.verbosity != Verbosity::Silent {
                            let mut m = b"warning: the '-b ".to_vec();
                            m.extend_from_slice(&suffix);
                            m.extend_from_slice(b"' option is obsolete; use '-b -z ");
                            m.extend_from_slice(&suffix);
                            m.extend_from_slice(b"' instead\n");
                            util::say(&m);
                        }
                        self.set_suffix(&suffix);
                    }
                }
                Opt::Short(b'B', v) | Opt::Long("prefix", v) => {
                    let v = arg(&v);
                    if v.is_empty() {
                        self.fatal(b"backup prefix is empty");
                    }
                    self.origprae = Some(v);
                }
                Opt::Short(b'c', _) | Opt::Long("context", _) => self.diff_type = Diff::Context,
                Opt::Short(b'd', v) | Opt::Long("directory", v) => {
                    let v = arg(&v);
                    if let Err(e) = std::env::set_current_dir(quoting::os_from_bytes(&v)) {
                        let mut m = b"Can't change to directory ".to_vec();
                        m.extend_from_slice(&self.q(&v));
                        self.pfatal(&m, e.raw_os_error().unwrap_or(errno::ENOENT));
                    }
                }
                Opt::Short(b'D', v) | Opt::Long("ifdef", v) => self.do_defines = Some(arg(&v)),
                Opt::Short(b'e', _) | Opt::Long("ed", _) => self.diff_type = Diff::Ed,
                Opt::Short(b'E', _) | Opt::Long("remove-empty-files", _) => {
                    self.remove_empty_files = true;
                }
                Opt::Short(b'f', _) | Opt::Long("force", _) => self.force = true,
                Opt::Short(b'F', v) | Opt::Long("fuzz", v) => {
                    self.maxfuzz = Lin::from(self.numeric_string(&arg(&v), false, "fuzz factor"));
                }
                Opt::Short(b'g', v) | Opt::Long("get", v) => {
                    self.patch_get = self.numeric_string(&arg(&v), true, "get option value");
                }
                Opt::Short(b'i', v) | Opt::Long("input", v) => self.patchname = Some(arg(&v)),
                Opt::Short(b'l', _) | Opt::Long("ignore-whitespace", _) => {
                    self.canonicalize_ws = true;
                }
                Opt::Short(b'm', v) | Opt::Long("merge", v) => {
                    // Debian's `m-merge`: `-m` is `--merge`, with no value.
                    self.merge = true;
                    match v.as_ref().map(|v| os_bytes(v).into_owned()) {
                        None => self.conflict_style = ConflictStyle::Merge,
                        Some(s) if s == b"merge" => self.conflict_style = ConflictStyle::Merge,
                        Some(s) if s == b"diff3" => self.conflict_style = ConflictStyle::Diff3,
                        Some(_) => self.usage(false, 2),
                    }
                }
                Opt::Short(b'n', _) | Opt::Long("normal", _) => self.diff_type = Diff::Normal,
                Opt::Short(b'N', _) | Opt::Long("forward", _) => self.noreverse = true,
                Opt::Short(b'o', v) | Opt::Long("output", v) => self.outfile = Some(arg(&v)),
                Opt::Short(b'p', v) | Opt::Long("strip", v) => {
                    self.strippath = self.numeric_string(&arg(&v), false, "strip count");
                }
                Opt::Short(b'r', v) | Opt::Long("reject-file", v) => self.rejname = Some(arg(&v)),
                Opt::Short(b'R', _) | Opt::Long("reverse", _) => {
                    self.reverse = true;
                    self.reverse_flag_specified = true;
                }
                Opt::Short(b's', _) | Opt::Long("quiet" | "silent", _) => {
                    self.verbosity = Verbosity::Silent;
                }
                Opt::Short(b't', _) | Opt::Long("batch", _) => self.batch = true,
                Opt::Short(b'T', _) | Opt::Long("set-time", _) => self.set_time = true,
                Opt::Short(b'u', _) | Opt::Long("unified", _) => self.diff_type = Diff::Uni,
                Opt::Short(b'v', _) | Opt::Long("version", _) => {
                    util::print_stdout(b"patch (SlateOS coreutils) 0.1.0\n");
                    self.exit(0);
                }
                Opt::Short(b'V', v) | Opt::Long("version-control", v) => {
                    self.version_control = Some(arg(&v));
                    self.version_control_context = "--version-control or -V option";
                }
                Opt::Short(b'x', v) | Opt::Long("debug", v) => {
                    self.debug = self.numeric_string(&arg(&v), true, "debugging option");
                }
                Opt::Short(b'Y', v) | Opt::Long("basename-prefix", v) => {
                    let v = arg(&v);
                    if v.is_empty() {
                        self.fatal(b"backup basename prefix is empty");
                    }
                    self.origbase = Some(v);
                }
                Opt::Short(b'z', v) | Opt::Long("suffix", v) => self.set_suffix(&arg(&v)),
                Opt::Short(b'Z', _) | Opt::Long("set-utc", _) => self.set_utc = true,
                Opt::Long("dry-run", _) => self.dry_run = true,
                Opt::Long("verbose", _) => self.verbosity = Verbosity::Verbose,
                Opt::Long("binary", _) => self.no_strip_trailing_cr = true,
                Opt::Long("help", _) => self.usage(true, 0),
                Opt::Long("backup-if-mismatch", _) => self.backup_if_mismatch = true,
                Opt::Long("no-backup-if-mismatch", _) => self.backup_if_mismatch = false,
                Opt::Long("posix", _) => self.posixly_correct = true,
                Opt::Long("quoting-style", v) => {
                    let v = arg(&v);
                    match util::quoting_style(&v) {
                        Ok(s) => self.quoting = s,
                        Err(ambiguous) => {
                            let name = self.program_name.clone();
                            util::invalid_arg(&name, "quoting style", &v, ambiguous);
                            self.usage(false, 2);
                        }
                    }
                }
                Opt::Long("reject-format", v) => match arg(&v).as_slice() {
                    b"context" => self.reject_format = Diff::NewContext,
                    b"unified" => self.reject_format = Diff::Uni,
                    _ => self.usage(false, 2),
                },
                Opt::Long("read-only", v) => match arg(&v).as_slice() {
                    b"ignore" => self.read_only_behavior = ReadOnly::Ignore,
                    b"warn" => self.read_only_behavior = ReadOnly::Warn,
                    b"fail" => self.read_only_behavior = ReadOnly::Fail,
                    _ => self.usage(false, 2),
                },
                Opt::Long("follow-symlinks", _) => self.follow_symlinks = true,
                Opt::Operand(o) => operands.push(os_bytes(o).into_owned()),
                Opt::Short(..) | Opt::Long(..) => self.usage(false, 2),
            }
        }

        // Process any filename args.
        let mut ops = operands.into_iter();
        if let Some(first) = ops.next() {
            self.inname = Some(first);
            self.explicit_inname = true;
            self.invc = -1;
            if let Some(second) = ops.next() {
                self.patchname = Some(second);
                if let Some(extra) = ops.next() {
                    let mut m = self.program_name.clone();
                    m.extend_from_slice(b": ");
                    m.extend_from_slice(&self.q(&extra));
                    m.extend_from_slice(b": extra operand\n");
                    util::print_stderr(&m);
                    self.usage(false, 2);
                }
            }
        }
    }

    /// `-z SUFFIX`.
    fn set_suffix(&mut self, suffix: &[u8]) {
        if suffix.is_empty() {
            self.fatal(b"backup suffix is empty");
        }
        self.origsuff = Some(suffix.to_vec());
    }

    /// `reinitialize_almost_everything`.
    fn reinitialize_almost_everything(&mut self) {
        self.re_patch();
        self.re_input();
        self.input_lines = 0;
        self.last_frozen_line = 0;
        if self.inname.is_some() && !self.explicit_inname {
            self.inname = None;
        }
        self.in_offset = 0;
        self.out_offset = 0;
        self.diff_type = Diff::No;
        self.revision = None;
        self.reverse = self.reverse_flag_specified;
        self.skip_rest_of_patch = false;
    }

    /// `locate_hunk`.
    fn locate_hunk(&mut self, fuzz: Lin) -> Lin {
        let first_guess = self.pch_first().saturating_add(self.in_offset);
        let pat_lines = self.pch_ptrn_lines();
        let prefix_context = self.pch_prefix_context();
        let suffix_context = self.pch_suffix_context();
        let context = prefix_context.max(suffix_context);
        let mut prefix_fuzz = fuzz + prefix_context - context;
        let suffix_fuzz = fuzz + suffix_context - context;
        let max_where = self.input_lines - (pat_lines - suffix_fuzz) + 1;
        let min_where = self.last_frozen_line + 1;
        let max_pos_offset = max_where - first_guess;
        let mut max_neg_offset = first_guess - min_where;
        let max_offset = max_pos_offset.max(max_neg_offset);

        if pat_lines == 0 {
            return first_guess;
        }
        // Do not try lines <= 0.
        if first_guess <= max_neg_offset {
            max_neg_offset = first_guess - 1;
        }

        if prefix_fuzz < 0 && self.pch_first() <= 1 {
            // Can only match start of file.
            if suffix_fuzz < 0
                && (pat_lines != self.input_lines || prefix_context < self.last_frozen_line)
            {
                // Can only match entire file.
                return 0;
            }
            let offset = 1 - first_guess;
            if self.last_frozen_line <= prefix_context
                && offset <= max_pos_offset
                && self.patch_match(first_guess, offset, 0, suffix_fuzz)
            {
                self.in_offset += offset;
                return first_guess + offset;
            }
            return 0;
        } else if prefix_fuzz < 0 {
            prefix_fuzz = 0;
        }

        if suffix_fuzz < 0 {
            // Can only match end of file.
            let offset = first_guess - (self.input_lines - pat_lines + 1);
            if offset <= max_neg_offset && self.patch_match(first_guess, -offset, prefix_fuzz, 0) {
                self.in_offset -= offset;
                return first_guess - offset;
            }
            return 0;
        }

        let min_offset = if max_pos_offset < 0 {
            first_guess - max_where
        } else if max_neg_offset < 0 {
            first_guess - min_where
        } else {
            0
        };
        let mut offset = min_offset;
        while offset <= max_offset {
            if offset <= max_pos_offset
                && self.patch_match(first_guess, offset, prefix_fuzz, suffix_fuzz)
            {
                if self.debug & 1 != 0 {
                    util::say(
                        format!(
                            "Offset changing from {} to {}\n",
                            self.in_offset,
                            self.in_offset + offset
                        )
                        .as_bytes(),
                    );
                }
                self.in_offset += offset;
                return first_guess + offset;
            }
            if offset <= max_neg_offset
                && self.patch_match(first_guess, -offset, prefix_fuzz, suffix_fuzz)
            {
                if self.debug & 1 != 0 {
                    util::say(
                        format!(
                            "Offset changing from {} to {}\n",
                            self.in_offset,
                            self.in_offset - offset
                        )
                        .as_bytes(),
                    );
                }
                self.in_offset -= offset;
                return first_guess - offset;
            }
            offset += 1;
        }
        0
    }

    /// `mangled_patch`.
    fn mangled_patch(&mut self, old: Lin, new: Lin) -> ! {
        if self.debug & 1 != 0 {
            let mut m = b"oldchar = '".to_vec();
            m.push(self.pch_char(old));
            m.extend_from_slice(b"', newchar = '");
            m.push(self.pch_char(new));
            m.extend_from_slice(b"'\n");
            util::say(&m);
        }
        let beg = self.pch_hunk_beg();
        let m = format!(
            "Out-of-sync patch, lines {},{} -- mangled text or line numbers, maybe?",
            beg + old,
            beg + new
        );
        self.fatal(m.as_bytes());
    }

    fn rej_write(&mut self, bytes: &[u8]) {
        let failed = match self.rejfp.as_mut() {
            Some(w) => w.write_all(bytes).is_err(),
            None => false,
        };
        if failed {
            self.write_fatal();
        }
    }

    /// `pch_write_line` to the reject file. A line of no bytes -- one with
    /// no newline that was empty -- is a write error, as upstream's
    /// `fwrite` of nothing reports one; `errno` is still 0 then.
    fn rej_write_line(&mut self, line: Lin) {
        let text = self.pfetch(line).to_vec();
        if text.is_empty() {
            self.pfatal(b"write error", 0);
        }
        self.rej_write(&text);
    }

    /// `print_unidiff_range`.
    fn print_unidiff_range(start: Lin, count: Lin) -> String {
        match count {
            0 => format!("{},0", start - 1),
            1 => format!("{start}"),
            _ => format!("{start},{count}"),
        }
    }

    /// `print_header_line`.
    fn print_header_line(&mut self, tag: &str, rev: bool) {
        let mut m = format!("{tag} ").into_bytes();
        let which = usize::from(rev);
        m.extend_from_slice(self.pch_name(which).unwrap_or(b"/dev/null"));
        if let Some(t) = self.pch_timestr(rev) {
            m.extend_from_slice(t);
        }
        m.push(b'\n');
        self.rej_write(&m);
    }

    /// `abort_hunk_unified`.
    fn abort_hunk_unified(&mut self, header: bool, rev: bool) {
        let mut old: Lin = 1;
        let lastline = self.pch_ptrn_lines();
        let mut new = lastline + 1;
        let c_function = self.pch_c_function().map(<[u8]>::to_vec);

        if header {
            if let Some(index) = self.pch_name(INDEX).map(<[u8]>::to_vec) {
                let mut m = b"Index: ".to_vec();
                m.extend_from_slice(&index);
                m.push(b'\n');
                self.rej_write(&m);
            }
            self.print_header_line("---", rev);
            self.print_header_line("+++", !rev);
        }

        // Add out_offset to guess the same as the previous successful hunk.
        let mut m = b"@@ -".to_vec();
        m.extend_from_slice(
            Self::print_unidiff_range(self.pch_first() + self.out_offset, lastline).as_bytes(),
        );
        m.extend_from_slice(b" +");
        m.extend_from_slice(
            Self::print_unidiff_range(self.pch_newfirst() + self.out_offset, self.pch_repl_lines())
                .as_bytes(),
        );
        m.extend_from_slice(b" @@");
        if let Some(f) = &c_function {
            m.extend_from_slice(f);
        }
        m.push(b'\n');
        self.rej_write(&m);

        while self.pch_char(new) == b'=' || self.pch_char(new) == b'\n' {
            new += 1;
        }

        if self.diff_type != Diff::Uni {
            self.pch_normalize(Diff::Uni);
        }

        loop {
            while self.pch_char(old) == b'-' {
                self.rej_write(b"-");
                self.rej_write_line(old);
                old += 1;
            }
            while self.pch_char(new) == b'+' {
                self.rej_write(b"+");
                self.rej_write_line(new);
                new += 1;
            }
            if old > lastline {
                break;
            }
            if self.pch_char(new) != self.pch_char(old) {
                self.mangled_patch(old, new);
            }
            self.rej_write(b" ");
            self.rej_write_line(old);
            old += 1;
            new += 1;
        }
        if self.pch_char(new) != b'^' {
            self.mangled_patch(old, new);
        }
    }

    /// `abort_hunk_context`.
    fn abort_hunk_context(&mut self, header: bool, rev: bool) {
        let pat_end = self.pch_end();
        let oldfirst = self.pch_first() + self.out_offset;
        let newfirst = self.pch_newfirst() + self.out_offset;
        let oldlast = oldfirst + self.pch_ptrn_lines() - 1;
        let newlast = newfirst + self.pch_repl_lines() - 1;
        let new_style = self.diff_type >= Diff::NewContext;
        let stars = if new_style { " ****" } else { "" };
        let minuses = if new_style { " ----" } else { " -----" };
        let c_function = self.pch_c_function().map(<[u8]>::to_vec);

        if self.diff_type == Diff::Uni {
            self.pch_normalize(Diff::NewContext);
        }

        if header {
            if let Some(index) = self.pch_name(INDEX).map(<[u8]>::to_vec) {
                let mut m = b"Index: ".to_vec();
                m.extend_from_slice(&index);
                m.push(b'\n');
                self.rej_write(&m);
            }
            self.print_header_line("***", rev);
            self.print_header_line("---", !rev);
        }
        let mut m = b"***************".to_vec();
        if let Some(f) = &c_function {
            m.extend_from_slice(f);
        }
        m.push(b'\n');
        self.rej_write(&m);
        for i in 0..=pat_end {
            match self.pch_char(i) {
                b'*' => {
                    let s = match oldlast.cmp(&oldfirst) {
                        std::cmp::Ordering::Less => format!("*** 0{stars}\n"),
                        std::cmp::Ordering::Equal => format!("*** {oldfirst}{stars}\n"),
                        std::cmp::Ordering::Greater => format!("*** {oldfirst},{oldlast}{stars}\n"),
                    };
                    self.rej_write(s.as_bytes());
                }
                b'=' => {
                    let s = match newlast.cmp(&newfirst) {
                        std::cmp::Ordering::Less => format!("--- 0{minuses}\n"),
                        std::cmp::Ordering::Equal => format!("--- {newfirst}{minuses}\n"),
                        std::cmp::Ordering::Greater => {
                            format!("--- {newfirst},{newlast}{minuses}\n")
                        }
                    };
                    self.rej_write(s.as_bytes());
                }
                c @ (b' ' | b'-' | b'+' | b'!') => {
                    self.rej_write(&[c, b' ']);
                    self.rej_write_line(i);
                }
                b'\n' => self.rej_write_line(i),
                _ => self.fatal(b"fatal internal error in abort_hunk_context"),
            }
        }
    }

    /// `abort_hunk`.
    fn abort_hunk(&mut self, outname: Option<&[u8]>, header: bool, rev: bool) {
        if !self.tmprej.needs_removal {
            self.init_reject(outname);
        }
        if self.reject_format == Diff::Uni
            || (self.reject_format == Diff::No && self.diff_type == Diff::Uni)
        {
            self.abort_hunk_unified(header, rev);
        } else {
            self.abort_hunk_context(header, rev);
        }
    }

    fn out_write(&mut self, outstate: &mut OutState, bytes: &[u8]) {
        let failed = match outstate.ofp.as_mut() {
            Some(o) => o.w.write_all(bytes).is_err(),
            None => false,
        };
        if failed {
            self.write_fatal();
        }
    }

    /// `pch_write_line` to the output: whether the line ended in a newline.
    /// An empty line is a write error, as in [`Ctx::rej_write_line`].
    fn out_write_line(&mut self, outstate: &mut OutState, line: Lin) -> bool {
        let text = self.pfetch(line).to_vec();
        if text.is_empty() {
            self.pfatal(b"write error", 0);
        }
        self.out_write(outstate, &text);
        text.last() == Some(&b'\n')
    }

    /// `apply_hunk`.
    fn apply_hunk(&mut self, outstate: &mut OutState, where_: Lin) -> bool {
        #[derive(PartialEq, Eq)]
        enum Def {
            Outside,
            InIfndef,
            InIfdef,
            InElse,
        }
        let mut old: Lin = 1;
        let lastline = self.pch_ptrn_lines();
        let mut new = lastline + 1;
        let mut def_state = Def::Outside;
        let defines = self.do_defines.clone();
        let pat_end = self.pch_end();
        let where_ = where_ - 1;

        while self.pch_char(new) == b'=' || self.pch_char(new) == b'\n' {
            new += 1;
        }

        while old <= lastline {
            if self.pch_char(old) == b'-' {
                if !self.copy_till(outstate, where_ + old - 1) {
                    return false;
                }
                if let Some(d) = &defines {
                    if def_state == Def::Outside {
                        let mut m = skip_nl(outstate.after_newline, NOT_DEFINED).to_vec();
                        m.extend_from_slice(d);
                        m.push(b'\n');
                        self.out_write(outstate, &m);
                        def_state = Def::InIfndef;
                    } else if def_state == Def::InIfdef {
                        let m = skip_nl(outstate.after_newline, ELSE_DEFINED).to_vec();
                        self.out_write(outstate, &m);
                        def_state = Def::InElse;
                    }
                    outstate.after_newline = self.out_write_line(outstate, old);
                    outstate.zero_output = false;
                }
                self.last_frozen_line += 1;
                old += 1;
            } else if new > pat_end {
                break;
            } else if self.pch_char(new) == b'+' {
                if !self.copy_till(outstate, where_ + old - 1) {
                    return false;
                }
                if let Some(d) = &defines {
                    if def_state == Def::InIfndef {
                        let m = skip_nl(outstate.after_newline, ELSE_DEFINED).to_vec();
                        self.out_write(outstate, &m);
                        def_state = Def::InElse;
                    } else if def_state == Def::Outside {
                        let mut m = skip_nl(outstate.after_newline, IF_DEFINED).to_vec();
                        m.extend_from_slice(d);
                        m.push(b'\n');
                        self.out_write(outstate, &m);
                        def_state = Def::InIfdef;
                    }
                }
                outstate.after_newline = self.out_write_line(outstate, new);
                outstate.zero_output = false;
                new += 1;
            } else if self.pch_char(new) != self.pch_char(old) {
                self.mangled_patch(old, new);
            } else if self.pch_char(new) == b'!' {
                if !self.copy_till(outstate, where_ + old - 1) {
                    return false;
                }
                if let Some(d) = &defines {
                    let mut m = NOT_DEFINED.get(1..).unwrap_or_default().to_vec();
                    m.extend_from_slice(d);
                    m.push(b'\n');
                    self.out_write(outstate, &m);
                    def_state = Def::InIfndef;
                }
                loop {
                    if defines.is_some() {
                        outstate.after_newline = self.out_write_line(outstate, old);
                    }
                    self.last_frozen_line += 1;
                    old += 1;
                    if self.pch_char(old) != b'!' {
                        break;
                    }
                }
                if defines.is_some() {
                    let m = skip_nl(outstate.after_newline, ELSE_DEFINED).to_vec();
                    self.out_write(outstate, &m);
                    def_state = Def::InElse;
                }
                loop {
                    outstate.after_newline = self.out_write_line(outstate, new);
                    new += 1;
                    if self.pch_char(new) != b'!' {
                        break;
                    }
                }
                outstate.zero_output = false;
            } else {
                old += 1;
                new += 1;
                if defines.is_some() && def_state != Def::Outside {
                    let m = skip_nl(outstate.after_newline, END_DEFINED).to_vec();
                    self.out_write(outstate, &m);
                    outstate.after_newline = true;
                    def_state = Def::Outside;
                }
            }
        }
        if new <= pat_end && self.pch_char(new) == b'+' {
            if !self.copy_till(outstate, where_ + old - 1) {
                return false;
            }
            if let Some(d) = &defines {
                if def_state == Def::Outside {
                    let mut m = skip_nl(outstate.after_newline, IF_DEFINED).to_vec();
                    m.extend_from_slice(d);
                    m.push(b'\n');
                    self.out_write(outstate, &m);
                    def_state = Def::InIfdef;
                } else if def_state == Def::InIfndef {
                    let m = skip_nl(outstate.after_newline, ELSE_DEFINED).to_vec();
                    self.out_write(outstate, &m);
                    def_state = Def::InElse;
                }
                outstate.zero_output = false;
            }
            loop {
                if !outstate.after_newline {
                    self.out_write(outstate, b"\n");
                }
                outstate.after_newline = self.out_write_line(outstate, new);
                outstate.zero_output = false;
                new += 1;
                if !(new <= pat_end && self.pch_char(new) == b'+') {
                    break;
                }
            }
        }
        if defines.is_some() && def_state != Def::Outside {
            let m = skip_nl(outstate.after_newline, END_DEFINED).to_vec();
            self.out_write(outstate, &m);
            outstate.after_newline = true;
        }
        self.out_offset += self.pch_repl_lines() - self.pch_ptrn_lines();
        true
    }

    /// `create_output_file`.
    fn create_output_file(&mut self, name: &[u8], open_flags: i32) -> Output {
        let mode = self.instat.mode;
        let fd = self.create_file(name, sys::oflag::WRONLY | open_flags, mode, true);
        Output {
            w: std::io::BufWriter::new(util::file_from_fd(fd)),
            fd,
        }
    }

    /// `open_outfile`.
    fn open_outfile(&mut self, name: &[u8]) -> Output {
        if name != b"-" {
            return self.create_output_file(name, 0);
        }
        let dup = match sys::dup_fd(1) {
            Ok(fd) => fd,
            Err(e) => self.pfatal(b"Failed to duplicate standard output", e),
        };
        if let Err(e) = libcall::fd::dup2(2, 1) {
            self.pfatal(b"Failed to redirect messages to standard error", e);
        }
        util::stdout_is_stderr_now();
        Output {
            w: std::io::BufWriter::new(util::file_from_fd(dup)),
            fd: dup,
        }
    }

    /// `init_reject`: the temporary reject file, beside `outname` -- or, with
    /// none, in `$TMPDIR`.
    fn init_reject(&mut self, outname: Option<&[u8]>) {
        let (name, fd) = self.make_tempfile(b'r', outname, sys::oflag::WRONLY, 0o666);
        self.tmprej.name = Some(name.clone());
        let Ok(fd) = fd else {
            let mut m = b"Can't create temporary file ".to_vec();
            m.extend_from_slice(&name);
            let e = fd.err().unwrap_or(errno::EIO);
            self.pfatal(&m, e);
        };
        self.tmprej.needs_removal = true;
        util::register_temp(TMP_REJ, &name);
        self.rejfp = Some(std::io::BufWriter::new(util::file_from_fd(fd)));
    }

    /// `copy_till`: the input up to and including line `lastline` copied out.
    pub fn copy_till(&mut self, outstate: &mut OutState, lastline: Lin) -> bool {
        let mut r = self.last_frozen_line;
        if r > lastline {
            util::say(b"misordered hunks! output would be garbled\n");
            return false;
        }
        while r < lastline {
            r += 1;
            let line = self.ifetch(r, false).to_vec();
            if !line.is_empty() {
                if !outstate.after_newline {
                    self.out_write(outstate, b"\n");
                }
                self.out_write(outstate, &line);
                outstate.after_newline = line.last() == Some(&b'\n');
                outstate.zero_output = false;
            }
        }
        self.last_frozen_line = r;
        true
    }

    /// `spew_output`: the rest of the input copied out, and the output
    /// closed, its status in `st`.
    fn spew_output(&mut self, outstate: &mut OutState, st: &mut Option<Stat>) -> bool {
        if self.debug & 256 != 0 {
            util::say(
                format!("il={} lfl={}\n", self.input_lines, self.last_frozen_line).as_bytes(),
            );
        }
        if self.last_frozen_line < self.input_lines && !self.copy_till(outstate, self.input_lines) {
            return false;
        }
        if self.outfile.is_none()
            && let Some(mut o) = outstate.ofp.take()
        {
            let flushed = o.w.flush().is_ok();
            let stat = sys::fstat_fd(o.fd);
            let (file, _) = o.w.into_parts();
            let closed = util::close_file(file);
            match (flushed, stat, closed) {
                (true, Ok(s), true) => *st = Some(s),
                _ => self.write_fatal(),
            }
        }
        true
    }

    /// `patch_match`: whether the hunk's old lines match the input at
    /// `base + offset`.
    fn patch_match(&mut self, base: Lin, offset: Lin, prefix_fuzz: Lin, suffix_fuzz: Lin) -> bool {
        let mut pline = 1 + prefix_fuzz;
        let pat_lines = self.pch_ptrn_lines() - suffix_fuzz;
        let mut iline = base + offset + prefix_fuzz;
        while pline <= pat_lines {
            let p = self.ifetch(iline, offset >= 0).to_vec();
            let pat = self.pfetch(pline);
            if self.canonicalize_ws {
                if !similar(&p, pat) {
                    return false;
                }
            } else if p.as_slice() != pat {
                return false;
            }
            pline += 1;
            iline += 1;
        }
        true
    }

    /// `check_line_endings`: whether the patch and the input end their lines
    /// differently.
    fn check_line_endings(&mut self, mut where_: Lin) -> bool {
        let p = self.pfetch(1);
        if p.is_empty() {
            return false;
        }
        let patch_crlf = p.ends_with(b"\r\n");
        if self.input_lines == 0 {
            return false;
        }
        if where_ > self.input_lines {
            where_ = self.input_lines;
        }
        let p = self.ifetch(where_, false);
        if p.is_empty() {
            return false;
        }
        let input_crlf = p.ends_with(b"\r\n");
        patch_crlf != input_crlf
    }

    /// `delete_file_later`.
    fn delete_file_later(&mut self, name: &[u8], st: Option<&Stat>, backup: bool) {
        let st = match st {
            Some(s) => *s,
            None => match self.stat_file(name) {
                Ok(s) => s,
                Err(e) => {
                    let mut m = b"Can't get file attributes of file ".to_vec();
                    m.extend_from_slice(name);
                    self.pfatal(&m, e);
                }
            },
        };
        self.files_to_delete.push(FileToDelete {
            name: name.to_vec(),
            st,
            backup,
        });
        self.insert_file_id(&st, FileIdType::DeleteLater);
    }

    /// `delete_files`.
    fn delete_files(&mut self) {
        let list = std::mem::take(&mut self.files_to_delete);
        for f in &list {
            if self.lookup_file_id(&f.st) == FileIdType::DeleteLater {
                let mode = f.st.mode;
                if self.verbosity == Verbosity::Verbose {
                    let mut m = format!(
                        "Removing {} ",
                        if mode & sys::S_IFMT == sys::S_IFLNK {
                            "symbolic link"
                        } else {
                            "file"
                        }
                    )
                    .into_bytes();
                    m.extend_from_slice(&self.q(&f.name));
                    m.push(b'\n');
                    util::say(&m);
                }
                self.move_file(None, None, None, &f.name, mode, f.backup);
                self.removedirs(&f.name);
            }
        }
    }

    /// `output_file_later`.
    fn output_file_later(
        &mut self,
        from: &[u8],
        from_st: &Stat,
        to: Option<&[u8]>,
        mode: u32,
        backup: bool,
    ) {
        self.files_to_output.push(FileToOutput {
            from: from.to_vec(),
            from_st: *from_st,
            to: to.map(<[u8]>::to_vec),
            mode,
            backup,
        });
    }

    /// `output_file_now`.
    fn output_file_now(
        &mut self,
        from: &[u8],
        from_needs_removal: &mut bool,
        from_st: &Stat,
        to: Option<&[u8]>,
        mode: u32,
        backup: bool,
    ) {
        match to {
            None => {
                if backup {
                    self.create_backup(from, Some(from_st), true);
                }
            }
            Some(to) => {
                self.move_file(
                    Some(from),
                    Some(from_needs_removal),
                    Some(from_st),
                    to,
                    mode,
                    backup,
                );
            }
        }
    }

    /// `output_file`. `from == None` deletes `to` later; a git-style diff
    /// that does not create the file queues the move.
    fn output_file(
        &mut self,
        from: Option<&[u8]>,
        from_needs_removal: Option<&mut bool>,
        from_st: Option<&Stat>,
        to: Option<&[u8]>,
        to_st: Option<&Stat>,
        mode: u32,
        backup: bool,
    ) {
        match from {
            None => {
                let to = to.unwrap_or_default().to_vec();
                self.delete_file_later(&to, to_st, backup);
            }
            Some(from) => {
                let from_st = from_st.copied().unwrap_or_default();
                if self.pch_git_diff() && self.pch_says_nonexistent(self.reverse) != 2 {
                    self.output_file_later(from, &from_st, to, mode, backup);
                    if let Some(r) = from_needs_removal {
                        *r = false;
                        if self.tmpout.name.as_deref() == Some(from) {
                            util::register_queued(from);
                        }
                    }
                } else {
                    let mut scratch = true;
                    let r = from_needs_removal.unwrap_or(&mut scratch);
                    self.output_file_now(from, r, &from_st, to, mode, backup);
                }
            }
        }
    }

    /// `output_files`: the queued moves done, up to and including the one
    /// whose source is `st`, or all of them.
    fn output_files(&mut self, st: Option<&Stat>) {
        let list = std::mem::take(&mut self.files_to_output);
        let mut done = 0usize;
        for (i, f) in list.iter().enumerate() {
            let mut from_needs_removal = true;
            self.output_file_now(
                &f.from,
                &mut from_needs_removal,
                &f.from_st,
                f.to.as_deref(),
                f.mode,
                f.backup,
            );
            if f.to.is_some() && from_needs_removal {
                // As upstream: a source left behind is removed, unchecked.
                let _ = self.safe.unlink(&f.from);
            }
            util::unregister_queued(&f.from);
            done = i.saturating_add(1);
            if let Some(st) = st
                && st.dev == f.from_st.dev
                && st.ino == f.from_st.ino
            {
                // The rest stays queued.
                self.files_to_output = list.get(done..).unwrap_or_default().to_vec();
                return;
            }
        }
        let _ = done;
    }

    /// `remove_if_needed`: temporary file `which` -- [`TMP_IN`], [`TMP_OUT`],
    /// [`TMP_PAT`], [`TMP_REJ`] or [`TMP_ED`] -- removed if it is there.
    pub fn remove_if_needed(&mut self, which: usize) {
        let t = match which {
            TMP_IN => &mut self.tmpin,
            TMP_OUT => &mut self.tmpout,
            TMP_PAT => &mut self.tmppat,
            TMP_ED => &mut self.tmped,
            _ => &mut self.tmprej,
        };
        if t.needs_removal {
            t.needs_removal = false;
            if let Some(name) = t.name.clone() {
                // Upstream's `safe_unlink`, unchecked.
                let _ = self.safe.unlink(&name);
            }
        }
        util::unregister_temp(which);
    }

    /// `cleanup`. Once only: a failure inside it ends in `fatal_exit`, which
    /// calls it again (Debian's "Abort when cleaning up fails").
    fn cleanup(&mut self) {
        if self.in_cleanup {
            return;
        }
        self.in_cleanup = true;
        for which in [TMP_IN, TMP_OUT, TMP_PAT, TMP_ED, TMP_REJ] {
            self.remove_if_needed(which);
        }
        self.output_files(None);
    }

    /// `fatal_exit (0)`.
    pub fn fatal_exit(&mut self) -> ! {
        self.cleanup();
        self.exit(2);
    }
}

/// `similar`: whether two lines match with their white space canonicalized.
pub fn similar(a: &[u8], b: &[u8]) -> bool {
    // Ignore presence or absence of trailing newlines.
    let mut a = a.strip_suffix(b"\n").unwrap_or(a);
    let mut b = b.strip_suffix(b"\n").unwrap_or(b);
    let blank = |c: Option<&u8>| matches!(c, Some(b' ' | b'\t'));
    loop {
        if b.is_empty() || blank(b.first()) {
            while blank(b.first()) {
                b = b.get(1..).unwrap_or_default();
            }
            if !a.is_empty() {
                if !blank(a.first()) {
                    return false;
                }
                while blank(a.first()) {
                    a = a.get(1..).unwrap_or_default();
                }
            }
            if a.is_empty() || b.is_empty() {
                return a.len() == b.len();
            }
        } else if a.is_empty() || a.first() != b.first() {
            return false;
        } else {
            a = a.get(1..).unwrap_or_default();
            b = b.get(1..).unwrap_or_default();
        }
    }
}

/// The body of `main`, after the options: each patch in the input, applied.
#[allow(clippy::arithmetic_side_effects)]
fn run(cx: &mut Ctx) -> i32 {
    let mut somefailed = false;
    let mut outstate = OutState::new();
    let mut tmpoutst: Option<Stat>;
    let mut written_to_rejname = false;
    let mut skip_reject_file = false;
    let mut apply_empty_patch = false;
    let mut file_type: u32 = 0;
    let mut outfd: Option<i32> = None;
    let mut have_git_diff = false;

    // Make get_date() assume that context diff headers use UTC.
    if cx.set_utc {
        // SAFETY: this program has one thread.
        unsafe { std::env::set_var("TZ", "UTC") };
    }

    if cx.make_backups || cx.backup_if_mismatch {
        let context = cx.version_control_context;
        let version = cx.version_control.clone();
        cx.backup_type = cx.get_version(context, version.as_deref());
    }

    if let Some(name) = cx.outfile.clone() {
        outstate.ofp = Some(cx.open_outfile(&name));
    }

    // Make sure we clean up in case of disaster.
    util::set_signals(false);

    // When the file to patch is specified on the command line, allow that
    // file to lie outside the current working tree. Still doesn't allow to
    // follow symlinks.
    if cx.inname.is_some() {
        cx.safe.unsafe_ = true;
    }

    if cx.inname.is_some() && cx.outfile.is_some() {
        apply_empty_patch = true;
        file_type = sys::S_IFREG;
        cx.inerrno = -1;
    }

    let patchname = cx.patchname.clone();
    cx.open_patch_file(patchname.as_deref());
    let mut first = true;
    loop {
        if !first {
            cx.reinitialize_almost_everything();
            skip_reject_file = false;
            apply_empty_patch = false;
        }
        first = false;
        let need_header = !(cx.inname.is_some() || cx.posixly_correct);
        let another = cx.there_is_another_patch(need_header, &mut file_type);
        if !(another || apply_empty_patch) {
            break;
        }

        let mut hunk: i32 = 0;
        let mut failed: i32 = 0;
        let mut mismatch = false;
        let mut outname: Option<Vec<u8>> = None;
        tmpoutst = None;

        if cx.skip_rest_of_patch {
            somefailed = true;
        }

        if have_git_diff != cx.pch_git_diff() {
            if have_git_diff {
                cx.output_files(None);
                cx.inerrno = -1;
            }
            have_git_diff = !have_git_diff;
        }

        if cx.tmprej.needs_removal {
            if let Some(mut w) = cx.rejfp.take() {
                // Being discarded: a failure here costs nothing.
                let _ = w.flush();
            }
            cx.remove_if_needed(TMP_REJ);
        }
        if cx.tmpout.needs_removal {
            if let Some(fd) = outfd.take() {
                // Being discarded with the file: a failure costs nothing.
                let _ = sys::close_fd(fd);
            }
            cx.remove_if_needed(TMP_OUT);
        }
        cx.remove_if_needed(TMP_ED);

        if !cx.skip_rest_of_patch && file_type == 0 {
            let mut m = b"File ".to_vec();
            m.extend_from_slice(&cx.q(cx.inname.as_deref().unwrap_or_default()));
            m.extend_from_slice(
                format!(
                    ": can't change file type from 0{:o} to 0{:o}.\n",
                    cx.pch_mode(cx.reverse) & sys::S_IFMT,
                    cx.pch_mode(!cx.reverse) & sys::S_IFMT
                )
                .as_bytes(),
            );
            util::say(&m);
            cx.skip_rest_of_patch = true;
            somefailed = true;
        }

        // Whether `outname` IS `inname` -- upstream compares the pointers --
        // rather than merely spelled the same.
        let mut outname_is_inname = false;
        if !cx.skip_rest_of_patch {
            outname = if let Some(o) = &cx.outfile {
                Some(o.clone())
            } else if cx.pch_copy() || cx.pch_rename() {
                cx.pch_name(usize::from(!cx.reverse)).map(<[u8]>::to_vec)
            } else {
                outname_is_inname = true;
                cx.inname.clone()
            };
        }

        if cx.pch_git_diff() && !cx.skip_rest_of_patch {
            let inname = cx.inname.clone().unwrap_or_default();
            let on = outname.clone().unwrap_or_default();
            let (mut outerrno, mut outstat);
            if inname == on {
                if cx.inerrno == -1 {
                    match cx.stat_file(&inname) {
                        Ok(s) => {
                            cx.instat = s;
                            cx.inerrno = 0;
                        }
                        Err(e) => cx.inerrno = e,
                    }
                }
                outstat = cx.instat;
                outerrno = cx.inerrno;
            } else {
                match cx.stat_file(&on) {
                    Ok(s) => {
                        outstat = s;
                        outerrno = 0;
                    }
                    Err(e) => {
                        outstat = Stat::default();
                        outerrno = e;
                    }
                }
            }
            if outerrno == 0 {
                if cx.has_queued_output(&outstat) {
                    cx.output_files(Some(&outstat));
                    match cx.stat_file(&on) {
                        Ok(s) => {
                            outstat = s;
                            outerrno = 0;
                        }
                        Err(e) => outerrno = e,
                    }
                    cx.inerrno = -1;
                }
                if outerrno == 0 {
                    cx.set_queued_output(&outstat, true);
                }
            }
        }

        if !cx.skip_rest_of_patch {
            let inname = cx.inname.clone().unwrap_or_default();
            let on = outname.clone().unwrap_or_default();
            if !cx.get_input_file(&inname, &on, file_type) {
                cx.skip_rest_of_patch = true;
                somefailed = true;
            }
        }

        if cx.read_only_behavior != ReadOnly::Ignore && cx.inerrno == 0 && !cx.instat.is_lnk() {
            let inname = cx.inname.clone().unwrap_or_default();
            if cx.safe.access(&inname, sys::W_OK).is_err() {
                let mut m = b"File ".to_vec();
                m.extend_from_slice(&cx.q(&inname));
                m.extend_from_slice(b" is read-only; ");
                util::say(&m);
                if cx.read_only_behavior == ReadOnly::Warn {
                    util::say(b"trying to patch anyway\n");
                } else {
                    util::say(b"refusing to patch\n");
                    cx.skip_rest_of_patch = true;
                    somefailed = true;
                }
            }
        }

        let on = outname.clone().unwrap_or_default();
        let (tmpname, made) = cx.make_tempfile(
            b'o',
            outname.as_deref(),
            sys::oflag::WRONLY,
            cx.instat.mode & sys::S_IRWXUGO,
        );
        cx.tmpout.name = Some(tmpname.clone());
        match made {
            Err(e) => {
                if e == errno::ELOOP || e == errno::EXDEV {
                    let mut m = b"Invalid file name ".to_vec();
                    m.extend_from_slice(&cx.q(&on));
                    m.extend_from_slice(b" -- skipping patch\n");
                    util::say(&m);
                    cx.skip_rest_of_patch = true;
                    skip_reject_file = true;
                    somefailed = true;
                } else {
                    let mut m = b"Can't create temporary file ".to_vec();
                    m.extend_from_slice(&tmpname);
                    cx.pfatal(&m, e);
                }
            }
            Ok(fd) => {
                outfd = Some(fd);
                cx.tmpout.needs_removal = true;
                util::register_temp(TMP_OUT, &tmpname);
            }
        }

        if cx.diff_type == Diff::Ed {
            outstate.zero_output = false;
            somefailed |= cx.skip_rest_of_patch;
            let inname = cx.inname.clone().unwrap_or_default();
            cx.do_ed_script(&inname, &tmpname, outstate.ofp.as_mut());
            if !cx.dry_run && cx.outfile.is_none() && !cx.skip_rest_of_patch {
                match sys::fstat_fd(outfd.unwrap_or(-1)) {
                    Ok(s) => {
                        outstate.zero_output = s.size == 0;
                        tmpoutst = Some(s);
                    }
                    Err(e) => cx.pfatal(&tmpname, e),
                }
            }
            if let Some(fd) = outfd.take() {
                // The output was written by `ed`, through its own descriptor;
                // this one has nothing pending to lose.
                let _ = sys::close_fd(fd);
            }
        } else {
            let mut apply_anyway = cx.merge; // don't try to reverse when merging

            if !cx.skip_rest_of_patch && cx.diff_type == Diff::GitBinary {
                let mut m = b"File ".to_vec();
                m.extend_from_slice(&cx.q(&on));
                m.extend_from_slice(b": git binary diffs are not supported.\n");
                util::say(&m);
                cx.skip_rest_of_patch = true;
                somefailed = true;
            }
            // Initialize the patched file.
            if !cx.skip_rest_of_patch && cx.outfile.is_none() {
                outstate = OutState::new();
                if let Some(fd) = outfd.take() {
                    outstate.ofp = Some(Output {
                        w: std::io::BufWriter::new(util::file_from_fd(fd)),
                        fd,
                    });
                }
            } else {
                // When writing to a single output file (-o FILE), always
                // pretend that the output file ends in a newline. Otherwise,
                // when another file is written to the same output file,
                // apply_hunk will fail (Debian's 0009).
                outstate.after_newline = true;
            }

            // Find out where all the lines are.
            if !cx.skip_rest_of_patch {
                let inname = cx.inname.clone().unwrap_or_default();
                cx.scan_input(&inname, file_type);

                if cx.verbosity != Verbosity::Silent {
                    let renamed = inname != on;
                    let skip_rename = !renamed && cx.pch_rename();
                    let mut m = format!(
                        "{} {} ",
                        if cx.dry_run { "checking" } else { "patching" },
                        if file_type & sys::S_IFMT == sys::S_IFLNK {
                            "symbolic link"
                        } else {
                            "file"
                        }
                    )
                    .into_bytes();
                    m.extend_from_slice(&cx.q(&on));
                    m.push(if renamed || skip_rename { b' ' } else { b'\n' });
                    util::say(&m);
                    if renamed || skip_rename {
                        let mut m = b"(".to_vec();
                        if skip_rename {
                            m.extend_from_slice(b"already ");
                        }
                        m.extend_from_slice(if cx.pch_copy() {
                            b"copied"
                        } else if cx.pch_rename() {
                            b"renamed"
                        } else {
                            b"read"
                        });
                        m.extend_from_slice(b" from ");
                        if !skip_rename {
                            m.extend_from_slice(&inname);
                        } else {
                            // `pch_name (! strcmp (inname, pch_name (OLD)))`:
                            // the new name when inname is the old one.
                            let old_is_in = cx.pch_name(OLD) == Some(inname.as_slice());
                            m.extend_from_slice(
                                cx.pch_name(usize::from(old_is_in)).unwrap_or_default(),
                            );
                        }
                        m.extend_from_slice(b")\n");
                        util::say(&m);
                    }
                    if cx.verbosity == Verbosity::Verbose {
                        util::say(if cx.using_plan_a {
                            b"Using Plan A...\n"
                        } else {
                            b"Using Plan B...\n"
                        });
                    }
                }
            }

            // Apply each hunk of patch.
            loop {
                let got_hunk = cx.another_hunk(cx.diff_type, cx.reverse);
                if got_hunk <= 0 {
                    break;
                }
                let mut where_: Lin = 0;
                let mut fuzz: Lin = 0;
                let mymaxfuzz = if cx.merge {
                    // When in merge mode, don't apply with fuzz.
                    0
                } else {
                    let context = cx.pch_prefix_context().max(cx.pch_suffix_context());
                    cx.maxfuzz.min(context)
                };

                hunk = hunk.saturating_add(1);
                if !cx.skip_rest_of_patch {
                    loop {
                        where_ = cx.locate_hunk(fuzz);
                        if where_ == 0 || fuzz != 0 || cx.in_offset != 0 {
                            mismatch = true;
                        }
                        if hunk == 1
                            && where_ == 0
                            && !(cx.force || apply_anyway)
                            && cx.reverse == cx.reverse_flag_specified
                        {
                            // DWIM for reversed patch?
                            cx.pch_swap();
                            // Try again.
                            where_ = cx.locate_hunk(fuzz);
                            let what = if cx.reverse {
                                "Unreversed"
                            } else {
                                "Reversed (or previously applied)"
                            };
                            if where_ != 0
                                && cx.ok_to_reverse(format!("{what} patch detected!").as_bytes())
                            {
                                cx.reverse = !cx.reverse;
                            } else {
                                // Put it back to normal.
                                cx.pch_swap();
                                if where_ != 0 {
                                    apply_anyway = true;
                                    fuzz -= 1; // Undo '++fuzz' below.
                                    where_ = 0;
                                }
                            }
                        }
                        // `while (!skip_rest_of_patch && !where && ++fuzz <= mymaxfuzz)`.
                        if cx.skip_rest_of_patch || where_ != 0 {
                            break;
                        }
                        fuzz += 1;
                        if fuzz > mymaxfuzz {
                            break;
                        }
                    }

                    if cx.skip_rest_of_patch {
                        // Just got decided.
                        if outstate.ofp.is_some() && cx.outfile.is_none() {
                            if let Some(o) = outstate.ofp.take() {
                                let (file, _) = o.w.into_parts();
                                let _ = util::close_file(file);
                            }
                            outfd = None;
                        }
                    }
                }

                let newwhere = (if where_ != 0 { where_ } else { cx.pch_first() }) + cx.out_offset;
                let rejected = if cx.skip_rest_of_patch {
                    true
                } else if cx.merge {
                    !cx.merge_hunk(hunk, &mut outstate, where_, &mut somefailed)
                } else {
                    (where_ == 1 && cx.pch_says_nonexistent(cx.reverse) == 2 && cx.instat.size != 0)
                        || where_ == 0
                        || !cx.apply_hunk(&mut outstate, where_)
                };
                if rejected {
                    if !skip_reject_file {
                        let rev = cx.reverse;
                        cx.abort_hunk(outname.as_deref(), failed == 0, rev);
                    }
                    failed = failed.saturating_add(1);
                    if cx.verbosity == Verbosity::Verbose
                        || (!cx.skip_rest_of_patch && cx.verbosity != Verbosity::Silent)
                    {
                        let endings = !cx.skip_rest_of_patch && cx.check_line_endings(newwhere);
                        util::say(
                            format!(
                                "Hunk #{hunk} {} at {newwhere}{}.\n",
                                if cx.skip_rest_of_patch {
                                    "ignored"
                                } else {
                                    "FAILED"
                                },
                                if endings {
                                    " (different line endings)"
                                } else {
                                    ""
                                }
                            )
                            .as_bytes(),
                        );
                    }
                } else if !cx.merge
                    && (cx.verbosity == Verbosity::Verbose
                        || (cx.verbosity != Verbosity::Silent && (fuzz != 0 || cx.in_offset != 0)))
                {
                    let mut m = format!("Hunk #{hunk} succeeded at {newwhere}");
                    if fuzz != 0 {
                        m.push_str(&format!(" with fuzz {fuzz}"));
                    }
                    if cx.in_offset != 0 {
                        m.push_str(&format!(
                            " (offset {} line{})",
                            cx.in_offset,
                            plural(cx.in_offset)
                        ));
                    }
                    m.push_str(".\n");
                    util::say(m.as_bytes());
                }
            }

            if !cx.skip_rest_of_patch {
                // Finish spewing out the new file.
                if !cx.spew_output(&mut outstate, &mut tmpoutst) {
                    util::say(b"Skipping patch.\n");
                    cx.skip_rest_of_patch = true;
                }
            }
        }

        // And put the output where desired.
        util::ignore_signals();
        if !cx.skip_rest_of_patch && cx.outfile.is_none() {
            let backup = cx.make_backups || (cx.backup_if_mismatch && (mismatch || failed != 0));
            let inname = cx.inname.clone().unwrap_or_default();
            if outstate.zero_output
                && (cx.remove_empty_files
                    || (cx.pch_says_nonexistent(!cx.reverse) == 2 && !cx.posixly_correct)
                    || file_type & sys::S_IFMT == sys::S_IFLNK)
            {
                if !cx.dry_run {
                    let to_st = outname_is_inname.then_some(cx.instat);
                    cx.output_file(
                        None,
                        None,
                        None,
                        Some(&on),
                        to_st.as_ref(),
                        file_type,
                        backup,
                    );
                }
            } else {
                if !outstate.zero_output
                    && cx.pch_says_nonexistent(!cx.reverse) == 2
                    && (cx.remove_empty_files || !cx.posixly_correct)
                    && !(cx.merge && somefailed)
                {
                    mismatch = true;
                    somefailed = true;
                    if cx.verbosity != Verbosity::Silent {
                        let mut m = b"Not deleting file ".to_vec();
                        m.extend_from_slice(&cx.q(&on));
                        m.extend_from_slice(b" as content differs from patch\n");
                        util::say(&m);
                    }
                }

                if !cx.dry_run {
                    let old_mode = cx.pch_mode(cx.reverse);
                    let new_mode = cx.pch_mode(!cx.reverse);
                    let set_mode = new_mode != 0 && old_mode != new_mode;

                    // Avoid replacing files when nothing has changed.
                    if failed < hunk
                        || cx.diff_type == Diff::Ed
                        || set_mode
                        || cx.pch_copy()
                        || cx.pch_rename()
                    {
                        let mut attr = 0u32;
                        let new_time = cx.pch_timestamp(!cx.reverse);
                        let mode = file_type
                            | ((if set_mode { new_mode } else { cx.instat.mode }) & sys::S_IRWXUGO);

                        if (cx.set_time || cx.set_utc) && new_time.sec != -1 {
                            let old_time = cx.pch_timestamp(cx.reverse);
                            if !cx.force
                                && cx.inerrno == 0
                                && cx.pch_says_nonexistent(cx.reverse) != 2
                                && old_time.sec != -1
                                && old_time != cx.instat.mtime
                            {
                                let mut m = b"Not setting time of file ".to_vec();
                                m.extend_from_slice(&cx.q(&on));
                                m.extend_from_slice(b" (time mismatch)\n");
                                util::say(&m);
                            } else if !cx.force && (mismatch || failed != 0) {
                                let mut m = b"Not setting time of file ".to_vec();
                                m.extend_from_slice(&cx.q(&on));
                                m.extend_from_slice(b" (contents mismatch)\n");
                                util::say(&m);
                            } else {
                                attr |= util::FA_TIMES;
                            }
                        }

                        if cx.inerrno != 0 {
                            if set_mode {
                                attr |= util::FA_MODE;
                            }
                            cx.set_file_attributes(
                                &tmpname,
                                attr,
                                None,
                                None,
                                mode,
                                Some(new_time),
                            );
                        } else {
                            attr |= util::FA_IDS | util::FA_MODE | util::FA_XATTRS;
                            let st = cx.instat;
                            cx.set_file_attributes(
                                &tmpname,
                                attr,
                                Some(&inname),
                                Some(&st),
                                mode,
                                Some(new_time),
                            );
                        }

                        let tst = tmpoutst.unwrap_or_default();
                        let mut needs = cx.tmpout.needs_removal;
                        cx.output_file(
                            Some(&tmpname),
                            Some(&mut needs),
                            Some(&tst),
                            Some(&on),
                            None,
                            mode,
                            backup,
                        );
                        cx.tmpout.needs_removal = needs;
                        if !needs {
                            util::unregister_temp(TMP_OUT);
                        }

                        if cx.pch_rename() {
                            let st = cx.instat;
                            cx.output_file(
                                None,
                                None,
                                None,
                                Some(&inname),
                                Some(&st),
                                mode,
                                backup,
                            );
                        }
                    } else {
                        let tst = tmpoutst.unwrap_or_default();
                        cx.output_file(Some(&on), None, Some(&tst), None, None, file_type, backup);
                    }
                }
            }
        }
        if cx.diff_type != Diff::Ed {
            if failed != 0 && !skip_reject_file {
                let flushed = match cx.rejfp.take() {
                    Some(w) => match w.into_inner() {
                        Ok(f) => {
                            let st = util::fstat_file(&f);
                            let closed = util::close_file(f);
                            st.filter(|_| closed)
                        }
                        Err(_) => None,
                    },
                    None => None,
                };
                let Some(rejst) = flushed else {
                    cx.write_fatal();
                };
                somefailed = true;
                util::say(
                    format!(
                        "{failed} out of {hunk} hunk{} {}",
                        plural(Lin::from(hunk)),
                        if cx.skip_rest_of_patch {
                            "ignored"
                        } else {
                            "FAILED"
                        }
                    )
                    .as_bytes(),
                );
                if !on.is_empty() && cx.rejname.as_deref() != Some(b"-".as_slice()) {
                    let rej = match cx.rejname.clone() {
                        Some(r) => r,
                        None => {
                            let saved =
                                std::mem::replace(&mut cx.simple_backup_suffix, b".rej".to_vec());
                            let mut rej = cx.find_backup_file_name(&on, BackupType::SimpleBackups);
                            cx.simple_backup_suffix = saved;
                            if rej.last() == Some(&b'~') {
                                rej.pop();
                                rej.push(b'#');
                            }
                            rej
                        }
                    };
                    if !cx.dry_run {
                        let mut m = b" -- saving rejects to file ".to_vec();
                        m.extend_from_slice(&cx.q(&rej));
                        m.push(b'\n');
                        util::say(&m);
                        let tmprej = cx.tmprej.name.clone().unwrap_or_default();
                        if cx.rejname.is_some() {
                            if !written_to_rejname {
                                cx.copy_file(&tmprej, &rej, None, 0, sys::S_IFREG | 0o666, true);
                                written_to_rejname = true;
                            } else {
                                cx.append_to_file(&tmprej, &rej);
                            }
                        } else {
                            match cx.stat_file(&rej) {
                                Err(e) if e != errno::ENOENT => cx.write_fatal(),
                                Ok(oldst) if cx.lookup_file_id(&oldst) == FileIdType::Created => {
                                    cx.append_to_file(&tmprej, &rej);
                                }
                                _ => {
                                    let mut needs = cx.tmprej.needs_removal;
                                    cx.move_file(
                                        Some(&tmprej),
                                        Some(&mut needs),
                                        Some(&rejst),
                                        &rej,
                                        sys::S_IFREG | 0o666,
                                        false,
                                    );
                                    cx.tmprej.needs_removal = needs;
                                    if !needs {
                                        util::unregister_temp(TMP_REJ);
                                    }
                                }
                            }
                        }
                    } else {
                        util::say(b"\n");
                    }
                } else {
                    util::say(b"\n");
                }
            }
        }
        util::set_signals(true);
    }
    if let Some(mut o) = outstate.ofp.take() {
        let flushed = o.w.flush().is_ok();
        let (file, _) = o.w.into_parts();
        if !flushed || !util::close_file(file) {
            cx.write_fatal();
        }
    }
    cx.cleanup();
    cx.delete_files();
    i32::from(somefailed)
}

fn main() {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let program_name = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_else(|| b"patch".to_vec());
    let mut cx = Ctx::new(program_name);

    cx.initial_time = util::now();

    if let Some(v) = std::env::var_os("QUOTING_STYLE") {
        cx.quoting = util::quoting_style(&os_bytes(&v)).unwrap_or(quoting::Style::Shell);
    }

    cx.posixly_correct = std::env::var_os("POSIXLY_CORRECT").is_some();
    cx.backup_if_mismatch = !cx.posixly_correct;
    if let Some(v) = std::env::var_os("PATCH_GET") {
        cx.patch_get = cx.numeric_string(&os_bytes(&v), true, "PATCH_GET value");
    }

    cx.simple_backup_suffix = match std::env::var_os("SIMPLE_BACKUP_SUFFIX") {
        Some(v) if !v.is_empty() => os_bytes(&v).into_owned(),
        _ => b".orig".to_vec(),
    };

    if let Some(v) = std::env::var_os("PATCH_VERSION_CONTROL") {
        cx.version_control = Some(os_bytes(&v).into_owned());
        cx.version_control_context = "$PATCH_VERSION_CONTROL";
    } else if let Some(v) = std::env::var_os("VERSION_CONTROL") {
        cx.version_control = Some(os_bytes(&v).into_owned());
        cx.version_control_context = "$VERSION_CONTROL";
    }

    cx.get_some_switches(&argv);
    cx.safe.debug = cx.debug;

    let status = run(&mut cx);
    cx.exit(status);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::similar;

    #[test]
    fn white_space_is_canonicalized_as_similar_does() {
        assert!(similar(b"a b\n", b"a  b"));
        assert!(similar(b"a\tb", b"a b\n"));
        assert!(!similar(b"ab", b"a b"));
        assert!(similar(b"  a", b" a"));
        assert!(!similar(b"a", b" a"));
        assert!(similar(b"a  ", b"a"));
        assert!(similar(b"", b"\n"));
    }
}
