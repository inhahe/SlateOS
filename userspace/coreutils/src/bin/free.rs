//! `free` — report the amount of free and used memory in the system.
//!
//! A transcription of procps-ng 4.0.4's `src/free.c`, together with the parts
//! of `library/meminfo.c` and `local/strutils.c` that it depends on, measured
//! against the system binary of that exact version (`free from procps-ng
//! 4.0.4`). `free` is not a coreutils tool — it belongs to procps — and its
//! output is a fixed-width table that monitoring scripts slice by column, so
//! every string and every column width below was measured before it was
//! written down rather than reasoned about.
//!
//! # What was wrong with the implementation this replaces
//!
//! The version this replaces was not a port of anything. It invented an
//! interface — `free [-h] [-k] [-m] [-g]` — that happens to share four letters
//! with the real one and agrees with it on none of the output.
//!
//! 1. **`argv` was read as `Vec<String>`**, so an argument holding a byte that
//!    is not valid UTF-8 aborted the process. That is the defect this sweep
//!    exists to remove.
//! 2. **The header was wrong.** It printed `total`/`used`/`free`/`shared`/
//!    `buff/cache`/`available` at invented widths under a `{:>15}`/`{:>10}`
//!    scheme. Upstream's header is a single 80-byte literal and its columns are
//!    a 9-column label field followed by `%11s` fields — so every script that
//!    cuts by character offset read the wrong bytes.
//! 3. **The row labels were `Mem (KiB):` and `Swap (KiB):`**, which are 10 and
//!    11 characters against upstream's 9-column field, so even the labels
//!    pushed the numbers out of column. Upstream's are `Mem:` and `Swap:`.
//! 4. **`shared` was hardcoded to `0`** rather than read from `Shmem`.
//! 5. **`used` was `total - free - buffers - cached`.** Upstream's is
//!    `MemTotal - MemAvailable`, which is a different number on every Linux
//!    since 3.14 — and it falls back to `MemTotal - MemFree` only when that
//!    subtraction goes negative.
//! 6. **There was no `available` derivation at all**, no
//!    `MemAvailable`-is-absent substitution and no `MemAvailable > MemTotal`
//!    guard (the one that stops `free` reporting nonsense inside an LXC
//!    container).
//! 7. **`Cached` was used raw.** Upstream's cache figure is
//!    `Cached + SReclaimable`; on a normal machine that is off by hundreds of
//!    megabytes.
//! 8. **Nine of the eleven options did not exist**: `-b`, `-l`, `-L`, `-t`,
//!    `-v`, `-w`, `-s`, `-c`, `--si`, every long option, `--help` and `-V`.
//! 9. **`-h` was hand-rolled** with fixed thresholds, stopped at `Gi`, and
//!    printed `1023.0Ki` where upstream prints `1.0Mi`. Upstream does not
//!    threshold at all: it formats at each unit in turn and takes the first
//!    rendering that *fits the column*, which is why `1023 kB` rounds up
//!    across a unit boundary and `10188 kB` prints `9.9Mi`.
//! 10. **Any failure to read `/proc/meminfo` printed one invented sentence**,
//!     `free: cannot read /proc/meminfo`. Upstream distinguishes the file not
//!     existing from every other failure.
//! 11. **Operands were ignored.** `free /etc/passwd` printed the table.
//!     Upstream prints its usage to stderr and exits 1.
//! 12. **Write errors were discarded** — `let _ = writeln!(…)` throughout — so
//!     `free > /full/disk` exited 0 having printed nothing.
//! 13. **No `guard_std_fds!()`**, so `free >&-` could write the table into
//!     whatever file happened to open as descriptor 1.
//! 14. **No `--version`.**
//!
//! # Measured against procps-ng 4.0.4
//!
//! Most of these were measured against a fabricated `/proc/meminfo` bind-mounted
//! into a private mount namespace (`unshare -r -m`), so the derivations could be
//! exercised at chosen values rather than at whatever the dev machine happened
//! to hold. Six such files — the six in the test module below — were rendered
//! under twelve option sets each, and **all 72 outputs are byte-identical**
//! between this transcription and the shipped binary, trailing spaces included.
//!
//! | Input | Output |
//! |---|---|
//! | `MemTotal 10188 kB`, `-h` | `9.9Mi` — `.1f` fits 5 columns |
//! | `10240 kB`, `-h` | `10Mi` — `10.0Mi` is 6, so the integer form wins |
//! | `1023 kB`, `-h` | `1.0Mi` — rounds *up* across the unit boundary |
//! | `999 kB`, `-h` | `999Ki` |
//! | `1000000000 kB`, `-h` | `953Gi` |
//! | `1073741824 kB`, `-h` | `1.0Ti` |
//! | `1099511 kB`, `-h --si` | `1.1G` |
//! | `MemTotal 1`, `MemFree 2` | `used` prints `-1`; `-b` prints `-1024` |
//! | no `MemAvailable` | `available` = `MemFree` |
//! | `MemAvailable > MemTotal` | `available` = `MemFree` (the LXC guard) |
//! | no `LowTotal` | `Low:` row = `MemTotal`/`MemTotal-MemFree`/`MemFree` |
//! | `free -k -m` | `free: Multiple unit options don't make sense.`, 1 |
//! | `free -s abc` | `free: seconds argument failed: 'abc': Invalid argument` |
//! | `free -s ''` | `free: seconds argument failed: ''` — no suffix |
//! | `free -s .` | ``free: seconds argument `.' is not positive number`` |
//! | `free -c 0` | `free: failed to parse count argument: '0': Numerical result out of range` |
//! | `free -c abc` | `free: failed to parse count argument: 'abc'` — no suffix |
//! | `free foo` | the usage block on stderr with *no* message, status 1 |
//! | `free --k` | `option '--k' is ambiguous; possibilities: '--kilo' '--kibi'` |
//! | `free --te` | `… possibilities: '--tera' '--tebi'` |
//! | `free --s` | `… possibilities: '--si' '--seconds'` |
//! | `free --co` | `… possibilities: '--committed' '--count'` |
//! | `free -L` | `SwapUse …11… CachUse …11…  MemUse …11… MemFree …11… ` + `\n` |
//!
//! Every command-line error — getopt's own and `free foo` alike — is followed
//! by the *whole* usage block on stderr, never a `Try 'free --help'` referral.
//!
//! # Deliberate differences from procps-ng
//!
//! 1. **`SwapFree` is derived from `SwapTotal - SwapUsed` when the `SwapFree`
//!    key is absent and `SwapUsed` is present.** This OS's `/proc/meminfo`
//!    publishes `SwapUsed` and not `SwapFree` (see
//!    `requests/b-a-proc-meminfo-omits-the-linux-keys-that-thirteen-tools-read.md`),
//!    and upstream's `swap_used = SwapTotal - SwapFree` then reports swap as
//!    **entirely full on every machine that has any**. That is a wrong answer
//!    in the alarming direction rather than a cosmetic difference, so it is
//!    fixed here rather than reproduced — see `design-decisions.md` 623 for the
//!    rule and 624 for this application of it. The substitution keys on the
//!    `SwapFree` line being *absent*, not on its value being zero, so a Linux
//!    machine whose swap genuinely is full still reads full.
//! 2. **`-c` outside `int` range is refused rather than truncated.** Upstream
//!    parses the count with `strtol` into a `long` and assigns it to an `int`,
//!    so `free -c 4294967297` silently becomes `free -c 1` and prints once,
//!    while `-c 4294967296` and `-c 2147483648` — which truncate to 0 and to
//!    `INT_MIN` — are refused. A count that wraps into a different count is a
//!    wrong answer, so anything outside `i32` is refused here.
//! 3. **`free -c ''` prints no `strerror` suffix.** Upstream's `strtol_or_err`
//!    does not zero `errno` before rejecting an empty string, so the message
//!    carries whatever `errno` the last unrelated library call left behind —
//!    measured as `: No such file or directory` on the reference machine, which
//!    is not reproducible and not information. (`-s ''` needs no divergence:
//!    `free.c` zeroes `errno` itself on that path.)
//! 4. **`/proc/meminfo` is re-read on every iteration of `-s`/`-c`.** procps'
//!    library caches the file for one second, so `free -c 2 -s 0.1` prints the
//!    same numbers twice. Repeating a stale reading is the one thing a repeat
//!    mode exists not to do.
//! 5. **`--version` reports `free from SlateOS coreutils 0.1.0`**, as every
//!    other utility here does.
//! 6. **Bytes echoed back in a diagnostic go through
//!    `coreutils::quote::escape_unprintable`**, so a control character in a
//!    rejected argument cannot rewrite the terminal.

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::stdfd::{self, Stream};

/// `free`'s usage status is 1: measured with `free --zzz; echo $?`.
const FREE: Program = Program::new("free", 1);

