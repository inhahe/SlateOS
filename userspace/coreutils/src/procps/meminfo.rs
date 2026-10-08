//! `/proc/meminfo`, transcribed from procps-ng 4.0.4's `library/meminfo.c`:
//! the file's keys, and the figures the library derives from them.
//!
//! `free` and `vmstat` both print from this, as upstream's two programs both
//! print from one `meminfo_read_failed()`. The derivations are the library's
//! own, in its order -- `used`, `cached_all` and `swap_used` are not the
//! kernel's numbers but procps', and a program that computed them for itself
//! would be one formula away from disagreeing with its sibling.
//!
//! # Deliberately not `procinfo::MemInfo`
//!
//! On 2026-09-10 five of the six hand-written meminfo parsers in `userspace/`
//! were replaced by the shared one. This one stays, because the two have
//! **opposite parsing policies and both are correct for their jobs**:
//!
//! - [`parse`] below is procps' own walk of the file: a key is everything up
//!   to the next `:`, and its value is C's `strtoul` from there (the shared
//!   [`strtoul`]) -- leading whitespace, a sign, digits, saturating on
//!   overflow. It accepts a bare number, ignores any suffix, and is what
//!   procps actually does.
//! - `procinfo::parse_kib` **refuses a unit it does not recognise**, so that
//!   `16 MB` reads as absent rather than as `16` KiB -- the same number in the
//!   same font, off by 1024.
//!
//! That refusal is the right default for a shared reader, and the wrong
//! behaviour for programs whose purpose is to match upstream's, including on
//! malformed input. Converting this would be uniformity bought with fidelity.
//! So if a later sweep for `/proc/meminfo` finds this file: it is not an
//! oversight. Change it only if the goal changes from "behave like procps" to
//! something else.
//!
//! # One divergence
//!
//! This OS's `/proc/meminfo` publishes `SwapUsed:` and no `SwapFree:` (see
//! `requests/b-a-proc-meminfo-omits-the-linux-keys-that-thirteen-tools-read.md`).
//! Read as procps reads it, swap would show as wholly used. [`derive`]
//! substitutes `SwapTotal - SwapUsed` for the missing key -- keyed on the
//! *absence* of a `SwapFree:` line, so a Linux machine whose swap is genuinely
//! full is not rewritten. `free`'s module docs number this as its divergence 1.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use super::scanf::{c_str, strchr_from, strtoul};

/// `MEMINFO_FILE`.
pub const MEMINFO_FILE: &str = "/proc/meminfo";

/// `MEMINFO_BUFF`, of which one byte is kept for the terminating NUL: the
/// library reads the file in one `read` of at most this much.
const MEMINFO_BUFF: usize = 8192;

/// procps' `struct meminfo_info`, for a program that reads the file again and
/// again (`vmstat`): the file, opened once and read from its start each time.
#[derive(Debug)]
pub struct MemInfo {
    file: File,
}

impl MemInfo {
    /// `procps_meminfo_new`, with its priming read.
    ///
    /// # Errors
    ///
    /// The file could not be opened or read, or read empty.
    pub fn new() -> io::Result<Self> {
        let mut info = Self {
            file: File::open(MEMINFO_FILE)?,
        };
        info.select()?;
        Ok(info)
    }

    /// `procps_meminfo_select`: the file read again, as [`read_once`] reads
    /// it, and derived.
    ///
    /// # Errors
    ///
    /// As [`MemInfo::new`].
    pub fn select(&mut self) -> io::Result<Mem> {
        Ok(derive(&parse(&read_once(&mut self.file)?)))
    }
}

/// `meminfo_read_failed`'s read: one `read` of at most `MEMINFO_BUFF - 1`
/// bytes from the start of `file`, tried again on `EINTR` and `EAGAIN`. A
/// longer file is cut there, as the library cuts it, and an empty one is
/// `EIO` -- which is why a program reading an empty `/proc/meminfo` says it
/// is "Unable to create meminfo structure" rather than printing zeros.
fn read_once(file: &mut File) -> io::Result<Vec<u8>> {
    /// `EIO`.
    const EIO: i32 = 5;
    file.seek(SeekFrom::Start(0))?;
    let mut buf = vec![0u8; MEMINFO_BUFF.saturating_sub(1)];
    let size = loop {
        match file.read(&mut buf) {
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            other => break other?,
        }
    };
    if size == 0 {
        return Err(io::Error::from_raw_os_error(EIO));
    }
    buf.truncate(size);
    Ok(buf)
}

