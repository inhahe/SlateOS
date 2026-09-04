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
}

impl MemInfo {
    /// Parse `/proc/meminfo`.
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

/// The scheduler counters at the end of `/proc/stat`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SchedCounters {
    /// `procs_running`: tasks currently on a run queue.
    pub running: Option<u64>,
    /// `procs_blocked`: tasks blocked on I/O.
    pub blocked: Option<u64>,
    /// `processes`: total forks since boot.
    pub forks: Option<u64>,
}

impl SchedCounters {
    /// Parse the `procs_*` and `processes` lines of `/proc/stat`.
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

    /// The `procs_*` counters of `/proc/stat`.
    ///
    /// # Errors
    /// Propagates any read error other than "not found".
    pub fn sched_counters(&self) -> io::Result<Option<SchedCounters>> {
        Ok(self
            .read_optional("stat")?
            .as_deref()
            .map(SchedCounters::parse))
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
}

#[cfg(test)]
mod tests;