/// Upstream's `getopt_long` string, byte for byte. `-c` and `-s` take
/// arguments; there is no leading `+`, so options may follow operands — which
/// matters only for the diagnostic, since any operand at all is fatal.
const SHORT_OPTIONS: &str = "bkmghlLtvc:ws:V";

/// Upstream's `longopts[]`, **in declaration order**.
///
/// The order is observable, not incidental: glibc reports the ambiguous
/// candidates for a prefix in table order, so `free --k` naming `'--kilo'`
/// before `'--kibi'` is a fact about this table.
/// `scripts/getopt-ambiguity-check.py` compares the two as sequences.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("bytes", Takes::Nothing),
    ("kilo", Takes::Nothing),
    ("mega", Takes::Nothing),
    ("giga", Takes::Nothing),
    ("tera", Takes::Nothing),
    ("peta", Takes::Nothing),
    ("kibi", Takes::Nothing),
    ("mebi", Takes::Nothing),
    ("gibi", Takes::Nothing),
    ("tebi", Takes::Nothing),
    ("pebi", Takes::Nothing),
    ("human", Takes::Nothing),
    ("si", Takes::Nothing),
    ("lohi", Takes::Nothing),
    ("line", Takes::Nothing),
    ("total", Takes::Nothing),
    ("committed", Takes::Nothing),
    ("seconds", Takes::Required),
    ("count", Takes::Required),
    ("wide", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Upstream's `usage()` output, byte for byte — 1125 bytes.
///
/// It is the same text on stdout for `--help` and on stderr after any
/// command-line error, which is why it is one constant and not two. The leading
/// newline is `USAGE_HEADER`'s and is what puts a blank line between a getopt
/// diagnostic and the block that follows it.
const HELP: &str = concat!(
    "\n",
    "Usage:\n",
    " free [options]\n",
    "\n",
    "Options:\n",
    " -b, --bytes         show output in bytes\n",
    "     --kilo          show output in kilobytes\n",
    "     --mega          show output in megabytes\n",
    "     --giga          show output in gigabytes\n",
    "     --tera          show output in terabytes\n",
    "     --peta          show output in petabytes\n",
    " -k, --kibi          show output in kibibytes\n",
    " -m, --mebi          show output in mebibytes\n",
    " -g, --gibi          show output in gibibytes\n",
    "     --tebi          show output in tebibytes\n",
    "     --pebi          show output in pebibytes\n",
    " -h, --human         show human-readable output\n",
    "     --si            use powers of 1000 not 1024\n",
    " -l, --lohi          show detailed low and high memory statistics\n",
    " -L, --line          show output on a single line\n",
    " -t, --total         show total for RAM + swap\n",
    " -v, --committed     show committed memory and commit limit\n",
    " -s N, --seconds N   repeat printing every N seconds\n",
    " -c N, --count N     repeat printing N times, then exit\n",
    " -w, --wide          wide output\n",
    "\n",
    "     --help     display this help and exit\n",
    " -V, --version  output version information and exit\n",
    "\n",
    "For more details see free(1).\n",
);

/// Upstream prints `PROCPS_NG_VERSION`, which is `free from procps-ng 4.0.4`.
fn version_text() -> &'static str {
    "free from SlateOS coreutils 0.1.0\n"
}

/// The file every figure below comes from.
const MEMINFO_PATH: &str = "/proc/meminfo";

// ---------------------------------------------------------------------------
// Command-line state
// ---------------------------------------------------------------------------

/// Upstream's `flags` bitfield, one field per bit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Flags {
    /// `-h` — fit each figure into the column at the largest unit that fits.
    human: bool,
    /// `-l` — add the `Low:` and `High:` rows.
    lohi: bool,
    /// `-w` — split `buff/cache` into `buffers` and `cache`.
    wide: bool,
    /// `-t` — add the `Total:` row.
    total: bool,
    /// `--si` and the `--kilo` family — powers of 1000 rather than 1024.
    si: bool,
    /// `-s` or `-c` — loop.
    repeat: bool,
    /// `-c` — loop a bounded number of times.
    repeat_count: bool,
    /// `-v` — add the `Comm:` row.
    committed: bool,
    /// `-L` — one line instead of a table.
    line: bool,
}

/// Upstream's `struct commandline_arguments`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CmdArgs {
    /// 0 = default (kibibytes), 1 = bytes, 2 = kilo/kibi … 6 = peta/pebi.
    exponent: u32,
    /// Microseconds between iterations. Upstream stores this in a `float`,
    /// which is visible in the rounding, so this is `f32` and not `f64`.
    repeat_interval_us: f32,
    /// Iterations remaining. Upstream's is an `int`; see divergence 2.
    repeat_counter: i32,
}

impl Default for CmdArgs {
    /// Upstream's `args.exponent = 0; args.repeat_interval = 1000000;
    /// args.repeat_counter = 0;`.
    fn default() -> Self {
        CmdArgs {
            exponent: 0,
            repeat_interval_us: 1_000_000.0,
            repeat_counter: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// The two number parsers, transcribed from procps' `local/strutils.c`
// ---------------------------------------------------------------------------

/// Why a number was refused, which decides the `: …` suffix on the diagnostic.
///
/// Upstream passes an `errno` to `error(3)`, which appends `strerror` of it.
/// The two values it can pass here are `ERANGE` and `EINVAL`; both are spelled
/// out as the literals glibc prints, because `free.c` *chooses* them rather
/// than receiving them from the OS — there is no host error to translate, and
/// `coreutils::errmsg::strerror` maps `io::ErrorKind`s, of which neither has
/// one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumFault {
    /// `ERANGE` — `Numerical result out of range`.
    Range,
    /// No suffix at all: upstream reached `error(…, errno, …)` with `errno`
    /// still 0.
    NoConversion,
    /// `EINVAL` — `Invalid argument`.
    Invalid,
}

impl NumFault {
    /// The text `error(3)` appends, including its leading `: `.
    fn suffix(self) -> &'static str {
        match self {
            NumFault::Range => ": Numerical result out of range",
            NumFault::NoConversion => "",
            NumFault::Invalid => ": Invalid argument",
        }
    }
}

/// `strtol_or_err` with base 10, returning the `long` upstream would.
///
/// Leading whitespace and a sign are accepted because that is `strtol`; a
/// trailing byte that is not a digit is not, because `strtol_or_err` insists on
/// `*end == '\0'`.
fn strtol(text: &[u8]) -> Result<i64, NumFault> {
    if text.is_empty() {
        // Upstream's `str != NULL && *str != '\0'` guard skips the conversion
        // entirely and reports with a stale `errno`; divergence 3.
        return Err(NumFault::NoConversion);
    }
    let mut i = 0usize;
    while matches!(text.get(i), Some(c) if c.is_ascii_whitespace()) {
        i = i.saturating_add(1);
    }
    let negative = match text.get(i) {
        Some(b'-') => {
            i = i.saturating_add(1);
            true
        }
        Some(b'+') => {
            i = i.saturating_add(1);
            false
        }
        _ => false,
    };
    let start = i;
    let mut value: i64 = 0;
    let mut overflow = false;
    while let Some(&c) = text.get(i) {
        if !c.is_ascii_digit() {
            break;
        }
        let digit = i64::from(c - b'0');
        // Accumulated with the sign already applied so that `-9223372036854775808`
        // is reachable, as it is for `strtol`.
        match value
            .checked_mul(10)
            .and_then(|v| v.checked_add(if negative { -digit } else { digit }))
        {
            Some(v) => value = v,
            // `strtol` keeps consuming digits after saturating, and so must
            // this, or `999999999999999999999x` would report the wrong fault.
            None => overflow = true,
        }
        i = i.saturating_add(1);
    }
    if i == start {
        // `str == end`: no digits at all.
        return Err(NumFault::NoConversion);
    }
    if i != text.len() {
        // `*end != '\0'`: trailing junk. Upstream reports with `errno` 0.
        return Err(NumFault::NoConversion);
    }
    if overflow {
        return Err(NumFault::Range);
    }
    Ok(value)
}

/// `strtod_nol_or_err` — procps' locale-independent decimal reader.
///
/// It is not `strtod`: there is no exponent, no hex form, no infinity and no
/// NaN, and the radix point may be `.` **or** `,` (the comment in `strutils.c`
/// notes that this is why the other cannot be a thousands separator). The
/// digits are accumulated by the same walk-forward-then-multiply-down loop
/// upstream uses, because its rounding is what ends up in the `float` the
/// caller compares against 1.
fn strtod_nol(text: &[u8]) -> Result<f64, NumFault> {
    if text.is_empty() {
        return Err(NumFault::NoConversion);
    }
    let mut cp = 0usize;
    while matches!(text.get(cp), Some(c) if c.is_ascii_whitespace()) {
        cp = cp.saturating_add(1);
    }
    let negative = match text.get(cp) {
        Some(b'-') => {
            cp = cp.saturating_add(1);
            true
        }
        Some(b'+') => {
            cp = cp.saturating_add(1);
            false
        }
        _ => false,
    };

    // Walk to the end of the integer part first so that `mult` starts at the
    // right power of ten and the digits can be consumed most-significant first.
    let mut num = 0.0f64;
    let mut mult = 0.1f64;
    let mut radix = cp;
    while matches!(text.get(radix), Some(c) if c.is_ascii_digit()) {
        radix = radix.saturating_add(1);
        mult *= 10.0;
    }
    while let Some(&c) = text.get(cp) {
        if !c.is_ascii_digit() {
            break;
        }
        num += f64::from(c - b'0') * mult;
        mult /= 10.0;
        cp = cp.saturating_add(1);
    }
    if cp == text.len() {
        return Ok(if negative { -num } else { num });
    }
    if !matches!(text.get(cp), Some(b'.' | b',')) {
        return Err(NumFault::Invalid);
    }
    cp = cp.saturating_add(1);
    mult = 0.1;
    while let Some(&c) = text.get(cp) {
        if !c.is_ascii_digit() {
            break;
        }
        num += f64::from(c - b'0') * mult;
        mult /= 10.0;
        cp = cp.saturating_add(1);
    }
    if cp == text.len() {
        return Ok(if negative { -num } else { num });
    }
    // Trailing junk after the fraction falls out of upstream's `if` block and
    // reaches `error(…, errno, …)` with `errno` still 0: no suffix.
    Err(NumFault::NoConversion)
}

// ---------------------------------------------------------------------------
// `/proc/meminfo`, transcribed from procps' `library/meminfo.c`
// ---------------------------------------------------------------------------

/// The keys read straight out of the file, before any derivation.
///
/// Every field is the kibibyte figure the kernel printed. A key that is not in
/// the file stays 0 — that is what procps' hash lookup does with an unknown
/// name, and several of the derivations below exist precisely to notice it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Raw {
    mem_total: u64,
    mem_free: u64,
    mem_available: u64,
    buffers: u64,
    cached: u64,
    s_reclaimable: u64,
    shmem: u64,
    high_total: u64,
    high_free: u64,
    low_total: u64,
    low_free: u64,
    swap_total: u64,
    swap_free: u64,
    commit_limit: u64,
    committed_as: u64,
    /// SlateOS publishes this instead of `SwapFree`; see divergence 1.
    swap_used: u64,
    /// Whether a `SwapFree:` line was present at all. Keyed on presence rather
    /// than on the value, so a Linux machine whose swap is genuinely full — a
    /// real `SwapFree: 0` — is not rewritten.
    swap_free_seen: bool,
    /// Whether a `SwapUsed:` line was present, i.e. whether there is anything
    /// to substitute *from*.
    swap_used_seen: bool,
}

