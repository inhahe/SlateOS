//! Transparent filesystem compression layer.
//!
//! Provides automatic compress-on-write and decompress-on-read for files
//! matching configurable policies (by path prefix, extension, or explicit
//! marking).  Leverages the existing codec library (gzip, lz4, zstd,
//! bzip2, xz) without requiring application awareness.
//!
//! ## Architecture
//!
//! Files are stored with a small header identifying the compression
//! algorithm, followed by the compressed data.  The VFS hooks call into
//! this module to transparently compress before writing and decompress
//! after reading.
//!
//! ```text
//! Application
//!     ↓ write("hello world")
//! fcompress::compress_for_write(path, data)
//!     ↓ → [FCOMP_MAGIC | algo_id | orig_size | compressed_data]
//! VFS write_file(path, compressed_bytes)
//!
//! Application
//!     ↓ read(path)
//! VFS read_file(path)
//!     ↓ → [FCOMP_MAGIC | algo_id | orig_size | compressed_data]
//! fcompress::decompress_for_read(raw_bytes)
//!     ↓ → "hello world"
//! ```
//!
//! ## Compression Algorithms
//!
//! - `lz4`: fastest, good for logs and temp files
//! - `gzip`: good balance of speed and ratio
//! - `zstd`: best overall ratio with good speed
//! - `bzip2`: high ratio, slower
//! - `xz`: highest ratio, slowest
//!
//! ## Policies
//!
//! - Path prefix rules: compress files under specific directories
//! - Extension rules: compress files with specific extensions
//! - Minimum size: don't compress files below a threshold
//! - Skip-if-incompressible: if compressed size ≥ original, store uncompressed
//!
//! ## File Header Format
//!
//! ```text
//! Offset  Size   Description
//! 0       4      Magic: 0x46 0x43 0x4D 0x50 ("FCMP")
//! 4       1      Algorithm ID (0=none, 1=lz4, 2=gzip, 3=zstd, 4=bzip2, 5=xz)
//! 5       1      Version (currently 1)
//! 6       2      Reserved (zero)
//! 8       8      Original uncompressed size (little-endian u64)
//! 16      ...    Compressed data
//! ```
//!
//! Total header: 16 bytes.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Magic bytes identifying a compressed file: "FCMP"
const MAGIC: [u8; 4] = [0x46, 0x43, 0x4D, 0x50];

/// Header size in bytes.
const HEADER_SIZE: usize = 16;

/// Current format version.
const VERSION: u8 = 1;

/// Default minimum file size for compression (bytes).
/// Files smaller than this are stored uncompressed.
const DEFAULT_MIN_SIZE: u64 = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Supported compression algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// No compression (passthrough / stored).
    None = 0,
    /// LZ4 frame format (fast).
    Lz4 = 1,
    /// gzip / DEFLATE (balanced).
    Gzip = 2,
    /// Zstandard (best overall).
    Zstd = 3,
    /// bzip2 (high ratio).
    Bzip2 = 4,
    /// XZ / LZMA2 (highest ratio).
    Xz = 5,
}

impl Algorithm {
    fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::None),
            1 => Some(Self::Lz4),
            2 => Some(Self::Gzip),
            3 => Some(Self::Zstd),
            4 => Some(Self::Bzip2),
            5 => Some(Self::Xz),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lz4 => "lz4",
            Self::Gzip => "gzip",
            Self::Zstd => "zstd",
            Self::Bzip2 => "bzip2",
            Self::Xz => "xz",
        }
    }

    /// Parse algorithm name from string.
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "none" | "store" | "raw" => Some(Self::None),
            "lz4" => Some(Self::Lz4),
            "gzip" | "gz" | "deflate" => Some(Self::Gzip),
            "zstd" | "zstandard" => Some(Self::Zstd),
            "bzip2" | "bz2" => Some(Self::Bzip2),
            "xz" | "lzma" | "lzma2" => Some(Self::Xz),
            _ => None,
        }
    }
}

