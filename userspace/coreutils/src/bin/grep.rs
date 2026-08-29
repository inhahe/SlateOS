//! grep — select the lines of its input that match a pattern.
//!
//! ```text
//! grep [OPTION]... PATTERN [FILE]...
//! grep [OPTION]... -e PATTERN... [FILE]...
//! grep [OPTION]... -f PATTERNFILE... [FILE]...
//! ```
//!
//! | | |
//! |---|---|
//! | `-E` | PATTERN is an extended regular expression |
//! | `-F` | PATTERN is a fixed string, not a pattern |
//! | `-e P` | use P as a pattern; may be repeated |
//! | `-f F` | read patterns from file F, one per line |
//! | `-i` | match without regard to letter case |
//! | `-v` | select the lines that do *not* match |
//! | `-w` | the match must be a whole word |
//! | `-x` | the match must be the whole line |
//! | `-c` | print a count of selected lines instead of the lines |
//! | `-l` / `-L` | print the names of files that do / do not match |
//! | `-o` | print only the matching part of each line |
//! | `-n` | prefix each line with its line number |
//! | `-H` / `-h` | always / never prefix a line with its file name |
//! | `-q` | print nothing; the exit status is the answer |
//! | `-s` | do not report unreadable files |
//! | `-m N` | stop after N selected lines per file; `-m 0` prints nothing |
//! | `-r` / `-R` | search directories recursively; `-R` follows symlinks it finds |
//! | `-d A` | do `A` with a directory: `read`, `recurse` (which is `-r`) or `skip` |
//! | `-D A` | do `A` with a device, socket or FIFO: `read` or `skip` |
//! | `--include=G` / `--exclude=G` | search only / never the files whose name matches glob `G` |
//! | `--exclude-from=F` | read `--exclude` globs from file `F`, one per line |
//! | `--exclude-dir=G` | do not descend into a directory whose name matches `G` |
//! | `-Z` | write a NUL after a file name instead of the `:` or newline |
//! | `-z` | the input is NUL-separated too, and so is the output |
//! | `-a` | accepted and ignored: this grep never suppresses binary output |
//! | `--` | end of options; what follows is a pattern or a file |
//!
//! Exit status: 0 if a line was selected, 1 if none was, 2 on an error.
//!
//! ## Patterns are patterns
//!
//! Until `userspace/ere` existed this program matched with `str::contains`, so
//! `grep '^posix'` found nothing, `grep ' [ab]='` matched only a line that
//! literally contained `[ab]=`, and `-E` was an unknown option. It was a
//! quiet failure — the exit status said "no match", which is what a real grep
//! says about a file that does not match — and it survived its own test suite
//! because every test asserted the substring behaviour it had. See
//! `design-decisions.md` §322.
//!
//! Patterns are POSIX **Basic** regular expressions by default, *egrep*
//! syntax under `-E` — which is not quite POSIX-extended; see `ere::Syntax` —
//! and literal text under `-F`. Lines are bytes: a path on this system may
//! hold any byte but `/` and NUL, so a grep that insisted on UTF-8 could not
//! search a file listing.
//!
//! ## `-Z` and `-z` are not decoration here
//!
//! On a system whose paths may contain a newline, `grep -rl … | xargs` is
//! ambiguous by construction: the delimiter is a byte a name is allowed to
//! hold. `-Z` delimits names with the one byte a name cannot hold, and `-z`
//! makes grep read such a stream, so `find -print0 | xargs -0 grep -z` and
//! `grep -rlZ … | xargs -0` are the spellings that are actually correct.

use coreutils::diag;
use coreutils::filekind;
// Aliased for the same reason `ere::Syntax` is: `Flags` is far too plain a name
// to stand unqualified next to grep's own option soup.
use coreutils::fnmatch::{Flags as FnmatchFlags, fnmatch};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{self, quotef_os};
use coreutils::stdfd;
use coreutils::xnum;
use std::borrow::Cow;
use std::collections::{BTreeSet, VecDeque};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use coreutils::errmsg::strerror;
// Aliased: this file already has a `Syntax` — the `-G`/`-E`/`-F` selector —
// and `ere::Syntax` is the dialect that selector maps *onto*.
use ere::{MatchLimit, Regex, Syntax as EreSyntax, bre};

// Before `main`, so that `stdfd::restore` still sees a caller's `grep … >&-` as
// the closed descriptor it is rather than the `/dev/null` the Rust runtime
// substitutes for it before `main` runs. Without it `grep a abc >&-` prints
// nothing and exits 0, where GNU says `grep: write error: Bad file descriptor`
// and exits 2.
coreutils::guard_std_fds!();

/// Which language the patterns are written in.
#[derive(Clone, Copy, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Syntax {
    /// POSIX Basic regular expressions — grep's default, where `+` and `?` are
    /// literal characters and groups are spelled `\(…\)`.
    #[default]
    Basic,
    /// POSIX Extended regular expressions (`-E`).
    Extended,
    /// Literal text (`-F`). Not a language at all, but it reaches the same
    /// engine by being quoted into one, so `-i`, `-w` and `-x` need no second
    /// implementation.
    Fixed,
}

/// What is printed between two groups of output that are not adjacent in the
/// file.
///
/// Three states rather than an `Option<Vec<u8>>` because *empty* and *absent*
/// are different answers: `--group-separator=` prints a blank line, while
/// `--no-group-separator` prints nothing at all. An `Option` would have to
/// spell one of the two as `Some(b"")` and then remember which.
#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum GroupSep {
    /// GNU's default, `--`.
    #[default]
    Dashes,
    /// `--group-separator=SEP`.
    Custom(Vec<u8>),
    /// `--no-group-separator`.
    Suppressed,
}

impl GroupSep {
    /// The bytes written before the separator's newline, or `None` when no
    /// separator line is written at all.
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Dashes => Some(b"--"),
            Self::Custom(s) => Some(s),
            Self::Suppressed => None,
        }
    }
}

/// `--color[=WHEN]`.
///
/// The argument is *optional* — `grep --color foo file` means `auto` and does
/// not eat `foo` — which is why this cannot go through the same "value or the
/// next argv entry" path as `--context`.
#[derive(Clone, Copy, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum ColorWhen {
    #[default]
    Never,
    Always,
    /// Colour only when standard output is a terminal. Resolved once, after
    /// parsing, rather than asked per line.
    Auto,
}

/// The eight SGR capabilities `GREP_COLORS` names, and the two booleans.
///
/// Each is stored as the *parameter* text — `01;31`, not the whole escape — so
/// that "unset" and "empty" are the same thing and mean "write this text
/// plainly", which is what GNU's default `sl=`/`cx=` rely on.
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Colors {
    /// `ms`: matched text on a *selected* line.
    selected_match: Vec<u8>,
    /// `mc`: matched text on a *context* line. `-v` is what makes the two
    /// visibly different: under it the matches are on the context lines.
    context_match: Vec<u8>,
    /// `sl`: everything else on a selected line. Empty by default, which is why
    /// plain `grep --color=always` colours only the matches.
    selected_line: Vec<u8>,
    /// `cx`: everything else on a context line. Empty by default.
    context_line: Vec<u8>,
    /// `fn`: the file name in a prefix, and in `-c`/`-l`/`-L` output.
    filename: Vec<u8>,
    /// `ln`: the line number.
    line_number: Vec<u8>,
    /// `bn`: the byte offset.
    byte_number: Vec<u8>,
    /// `se`: every separator — the `:` and `-` between prefix fields, and the
    /// `--` between context groups.
    separator: Vec<u8>,
    /// `rv`: swap `sl` and `cx`, but only when `-v` is also in effect. The
    /// point is that under `-v` the *selected* lines are the boring ones.
    reverse_video: bool,
    /// `ne`: do not append "erase in line" (`\e[K`) to each escape. It is there
    /// by default so that a highlight reaching the end of a line does not paint
    /// the rest of the terminal row on a screen whose background colour differs.
    no_erase: bool,
}

impl Default for Colors {
    /// GNU's defaults: `ms=01;31:mc=01;31:sl=:cx=:fn=35:ln=32:bn=32:se=36`.
    fn default() -> Self {
        Self {
            selected_match: b"01;31".to_vec(),
            context_match: b"01;31".to_vec(),
            selected_line: Vec::new(),
            context_line: Vec::new(),
            filename: b"35".to_vec(),
            line_number: b"32".to_vec(),
            byte_number: b"32".to_vec(),
            separator: b"36".to_vec(),
            reverse_video: false,
            no_erase: false,
        }
    }
}

impl Colors {
    /// The escape that begins a run of `cap`-coloured output, or nothing at all
    /// when `cap` is empty — an empty capability means "write it plainly", and
    /// emitting `\e[m` for it would reset a colour the caller had set.
    fn start(&self, cap: &[u8]) -> Vec<u8> {
        if cap.is_empty() {
            return Vec::new();
        }
        let mut v = b"\x1b[".to_vec();
        v.extend_from_slice(cap);
        v.push(b'm');
        if !self.no_erase {
            v.extend_from_slice(b"\x1b[K");
        }
        v
    }

    /// The escape that ends one, matched to [`Colors::start`] — nothing when
    /// `cap` is empty.
    fn end(&self, cap: &[u8]) -> Vec<u8> {
        if cap.is_empty() {
            return Vec::new();
        }
        let mut v = b"\x1b[m".to_vec();
        if !self.no_erase {
            v.extend_from_slice(b"\x1b[K");
        }
        v
    }

    /// `text` wrapped in `cap`'s pair.
    fn wrap(&self, cap: &[u8], text: &[u8]) -> Vec<u8> {
        let mut v = self.start(cap);
        v.extend_from_slice(text);
        v.extend_from_slice(&self.end(cap));
        v
    }

    /// Apply one `GREP_COLORS` specification: `key=value` pairs and bare
    /// boolean keys, separated by `:`.
    ///
    /// A key that is not one of the ten, and a value that is not SGR
    /// parameters, are both ignored **in silence** — measured, and it is the
    /// only tolerable behaviour for a variable that is set once in a shell
    /// profile and then inherited by every grep in every script.
    fn apply(&mut self, spec: &[u8]) {
        for item in spec.split(|&b| b == b':') {
            let (key, value, valued) = match item.iter().position(|&b| b == b'=') {
                Some(i) => (
                    item.get(..i).unwrap_or_default(),
                    item.get(i.saturating_add(1)..).unwrap_or_default(),
                    true,
                ),
                // No `=` at all. `rv` and `ne` are booleans and still fire;
                // `GREP_COLORS=ms` is *ignored* rather than read as `ms=`,
                // which is the difference between "no highlight" and the
                // default one. Measured.
                None => (item, &[][..], false),
            };
            // A capability is SGR parameters: digits and `;`. Anything else is
            // not something to hand to a terminal.
            let sane = valued && value.iter().all(|b| b.is_ascii_digit() || *b == b';');
            let set = |field: &mut Vec<u8>| {
                if sane {
                    *field = value.to_vec();
                }
            };
            match key {
                b"ms" => set(&mut self.selected_match),
                b"mc" => set(&mut self.context_match),
                // `mt` is both at once, and order decides: the last assignment
                // to a field wins, so `ms=…:mt=…` and `mt=…:ms=…` differ.
                b"mt" => {
                    set(&mut self.selected_match);
                    set(&mut self.context_match);
                }
                b"sl" => set(&mut self.selected_line),
                b"cx" => set(&mut self.context_line),
                b"fn" => set(&mut self.filename),
                b"ln" => set(&mut self.line_number),
                b"bn" => set(&mut self.byte_number),
                b"se" => set(&mut self.separator),
                b"rv" => self.reverse_video = true,
                b"ne" => self.no_erase = true,
                _ => {}
            }
        }
    }
}

/// `-d ACTION` / `--directories=ACTION`: what to do with a directory.
///
/// `-r` and `-R` are not a separate setting — they *are* `-d recurse`, which is
/// why the last of `-r` and `-d skip` wins whichever order they are written in.
/// Measured: `grep -r -d skip foo dir` skips, `grep -d skip -r foo dir`
/// recurses, and `grep -r -d read foo dir` says `Is a directory`. Modelling
/// recursion as its own `bool` — which this did until 2026-08-25 — cannot
/// express that, because two independent flags have no order between them.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Directories {
    /// `read`, the default: try to read the directory, which fails with
    /// `Is a directory` and status 2. `-s` silences the message; the status
    /// stands, because the file was named and not searched.
    #[default]
    Read,
    /// `recurse`: what `-r` and `-R` set.
    Recurse,
    /// `skip`: pass over it in silence, and **without** raising the status —
    /// `grep -d skip foo dir` exits 1, not 2. Skipping is not an error.
    Skip,
}

/// `-D ACTION` / `--devices=ACTION`: what to do with a character device, block
/// device, socket or FIFO.
///
/// Three states rather than two, because the default is neither "read" nor
/// "skip": it reads a device **named on the command line** and skips one the
/// recursive walk **finds**. That asymmetry is what lets `grep -r pat /` finish
/// on a system with FIFOs in it while `grep pat /dev/stdin` still works.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Devices {
    /// The default: read a device the command line names, skip one the walk
    /// finds.
    #[default]
    CommandLine,
    /// `read`: read them wherever they are found. `grep -D read -r pat .` over
    /// a tree holding a FIFO with no writer blocks forever — GNU does too.
    Read,
    /// `skip`: never read one.
    Skip,
}

/// One run of consecutive same-kind selector options.
///
/// `--include a --include b --exclude c` is two segments, not three patterns:
/// a run of `--include`s coalesces into one, and so does a run of `--exclude`s
/// (`--exclude-from` extends the current exclude run rather than starting a new
/// one). A segment matches when *any* of its globs does.
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Segment {
    /// Whether matching this segment means *keep* (`--include`) or *drop*
    /// (`--exclude`, `--exclude-from`, `--exclude-dir`).
    include: bool,
    globs: Vec<Vec<u8>>,
}

/// The `--include`/`--exclude` list for one kind of name — files, or
/// directories — and the rule that turns it into a yes or a no.
///
/// # The rule
///
/// This is gnulib's `excluded_file_name`, and it is not the "last one wins" or
/// "include beats exclude" that either reading of the manual suggests. Three
/// steps, in order:
///
/// 1. Try the segments **newest first**. The first one that matches decides:
///    an include segment means keep, an exclude segment means drop.
/// 2. If none matches, look at the **oldest** segment. Drop iff it is an
///    include — because a command that opens with `--include` is a whitelist,
///    and a whitelist's default is to reject.
/// 3. With no segments at all, keep.
///
/// Step 2 is the surprising one, and together with step 1 it makes swapping two
/// options change far more than their order. Measured, GNU grep 3.11:
///
/// | command | `s1.txt` | `s2.log` | `s2.txt` |
/// |---|---|---|---|
/// | `--include='*.txt' --exclude='s1*'` | dropped — newest segment matches, and it excludes | dropped — nothing matches, and the oldest segment is an include | kept |
/// | `--exclude='s1*' --include='*.txt'` | **kept** — newest segment matches, and it includes | kept — nothing matches, and the oldest segment is an exclude | kept |
///
/// So the second command searches *everything*, `s1.txt` included: written in
/// that order the `--exclude` cannot reject anything the `--include` names, and
/// cannot reject anything it does not name either.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Selectors {
    segments: Vec<Segment>,
}

impl Selectors {
    /// Add one glob, extending the newest segment when it is of the same kind.
    fn push(&mut self, include: bool, glob: Vec<u8>) {
        match self.segments.last_mut() {
            Some(seg) if seg.include == include => seg.globs.push(glob),
            _ => self.segments.push(Segment {
                include,
                globs: vec![glob],
            }),
        }
    }

    /// Whether `name` is *excluded* — the sense gnulib's function returns, and
    /// the sense the callers want, since "no selectors at all" must answer
    /// `false`.
    fn excludes(&self, name: &[u8]) -> bool {
        for seg in self.segments.iter().rev() {
            if seg.globs.iter().any(|g| glob_matches(g, name)) {
                return !seg.include;
            }
        }
        // Nothing matched: the oldest segment sets the default.
        self.segments.first().is_some_and(|seg| seg.include)
    }
}

/// gnulib's `exclude_fnmatch` without `EXCLUDE_ANCHORED`: the glob is tried
/// against the whole name, and then against each suffix of it that begins just
/// after a `/`.
///
/// The suffix pass is why `grep --exclude='top.txt' foo ./top.txt` excludes the
/// file even though the operand was written with a `./` on the front, and why
/// `grep --exclude-dir='su*' -r foo ./sub` skips the directory. It is invisible
/// for a name the walk found, because the walk matches base names, which hold
/// no `/` — see [`Options::skipped_file`].
///
/// `Flags::NONE`, deliberately: grep passes no `FNM_PATHNAME`, so `*` crosses a
/// `/`, and no `FNM_PERIOD`, so `--include='*'` matches a dotfile. `\` still
/// escapes.
fn glob_matches(glob: &[u8], name: &[u8]) -> bool {
    if fnmatch(glob, name, FnmatchFlags::NONE) {
        return true;
    }
    for (i, b) in name.iter().enumerate() {
        // `p[1] != '/'` is gnulib's, and it is what stops `a//b` offering the
        // suffix `/b` as well as `b`.
        if *b == b'/' && name.get(i.saturating_add(1)) != Some(&b'/') {
            let suffix = name.get(i.saturating_add(1)..).unwrap_or_default();
            if fnmatch(glob, suffix, FnmatchFlags::NONE) {
                return true;
            }
        }
    }
    false
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    syntax: Syntax,
    ignore_case: bool,
    invert: bool,
    count_only: bool,
    line_numbers: bool,
    /// `-d`/`--directories`, which `-r` and `-R` also write. See
    /// [`Directories`] for why recursion is a *value of this* rather than a
    /// flag beside it.
    directories: Directories,
    /// `-D`/`--devices`.
    devices: Devices,
    /// `--include`, `--exclude` and `--exclude-from`, in the order written.
    /// Consulted for anything that is not a directory.
    file_selectors: Selectors,
    /// `--exclude-dir`, in the order written. Consulted for directories, and
    /// only ever holding exclude segments — GNU has no `--include-dir`, so a
    /// `--include` never reaches a directory and cannot filter the walk by the
    /// names of the directories in it.
    dir_selectors: Selectors,
    /// `-R`: also follow a symbolic link found *during* the walk.
    ///
    /// `-r` and `-R` differ over exactly this. Both follow a link named on the
    /// command line — `grep -r foo link-to-dir` descends it — and only `-R`
    /// follows one it discovers. Ours followed them always until 2026-08-25,
    /// which reported files GNU does not and, on a tree containing a link back
    /// to one of its own ancestors, did not terminate.
    deref_links: bool,
    quiet: bool,
    /// `-w`: the match must be bounded by non-word characters.
    word: bool,
    /// `-x`: the match must be the entire line.
    whole_line: bool,
    /// `-o`: print the matching part rather than the line.
    only_matching: bool,
    files_with_matches: bool,
    files_without_match: bool,
    no_messages: bool,
    /// `-H`/`-h`, when given: overrides the usual "prefix when searching more
    /// than one file" rule.
    filename: Option<bool>,
    max_count: Option<usize>,
    /// `-Z`: write a NUL after a file name instead of the `:` or newline that
    /// normally follows it.
    ///
    /// Not a convenience. A path on this system may hold any byte but `/` and
    /// NUL — newline included — so `grep -rl … | xargs` is ambiguous by
    /// construction and `grep -rlZ … | xargs -0` is the only spelling that is
    /// not. The one byte a name cannot contain is the one that delimits it.
    null_name: bool,
    /// `-A N`: lines of trailing context printed after each selected line.
    ///
    /// `Option`, not a plain `usize`, for two reasons that a zero cannot carry.
    /// `-A 0` is not "no context": it still puts a `--` between groups that are
    /// not adjacent, which plain `grep` never does. And an unset `-A` falls
    /// back to `-C`'s value *after* parsing finishes, which is what makes
    /// `-A 3 -C 1` and `-C 1 -A 3` the same command — under a plain `usize`
    /// the later option would overwrite the earlier one and they would differ.
    after_context: Option<usize>,
    /// `-B N`: lines of leading context printed before each selected line.
    before_context: Option<usize>,
    /// `-C N`, `--context=N`, or the digit shorthand `-N`: the value that
    /// `-A` and `-B` fall back to when they were not given one.
    default_context: Option<usize>,
    /// `--group-separator=SEP` / `--no-group-separator`.
    group_sep: GroupSep,
    /// `-b`: prefix each printed line with its byte offset in the file.
    ///
    /// The offset is of *what is printed*, not of the line: under `-o` each
    /// match carries its own, so `grep -bo foo` over `foo bar foo` at offset 6
    /// prints `6` and `14`. Under `-z` it still counts bytes, NUL separators
    /// included.
    byte_offset: bool,
    /// `-T`: line the bodies up, by padding the numeric prefix fields to a
    /// common width and ending the prefix with a tab.
    ///
    /// The width is neither a constant nor the widest value actually printed —
    /// it is fixed *before the first line is read*, from the file's size, which
    /// is why it can be applied to a stream. Measured against GNU grep 3.11:
    /// the digit count of the size, plus one when `-n` is on because a file of
    /// N bytes can hold N+1 lines. So a 99-byte file pads line numbers to three
    /// columns and byte offsets to two, and a 9-byte file pads them to two and
    /// one. See [`offset_width`].
    align_tabs: bool,
    /// `--color=WHEN` as it was written on the command line. Turned into
    /// `color` — the answer actually used — once, after parsing, because `auto`
    /// asks the operating system a question and a line of output is the wrong
    /// place to ask it.
    color_when: ColorWhen,
    /// Whether output is coloured at all: [`Options::color_when`] resolved.
    color: bool,
    /// The palette, from `GREP_COLORS`. Read even when `color` is false, so
    /// that a malformed variable is ignored the same way either way.
    colors: Colors,
    /// `-z`: the *input* is NUL-separated too, and so is the output.
    ///
    /// The other half of the same pipeline: `find -print0 | xargs -0 grep -z`
    /// only works if grep agrees about what a line is. Kept separate from
    /// `null_name` because GNU keeps them separate, and because `-z` alone
    /// changes what a line *is* while `-Z` alone only changes how a name is
    /// punctuated.
    null_data: bool,
    /// `--label=NAME`: what standard input is *called* in a prefix or a
    /// diagnostic, in place of `(standard input)`.
    ///
    /// Bytes, because it goes into a prefix beside a file name and a name on
    /// this system may hold any byte but `/` and NUL. It renames only the
    /// stream read from fd 0 — an operand spelled `-` is that stream, so
    /// `grep --label=x foo -` prints `x:`, but a *file* actually named `-`
    /// cannot be reached either way and is not what this renames.
    label: Option<Vec<u8>>,
    /// `--line-buffered`: flush after every printed line.
    ///
    /// Accepted and honoured rather than ignored, because the whole point of
    /// the option is what a *reader on the other end of a pipe* sees, and a
    /// program that accepted it without flushing would answer the question
    /// wrongly in exactly the case the caller wrote it for.
    line_buffered: bool,
    /// `-I` / `--binary-files=without-match`: skip a file that looks binary.
    ///
    /// This grep has no other binary handling — it never suppresses output for
    /// input it thinks is binary — so `binary` and `text` are the same setting
    /// here and only `without-match` does anything. See [`BinaryFiles`].
    binary_files: BinaryFiles,
}

/// `--binary-files=TYPE`, and the `-a` / `-I` shorthands for two of its values.
///
/// GNU has three behaviours; this grep has two, because it never replaces a
/// matching line with `Binary file F matches`. So `Binary` and `Text` are
/// indistinguishable here and `WithoutMatch` is the only value that changes
/// anything. The three are still kept apart rather than collapsed into a
/// `bool`, because the *option* has three values and a caller who writes
/// `--binary-files=binary` after `-I` is turning the skipping back off.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum BinaryFiles {
    /// GNU's default, and the only one that differs from `Text` upstream.
    #[default]
    Binary,
    /// `-a`. Identical to [`BinaryFiles::Binary`] here.
    Text,
    /// `-I`. A file holding a NUL in its first read is skipped entirely.
    WithoutMatch,
}

impl Options {
    /// Whether directories are walked — `-r`, `-R` or `-d recurse`.
    fn recursive(&self) -> bool {
        self.directories == Directories::Recurse
    }

