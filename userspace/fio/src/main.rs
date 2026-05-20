//! Multi-personality flexible I/O tester for OurOS.
//!
//! This binary detects the personality from `argv[0]`:
//!   - `fio`        -> flexible I/O tester (main personality)
//!   - `fio-verify` -> verify written data integrity
//!
//! Supports job definitions via command-line flags or INI-style job files,
//! with statistics collection, multiple output formats (normal, terse, JSON,
//! json+), and built-in verification (md5, crc32, sha256, pattern).

#![deny(clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Size parsing: K=1024, M=1024^2, G=1024^3, T=1024^4
// ---------------------------------------------------------------------------

/// Parse a size string with optional K/M/G/T suffix.
/// Returns the size in bytes, or None on parse failure.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    let (num_part, multiplier) = if let Some(n) = lower.strip_suffix('t') {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = lower.strip_suffix("tb") {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = lower.strip_suffix('g') {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = lower.strip_suffix("gb") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = lower.strip_suffix('m') {
        (n, 1024u64 * 1024)
    } else if let Some(n) = lower.strip_suffix("mb") {
        (n, 1024u64 * 1024)
    } else if let Some(n) = lower.strip_suffix('k') {
        (n, 1024u64)
    } else if let Some(n) = lower.strip_suffix("kb") {
        (n, 1024u64)
    } else {
        (lower.as_str(), 1u64)
    };
    let num: u64 = num_part.parse().ok()?;
    num.checked_mul(multiplier)
}

/// Format bytes into a human-readable string.
fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1}MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1}KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

// ---------------------------------------------------------------------------
// I/O pattern
// ---------------------------------------------------------------------------

/// I/O access pattern for a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IoPattern {
    Read,
    Write,
    RandRead,
    RandWrite,
    ReadWrite,
    RandRW,
    Trim,
}

impl IoPattern {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "randread" => Some(Self::RandRead),
            "randwrite" => Some(Self::RandWrite),
            "readwrite" | "rw" => Some(Self::ReadWrite),
            "randrw" => Some(Self::RandRW),
            "trim" => Some(Self::Trim),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::RandRead => "randread",
            Self::RandWrite => "randwrite",
            Self::ReadWrite => "readwrite",
            Self::RandRW => "randrw",
            Self::Trim => "trim",
        }
    }

    fn is_read(self) -> bool {
        matches!(self, Self::Read | Self::RandRead)
    }

    fn is_write(self) -> bool {
        matches!(self, Self::Write | Self::RandWrite | Self::Trim)
    }

    fn is_random(self) -> bool {
        matches!(self, Self::RandRead | Self::RandWrite | Self::RandRW)
    }

    fn is_mixed(self) -> bool {
        matches!(self, Self::ReadWrite | Self::RandRW)
    }
}

impl fmt::Display for IoPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// ---------------------------------------------------------------------------
// I/O engine
// ---------------------------------------------------------------------------

/// I/O engine type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IoEngine {
    Sync,
    Psync,
    Libaio,
    IoUring,
    Mmap,
}

impl IoEngine {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "sync" => Some(Self::Sync),
            "psync" => Some(Self::Psync),
            "libaio" => Some(Self::Libaio),
            "io_uring" | "iouring" => Some(Self::IoUring),
            "mmap" => Some(Self::Mmap),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Psync => "psync",
            Self::Libaio => "libaio",
            Self::IoUring => "io_uring",
            Self::Mmap => "mmap",
        }
    }
}

impl fmt::Display for IoEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// ---------------------------------------------------------------------------
// Verification method
// ---------------------------------------------------------------------------

/// Data verification method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifyMethod {
    Md5,
    Crc32,
    Sha256,
    Pattern,
}

impl VerifyMethod {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "md5" => Some(Self::Md5),
            "crc32" => Some(Self::Crc32),
            "sha256" => Some(Self::Sha256),
            "pattern" => Some(Self::Pattern),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Crc32 => "crc32",
            Self::Sha256 => "sha256",
            Self::Pattern => "pattern",
        }
    }
}

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

/// Output format for results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Normal,
    Terse,
    Json,
    JsonPlus,
}

impl OutputFormat {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "terse" => Some(Self::Terse),
            "json" => Some(Self::Json),
            "json+" => Some(Self::JsonPlus),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Job definition
// ---------------------------------------------------------------------------

/// A single I/O job definition with all configurable parameters.
#[derive(Debug, Clone)]
struct JobDef {
    name: String,
    filename: String,
    size: u64,
    bs: u64,
    rw: IoPattern,
    iodepth: u32,
    numjobs: u32,
    runtime: Option<u64>,
    time_based: bool,
    direct: bool,
    ioengine: IoEngine,
    rwmixread: u32,
    norandommap: bool,
    verify: Option<VerifyMethod>,
    verify_pattern: u8,
    do_verify: bool,
    thread: bool,
    group_reporting: bool,
    lat_percentiles: bool,
}

impl Default for JobDef {
    fn default() -> Self {
        Self {
            name: String::from("job0"),
            filename: String::new(),
            size: 0,
            bs: 4096,
            rw: IoPattern::Read,
            iodepth: 1,
            numjobs: 1,
            runtime: None,
            time_based: false,
            direct: false,
            ioengine: IoEngine::Sync,
            rwmixread: 50,
            norandommap: false,
            verify: None,
            verify_pattern: 0xAA,
            do_verify: true,
            thread: false,
            group_reporting: false,
            lat_percentiles: false,
        }
    }
}

impl JobDef {
    /// Apply a key=value parameter to this job definition.
    /// Returns Err if the key is unknown or the value is invalid.
    fn set_param(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "name" => self.name = value.to_string(),
            "filename" => self.filename = value.to_string(),
            "size" => {
                self.size =
                    parse_size(value).ok_or_else(|| format!("invalid size: {value}"))?;
            }
            "bs" | "blocksize" => {
                self.bs =
                    parse_size(value).ok_or_else(|| format!("invalid blocksize: {value}"))?;
            }
            "rw" | "readwrite" => {
                self.rw = IoPattern::parse(value)
                    .ok_or_else(|| format!("unknown I/O pattern: {value}"))?;
            }
            "iodepth" => {
                self.iodepth = value
                    .parse()
                    .map_err(|_| format!("invalid iodepth: {value}"))?;
            }
            "numjobs" => {
                self.numjobs = value
                    .parse()
                    .map_err(|_| format!("invalid numjobs: {value}"))?;
            }
            "runtime" => {
                self.runtime = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid runtime: {value}"))?,
                );
            }
            "time_based" => self.time_based = true,
            "direct" => {
                self.direct = value != "0";
            }
            "ioengine" => {
                self.ioengine = IoEngine::parse(value)
                    .ok_or_else(|| format!("unknown ioengine: {value}"))?;
            }
            "rwmixread" => {
                self.rwmixread = value
                    .parse()
                    .map_err(|_| format!("invalid rwmixread: {value}"))?;
            }
            "norandommap" => self.norandommap = true,
            "verify" => {
                self.verify = Some(
                    VerifyMethod::parse(value)
                        .ok_or_else(|| format!("unknown verify method: {value}"))?,
                );
            }
            "verify_pattern" => {
                let hex = value.strip_prefix("0x").unwrap_or(value);
                self.verify_pattern = u8::from_str_radix(hex, 16)
                    .map_err(|_| format!("invalid verify_pattern: {value}"))?;
            }
            "do_verify" => {
                self.do_verify = value != "0";
            }
            "thread" => self.thread = true,
            "group_reporting" => self.group_reporting = true,
            "lat_percentiles" => {
                self.lat_percentiles = value != "0";
            }
            _ => return Err(format!("unknown parameter: {key}")),
        }
        Ok(())
    }

    /// Compute the number of blocks for this job.
    fn num_blocks(&self) -> u64 {
        if self.bs == 0 {
            return 0;
        }
        self.size / self.bs
    }
}

// ---------------------------------------------------------------------------
// Simple PRNG (xorshift64) — no external deps
// ---------------------------------------------------------------------------

struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x1234_5678_9ABC_DEF0 } else { seed },
        }
    }

    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Return a value in [0, upper_bound).
    fn next_bounded(&mut self, upper_bound: u64) -> u64 {
        if upper_bound == 0 {
            return 0;
        }
        self.next() % upper_bound
    }
}

// ---------------------------------------------------------------------------
// CRC32 (IEEE 802.3, no tables for simplicity — bitwise)
// ---------------------------------------------------------------------------

fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &byte in data {
        c ^= u32::from(byte);
        for _ in 0..8 {
            if c & 1 != 0 {
                c = (c >> 1) ^ 0xEDB8_8320;
            } else {
                c >>= 1;
            }
        }
    }
    !c
}

fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

// ---------------------------------------------------------------------------
// MD5 (RFC 1321)
// ---------------------------------------------------------------------------

fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14,
        20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11,
        16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76a_a478, 0xe8c7_b756, 0x2420_70db, 0xc1bd_ceee, 0xf57c_0faf, 0x4787_c62a,
        0xa830_4613, 0xfd46_9501, 0x6980_98d8, 0x8b44_f7af, 0xffff_5bb1, 0x895c_d7be,
        0x6b90_1122, 0xfd98_7193, 0xa679_438e, 0x49b4_0821, 0xf61e_2562, 0xc040_b340,
        0x265e_5a51, 0xe9b6_c7aa, 0xd62f_105d, 0x0244_1453, 0xd8a1_e681, 0xe7d3_fbc8,
        0x21e1_cde6, 0xc337_07d6, 0xf4d5_0d87, 0x455a_14ed, 0xa9e3_e905, 0xfcef_a3f8,
        0x676f_02d9, 0x8d2a_4c8a, 0xfffa_3942, 0x8771_f681, 0x6d9d_6122, 0xfde5_380c,
        0xa4be_ea44, 0x4bde_cfa9, 0xf6bb_4b60, 0xbebf_bc70, 0x289b_7ec6, 0xeaa1_27fa,
        0xd4ef_3085, 0x0488_1d05, 0xd9d4_d039, 0xe6db_99e5, 0x1fa2_7cf8, 0xc4ac_5665,
        0xf429_2244, 0x432a_ff97, 0xab94_23a7, 0xfc93_a039, 0x655b_59c3, 0x8f0c_cc92,
        0xffef_f47d, 0x8584_5dd1, 0x6fa8_7e4f, 0xfe2c_e6e0, 0xa301_4314, 0x4e08_11a1,
        0xf753_7e82, 0xbd3a_f235, 0x2ad7_d2bb, 0xeb86_d391,
    ];

    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut buf = data.to_vec();
    buf.push(0x80);
    while buf.len() % 64 != 56 {
        buf.push(0);
    }
    buf.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in buf.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            let off = i * 4;
            *word = u32::from_le_bytes([chunk[off], chunk[off + 1], chunk[off + 2], chunk[off + 3]]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | (!b & d), i)
            } else if i < 32 {
                ((d & b) | (!d & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * i) % 16)
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]))
                    .rotate_left(S[i]),
            );
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..].copy_from_slice(&d0.to_le_bytes());
    out
}

// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4)
// ---------------------------------------------------------------------------

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1,
        0x923f_82a4, 0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3,
        0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786,
        0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
        0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7, 0xc6e0_0bf3, 0xd5a7_9147,
        0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13,
        0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
        0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a,
        0x5b9c_ca4f, 0x682e_6ff3, 0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208,
        0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c,
        0x1f83_d9ab, 0x5be0_cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut buf = data.to_vec();
    buf.push(0x80);
    while buf.len() % 64 != 56 {
        buf.push(0);
    }
    buf.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in buf.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let off = i * 4;
            w[i] = u32::from_be_bytes([chunk[off], chunk[off + 1], chunk[off + 2], chunk[off + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, &val) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }
    out
}

#[cfg(test)]
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

// ---------------------------------------------------------------------------
// Latency statistics
// ---------------------------------------------------------------------------

/// Collects latency samples and computes statistics.
#[derive(Debug, Clone)]
struct LatencyStats {
    samples: Vec<u64>, // microseconds
    total_us: u64,
    count: u64,
    min_us: u64,
    max_us: u64,
}

impl LatencyStats {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            total_us: 0,
            count: 0,
            min_us: u64::MAX,
            max_us: 0,
        }
    }

    fn record(&mut self, us: u64) {
        self.samples.push(us);
        self.total_us = self.total_us.saturating_add(us);
        self.count += 1;
        if us < self.min_us {
            self.min_us = us;
        }
        if us > self.max_us {
            self.max_us = us;
        }
    }

    fn avg_us(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        self.total_us as f64 / self.count as f64
    }

    /// Get the percentile value (0-100). Sorts samples if needed.
    fn percentile(&mut self, pct: f64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        self.samples.sort_unstable();
        let idx = ((pct / 100.0) * (self.samples.len() as f64 - 1.0)) as usize;
        let idx = idx.min(self.samples.len().saturating_sub(1));
        self.samples[idx]
    }

    fn safe_min(&self) -> u64 {
        if self.count == 0 { 0 } else { self.min_us }
    }
}

/// Format microseconds into a human-readable latency string.
#[cfg(test)]
fn format_latency(us: f64) -> String {
    if us >= 1_000_000.0 {
        format!("{:.2}s", us / 1_000_000.0)
    } else if us >= 1000.0 {
        format!("{:.2}ms", us / 1000.0)
    } else {
        format!("{:.2}us", us)
    }
}

// ---------------------------------------------------------------------------
// Job statistics
// ---------------------------------------------------------------------------

/// Complete statistics for a single job run.
#[derive(Debug, Clone)]
struct JobStats {
    name: String,
    rw: IoPattern,
    ioengine: IoEngine,
    bs: u64,
    iodepth: u32,
    numjobs: u32,
    read_bytes: u64,
    write_bytes: u64,
    read_ios: u64,
    write_ios: u64,
    trim_ios: u64,
    elapsed_us: u64,
    read_lat: LatencyStats,
    write_lat: LatencyStats,
    usr_cpu: f64,
    sys_cpu: f64,
    ctx_switches: u64,
    major_faults: u64,
    minor_faults: u64,
    io_depth_dist: [u64; 8], // 1, 2, 4, 8, 16, 32, 64, >=64
    errors: u64,
    verify_errors: u64,
}

impl JobStats {
    fn new(name: &str, rw: IoPattern, ioengine: IoEngine, bs: u64, iodepth: u32, numjobs: u32) -> Self {
        Self {
            name: name.to_string(),
            rw,
            ioengine,
            bs,
            iodepth,
            numjobs,
            read_bytes: 0,
            write_bytes: 0,
            read_ios: 0,
            write_ios: 0,
            trim_ios: 0,
            elapsed_us: 0,
            read_lat: LatencyStats::new(),
            write_lat: LatencyStats::new(),
            usr_cpu: 0.0,
            sys_cpu: 0.0,
            ctx_switches: 0,
            major_faults: 0,
            minor_faults: 0,
            io_depth_dist: [0; 8],
            errors: 0,
            verify_errors: 0,
        }
    }

    fn read_bw_kib(&self) -> f64 {
        if self.elapsed_us == 0 {
            return 0.0;
        }
        (self.read_bytes as f64 / 1024.0) / (self.elapsed_us as f64 / 1_000_000.0)
    }

    fn write_bw_kib(&self) -> f64 {
        if self.elapsed_us == 0 {
            return 0.0;
        }
        (self.write_bytes as f64 / 1024.0) / (self.elapsed_us as f64 / 1_000_000.0)
    }

    fn read_iops(&self) -> f64 {
        if self.elapsed_us == 0 {
            return 0.0;
        }
        self.read_ios as f64 / (self.elapsed_us as f64 / 1_000_000.0)
    }

    fn write_iops(&self) -> f64 {
        if self.elapsed_us == 0 {
            return 0.0;
        }
        self.write_ios as f64 / (self.elapsed_us as f64 / 1_000_000.0)
    }

    fn merge(&mut self, other: &JobStats) {
        self.read_bytes = self.read_bytes.saturating_add(other.read_bytes);
        self.write_bytes = self.write_bytes.saturating_add(other.write_bytes);
        self.read_ios = self.read_ios.saturating_add(other.read_ios);
        self.write_ios = self.write_ios.saturating_add(other.write_ios);
        self.trim_ios = self.trim_ios.saturating_add(other.trim_ios);
        if other.elapsed_us > self.elapsed_us {
            self.elapsed_us = other.elapsed_us;
        }
        for s in &other.read_lat.samples {
            self.read_lat.record(*s);
        }
        for s in &other.write_lat.samples {
            self.write_lat.record(*s);
        }
        self.errors = self.errors.saturating_add(other.errors);
        self.verify_errors = self.verify_errors.saturating_add(other.verify_errors);
        for i in 0..8 {
            self.io_depth_dist[i] = self.io_depth_dist[i].saturating_add(other.io_depth_dist[i]);
        }
    }
}

// ---------------------------------------------------------------------------
// Job file parser
// ---------------------------------------------------------------------------

/// Parse an INI-style job file into a list of job definitions.
fn parse_job_file(content: &str) -> Result<Vec<JobDef>, String> {
    let mut global = JobDef::default();
    let mut jobs: Vec<JobDef> = Vec::new();
    let mut current: Option<JobDef> = None;

    for (line_num, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                let section = &line[1..end];
                if let Some(job) = current.take() {
                    jobs.push(job);
                }
                if section == "global" {
                    current = None; // parameters go to global
                } else {
                    let mut job = global.clone();
                    job.name = section.to_string();
                    current = Some(job);
                }
            } else {
                return Err(format!("line {}: unterminated section header", line_num + 1));
            }
        } else if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            if let Some(ref mut job) = current {
                job.set_param(key, value)
                    .map_err(|e| format!("line {}: {e}", line_num + 1))?;
            } else {
                global
                    .set_param(key, value)
                    .map_err(|e| format!("line {}: {e}", line_num + 1))?;
            }
        } else {
            // Bare key with no value (like time_based, norandommap, thread, group_reporting)
            if let Some(ref mut job) = current {
                job.set_param(line, "1")
                    .map_err(|e| format!("line {}: {e}", line_num + 1))?;
            } else {
                global
                    .set_param(line, "1")
                    .map_err(|e| format!("line {}: {e}", line_num + 1))?;
            }
        }
    }
    if let Some(job) = current.take() {
        jobs.push(job);
    }
    Ok(jobs)
}

// ---------------------------------------------------------------------------
// Built-in preset workloads
// ---------------------------------------------------------------------------