/// Read the `Key: <n> kB` lines, ignoring every key we have no field for.
///
/// procps splits on the first `:`, reads the remainder with `strtoul` and skips
/// to the next newline, so a key it does not know costs nothing and a line
/// without a `:` is skipped. Trailing units (`kB`) are ignored because
/// `strtoul` stops at the first non-digit.
fn parse_meminfo(text: &[u8]) -> Raw {
    let mut raw = Raw::default();
    for line in text.split(|&c| c == b'\n') {
        let Some(colon) = line.iter().position(|&c| c == b':') else {
            continue;
        };
        let (key, rest) = line.split_at(colon);
        // `split_at` leaves the `:` at the head of `rest`.
        let value = read_ul(rest.get(1..).unwrap_or_default());
        let slot: &mut u64 = match key {
            b"MemTotal" => &mut raw.mem_total,
            b"MemFree" => &mut raw.mem_free,
            b"MemAvailable" => &mut raw.mem_available,
            b"Buffers" => &mut raw.buffers,
            b"Cached" => &mut raw.cached,
            b"SReclaimable" => &mut raw.s_reclaimable,
            b"Shmem" => &mut raw.shmem,
            b"HighTotal" => &mut raw.high_total,
            b"HighFree" => &mut raw.high_free,
            b"LowTotal" => &mut raw.low_total,
            b"LowFree" => &mut raw.low_free,
            b"SwapTotal" => &mut raw.swap_total,
            b"SwapFree" => {
                raw.swap_free_seen = true;
                &mut raw.swap_free
            }
            b"SwapUsed" => {
                raw.swap_used_seen = true;
                &mut raw.swap_used
            }
            b"CommitLimit" => &mut raw.commit_limit,
            b"Committed_AS" => &mut raw.committed_as,
            _ => continue,
        };
        *slot = value;
    }
    raw
}

/// `strtoul(head, NULL, 10)` — leading whitespace, then digits, saturating.
///
/// Saturating rather than wrapping: `strtoul` clamps to `ULONG_MAX` on
/// overflow, and a `/proc/meminfo` line that overflows 64 bits is corrupt
/// rather than enormous, so the clamp is also the honest reading.
fn read_ul(text: &[u8]) -> u64 {
    let mut value: u64 = 0;
    let mut i = 0usize;
    while matches!(text.get(i), Some(c) if c.is_ascii_whitespace()) {
        i = i.saturating_add(1);
    }
    while let Some(&c) = text.get(i) {
        if !c.is_ascii_digit() {
            break;
        }
        value = value.saturating_mul(10).saturating_add(u64::from(c - b'0'));
        i = i.saturating_add(1);
    }
    value
}

/// Everything `free` actually prints, after procps' derivations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Mem {
    total: u64,
    free: u64,
    available: u64,
    buffers: u64,
    /// `Cached + SReclaimable` — procps' `derived_mem_cached`.
    cached_all: u64,
    /// `Shmem`.
    shared: u64,
    /// `MemTotal - MemAvailable`, or `MemTotal - MemFree` if that went
    /// negative. Stored as the `unsigned long` upstream stores it in, wrap and
    /// all, because the wrap is visible: see the `-1` row in the table above.
    used: u64,
    high_total: u64,
    high_free: u64,
    high_used: u64,
    low_total: u64,
    low_free: u64,
    low_used: u64,
    swap_total: u64,
    swap_free: u64,
    swap_used: u64,
    commit_limit: u64,
    committed_as: u64,
}

/// procps' `meminfo_read_failed()` tail, in its original order.
///
/// The order matters at least twice: `derived_mem_cached` is computed *before*
/// the `MemAvailable > MemTotal` guard rewrites `MemAvailable`, and the
/// `LowTotal == 0` substitution happens *after* `derived_mem_hi_used`, so a
/// kernel exporting neither still gets a `High:` row of zeroes.
fn derive(raw: &Raw) -> Mem {
    let mut m = Mem {
        total: raw.mem_total,
        free: raw.mem_free,
        available: raw.mem_available,
        buffers: raw.buffers,
        shared: raw.shmem,
        high_total: raw.high_total,
        high_free: raw.high_free,
        low_total: raw.low_total,
        low_free: raw.low_free,
        swap_total: raw.swap_total,
        swap_free: raw.swap_free,
        commit_limit: raw.commit_limit,
        committed_as: raw.committed_as,
        ..Mem::default()
    };

    // Divergence 1, applied before anything reads `swap_free`: without it the
    // `SwapFree < SwapTotal` test below is `0 < SwapTotal` and swap reads full.
    if !raw.swap_free_seen && raw.swap_used_seen {
        m.swap_free = raw.swap_total.saturating_sub(raw.swap_used);
    }

    // "if (0 == MemAvailable) MemAvailable = MemFree" — kernels before 3.14.
    if m.available == 0 {
        m.available = m.free;
    }
    m.cached_all = raw.cached.wrapping_add(raw.s_reclaimable);
    // The LXC guard: a container sees the host's MemAvailable against its own
    // MemTotal, which can be larger.
    if m.available > m.total {
        m.available = m.free;
    }
    // `long mem_used`, deliberately signed, and deliberately allowed to stay
    // negative on the second attempt.
    let signed = (m.total as i64).wrapping_sub(m.available as i64);
    let signed = if signed < 0 {
        (m.total as i64).wrapping_sub(m.free as i64)
    } else {
        signed
    };
    m.used = signed as u64;

    if m.high_free < m.high_total {
        m.high_used = m.high_total.wrapping_sub(m.high_free);
    }
    // A 64-bit kernel exports no Low*/High*; procps reads the whole of memory
    // as "low", which is the correct statement there.
    if m.low_total == 0 {
        m.low_total = m.total;
        m.low_free = m.free;
    }
    if m.low_free < m.low_total {
        m.low_used = m.low_total.wrapping_sub(m.low_free);
    }
    if m.swap_free < m.swap_total {
        m.swap_used = m.swap_total.wrapping_sub(m.swap_free);
    }
    m
}

// ---------------------------------------------------------------------------
// Formatting, transcribed from `scale_size()` and `print_head_col()`
// ---------------------------------------------------------------------------