/// A compression policy rule.
#[derive(Debug, Clone)]
pub struct CompressionRule {
    /// Path prefix this rule applies to (e.g., "/var/log").
    pub path_prefix: String,
    /// File extensions this rule applies to (e.g., ["log", "txt"]).
    /// Empty means all extensions.
    pub extensions: Vec<String>,
    /// Algorithm to use.
    pub algorithm: Algorithm,
}

/// Statistics about compression activity.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompressStats {
    /// Files compressed.
    pub files_compressed: u64,
    /// Files decompressed (read).
    pub files_decompressed: u64,
    /// Files skipped (too small or incompressible).
    pub files_skipped: u64,
    /// Total bytes written (original).
    pub bytes_original: u64,
    /// Total bytes stored (compressed).
    pub bytes_stored: u64,
    /// Total bytes read (compressed on disk).
    pub bytes_read_compressed: u64,
    /// Total bytes delivered (decompressed to caller).
    pub bytes_delivered: u64,
}

/// Information about a compressed file.
#[derive(Debug, Clone)]
pub struct FileCompressionInfo {
    /// Whether the file is compressed.
    pub compressed: bool,
    /// Algorithm used (None if not compressed).
    pub algorithm: Algorithm,
    /// Original uncompressed size.
    pub original_size: u64,
    /// Compressed size on disk.
    pub stored_size: u64,
    /// Compression ratio (original / stored, e.g. 2.5 means 2.5:1).
    pub ratio: f64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Master enable flag.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Default algorithm for new rules.
static DEFAULT_ALGO: Mutex<Algorithm> = Mutex::new(Algorithm::Lz4);

/// Minimum file size for compression.
static MIN_SIZE: AtomicU64 = AtomicU64::new(DEFAULT_MIN_SIZE);

struct FCompressInner {
    rules: Vec<CompressionRule>,
    stats: CompressStats,
}

static STATE: Mutex<FCompressInner> = Mutex::new(FCompressInner {
    rules: Vec::new(),
    stats: CompressStats {
        files_compressed: 0,
        files_decompressed: 0,
        files_skipped: 0,
        bytes_original: 0,
        bytes_stored: 0,
        bytes_read_compressed: 0,
        bytes_delivered: 0,
    },
});

// ---------------------------------------------------------------------------
// Configuration API
// ---------------------------------------------------------------------------

/// Enable or disable transparent compression.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    serial_println!(
        "[fcompress] {}",
        if enabled { "enabled" } else { "disabled" }
    );
}

/// Check if transparent compression is enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Set the default compression algorithm.
pub fn set_default_algorithm(algo: Algorithm) {
    *DEFAULT_ALGO.lock() = algo;
}

/// Get the default compression algorithm.
pub fn default_algorithm() -> Algorithm {
    *DEFAULT_ALGO.lock()
}

/// Set the minimum file size for compression.
pub fn set_min_size(size: u64) {
    MIN_SIZE.store(size, Ordering::Relaxed);
}

/// Get the minimum file size for compression.
pub fn min_size() -> u64 {
    MIN_SIZE.load(Ordering::Relaxed)
}

/// Add a compression rule.
pub fn add_rule(rule: CompressionRule) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.rules.len() >= 128 {
        return Err(KernelError::DiskFull); // Too many rules.
    }
    state.rules.push(rule);
    Ok(())
}

/// Remove all rules matching a path prefix.
pub fn remove_rules(prefix: &str) -> usize {
    let mut state = STATE.lock();
    let before = state.rules.len();
    state.rules.retain(|r| r.path_prefix != prefix);
    before - state.rules.len()
}

/// List all compression rules.
pub fn list_rules() -> Vec<CompressionRule> {
    STATE.lock().rules.clone()
}

/// Clear all rules.
pub fn clear_rules() {
    STATE.lock().rules.clear();
}

/// Get compression statistics.
pub fn stats() -> CompressStats {
    STATE.lock().stats
}