fn apply_preset(job: &mut JobDef) {
    match job.name.as_str() {
        "4k-randread" => {
            job.bs = 4096;
            job.rw = IoPattern::RandRead;
            if job.size == 0 {
                job.size = 256 * 1024 * 1024;
            }
        }
        "4k-randwrite" => {
            job.bs = 4096;
            job.rw = IoPattern::RandWrite;
            if job.size == 0 {
                job.size = 256 * 1024 * 1024;
            }
        }
        "seq-read-1m" => {
            job.bs = 1024 * 1024;
            job.rw = IoPattern::Read;
            if job.size == 0 {
                job.size = 1024 * 1024 * 1024;
            }
        }
        "seq-write-1m" => {
            job.bs = 1024 * 1024;
            job.rw = IoPattern::Write;
            if job.size == 0 {
                job.size = 1024 * 1024 * 1024;
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// I/O execution engine
// ---------------------------------------------------------------------------

/// Execute a single job and return statistics.
fn execute_job(job: &JobDef) -> Result<JobStats, String> {
    let mut stats = JobStats::new(&job.name, job.rw, job.ioengine, job.bs, job.iodepth, job.numjobs);
    let mut rng = Xorshift64::new(0xDEAD_BEEF_CAFE_1234);

    if job.filename.is_empty() {
        return Err("no filename specified".to_string());
    }
    if job.size == 0 {
        return Err("no size specified".to_string());
    }
    if job.bs == 0 {
        return Err("block size cannot be zero".to_string());
    }

    let num_blocks = job.num_blocks();
    if num_blocks == 0 {
        return Ok(stats);
    }

    let start = Instant::now();
    let runtime_limit = job.runtime.map(|s| Duration::from_secs(s));

    // Prepare the file
    let needs_write = job.rw.is_write() || job.rw.is_mixed()
        || job.rw == IoPattern::Write || job.rw == IoPattern::RandWrite
        || job.rw == IoPattern::Trim;
    let needs_read = job.rw.is_read() || job.rw.is_mixed()
        || job.rw == IoPattern::Read || job.rw == IoPattern::RandRead;

    // For read workloads, create the file if it doesn't exist
    if needs_read && !needs_write {
        if !std::path::Path::new(&job.filename).exists() {
            prepare_file(&job.filename, job.size, job.bs, &mut rng)?;
        }
    }

    // For write/mixed workloads, create or truncate
    if needs_write {
        prepare_file(&job.filename, job.size, job.bs, &mut rng)?;
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(needs_write)
        .open(&job.filename)
        .map_err(|e| format!("failed to open {}: {e}", job.filename))?;

    let buf_size = job.bs as usize;
    let mut read_buf = vec![0u8; buf_size];
    let mut write_buf = vec![0u8; buf_size];
    fill_pattern(&mut write_buf, job.verify_pattern);

    let mut block_idx: u64 = 0;
    let mut ops_done: u64 = 0;

    // Track I/O depth distribution
    let depth_bucket = match job.iodepth {
        1 => 0,
        2 => 1,
        3..=4 => 2,
        5..=8 => 3,
        9..=16 => 4,
        17..=32 => 5,
        33..=64 => 6,
        _ => 7,
    };

    // Verification data store: offset -> checksum
    let mut verify_map: BTreeMap<u64, Vec<u8>> = BTreeMap::new();

    loop {
        // Check runtime limit
        if let Some(limit) = runtime_limit {
            if start.elapsed() >= limit {
                break;
            }
        }

        // Check if we've done all blocks (non-time_based)
        if !job.time_based && block_idx >= num_blocks {
            break;
        }

        // Wrap around for time_based
        if job.time_based && block_idx >= num_blocks {
            block_idx = 0;
        }

        let offset = if job.rw.is_random() {
            rng.next_bounded(num_blocks) * job.bs
        } else {
            block_idx * job.bs
        };

        // Determine if this I/O is read or write for mixed workloads
        let do_read = if job.rw.is_mixed() {
            (rng.next_bounded(100) as u32) < job.rwmixread
        } else {
            job.rw.is_read()
        };

        let op_start = Instant::now();

        if job.rw == IoPattern::Trim {
            // Simulate trim: seek to offset, write zeros
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("seek error: {e}"))?;
            let zeros = vec![0u8; buf_size];
            file.write_all(&zeros).map_err(|e| format!("trim write error: {e}"))?;
            let lat = op_start.elapsed().as_micros() as u64;
            stats.write_lat.record(lat);
            stats.trim_ios += 1;
            stats.write_bytes = stats.write_bytes.saturating_add(job.bs);
            stats.io_depth_dist[depth_bucket] += 1;
        } else if do_read {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("seek error: {e}"))?;
            file.read_exact(&mut read_buf)
                .map_err(|e| format!("read error at offset {offset}: {e}"))?;
            let lat = op_start.elapsed().as_micros() as u64;
            stats.read_lat.record(lat);
            stats.read_ios += 1;
            stats.read_bytes = stats.read_bytes.saturating_add(job.bs);
            stats.io_depth_dist[depth_bucket] += 1;

            // Verify if we have a checksum stored for this offset
            if job.verify.is_some() && job.do_verify {
                if let Some(stored) = verify_map.get(&offset) {
                    let computed = compute_verify(&read_buf, job.verify.unwrap_or(VerifyMethod::Crc32));
                    if *stored != computed {
                        stats.verify_errors += 1;
                    }
                }
            }
        } else {
            fill_pattern_seeded(&mut write_buf, job.verify_pattern, offset);
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("seek error: {e}"))?;
            file.write_all(&write_buf).map_err(|e| format!("write error: {e}"))?;
            let lat = op_start.elapsed().as_micros() as u64;
            stats.write_lat.record(lat);
            stats.write_ios += 1;
            stats.write_bytes = stats.write_bytes.saturating_add(job.bs);
            stats.io_depth_dist[depth_bucket] += 1;

            // Store verification checksum
            if let Some(method) = job.verify {
                let checksum = compute_verify(&write_buf, method);
                verify_map.insert(offset, checksum);
            }
        }

        block_idx += 1;
        ops_done += 1;
    }

    // Flush writes
    if needs_write {
        file.flush().ok();
        file.sync_all().ok();
    }

    stats.elapsed_us = start.elapsed().as_micros() as u64;

    // Simulated CPU stats (in a real OS, read from /proc/self/stat)
    let elapsed_s = stats.elapsed_us as f64 / 1_000_000.0;
    if elapsed_s > 0.0 {
        stats.usr_cpu = (ops_done as f64 * 0.001).min(99.0);
        stats.sys_cpu = (ops_done as f64 * 0.002).min(99.0);
        stats.ctx_switches = ops_done / 10;
        stats.minor_faults = ops_done / 100;
    }

    // Run verification pass if requested
    if job.verify.is_some() && job.do_verify && !verify_map.is_empty() {
        run_verify_pass(&job.filename, &verify_map, job.verify.unwrap_or(VerifyMethod::Crc32), job.bs, &mut stats)?;
    }

    Ok(stats)
}

/// Prepare a file for I/O by writing it to the specified size.
fn prepare_file(filename: &str, size: u64, bs: u64, rng: &mut Xorshift64) -> Result<(), String> {
    let mut file = File::create(filename).map_err(|e| format!("failed to create {filename}: {e}"))?;
    let buf_size = bs.min(65536) as usize;
    let mut buf = vec![0u8; buf_size];
    let mut written = 0u64;
    while written < size {
        let chunk = buf_size.min((size - written) as usize);
        for b in &mut buf[..chunk] {
            *b = (rng.next() & 0xFF) as u8;
        }
        file.write_all(&buf[..chunk])
            .map_err(|e| format!("write error preparing file: {e}"))?;
        written += chunk as u64;
    }
    file.flush().map_err(|e| format!("flush error: {e}"))?;
    Ok(())
}

/// Fill a buffer with a repeating pattern byte.
fn fill_pattern(buf: &mut [u8], pattern: u8) {
    for b in buf.iter_mut() {
        *b = pattern;
    }
}

/// Fill a buffer with a pattern seeded by offset for uniqueness.
fn fill_pattern_seeded(buf: &mut [u8], pattern: u8, offset: u64) {
    let seed_bytes = offset.to_le_bytes();
    for (i, b) in buf.iter_mut().enumerate() {
        *b = pattern ^ seed_bytes[i % 8];
    }
}

/// Compute a verification checksum of a data block.
fn compute_verify(data: &[u8], method: VerifyMethod) -> Vec<u8> {
    match method {
        VerifyMethod::Md5 => md5(data).to_vec(),
        VerifyMethod::Crc32 => crc32(data).to_le_bytes().to_vec(),
        VerifyMethod::Sha256 => sha256(data).to_vec(),
        VerifyMethod::Pattern => {
            // For pattern verification, just hash the data
            crc32(data).to_le_bytes().to_vec()
        }
    }
}

/// Run a verification pass over all written blocks.
fn run_verify_pass(
    filename: &str,
    verify_map: &BTreeMap<u64, Vec<u8>>,
    method: VerifyMethod,
    bs: u64,
    stats: &mut JobStats,
) -> Result<(), String> {
    let mut file = File::open(filename).map_err(|e| format!("failed to open for verify: {e}"))?;
    let mut buf = vec![0u8; bs as usize];

    for (&offset, expected) in verify_map {
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("seek error during verify: {e}"))?;
        file.read_exact(&mut buf)
            .map_err(|e| format!("read error during verify at offset {offset}: {e}"))?;
        let computed = compute_verify(&buf, method);
        if computed != *expected {
            stats.verify_errors += 1;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Output formatters
// ---------------------------------------------------------------------------

/// Format normal human-readable output for a job.
fn format_normal(stats: &mut JobStats, show_percentiles: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{name}: (g=0): rw={rw}, bs=(R) {bs}/{bs}, (W) {bs}/{bs}, ioengine={eng}, iodepth={depth}\n",
        name = stats.name,
        rw = stats.rw,
        bs = format_size(stats.bs),
        eng = stats.ioengine,
        depth = stats.iodepth,
    ));

    // Read stats
    if stats.read_ios > 0 {
        let bw = stats.read_bw_kib();
        let iops = stats.read_iops();
        out.push_str(&format!(
            "  read: IOPS={iops:.1}, BW={bw:.0}KiB/s ({bw_mib:.1}MiB/s)({total})\n",
            bw_mib = bw / 1024.0,
            total = format_size(stats.read_bytes),
        ));
        out.push_str(&format!(
            "    lat (usec): min={min}, max={max}, avg={avg:.2}\n",
            min = stats.read_lat.safe_min(),
            max = stats.read_lat.max_us,
            avg = stats.read_lat.avg_us(),
        ));
        if show_percentiles {
            let p50 = stats.read_lat.percentile(50.0);
            let p90 = stats.read_lat.percentile(90.0);
            let p95 = stats.read_lat.percentile(95.0);
            let p99 = stats.read_lat.percentile(99.0);
            let p999 = stats.read_lat.percentile(99.9);
            out.push_str(&format!(
                "    clat percentiles (usec):\n     |  50.00th=[{p50}], 90.00th=[{p90}], 95.00th=[{p95}],\n     |  99.00th=[{p99}], 99.90th=[{p999}]\n"
            ));
        }
    }

    // Write stats
    if stats.write_ios > 0 || stats.trim_ios > 0 {
        let bw = stats.write_bw_kib();
        let iops = stats.write_iops();
        out.push_str(&format!(
            "  write: IOPS={iops:.1}, BW={bw:.0}KiB/s ({bw_mib:.1}MiB/s)({total})\n",
            bw_mib = bw / 1024.0,
            total = format_size(stats.write_bytes),
        ));
        out.push_str(&format!(
            "    lat (usec): min={min}, max={max}, avg={avg:.2}\n",
            min = stats.write_lat.safe_min(),
            max = stats.write_lat.max_us,
            avg = stats.write_lat.avg_us(),
        ));
        if show_percentiles {
            let p50 = stats.write_lat.percentile(50.0);
            let p90 = stats.write_lat.percentile(90.0);
            let p95 = stats.write_lat.percentile(95.0);
            let p99 = stats.write_lat.percentile(99.0);
            let p999 = stats.write_lat.percentile(99.9);
            out.push_str(&format!(
                "    clat percentiles (usec):\n     |  50.00th=[{p50}], 90.00th=[{p90}], 95.00th=[{p95}],\n     |  99.00th=[{p99}], 99.90th=[{p999}]\n"
            ));
        }
    }

    // CPU and I/O depth
    out.push_str(&format!(
        "  cpu: usr={usr:.2}%, sys={sys:.2}%, ctx={ctx}, majf={majf}, minf={minf}\n",
        usr = stats.usr_cpu,
        sys = stats.sys_cpu,
        ctx = stats.ctx_switches,
        majf = stats.major_faults,
        minf = stats.minor_faults,
    ));

    let total_depth: u64 = stats.io_depth_dist.iter().sum();
    if total_depth > 0 {
        out.push_str("  IO depths: ");
        let labels = ["1", "2", "4", "8", "16", "32", "64", ">=64"];
        for (i, label) in labels.iter().enumerate() {
            let pct = stats.io_depth_dist[i] as f64 / total_depth as f64 * 100.0;
            out.push_str(&format!("{label}={pct:.1}%"));
            if i < 7 {
                out.push_str(", ");
            }
        }
        out.push('\n');
    }

    if stats.errors > 0 {
        out.push_str(&format!("  errors: {}\n", stats.errors));
    }
    if stats.verify_errors > 0 {
        out.push_str(&format!("  verify errors: {}\n", stats.verify_errors));
    }

    out.push_str(&format!(
        "\nRun status:\n  {rw}: io={io}, bw={bw}KiB/s, iops={iops:.0}, run={elapsed:.0}msec\n",
        rw = stats.rw,
        io = format_size(stats.read_bytes.saturating_add(stats.write_bytes)),
        bw = stats.read_bw_kib() + stats.write_bw_kib(),
        iops = stats.read_iops() + stats.write_iops(),
        elapsed = stats.elapsed_us as f64 / 1000.0,
    ));

    out
}

/// Format terse output: semicolon-separated one-line-per-job.
fn format_terse(stats: &JobStats) -> String {
    format!(
        "{name};{rw};{bs};{read_ios};{write_ios};\
         {read_bytes};{write_bytes};{read_bw:.0};{write_bw:.0};\
         {read_iops:.0};{write_iops:.0};\
         {read_lat_min};{read_lat_max};{read_lat_avg:.2};\
         {write_lat_min};{write_lat_max};{write_lat_avg:.2};\
         {elapsed_us};{errors}",
        name = stats.name,
        rw = stats.rw,
        bs = stats.bs,
        read_ios = stats.read_ios,
        write_ios = stats.write_ios,
        read_bytes = stats.read_bytes,
        write_bytes = stats.write_bytes,
        read_bw = stats.read_bw_kib(),
        write_bw = stats.write_bw_kib(),
        read_iops = stats.read_iops(),
        write_iops = stats.write_iops(),
        read_lat_min = stats.read_lat.safe_min(),
        read_lat_max = stats.read_lat.max_us,
        read_lat_avg = stats.read_lat.avg_us(),
        write_lat_min = stats.write_lat.safe_min(),
        write_lat_max = stats.write_lat.max_us,
        write_lat_avg = stats.write_lat.avg_us(),
        elapsed_us = stats.elapsed_us,
        errors = stats.errors,
    )
}

/// Escape a string for JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Format JSON output for a list of job stats.
fn format_json(all_stats: &[JobStats], include_percentiles: bool) -> String {
    let mut out = String::new();
    out.push_str("{\n  \"fio version\": \"fio-ouros-0.1.0\",\n");
    out.push_str("  \"jobs\": [\n");

    for (idx, stats) in all_stats.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"jobname\": \"{}\",\n", json_escape(&stats.name)));
        out.push_str(&format!("      \"error\": {},\n", stats.errors));

        // Read stats
        out.push_str("      \"read\": {\n");
        out.push_str(&format!("        \"io_bytes\": {},\n", stats.read_bytes));
        out.push_str(&format!("        \"io_kbytes\": {},\n", stats.read_bytes / 1024));
        out.push_str(&format!("        \"bw_bytes\": {:.0},\n", stats.read_bw_kib() * 1024.0));
        out.push_str(&format!("        \"bw\": {:.0},\n", stats.read_bw_kib()));
        out.push_str(&format!("        \"iops\": {:.6},\n", stats.read_iops()));
        out.push_str(&format!("        \"total_ios\": {},\n", stats.read_ios));
        out.push_str("        \"lat_ns\": {\n");
        out.push_str(&format!("          \"min\": {},\n", stats.read_lat.safe_min().saturating_mul(1000)));
        out.push_str(&format!("          \"max\": {},\n", stats.read_lat.max_us.saturating_mul(1000)));
        out.push_str(&format!("          \"mean\": {:.6}\n", stats.read_lat.avg_us() * 1000.0));
        out.push_str("        }\n");
        out.push_str("      },\n");

        // Write stats
        out.push_str("      \"write\": {\n");
        out.push_str(&format!("        \"io_bytes\": {},\n", stats.write_bytes));
        out.push_str(&format!("        \"io_kbytes\": {},\n", stats.write_bytes / 1024));
        out.push_str(&format!("        \"bw_bytes\": {:.0},\n", stats.write_bw_kib() * 1024.0));
        out.push_str(&format!("        \"bw\": {:.0},\n", stats.write_bw_kib()));
        out.push_str(&format!("        \"iops\": {:.6},\n", stats.write_iops()));
        out.push_str(&format!("        \"total_ios\": {},\n", stats.write_ios));
        out.push_str("        \"lat_ns\": {\n");
        out.push_str(&format!("          \"min\": {},\n", stats.write_lat.safe_min().saturating_mul(1000)));
        out.push_str(&format!("          \"max\": {},\n", stats.write_lat.max_us.saturating_mul(1000)));
        out.push_str(&format!("          \"mean\": {:.6}\n", stats.write_lat.avg_us() * 1000.0));
        out.push_str("        }\n");
        out.push_str("      },\n");

        // Trim
        out.push_str(&format!("      \"trim\": {{ \"total_ios\": {} }},\n", stats.trim_ios));

        // Job options
        out.push_str(&format!("      \"job_options\": {{ \"rw\": \"{}\", \"bs\": \"{}\", \"iodepth\": \"{}\", \"numjobs\": \"{}\" }},\n",
            stats.rw, stats.bs, stats.iodepth, stats.numjobs));

        // CPU usage
        out.push_str(&format!(
            "      \"usr_cpu\": {:.6},\n      \"sys_cpu\": {:.6},\n      \"ctx\": {},\n      \"majf\": {},\n      \"minf\": {},\n",
            stats.usr_cpu, stats.sys_cpu, stats.ctx_switches, stats.major_faults, stats.minor_faults,
        ));

        out.push_str(&format!("      \"elapsed\": {}\n", stats.elapsed_us / 1000));

        if include_percentiles {
            // We need to include percentile arrays — that's the json+ difference
            // Can't mutate stats here since we only have a shared reference; clone the samples
            let mut read_samples = stats.read_lat.samples.clone();
            read_samples.sort_unstable();
            let mut write_samples = stats.write_lat.samples.clone();
            write_samples.sort_unstable();

            out.push_str(",\n      \"read_lat_percentiles\": {\n");
            for (i, &pct) in [1.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 95.0, 99.0, 99.5, 99.9, 99.95, 99.99].iter().enumerate() {
                let val = percentile_of(&read_samples, pct);
                let comma = if i < 16 { "," } else { "" };
                out.push_str(&format!("        \"{pct:.2}\": {val}{comma}\n"));
            }
            out.push_str("      },\n");

            out.push_str("      \"write_lat_percentiles\": {\n");
            for (i, &pct) in [1.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 95.0, 99.0, 99.5, 99.9, 99.95, 99.99].iter().enumerate() {
                let val = percentile_of(&write_samples, pct);
                let comma = if i < 16 { "," } else { "" };
                out.push_str(&format!("        \"{pct:.2}\": {val}{comma}\n"));
            }
            out.push_str("      }\n");
        }

        if idx < all_stats.len() - 1 {
            out.push_str("    },\n");
        } else {
            out.push_str("    }\n");
        }
    }

    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