/// Upstream's `double power(unsigned int base, unsigned int expo)`.
///
/// Repeated multiplication rather than `powi` so the transcription is visibly
/// the same computation; for the six exponents that reach it the two agree
/// exactly anyway, since 1024⁶ = 2⁶⁰ and 1000⁶ = 10¹⁸ are both exact `f64`.
fn power(base: u32, expo: u32) -> f64 {
    let mut acc = 1.0f64;
    for _ in 0..expo {
        acc *= f64::from(base);
    }
    acc
}

/// The unit letters, `{ 'B', 'K', 'M', 'G', 'T', 'P' }`.
const UNITS: [char; 6] = ['B', 'K', 'M', 'G', 'T', 'P'];

/// Render one kibibyte figure the way upstream does.
///
/// Human-readable mode does **not** threshold. It renders at each unit in turn
/// and returns the first rendering that fits the column — 4 characters for SI
/// (`1.1G`), 5 for binary (`1.0Mi`) — trying the one-decimal form before the
/// integer form at each unit. That is why `1023 kB` prints `1.0Mi` (the `Ki`
/// forms are 8 and 6 characters wide, both too wide) and `10240 kB` prints
/// `10Mi` rather than `10.0Mi`.
fn scale_size(size: u64, flags: Flags, args: CmdArgs) -> String {
    let base: u32 = if flags.si { 1000 } else { 1024 };
    // `bytes = size * 1024LL` in C converts both operands to `unsigned long`
    // and then assigns the result to a `long long`, so a `used` figure that
    // underflowed to `ULONG_MAX` reappears here as −1024. Reproducing the wrap
    // is what makes `free` print `used  -1` on a machine whose MemFree exceeds
    // its MemTotal, which is what upstream does.
    let bytes = size.wrapping_mul(1024) as i64;

    if !flags.human {
        return match args.exponent {
            // `bytes / (long long int)base` — the float base is truncated to an
            // integer first, so this is integer division.
            0 => (bytes / i64::from(base)).to_string(),
            1 => bytes.to_string(),
            e => trunc(bytes as f64 / power(base, e.saturating_sub(1))).to_string(),
        };
    }

    // `%lld%c` with 'B': the whole byte count, if it fits in four columns.
    let plain = format!("{}{}", bytes, UNITS[0]);
    if plain.len() <= 4 {
        return plain;
    }
    // Binary units carry a trailing 'i', so they are allowed one more column.
    let width = if flags.si { 4 } else { 5 };
    let tail = if flags.si { "" } else { "i" };
    let mut last = plain;
    for (i, unit) in UNITS.iter().enumerate().skip(1) {
        let scaled = bytes as f64 / power(base, u32::try_from(i).unwrap_or(0));
        // The C casts the quotient to `float` before handing it to `%.1f`,
        // which narrows it; `1099511 kB / 1000³` is 1.125898752 as a double and
        // 1.1258987 as a float, and both print `1.1`, but the narrowing is
        // visible at other values so it is reproduced rather than skipped.
        last = format!("{:.1}{}{}", scaled as f32, unit, tail);
        if last.len() <= width {
            return last;
        }
        last = format!("{}{}{}", trunc(scaled), unit, tail);
        if last.len() <= width {
            return last;
        }
    }
    // "On system where there is more than exbibyte of memory or swap the output
    // does not fit to column" — upstream returns the last, too-wide attempt.
    last
}

/// `(long)` applied to a double: truncation toward zero, saturating rather than
/// undefined at the edges.
fn trunc(value: f64) -> i64 {
    value as i64
}

/// Upstream's `print_head_col`: the label, then spaces out to 9 columns.
///
/// Upstream measures the label with `wcswidth` because the translated forms are
/// not ASCII. Ours are the untranslated ones — `Mem:`, `Low:`, `High:`,
/// `Swap:`, `Total:`, `Comm:` — all ASCII and all shorter than 9, but the
/// clamp is kept because the field is never allowed to grow.
fn head_col(label: &str) -> String {
    let spaces = 9usize.saturating_sub(label.chars().count());
    format!("{label}{:spaces$}", "", spaces = spaces)
}

/// The two header literals, measured byte for byte: 80 and 92 bytes.
///
/// They are literals rather than something assembled from column widths
/// because that is what they are upstream, and because the `buff/cache` column
/// is 11 wide with two leading spaces while `buffers` and `cache` are 11 wide
/// with five and seven — a shape no loop would produce by accident.
const HEADER_NARROW: &str =
    "               total        used        free      shared  buff/cache   available";
const HEADER_WIDE: &str =
    "               total        used        free      shared     buffers       cache   available";

/// One three-column row: a 9-column label then three `%11s` fields.
fn row3<W: Write>(
    out: &mut W,
    label: &str,
    values: [u64; 3],
    flags: Flags,
    args: CmdArgs,
) -> io::Result<()> {
    write!(out, "{}", head_col(label))?;
    write!(out, "{:>11}", scale_size(values[0], flags, args))?;
    write!(out, " {:>11}", scale_size(values[1], flags, args))?;
    writeln!(out, " {:>11}", scale_size(values[2], flags, args))
}

/// The default block: a header, `Mem:`, `Swap:`, and whatever `-l`, `-t` and
/// `-v` added.
fn print_table<W: Write>(out: &mut W, m: &Mem, flags: Flags, args: CmdArgs) -> io::Result<()> {
    writeln!(
        out,
        "{}",
        if flags.wide {
            HEADER_WIDE
        } else {
            HEADER_NARROW
        }
    )?;

    write!(out, "{}", head_col("Mem:"))?;
    write!(out, "{:>11}", scale_size(m.total, flags, args))?;
    write!(out, " {:>11}", scale_size(m.used, flags, args))?;
    write!(out, " {:>11}", scale_size(m.free, flags, args))?;
    write!(out, " {:>11}", scale_size(m.shared, flags, args))?;
    if flags.wide {
        write!(out, " {:>11}", scale_size(m.buffers, flags, args))?;
        write!(out, " {:>11}", scale_size(m.cached_all, flags, args))?;
    } else {
        let both = m.buffers.wrapping_add(m.cached_all);
        write!(out, " {:>11}", scale_size(both, flags, args))?;
    }
    writeln!(out, " {:>11}", scale_size(m.available, flags, args))?;

    // Upstream prints the `High:` row even when it is all zeroes: a kernel that
    // exports no high memory has none, and a blank row would be a different
    // claim from a row of zeroes.
    if flags.lohi {
        row3(
            out,
            "Low:",
            [m.low_total, m.low_used, m.low_free],
            flags,
            args,
        )?;
        row3(
            out,
            "High:",
            [m.high_total, m.high_used, m.high_free],
            flags,
            args,
        )?;
    }

    row3(
        out,
        "Swap:",
        [m.swap_total, m.swap_used, m.swap_free],
        flags,
        args,
    )?;

    if flags.total {
        row3(
            out,
            "Total:",
            [
                m.total.wrapping_add(m.swap_total),
                m.used.wrapping_add(m.swap_used),
                m.free.wrapping_add(m.swap_free),
            ],
            flags,
            args,
        )?;
    }
    if flags.committed {
        row3(
            out,
            "Comm:",
            [
                m.commit_limit,
                m.committed_as,
                m.commit_limit.wrapping_sub(m.committed_as),
            ],
            flags,
            args,
        )?;
    }
    Ok(())
}

/// `-L`: four `LABEL <11-wide value> ` groups, note the trailing space.
///
/// The labels are seven characters each — upstream's translator hint says so
/// explicitly — which is why ` MemUse` carries a leading space.
fn print_line<W: Write>(
    out: &mut W,
    m: &Mem,
    flags: Flags,
    args: CmdArgs,
    newline: bool,
) -> io::Result<()> {
    let cache = m.buffers.wrapping_add(m.cached_all);
    write!(out, "SwapUse {:>11} ", scale_size(m.swap_used, flags, args))?;
    write!(out, "CachUse {:>11} ", scale_size(cache, flags, args))?;
    write!(out, " MemUse {:>11} ", scale_size(m.used, flags, args))?;
    write!(out, "MemFree {:>11} ", scale_size(m.free, flags, args))?;
    if newline {
        writeln!(out)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

/// What a successful scan of `argv` asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Request {
    /// `--help`: the usage block on stdout, status 0.
    Help,
    /// `-V`/`--version`.
    Version,
    /// Print the table (or line), possibly repeatedly.
    Run { flags: Flags, args: CmdArgs },
}

/// A refusal, and whether the usage block follows it.
///
/// The two halves are independent upstream and both shapes occur: a bad option
/// prints getopt's sentence *and* the block, an operand prints the block with
/// no sentence at all, and `free -k -m` prints a sentence with no block.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fault {
    sentence: Option<String>,
    usage: bool,
}

impl Fault {
    /// `xerrx(EXIT_FAILURE, …)`: a sentence, no usage block.
    fn message(text: String) -> Self {
        Fault {
            sentence: Some(text),
            usage: false,
        }
    }
}

/// Bytes as they may be shown to a terminal.
fn shown(bytes: &[u8]) -> String {
    coreutils::quote::escape_unprintable(bytes)
}