    /// gnulib's `skip_devices`: whether a device found *here* is passed over.
    ///
    /// `command_line` distinguishes the two places a device can turn up,
    /// because the default setting treats them differently — see [`Devices`].
    fn skip_devices(&self, command_line: bool) -> bool {
        self.devices == Devices::Skip || (self.devices == Devices::CommandLine && !command_line)
    }

    /// GNU's `skipped_file`: whether the selectors reject this name.
    ///
    /// Which list is asked depends on `is_dir` and on nothing else — so
    /// `--exclude=sub` does not stop `grep -r pat sub`, and `--exclude-dir=sub`
    /// does, even without `-r`.
    ///
    /// **`name` is not the path.** For an operand it is the operand exactly as
    /// written, `./` and all; for an entry the walk found it is that entry's
    /// **base name**, never the path the walk built up to reach it. That is
    /// GNU's `ent->fts_name`, and it is why `--exclude='sub/s1'` excludes
    /// nothing under `-r` while `--exclude='s1'` excludes it at every depth.
    fn skipped_file(&self, name: &[u8], is_dir: bool) -> bool {
        if is_dir {
            self.dir_selectors.excludes(name)
        } else {
            self.file_selectors.excludes(name)
        }
    }

    /// The byte that ends a line of input and of output: `\n`, or NUL under
    /// `-z`.
    fn line_sep(&self) -> u8 {
        if self.null_data { 0 } else { b'\n' }
    }

    /// Lines of trailing context, with `-C`'s value standing in for an unset
    /// `-A`.
    fn out_after(&self) -> usize {
        self.after_context.or(self.default_context).unwrap_or(0)
    }

    /// Lines of leading context, with `-C`'s value standing in for an unset
    /// `-B`.
    fn out_before(&self) -> usize {
        self.before_context.or(self.default_context).unwrap_or(0)
    }

    /// Whether the caller asked for context *at all* — which is not the same
    /// as asking for a non-zero amount of it.
    ///
    /// This is what gates the `--` between groups, and it is why `-A 0` and
    /// plain `grep` differ: both print only the selected lines, but only the
    /// first separates non-adjacent runs of them.
    fn context_requested(&self) -> bool {
        self.after_context.is_some()
            || self.before_context.is_some()
            || self.default_context.is_some()
    }

    /// Whether context is *printed* at all.
    ///
    /// `-c`, `-l`, `-L` and `-q` answer a question about the file rather than
    /// about its lines, and GNU ignores `-A`/`-B`/`-C` outright under each of
    /// them — including the group separator.
    fn context_printed(&self) -> bool {
        !self.count_only && !self.quiet && !self.files_with_matches && !self.files_without_match
    }

    /// `text` wrapped in `cap`, or `text` alone when `--color` is off.
    ///
    /// Every coloured field goes through here rather than through
    /// [`Colors::wrap`] directly, so that "is colour on at all" is asked in one
    /// place instead of at each of the eight call sites.
    fn paint(&self, cap: &[u8], text: &[u8]) -> Vec<u8> {
        if self.color {
            self.colors.wrap(cap, text)
        } else {
            text.to_vec()
        }
    }

    /// The capability for the body of a line — `sl` for a selected line, `cx`
    /// for a context one, swapped when `rv` is set *and* `-v` is in effect.
    ///
    /// `rv` exists because `-v` inverts which lines are interesting: the
    /// selected ones are the ones that did **not** match, so a caller who
    /// colours selected lines specially wants that colouring to follow the
    /// matches rather than the selection.
    fn line_cap(&self, selected: bool) -> &[u8] {
        if selected ^ (self.invert && self.colors.reverse_video) {
            &self.colors.selected_line
        } else {
            &self.colors.context_line
        }
    }

    /// The capability for matched text within a line: `ms` on a selected line,
    /// `mc` on a context one. Unlike [`Options::line_cap`] this follows the
    /// line's kind directly — `rv` does not touch it.
    fn match_cap(&self, selected: bool) -> &[u8] {
        if selected {
            &self.colors.selected_match
        } else {
            &self.colors.context_match
        }
    }
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct GrepArgs {
    opts: Options,
    /// Patterns given directly, by `-e` or as the first operand.
    patterns: Vec<Vec<u8>>,
    /// Files named by `-f`, whose lines are patterns. Read by `main`, so that
    /// argument parsing stays a pure function of argv.
    pattern_files: Vec<OsString>,
    files: Vec<OsString>,
    /// Whether the sole operand is a `.` this parser supplied rather than one
    /// the caller wrote, in which case the walk's names print without their
    /// leading `./`. GNU's `omit_dot_slash`.
    omit_dot_slash: bool,
}

/// A compiled pattern.
///
/// The engine rejects an empty pattern, as glibc does — but `grep ''` is not a
/// malformed pattern, it is the pattern that matches every line, and scripts
/// use it. So the empty case is carried here rather than pushed into the
/// engine, where it would have to be spelled as a regex that matches empty at
/// every position and would then be indistinguishable from one.
enum Pat {
    Empty,
    Re(Regex),
}

/// GNU grep's own `short_options`, copied from `grep.c` rather than derived.
///
/// Two things about it are not obvious. The ten digits are options in their own
/// right — `-2` is `-C 2` — and are handled by [`parse_args`]'s own scan rather
/// than by a `match` arm, because what a digit means depends on whether the
/// digit before it was in the same word; see [`DIGIT_LIMIT`]. And options this
/// grep does not implement are listed anyway, colons included: drop the `X:`
/// and `grep -X pcre foo f` leaves `pcre` behind as an operand, which is a
/// wrong answer rather than a refusal.
const SHORT_OPTIONS: &str = "0123456789A:B:C:D:EFGHIPTUVX:abcd:e:f:hiLlm:noqRrsuvwxyZz";

/// GNU grep's `long_options[]`, in **its** order rather than alphabetically.
///
/// The order is output: `getopt_long` lists an ambiguous prefix's candidates in
/// table order, so `grep --n` names `--no-ignore-case` before `--no-filename`.
/// `scripts/getopt-ambiguity-check.py` compares this table name-for-name
/// against the readout of `grep --=x`, which is what keeps the two in step.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("basic-regexp", Takes::Nothing),
    ("extended-regexp", Takes::Nothing),
    ("fixed-regexp", Takes::Nothing),
    ("fixed-strings", Takes::Nothing),
    ("perl-regexp", Takes::Nothing),
    ("after-context", Takes::Required),
    ("before-context", Takes::Required),
    ("binary-files", Takes::Required),
    ("byte-offset", Takes::Nothing),
    ("context", Takes::Required),
    ("color", Takes::Optional),
    ("colour", Takes::Optional),
    ("count", Takes::Nothing),
    ("devices", Takes::Required),
    ("directories", Takes::Required),
    ("exclude", Takes::Required),
    ("exclude-from", Takes::Required),
    ("exclude-dir", Takes::Required),
    ("file", Takes::Required),
    ("files-with-matches", Takes::Nothing),
    ("files-without-match", Takes::Nothing),
    ("group-separator", Takes::Required),
    ("help", Takes::Nothing),
    ("include", Takes::Required),
    ("ignore-case", Takes::Nothing),
    ("no-ignore-case", Takes::Nothing),
    ("initial-tab", Takes::Nothing),
    ("label", Takes::Required),
    ("line-buffered", Takes::Nothing),
    ("line-number", Takes::Nothing),
    ("line-regexp", Takes::Nothing),
    ("max-count", Takes::Required),
    ("no-filename", Takes::Nothing),
    ("no-group-separator", Takes::Nothing),
    ("no-messages", Takes::Nothing),
    ("null", Takes::Nothing),
    ("null-data", Takes::Nothing),
    ("only-matching", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("recursive", Takes::Nothing),
    ("dereference-recursive", Takes::Nothing),
    ("regexp", Takes::Required),
    ("invert-match", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("text", Takes::Nothing),
    ("binary", Takes::Nothing),
    ("unix-byte-offsets", Takes::Nothing),
    ("version", Takes::Nothing),
    ("with-filename", Takes::Nothing),
    ("word-regexp", Takes::Nothing),
];

/// The spellings that name **one** option, for the ambiguity rule.
///
/// See `getopt::Program::resolve_long_aliased`: without these, `--colo` would
/// be refused as ambiguous between `--color` and `--colour`, which are the same
/// option. Upstream judges ambiguity by `struct option`'s `val`, which is what
/// this stands in for.
///
/// `no-group-separator` is here because upstream gives it the same `val` as
/// `--group-separator` and tells the two apart by whether `optarg` is NULL. No
/// prefix can match both names, so the pairing changes no output; it is
/// recorded because this table is a transcription, and leaving the pair out
/// would make the transcription say something upstream does not.
const ALIASES: &[(&str, &str)] = &[
    ("fixed-regexp", "fixed-strings"),
    ("colour", "color"),
    ("silent", "quiet"),
    ("no-group-separator", "group-separator"),
];

/// grep's usage status is **2**, not the family's 1: it has already spent 1 on
/// "nothing was selected".
const GREP: Program = Program::new("grep", 2);

/// What upstream's `usage (EXIT_TROUBLE)` prints.
///
/// It is two lines rather than one because grep's diagnostics do not have
/// `getopt::Error::message`'s shape: upstream puts the `Usage:` summary
/// *between* the diagnostic and the `Try '…'` referral, so the two halves are
/// printed by [`run_main`] around it rather than joined by the module.
const USAGE: &str = "\
Usage: grep [OPTION]... PATTERNS [FILE]...
Try 'grep --help' for more information.";

/// What `--help` prints, on **stdout**, exiting 0.
///
/// GNU grep 3.11's own text, word for word, minus the four-line
/// `Report bugs to: …` footer — three of those lines are GNU project addresses
/// that would send a report about this program to the wrong people, as `ls`
/// and the rest of this family already drop them.
///
/// Verbatim includes the options this grep refuses. `-P` is listed because a
/// real GNU grep built without PCRE lists it too and then answers
/// [`perl_unsupported`]; a help text that quietly omitted it would be claiming
/// to be a *different* grep rather than one without PCRE.
const HELP: &str = "\
Usage: grep [OPTION]... PATTERNS [FILE]...
Search for PATTERNS in each FILE.
Example: grep -i 'hello world' menu.h main.c
PATTERNS can contain multiple patterns separated by newlines.

Pattern selection and interpretation:
  -E, --extended-regexp     PATTERNS are extended regular expressions
  -F, --fixed-strings       PATTERNS are strings
  -G, --basic-regexp        PATTERNS are basic regular expressions
  -P, --perl-regexp         PATTERNS are Perl regular expressions
  -e, --regexp=PATTERNS     use PATTERNS for matching
  -f, --file=FILE           take PATTERNS from FILE
  -i, --ignore-case         ignore case distinctions in patterns and data
      --no-ignore-case      do not ignore case distinctions (default)
  -w, --word-regexp         match only whole words
  -x, --line-regexp         match only whole lines
  -z, --null-data           a data line ends in 0 byte, not newline

Miscellaneous:
  -s, --no-messages         suppress error messages
  -v, --invert-match        select non-matching lines
  -V, --version             display version information and exit
      --help                display this help text and exit

Output control:
  -m, --max-count=NUM       stop after NUM selected lines
  -b, --byte-offset         print the byte offset with output lines
  -n, --line-number         print line number with output lines
      --line-buffered       flush output on every line
  -H, --with-filename       print file name with output lines
  -h, --no-filename         suppress the file name prefix on output
      --label=LABEL         use LABEL as the standard input file name prefix
  -o, --only-matching       show only nonempty parts of lines that match
  -q, --quiet, --silent     suppress all normal output
      --binary-files=TYPE   assume that binary files are TYPE;
                            TYPE is 'binary', 'text', or 'without-match'
  -a, --text                equivalent to --binary-files=text
  -I                        equivalent to --binary-files=without-match
  -d, --directories=ACTION  how to handle directories;
                            ACTION is 'read', 'recurse', or 'skip'
  -D, --devices=ACTION      how to handle devices, FIFOs and sockets;
                            ACTION is 'read' or 'skip'
  -r, --recursive           like --directories=recurse
  -R, --dereference-recursive  likewise, but follow all symlinks
      --include=GLOB        search only files that match GLOB (a file pattern)
      --exclude=GLOB        skip files that match GLOB
      --exclude-from=FILE   skip files that match any file pattern from FILE
      --exclude-dir=GLOB    skip directories that match GLOB
  -L, --files-without-match  print only names of FILEs with no selected lines
  -l, --files-with-matches  print only names of FILEs with selected lines
  -c, --count               print only a count of selected lines per FILE
  -T, --initial-tab         make tabs line up (if needed)
  -Z, --null                print 0 byte after FILE name

Context control:
  -B, --before-context=NUM  print NUM lines of leading context
  -A, --after-context=NUM   print NUM lines of trailing context
  -C, --context=NUM         print NUM lines of output context
  -NUM                      same as --context=NUM
      --group-separator=SEP  print SEP on line between matches with context
      --no-group-separator  do not print separator for matches with context
      --color[=WHEN],
      --colour[=WHEN]       use markers to highlight the matching strings;
                            WHEN is 'always', 'never', or 'auto'
  -U, --binary              do not strip CR characters at EOL (MSDOS/Windows)

When FILE is '-', read standard input.  With no FILE, read '.' if
recursive, '-' otherwise.  With fewer than two FILEs, assume -h.
Exit status is 0 if any line is selected, 1 otherwise;
if any error occurs and -q is not given, the exit status is 2.";

/// How many digits of the `-NUM` shorthand upstream keeps before it gives up.
///
/// `get_nondigit_option` writes into a `char buf[INT_BUFSIZE_BOUND (intmax_t) +
/// 4]` and stops at `buf + sizeof buf - 4`, which for a 64-bit `intmax_t` is 21
/// bytes in. Beyond that it appends `...` and lets `context_length_arg` refuse
/// the result, so a 22-digit run answers
/// `grep: 123456789012345678901...: invalid context length argument` rather
/// than silently taking the first 21 digits.
const DIGIT_LIMIT: usize = 21;

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    /// `--help` — and also `--color=WHEN` with a `WHEN` upstream does not know,
    /// which sets the same `show_help` flag rather than reporting anything. So
    /// `grep --color=bogus foo f` prints the whole help text on **stdout** and
    /// exits 0. Measured against GNU grep 3.11.
    Help,
    /// `-V` / `--version`.
    Version,
    /// No pattern at all: upstream's bare `usage (EXIT_TROUBLE)`, which prints
    /// the two [`USAGE`] lines with no `grep: …` diagnostic above them. A
    /// variant rather than an [`getopt::Error`] because an `Error` always has a
    /// sentence, and this one has none.
    BadUsage,
    Run(Box<GrepArgs>),
}

/// One row of upstream's `matchers[]`: the language `-G`, `-E`, `-F`, `-P` and
/// the undocumented `-X` select between.
///
/// A type of its own rather than [`Syntax`] directly, because upstream refuses
/// a *second, different* matcher — `grep -E -F` is `conflicting matchers
/// specified` — and that is a question about which row was chosen, not about
/// the dialect the row maps onto. `grep -E -E` is fine, and so is `grep -X
/// egrep -E`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Matcher {
    Grep,
    Egrep,
    Fgrep,
    Awk,
    Gawk,
    Posixawk,
}

impl Matcher {
    /// The dialect this row compiles patterns in.
    fn syntax(self) -> Syntax {
        match self {
            Matcher::Grep => Syntax::Basic,
            Matcher::Fgrep => Syntax::Fixed,
            // `awk`, `gawk` and `posixawk` are three more `RE_SYNTAX_*`
            // settings upstream, differing from egrep's in details this engine
            // does not model — whether `\` is special inside a bracket
            // expression, whether back-references exist. Extended is the
            // nearest dialect there is; the divergence is recorded in
            // `known-issues.md` rather than hidden by refusing the names, since
            // refusing them would be the larger lie.
            Matcher::Egrep | Matcher::Awk | Matcher::Gawk | Matcher::Posixawk => Syntax::Extended,
        }
    }
}

/// GNU's `struct option` `val`: the short letter a long option **is**, or one
/// of the long-only options that have no letter.
///
/// Routing both spellings through one value is upstream's design and not a
/// tidying of it: the option loop is a `switch` over exactly this, which is why
/// `--ignore-case` and `-i` cannot drift apart.
#[derive(Clone, Copy)]
enum Flag {
    Short(u8),
    BinaryFiles,
    Color,
    Exclude,
    ExcludeDir,
    ExcludeFrom,
    GroupSeparator,
    NoGroupSeparator,
    Include,
    Label,
    LineBuffered,
    NoIgnoreCase,
    Help,
}

/// The `val` of one long option.
///
/// Every name in [`LONG_OPTIONS`] has a line here, because the table's names
/// are the only strings `getopt` can hand back. The final arm is not dead code
/// for all that: it is what a name added to the table without a line here lands
/// on, and printing the list of options is the least wrong thing to do with an
/// option nobody has finished wiring up.
fn long_flag(name: &str) -> Flag {
    match name {
        "basic-regexp" => Flag::Short(b'G'),
        "extended-regexp" => Flag::Short(b'E'),
        "fixed-regexp" | "fixed-strings" => Flag::Short(b'F'),
        "perl-regexp" => Flag::Short(b'P'),
        "after-context" => Flag::Short(b'A'),
        "before-context" => Flag::Short(b'B'),
        "binary-files" => Flag::BinaryFiles,
        "byte-offset" => Flag::Short(b'b'),
        "context" => Flag::Short(b'C'),
        "color" | "colour" => Flag::Color,
        "count" => Flag::Short(b'c'),
        "devices" => Flag::Short(b'D'),
        "directories" => Flag::Short(b'd'),
        "exclude" => Flag::Exclude,
        "exclude-from" => Flag::ExcludeFrom,
        "exclude-dir" => Flag::ExcludeDir,
        "file" => Flag::Short(b'f'),
        "files-with-matches" => Flag::Short(b'l'),
        "files-without-match" => Flag::Short(b'L'),
        "group-separator" => Flag::GroupSeparator,
        "include" => Flag::Include,
        "ignore-case" => Flag::Short(b'i'),
        "no-ignore-case" => Flag::NoIgnoreCase,
        "initial-tab" => Flag::Short(b'T'),
        "label" => Flag::Label,
        "line-buffered" => Flag::LineBuffered,
        "line-number" => Flag::Short(b'n'),
        "line-regexp" => Flag::Short(b'x'),
        "max-count" => Flag::Short(b'm'),
        "no-filename" => Flag::Short(b'h'),
        "no-group-separator" => Flag::NoGroupSeparator,
        "no-messages" => Flag::Short(b's'),
        "null" => Flag::Short(b'Z'),
        "null-data" => Flag::Short(b'z'),
        "only-matching" => Flag::Short(b'o'),
        "quiet" | "silent" => Flag::Short(b'q'),
        "recursive" => Flag::Short(b'r'),
        "dereference-recursive" => Flag::Short(b'R'),
        "regexp" => Flag::Short(b'e'),
        "invert-match" => Flag::Short(b'v'),
        "text" => Flag::Short(b'a'),
        "binary" => Flag::Short(b'U'),
        "unix-byte-offsets" => Flag::Short(b'u'),
        "version" => Flag::Short(b'V'),
        "with-filename" => Flag::Short(b'H'),
        "word-regexp" => Flag::Short(b'w'),
        _ => Flag::Help,
    }
}

/// The value of an option whose table entry says it takes one.
///
/// `getopt` has already refused the command line where it is missing — that is
/// `option '--file' requires an argument`, printed before this could run — so
/// the `None` is unreachable. It is spelled as a default rather than an
/// `expect` because searching for the empty string is a smaller failure than a
/// panic, and because this family forbids `unwrap`/`expect` outside tests.
fn required(value: Option<OsString>) -> OsString {
    value.unwrap_or_default()
}

/// Upstream's `setmatcher`: choose a pattern language, refusing a second and
/// different one.
///
/// # Errors
///
/// `conflicting matchers specified`, exit 2 — upstream's wording and status.
/// Repeating the *same* matcher is not a conflict.
fn set_matcher(previous: Option<Matcher>, chosen: Matcher) -> Result<Matcher, getopt::Error> {
    match previous {
        Some(p) if p != chosen => Err(GREP.usage("conflicting matchers specified".to_string())),
        _ => Ok(chosen),
    }
}

/// `-X NAME`: upstream's undocumented matcher selector.
///
/// # Errors
///
/// `invalid matcher NAME`, exit 2 — and, for `perl` alone, the message a build
/// without PCRE gives instead. See [`perl_unsupported`].
fn matcher_arg(name: &[u8]) -> Result<Matcher, getopt::Error> {
    match name {
        b"grep" => Ok(Matcher::Grep),
        b"egrep" => Ok(Matcher::Egrep),
        b"fgrep" => Ok(Matcher::Fgrep),
        b"awk" => Ok(Matcher::Awk),
        b"gawk" => Ok(Matcher::Gawk),
        b"posixawk" => Ok(Matcher::Posixawk),
        b"perl" => Err(perl_unsupported()),
        // `%s`, not `quote`: upstream prints the name bare. Lossy, because a
        // `getopt::Error` carries a `String` — a matcher name that is not UTF-8
        // matches none of the six above and so is only ever echoed back.
        other => Err(GREP.usage(format!(
            "invalid matcher {}",
            String::from_utf8_lossy(other)
        ))),
    }
}

/// `-P` / `--perl-regexp` / `-X perl`, which this engine cannot do.
///
/// The wording is upstream's own, from the `#if !HAVE_LIBPCRE` arm of
/// `setmatcher` — a real GNU grep built without PCRE says exactly this. That is
/// deliberate: reporting `-P` as an *unknown option* would tell a script that
/// its grep is too old, where the truth is that this one has no PCRE. Ubuntu's
/// grep is built with PCRE, so `scripts/grep-diff.sh` records the case as an
/// expected difference rather than as a match.
fn perl_unsupported() -> getopt::Error {
    GREP.usage("Perl matching not supported in a --disable-perl-regexp build".to_string())
}

