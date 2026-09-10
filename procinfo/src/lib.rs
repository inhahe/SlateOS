//! Reading and parsing the kernel's `/proc` and `/sys` interfaces.
//!
//! This crate is the *facts* half of a system-information program: it opens
//! the kernel's pseudo-files, parses them into typed values, and stops. It
//! prints nothing, formats nothing, and has no opinion about columns. The two
//! system-information programs in this tree — `userspace/sysinfo` (the CLI)
//! and `apps/sysinfo` (the graphical one) — differ entirely in the second
//! half and not at all in this one.
//!
//! # Why it exists
//!
//! Requested by lane C in
//! `requests/c-b-the-proc-readers-in-userspace-sysinfo-should-be-a-crate-both-sysinfos-can-use.md`.
//! `userspace/sysinfo` read real data; `apps/sysinfo` displayed integer
//! literals and a hardcoded uptime string. The fix that does not end in two
//! parsers of `/proc/meminfo` is the one that gives the first parser a name
//! the other program can `use` — the same argument that moved `deflate` and
//! `ziparchive` out of the kernel binary, one level down.
//!
//! Two `/proc` parsers in one repository is the arrangement where a kernel
//! change fixes one program and not the other, and nobody notices, because
//! both still produce numbers.
//!
//! # Three things this crate does that the code it replaces did not
//!
//! **1. It distinguishes "the kernel does not export this" from "we could not
//! read it".** The original `read_proc` was `fs::read_to_string(path).ok()`,
//! so a permission error, an I/O error and a file that does not exist on this
//! kernel all arrived as `None` and were reported to the user as
//! "(cpuinfo not available)". That is a lie in two of the three cases, and it
//! is the case where something is actually wrong that it lies about.
//! [`ProcFs::read_optional`] maps only [`io::ErrorKind::NotFound`] to
//! `Ok(None)` and propagates everything else.
//!
//! **2. It treats `/proc` content as bytes.** A mount point is a path, and a
//! SlateOS path is any bytes except `/` and NUL (`design.txt`; CLAUDE.md
//! self-review item 7). Parsing `/proc/mounts` through `String` means a mount
//! point that is not UTF-8 either disappears or is silently corrupted by a
//! lossy conversion. Every field this crate extracts from a path-bearing file
//! is a `Vec<u8>`.
//!
//! **3. It undoes the kernel's octal escaping.** `/proc/mounts` is
//! whitespace-separated, so the kernel escapes space, tab, newline and
//! backslash inside the device and mount-point fields as `\040`, `\011`,
//! `\012` and `\134` (Linux `fs/proc_namespace.c`, `mangle()` over
//! `seq_escape`). Code that splits on whitespace and prints the pieces gets
//! the field boundaries right and the *contents* wrong: a mount at
//! `/mnt/my backup` displays as `/mnt/my\040backup`. See [`unescape_octal`].
//!
//! # Testing
//!
//! Everything here is testable on the development host, which does not have a
//! `/proc`. The parsers are pure functions over `&[u8]` and are tested against
//! captured content; the reader is [`ProcFs::at`], which takes the directory
//! to read *as an argument* rather than hardcoding `/proc`, so the collectors
//! are tested against a fixture tree. A reader that can only be pointed at the
//! real thing is a reader whose error handling is never exercised, and error
//! handling is most of what item 1 above is about.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ============================================================================
// Byte-level helpers
// ============================================================================

/// Undo the octal escaping the kernel applies to whitespace-separated fields.
///
/// `/proc/mounts`, `/proc/self/mountinfo` and `/proc/swaps` are all
/// whitespace-separated, and all of them can contain a path with a space in
/// it. The kernel resolves that by escaping the four characters that would
/// break the format — space, tab, newline and backslash — as a backslash
/// followed by exactly three octal digits (`\040`, `\011`, `\012`, `\134`).
///
/// A backslash that is *not* followed by three octal digits is not an escape
/// and is returned as itself. That is deliberate and it is the conservative
/// direction: the kernel never emits such a sequence, so encountering one
/// means the input did not come from the kernel, and inventing a byte for it
/// would corrupt a path that a caller may be about to act on.
///
/// ```
/// # use procinfo::unescape_octal;
/// assert_eq!(unescape_octal(br"/mnt/my\040backup"), b"/mnt/my backup".to_vec());
/// assert_eq!(unescape_octal(br"/mnt/c\134d"), br"/mnt/c\d".to_vec());
/// assert_eq!(unescape_octal(br"/mnt/\09"), br"/mnt/\09".to_vec());
/// ```
#[must_use]
pub fn unescape_octal(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0usize;
    while let Some(&byte) = field.get(i) {
        // An escape is exactly four bytes: `\` and three octal digits. Anything
        // shorter or with a non-octal digit is not one.
        let escape = if byte == b'\\' {
            let digits = field.get(i.saturating_add(1)..i.saturating_add(4));
            digits.and_then(|d| {
                let mut value: u32 = 0;
                for &digit in d {
                    let place = digit.checked_sub(b'0')?;
                    if place > 7 {
                        return None;
                    }
                    value = value.checked_mul(8)?.checked_add(u32::from(place))?;
                }
                u8::try_from(value).ok()
            })
        } else {
            None
        };

        if let Some(decoded) = escape {
            out.push(decoded);
            i = i.saturating_add(4);
        } else {
            out.push(byte);
            i = i.saturating_add(1);
        }
    }
    out
}

/// Split a line on runs of ASCII whitespace, discarding empty fields.
///
/// `str::split_whitespace` for bytes. Present because every file this crate
/// reads that is not key-value is whitespace-separated, and because doing it
/// through `String` is exactly the conversion the crate exists to avoid.
fn split_ws(line: &[u8]) -> Vec<&[u8]> {
    line.split(|b| b.is_ascii_whitespace())
        .filter(|f| !f.is_empty())
        .collect()
}

/// Trim leading and trailing ASCII whitespace from a byte slice.
#[must_use]
fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |p| p.saturating_add(1));
    bytes.get(start..end).unwrap_or(&[])
}

/// Interpret a field as UTF-8, or return `None`.
///
/// Deliberately not lossy. A caller that gets `None` here knows the field was
/// not text and can print the bytes; a caller handed a string full of U+FFFD
/// knows nothing and has already lost the data (CLAUDE.md self-review item 7).
#[must_use]
fn as_str(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

/// Parse an ASCII decimal integer out of a byte field.
fn parse_u64(bytes: &[u8]) -> Option<u64> {
    as_str(trim(bytes))?.parse().ok()
}

/// Parse a **signed** ASCII decimal out of a byte field.
///
/// `nice` is the only signed thing `/proc` reports that this crate reads, and
/// it goes negative for every process a user has bothered to prioritise.
fn parse_i64(bytes: &[u8]) -> Option<i64> {
    as_str(trim(bytes))?.parse().ok()
}

/// Parse an ASCII decimal/float out of a byte field.
fn parse_f64(bytes: &[u8]) -> Option<f64> {
    as_str(trim(bytes))?.parse().ok()
}

// ============================================================================
// Key-value files (`/proc/cpuinfo`, `/proc/meminfo`, `/proc/self/status`, …)
// ============================================================================

/// One `key: value` line of a `/proc` key-value file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValue {
    /// The text left of the separator, trimmed.
    pub key: Vec<u8>,
    /// The text right of the separator, trimmed.
    pub value: Vec<u8>,
}