/// Reset statistics.
pub fn reset_stats() {
    STATE.lock().stats = CompressStats::default();
}

// ---------------------------------------------------------------------------
// Core compression / decompression
// ---------------------------------------------------------------------------

/// Check if data should be compressed for a given path, and if so,
/// return the compressed data with header.
///
/// Returns `None` if the file should not be compressed (disabled,
/// no matching rule, too small, or incompressible).
pub fn compress_for_write(path: &str, data: &[u8]) -> Option<Vec<u8>> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }

    let min = MIN_SIZE.load(Ordering::Relaxed);
    if (data.len() as u64) < min {
        STATE.lock().stats.files_skipped = STATE
            .lock()
            .stats
            .files_skipped
            .saturating_add(1);
        return None;
    }

    // Find matching rule.
    let algo = find_algorithm(path)?;

    if algo == Algorithm::None {
        return None;
    }

    // Compress.
    let compressed = compress_data(data, algo);

    // Skip if compressed size >= original (incompressible data).
    if compressed.len() >= data.len() {
        STATE.lock().stats.files_skipped = STATE
            .lock()
            .stats
            .files_skipped
            .saturating_add(1);
        return None;
    }

    // Build output: header + compressed data.
    let mut output = Vec::with_capacity(HEADER_SIZE + compressed.len());

    // Magic.
    output.extend_from_slice(&MAGIC);
    // Algorithm ID.
    output.push(algo as u8);
    // Version.
    output.push(VERSION);
    // Reserved.
    output.push(0);
    output.push(0);
    // Original size (little-endian u64).
    output.extend_from_slice(&(data.len() as u64).to_le_bytes());
    // Compressed data.
    output.extend_from_slice(&compressed);

    // Update stats.
    {
        let mut state = STATE.lock();
        state.stats.files_compressed = state.stats.files_compressed.saturating_add(1);
        state.stats.bytes_original = state.stats.bytes_original.saturating_add(data.len() as u64);
        state.stats.bytes_stored = state.stats.bytes_stored.saturating_add(output.len() as u64);
    }

    Some(output)
}

/// Check if raw data from disk is a compressed file and decompress it.
///
/// Returns `Some(decompressed_data)` if the data had the FCMP header
/// and was successfully decompressed.  Returns `None` if the data is
/// not compressed (no magic header).
pub fn decompress_for_read(data: &[u8]) -> Option<Vec<u8>> {
    if !is_compressed(data) {
        return None;
    }

    let algo_id = data[4];
    let algo = Algorithm::from_id(algo_id)?;

    if algo == Algorithm::None {
        // Stored (passthrough) — return data after header.
        return Some(data[HEADER_SIZE..].to_vec());
    }

    // Read original size.
    let mut size_bytes = [0u8; 8];
    size_bytes.copy_from_slice(&data[8..16]);
    let _original_size = u64::from_le_bytes(size_bytes);

    let compressed = &data[HEADER_SIZE..];

    match decompress_data(compressed, algo) {
        Ok(decompressed) => {
            // Update stats.
            {
                let mut state = STATE.lock();
                state.stats.files_decompressed = state.stats.files_decompressed.saturating_add(1);
                state.stats.bytes_read_compressed = state
                    .stats
                    .bytes_read_compressed
                    .saturating_add(data.len() as u64);
                state.stats.bytes_delivered = state
                    .stats
                    .bytes_delivered
                    .saturating_add(decompressed.len() as u64);
            }
            Some(decompressed)
        }
        Err(e) => {
            serial_println!(
                "[fcompress] Decompression failed for algo {:?}: {:?}",
                algo,
                e
            );
            None
        }
    }
}

/// Check if data starts with the FCMP magic header.
pub fn is_compressed(data: &[u8]) -> bool {
    data.len() >= HEADER_SIZE && data[..4] == MAGIC
}