/// `check_unit_set()` — the second unit option is fatal.
fn check_unit_set(unit_set: &mut bool) -> Result<(), Fault> {
    if *unit_set {
        return Err(Fault::message(
            "Multiple unit options don't make sense.".to_owned(),
        ));
    }
    *unit_set = true;
    Ok(())
}

/// Upstream's `getopt_long` loop plus the `optind != argc` check that follows
/// it.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per option, in upstream's order; splitting it would hide \
              the correspondence that makes this checkable against free.c"
)]
fn scan(argv: &[OsString]) -> Result<Request, Fault> {
    let mut flags = Flags::default();
    let mut args = CmdArgs::default();
    let mut unit_set = false;
    let mut operand = false;

    for item in FREE.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let opt = match item {
            Ok(opt) => opt,
            // getopt's own diagnostics are followed by the whole usage block,
            // not by the `Try 'free --help'` referral `Error::message` would
            // add, so only the sentence is taken.
            Err(e) => {
                return Err(Fault {
                    sentence: Some(e.sentence),
                    usage: true,
                });
            }
        };
        match opt {
            Opt::Short(b'b', _) | Opt::Long("bytes", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 1;
            }
            Opt::Short(b'k', _) | Opt::Long("kibi", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 2;
            }
            Opt::Short(b'm', _) | Opt::Long("mebi", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 3;
            }
            Opt::Short(b'g', _) | Opt::Long("gibi", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 4;
            }
            Opt::Long("tebi", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 5;
            }
            Opt::Long("pebi", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 6;
            }
            // The decimal family sets the same exponents and adds SI, so
            // `--mega` is `-m` measured in powers of 1000.
            Opt::Long("kilo", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 2;
                flags.si = true;
            }
            Opt::Long("mega", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 3;
                flags.si = true;
            }
            Opt::Long("giga", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 4;
                flags.si = true;
            }
            Opt::Long("tera", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 5;
                flags.si = true;
            }
            Opt::Long("peta", _) => {
                check_unit_set(&mut unit_set)?;
                args.exponent = 6;
                flags.si = true;
            }
            Opt::Short(b'h', _) | Opt::Long("human", _) => flags.human = true,
            Opt::Long("si", _) => flags.si = true,
            Opt::Short(b'l', _) | Opt::Long("lohi", _) => flags.lohi = true,
            Opt::Short(b'L', _) | Opt::Long("line", _) => flags.line = true,
            Opt::Short(b't', _) | Opt::Long("total", _) => flags.total = true,
            Opt::Short(b'v', _) | Opt::Long("committed", _) => flags.committed = true,
            Opt::Short(b'w', _) | Opt::Long("wide", _) => flags.wide = true,
            Opt::Short(b's', value) | Opt::Long("seconds", value) => {
                flags.repeat = true;
                let raw = value.unwrap_or_default();
                let text = coreutils::quote::os_bytes(&raw).into_owned();
                let seconds = strtod_nol(&text).map_err(|f| {
                    Fault::message(format!(
                        "seconds argument failed: '{}'{}",
                        shown(&text),
                        f.suffix()
                    ))
                })?;
                // `1000000 * <double>` assigned to a `float`, then compared
                // against 1 — so `-s 0.0000001` is refused for rounding to 0.1
                // microseconds and not for being negative.
                args.repeat_interval_us = (1_000_000.0 * seconds) as f32;
                if args.repeat_interval_us < 1.0 {
                    return Err(Fault::message(format!(
                        "seconds argument `{}' is not positive number",
                        shown(&text)
                    )));
                }
            }
            Opt::Short(b'c', value) | Opt::Long("count", value) => {
                flags.repeat = true;
                flags.repeat_count = true;
                let raw = value.unwrap_or_default();
                let text = coreutils::quote::os_bytes(&raw).into_owned();
                let count = strtol(&text).map_err(|f| {
                    Fault::message(format!(
                        "failed to parse count argument: '{}'{}",
                        shown(&text),
                        f.suffix()
                    ))
                })?;
                // Divergence 2: upstream lets the `long` truncate into an
                // `int`, so `-c 4294967297` prints once. Out of range is out of
                // range, and it reports the same `ERANGE` upstream reports for
                // the counts that happen to truncate below 1.
                let out_of_range = || {
                    Fault::message(format!(
                        "failed to parse count argument: '{}': Numerical result out of range",
                        shown(&text)
                    ))
                };
                let count = i32::try_from(count).map_err(|_| out_of_range())?;
                if count < 1 {
                    return Err(out_of_range());
                }
                args.repeat_counter = count;
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Short(b'V', _) | Opt::Long("version", _) => return Ok(Request::Version),
            // `if (optind != argc) usage(stderr)` — recorded rather than acted
            // on, because an option error later in the line is reported first.
            Opt::Operand(_) => operand = true,
            // Unreachable: every letter of `SHORT_OPTIONS` and every row of
            // `LONG_OPTIONS` is handled above.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }

    if operand {
        return Err(Fault {
            sentence: None,
            usage: true,
        });
    }
    Ok(Request::Run { flags, args })
}

// ---------------------------------------------------------------------------
// Driving it
// ---------------------------------------------------------------------------

/// Print a fault the way upstream does, and give back its exit status.
fn report<E: Write>(err: &mut E, fault: &Fault) -> u8 {
    if let Some(sentence) = &fault.sentence {
        // `\n` and not `writeln!`, so a stream that fails halfway leaves no
        // half-line behind.
        let _ = err.write_all(format!("free: {sentence}\n").as_bytes());
    }
    if fault.usage {
        let _ = err.write_all(HELP.as_bytes());
    }
    1
}

/// The `do … while (flags & FREE_REPEAT)` loop.
///
/// `read_meminfo` and `sleep` are supplied by the caller so the tests can drive
/// the repeat logic from fabricated files without waiting for anything, and can
/// count how many times the file was read — which is the whole of divergence 4.
fn print_all<W: Write, R: FnMut() -> io::Result<Vec<u8>>, S: FnMut(Duration)>(
    out: &mut W,
    flags: Flags,
    mut args: CmdArgs,
    read_meminfo: &mut R,
    sleep: &mut S,
) -> Result<io::Result<()>, Fault> {
    loop {
        let text = read_meminfo().map_err(|e| {
            Fault::message(if e.kind() == io::ErrorKind::NotFound {
                format!("Memory information file {MEMINFO_PATH} does not exist")
            } else {
                "Unable to create meminfo structure".to_owned()
            })
        })?;
        let mem = derive(&parse_meminfo(&text));

        let written = if flags.line {
            // Upstream withholds the newline while more iterations are coming,
            // and the loop tail supplies it instead.
            let newline = !flags.repeat || args.repeat_counter == 1;
            print_line(out, &mem, flags, args, newline)
        } else {
            print_table(out, &mem, flags, args)
        }
        .and_then(|()| out.flush());
        if written.is_err() {
            return Ok(written);
        }

        if flags.repeat_count {
            args.repeat_counter = args.repeat_counter.saturating_sub(1);
            if args.repeat_counter < 1 {
                return Ok(Ok(()));
            }
        }
        if !flags.repeat {
            return Ok(Ok(()));
        }
        if let Err(e) = writeln!(out) {
            return Ok(Err(e));
        }
        // The interval is a positive `float` of microseconds; `usleep` truncates
        // it to whole microseconds.
        sleep(Duration::from_micros(args.repeat_interval_us as u64));
    }
}

/// Everything between `main`'s braces, with the world supplied by the caller.
fn run<O: Write, E: Write, R: FnMut() -> io::Result<Vec<u8>>, S: FnMut(Duration)>(
    argv: &[OsString],
    out: &mut O,
    err: &mut E,
    read_meminfo: &mut R,
    sleep: &mut S,
) -> u8 {
    let request = match scan(argv) {
        Ok(request) => request,
        Err(fault) => return report(err, &fault),
    };
    let written = match request {
        Request::Help => out.write_all(HELP.as_bytes()),
        Request::Version => out.write_all(version_text().as_bytes()),
        Request::Run { flags, args } => match print_all(out, flags, args, read_meminfo, sleep) {
            Ok(written) => written,
            Err(fault) => return report(err, &fault),
        },
    };
    match written {
        Ok(()) => 0,
        // Unreachable through `Stream`, which records failures for
        // `close_stdout` rather than returning them; reached by a test's
        // deliberately-broken writer, and by anything else that grows a real
        // `io::Write` here later.
        Err(e) => report(
            err,
            &Fault::message(format!("write error: {}", coreutils::errmsg::strerror(&e))),
        ),
    }
}

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();

    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut out = Stream::stdout();
    let mut err = Stream::stderr();

    let mut read_meminfo = || std::fs::read(MEMINFO_PATH);
    let mut sleep = std::thread::sleep;

    let status = run(&argv, &mut out, &mut err, &mut read_meminfo, &mut sleep);
    stdfd::close_stdout("free", out, ExitCode::from(status))
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "panicking on bad data is the point of a test"
)]
mod tests {
    use super::{
        CmdArgs, Flags, MEMINFO_PATH, NumFault, Request, derive, parse_meminfo, run, scale_size,
        scan, strtod_nol, strtol,
    };
    use std::ffi::OsString;
    use std::io;
    use std::time::Duration;