impl KeyValue {
    /// The key as UTF-8, or `None` if it is not.
    #[must_use]
    pub fn key_str(&self) -> Option<&str> {
        as_str(&self.key)
    }

    /// The value as UTF-8, or `None` if it is not.
    #[must_use]
    pub fn value_str(&self) -> Option<&str> {
        as_str(&self.value)
    }
}

/// Parse every `key: value` (or `key<TAB>value`) line of a `/proc` file.
///
/// Blank lines and lines with no separator are skipped: `/proc/cpuinfo`
/// separates its per-CPU blocks with a blank line and there is nothing to
/// report about it. Order is preserved, and duplicate keys are *kept* —
/// `/proc/cpuinfo` repeats every key once per logical CPU, and collapsing
/// them into a map is how a reader ends up reporting one core on a
/// sixteen-core machine.
#[must_use]
pub fn parse_key_values(content: &[u8]) -> Vec<KeyValue> {
    let mut pairs = Vec::new();
    for line in content.split(|&b| b == b'\n') {
        let line = trim(line);
        if line.is_empty() {
            continue;
        }
        // Colon first, then tab: `/proc/net/dev` and `/proc/cpuinfo` both use
        // a colon, `/proc/self/statm`-style files use a tab, and a value can
        // itself contain a tab (`cpuinfo`'s `flags`), so the colon must win.
        let cut = line
            .iter()
            .position(|&b| b == b':')
            .or_else(|| line.iter().position(|&b| b == b'\t'));
        let Some(cut) = cut else { continue };
        let (key, rest) = line.split_at(cut);
        // `rest` starts with the separator itself, which is one byte.
        let value = rest.get(1..).unwrap_or(&[]);
        pairs.push(KeyValue {
            key: trim(key).to_vec(),
            value: trim(value).to_vec(),
        });
    }
    pairs
}

/// The first value whose key is exactly `key`.
///
/// "First", not "only": see [`parse_key_values`] on duplicate keys.
#[must_use]
pub fn key_value(content: &[u8], key: &str) -> Option<Vec<u8>> {
    parse_key_values(content)
        .into_iter()
        .find(|kv| kv.key == key.as_bytes())
        .map(|kv| kv.value)
}

/// Parse a `/proc/meminfo`-style `"12345 kB"` value into kibibytes.
///
/// Returns `None` for a value with a unit this does not know, rather than
/// guessing: `meminfo` has only ever emitted `kB`, so an unrecognised unit
/// means the format changed and a silently-wrong number is worse than a
/// missing one.
#[must_use]
pub fn parse_kib(value: &[u8]) -> Option<u64> {
    let value = trim(value);
    let text = as_str(value)?;
    let number = match text.strip_suffix("kB").or_else(|| text.strip_suffix("KB")) {
        Some(head) => head.trim_end(),
        // A bare number is kB by meminfo's convention; anything else is a unit
        // we do not know.
        None if text.bytes().all(|b| b.is_ascii_digit()) => text,
        None => return None,
    };
    number.parse().ok()
}

// ============================================================================
// /proc/cpuinfo
// ============================================================================

/// What `/proc/cpuinfo` says about this machine's processors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuInfo {
    /// `model name` of the first logical CPU.
    pub model: Option<Vec<u8>>,
    /// `vendor_id` of the first logical CPU.
    pub vendor: Option<Vec<u8>>,
    /// Number of `processor` lines, i.e. logical CPUs. Never reported as zero:
    /// a `/proc/cpuinfo` that exists but names no processor is a format we do
    /// not understand, and [`CpuInfo::parse`] returns `None` for it.
    pub logical_cpus: usize,
    /// `cpu MHz` of the first logical CPU, as written.
    pub mhz: Option<Vec<u8>>,
    /// `cache size` of the first logical CPU, as written.
    pub cache: Option<Vec<u8>>,
}

impl CpuInfo {
    /// Parse `/proc/cpuinfo`.
    ///
    /// Returns `None` when the content names no `processor` at all, which is
    /// the only way this file can be present and meaningless. The previous
    /// code substituted `1` for a zero count, so an unparseable `cpuinfo` and
    /// a genuine uniprocessor were indistinguishable in the output.
    #[must_use]
    pub fn parse(content: &[u8]) -> Option<Self> {
        let pairs = parse_key_values(content);
        let first = |name: &str| {
            pairs
                .iter()
                .find(|kv| kv.key == name.as_bytes())
                .map(|kv| kv.value.clone())
        };
        let logical_cpus = pairs.iter().filter(|kv| kv.key == b"processor").count();
        if logical_cpus == 0 {
            return None;
        }
        Some(Self {
            model: first("model name"),
            vendor: first("vendor_id"),
            logical_cpus,
            mhz: first("cpu MHz"),
            cache: first("cache size"),
        })
    }
}

// ============================================================================
// /proc/meminfo
// ============================================================================

/// What `/proc/meminfo` says, in kibibytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemInfo {
    /// `MemTotal`.
    pub total_kib: Option<u64>,
    /// `MemFree`.
    pub free_kib: Option<u64>,
    /// `MemAvailable`.
    pub available_kib: Option<u64>,
    /// `Buffers`.
    pub buffers_kib: Option<u64>,
    /// `Cached`.
    pub cached_kib: Option<u64>,
    /// `SwapTotal`.
    pub swap_total_kib: Option<u64>,
    /// `SwapFree`.
    pub swap_free_kib: Option<u64>,
    /// `Shmem` — memory held in tmpfs and shared mappings. `free` subtracts it
    /// from the cache column, because a shared page is not reclaimable the way
    /// a page-cache page is.
    pub shmem_kib: Option<u64>,
    /// `SReclaimable` — the reclaimable half of slab. `free` counts it as
    /// buff/cache rather than as used.
    pub sreclaimable_kib: Option<u64>,
    /// `CommitLimit` — **never published by this kernel.** Always `None` here.
    pub commit_limit_kib: Option<u64>,
    /// `Committed_AS` — **never published by this kernel.** Always `None` here.
    pub committed_as_kib: Option<u64>,
    /// `HighTotal` — **never published by this kernel.** Always `None` here.
    pub high_total_kib: Option<u64>,
    /// `LowTotal` — **never published by this kernel.** Always `None` here.
    pub low_total_kib: Option<u64>,
}

impl MemInfo {
    /// Parse `/proc/meminfo`.
    ///
    /// # Four of these fields are always `None` on SlateOS, on purpose
    ///
    /// `CommitLimit`, `Committed_AS`, `HighTotal` and `LowTotal` are parsed
    /// because the format defines them and a parser should read what it is
    /// given, but `kernel/src/fs/procfs.rs::gen_meminfo` deliberately does not
    /// emit any of them, and its reasoning is worth not re-litigating from a
    /// caller:
    ///
    /// - `High*`/`Low*` are a 32-bit-x86 concept. procps substitutes
    ///   `LowTotal = MemTotal` when they are absent, which is the correct
    ///   reading on a 64-bit machine — so a caller wanting `-l` output should
    ///   do that substitution rather than print a zero.
    /// - `CommitLimit`/`Committed_AS` exist only to report Linux's strict
    ///   commit accounting (`overcommit_memory = 2`), which this kernel does
    ///   not perform. Publishing `Committed_AS` without a limit would print a
    ///   real numerator over a zero denominator in `free -v`, which reads as a
    ///   machine committed past its limit.
    ///
    /// So `None` here means "this kernel does not account for that", not "the
    /// read failed". A caller must not turn it into `0`.
    #[must_use]
    pub fn parse(content: &[u8]) -> Self {
        let pairs = parse_key_values(content);
        let field = |name: &str| {
            pairs
                .iter()
                .find(|kv| kv.key == name.as_bytes())
                .and_then(|kv| parse_kib(&kv.value))
        };
        Self {
            total_kib: field("MemTotal"),
            free_kib: field("MemFree"),
            available_kib: field("MemAvailable"),
            buffers_kib: field("Buffers"),
            cached_kib: field("Cached"),
            swap_total_kib: field("SwapTotal"),
            swap_free_kib: field("SwapFree"),
            shmem_kib: field("Shmem"),
            sreclaimable_kib: field("SReclaimable"),
            commit_limit_kib: field("CommitLimit"),
            committed_as_kib: field("Committed_AS"),
            high_total_kib: field("HighTotal"),
            low_total_kib: field("LowTotal"),
        }
    }