/// Parse grep's argv, exactly as `get_nondigit_option` and the `switch` it
/// feeds do.
///
/// # The digit scan
///
/// `-2` is `-C 2`, and `getopt` cannot be told that, because what a digit means
/// depends on *where it was written*: `-1 -2` is a context of two — the second
/// run replaces the first — while `-12` is a context of twelve. Upstream
/// settles it with three variables around the `getopt_long` call, and this is
/// them: `prev_digit_optind` outlives one run of digits (upstream's `static`),
/// `was_digit` and the digit buffer do not, and `optind` before and after each
/// call is what says whether two digits shared a word. Leading zeros are
/// overwritten rather than accumulated, so `-0005` is a context of five and not
/// an overflow.
///
/// # Errors
///
/// Anything upstream's option loop dies on: an unknown option, a missing or
/// unusable value, or two different `-E`/`-F`/`-G`/`-X` matchers. `--help`,
/// `--version` and a bare `grep` are [`Request`]s rather than errors, because
/// upstream reaches them by falling out of the loop rather than by dying in it.
fn parse_args(argv: &[OsString]) -> Result<Request, getopt::Error> {
    let mut opts = Options::default();
    let mut patterns: Vec<Vec<u8>> = Vec::new();
    let mut pattern_files: Vec<OsString> = Vec::new();
    let mut operands: Vec<OsString> = Vec::new();
    let mut matcher: Option<Matcher> = None;
    let mut show_help = false;
    let mut show_version = false;

    let mut parser = GREP.parse_aliased(argv, SHORT_OPTIONS, LONG_OPTIONS, ALIASES);
    // Upstream's `static int prev_digit_optind`: it outlives one call to
    // `get_nondigit_option`, which is the whole mechanism by which `-1 -2` and
    // `-12` come out different.
    let mut prev_digit_optind: Option<usize> = None;
    'scan: loop {
        let mut digits: Vec<u8> = Vec::new();
        let mut was_digit = false;
        let mut this_digit_optind = parser.optind();
        let item = loop {
            let Some(next) = parser.next() else { break None };
            let opt = next?;
            let digit = match &opt {
                Opt::Short(c @ b'0'..=b'9', None) => *c,
                _ => break Some(opt),
            };
            if prev_digit_optind != Some(this_digit_optind) || !was_digit {
                // A new run: this digit is the first of a fresh number.
                digits.clear();
            } else if digits.first() == Some(&b'0') {
                // Upstream's `p -= buf[0] == '0'`, which overwrites while the
                // number so far is a single leading zero. Without it
                // `-00000000000000000000000` would be refused as too long
                // rather than read as zero.
                digits.pop();
            }
            if digits.len() == DIGIT_LIMIT {
                digits.extend_from_slice(b"...");
                break None;
            }
            digits.push(digit);
            was_digit = true;
            prev_digit_optind = Some(this_digit_optind);
            this_digit_optind = parser.optind();
        };
        if !digits.is_empty() {
            opts.default_context = Some(context_len(&digits)?);
        }
        let Some(item) = item else { break 'scan };

        let (flag, value) = match item {
            Opt::Operand(word) => {
                operands.push(word.clone());
                continue 'scan;
            }
            Opt::Short(c, v) => (Flag::Short(c), v),
            Opt::Long(name, v) => (long_flag(name), v),
        };
        match flag {
            Flag::Short(b'E') => matcher = Some(set_matcher(matcher, Matcher::Egrep)?),
            Flag::Short(b'F') => matcher = Some(set_matcher(matcher, Matcher::Fgrep)?),
            Flag::Short(b'G') => matcher = Some(set_matcher(matcher, Matcher::Grep)?),
            // Before the conflict test, as upstream has it: `setmatcher` looks
            // the name up in a table `perl` is not in when PCRE is absent, so
            // `grep -E -P` reports the missing PCRE and not the conflict.
            Flag::Short(b'P') => return Err(perl_unsupported()),
            Flag::Short(b'X') => {
                let chosen = matcher_arg(&quote::os_bytes(&required(value)))?;
                matcher = Some(set_matcher(matcher, chosen)?);
            }
            // `-y` is the old-timers' spelling of `-i`, and upstream still
            // carries it in the same `case`.
            Flag::Short(b'i' | b'y') => opts.ignore_case = true,
            Flag::NoIgnoreCase => opts.ignore_case = false,
            Flag::Short(b'v') => opts.invert = true,
            Flag::Short(b'c') => opts.count_only = true,
            Flag::Short(b'n') => opts.line_numbers = true,
            Flag::Short(b'r') => opts.directories = Directories::Recurse,
            Flag::Short(b'R') => {
                opts.directories = Directories::Recurse;
                opts.deref_links = true;
            }
            Flag::Short(b'w') => opts.word = true,
            Flag::Short(b'x') => opts.whole_line = true,
            Flag::Short(b'o') => opts.only_matching = true,
            Flag::Short(b'l') => opts.files_with_matches = true,
            Flag::Short(b'L') => opts.files_without_match = true,
            Flag::Short(b'H') => opts.filename = Some(true),
            Flag::Short(b'h') => opts.filename = Some(false),
            Flag::Short(b'q') => opts.quiet = true,
            Flag::Short(b's') => opts.no_messages = true,
            Flag::Short(b'Z') => opts.null_name = true,
            Flag::Short(b'z') => opts.null_data = true,
            Flag::Short(b'b') => opts.byte_offset = true,
            Flag::Short(b'T') => opts.align_tabs = true,
            Flag::Short(b'a') => opts.binary_files = BinaryFiles::Text,
            Flag::Short(b'I') => opts.binary_files = BinaryFiles::WithoutMatch,
            Flag::BinaryFiles => {
                opts.binary_files = binary_files_arg(&quote::os_bytes(&required(value)))?;
            }
            // MS-DOS's "do not strip the CR at end of line". Upstream guards it
            // with `if (O_BINARY)`, which is false on every system this runs
            // on, so it is accepted and does nothing there too.
            Flag::Short(b'U') => {}
            // A warning, not an error: upstream calls `error (0, 0, …)` and
            // carries on, so the search still happens and the status is still
            // the search's. Printed here, in argv order, because that is where
            // upstream prints it.
            Flag::Short(b'u') => diag!("grep: warning: --unix-byte-offsets (-u) is obsolete"),
            Flag::Short(b'V') => show_version = true,
            Flag::Help => show_help = true,
            Flag::Short(b'd') => {
                opts.directories = directories_arg(&quote::os_bytes(&required(value)))?;
            }
            Flag::Short(b'D') => opts.devices = devices_arg(&quote::os_bytes(&required(value)))?,
            Flag::Short(b'A') => {
                opts.after_context = Some(context_len(&quote::os_bytes(&required(value)))?);
            }
            Flag::Short(b'B') => {
                opts.before_context = Some(context_len(&quote::os_bytes(&required(value)))?);
            }
            Flag::Short(b'C') => {
                opts.default_context = Some(context_len(&quote::os_bytes(&required(value)))?);
            }
            Flag::Short(b'e') => {
                patterns.extend(split_arg_patterns(&quote::os_bytes(&required(value))));
            }
            Flag::Short(b'f') => pattern_files.push(required(value)),
            Flag::Short(b'm') => opts.max_count = max_count_arg(&quote::os_bytes(&required(value)))?,
            Flag::Include => opts
                .file_selectors
                .push(true, quote::os_bytes(&required(value)).into_owned()),
            Flag::Exclude => opts
                .file_selectors
                .push(false, quote::os_bytes(&required(value)).into_owned()),
            // Trailing slashes are stripped from the *pattern*, so
            // `--exclude-dir=sub/` and `--exclude-dir=sub` are the same
            // request. Upstream does it with `strip_trailing_slashes`, and
            // without it the pattern could never match, because the names it is
            // compared against never end in one.
            Flag::ExcludeDir => {
                let mut pat = quote::os_bytes(&required(value)).into_owned();
                while pat.len() > 1 && pat.last() == Some(&b'/') {
                    pat.pop();
                }
                opts.dir_selectors.push(false, pat);
            }
            Flag::ExcludeFrom => {
                let path = required(value);
                let raw = fs::read(&path).map_err(|e| {
                    GREP.usage(format!("{}: {}", quotef_os(&path), strerror(&e)))
                })?;
                for pat in split_exclude_file(&raw) {
                    opts.file_selectors.push(false, pat);
                }
            }
            // `--group-separator=` with nothing after the `=` is a *blank line*
            // between groups, not a request for no separator — which is why an
            // empty value is stored rather than folded into `Suppressed`.
            Flag::GroupSeparator => {
                opts.group_sep = GroupSep::Custom(quote::os_bytes(&required(value)).into_owned());
            }
            Flag::NoGroupSeparator => opts.group_sep = GroupSep::Suppressed,
            // A `WHEN` upstream does not know is not an error there: it sets
            // `show_help`, so the whole help text goes to stdout and the status
            // is 0. Reproduced rather than tidied — a script testing
            // `grep --color=$var` gets upstream's answer.
            Flag::Color => match color_when(value.as_deref()) {
                Some(when) => opts.color_when = when,
                None => show_help = true,
            },
            Flag::Label => opts.label = Some(quote::os_bytes(&required(value)).into_owned()),
            Flag::LineBuffered => opts.line_buffered = true,
            // Upstream's `default: usage (EXIT_TROUBLE)`. Unreachable as things
            // stand — `SHORT_OPTIONS` lists exactly the letters matched above,
            // and `getopt` refuses any other — but a letter added to that
            // string without an arm here must be a refusal rather than a silent
            // no-op.
            Flag::Short(other) => return Err(GREP.invalid_option(other)),
        }
    }

    // Both are tested after the loop rather than inside it, and in this order,
    // because upstream does: `grep --help --version` prints the version.
    if show_version {
        return Ok(Request::Version);
    }
    if show_help {
        return Ok(Request::Help);
    }

    opts.syntax = matcher.unwrap_or(Matcher::Grep).syntax();

    // The first operand is the pattern only when no `-e`/`-f` supplied one;
    // with them, every operand is a file. This is what makes `grep -e -v file`
    // search for the text `-v`.
    let mut files = operands;
    if patterns.is_empty() && pattern_files.is_empty() {
        if files.is_empty() {
            return Ok(Request::BadUsage);
        }
        let first = quote::os_bytes(&files.remove(0)).into_owned();
        // Upstream strips one backslash from a command-line pattern beginning
        // `\-`, so that the non-POSIX `grep '\-x'` — written to stop `-x` being
        // read as an option — does not also earn a stray-backslash warning.
        // Not under `-F`, where the backslash is a literal character, and not
        // for `-e`, which upstream leaves alone.
        let stripped = match first.strip_prefix(b"\\-") {
            Some(rest) if matcher != Some(Matcher::Fgrep) => {
                let mut with_dash = vec![b'-'];
                with_dash.extend_from_slice(rest);
                with_dash
            }
            _ => first,
        };
        patterns.extend(split_arg_patterns(&stripped));
    }

    // Recursion with no operand walks the working directory, as GNU does;
    // without it there is nothing to walk and the input is stdin.
    //
    // `omit_dot_slash` is the half of that nobody expects: the walk is rooted at
    // `.`, but the names GNU prints have no `./` on them — `grep -rl foo` says
    // `sub/s1` where `grep -rl foo .` says `./sub/s1`. It is GNU's
    // `filename_prefix_len`, and it applies only when the `.` was *supplied* by
    // this branch, never when the caller wrote it.
    let mut omit_dot_slash = false;
    if files.is_empty() {
        if opts.recursive() {
            files.push(OsString::from("."));
            omit_dot_slash = true;
        } else {
            files.push(OsString::from("-"));
        }
    }

    Ok(Request::Run(Box::new(GrepArgs {
        opts,
        patterns,
        pattern_files,
        files,
        omit_dot_slash,
    })))
}

/// The value of `--binary-files=TYPE`.
///
/// # Errors
///
/// `grep: unknown binary-files type`, exit 2. An **exact** match, not
/// `argmatch`'s prefix one: upstream compares with `STREQ`, so
/// `--binary-files=w` is refused where `--directories=r` would be accepted.
/// Measured, and the value is not named in the message — upstream's choice, not
/// an omission here.
fn binary_files_arg(value: &[u8]) -> Result<BinaryFiles, getopt::Error> {
    match value {
        b"binary" => Ok(BinaryFiles::Binary),
        b"text" => Ok(BinaryFiles::Text),
        b"without-match" => Ok(BinaryFiles::WithoutMatch),
        _ => Err(GREP.usage("unknown binary-files type".to_string())),
    }
}

/// The value of `-m` / `--max-count`.
///
/// `None` is "no limit", which is what upstream's `INTMAX_MAX` default means
/// and also what a **negative** or overflowing value means: `outleft` starts at
/// `max_count` and is compared against zero as it counts down, so a negative
/// one never reaches it. Measured: `grep -m -1 MATCH f` and
/// `grep -m 99999999999999999999999 MATCH f` both print every match.
///
/// # Errors
///
/// `grep: invalid max count`, exit 2, for a value that is not a number at all
/// or that has a trailing character — upstream accepts `LONGINT_OK` and
/// `LONGINT_OVERFLOW` from `xstrtoimax` and dies on everything else.
fn max_count_arg(value: &[u8]) -> Result<Option<usize>, getopt::Error> {
    let (n, status) = xnum::xstrtoimax(value, None);
    match status {
        xnum::Status::Ok | xnum::Status::Overflow => {
            Ok(usize::try_from(n).ok().filter(|_| n >= 0))
        }
        _ => Err(GREP.usage("invalid max count".to_string())),
    }
}

/// The value of `-d` / `--directories`.
///
/// # Errors
///
/// GNU routes this through gnulib's `argmatch`, so a bad value prints **seven**
/// lines — the rejected value, `Valid arguments are:` and the three of them,
/// then `Usage: …` and `Try 'grep --help' …` — and exits **1**, not grep's
/// usual 2. [`Program::argmatch`] is that function, status included, and
/// [`run_main`] prints the [`USAGE`] pair below it, so all seven lines and the
/// status now match. It is also a **prefix** match, which is upstream's
/// behaviour and not an accident: `grep -d rec` recurses.
fn directories_arg(value: &[u8]) -> Result<Directories, getopt::Error> {
    GREP.argmatch(
        value,
        "--directories",
        &[
            ("read", Directories::Read),
            ("recurse", Directories::Recurse),
            ("skip", Directories::Skip),
        ],
    )
}

/// The value of `-D` / `--devices`.
///
/// # Errors
///
/// `grep: unknown devices method`, exit 2 — GNU's own wording and status, which
/// this one can reproduce exactly because GNU checks `-D` by hand rather than
/// through `argmatch`. So this one is an **exact** comparison where `-d` above
/// is a prefix one, and the value is not named in the message: both are GNU's
/// choices and not omissions here.
fn devices_arg(value: &[u8]) -> Result<Devices, getopt::Error> {
    match value {
        b"read" => Ok(Devices::Read),
        b"skip" => Ok(Devices::Skip),
        _ => Err(GREP.usage("unknown devices method".to_string())),
    }
}

/// The patterns held in a `--exclude-from` file.
///
/// gnulib's `add_exclude_fp`, which is *not* [`split_patterns`]: there is no
/// comment syntax — a line beginning `#` is a glob that matches a name
/// beginning `#` — and a blank line is an empty pattern, which matches nothing
/// rather than everything. (An empty *`-f` pattern* matches every line; the
/// two files look alike and mean opposite things.)
///
/// One pattern per newline, plus a final unterminated remainder if there is
/// one, so an empty file contributes nothing at all — which matters, because a
/// segment made of no patterns would still set the "oldest segment" default in
/// [`Selectors::excludes`].
fn split_exclude_file(raw: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, b) in raw.iter().enumerate() {
        if *b == b'\n' {
            out.push(raw.get(start..i).unwrap_or_default().to_vec());
            start = i.saturating_add(1);
        }
    }
    if start < raw.len() {
        out.push(raw.get(start..).unwrap_or_default().to_vec());
    }
    out
}

/// The value of `--color[=WHEN]`.
///
/// The three GNU spellings each have two synonyms nobody documents but scripts
/// use: `force`/`yes` for `always`, `none`/`no` for `never`, `tty`/`if-tty` for
/// `auto`. All six are matched without regard to case, as GNU does.
///
/// A `WHEN` that is none of them answers `None`, which is **not** an error:
/// upstream sets its show-help flag, so `grep --color=bogus foo file` prints the
/// entire help text on **stdout** and exits 0. That is why this returns an
/// `Option` rather than a `Result` — there is no diagnostic to carry, and the
/// caller turns the `None` into [`Request::Help`].
///
/// A missing `WHEN` — `--color` with no `=` — is `auto`, which is what makes
/// `grep --color foo file` search for `foo` rather than eat the pattern.
fn color_when(value: Option<&OsStr>) -> Option<ColorWhen> {
    let Some(v) = value else {
        return Some(ColorWhen::Auto);
    };
    let lower = quote::os_bytes(v).to_ascii_lowercase();
    match lower.as_slice() {
        b"always" | b"yes" | b"force" => Some(ColorWhen::Always),
        b"never" | b"no" | b"none" => Some(ColorWhen::Never),
        b"auto" | b"tty" | b"if-tty" => Some(ColorWhen::Auto),
        _ => None,
    }
}

/// The value of `-A`, `-B` or `-C`, or the diagnostic GNU gives for one that is
/// not a count.
///
/// Upstream's `context_length_arg` is `xstrtoimax` followed by
/// `if (! (0 <= value))`, so a value too large for an `intmax_t` is **accepted**
/// and clamped — `grep -A 99999999999999999999999` prints the rest of the file
/// rather than refusing. That is why this reads through [`xnum::xstrtoimax`]
/// and treats `Overflow` as success rather than parsing to a `usize`, which
/// would refuse it.
///
/// # Errors
///
/// The wording is grep's own — `grep: x: invalid context length argument`, exit
/// 2 — and not the family's `invalid number`, because a script that greps for
/// the message is greping for that one. A negative value lands here too:
/// `grep -A -1` takes `-1` as the argument (the option demands one) and then
/// refuses it, rather than reading it as the digit shorthand.
fn context_len(value: &[u8]) -> Result<usize, getopt::Error> {
    let (n, status) = xnum::xstrtoimax(value, None);
    let ok = matches!(status, xnum::Status::Ok | xnum::Status::Overflow) && n >= 0;
    // `Overflow` reports `intmax_t`'s limit as the value, so the `try_from`
    // succeeds and the clamp is upstream's own.
    match usize::try_from(n).ok().filter(|_| ok) {
        Some(n) => Ok(n),
        None => Err(GREP.usage(format!(
            "{}: invalid context length argument",
            String::from_utf8_lossy(value)
        ))),
    }
}

/// The patterns held in the text of a `-f` file.
///
/// Each line is its own pattern. A trailing newline ends the last pattern
/// rather than starting an empty one — the distinction matters, because an
/// empty pattern matches every line, so getting it wrong turns
/// `grep -f list.txt` into `cat`.
fn split_patterns(raw: &[u8]) -> Vec<Vec<u8>> {
    let body = raw.strip_suffix(b"\n").unwrap_or(raw);
    if body.is_empty() {
        return vec![Vec::new()];
    }
    body.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect()
}

/// The patterns held in one `-e` argument or in the positional pattern.
///
/// A newline separates patterns here too — `grep 'aaa
/// ccc' f` searches for either, which is how a pattern list gets into a script
/// without a temporary file — but a *trailing* one does not terminate the last
/// pattern the way [`split_patterns`] has it. It starts an empty one, and an
/// empty pattern matches every line: measured, `grep -E -e 'aaa'$'\n' f` prints
/// all of `f`.
///
/// The asymmetry is not an inconsistency in GNU, it is the same rule seen from
/// two sides. grep accumulates every pattern into one buffer and appends a `\n`
/// after each `-e`, then splits the buffer on newlines; `-f` contributes the
/// file's bytes and only adds the separator if the file did not end with one.
/// So `-e 'aaa'$'\n'` contributes `aaa\n\n` and a file holding `aaa\n`
/// contributes `aaa\n`. Splitting per argument rather than accumulating gives
/// the same answer for every combination — `-e a$'\n' -e b` is `a`, ``, `b`
/// either way — and keeps each pattern's origin available for a diagnostic.
fn split_arg_patterns(raw: &[u8]) -> Vec<Vec<u8>> {
    raw.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect()
}