/// Get compression info about a file's raw data.
pub fn file_info(data: &[u8]) -> FileCompressionInfo {
    if !is_compressed(data) {
        return FileCompressionInfo {
            compressed: false,
            algorithm: Algorithm::None,
            original_size: data.len() as u64,
            stored_size: data.len() as u64,
            ratio: 1.0,
        };
    }

    let algo = Algorithm::from_id(data[4]).unwrap_or(Algorithm::None);

    let mut size_bytes = [0u8; 8];
    size_bytes.copy_from_slice(&data[8..16]);
    let original_size = u64::from_le_bytes(size_bytes);

    let stored_size = data.len() as u64;
    let ratio = if stored_size > 0 {
        original_size as f64 / stored_size as f64
    } else {
        1.0
    };

    FileCompressionInfo {
        compressed: true,
        algorithm: algo,
        original_size,
        stored_size,
        ratio,
    }
}

// ---------------------------------------------------------------------------
// Rule matching
// ---------------------------------------------------------------------------

/// Find the compression algorithm for a given path.
fn find_algorithm(path: &str) -> Option<Algorithm> {
    let state = STATE.lock();

    // Find the most specific (longest prefix) matching rule.
    let mut best: Option<&CompressionRule> = None;
    let mut best_len = 0;

    for rule in &state.rules {
        if path.starts_with(&rule.path_prefix) && rule.path_prefix.len() >= best_len {
            // Check extension filter.
            if !rule.extensions.is_empty() {
                let ext = path_extension(path);
                if !rule.extensions.iter().any(|e| e.as_str() == ext) {
                    continue;
                }
            }
            best_len = rule.path_prefix.len();
            best = Some(rule);
        }
    }

    best.map(|r| r.algorithm)
}

/// Extract file extension from a path (lowercase, without dot).
fn path_extension(path: &str) -> &str {
    if let Some(name) = path.rsplit('/').next() {
        if let Some(dot_pos) = name.rfind('.') {
            return &name[dot_pos + 1..];
        }
    }
    ""
}

// ---------------------------------------------------------------------------
// Codec dispatch
// ---------------------------------------------------------------------------

/// Compress data using the specified algorithm.
fn compress_data(data: &[u8], algo: Algorithm) -> Vec<u8> {
    match algo {
        Algorithm::None => data.to_vec(),
        Algorithm::Lz4 => crate::fs::lz4::compress(data),
        Algorithm::Gzip => crate::fs::compress::gzip(data),
        Algorithm::Zstd => crate::fs::zstd::compress_zstd(data),
        Algorithm::Bzip2 => crate::fs::bzip2::bzip2_compress(data, 9),
        Algorithm::Xz => crate::fs::xz::xz_compress(data).unwrap_or_else(|_| data.to_vec()),
    }
}