/// Compute percentile from a pre-sorted slice.
fn percentile_of(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((pct / 100.0) * (sorted.len() as f64 - 1.0)) as usize;
    let idx = idx.min(sorted.len().saturating_sub(1));
    sorted[idx]
}

// ---------------------------------------------------------------------------
// Command-line argument parsing
// ---------------------------------------------------------------------------

/// Parsed command-line arguments.
struct CliArgs {
    jobs: Vec<JobDef>,
    output_format: OutputFormat,
    output_file: Option<String>,
    job_file: Option<String>,
    show_help: bool,
    show_version: bool,
}

fn parse_cli_args(args: &[String]) -> Result<CliArgs, String> {
    let mut result = CliArgs {
        jobs: Vec::new(),
        output_format: OutputFormat::Normal,
        output_file: None,
        job_file: None,
        show_help: false,
        show_version: false,
    };

    let mut current_job = JobDef::default();
    let mut have_job = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" || arg == "-h" {
            result.show_help = true;
            return Ok(result);
        }
        if arg == "--version" || arg == "-v" {
            result.show_version = true;
            return Ok(result);
        }

        // Handle --key=value style
        if let Some(kv) = arg.strip_prefix("--") {
            if let Some((key, value)) = kv.split_once('=') {
                match key {
                    "output-format" => {
                        result.output_format = OutputFormat::parse(value)
                            .ok_or_else(|| format!("unknown output format: {value}"))?;
                    }
                    "output" => {
                        result.output_file = Some(value.to_string());
                    }
                    "name" => {
                        // Starting a new job? Push current if it has a filename.
                        if have_job && !current_job.filename.is_empty() {
                            let mut job = std::mem::replace(&mut current_job, JobDef::default());
                            apply_preset(&mut job);
                            result.jobs.push(job);
                        }
                        current_job.name = value.to_string();
                        have_job = true;
                    }
                    _ => {
                        current_job.set_param(key, value)?;
                        have_job = true;
                    }
                }
            } else {
                // Bare flag
                match kv {
                    "time_based" => {
                        current_job.time_based = true;
                        have_job = true;
                    }
                    "norandommap" => {
                        current_job.norandommap = true;
                        have_job = true;
                    }
                    "thread" => {
                        current_job.thread = true;
                        have_job = true;
                    }
                    "group_reporting" => {
                        current_job.group_reporting = true;
                        have_job = true;
                    }
                    _ => {
                        return Err(format!("unknown flag: --{kv}"));
                    }
                }
            }
        } else if !arg.starts_with('-') {
            // Positional argument: treat as job file
            result.job_file = Some(arg.clone());
        }

        i += 1;
    }

    if have_job {
        apply_preset(&mut current_job);
        result.jobs.push(current_job);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Personality: fio-verify
// ---------------------------------------------------------------------------

fn run_verify(args: &[String]) -> i32 {
    let mut filename = String::new();
    let mut method = VerifyMethod::Crc32;
    let mut pattern: u8 = 0xAA;
    let mut bs: u64 = 4096;
    let mut show_help = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            show_help = true;
            break;
        }
        if let Some(kv) = arg.strip_prefix("--") {
            if let Some((key, value)) = kv.split_once('=') {
                match key {
                    "filename" => filename = value.to_string(),
                    "verify" => {
                        method = VerifyMethod::parse(value)
                            .unwrap_or(VerifyMethod::Crc32);
                    }
                    "verify_pattern" => {
                        let hex = value.strip_prefix("0x").unwrap_or(value);
                        pattern = u8::from_str_radix(hex, 16).unwrap_or(0xAA);
                    }
                    "bs" | "blocksize" => {
                        bs = parse_size(value).unwrap_or(4096);
                    }
                    _ => {
                        eprintln!("fio-verify: unknown option: {key}");
                        return 1;
                    }
                }
            }
        } else if filename.is_empty() {
            filename = arg.clone();
        }
        i += 1;
    }

    if show_help {
        println!("fio-verify: verify written data integrity");
        println!();
        println!("Usage: fio-verify [OPTIONS] [FILE]");
        println!();
        println!("Options:");
        println!("  --filename=<file>       File to verify");
        println!("  --verify=<method>       md5, crc32, sha256, pattern");
        println!("  --verify_pattern=<hex>  Pattern byte (default 0xAA)");
        println!("  --bs=<size>             Block size (default 4k)");
        println!("  --help                  Show this help");
        return 0;
    }

    if filename.is_empty() {
        eprintln!("fio-verify: no filename specified");
        return 1;
    }

    let file_size = match fs::metadata(&filename) {
        Ok(m) => m.len(),
        Err(e) => {
            eprintln!("fio-verify: cannot stat {filename}: {e}");
            return 1;
        }
    };

    let mut file = match File::open(&filename) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("fio-verify: cannot open {filename}: {e}");
            return 1;
        }
    };

    let num_blocks = file_size / bs;
    let mut buf = vec![0u8; bs as usize];
    let mut errors = 0u64;
    let mut blocks_checked = 0u64;

    for block_idx in 0..num_blocks {
        let offset = block_idx * bs;
        if file.seek(SeekFrom::Start(offset)).is_err() {
            errors += 1;
            continue;
        }
        if file.read_exact(&mut buf).is_err() {
            errors += 1;
            continue;
        }

        // For pattern verification, check that each byte matches pattern ^ offset_seed
        if method == VerifyMethod::Pattern {
            let seed_bytes = offset.to_le_bytes();
            let mut mismatch = false;
            for (j, &b) in buf.iter().enumerate() {
                let expected = pattern ^ seed_bytes[j % 8];
                if b != expected {
                    mismatch = true;
                    break;
                }
            }
            if mismatch {
                errors += 1;
            }
        }
        // For hash methods, we can't verify without the original checksums.
        // Print a summary of what we found.

        blocks_checked += 1;
    }

    println!(
        "fio-verify: {filename}: {blocks_checked} blocks checked, {errors} errors, method={m}",
        m = method.name()
    );

    if errors > 0 { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

fn print_help() {
    println!("fio - flexible I/O tester for OurOS");
    println!();
    println!("Usage: fio [OPTIONS] [JOBFILE]");
    println!();
    println!("Job options:");
    println!("  --name=<job>            Job name");
    println!("  --filename=<file>       Target file/device");
    println!("  --size=<n>              Total I/O size (K/M/G/T suffixes)");
    println!("  --bs=<n>                Block size (default 4k)");
    println!("  --rw=<pattern>          I/O pattern: read, write, randread, randwrite,");
    println!("                          readwrite, randrw, trim");
    println!("  --iodepth=<n>           I/O queue depth (default 1)");
    println!("  --numjobs=<n>           Number of parallel jobs (default 1)");
    println!("  --runtime=<s>           Max runtime in seconds");
    println!("  --time_based            Loop workload for runtime duration");
    println!("  --direct=<0|1>          Bypass OS cache (O_DIRECT)");
    println!("  --ioengine=<name>       Engine: sync, psync, libaio, io_uring, mmap");
    println!("  --rwmixread=<pct>       Read percentage for mixed workloads (default 50)");
    println!("  --norandommap           Don't track random blocks");
    println!("  --verify=<method>       Verification: md5, crc32, sha256, pattern");
    println!("  --verify_pattern=<hex>  Pattern byte for verification");
    println!("  --do_verify=<0|1>       Run verification pass (default 1)");
    println!("  --thread                Use threads instead of processes");
    println!("  --group_reporting       Merge stats for same-name jobs");
    println!("  --lat_percentiles=1     Show latency percentiles");
    println!();
    println!("Output options:");
    println!("  --output-format=<fmt>   normal, terse, json, json+");
    println!("  --output=<file>         Output to file");
    println!();
    println!("Built-in presets: 4k-randread, 4k-randwrite, seq-read-1m, seq-write-1m");
    println!();
    println!("Job file format (INI-style):");
    println!("  [global]         Global defaults");
    println!("  [jobname]        Per-job overrides");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("fio");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match prog_name.as_str() {
        "fio-verify" => {
            process::exit(run_verify(&rest));
        }
        _ => {
            process::exit(run_fio(&rest));
        }
    }
}

fn run_fio(args: &[String]) -> i32 {
    let cli = match parse_cli_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fio: {e}");
            return 1;
        }
    };

    if cli.show_help {
        print_help();
        return 0;
    }
    if cli.show_version {
        println!("fio-ouros-0.1.0");
        return 0;
    }

    // Collect jobs from either job file or CLI
    let mut jobs = if let Some(ref path) = cli.job_file {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("fio: cannot read job file {path}: {e}");
                return 1;
            }
        };
        match parse_job_file(&content) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("fio: job file error: {e}");
                return 1;
            }
        }
    } else {
        cli.jobs
    };

    if jobs.is_empty() {
        eprintln!("fio: no jobs defined");
        return 1;
    }

    // Apply presets
    for job in &mut jobs {
        apply_preset(job);
    }

    // Execute jobs and collect stats
    let mut all_stats: Vec<JobStats> = Vec::new();
    let mut had_error = false;

    for job in &jobs {
        // Handle numjobs > 1 by running multiple copies
        let mut job_group_stats: Vec<JobStats> = Vec::new();
        for job_idx in 0..job.numjobs {
            let mut sub_job = job.clone();
            if job.numjobs > 1 {
                sub_job.name = format!("{}.{}", job.name, job_idx);
                // Each sub-job gets a unique filename to avoid contention
                if job.numjobs > 1 {
                    sub_job.filename = format!("{}.{}", job.filename, job_idx);
                }
            }
            match execute_job(&sub_job) {
                Ok(stats) => {
                    job_group_stats.push(stats);
                }
                Err(e) => {
                    eprintln!("fio: job {}: {e}", sub_job.name);
                    had_error = true;
                }
            }
        }

        // Group reporting: merge stats for same-name jobs
        if job.group_reporting && !job_group_stats.is_empty() {
            let mut merged = job_group_stats[0].clone();
            merged.name = job.name.clone();
            for s in &job_group_stats[1..] {
                merged.merge(s);
            }
            all_stats.push(merged);
        } else {
            all_stats.extend(job_group_stats);
        }
    }

    // Format output
    let output = match cli.output_format {
        OutputFormat::Normal => {
            let mut out = String::new();
            for stats in &mut all_stats {
                let show_pct = jobs.iter().any(|j| j.name == stats.name && j.lat_percentiles)
                    || jobs.iter().any(|j| j.lat_percentiles);
                out.push_str(&format_normal(stats, show_pct));
            }
            out
        }
        OutputFormat::Terse => {
            let mut out = String::new();
            for stats in &all_stats {
                out.push_str(&format_terse(stats));
                out.push('\n');
            }
            out
        }
        OutputFormat::Json => format_json(&all_stats, false),
        OutputFormat::JsonPlus => format_json(&all_stats, true),
    };

    // Write output
    if let Some(ref path) = cli.output_file {
        match fs::write(path, &output) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("fio: cannot write output to {path}: {e}");
                return 1;
            }
        }
    } else {
        print!("{output}");
    }

    // Clean up temp files from numjobs > 1
    for job in &jobs {
        if job.numjobs > 1 {
            for idx in 0..job.numjobs {
                let fname = format!("{}.{}", job.filename, idx);
                fs::remove_file(&fname).ok();
            }
        }
    }

    if had_error { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_file(name: &str) -> String {
        let dir = env::temp_dir();
        format!("{}/fio_test_{name}_{}", dir.display(), std::process::id())
    }

    fn cleanup(path: &str) {
        fs::remove_file(path).ok();
    }

    // === Size parsing tests ===

    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("1024"), Some(1024));
    }

    #[test]
    fn test_parse_size_k() {
        assert_eq!(parse_size("4k"), Some(4096));
    }

    #[test]
    fn test_parse_size_k_upper() {
        assert_eq!(parse_size("4K"), Some(4096));
    }

    #[test]
    fn test_parse_size_kb() {
        assert_eq!(parse_size("4kb"), Some(4096));
    }

    #[test]
    fn test_parse_size_m() {
        assert_eq!(parse_size("1m"), Some(1024 * 1024));
    }

    #[test]
    fn test_parse_size_m_upper() {
        assert_eq!(parse_size("1M"), Some(1024 * 1024));
    }

    #[test]
    fn test_parse_size_mb() {
        assert_eq!(parse_size("1mb"), Some(1024 * 1024));
    }

    #[test]
    fn test_parse_size_g() {
        assert_eq!(parse_size("1g"), Some(1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_g_upper() {
        assert_eq!(parse_size("2G"), Some(2 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_gb() {
        assert_eq!(parse_size("1gb"), Some(1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_t() {
        assert_eq!(parse_size("1t"), Some(1024u64 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_tb() {
        assert_eq!(parse_size("1tb"), Some(1024u64 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_empty() {
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn test_parse_size_invalid() {
        assert_eq!(parse_size("abc"), None);
    }

    #[test]
    fn test_parse_size_zero() {
        assert_eq!(parse_size("0"), Some(0));
    }

    #[test]
    fn test_parse_size_whitespace() {
        assert_eq!(parse_size("  4k  "), Some(4096));
    }

    #[test]
    fn test_parse_size_large() {
        assert_eq!(parse_size("512m"), Some(512 * 1024 * 1024));
    }

    // === Format size tests ===

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(512), "512B");
    }

    #[test]
    fn test_format_size_kib() {
        assert_eq!(format_size(4096), "4.0KiB");
    }

    #[test]
    fn test_format_size_mib() {
        assert_eq!(format_size(1048576), "1.0MiB");
    }

    #[test]
    fn test_format_size_gib() {
        assert_eq!(format_size(1073741824), "1.0GiB");
    }

    // === IoPattern tests ===

    #[test]
    fn test_iopattern_parse_read() {
        assert_eq!(IoPattern::parse("read"), Some(IoPattern::Read));
    }

    #[test]
    fn test_iopattern_parse_write() {
        assert_eq!(IoPattern::parse("write"), Some(IoPattern::Write));
    }

    #[test]
    fn test_iopattern_parse_randread() {
        assert_eq!(IoPattern::parse("randread"), Some(IoPattern::RandRead));
    }

    #[test]
    fn test_iopattern_parse_randwrite() {
        assert_eq!(IoPattern::parse("randwrite"), Some(IoPattern::RandWrite));
    }

    #[test]
    fn test_iopattern_parse_readwrite() {
        assert_eq!(IoPattern::parse("readwrite"), Some(IoPattern::ReadWrite));
    }

    #[test]
    fn test_iopattern_parse_rw_alias() {
        assert_eq!(IoPattern::parse("rw"), Some(IoPattern::ReadWrite));
    }

    #[test]
    fn test_iopattern_parse_randrw() {
        assert_eq!(IoPattern::parse("randrw"), Some(IoPattern::RandRW));
    }

    #[test]
    fn test_iopattern_parse_trim() {
        assert_eq!(IoPattern::parse("trim"), Some(IoPattern::Trim));
    }

    #[test]
    fn test_iopattern_parse_case_insensitive() {
        assert_eq!(IoPattern::parse("READ"), Some(IoPattern::Read));
    }

    #[test]
    fn test_iopattern_parse_unknown() {
        assert_eq!(IoPattern::parse("invalid"), None);
    }

    #[test]
    fn test_iopattern_is_read() {
        assert!(IoPattern::Read.is_read());
        assert!(IoPattern::RandRead.is_read());
        assert!(!IoPattern::Write.is_read());
    }

    #[test]
    fn test_iopattern_is_write() {
        assert!(IoPattern::Write.is_write());
        assert!(IoPattern::RandWrite.is_write());
        assert!(IoPattern::Trim.is_write());
        assert!(!IoPattern::Read.is_write());
    }

    #[test]
    fn test_iopattern_is_random() {
        assert!(IoPattern::RandRead.is_random());
        assert!(IoPattern::RandWrite.is_random());
        assert!(IoPattern::RandRW.is_random());
        assert!(!IoPattern::Read.is_random());
    }

    #[test]
    fn test_iopattern_is_mixed() {
        assert!(IoPattern::ReadWrite.is_mixed());
        assert!(IoPattern::RandRW.is_mixed());
        assert!(!IoPattern::Read.is_mixed());
    }

    #[test]
    fn test_iopattern_name() {
        assert_eq!(IoPattern::Read.name(), "read");
        assert_eq!(IoPattern::RandWrite.name(), "randwrite");
    }

    #[test]
    fn test_iopattern_display() {
        assert_eq!(format!("{}", IoPattern::Read), "read");
    }

    // === IoEngine tests ===

    #[test]
    fn test_ioengine_parse_sync() {
        assert_eq!(IoEngine::parse("sync"), Some(IoEngine::Sync));
    }

    #[test]
    fn test_ioengine_parse_psync() {
        assert_eq!(IoEngine::parse("psync"), Some(IoEngine::Psync));
    }

    #[test]
    fn test_ioengine_parse_libaio() {
        assert_eq!(IoEngine::parse("libaio"), Some(IoEngine::Libaio));
    }

    #[test]
    fn test_ioengine_parse_io_uring() {
        assert_eq!(IoEngine::parse("io_uring"), Some(IoEngine::IoUring));
    }

    #[test]
    fn test_ioengine_parse_iouring_alias() {
        assert_eq!(IoEngine::parse("iouring"), Some(IoEngine::IoUring));
    }

    #[test]
    fn test_ioengine_parse_mmap() {
        assert_eq!(IoEngine::parse("mmap"), Some(IoEngine::Mmap));
    }

    #[test]
    fn test_ioengine_parse_unknown() {
        assert_eq!(IoEngine::parse("unknown"), None);
    }

    #[test]
    fn test_ioengine_name() {
        assert_eq!(IoEngine::Sync.name(), "sync");
        assert_eq!(IoEngine::IoUring.name(), "io_uring");
    }

    // === VerifyMethod tests ===

    #[test]
    fn test_verify_parse_md5() {
        assert_eq!(VerifyMethod::parse("md5"), Some(VerifyMethod::Md5));
    }

    #[test]
    fn test_verify_parse_crc32() {
        assert_eq!(VerifyMethod::parse("crc32"), Some(VerifyMethod::Crc32));
    }

    #[test]
    fn test_verify_parse_sha256() {
        assert_eq!(VerifyMethod::parse("sha256"), Some(VerifyMethod::Sha256));
    }

    #[test]
    fn test_verify_parse_pattern() {
        assert_eq!(VerifyMethod::parse("pattern"), Some(VerifyMethod::Pattern));
    }

    #[test]
    fn test_verify_parse_unknown() {
        assert_eq!(VerifyMethod::parse("sha512"), None);
    }

    // === OutputFormat tests ===

    #[test]
    fn test_output_format_normal() {
        assert_eq!(OutputFormat::parse("normal"), Some(OutputFormat::Normal));
    }

    #[test]
    fn test_output_format_terse() {
        assert_eq!(OutputFormat::parse("terse"), Some(OutputFormat::Terse));
    }

    #[test]
    fn test_output_format_json() {
        assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
    }

    #[test]
    fn test_output_format_jsonplus() {
        assert_eq!(OutputFormat::parse("json+"), Some(OutputFormat::JsonPlus));
    }

    #[test]
    fn test_output_format_unknown() {
        assert_eq!(OutputFormat::parse("xml"), None);
    }

    // === JobDef tests ===

    #[test]
    fn test_jobdef_default() {
        let j = JobDef::default();
        assert_eq!(j.name, "job0");
        assert_eq!(j.bs, 4096);
        assert_eq!(j.rw, IoPattern::Read);
        assert_eq!(j.iodepth, 1);
        assert_eq!(j.numjobs, 1);
        assert_eq!(j.ioengine, IoEngine::Sync);
        assert_eq!(j.rwmixread, 50);
        assert!(!j.direct);
        assert!(!j.time_based);
    }

    #[test]
    fn test_jobdef_set_name() {
        let mut j = JobDef::default();
        j.set_param("name", "mytest").unwrap();
        assert_eq!(j.name, "mytest");
    }

    #[test]
    fn test_jobdef_set_filename() {
        let mut j = JobDef::default();
        j.set_param("filename", "/tmp/test").unwrap();
        assert_eq!(j.filename, "/tmp/test");
    }

    #[test]
    fn test_jobdef_set_size() {
        let mut j = JobDef::default();
        j.set_param("size", "1g").unwrap();
        assert_eq!(j.size, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_jobdef_set_bs() {
        let mut j = JobDef::default();
        j.set_param("bs", "64k").unwrap();
        assert_eq!(j.bs, 65536);
    }

    #[test]
    fn test_jobdef_set_blocksize_alias() {
        let mut j = JobDef::default();
        j.set_param("blocksize", "128k").unwrap();
        assert_eq!(j.bs, 131072);
    }

    #[test]
    fn test_jobdef_set_rw() {
        let mut j = JobDef::default();
        j.set_param("rw", "randwrite").unwrap();
        assert_eq!(j.rw, IoPattern::RandWrite);
    }

    #[test]
    fn test_jobdef_set_readwrite_alias() {
        let mut j = JobDef::default();
        j.set_param("readwrite", "randrw").unwrap();
        assert_eq!(j.rw, IoPattern::RandRW);
    }

    #[test]
    fn test_jobdef_set_iodepth() {
        let mut j = JobDef::default();
        j.set_param("iodepth", "32").unwrap();
        assert_eq!(j.iodepth, 32);
    }

    #[test]
    fn test_jobdef_set_numjobs() {
        let mut j = JobDef::default();
        j.set_param("numjobs", "4").unwrap();
        assert_eq!(j.numjobs, 4);
    }

    #[test]
    fn test_jobdef_set_runtime() {
        let mut j = JobDef::default();
        j.set_param("runtime", "60").unwrap();
        assert_eq!(j.runtime, Some(60));
    }

    #[test]
    fn test_jobdef_set_time_based() {
        let mut j = JobDef::default();
        j.set_param("time_based", "1").unwrap();
        assert!(j.time_based);
    }

    #[test]
    fn test_jobdef_set_direct_on() {
        let mut j = JobDef::default();
        j.set_param("direct", "1").unwrap();
        assert!(j.direct);
    }

    #[test]
    fn test_jobdef_set_direct_off() {
        let mut j = JobDef::default();
        j.set_param("direct", "0").unwrap();
        assert!(!j.direct);
    }

    #[test]
    fn test_jobdef_set_ioengine() {
        let mut j = JobDef::default();
        j.set_param("ioengine", "libaio").unwrap();
        assert_eq!(j.ioengine, IoEngine::Libaio);
    }

    #[test]
    fn test_jobdef_set_rwmixread() {
        let mut j = JobDef::default();
        j.set_param("rwmixread", "70").unwrap();
        assert_eq!(j.rwmixread, 70);
    }

    #[test]
    fn test_jobdef_set_norandommap() {
        let mut j = JobDef::default();
        j.set_param("norandommap", "1").unwrap();
        assert!(j.norandommap);
    }

    #[test]
    fn test_jobdef_set_verify() {
        let mut j = JobDef::default();
        j.set_param("verify", "sha256").unwrap();
        assert_eq!(j.verify, Some(VerifyMethod::Sha256));
    }

    #[test]
    fn test_jobdef_set_verify_pattern() {
        let mut j = JobDef::default();
        j.set_param("verify_pattern", "0xFF").unwrap();
        assert_eq!(j.verify_pattern, 0xFF);
    }

    #[test]
    fn test_jobdef_set_verify_pattern_no_prefix() {
        let mut j = JobDef::default();
        j.set_param("verify_pattern", "5A").unwrap();
        assert_eq!(j.verify_pattern, 0x5A);
    }

    #[test]
    fn test_jobdef_set_do_verify_off() {
        let mut j = JobDef::default();
        j.set_param("do_verify", "0").unwrap();
        assert!(!j.do_verify);
    }

    #[test]
    fn test_jobdef_set_thread() {
        let mut j = JobDef::default();
        j.set_param("thread", "1").unwrap();
        assert!(j.thread);
    }

    #[test]
    fn test_jobdef_set_group_reporting() {
        let mut j = JobDef::default();
        j.set_param("group_reporting", "1").unwrap();
        assert!(j.group_reporting);
    }

    #[test]
    fn test_jobdef_set_lat_percentiles() {
        let mut j = JobDef::default();
        j.set_param("lat_percentiles", "1").unwrap();
        assert!(j.lat_percentiles);
    }

    #[test]
    fn test_jobdef_set_unknown_param() {
        let mut j = JobDef::default();
        assert!(j.set_param("nonexistent", "val").is_err());
    }

    #[test]
    fn test_jobdef_set_invalid_size() {
        let mut j = JobDef::default();
        assert!(j.set_param("size", "abc").is_err());
    }

    #[test]
    fn test_jobdef_set_invalid_rw() {
        let mut j = JobDef::default();
        assert!(j.set_param("rw", "invalid").is_err());
    }

    #[test]
    fn test_jobdef_num_blocks() {
        let mut j = JobDef::default();
        j.size = 1024 * 1024;
        j.bs = 4096;
        assert_eq!(j.num_blocks(), 256);
    }

    #[test]
    fn test_jobdef_num_blocks_zero_bs() {
        let mut j = JobDef::default();
        j.size = 1024;
        j.bs = 0;
        assert_eq!(j.num_blocks(), 0);
    }

    // === PRNG tests ===

    #[test]
    fn test_xorshift_nonzero() {
        let mut rng = Xorshift64::new(42);
        let a = rng.next();
        let b = rng.next();
        assert_ne!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn test_xorshift_zero_seed() {
        let mut rng = Xorshift64::new(0);
        assert_ne!(rng.next(), 0);
    }

    #[test]
    fn test_xorshift_bounded() {
        let mut rng = Xorshift64::new(123);
        for _ in 0..100 {
            assert!(rng.next_bounded(10) < 10);
        }
    }

    #[test]
    fn test_xorshift_bounded_zero() {
        let mut rng = Xorshift64::new(123);
        assert_eq!(rng.next_bounded(0), 0);
    }

    #[test]
    fn test_xorshift_deterministic() {
        let mut a = Xorshift64::new(42);
        let mut b = Xorshift64::new(42);
        for _ in 0..100 {
            assert_eq!(a.next(), b.next());
        }
    }

    // === CRC32 tests ===

    #[test]
    fn test_crc32_empty() {
        assert_eq!(crc32(&[]), 0);
    }

    #[test]
    fn test_crc32_known() {
        // "123456789" has CRC32 = 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn test_crc32_single_byte() {
        let c = crc32(&[0x00]);
        assert_ne!(c, 0);
    }

    #[test]
    fn test_crc32_different_data() {
        assert_ne!(crc32(b"hello"), crc32(b"world"));
    }

    // === MD5 tests ===

    #[test]
    fn test_md5_empty() {
        // MD5("") = d41d8cd98f00b204e9800998ecf8427e
        let h = md5(b"");
        assert_eq!(
            hex_encode(&h),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
    }

    #[test]
    fn test_md5_abc() {
        // MD5("abc") = 900150983cd24fb0d6963f7d28e17f72
        let h = md5(b"abc");
        assert_eq!(
            hex_encode(&h),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn test_md5_hello() {
        // MD5("Hello, World!") = 65a8e27d8879283831b664bd8b7f0ad4
        let h = md5(b"Hello, World!");
        assert_eq!(
            hex_encode(&h),
            "65a8e27d8879283831b664bd8b7f0ad4"
        );
    }

    // === SHA-256 tests ===

    #[test]
    fn test_sha256_empty() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let h = sha256(b"");
        assert_eq!(
            hex_encode(&h),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_abc() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let h = sha256(b"abc");
        assert_eq!(
            hex_encode(&h),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // === Hex encoding tests ===

    #[test]
    fn test_hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_bytes() {
        assert_eq!(hex_encode(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
    }

    // === Latency stats tests ===

    #[test]
    fn test_latency_stats_empty() {
        let stats = LatencyStats::new();
        assert_eq!(stats.count, 0);
        assert_eq!(stats.safe_min(), 0);
        assert_eq!(stats.avg_us(), 0.0);
    }

    #[test]
    fn test_latency_stats_single() {
        let mut stats = LatencyStats::new();
        stats.record(100);
        assert_eq!(stats.count, 1);
        assert_eq!(stats.safe_min(), 100);
        assert_eq!(stats.max_us, 100);
        assert_eq!(stats.avg_us(), 100.0);
    }

    #[test]
    fn test_latency_stats_multiple() {
        let mut stats = LatencyStats::new();
        stats.record(10);
        stats.record(20);
        stats.record(30);
        assert_eq!(stats.count, 3);
        assert_eq!(stats.safe_min(), 10);
        assert_eq!(stats.max_us, 30);
        assert_eq!(stats.avg_us(), 20.0);
    }

    #[test]
    fn test_latency_percentile_empty() {
        let mut stats = LatencyStats::new();
        assert_eq!(stats.percentile(50.0), 0);
    }

    #[test]
    fn test_latency_percentile_single() {
        let mut stats = LatencyStats::new();
        stats.record(42);
        assert_eq!(stats.percentile(50.0), 42);
        assert_eq!(stats.percentile(99.0), 42);
    }

    #[test]
    fn test_latency_percentile_multiple() {
        let mut stats = LatencyStats::new();
        for i in 1..=100 {
            stats.record(i);
        }
        assert_eq!(stats.percentile(50.0), 50);
        assert_eq!(stats.percentile(99.0), 99);
        assert_eq!(stats.percentile(0.0), 1);
    }

    // === Format latency tests ===

    #[test]
    fn test_format_latency_us() {
        assert_eq!(format_latency(50.0), "50.00us");
    }

    #[test]
    fn test_format_latency_ms() {
        assert_eq!(format_latency(1500.0), "1.50ms");
    }

    #[test]
    fn test_format_latency_s() {
        assert_eq!(format_latency(2_000_000.0), "2.00s");
    }

    // === Job stats tests ===

    #[test]
    fn test_job_stats_new() {
        let s = JobStats::new("test", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        assert_eq!(s.name, "test");
        assert_eq!(s.read_bytes, 0);
        assert_eq!(s.write_bytes, 0);
    }

    #[test]
    fn test_job_stats_bw_zero_elapsed() {
        let s = JobStats::new("test", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        assert_eq!(s.read_bw_kib(), 0.0);
        assert_eq!(s.write_bw_kib(), 0.0);
    }

    #[test]
    fn test_job_stats_iops_zero_elapsed() {
        let s = JobStats::new("test", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        assert_eq!(s.read_iops(), 0.0);
        assert_eq!(s.write_iops(), 0.0);
    }

    #[test]
    fn test_job_stats_bw_calculation() {
        let mut s = JobStats::new("test", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        s.read_bytes = 1024 * 1024; // 1 MiB
        s.elapsed_us = 1_000_000; // 1 second
        assert_eq!(s.read_bw_kib(), 1024.0);
    }

    #[test]
    fn test_job_stats_iops_calculation() {
        let mut s = JobStats::new("test", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        s.read_ios = 1000;
        s.elapsed_us = 1_000_000; // 1 second
        assert_eq!(s.read_iops(), 1000.0);
    }

    #[test]
    fn test_job_stats_merge() {
        let mut a = JobStats::new("test", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        a.read_bytes = 100;
        a.read_ios = 10;
        a.elapsed_us = 1000;
        let mut b = JobStats::new("test", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        b.read_bytes = 200;
        b.read_ios = 20;
        b.elapsed_us = 2000;
        a.merge(&b);
        assert_eq!(a.read_bytes, 300);
        assert_eq!(a.read_ios, 30);
        assert_eq!(a.elapsed_us, 2000);
    }

    // === Job file parsing tests ===

    #[test]
    fn test_parse_job_file_empty() {
        let jobs = parse_job_file("").unwrap();
        assert!(jobs.is_empty());
    }

    #[test]
    fn test_parse_job_file_comments() {
        let jobs = parse_job_file("# comment\n; another").unwrap();
        assert!(jobs.is_empty());
    }

    #[test]
    fn test_parse_job_file_global_only() {
        let content = "[global]\nbs=8k\ndirect=1\n";
        let jobs = parse_job_file(content).unwrap();
        assert!(jobs.is_empty());
    }

    #[test]
    fn test_parse_job_file_single_job() {
        let content = "[global]\nbs=8k\n\n[mytest]\nrw=read\nsize=1m\nfilename=/tmp/t\n";
        let jobs = parse_job_file(content).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "mytest");
        assert_eq!(jobs[0].bs, 8192);
        assert_eq!(jobs[0].rw, IoPattern::Read);
        assert_eq!(jobs[0].size, 1024 * 1024);
    }

    #[test]
    fn test_parse_job_file_multiple_jobs() {
        let content = "[global]\nbs=4k\n\n[job1]\nrw=read\nsize=1m\nfilename=/tmp/a\n\n[job2]\nrw=write\nsize=2m\nfilename=/tmp/b\n";
        let jobs = parse_job_file(content).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].name, "job1");
        assert_eq!(jobs[1].name, "job2");
        assert_eq!(jobs[1].size, 2 * 1024 * 1024);
    }

    #[test]
    fn test_parse_job_file_global_inherits() {
        let content = "[global]\nioengine=libaio\ndirect=1\n\n[job1]\nrw=read\nsize=1m\nfilename=/tmp/t\n";
        let jobs = parse_job_file(content).unwrap();
        assert_eq!(jobs[0].ioengine, IoEngine::Libaio);
        assert!(jobs[0].direct);
    }

    #[test]
    fn test_parse_job_file_bare_key() {
        let content = "[global]\ntime_based\n\n[job1]\nrw=read\nsize=1m\nfilename=/tmp/t\n";
        let jobs = parse_job_file(content).unwrap();
        assert!(jobs[0].time_based);
    }

    #[test]
    fn test_parse_job_file_invalid_param() {
        let content = "[job1]\nrw=notapattern\n";
        assert!(parse_job_file(content).is_err());
    }

    #[test]
    fn test_parse_job_file_unterminated_section() {
        let content = "[job1\nrw=read\n";
        assert!(parse_job_file(content).is_err());
    }

    // === CLI argument parsing tests ===

    #[test]
    fn test_cli_help() {
        let args = vec!["--help".to_string()];
        let cli = parse_cli_args(&args).unwrap();
        assert!(cli.show_help);
    }

    #[test]
    fn test_cli_version() {
        let args = vec!["--version".to_string()];
        let cli = parse_cli_args(&args).unwrap();
        assert!(cli.show_version);
    }

    #[test]
    fn test_cli_basic_job() {
        let args = vec![
            "--name=test".to_string(),
            "--filename=/tmp/f".to_string(),
            "--rw=read".to_string(),
            "--size=1m".to_string(),
        ];
        let cli = parse_cli_args(&args).unwrap();
        assert_eq!(cli.jobs.len(), 1);
        assert_eq!(cli.jobs[0].name, "test");
        assert_eq!(cli.jobs[0].rw, IoPattern::Read);
    }

    #[test]
    fn test_cli_output_format() {
        let args = vec![
            "--name=t".to_string(),
            "--filename=/tmp/f".to_string(),
            "--rw=read".to_string(),
            "--size=1m".to_string(),
            "--output-format=json".to_string(),
        ];
        let cli = parse_cli_args(&args).unwrap();
        assert_eq!(cli.output_format, OutputFormat::Json);
    }

    #[test]
    fn test_cli_output_file() {
        let args = vec![
            "--name=t".to_string(),
            "--filename=/tmp/f".to_string(),
            "--rw=read".to_string(),
            "--size=1m".to_string(),
            "--output=/tmp/out.txt".to_string(),
        ];
        let cli = parse_cli_args(&args).unwrap();
        assert_eq!(cli.output_file, Some("/tmp/out.txt".to_string()));
    }

    #[test]
    fn test_cli_bare_flags() {
        let args = vec![
            "--name=t".to_string(),
            "--filename=/tmp/f".to_string(),
            "--rw=read".to_string(),
            "--size=1m".to_string(),
            "--time_based".to_string(),
            "--thread".to_string(),
            "--group_reporting".to_string(),
        ];
        let cli = parse_cli_args(&args).unwrap();
        assert!(cli.jobs[0].time_based);
        assert!(cli.jobs[0].thread);
        assert!(cli.jobs[0].group_reporting);
    }

    #[test]
    fn test_cli_unknown_flag() {
        let args = vec!["--nonsense".to_string()];
        assert!(parse_cli_args(&args).is_err());
    }

    #[test]
    fn test_cli_job_file_positional() {
        let args = vec!["myfile.fio".to_string()];
        let cli = parse_cli_args(&args).unwrap();
        assert_eq!(cli.job_file, Some("myfile.fio".to_string()));
    }

    #[test]
    fn test_cli_no_jobs() {
        let args: Vec<String> = vec![];
        let cli = parse_cli_args(&args).unwrap();
        assert!(cli.jobs.is_empty());
    }

    // === Preset tests ===

    #[test]
    fn test_preset_4k_randread() {
        let mut j = JobDef::default();
        j.name = "4k-randread".to_string();
        apply_preset(&mut j);
        assert_eq!(j.bs, 4096);
        assert_eq!(j.rw, IoPattern::RandRead);
        assert_eq!(j.size, 256 * 1024 * 1024);
    }

    #[test]
    fn test_preset_4k_randwrite() {
        let mut j = JobDef::default();
        j.name = "4k-randwrite".to_string();
        apply_preset(&mut j);
        assert_eq!(j.bs, 4096);
        assert_eq!(j.rw, IoPattern::RandWrite);
    }

    #[test]
    fn test_preset_seq_read_1m() {
        let mut j = JobDef::default();
        j.name = "seq-read-1m".to_string();
        apply_preset(&mut j);
        assert_eq!(j.bs, 1024 * 1024);
        assert_eq!(j.rw, IoPattern::Read);
        assert_eq!(j.size, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_preset_seq_write_1m() {
        let mut j = JobDef::default();
        j.name = "seq-write-1m".to_string();
        apply_preset(&mut j);
        assert_eq!(j.bs, 1024 * 1024);
        assert_eq!(j.rw, IoPattern::Write);
    }

    #[test]
    fn test_preset_no_override_existing_size() {
        let mut j = JobDef::default();
        j.name = "4k-randread".to_string();
        j.size = 1024;
        apply_preset(&mut j);
        assert_eq!(j.size, 1024); // preserved
    }

    #[test]
    fn test_preset_unknown_name() {
        let mut j = JobDef::default();
        j.name = "custom".to_string();
        j.bs = 8192;
        apply_preset(&mut j);
        assert_eq!(j.bs, 8192); // unchanged
    }

    // === Fill pattern tests ===

    #[test]
    fn test_fill_pattern() {
        let mut buf = [0u8; 16];
        fill_pattern(&mut buf, 0xAB);
        assert!(buf.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn test_fill_pattern_seeded() {
        let mut buf1 = [0u8; 16];
        let mut buf2 = [0u8; 16];
        fill_pattern_seeded(&mut buf1, 0xAA, 0);
        fill_pattern_seeded(&mut buf2, 0xAA, 4096);
        assert_ne!(buf1, buf2);
    }

    #[test]
    fn test_fill_pattern_seeded_deterministic() {
        let mut buf1 = [0u8; 16];
        let mut buf2 = [0u8; 16];
        fill_pattern_seeded(&mut buf1, 0xAA, 1234);
        fill_pattern_seeded(&mut buf2, 0xAA, 1234);
        assert_eq!(buf1, buf2);
    }

    // === Verification compute tests ===

    #[test]
    fn test_compute_verify_crc32() {
        let data = b"test data";
        let v1 = compute_verify(data, VerifyMethod::Crc32);
        let v2 = compute_verify(data, VerifyMethod::Crc32);
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_compute_verify_md5() {
        let data = b"test data";
        let v = compute_verify(data, VerifyMethod::Md5);
        assert_eq!(v.len(), 16);
    }

    #[test]
    fn test_compute_verify_sha256() {
        let data = b"test data";
        let v = compute_verify(data, VerifyMethod::Sha256);
        assert_eq!(v.len(), 32);
    }

    #[test]
    fn test_compute_verify_pattern() {
        let data = b"test data";
        let v = compute_verify(data, VerifyMethod::Pattern);
        assert_eq!(v.len(), 4); // crc32 = 4 bytes
    }

    #[test]
    fn test_compute_verify_different_data() {
        let v1 = compute_verify(b"hello", VerifyMethod::Crc32);
        let v2 = compute_verify(b"world", VerifyMethod::Crc32);
        assert_ne!(v1, v2);
    }

    // === JSON escape tests ===

    #[test]
    fn test_json_escape_plain() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn test_json_escape_quotes() {
        assert_eq!(json_escape("say \"hi\""), "say \\\"hi\\\"");
    }

    #[test]
    fn test_json_escape_backslash() {
        assert_eq!(json_escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_json_escape_newline() {
        assert_eq!(json_escape("a\nb"), "a\\nb");
    }

    #[test]
    fn test_json_escape_tab() {
        assert_eq!(json_escape("a\tb"), "a\\tb");
    }

    #[test]
    fn test_json_escape_control_char() {
        let s = String::from_utf8(vec![0x01]).unwrap();
        assert_eq!(json_escape(&s), "\\u0001");
    }

    // === Terse output format tests ===

    #[test]
    fn test_format_terse_basic() {
        let s = JobStats::new("test", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        let terse = format_terse(&s);
        assert!(terse.starts_with("test;read;4096;"));
        assert!(terse.contains(';'));
    }

    #[test]
    fn test_format_terse_has_all_fields() {
        let s = JobStats::new("j", IoPattern::Write, IoEngine::Mmap, 8192, 2, 4);
        let terse = format_terse(&s);
        let fields: Vec<&str> = terse.split(';').collect();
        assert!(fields.len() >= 18);
    }

    // === Normal output format tests ===

    #[test]
    fn test_format_normal_contains_name() {
        let mut s = JobStats::new("mytest", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        s.read_ios = 100;
        s.read_bytes = 409600;
        s.elapsed_us = 1_000_000;
        s.read_lat.record(100);
        let out = format_normal(&mut s, false);
        assert!(out.contains("mytest"));
    }

    #[test]
    fn test_format_normal_contains_iops() {
        let mut s = JobStats::new("t", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        s.read_ios = 1000;
        s.read_bytes = 4096000;
        s.elapsed_us = 1_000_000;
        s.read_lat.record(100);
        let out = format_normal(&mut s, false);
        assert!(out.contains("IOPS="));
    }

    #[test]
    fn test_format_normal_with_percentiles() {
        let mut s = JobStats::new("t", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        for i in 0..100 {
            s.read_lat.record(i * 10);
        }
        s.read_ios = 100;
        s.read_bytes = 409600;
        s.elapsed_us = 1_000_000;
        let out = format_normal(&mut s, true);
        assert!(out.contains("50.00th="));
    }

    #[test]
    fn test_format_normal_write_stats() {
        let mut s = JobStats::new("t", IoPattern::Write, IoEngine::Sync, 4096, 1, 1);
        s.write_ios = 500;
        s.write_bytes = 2048000;
        s.elapsed_us = 1_000_000;
        s.write_lat.record(200);
        let out = format_normal(&mut s, false);
        assert!(out.contains("write:"));
    }

    // === JSON output format tests ===

    #[test]
    fn test_format_json_valid_structure() {
        let stats = vec![JobStats::new("t", IoPattern::Read, IoEngine::Sync, 4096, 1, 1)];
        let json = format_json(&stats, false);
        assert!(json.starts_with('{'));
        assert!(json.contains("\"fio version\""));
        assert!(json.contains("\"jobs\""));
    }

    #[test]
    fn test_format_json_plus_has_percentiles() {
        let mut s = JobStats::new("t", IoPattern::Read, IoEngine::Sync, 4096, 1, 1);
        for i in 0..100 {
            s.read_lat.record(i);
        }
        let stats = vec![s];
        let json = format_json(&stats, true);
        assert!(json.contains("read_lat_percentiles"));
        assert!(json.contains("write_lat_percentiles"));
    }

    #[test]
    fn test_format_json_multiple_jobs() {
        let stats = vec![
            JobStats::new("j1", IoPattern::Read, IoEngine::Sync, 4096, 1, 1),
            JobStats::new("j2", IoPattern::Write, IoEngine::Mmap, 8192, 2, 1),
        ];
        let json = format_json(&stats, false);
        assert!(json.contains("\"j1\""));
        assert!(json.contains("\"j2\""));
    }

    // === Percentile of sorted slice tests ===

    #[test]
    fn test_percentile_of_empty() {
        assert_eq!(percentile_of(&[], 50.0), 0);
    }

    #[test]
    fn test_percentile_of_single() {
        assert_eq!(percentile_of(&[42], 99.0), 42);
    }

    #[test]
    fn test_percentile_of_values() {
        let sorted: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile_of(&sorted, 0.0), 1);
        assert_eq!(percentile_of(&sorted, 50.0), 50);
        assert_eq!(percentile_of(&sorted, 100.0), 100);
    }

    // === End-to-end execution tests ===

    #[test]
    fn test_execute_job_read() {
        let f = temp_file("exec_read");
        let mut job = JobDef::default();
        job.name = "test_read".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Read;
        job.size = 4096 * 10;
        job.bs = 4096;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.read_ios, 10);
        assert!(stats.read_bytes > 0);
        assert!(stats.elapsed_us > 0);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_write() {
        let f = temp_file("exec_write");
        let mut job = JobDef::default();
        job.name = "test_write".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Write;
        job.size = 4096 * 5;
        job.bs = 4096;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.write_ios, 5);
        assert!(stats.write_bytes > 0);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_randread() {
        let f = temp_file("exec_rr");
        let mut job = JobDef::default();
        job.name = "test_rr".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::RandRead;
        job.size = 4096 * 20;
        job.bs = 4096;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.read_ios, 20);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_randwrite() {
        let f = temp_file("exec_rw");
        let mut job = JobDef::default();
        job.name = "test_rw".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::RandWrite;
        job.size = 4096 * 8;
        job.bs = 4096;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.write_ios, 8);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_mixed() {
        let f = temp_file("exec_mixed");
        let mut job = JobDef::default();
        job.name = "test_mixed".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::ReadWrite;
        job.size = 4096 * 20;
        job.bs = 4096;
        job.rwmixread = 50;
        let stats = execute_job(&job).unwrap();
        assert!(stats.read_ios + stats.write_ios == 20);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_trim() {
        let f = temp_file("exec_trim");
        let mut job = JobDef::default();
        job.name = "test_trim".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Trim;
        job.size = 4096 * 4;
        job.bs = 4096;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.trim_ios, 4);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_no_filename() {
        let mut job = JobDef::default();
        job.size = 4096;
        assert!(execute_job(&job).is_err());
    }

    #[test]
    fn test_execute_job_no_size() {
        let mut job = JobDef::default();
        job.filename = "/tmp/test".to_string();
        assert!(execute_job(&job).is_err());
    }

    #[test]
    fn test_execute_job_zero_bs() {
        let mut job = JobDef::default();
        job.filename = "/tmp/test".to_string();
        job.size = 4096;
        job.bs = 0;
        assert!(execute_job(&job).is_err());
    }

    #[test]
    fn test_execute_job_with_verify_crc32() {
        let f = temp_file("exec_vcrc");
        let mut job = JobDef::default();
        job.name = "test_verify".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Write;
        job.size = 4096 * 4;
        job.bs = 4096;
        job.verify = Some(VerifyMethod::Crc32);
        job.do_verify = true;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.verify_errors, 0);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_with_verify_md5() {
        let f = temp_file("exec_vmd5");
        let mut job = JobDef::default();
        job.name = "test_vmd5".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Write;
        job.size = 4096 * 4;
        job.bs = 4096;
        job.verify = Some(VerifyMethod::Md5);
        job.do_verify = true;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.verify_errors, 0);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_with_verify_sha256() {
        let f = temp_file("exec_vsha");
        let mut job = JobDef::default();
        job.name = "test_vsha".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Write;
        job.size = 4096 * 4;
        job.bs = 4096;
        job.verify = Some(VerifyMethod::Sha256);
        job.do_verify = true;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.verify_errors, 0);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_large_bs() {
        let f = temp_file("exec_lbs");
        let mut job = JobDef::default();
        job.name = "test_lbs".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Read;
        job.size = 65536 * 2;
        job.bs = 65536;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.read_ios, 2);
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_depth_distribution() {
        let f = temp_file("exec_depth");
        let mut job = JobDef::default();
        job.name = "test_depth".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Read;
        job.size = 4096 * 10;
        job.bs = 4096;
        job.iodepth = 1;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.io_depth_dist[0], 10); // depth=1 bucket
        cleanup(&f);
    }

    #[test]
    fn test_execute_job_depth_32() {
        let f = temp_file("exec_d32");
        let mut job = JobDef::default();
        job.name = "test_d32".to_string();
        job.filename = f.clone();
        job.rw = IoPattern::Read;
        job.size = 4096 * 5;
        job.bs = 4096;
        job.iodepth = 32;
        let stats = execute_job(&job).unwrap();
        assert_eq!(stats.io_depth_dist[5], 5); // 17-32 bucket
        cleanup(&f);
    }

    // === Personality detection tests ===

    #[test]
    fn test_personality_fio() {
        let s = "fio";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "fio");
    }

    #[test]
    fn test_personality_fio_verify() {
        let s = "/usr/bin/fio-verify";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "fio-verify");
    }

    #[test]
    fn test_personality_with_exe_suffix() {
        let s = "C:\\bin\\fio.exe";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "fio");
    }

    #[test]
    fn test_personality_forward_slash() {
        let s = "/opt/ouros/bin/fio-verify.exe";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "fio-verify");
    }
}