/// Quote a literal so the regex engine matches it as text (`-F`).
///
/// Escaping is done against the *Extended* metacharacters, which is the
/// superset: a byte that is special in BRE but not ERE is escaped anyway, and
/// `\+` is `+` in both.
fn quote_ere(literal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(literal.len());
    for &b in literal {
        if b"\\.[]{}()*+?^$|".contains(&b) {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// Compile every pattern, or name the first one that will not compile.
///
/// The second half of the answer is the diagnostics to print before searching:
/// egrep syntax accepts shapes POSIX-extended refuses, and GNU says so rather
/// than accepting them silently — `grep -E '*a'` matches `a` *and* writes
/// `grep: warning: * at start of expression`. See [`ere::Warning`]. They are
/// returned rather than printed here so that the tests can read them and so
/// that a later compile error still takes precedence over them: a pattern list
/// that does not compile prints its error and searches nothing, and warning
/// first about a run that is not going to happen would be noise.
fn compile_patterns(
    patterns: &[Vec<u8>],
    opts: &Options,
) -> Result<(Vec<Pat>, Vec<String>), String> {
    let mut out = Vec::with_capacity(patterns.len());
    let mut warnings = Vec::new();
    // GNU collapses duplicate patterns, which is invisible in the output of a
    // search -- two copies of a pattern select the same lines as one -- and
    // visible here: `grep -E -e '*a' -e '*a'` warns once, `-e '*a' -e '*b'`
    // twice. Measured. Doing it for real rather than only for the diagnostic
    // also saves the duplicate its search.
    let mut seen: BTreeSet<&[u8]> = BTreeSet::new();
    for p in patterns {
        if !seen.insert(p.as_slice()) {
            continue;
        }
        if p.is_empty() {
            out.push(Pat::Empty);
            continue;
        }
        let compiled = match opts.syntax {
            // Only `-E` can produce a warning: `-F` escapes every
            // metacharacter, and in a BRE a leading `*` is an ordinary
            // character rather than an operator with nothing to repeat, so
            // there is nothing to remark on. Measured — `grep -G '*a'` and
            // `grep -F '*a'` are both silent.
            Syntax::Basic => bre::compile(p, opts.ignore_case).map(|re| (re, Vec::new())),
            // `-E` is *egrep* syntax, which is not the POSIX-extended syntax
            // the same engine gives `osh`, `find -regextype posix-extended`
            // and `awk`. The two differ on what happens to nonsense: GNU
            // `grep -E '*a'` warns and matches "a", and `grep -E 'a{b}'`
            // matches the text `a{b}`, where POSIX-extended refuses both.
            // Compiling grep's patterns in the stricter dialect would refuse
            // patterns GNU grep runs. See `ere::Syntax` for the measured table.
            Syntax::Extended => Regex::new_syntax_warn(p, opts.ignore_case, EreSyntax::EGREP),
            // `-F` escapes every metacharacter before it gets here, so no
            // pattern reaching this arm can contain the constructs the two
            // dialects disagree about.
            Syntax::Fixed => {
                Regex::new_flags(&quote_ere(p), opts.ignore_case).map(|re| (re, Vec::new()))
            }
        };
        match compiled {
            Ok((re, warned)) => {
                // The pattern is not named: GNU's line is the operator and
                // nothing else, even with several `-e`, which is why `-e '*a'
                // -e '*b'` prints two lines that are identical but for the
                // operator. Measured.
                warnings.extend(warned.into_iter().map(|w| match w {
                    ere::Warning::QuantifierAtStart(q) => {
                        format!("warning: {} at start of expression", q.token())
                    }
                }));
                out.push(Pat::Re(re));
            }
            Err(e) => {
                // Escaped, not lossy: a pattern is an argv token, so it is a
                // byte string and need not decode. `from_utf8_lossy` would
                // substitute U+FFFD and hand the user a message naming a
                // *different* pattern from the one they typed, which is the one
                // thing this diagnostic exists to get right. See
                // design-decisions.md §369.
                return Err(format!(
                    "{}: {}",
                    quote::escape_unprintable(p),
                    quote::escape_unprintable(&e.detail)
                ));
            }
        }
    }
    Ok((out, warnings))
}

/// Whether a byte can be part of a word, for `-w`.
///
/// ASCII letters, digits and `_`, as POSIX says — plus every byte above ASCII,
/// because in any encoding this system will see one of those is part of a
/// letter. Calling them non-word would make `grep -w foo` match the `foo` in
/// `naïvefoo`.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Whether a candidate match satisfies `-w`/`-x`. Without either it always
/// does; they are the only constraints that look outside the match.
fn accepted(span: (usize, usize), line: &[u8], opts: &Options) -> bool {
    let (start, end) = span;
    if opts.whole_line && !(start == 0 && end == line.len()) {
        return false;
    }
    if opts.word {
        let before = start
            .checked_sub(1)
            .and_then(|i| line.get(i).copied())
            .is_some_and(is_word_byte);
        let after = line.get(end).copied().is_some_and(is_word_byte);
        if before || after {
            return false;
        }
    }
    true
}

/// Where a match sits, or that the search gave up.
///
/// The third outcome is the point of the `Result`. `-v` prints every line that
/// did *not* match, and `grep -v` feeding an `xargs rm` is a real shape of
/// script; reading "I abandoned the search" as "did not match" would delete
/// files on the strength of a question grep declined to answer. Only a pattern
/// with a backreference can produce one.
type Match = Result<Option<(usize, usize)>, MatchLimit>;

/// Report an abandoned search as an I/O error, which is the channel this
/// program already uses to fail a file with a diagnostic and a non-zero status.
fn limit_err(e: MatchLimit) -> io::Error {
    io::Error::other(e.to_string())
}

/// The leftmost-longest match of any pattern at or after byte offset `from`.
///
/// Several patterns are several searches; the winner is the one that starts
/// earliest, and among those the longest — the same rule the engine applies
/// within one pattern, extended across the set so that `-e ab -e abc` behaves
/// like `abc\|ab`.
fn leftmost(pats: &[Pat], line: &[u8], from: usize) -> Match {
    let mut best: Option<(usize, usize)> = None;
    for p in pats {
        let found = match p {
            Pat::Empty => (from <= line.len()).then_some((from, from)),
            Pat::Re(re) => re.find_at(line, from)?,
        };
        if let Some((s, e)) = found {
            best = Some(match best {
                Some(b) if b.0 < s || (b.0 == s && b.1 >= e) => b,
                _ => (s, e),
            });
        }
    }
    Ok(best)
}

/// The leftmost match at or after `from` that also satisfies `-w`/`-x`.
fn next_match(pats: &[Pat], line: &[u8], from: usize, opts: &Options) -> Match {
    let mut pos = from;
    loop {
        let Some(cand) = leftmost(pats, line, pos)? else {
            return Ok(None);
        };
        if accepted(cand, line, opts) {
            return Ok(Some(cand));
        }
        // Retry one byte on rather than past the candidate: a match that *is* a
        // word can begin inside one that is not, as `-w` on `xfoo foo` shows.
        // `find_at` rounds a mid-character offset forward, so stepping by a
        // byte cannot land inside a character.
        pos = cand.0.saturating_add(1);
        if pos > line.len() {
            return Ok(None);
        }
    }
}

/// Every non-overlapping accepted match of the line, left to right — what `-o`
/// prints.
fn matches_in(
    pats: &[Pat],
    line: &[u8],
    opts: &Options,
) -> Result<Vec<(usize, usize)>, MatchLimit> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some((s, e)) = next_match(pats, line, pos, opts)? {
        out.push((s, e));
        // An empty match is at a position rather than over one, so the scan has
        // to step past it or it would be found here for ever.
        pos = if e > s { e } else { e.saturating_add(1) };
        if pos > line.len() {
            break;
        }
    }
    Ok(out)
}

/// Whether the line is selected, `-v` included.
fn line_selected(line: &[u8], pats: &[Pat], opts: &Options) -> Result<bool, MatchLimit> {
    let matched = next_match(pats, line, 0, opts)?.is_some();
    Ok(matched != opts.invert)
}

/// What a file is called in output. Standard input is spelled `-` on the
/// command line and *named* `(standard input)` in a diagnostic or a prefix, as
/// every other grep does — a `-:` prefix reads as part of the line.
///
/// `--label` replaces that name and only that name: it is what makes
/// `cat f | grep -H --label=f pat` print the prefix a caller wanted, and it has
/// no effect on a file named on the command line. Bytes rather than `str`
/// because a label, like a path, may hold any byte but NUL.
fn display_name<'a>(path: &'a OsStr, opts: &'a Options) -> Cow<'a, [u8]> {
    if path == "-" {
        match &opts.label {
            Some(label) => Cow::Borrowed(label),
            None => Cow::Borrowed(b"(standard input)"),
        }
    } else {
        quote::os_bytes(path)
    }
}

/// Whether every printed line is prefixed with the name of the file it came
/// from.
///
/// `explicit` is `-H`/`-h`, which settle it outright. Otherwise the rule is not
/// "more than one file was searched" but **"more than one file was *named*, or
/// `-r` was pointed at a directory"** — which is a question about the operands,
/// not about what they expand to. Measured, GNU grep 3.11:
///
/// | command | prefix? | why |
/// |---|---|---|
/// | `grep -r foo dir` where `dir` holds one file | yes | an operand was a directory |
/// | `grep -r foo link-to-file` | no | one operand, and not a directory |
/// | `grep foo a b` | yes | two operands |
///
/// Counting the expansion instead got the first row wrong. It is also the only
/// formulation that can be answered *before* the walk begins, which the walk
/// now requires: it streams, so by the time the size of the expansion is known,
/// lines have already been printed with or without a prefix.
fn wants_filename(explicit: Option<bool>, operands: usize, named_a_directory: bool) -> bool {
    explicit.unwrap_or(operands > 1 || named_a_directory)
}

/// Everything one printed line's prefix is built from.
///
/// A struct rather than a longer parameter list because the fields are not
/// interchangeable and four of them are numbers: `line_prefix(name, 3, 118,
/// true, 2, b'-')` is a call nobody can read, and swapping two of its arguments
/// compiles.
struct Prefix<'a> {
    filename: &'a [u8],
    show_filename: bool,
    /// Zero-based internally; printed one-based.
    line_idx: usize,
    /// The byte offset within the file of whatever follows this prefix — the
    /// line, or under `-o` the match.
    byte_pos: u64,
    /// The column the numeric fields are right-aligned in under `-T`; ignored
    /// without it. See [`offset_width`].
    width: usize,
    /// `:` for a selected line, `-` for a context line.
    field: u8,
}

/// The prefix shown before a printed line: file name, line number, byte offset,
/// any combination, or none.
///
/// Bytes rather than `String` because `-Z` puts a NUL after the name, and
/// because the name itself is a path — which on this system may hold any byte
/// but `/` and NUL. Only the *name's* separator changes under `-Z`; the ones
/// after a line number and a byte offset stay as `field`, which is what GNU
/// does and is what keeps `-nZ` output parseable at all.
///
/// `field` is `:` for a selected line and `-` for a context line, and it
/// punctuates *every* field rather than just the last: `grep -nHC1` writes
/// `ctx:3:HIT` for the match and `ctx-2-2` for its neighbour. That is the only
/// thing distinguishing the two kinds of line in the output, so a caller
/// filtering `grep -C` output for real matches is reading this byte.
///
/// `-T`'s tab goes *after* the last separator and only when some field was
/// printed at all: `grep -TnH` writes `f: 2:\tbody`, `grep -TH` writes
/// `f:\tbody`, `grep -THZ` writes `f\0\tbody`, and plain `grep -T` writes no
/// tab because it wrote no prefix to line up. Measured.
fn line_prefix(p: &Prefix, opts: &Options) -> Vec<u8> {
    let mut prefix = Vec::new();
    let mut any = false;
    let number = |cap: &[u8], n: u64| {
        // The padding goes *inside* the escape — measured: `-T` with colour
        // writes `\e[32m\e[K  12\e[m\e[K`, not two plain spaces and then the
        // escape. It matters on a terminal whose `ln` sets a background.
        let mut field = Vec::new();
        push_number(&mut field, n, if opts.align_tabs { p.width } else { 0 });
        opts.paint(cap, &field)
    };
    if p.show_filename {
        prefix.extend_from_slice(&opts.paint(&opts.colors.filename, p.filename));
        // `-Z`'s NUL is a delimiter for a machine, and GNU leaves it outside
        // the `se` escape — it is the one separator that is not coloured.
        if opts.null_name {
            prefix.push(0);
        } else {
            prefix.extend_from_slice(&opts.paint(&opts.colors.separator, &[p.field]));
        }
        any = true;
    }
    if opts.line_numbers {
        // Zero-based internally, one-based on the way out.
        let n = u64::try_from(p.line_idx.saturating_add(1)).unwrap_or(u64::MAX);
        prefix.extend_from_slice(&number(&opts.colors.line_number, n));
        prefix.extend_from_slice(&opts.paint(&opts.colors.separator, &[p.field]));
        any = true;
    }
    if opts.byte_offset {
        prefix.extend_from_slice(&number(&opts.colors.byte_number, p.byte_pos));
        prefix.extend_from_slice(&opts.paint(&opts.colors.separator, &[p.field]));
        any = true;
    }
    if any && opts.align_tabs {
        // Uncoloured, like `-Z`'s NUL: it is whitespace, and painting it would
        // extend a background colour across the gutter.
        prefix.push(b'\t');
    }
    prefix
}

/// A decimal number right-aligned in `width` columns, or unpadded when `width`
/// is zero or too small to hold it.
fn push_number(out: &mut Vec<u8>, n: u64, width: usize) {
    let text = n.to_string();
    for _ in text.len()..width {
        out.push(b' ');
    }
    out.extend_from_slice(text.as_bytes());
}

/// The column `-T` right-aligns the numeric prefix fields in.
///
/// GNU fixes this from the file's *size* before reading a line, not from the
/// widest value it goes on to print — which is what lets `-T` work on a stream,
/// and what makes the padding of a given file independent of which lines match.
/// Measured against GNU grep 3.11:
///
/// | size | `-b` pads to | `-n` pads to |
/// |---|---|---|
/// | 9 | 1 | 2 |
/// | 34 | 2 | 2 |
/// | 99 | 2 | 3 |
/// | 1504 | 4 | 4 |
///
/// So: the digit count of the size, plus one for `-n` — a file of N bytes holds
/// at most N+1 lines. With both flags the wider of the two is used for both,
/// which falls out of computing it once.
///
/// `None` is an input whose size cannot be taken, which is every pipe. GNU pads
/// those to 19 columns; that is the digit count of `i64::MAX`, and the
/// measurement is the reason for the odd-looking constant.
/// The size `-T` sizes its columns from, or `None` for an input that has no
/// meaningful one.
///
/// [`filekind::is_regular`] rather than `Metadata::is_file`: on the harness's
/// Windows host a pipe answers yes to the latter and reports however many bytes
/// happen to be sitting in the pipe buffer, so every run of the same pipeline
/// would pad to a different width. GNU asks `S_ISREG`, and this is that
/// question asked portably.
fn regular_size(file: &File) -> Option<u64> {
    if !filekind::is_regular(file) {
        return None;
    }
    file.metadata().map(|m| m.len()).ok()
}

fn offset_width(size: Option<u64>, opts: &Options) -> usize {
    let mut n = size.unwrap_or_else(|| u64::try_from(i64::MAX).unwrap_or(u64::MAX));
    if opts.line_numbers {
        n = n.saturating_add(1);
    }
    let mut width = 1usize;
    while n >= 10 {
        n /= 10;
        width = width.saturating_add(1);
    }
    width
}

/// Settle `--color` into a yes or a no, and read the palette.
///
/// Both questions are asked once, here, rather than per line: `auto` queries
/// the operating system, and `GREP_COLORS` has to be parsed before the first
/// byte of output.
///
/// `GREP_COLOR` — singular, deprecated in 2011 — still works and still sets
/// both match colours, and GNU still warns about it on stderr. It is read
/// *first*, so that a `GREP_COLORS` that also sets `ms`/`mc` wins. Neither is
/// read at all when colour is off, which is what keeps the warning from
/// appearing for a caller who never asked for colour.
fn resolve_colors(opts: &mut Options) {
    opts.color = match opts.color_when {
        ColorWhen::Always => true,
        ColorWhen::Never => false,
        // GNU asks about *stdout*, not stdin: the question is whether whoever
        // reads this output can render an escape sequence.
        ColorWhen::Auto => io::stdout().is_terminal(),
    };
    if !opts.color {
        return;
    }
    if let Some(v) = env::var_os("GREP_COLOR") {
        let raw = quote::os_bytes(&v).into_owned();
        if !raw.is_empty() {
            // The text is GNU's, quoted the way GNU quotes it — a script that
            // greps its own stderr for this warning greps for that wording.
            let shown = String::from_utf8_lossy(&raw).into_owned();
            diag!(
                "grep: warning: GREP_COLOR='{shown}' is deprecated; use GREP_COLORS='mt={shown}'"
            );
            opts.colors.selected_match.clone_from(&raw);
            opts.colors.context_match = raw;
        }
    }
    if let Some(v) = env::var_os("GREP_COLORS") {
        opts.colors.apply(&quote::os_bytes(&v));
    }
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 2)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let mut parsed = match parse_args(&args) {
        Ok(Request::Run(p)) => p,
        Ok(Request::Help) => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Request::Version) => {
            println!("grep (SlateOS coreutils) 0.1.0");
            return ExitCode::SUCCESS;
        }
        // No diagnostic above it: upstream's `usage (EXIT_TROUBLE)` for a
        // pattern-less command line prints these two lines and nothing else.
        Ok(Request::BadUsage) => {
            diag!("{USAGE}");
            return ExitCode::from(2);
        }
        Err(e) => {
            // `e.sentence`, then [`USAGE`] — not `e.message()`, which would
            // join the sentence to the `Try '…'` referral directly. Upstream
            // puts the `Usage:` summary between the two, and prints all three
            // for `argmatch` failures as well, which is why the referral is
            // spelled inside `USAGE` rather than taken from the error.
            diag!("grep: {}", e.sentence);
            if e.referral.is_some() {
                diag!("{USAGE}");
            }
            return ExitCode::from(u8::try_from(e.status).unwrap_or(2));
        }
    };
    resolve_colors(&mut parsed.opts);

    let mut patterns = parsed.patterns;
    for pf in &parsed.pattern_files {
        let raw = if pf == "-" {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf).map(|_| buf)
        } else {
            fs::read(pf)
        };
        match raw {
            Ok(raw) => patterns.extend(split_patterns(&raw)),
            Err(e) => {
                diag!("grep: {}: {}", quotef_os(pf), strerror(&e));
                return ExitCode::from(2);
            }
        }
    }

    let pats = match compile_patterns(&patterns, &parsed.opts) {
        Ok((p, warnings)) => {
            // Before any searching, and without touching the exit status: a
            // warned-about pattern still runs, and `grep -E '*a' f` exits 0 or
            // 1 on the search as usual. `-q` and `-s` do not suppress these
            // either — `-s` is about unreadable files and `-q` about the
            // selected lines, and neither is about the pattern. Measured.
            for w in warnings {
                diag!("grep: {w}");
            }
            p
        }
        Err(e) => {
            diag!("grep: {e}");
            return ExitCode::from(2);
        }
    };

    let named_a_directory = parsed.opts.recursive()
        && parsed.files.iter().any(|f| {
            // `is_dir` follows a symlink, matching what `Run::operand` does
            // with the same operand a moment later.
            Path::new(f).is_dir()
        });
    let show_filename = wants_filename(parsed.opts.filename, parsed.files.len(), named_a_directory);

    let mut run = Run {
        // `Stream`, not `io::stdout().lock()`, because the verdict on the
        // output is part of the answer: `grep a file >&-` wrote into a buffer
        // that was discarded by a final `let _ = out.flush()` and exited 0
        // having printed nothing. `Stream` records the failure instead of
        // returning it — so no write below can fail — and `close_stdout_with`
        // reports it as `grep: write error: …` and exits 2, as GNU does.
        out: stdfd::Stream::stdout(),
        pats: &pats,
        opts: &parsed.opts,
        show_filename,
        omit_dot_slash: parsed.omit_dot_slash,
        any_match: false,
        had_error: false,
        done: false,
        printed_before: false,
    };

    for f in &parsed.files {
        run.operand(f);
        if run.done {
            break;
        }
    }

    let earned = if run.any_match && parsed.opts.quiet {
        // `-q` asks one question — "is there a match?" — and an error reading
        // some *other* file does not unanswer it. POSIX says so outright
        // ("exit with zero status if an input line is selected, even if an
        // error was detected"), and it is measurable: `grep -q foo nonexistent
        // words` prints the diagnostic and still exits 0, where the same
        // command without `-q` exits 2.
        ExitCode::SUCCESS
    } else if run.had_error {
        // Otherwise an error outranks both answers: a script that
        // distinguishes 0 from 1 is asking about the content of files it
        // believes were all read.
        ExitCode::from(2)
    } else if run.any_match {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    };
    // grep's `exit_failure` is 2, not the family's 1.
    stdfd::close_stdout_with("grep", run.out, earned, 2)
}

/// Everything one run carries from file to file.
///
/// It is a struct rather than a handful of locals because the recursive walk
/// has to **stream**: GNU searches each file at the moment it reaches it, so
/// the walk and the searching are one traversal and not two phases. Collecting
/// the tree into a `Vec` first — which is what this did until 2026-08-25 — got
/// two observable things wrong:
///
/// * `grep -rq foo dir` stops at the first match and never looks at what comes
///   after it, so it never emits the diagnostics those files would have
///   produced. Measured: on a tree holding a symlink loop, `grep -Rq foo L`
///   prints nothing, while `grep -Rq zzz L` prints the loop warning — the
///   difference being only how far the walk got.
/// * A diagnostic about one file belongs *between* the matches of its
///   neighbours. Expanding first put every diagnostic before every match.
struct Run<'a> {
    out: stdfd::Stream,
    pats: &'a [Pat],
    opts: &'a Options,
    show_filename: bool,
    /// Strip the `./` the walk builds onto every name, because the `.` it is
    /// walking was supplied by [`parse_args`] rather than written by the
    /// caller. See `GrepArgs::omit_dot_slash`.
    omit_dot_slash: bool,
    any_match: bool,
    had_error: bool,
    /// Set once `-q` has its answer. Every remaining file and directory is
    /// then skipped — including the diagnostics they would have produced,
    /// which is the point.
    done: bool,
    /// Whether any group of output has been printed yet, anywhere in the run.
    ///
    /// Crosses file boundaries because the `--` between context groups does:
    /// `grep -A1 HIT a b` puts one between `a`'s last group and `b`'s first,
    /// and only the very first group of the whole run goes without.
    printed_before: bool,
}

impl Run<'_> {
    /// Handle one command-line operand.
    ///
    /// # Why this stats before it decides anything
    ///
    /// GNU's order is open, `fstat`, *then* choose — and the choices need the
    /// mode, so there is no way to skip the stat. Doing it before the open
    /// instead of after is the one deliberate difference, and it is what keeps
    /// `grep -D skip pat fifo` from hanging: opening a FIFO that has no writer
    /// blocks forever, so a `grep` that opened first would hang on exactly the
    /// input it was told to skip. GNU avoids the same hang from the other side,
    /// by adding `O_NONBLOCK` when devices are to be skipped.
    ///
    /// The order of the tests below is GNU's, and it is observable:
    ///
    /// * The stat comes **first**, so `grep --exclude='*' pat nosuch` still
    ///   reports the missing file and exits 2. A name that does not exist is
    ///   not excluded; it is an error.
    /// * The selectors come **before** the directory handling, so
    ///   `grep --exclude-dir=sub pat sub` is silent and exits 1 where plain
    ///   `grep pat sub` says `Is a directory` and exits 2.
    /// * `--exclude` and `--exclude-dir` are chosen between by what the operand
    ///   *is*, so `grep --exclude=sub -r pat sub` searches `sub` after all.
    fn operand(&mut self, f: &OsStr) {
        // Standard input is exempt from every one of these: it is not a name,
        // so no glob can select it, and GNU's tests are all guarded on the
        // descriptor not being stdin. `grep --exclude='*' pat -` reads it.
        if f == "-" {
            self.search(f);
            return;
        }
        // `metadata`, which follows a symlink — deliberately, and unlike the
        // walk. A link named on the command line is followed by `-r` as well as
        // `-R`; it is only a link *discovered* by the walk that `-r` skips.
        let md = match fs::metadata(Path::new(f)) {
            Ok(md) => md,
            Err(e) => {
                if !self.opts.no_messages {
                    diag!("grep: {}: {}", quotef_os(f), strerror(&e));
                }
                self.had_error = true;
                return;
            }
        };
        let is_dir = md.is_dir();

        if self.opts.skipped_file(&quote::os_bytes(f), is_dir) {
            return;
        }

        if is_dir {
            match self.opts.directories {
                Directories::Recurse => {
                    let mut ancestors: Vec<PathBuf> = Vec::new();
                    self.walk(Path::new(f), &mut ancestors);
                }
                // Silent, and *not* an error: `grep -d skip pat dir` exits 1.
                Directories::Skip => {}
                Directories::Read => {
                    if !self.opts.no_messages {
                        diag!("grep: {}: Is a directory", quotef_os(f));
                    }
                    // Named but not searched, so the run's answer is about less
                    // than it was asked about — status 2, as for a file that
                    // could not be opened. `-s` silences the message, not this.
                    self.had_error = true;
                }
            }
            return;
        }

        if self.opts.skip_devices(true) && filekind::is_device(&md) {
            return;
        }

        self.search(f);
    }

    /// Search one named file — or standard input, spelled `-`.
    fn search(&mut self, path: &OsStr) {
        // The size is `-T`'s alone, so it is not asked for without it: an
        // `fstat` per file is cheap but not free, and a `grep -r` over a large
        // tree pays it once per entry.
        let mut size: Option<u64> = None;
        let reader: Box<dyn Read> = if path == "-" {
            if self.opts.align_tabs {
                size = filekind::borrowed_stdin().and_then(|f| regular_size(&f));
            }
            Box::new(io::stdin())
        } else {
            if Path::new(path).is_dir() {
                // A backstop, not the usual route: [`Run::operand`] settles
                // every directory it is given, and the walk recurses rather
                // than arriving here, so this is reached only when a name
                // becomes a directory between the stat there and the open here.
                // It stays because on a host where opening a directory fails
                // outright — Windows, where the differential harness used to
                // run — the `read` below would report the wrong errno.
                if !self.opts.no_messages {
                    diag!("grep: {}: Is a directory", quotef_os(path));
                }
                // Named but not searched, so the run's answer is about less
                // than it was asked about — status 2, as for a file that could
                // not be opened.
                self.had_error = true;
                return;
            }
            match File::open(path) {
                Ok(f) => {
                    if self.opts.align_tabs {
                        size = regular_size(&f);
                    }
                    Box::new(f)
                }
                Err(e) => {
                    if !self.opts.no_messages {
                        diag!("grep: {}: {}", quotef_os(path), strerror(&e));
                    }
                    // A file that could not be read is an error, not an absence
                    // of matches: exiting 1 would tell a script the file has
                    // been searched and found wanting.
                    self.had_error = true;
                    return;
                }
            }
        };

        let shown = display_name(path, self.opts);
        let src = Source {
            filename: &shown,
            show_filename: self.show_filename,
            width: offset_width(size, self.opts),
        };
        match search_stream(
            &mut self.out,
            reader,
            self.pats,
            &src,
            self.opts,
            &mut self.printed_before,
        ) {
            Ok(matched) => {
                if matched {
                    self.any_match = true;
                    if self.opts.quiet {
                        // `-q` is a question, and it has been answered.
                        self.done = true;
                        return;
                    }
                }
                // `-l` and `-L` name the file rather than the lines; which of
                // the two asked decides which answer is worth naming.
                let name_it = (self.opts.files_with_matches && matched)
                    || (self.opts.files_without_match && !matched);
                if name_it {
                    // NUL after the name under `-Z`, newline otherwise — and
                    // *not* `-z`'s separator, which describes the input. This
                    // is the half of `-Z` that matters: `grep -rlZ | xargs -0`
                    // is the only listing of paths that survives a path
                    // containing a newline, which this system permits.
                    let painted = self.opts.paint(&self.opts.colors.filename, &shown);
                    let _ = self.out.write_all(&painted);
                    let _ = self.out.write_all(if self.opts.null_name {
                        &b"\0"[..]
                    } else {
                        &b"\n"[..]
                    });
                    let _ = line_flush(&mut self.out, self.opts);
                }
            }
            Err(e) => {
                if !self.opts.no_messages {
                    diag!("grep: {}: {}", quotef_os(path), strerror(&e));
                }
                self.had_error = true;
            }
        }
    }

    /// Search a file the walk found, rather than one the command line named.
    ///
    /// The name travels as bytes the whole way — `OsStr` in, `OsStr` out —
    /// because this system permits any byte but `/` and NUL in a filename, and
    /// an intermediate `String` would replace the ones that are not UTF-8 with
    /// `U+FFFD`: `grep -rl` would then print a name that cannot be opened.
    fn search_found(&mut self, path: &Path) {
        let raw = quote::os_bytes(path.as_os_str());
        // GNU's `filename_prefix_len`: the walk is rooted at a `.` this program
        // supplied, and the names it prints carry no `./`. Applied here rather
        // than at the root because it is a property of how the name is
        // *displayed*, not of where the walk goes.
        let shown: &[u8] = if self.omit_dot_slash {
            raw.strip_prefix(b"./".as_slice()).unwrap_or(&raw)
        } else {
            &raw
        };
        self.search(&quote::os_from_bytes(shown));
    }

    /// Walk one directory, searching what it holds, deepest-last and in sorted
    /// order.
    ///
    /// # Which symlinks are followed
    ///
    /// A symlink met *during* the walk is skipped by `-r` and followed by `-R`
    /// — that single difference is the whole of what separates the two flags.
    /// A symlink *named on the command line* is followed by both, which is why
    /// [`Run::operand`] asks `is_dir` (following) where this asks
    /// `symlink_metadata` (not following).
    ///
    /// # The loop
    ///
    /// `-R` on a tree containing a link back to one of its own ancestors
    /// describes an infinite tree. GNU prints
    /// `grep: PATH: warning: recursive directory loop` and carries on.
    /// Measured, and all three halves of that matter: the message is
    /// suppressed by `-s`, it does **not** change the exit status (`grep -R zzz
    /// loop-tree` exits 1, not 2), and it names the link rather than what the
    /// link resolves to.
    ///
    /// # Ordering
    ///
    /// Sorted, where GNU emits readdir order. See `design-decisions.md` §380:
    /// the order a directory hands back its entries is a property of the
    /// filesystem, so GNU's output is not reproducible between two machines
    /// holding the same files, whereas a sorted listing is stable and
    /// diffable.
    fn walk(&mut self, dir: &Path, ancestors: &mut Vec<PathBuf>) {
        if self.done {
            return;
        }
        if self.opts.deref_links {
            // Identity by resolved path: two names for one directory are one
            // directory, and it is the resolved form that says so. A failure
            // to resolve is not fatal here — the `read_dir` below will report
            // it properly — so the unresolvable path stands in for itself.
            let real = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
            if ancestors.contains(&real) {
                if !self.opts.no_messages {
                    diag!(
                        "grep: {}: warning: recursive directory loop",
                        quotef_os(dir)
                    );
                }
                return;
            }
            ancestors.push(real);
        }

        self.walk_entries(dir, ancestors);

        if self.opts.deref_links {
            ancestors.pop();
        }
    }

    /// The body of [`Run::walk`], split out so that the ancestor pushed by the
    /// caller is popped on every path out of it.
    fn walk_entries(&mut self, dir: &Path, ancestors: &mut Vec<PathBuf>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                if !self.opts.no_messages {
                    diag!("grep: {}: {}", quotef_os(dir), strerror(&e));
                }
                self.had_error = true;
                return;
            }
        };

        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => paths.push(e.path()),
                Err(e) => {
                    if !self.opts.no_messages {
                        diag!("grep: {}: {}", quotef_os(dir), strerror(&e));
                    }
                    self.had_error = true;
                }
            }
        }
        paths.sort();

        for path in paths {
            if self.done {
                return;
            }
            // `symlink_metadata`, not `metadata`: the question here is what the
            // entry *is*, not what it points at.
            let md = match fs::symlink_metadata(&path) {
                Ok(md) => md,
                Err(e) => {
                    if !self.opts.no_messages {
                        diag!("grep: {}: {}", quotef_os(&path), strerror(&e));
                    }
                    self.had_error = true;
                    continue;
                }
            };

            // The name the selectors are shown is the entry's **base name**,
            // never the path the walk built to reach it — GNU matches
            // `ent->fts_name`. So `--exclude='sub/s1'` excludes nothing under
            // `-r`, and `--include='*/s1'` matches nothing at all, while
            // `--exclude='s1'` excludes it at every depth.
            let name = path.file_name().map(quote::os_bytes).unwrap_or_default();

            if md.file_type().is_symlink() {
                if !self.opts.deref_links {
                    // `-r` passes over it in silence — including a dangling
                    // one, which it never asks about and so never reports.
                    continue;
                }
                match fs::metadata(&path) {
                    Ok(target) => {
                        // Asked of the *target*, because that is what `-R`'s
                        // walk treats the entry as: a link to a directory is a
                        // directory here, and so faces `--exclude-dir`.
                        if self.opts.skipped_file(&name, target.is_dir()) {
                            continue;
                        }
                        if target.is_dir() {
                            self.walk(&path, ancestors);
                        } else if !(self.opts.skip_devices(false) && filekind::is_device(&target)) {
                            self.search_found(&path);
                        }
                    }
                    Err(e) => {
                        if !self.opts.no_messages {
                            diag!("grep: {}: {}", quotef_os(&path), strerror(&e));
                        }
                        self.had_error = true;
                    }
                }
                continue;
            }

            if self.opts.skipped_file(&name, md.is_dir()) {
                continue;
            }

            if md.is_dir() {
                self.walk(&path, ancestors);
            } else if !(self.opts.skip_devices(false) && filekind::is_device(&md)) {
                // The default already skips a device the walk found, which is
                // what stops `grep -r pat /` blocking on the first FIFO with no
                // writer. Only `-D read` reads them here.
                self.search_found(&path);
            }
        }
    }
}