    /// Memory in use, in kibibytes: total minus free.
    ///
    /// `None` unless *both* figures were present. The version this replaces
    /// printed a percentage computed from `"?"` parsed as zero in some paths;
    /// a used-memory figure derived from a missing total is not a number worth
    /// showing.
    #[must_use]
    pub fn used_kib(&self) -> Option<u64> {
        let total = self.total_kib?;
        let free = self.free_kib?;
        Some(total.saturating_sub(free))
    }

    /// Memory in use **excluding** what the kernel can reclaim: total minus
    /// free, buffers and page cache.
    ///
    /// # Two "used" figures, and why both are here
    ///
    /// [`MemInfo::used_kib`] is `total - free`, which counts the page cache as
    /// used -- true of the kernel's bookkeeping and misleading to a person,
    /// because a machine with 30 GiB of cache is not short of memory. This is
    /// the figure `htop` and `free` put in front of a user.
    ///
    /// Neither is wrong; they answer different questions, and the reason they
    /// are two named methods rather than one is that every program that
    /// computed this for itself picked one silently. `None` unless every
    /// figure it needs was present, for [`MemInfo::used_kib`]'s reason: a
    /// number derived from a missing total is not worth showing.
    #[must_use]
    pub fn used_excluding_cache_kib(&self) -> Option<u64> {
        let total = self.total_kib?;
        let free = self.free_kib?;
        Some(
            total
                .saturating_sub(free)
                .saturating_sub(self.buffers_kib?)
                .saturating_sub(self.cached_kib?),
        )
    }

    /// Swap in use: total minus free.
    ///
    /// `None` unless both were present. A machine with no swap reports
    /// `SwapTotal: 0`, which is `Some(0)` -- distinct from a kernel that does
    /// not export the field at all.
    #[must_use]
    pub fn swap_used_kib(&self) -> Option<u64> {
        let total = self.swap_total_kib?;
        let free = self.swap_free_kib?;
        Some(total.saturating_sub(free))
    }

    /// Fraction of memory in use, 0.0–100.0.
    ///
    /// `None` when either figure is missing or the total is zero — dividing by
    /// a zero total is how a `/proc/meminfo` from a kernel that does not fill
    /// it in becomes `NaN%` on screen.
    // The counts are kibibytes, so `u64` values that could lose precision as
    // `f64` describe more than 8 zebibytes of RAM; and the result is a
    // percentage displayed to one decimal either way.
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn used_percent(&self) -> Option<f64> {
        let total = self.total_kib?;
        if total == 0 {
            return None;
        }
        Some((self.used_kib()? as f64 / total as f64) * 100.0)
    }
}

// ============================================================================
// /proc/mounts
// ============================================================================

/// One line of `/proc/mounts` (or `/etc/mtab`, which has the same format).
///
/// The two path-bearing fields are `Vec<u8>` and are stored **unescaped** —
/// the kernel's `\040`-style escaping is undone by [`Mount::parse_line`], so
/// these are the real bytes of the real path. A caller that wants to write the
/// line back out to something that re-parses it must escape it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// Field 1: the device, or a pseudo-device name such as `proc` or `tmpfs`.
    pub device: Vec<u8>,
    /// Field 2: where it is mounted.
    pub mount_point: Vec<u8>,
    /// Field 3: the filesystem type.
    pub fstype: Vec<u8>,
    /// Field 4: the comma-separated mount options.
    pub options: Vec<u8>,
    /// Field 5: the `dump` frequency. Always 0 from the kernel; kept because
    /// `/etc/mtab` written by other tools need not be.
    pub dump: u64,
    /// Field 6: the `fsck` pass number.
    pub pass: u64,
}

impl Mount {
    /// Parse one line, or `None` if it does not have the six fields.
    #[must_use]
    pub fn parse_line(line: &[u8]) -> Option<Self> {
        let fields = split_ws(line);
        // Six fields exactly is what the kernel writes. Accept four so an
        // `/etc/mtab` that omits the two numeric ones still parses, but do not
        // accept fewer: at three the options field is missing, and a mount
        // shown without its options can be shown as read-write when it is not.
        let device = fields.first()?;
        let mount_point = fields.get(1)?;
        let fstype = fields.get(2)?;
        let options = fields.get(3)?;
        Some(Self {
            device: unescape_octal(device),
            mount_point: unescape_octal(mount_point),
            // fstype and options are kernel identifiers and never contain
            // whitespace, so they are never escaped -- but a hand-written
            // /etc/mtab may escape them anyway, and unescaping something that
            // contains no escapes is the identity.
            fstype: unescape_octal(fstype),
            options: unescape_octal(options),
            dump: fields.get(4).and_then(|f| parse_u64(f)).unwrap_or(0),
            pass: fields.get(5).and_then(|f| parse_u64(f)).unwrap_or(0),
        })
    }

    /// Parse a whole `/proc/mounts`.
    ///
    /// Lines that do not parse are skipped. They are not an error: a
    /// `/proc/mounts` is read without locking and a mount can vanish between
    /// the kernel formatting one line and the next.
    #[must_use]
    pub fn parse_all(content: &[u8]) -> Vec<Self> {
        content
            .split(|&b| b == b'\n')
            .filter_map(Self::parse_line)
            .collect()
    }

    /// The mount options split on commas, in order.
    #[must_use]
    pub fn option_list(&self) -> Vec<&[u8]> {
        self.options
            .split(|&b| b == b',')
            .filter(|o| !o.is_empty())
            .collect()
    }

    /// Whether the mount carries a given option, e.g. `ro`.
    ///
    /// Matches a whole option, so `ro` does not match `rootcontext=…` — which
    /// a substring search does, and which would report a read-write mount as
    /// read-only.
    #[must_use]
    pub fn has_option(&self, name: &str) -> bool {
        self.option_list().contains(&name.as_bytes())
    }

    /// Whether the mount is read-only.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.has_option("ro")
    }
}

// ============================================================================
// /proc/loadavg, /proc/uptime, /proc/stat
// ============================================================================

/// `/proc/loadavg`: `0.00 0.01 0.05 1/234 5678`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadAvg {
    /// One-minute load average.
    pub one: f64,
    /// Five-minute load average.
    pub five: f64,
    /// Fifteen-minute load average.
    pub fifteen: f64,
    /// Runnable entities (the numerator of the fourth field).
    pub runnable: Option<u64>,
    /// Total entities (the denominator of the fourth field).
    pub total: Option<u64>,
    /// The most recently created PID (the fifth field).
    pub last_pid: Option<u64>,
}