    /// Width fitting across every unit boundary at once: nine fields whose
    /// values are 1..9 kibibytes.
    const A: &str = "MemTotal:              1 kB\n\
                     MemFree:               2 kB\n\
                     MemAvailable:          3 kB\n\
                     Buffers:               4 kB\n\
                     Cached:                5 kB\n\
                     SReclaimable:          6 kB\n\
                     Shmem:                 7 kB\n\
                     SwapTotal:             8 kB\n\
                     SwapFree:              9 kB\n";

    /// The exact powers, to see which unit each lands in.
    const B: &str = "MemTotal:           1024 kB\n\
                     MemFree:            1023 kB\n\
                     MemAvailable:       1000 kB\n\
                     Buffers:          1048576 kB\n\
                     Cached:           1073741 kB\n\
                     SReclaimable:           0 kB\n\
                     Shmem:            1099511 kB\n\
                     SwapTotal:     1073741824 kB\n\
                     SwapFree:      1000000000 kB\n";

    /// The `.1f`-versus-integer changeover: 9.9x, 10.0, 99.9, 100.
    const C: &str = "MemTotal:          10188 kB\n\
                     MemFree:           10240 kB\n\
                     MemAvailable:     102297 kB\n\
                     Buffers:          102400 kB\n\
                     Cached:                0 kB\n\
                     SReclaimable:          0 kB\n\
                     Shmem:              1023 kB\n\
                     SwapTotal:          1000 kB\n\
                     SwapFree:            999 kB\n";

    /// No `MemAvailable`, no `LowTotal`, no `SwapFree` — the substitution
    /// paths, and the shape this OS's `/proc/meminfo` has today.
    const D: &str = "MemTotal:        1000000 kB\n\
                     MemFree:          400000 kB\n\
                     Buffers:           10000 kB\n\
                     Cached:            20000 kB\n\
                     SReclaimable:       5000 kB\n\
                     Shmem:              3000 kB\n\
                     SwapTotal:       2000000 kB\n\
                     CommitLimit:      777777 kB\n\
                     Committed_AS:     111111 kB\n";

    /// `MemAvailable` greater than `MemTotal` — the LXC guard.
    const E: &str = "MemTotal:         100000 kB\n\
                     MemFree:           90000 kB\n\
                     MemAvailable:     200000 kB\n\
                     SwapTotal:         50000 kB\n\
                     SwapFree:          50000 kB\n";

    /// `HighTotal`/`LowTotal` actually present.
    const F: &str = "MemTotal:         100000 kB\n\
                     MemFree:           40000 kB\n\
                     MemAvailable:      60000 kB\n\
                     HighTotal:         30000 kB\n\
                     HighFree:          10000 kB\n\
                     LowTotal:          70000 kB\n\
                     LowFree:           30000 kB\n\
                     SwapTotal:         50000 kB\n\
                     SwapFree:          20000 kB\n";

    /// Run `free` over a fixed `/proc/meminfo` and collect both streams.
    fn render(meminfo: &str, argv: &[&str]) -> (String, String, u8) {
        let (out, err, status, _, _) = drive(meminfo, argv);
        (out, err, status)
    }

    /// The `n`th value column of a table row, counted after the 9-column label.
    fn col(line: &str, n: usize) -> &str {
        line.get(9..)
            .unwrap_or_default()
            .split_whitespace()
            .nth(n)
            .unwrap_or_default()
    }

    /// The same, reporting how many times the file was read and how long the
    /// run asked to sleep for in total.
    fn drive(meminfo: &str, argv: &[&str]) -> (String, String, u8, usize, Vec<Duration>) {
        let args: Vec<OsString> = argv.iter().map(OsString::from).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let body = meminfo.as_bytes().to_vec();
        let mut reads = 0usize;
        let mut naps: Vec<Duration> = Vec::new();
        let status = {
            let mut read = || -> io::Result<Vec<u8>> {
                reads += 1;
                Ok(body.clone())
            };
            let mut sleep = |d: Duration| naps.push(d);
            run(&args, &mut out, &mut err, &mut read, &mut sleep)
        };
        (
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            status,
            reads,
            naps,
        )
    }

    // -- the table, against procps-ng 4.0.4 under a bind-mounted meminfo -----

    #[test]
    fn default_table_matches_upstream() {
        let (out, err, status) = render(A, &[]);
        assert_eq!(
            out,
            "               total        used        free      shared  buff/cache   available\n\
             Mem:               1          -1           2           7          15           2\n\
             Swap:              8           0           9\n"
        );
        assert_eq!(err, "");
        assert_eq!(status, 0);
    }

    /// The `used` column goes negative when `MemFree` exceeds `MemTotal`: the
    /// subtraction underflows an `unsigned long` and `scale_size` multiplies it
    /// back into a `long long`, which is where the sign reappears.
    #[test]
    fn used_prints_negative_at_every_scale() {
        assert_eq!(col(render(A, &[]).0.lines().nth(1).unwrap(), 1), "-1");
        assert_eq!(
            col(render(A, &["-b"]).0.lines().nth(1).unwrap(), 1),
            "-1024"
        );
        assert!(render(A, &["-h"]).0.contains("-1Ki"));
        assert!(render(A, &["-h", "--si"]).0.contains(" -1K"));
    }

    #[test]
    fn human_readable_matches_upstream() {
        assert_eq!(
            render(A, &["-h"]).0,
            "               total        used        free      shared  buff/cache   available\n\
             Mem:           1.0Ki        -1Ki       2.0Ki       7.0Ki        15Ki       2.0Ki\n\
             Swap:          8.0Ki          0B       9.0Ki\n"
        );
        assert_eq!(
            render(A, &["-h", "--si"]).0,
            "               total        used        free      shared  buff/cache   available\n\
             Mem:            1.0K         -1K        2.0K        7.2K         15K        2.0K\n\
             Swap:           8.2K          0B        9.2K\n"
        );
    }

    /// The whole `-h` table at the values where the one-decimal form stops
    /// fitting, so a change to the fitting rule cannot pass unnoticed.
    #[test]
    fn human_readable_at_the_changeover() {
        assert_eq!(
            render(C, &["-h"]).0,
            "               total        used        free      shared  buff/cache   available\n\
             Mem:           9.9Mi       -52Ki        10Mi       1.0Mi       100Mi        10Mi\n\
             Swap:          1.0Mi       1.0Ki       999Ki\n"
        );
        assert_eq!(
            render(C, &["-h", "--si"]).0,
            "               total        used        free      shared  buff/cache   available\n\
             Mem:             10M        -53K         10M        1.0M        104M         10M\n\
             Swap:           1.0M        1.0K        1.0M\n"
        );
        // `MemAvailable` 102297 exceeds `MemTotal` 10188, so `available` is
        // `MemFree` and `used` goes negative.
        assert_eq!(col(render(C, &[]).0.lines().nth(1).unwrap(), 1), "-52");
    }

    #[test]
    fn wide_splits_buffers_from_cache() {
        assert_eq!(
            render(A, &["-w"]).0,
            "               total        used        free      shared     buffers       cache   available\n\
             Mem:               1          -1           2           7           4          11           2\n\
             Swap:              8           0           9\n"
        );
    }

    /// A kernel that exports no `LowTotal` is read as all-low, and the `High:`
    /// row is still printed — as zeroes, which is the true statement.
    #[test]
    fn lohi_substitutes_when_absent_and_uses_them_when_present() {
        assert_eq!(
            render(A, &["-l"]).0.lines().nth(2).unwrap(),
            "Low:               1           0           2"
        );
        assert_eq!(
            render(A, &["-l"]).0.lines().nth(3).unwrap(),
            "High:              0           0           0"
        );
        assert_eq!(
            render(F, &["-l"]).0.lines().nth(2).unwrap(),
            "Low:           70000       40000       30000"
        );
        assert_eq!(
            render(F, &["-l"]).0.lines().nth(3).unwrap(),
            "High:          30000       20000       10000"
        );
    }

    #[test]
    fn total_and_comm_rows() {
        assert_eq!(
            render(A, &["-t"]).0.lines().nth(3).unwrap(),
            "Total:             9          -1          11"
        );
        assert_eq!(
            render(D, &["-v"]).0.lines().nth(3).unwrap(),
            "Comm:         777777      111111      666666"
        );
    }

