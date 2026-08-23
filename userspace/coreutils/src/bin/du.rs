//! du — estimate file space usage.
//!
//! This is a port of GNU `du`, written against measurements of GNU coreutils
//! 9.4 rather than against a reading of its source. Where a rule below looks
//! arbitrary, it is because it *is* arbitrary and was observed; the probes are
//! quoted in the doc comment of whichever function implements the rule.
//!
//! # What it replaces
//!
//! The previous `du` accepted three options — `-h`, `-s`, `-a` — parsed argv by
//! hand into `Vec<String>` (so a file name that is not UTF-8 aborted the
//! program before it printed anything), silently treated every `--long-option`
//! as a *file name*, printed paths through `Path::display` (which replaces
//! undecodable bytes rather than reporting them), and had no hard-link
//! detection at all, so `du -c f f` on one file counted it twice. It also
//! divided by 1024 in the non-`-h` case, ignoring `-B`, `DU_BLOCK_SIZE` and
//! `POSIXLY_CORRECT` because it did not know they existed.
//!
//! # The three parts that are not obvious
//!
//! **The block size is a grammar, not a number.** `-B`, `--block-size` and the
//! `DU_BLOCK_SIZE`/`BLOCK_SIZE`/`BLOCKSIZE` chain all run through gnulib's
//! `humblock`, which decides *both* the divisor and the rendering flags from
//! one string: `-B K` prints `3080K` while `-B 1K` prints `3080`, because a
//! spec with no digit in it also turns the unit suffix on. See [`humblock`].
//!
//! **A size is counted once per inode, not once per name.** Without `-l`, the
//! second and later encounters of a `(st_dev, st_ino)` pair are skipped
//! entirely — not printed, not added to the parent, and not added to the `-c`
//! grand total. Measured: `du -c f f` on one 4 KiB file totals 4096, and
//! `du -l -c f f` totals 8192.
//!
//! **`-S` needs two accumulators, not one.** A directory under `--separate-dirs`
//! *displays* its own size plus its non-directory children, but still
//! *propagates* the whole subtree upward, because its parent's figure and the
//! grand total are unaffected by how the child chose to display itself. One
//! accumulator cannot be both. See [`Walk::entry`].
//!
//! # Not implemented
//!
//! `--time` and `--time-style` are recognised, rejected by name, and left in
//! the option table — the table is what decides whether `--t` is ambiguous, so
//! removing them would make `du --t` answer something GNU does not. They want a
//! shared `time_style` parser (`ls -l`, `date`, `pr` and `stat` all need the
//! same one), which is a module of its own rather than a corner of this file.
//!
//! Built only on unix-family targets — our `x86_64-slateos` presents as
//! `linux-musl`, so `cfg(unix)` matches. On a non-unix host the walk is still
//! compiled and unit-tested against [`FakeTree`]; only [`RealTree`], which
//! needs `st_dev`/`st_ino`/`st_blocks`, is gated out.

#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::errmsg::strerror;
use coreutils::fnmatch::{Flags, fnmatch};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::human::{Opts, default_block_size, human_readable};
use coreutils::quote::{os_bytes, quote, quoteaf, quotef};
// Only [`RealTree`] turns a byte path back into an `OsString`, and it is the
// half of this file that the Windows development host does not compile — so
// importing this unconditionally is an unused import there and a needed one on
// the target. Anything that is `cfg(unix)` in this file is checked with
// `cargo check --target x86_64-unknown-linux-gnu`; the host build alone would
// not have caught this.
#[cfg(unix)]
use coreutils::quote::os_from_bytes;
use coreutils::xnum::{Status, strtol_fatal, xstrtoimax_base};
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

const DU: Program = Program::new("du", 1);

/// GNU's own short-option string, in GNU's own order.
///
/// The order is observable — `du --=x` lists the long table in declaration
/// order — so it is reproduced rather than sorted.
const SHORT_OPTIONS: &str = "0abd:chHklmst:xB:DLPSX:";

/// GNU's long table, in declaration order. `si` sits between `inodes` and
/// `max-depth`, which is not alphabetical and is not a typo.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("apparent-size", Takes::Nothing),
    ("block-size", Takes::Required),
    ("bytes", Takes::Nothing),
    ("count-links", Takes::Nothing),
    ("dereference", Takes::Nothing),
    ("dereference-args", Takes::Nothing),
    ("exclude", Takes::Required),
    ("exclude-from", Takes::Required),
    ("files0-from", Takes::Required),
    ("human-readable", Takes::Nothing),
    ("inodes", Takes::Nothing),
    ("si", Takes::Nothing),
    ("max-depth", Takes::Required),
    ("null", Takes::Nothing),
    ("no-dereference", Takes::Nothing),
    ("one-file-system", Takes::Nothing),
    ("separate-dirs", Takes::Nothing),
    ("summarize", Takes::Nothing),
    ("total", Takes::Nothing),
    ("threshold", Takes::Required),
    ("time", Takes::Optional),
    ("time-style", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

// ------------------------------------------------------------ block sizes ---

/// `-t`'s suffix list, which is *not* `-B`'s.
///
/// Measured, GNU du 9.4: `-t 1m` and `-t 1k` are accepted but `-t 1g`, `-t 1t`,
/// `-t 1p`, `-t 1e`, `-t 1z` and `-t 1y` are `invalid suffix in -t argument`.
/// Lower case is honoured for `k` and `m` and refused for every larger letter,
/// which is upstream's list verbatim.
const THRESHOLD_SUFFIXES: &[u8] = b"kKmMGTPEZYRQ0";

/// How a count is turned into the text of a column: the divisor and the flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Format {
    opts: Opts,
    block_size: u64,
}

impl Format {
    /// The block size with no `-B` and no environment variable set.
    const fn plain(posixly_correct: bool) -> Self {
        Self {
            opts: Opts::NONE,
            block_size: default_block_size(posixly_correct),
        }
    }
}

/// gnulib's `human_options`, in `du`'s own vocabulary: [`Format`] rather than a
/// loose pair.
///
/// The parsing itself is [`coreutils::human::human_options`] — `ls
/// --block-size`, `df -B` and the `BLOCK_SIZE` environment variable all reach
/// the same code, which they must, because `BLOCK_SIZE=K` in a profile has to
/// mean the same thing to every utility that reads it.
fn human_options(spec: &[u8], posixly_correct: bool) -> (Format, Status) {
    let (block_size, opts, status) = coreutils::human::human_options(spec, posixly_correct);
    (Format { opts, block_size }, status)
}

/// The environment's opinion of the block size, before any option is read.
///
/// The chain is first-*set* rather than first-non-empty: measured, with
/// `DU_BLOCK_SIZE=` (empty) and `BLOCK_SIZE=1` both exported, GNU du prints
/// 1024-byte blocks — the empty string was taken, failed to parse, and was
/// repaired to the default, rather than passed over in favour of `BLOCK_SIZE`.
///
/// The third variable is the BSD spelling, which is why the names of the first
/// two differ from it by a single underscore.
fn environment_format(
    du_block_size: Option<&[u8]>,
    block_size: Option<&[u8]>,
    bsd_blocksize: Option<&[u8]>,
    posixly_correct: bool,
) -> Format {
    match du_block_size.or(block_size).or(bsd_blocksize) {
        Some(spec) => human_options(spec, posixly_correct).0,
        None => Format::plain(posixly_correct),
    }
}

// ------------------------------------------------------------- exclusions ---

/// gnulib's `exclude_fnmatch` without `EXCLUDE_ANCHORED`: the pattern is tried
/// against the whole name and then against every suffix that begins just after
/// a `/`.
///
/// The flags handed to [`fnmatch`] are **empty** — no `PATHNAME`, no `PERIOD` —
/// which is what makes `du --exclude='ex*bb'` prune `ex/aa/bb` (the `*` crosses
/// a separator) and `du --exclude='ex/*'` prune `ex/.hid` (a leading dot is not
/// special). Both measured.
///
/// The `p[1] != '/'` guard means a run of slashes offers only its last position
/// as a restart point, so `a//b` is tried as `a//b` and `b`, never as `/b`.
fn matches_unanchored(pattern: &[u8], name: &[u8]) -> bool {
    if fnmatch(pattern, name, Flags::NONE) {
        return true;
    }
    for (index, &byte) in name.iter().enumerate() {
        if byte != b'/' || name.get(index.saturating_add(1)) == Some(&b'/') {
            continue;
        }
        let Some(tail) = name.get(index.saturating_add(1)..) else {
            continue;
        };
        if fnmatch(pattern, tail, Flags::NONE) {
            return true;
        }
    }
    false
}

/// Split an `--exclude-from` file into patterns.
///
/// The separator is hard-coded `'\n'` upstream (`add_exclude_file (…, '\n')`),
/// so a NUL-separated exclude file is not a thing `du` can read however the
/// names inside it were produced. Blank lines are dropped rather than becoming
/// a pattern that matches nothing.
fn exclude_patterns(text: &[u8]) -> Vec<Vec<u8>> {
    text.split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

// ------------------------------------------------------------- path rules ---

/// A root operand as `du` displays it: a trailing run of slashes collapses to
/// exactly one.
///
/// Measured: `du -s t//` prints `t/`, not `t//` and not `t`. The single
/// surviving slash then matters twice over — `--exclude=t` does *not* prune a
/// root displayed as `t/`, while `--exclude=t/` does.
fn normalise_root(name: &[u8]) -> Vec<u8> {
    match name.iter().rposition(|&byte| byte != b'/') {
        None if name.is_empty() => Vec::new(),
        None => vec![b'/'],
        Some(last) => {
            let end = last.saturating_add(1);
            let mut out = name.get(..end).unwrap_or_default().to_vec();
            if end < name.len() {
                out.push(b'/');
            }
            out
        }
    }
}

/// A child's displayed name: the parent with *every* trailing slash removed,
/// one `/`, and the entry name.
///
/// Measured: `du -a t/` prints `t/k1` (one slash, not two) and `du -a ./t/`
/// prints `./t/k1`, so the parent's own spelling is preserved apart from its
/// trailing slashes. A root of `/` strips to nothing and rejoins as `/etc`.
fn join(parent: &[u8], child: &[u8]) -> Vec<u8> {
    let end = parent
        .iter()
        .rposition(|&byte| byte != b'/')
        .map_or(0, |last| last.saturating_add(1));
    let mut out = parent.get(..end).unwrap_or_default().to_vec();
    out.push(b'/');
    out.extend_from_slice(child);
    out
}

/// Split a `--files0-from` file into names.
///
/// A single trailing empty element is dropped, because a well-formed list ends
/// with a NUL rather than separating with one. An *interior* empty element is
/// kept, and becomes the `invalid zero-length file name` diagnostic — which is
/// the only way a caller ever learns that its `find -print0` produced one.
fn split_nul(text: &[u8]) -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = text.split(|&byte| byte == 0).map(<[u8]>::to_vec).collect();
    if names.last().is_some_and(Vec::is_empty) {
        names.pop();
    }
    names
}

