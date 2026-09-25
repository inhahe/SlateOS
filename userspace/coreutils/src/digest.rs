//! `md5sum`, `sha1sum`, `sha224sum`, `sha256sum`, `sha384sum`, `sha512sum`,
//! `b2sum` and `cksum`, which upstream are one program.
//!
//! A port of GNU coreutils 9.4's `src/digest.c` — the single file that is
//! compiled nine times, once per `HASH_ALGO_*`, to produce those eight and
//! `sum`. Every one of them is built on this module; each bin is a
//! [`Build`] and a call to [`main`]. (`sum`, `HASH_ALGO_SUM`, keeps a driver
//! of its own in its bin — it shares none of the check machinery — and shares
//! its two checksums with `cksum` through [`crate::sum`].)
//!
//! # Why this is a module and not eight `main`s
//!
//! Because the interesting half of these programs is not the hash. It is
//! `--check`, and `--check` is a *parser* for output this program wrote
//! earlier — three formats, an escaping convention, and a rule about which of
//! the formats may appear in one file. Several hand-written copies of that
//! would disagree, and a disagreement here is not cosmetic: `sha256sum -c`
//! reporting `OK` for a line it misread is the exact failure the utility exists
//! to prevent. The hashes themselves live in the workspace's hash crates —
//! `md5`, `sha1`, `sha2`, `blake2`, `sm3` — and the two legacy checksums in
//! [`crate::sum`] and [`crate::cksum`]; [`Algo::stream`] is the one place that
//! names them.
//!
//! # The three builds
//!
//! Upstream's `#if` blocks split into three kinds of program, and [`Build`]
//! is which one is running:
//!
//! * **one algorithm at its full width** — `md5sum` through `sha512sum`;
//! * **`b2sum`**, BLAKE2b at a width `-l` chooses, which a checksum line may
//!   also state: `BLAKE2b-256 (f) = …`, or an untagged digest whose length says
//!   it;
//! * **`cksum`**, any of eleven algorithms. `-a` picks one — the CRC by default
//!   — and without `-a`, `--check` takes each line's algorithm from its tag.
//!   Its output is tagged by default (`--untagged` for the other form), and it
//!   alone has `--base64` and `--raw`.
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
//! and rejects every later line of the other kind — for the rest of the run,
//! across every check file named, since the latch is a file-scope `static`
//! nothing resets. This port does the same, in [`State::bsd_reversed`]. (It
//! reset the latch per check file until 2026-09-25, so `md5sum -c A B` with `A`
//! reversed and `B` standard verified both, where upstream refuses every line
//! of `B`.)
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
//! # Upstream's quirks, kept
//!
//! These are all visible, all measured against 9.4, and all reproduced rather
//! than tidied, because a checksum file is an interchange format and a line
//! one implementation accepts and the other refuses is a file that verifies on
//! one machine only.
//!
//! * **The state `--check` works in is the run's, not the line's.** `cksum`'s
//!   algorithm and every build's digest width are globals upstream, which
//!   a tagged line sets and nothing resets. So after `SHA256-128 (f) = …` in a
//!   `cksum -a sha256 -c` file, an *untagged* line needs 32 hex digits, not 64;
//!   and the `improperly formatted %s checksum line` warning names whichever
//!   algorithm the last tag chose — `CRC` before any line has chosen one.
//! * **A tagged line may truncate any algorithm.** `cksum -a sha256 -c`
//!   accepts `SHA256-128 (f) = <32 hex digits>` and compares the first 16
//!   bytes of the SHA-256.
//! * **In the variable-width builds the byte after the tag is skipped
//!   unread** — upstream overwrites it with a NUL to end the tag — so `b2sum
//!   -c` accepts `BLAKE2bX (f) = …` as readily as `BLAKE2b (f) = …`.
//! * **A tagged line's length is read with C's prefix rule** (`strtoumax`,
//!   base 0), so `BLAKE2b-0x100` is 256 bits and `BLAKE2b-0400` is too.
//! * **`-l 0` means the default width**, because 0 is also how upstream spells
//!   "not given".
//!
//! # Deliberate differences from GNU
//!
//! * **`-b`/`-t` do nothing but set the indicator byte**, exactly as on
//!   GNU/Linux, where `O_BINARY` is 0. Upstream's DOS-only paths
//!   (`xset_binary_mode`, the `isatty` test that makes `-b` mean "binary unless
//!   stdin is a terminal") are not reachable on any platform we target and are
//!   not reproduced.
//! * **`cksum --debug` prints nothing.** Upstream's x86 build reports whether
//!   its PCLMUL CRC is in use; this port has only the table CRC, so it behaves
//!   as upstream built without `USE_PCLMUL_CRC32` does. See [`crate::cksum`].
//! * **`--help` omits the GNU project's `Report bugs to:` block**, as every
//!   converted utility here does.
//! * **No `fadvise`.** `FADVISE_SEQUENTIAL` is a hint; its absence changes
//!   throughput, never output.
//!
//! # How this is tested
//!
//! `scripts/digest-diff.sh` builds every bin for Linux inside WSL and runs them
//! against GNU coreutils 9.4's case by case, and `scripts/cksum-diff.sh` does
//! the same for what only `cksum` has — the same answer `cmp`, `tee`, `echo`,
//! `du`, `find` and `ls` use (`design-decisions.md` §374). The unit tests at
//! the bottom of this file cover the line parser, which is the part a harness
//! reaches only through whole files.

use crate::basenc::{base64_encode, is_base64};
use crate::diag;
use crate::errmsg::strerror;
use crate::getopt::{self, Opt, Program, Takes};
use crate::quote::{os_bytes, os_from_bytes, quote, quotef};
// Imported as a module rather than by item: this file already has a `Stream`,
// the hash trait, so `stdfd::Stream` has to stay spelled out.
use crate::stdfd;
use crate::xnum::{self, Status};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::process::ExitCode;

// ------------------------------------------------------------- the algorithms ---

/// A hash being fed a file, one chunk at a time.
///
/// Streaming rather than `fn(&[u8]) -> Vec<u8>`, because the versions these
/// programs replaced read the whole file into memory first and that is not a
/// detail: `md5sum` on a disk image is a normal thing to do, and an allocation
/// the size of the input is a way to be killed by the OOM path rather than to
/// answer.
pub trait Stream {
    /// Absorb the next bytes of the message.
    fn update(&mut self, data: &[u8]);
    /// Finish, yielding the raw digest. For the three legacy checksums that is
    /// the checksum as big-endian bytes: two for BSD and System V, four for
    /// the CRC.
    fn finish(self: Box<Self>) -> Vec<u8>;
}

/// One of the eleven digests `cksum -a` names.
///
/// Declared in upstream's `algorithm_args` order, which is observable:
/// `cksum -a nope` lists the valid names in it, and `cksum --check` refuses
/// every algorithm up to and including `crc` by comparing positions
/// (`algo_tag <= crc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    Bsd,
    Sysv,
    Crc,
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Blake2b,
    Sm3,
}

impl Algo {
    /// Every algorithm, in `algorithm_args` order.
    pub const ALL: [Algo; 11] = [
        Algo::Bsd,
        Algo::Sysv,
        Algo::Crc,
        Algo::Md5,
        Algo::Sha1,
        Algo::Sha224,
        Algo::Sha256,
        Algo::Sha384,
        Algo::Sha512,
        Algo::Blake2b,
        Algo::Sm3,
    ];