    #[test]
    fn scaled_units_match_upstream() {
        assert_eq!(
            render(B, &["-b"]).0.lines().nth(2).unwrap(),
            "Swap:    1099511627776 75511627776 1024000000000"
        );
        assert_eq!(
            render(B, &["-m"]).0.lines().nth(1).unwrap(),
            "Mem:               1           0           0        1073        2072           0"
        );
        assert_eq!(
            render(B, &["--mega"]).0.lines().nth(1).unwrap(),
            "Mem:               1           0           1        1125        2173           1"
        );
    }

    /// `MemAvailable` larger than `MemTotal` — which happens inside a
    /// container, where the file is the host's — falls back to `MemFree`.
    #[test]
    fn available_greater_than_total_falls_back_to_free() {
        assert_eq!(
            render(E, &[]).0.lines().nth(1).unwrap(),
            "Mem:          100000       10000       90000           0           0       90000"
        );
    }

    /// No `MemAvailable` line at all: pre-3.14 kernels, and this OS today.
    #[test]
    fn available_absent_falls_back_to_free() {
        assert_eq!(
            render(D, &[]).0.lines().nth(1).unwrap(),
            "Mem:         1000000      600000      400000        3000       35000      400000"
        );
    }

    #[test]
    fn cache_column_includes_reclaimable_slab() {
        // Buffers 10000 + Cached 20000 + SReclaimable 5000 = 35000.
        assert_eq!(col(render(D, &[]).0.lines().nth(1).unwrap(), 4), "35000");
        assert_eq!(
            render(D, &["-w"]).0.lines().nth(1).unwrap(),
            "Mem:         1000000      600000      400000        3000       10000       25000      400000"
        );
    }

    #[test]
    fn line_mode_matches_upstream() {
        assert_eq!(
            render(F, &["-L"]).0,
            "SwapUse       30000 CachUse           0  MemUse       40000 MemFree       40000 \n"
        );
        assert_eq!(
            render(F, &["-L", "-h"]).0,
            "SwapUse        29Mi CachUse          0B  MemUse        39Mi MemFree        39Mi \n"
        );
    }

    // -- the SlateOS divergence ---------------------------------------------

    /// `SwapUsed` present and `SwapFree` absent — this OS's file. Upstream's
    /// arithmetic would report swap entirely full; ours reports what the kernel
    /// said.
    #[test]
    fn swap_used_substitutes_for_an_absent_swap_free() {
        let slate = "MemTotal:        1000000 kB\n\
                     MemFree:          400000 kB\n\
                     SwapTotal:       2000000 kB\n\
                     SwapUsed:          64000 kB\n";
        assert_eq!(
            render(slate, &[]).0.lines().nth(2).unwrap(),
            "Swap:        2000000       64000     1936000"
        );
    }

    /// The substitution keys on the line being *absent*, not on its value. A
    /// Linux machine whose swap really is full prints full.
    #[test]
    fn a_genuine_swap_free_of_zero_is_left_alone() {
        let full = "MemTotal:        1000000 kB\n\
                    MemFree:          400000 kB\n\
                    SwapTotal:       2000000 kB\n\
                    SwapFree:              0 kB\n\
                    SwapUsed:        2000000 kB\n";
        assert_eq!(
            render(full, &[]).0.lines().nth(2).unwrap(),
            "Swap:        2000000     2000000           0"
        );
    }

    /// With neither key, upstream's own behaviour stands — which is the wrong
    /// answer this file's request to lane A exists to fix at the producer.
    #[test]
    fn neither_key_reproduces_upstream() {
        assert_eq!(
            render(D, &[]).0.lines().nth(2).unwrap(),
            "Swap:        2000000     2000000           0"
        );
    }

    // -- scale_size ---------------------------------------------------------

    fn human(kb: u64, si: bool) -> String {
        let flags = Flags {
            human: true,
            si,
            ..Flags::default()
        };
        scale_size(kb, flags, CmdArgs::default())
    }

    /// Upstream does not threshold: it takes the first rendering that fits the
    /// column, one decimal place first, at each unit in turn.
    #[test]
    fn human_widths_are_fitted_not_thresholded() {
        assert_eq!(human(10_188, false), "9.9Mi");
        assert_eq!(human(10_240, false), "10Mi");
        assert_eq!(human(1_023, false), "1.0Mi");
        assert_eq!(human(102_400, false), "100Mi");
        assert_eq!(human(999, false), "999Ki");
        assert_eq!(human(1_000, false), "1.0Mi");
        assert_eq!(human(1_073_741_824, false), "1.0Ti");
        assert_eq!(human(1_000_000_000, false), "953Gi");
        assert_eq!(human(0, false), "0B");
        assert_eq!(human(1_099_511, true), "1.1G");
        assert_eq!(human(73_741_824, true), "75G");
        assert_eq!(human(1_000_000_000, true), "1.0T");
    }

    /// A figure past pebibytes stops fitting; upstream returns the too-wide
    /// last attempt rather than truncating it. Measured with `MemTotal:
    /// 9007199254740991 kB`, which prints `Mem:          8191Pi` — six columns
    /// where the field allows five.
    #[test]
    fn beyond_pebi_overflows_the_column() {
        assert_eq!(human(9_007_199_254_740_991, false), "8191Pi");
    }

    #[test]
    fn non_human_exponents() {
        let at = |kb: u64, exponent: u32, si: bool| {
            scale_size(
                kb,
                Flags {
                    si,
                    ..Flags::default()
                },
                CmdArgs {
                    exponent,
                    ..CmdArgs::default()
                },
            )
        };
        assert_eq!(at(1_048_576, 0, false), "1048576");
        assert_eq!(at(1_048_576, 1, false), "1073741824");
        assert_eq!(at(1_048_576, 2, false), "1048576");
        assert_eq!(at(1_048_576, 3, false), "1024");
        assert_eq!(at(1_048_576, 4, false), "1");
        // `--si` with no `-h` divides by a thousand, not a kibi.
        assert_eq!(at(1_048_576, 0, true), "1073741");
        assert_eq!(at(1_048_576, 3, true), "1073");
    }

    // -- the number parsers -------------------------------------------------

    #[test]
    fn strtol_follows_the_c_function() {
        assert_eq!(strtol(b"5"), Ok(5));
        assert_eq!(strtol(b"  +5"), Ok(5));
        assert_eq!(strtol(b"-5"), Ok(-5));
        assert_eq!(strtol(b"-9223372036854775808"), Ok(i64::MIN));
        assert_eq!(strtol(b""), Err(NumFault::NoConversion));
        assert_eq!(strtol(b"abc"), Err(NumFault::NoConversion));
        assert_eq!(strtol(b"5x"), Err(NumFault::NoConversion));
        assert_eq!(strtol(b"5 "), Err(NumFault::NoConversion));
        assert_eq!(strtol(b"99999999999999999999"), Err(NumFault::Range));
    }

    /// procps' reader is locale-independent, which is why a comma is a radix
    /// point rather than a separator.
    #[test]
    fn strtod_nol_follows_the_c_function() {
        assert_eq!(strtod_nol(b"1"), Ok(1.0));
        assert_eq!(strtod_nol(b"1.5"), Ok(1.5));
        assert_eq!(strtod_nol(b"1,5"), Ok(1.5));
        assert_eq!(strtod_nol(b"-0.5"), Ok(-0.5));
        assert_eq!(strtod_nol(b"."), Ok(0.0));
        assert_eq!(strtod_nol(b"-"), Ok(0.0));
        assert_eq!(strtod_nol(b""), Err(NumFault::NoConversion));
        assert_eq!(strtod_nol(b"abc"), Err(NumFault::Invalid));
        // Junk *after* a fraction falls out of upstream's `if` with `errno`
        // still zero, so it is a different fault from junk instead of one.
        assert_eq!(strtod_nol(b"1.5x"), Err(NumFault::NoConversion));
        assert_eq!(strtod_nol(b"1e3"), Err(NumFault::Invalid));
    }

    // -- the command line ---------------------------------------------------

    #[test]
    fn help_and_version_go_to_stdout() {
        let (out, err, status) = render(A, &["--help"]);
        assert!(out.starts_with("\nUsage:\n free [options]\n"));
        assert!(out.ends_with("For more details see free(1).\n"));
        assert_eq!(out.len(), 1125);
        assert_eq!((err.as_str(), status), ("", 0));

        let (out, err, status) = render(A, &["-V"]);
        assert_eq!(out, "free from SlateOS coreutils 0.1.0\n");
        assert_eq!((err.as_str(), status), ("", 0));
    }

    /// Every command-line error prints the whole usage block, never the
    /// `Try 'free --help'` referral getopt would otherwise attach.
    #[test]
    fn bad_option_prints_the_sentence_then_the_block() {
        let (out, err, status) = render(A, &["-z"]);
        assert_eq!(out, "");
        assert_eq!(status, 1);
        assert!(err.starts_with("free: invalid option -- 'z'\n\nUsage:\n"));
        assert!(!err.contains("Try 'free --help'"));
    }