// ------------------------------------------------------------------ model ---

/// Which symlinks are followed. Last spelling on the command line wins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Deref {
    /// `-P`, and the default: a symlink is counted as itself.
    #[default]
    Never,
    /// `-D`, `-H`, `--dereference-args`: only the operands are followed.
    Args,
    /// `-L`, `--dereference`: every symlink is followed.
    Always,
}

/// Everything the walk needs that the command line decided.
#[expect(
    clippy::struct_excessive_bools,
    reason = "these are `du`'s independent on/off switches, not a state \
              machine — `-a`, `-b`, `--inodes`, `-l`, `-c`, `-S`, `-x` and \
              `-0` combine freely and each is read at a different point in \
              the walk, so folding them into two-variant enums would name \
              eight new types and relate none of them"
)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Config {
    all: bool,
    apparent_size: bool,
    inodes: bool,
    count_links: bool,
    total: bool,
    separate_dirs: bool,
    one_file_system: bool,
    null: bool,
    deref: Deref,
    max_depth: u64,
    threshold: Option<i64>,
    format: Format,
    excludes: Vec<Vec<u8>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            all: false,
            apparent_size: false,
            inodes: false,
            count_links: false,
            total: false,
            separate_dirs: false,
            one_file_system: false,
            null: false,
            deref: Deref::Never,
            max_depth: u64::MAX,
            threshold: None,
            format: Format::plain(false),
            excludes: Vec::new(),
        }
    }
}

impl Config {
    fn excluded(&self, name: &[u8]) -> bool {
        self.excludes
            .iter()
            .any(|pattern| matches_unanchored(pattern, name))
    }

    /// `-t`: a positive threshold keeps the entries at least that large, a
    /// negative one keeps the entries no larger than its magnitude.
    ///
    /// The comparison is against *bytes* (or, under `--inodes`, against the
    /// inode count) — never against the block-scaled figure that is printed, so
    /// `-B M -t 1` does not silently mean `-t 1M`.
    fn passes_threshold(&self, units: u64) -> bool {
        let Some(threshold) = self.threshold else {
            return true;
        };
        let value = i64::try_from(units).unwrap_or(i64::MAX);
        if threshold < 0 {
            value <= threshold.saturating_neg()
        } else {
            value >= threshold
        }
    }

    /// The column text for a count of bytes, or of inodes under `--inodes`.
    ///
    /// `--inodes` renders with a `to_block_size` of one whatever `-B` said,
    /// which is why `du -B K --inodes` prints `3` and not `3K`: the block size
    /// scales a size and there is no size here to scale.
    fn render(&self, units: u64) -> String {
        let to = if self.inodes {
            1
        } else {
            self.format.block_size
        };
        human_readable(units, self.format.opts, 1, to)
    }
}

/// What the command line asked for, once it has been understood.
enum Request {
    Help,
    Version,
    Run(Config, Source),
}

/// Where the operands come from.
enum Source {
    Operands(Vec<Vec<u8>>),
    /// `--files0-from=F`; `-` means standard input.
    Files0From(Vec<u8>),
}

/// A command line that will not run, and everything to print about it.
///
/// This is a list rather than one message because `du`'s option loop does not
/// stop at the first complaint. `-d zz` prints its sentence on the spot and
/// sets a flag; the `Try 'du --help'` line arrives at the end, from the
/// `usage (EXIT_FAILURE)` that the flag triggers. So `du -d zz -B zz` prints
/// *two* sentences and *no* referral — the `-B` failure is fatal on the spot
/// and never reaches the end of the loop. Measured, verbatim:
///
/// ```text
/// $ du -d zz -B zz t
/// du: invalid maximum depth ‘zz’
/// du: invalid suffix in -B argument 'zz'
/// $ du -d zz t
/// du: invalid maximum depth ‘zz’
/// Try 'du --help' for more information.
/// ```
#[derive(Debug, PartialEq, Eq)]
struct Refusal {
    /// Complete stderr lines, prefixed where GNU prefixes them.
    lines: Vec<String>,
    /// Whether `Try 'du --help' for more information.` follows.
    referral: bool,
    status: i32,
}

impl Refusal {
    fn from_getopt(error: &getopt::Error) -> Self {
        Self {
            lines: vec![format!("du: {}", error.sentence)],
            referral: error.referral.is_some(),
            status: error.status,
        }
    }

    /// One sentence, a referral, and status 1 — the shape of a usage error the
    /// option loop raises itself rather than receiving from `getopt`.
    fn usage(sentence: &str) -> Self {
        Self {
            lines: vec![format!("du: {sentence}")],
            referral: true,
            status: 1,
        }
    }

    fn print(&self, err: &mut dyn Write) {
        for line in &self.lines {
            // A diagnostic that cannot be written has nowhere left to be
            // reported, so the failure is deliberately dropped here.
            let _ = writeln!(err, "{line}");
        }
        if self.referral {
            let _ = writeln!(err, "Try 'du --help' for more information.");
        }
    }
}

// ------------------------------------------------------------------- walk ---

/// The parts of a `stat` result `du` reads.
///
/// `nlink` is absent on purpose: the hard-link table is consulted for *every*
/// entry, not only for the ones the kernel says are linked more than once.
/// Measured, on a file with one link: `du -c f f` totals 4096 and `du -l -c f f`
/// totals 8192, so the second `f` was dropped by the `(dev, ino)` table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Meta {
    dev: u64,
    ino: u64,
    size: u64,
    blocks: u64,
    is_dir: bool,
    is_symlink: bool,
}

/// The filesystem, injected so the walk is testable on a host that has no
/// `st_blocks`.
///
/// Same reasoning as [`coreutils::canon`]'s `Fs`: the interesting cases here —
/// a directory that cannot be read, a symlink whose target is missing, two
/// names sharing an inode, a subtree on another device — are all awkward or
/// impossible to stage on the development host, and all trivial to state in a
/// table.
trait Tree {
    fn lstat(&self, path: &[u8]) -> io::Result<Meta>;
    fn stat(&self, path: &[u8]) -> io::Result<Meta>;
    fn read_dir(&self, path: &[u8]) -> io::Result<Vec<Vec<u8>>>;
}

/// The running state of one `du` invocation.
struct Walk<'a> {
    cfg: &'a Config,
    tree: &'a dyn Tree,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
    seen: HashSet<(u64, u64)>,
    grand: u64,
    failed: bool,
    /// How `--files0-from`'s file was spelled, when the roots came from one.
    ///
    /// It is here rather than consumed while the list was read because the
    /// empty names in that list are reported *in position* — GNU walks the
    /// token list one entry at a time and diagnoses an empty entry when it
    /// reaches it, so `printf 'f\0\0sub\0'` prints `f`'s row, then the
    /// complaint, then `sub`'s. Pre-scanning for empties puts every complaint
    /// before every row, which is a visibly different transcript.
    list: Option<Vec<u8>>,
}