impl LoadAvg {
    /// Parse `/proc/loadavg`. `None` unless the three averages are all present.
    #[must_use]
    pub fn parse(content: &[u8]) -> Option<Self> {
        let fields = split_ws(content);
        let one = parse_f64(fields.first()?)?;
        let five = parse_f64(fields.get(1)?)?;
        let fifteen = parse_f64(fields.get(2)?)?;
        let (runnable, total) = match fields.get(3) {
            Some(entities) => {
                let mut halves = entities.split(|&b| b == b'/');
                let run = halves.next().and_then(parse_u64);
                let tot = halves.next().and_then(parse_u64);
                (run, tot)
            }
            None => (None, None),
        };
        Some(Self {
            one,
            five,
            fifteen,
            runnable,
            total,
            last_pid: fields.get(4).and_then(|f| parse_u64(f)),
        })
    }
}

/// `/proc/uptime`: seconds since boot, and aggregate idle seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uptime {
    /// Time since boot.
    pub up: Duration,
    /// Summed idle time across all CPUs, if the kernel reports it.
    pub idle: Option<Duration>,
}

impl Uptime {
    /// Parse `/proc/uptime`. `None` if the first field is not a number.
    #[must_use]
    pub fn parse(content: &[u8]) -> Option<Self> {
        let fields = split_ws(content);
        let up = seconds_to_duration(parse_f64(fields.first()?)?)?;
        Some(Self {
            up,
            idle: fields
                .get(1)
                .and_then(|f| parse_f64(f))
                .and_then(seconds_to_duration),
        })
    }

    /// Uptime broken into whole days, hours, minutes and seconds.
    #[must_use]
    pub fn dhms(&self) -> (u64, u64, u64, u64) {
        let secs = self.up.as_secs();
        (
            secs / 86_400,
            (secs % 86_400) / 3_600,
            (secs % 3_600) / 60,
            secs % 60,
        )
    }
}

/// Seconds as a float to a `Duration`, rejecting negatives and NaN.
///
/// `Duration::from_secs_f64` panics on those, and `/proc/uptime` is a file
/// this crate does not control the contents of.
fn seconds_to_duration(secs: f64) -> Option<Duration> {
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Duration::try_from_secs_f64(secs).ok()
}

/// `/proc/stat` in full, from one read: see [`ProcFs::stat`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stat {
    /// The `cpu` and `cpuN` lines.
    pub cpu: CpuStats,
    /// Everything else. See [`StatCounters`].
    pub counters: StatCounters,
}

/// Everything `/proc/stat` says apart from the CPU lines, which are
/// [`CpuStats`].
///
/// # Why it is named for the file rather than for the scheduler
///
/// It was `SchedCounters`, holding only `procs_running`, `procs_blocked` and
/// `processes`. It grew `intr`, `ctxt` and `btime` on 2026-09-10, when
/// `userspace/vmstat` -- which had its own copy of this parser, and its own
/// `CpuTimes` beside it -- was converted to use this crate. A boot *timestamp*
/// is not a scheduler counter, and leaving the name would have meant either a
/// name that lied or a second struct for the same three-line pass over the
/// same file.
///
/// Every field is `Option` because every line is optional: `proc(5)` does not
/// promise `btime` or `steal`, and a kernel that omits one is not malformed.
/// `None` means "the file did not say", which is a different fact from zero
/// and is the caller's to interpret.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatCounters {
    /// `procs_running`: tasks currently on a run queue.
    pub running: Option<u64>,
    /// `procs_blocked`: tasks blocked on I/O.
    pub blocked: Option<u64>,
    /// `processes`: total forks since boot.
    pub forks: Option<u64>,
    /// `ctxt`: context switches since boot.
    pub context_switches: Option<u64>,
    /// `intr`: total interrupts since boot.
    ///
    /// The *first* number on the `intr` line. The rest are per-IRQ counts and
    /// there can be hundreds of them, so a reader that wants the total must
    /// stop after one field -- which is the bug this being shared prevents.
    pub interrupts: Option<u64>,
    /// `btime`: the wall-clock second at which the system booted.
    ///
    /// Not a counter, and the reason this struct is named for its file. Two
    /// other programs read `/proc/stat` for this one number
    /// (`userspace/uptime`, `userspace/hwclock`) and each parses the file
    /// itself.
    pub boot_time: Option<u64>,
}

impl StatCounters {
    /// Parse the non-CPU lines of `/proc/stat` in one pass.
    #[must_use]
    pub fn parse(content: &[u8]) -> Self {
        let mut out = Self::default();
        for line in content.split(|&b| b == b'\n') {
            let fields = split_ws(line);
            let (Some(name), Some(value)) = (fields.first(), fields.get(1)) else {
                continue;
            };
            match *name {
                b"procs_running" => out.running = parse_u64(value),
                b"procs_blocked" => out.blocked = parse_u64(value),
                b"processes" => out.forks = parse_u64(value),
                b"ctxt" => out.context_switches = parse_u64(value),
                // Only `fields[1]`: the rest of the `intr` line is one count
                // per IRQ.
                b"intr" => out.interrupts = parse_u64(value),
                b"btime" => out.boot_time = parse_u64(value),
                _ => {}
            }
        }
        out
    }
}

// ============================================================================
// /proc/net/dev
// ============================================================================

/// One interface's counters from `/proc/net/dev`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetDevice {
    /// Interface name, e.g. `eth0`.
    pub name: Vec<u8>,
    /// Bytes received.
    pub rx_bytes: Option<u64>,
    /// Packets received.
    pub rx_packets: Option<u64>,
    /// Bytes transmitted.
    pub tx_bytes: Option<u64>,
    /// Packets transmitted.
    pub tx_packets: Option<u64>,
}

impl NetDevice {
    /// Parse `/proc/net/dev`, skipping its two header lines.
    ///
    /// The header is skipped by *shape*, not by count: a line with no colon is
    /// not an interface line. The code this replaces skipped exactly one line
    /// and then filtered the second by its leading text, which meant a kernel
    /// that emitted one header line or three produced either a phantom
    /// interface or a missing one.
    ///
    /// Field layout (Linux `net/core/net-procfs.c`): after the colon come 8
    /// receive counters then 8 transmit counters, so transmitted bytes are
    /// field 8 counting from zero — the previous code took field 8 for `tx`
    /// and field 0 for `rx`, which is right, and is worth restating here
    /// because it is the kind of index nobody re-derives.
    #[must_use]
    pub fn parse_all(content: &[u8]) -> Vec<Self> {
        let mut out = Vec::new();
        for line in content.split(|&b| b == b'\n') {
            let line = trim(line);
            let Some(cut) = line.iter().position(|&b| b == b':') else {
                continue;
            };
            let (name, rest) = line.split_at(cut);
            let name = trim(name);
            if name.is_empty() {
                continue;
            }
            let counters = split_ws(rest.get(1..).unwrap_or(&[]));
            let at = |index: usize| counters.get(index).and_then(|f| parse_u64(f));
            out.push(Self {
                name: name.to_vec(),
                rx_bytes: at(0),
                rx_packets: at(1),
                tx_bytes: at(8),
                tx_packets: at(9),
            });
        }
        out
    }
}