/// The file at `path` as the library reads it: opened, then read once as
/// [`read_once`] does. For a program that reads the file afresh for every
/// report (`free`), where [`MemInfo`] keeps it open.
///
/// # Errors
///
/// The file could not be opened or read, or read empty (`EIO`).
pub fn read(path: &str) -> io::Result<Vec<u8>> {
    read_once(&mut File::open(path)?)
}

/// The keys read straight out of the file, before any derivation.
///
/// Every field is the kibibyte figure the kernel printed. A key that is not in
/// the file stays 0 -- that is what procps' hash lookup does with an unknown
/// name, and several of the derivations below exist precisely to notice it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Raw {
    pub mem_total: u64,
    pub mem_free: u64,
    pub mem_available: u64,
    pub buffers: u64,
    pub cached: u64,
    pub s_reclaimable: u64,
    pub shmem: u64,
    /// `Active:`, which `vmstat -a` prints.
    pub active: u64,
    /// `Inactive:`, likewise.
    pub inactive: u64,
    pub high_total: u64,
    pub high_free: u64,
    pub low_total: u64,
    pub low_free: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub commit_limit: u64,
    pub committed_as: u64,
    /// SlateOS publishes this instead of `SwapFree`; see the module docs.
    pub swap_used: u64,
    /// Whether a `SwapFree:` line was present at all. Keyed on presence rather
    /// than on the value, so a Linux machine whose swap is genuinely full -- a
    /// real `SwapFree: 0` -- is not rewritten.
    pub swap_free_seen: bool,
    /// Whether a `SwapUsed:` line was present, i.e. whether there is anything
    /// to substitute *from*.
    pub swap_used_seen: bool,
}

/// Read the `Key: <n> kB` lines the way `meminfo_read_failed` does, ignoring
/// every key there is no field for.
///
/// It is a walk of the text rather than a split into lines, and the
/// difference shows on a file that is not quite what the kernel writes:
///
/// - The text is a C string: a NUL ends it.
/// - A key runs from where the last line ended to the next `:` anywhere
///   after it. A line with no `:` therefore becomes the front of the next
///   line's key, which then matches nothing -- and that line's value is lost
///   with it.
/// - The value is `strtoul` from just after the `:`: whitespace first,
///   newlines included, then a sign (a minus negates, in `unsigned long`) and
///   digits, saturating at `ULONG_MAX`. A unit after it (`kB`) is ignored.
/// - The next key starts after the first newline after the `:`.
///
/// A key it does not know costs nothing, and a key given twice keeps its
/// last value.
#[must_use]
pub fn parse(text: &[u8]) -> Raw {
    let text = c_str(text);
    let mut raw = Raw::default();
    let mut head = 0usize;
    while let Some(colon) = strchr_from(text, head, b':') {
        let key = text.get(head..colon).unwrap_or_default();
        head = colon.saturating_add(1);
        let value = strtoul(text.get(head..).unwrap_or_default()).0;
        let slot: Option<&mut u64> = match key {
            b"MemTotal" => Some(&mut raw.mem_total),
            b"MemFree" => Some(&mut raw.mem_free),
            b"MemAvailable" => Some(&mut raw.mem_available),
            b"Buffers" => Some(&mut raw.buffers),
            b"Cached" => Some(&mut raw.cached),
            b"SReclaimable" => Some(&mut raw.s_reclaimable),
            b"Shmem" => Some(&mut raw.shmem),
            b"Active" => Some(&mut raw.active),
            b"Inactive" => Some(&mut raw.inactive),
            b"HighTotal" => Some(&mut raw.high_total),
            b"HighFree" => Some(&mut raw.high_free),
            b"LowTotal" => Some(&mut raw.low_total),
            b"LowFree" => Some(&mut raw.low_free),
            b"SwapTotal" => Some(&mut raw.swap_total),
            b"SwapFree" => {
                raw.swap_free_seen = true;
                Some(&mut raw.swap_free)
            }
            b"SwapUsed" => {
                raw.swap_used_seen = true;
                Some(&mut raw.swap_used)
            }
            b"CommitLimit" => Some(&mut raw.commit_limit),
            b"Committed_AS" => Some(&mut raw.committed_as),
            _ => None,
        };
        if let Some(slot) = slot {
            *slot = value;
        }
        let Some(nl) = strchr_from(text, head, b'\n') else {
            break;
        };
        head = nl.saturating_add(1);
    }
    raw
}