impl Walk<'_> {
    fn diagnose(&mut self, line: &str) {
        // Nothing useful remains to be done if stderr itself is broken.
        let _ = writeln!(self.err, "{line}");
        self.failed = true;
    }

    /// Write one output row, ending it with NUL under `-0`.
    fn emit(&mut self, units: u64, name: &[u8]) {
        let mut line = self.cfg.render(units).into_bytes();
        line.push(b'\t');
        line.extend_from_slice(name);
        line.push(if self.cfg.null { 0 } else { b'\n' });
        if self.out.write_all(&line).is_err() {
            self.failed = true;
        }
    }

    /// `lstat` or `stat` the entry, wording the failure as `du` words it.
    ///
    /// The two shapes come from two `fts` info codes and differ in whether they
    /// carry an errno at all. `FTS_NS` — an `lstat` that failed — reports one;
    /// `FTS_SLNONE` — a dangling symlink met while following — carries errno 0,
    /// so the message simply stops:
    ///
    /// ```text
    /// $ du nosuch      ->  du: cannot access 'nosuch': No such file or directory
    /// $ du -L dangle   ->  du: cannot access 'dangle'
    /// ```
    fn describe(&mut self, path: &[u8], follow: bool) -> Option<Meta> {
        let result = if follow {
            self.tree.stat(path)
        } else {
            self.tree.lstat(path)
        };
        match result {
            Ok(meta) => Some(meta),
            Err(error) => {
                let dangling =
                    follow && matches!(self.tree.lstat(path), Ok(link) if link.is_symlink);
                let line = if dangling {
                    format!("du: cannot access {}", quoteaf(path))
                } else {
                    format!("du: cannot access {}: {}", quoteaf(path), strerror(&error))
                };
                self.diagnose(&line);
                None
            }
        }
    }

    /// Account for one entry and, if it is a directory, everything under it.
    ///
    /// Returns the entry's *full* size — what it contributes to its parent and
    /// to the grand total — together with whether it was a directory, which is
    /// the one thing the parent needs in order to honour `-S`. `None` means the
    /// entry was skipped outright: excluded, already counted under another
    /// name, on the far side of a `-x` boundary, or unreadable.
    ///
    /// The two accumulators are the `-S` rule. Measured on a two-level tree:
    ///
    /// ```text
    /// $ du -S -c -B1 t
    /// 12288   t/sub
    /// 12288   t
    /// 24576   total
    /// ```
    ///
    /// `t` displays 12288 — itself and its files, not `sub` — while still
    /// handing 24576 upward. A single accumulator would have to choose, and
    /// either the row or the total would be wrong.
    fn entry(&mut self, path: &[u8], level: u64, root_dev: Option<u64>) -> Option<(u64, bool)> {
        if self.cfg.excluded(path) {
            return None;
        }

        let follow = match self.cfg.deref {
            Deref::Always => true,
            Deref::Args => level == 0,
            Deref::Never => false,
        };
        let meta = self.describe(path, follow)?;

        let root_dev = root_dev.unwrap_or(meta.dev);
        if self.cfg.one_file_system && level > 0 && meta.dev != root_dev {
            return None;
        }
        if !self.cfg.count_links && !self.seen.insert((meta.dev, meta.ino)) {
            return None;
        }

        let own = if self.cfg.inodes {
            1
        } else if self.cfg.apparent_size {
            // A directory's apparent size is zero, not its `st_size`. Measured:
            // `du -B1 -s --apparent-size` of a directory holding one 102400-byte
            // file answers 102400, where counting the directory's own 4096 would
            // have answered 106496.
            if meta.is_dir { 0 } else { meta.size }
        } else {
            meta.blocks.saturating_mul(512)
        };

        let mut full = own;
        let mut displayed = own;
        if meta.is_dir {
            match self.tree.read_dir(path) {
                Ok(names) => {
                    for name in names {
                        let child = join(path, &name);
                        if let Some((size, child_is_dir)) =
                            self.entry(&child, level.saturating_add(1), Some(root_dev))
                        {
                            full = full.saturating_add(size);
                            if !(self.cfg.separate_dirs && child_is_dir) {
                                displayed = displayed.saturating_add(size);
                            }
                        }
                    }
                }
                Err(error) => {
                    // The directory still prints, at its own size, and still
                    // counts toward its parent — measured: an unreadable
                    // subdirectory contributes its 4096 and the exit status is
                    // 1.
                    let line = format!(
                        "du: cannot read directory {}: {}",
                        quoteaf(path),
                        strerror(&error)
                    );
                    self.diagnose(&line);
                }
            }
        }

        let deep_enough = level <= self.cfg.max_depth;
        let print = level == 0 || ((self.cfg.all || meta.is_dir) && deep_enough);
        if print && self.cfg.passes_threshold(displayed) {
            self.emit(displayed, path);
        }
        Some((full, meta.is_dir))
    }

    /// Walk every operand and, under `-c`, print the grand total.
    ///
    /// The total is exempt from `-t`: measured, `du -t 1000000 -c` prints a
    /// total line even when the threshold suppressed every row that made it up.
    fn run(&mut self, roots: &[Vec<u8>]) {
        for (index, root) in roots.iter().enumerate() {
            // An empty name is never looked up — it is refused, and the
            // sentence says where it came from. Measured on 9.4:
            //
            //     $ du ''                       du: invalid zero-length file name
            //     $ du --files0-from=z0         du: z0:2: invalid zero-length file name
            //
            // The bare form carries neither a label nor a position because
            // there is no list to point into.
            if root.is_empty() {
                let line = match self.list.clone() {
                    Some(label) => format!(
                        "du: {}:{}: invalid zero-length file name",
                        quotef(&label),
                        index.saturating_add(1)
                    ),
                    None => "du: invalid zero-length file name".to_string(),
                };
                self.diagnose(&line);
                continue;
            }
            let name = normalise_root(root);
            if let Some((size, _)) = self.entry(&name, 0, None) {
                self.grand = self.grand.saturating_add(size);
            }
        }
        if self.cfg.total {
            let grand = self.grand;
            self.emit(grand, b"total");
        }
    }
}

// ---------------------------------------------------------------- parsing ---

/// gnulib's `xstrtol`-with-base-0 as `du -d` uses it.
///
/// Three quirks, each measured rather than assumed:
///
/// * The base is **0**, so `-d 0x10` is sixteen levels and `-d 010` is eight.
/// * There are **no suffixes**, so `-d 1K` is `invalid maximum depth ‘1K’`
///   even though `-t 1K` is a kibibyte.
/// * The type is **signed**, and the result is then used as an unsigned depth,
///   so `-d -1` means *unlimited* while `-d 18446744073709551615` — the same
///   bit pattern, written positively — is rejected. The acceptance boundary was
///   found by bisection: `9223372036854775807` and `-9223372036854775808` are
///   accepted, `9223372036854775808` and `-18446744073709551615` are not.
fn parse_max_depth(arg: &[u8]) -> Option<u64> {
    let (value, status) = xstrtoimax_base(arg, 0, Some(b""));
    if status == Status::Ok {
        #[expect(
            clippy::cast_sign_loss,
            reason = "the two's-complement reading is the behaviour: -1 is an unlimited depth"
        )]
        Some(value as u64)
    } else {
        None
    }
}

/// The environment `du` reads, gathered up so parsing stays a pure function.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Environment {
    du_block_size: Option<Vec<u8>>,
    block_size: Option<Vec<u8>>,
    blocksize: Option<Vec<u8>>,
    posixly_correct: bool,
}

/// The option loop.
///
/// `read_file` is how `-X` reaches the disk; it is a parameter so that the
/// ordering of an `-X` failure against a `-d` failure can be tested without
/// staging files. `-X -` is handed the empty name, which the caller maps to
/// standard input.
#[expect(
    clippy::too_many_lines,
    reason = "one option per arm; splitting the match would hide the order the arms must keep"
)]
fn parse_args(
    argv: &[OsString],
    env: &Environment,
    read_file: &dyn Fn(&[u8]) -> io::Result<Vec<u8>>,
) -> Result<Request, Refusal> {
    let mut cfg = Config {
        format: environment_format(
            env.du_block_size.as_deref(),
            env.block_size.as_deref(),
            env.blocksize.as_deref(),
            env.posixly_correct,
        ),
        ..Config::default()
    };
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut files0_from: Option<Vec<u8>> = None;
    let mut summarize = false;
    let mut max_depth_specified = false;
    // Sentences already emitted by an option that did not stop the loop. The
    // referral they earn is appended once, at the end.
    let mut deferred: Vec<String> = Vec::new();

    for item in DU.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let item = item.map_err(|error| Refusal::from_getopt(&error))?;
        // The two spellings of an option that takes a value must name
        // themselves in their own diagnostics: `-B zz` says `-B`, and
        // `--block-size=zz` says `--block-size`.
        let (key, value, spelling): (u8, Option<OsString>, String) = match item {
            Opt::Operand(word) => {
                operands.push(os_bytes(word).into_owned());
                continue;
            }
            Opt::Short(flag, value) => {
                let spelling = format!("-{}", char::from(flag));
                (flag, value, spelling)
            }
            Opt::Long(name, value) => {
                let spelling = format!("--{name}");
                let key = long_key(name);
                (key, value, spelling)
            }
        };
        let raw = value.as_deref().map(|v| os_bytes(v).into_owned());
        let arg = raw.as_deref().unwrap_or_default();

        match key {
            b'\0' => return Ok(Request::Help),
            b'\x01' => return Ok(Request::Version),
            b'0' => cfg.null = true,
            b'a' => cfg.all = true,
            b'A' => cfg.apparent_size = true,
            b'b' => {
                cfg.apparent_size = true;
                cfg.format = Format {
                    opts: Opts::NONE,
                    block_size: 1,
                };
            }
            b'c' => cfg.total = true,
            b'd' => match parse_max_depth(arg) {
                Some(depth) => {
                    max_depth_specified = true;
                    cfg.max_depth = depth;
                }
                None => deferred.push(format!("du: invalid maximum depth {}", quote(arg))),
            },
            b'h' => {
                cfg.format = Format {
                    opts: Opts::AUTOSCALE | Opts::SI | Opts::BASE_1024,
                    block_size: 1,
                };
            }
            b'i' => cfg.inodes = true,
            b'k' => {
                cfg.format = Format {
                    opts: Opts::NONE,
                    block_size: 1024,
                };
            }
            b'l' => cfg.count_links = true,
            b'm' => {
                cfg.format = Format {
                    opts: Opts::NONE,
                    block_size: 1024 * 1024,
                };
            }
            b's' => summarize = true,
            b'x' => cfg.one_file_system = true,
            b'B' => {
                let (format, status) = human_options(arg, env.posixly_correct);
                if let Some(sentence) = strtol_fatal(status, &spelling, arg) {
                    // Fatal on the spot, and with no referral: this one is
                    // `error (EXIT_FAILURE, …)` upstream, so anything the loop
                    // had already printed stands and nothing follows it.
                    let mut lines = deferred;
                    lines.push(format!("du: {sentence}"));
                    return Err(Refusal {
                        lines,
                        referral: false,
                        status: 1,
                    });
                }
                cfg.format = format;
            }
            // `-H` and `-D` are one case upstream, and the comment there says
            // why the letter is not the obvious one: before 2008-12 `-H` meant
            // `--si`. Measured on 9.4, `du -a -H t` and `du -a -D t` print the
            // same five rows, so the alias is complete rather than approximate.
            b'D' | b'H' => cfg.deref = Deref::Args,
            b'L' => cfg.deref = Deref::Always,
            b'P' => cfg.deref = Deref::Never,
            b'S' => cfg.separate_dirs = true,
            b't' => {
                let (threshold, status) = xstrtoimax_base(arg, 0, Some(THRESHOLD_SUFFIXES));
                if let Some(sentence) = strtol_fatal(status, &spelling, arg) {
                    let mut lines = deferred;
                    lines.push(format!("du: {sentence}"));
                    return Err(Refusal {
                        lines,
                        referral: false,
                        status: 1,
                    });
                }
                if threshold == 0 && arg.first() == Some(&b'-') {
                    // Upstream's literal, `--threshold` and all, whichever
                    // spelling the user typed. Measured: `du -t -0` answers
                    // `du: invalid --threshold argument '-0'`.
                    let mut lines = deferred;
                    lines.push("du: invalid --threshold argument '-0'".to_string());
                    return Err(Refusal {
                        lines,
                        referral: false,
                        status: 1,
                    });
                }
                cfg.threshold = Some(threshold);
            }
            b'X' => match read_file(arg) {
                Ok(text) => cfg.excludes.extend(exclude_patterns(&text)),
                Err(error) => {
                    deferred.push(format!("du: {}: {}", quotef(arg), strerror(&error)));
                }
            },
            b'E' => cfg.excludes.push(arg.to_vec()),
            b'F' => files0_from = Some(arg.to_vec()),
            b'I' => {
                cfg.format = Format {
                    opts: Opts::AUTOSCALE | Opts::SI,
                    block_size: 1,
                };
            }
            // `T` is `--time` and `--time-style`, which this `du` recognises
            // precisely so that it can refuse them: they are in the long table
            // so that `--t` is reported as ambiguous and `--time-st` resolves,
            // rather than either being an unknown option. Any other key
            // arriving here is a short letter listed in `SHORT_OPTIONS` with no
            // arm above, which is a bug in this file — the same sentence is the
            // honest answer for it, since the option is indeed not implemented.
            _ => return Err(unimplemented(&spelling)),
        }
    }

    if !deferred.is_empty() {
        return Err(Refusal {
            lines: deferred,
            referral: true,
            status: 1,
        });
    }

    if let Some(from) = files0_from {
        if let Some(extra) = operands.first() {
            return Err(Refusal {
                lines: vec![
                    format!("du: extra operand {}", quote(extra)),
                    "file operands cannot be combined with --files0-from".to_string(),
                ],
                referral: true,
                status: 1,
            });
        }
        finish(&mut cfg, summarize, max_depth_specified)?;
        return Ok(Request::Run(cfg, Source::Files0From(from)));
    }

    let warning = finish(&mut cfg, summarize, max_depth_specified)?;
    if operands.is_empty() {
        operands.push(b".".to_vec());
    }
    if let Some(text) = warning {
        // A warning, not a refusal: `du -s -d0` says its piece and then runs.
        // Threaded out through `Request` would need a third variant for one
        // string, so it is printed here by the only caller that can.
        eprintln!("du: {text}");
    }
    Ok(Request::Run(cfg, Source::Operands(operands)))
}