// ============================================================================
// The reader
// ============================================================================

/// A `/proc`-shaped directory to read facts out of.
///
/// Constructed with [`ProcFs::new`] for the real `/proc`, or [`ProcFs::at`]
/// for a directory of captured files — which is how every collector below is
/// tested on a development host that has no `/proc`.
#[derive(Debug, Clone)]
pub struct ProcFs {
    root: PathBuf,
}

impl Default for ProcFs {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcFs {
    /// The kernel's `/proc`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            root: PathBuf::from("/proc"),
        }
    }

    /// A directory laid out like `/proc`.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory this reads from.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read `<root>/<relative>` as bytes.
    ///
    /// Bytes, not a `String`: `/proc/mounts` contains paths, and a path is not
    /// text (CLAUDE.md self-review item 7).
    ///
    /// # Errors
    ///
    /// Any `std::fs::read` error, including [`io::ErrorKind::NotFound`]. Use
    /// [`ProcFs::read_optional`] when a missing file is an expected answer
    /// rather than a failure.
    pub fn read(&self, relative: &str) -> io::Result<Vec<u8>> {
        fs::read(self.root.join(relative))
    }

    /// Read `<root>/<relative>`, treating "no such file" as an answer.
    ///
    /// This is the distinction the code this crate replaces did not draw. A
    /// kernel that does not export `/proc/meminfo` and a `/proc/meminfo` we
    /// are not permitted to open are different situations, and only one of
    /// them should be reported to the user as "not available".
    ///
    /// # Errors
    ///
    /// Every error except [`io::ErrorKind::NotFound`], which becomes
    /// `Ok(None)`.
    pub fn read_optional(&self, relative: &str) -> io::Result<Option<Vec<u8>>> {
        match self.read(relative) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// `/proc/cpuinfo`, parsed.
    ///
    /// `Ok(None)` when the file is absent or names no processor.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn cpu(&self) -> io::Result<Option<CpuInfo>> {
        Ok(self
            .read_optional("cpuinfo")?
            .as_deref()
            .and_then(CpuInfo::parse))
    }

    /// `/proc/meminfo`, parsed.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn memory(&self) -> io::Result<Option<MemInfo>> {
        Ok(self
            .read_optional("meminfo")?
            .as_deref()
            .map(MemInfo::parse))
    }

    /// `/proc/mounts`, parsed and unescaped.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn mounts(&self) -> io::Result<Option<Vec<Mount>>> {
        Ok(self
            .read_optional("mounts")?
            .as_deref()
            .map(Mount::parse_all))
    }

    /// `/proc/swaps`, minus its header line.
    ///
    /// Returned as raw lines: the format is
    /// `Filename Type Size Used Priority`, and no caller in this tree yet
    /// needs it decomposed. `Some(vec![])` means "swap is configured off",
    /// which is different from `None`, "this kernel has no `/proc/swaps`".
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn swaps(&self) -> io::Result<Option<Vec<Vec<u8>>>> {
        let Some(content) = self.read_optional("swaps")? else {
            return Ok(None);
        };
        Ok(Some(
            content
                .split(|&b| b == b'\n')
                .skip(1)
                .map(trim)
                .filter(|line| !line.is_empty())
                .map(<[u8]>::to_vec)
                .collect(),
        ))
    }

    /// `/proc/loadavg`, parsed.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn load_average(&self) -> io::Result<Option<LoadAvg>> {
        Ok(self
            .read_optional("loadavg")?
            .as_deref()
            .and_then(LoadAvg::parse))
    }

    /// `/proc/uptime`, parsed.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn uptime(&self) -> io::Result<Option<Uptime>> {
        Ok(self
            .read_optional("uptime")?
            .as_deref()
            .and_then(Uptime::parse))
    }

    /// The non-CPU lines of `/proc/stat`: see [`StatCounters`].
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn stat_counters(&self) -> io::Result<Option<StatCounters>> {
        Ok(self
            .read_optional("stat")?
            .as_deref()
            .map(StatCounters::parse))
    }

    /// `/proc/net/dev`, parsed.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn net_devices(&self) -> io::Result<Option<Vec<NetDevice>>> {
        Ok(self
            .read_optional("net/dev")?
            .as_deref()
            .map(NetDevice::parse_all))
    }

    /// `/proc/version`, trimmed.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn version(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self
            .read_optional("version")?
            .map(|content| trim(&content).to_vec()))
    }

    /// `/proc/cmdline`, trimmed. `Ok(None)` if absent *or* empty.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn cmdline(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self
            .read_optional("cmdline")?
            .map(|content| trim(&content).to_vec())
            .filter(|line| !line.is_empty()))
    }

    /// The hostname, from `/proc/sys/kernel/hostname`.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn hostname(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self
            .read_optional("sys/kernel/hostname")?
            .map(|content| trim(&content).to_vec())
            .filter(|line| !line.is_empty()))
    }

    /// Every numeric entry in the root, i.e. every process ID.
    ///
    /// Sorted ascending, so two calls on an unchanged system compare equal;
    /// directory order is not specified and is not stable.
    ///
    /// # Errors
    ///
    /// Any error opening the directory. A single unreadable *entry* is
    /// skipped, since a process that exits during the walk is normal and is
    /// not a failure of the walk.
    pub fn process_ids(&self) -> io::Result<Vec<u64>> {
        let mut pids = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            // A vanished entry is expected: `/proc` changes under the reader.
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.is_empty()
                && name.bytes().all(|b| b.is_ascii_digit())
                && let Ok(pid) = name.parse::<u64>()
            {
                pids.push(pid);
            }
        }
        pids.sort_unstable();
        Ok(pids)
    }

    /// `/proc/stat`'s CPU lines, parsed.
    ///
    /// # Errors
    ///
    /// Any read error other than "no such file", which is `Ok(None)`.
    pub fn cpu_stats(&self) -> io::Result<Option<CpuStats>> {
        Ok(self.read_optional("stat")?.map(|c| CpuStats::parse(&c)))
    }

    /// `/proc/stat`, parsed whole: the CPU lines **and** everything else, from
    /// a single read.
    ///
    /// # Why this exists beside [`ProcFs::cpu_stats`] and
    /// [`ProcFs::stat_counters`]
    ///
    /// A sampler that wants both -- `userspace/vmstat` is the case this was
    /// written for -- would otherwise read the file twice per sample, and the
    /// two reads are of *different instants*. The CPU delta and the
    /// context-switch delta would then describe overlapping but unequal
    /// intervals: a skew that shows up as percentages which do not quite add
    /// up, and which is very hard to trace back to its cause.
    ///
    /// That is the same argument [`CpuStats::parse`] already makes for reading
    /// every `cpu` line in one pass rather than reopening the file for the
    /// `cpuN` lines. This extends it across the CPU/counter boundary.
    ///
    /// # Errors
    ///
    /// Any read error other than "no such file", which is `Ok(None)`.
    pub fn stat(&self) -> io::Result<Option<Stat>> {
        Ok(self.read_optional("stat")?.map(|content| Stat {
            cpu: CpuStats::parse(&content),
            counters: StatCounters::parse(&content),
        }))
    }

    /// `/proc/<pid>/stat`, parsed.
    ///
    /// # Errors
    ///
    /// Any read error other than "no such file", which is `Ok(None)` -- a
    /// process that exits between [`ProcFs::process_ids`] and this call is the
    /// normal case, not a failure.
    pub fn process_stat(&self, pid: u64) -> io::Result<Option<ProcessStat>> {
        Ok(self
            .read_optional(&format!("{pid}/stat"))?
            .and_then(|c| ProcessStat::parse(&c)))
    }

    /// `/proc/<pid>/statm`, parsed.
    ///
    /// # Errors
    ///
    /// As [`ProcFs::process_stat`].
    pub fn process_statm(&self, pid: u64) -> io::Result<Option<ProcessStatm>> {
        Ok(self
            .read_optional(&format!("{pid}/statm"))?
            .and_then(|c| ProcessStatm::parse(&c)))
    }

    /// `/proc/<pid>/status`, parsed.
    ///
    /// # Errors
    ///
    /// As [`ProcFs::process_stat`].
    pub fn process_status(&self, pid: u64) -> io::Result<Option<ProcessStatus>> {
        Ok(self
            .read_optional(&format!("{pid}/status"))?
            .map(|c| ProcessStatus::parse(&c)))
    }

    /// `/proc/<pid>/cmdline`, split into arguments.
    ///
    /// `Ok(Some(vec![]))` is a kernel thread; `Ok(None)` is a process that is
    /// no longer there. The two used to be the same answer.
    ///
    /// # Errors
    ///
    /// As [`ProcFs::process_stat`].
    pub fn process_cmdline(&self, pid: u64) -> io::Result<Option<Vec<Vec<u8>>>> {
        Ok(self
            .read_optional(&format!("{pid}/cmdline"))?
            .map(|c| cmdline_args(&c)))
    }
}