/// Write one line of *context* — a line printed because it neighbours a
/// selected line, not because it was selected itself.
///
/// Under `-o` this writes nothing at all, not even the prefix: `-o` prints the
/// part of a line that matched, and a context line has no such part. GNU is the
/// same, and the caller still counts the line as output, because the `--` that
/// separates groups is placed by where the file has been *read up to*, not by
/// how many bytes came out. That is what makes `grep -oC2 HIT ctx` print two
/// bare `HIT`s with no separator between them while `grep -oA1 HIT ctx` puts
/// one in: in the first the two groups' ranges touch, in the second they do
/// not, and neither one printed anything for the lines in between.
fn write_context_line(
    out: &mut impl Write,
    body: &[u8],
    p: &Prefix,
    pats: &[Pat],
    opts: &Options,
) -> io::Result<()> {
    if opts.only_matching {
        return Ok(());
    }
    out.write_all(&line_prefix(p, opts))?;
    write_body(out, body, false, pats, opts)?;
    out.write_all(&[opts.line_sep()])?;
    line_flush(out, opts)
}

/// `--line-buffered`'s `fflush`, at each of the three places upstream calls it.
///
/// Once per *printed line* — so `-o` flushes after the last of a line's matches
/// rather than after each one — plus once after a `-c` count and once after an
/// `-l`/`-L` name. It is the point of the option: without it a `grep --color
/// … | while read` pipeline sees nothing until 4 KiB have accumulated, and a
/// log follower appears to hang.
fn line_flush(out: &mut impl Write, opts: &Options) -> io::Result<()> {
    if opts.line_buffered { out.flush() } else { Ok(()) }
}

/// One line's text, with `--color`'s escapes woven through it.
///
/// Reproduces GNU's two-stage model, which is not the obvious one and was
/// measured rather than guessed:
///
/// * **Matches.** A line is searched for matches to highlight when
///   `selected ^ -v` — so plain `grep` highlights the selected lines, and
///   `grep -v` highlights the *context* lines, which are the ones the pattern
///   actually hit. The capability is `ms` on a selected line and `mc` on a
///   context one. Each match is preceded by the *line* capability's opening
///   escape and the text since the last match, and that opening escape is
///   never closed — the match's own escape pair is what ends it. An empty
///   match is skipped, exactly as under `-o`.
/// * **Tail.** Whatever follows the last match is then written wrapped in the
///   line capability, but only when that capability is non-empty — which is
///   why plain `grep --color=always` (where `sl` and `cx` are both empty)
///   emits no escapes around the unmatched text at all. The tail stops short
///   of a `\r` that ends the line, so that a CRLF file's carriage return is
///   not painted; anything left after that is written plainly.
///
/// The two stages together are why `GREP_COLORS='ms='` and
/// `GREP_COLORS='ms=:sl=33'` produce differently *shaped* output rather than
/// the same shape with one escape missing.
fn write_body(
    out: &mut impl Write,
    body: &[u8],
    selected: bool,
    pats: &[Pat],
    opts: &Options,
) -> io::Result<()> {
    if !opts.color {
        return out.write_all(body);
    }
    let line_cap = opts.line_cap(selected);
    let match_cap = opts.match_cap(selected);
    let mut done = 0usize;
    if (selected ^ opts.invert) && !match_cap.is_empty() {
        for (s, e) in matches_in(pats, body, opts).map_err(limit_err)? {
            // Empty matches are at every position, so highlighting them would
            // bury the line in escapes; GNU skips them here for the same
            // reason `-o` does not print them.
            if e <= s || s < done {
                continue;
            }
            out.write_all(&opts.colors.start(line_cap))?;
            out.write_all(body.get(done..s).unwrap_or_default())?;
            out.write_all(
                &opts
                    .colors
                    .wrap(match_cap, body.get(s..e).unwrap_or_default()),
            )?;
            done = e;
        }
    }
    if !line_cap.is_empty() {
        let tail_end = body
            .len()
            .saturating_sub(usize::from(body.last() == Some(&b'\r')));
        if tail_end > done {
            out.write_all(&opts.colors.start(line_cap))?;
            out.write_all(body.get(done..tail_end).unwrap_or_default())?;
            out.write_all(&opts.colors.end(line_cap))?;
            done = tail_end;
        }
    }
    out.write_all(body.get(done..).unwrap_or_default())
}

/// The one stream being searched, and what its prefixes need to know about it.
///
/// Separate from [`Options`] because these three are per-file where the options
/// are per-run, and `width` in particular is a *measurement* of the file taken
/// before the first line is read.
struct Source<'a> {
    /// The name shown in a prefix — `(standard input)` for `-`, or
    /// `--label`'s value when one was given. Bytes: a path here may hold any
    /// byte but `/` and NUL.
    filename: &'a [u8],
    show_filename: bool,
    /// The column `-T` right-aligns numbers in. Meaningless without it.
    width: usize,
}