/// The consistency checks `du` makes once the whole command line is read, and
/// the `-s`-to-`--max-depth=0` translation they guard.
///
/// Measured, in this order:
///
/// ```text
/// $ du -s -a t   -> du: cannot both summarize and show all entries      (fatal)
/// $ du -s -d1 t  -> du: warning: summarizing conflicts with --max-depth=1 (fatal)
/// $ du -s -d0 t  -> du: warning: summarizing is the same as using --max-depth=0
///                   … and then the listing, exit 0
/// ```
fn finish(
    cfg: &mut Config,
    summarize: bool,
    max_depth_specified: bool,
) -> Result<Option<String>, Refusal> {
    if cfg.all && summarize {
        return Err(Refusal::usage("cannot both summarize and show all entries"));
    }
    let mut warning = None;
    if summarize && max_depth_specified {
        if cfg.max_depth == 0 {
            warning = Some("warning: summarizing is the same as using --max-depth=0".to_string());
        } else {
            return Err(Refusal::usage(&format!(
                "warning: summarizing conflicts with --max-depth={}",
                cfg.max_depth
            )));
        }
    }
    if summarize {
        cfg.max_depth = 0;
    }
    Ok(warning)
}

/// The single byte the option loop switches on for a long option.
///
/// Long options that have a short spelling map onto it; the rest take a byte
/// that no short option uses, which keeps the loop one `match` instead of two.
fn long_key(name: &str) -> u8 {
    match name {
        "all" => b'a',
        "apparent-size" => b'A',
        "block-size" => b'B',
        "bytes" => b'b',
        "count-links" => b'l',
        "dereference" => b'L',
        "dereference-args" => b'D',
        "exclude" => b'E',
        "exclude-from" => b'X',
        "files0-from" => b'F',
        "human-readable" => b'h',
        "inodes" => b'i',
        "si" => b'I',
        "max-depth" => b'd',
        "null" => b'0',
        "no-dereference" => b'P',
        "one-file-system" => b'x',
        "separate-dirs" => b'S',
        "summarize" => b's',
        "total" => b'c',
        "threshold" => b't',
        "help" => b'\0',
        "version" => b'\x01',
        // `--time` and `--time-style`, which are in the table for the sake of
        // `--t`'s ambiguity and rejected by name when actually used.
        _ => b'T',
    }
}

fn unimplemented(spelling: &str) -> Refusal {
    Refusal::from_getopt(
        &DU.usage_referring(format!("option '{spelling}' is not implemented by this du")),
    )
}

fn help_text() -> String {
    "\
Usage: du [OPTION]... [FILE]...
  or:  du [OPTION]... --files0-from=F
Summarize device usage of the set of FILEs, recursively for directories.

Mandatory arguments to long options are mandatory for short options too.
  -0, --null            end each output line with NUL, not newline
  -a, --all             write counts for all files, not just directories
      --apparent-size   print apparent sizes rather than device usage
  -B, --block-size=SIZE  scale sizes by SIZE before printing them; e.g.,
                           '-BM' prints sizes in units of 1,048,576 bytes;
                           see SIZE format below
  -b, --bytes           equivalent to '--apparent-size --block-size=1'
  -c, --total           produce a grand total
  -D, --dereference-args  dereference only symlinks that are listed on the
                          command line
  -d, --max-depth=N     print the total for a directory (or file, with --all)
                          only if it is N or fewer levels below the command
                          line argument;  --max-depth=0 is the same as
                          --summarize
      --files0-from=F   summarize device usage of the
                          NUL-terminated file names specified in file F;
                          if F is -, then read names from standard input
  -H                    equivalent to --dereference-args (-D)
  -h, --human-readable  print sizes in human readable format (e.g., 1K 234M 2G)
      --inodes          list inode usage information instead of block usage
  -k                    like --block-size=1K
  -L, --dereference     dereference all symbolic links
  -l, --count-links     count sizes many times if hard linked
  -m                    like --block-size=1M
  -P, --no-dereference  don't follow any symbolic links (this is the default)
  -S, --separate-dirs   for directories do not include size of subdirectories
      --si              like -h, but use powers of 1000 not 1024
  -s, --summarize       display only a total for each argument
  -t, --threshold=SIZE  exclude entries smaller than SIZE if positive,
                          or entries greater than SIZE if negative
  -X, --exclude-from=FILE  exclude files that match any pattern in FILE
      --exclude=PATTERN    exclude files that match PATTERN
  -x, --one-file-system    skip directories on different file systems
      --help        display this help and exit
      --version     output version information and exit

Display values are in units of the first available SIZE from --block-size,
and the DU_BLOCK_SIZE, BLOCK_SIZE and BLOCKSIZE environment variables.
Otherwise, units default to 1024 bytes (or 512 if POSIXLY_CORRECT is set).

The SIZE argument is an integer and optional unit (example: 10K is 10*1024).
Units are K,M,G,T,P,E,Z,Y (powers of 1024) or KB,MB,... (powers of 1000).
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.
"
    .to_string()
}

// ------------------------------------------------------------------- main ---

#[cfg(not(unix))]
fn main() -> ExitCode {
    eprintln!("du: unix-only utility; not supported on this platform");
    ExitCode::from(1)
}

/// The real filesystem.
#[cfg(unix)]
struct RealTree;

#[cfg(unix)]
impl Tree for RealTree {
    fn lstat(&self, path: &[u8]) -> io::Result<Meta> {
        meta_of(&std::fs::symlink_metadata(os_from_bytes(path))?)
    }

    fn stat(&self, path: &[u8]) -> io::Result<Meta> {
        meta_of(&std::fs::metadata(os_from_bytes(path))?)
    }

    fn read_dir(&self, path: &[u8]) -> io::Result<Vec<Vec<u8>>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(os_from_bytes(path))? {
            names.push(os_bytes(&entry?.file_name()).into_owned());
        }
        Ok(names)
    }
}

#[cfg(unix)]
fn meta_of(meta: &std::fs::Metadata) -> io::Result<Meta> {
    use std::os::unix::fs::MetadataExt;
    Ok(Meta {
        dev: meta.dev(),
        ino: meta.ino(),
        size: meta.size(),
        blocks: meta.blocks(),
        is_dir: meta.is_dir(),
        is_symlink: meta.file_type().is_symlink(),
    })
}

/// Read a whole file, or standard input when the name is exactly `-`.
///
/// An *empty* name is a file name, not a second spelling of standard input —
/// measured, `du -X '' t` is `du: '': No such file or directory` and
/// `du --files0-from=` is `du: cannot open '' for reading: No such file or
/// directory`, while both accept `-` and read stdin.
#[cfg(unix)]
fn slurp(name: &[u8]) -> io::Result<Vec<u8>> {
    use std::io::Read;
    if name == b"-" {
        let mut text = Vec::new();
        io::stdin().read_to_end(&mut text)?;
        return Ok(text);
    }
    std::fs::read(os_from_bytes(name))
}