// ---------------------------------------------------------------------------
// Per-process: /proc/<pid>/stat, statm, status, cmdline
// ---------------------------------------------------------------------------

/// KiB per page on SlateOS.
///
/// `design.txt` fixes this at **16 KiB**, not the 4 KiB every `/proc` example
/// on the internet assumes, and `/proc/<pid>/stat` reports RSS in *pages* --
/// so a reader that gets this wrong is out by a factor of four and still
/// produces plausible numbers.
///
/// It lives here because it was a private `const PAGE_SIZE_KB: u64 = 16;` in
/// both `userspace/htop` and `userspace/ps`, which is one copy per program of
/// a fact about the kernel. Both were right; nothing made them stay right.
pub const PAGE_SIZE_KIB: u64 = 16;

/// Scheduler ticks per second, the unit `utime`/`stime` are counted in.
pub const TICKS_PER_SEC: u64 = 100;

/// One process, as `/proc/<pid>/stat` reports it.
///
/// Only the fields something in this tree reads. `stat` has 52 of them and
/// adding one is a line; carrying all 52 unread would be a claim to have
/// checked all 52.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessStat {
    /// Process ID.
    pub pid: u64,
    /// The executable name, **as bytes**.
    ///
    /// Not a `String`: a command name comes from `argv[0]`, which is bytes,
    /// and our own filesystem allows every byte but `/` and NUL
    /// (`CLAUDE.md` self-review item 7). `htop` read this through
    /// `read_to_string`, so a process whose name is not UTF-8 was skipped
    /// entirely rather than shown with its name escaped.
    pub comm: Vec<u8>,
    /// State letter: `R`, `S`, `D`, `Z`, `T`, …
    pub state: u8,
    /// Parent process ID.
    pub ppid: u64,
    /// Process group ID.
    pub pgrp: u64,
    /// Session ID.
    pub session: u64,
    /// Controlling terminal, as the kernel's packed device number.
    ///
    /// Zero means no controlling terminal. Decoding it into `tty7` or
    /// `pts/3` is a presentation question and deliberately not answered here.
    pub tty_nr: i64,
    /// User-mode time in ticks.
    pub utime_ticks: u64,
    /// Kernel-mode time in ticks.
    pub stime_ticks: u64,
    /// Scheduling priority.
    pub priority: i64,
    /// Nice value.
    pub nice: i64,
    /// Threads in this process.
    pub num_threads: u64,
    /// Virtual memory size, in **bytes** -- `stat` reports this one in bytes
    /// and the next one in pages, which is the kind of thing this crate exists
    /// to stop each caller rediscovering.
    pub vsize_bytes: u64,
    /// The CPU this process last ran on -- `stat`'s field 39, index 36 here.
    ///
    /// `Option`, unlike the fields above, because it is the **last** field
    /// anything reads and the parse deliberately accepts a line that stops
    /// short: a kernel that exports fewer fields should not blank the process
    /// table. Zero is a real CPU number, so "the line was too short" needs its
    /// own value rather than being folded into it -- which is what
    /// `userspace/sysstat`'s copy did with `.unwrap_or(0)`, reporting every
    /// process as running on CPU 0.
    pub processor: Option<u32>,
    /// Resident set size, in **pages**. See [`ProcessStat::rss_kib`].
    pub rss_pages: u64,
    /// Start time, in ticks **after boot** -- not a wall-clock instant.
    ///
    /// Turning it into one needs `/proc/uptime` and the current time, which is
    /// two more readings and therefore the caller's job. See
    /// [`Uptime`].
    pub starttime_ticks: u64,
}

impl ProcessStat {
    /// Parse the single line of `/proc/<pid>/stat`.
    ///
    /// # The comm field is why this cannot be a `split_whitespace`
    ///
    /// The second field is the executable name in parentheses, and it may
    /// contain **both spaces and parentheses** -- `(my (odd) name)` is a legal
    /// value. Splitting on whitespace therefore mis-numbers every field after
    /// it for exactly the processes most worth looking at. The name runs to the
    /// **last** `)` in the line, which works because every field after it is
    /// numeric.
    ///
    /// Returns `None` if the line is not shaped like `stat` at all -- no
    /// parentheses, or too few fields after them. A *missing* field within a
    /// well-formed line is taken as zero rather than failing the whole read,
    /// because a kernel that stops exporting field 31 should not blank the
    /// process table.
    #[must_use]
    pub fn parse(content: &[u8]) -> Option<Self> {
        let open = content.iter().position(|b| *b == b'(')?;
        let close = content.iter().rposition(|b| *b == b')')?;
        if close < open {
            return None;
        }
        let pid = parse_u64(content.get(..open)?)?;
        let comm = content.get(open.saturating_add(1)..close)?.to_vec();
        // `close + 1` is the space; the state letter follows it.
        let rest = content.get(close.saturating_add(2)..)?;
        let fields = split_ws(rest);
        // `stat`'s field 24 (rss) is index 21 here, and it is the last one
        // anything reads. Fewer than that and the line is not usable.
        if fields.len() < 22 {
            return None;
        }
        let at = |i: usize| -> u64 { fields.get(i).and_then(|f| parse_u64(f)).unwrap_or(0) };
        let at_i = |i: usize| -> i64 { fields.get(i).and_then(|f| parse_i64(f)).unwrap_or(0) };
        Some(Self {
            pid,
            comm,
            state: fields
                .first()
                .and_then(|f| f.first().copied())
                .unwrap_or(b'?'),
            ppid: at(1),
            pgrp: at(2),
            session: at(3),
            tty_nr: at_i(4),
            utime_ticks: at(11),
            stime_ticks: at(12),
            priority: at_i(15),
            nice: at_i(16),
            // A process always has at least the one thread running it, so zero
            // here means "the kernel did not say", not "no threads".
            num_threads: fields.get(17).and_then(|f| parse_u64(f)).unwrap_or(1),
            processor: fields
                .get(36)
                .and_then(|f| parse_u64(f))
                .and_then(|v| u32::try_from(v).ok()),
            starttime_ticks: at(19),
            vsize_bytes: at(20),
            rss_pages: at(21),
        })
    }

