//! `md5sum` and `sha256sum`, which upstream are one program.
//!
//! A port of GNU coreutils 9.4's `src/digest.c` — the single file that is
//! compiled eight times, once per `HASH_ALGO_*`, to produce `md5sum`,
//! `sha1sum`, `sha224sum`, `sha256sum`, `sha384sum`, `sha512sum`, `b2sum` and
//! `cksum`. Only the two we ship are built on it here; the shape is upstream's
//! so a third costs an [`Algorithm`] and nothing else.
//!
//! # Why this is a module and not two `main`s
//!
//! Because the interesting half of these programs is not the hash. It is
//! `--check`, and `--check` is a *parser* for output this program wrote
//! earlier — three formats, an escaping convention, and a rule about which of
//! the formats may appear in one file. Two hand-written copies of that would
//! disagree, and a disagreement here is not cosmetic: `sha256sum -c` reporting
//! `OK` for a line it misread is the exact failure the utility exists to
//! prevent. The hash itself is the part that is genuinely per-utility, so that
//! is what stays in the bin: each supplies an [`Algorithm`] and this module is
//! everything else.
//!
//! # What the shipped versions could not do
//!
//! Both were about forty lines and accepted **no options at all** — not `-c`,
//! which is the reason anybody runs these; not `--tag`, `-b`, `-t`, `-z`,
//! `--ignore-missing`, `--quiet`, `--status`, `--strict`, `-w`, `--help` or
//! `--version`. A `-c` typed at either of them was read as a *file name*, so
//! `md5sum -c SUMS` printed a checksum of a file called `-c` (or, more often,
//! `No such file or directory`) and exited 1 — the same status a real check
//! failure gives, which is the worst way to be wrong. They also read the whole
//! file into memory before hashing it, and read argv as `String`, so a name
//! holding a byte that is not valid UTF-8 — legal here, `design.txt` forbids
//! only `/` and NUL — panicked before `main`'s first statement.
//!
//! # The three formats `--check` reads, and the rule that keeps them apart
//!
//! ```text
//! <hex>  NAME     the default: two spaces, or one space and '*' for -b
//! <hex> NAME      "BSD reversed": one space, no type indicator
//! MD5 (NAME) = <hex>   --tag, the BSD `md5` command's own format
//! ```
//!
//! A file may not mix the first two, and the reason is a security argument
//! rather than tidiness. In the default format the byte after the digest is a
//! type indicator and the name starts one byte later; in the reversed format
//! that byte is already the name. So a *reversed* line whose name begins with a
//! space or `*` would be read as a default line naming a different file — which
//! is a rename away from making a checksum file verify the wrong contents.
//! Upstream latches `bsd_reversed` on the first line that settles the question
//! and rejects every later line of the other kind; this port does the same, in
//! [`Checker::bsd_reversed`].
//!
//! # Escaping, which is why a name can survive a newline
//!
//! A name containing `\n`, `\r` or `\` is written with those bytes replaced by
//! `\n`, `\r` and `\\`, and the *line* is prefixed with a single `\` to say so.
//! Without it a name holding a newline would produce a checksum file that reads
//! as two lines, one of which is attacker-chosen text. `-z` turns the escaping
//! off and terminates each record with NUL instead, which is safe for the same
//! reason `find -print0` is.
//!
//! The unescape direction refuses anything it cannot round-trip — a trailing
//! lone `\`, or `\` followed by anything but `n`, `r` or `\` — rather than
//! guessing, so a hand-edited checksum file fails as misformatted instead of
//! naming a file nobody wrote down.
//!
//! # Deliberate differences from GNU
//!
//! * **`-b`/`-t` do nothing but set the indicator byte**, exactly as on
//!   GNU/Linux, where `O_BINARY` is 0. Upstream's DOS-only paths
//!   (`xset_binary_mode`, the `isatty` test that makes `-b` mean "binary unless
//!   stdin is a terminal") are not reachable on any platform we target and are
//!   not reproduced.
//! * **`--help` omits the GNU project's `Report bugs to:` block**, as every
//!   converted utility here does.
//! * **No `fadvise`.** `FADVISE_SEQUENTIAL` is a hint; its absence changes
//!   throughput, never output.
//!
//! # How this is tested
//!
//! `scripts/digest-diff.sh` builds both bins for Linux inside WSL and runs them
//! against GNU `md5sum`/`sha256sum` case by case — the same answer `cmp`, `tee`,
//! `echo`, `du`, `find` and `ls` use (`design-decisions.md` §374). The unit
//! tests at the bottom of this file cover the line parser, which is the part a
//! harness reaches only through whole files.

use crate::diag;
use crate::errmsg::strerror;
use crate::getopt::{self, Opt, Program, Takes};
use crate::quote::{os_bytes, os_from_bytes, quotef};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::process::ExitCode;

// ------------------------------------------------------------- the algorithm ---

/// A hash being fed a file, one chunk at a time.
///
/// Streaming rather than `fn(&[u8]) -> Vec<u8>`, because the two shipped
/// versions read the whole file into memory first and that is not a detail:
/// `md5sum` on a disk image is a normal thing to do, and an allocation the size
/// of the input is a way to be killed by the OOM path rather than to answer.
pub trait Stream {
    /// Absorb the next bytes of the message.
    fn update(&mut self, data: &[u8]);
    /// Finish, yielding the raw digest — `bits / 8` bytes.
    fn finish(self: Box<Self>) -> Vec<u8>;
}