#[cfg(unix)]
fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let env = Environment {
        du_block_size: std::env::var_os("DU_BLOCK_SIZE").map(|v| os_bytes(&v).into_owned()),
        block_size: std::env::var_os("BLOCK_SIZE").map(|v| os_bytes(&v).into_owned()),
        blocksize: std::env::var_os("BLOCKSIZE").map(|v| os_bytes(&v).into_owned()),
        posixly_correct: std::env::var_os("POSIXLY_CORRECT").is_some(),
    };

    let request = match parse_args(&argv, &env, &slurp) {
        Ok(request) => request,
        Err(refusal) => {
            refusal.print(&mut io::stderr());
            return ExitCode::from(u8::try_from(refusal.status).unwrap_or(1));
        }
    };

    let (cfg, source) = match request {
        Request::Help => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Request::Version => {
            println!("du (SlateOS coreutils) 0.1.0");
            return ExitCode::SUCCESS;
        }
        Request::Run(cfg, source) => (cfg, source),
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut err = io::stderr();

    let (roots, list) = match source {
        Source::Operands(names) => (names, None),
        Source::Files0From(from) => match read_operand_list(&from, &mut err) {
            // The label the diagnostics carry is the spelling as given, `-`
            // included: `du --files0-from=-` reports `du: -:2: …`.
            Ok(names) => (names, Some(from)),
            Err(status) => return ExitCode::from(status),
        },
    };

    let tree = RealTree;
    let mut walk = Walk {
        cfg: &cfg,
        tree: &tree,
        out: &mut out,
        err: &mut err,
        seen: HashSet::new(),
        grand: 0,
        failed: false,
        list,
    };
    walk.run(&roots);
    let failed = walk.failed;
    if out.flush().is_err() {
        return ExitCode::from(1);
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Read `--files0-from`'s list into the operand vector, empty names included.
///
/// The empty names stay in the list on purpose: [`Walk::run`] reports each one
/// where it stands, because that is where GNU reports it. See the `list` field
/// of [`Walk`].
///
/// Failing to *read* the list is fatal and is worded differently from failing
/// to *open* it, which is not a distinction one would invent — it exists
/// because the two are separate calls in gnulib and a directory passes the
/// first and fails the second. Measured, on 9.4:
///
/// ```text
/// $ du --files0-from='no such'      du: cannot open 'no such' for reading: …
/// $ du --files0-from=secret         du: cannot open 'secret' for reading: …
/// $ du --files0-from=plain          du: plain: read error: Is a directory
/// $ du --files0-from='a b'          du: 'a b': read error: Is a directory
/// ```
///
/// The last pair is why the two sentences also quote differently: the open
/// failure is `quoteaf` (always quoted), the read failure `quotef` (quoted
/// only where the name needs it). An empty spelling is a file named `""` and
/// not a second way to write `-`; see [`slurp`].
#[cfg(unix)]
fn read_operand_list(name: &[u8], err: &mut dyn Write) -> Result<Vec<Vec<u8>>, u8> {
    use std::io::Read;

    let text = if name == b"-" {
        let mut text = Vec::new();
        match io::stdin().read_to_end(&mut text) {
            Ok(_) => text,
            Err(error) => {
                let line = format!("du: {}: read error: {}", quotef(name), strerror(&error));
                let _ = writeln!(err, "{line}");
                return Err(1);
            }
        }
    } else {
        match std::fs::File::open(os_from_bytes(name)) {
            Ok(mut file) => {
                let mut text = Vec::new();
                match file.read_to_end(&mut text) {
                    Ok(_) => text,
                    Err(error) => {
                        let line =
                            format!("du: {}: read error: {}", quotef(name), strerror(&error));
                        let _ = writeln!(err, "{line}");
                        return Err(1);
                    }
                }
            }
            Err(error) => {
                // `error (EXIT_FAILURE, …)`, so no referral follows.
                let _ = writeln!(
                    err,
                    "du: cannot open {} for reading: {}",
                    quoteaf(name),
                    strerror(&error)
                );
                return Err(1);
            }
        }
    };

    Ok(split_nul(&text))
}

// ------------------------------------------------------------------ tests ---

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that cannot build its own fixture should fail loudly"
)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // ------------------------------------------------------ block sizes ---

    /// Every expectation here is a `du -s -B <spec>` run against a tree of
    /// 3153920 bytes / 3080 KiB, recorded rather than reasoned.
    #[test]
    fn block_size_specs_decide_the_divisor_and_the_suffix() {
        let render = |spec: &[u8]| {
            let (format, status) = human_options(spec, false);
            assert_eq!(status, Status::Ok, "spec {spec:?} was refused");
            let cfg = Config {
                format,
                ..Config::default()
            };
            cfg.render(3_153_920)
        };
        assert_eq!(render(b"1"), "3153920");
        assert_eq!(render(b"512"), "6160");
        assert_eq!(render(b"1024"), "3080");
        assert_eq!(render(b"1K"), "3080");
        assert_eq!(render(b"0x400"), "3080");
        assert_eq!(render(b"1KiB"), "3080");
        assert_eq!(render(b"1KB"), "3154");
        assert_eq!(render(b"1M"), "4");
        // A spec with no digit turns the unit on as well as dividing by it.
        assert_eq!(render(b"K"), "3080K");
        assert_eq!(render(b"M"), "4M");
        assert_eq!(render(b"G"), "1G");
        assert_eq!(render(b"KB"), "3154kB");
        assert_eq!(render(b"KiB"), "3080KiB");
        assert_eq!(render(b"MiB"), "4MiB");
        // The words, and their unambiguous prefixes.
        assert_eq!(render(b"human-readable"), "3.1M");
        assert_eq!(render(b"human"), "3.1M");
        assert_eq!(render(b"h"), "3.1M");
        assert_eq!(render(b"si"), "3.2M");
        assert_eq!(render(b"s"), "3.2M");
        // A leading apostrophe asks for the locale's thousands separator and is
        // not part of the number. In the C locale there is no separator, so the
        // only observable effect is that the apostrophe is *consumed* rather
        // than being an invalid suffix — measured, `du -B "'1" -s t` prints
        // `3088384` with no punctuation and `du -B "'1024" -s t` prints `3016`.
        assert_eq!(render(b"'1048576"), "4");
        assert_eq!(render(b"'1"), "3153920");
    }

    /// The three refusal shapes, and which spec earns which.
    ///
    /// The split is not intuitive and is entirely a property of gnulib's suffix
    /// list: `R` and `Q` are absent from it, so they are *invalid arguments*,
    /// while `Z` and `Y` are present and merely overflow.
    #[test]
    fn a_bad_block_size_picks_one_of_three_sentences() {
        let refuse = |spec: &[u8]| {
            let (_, status) = human_options(spec, false);
            strtol_fatal(status, "-B", spec).unwrap_or_else(|| panic!("{spec:?} was accepted"))
        };
        assert_eq!(refuse(b"Si"), "invalid -B argument 'Si'");
        assert_eq!(refuse(b"SI"), "invalid -B argument 'SI'");
        assert_eq!(refuse(b"HUMAN"), "invalid -B argument 'HUMAN'");
        assert_eq!(refuse(b""), "invalid -B argument ''");
        assert_eq!(refuse(b"B"), "invalid -B argument 'B'");
        assert_eq!(refuse(b"iB"), "invalid -B argument 'iB'");
        assert_eq!(refuse(b"R"), "invalid -B argument 'R'");
        assert_eq!(refuse(b"Q"), "invalid -B argument 'Q'");
        assert_eq!(refuse(b"-1"), "invalid -B argument '-1'");
        // A block size of zero parses and is then rejected by the *other* rule,
        // which is why it gets the `invalid argument` sentence and not the
        // suffix one.
        assert_eq!(refuse(b"0"), "invalid -B argument '0'");
        // GNU prints `'1'024'` — the format string's marks are literal and the
        // argument is interpolated raw. [`strtol_fatal`] escapes the inner mark
        // on purpose, so that an argument cannot forge the end of the quoted
        // run; see its doc comment. This is the one recorded difference.
        assert_eq!(refuse(b"1'024"), "invalid suffix in -B argument '1\\'024'");
        assert_eq!(refuse(b"1E100"), "invalid suffix in -B argument '1E100'");
        assert_eq!(refuse(b"1Q"), "invalid suffix in -B argument '1Q'");
        assert_eq!(refuse(b"ki"), "invalid suffix in -B argument 'ki'");
        assert_eq!(refuse(b"Ki"), "invalid suffix in -B argument 'Ki'");
        assert_eq!(refuse(b"1i"), "invalid suffix in -B argument '1i'");
        assert_eq!(refuse(b"1B"), "invalid suffix in -B argument '1B'");
        assert_eq!(refuse(b"1e3"), "invalid suffix in -B argument '1e3'");
        assert_eq!(refuse(b"Z"), "-B argument 'Z' too large");
        assert_eq!(refuse(b"Y"), "-B argument 'Y' too large");
    }

    /// A `-B` that failed still repairs the block size, because the environment
    /// caller keeps the repair and drops the complaint.
    #[test]
    fn the_environment_keeps_what_the_option_would_have_died_on() {
        let format =
            |spec: &[u8]| environment_format(Some(spec), Some(b"512"), None, false).block_size;
        // `zz`: the first `z` is in the suffix list but has no multiplier, so
        // the value is the bare-suffix 1 and the status is a suffix error the
        // environment path ignores. Measured: `DU_BLOCK_SIZE=zz du -s` prints
        // bytes.
        assert_eq!(format(b"zz"), 1);
        // Zero is repaired to the default, not left to divide by.
        assert_eq!(format(b"0"), 1024);
        // An empty variable is *set*, so it is used, fails and is repaired —
        // `BLOCK_SIZE` never gets a look in.
        assert_eq!(format(b""), 1024);
        // Only a genuinely unset variable falls through.
        assert_eq!(
            environment_format(None, Some(b"512"), Some(b"1"), false).block_size,
            512
        );
        assert_eq!(
            environment_format(None, None, Some(b"1"), false).block_size,
            1
        );
        assert_eq!(environment_format(None, None, None, false).block_size, 1024);
        assert_eq!(environment_format(None, None, None, true).block_size, 512);
    }

    // ------------------------------------------------------------ paths ---

    #[test]
    fn a_roots_trailing_slashes_collapse_to_one() {
        assert_eq!(normalise_root(b"t"), b"t");
        assert_eq!(normalise_root(b"t/"), b"t/");
        assert_eq!(normalise_root(b"t//"), b"t/");
        assert_eq!(normalise_root(b"t////"), b"t/");
        assert_eq!(normalise_root(b"/"), b"/");
        assert_eq!(normalise_root(b"///"), b"/");
        assert_eq!(normalise_root(b""), b"");
        assert_eq!(normalise_root(b"./t/"), b"./t/");
    }

    #[test]
    fn a_child_hangs_off_the_parent_with_exactly_one_slash() {
        assert_eq!(join(b"t", b"k1"), b"t/k1");
        assert_eq!(join(b"t/", b"k1"), b"t/k1");
        assert_eq!(join(b"./t/", b"k1"), b"./t/k1");
        assert_eq!(join(b"/", b"etc"), b"/etc");
        // A name is bytes, not text: a child that is not UTF-8 joins unchanged.
        assert_eq!(join(b"t", b"\xff\xfe"), b"t/\xff\xfe");
    }

    // --------------------------------------------------------- excludes ---

    /// The exclude matcher is unanchored and its `fnmatch` flags are empty.
    ///
    /// Both halves are measured. `ex*bb` prunes `ex/aa/bb` only because `*`
    /// crosses `/` (no `FNM_PATHNAME`), and `ex/*` prunes `ex/.hid` only
    /// because a leading dot is not special (no `FNM_PERIOD`).
    #[test]
    fn an_exclude_pattern_is_tried_at_every_slash() {
        assert!(matches_unanchored(b"bb", b"ex/aa/bb"));
        assert!(matches_unanchored(b"aa/bb", b"ex/aa/bb"));
        assert!(matches_unanchored(b"ex/aa/bb", b"ex/aa/bb"));
        assert!(!matches_unanchored(b"aa", b"ex/aa/bb"));
        assert!(matches_unanchored(b"ex*bb", b"ex/aa/bb"));
        assert!(matches_unanchored(b"ex/*", b"ex/.hid"));
        assert!(matches_unanchored(b"*\xff*", b"\xff\xfe"));
        // A run of slashes offers only the last of its positions, so the tail
        // never begins with a slash.
        assert!(matches_unanchored(b"b", b"a//b"));
        assert!(!matches_unanchored(b"/b", b"a//b"));
        // A root displayed with a trailing slash is a different name.
        assert!(!matches_unanchored(b"t", b"t/"));
        assert!(matches_unanchored(b"t/", b"t/"));
    }

    #[test]
    fn an_exclude_file_is_one_pattern_per_line_and_drops_blanks() {
        assert_eq!(
            exclude_patterns(b"a\n\nb*\n"),
            vec![b"a".to_vec(), b"b*".to_vec()]
        );
        // The separator is a newline whatever the names contain, so a NUL is
        // part of the pattern rather than a separator.
        assert_eq!(exclude_patterns(b"a\0b\n"), vec![b"a\0b".to_vec()]);
    }

    // -------------------------------------------------------- max depth ---

    #[test]
    fn max_depth_reads_a_signed_number_in_base_zero() {
        assert_eq!(parse_max_depth(b"0"), Some(0));
        assert_eq!(parse_max_depth(b"1"), Some(1));
        assert_eq!(parse_max_depth(b"+1"), Some(1));
        assert_eq!(parse_max_depth(b" 2"), Some(2));
        assert_eq!(parse_max_depth(b"0x10"), Some(16));
        assert_eq!(parse_max_depth(b"010"), Some(8));
        // A negative depth wraps, which is how `-d -1` means unlimited.
        assert_eq!(parse_max_depth(b"-1"), Some(u64::MAX));
        assert_eq!(parse_max_depth(b"-0"), Some(0));
        // The boundary, bisected against GNU du 9.4.
        assert_eq!(
            parse_max_depth(b"9223372036854775807"),
            Some(9_223_372_036_854_775_807)
        );
        assert_eq!(parse_max_depth(b"9223372036854775808"), None);
        assert_eq!(parse_max_depth(b"18446744073709551615"), None);
        assert_eq!(parse_max_depth(b"-18446744073709551615"), None);
        assert_eq!(parse_max_depth(b"0x8000000000000000"), None);
        // No suffixes at all, unlike `-t`.
        assert_eq!(parse_max_depth(b"1K"), None);
        assert_eq!(parse_max_depth(b"1x"), None);
        assert_eq!(parse_max_depth(b"1.5"), None);
        assert_eq!(parse_max_depth(b""), None);
        assert_eq!(parse_max_depth(b"zz"), None);
    }

    // -------------------------------------------------------- threshold ---

    #[test]
    fn a_threshold_keeps_the_large_or_the_small_by_its_sign() {
        let with = |threshold: i64| Config {
            threshold: Some(threshold),
            ..Config::default()
        };
        // Measured against a 106496-byte tree.
        assert!(with(106_496).passes_threshold(106_496));
        assert!(!with(106_497).passes_threshold(106_496));
        assert!(with(-106_496).passes_threshold(106_496));
        assert!(with(-106_497).passes_threshold(106_496));
        assert!(!with(-106_495).passes_threshold(106_496));
        assert!(Config::default().passes_threshold(0));
    }

    // ------------------------------------------------------------- walk ---

    /// A filesystem written down rather than created.
    struct FakeTree {
        /// name -> (metadata, children if a directory), or an error number.
        nodes: BTreeMap<Vec<u8>, Node>,
    }

    enum Node {
        File(Meta),
        Dir(Meta, Vec<Vec<u8>>),
        /// A directory whose contents cannot be read.
        Unreadable(Meta),
        /// A symlink, with the name it points at (empty means dangling).
        Link(Meta, Vec<u8>),
    }

    impl FakeTree {
        fn look(&self, path: &[u8]) -> io::Result<&Node> {
            // A trailing slash names the same file — the kernel resolves `t/`
            // and `t` to one inode, and `du` is handed the name with the slash
            // still on it, so a map keyed by the bare name has to do the same.
            // Not cosmetic: without it every `du t/` test looks up a name that
            // is not in the fixture and gets `ENOENT` instead of the tree.
            let mut key = path;
            while key.len() > 1 && key.last() == Some(&b'/') {
                key = key.get(..key.len().saturating_sub(1)).unwrap_or_default();
            }
            self.nodes
                .get(key)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No such file or directory"))
        }
    }

    impl Tree for FakeTree {
        fn lstat(&self, path: &[u8]) -> io::Result<Meta> {
            Ok(match self.look(path)? {
                Node::File(meta)
                | Node::Unreadable(meta)
                | Node::Link(meta, _)
                | Node::Dir(meta, _) => *meta,
            })
        }

        fn stat(&self, path: &[u8]) -> io::Result<Meta> {
            match self.look(path)? {
                Node::Link(_, target) => {
                    if target.is_empty() {
                        Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "No such file or directory",
                        ))
                    } else {
                        self.stat(target)
                    }
                }
                Node::File(meta) | Node::Unreadable(meta) | Node::Dir(meta, _) => Ok(*meta),
            }
        }

        fn read_dir(&self, path: &[u8]) -> io::Result<Vec<Vec<u8>>> {
            match self.look(path)? {
                Node::Dir(_, names) => Ok(names.clone()),
                Node::Unreadable(_) => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Permission denied",
                )),
                _ => Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "Not a directory",
                )),
            }
        }
    }

    fn file(ino: u64, blocks: u64, size: u64) -> Meta {
        Meta {
            dev: 1,
            ino,
            size,
            blocks,
            is_dir: false,
            is_symlink: false,
        }
    }

    fn dir(ino: u64, blocks: u64) -> Meta {
        Meta {
            dev: 1,
            ino,
            size: 4096,
            blocks,
            is_dir: true,
            is_symlink: false,
        }
    }

    /// `t` holds a 4 KiB file and `t/sub`, which holds another. Every directory
    /// costs 8 blocks (4 KiB) and every file 8 blocks.
    fn two_levels() -> FakeTree {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            b"t".to_vec(),
            Node::Dir(dir(1, 8), vec![b"f".to_vec(), b"sub".to_vec()]),
        );
        nodes.insert(b"t/f".to_vec(), Node::File(file(2, 8, 4096)));
        nodes.insert(b"t/sub".to_vec(), Node::Dir(dir(3, 8), vec![b"g".to_vec()]));
        nodes.insert(b"t/sub/g".to_vec(), Node::File(file(4, 8, 4096)));
        FakeTree { nodes }
    }

    /// `u/f`, `u/a/g`, `u/a/b/h` — a 4 KiB file at each of three levels, with
    /// the directory order GNU's `readdir` happened to give when this was
    /// measured, so the row order can be compared against the recording.
    fn three_levels() -> FakeTree {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            b"u".to_vec(),
            Node::Dir(dir(10, 8), vec![b"a".to_vec(), b"f".to_vec()]),
        );
        nodes.insert(
            b"u/a".to_vec(),
            Node::Dir(dir(11, 8), vec![b"g".to_vec(), b"b".to_vec()]),
        );
        nodes.insert(b"u/a/g".to_vec(), Node::File(file(12, 8, 4096)));
        nodes.insert(
            b"u/a/b".to_vec(),
            Node::Dir(dir(13, 8), vec![b"h".to_vec()]),
        );
        nodes.insert(b"u/a/b/h".to_vec(), Node::File(file(14, 8, 4096)));
        nodes.insert(b"u/f".to_vec(), Node::File(file(15, 8, 4096)));
        FakeTree { nodes }
    }

    /// Run a walk and return `(stdout, stderr, failed)`.
    fn walk(cfg: &Config, tree: &dyn Tree, roots: &[&[u8]]) -> (String, String, bool) {
        walk_list(cfg, tree, roots, None)
    }

    /// [`walk`], with the roots declared to have come from a `--files0-from`
    /// file of the given name — which is the only thing that makes an empty
    /// root a diagnostic rather than a lookup of `""`.
    fn walk_list(
        cfg: &Config,
        tree: &dyn Tree,
        roots: &[&[u8]],
        list: Option<&[u8]>,
    ) -> (String, String, bool) {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let names: Vec<Vec<u8>> = roots.iter().map(|r| r.to_vec()).collect();
        let failed = {
            let mut walk = Walk {
                cfg,
                tree,
                out: &mut out,
                err: &mut err,
                seen: HashSet::new(),
                grand: 0,
                failed: false,
                list: list.map(<[u8]>::to_vec),
            };
            walk.run(&names);
            walk.failed
        };
        (
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
            failed,
        )
    }

    fn bytes() -> Config {
        Config {
            format: Format {
                opts: Opts::NONE,
                block_size: 1,
            },
            ..Config::default()
        }
    }

    #[test]
    fn a_directory_prints_after_its_children_and_includes_them() {
        let (out, err, failed) = walk(&bytes(), &two_levels(), &[b"t"]);
        assert_eq!(out, "8192\tt/sub\n16384\tt\n");
        assert_eq!(err, "");
        assert!(!failed);
    }

    #[test]
    fn all_shows_the_files_and_summarize_shows_only_the_root() {
        let all = Config {
            all: true,
            ..bytes()
        };
        let (out, _, _) = walk(&all, &two_levels(), &[b"t"]);
        assert_eq!(out, "4096\tt/f\n4096\tt/sub/g\n8192\tt/sub\n16384\tt\n");

        let summary = Config {
            max_depth: 0,
            ..bytes()
        };
        let (out, _, _) = walk(&summary, &two_levels(), &[b"t"]);
        assert_eq!(out, "16384\tt\n");
    }

    /// `-S` displays a directory without its subdirectories but still hands the
    /// whole subtree upward — the two accumulators, seen apart.
    ///
    /// The grand total is the giveaway: were it the sum of the printed figures
    /// it would read 24576 here, and GNU prints 16384.
    #[test]
    fn separate_dirs_changes_the_row_and_not_the_total() {
        let cfg = Config {
            separate_dirs: true,
            total: true,
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &two_levels(), &[b"t"]);
        assert_eq!(out, "8192\tt/sub\n8192\tt\n16384\ttotal\n");
    }

    /// `-S`'s two accumulators again, one level deeper — where the sum of the
    /// rows and the sum of the subtrees are three different numbers rather than
    /// two, so a single accumulator cannot be made to fit by luck.
    ///
    /// Measured on a three-level tree with a 4 KiB file at each level and 4 KiB
    /// directories, in 1 KiB blocks:
    ///
    /// ```text
    /// $ du -S -c -a u
    /// 4  u/a/g      4  u/a/b/h      8  u/a/b      8  u/a      4  u/f      8  u
    /// 24 total
    /// ```
    ///
    /// The printed directory figures are 8, 8 and 8; the total is 24. It is not
    /// their sum (which would be 24 only by coincidence at two levels — here it
    /// is 24 against a row sum of 8+8+8 = 24 *including* the files, and 24
    /// against a directory-row sum of 24 as well). What pins it is the middle
    /// row: `u/a` reads 8 rather than 16, because `u/a/b` is a directory and is
    /// withheld from the row — yet those same blocks still reach the total.
    #[test]
    fn separate_dirs_still_hands_whole_subtrees_to_the_total() {
        let cfg = Config {
            separate_dirs: true,
            total: true,
            all: true,
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &three_levels(), &[b"u"]);
        assert_eq!(
            out,
            "4096\tu/a/g\n4096\tu/a/b/h\n8192\tu/a/b\n8192\tu/a\n4096\tu/f\n\
             8192\tu\n24576\ttotal\n"
        );

        // The same tree without `-S`, to show which figures the flag moved.
        let plain = Config {
            total: true,
            ..bytes()
        };
        let (out, _, _) = walk(&plain, &three_levels(), &[b"u"]);
        assert_eq!(out, "8192\tu/a/b\n16384\tu/a\n24576\tu\n24576\ttotal\n");
    }

    /// The same inode reached twice is counted once — including in the total.
    ///
    /// Measured, and it holds for directories as well as files: `du -s -c t t`
    /// prints one `t` row and a total equal to it, not two rows and double.
    #[test]
    fn a_repeated_inode_is_skipped_entirely() {
        let cfg = Config {
            total: true,
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &two_levels(), &[b"t/f", b"t/f"]);
        assert_eq!(out, "4096\tt/f\n4096\ttotal\n");

        let counted = Config {
            count_links: true,
            ..cfg
        };
        let (out, _, _) = walk(&counted, &two_levels(), &[b"t/f", b"t/f"]);
        assert_eq!(out, "4096\tt/f\n4096\tt/f\n8192\ttotal\n");

        // A directory named twice: the second naming is skipped whole, so its
        // children are never even reached.
        let summary = Config {
            max_depth: 0,
            total: true,
            ..bytes()
        };
        let (out, _, _) = walk(&summary, &two_levels(), &[b"t", b"t"]);
        assert_eq!(out, "16384\tt\n16384\ttotal\n");
    }

    #[test]
    fn max_depth_stops_the_rows_and_not_the_arithmetic() {
        let cfg = Config {
            max_depth: 0,
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &two_levels(), &[b"t"]);
        assert_eq!(out, "16384\tt\n");
    }

    #[test]
    fn apparent_size_counts_no_bytes_for_a_directory() {
        let cfg = Config {
            apparent_size: true,
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &two_levels(), &[b"t"]);
        assert_eq!(out, "4096\tt/sub\n8192\tt\n");
    }

    #[test]
    fn inodes_counts_one_per_entry_and_ignores_the_block_size() {
        let cfg = Config {
            inodes: true,
            format: Format {
                opts: Opts::NONE,
                block_size: 1024,
            },
            ..Config::default()
        };
        let (out, _, _) = walk(&cfg, &two_levels(), &[b"t"]);
        assert_eq!(out, "2\tt/sub\n4\tt\n");
    }

    #[test]
    fn an_unreadable_directory_still_counts_and_sets_the_status() {
        let mut tree = two_levels();
        tree.nodes
            .insert(b"t/sub".to_vec(), Node::Unreadable(dir(3, 8)));
        let (out, err, failed) = walk(&bytes(), &tree, &[b"t"]);
        assert_eq!(out, "4096\tt/sub\n12288\tt\n");
        assert_eq!(
            err,
            "du: cannot read directory 't/sub': Permission denied\n"
        );
        assert!(failed);
    }

    #[test]
    fn a_missing_operand_is_reported_with_its_errno() {
        let (out, err, failed) = walk(&bytes(), &two_levels(), &[b"nosuch"]);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "du: cannot access 'nosuch': No such file or directory\n"
        );
        assert!(failed);
    }

    /// A dangling symlink met while following gets the *shorter* message —
    /// `fts` reports it with errno 0, so there is nothing to append.
    #[test]
    fn a_dangling_symlink_followed_has_no_errno_tail() {
        let mut tree = two_levels();
        let mut meta = file(9, 0, 7);
        meta.is_symlink = true;
        tree.nodes
            .insert(b"dangle".to_vec(), Node::Link(meta, Vec::new()));

        let follow = Config {
            deref: Deref::Always,
            ..bytes()
        };
        let (out, err, failed) = walk(&follow, &tree, &[b"dangle"]);
        assert_eq!(out, "");
        assert_eq!(err, "du: cannot access 'dangle'\n");
        assert!(failed);

        // Left alone, the link is simply counted as itself.
        let (out, err, failed) = walk(&bytes(), &tree, &[b"dangle"]);
        assert_eq!(out, "0\tdangle\n");
        assert_eq!(err, "");
        assert!(!failed);
    }

    /// `-x` omits the crossing entry *entirely* — it is not printed at its own
    /// size and it contributes nothing upward. That is worth pinning because
    /// the other reading is at least as plausible: a mount point is a real
    /// directory on the parent's device as far as the parent's `readdir` is
    /// concerned, so "print the mount point, don't descend" would be a
    /// defensible design and is not what GNU does.
    ///
    /// Measured against a live mount, `/sys/fs/cgroup` being on device 23 while
    /// `/sys/fs` is on 22:
    ///
    /// ```text
    /// $ du -a -d1 /sys/fs      -> … 0 /sys/fs/cgroup … 0 /sys/fs
    /// $ du -a -d1 -x /sys/fs   -> …  (no cgroup row)  … 0 /sys/fs
    /// $ du -s -x /sys/fs/cgroup -> 0 /sys/fs/cgroup       (an operand is kept)
    /// ```
    #[test]
    fn one_file_system_drops_the_crossing_entry_but_keeps_the_operand() {
        let mut tree = two_levels();
        let mut other = dir(3, 8);
        other.dev = 2;
        tree.nodes
            .insert(b"t/sub".to_vec(), Node::Dir(other, vec![b"g".to_vec()]));
        let mut child = file(4, 8, 4096);
        child.dev = 2;
        tree.nodes.insert(b"t/sub/g".to_vec(), Node::File(child));

        let cfg = Config {
            one_file_system: true,
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &tree, &[b"t"]);
        assert_eq!(out, "8192\tt\n");
        // The operand itself is kept whatever device it is on.
        let (out, _, _) = walk(&cfg, &tree, &[b"t/sub"]);
        assert_eq!(out, "8192\tt/sub\n");
    }

    #[test]
    fn an_excluded_name_contributes_nothing_at_all() {
        let cfg = Config {
            total: true,
            excludes: vec![b"sub".to_vec()],
            ..bytes()
        };
        let (out, err, failed) = walk(&cfg, &two_levels(), &[b"t"]);
        assert_eq!(out, "8192\tt\n8192\ttotal\n");
        assert_eq!(err, "");
        assert!(!failed);

        // Excluding the operand yields nothing and is not a failure.
        let root = Config {
            excludes: vec![b"t".to_vec()],
            ..bytes()
        };
        let (out, _, failed) = walk(&root, &two_levels(), &[b"t"]);
        assert_eq!(out, "");
        assert!(!failed);
    }

    #[test]
    fn null_ends_every_row_including_the_total() {
        let cfg = Config {
            null: true,
            total: true,
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &two_levels(), &[b"t"]);
        // Measured with `du -c -0 t | od -c`: `8 \t t/sub \0 16 \t t \0 16 \t
        // total \0`. The total carries a NUL too, and it is the *root's* size —
        // the grand total is the sum of the operands, not of the printed rows.
        assert_eq!(out, "8192\tt/sub\u{0}16384\tt\u{0}16384\ttotal\u{0}");
    }

    #[test]
    fn the_grand_total_ignores_the_threshold() {
        let cfg = Config {
            total: true,
            threshold: Some(1_000_000),
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &two_levels(), &[b"t"]);
        assert_eq!(out, "16384\ttotal\n");
    }

    #[test]
    fn a_roots_trailing_slash_survives_into_every_child() {
        let cfg = Config {
            all: true,
            ..bytes()
        };
        let (out, _, _) = walk(&cfg, &two_levels(), &[b"t//"]);
        assert_eq!(out, "4096\tt/f\n4096\tt/sub/g\n8192\tt/sub\n16384\tt/\n");
    }

    // ---------------------------------------------------------- files0 ---

    #[test]
    fn a_nul_list_drops_only_its_final_empty_element() {
        assert_eq!(split_nul(b"t\0"), vec![b"t".to_vec()]);
        assert_eq!(split_nul(b"t"), vec![b"t".to_vec()]);
        assert_eq!(split_nul(b""), Vec::<Vec<u8>>::new());
        assert_eq!(
            split_nul(b"t\0\0hl\0"),
            vec![b"t".to_vec(), Vec::new(), b"hl".to_vec()]
        );
    }

    /// The complaint about an empty name lands *between* the rows either side
    /// of it, not before all of them. Measured on 9.4:
    ///
    /// ```text
    /// $ printf 't/f\0\0t/sub\0' > z0 && du --files0-from=z0
    /// 0	t/f
    /// du: z0:2: invalid zero-length file name
    /// 4	t/sub
    /// ```
    ///
    /// Only the ordering distinguishes this from a pre-scan, which is why the
    /// test interleaves rather than merely counting the diagnostics: it is the
    /// difference the harness caught.
    // The tabs in that transcript are the tabs `du` actually writes between the
    // size and the name. Widening them to spaces, as the lint asks, would make
    // the recorded output a paraphrase rather than a measurement.
    #[allow(clippy::tabs_in_doc_comments)]
    #[test]
    fn an_empty_name_is_reported_where_it_stands_in_the_list() {
        let cfg = bytes();
        let (out, err, failed) = walk_list(
            &cfg,
            &two_levels(),
            &[b"t/f", b"", b"t/sub"],
            Some(b"list0"),
        );
        assert_eq!(out, "4096\tt/f\n8192\tt/sub\n");
        assert_eq!(err, "du: list0:2: invalid zero-length file name\n");
        assert!(failed);
    }

    /// The same list without a `--files0-from` behind it: still refused, but
    /// the sentence loses the label and the position, because there is no
    /// list to point into. `du ''` is `du: invalid zero-length file name`.
    #[test]
    fn an_empty_operand_is_refused_without_a_position() {
        let cfg = bytes();
        let (out, err, failed) = walk(&cfg, &two_levels(), &[b"t/f", b""]);
        assert_eq!(out, "4096\tt/f\n");
        assert_eq!(err, "du: invalid zero-length file name\n");
        assert!(failed);
    }

    /// A name the caller spelled with a space is quoted; one that needs no
    /// quoting is left bare. `quotef`, not `quoteaf` — measured above.
    #[test]
    fn the_list_label_is_quoted_only_when_it_needs_to_be() {
        let cfg = bytes();
        let (_, plain, _) = walk_list(&cfg, &two_levels(), &[b""], Some(b"list0"));
        assert_eq!(plain, "du: list0:1: invalid zero-length file name\n");
        let (_, spaced, _) = walk_list(&cfg, &two_levels(), &[b""], Some(b"a b"));
        assert_eq!(spaced, "du: 'a b':1: invalid zero-length file name\n");
    }

    // --------------------------------------------------------- parsing ---

    fn parse(words: &[&str]) -> Result<Request, Refusal> {
        let argv: Vec<OsString> = words.iter().map(OsString::from).collect();
        parse_args(&argv, &Environment::default(), &|_| {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No such file or directory",
            ))
        })
    }

    fn config(words: &[&str]) -> Config {
        match parse(words) {
            Ok(Request::Run(cfg, _)) => cfg,
            _ => panic!("{words:?} did not produce a runnable command"),
        }
    }

    #[test]
    fn the_last_size_option_wins_and_apparent_size_is_sticky() {
        // Measured: `du -s -b -k` prints 1024-byte blocks of the apparent size,
        // and `du -s -k -b` prints bytes.
        let after_b = config(&["-b", "-k"]);
        assert!(after_b.apparent_size);
        assert_eq!(after_b.format.block_size, 1024);
        assert_eq!(after_b.format.opts, Opts::NONE);

        let after_k = config(&["-k", "-b"]);
        assert!(after_k.apparent_size);
        assert_eq!(after_k.format.block_size, 1);

        // `-h` resets the flags as well as the divisor, and `-b` after it
        // resets them back.
        let human = config(&["-b", "-h"]);
        assert!(human.apparent_size);
        assert_eq!(
            human.format.opts,
            Opts::AUTOSCALE | Opts::SI | Opts::BASE_1024
        );
        assert_eq!(config(&["-h", "-b"]).format.opts, Opts::NONE);
        assert_eq!(config(&["--si"]).format.opts, Opts::AUTOSCALE | Opts::SI);
        assert_eq!(config(&["-m"]).format.block_size, 1024 * 1024);
        // `--apparent-size` on its own leaves the divisor alone.
        assert_eq!(config(&["--apparent-size"]).format.block_size, 1024);
        assert!(config(&["--apparent-size"]).apparent_size);
    }

    #[test]
    fn the_last_dereference_option_wins() {
        assert_eq!(config(&["-L", "-P"]).deref, Deref::Never);
        assert_eq!(config(&["-P", "-L"]).deref, Deref::Always);
        assert_eq!(config(&["-L", "-H"]).deref, Deref::Args);
        assert_eq!(config(&["-D"]).deref, Deref::Args);
        assert_eq!(config(&["--dereference-args"]).deref, Deref::Args);
        assert_eq!(config(&[]).deref, Deref::Never);
    }

    #[test]
    fn summarize_is_max_depth_zero_and_conflicts_are_measured() {
        assert_eq!(config(&["-s"]).max_depth, 0);
        assert_eq!(config(&[]).max_depth, u64::MAX);
        assert_eq!(config(&["-d", "2"]).max_depth, 2);

        let refusal = parse(&["-s", "-a"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec!["du: cannot both summarize and show all entries"]
        );
        assert!(refusal.referral);

        let refusal = parse(&["-s", "-d1"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec!["du: warning: summarizing conflicts with --max-depth=1"]
        );
        assert!(refusal.referral);

        // `-s -d0` is a warning that does not stop the run.
        assert_eq!(config(&["-s", "-d0"]).max_depth, 0);
    }

    /// `-d`'s complaint is deferred and `-B`'s is not, so the pair prints two
    /// sentences and no referral.
    #[test]
    fn a_deferred_complaint_keeps_its_place_in_the_line() {
        let refusal = parse(&["-d", "zz"]).err().unwrap();
        assert_eq!(refusal.lines, vec!["du: invalid maximum depth ‘zz’"]);
        assert!(refusal.referral);
        assert_eq!(refusal.status, 1);

        let refusal = parse(&["-d", "zz", "-B", "zz"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec![
                "du: invalid maximum depth ‘zz’",
                "du: invalid suffix in -B argument 'zz'",
            ]
        );
        assert!(!refusal.referral);

        // An `-X` that cannot be read is deferred too, and keeps its order.
        let refusal = parse(&["-X", "nosuch", "-d", "zz"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec![
                "du: nosuch: No such file or directory",
                "du: invalid maximum depth ‘zz’",
            ]
        );
        assert!(refusal.referral);
    }

    #[test]
    fn each_spelling_of_an_option_names_itself_in_its_diagnostic() {
        let refusal = parse(&["--block-size=zz"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec!["du: invalid suffix in --block-size argument 'zz'"]
        );
        let refusal = parse(&["--threshold=1g"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec!["du: invalid suffix in --threshold argument '1g'"]
        );
        let refusal = parse(&["-t", "1g"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec!["du: invalid suffix in -t argument '1g'"]
        );
        // Except `-t -0`, whose sentence is a literal upstream and names
        // `--threshold` however it was reached.
        let refusal = parse(&["-t", "-0"]).err().unwrap();
        assert_eq!(refusal.lines, vec!["du: invalid --threshold argument '-0'"]);
        let refusal = parse(&["-t", "-0K"]).err().unwrap();
        assert_eq!(refusal.lines, vec!["du: invalid --threshold argument '-0'"]);
    }

    #[test]
    fn the_threshold_suffix_list_is_not_the_block_size_one() {
        assert_eq!(config(&["-t", "1k"]).threshold, Some(1024));
        assert_eq!(config(&["-t", "1M"]).threshold, Some(1024 * 1024));
        assert_eq!(config(&["-t", "1KB"]).threshold, Some(1000));
        assert_eq!(config(&["-t", "-1K"]).threshold, Some(-1024));
        assert_eq!(config(&["-t", "0x10"]).threshold, Some(16));
        for bad in ["1g", "1t", "1p", "1e", "1z", "1y", "1b", "1B", "1w"] {
            assert!(parse(&["-t", bad]).is_err(), "-t {bad} was accepted");
        }
        for big in ["1Z", "1Y", "1R", "1Q"] {
            let refusal = parse(&["-t", big]).err().unwrap();
            assert_eq!(
                refusal.lines,
                vec![format!("du: -t argument '{big}' too large")]
            );
        }
    }

    #[test]
    fn an_unknown_option_is_getopts_sentence_with_a_referral() {
        let refusal = parse(&["-q"]).err().unwrap();
        assert_eq!(refusal.lines, vec!["du: invalid option -- 'q'"]);
        assert!(refusal.referral);

        let refusal = parse(&["--e=x"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec!["du: option '--e=x' is ambiguous; possibilities: '--exclude' '--exclude-from'"]
        );
        assert!(refusal.referral);
    }

    /// `--time` stays in the table so that `--t` remains ambiguous, and is
    /// refused by name when it is actually used.
    #[test]
    fn time_is_refused_rather_than_silently_missing() {
        let refusal = parse(&["--time"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec!["du: option '--time' is not implemented by this du"]
        );
        assert!(refusal.referral);
        let refusal = parse(&["--time-style=iso"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec!["du: option '--time-style' is not implemented by this du"]
        );
        // Still ambiguous, which is the reason the entries are kept.
        let refusal = parse(&["--t"]).err().unwrap();
        assert!(refusal.lines[0].contains("ambiguous"));
    }

    #[test]
    fn operands_default_to_the_working_directory() {
        match parse(&[]) {
            Ok(Request::Run(_, Source::Operands(names))) => {
                assert_eq!(names, vec![b".".to_vec()]);
            }
            _ => panic!("no operands did not default to '.'"),
        }
    }

    #[test]
    fn files0_from_refuses_to_share_the_command_line_with_an_operand() {
        let refusal = parse(&["--files0-from=list", "t"]).err().unwrap();
        assert_eq!(
            refusal.lines,
            vec![
                "du: extra operand ‘t’",
                "file operands cannot be combined with --files0-from",
            ]
        );
        assert!(refusal.referral);
    }

    #[test]
    fn help_and_version_short_circuit_everything_after_them() {
        assert!(matches!(parse(&["--help"]), Ok(Request::Help)));
        assert!(matches!(parse(&["--version"]), Ok(Request::Version)));
        // Even a command line that would otherwise be refused.
        assert!(matches!(parse(&["--help", "-q"]), Ok(Request::Help)));
    }

    /// An operand that is not UTF-8 reaches the walk unchanged, which is the
    /// whole reason argv is read as `OsString`.
    #[test]
    fn a_name_that_is_not_text_survives_parsing() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let argv = vec![OsString::from_vec(b"\xff\xfe".to_vec())];
            match parse_args(&argv, &Environment::default(), &|_| {
                Err(io::Error::other("unused"))
            }) {
                Ok(Request::Run(_, Source::Operands(names))) => {
                    assert_eq!(names, vec![b"\xff\xfe".to_vec()]);
                }
                _ => panic!("a non-UTF-8 operand did not survive"),
            }
        }
    }
}