    /// Total CPU time in ticks.
    #[must_use]
    pub fn cpu_ticks(&self) -> u64 {
        self.utime_ticks.saturating_add(self.stime_ticks)
    }

    /// Resident set size in KiB, converted with [`PAGE_SIZE_KIB`].
    #[must_use]
    pub fn rss_kib(&self) -> u64 {
        self.rss_pages.saturating_mul(PAGE_SIZE_KIB)
    }

    /// Virtual size in KiB. `stat` gives bytes here, unlike RSS.
    #[must_use]
    pub fn vsize_kib(&self) -> u64 {
        self.vsize_bytes / 1024
    }
}

/// The three fields of `/proc/<pid>/statm` anything here reads, in pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessStatm {
    /// Total program size.
    pub size_pages: u64,
    /// Resident set size.
    pub resident_pages: u64,
    /// Resident shared pages.
    pub shared_pages: u64,
}

impl ProcessStatm {
    /// Parse `/proc/<pid>/statm`: seven whitespace-separated page counts.
    #[must_use]
    pub fn parse(content: &[u8]) -> Option<Self> {
        let f = split_ws(content);
        if f.len() < 3 {
            return None;
        }
        Some(Self {
            size_pages: f.first().and_then(|x| parse_u64(x)).unwrap_or(0),
            resident_pages: f.get(1).and_then(|x| parse_u64(x)).unwrap_or(0),
            shared_pages: f.get(2).and_then(|x| parse_u64(x)).unwrap_or(0),
        })
    }

    /// Shared resident memory in KiB.
    #[must_use]
    pub fn shared_kib(&self) -> u64 {
        self.shared_pages.saturating_mul(PAGE_SIZE_KIB)
    }
}

/// The fields of `/proc/<pid>/status` that anything here reads.
///
/// # One reader, not one per field
///
/// This began as a `status_uid(content)` free function, which is the right
/// shape for exactly one caller and the wrong one for two: `ps` wants the GID,
/// the supplementary groups and the `Vm*` figures from the same file, and
/// adding a `status_gid`, `status_groups`, … beside it would be four scans of
/// one file and four places to disagree about what `Uid:`'s four columns mean.
/// Parsing the file once into a struct is the same choice this whole crate is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessStatus {
    /// The **real** UID -- the first of the four on the `Uid:` line, which
    /// carries real, effective, saved-set and filesystem in that order.
    ///
    /// A caller wanting the effective UID wants another field here, not a
    /// different index at the call site -- which is [`Self::euid`], added on
    /// 2026-09-10 when `userspace/pgrep` became the first caller to need it.
    pub uid: Option<u32>,
    /// The **effective** UID -- the second column of the same `Uid:` line.
    ///
    /// The one a permission check is about, and the one `pgrep -U` versus
    /// `pgrep -u` distinguishes: `-u` selects on effective, `-U` on real.
    pub euid: Option<u32>,
    /// The real GID, from `Gid:`, on the same four-column rule.
    pub gid: Option<u32>,
    /// Supplementary groups, from `Groups:`. Empty is a real answer: a process
    /// may genuinely have none.
    pub groups: Vec<u32>,
    /// `VmSize` in KiB. Absent for a kernel thread, which has no address
    /// space -- which is why this is an `Option` and not a zero.
    pub vm_size_kib: Option<u64>,
    /// `VmRSS` in KiB. Absent for the same reason.
    pub vm_rss_kib: Option<u64>,
}

impl ProcessStatus {
    /// Parse `/proc/<pid>/status`.
    #[must_use]
    pub fn parse(content: &[u8]) -> Self {
        // `Uid:` and `Gid:` carry real, effective, saved-set and filesystem in
        // that order. Indexing that line is the thing this crate exists to do
        // once: four columns, one meaning each, named rather than counted.
        let nth_id = |key: &str, n: usize| -> Option<u32> {
            let value = key_value(content, key)?;
            let field = split_ws(&value).get(n).copied()?.to_vec();
            parse_u64(&field).and_then(|v| u32::try_from(v).ok())
        };
        let groups = key_value(content, "Groups").map_or_else(Vec::new, |value| {
            split_ws(&value)
                .iter()
                .filter_map(|f| parse_u64(f))
                .filter_map(|v| u32::try_from(v).ok())
                .collect()
        });
        Self {
            uid: nth_id("Uid", 0),
            euid: nth_id("Uid", 1),
            gid: nth_id("Gid", 0),
            groups,
            vm_size_kib: key_value(content, "VmSize").and_then(|v| parse_kib(&v)),
            vm_rss_kib: key_value(content, "VmRSS").and_then(|v| parse_kib(&v)),
        }
    }
}

/// `/proc/<pid>/cmdline` split into its arguments.
///
/// NUL-separated and NUL-terminated, so the trailing empty element is dropped.
/// **Bytes**: an argument is not text, and a program whose arguments are not
/// UTF-8 is exactly the one a process viewer is most useful for.
///
/// An empty result means a kernel thread, which has no command line at all --
/// distinct from a process whose command line we could not read, which is an
/// `Err` from [`ProcFs::process_cmdline`].
#[must_use]
pub fn cmdline_args(content: &[u8]) -> Vec<Vec<u8>> {
    content
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

// ---------------------------------------------------------------------------
// /proc/stat: CPU time
// ---------------------------------------------------------------------------

/// One `cpu` line of `/proc/stat`: time in ticks since boot, by category.
///
/// All ten fields Linux publishes, not the seven a viewer happens to show. The
/// two that get left out are the two that matter here:
///
/// * **`steal`** is time the hypervisor gave to somebody else. SlateOS
///   develops and tests under QEMU, so this is not hypothetical — and a reader
///   that omits it computes a *total* smaller than the truth, which makes every
///   process's CPU percentage larger than the truth. The error is invisible
///   because the numbers stay plausible.
/// * **`guest`** and **`guest_nice`** must be left out of the total for the
///   opposite reason: Linux already counts them inside `user` and `nice`, so
///   adding them again double-counts. They are carried because a caller may
///   want to *show* them, and dropped from [`CpuTimes::total`] because a caller
///   summing the struct's fields would otherwise be wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    /// Normal processes in user mode.
    pub user: u64,
    /// Niced processes in user mode.
    pub nice: u64,
    /// Processes in kernel mode.
    pub system: u64,
    /// Twiddling thumbs.
    pub idle: u64,
    /// Waiting for I/O.
    pub iowait: u64,
    /// Servicing hardware interrupts.
    pub irq: u64,
    /// Servicing soft interrupts.
    pub softirq: u64,
    /// Involuntary wait: the hypervisor was running something else.
    pub steal: u64,
    /// Running a guest. **Already counted in `user`.**
    pub guest: u64,
    /// Running a niced guest. **Already counted in `nice`.**
    pub guest_nice: u64,
}