/// Everything `digest.c`'s `#if HASH_ALGO_*` block decides, as data.
///
/// Upstream these are preprocessor defines, so the compiler proves each build
/// consistent. Here they are a struct, and the one invariant that is no longer
/// checked for free is `bits` against what [`Stream::finish`] actually returns.
/// [`Algorithm::hex_len`] is derived from `bits`, and a mismatch would make
/// every digest print truncated or panic — so the bins assert it, in
/// `digest_length_matches_the_declared_bits`.
pub struct Algorithm {
    /// `argv[0]` as diagnostics spell it: `md5sum`.
    pub program: &'static str,
    /// The name inside a `--tag` line and in `improperly formatted %s checksum
    /// line`: `MD5`, `SHA256`.
    pub tag: &'static str,
    /// Digest width. Decides the hex length, and so which check lines parse.
    pub bits: usize,
    /// The standard named by `--help`'s last paragraph: `RFC 1321`.
    pub reference: &'static str,
    /// Start a new hash.
    pub new: fn() -> Box<dyn Stream>,
}

impl Algorithm {
    /// `DIGEST_HEX_BYTES`: how many hex digits a digest of this width takes.
    #[must_use]
    pub const fn hex_len(&self) -> usize {
        self.bits / 4
    }

    /// `MIN_DIGEST_LINE_LENGTH`: the digest, a blank, and a one-byte name.
    const fn min_line_len(&self) -> usize {
        self.hex_len() + 2
    }
}

// -------------------------------------------------------------- the options ---

/// Upstream's `short_opts` for the non-`cksum`, non-`b2sum` builds, verbatim.
const SHORT_OPTIONS: &str = "bctwz";