    /// `algorithm_args`: the name `cksum -a` takes.
    #[must_use]
    pub const fn arg(self) -> &'static str {
        match self {
            Algo::Bsd => "bsd",
            Algo::Sysv => "sysv",
            Algo::Crc => "crc",
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha224 => "sha224",
            Algo::Sha256 => "sha256",
            Algo::Sha384 => "sha384",
            Algo::Sha512 => "sha512",
            Algo::Blake2b => "blake2b",
            Algo::Sm3 => "sm3",
        }
    }

    /// `algorithm_tags`: the name inside a tagged line, and in `improperly
    /// formatted %s checksum line`.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Algo::Bsd => "BSD",
            Algo::Sysv => "SYSV",
            Algo::Crc => "CRC",
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA1",
            Algo::Sha224 => "SHA224",
            Algo::Sha256 => "SHA256",
            Algo::Sha384 => "SHA384",
            Algo::Sha512 => "SHA512",
            Algo::Blake2b => "BLAKE2b",
            Algo::Sm3 => "SM3",
        }
    }

    /// `algorithm_bits`: the full digest width.
    #[must_use]
    pub const fn bits(self) -> usize {
        match self {
            Algo::Bsd | Algo::Sysv => 16,
            Algo::Crc => 32,
            Algo::Md5 => 128,
            Algo::Sha1 => 160,
            Algo::Sha224 => 224,
            Algo::Sha256 | Algo::Sm3 => 256,
            Algo::Sha384 => 384,
            Algo::Sha512 | Algo::Blake2b => 512,
        }
    }

    /// The three `cksum --check` cannot read: upstream's `algo_tag <= crc`.
    const fn is_legacy(self) -> bool {
        matches!(self, Algo::Bsd | Algo::Sysv | Algo::Crc)
    }

    /// A fresh hash. `out_len` is BLAKE2b's digest width in bytes, clamped to
    /// the 1..=64 it accepts; every other algorithm has one width and ignores
    /// it.
    #[must_use]
    pub fn stream(self, out_len: usize) -> Box<dyn Stream> {
        match self {
            Algo::Bsd => Box::new(BsdStream(crate::sum::Bsd::default())),
            Algo::Sysv => Box::new(SysvStream(crate::sum::Sysv::default())),
            Algo::Crc => Box::new(CrcStream(crate::cksum::Crc::default())),
            Algo::Md5 => Box::new(Md5Stream(md5::Md5::new())),
            Algo::Sha1 => Box::new(Sha1Stream(sha1::Sha1::new())),
            Algo::Sha224 => Box::new(Sha224Stream(sha2::Sha224::new())),
            Algo::Sha256 => Box::new(Sha256Stream(sha2::Sha256::new())),
            Algo::Sha384 => Box::new(Sha384Stream(sha2::Sha384::new())),
            Algo::Sha512 => Box::new(Sha512Stream(sha2::Sha512::new())),
            Algo::Blake2b => Box::new(Blake2bStream(blake2::Blake2b::new(
                out_len.clamp(1, blake2::OUT_BYTES),
            ))),
            Algo::Sm3 => Box::new(Sm3Stream(sm3::Sm3::new())),
        }
    }
}

/// A hash crate's type under this module's trait: `update` passes through,
/// and `finalize`, which takes the hasher by value, is what `Box<Self>` unwraps
/// to. A newtype rather than an `impl Stream for md5::Md5` because the types
/// belong to other crates.
macro_rules! hash_stream {
    ($name:ident, $inner:ty) => {
        struct $name($inner);

        impl Stream for $name {
            fn update(&mut self, data: &[u8]) {
                self.0.update(data);
            }

            fn finish(self: Box<Self>) -> Vec<u8> {
                self.0.finalize().to_vec()
            }
        }
    };
}

hash_stream!(Md5Stream, md5::Md5);
hash_stream!(Sha1Stream, sha1::Sha1);
hash_stream!(Sha224Stream, sha2::Sha224);
hash_stream!(Sha256Stream, sha2::Sha256);
hash_stream!(Sha384Stream, sha2::Sha384);
hash_stream!(Sha512Stream, sha2::Sha512);
hash_stream!(Sm3Stream, sm3::Sm3);

/// BLAKE2b, whose width is chosen at run time.
///
/// `None` only if [`blake2::Blake2b::new`] refused the width, which
/// [`Algo::stream`]'s clamp makes impossible; it is an `Option` because the
/// constructor's is.
struct Blake2bStream(Option<blake2::Blake2b>);

impl Stream for Blake2bStream {
    fn update(&mut self, data: &[u8]) {
        if let Some(state) = &mut self.0 {
            state.update(data);
        }
    }

    fn finish(self: Box<Self>) -> Vec<u8> {
        // Empty only in the unreachable `None` case above, where there is no
        // digest to give; an empty one matches no recorded checksum and prints
        // as nothing, rather than as a plausible wrong one.
        self.0
            .map(|state| state.finalize().as_bytes().to_vec())
            .unwrap_or_default()
    }
}

struct BsdStream(crate::sum::Bsd);

impl Stream for BsdStream {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finish(self: Box<Self>) -> Vec<u8> {
        self.0.checksum().to_be_bytes().to_vec()
    }
}

struct SysvStream(crate::sum::Sysv);

impl Stream for SysvStream {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finish(self: Box<Self>) -> Vec<u8> {
        self.0.checksum().to_be_bytes().to_vec()
    }
}

struct CrcStream(crate::cksum::Crc);

impl Stream for CrcStream {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finish(self: Box<Self>) -> Vec<u8> {
        self.0.finish().to_be_bytes().to_vec()
    }
}

// ----------------------------------------------------------------- the builds ---

/// Which of upstream's builds of `digest.c` is running: everything its
/// `HASH_ALGO_*` defines decide about the *program* rather than the hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    Md5sum,
    Sha1sum,
    Sha224sum,
    Sha256sum,
    Sha384sum,
    Sha512sum,
    B2sum,
    Cksum,
}

impl Build {
    /// `PROGRAM_NAME`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Build::Md5sum => "md5sum",
            Build::Sha1sum => "sha1sum",
            Build::Sha224sum => "sha224sum",
            Build::Sha256sum => "sha256sum",
            Build::Sha384sum => "sha384sum",
            Build::Sha512sum => "sha512sum",
            Build::B2sum => "b2sum",
            Build::Cksum => "cksum",
        }
    }

    /// The algorithm the build hashes with: its only one, or `cksum`'s default,
    /// the CRC.
    #[must_use]
    pub const fn algo(self) -> Algo {
        match self {
            Build::Md5sum => Algo::Md5,
            Build::Sha1sum => Algo::Sha1,
            Build::Sha224sum => Algo::Sha224,
            Build::Sha256sum => Algo::Sha256,
            Build::Sha384sum => Algo::Sha384,
            Build::Sha512sum => Algo::Sha512,
            Build::B2sum => Algo::Blake2b,
            Build::Cksum => Algo::Crc,
        }
    }

    /// `DIGEST_REFERENCE`, which `--help` ends by citing. `cksum` has none.
    const fn reference(self) -> &'static str {
        match self {
            Build::Md5sum => "RFC 1321",
            Build::Sha1sum => "FIPS-180-1",
            Build::Sha224sum => "RFC 3874",
            Build::Sha256sum | Build::Sha384sum | Build::Sha512sum => "FIPS-180-2",
            Build::B2sum => "RFC 7693",
            Build::Cksum => "",
        }
    }

    /// `HASH_ALGO_BLAKE2 || HASH_ALGO_CKSUM`: the builds with `-l`, whose
    /// digest width is a variable and whose check lines may state one.
    const fn variable_width(self) -> bool {
        matches!(self, Build::B2sum | Build::Cksum)
    }

    /// Upstream's `short_opts`, verbatim.
    const fn short_options(self) -> &'static str {
        match self {
            Build::Cksum => "a:l:bctwz",
            Build::B2sum => "l:bctwz",
            _ => "bctwz",
        }
    }

    /// Upstream's `long_options`, in its order — which is observable, because
    /// glibc lists the candidates for an ambiguous prefix in table order.
    /// Measured: `md5sum --=x` names them `'--check' '--ignore-missing'
    /// '--quiet' '--status' '--warn' '--strict' '--tag' '--zero' '--binary'
    /// '--text' '--help' '--version'`.
    const fn long_options(self) -> &'static [(&'static str, Takes)] {
        match self {
            Build::Cksum => CKSUM_LONG_OPTIONS,
            Build::B2sum => B2SUM_LONG_OPTIONS,
            _ => LONG_OPTIONS,
        }
    }
}

// -------------------------------------------------------------- the options ---

/// The single-algorithm builds' table.
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