impl CpuTimes {
    /// Parse one `cpu`/`cpuN` line, with the CPU's index when it has one.
    ///
    /// `Ok((None, times))` is the aggregate `cpu ` line; `Some(n)` is `cpuN`.
    /// Returns `None` for any other line, so a caller can hand it every line of
    /// the file.
    ///
    /// A short line is accepted, not rejected: `iowait` arrived in 2.5.41 and
    /// `steal` in 2.6.11, so a kernel that publishes seven fields is publishing
    /// a valid older format rather than a broken one. Missing fields read as
    /// zero. The previous reader required seven and silently dropped any CPU
    /// with fewer, which turns an older kernel into an empty CPU list.
    #[must_use]
    pub fn parse_line(line: &[u8]) -> Option<(Option<u64>, Self)> {
        let fields = split_ws(line);
        let label = fields.first()?;
        let rest = label.strip_prefix(b"cpu".as_slice())?;
        let index = if rest.is_empty() {
            None
        } else {
            Some(parse_u64(rest)?)
        };
        let at = |i: usize| -> u64 { fields.get(i).and_then(|f| parse_u64(f)).unwrap_or(0) };
        Some((
            index,
            Self {
                user: at(1),
                nice: at(2),
                system: at(3),
                idle: at(4),
                iowait: at(5),
                irq: at(6),
                softirq: at(7),
                steal: at(8),
                guest: at(9),
                guest_nice: at(10),
            },
        ))
    }

    /// Total time in ticks — everything except `guest` and `guest_nice`.
    ///
    /// See [`CpuTimes`] for why those two are excluded and `steal` is not.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.user
            .saturating_add(self.nice)
            .saturating_add(self.system)
            .saturating_add(self.idle)
            .saturating_add(self.iowait)
            .saturating_add(self.irq)
            .saturating_add(self.softirq)
            .saturating_add(self.steal)
    }

    /// The time accumulated between `earlier` and `self`, field by field.
    ///
    /// # Why a viewer needs this and not the raw counters
    ///
    /// Everything in `/proc/stat` counts **since boot**. Dividing one sample by
    /// its own total answers "how has this machine spent its life", which after
    /// a few hours of uptime is a number that barely moves whatever the machine
    /// is doing. What a viewer wants is the same ratio over the interval
    /// between two samples, and that needs the subtraction to happen before the
    /// division.
    ///
    /// `saturating_sub` per field rather than an assertion that time only goes
    /// forwards: a CPU that is taken offline and brought back starts its
    /// counters again, so `earlier` can legitimately be larger. Saturating
    /// gives that CPU a zero-length interval for one refresh, which shows as an
    /// idle bar and corrects itself on the next one. Subtracting with `-` would
    /// panic in debug and wrap to something enormous in release.
    #[must_use]
    pub fn since(&self, earlier: &Self) -> Self {
        Self {
            user: self.user.saturating_sub(earlier.user),
            nice: self.nice.saturating_sub(earlier.nice),
            system: self.system.saturating_sub(earlier.system),
            idle: self.idle.saturating_sub(earlier.idle),
            iowait: self.iowait.saturating_sub(earlier.iowait),
            irq: self.irq.saturating_sub(earlier.irq),
            softirq: self.softirq.saturating_sub(earlier.softirq),
            steal: self.steal.saturating_sub(earlier.steal),
            guest: self.guest.saturating_sub(earlier.guest),
            guest_nice: self.guest_nice.saturating_sub(earlier.guest_nice),
        }
    }

    /// Time not spent idle or waiting for I/O.
    ///
    /// `iowait` counts as not-busy, which is the convention `top` and `htop`
    /// use and is arguable either way — a disk-bound machine is *doing*
    /// something. It is stated here so that a caller who disagrees knows to
    /// compute their own rather than discovering the choice from a graph.
    #[must_use]
    pub fn busy(&self) -> u64 {
        self.total()
            .saturating_sub(self.idle)
            .saturating_sub(self.iowait)
    }
}

/// The CPU half of `/proc/stat`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CpuStats {
    /// The aggregate `cpu ` line, if the kernel published one.
    pub total: Option<CpuTimes>,
    /// The `cpuN` lines, in file order — which is CPU-index order in practice
    /// but is not promised by `proc(5)`.
    pub per_cpu: Vec<CpuTimes>,
}

impl CpuStats {
    /// Parse every `cpu` line of `/proc/stat` in **one** pass.
    ///
    /// The reader this replaces opened `/proc/stat` a second time when it found
    /// no `cpuN` lines, so on a single-CPU machine it read the file twice and
    /// could see two different instants.
    #[must_use]
    pub fn parse(content: &[u8]) -> Self {
        let mut out = Self::default();
        for line in content.split(|b| *b == b'\n') {
            match CpuTimes::parse_line(line) {
                Some((None, times)) => out.total = Some(times),
                Some((Some(_), times)) => out.per_cpu.push(times),
                None => {}
            }
        }
        out
    }

    /// Per-CPU times, falling back to the aggregate when the kernel published
    /// no `cpuN` lines.
    ///
    /// A caller drawing one bar per CPU wants this rather than `per_cpu`: one
    /// bar for the whole machine is a better answer than no bars.
    #[must_use]
    pub fn per_cpu_or_total(&self) -> Vec<CpuTimes> {
        if self.per_cpu.is_empty() {
            self.total.into_iter().collect()
        } else {
            self.per_cpu.clone()
        }
    }
}

// ---------------------------------------------------------------------------
// Showing bytes to a person
// ---------------------------------------------------------------------------

/// Bytes from `/proc` rendered for a terminal.
///
/// # Why a display helper is in a crate that says it does not format
///
/// This crate's header says it deals in facts and leaves presentation alone,
/// and that is still the rule. This is the one exception, and it earns its
/// place by being the direct consequence of the rule rather than a breach of
/// it: the crate returns **bytes** for command names and arguments, precisely
/// because forcing them through UTF-8 would be wrong, and so every caller
/// inherits the same problem of putting one on a screen. Two callers solved it
/// two different ways within a day of each other -- `htop` with this function
/// and `ps` with `from_utf8_lossy` -- which is the duplication this crate
/// exists to end, one level up.
///
/// # `\xNN`, not `from_utf8_lossy`
///
/// `CLAUDE.md` self-review item 7 forbids the lossy conversion as silent data
/// corruption. Here the corruption would be of the thing the user is reading:
/// U+FFFD maps every invalid byte to the same character, so two different
/// process names become the same string and a viewer cannot tell one from the
/// other. This is explicit and does not collide.
///
/// The alternative that is worse than either is what `htop` did before it had
/// this: read through `read_to_string`, fail, and drop the process from the
/// list. A viewer that hides exactly the processes with unusual names is worse
/// than one that renders them oddly.
#[must_use]
pub fn display_bytes(raw: &[u8]) -> String {
    match core::str::from_utf8(raw) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let mut out = String::with_capacity(raw.len());
            let mut rest = raw;
            loop {
                match core::str::from_utf8(rest) {
                    Ok(text) => {
                        out.push_str(text);
                        return out;
                    }
                    Err(e) => {
                        let good = e.valid_up_to();
                        if let Some(text) =
                            rest.get(..good).and_then(|b| core::str::from_utf8(b).ok())
                        {
                            out.push_str(text);
                        }
                        let bad = e.error_len().unwrap_or(1);
                        for b in rest.get(good..good.saturating_add(bad)).unwrap_or_default() {
                            let _ = write!(out, "\\x{b:02x}");
                        }
                        let Some(next) = rest.get(good.saturating_add(bad)..) else {
                            return out;
                        };
                        rest = next;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