/// Search one stream, printing what the options ask for. Returns whether any
/// line was selected.
///
/// `printed_before` is the run's memory of whether *anything* has been printed
/// yet, and it lives outside this function because the group separator does
/// too: `grep -A1 HIT a b` puts a `--` between the last group of `a` and the
/// first of `b`, so a file cannot decide on its own whether its opening group
/// needs one. It never leads the very first group of the run.
fn search_stream(
    out: &mut impl Write,
    reader: impl Read,
    pats: &[Pat],
    src: &Source<'_>,
    opts: &Options,
    printed_before: &mut bool,
) -> io::Result<bool> {
    let filename = src.filename;
    let show_filename = src.show_filename;
    // `-m 0` is not "no limit", and it is not "stop after the first" either:
    // GNU prints nothing at all — not even the `-c` count line, which is the
    // surprising half — and reports the file as not matching. Answering it
    // before opening a line is also the only way to get that, since the count
    // line below is printed unconditionally.
    if opts.max_count == Some(0) {
        return Ok(false);
    }

    let mut buf = BufReader::new(reader);
    // Printing nothing means the first selected line settles it, and reading
    // the rest of a file is work whose result is discarded — which for `-q` on
    // a pipe is also the difference between returning and waiting.
    let stop_at_first = opts.quiet || opts.files_with_matches || opts.files_without_match;
    let sep = opts.line_sep();
    // `-c`/`-l`/`-L`/`-q` answer a question about the file, so GNU ignores
    // `-A`/`-B`/`-C` under them entirely — the separator included.
    let show_context = opts.context_printed();
    let out_before = if show_context { opts.out_before() } else { 0 };
    let out_after = if show_context { opts.out_after() } else { 0 };
    let separate_groups = show_context && opts.context_requested();

    let mut match_count: usize = 0;
    let mut line_idx: usize = 0;
    let mut line: Vec<u8> = Vec::new();
    // Bytes of the file that precede the line about to be read — `-b`'s answer,
    // and the base its per-match offsets are measured from. Counted rather than
    // asked for because the input need not be seekable, and it counts the line
    // separator too: under `-z` the NULs are bytes of the file like any other.
    let mut byte_pos: u64 = 0;
    // Lines held back as possible leading context: the last `out_before` lines
    // that were neither selected nor already printed, oldest first. The byte
    // offset travels with the text because by the time the line is printed the
    // stream has moved past it.
    let mut before: VecDeque<(usize, u64, Vec<u8>)> = VecDeque::new();
    // Trailing context still owed to the most recent selected line.
    let mut pending_after: usize = 0;
    // The one-based number of the last line this *file* has printed — or has
    // decided to print nothing for, under `-o`. `None` until the file prints
    // its first, which is also what makes a file's opening group take a
    // separator: it is never adjacent to anything.
    let mut last_out: Option<usize> = None;
    // `-m` has been satisfied, but trailing context may still be owed. Lines
    // read after this point are not tested against the pattern at all, so a
    // line that *would* have matched prints as context — measured:
    // `grep -n -m1 -A2 HIT` over three `HIT`s gives `1:HIT`, `2-HIT`, `3-HIT`.
    let mut limit_reached = false;

    loop {
        line.clear();
        // Lines are read as bytes: a file this system can name may hold any
        // byte but `/` and NUL, and `String`-typed input could not carry one.
        if buf.read_until(sep, &mut line)? == 0 {
            break;
        }
        // The separator is not part of the line, and a final line without one
        // is still a line.
        let body = line.strip_suffix(&[sep][..]).unwrap_or(&line);
        let lineno = line_idx.saturating_add(1);
        let here = byte_pos;
        // Advanced now, before any `continue` or `break` below can skip it.
        byte_pos = byte_pos.saturating_add(u64::try_from(line.len()).unwrap_or(0));
        let at = |line_idx: usize, byte_pos: u64, field: u8| Prefix {
            filename,
            show_filename,
            line_idx,
            byte_pos,
            width: src.width,
            field,
        };

        if limit_reached {
            if pending_after == 0 {
                break;
            }
            write_context_line(out, body, &at(line_idx, here, b'-'), pats, opts)?;
            last_out = Some(lineno);
            pending_after = pending_after.saturating_sub(1);
            if pending_after == 0 {
                // Stopping here rather than on the next iteration keeps `-m`
                // from swallowing one more line of a pipe than it printed.
                break;
            }
            line_idx = lineno;
            continue;
        }

        if line_selected(body, pats, opts).map_err(limit_err)? {
            match_count = match_count.saturating_add(1);
            if stop_at_first {
                return Ok(true);
            }
            if !opts.count_only {
                // Where this group of output begins: `out_before` lines back,
                // but never behind what has already been printed. The clamp is
                // what merges overlapping groups into one — and what decides
                // the separator, since a group that starts exactly where the
                // last one stopped is a continuation and takes none.
                let floor = last_out.map_or(1, |l| l.saturating_add(1));
                let start = lineno.saturating_sub(out_before).max(floor);
                let adjacent = last_out.is_some_and(|l| start == l.saturating_add(1));
                if separate_groups
                    && *printed_before
                    && !adjacent
                    && let Some(s) = opts.group_sep.bytes()
                {
                    // `se` paints the `--` too, and the newline after it is
                    // left plain.
                    out.write_all(&opts.paint(&opts.colors.separator, s))?;
                    // A newline even under `-z`, where every *line* ends with
                    // NUL. Measured; and it follows — the separator is not a
                    // line of the file.
                    out.write_all(b"\n")?;
                }
                for (n, pos, text) in before.drain(..) {
                    if n >= start {
                        write_context_line(
                            out,
                            &text,
                            &at(n.saturating_sub(1), pos, b'-'),
                            pats,
                            opts,
                        )?;
                    }
                }
                if opts.only_matching {
                    // `-o` with `-v` prints nothing: the part of the line that
                    // did not match is the whole line, and GNU declines to call
                    // that a match.
                    if !opts.invert {
                        for (s, e) in matches_in(pats, body, opts).map_err(limit_err)? {
                            // An *empty* match is skipped rather than printed.
                            // `grep -o 'o*'` on "foo bar" prints one `oo`, not
                            // an `oo` surrounded by six blank lines: a pattern
                            // that can match nothing matches at every position,
                            // so printing those would make `-o` unusable for
                            // exactly the patterns people write it for. The
                            // *line* still counts as selected, which is why
                            // this loop can legitimately print nothing.
                            if e > s {
                                // `-bo` reports each match's own offset, not the
                                // line's: `grep -bo foo` over `foo bar foo` at
                                // offset 6 prints 6 and 14.
                                let off = here.saturating_add(u64::try_from(s).unwrap_or(0));
                                out.write_all(&line_prefix(&at(line_idx, off, b':'), opts))?;
                                // `-o` prints nothing but matches, so `sl`/`cx`
                                // never apply: the whole of what it writes is
                                // matched text, in `ms`.
                                out.write_all(&opts.paint(
                                    opts.match_cap(true),
                                    body.get(s..e).unwrap_or_default(),
                                ))?;
                                out.write_all(&[sep])?;
                            }
                        }
                    }
                } else {
                    out.write_all(&line_prefix(&at(line_idx, here, b':'), opts))?;
                    write_body(out, body, true, pats, opts)?;
                    out.write_all(&[sep])?;
                }
                // One flush per printed *line*, which under `-o` is after the
                // last of that line's matches — upstream's `prline` ends here.
                line_flush(out, opts)?;
                last_out = Some(lineno);
                *printed_before = true;
                pending_after = out_after;
            }
            if opts.max_count.is_some_and(|m| match_count >= m) {
                if pending_after == 0 {
                    break;
                }
                limit_reached = true;
            }
        } else if pending_after > 0 {
            write_context_line(out, body, &at(line_idx, here, b'-'), pats, opts)?;
            last_out = Some(lineno);
            pending_after = pending_after.saturating_sub(1);
        } else if out_before > 0 {
            before.push_back((lineno, here, body.to_vec()));
            if before.len() > out_before {
                before.pop_front();
            }
        }
        line_idx = lineno;
    }

    if opts.count_only {
        if show_filename {
            out.write_all(&opts.paint(&opts.colors.filename, filename))?;
            // As in a line prefix: `se` paints the `:`, `-Z`'s NUL stays plain.
            if opts.null_name {
                out.write_all(&[0])?;
            } else {
                out.write_all(&opts.paint(&opts.colors.separator, b":"))?;
            }
        }
        // The count itself is never coloured — there is no capability for it.
        out.write_all(match_count.to_string().as_bytes())?;
        // A newline, not `sep`: `-z` says what a *line of input* is, and a
        // count is not one. Measured — `grep -zHc` ends its count with `\n`
        // even though every matched line it would otherwise print ends with
        // NUL.
        out.write_all(b"\n")?;
        line_flush(out, opts)?;
    }

    Ok(match_count > 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// [`parse_args`] for the common case: a command line that is a search.
    ///
    /// The `panic` is the assertion — a test that spelled `--help` by accident
    /// should say so rather than quietly examine a default [`Options`].
    fn parse_ok(argv: &[&str]) -> GrepArgs {
        match parse_args(&s(argv)) {
            Ok(Request::Run(p)) => *p,
            Ok(_) => panic!("expected a search, got a help/version/usage request"),
            Err(e) => panic!("expected a search, got: {}", e.sentence),
        }
    }

    /// The sentence [`parse_args`] refuses `argv` with — without the `grep: `
    /// prefix `run_main` adds, and without the [`USAGE`] block printed under
    /// it, so a test asserts on the wording alone.
    fn parse_err(argv: &[&str]) -> String {
        match parse_args(&s(argv)) {
            Ok(_) => panic!("expected a refusal, got a request"),
            Err(e) => e.sentence,
        }
    }

    /// Compile one pattern under `opts` — most cases have exactly one.
    fn pats(pattern: &str, opts: &Options) -> Vec<Pat> {
        compile_patterns(&[pattern.as_bytes().to_vec()], opts)
            .unwrap()
            .0
    }

    /// The diagnostics compiling `patterns` under `opts` would print, without
    /// the `grep: ` prefix `main` adds.
    fn pat_warnings(patterns: &[&str], opts: &Options) -> Vec<String> {
        let owned: Vec<Vec<u8>> = patterns.iter().map(|p| p.as_bytes().to_vec()).collect();
        compile_patterns(&owned, opts).unwrap().1
    }

    /// The `unwrap` is the assertion: a test pattern that exhausted the
    /// backtracking budget would be a bug in the budget, not in the test.
    fn selects(line: &str, pattern: &str, opts: &Options) -> bool {
        line_selected(line.as_bytes(), &pats(pattern, opts), opts).unwrap()
    }

    // ---------------- parse_args ----------------

    /// A bare `grep` is not a *diagnostic* upstream — it is `usage
    /// (EXIT_TROUBLE)`, which prints the two [`USAGE`] lines and nothing above
    /// them. Asserting on the variant rather than on a sentence is the point:
    /// this used to report `missing PATTERN`, a message GNU never prints.
    #[test]
    fn parse_empty_is_a_usage_error_with_no_sentence() {
        assert_eq!(parse_args(&s(&[])).unwrap(), Request::BadUsage);
    }

    #[test]
    fn parse_help_and_version_are_requests_not_searches() {
        assert_eq!(parse_args(&s(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&s(&["-V"])).unwrap(), Request::Version);
        assert_eq!(parse_args(&s(&["--version"])).unwrap(), Request::Version);
        // Upstream tests `show_version` first, so this is the version.
        assert_eq!(
            parse_args(&s(&["--help", "--version"])).unwrap(),
            Request::Version
        );
    }

    /// The first line of [`HELP`] and of [`USAGE`] is GNU's, word for word, so
    /// that a caller matching on it still matches. Asserted because the rest of
    /// both blocks is long enough that an edit could drift the first line
    /// without anyone noticing.
    #[test]
    fn usage_and_help_open_with_gnus_own_line() {
        let gnu = "Usage: grep [OPTION]... PATTERNS [FILE]...";
        assert_eq!(USAGE.lines().next(), Some(gnu));
        assert_eq!(HELP.lines().next(), Some(gnu));
        assert_eq!(
            USAGE.lines().nth(1),
            Some("Try 'grep --help' for more information.")
        );
    }

    #[test]
    fn parse_pattern_only_reads_stdin() {
        let a = parse_ok(&["foo"]);
        assert_eq!(a.patterns, vec![b"foo".to_vec()]);
        assert_eq!(a.files, vec!["-"]);
        assert_eq!(a.opts, Options::default());
    }

    #[test]
    fn parse_pattern_and_files() {
        let a = parse_ok(&["foo", "a.txt", "b.txt"]);
        assert_eq!(a.patterns, vec![b"foo".to_vec()]);
        assert_eq!(a.files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn parse_clustered_flags() {
        let a = parse_ok(&["-ivcnr", "foo"]);
        assert!(a.opts.ignore_case);
        assert!(a.opts.invert);
        assert!(a.opts.count_only);
        assert!(a.opts.line_numbers);
        assert!(a.opts.recursive());
    }

    #[test]
    fn parse_unknown_flag_errors() {
        // `-Q` rather than `-Z`, which this test used until `-Z` was
        // implemented: an "unknown option" test has to name a letter grep does
        // not have, and every such test is one feature away from asserting the
        // wrong thing.
        //
        // The wording is glibc's now rather than this program's own: `getopt`
        // writes `invalid option -- 'Q'` for a letter and `unrecognized option
        // '--zzz'` for a name, and they are different sentences upstream too.
        assert_eq!(parse_err(&["-Q", "foo"]), "invalid option -- 'Q'");
        assert_eq!(parse_err(&["--zzz", "foo"]), "unrecognized option '--zzz'");
    }

    #[test]
    fn parse_the_two_null_flags_are_different_flags() {
        let a = parse_ok(&["-Z", "foo"]);
        assert!(a.opts.null_name && !a.opts.null_data);
        let a = parse_ok(&["-z", "foo"]);
        assert!(a.opts.null_data && !a.opts.null_name);
        let a = parse_ok(&["--null", "--null-data", "foo"]);
        assert!(a.opts.null_name && a.opts.null_data);
    }

    #[test]
    fn parse_bare_dash_is_a_filename() {
        let a = parse_ok(&["foo", "-"]);
        assert_eq!(a.files, vec!["-"]);
    }

    #[test]
    fn parse_flag_after_pattern_still_a_flag() {
        // Order-insensitive, as GNU is by default: options may follow operands.
        let a = parse_ok(&["foo", "-v", "x.txt"]);
        assert!(a.opts.invert);
        assert_eq!(a.patterns, vec![b"foo".to_vec()]);
        assert_eq!(a.files, vec!["x.txt"]);
    }

    #[test]
    fn parse_the_options_that_used_to_be_unknown() {
        // Every one of these was `unknown option` until this rewrite, and each
        // is in the failure lane C reported: `grep -E`, `grep -q`, `grep -c --`.
        assert_eq!(
            parse_ok(&["-E", "a+"]).opts.syntax,
            Syntax::Extended
        );
        assert!(parse_ok(&["-q", "a"]).opts.quiet);
        assert_eq!(
            parse_ok(&["-F", "a"]).opts.syntax,
            Syntax::Fixed
        );
        for flag in ["-w", "-x", "-o", "-l", "-L", "-H", "-h", "-s", "-a"] {
            assert!(parse_args(&s(&[flag, "a"])).is_ok(), "{flag} rejected");
        }
    }

    #[test]
    fn parse_double_dash_ends_the_options() {
        let a = parse_ok(&["-c", "--", "-v", "x.txt"]);
        assert!(a.opts.count_only);
        assert!(!a.opts.invert, "-v after -- is the pattern, not an option");
        assert_eq!(a.patterns, vec![b"-v".to_vec()]);
        assert_eq!(a.files, vec!["x.txt"]);
    }

    #[test]
    fn parse_e_makes_every_operand_a_file() {
        let a = parse_ok(&["-e", "-v", "x.txt"]);
        assert!(!a.opts.invert);
        assert_eq!(a.patterns, vec![b"-v".to_vec()]);
        assert_eq!(a.files, vec!["x.txt"]);
    }

    #[test]
    fn parse_repeated_e_collects_patterns() {
        let a = parse_ok(&["-e", "foo", "-e", "bar"]);
        assert_eq!(a.patterns, vec![b"foo".to_vec(), b"bar".to_vec()]);
    }

    #[test]
    fn parse_an_option_argument_may_be_glued_to_its_cluster() {
        assert_eq!(
            parse_ok(&["-m5", "a"]).opts.max_count,
            Some(5)
        );
        assert_eq!(
            parse_ok(&["-m", "5", "a"]).opts.max_count,
            Some(5)
        );
        assert_eq!(
            parse_ok(&["-im5", "a"]).opts.max_count,
            Some(5)
        );
        assert_eq!(
            parse_ok(&["-efoo"]).patterns,
            vec![b"foo".to_vec()]
        );
    }

    #[test]
    fn parse_a_missing_option_argument_is_an_error() {
        assert_eq!(parse_err(&["-e"]), "option requires an argument -- 'e'");
        assert_eq!(parse_err(&["-m"]), "option requires an argument -- 'm'");
        // The long form names the option first rather than last, which is
        // glibc's shape and not a choice of this program's.
        assert_eq!(
            parse_err(&["--regexp"]),
            "option '--regexp' requires an argument"
        );
        assert_eq!(parse_err(&["-m", "x", "a"]), "invalid max count");
    }

    #[test]
    fn parse_long_options() {
        let a = parse_ok(&["--ignore-case", "--max-count=2", "--regexp=foo"]);
        assert!(a.opts.ignore_case);
        assert_eq!(a.opts.max_count, Some(2));
        assert_eq!(a.patterns, vec![b"foo".to_vec()]);
        assert_eq!(
            parse_err(&["--nope", "a"]),
            "unrecognized option '--nope'"
        );
    }

    #[test]
    fn parse_recursive_with_no_operand_walks_here() {
        let a = parse_ok(&["-r", "foo"]);
        assert_eq!(a.files, vec!["."]);
        // …and prints the names it finds *without* the `./` that walking a `.`
        // would otherwise put on them, which a caller who wrote the `.` gets.
        assert!(a.omit_dot_slash);
        assert!(!parse_ok(&["-r", "foo", "."]).omit_dot_slash);
        // Not recursing means no walk to name anything, so the input is stdin
        // and the question never arises.
        let plain = parse_ok(&["foo"]);
        assert_eq!(plain.files, vec!["-"]);
        assert!(!plain.omit_dot_slash);
        // `-d recurse` reaches the same branch, because it is the same setting.
        assert_eq!(
            parse_ok(&["-d", "recurse", "foo"]).files,
            vec!["."]
        );
    }

    /// `-r`, `-R` and `-d` all write one setting, so the **last** of them wins
    /// whichever order they are written in. Two independent booleans could not
    /// express this, and expressing it is the whole reason [`Directories`]
    /// exists.
    #[test]
    fn recursion_and_d_are_one_setting_and_the_last_wins() {
        let d = |a: &[&str]| parse_ok(a).opts.directories;
        assert_eq!(d(&["foo"]), Directories::Read);
        assert_eq!(d(&["-r", "foo"]), Directories::Recurse);
        assert_eq!(d(&["-d", "skip", "foo"]), Directories::Skip);
        assert_eq!(d(&["-r", "-d", "skip", "foo"]), Directories::Skip);
        assert_eq!(d(&["-d", "skip", "-r", "foo"]), Directories::Recurse);
        assert_eq!(d(&["-r", "-d", "read", "foo"]), Directories::Read);
        // Bundled, split and long-with-`=` are the same option three ways.
        assert_eq!(d(&["-dskip", "foo"]), Directories::Skip);
        assert_eq!(d(&["--directories=skip", "foo"]), Directories::Skip);
        assert_eq!(d(&["--directories", "skip", "foo"]), Directories::Skip);
        // `-R` sets the same value *and* the dereference flag.
        let upper = parse_ok(&["-R", "foo"]).opts;
        assert_eq!(upper.directories, Directories::Recurse);
        assert!(upper.deref_links);
        // …and `-d` does not clear it, because it is a different field. GNU is
        // the same: `-R -d recurse` still follows links met during the walk.
        let both = parse_ok(&["-R", "-d", "recurse", "foo"])
            .opts;
        assert!(both.deref_links);

        // A prefix is enough, as it is for a long option's *name*: `argmatch`
        // is a prefix match, and upstream's `grep -d rec` recurses.
        assert_eq!(d(&["-d", "rec", "foo"]), Directories::Recurse);
        // gnulib's `argmatch` lists the valid words and exits **1** rather than
        // grep's usual 2. The curly quotes are gnulib's `quote`.
        let bad = parse_args(&s(&["-d", "bogus", "foo"])).unwrap_err();
        assert_eq!(
            bad.sentence,
            "invalid argument \u{2018}bogus\u{2019} for \u{2018}--directories\u{2019}\n\
             Valid arguments are:\n  - \u{2018}read\u{2019}\n  - \u{2018}recurse\u{2019}\n  - \u{2018}skip\u{2019}"
        );
        assert_eq!(bad.status, 1);
        assert!(bad.referral.is_some());
        assert_eq!(parse_err(&["-d"]), "option requires an argument -- 'd'");
    }

    /// `-D`'s default is neither "read" nor "skip" but a third thing, and the
    /// third thing is the one that keeps `grep -r` from blocking on a FIFO.
    #[test]
    fn devices_default_reads_named_ones_and_skips_found_ones() {
        let dev = |a: &[&str]| parse_ok(a).opts;

        let default = dev(&["foo"]);
        assert_eq!(default.devices, Devices::CommandLine);
        assert!(!default.skip_devices(true));
        assert!(default.skip_devices(false));

        let read = dev(&["-D", "read", "foo"]);
        assert_eq!(read.devices, Devices::Read);
        assert!(!read.skip_devices(true));
        assert!(!read.skip_devices(false));

        let skip = dev(&["--devices=skip", "foo"]);
        assert_eq!(skip.devices, Devices::Skip);
        assert!(skip.skip_devices(true));
        assert!(skip.skip_devices(false));

        // GNU checks `-D` by hand rather than through argmatch, so this is its
        // exact wording — and it does not name the offending value.
        assert_eq!(parse_err(&["-D", "bogus", "foo"]), "unknown devices method");
        // …and *not* through `argmatch`, so a prefix is refused where `-d rec`
        // is accepted. Upstream compares with `STREQ`.
        assert_eq!(parse_err(&["-D", "rea", "foo"]), "unknown devices method");
    }

    /// The three steps of gnulib's `excluded_file_name`, each isolated.
    #[test]
    fn selector_segments_are_read_newest_first() {
        let sel = |opts: &[&str]| {
            let mut a: Vec<&str> = opts.to_vec();
            a.push("foo");
            parse_ok(&a).opts.file_selectors
        };

        // Nothing at all: everything is searched.
        assert!(!sel(&[]).excludes(b"s1.txt"));

        // One include is a whitelist — rule 1 keeps what it names, rule 2
        // drops what it does not.
        let inc = sel(&["--include=*.txt"]);
        assert!(!inc.excludes(b"s1.txt"));
        assert!(inc.excludes(b"s2.log"));

        // One exclude is a blacklist, and its default is the opposite.
        let exc = sel(&["--exclude=s1*"]);
        assert!(exc.excludes(b"s1.txt"));
        assert!(!exc.excludes(b"s2.log"));

        // The pair, both ways round — the table in [`Selectors`]'s docs.
        let ie = sel(&["--include=*.txt", "--exclude=s1*"]);
        assert!(ie.excludes(b"s1.txt"));
        assert!(ie.excludes(b"s2.log"));
        assert!(!ie.excludes(b"s2.txt"));

        let ei = sel(&["--exclude=s1*", "--include=*.txt"]);
        assert!(!ei.excludes(b"s1.txt"));
        assert!(!ei.excludes(b"s2.log"));
        assert!(!ei.excludes(b"s2.txt"));

        // Consecutive same-kind options coalesce, so `--include a --include b`
        // is one segment matching either — not two segments where the newer
        // shadows the older.
        let two = sel(&["--include=*.txt", "--include=*.log"]);
        assert!(!two.excludes(b"s1.txt"));
        assert!(!two.excludes(b"s2.log"));
        assert!(two.excludes(b"s3.bin"));

        // …and a segment is only broken by a *change* of kind, which is what
        // makes three options able to mean three different things.
        let three = sel(&["--exclude=s1*", "--include=*.log", "--exclude=s2*"]);
        assert!(three.excludes(b"s2.log")); // newest segment matches
        assert!(!three.excludes(b"s3.log")); // the include matches
        assert!(!three.excludes(b"s3.txt")); // nothing matches; oldest excludes

        // An empty pattern matches nothing, so `--include=''` is a whitelist
        // with nothing on it: everything is dropped.
        assert!(sel(&["--include="]).excludes(b"s1.txt"));
        assert!(!sel(&["--exclude="]).excludes(b"s1.txt"));
    }

    /// `--exclude` and `--exclude-dir` are separate lists chosen between by
    /// what the name *is*, not two spellings of one list.
    #[test]
    fn directories_have_their_own_selector_list() {
        let opts = parse_ok(&["--exclude=sub", "--exclude-dir=deep", "foo"])
            .opts;
        assert!(opts.skipped_file(b"sub", false));
        assert!(!opts.skipped_file(b"sub", true));
        assert!(opts.skipped_file(b"deep", true));
        assert!(!opts.skipped_file(b"deep", false));

        // A trailing slash on the *pattern* is stripped; without that it could
        // never match, since no name it is compared against ends in one.
        let slash = parse_ok(&["--exclude-dir=deep//", "foo"])
            .opts;
        assert!(slash.skipped_file(b"deep", true));
    }

    /// The suffix pass of gnulib's `exclude_fnmatch`, which is what lets a
    /// pattern written without the `./` still match an operand written with it.
    #[test]
    fn a_glob_is_tried_against_every_suffix_after_a_slash() {
        assert!(glob_matches(b"top.txt", b"./top.txt"));
        assert!(glob_matches(b"./top.txt", b"./top.txt"));
        assert!(glob_matches(b"deepfile", b"a/b/c/deepfile"));
        assert!(glob_matches(b"c/deepfile", b"a/b/c/deepfile"));
        assert!(glob_matches(b"su*", b"./sub"));
        assert!(!glob_matches(b"b/deepfile", b"a/b/c/deepfile"));

        // No `FNM_PATHNAME`, so `*` crosses a `/`; no `FNM_PERIOD`, so `*`
        // matches a leading dot. Both are grep's choices, not fnmatch's
        // defaults elsewhere in this family.
        assert!(glob_matches(b"a*e", b"a/b/c/deepfile"));
        assert!(glob_matches(b"*", b".dotfile"));
        // `\` still escapes, and a bracket still negates.
        assert!(glob_matches(b"t[!1].txt", b"t2.txt"));
        assert!(!glob_matches(b"t[!1].txt", b"t1.txt"));
        assert!(glob_matches(br"t\1.txt", b"t1.txt"));
        // Case matters: grep passes no `FNM_CASEFOLD`, and `-i` does not reach
        // here — it is about the pattern, not about file names.
        assert!(!glob_matches(b"*.TXT", b"a.txt"));
    }

    /// A `--exclude-from` file is not a `-f` file: no comments, and a blank
    /// line is an empty glob rather than a glob that matches everything.
    #[test]
    fn exclude_from_splits_at_newlines_and_has_no_comment_syntax() {
        assert_eq!(split_exclude_file(b""), Vec::<Vec<u8>>::new());
        assert_eq!(split_exclude_file(b"a\n"), vec![b"a".to_vec()]);
        // A final line with no newline on it still counts.
        assert_eq!(
            split_exclude_file(b"a\nb"),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
        assert_eq!(
            split_exclude_file(b"a\n\nb\n"),
            vec![b"a".to_vec(), Vec::new(), b"b".to_vec()]
        );
        // `#` is a character a file name may begin with, so it is a glob.
        assert_eq!(split_exclude_file(b"#a\n"), vec![b"#a".to_vec()]);
    }

    /// `--exclude-from` extends the current exclude run rather than starting a
    /// segment of its own — which matters, because a segment boundary is what
    /// the newest-first scan stops at.
    #[test]
    fn exclude_from_joins_the_neighbouring_exclude_segment() {
        let dir = std::env::temp_dir().join(format!(
            "slateos-grep-exfrom-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("list");
        fs::write(&file, b"*.log\n").expect("write exclude list");
        let name = file.to_string_lossy().into_owned();

        let opts = parse_ok(&[
            "--include=*.txt",
            "--exclude=s1*",
            &format!("--exclude-from={name}"),
            "foo",
        ])
        .opts;
        assert_eq!(opts.file_selectors.segments.len(), 2);
        assert!(opts.file_selectors.excludes(b"a.log"));
        assert!(opts.file_selectors.excludes(b"s1.txt"));
        assert!(!opts.file_selectors.excludes(b"s2.txt"));

        // A file that cannot be read is an error at parse time, worded as the
        // ordinary "no such file" is.
        let missing = dir.join("nope");
        let err = parse_args(&s(&[
            &format!("--exclude-from={}", missing.to_string_lossy()),
            "foo",
        ]))
        .unwrap_err();
        assert!(err.sentence.contains("No such file"), "{}", err.sentence);

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir(&dir);
    }

    /// `-r` and `-R` are not synonyms, and the whole of the difference is one
    /// flag: whether a symbolic link *found during the walk* is followed.
    /// Treating them as synonyms made `-r` report files GNU does not, and made
    /// it run forever on a tree containing a link back to one of its own
    /// ancestors.
    #[test]
    fn parse_capital_r_is_the_dereferencing_one() {
        let lower = parse_ok(&["-r", "foo"]).opts;
        assert!(lower.recursive());
        assert!(!lower.deref_links);

        let upper = parse_ok(&["-R", "foo"]).opts;
        assert!(upper.recursive());
        assert!(upper.deref_links);

        // The long spellings say the same thing at more length, and
        // `--dereference-recursive` implies `--recursive` rather than needing
        // it alongside.
        let long = parse_ok(&["--recursive", "foo"]).opts;
        assert!(long.recursive());
        assert!(!long.deref_links);

        let long_deref = parse_ok(&["--dereference-recursive", "foo"])
            .opts;
        assert!(long_deref.recursive());
        assert!(long_deref.deref_links);
    }

    #[test]
    fn context_flags_keep_their_own_values_whatever_the_order() {
        // The whole reason the three are `Option`s: `-A` and `-B` fall back to
        // `-C` only where they were not given, and the fallback happens after
        // parsing rather than during it. A plain `usize` would make these two
        // commands differ, and GNU says they do not.
        for argv in [
            s(&["-A", "3", "-C", "1", "foo"]),
            s(&["-C", "1", "-A", "3", "foo"]),
        ] {
            let Ok(Request::Run(p)) = parse_args(&argv) else {
                panic!("expected a search")
            };
            assert_eq!(p.opts.out_after(), 3);
            assert_eq!(p.opts.out_before(), 1);
        }

        // -C alone feeds both sides.
        let c = parse_ok(&["-C", "2", "foo"]).opts;
        assert_eq!((c.out_before(), c.out_after()), (2, 2));

        // …and -A alone leaves the other side at zero without claiming that
        // no context was asked for.
        let a = parse_ok(&["-A", "2", "foo"]).opts;
        assert_eq!((a.out_before(), a.out_after()), (0, 2));
        assert!(a.context_requested());

        // `-A 0` asks for context and gets none, which is not the same state
        // as never having asked: only the first puts `--` between groups.
        let zero = parse_ok(&["-A", "0", "foo"]).opts;
        assert!(zero.context_requested());
        assert_eq!(zero.out_after(), 0);
        assert!(!parse_ok(&["foo"]).opts.context_requested());
    }

    #[test]
    fn the_digit_shorthand_accumulates_and_stops_at_the_first_non_digit() {
        let one = parse_ok(&["-1", "foo"]).opts;
        assert_eq!(one.default_context, Some(1));

        // Twelve, not two: the digits of a cluster build one number.
        let twelve = parse_ok(&["-12", "foo"]).opts;
        assert_eq!(twelve.default_context, Some(12));

        // A non-digit ends the number and is read as its own option, in
        // either order.
        let with_n = parse_ok(&["-1n", "foo"]).opts;
        assert_eq!(with_n.default_context, Some(1));
        assert!(with_n.line_numbers);

        let after_n = parse_ok(&["-n1", "foo"]).opts;
        assert_eq!(after_n.default_context, Some(1));
        assert!(after_n.line_numbers);
    }

    #[test]
    fn a_context_length_that_is_not_a_count_is_grep_s_own_diagnostic() {
        // Not the family's `invalid number`: a script matching on the message
        // is matching on this one.
        for argv in [
            &["-A", "x", "foo"][..],
            &["-B", "x", "foo"],
            &["-C", "x", "foo"],
            &["--context=x", "foo"],
            // `-A -1` is not the digit shorthand — `-A` demands an argument,
            // so `-1` is consumed as one and then refused.
            &["-A", "-1", "foo"],
        ] {
            let err = parse_err(argv);
            assert!(err.ends_with(": invalid context length argument"), "{err}");
        }
        // …but a value too *large* is accepted and clamped, because upstream's
        // `context_length_arg` treats `xstrtoimax`'s overflow as success.
        assert!(
            parse_ok(&["-A", "99999999999999999999999", "foo"])
                .opts
                .out_after()
                > 0
        );
    }

    #[test]
    fn a_long_option_takes_the_next_argv_entry_when_there_is_no_equals() {
        // getopt_long accepts both spellings, so `--regexp foo` and
        // `--regexp=foo` are one command written two ways. Taking only the
        // `=` form rejected the other as a missing argument.
        let spaced = parse_ok(&["--regexp", "foo", "words"]);
        let equals = parse_ok(&["--regexp=foo", "words"]);
        assert_eq!(spaced.patterns, equals.patterns);
        assert_eq!(spaced.files, equals.files);

        let ctx = parse_ok(&["--context", "1", "foo"]).opts;
        assert_eq!(ctx.default_context, Some(1));

        // The value is consumed, so it is not left behind as an operand.
        let mc = parse_ok(&["--max-count", "2", "foo", "words"]);
        assert_eq!(mc.opts.max_count, Some(2));
        assert_eq!(mc.files, vec!["words"]);

        let missing = parse_err(&["--context"]);
        assert_eq!(missing, "option '--context' requires an argument");
    }

    #[test]
    fn an_empty_group_separator_is_a_blank_line_and_not_an_absent_one() {
        // The distinction an `Option<Vec<u8>>` could not carry.
        let dflt = parse_ok(&["foo"]).opts;
        assert_eq!(dflt.group_sep.bytes(), Some(&b"--"[..]));

        let empty = parse_ok(&["--group-separator=", "foo"]).opts;
        assert_eq!(empty.group_sep.bytes(), Some(&b""[..]));

        let custom = parse_ok(&["--group-separator=XX", "foo"])
            .opts;
        assert_eq!(custom.group_sep.bytes(), Some(&b"XX"[..]));

        let none = parse_ok(&["--no-group-separator", "foo"])
            .opts;
        assert_eq!(none.group_sep.bytes(), None);
    }

    // ---------------- patterns are patterns ----------------
    //
    // The eight oils tests lane C reported were failing all landed here: what
    // they asked of `grep` was a regular expression, and what they got was a
    // substring search that answered "no match".

    #[test]
    fn a_bracket_expression_is_a_set_not_three_characters() {
        let o = Options::default();
        assert!(selects("declare -r a=\"1\"", " [ab]=", &o));
        assert!(selects("declare -r b=\"2\"", " [ab]=", &o));
        assert!(!selects("declare -r c=\"3\"", " [ab]=", &o));
        // …and the old behaviour is now the one that fails.
        assert!(!selects("literal [ab]= here", " [ab]=", &o));
    }

    #[test]
    fn an_anchor_anchors() {
        let o = Options::default();
        assert!(selects("posix on", "^posix", &o));
        assert!(!selects("set -o posix", "^posix", &o));
        assert!(selects("declare -a FUNCNAME", "^declare -a FUNCNAME$", &o));
        assert!(!selects(
            "declare -a FUNCNAMEX",
            "^declare -a FUNCNAME$",
            &o
        ));
    }

    #[test]
    fn basic_and_extended_disagree_about_plus_and_that_is_the_point() {
        // BRE: `a+b` is three literal characters. ERE: one or more `a`, then
        // `b`. Not a subset relation, which is why `-E` cannot be a no-op.
        let bre = Options::default();
        let ere = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        assert!(selects("a+b", "a+b", &bre));
        assert!(!selects("aab", "a+b", &bre));
        assert!(!selects("a+b", "a+b", &ere));
        assert!(selects("aab", "a+b", &ere));
        // Groups swap in the same way.
        assert!(selects("xayb", "\\(a\\|b\\)", &bre));
        assert!(selects("xayb", "(a|b)", &ere));
    }

    #[test]
    fn fixed_strings_have_no_metacharacters() {
        let f = Options {
            syntax: Syntax::Fixed,
            ..Options::default()
        };
        assert!(selects("a.c", "a.c", &f));
        assert!(!selects("abc", "a.c", &f));
        assert!(selects("cost: $5*", "$5*", &f));
    }

    #[test]
    fn case_folding_reaches_the_pattern_not_only_the_line() {
        let o = Options {
            ignore_case: true,
            ..Options::default()
        };
        assert!(selects("Hello World", "hello", &o));
        assert!(selects("HELLO", "^hel", &o));
        assert!(selects("ABC", "[a-c]*$", &o));
    }

    #[test]
    fn a_malformed_pattern_is_reported_rather_than_matched_literally() {
        let o = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        let err = compile_patterns(&[b"a[".to_vec()], &o).err().unwrap();
        assert!(err.contains("a["), "{err}");
        // A reference to a group the pattern does not have is a compile error,
        // not a literal digit.
        let err = compile_patterns(&[b"\\(a\\)\\2".to_vec()], &Options::default())
            .err()
            .unwrap();
        assert!(err.contains("backreference"), "{err}");
    }

    #[test]
    fn a_backreference_selects_a_line_that_repeats_itself() {
        let o = Options::default();
        assert!(selects("abcabc", "\\(abc\\)\\1", &o));
        assert!(!selects("abcdef", "\\(abc\\)\\1", &o));
        // The same pattern in ERE syntax, where the parentheses are bare.
        let e = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        assert!(selects("xyxy", "(xy)\\1", &e));
        assert!(!selects("xyzy", "(xy)\\1", &e));
    }

    #[test]
    fn a_pathological_backreference_gives_up_rather_than_hanging() {
        // Backreference matching is NP-hard, so the engine spends a budget and
        // then declines to answer. `line_selected` must report that as an error
        // and not as "did not match" — `-v` would otherwise print the line.
        let o = Options::default();
        let pats = compile_patterns(
            &[b"\\(a*\\)\\(a*\\)\\(a*\\)\\(a*\\)\\(a*\\)\\1\\2\\3\\4\\5b".to_vec()],
            &o,
        )
        .unwrap()
        .0;
        let line = vec![b'a'; 300];
        assert!(line_selected(&line, &pats, &o).is_err());
    }

    #[test]
    fn the_empty_pattern_matches_every_line() {
        // The engine rejects an empty pattern; `grep ''` is not an error.
        let o = Options::default();
        assert!(selects("anything", "", &o));
        assert!(selects("", "", &o));
    }

    #[test]
    fn several_patterns_are_matched_as_one_set() {
        let o = Options::default();
        let p = compile_patterns(&[b"foo".to_vec(), b"^bar".to_vec()], &o)
            .unwrap()
            .0;
        assert!(line_selected(b"a foo b", &p, &o).unwrap());
        assert!(line_selected(b"bar b", &p, &o).unwrap());
        assert!(!line_selected(b"a bar", &p, &o).unwrap());
    }

    // ---------------- -w / -x / -o ----------------

    #[test]
    fn word_regexp_needs_non_word_neighbours() {
        let o = Options {
            word: true,
            ..Options::default()
        };
        assert!(selects("foo", "foo", &o));
        assert!(selects("a foo b", "foo", &o));
        assert!(selects("(foo)", "foo", &o));
        assert!(!selects("foobar", "foo", &o));
        assert!(!selects("barfoo", "foo", &o));
        assert!(!selects("foo_bar", "foo", &o));
        // A match that is a word may begin inside one that is not, so a
        // rejected candidate must not end the search for the line.
        assert!(selects("xfoo foo", "foo", &o));
    }

    #[test]
    fn line_regexp_needs_the_whole_line() {
        let o = Options {
            whole_line: true,
            ..Options::default()
        };
        assert!(selects("foo", "foo", &o));
        assert!(!selects("foo ", "foo", &o));
        assert!(selects("foo bar", "foo.*", &o));
    }

    #[test]
    fn only_matching_reports_the_parts_not_the_line() {
        let o = Options {
            syntax: Syntax::Extended,
            only_matching: true,
            ..Options::default()
        };
        let p = pats("[0-9]+", &o);
        let spans = matches_in(&p, b"ab12cd345", &o).unwrap();
        assert_eq!(spans, vec![(2, 4), (6, 9)]);
    }

    #[test]
    fn the_longest_alternative_wins_as_posix_requires() {
        // Leftmost-*longest*, not leftmost-first: `-o` is where the difference
        // becomes visible output rather than an internal detail.
        let o = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        let p = pats("a|ab", &o);
        assert_eq!(matches_in(&p, b"ab", &o).unwrap(), vec![(0, 2)]);
        // …and across `-e` patterns, which are a set and not an order.
        let p = compile_patterns(&[b"a".to_vec(), b"ab".to_vec()], &o)
            .unwrap()
            .0;
        assert_eq!(matches_in(&p, b"ab", &o).unwrap(), vec![(0, 2)]);
    }

    // ---------------- lines are bytes ----------------

    #[test]
    fn a_line_that_is_not_utf8_is_still_a_line() {
        let o = Options::default();
        let p = pats("b", &o);
        // 0xFF begins no valid UTF-8 sequence. A `String`-typed pipeline would
        // have replaced it or refused the file.
        let (out, matched) = run_search(&[b'a', 0xFF, b'b', b'\n'], &p, &o, "f", false);
        assert!(matched);
        assert_eq!(out, vec![b'a', 0xFF, b'b', b'\n']);
    }

    #[test]
    fn a_nul_in_the_input_is_data_like_any_other_byte() {
        let o = Options::default();
        let p = pats("x", &o);
        let (out, _) = run_search(b"a\0x\n", &p, &o, "f", false);
        assert_eq!(out, b"a\0x\n");
    }

    #[test]
    fn a_final_line_without_a_newline_is_still_searched() {
        let o = Options::default();
        let p = pats("b", &o);
        let (out, matched) = run_search(b"a\nb", &p, &o, "f", false);
        assert!(matched);
        assert_eq!(out, b"b\n", "and it is printed with one");
    }

    // ---------------- line_prefix ----------------

    /// `line_prefix` reads two fields off `Options`; these name them without
    /// making every assertion below construct a whole struct.
    fn pfx_opts(line_numbers: bool, null_name: bool) -> Options {
        Options {
            line_numbers,
            null_name,
            ..Options::default()
        }
    }

    /// The prefix an assertion about names and line numbers cares about: no
    /// byte offset and no `-T` width, so neither of those two fields can
    /// colour the result of a test that is not about them.
    fn px(filename: &[u8], line_idx: usize, show_filename: bool, field: u8) -> Prefix<'_> {
        Prefix {
            filename,
            show_filename,
            line_idx,
            byte_pos: 0,
            width: 0,
            field,
        }
    }

    #[test]
    fn standard_input_has_a_name_of_its_own() {
        let o = Options::default();
        assert_eq!(&*display_name(OsStr::new("-"), &o), b"(standard input)");
        assert_eq!(&*display_name(OsStr::new("a.txt"), &o), b"a.txt");
        // `grep -H pattern -` printing `-:line` reads as part of the line.
        let shown = display_name(OsStr::new("-"), &o);
        assert_eq!(
            line_prefix(&px(&shown, 0, true, b':'), &pfx_opts(false, false)),
            b"(standard input):"
        );
    }

    /// The prefix rule is about the operands, not the expansion: one directory
    /// operand prefixes even when it holds a single file, and one symlink-to-a-
    /// file operand does not prefix at all. Ours counted the expansion, so
    /// `grep -r foo dir-with-one-file` printed a bare line where GNU prints
    /// `dir/file:line`.
    #[test]
    fn a_directory_operand_earns_the_prefix_however_little_is_in_it() {
        assert!(wants_filename(None, 1, true));
        assert!(!wants_filename(None, 1, false));
        assert!(wants_filename(None, 2, false));
        assert!(!wants_filename(None, 0, false));
        // -H and -h outrank the count either way.
        assert!(wants_filename(Some(true), 1, false));
        assert!(!wants_filename(Some(false), 9, true));
    }

    #[test]
    fn prefix_none() {
        assert_eq!(
            line_prefix(&px(b"f", 0, false, b':'), &pfx_opts(false, false)),
            b""
        );
    }

    #[test]
    fn prefix_filename_only() {
        assert_eq!(
            line_prefix(&px(b"a.txt", 0, true, b':'), &pfx_opts(false, false)),
            b"a.txt:"
        );
    }

    #[test]
    fn prefix_line_number_only() {
        assert_eq!(
            line_prefix(&px(b"ignored", 0, false, b':'), &pfx_opts(true, false)),
            b"1:"
        );
        assert_eq!(
            line_prefix(&px(b"ignored", 41, false, b':'), &pfx_opts(true, false)),
            b"42:"
        );
    }

    #[test]
    fn prefix_filename_and_line_number() {
        assert_eq!(
            line_prefix(&px(b"a.txt", 9, true, b':'), &pfx_opts(true, false)),
            b"a.txt:10:"
        );
    }

    #[test]
    fn null_ends_the_name_but_never_the_line_number() {
        // `-Z` is about the byte that follows a *file name*. Applying it to the
        // line number too would make `-nZ` output unparseable, and it is not
        // what GNU does.
        assert_eq!(
            line_prefix(&px(b"a.txt", 0, true, b':'), &pfx_opts(false, true)),
            b"a.txt\0"
        );
        assert_eq!(
            line_prefix(&px(b"a.txt", 9, true, b':'), &pfx_opts(true, true)),
            b"a.txt\x0010:"
        );
    }

    // ---------------- -b and -T ----------------

    /// The width is fixed from the file's size before a line is read, which is
    /// what lets `-T` apply to input that cannot be measured twice. Every
    /// number here was read off GNU grep 3.11.
    #[test]
    fn the_tab_width_comes_from_the_size_and_grows_by_one_for_the_line_count() {
        let plain = Options::default();
        let numbered = Options {
            line_numbers: true,
            ..Options::default()
        };
        // A file of N bytes holds at most N+1 lines, so `-n` can need one more
        // column than the size alone does — but only across a power of ten.
        for (size, without_n, with_n) in [
            (0, 1, 1),
            (1, 1, 1),
            (9, 1, 2),
            (10, 2, 2),
            (99, 2, 3),
            (100, 3, 3),
            (1492, 4, 4),
        ] {
            assert_eq!(offset_width(Some(size), &plain), without_n, "size {size}");
            assert_eq!(
                offset_width(Some(size), &numbered),
                with_n,
                "size {size} with -n"
            );
        }
        // Nothing to measure — a pipe — falls back to the largest a signed
        // `off_t` holds, which is nineteen digits either way.
        assert_eq!(offset_width(None, &plain), 19);
        assert_eq!(offset_width(None, &numbered), 19);
    }

    /// `-T` right-aligns the numeric fields and ends the prefix with a tab —
    /// *after* the last separator, with no backspace, which is the detail two
    /// readings of GNU's source got wrong and one `od -c` settled.
    #[test]
    fn the_initial_tab_follows_the_last_field_and_pads_only_the_numbers() {
        let t = |line_numbers, byte_offset, null_name| Options {
            align_tabs: true,
            line_numbers,
            byte_offset,
            null_name,
            ..Options::default()
        };
        let p = |width, show_filename| Prefix {
            filename: b"a.txt",
            show_filename,
            line_idx: 0,
            byte_pos: 0,
            width,
            field: b':',
        };
        // Both numeric fields take the same width — there is one width, not one
        // per field.
        assert_eq!(
            line_prefix(&p(3, false), &t(true, true, false)),
            b"  1:  0:\t"
        );
        // The name is never padded, and `-Z`'s NUL does not stop the tab.
        assert_eq!(
            line_prefix(&p(2, true), &t(true, false, false)),
            b"a.txt: 1:\t"
        );
        assert_eq!(
            line_prefix(&p(2, true), &t(true, false, true)),
            b"a.txt\0 1:\t"
        );
        // A name on its own still ends in a tab…
        assert_eq!(
            line_prefix(&p(2, true), &t(false, false, false)),
            b"a.txt:\t"
        );
        assert_eq!(
            line_prefix(&p(2, true), &t(false, false, true)),
            b"a.txt\0\t"
        );
        // …but an empty prefix does not gain one, because the tab follows the
        // last field and there is no field.
        assert_eq!(line_prefix(&p(2, false), &t(false, false, false)), b"");
        // And without `-T` the width is ignored outright.
        let no_t = Options {
            line_numbers: true,
            byte_offset: true,
            ..Options::default()
        };
        assert_eq!(line_prefix(&p(9, false), &no_t), b"1:0:");
    }

    /// `-b` reports where in the *file* the printed text starts, which is the
    /// line's own offset — and under `-o` each match's.
    #[test]
    fn the_byte_offset_counts_separators_and_follows_the_match_under_o() {
        let opts = Options {
            byte_offset: true,
            ..Options::default()
        };
        let p = pats("foo", &opts);
        let (out, _) = run_search(b"foo bar foo\nbaz\nfoo\n", &p, &opts, "f", false);
        assert_eq!(out, b"0:foo bar foo\n16:foo\n");

        let o = Options {
            byte_offset: true,
            only_matching: true,
            ..Options::default()
        };
        let (out, _) = run_search(b"foo bar foo\nbaz\nfoo\n", &pats("foo", &o), &o, "f", false);
        assert_eq!(out, b"0:foo\n8:foo\n16:foo\n");

        // Under `-z` the NUL separators are bytes of the file like any other.
        let z = Options {
            byte_offset: true,
            null_data: true,
            ..Options::default()
        };
        let (out, _) = run_search(b"foo\0bar\0foo\0", &pats("foo", &z), &z, "f", false);
        assert_eq!(out, b"0:foo\08:foo\0");

        // A context line carries its own offset, ended by `-` like every other
        // field of a context line's prefix.
        let c = Options {
            byte_offset: true,
            ..ctx_opts(None, None, Some(1))
        };
        let (out, _) = run_search(b"foo bar foo\nbaz\nfoo\n", &pats("baz", &c), &c, "f", false);
        assert_eq!(out, b"0-foo bar foo\n12:baz\n16-foo\n");
    }

    // ---------------- search_stream ----------------

    /// A source with no `-T` width, which is what every test that is not about
    /// `-T` wants: [`line_prefix`] ignores the width unless `align_tabs` is on,
    /// so zero here can never affect an assertion that does not set it.
    fn src(filename: &str, show_filename: bool) -> Source<'_> {
        Source {
            filename: filename.as_bytes(),
            show_filename,
            width: 0,
        }
    }

    fn run_search(
        input: &[u8],
        pats: &[Pat],
        opts: &Options,
        filename: &str,
        show_filename: bool,
    ) -> (Vec<u8>, bool) {
        let mut out: Vec<u8> = Vec::new();
        let mut printed_before = false;
        let matched = search_stream(
            &mut out,
            input,
            pats,
            &src(filename, show_filename),
            opts,
            &mut printed_before,
        )
        .unwrap();
        (out, matched)
    }

    fn run(
        input: &[u8],
        pattern: &str,
        opts: Options,
        filename: &str,
        show_filename: bool,
    ) -> (String, bool) {
        let p = pats(pattern, &opts);
        let (out, matched) = run_search(input, &p, &opts, filename, show_filename);
        (String::from_utf8(out).unwrap(), matched)
    }

    #[test]
    fn search_basic_match() {
        let (out, matched) = run(b"foo\nbar\nfoobar\n", "foo", Options::default(), "f", false);
        assert!(matched);
        assert_eq!(out, "foo\nfoobar\n");
    }

    // ---------------- context ----------------
    //
    // `CTX` is the harness fixture: eight numbered lines with hits six apart,
    // far enough that `-C 1` leaves a gap and `-C 3` closes it. Every
    // expectation below was measured against GNU grep 3.11.

    const CTX: &[u8] = b"1\n2\nHIT\n4\n5\n6\nHIT\n8\n";

    fn ctx_opts(before: Option<usize>, after: Option<usize>, default: Option<usize>) -> Options {
        Options {
            before_context: before,
            after_context: after,
            default_context: default,
            ..Options::default()
        }
    }

    #[test]
    fn context_prints_the_neighbours_and_separates_groups_that_do_not_touch() {
        let (out, _) = run(CTX, "HIT", ctx_opts(None, Some(1), None), "f", false);
        assert_eq!(out, "HIT\n4\n--\nHIT\n8\n");

        let (out, _) = run(CTX, "HIT", ctx_opts(Some(1), None, None), "f", false);
        assert_eq!(out, "2\nHIT\n--\n6\nHIT\n");

        let (out, _) = run(CTX, "HIT", ctx_opts(None, None, Some(1)), "f", false);
        assert_eq!(out, "2\nHIT\n4\n--\n6\nHIT\n8\n");
    }

    #[test]
    fn groups_that_meet_are_one_group_with_no_separator() {
        // -C 3 reaches from line 3 back over 4-6 to line 7, so the two groups
        // become one run of the whole file.
        let (out, _) = run(CTX, "HIT", ctx_opts(None, None, Some(3)), "f", false);
        assert_eq!(out, "1\n2\nHIT\n4\n5\n6\nHIT\n8\n");

        // More context than there is file clamps at both ends rather than
        // repeating lines or running off.
        let (out, _) = run(CTX, "HIT", ctx_opts(Some(99), None, None), "f", false);
        assert_eq!(out, "1\n2\nHIT\n4\n5\n6\nHIT\n");
    }

    #[test]
    fn zero_context_still_separates_where_plain_grep_does_not() {
        // The one observable difference between `-A 0` and no context at all,
        // and the reason `context_requested` is not `out_after() > 0`.
        let (out, _) = run(CTX, "HIT", ctx_opts(None, Some(0), None), "f", false);
        assert_eq!(out, "HIT\n--\nHIT\n");

        let (out, _) = run(CTX, "HIT", Options::default(), "f", false);
        assert_eq!(out, "HIT\nHIT\n");
    }

    #[test]
    fn a_context_line_is_punctuated_with_a_dash_where_a_match_uses_a_colon() {
        // In *every* field, not just the last: this byte is the only thing
        // telling a caller which lines of `grep -C` output actually matched.
        let opts = Options {
            line_numbers: true,
            ..ctx_opts(None, None, Some(1))
        };
        let (out, _) = run(CTX, "HIT", opts, "f", true);
        assert_eq!(out, "f-2-2\nf:3:HIT\nf-4-4\n--\nf-6-6\nf:7:HIT\nf-8-8\n");
    }

    #[test]
    fn the_group_separator_can_be_changed_or_removed() {
        let custom = Options {
            group_sep: GroupSep::Custom(b"XX".to_vec()),
            ..ctx_opts(None, None, Some(1))
        };
        let (out, _) = run(CTX, "HIT", custom, "f", false);
        assert_eq!(out, "2\nHIT\n4\nXX\n6\nHIT\n8\n");

        // An empty separator is a blank line — a different answer from none.
        let empty = Options {
            group_sep: GroupSep::Custom(Vec::new()),
            ..ctx_opts(None, None, Some(1))
        };
        let (out, _) = run(CTX, "HIT", empty, "f", false);
        assert_eq!(out, "2\nHIT\n4\n\n6\nHIT\n8\n");

        let none = Options {
            group_sep: GroupSep::Suppressed,
            ..ctx_opts(None, None, Some(1))
        };
        let (out, _) = run(CTX, "HIT", none, "f", false);
        assert_eq!(out, "2\nHIT\n4\n6\nHIT\n8\n");
    }

    #[test]
    fn trailing_context_outlives_the_max_count_and_demotes_what_it_covers() {
        // Measured: `grep -n -m1 -A2` over three consecutive hits prints the
        // first as a match and the next two as *context*, because once `-m` is
        // satisfied the remaining lines are never tested against the pattern.
        let opts = Options {
            line_numbers: true,
            max_count: Some(1),
            ..ctx_opts(None, Some(2), None)
        };
        let (out, _) = run(b"HIT\nHIT\nHIT\n", "HIT", opts, "f", false);
        assert_eq!(out, "1:HIT\n2-HIT\n3-HIT\n");

        // With the limit at two, the second one is still a match and only the
        // third is demoted.
        let opts = Options {
            line_numbers: true,
            max_count: Some(2),
            ..ctx_opts(None, Some(2), None)
        };
        let (out, _) = run(b"HIT\nHIT\nHIT\n", "HIT", opts, "f", false);
        assert_eq!(out, "1:HIT\n2:HIT\n3-HIT\n");

        // Leading context owes nothing once the limit is reached, so `-B`
        // stops dead where `-A` runs on.
        let opts = Options {
            line_numbers: true,
            max_count: Some(1),
            ..ctx_opts(Some(2), None, None)
        };
        let (out, _) = run(b"HIT\nHIT\nHIT\n", "HIT", opts, "f", false);
        assert_eq!(out, "1:HIT\n");
    }

    #[test]
    fn only_matching_prints_nothing_for_a_context_line_but_still_groups_by_it() {
        // The subtle half of `-o`: the group separator is decided by how far
        // the file has been read, not by how many bytes came out. `-A 1`
        // leaves a gap between the two groups and gets a `--`; `-C 2` closes
        // it and does not — even though both printed only the two `HIT`s.
        let opts = Options {
            only_matching: true,
            ..ctx_opts(None, Some(1), None)
        };
        let (out, _) = run(CTX, "HIT", opts, "f", false);
        assert_eq!(out, "HIT\n--\nHIT\n");

        let opts = Options {
            only_matching: true,
            ..ctx_opts(None, None, Some(2))
        };
        let (out, _) = run(CTX, "HIT", opts, "f", false);
        assert_eq!(out, "HIT\nHIT\n");
    }

    #[test]
    fn context_is_ignored_by_the_options_that_answer_about_the_file() {
        // `-c`, `-l`, `-L` and `-q` are questions about the file, and GNU
        // drops `-A`/`-B`/`-C` under each — separator included.
        let opts = Options {
            count_only: true,
            ..ctx_opts(None, None, Some(1))
        };
        let (out, _) = run(CTX, "HIT", opts, "f", false);
        assert_eq!(out, "2\n");

        let opts = Options {
            quiet: true,
            ..ctx_opts(None, None, Some(1))
        };
        let (out, matched) = run(CTX, "HIT", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "");
    }

    #[test]
    fn a_new_file_is_never_adjacent_to_the_one_before_it() {
        // The reason `printed_before` outlives a single file. `top` matches on
        // its first line, so its group cannot be preceded by anything within
        // its own file — and GNU still puts a `--` in front of it, because the
        // adjacency being tested is with the *previous file's* last line.
        let opts = ctx_opts(None, Some(1), None);
        let p = pats("HIT", &opts);
        let mut out: Vec<u8> = Vec::new();
        let mut printed_before = false;
        for (name, body) in [("a", CTX), ("b", &b"HIT\n2\n3\n"[..])] {
            search_stream(
                &mut out,
                body,
                &p,
                &src(name, true),
                &opts,
                &mut printed_before,
            )
            .unwrap();
        }
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "a:HIT\na-4\n--\na:HIT\na-8\n--\nb:HIT\nb-2\n"
        );

        // …and a file that prints nothing contributes no separator of its own.
        let mut out: Vec<u8> = Vec::new();
        let mut printed_before = false;
        for (name, body) in [("a", CTX), ("empty", &b""[..]), ("b", CTX)] {
            search_stream(
                &mut out,
                body,
                &p,
                &src(name, false),
                &opts,
                &mut printed_before,
            )
            .unwrap();
        }
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "HIT\n4\n--\nHIT\n8\n--\nHIT\n4\n--\nHIT\n8\n"
        );
    }

    #[test]
    fn inverted_context_is_context_around_the_lines_that_did_not_match() {
        // Every line of the fixture: the six non-hits are selected, and the
        // two hits are each a neighbour of one.
        let opts = Options {
            invert: true,
            ..ctx_opts(None, None, Some(1))
        };
        let (out, _) = run(CTX, "HIT", opts, "f", false);
        assert_eq!(out, "1\n2\nHIT\n4\n5\n6\nHIT\n8\n");
    }

    #[test]
    fn search_no_match_returns_false() {
        let (out, matched) = run(b"abc\ndef\n", "xyz", Options::default(), "f", false);
        assert!(!matched);
        assert_eq!(out, "");
    }

    #[test]
    fn search_count_only() {
        let opts = Options {
            count_only: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\nab\nabc\n", "a", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "3\n");
    }

    #[test]
    fn search_count_only_with_filename() {
        let opts = Options {
            count_only: true,
            ..Options::default()
        };
        let (out, _) = run(b"a\nab\nabc\n", "a", opts, "x.txt", true);
        assert_eq!(out, "x.txt:3\n");
    }

    #[test]
    fn search_count_of_no_matches_is_zero_not_silence() {
        let opts = Options {
            count_only: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\nb\n", "z", opts, "f", false);
        assert!(!matched);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn search_line_numbers() {
        let opts = Options {
            line_numbers: true,
            ..Options::default()
        };
        let (out, _) = run(b"x\nfoo\nbar\nfoo\n", "foo", opts, "f", false);
        assert_eq!(out, "2:foo\n4:foo\n");
    }

    #[test]
    fn search_invert() {
        let opts = Options {
            invert: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\nb\nc\n", "b", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "a\nc\n");
    }

    #[test]
    fn search_ignore_case() {
        let opts = Options {
            ignore_case: true,
            ..Options::default()
        };
        let (out, _) = run(b"FOO\nbar\nFooBar\n", "foo", opts, "f", false);
        assert_eq!(out, "FOO\nFooBar\n");
    }

    #[test]
    fn search_show_filename_prefix() {
        let (out, _) = run(b"foo\n", "foo", Options::default(), "x.txt", true);
        assert_eq!(out, "x.txt:foo\n");
    }

    #[test]
    fn search_max_count_stops_early() {
        let opts = Options {
            max_count: Some(2),
            ..Options::default()
        };
        let (out, _) = run(b"a\na\na\na\n", "a", opts, "f", false);
        assert_eq!(out, "a\na\n");
    }

    #[test]
    fn max_count_zero_is_not_no_limit_and_not_one() {
        // Measured against GNU grep 3.0: `-m 0` prints nothing and reports no
        // match, and — the part that is easy to get wrong — suppresses the
        // `-c` count line entirely rather than printing `0`.
        let opts = Options {
            max_count: Some(0),
            ..Options::default()
        };
        let (out, matched) = run(b"a\na\n", "a", opts, "f", false);
        assert_eq!(out, "");
        assert!(!matched);

        let opts = Options {
            max_count: Some(0),
            count_only: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\na\n", "a", opts, "f", true);
        assert_eq!(out, "");
        assert!(!matched);
    }

    #[test]
    fn only_matching_skips_the_empty_matches() {
        // `o*` matches the empty string at every position that is not an `o`,
        // and GNU prints none of them: `-o` on "foo bar" is one line, `oo`.
        // Printing the empty ones would surround it with six blank lines and
        // make `-o` useless for exactly the patterns it exists for.
        let opts = Options {
            only_matching: true,
            ..Options::default()
        };
        let (out, matched) = run(b"foo bar\n", "o*", opts, "f", false);
        assert_eq!(out, "oo\n");
        assert!(matched);

        // A line whose *only* matches are empty prints nothing at all, yet is
        // still a selected line — which is why the exit status is 0.
        let opts = Options {
            only_matching: true,
            ..Options::default()
        };
        let (out, matched) = run(b"xyz\n", "o*", opts, "f", false);
        assert_eq!(out, "");
        assert!(matched);
    }

    #[test]
    fn null_data_changes_what_a_line_is_on_both_sides() {
        let opts = Options {
            null_data: true,
            ..Options::default()
        };
        let (out, matched) = run(b"foo\0bar\0", "o", opts, "f", false);
        assert_eq!(out, "foo\0");
        assert!(matched);

        // A NUL-separated stream has no newlines in it, so a `-z` grep that
        // still split on `\n` would return the whole file as one line.
        let opts = Options {
            null_data: true,
            line_numbers: true,
            ..Options::default()
        };
        let (out, _) = run(b"foo\0bar\0", "a", opts, "f", false);
        assert_eq!(out, "2:bar\0");
    }

    #[test]
    fn a_count_is_not_a_line_so_z_does_not_reach_it() {
        // Measured: `grep -zHc` ends its count with a newline even though the
        // matched lines it would otherwise print end with NUL.
        let opts = Options {
            null_data: true,
            count_only: true,
            ..Options::default()
        };
        let (out, _) = run(b"foo\0bar\0", "o", opts, "f.txt", true);
        assert_eq!(out, "f.txt:1\n");

        // `-Z` *does* reach it: it is the byte after a file name.
        let opts = Options {
            null_name: true,
            count_only: true,
            ..Options::default()
        };
        let (out, _) = run(b"foo\nbar\n", "o", opts, "f.txt", true);
        assert_eq!(out, "f.txt\x001\n");
    }

    #[test]
    fn search_quiet_prints_nothing_but_answers() {
        let opts = Options {
            quiet: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\nfoo\n", "foo", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "");
    }

    #[test]
    fn search_only_matching_prints_each_match_on_its_own_line() {
        let opts = Options {
            syntax: Syntax::Extended,
            only_matching: true,
            line_numbers: true,
            ..Options::default()
        };
        let (out, _) = run(b"ab12cd345\nnone\n", "[0-9]+", opts, "f", false);
        assert_eq!(out, "1:12\n1:345\n");
    }

    #[test]
    fn search_only_matching_with_invert_prints_nothing() {
        let opts = Options {
            only_matching: true,
            invert: true,
            ..Options::default()
        };
        let (out, matched) = run(b"abc\n", "z", opts, "f", false);
        assert!(matched, "the line is still selected");
        assert_eq!(out, "");
    }

    // ---------------- split_patterns ----------------

    #[test]
    fn a_pattern_file_holds_one_pattern_per_line() {
        assert_eq!(
            split_patterns(b"foo\nbar\n"),
            vec![b"foo".to_vec(), b"bar".to_vec()]
        );
        assert_eq!(
            split_patterns(b"foo\nbar"),
            vec![b"foo".to_vec(), b"bar".to_vec()]
        );
        // A trailing newline ends the last pattern; it does not begin an empty
        // one, which would match every line and turn grep into cat.
        assert_eq!(split_patterns(b"foo\n"), vec![b"foo".to_vec()]);
        // An empty file, though, *is* the empty pattern.
        assert_eq!(split_patterns(b""), vec![Vec::<u8>::new()]);
        assert_eq!(
            split_patterns(b"a\n\nb\n").len(),
            3,
            "a blank line is the empty pattern"
        );
    }

    /// A newline in `-e` or in the positional pattern separates patterns.
    ///
    /// Every row measured against grep 3.11 by searching a file of `aaa`/`bbb`
    /// and comparing which lines came back.
    #[test]
    fn a_newline_in_a_pattern_argument_separates_patterns() {
        assert_eq!(
            split_arg_patterns(b"aaa\nccc"),
            vec![b"aaa".to_vec(), b"ccc".to_vec()]
        );
        // Unlike a `-f` file, a trailing newline here begins an empty pattern
        // rather than ending the last one — and the empty pattern matches every
        // line, so this really does turn the search into `cat`. Measured:
        // `grep -E -e 'aaa'$'\n' f` prints all of `f`.
        assert_eq!(
            split_arg_patterns(b"aaa\n"),
            vec![b"aaa".to_vec(), Vec::new()]
        );
        assert_eq!(
            split_arg_patterns(b"\naaa"),
            vec![Vec::new(), b"aaa".to_vec()]
        );
        // One argument with no newline is one pattern, and the empty argument
        // is the empty pattern — not zero patterns, which would make `grep -e
        // '' f` read its pattern from the operand instead.
        assert_eq!(split_arg_patterns(b"aaa"), vec![b"aaa".to_vec()]);
        assert_eq!(split_arg_patterns(b""), vec![Vec::<u8>::new()]);
    }

    #[test]
    fn a_multiline_pattern_argument_reaches_the_pattern_list() {
        assert_eq!(
            parse_ok(&["-e", "aaa\nccc", "f"]).patterns,
            vec![b"aaa".to_vec(), b"ccc".to_vec()]
        );
        assert_eq!(
            parse_ok(&["--regexp=aaa\nccc", "f"])
                .patterns,
            vec![b"aaa".to_vec(), b"ccc".to_vec()]
        );
        assert_eq!(
            parse_ok(&["aaa\nccc", "f"]).patterns,
            vec![b"aaa".to_vec(), b"ccc".to_vec()]
        );
        // Splitting the operand must not consume it twice: the file list is
        // what is left after the pattern, however many patterns it held.
        assert_eq!(parse_ok(&["aaa\nccc", "f"]).files, vec!["f"]);
        // Several `-e` accumulate exactly as one argument holding both would.
        assert_eq!(
            parse_ok(&["-e", "a\n", "-e", "b", "f"])
                .patterns,
            parse_ok(&["-e", "a\n\nb", "f"]).patterns
        );
    }

    /// `-E` reports a quantifier with nothing before it, and still searches.
    ///
    /// Every row measured against grep 3.11: the pattern was run on one line of
    /// stdin and stderr compared verbatim. The engine decides *which* patterns
    /// warn — see `ere::Warning` and its own test — so what is checked here is
    /// grep's half: the wording, that only `-E` produces any, and that a
    /// duplicate pattern is collapsed rather than warned about twice.
    #[test]
    fn a_quantifier_with_nothing_before_it_is_reported_under_e() {
        let e = &Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        assert_eq!(
            pat_warnings(&["*a"], e),
            vec!["warning: * at start of expression"]
        );
        assert_eq!(
            pat_warnings(&["{2}a"], e),
            vec!["warning: {...} at start of expression"],
            "an interval is named by class, not by the text that was written"
        );
        // One line per operator, in order, and the pattern is never named --
        // which is why two different patterns give two identical lines.
        assert_eq!(pat_warnings(&["*+?a"], e).len(), 3);
        assert_eq!(
            pat_warnings(&["*a", "*b"], e),
            vec![
                "warning: * at start of expression",
                "warning: * at start of expression"
            ]
        );
        // GNU collapses a repeated pattern before compiling it, so it warns
        // once. Measured: `-e '*a' -e '*a'` prints one line, `-e '*a' -e '*b'`
        // two.
        assert_eq!(pat_warnings(&["*a", "*a"], e).len(), 1);
        // Ordinary patterns are silent, and so is every other syntax: `*` at
        // the start of a BRE is a literal asterisk, and `-F` escapes it.
        assert!(pat_warnings(&["x*", "^a*", "()*"], e).is_empty());
        for syntax in [Syntax::Basic, Syntax::Fixed] {
            let o = &Options {
                syntax,
                ..Options::default()
            };
            assert!(
                pat_warnings(&["*a"], o).is_empty(),
                "{syntax:?} has no operator to complain about"
            );
        }
    }

    /// A warned-about pattern is compiled and run like any other.
    #[test]
    fn a_warned_pattern_still_searches() {
        let o = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        // `*a` is the empty expression repeated, then `a` — so it selects every
        // line holding an `a`, exactly as `a` does.
        let p = pats("*a", &o);
        assert!(line_selected(b"xax", &p, &o).unwrap());
        assert!(!line_selected(b"xxx", &p, &o).unwrap());
    }

    /// Collapsing a duplicate pattern does not change what is selected.
    ///
    /// It is a search optimisation that happens to be observable only through
    /// the diagnostic above, so the thing worth pinning is that it stays
    /// unobservable everywhere else.
    #[test]
    fn a_duplicate_pattern_is_collapsed_without_changing_the_answer() {
        let o = Options::default();
        let (one, _) = compile_patterns(&[b"foo".to_vec()], &o).unwrap();
        let (twice, _) =
            compile_patterns(&[b"foo".to_vec(), b"foo".to_vec(), b"foo".to_vec()], &o).unwrap();
        assert_eq!(twice.len(), one.len(), "the copies are dropped");
        for line in [&b"a foo b"[..], b"foo", b"bar", b""] {
            assert_eq!(
                line_selected(line, &one, &o).unwrap(),
                line_selected(line, &twice, &o).unwrap(),
                "{line:?}"
            );
        }
        // The empty pattern deduplicates too, and one copy of it still matches
        // every line -- dropping it entirely would be the damaging mistake.
        let (empty, _) = compile_patterns(&[Vec::new(), Vec::new()], &o).unwrap();
        assert_eq!(empty.len(), 1);
        assert!(line_selected(b"anything", &empty, &o).unwrap());
    }

    #[test]
    fn quoting_a_literal_defuses_every_metacharacter() {
        assert_eq!(quote_ere(b"a.c"), b"a\\.c".to_vec());
        assert_eq!(quote_ere(b"a+b|c"), b"a\\+b\\|c".to_vec());
        assert_eq!(quote_ere(b"plain"), b"plain".to_vec());
    }

    // ---------------- colour ----------------

    /// Options with colour on and everything else default, so that a colour
    /// assertion is never reading a second feature by accident.
    fn colored() -> Options {
        Options {
            color_when: ColorWhen::Always,
            color: true,
            ..Options::default()
        }
    }

    /// What `search_stream` writes, as bytes — colour output is not text, and
    /// `String` would hide exactly the escapes being asserted about.
    fn painted(input: &[u8], pattern: &str, opts: &Options, show_filename: bool) -> Vec<u8> {
        let p = pats(pattern, opts);
        run_search(input, &p, opts, "f", show_filename).0
    }

    #[test]
    fn a_when_word_is_one_of_three_answers_and_bare_means_auto() {
        let w = |word: &str| color_when(Some(OsStr::new(word)));
        assert_eq!(color_when(None), Some(ColorWhen::Auto));
        for word in ["always", "yes", "force"] {
            assert_eq!(w(word), Some(ColorWhen::Always));
        }
        for word in ["never", "no", "none"] {
            assert_eq!(w(word), Some(ColorWhen::Never));
        }
        for word in ["auto", "tty", "if-tty"] {
            assert_eq!(w(word), Some(ColorWhen::Auto));
        }
        // The words are matched case-insensitively.
        assert_eq!(w("ALWAYS"), Some(ColorWhen::Always));
        // An unrecognised WHEN is not an error: GNU prints its help and exits 0.
        assert_eq!(w("bogus"), None);
    }

    #[test]
    fn a_capability_without_a_value_is_ignored_and_a_boolean_is_not() {
        // `ms` is not `ms=`: the first leaves the default highlight standing,
        // the second removes it. Getting this wrong turns a stray `GREP_COLORS`
        // in a profile into a grep that quietly stops highlighting.
        let mut c = Colors::default();
        c.apply(b"ms");
        assert_eq!(c.selected_match, b"01;31".to_vec());
        c.apply(b"ms=");
        assert!(c.selected_match.is_empty());

        // The two booleans have no value to miss, and fire either way.
        let mut c = Colors::default();
        c.apply(b"rv:ne");
        assert!(c.reverse_video && c.no_erase);
        let mut c = Colors::default();
        c.apply(b"rv=1:ne=1");
        assert!(c.reverse_video && c.no_erase);
    }

    #[test]
    fn an_unknown_key_or_an_unusable_value_is_ignored_in_silence() {
        let mut c = Colors::default();
        // Not one of the ten keys; not SGR parameters; nothing at all.
        c.apply(b"zz=1:ms=nope::");
        assert_eq!(c, Colors::default());
        // An empty specification is not a specification.
        let mut c = Colors::default();
        c.apply(b"");
        assert_eq!(c, Colors::default());
    }

    #[test]
    fn mt_sets_both_match_colours_and_the_last_assignment_wins() {
        let mut c = Colors::default();
        c.apply(b"mt=44");
        assert_eq!(c.selected_match, b"44".to_vec());
        assert_eq!(c.context_match, b"44".to_vec());
        // Order decides, because each key assigns rather than merges.
        let mut c = Colors::default();
        c.apply(b"ms=44:mt=45");
        assert_eq!(c.selected_match, b"45".to_vec());
        let mut c = Colors::default();
        c.apply(b"mt=45:ms=44");
        assert_eq!(c.selected_match, b"44".to_vec());
        assert_eq!(c.context_match, b"45".to_vec());
    }

    #[test]
    fn ne_drops_the_erase_that_otherwise_follows_every_escape() {
        let c = Colors::default();
        assert_eq!(c.wrap(b"32", b"x"), b"\x1b[32m\x1b[Kx\x1b[m\x1b[K".to_vec());
        let mut c = Colors::default();
        c.apply(b"ne");
        assert_eq!(c.wrap(b"32", b"x"), b"\x1b[32mx\x1b[m".to_vec());
        // An empty capability writes the text and nothing else, which is what
        // makes the default `sl=`/`cx=` mean "leave this alone".
        assert_eq!(c.wrap(b"", b"x"), b"x".to_vec());
    }

    #[test]
    fn every_prefix_field_carries_its_own_colour_and_the_delimiters_do_not() {
        let opts = Options {
            line_numbers: true,
            byte_offset: true,
            ..colored()
        };
        assert_eq!(
            line_prefix(
                &Prefix {
                    filename: b"f",
                    show_filename: true,
                    line_idx: 0,
                    byte_pos: 7,
                    width: 0,
                    field: b':',
                },
                &opts
            ),
            [
                b"\x1b[35m\x1b[Kf\x1b[m\x1b[K".as_slice(), // fn
                b"\x1b[36m\x1b[K:\x1b[m\x1b[K".as_slice(), // se
                b"\x1b[32m\x1b[K1\x1b[m\x1b[K".as_slice(), // ln
                b"\x1b[36m\x1b[K:\x1b[m\x1b[K".as_slice(), // se
                b"\x1b[32m\x1b[K7\x1b[m\x1b[K".as_slice(), // bn
                b"\x1b[36m\x1b[K:\x1b[m\x1b[K".as_slice(), // se
            ]
            .concat()
        );

        // `-T`'s padding belongs *inside* the number's escape — a terminal
        // whose `ln` sets a background paints the padding too — and its tab
        // belongs outside every escape, as does `-Z`'s NUL.
        let opts = Options {
            line_numbers: true,
            align_tabs: true,
            null_name: true,
            ..colored()
        };
        assert_eq!(
            line_prefix(
                &Prefix {
                    filename: b"f",
                    show_filename: true,
                    line_idx: 0,
                    byte_pos: 0,
                    width: 3,
                    field: b':',
                },
                &opts
            ),
            [
                b"\x1b[35m\x1b[Kf\x1b[m\x1b[K".as_slice(),
                b"\0".as_slice(),
                b"\x1b[32m\x1b[K  1\x1b[m\x1b[K".as_slice(),
                b"\x1b[36m\x1b[K:\x1b[m\x1b[K".as_slice(),
                b"\t".as_slice(),
            ]
            .concat()
        );
    }

    #[test]
    fn by_default_only_the_matches_are_painted() {
        // `sl` and `cx` are empty out of the box, so the text around a match is
        // written with no escapes at all — not with an escape naming no colour.
        assert_eq!(
            painted(b"foo bar foo\nqux\n", "foo", &colored(), false),
            b"\x1b[01;31m\x1b[Kfoo\x1b[m\x1b[K bar \x1b[01;31m\x1b[Kfoo\x1b[m\x1b[K\n".to_vec()
        );
    }

    #[test]
    fn a_selected_line_colour_opens_before_each_match_and_closes_only_at_the_tail() {
        let mut opts = colored();
        opts.colors.apply(b"sl=33");
        // `qux foo bar` has text before, between (none) and after its match:
        // the run before the match is opened and left open — the match's own
        // escape pair ends it — and only the trailing run is closed.
        assert_eq!(
            painted(b"qux foo bar\n", "foo", &opts, false),
            [
                b"\x1b[33m\x1b[Kqux ".as_slice(),
                b"\x1b[01;31m\x1b[Kfoo\x1b[m\x1b[K".as_slice(),
                b"\x1b[33m\x1b[K bar\x1b[m\x1b[K".as_slice(),
                b"\n".as_slice(),
            ]
            .concat()
        );
        // A line that *ends* on its match has no tail, and so no closing
        // escape of its own.
        assert_eq!(
            painted(b"qux foo\n", "foo", &opts, false),
            [
                b"\x1b[33m\x1b[Kqux ".as_slice(),
                b"\x1b[01;31m\x1b[Kfoo\x1b[m\x1b[K".as_slice(),
                b"\n".as_slice(),
            ]
            .concat()
        );
    }

    #[test]
    fn with_no_match_colour_the_whole_line_is_one_closed_run() {
        // The shape changes, not just an escape: with `ms` empty there is no
        // per-match pass, so the line is a single `sl` run — opened, written,
        // closed — rather than one run per match left hanging.
        let mut opts = colored();
        opts.colors.apply(b"ms=:sl=33");
        assert_eq!(
            painted(b"foo bar foo\n", "foo", &opts, false),
            b"\x1b[33m\x1b[Kfoo bar foo\x1b[m\x1b[K\n".to_vec()
        );
        // And with neither, nothing is painted at all.
        let mut opts = colored();
        opts.colors.apply(b"ms=");
        assert_eq!(
            painted(b"foo bar foo\n", "foo", &opts, false),
            b"foo bar foo\n".to_vec()
        );
    }

    #[test]
    fn an_empty_match_is_not_painted_any_more_than_it_is_printed() {
        let mut opts = colored();
        opts.colors.apply(b"sl=33");
        // `o*` matches nothing at most positions and `oo` at one. Painting the
        // empty matches would bury the line in escapes, so they are skipped —
        // the same rule `-o` follows — and the line's tail run covers them.
        assert_eq!(
            painted(b"foo bar\n", "o*", &opts, false),
            [
                b"\x1b[33m\x1b[Kf".as_slice(),
                b"\x1b[01;31m\x1b[Koo\x1b[m\x1b[K".as_slice(),
                b"\x1b[33m\x1b[K bar\x1b[m\x1b[K".as_slice(),
                b"\n".as_slice(),
            ]
            .concat()
        );
        // A pattern that can *only* match nothing paints no match at all, and
        // the whole line is the tail.
        assert_eq!(
            painted(b"foo\n", "", &opts, false),
            b"\x1b[33m\x1b[Kfoo\x1b[m\x1b[K\n".to_vec()
        );
    }

    #[test]
    fn the_carriage_return_of_a_crlf_line_is_terminator_and_not_text() {
        let mut opts = colored();
        opts.colors.apply(b"ms=:sl=33");
        // A CR immediately before the line separator is left outside the run…
        assert_eq!(
            painted(b"foo\r\n", "foo", &opts, false),
            b"\x1b[33m\x1b[Kfoo\x1b[m\x1b[K\r\n".to_vec()
        );
        // …but a CR anywhere else in the line is ordinary text.
        assert_eq!(
            painted(b"foo\rzz\n", "foo", &opts, false),
            b"\x1b[33m\x1b[Kfoo\rzz\x1b[m\x1b[K\n".to_vec()
        );
        // A final line with no separator still ends in a CR that is terminator.
        assert_eq!(
            painted(b"foo\r", "foo", &opts, false),
            b"\x1b[33m\x1b[Kfoo\x1b[m\x1b[K\r\n".to_vec()
        );
    }

    #[test]
    fn under_invert_the_highlight_follows_the_matches_onto_the_context_lines() {
        // `-v` selects what did *not* match, so the selected lines have nothing
        // to highlight and the context lines have everything — in `mc`, not
        // `ms`. This is the one place the two match capabilities differ.
        let opts = Options {
            invert: true,
            ..ctx_colored(b"mc=44")
        };
        assert_eq!(
            painted(b"foo\nbar\nfoo\n", "bar", &opts, false),
            [
                num(1, b':').as_slice(),
                b"foo\n".as_slice(),
                num(2, b'-').as_slice(),
                b"\x1b[44m\x1b[Kbar\x1b[m\x1b[K\n".as_slice(),
                num(3, b':').as_slice(),
                b"foo\n".as_slice(),
            ]
            .concat()
        );
    }

    /// `-C 1` with line numbers and colour on, plus one `GREP_COLORS` spec —
    /// the shape every context-colouring assertion below needs.
    fn ctx_colored(spec: &[u8]) -> Options {
        let mut opts = Options {
            line_numbers: true,
            default_context: Some(1),
            ..colored()
        };
        opts.colors.apply(spec);
        opts
    }

    /// A `-n` prefix in the default palette: the number in `ln`, the separator
    /// in `se`. Spelled once because a context assertion is about the *body*,
    /// and repeating twenty bytes of escape in front of each one would hide it.
    fn num(n: u64, field: u8) -> Vec<u8> {
        let c = Colors::default();
        let mut v = c.wrap(&c.line_number, n.to_string().as_bytes());
        v.extend_from_slice(&c.wrap(&c.separator, &[field]));
        v
    }

    #[test]
    fn rv_swaps_the_line_colours_but_only_under_invert() {
        // Without `-v`, `rv` changes nothing: the selected lines are still the
        // interesting ones.
        let opts = ctx_colored(b"rv:sl=33:cx=34");
        assert_eq!(
            painted(b"a\nbar\nc\n", "bar", &opts, false),
            [
                num(1, b'-').as_slice(),
                b"\x1b[34m\x1b[Ka\x1b[m\x1b[K\n".as_slice(),
                num(2, b':').as_slice(),
                b"\x1b[33m\x1b[K\x1b[01;31m\x1b[Kbar\x1b[m\x1b[K\n".as_slice(),
                num(3, b'-').as_slice(),
                b"\x1b[34m\x1b[Kc\x1b[m\x1b[K\n".as_slice(),
            ]
            .concat()
        );
        // With `-v` it swaps them, so the lines carrying the matches are the
        // ones wearing `sl`.
        let opts = Options {
            invert: true,
            ..ctx_colored(b"rv:sl=33:cx=34")
        };
        assert_eq!(
            painted(b"a\nbar\nc\n", "bar", &opts, false),
            [
                num(1, b':').as_slice(),
                b"\x1b[34m\x1b[Ka\x1b[m\x1b[K\n".as_slice(),
                num(2, b'-').as_slice(),
                b"\x1b[33m\x1b[K\x1b[01;31m\x1b[Kbar\x1b[m\x1b[K\n".as_slice(),
                num(3, b':').as_slice(),
                b"\x1b[34m\x1b[Kc\x1b[m\x1b[K\n".as_slice(),
            ]
            .concat()
        );
    }

    #[test]
    fn the_group_separator_is_painted_and_its_newline_is_not() {
        let opts = ctx_colored(b"se=45");
        let out = painted(b"HIT\nx\ny\nz\nHIT\n", "HIT", &opts, false);
        let want = b"\x1b[45m\x1b[K--\x1b[m\x1b[K\n";
        assert!(
            out.windows(want.len()).any(|w| w == want.as_slice()),
            "group separator painted, newline plain: {out:?}"
        );
    }

    #[test]
    fn only_matching_prints_matched_text_and_therefore_only_a_match_colour() {
        // `-o` writes nothing but matches, so `sl` has nothing to apply to —
        // setting it changes the output not at all.
        let mut opts = Options {
            only_matching: true,
            ..colored()
        };
        opts.colors.apply(b"sl=33:cx=34");
        assert_eq!(
            painted(b"foo bar foo\n", "foo", &opts, false),
            [
                b"\x1b[01;31m\x1b[Kfoo\x1b[m\x1b[K\n".as_slice(),
                b"\x1b[01;31m\x1b[Kfoo\x1b[m\x1b[K\n".as_slice(),
            ]
            .concat()
        );
    }

    #[test]
    fn the_file_name_outputs_colour_the_name_and_nothing_else() {
        // `-c` paints the name and the `:` but never the count: there is no
        // capability for a count.
        let opts = Options {
            count_only: true,
            ..colored()
        };
        assert_eq!(
            painted(b"foo\nbar\n", "foo", &opts, true),
            b"\x1b[35m\x1b[Kf\x1b[m\x1b[K\x1b[36m\x1b[K:\x1b[m\x1b[K1\n".to_vec()
        );
        // Under `-Z` the NUL replaces the separator and stays outside the
        // escapes, exactly as it does in a line prefix.
        let opts = Options {
            count_only: true,
            null_name: true,
            ..colored()
        };
        assert_eq!(
            painted(b"foo\nbar\n", "foo", &opts, true),
            b"\x1b[35m\x1b[Kf\x1b[m\x1b[K\x001\n".to_vec()
        );
    }
}