/// The figures procps' programs print, after the library's derivations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mem {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub buffers: u64,
    /// `Cached + SReclaimable` -- procps' `derived_mem_cached`.
    pub cached_all: u64,
    /// `Shmem`.
    pub shared: u64,
    /// `Active`.
    pub active: u64,
    /// `Inactive`.
    pub inactive: u64,
    /// `MemTotal - MemAvailable`, or `MemTotal - MemFree` if that went
    /// negative. Stored as the `unsigned long` upstream stores it in, wrap and
    /// all, because the wrap is visible: see `free`'s `-1` row.
    pub used: u64,
    pub high_total: u64,
    pub high_free: u64,
    pub high_used: u64,
    pub low_total: u64,
    pub low_free: u64,
    pub low_used: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub swap_used: u64,
    pub commit_limit: u64,
    pub committed_as: u64,
}

/// procps' `meminfo_read_failed()` tail, in its original order.
///
/// The order matters at least twice: `derived_mem_cached` is computed *before*
/// the `MemAvailable > MemTotal` guard rewrites `MemAvailable`, and the
/// `LowTotal == 0` substitution happens *after* `derived_mem_hi_used`, so a
/// kernel exporting neither still gets a `High:` row of zeroes.
#[must_use]
pub fn derive(raw: &Raw) -> Mem {
    let mut m = Mem {
        total: raw.mem_total,
        free: raw.mem_free,
        available: raw.mem_available,
        buffers: raw.buffers,
        shared: raw.shmem,
        active: raw.active,
        inactive: raw.inactive,
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

    // The divergence, applied before anything reads `swap_free`: without it the
    // `SwapFree < SwapTotal` test below is `0 < SwapTotal` and swap reads full.
    if !raw.swap_free_seen && raw.swap_used_seen {
        m.swap_free = raw.swap_total.saturating_sub(raw.swap_used);
    }

    // "if (0 == MemAvailable) MemAvailable = MemFree" -- kernels before 3.14.
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn keys_values_and_units() {
        let raw = parse(b"MemTotal:       1024 kB\nZeroPoolHits:      7\nMemFree:  512 kB\n");
        assert_eq!((raw.mem_total, raw.mem_free), (1024, 512));
        assert!(!raw.swap_free_seen);
    }

    #[test]
    fn a_line_without_a_colon_takes_the_next_key_with_it() {
        // The key after `MemTotal` is "junk\nMemFree", which is no key at all.
        let raw = parse(b"MemTotal: 1024 kB\njunk\nMemFree: 512 kB\nBuffers: 9 kB\n");
        assert_eq!((raw.mem_total, raw.mem_free, raw.buffers), (1024, 0, 9));
    }

    #[test]
    fn the_value_is_strtoul_from_the_colon() {
        // Whitespace before the number may include the newline.
        assert_eq!(parse(b"MemTotal:\n  77 kB\n").mem_total, 77);
        // A sign is read, a minus negating in `unsigned long`.
        assert_eq!(parse(b"MemTotal: -1 kB\n").mem_total, u64::MAX);
        assert_eq!(parse(b"MemTotal: +5 kB\n").mem_total, 5);
        // Too many digits saturate; none make 0.
        assert_eq!(
            parse(b"MemTotal: 99999999999999999999999\n").mem_total,
            u64::MAX
        );
        assert_eq!(parse(b"MemTotal: kB\n").mem_total, 0);
    }

    #[test]
    fn an_empty_file_is_eio_and_a_long_one_is_cut_at_the_buffer() {
        let dir = std::env::temp_dir().join(format!("meminfo-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty");
        std::fs::write(&empty, b"").unwrap();
        let e = read(empty.to_str().unwrap()).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(5));
        // 8191 bytes are read; the key that starts past them is not seen.
        let long = dir.join("long");
        let mut text = vec![b'x'; 8190];
        text.extend_from_slice(b"\nMemTotal: 5 kB\n");
        std::fs::write(&long, &text).unwrap();
        let got = read(long.to_str().unwrap()).unwrap();
        assert_eq!(got.len(), 8191);
        assert_eq!(parse(&got).mem_total, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_nul_ends_the_text_and_a_repeated_key_keeps_its_last_value() {
        assert_eq!(
            parse(b"MemTotal: 5 kB\nMemFree: 3\0 kB\nBuffers: 2 kB\n").buffers,
            0
        );
        assert_eq!(parse(b"MemTotal: 5 kB\nMemTotal: 6 kB\n").mem_total, 6);
    }
}