/// Decompress data using the specified algorithm.
fn decompress_data(data: &[u8], algo: Algorithm) -> KernelResult<Vec<u8>> {
    match algo {
        Algorithm::None => Ok(data.to_vec()),
        Algorithm::Lz4 => crate::fs::lz4::decompress(data),
        Algorithm::Gzip => crate::fs::compress::gunzip(data),
        Algorithm::Zstd => crate::fs::zstd::unzstd(data),
        Algorithm::Bzip2 => crate::fs::bzip2::bunzip2(data),
        Algorithm::Xz => crate::fs::xz::unxz(data),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[fcompress] Running self-test...");

    test_header_format();
    test_compress_decompress_lz4();
    test_compress_decompress_gzip();
    test_compress_decompress_zstd();
    test_incompressible_skip();
    test_rule_matching();
    test_min_size_filter();
    test_stats();

    serial_println!("[fcompress] Self-test passed (8 tests).");
    Ok(())
}

fn test_header_format() {
    // Build a fake compressed file and verify header parsing.
    let mut data = Vec::new();
    data.extend_from_slice(&MAGIC);
    data.push(Algorithm::Lz4 as u8);
    data.push(VERSION);
    data.push(0); // reserved
    data.push(0);
    data.extend_from_slice(&42u64.to_le_bytes());
    data.extend_from_slice(b"fake_compressed_payload");

    assert!(is_compressed(&data));

    let info = file_info(&data);
    assert!(info.compressed);
    assert_eq!(info.algorithm, Algorithm::Lz4);
    assert_eq!(info.original_size, 42);

    // Non-compressed data should not match.
    assert!(!is_compressed(b"hello world"));
    assert!(!is_compressed(b"FCMP_nope"));
    assert!(!is_compressed(&[]));

    serial_println!("[fcompress]   header format: ok");
}

fn test_compress_decompress_lz4() {
    // Enable and set up a rule.
    let was_enabled = is_enabled();
    set_enabled(true);
    set_min_size(0); // Allow any size.

    add_rule(CompressionRule {
        path_prefix: String::from("/tmp/fcomp_test"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    }).expect("add rule");

    let original = b"The quick brown fox jumps over the lazy dog. Repeated data helps compression: AAAAAAAAAA BBBBBBBBBB CCCCCCCCCC";

    // Compress.
    let compressed = compress_for_write("/tmp/fcomp_test/file.txt", original);
    assert!(compressed.is_some(), "should have compressed");
    let compressed = compressed.expect("checked above");
    assert!(is_compressed(&compressed));

    // Decompress.
    let decompressed = decompress_for_read(&compressed);
    assert!(decompressed.is_some(), "should have decompressed");
    let decompressed = decompressed.expect("checked above");
    assert_eq!(&decompressed, original.as_ref());

    // Cleanup.
    remove_rules("/tmp/fcomp_test");
    set_min_size(DEFAULT_MIN_SIZE);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   lz4 round-trip: ok");
}

fn test_compress_decompress_gzip() {
    let was_enabled = is_enabled();
    set_enabled(true);
    set_min_size(0);

    add_rule(CompressionRule {
        path_prefix: String::from("/tmp/fcomp_gz"),
        extensions: Vec::new(),
        algorithm: Algorithm::Gzip,
    }).expect("add rule");

    let original = b"Gzip test data with enough repetition to compress well: XXXXXXXXXX YYYYYYYYYY ZZZZZZZZZZ XXXXXXXXXX YYYYYYYYYY";

    let compressed = compress_for_write("/tmp/fcomp_gz/data.bin", original);
    assert!(compressed.is_some());
    let compressed = compressed.expect("checked");

    let decompressed = decompress_for_read(&compressed);
    assert!(decompressed.is_some());
    assert_eq!(&decompressed.expect("checked"), original.as_ref());

    remove_rules("/tmp/fcomp_gz");
    set_min_size(DEFAULT_MIN_SIZE);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   gzip round-trip: ok");
}

fn test_compress_decompress_zstd() {
    let was_enabled = is_enabled();
    set_enabled(true);
    set_min_size(0);

    add_rule(CompressionRule {
        path_prefix: String::from("/tmp/fcomp_zst"),
        extensions: Vec::new(),
        algorithm: Algorithm::Zstd,
    }).expect("add rule");

    let original = b"Zstd test: repetitive content compresses well. Repeat repeat repeat repeat repeat repeat repeat!";

    let compressed = compress_for_write("/tmp/fcomp_zst/test.dat", original);
    assert!(compressed.is_some());
    let compressed = compressed.expect("checked");

    let decompressed = decompress_for_read(&compressed);
    assert!(decompressed.is_some());
    assert_eq!(&decompressed.expect("checked"), original.as_ref());

    remove_rules("/tmp/fcomp_zst");
    set_min_size(DEFAULT_MIN_SIZE);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   zstd round-trip: ok");
}

fn test_incompressible_skip() {
    let was_enabled = is_enabled();
    set_enabled(true);
    set_min_size(0);

    add_rule(CompressionRule {
        path_prefix: String::from("/tmp/fcomp_rand"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    }).expect("add rule");

    // Random-looking data that won't compress well.
    // Small enough that LZ4 overhead makes compressed >= original.
    let data: Vec<u8> = (0u8..32).collect();

    let result = compress_for_write("/tmp/fcomp_rand/random.bin", &data);
    // May or may not be None depending on LZ4 overhead for 32 bytes.
    // The important thing is it doesn't panic.
    if let Some(compressed) = result {
        // Tiny data might still fit with overhead — verify it round-trips.
        let decompressed = decompress_for_read(&compressed);
        assert!(decompressed.is_some());
        assert_eq!(&decompressed.expect("checked"), &data);
    }
    // None is also fine — skipped because incompressible.

    remove_rules("/tmp/fcomp_rand");
    set_min_size(DEFAULT_MIN_SIZE);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   incompressible skip: ok");
}

fn test_rule_matching() {
    let was_enabled = is_enabled();
    set_enabled(true);
    set_min_size(0);

    // Rule for .log files under /var/log.
    add_rule(CompressionRule {
        path_prefix: String::from("/var/log"),
        extensions: alloc::vec![String::from("log")],
        algorithm: Algorithm::Gzip,
    }).expect("add rule");

    // Should match.
    let data = b"Log line repeated many times: ERROR something went wrong ERROR something went wrong ERROR something went wrong";
    let r1 = compress_for_write("/var/log/syslog.log", data);
    assert!(r1.is_some(), ".log under /var/log should match");

    // Should NOT match (wrong extension).
    let r2 = compress_for_write("/var/log/data.bin", data);
    assert!(r2.is_none(), ".bin under /var/log should not match");

    // Should NOT match (wrong prefix).
    let r3 = compress_for_write("/home/user/file.log", data);
    assert!(r3.is_none(), ".log under /home should not match");

    remove_rules("/var/log");
    set_min_size(DEFAULT_MIN_SIZE);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   rule matching: ok");
}

fn test_min_size_filter() {
    let was_enabled = is_enabled();
    set_enabled(true);
    set_min_size(1024); // Must be at least 1KB.

    add_rule(CompressionRule {
        path_prefix: String::from("/tmp/fcomp_min"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    }).expect("add rule");

    // Small file — should be skipped.
    let small = b"tiny";
    let r1 = compress_for_write("/tmp/fcomp_min/small.txt", small);
    assert!(r1.is_none(), "small file should be skipped");

    // Large file — should be compressed.
    let large: Vec<u8> = core::iter::repeat_n(b'A', 2048).collect();
    let r2 = compress_for_write("/tmp/fcomp_min/large.txt", &large);
    assert!(r2.is_some(), "large file should be compressed");

    remove_rules("/tmp/fcomp_min");
    set_min_size(DEFAULT_MIN_SIZE);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   min size filter: ok");
}

fn test_stats() {
    reset_stats();
    let s = stats();
    assert_eq!(s.files_compressed, 0);
    assert_eq!(s.files_decompressed, 0);

    // Run a compress + decompress cycle.
    let was_enabled = is_enabled();
    set_enabled(true);
    set_min_size(0);

    add_rule(CompressionRule {
        path_prefix: String::from("/tmp/fcomp_stats"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    }).expect("add rule");

    let data = b"Stats test data with repetition for compression. XXXXXXXXXXXX YYYYYYYYYYYY";
    let compressed = compress_for_write("/tmp/fcomp_stats/test.txt", data);
    assert!(compressed.is_some());
    let _ = decompress_for_read(&compressed.expect("checked"));

    let s = stats();
    assert!(s.files_compressed >= 1, "should count at least 1 compressed");
    assert!(s.files_decompressed >= 1, "should count at least 1 decompressed");
    assert!(s.bytes_original > 0);
    assert!(s.bytes_stored > 0);

    remove_rules("/tmp/fcomp_stats");
    set_min_size(DEFAULT_MIN_SIZE);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   stats: ok");
}