/// `b2sum`'s: `--length` first, under `#if HASH_ALGO_BLAKE2 || HASH_ALGO_CKSUM`.
const B2SUM_LONG_OPTIONS: &[(&str, Takes)] = &[
    ("length", Takes::Required),
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

/// `cksum`'s: `b2sum`'s, with the five `#if HASH_ALGO_CKSUM` entries between
/// `--zero` and `--binary`. `-b` and `-t` are still accepted, though `--help`
/// no longer lists them.
const CKSUM_LONG_OPTIONS: &[(&str, Takes)] = &[
    ("length", Takes::Required),
    ("check", Takes::Nothing),
    ("ignore-missing", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("status", Takes::Nothing),
    ("warn", Takes::Nothing),
    ("strict", Takes::Nothing),
    ("tag", Takes::Nothing),
    ("zero", Takes::Nothing),
    ("algorithm", Takes::Required),
    ("base64", Takes::Nothing),
    ("debug", Takes::Nothing),
    ("raw", Takes::Nothing),
    ("untagged", Takes::Nothing),
    ("binary", Takes::Nothing),
    ("text", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `algorithm_args` paired with `algorithm_types`, for `-a`.
const ALGORITHM_ARGS: [(&str, Algo); 11] = [
    ("bsd", Algo::Bsd),
    ("sysv", Algo::Sysv),
    ("crc", Algo::Crc),
    ("md5", Algo::Md5),
    ("sha1", Algo::Sha1),
    ("sha224", Algo::Sha224),
    ("sha256", Algo::Sha256),
    ("sha384", Algo::Sha384),
    ("sha512", Algo::Sha512),
    ("blake2b", Algo::Blake2b),
    ("sm3", Algo::Sm3),
];

/// `BLAKE2B_MAX_LEN * 8`: the widest digest `-l` may ask for, in bits.
const BLAKE2B_MAX_BITS: u64 = 512;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    check: bool,
    /// Upstream's `int binary`, whose third state is load-bearing: `-1` means
    /// "neither `-b` nor `-t` was given", and the *only* thing that state does
    /// on a POSIX host is decide whether `--binary and --text are meaningless
    /// when verifying` fires. `Option` rather than a `bool` plus a flag so the
    /// two cannot drift apart.
    binary: Option<bool>,
    /// `prefix_tag`: false to start with, except in `cksum`, whose output is
    /// tagged unless `--untagged` says otherwise.
    tag: bool,
    zero: bool,
    status_only: bool,
    warn: bool,
    quiet: bool,
    strict: bool,
    ignore_missing: bool,
    /// `cksum -a`. `None` is upstream's `algorithm_specified = false`.
    algorithm: Option<Algo>,
    /// `cksum --base64`.
    base64: bool,
    /// `cksum --raw`.
    raw: bool,
    /// `cksum --debug`, accepted and silent: see the module docs.
    debug: bool,
    /// `-l`, in bits: upstream's `digest_length` as the option loop leaves it,
    /// with 0 for "not given" — so `-l 0` is also "not given".
    length: u64,
    /// `-l`'s argument as typed, for the two messages that quote it.
    length_text: Vec<u8>,
    files: Vec<OsString>,
}

impl Settings {
    fn new(build: Build) -> Self {
        Settings {
            check: false,
            binary: None,
            tag: build == Build::Cksum,
            zero: false,
            status_only: false,
            warn: false,
            quiet: false,
            strict: false,
            ignore_missing: false,
            algorithm: None,
            base64: false,
            raw: false,
            debug: false,
            length: 0,
            length_text: Vec::new(),
            files: Vec::new(),
        }
    }
}

/// Read the command line. Upstream's option loop, one arm per `case`.
///
/// The three-way exclusion between `--status`, `-w` and `--quiet` is upstream's
/// and is *last-wins* rather than an error: each arm clears the other two, so
/// `--status -w` warns and `-w --status` is silent.
///
/// `-a` and `-l` are checked where they are met, because upstream exits from
/// inside the loop for them: `b2sum -l 7 --nope` complains about the length and
/// never reaches the unknown option.
fn parse_args(build: Build, program: Program, args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut set = Settings::new(build);
    for item in program.parse(args, build.short_options(), build.long_options()) {
        match item? {
            Opt::Short(b'a', Some(value)) | Opt::Long("algorithm", Some(value)) => {
                // `XARGMATCH_EXACT`: no abbreviations, so `-a sha` is refused
                // rather than read as `sha1`.
                set.algorithm = Some(program.argmatch_exact(
                    &os_bytes(&value),
                    "--algorithm",
                    &ALGORITHM_ARGS,
                )?);
            }
            Opt::Long("debug", _) => set.debug = true,
            Opt::Short(b'l', Some(value)) | Opt::Long("length", Some(value)) => {
                let text = os_bytes(&value);
                // `xdectoumax (optarg, 0, UINTMAX_MAX, "", _("invalid length"), 0)`
                let length = xnum::xdectoumax(&text, 0, u64::MAX, Some(b""), "invalid length")
                    .map_err(|message| program.usage(message))?;
                if !length.is_multiple_of(8) {
                    return Err(program.usage(format!(
                        "invalid length: {}\n{}: length is not a multiple of 8",
                        quote(&text),
                        build.name()
                    )));
                }
                set.length = length;
                set.length_text = text.into_owned();
            }
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
            Opt::Long("base64", _) => set.base64 = true,
            Opt::Long("raw", _) => set.raw = true,
            Opt::Long("untagged", _) => set.tag = false,
            // `case TAG_OPTION: prefix_tag = true; binary = 1;` — the second
            // assignment is why `--tag --text` is an error but `--text --tag`
            // is not, and why `cksum --tag -c` is refused as `--binary` with
            // `--check`.
            Opt::Long("tag", _) => {
                set.tag = true;
                set.binary = Some(true);
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => set.files.push(word.clone()),
            // Every entry of the tables is handled above; an unknown option
            // arrives as an `Err` from `parse`, and an option that requires a
            // value always has one.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    Ok(Request::Run(set))
}

/// The consistency checks after the loop, in upstream's order — which is
/// observable, since only the first to fire is printed. Some end the run with
/// a bare message (`error (EXIT_FAILURE, …)`); the rest add upstream's
/// `usage (EXIT_FAILURE)` referral.
fn validate(build: Build, program: Program, set: &Settings) -> Result<(), getopt::Error> {
    let algo = set.algorithm.unwrap_or(build.algo());
    if build.variable_width() {
        if build == Build::Cksum && set.length != 0 && algo != Algo::Blake2b {
            return Err(
                program.usage("--length is only supported with --algorithm=blake2b".to_string())
            );
        }
        if set.length > BLAKE2B_MAX_BITS {
            return Err(program.usage(format!(
                "invalid length: {}\n{}: maximum digest length for {} is {BLAKE2B_MAX_BITS} bits",
                quote(&set.length_text),
                build.name(),
                quote(algo.tag().as_bytes()),
            )));
        }
    }
    if build == Build::Cksum {
        if algo.is_legacy() && set.check && set.algorithm.is_some() {
            return Err(program
                .usage("--check is not supported with --algorithm={bsd,sysv,crc}".to_string()));
        }
        if set.base64 && set.raw {
            return Err(
                program.usage_referring("--base64 and --raw are mutually exclusive".to_string())
            );
        }
    }
    // `if (prefix_tag && !binary)`: `!binary` is false for the unset `-1`, so
    // this needs `-t` *explicitly*, after any `--tag`.
    if set.tag && set.binary == Some(false) {
        return Err(program.usage_referring(
            if build == Build::Cksum {
                "--text mode is only supported with --untagged"
            } else {
                "--tag does not support --text mode"
            }
            .to_string(),
        ));
    }
    if set.zero && set.check {
        return Err(program.usage_referring(
            "the --zero option is not supported when verifying checksums".to_string(),
        ));
    }
    // `#if !HASH_ALGO_CKSUM`: tagged is `cksum`'s default, so there it cannot
    // be meaningless.
    if build != Build::Cksum && set.tag && set.check {
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

/// The part of `--help` every build shares: the options that check.
const CHECK_OPTIONS_HELP: &str = "
The following five options are useful only when verifying checksums:
      --ignore-missing  don't fail or report status for missing files
      --quiet           don't print OK for each successfully verified file
      --status          don't output anything, status code shows success
      --strict          exit non-zero for improperly formatted checksum lines
  -w, --warn            warn about improperly formatted checksum lines

";

/// The single-algorithm builds' and `b2sum`'s closing paragraphs.
fn reference_help(build: Build) -> String {
    format!(
        "
The sums are computed as described in {}.
When checking, the input should be a former output of this program.
The default mode is to print a line with: checksum, a space,
a character indicating input mode ('*' for binary, ' ' for text
or where binary is insignificant), and name for each FILE.

Note: There is no difference between binary mode and text mode on GNU systems.
",
        build.reference()
    )
}

/// GNU's `--help`, minus the project's `Report bugs to:` block.
///
/// The `-b`/`-t` wordings are the `O_BINARY == 0` arms of upstream's `if`,
/// which are the ones GNU/Linux prints.
fn help_text(build: Build) -> String {
    let name = build.name();
    let algo = build.algo();
    match build {
        Build::Cksum => format!(
            "\
Usage: {name} [OPTION]... [FILE]...
Print or verify checksums.
By default use the 32 bit CRC algorithm.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -a, --algorithm=TYPE  select the digest type to use.  See DIGEST below.
      --base64          emit base64-encoded digests, not hexadecimal
  -c, --check           read checksums from the FILEs and check them
  -l, --length=BITS     digest length in bits; must not exceed the max for
                          the blake2 algorithm and must be a multiple of 8
      --raw             emit a raw binary digest, not hexadecimal
      --tag             create a BSD-style checksum (the default)
      --untagged        create a reversed style checksum, without digest type
  -z, --zero            end each output line with NUL, not newline,
                          and disable file name escaping
{CHECK_OPTIONS_HELP}      --debug           indicate which implementation used
      --help        display this help and exit
      --version     output version information and exit

DIGEST determines the digest algorithm and default output format:
  sysv      (equivalent to sum -s)
  bsd       (equivalent to sum -r)
  crc       (equivalent to cksum)
  md5       (equivalent to md5sum)
  sha1      (equivalent to sha1sum)
  sha224    (equivalent to sha224sum)
  sha256    (equivalent to sha256sum)
  sha384    (equivalent to sha384sum)
  sha512    (equivalent to sha512sum)
  blake2b   (equivalent to b2sum)
  sm3       (only available through cksum)

When checking, the input should be a former output of this program,
or equivalent standalone program.
"
        ),
        Build::B2sum => format!(
            "\
Usage: {name} [OPTION]... [FILE]...
Print or check {tag} ({bits}-bit) checksums.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -b, --binary          read in binary mode
  -c, --check           read checksums from the FILEs and check them
  -l, --length=BITS     digest length in bits; must not exceed the max for
                          the blake2 algorithm and must be a multiple of 8
      --tag             create a BSD-style checksum
  -t, --text            read in text mode (default)
  -z, --zero            end each output line with NUL, not newline,
                          and disable file name escaping
{CHECK_OPTIONS_HELP}      --help        display this help and exit
      --version     output version information and exit
{reference}",
            tag = algo.tag(),
            bits = algo.bits(),
            reference = reference_help(build),
        ),
        _ => format!(
            "\
Usage: {name} [OPTION]... [FILE]...
Print or check {tag} ({bits}-bit) checksums.

With no FILE, or when FILE is -, read standard input.
  -b, --binary          read in binary mode
  -c, --check           read checksums from the FILEs and check them
      --tag             create a BSD-style checksum
  -t, --text            read in text mode (default)
  -z, --zero            end each output line with NUL, not newline,
                          and disable file name escaping
{CHECK_OPTIONS_HELP}      --help        display this help and exit
      --version     output version information and exit
{reference}",
            tag = algo.tag(),
            bits = algo.bits(),
            reference = reference_help(build),
        ),
    }
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
    /// The digest, and the byte count the legacy checksums print beside it.
    Ok {
        digest: Vec<u8>,
        length: u64,
    },
    /// `ENOENT` under `--ignore-missing`: not an error, not a result.
    Missing,
    Failed,
}

/// What reading one operand came to: [`feed_file`]'s answer.
pub enum Fed {
    /// Every byte reached the sink.
    Ok,
    /// `ENOENT`, and the caller asked for that to be quiet (`--ignore-missing`).
    Missing,
    /// It could not be opened or read. The diagnostic is already written.
    Failed,
}

/// Upstream's `digest_file`, minus the digest: open NAME, hand every byte of
/// it to `sink` in 64 KiB pieces, and say how that went.
///
/// `-` is standard input, and is *not* a file called `-`: that is POSIX for
/// these utilities, unlike `tee`. A failure is diagnosed here, as
/// `PROGRAM: NAME: strerror`, because upstream does and because a caller like
/// `--check` goes on to print a *second*, different line about the same file.
///
/// Public because `sum` is upstream's `digest.c` too, compiled with
/// `HASH_ALGO_SUM`, with a driver of its own.
pub fn feed_file(
    program: &str,
    name: &[u8],
    ignore_missing: bool,
    read_stdin: &mut bool,
    sink: &mut dyn FnMut(&[u8]),
) -> Fed {
    let mut buf = vec![0u8; READ_CHUNK];

    let mut feed = |src: &mut dyn Read| -> io::Result<()> {
        loop {
            match src.read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(n) => sink(buf.get(..n).unwrap_or(&[])),
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
                    return Fed::Missing;
                }
                diag!("{program}: {}: {}", quotef(name), strerror(&e));
                return Fed::Failed;
            }
        }
    };

    match result {
        Ok(()) => Fed::Ok,
        Err(e) => {
            diag!("{program}: {}: {}", quotef(name), strerror(&e));
            Fed::Failed
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

/// Upstream's `hex_equal`: compare the recorded text against the leading
/// `recorded.len() / 2` bytes of the computed digest, ignoring the case of the
/// recorded hex digits.
///
/// The *leading* bytes, because a tagged line may state a width narrower than
/// the algorithm's (`SHA256-128`), and upstream then compares only that many.
fn hex_equal(recorded: &[u8], computed: &[u8]) -> bool {
    let Some(head) = computed.get(..recorded.len() / 2) else {
        return false;
    };
    recorded.len().is_multiple_of(2)
        && to_hex(head)
            .iter()
            .zip(recorded)
            .all(|(&want, &got)| want == got.to_ascii_lowercase())
}

/// `BASE64_LENGTH (n)`: characters in the padded encoding of `n` bytes.
const fn base64_length(bytes: usize) -> usize {
    bytes.div_ceil(3).saturating_mul(4)
}

// ------------------------------------------------------------- the run state ---

/// One parsed line of a checksum file.
#[derive(Debug, PartialEq, Eq)]
struct CheckLine {
    /// The digest *as written*, hex or base64 text, so its case survives for
    /// [`hex_equal`] to ignore.
    digest: Vec<u8>,
    /// The `*`/` ` indicator, absent in the tagged and reversed formats.
    binary: bool,
    name: Vec<u8>,
}

/// Which of the two untagged layouts the run is using, latched on the first
/// line that settles it. See the module docs for why mixing them is refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Layout {
    #[default]
    Undecided,
    /// `<hex>  NAME` — a type indicator byte after the digest.
    Standard,
    /// `<hex> NAME` — no indicator; the name starts immediately.
    BsdReversed,
}

/// Upstream's file-scope globals: what the options settled, and what
/// `--check` goes on changing line by line. One per run, **not** one per check
/// file — see the module docs' "quirks, kept".
struct State {
    build: Build,
    /// `cksum_algorithm`: fixed in every build but `cksum`, where `-a` sets it
    /// and, without `-a`, each tagged check line does.
    algo: Algo,
    /// `algorithm_specified`: `cksum -a` was given.
    algorithm_specified: bool,
    /// `digest_length`, in bits.
    digest_length: usize,
    /// `digest_hex_bytes`: hex digits in a digest of `digest_length` bits.
    digest_hex_bytes: usize,
    /// `min_digest_line_length`.
    min_line_len: usize,
    /// `bsd_reversed`, latched for the whole run.
    bsd_reversed: Layout,
    /// `base64_digest` (`cksum --base64`).
    base64: bool,
}

const fn is_white(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

impl State {
    /// The state `main` leaves behind once the options are validated.
    fn new(build: Build, set: &Settings) -> Self {
        let algo = set.algorithm.unwrap_or(build.algo());
        // `if (digest_length == 0) digest_length = …` — `b2sum`'s default is
        // BLAKE2b's full width, everything else's is its algorithm's.
        let digest_length = match usize::try_from(set.length) {
            Ok(0) | Err(_) => algo.bits(),
            Ok(bits) => bits,
        };
        State {
            build,
            algo,
            algorithm_specified: set.algorithm.is_some(),
            digest_length,
            digest_hex_bytes: digest_length / 4,
            // `MIN_DIGEST_LINE_LENGTH`: 3 in the variable-width builds (a
            // two-digit digest, a blank and a name, with `-l 8`), else the
            // full digest, a blank and a one-byte name.
            min_line_len: if build.variable_width() {
                3
            } else {
                (algo.bits() / 4).saturating_add(2)
            },
            bsd_reversed: Layout::Undecided,
            base64: set.base64,
        }
    }

    /// The BLAKE2b width in bytes, which is what a BLAKE2b stream is built
    /// with. Other algorithms ignore it.
    const fn digest_bytes(&self) -> usize {
        self.digest_length / 8
    }

    /// [`feed_file`] into a fresh hash of the current algorithm and width.
    fn hash_file(&self, name: &[u8], ignore_missing: bool, read_stdin: &mut bool) -> Hashed {
        let mut hasher = self.algo.stream(self.digest_bytes());
        let mut length: u64 = 0;
        match feed_file(
            self.build.name(),
            name,
            ignore_missing,
            read_stdin,
            &mut |data| {
                hasher.update(data);
                length = length.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
            },
        ) {
            Fed::Ok => Hashed::Ok {
                digest: hasher.finish(),
                length,
            },
            Fed::Missing => Hashed::Missing,
            Fed::Failed => Hashed::Failed,
        }
    }

    /// `DIGEST_OUT`: one output record, in whichever form the algorithm prints.
    ///
    /// `named` is upstream's `optind != argc` — any operand at all was given —
    /// which only the three legacy formats consult, to decide whether to print
    /// the name.
    // Upstream's `digest_output_fn` takes these same eight, and every one is
    // consulted by one of the formats; bundling them would only rename the
    // list.
    #[allow(clippy::too_many_arguments)]
    fn output(
        &self,
        name: &[u8],
        binary: bool,
        digest: &[u8],
        length: u64,
        raw: bool,
        tagged: bool,
        delim: u8,
        named: bool,
    ) -> Vec<u8> {
        let shown = named.then_some(name);
        match self.algo {
            Algo::Bsd => crate::sum::output_bsd(checksum16(digest), length, shown, raw, delim),
            Algo::Sysv => crate::sum::output_sysv(checksum16(digest), length, shown, raw, delim),
            Algo::Crc => crate::cksum::output_crc(checksum32(digest), length, shown, raw, delim),
            _ => self.output_file(name, binary, digest, raw, tagged, delim),
        }
    }

    /// Upstream's `output_file`: both layouts, and `cksum`'s `--raw` and
    /// `--base64`.
    fn output_file(
        &self,
        name: &[u8],
        binary: bool,
        digest: &[u8],
        raw: bool,
        tagged: bool,
        delim: u8,
    ) -> Vec<u8> {
        if raw {
            // `fwrite (digest, 1, digest_length / 8, stdout)`: no terminator.
            return digest.get(..self.digest_bytes()).unwrap_or(digest).to_vec();
        }

        let escape = delim == b'\n' && problematic(name);
        let shown = escape_name(name, escape);

        let mut out = Vec::with_capacity(shown.len().saturating_add(160));
        if escape {
            out.push(b'\\');
        }
        if tagged {
            out.extend_from_slice(self.algo.tag().as_bytes());
            // A narrower BLAKE2b says how narrow: `BLAKE2b-256 (f) = …`.
            if self.algo == Algo::Blake2b && self.digest_length < blake2::OUT_BYTES * 8 {
                out.extend_from_slice(format!("-{}", self.digest_length).as_bytes());
            }
            out.extend_from_slice(b" (");
            out.extend_from_slice(&shown);
            out.extend_from_slice(b") = ");
        }
        if self.base64 {
            out.extend_from_slice(&base64_encode(
                digest.get(..self.digest_bytes()).unwrap_or(digest),
            ));
        } else {
            out.extend_from_slice(&to_hex(
                digest.get(..self.digest_hex_bytes / 2).unwrap_or(digest),
            ));
        }
        if !tagged {
            out.push(b' ');
            out.push(if binary { b'*' } else { b' ' });
            out.extend_from_slice(&shown);
        }
        out.push(delim);
        out
    }

    /// Upstream's `valid_digits`: exactly `digest_hex_bytes` hex digits — or,
    /// in `cksum`, the base64 of `digest_length / 8` bytes, padding included.
    fn valid_digits(&self, s: &[u8]) -> bool {
        if self.build == Build::Cksum && s.len() == base64_length(self.digest_bytes()) {
            // `len - digest_length % 3` characters of the alphabet, then that
            // many `=`. (`digest_length % 3`, in *bits*, is the padding count:
            // eight is two modulo three, so it tracks the byte count's own.)
            let body = s.len().saturating_sub(self.digest_length % 3);
            let (text, pad) = s.split_at(body);
            return text.iter().all(|&c| is_base64(c)) && pad.iter().all(|&c| c == b'=');
        }
        s.len() == self.digest_hex_bytes && s.iter().all(u8::is_ascii_hexdigit)
    }

    /// Upstream's `split_3`. `line` has already had its terminator removed.
    ///
    /// Mutates the run state exactly where upstream mutates its globals: the
    /// algorithm (`cksum` without `-a`), the digest width (a stated width, or
    /// an untagged BLAKE2b digest's own length), and the layout latch.
    fn split_3(&mut self, line: &[u8]) -> Option<CheckLine> {
        let mut i = 0usize;
        while line.get(i).copied().is_some_and(is_white) {
            i = i.checked_add(1)?;
        }

        let mut escaped = false;
        if line.get(i) == Some(&b'\\') {
            i = i.checked_add(1)?;
            escaped = true;
        }

        // --- `cksum` without `-a`: the tag is the algorithm ---------------
        if self.build == Build::Cksum && !self.algorithm_specified {
            match algorithm_from_tag(line.get(i..)?) {
                // "We don't support checking these older formats."
                Some(algo) if algo.is_legacy() => return None,
                Some(algo) => self.algo = algo,
                // "We only support tagged format without -a."
                None => return None,
            }
        }

        // --- the tagged format ---------------------------------------------
        let tag = self.algo.tag().as_bytes();
        if line.get(i..)?.starts_with(tag) {
            i = i.checked_add(tag.len())?;
            if self.build.variable_width() {
                // `s[i++] = '\0'`: the byte after the tag ends it, whatever it
                // was — unless it is the `(` of the OpenSSL form, which is put
                // back.
                let length_specified = line.get(i) == Some(&b'-');
                let openssl_format = line.get(i) == Some(&b'(');
                if !openssl_format {
                    i = i.checked_add(1)?;
                }
                self.digest_length = self.algo.bits();
                if length_specified {
                    // `xstrtoumax (s + i, &siend, 0, &length, nullptr)`: base
                    // zero, any text after the number.
                    let (length, status, end) =
                        xnum::xstrtoumax_end(line.get(i..).unwrap_or(&[]), 0, None);
                    let length = usize::try_from(length).ok()?;
                    if !(status == Status::Ok
                        && length > 0
                        && length <= self.digest_length
                        && length.is_multiple_of(8))
                    {
                        return None;
                    }
                    i = i.checked_add(end)?;
                    self.digest_length = length;
                }
                self.digest_hex_bytes = self.digest_length / 4;
            }
            if line.get(i) == Some(&b' ') {
                i = i.checked_add(1)?;
            }
            if line.get(i) == Some(&b'(') {
                i = i.checked_add(1)?;
                return self.bsd_split_3(line.get(i..)?, escaped);
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
        if line.len().saturating_sub(i) < self.min_line_len.saturating_add(extra) {
            return None;
        }

        // --- an untagged BLAKE2b digest says its own width ------------------
        let start = i;
        if self.algo == Algo::Blake2b && self.build.variable_width() {
            let hex = line
                .get(start..)?
                .iter()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            if hex < 2 || !hex.is_multiple_of(2) || hex > blake2::OUT_BYTES * 2 {
                return None;
            }
            self.digest_hex_bytes = hex;
            self.digest_length = hex.saturating_mul(4);
        }

        // --- the digest ----------------------------------------------------
        while line.get(i).copied().is_some_and(|b| !is_white(b)) {
            i = i.checked_add(1)?;
        }
        // The digest must be followed by at least one whitespace character.
        if i == line.len() {
            return None;
        }
        let digest = line.get(start..i)?.to_vec();
        i = i.checked_add(1)?;
        if !self.valid_digits(&digest) {
            return None;
        }

        // --- which layout, and the indicator byte --------------------------
        let after = line.get(i).copied();
        let mut binary = false;
        if line.len().saturating_sub(i) == 1 || !matches!(after, Some(b' ' | b'*')) {
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
    fn bsd_split_3(&self, s: &[u8], escaped: bool) -> Option<CheckLine> {
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
        if !self.valid_digits(&digest) {
            return None;
        }
        Some(CheckLine {
            digest,
            // `*binary = 0` in upstream: a tagged line carries no indicator.
            binary: false,
            name,
        })
    }

    /// Does the recorded digest match? Upstream's choice between `b64_equal`
    /// and `hex_equal`: a digest shorter than the hex form is base64, which
    /// [`State::valid_digits`] only lets through in `cksum`.
    fn digest_matches(&self, recorded: &[u8], computed: &[u8]) -> bool {
        match recorded.len().cmp(&self.digest_hex_bytes) {
            // `b64_equal`: the exact encoding, `=` padding and case included.
            Ordering::Less => {
                base64_encode(computed.get(..self.digest_bytes()).unwrap_or(computed)) == recorded
            }
            Ordering::Equal => hex_equal(recorded, computed),
            Ordering::Greater => false,
        }
    }
}

/// The 16-bit checksum a BSD or System V stream finishes with.
fn checksum16(digest: &[u8]) -> u16 {
    digest
        .first_chunk::<2>()
        .map_or(0, |bytes| u16::from_be_bytes(*bytes))
}

/// The 32-bit CRC a CRC stream finishes with.
fn checksum32(digest: &[u8]) -> u32 {
    digest
        .first_chunk::<4>()
        .map_or(0, |bytes| u32::from_be_bytes(*bytes))
}

/// Upstream's `algorithm_from_tag`: the algorithm a tagged line names, if any.
///
/// The tag runs to the first blank, `-` or `(`, and must then be one of
/// `algorithm_tags` exactly. Anything longer than the longest tag is refused
/// before it is compared, as upstream's `max_tag_len` does.
fn algorithm_from_tag(s: &[u8]) -> Option<Algo> {
    let max_tag_len = Algo::ALL.iter().map(|a| a.tag().len()).max().unwrap_or(0);
    let mut i = 0usize;
    while i <= max_tag_len
        && s.get(i)
            .is_some_and(|&c| c != 0 && !is_white(c) && c != b'-' && c != b'(')
    {
        i = i.saturating_add(1);
    }
    if i > max_tag_len {
        return None;
    }
    let word = s.get(..i)?;
    Algo::ALL.into_iter().find(|a| a.tag().as_bytes() == word)
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

/// The whole of `digest.c`'s `main`, for one build.
///
/// # Panics
///
/// Does not. Every fallible step returns a status instead.
#[must_use]
pub fn main(build: Build) -> ExitCode {
    // Upstream registers `close_stdout` with `atexit`, so its verdict is
    // reached on every exit path, not just the last statement of `main`. One
    // value leaves this function; funnelling it here is the same guarantee.
    stdfd::close_stderr(run_main(build), 1)
}

/// Everything the utility does, so that [`main`] is only the exit path --
/// upstream's `main` minus the `atexit` handler it registers.
fn run_main(build: Build) -> ExitCode {
    stdfd::restore();

    let program = Program::new(build.name(), 1);
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // Decided before the stream exists, because upstream reaches `close_stdout`
    // from here with nothing buffered: a usage error writes only to stderr, so
    // there is no output whose fate could change the status.
    let request = match parse_args(build, program, &args) {
        Ok(r) => r,
        Err(e) => {
            program.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    // `--help` and `--version` are output like any other, and so are subject to
    // the same verdict: `md5sum --help >&-` is a failed write, which upstream
    // reports and exits 1 for rather than treating as a successful run.
    let mut set = match request {
        Request::Help => return say(build, &help_text(build)),
        Request::Version => {
            return say(
                build,
                &format!("{} (SlateOS coreutils) 0.1.0\n", build.name()),
            );
        }
        Request::Run(set) => set,
    };

    if let Err(e) = validate(build, program, &set) {
        program.report(&e);
        return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
    }

    // `if (!O_BINARY && binary < 0) binary = 0;` — on a POSIX host the unset
    // state resolves to text, which is why `md5sum f` prints two spaces.
    let binary = set.binary.unwrap_or(false);

    // `if (optind == argc) *operand_lim++ = "-"; else if (1 < argc - optind
    // && raw_digest) error (EXIT_FAILURE, …)`.
    let named = !set.files.is_empty();
    if !named {
        set.files.push(OsString::from("-"));
    } else if set.files.len() > 1 && set.raw {
        program.report(
            &program.usage("the --raw option is not supported with multiple files".to_string()),
        );
        return ExitCode::from(1);
    }

    let mut state = State::new(build, &set);
    let delim = if set.zero { 0 } else { b'\n' };
    let mut out = stdfd::Stream::stdout_line_buffered();
    let mut ok = true;
    let mut read_stdin = false;

    for operand in &set.files {
        let name = os_bytes(operand);
        if set.check {
            if !check_file(&mut state, &set, &name, &mut out, &mut read_stdin) {
                ok = false;
            }
        } else {
            match state.hash_file(&name, false, &mut read_stdin) {
                Hashed::Ok { digest, length } => {
                    let line = state.output(
                        &name, binary, &digest, length, set.raw, set.tag, delim, named,
                    );
                    // Cannot fail: a [`Stream`] latches its failure instead of
                    // returning it, and `close_stdout` below is what reads the
                    // verdict — in upstream's words rather than the bare
                    // `write error` this line used to print without an errno.
                    let _ = out.write_all(&line);
                }
                Hashed::Missing | Hashed::Failed => ok = false,
            }
        }
    }

    // `if (have_read_stdin && fclose (stdin) == EOF)`. There is no `fclose` on
    // a locked Rust stdin, and the case it catches — a read error latched but
    // not yet reported — is already reported by `feed_file`.
    let _ = read_stdin;

    let earned = if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    };
    stdfd::close_stdout(build.name(), out, earned)
}

/// `--help` and `--version`: the whole of the program's output, and then the
/// verdict on whether it arrived.
fn say(build: Build, text: &str) -> ExitCode {
    let mut out = stdfd::Stream::stdout_line_buffered();
    let _ = out.write_all(text.as_bytes());
    stdfd::close_stdout(build.name(), out, ExitCode::SUCCESS)
}

/// Upstream's `digest_check`, for one checksum file.
fn check_file(
    state: &mut State,
    set: &Settings,
    checkfile: &[u8],
    out: &mut impl Write,
    read_stdin: &mut bool,
) -> bool {
    let program = state.build.name();
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
                diag!("{program}: {}: {}", quotef(checkfile), strerror(&e));
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
        diag!("{program}: {}: read error", quotef(&shown_name));
        return false;
    }

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

        let Some(parsed) = state
            .split_3(line)
            .filter(|p| !(is_stdin && p.name == b"-"))
        else {
            tally.misformatted = tally.misformatted.saturating_add(1);
            if set.warn {
                // `DIGEST_TYPE_STRING` as it stands *now*: in `cksum` that is
                // whatever the last tag chose, this line's included.
                diag!(
                    "{program}: {}: {}: improperly formatted {} checksum line",
                    quotef(&shown_name),
                    line_number,
                    state.algo.tag()
                );
            }
            continue;
        };

        tally.any_formatted = true;
        let needs_escape = !set.status_only && problematic(&parsed.name);
        let shown = escape_name(&parsed.name, needs_escape);
        let prefix: &[u8] = if needs_escape { b"\\" } else { b"" };

        match state.hash_file(&parsed.name, set.ignore_missing, read_stdin) {
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
            Hashed::Ok {
                digest: computed, ..
            } => {
                let matched = state.digest_matches(&parsed.digest, &computed);
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
            "{program}: {}: no properly formatted checksum lines found",
            quotef(&shown_name)
        );
    } else if !set.status_only {
        if tally.misformatted != 0 {
            diag!(
                "{program}: WARNING: {} {}",
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
                "{program}: WARNING: {} {}",
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
                "{program}: WARNING: {} {}",
                tally.mismatched,
                plural(
                    tally.mismatched,
                    "computed checksum did NOT match",
                    "computed checksums did NOT match"
                )
            );
        }
        if set.ignore_missing && !tally.any_matched {
            diag!("{program}: {}: no file was verified", quotef(&shown_name));
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

    const H: &str = "b1946ac92492d2347c6235b4d2611184";

    /// The run state `main` would build for `build` and these arguments.
    fn state_for(build: Build, words: &[&str]) -> State {
        let program = Program::new(build.name(), 1);
        match parse_args(build, program, &args(words)).unwrap() {
            Request::Run(set) => State::new(build, &set),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn md5() -> State {
        state_for(Build::Md5sum, &[])
    }

    fn parse(line: &str) -> Option<CheckLine> {
        md5().split_3(line.as_bytes())
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

    #[test]
    fn a_fixed_build_does_not_skip_a_byte_after_the_tag() {
        // Only the variable-width builds consume the byte after the tag.
        assert!(parse(&format!("MD5x (a) = {H}")).is_none());
    }

    // ---------------- the anti-mixing rule ----------------

    #[test]
    fn a_reversed_line_locks_out_the_standard_one() {
        let mut s = md5();
        assert!(s.split_3(format!("{H} a").as_bytes()).is_some());
        // Now a standard line: its leading space would be read as an indicator.
        assert_eq!(s.bsd_reversed, Layout::BsdReversed);
        let got = s.split_3(format!("{H}  b").as_bytes()).unwrap();
        // Latched reversed, so the space is part of the name, not an indicator.
        assert_eq!(got.name, b" b");
    }

    #[test]
    fn a_standard_line_locks_out_the_reversed_one() {
        let mut s = md5();
        assert!(s.split_3(format!("{H}  a").as_bytes()).is_some());
        assert_eq!(s.bsd_reversed, Layout::Standard);
        assert!(s.split_3(format!("{H} a").as_bytes()).is_none());
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

    #[test]
    fn a_narrower_recorded_digest_compares_the_leading_bytes() {
        assert!(hex_equal(b"00", &[0x00, 0xff]));
        assert!(!hex_equal(b"ff", &[0x00, 0xff]));
        assert!(!hex_equal(b"00ff00", &[0x00, 0xff]));
    }

    // ---------------- b2sum: the width a line states ----------------

    fn b2(words: &[&str]) -> State {
        state_for(Build::B2sum, words)
    }

    #[test]
    fn b2sum_reads_a_stated_width() {
        let mut s = b2(&[]);
        let d = "a".repeat(64);
        let got = s
            .split_3(format!("BLAKE2b-256 (f) = {d}").as_bytes())
            .unwrap();
        assert_eq!(got.name, b"f");
        assert_eq!(s.digest_length, 256);
        assert_eq!(s.digest_hex_bytes, 64);
    }

    #[test]
    fn b2sum_stated_widths_use_cs_prefix_rule() {
        let d = "a".repeat(64);
        for tag in ["BLAKE2b-0x100", "BLAKE2b-0400", "BLAKE2b- 256"] {
            let mut s = b2(&[]);
            assert!(
                s.split_3(format!("{tag} (f) = {d}").as_bytes()).is_some(),
                "{tag}"
            );
            assert_eq!(s.digest_length, 256, "{tag}");
        }
    }

    #[test]
    fn b2sum_refuses_impossible_widths() {
        let d = "a".repeat(2);
        for tag in ["BLAKE2b-0", "BLAKE2b-7", "BLAKE2b-520", "BLAKE2b-x"] {
            assert!(
                b2(&[])
                    .split_3(format!("{tag} (f) = {d}").as_bytes())
                    .is_none(),
                "{tag}"
            );
        }
    }

    #[test]
    fn b2sum_skips_any_byte_after_the_tag() {
        let d = "a".repeat(128);
        assert!(
            b2(&[])
                .split_3(format!("BLAKE2bX (f) = {d}").as_bytes())
                .is_some()
        );
        // The OpenSSL form, with no blank before the `(`.
        assert!(
            b2(&[])
                .split_3(format!("BLAKE2b(f) = {d}").as_bytes())
                .is_some()
        );
    }

    #[test]
    fn an_untagged_blake2b_digest_says_its_width() {
        let mut s = b2(&[]);
        assert!(
            s.split_3(format!("{}  f", "ab".repeat(20)).as_bytes())
                .is_some()
        );
        assert_eq!(s.digest_length, 160);
        // Odd, too short and too long are all refused.
        assert!(
            b2(&[])
                .split_3(format!("{}  f", "a".repeat(3)).as_bytes())
                .is_none()
        );
        assert!(b2(&[]).split_3(b"a  f").is_none());
        assert!(
            b2(&[])
                .split_3(format!("{}  f", "a".repeat(130)).as_bytes())
                .is_none()
        );
    }

    // ---------------- cksum: the tag picks the algorithm ----------------

    fn ck(words: &[&str]) -> State {
        state_for(Build::Cksum, words)
    }

    #[test]
    fn cksum_without_a_reads_each_lines_tag() {
        let mut s = ck(&["-c"]);
        assert_eq!(s.algo, Algo::Crc);
        let d = "a".repeat(64);
        assert!(s.split_3(format!("SHA256 (f) = {d}").as_bytes()).is_some());
        assert_eq!(s.algo, Algo::Sha256);
        let d = "a".repeat(40);
        assert!(s.split_3(format!("SHA1 (g) = {d}").as_bytes()).is_some());
        assert_eq!(s.algo, Algo::Sha1);
    }

    #[test]
    fn cksum_without_a_refuses_untagged_and_legacy_lines() {
        assert!(ck(&["-c"]).split_3(format!("{H}  f").as_bytes()).is_none());
        assert!(ck(&["-c"]).split_3(b"CRC (f) = 1").is_none());
        assert!(ck(&["-c"]).split_3(b"BSD (f) = 1").is_none());
    }

    #[test]
    fn cksum_with_a_reads_untagged_lines() {
        let d = "a".repeat(64);
        assert!(
            ck(&["-a", "sha256", "-c"])
                .split_3(format!("{d}  f").as_bytes())
                .is_some()
        );
    }

    #[test]
    fn cksum_lets_a_tag_truncate_any_algorithm() {
        let mut s = ck(&["-a", "sha256", "-c"]);
        let d = "a".repeat(32);
        assert!(
            s.split_3(format!("SHA256-128 (f) = {d}").as_bytes())
                .is_some()
        );
        assert_eq!(s.digest_hex_bytes, 32);
        // And the width sticks: the next untagged line needs 32 digits.
        assert!(s.split_3(format!("{d}  g").as_bytes()).is_some());
        assert!(
            s.split_3(format!("{}  g", "a".repeat(64)).as_bytes())
                .is_none()
        );
    }

    #[test]
    fn cksum_accepts_base64_digests() {
        let mut s = ck(&["-c"]);
        // SHA-256: 32 bytes, 44 characters, one `=`.
        let b64 = format!("{}=", "A".repeat(43));
        assert!(
            s.split_3(format!("SHA256 (f) = {b64}").as_bytes())
                .is_some()
        );
        // Padding in the wrong place, or missing, is refused.
        let bad = "A".repeat(44);
        assert!(
            ck(&["-c"])
                .split_3(format!("SHA256 (f) = {bad}").as_bytes())
                .is_none()
        );
    }

    #[test]
    fn a_base64_digest_must_match_exactly() {
        let s = ck(&["-a", "sha256"]);
        let computed = [0u8; 32];
        let good = base64_encode(&computed);
        assert!(s.digest_matches(&good, &computed));
        let mut lower = good.clone();
        lower.make_ascii_lowercase();
        assert!(
            !s.digest_matches(&lower, &computed),
            "base64 is case-sensitive"
        );
    }

    #[test]
    fn algorithm_from_tag_is_exact_and_bounded() {
        assert_eq!(algorithm_from_tag(b"SHA256 (f)"), Some(Algo::Sha256));
        assert_eq!(algorithm_from_tag(b"BLAKE2b-256 (f)"), Some(Algo::Blake2b));
        assert_eq!(algorithm_from_tag(b"SM3(f)"), Some(Algo::Sm3));
        assert_eq!(
            algorithm_from_tag(b"sha256 (f)"),
            None,
            "tags are case-sensitive"
        );
        assert_eq!(algorithm_from_tag(b"SHA2 (f)"), None);
        assert_eq!(algorithm_from_tag(b"SHA256SUM (f)"), None);
    }

    // ---------------- rendering ----------------

    fn render(
        s: &State,
        name: &[u8],
        binary: bool,
        digest: &[u8],
        tagged: bool,
        zero: bool,
    ) -> Vec<u8> {
        let delim = if zero { 0 } else { b'\n' };
        s.output(name, binary, digest, 0, false, tagged, delim, true)
    }

    #[test]
    fn plain_render() {
        let d = [0xab; 16];
        let hex = "ab".repeat(16);
        assert_eq!(
            render(&md5(), b"a", false, &d, false, false),
            format!("{hex}  a\n").as_bytes()
        );
        assert_eq!(
            render(&md5(), b"a", true, &d, false, false),
            format!("{hex} *a\n").as_bytes()
        );
        assert_eq!(
            render(&md5(), b"a", true, &d, true, false),
            format!("MD5 (a) = {hex}\n").as_bytes()
        );
    }

    #[test]
    fn escaped_render() {
        let d = [0xab; 16];
        let hex = "ab".repeat(16);
        assert_eq!(
            render(&md5(), b"we\nird", false, &d, false, false),
            format!("\\{hex}  we\\nird\n").as_bytes()
        );
        assert_eq!(
            render(&md5(), b"we\nird", false, &d, false, true),
            format!("{hex}  we\nird\0").as_bytes()
        );
    }

    #[test]
    fn a_narrow_blake2b_says_so_in_its_tag() {
        let s = b2(&["-l", "256"]);
        let d = [0x01; 32];
        let line = render(&s, b"f", true, &d, true, false);
        assert!(line.starts_with(b"BLAKE2b-256 (f) = 0101"), "{line:?}");
        let full = b2(&[]);
        let line = render(&full, b"f", true, &[0x01; 64], true, false);
        assert!(line.starts_with(b"BLAKE2b (f) = "), "{line:?}");
    }

    #[test]
    fn cksum_base64_and_raw() {
        let s = ck(&["-a", "sha256", "--base64"]);
        let line = render(&s, b"f", false, &[0u8; 32], true, false);
        assert_eq!(
            line,
            format!("SHA256 (f) = {}=\n", "A".repeat(43)).as_bytes()
        );
        let s = ck(&["-a", "sha256", "--raw"]);
        assert_eq!(
            s.output(b"f", false, &[7u8; 32], 0, true, true, b'\n', true),
            [7u8; 32]
        );
    }

    #[test]
    fn cksum_legacy_outputs() {
        let s = ck(&[]);
        assert_eq!(
            s.output(
                b"f",
                false,
                &3_015_617_425u32.to_be_bytes(),
                6,
                false,
                true,
                b'\n',
                true
            ),
            b"3015617425 6 f\n"
        );
        assert_eq!(
            s.output(
                b"f",
                false,
                &1u32.to_be_bytes(),
                0,
                false,
                true,
                b'\n',
                false
            ),
            b"1 0\n"
        );
        let s = ck(&["-a", "bsd"]);
        assert_eq!(
            s.output(
                b"f",
                false,
                &36979u16.to_be_bytes(),
                6,
                false,
                true,
                b'\n',
                true
            ),
            b"36979     1 f\n"
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

    fn settings(build: Build, words: &[&str]) -> Settings {
        let program = Program::new(build.name(), 1);
        match parse_args(build, program, &args(words)).unwrap() {
            Request::Run(s) => s,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn md5_settings(words: &[&str]) -> Settings {
        settings(Build::Md5sum, words)
    }

    #[test]
    fn tag_implies_binary() {
        let s = md5_settings(&["--tag"]);
        assert!(s.tag);
        assert_eq!(s.binary, Some(true));
    }

    #[test]
    fn tag_then_text_is_an_error_but_text_then_tag_is_not() {
        assert!(validate(Build::Md5sum, P, &md5_settings(&["--tag", "--text"])).is_err());
        assert!(validate(Build::Md5sum, P, &md5_settings(&["--text", "--tag"])).is_ok());
    }

    #[test]
    fn status_warn_and_quiet_are_last_wins() {
        let s = md5_settings(&["--status", "-w", "-c"]);
        assert!(s.warn && !s.status_only && !s.quiet);
        let s = md5_settings(&["-w", "--status", "-c"]);
        assert!(s.status_only && !s.warn && !s.quiet);
        let s = md5_settings(&["--status", "--quiet", "-c"]);
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
                validate(Build::Md5sum, P, &md5_settings(&[word])).is_err(),
                "{word} should require -c"
            );
            assert!(validate(Build::Md5sum, P, &md5_settings(&[word, "-c"])).is_ok());
        }
    }

    #[test]
    fn validation_order_is_upstreams() {
        // `--tag --text -c` violates three rules; the --text one is reported.
        let e = validate(Build::Md5sum, P, &md5_settings(&["--tag", "--text", "-c"])).unwrap_err();
        assert_eq!(e.sentence, "--tag does not support --text mode");
    }

    #[test]
    fn zero_and_check_conflict() {
        let e = validate(Build::Md5sum, P, &md5_settings(&["-z", "-c"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "the --zero option is not supported when verifying checksums"
        );
    }

    #[test]
    fn binary_and_check_conflict() {
        let e = validate(Build::Md5sum, P, &md5_settings(&["-b", "-c"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "the --binary and --text options are meaningless when verifying checksums"
        );
    }

    #[test]
    fn operands_are_bytes() {
        // The whole point: a name that is not valid UTF-8 survives.
        let s = md5_settings(&["-c"]);
        assert!(s.files.is_empty());
        let raw = os_from_bytes(b"na\xffme");
        let argv = vec![OsString::from("-b"), raw.clone()];
        let Request::Run(s) = parse_args(Build::Md5sum, P, &argv).unwrap() else {
            panic!("expected Run")
        };
        assert_eq!(s.files, vec![raw]);
    }

    #[test]
    fn help_names_the_algorithm() {
        let text = help_text(Build::Md5sum);
        assert!(text.contains("Print or check MD5 (128-bit) checksums."));
        assert!(text.contains("described in RFC 1321."));
        let text = help_text(Build::B2sum);
        assert!(text.contains("Print or check BLAKE2b (512-bit) checksums."));
        assert!(text.contains("Mandatory arguments"));
        assert!(help_text(Build::Cksum).contains("By default use the 32 bit CRC algorithm."));
    }

    #[test]
    fn plurals_agree() {
        assert_eq!(plural(1, "a", "b"), "a");
        assert_eq!(plural(0, "a", "b"), "b");
        assert_eq!(plural(2, "a", "b"), "b");
    }

    // ---------------- b2sum and cksum options ----------------

    #[test]
    fn a_length_must_be_a_multiple_of_eight_and_is_checked_at_once() {
        let b2 = Program::new("b2sum", 1);
        let e = parse_args(Build::B2sum, b2, &args(&["-l", "7", "--nope"])).unwrap_err();
        assert!(
            e.sentence.ends_with("b2sum: length is not a multiple of 8"),
            "{}",
            e.sentence
        );
        assert!(e.referral.is_none());
        let e = parse_args(Build::B2sum, b2, &args(&["-l", "x"])).unwrap_err();
        assert!(e.sentence.starts_with("invalid length: "), "{}", e.sentence);
    }

    #[test]
    fn a_length_over_512_is_refused_after_parsing() {
        let b2 = Program::new("b2sum", 1);
        let set = settings(Build::B2sum, &["-l", "1024"]);
        let e = validate(Build::B2sum, b2, &set).unwrap_err();
        assert!(
            e.sentence.contains("maximum digest length for"),
            "{}",
            e.sentence
        );
        assert!(e.sentence.ends_with("is 512 bits"), "{}", e.sentence);
    }

    #[test]
    fn length_zero_is_the_default_width() {
        assert_eq!(b2(&["-l", "0"]).digest_length, 512);
        assert_eq!(b2(&["-l", "8"]).digest_length, 8);
    }

    #[test]
    fn cksum_option_rules() {
        let ck_p = Program::new("cksum", 1);
        let v = |w: &[&str]| validate(Build::Cksum, ck_p, &settings(Build::Cksum, w));
        assert_eq!(
            v(&["-l", "8"]).unwrap_err().sentence,
            "--length is only supported with --algorithm=blake2b"
        );
        assert!(v(&["-a", "blake2b", "-l", "8"]).is_ok());
        assert!(v(&["-l", "0", "-a", "sha1"]).is_ok(), "0 is not given");
        assert_eq!(
            v(&["-a", "crc", "-c"]).unwrap_err().sentence,
            "--check is not supported with --algorithm={bsd,sysv,crc}"
        );
        assert!(v(&["-c"]).is_ok(), "the default CRC was not specified");
        assert_eq!(
            v(&["--base64", "--raw"]).unwrap_err().sentence,
            "--base64 and --raw are mutually exclusive"
        );
        assert_eq!(
            v(&["-t"]).unwrap_err().sentence,
            "--text mode is only supported with --untagged"
        );
        assert!(v(&["--untagged", "-t"]).is_ok());
        assert!(v(&["--tag", "-c"]).is_err(), "--tag sets binary");
        assert!(v(&["-c", "-a", "sha1"]).is_ok());
    }

    #[test]
    fn cksum_algorithm_names_are_exact() {
        let ck_p = Program::new("cksum", 1);
        assert!(parse_args(Build::Cksum, ck_p, &args(&["-a", "sha"])).is_err());
        assert!(parse_args(Build::Cksum, ck_p, &args(&["-a", "SHA1"])).is_err());
        let set = settings(Build::Cksum, &["--algorithm=sm3"]);
        assert_eq!(set.algorithm, Some(Algo::Sm3));
    }

    // ---------------- the algorithm table ----------------

    #[test]
    fn every_stream_is_as_wide_as_its_algorithm_says() {
        for algo in Algo::ALL {
            let digest = algo.stream(64).finish();
            assert_eq!(digest.len() * 8, algo.bits(), "{algo:?}");
        }
        assert_eq!(Algo::Blake2b.stream(20).finish().len(), 20);
    }

    #[test]
    fn the_tables_agree_with_each_other() {
        for (i, algo) in Algo::ALL.into_iter().enumerate() {
            assert_eq!(ALGORITHM_ARGS[i], (algo.arg(), algo));
        }
    }

    /// One known answer per algorithm, through the same `Stream` the programs
    /// use, so a wiring mistake — two algorithms swapped — cannot pass.
    #[test]
    fn each_algorithm_is_the_one_its_name_says() {
        let hex = |algo: Algo| {
            let mut s = algo.stream(64);
            s.update(b"abc");
            to_hex(&s.finish())
        };
        let want = [
            (Algo::Md5, "900150983cd24fb0d6963f7d28e17f72"),
            (Algo::Sha1, "a9993e364706816aba3e25717850c26c9cd0d89d"),
            (
                Algo::Sha224,
                "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",
            ),
            (
                Algo::Sha256,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                Algo::Sm3,
                "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0",
            ),
        ];
        for (algo, digest) in want {
            assert_eq!(hex(algo), digest.as_bytes(), "{algo:?}");
        }
        assert!(hex(Algo::Sha384).starts_with(b"cb00753f45a35e8b"));
        assert!(hex(Algo::Sha512).starts_with(b"ddaf35a193617aba"));
        assert!(hex(Algo::Blake2b).starts_with(b"ba80a53f981c4d0d"));
    }
}