/// Upstream's `long_options`, in its order — which is observable, because glibc
/// lists the candidates for an ambiguous prefix in table order. Measured:
/// `md5sum --=x` names them `'--check' '--ignore-missing' '--quiet' '--status'
/// '--warn' '--strict' '--tag' '--zero' '--binary' '--text' '--help'
/// '--version'`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("check", Takes::Nothing),
    ("ignore-missing", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("status", Takes::Nothing),
    ("warn", Takes::Nothing),
    ("strict", Takes::Nothing),
    ("tag", Takes::Nothing),
    ("zero", Takes::Nothing),
    ("binary", Takes::Nothing),
    ("text", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Settings {
    check: bool,
    /// Upstream's `int binary`, whose third state is load-bearing: `-1` means
    /// "neither `-b` nor `-t` was given", and the *only* thing that state does
    /// on a POSIX host is decide whether `--binary and --text are meaningless
    /// when verifying` fires. `Option` rather than a `bool` plus a flag so the
    /// two cannot drift apart.
    binary: Option<bool>,
    tag: bool,
    zero: bool,
    status_only: bool,
    warn: bool,
    quiet: bool,
    strict: bool,
    ignore_missing: bool,
    files: Vec<OsString>,
}

/// Read the command line. Upstream's option loop, one arm per `case`.
///
/// The three-way exclusion between `--status`, `-w` and `--quiet` is upstream's
/// and is *last-wins* rather than an error: each arm clears the other two, so
/// `--status -w` warns and `-w --status` is silent.
fn parse_args(program: Program, args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut set = Settings::default();
    for item in program.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'c', _) | Opt::Long("check", _) => set.check = true,
            Opt::Short(b'b', _) | Opt::Long("binary", _) => set.binary = Some(true),
            Opt::Short(b't', _) | Opt::Long("text", _) => set.binary = Some(false),
            Opt::Short(b'w', _) | Opt::Long("warn", _) => {
                set.status_only = false;
                set.warn = true;
                set.quiet = false;
            }
            Opt::Short(b'z', _) | Opt::Long("zero", _) => set.zero = true,
            Opt::Long("status", _) => {
                set.status_only = true;
                set.warn = false;
                set.quiet = false;
            }
            Opt::Long("quiet", _) => {
                set.status_only = false;
                set.warn = false;
                set.quiet = true;
            }
            Opt::Long("strict", _) => set.strict = true,
            Opt::Long("ignore-missing", _) => set.ignore_missing = true,
            // `case TAG_OPTION: prefix_tag = true; binary = 1;` — the second
            // assignment is why `--tag --text` is an error but `--text --tag`
            // is not.
            Opt::Long("tag", _) => {
                set.tag = true;
                set.binary = Some(true);
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => set.files.push(word.clone()),
            // Every entry of the two tables is handled above; an unknown option
            // arrives as an `Err` from `parse`.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    Ok(Request::Run(set))
}

/// The nine post-parse consistency checks, in upstream's order — which is
/// observable, since only the first to fire is printed.
fn validate(program: Program, set: &Settings) -> Result<(), getopt::Error> {
    // `if (prefix_tag && !binary)`: `!binary` is false for the unset `-1`, so
    // this needs `-t` *explicitly*, after `--tag`.
    if set.tag && set.binary == Some(false) {
        return Err(program.usage_referring("--tag does not support --text mode".to_string()));
    }
    if set.zero && set.check {
        return Err(program.usage_referring(
            "the --zero option is not supported when verifying checksums".to_string(),
        ));
    }
    if set.tag && set.check {
        return Err(program.usage_referring(
            "the --tag option is meaningless when verifying checksums".to_string(),
        ));
    }
    if set.binary.is_some() && set.check {
        return Err(program.usage_referring(
            "the --binary and --text options are meaningless when verifying checksums".to_string(),
        ));
    }
    for (on, name) in [
        (set.ignore_missing, "--ignore-missing"),
        (set.status_only, "--status"),
        (set.warn, "--warn"),
        (set.quiet, "--quiet"),
        (set.strict, "--strict"),
    ] {
        if on && !set.check {
            return Err(program.usage_referring(format!(
                "the {name} option is meaningful only when verifying checksums"
            )));
        }
    }
    Ok(())
}

/// GNU's `--help`, minus the project's `Report bugs to:` block.
///
/// The `-b`/`-t` wordings are the `O_BINARY == 0` arms of upstream's `if`,
/// which are the ones GNU/Linux prints.
fn help_text(algo: &Algorithm) -> String {
    format!(
        "\
Usage: {program} [OPTION]... [FILE]...
Print or check {tag} ({bits}-bit) checksums.

With no FILE, or when FILE is -, read standard input.
  -b, --binary          read in binary mode
  -c, --check           read checksums from the FILEs and check them
      --tag             create a BSD-style checksum
  -t, --text            read in text mode (default)
  -z, --zero            end each output line with NUL, not newline,
                          and disable file name escaping

The following five options are useful only when verifying checksums:
      --ignore-missing  don't fail or report status for missing files
      --quiet           don't print OK for each successfully verified file
      --status          don't output anything, status code shows success
      --strict          exit non-zero for improperly formatted checksum lines
  -w, --warn            warn about improperly formatted checksum lines

      --help        display this help and exit
      --version     output version information and exit

The sums are computed as described in {reference}.
When checking, the input should be a former output of this program.
The default mode is to print a line with: checksum, a space,
a character indicating input mode ('*' for binary, ' ' for text
or where binary is insignificant), and name for each FILE.

Note: There is no difference between binary mode and text mode on GNU systems.
",
        program = algo.program,
        tag = algo.tag,
        bits = algo.bits,
        reference = algo.reference,
    )
}

// ------------------------------------------------------------- name escaping ---

/// Upstream's `problematic_chars`: does printing this name raw corrupt the line?
///
/// `\r` is in the set for the same reason `\n` is — a terminal reading the
/// output back treats it as a line boundary — and `\` because the escaping must
/// be reversible.
fn problematic(name: &[u8]) -> bool {
    name.iter().any(|b| matches!(b, b'\\' | b'\n' | b'\r'))
}

/// Upstream's `print_filename`. `escape` is decided once per line, by
/// [`problematic`], and announced by a `\` before the record.
fn escape_name(name: &[u8], escape: bool) -> Vec<u8> {
    if !escape {
        return name.to_vec();
    }
    let mut out = Vec::with_capacity(name.len());
    for &b in name {
        match b {
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            _ => out.push(b),
        }
    }
    out
}

/// Upstream's `filename_unescape`, the reverse.
///
/// Returns `None` for input this program could not have written: a trailing
/// lone backslash, a backslash before anything but `n`/`r`/`\`, or an embedded
/// NUL. Refusing rather than guessing is what keeps a hand-edited checksum file
/// from silently naming a different file.
fn unescape_name(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&b) = s.get(i) {
        match b {
            b'\\' => {
                i = i.checked_add(1)?;
                match s.get(i) {
                    Some(b'n') => out.push(b'\n'),
                    Some(b'r') => out.push(b'\r'),
                    Some(b'\\') => out.push(b'\\'),
                    // Includes the end of the string: a name ending in an
                    // unescaped backslash is invalid.
                    _ => return None,
                }
            }
            // A name may not contain a NUL, and one here means the line did.
            0 => return None,
            _ => out.push(b),
        }
        i = i.checked_add(1)?;
    }
    Some(out)
}

// ------------------------------------------------------------------ hashing ---

/// One read of the file. Upstream's `md5_stream` uses 32 KiB; the size is not
/// observable in the output, only in the syscall count.
const READ_CHUNK: usize = 65536;

/// The outcome of hashing one operand, keeping `missing` apart from the other
/// failures because `--ignore-missing` distinguishes them.
enum Hashed {
    Ok(Vec<u8>),
    /// `ENOENT` under `--ignore-missing`: not an error, not a result.
    Missing,
    Failed,
}

/// Upstream's `digest_file`. `-` is standard input, and is *not* a file called
/// `-`: that is POSIX for this utility, unlike `tee`.
///
/// Diagnoses its own failures, because upstream does and because the caller
/// (`--check`) prints a *second*, different line about the same file.
fn hash_file(algo: &Algorithm, name: &[u8], ignore_missing: bool, read_stdin: &mut bool) -> Hashed {
    let mut hasher = (algo.new)();
    let mut buf = vec![0u8; READ_CHUNK];

    let mut feed = |src: &mut dyn Read| -> io::Result<()> {
        loop {
            match src.read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(n) => hasher.update(buf.get(..n).unwrap_or(&[])),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    };

    let result = if name == b"-" {
        *read_stdin = true;
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        feed(&mut stdin)
    } else {
        match File::open(os_from_bytes(name)) {
            Ok(f) => {
                let mut reader = BufReader::new(f);
                feed(&mut reader)
            }
            Err(e) => {
                if ignore_missing && e.kind() == io::ErrorKind::NotFound {
                    return Hashed::Missing;
                }
                diag!("{}: {}: {}", algo.program, quotef(name), strerror(&e));
                return Hashed::Failed;
            }
        }
    };

    match result {
        Ok(()) => Hashed::Ok(hasher.finish()),
        Err(e) => {
            diag!("{}: {}: {}", algo.program, quotef(name), strerror(&e));
            Hashed::Failed
        }
    }
}

/// Lowercase hex, which is the only case this program writes. `--check` accepts
/// either, via [`hex_equal`].
fn to_hex(digest: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(digest.len().saturating_mul(2));
    for &b in digest {
        out.push(*HEX.get(usize::from(b >> 4)).unwrap_or(&b'0'));
        out.push(*HEX.get(usize::from(b & 0x0f)).unwrap_or(&b'0'));
    }
    out
}

/// Upstream's `hex_equal`: compare the recorded text against the computed
/// bytes, ignoring the case of the recorded hex digits.
fn hex_equal(recorded: &[u8], computed: &[u8]) -> bool {
    if recorded.len() != computed.len().saturating_mul(2) {
        return false;
    }
    to_hex(computed)
        .iter()
        .zip(recorded)
        .all(|(&want, &got)| want == got.to_ascii_lowercase())
}

// ------------------------------------------------------------------- checking ---

/// One parsed line of a checksum file.
#[derive(Debug, PartialEq, Eq)]
struct CheckLine {
    /// The digest *as written*, still hex text, so its case survives for
    /// [`hex_equal`] to ignore.
    digest: Vec<u8>,
    /// The `*`/` ` indicator, absent in the tagged and reversed formats.
    binary: bool,
    name: Vec<u8>,
}

/// Which of the two untagged layouts a file is using, latched on the first line
/// that settles it. See the module docs for why mixing them is refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Layout {
    #[default]
    Undecided,
    /// `<hex>  NAME` — a type indicator byte after the digest.
    Standard,
    /// `<hex> NAME` — no indicator; the name starts immediately.
    BsdReversed,
}

/// The per-file state `split_3` keeps in statics upstream.
struct Checker {
    bsd_reversed: Layout,
}

const fn is_white(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

impl Checker {
    const fn new() -> Self {
        Checker {
            bsd_reversed: Layout::Undecided,
        }
    }

    /// Upstream's `valid_digits`, for the non-`cksum` builds: exactly
    /// `hex_len` hex digits.
    fn valid_digits(algo: &Algorithm, s: &[u8]) -> bool {
        s.len() == algo.hex_len() && s.iter().all(u8::is_ascii_hexdigit)
    }

    /// Upstream's `split_3`. `line` has already had its terminator removed.
    fn split_3(&mut self, algo: &Algorithm, line: &[u8]) -> Option<CheckLine> {
        let mut i = 0usize;
        while line.get(i).copied().is_some_and(is_white) {
            i = i.checked_add(1)?;
        }

        let mut escaped = false;
        if line.get(i) == Some(&b'\\') {
            i = i.checked_add(1)?;
            escaped = true;
        }

        // --- the tagged format ---------------------------------------------
        let rest = line.get(i..)?;
        if rest.starts_with(algo.tag.as_bytes()) {
            i = i.checked_add(algo.tag.len())?;
            if line.get(i) == Some(&b' ') {
                i = i.checked_add(1)?;
            }
            if line.get(i) == Some(&b'(') {
                i = i.checked_add(1)?;
                return Self::bsd_split_3(algo, line.get(i..)?, escaped);
            }
            // A line that begins with the tag and is not tagged-format is not
            // then retried as a plain one: upstream returns false here.
            return None;
        }

        // --- too short to be either ----------------------------------------
        // `s_len - i < min_digest_line_length + (s[i] == '\\')`. The second
        // backslash test is upstream's and is *not* the one that set `escaped`
        // — that byte has already been consumed.
        let extra = usize::from(line.get(i) == Some(&b'\\'));
        if line.len().saturating_sub(i) < algo.min_line_len().saturating_add(extra) {
            return None;
        }

        // --- the digest ----------------------------------------------------
        let start = i;
        while line.get(i).copied().is_some_and(|b| !is_white(b)) {
            i = i.checked_add(1)?;
        }
        // The digest must be followed by at least one whitespace character.
        if i == line.len() {
            return None;
        }
        let digest = line.get(start..i)?.to_vec();
        i = i.checked_add(1)?;
        if !Self::valid_digits(algo, &digest) {
            return None;
        }

        // --- which layout, and the indicator byte --------------------------
        let after = line.get(i).copied();
        let mut binary = false;
        if line.len().saturating_sub(i) == 1 || !matches!(after, Some(b' ') | Some(b'*')) {
            if self.bsd_reversed == Layout::Standard {
                return None;
            }
            self.bsd_reversed = Layout::BsdReversed;
        } else if self.bsd_reversed != Layout::BsdReversed {
            self.bsd_reversed = Layout::Standard;
            binary = after == Some(b'*');
            i = i.checked_add(1)?;
        }

        // Everything left is the name, leading and trailing blanks included.
        let name = line.get(i..)?.to_vec();
        let name = if escaped { unescape_name(&name)? } else { name };
        Some(CheckLine {
            digest,
            binary,
            name,
        })
    }

    /// Upstream's `bsd_split_3`, given everything after the `(`.
    ///
    /// The name is found by scanning back from the end for `)`, not forward for
    /// the first one, so a name containing `)` still parses.
    fn bsd_split_3(algo: &Algorithm, s: &[u8], escaped: bool) -> Option<CheckLine> {
        if s.is_empty() {
            return None;
        }
        let mut i = s.len().checked_sub(1)?;
        while i > 0 && s.get(i) != Some(&b')') {
            i = i.checked_sub(1)?;
        }
        if s.get(i) != Some(&b')') {
            return None;
        }
        let name = s.get(..i)?.to_vec();
        let name = if escaped { unescape_name(&name)? } else { name };

        i = i.checked_add(1)?;
        while s.get(i).copied().is_some_and(is_white) {
            i = i.checked_add(1)?;
        }
        if s.get(i) != Some(&b'=') {
            return None;
        }
        i = i.checked_add(1)?;
        while s.get(i).copied().is_some_and(is_white) {
            i = i.checked_add(1)?;
        }
        let digest = s.get(i..)?.to_vec();
        if !Self::valid_digits(algo, &digest) {
            return None;
        }
        Some(CheckLine {
            digest,
            // `*binary = 0` in upstream: a tagged line carries no indicator.
            binary: false,
            name,
        })
    }
}

/// What one `--check` file amounted to. Upstream's four counters plus the two
/// booleans that decide the exit status.
#[derive(Default)]
struct CheckTally {
    misformatted: u64,
    mismatched: u64,
    unreadable: u64,
    any_formatted: bool,
    any_matched: bool,
}

impl CheckTally {
    /// Upstream's return from `digest_check`, verbatim: `--strict` adds the
    /// misformatted count to what already had to be zero.
    const fn ok(&self, strict: bool) -> bool {
        self.any_formatted
            && self.any_matched
            && self.mismatched == 0
            && self.unreadable == 0
            && (!strict || self.misformatted == 0)
    }
}

/// English plural agreement for the three `WARNING:` lines, which upstream gets
/// from `ngettext`.
fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

// --------------------------------------------------------------------- main ---

/// The whole of `digest.c`'s `main`, for one algorithm.
///
/// # Panics
///
/// Does not. Every fallible step returns a status instead.
#[must_use]
pub fn main(algo: &Algorithm) -> ExitCode {
    // Upstream registers `close_stdout` with `atexit`, so its verdict is
    // reached on every exit path, not just the last statement of `main`. One
    // value leaves this function; funnelling it here is the same guarantee.
    crate::stdfd::close_stderr(run_main(algo), 1)
}

/// Everything the utility does, so that [`main`] is only the exit path --
/// upstream's `main` minus the `atexit` handler it registers.
fn run_main(algo: &Algorithm) -> ExitCode {
    let program = Program::new(algo.program, 1);
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    let request = match parse_args(program, &args) {
        Ok(r) => r,
        Err(e) => {
            program.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    let mut set = match request {
        Request::Help => {
            print!("{}", help_text(algo));
            return ExitCode::SUCCESS;
        }
        Request::Version => {
            println!("{} (SlateOS coreutils) 0.1.0", algo.program);
            return ExitCode::SUCCESS;
        }
        Request::Run(set) => set,
    };

    if let Err(e) = validate(program, &set) {
        program.report(&e);
        return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
    }

    // `if (!O_BINARY && binary < 0) binary = 0;` — on a POSIX host the unset
    // state resolves to text, which is why `md5sum f` prints two spaces.
    let binary = set.binary.unwrap_or(false);

    // `if (optind == argc) *operand_lim++ = bad_cast ("-");`
    if set.files.is_empty() {
        set.files.push(OsString::from("-"));
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut ok = true;
    let mut read_stdin = false;

    for operand in &set.files {
        let name = os_bytes(operand);
        if set.check {
            if !check_file(algo, &set, &name, &mut out, &mut read_stdin) {
                ok = false;
            }
        } else {
            match hash_file(algo, &name, false, &mut read_stdin) {
                Hashed::Ok(digest) => {
                    let line = render(algo, &name, binary, &digest, set.tag, set.zero);
                    if out.write_all(&line).and_then(|()| out.flush()).is_err() {
                        // A write error here is upstream's `close_stdout`, which
                        // reports and exits 1 rather than continuing to a file
                        // whose checksum nobody will see.
                        diag!("{}: write error", algo.program);
                        return ExitCode::from(1);
                    }
                }
                Hashed::Missing | Hashed::Failed => ok = false,
            }
        }
    }

    // `if (have_read_stdin && fclose (stdin) == EOF)`. There is no `fclose` on
    // a locked Rust stdin, and the case it catches — a read error latched but
    // not yet reported — is already reported by `hash_file`.
    let _ = read_stdin;

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// One output record: `output_file`, both layouts.
fn render(
    algo: &Algorithm,
    name: &[u8],
    binary: bool,
    digest: &[u8],
    tagged: bool,
    zero: bool,
) -> Vec<u8> {
    let escape = !zero && problematic(name);
    let shown = escape_name(name, escape);
    let hex = to_hex(digest);

    let mut out = Vec::with_capacity(hex.len().saturating_add(shown.len()).saturating_add(8));
    if escape {
        out.push(b'\\');
    }
    if tagged {
        out.extend_from_slice(algo.tag.as_bytes());
        out.extend_from_slice(b" (");
        out.extend_from_slice(&shown);
        out.extend_from_slice(b") = ");
        out.extend_from_slice(&hex);
    } else {
        out.extend_from_slice(&hex);
        out.push(b' ');
        out.push(if binary { b'*' } else { b' ' });
        out.extend_from_slice(&shown);
    }
    out.push(if zero { 0 } else { b'\n' });
    out
}

/// Upstream's `digest_check`, for one checksum file.
fn check_file(
    algo: &Algorithm,
    set: &Settings,
    checkfile: &[u8],
    out: &mut impl Write,
    read_stdin: &mut bool,
) -> bool {
    let is_stdin = checkfile == b"-";
    // Upstream renames it for every diagnostic; the file is never reopened by
    // this name, so the substitution is purely in the messages.
    let shown_name: Vec<u8> = if is_stdin {
        b"standard input".to_vec()
    } else {
        checkfile.to_vec()
    };

    let mut source: Box<dyn Read> = if is_stdin {
        *read_stdin = true;
        Box::new(io::stdin())
    } else {
        match File::open(os_from_bytes(checkfile)) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("{}: {}: {}", algo.program, quotef(checkfile), strerror(&e));
                return false;
            }
        }
    };

    let mut text = Vec::new();
    if let Err(e) = source.read_to_end(&mut text) {
        // `ferror` — upstream prints the errno-free `%s: read error` here, not
        // `strerror`, which is why this reads oddly next to every other
        // diagnostic in the file.
        let _ = e;
        diag!("{}: {}: read error", algo.program, quotef(&shown_name));
        return false;
    }

    let mut checker = Checker::new();
    let mut tally = CheckTally::default();

    for (index, raw) in split_lines(&text).into_iter().enumerate() {
        let line_number = index.saturating_add(1);

        // Comment lines, tested before the terminator is stripped, exactly as
        // upstream: `if (line[0] == '#') continue;`
        if raw.first() == Some(&b'#') {
            continue;
        }
        let line = strip_terminator(raw);
        if line.is_empty() {
            continue;
        }

        let Some(parsed) = checker
            .split_3(algo, line)
            .filter(|p| !(is_stdin && p.name == b"-"))
        else {
            tally.misformatted = tally.misformatted.saturating_add(1);
            if set.warn {
                diag!(
                    "{}: {}: {}: improperly formatted {} checksum line",
                    algo.program,
                    quotef(&shown_name),
                    line_number,
                    algo.tag
                );
            }
            continue;
        };

        tally.any_formatted = true;
        let needs_escape = !set.status_only && problematic(&parsed.name);
        let shown = escape_name(&parsed.name, needs_escape);
        let prefix: &[u8] = if needs_escape { b"\\" } else { b"" };

        match hash_file(algo, &parsed.name, set.ignore_missing, read_stdin) {
            Hashed::Failed => {
                tally.unreadable = tally.unreadable.saturating_add(1);
                if !set.status_only {
                    let _ = out.write_all(prefix);
                    let _ = out.write_all(&shown);
                    let _ = out.write_all(b": FAILED open or read\n");
                    let _ = out.flush();
                }
            }
            Hashed::Missing => {}
            Hashed::Ok(computed) => {
                let matched = hex_equal(&parsed.digest, &computed);
                if matched {
                    tally.any_matched = true;
                } else {
                    tally.mismatched = tally.mismatched.saturating_add(1);
                }
                if !set.status_only {
                    if !matched || !set.quiet {
                        let _ = out.write_all(prefix);
                        let _ = out.write_all(&shown);
                    }
                    if matched {
                        if !set.quiet {
                            let _ = out.write_all(b": OK\n");
                        }
                    } else {
                        let _ = out.write_all(b": FAILED\n");
                    }
                    let _ = out.flush();
                }
            }
        }
        // `binary` is parsed and unused on a POSIX host, exactly as upstream:
        // it only ever selected `"rb"` over `"r"`.
        let _ = parsed.binary;
    }

    if !tally.any_formatted {
        diag!(
            "{}: {}: no properly formatted checksum lines found",
            algo.program,
            quotef(&shown_name)
        );
    } else if !set.status_only {
        if tally.misformatted != 0 {
            diag!(
                "{}: WARNING: {} {}",
                algo.program,
                tally.misformatted,
                plural(
                    tally.misformatted,
                    "line is improperly formatted",
                    "lines are improperly formatted"
                )
            );
        }
        if tally.unreadable != 0 {
            diag!(
                "{}: WARNING: {} {}",
                algo.program,
                tally.unreadable,
                plural(
                    tally.unreadable,
                    "listed file could not be read",
                    "listed files could not be read"
                )
            );
        }
        if tally.mismatched != 0 {
            diag!(
                "{}: WARNING: {} {}",
                algo.program,
                tally.mismatched,
                plural(
                    tally.mismatched,
                    "computed checksum did NOT match",
                    "computed checksums did NOT match"
                )
            );
        }
        if set.ignore_missing && !tally.any_matched {
            diag!(
                "{}: {}: no file was verified",
                algo.program,
                quotef(&shown_name)
            );
        }
    }

    tally.ok(set.strict)
}

/// Cut the file into `getline` lines: each ends after its `\n`, and a final
/// line without one is still a line.
fn split_lines(text: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (i, &b) in text.iter().enumerate() {
        if b == b'\n' {
            if let Some(line) = text.get(start..=i) {
                lines.push(line);
            }
            start = i.saturating_add(1);
        }
    }
    // A final line without a terminator is still a line; a file that ended
    // *with* one leaves nothing here, which is why this is a length test and
    // not an unconditional push.
    let tail: &[u8] = text.get(start..).unwrap_or(&[]);
    if !tail.is_empty() {
        lines.push(tail);
    }
    lines
}

/// Upstream's two decrements: a trailing `\n`, then a trailing `\r`. Both are
/// removed, so a checksum file written on Windows verifies here.
fn strip_terminator(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A stand-in hash: 16 bytes of nothing, so the parser tests can be about
    /// the parser. Its width is MD5's, which is the narrower of the two shipped
    /// and so the harder case for `min_line_len`.
    struct Zero;
    impl Stream for Zero {
        fn update(&mut self, _data: &[u8]) {}
        fn finish(self: Box<Self>) -> Vec<u8> {
            vec![0u8; 16]
        }
    }
    const MD5: Algorithm = Algorithm {
        program: "md5sum",
        tag: "MD5",
        bits: 128,
        reference: "RFC 1321",
        new: || Box::new(Zero),
    };

    const H: &str = "b1946ac92492d2347c6235b4d2611184";

    fn parse(line: &str) -> Option<CheckLine> {
        Checker::new().split_3(&MD5, line.as_bytes())
    }

    // ---------------- the three formats ----------------

    #[test]
    fn standard_format() {
        let got = parse(&format!("{H}  a")).unwrap();
        assert_eq!(got.digest, H.as_bytes());
        assert_eq!(got.name, b"a");
        assert!(!got.binary);
    }

    #[test]
    fn binary_indicator() {
        let got = parse(&format!("{H} *a")).unwrap();
        assert_eq!(got.name, b"a");
        assert!(got.binary);
    }

    #[test]
    fn bsd_reversed_format() {
        let got = parse(&format!("{H} a")).unwrap();
        assert_eq!(got.name, b"a");
    }

    #[test]
    fn tagged_format() {
        let got = parse(&format!("MD5 (a) = {H}")).unwrap();
        assert_eq!(got.name, b"a");
        assert_eq!(got.digest, H.as_bytes());
    }

    #[test]
    fn tagged_name_may_contain_a_paren() {
        let got = parse(&format!("MD5 (a)b) = {H}")).unwrap();
        assert_eq!(got.name, b"a)b");
    }

    #[test]
    fn tagged_rejects_a_wrong_tag() {
        assert!(parse(&format!("SHA256 (a) = {H}")).is_none());
    }

    // ---------------- the anti-mixing rule ----------------

    #[test]
    fn a_reversed_line_locks_out_the_standard_one() {
        let mut c = Checker::new();
        assert!(c.split_3(&MD5, format!("{H} a").as_bytes()).is_some());
        // Now a standard line: its leading space would be read as an indicator.
        assert_eq!(c.bsd_reversed, Layout::BsdReversed);
        let got = c.split_3(&MD5, format!("{H}  b").as_bytes()).unwrap();
        // Latched reversed, so the space is part of the name, not an indicator.
        assert_eq!(got.name, b" b");
    }

    #[test]
    fn a_standard_line_locks_out_the_reversed_one() {
        let mut c = Checker::new();
        assert!(c.split_3(&MD5, format!("{H}  a").as_bytes()).is_some());
        assert_eq!(c.bsd_reversed, Layout::Standard);
        assert!(c.split_3(&MD5, format!("{H} a").as_bytes()).is_none());
    }

    // ---------------- escaping ----------------

    #[test]
    fn escaped_line_round_trips() {
        let name = b"we\nird";
        assert!(problematic(name));
        let shown = escape_name(name, true);
        assert_eq!(shown, b"we\\nird");
        assert_eq!(unescape_name(&shown).unwrap(), name);
    }

    #[test]
    fn escaped_check_line_parses() {
        let got = parse(&format!("\\{H}  we\\nird")).unwrap();
        assert_eq!(got.name, b"we\nird");
    }

    #[test]
    fn a_backslash_pair_is_one_backslash() {
        let got = parse(&format!("\\{H}  back\\\\slash")).unwrap();
        assert_eq!(got.name, b"back\\slash");
    }

    #[test]
    fn carriage_return_escapes_too() {
        assert_eq!(escape_name(b"a\rb", true), b"a\\rb");
        assert_eq!(unescape_name(b"a\\rb").unwrap(), b"a\rb");
    }

    #[test]
    fn a_trailing_backslash_is_refused() {
        assert!(unescape_name(b"name\\").is_none());
    }

    #[test]
    fn an_unknown_escape_is_refused() {
        assert!(unescape_name(b"na\\me").is_none());
    }

    #[test]
    fn an_embedded_nul_is_refused() {
        assert!(unescape_name(b"na\0me").is_none());
    }

    #[test]
    fn unescaped_lines_keep_their_backslashes() {
        // No leading `\`, so `\n` here is two literal bytes of the name.
        let got = parse(&format!("{H}  a\\nb")).unwrap();
        assert_eq!(got.name, b"a\\nb");
    }

    // ---------------- rejections ----------------

    #[test]
    fn short_line_is_refused() {
        assert!(parse("abc  d").is_none());
    }

    #[test]
    fn non_hex_digest_is_refused() {
        assert!(parse(&format!("{}  a", "z".repeat(32))).is_none());
    }

    #[test]
    fn wrong_length_digest_is_refused() {
        assert!(parse(&format!("{}  a", "a".repeat(31))).is_none());
        assert!(parse(&format!("{}  a", "a".repeat(33))).is_none());
    }

    #[test]
    fn digest_with_no_name_is_refused() {
        assert!(parse(H).is_none());
        assert!(parse(&format!("{H} ")).is_none());
    }

    #[test]
    fn leading_blanks_are_skipped() {
        let got = parse(&format!("   {H}  a")).unwrap();
        assert_eq!(got.name, b"a");
    }

    #[test]
    fn trailing_blanks_belong_to_the_name() {
        let got = parse(&format!("{H}  a  ")).unwrap();
        assert_eq!(got.name, b"a  ");
    }

    #[test]
    fn uppercase_digest_still_matches() {
        assert!(hex_equal(b"00FF", &[0x00, 0xff]));
        assert!(hex_equal(b"00ff", &[0x00, 0xff]));
        assert!(!hex_equal(b"00fe", &[0x00, 0xff]));
    }

    // ---------------- rendering ----------------

    #[test]
    fn plain_render() {
        assert_eq!(render(&MD5, b"a", false, &[0xab], false, false), b"ab  a\n");
    }

    #[test]
    fn binary_render() {
        assert_eq!(render(&MD5, b"a", true, &[0xab], false, false), b"ab *a\n");
    }

    #[test]
    fn tagged_render() {
        assert_eq!(
            render(&MD5, b"a", true, &[0xab], true, false),
            b"MD5 (a) = ab\n"
        );
    }

    #[test]
    fn escaped_render() {
        assert_eq!(
            render(&MD5, b"we\nird", false, &[0xab], false, false),
            b"\\ab  we\\nird\n"
        );
    }

    #[test]
    fn zero_render_does_not_escape() {
        assert_eq!(
            render(&MD5, b"we\nird", false, &[0xab], false, true),
            b"ab  we\nird\0"
        );
    }

    // ---------------- line splitting ----------------

    #[test]
    fn split_lines_keeps_terminators_and_the_last_line() {
        assert_eq!(split_lines(b"a\nb\nc"), vec![&b"a\n"[..], b"b\n", b"c"]);
        assert_eq!(split_lines(b"a\n"), vec![&b"a\n"[..]]);
        assert_eq!(split_lines(b""), Vec::<&[u8]>::new());
    }

    #[test]
    fn crlf_is_stripped() {
        assert_eq!(strip_terminator(b"a\r\n"), b"a");
        assert_eq!(strip_terminator(b"a\n"), b"a");
        assert_eq!(strip_terminator(b"a"), b"a");
    }

    // ---------------- options ----------------

    const P: Program = Program::new("md5sum", 1);

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn settings(words: &[&str]) -> Settings {
        match parse_args(P, &args(words)).unwrap() {
            Request::Run(s) => s,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn tag_implies_binary() {
        let s = settings(&["--tag"]);
        assert!(s.tag);
        assert_eq!(s.binary, Some(true));
    }

    #[test]
    fn tag_then_text_is_an_error_but_text_then_tag_is_not() {
        assert!(validate(P, &settings(&["--tag", "--text"])).is_err());
        assert!(validate(P, &settings(&["--text", "--tag"])).is_ok());
    }

    #[test]
    fn status_warn_and_quiet_are_last_wins() {
        let s = settings(&["--status", "-w", "-c"]);
        assert!(s.warn && !s.status_only && !s.quiet);
        let s = settings(&["-w", "--status", "-c"]);
        assert!(s.status_only && !s.warn && !s.quiet);
        let s = settings(&["--status", "--quiet", "-c"]);
        assert!(s.quiet && !s.status_only && !s.warn);
    }

    #[test]
    fn check_only_options_need_check() {
        for word in [
            "--ignore-missing",
            "--status",
            "--warn",
            "--quiet",
            "--strict",
        ] {
            assert!(
                validate(P, &settings(&[word])).is_err(),
                "{word} should require -c"
            );
            assert!(validate(P, &settings(&[word, "-c"])).is_ok());
        }
    }

    #[test]
    fn validation_order_is_upstreams() {
        // `--tag --text -c` violates three rules; the --text one is reported.
        let e = validate(P, &settings(&["--tag", "--text", "-c"])).unwrap_err();
        assert_eq!(e.sentence, "--tag does not support --text mode");
    }

    #[test]
    fn zero_and_check_conflict() {
        let e = validate(P, &settings(&["-z", "-c"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "the --zero option is not supported when verifying checksums"
        );
    }

    #[test]
    fn binary_and_check_conflict() {
        let e = validate(P, &settings(&["-b", "-c"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "the --binary and --text options are meaningless when verifying checksums"
        );
    }

    #[test]
    fn operands_are_bytes() {
        // The whole point: a name that is not valid UTF-8 survives.
        let s = settings(&["-c"]);
        assert!(s.files.is_empty());
        let raw = os_from_bytes(b"na\xffme");
        let argv = vec![OsString::from("-b"), raw.clone()];
        let Request::Run(s) = parse_args(P, &argv).unwrap() else {
            panic!("expected Run")
        };
        assert_eq!(s.files, vec![raw]);
    }

    #[test]
    fn help_names_the_algorithm() {
        let text = help_text(&MD5);
        assert!(text.contains("Print or check MD5 (128-bit) checksums."));
        assert!(text.contains("described in RFC 1321."));
    }

    #[test]
    fn plurals_agree() {
        assert_eq!(plural(1, "a", "b"), "a");
        assert_eq!(plural(0, "a", "b"), "b");
        assert_eq!(plural(2, "a", "b"), "b");
    }
}