    /// An operand is fatal and carries no message at all — just the block.
    #[test]
    fn an_operand_prints_the_block_with_no_sentence() {
        let (out, err, status) = render(A, &["foo"]);
        assert_eq!(out, "");
        assert_eq!(status, 1);
        assert!(err.starts_with("\nUsage:\n free [options]\n"));
    }

    /// The ambiguity lists are the observable consequence of `LONG_OPTIONS`
    /// being in upstream's declaration order.
    #[test]
    fn ambiguous_prefixes_name_the_candidates_in_table_order() {
        for (prefix, possibilities) in [
            ("--k", "'--kilo' '--kibi'"),
            ("--te", "'--tera' '--tebi'"),
            ("--s", "'--si' '--seconds'"),
            ("--co", "'--committed' '--count'"),
        ] {
            let err = render(A, &[prefix]).1;
            assert!(
                err.starts_with(&format!(
                    "free: option '{prefix}' is ambiguous; possibilities: {possibilities}\n"
                )),
                "{prefix}: {err}"
            );
        }
        // `--li` is not ambiguous: only `--line` starts with it.
        assert_eq!(render(A, &["--li"]).0.lines().count(), 1);
    }

    #[test]
    fn a_second_unit_option_is_fatal_and_prints_no_block() {
        let (out, err, status) = render(A, &["-k", "-m"]);
        assert_eq!(out, "");
        assert_eq!(err, "free: Multiple unit options don't make sense.\n");
        assert_eq!(status, 1);
        // `-h` is not a unit, so it does not conflict with one.
        assert_eq!(render(A, &["-h", "-m"]).2, 0);
        // `--si` is not a unit either.
        assert_eq!(render(A, &["--si", "-m"]).2, 0);
        // But `--mega` is `-m` with SI, so it does.
        assert_eq!(render(A, &["--mega", "-m"]).2, 1);
    }

    #[test]
    fn seconds_diagnostics() {
        assert_eq!(
            render(A, &["-s", "abc"]).1,
            "free: seconds argument failed: 'abc': Invalid argument\n"
        );
        // Upstream zeroes `errno` on this path, so there is genuinely no
        // suffix — this one is a transcription, not a divergence.
        assert_eq!(
            render(A, &["-s", ""]).1,
            "free: seconds argument failed: ''\n"
        );
        assert_eq!(
            render(A, &["-s", "1.5x"]).1,
            "free: seconds argument failed: '1.5x'\n"
        );
        for zero in [".", ",", "-", "0", "-0.5", "0.0000001"] {
            assert_eq!(
                render(A, &["-s", zero]).1,
                format!("free: seconds argument `{zero}' is not positive number\n"),
                "{zero}"
            );
        }
    }

    #[test]
    fn count_diagnostics() {
        // Divergence 3: upstream appends a stale `strerror` here.
        assert_eq!(
            render(A, &["-c", ""]).1,
            "free: failed to parse count argument: ''\n"
        );
        assert_eq!(
            render(A, &["-c", "abc"]).1,
            "free: failed to parse count argument: 'abc'\n"
        );
        assert_eq!(
            render(A, &["-c", "0"]).1,
            "free: failed to parse count argument: '0': Numerical result out of range\n"
        );
        assert_eq!(
            render(A, &["-c", "99999999999999999999"]).1,
            "free: failed to parse count argument: '99999999999999999999': Numerical result out of range\n"
        );
        // Divergence 2: upstream truncates this `long` into an `int` and
        // prints once.
        assert_eq!(
            render(A, &["-c", "4294967297"]).1,
            "free: failed to parse count argument: '4294967297': Numerical result out of range\n"
        );
        assert_eq!(
            render(A, &["-c", "2147483648"]).1,
            "free: failed to parse count argument: '2147483648': Numerical result out of range\n"
        );
    }

    /// A control character in a rejected argument cannot reach the terminal
    /// unescaped.
    #[test]
    fn echoed_arguments_are_escaped() {
        assert_eq!(
            render(A, &["-c", "a\u{1b}[2Jb"]).1,
            "free: failed to parse count argument: 'a\\033[2Jb'\n"
        );
    }

    // -- repeating ----------------------------------------------------------

    #[test]
    fn count_repeats_that_many_times_with_a_blank_line_between() {
        let (out, err, status, reads, naps) = drive(F, &["-c", "3", "-s", "0.25"]);
        assert_eq!((err.as_str(), status), ("", 0));
        // Three tables, separated by a blank line, with none after the last.
        assert_eq!(out.matches("Mem:").count(), 3);
        assert_eq!(out.matches("\n\n").count(), 2);
        assert!(!out.ends_with("\n\n"));
        // Divergence 4: the file is read again for each iteration rather than
        // being cached for a second.
        assert_eq!(reads, 3);
        assert_eq!(naps, vec![Duration::from_micros(250_000); 2]);
    }

    /// `-L` withholds its newline until the last iteration, and the repeat tail
    /// supplies one instead — so two iterations give two lines, not a line and
    /// a blank.
    #[test]
    fn line_mode_repeats_without_blank_lines() {
        let out = drive(F, &["-L", "-c", "2"]).0;
        assert_eq!(out.lines().count(), 2);
        assert!(!out.contains("\n\n"));
        assert!(out.ends_with(" \n"));
    }

    /// `-s` alone never reaches the counter, so the `-L` newline never comes
    /// from the line itself.
    #[test]
    fn seconds_without_count_is_open_ended() {
        let flags = match scan(&[OsString::from("-s"), OsString::from("2")]).unwrap() {
            Request::Run { flags, args } => {
                assert_eq!(args.repeat_interval_us, 2_000_000.0);
                assert_eq!(args.repeat_counter, 0);
                flags
            }
            other => panic!("{other:?}"),
        };
        assert!(flags.repeat);
        assert!(!flags.repeat_count);
    }

    // -- reading the file ---------------------------------------------------

    #[test]
    fn a_missing_meminfo_is_named() {
        let args = [OsString::from("-b")];
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut read =
            || -> io::Result<Vec<u8>> { Err(io::Error::new(io::ErrorKind::NotFound, "nope")) };
        let mut sleep = |_: Duration| {};
        let status = run(&args, &mut out, &mut err, &mut read, &mut sleep);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            format!("free: Memory information file {MEMINFO_PATH} does not exist\n").as_bytes()
        );
    }

    #[test]
    fn any_other_read_failure_is_the_generic_sentence() {
        let args: [OsString; 0] = [];
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut read = || -> io::Result<Vec<u8>> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "nope"))
        };
        let mut sleep = |_: Duration| {};
        let status = run(&args, &mut out, &mut err, &mut read, &mut sleep);
        assert_eq!(status, 1);
        assert_eq!(err, b"free: Unable to create meminfo structure\n");
    }

    /// A key with no field is skipped, a line with no colon is skipped, and the
    /// `kB` suffix stops the number rather than joining it.
    #[test]
    fn unknown_keys_and_junk_lines_are_ignored() {
        let raw = parse_meminfo(
            b"MemTotal:       1024 kB\n\
              ZeroPoolHits:      7\n\
              this line has no colon\n\
              MemFree:         512 kB\n",
        );
        assert_eq!(raw.mem_total, 1024);
        assert_eq!(raw.mem_free, 512);
        assert!(!raw.swap_free_seen);
    }

    /// Every value the table prints, in one place, for the shape this OS has.
    #[test]
    fn derivation_of_the_slateos_shape() {
        let mem = derive(&parse_meminfo(D.as_bytes()));
        assert_eq!(mem.total, 1_000_000);
        assert_eq!(mem.free, 400_000);
        assert_eq!(mem.available, 400_000, "MemAvailable absent -> MemFree");
        assert_eq!(mem.used, 600_000);
        assert_eq!(mem.cached_all, 25_000, "Cached + SReclaimable");
        assert_eq!(mem.shared, 3_000);
        assert_eq!(mem.low_total, 1_000_000, "LowTotal absent -> MemTotal");
        assert_eq!(mem.low_used, 600_000);
        assert_eq!(mem.high_total, 0);
        assert_eq!(mem.commit_limit, 777_777);
        assert_eq!(mem.committed_as, 111_111);
    }

    /// The header is a fixed-width literal that scripts cut by offset; its
    /// length is part of the interface.
    #[test]
    fn header_widths() {
        // Nine columns of label, then 11-wide fields separated by one space:
        // 9 + 11 + 5×12 = 80 narrow, 9 + 11 + 6×12 = 92 wide.
        assert_eq!(render(A, &[]).0.lines().next().unwrap().len(), 80);
        assert_eq!(render(A, &["-w"]).0.lines().next().unwrap().len(), 92);
        for row in render(A, &["-l", "-t", "-v"]).0.lines().skip(1) {
            // Every row is either the six-column `Mem:` row or a three-column
            // one, and no label spills past its nine columns.
            assert!(row.len() == 44 || row.len() == 80, "{row:?}");
            assert!(row.get(..9).unwrap().ends_with(' '), "{row:?}");
        }
    }
}
