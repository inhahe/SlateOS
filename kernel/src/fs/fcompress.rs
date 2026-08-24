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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::path::{Path, PathBuf};
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
    /// Directory subtree this rule applies to (e.g., "/var/log").
    ///
    /// A `PathBuf` rather than a `String` per design-decisions.md §261: a
    /// legal filename may contain any byte but `/` and NUL, so a `String`
    /// prefix cannot even name every directory it might be pointed at.
    pub path_prefix: PathBuf,
    /// File extensions this rule applies to (e.g., ["log", "txt"]).
    /// Empty means all extensions.
    ///
    /// Extensions stay `Vec<String>` deliberately: an extension here is
    /// compared against something the user *typed into a rule*, and a rule
    /// naming an extension with no UTF-8 spelling is not something this
    /// API needs to express.  The comparison is still byte-wise against
    /// the real filename, so a non-UTF-8 filename is handled correctly —
    /// it simply never matches a rule that filters on an extension.
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
pub fn remove_rules(prefix: impl AsRef<Path>) -> usize {
    let prefix = prefix.as_ref();
    let mut state = STATE.lock();
    let before = state.rules.len();
    state.rules.retain(|r| r.path_prefix.as_path() != prefix);
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

/// Count one file that was considered for compression and left alone.
///
/// A function rather than an inline statement because the inline version was
/// written twice as `STATE.lock().stats.files_skipped =
/// STATE.lock().stats.files_skipped.saturating_add(1)`, which deadlocks: Rust
/// evaluates the right-hand side first, but its lock guard is a temporary that
/// lives to the end of the statement, so the left-hand side waits on a
/// non-reentrant lock the same thread already holds. Every caller now shares
/// this one correct acquisition.
fn note_skipped() {
    let mut state = STATE.lock();
    state.stats.files_skipped = state.stats.files_skipped.saturating_add(1);
}

/// Check if data should be compressed for a given path, and if so,
/// return the compressed data with header.
///
/// Returns `None` if the file should not be compressed (disabled,
/// no matching rule, too small, or incompressible).
pub fn compress_for_write(path: impl AsRef<Path>, data: &[u8]) -> Option<Vec<u8>> {
    let path = path.as_ref();
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }

    let min = MIN_SIZE.load(Ordering::Relaxed);
    if (data.len() as u64) < min {
        note_skipped();
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
        note_skipped();
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
///
/// A rule's `path_prefix` denotes a *directory subtree*, not a byte prefix.
/// The distinction is the whole point: the previous implementation used a
/// bare `path.starts_with(&rule.path_prefix)`, so a rule installed on
/// `/var/log` also silently claimed `/var/logbackup.tar` and
/// `/var/logger.db` — files in a different directory that merely happened
/// to share the first eight bytes.  `path_in_subtree` ends the match on a
/// component boundary, which is what a user typing a directory means.
/// (See `fs/pathutil.rs`'s module doc; this is the same defect already
/// swept out of `intercept`, `integrity`, `findex`, `freeze` and `atime`.)
fn find_algorithm(path: &Path) -> Option<Algorithm> {
    let state = STATE.lock();

    // Find the most specific (deepest) matching rule.
    let mut best: Option<&CompressionRule> = None;
    let mut best_depth = 0usize;

    for rule in &state.rules {
        if !crate::fs::pathutil::path_in_subtree(path, rule.path_prefix.as_path()) {
            continue;
        }
        // Check extension filter.
        if !rule.extensions.is_empty() {
            let matches_ext = path.extension().is_some_and(|ext| {
                rule.extensions
                    .iter()
                    .any(|e| e.as_bytes() == ext.as_bytes())
            });
            if !matches_ext {
                continue;
            }
        }
        // Depth is counted in components, not bytes: byte length would rank
        // a rule on `/aaaaaaaa` above one on `/a/b/c` despite the latter
        // being the more specific location.  `best.is_none()` is
        // load-bearing for a rule on `/` (or the empty catch-all prefix),
        // both of which have zero components and so could never win a bare
        // `>= best_depth` against the initial zero.  `>=` rather than `>`
        // preserves the previous last-rule-wins tie-break.
        let depth = rule.path_prefix.as_path().components().count();
        if best.is_none() || depth >= best_depth {
            best_depth = depth;
            best = Some(rule);
        }
    }

    best.map(|r| r.algorithm)
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
    test_prefix_is_a_subtree_not_a_byte_prefix();
    test_non_utf8_prefix();

    serial_println!("[fcompress] Self-test passed (10 tests).");
    Ok(())
}

/// A rule on `/var/log` must not claim `/var/logbackup.tar`.
///
/// Regression test for the bare `path.starts_with(&rule.path_prefix)` this
/// module used to match with.  Under it, a user who asked to compress one
/// log directory also silently got compression on every sibling file whose
/// name began with those same bytes — including files in a *different*
/// directory, which is not a place the user pointed at.
fn test_prefix_is_a_subtree_not_a_byte_prefix() {
    let was_enabled = is_enabled();
    let old_min = min_size();
    set_enabled(true);
    set_min_size(0);

    add_rule(CompressionRule {
        path_prefix: PathBuf::from("/tmp/fcomp_sub"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    })
    .expect("add rule");

    let data = compressible_sample();

    // Inside the subtree, and the directory itself's children: match.
    assert!(
        compress_for_write("/tmp/fcomp_sub/a.txt", &data).is_some(),
        "a file under the rule's directory should match"
    );
    assert!(
        compress_for_write("/tmp/fcomp_sub/deep/b.txt", &data).is_some(),
        "a file deeper under the rule's directory should match"
    );

    // Shares the prefix bytes but is a *sibling*, in another directory.
    assert!(
        compress_for_write("/tmp/fcomp_subtly_different.txt", &data).is_none(),
        "a sibling sharing the prefix bytes must not match"
    );
    assert!(
        compress_for_write("/tmp/fcomp_subx/a.txt", &data).is_none(),
        "a sibling directory sharing the prefix bytes must not match"
    );

    // A trailing slash on the rule must not change the answer.
    remove_rules("/tmp/fcomp_sub");
    add_rule(CompressionRule {
        path_prefix: PathBuf::from("/tmp/fcomp_sub/"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    })
    .expect("add trailing-slash rule");
    assert!(
        compress_for_write("/tmp/fcomp_sub/a.txt", &data).is_some(),
        "trailing slash on the rule must still match children"
    );
    assert!(
        compress_for_write("/tmp/fcomp_subx/a.txt", &data).is_none(),
        "trailing slash must not widen the match"
    );

    remove_rules("/tmp/fcomp_sub/");
    set_min_size(old_min);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   subtree (not byte-prefix) matching: ok");
}

/// A rule may be installed on a directory whose name has no UTF-8 spelling
/// (design-decisions.md §261), and must govern only that directory.
fn test_non_utf8_prefix() {
    let was_enabled = is_enabled();
    let old_min = min_size();
    set_enabled(true);
    set_min_size(0);

    // `\xFF` and `\xFE` can begin no UTF-8 sequence, so under a `String`
    // prefix these two directories would have collapsed onto one rule.
    let dir_a = Path::new(&b"/tmp/fcomp_\xFFd"[..]);
    let dir_b = Path::new(&b"/tmp/fcomp_\xFEd"[..]);

    add_rule(CompressionRule {
        path_prefix: dir_a.to_path_buf(),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    })
    .expect("add rule");

    let data = compressible_sample();

    let mut in_a = dir_a.to_path_buf();
    in_a.push("f.txt");
    let mut in_b = dir_b.to_path_buf();
    in_b.push("f.txt");

    assert!(
        compress_for_write(in_a.as_path(), &data).is_some(),
        "a file under the non-UTF-8 rule directory should match"
    );
    assert!(
        compress_for_write(in_b.as_path(), &data).is_none(),
        "a file under the other non-UTF-8 directory must not match"
    );

    // Removal is byte-exact too: the other spelling removes nothing.
    assert_eq!(
        remove_rules(dir_b),
        0,
        "removing by the other name is a no-op"
    );
    assert_eq!(remove_rules(dir_a), 1, "removing by the exact name works");

    set_min_size(old_min);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   non-UTF-8 prefix: ok");
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

/// A payload large and repetitive enough that a *framed* compressor can win.
///
/// All three round-trip tests below used to pass a ~110-byte string and assert
/// that `compress_for_write` returned `Some`. It cannot, and the assertion was
/// demanding a bug. LZ4 here is the **frame** format, which spends 27 bytes
/// before a single payload byte — 4 magic, 11 descriptor, 4 block header, 4 end
/// mark, 4 content checksum — and gzip and zstd carry their own fixed headers.
/// `compress_for_write` returns `None` whenever the compressed form is not
/// smaller (that is exactly the incompressible-skip path this module documents
/// and counts), so on an input that small `None` is the correct answer.
///
/// The one test in this file that got it right said so out loud —
/// `test_incompressible_skip` notes "Small enough that LZ4 overhead makes
/// compressed >= original" — so the overhead was understood; the round-trip
/// tests just never ran to reveal that they were on the wrong side of it.
///
/// 4 KiB of repeating text amortises any of the three headers many times over,
/// which lets these tests assert the thing they exist to assert: that data
/// survives compress → decompress unchanged, *and* that it actually got smaller.
fn compressible_sample() -> Vec<u8> {
    const UNIT: &[u8] = b"The quick brown fox jumps over the lazy dog. ";
    let mut data = Vec::with_capacity(4096);
    while data.len() < 4096 {
        data.extend_from_slice(UNIT);
    }
    data.truncate(4096);
    data
}

fn test_compress_decompress_lz4() {
    // Enable and set up a rule.
    let was_enabled = is_enabled();
    set_enabled(true);
    set_min_size(0); // Allow any size.

    add_rule(CompressionRule {
        path_prefix: PathBuf::from("/tmp/fcomp_test"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    })
    .expect("add rule");

    let original = compressible_sample();

    // Compress.
    let compressed = compress_for_write("/tmp/fcomp_test/file.txt", &original);
    assert!(compressed.is_some(), "should have compressed");
    let compressed = compressed.expect("checked above");
    assert!(is_compressed(&compressed));
    assert!(
        compressed.len() < original.len(),
        "compress_for_write returned Some, which promises the stored form is \
         smaller: {} >= {}",
        compressed.len(),
        original.len()
    );

    // Decompress.
    let decompressed = decompress_for_read(&compressed);
    assert!(decompressed.is_some(), "should have decompressed");
    let decompressed = decompressed.expect("checked above");
    assert_eq!(decompressed, original);

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
        path_prefix: PathBuf::from("/tmp/fcomp_gz"),
        extensions: Vec::new(),
        algorithm: Algorithm::Gzip,
    })
    .expect("add rule");

    let original = compressible_sample();

    let compressed = compress_for_write("/tmp/fcomp_gz/data.bin", &original);
    assert!(compressed.is_some(), "should have compressed");
    let compressed = compressed.expect("checked");
    assert!(
        compressed.len() < original.len(),
        "stored form must be smaller"
    );

    let decompressed = decompress_for_read(&compressed);
    assert!(decompressed.is_some());
    assert_eq!(decompressed.expect("checked"), original);

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
        path_prefix: PathBuf::from("/tmp/fcomp_zst"),
        extensions: Vec::new(),
        algorithm: Algorithm::Zstd,
    })
    .expect("add rule");

    let original = compressible_sample();

    let compressed = compress_for_write("/tmp/fcomp_zst/test.dat", &original);
    assert!(compressed.is_some(), "should have compressed");
    let compressed = compressed.expect("checked");
    assert!(
        compressed.len() < original.len(),
        "stored form must be smaller"
    );

    let decompressed = decompress_for_read(&compressed);
    assert!(decompressed.is_some());
    assert_eq!(decompressed.expect("checked"), original);

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
        path_prefix: PathBuf::from("/tmp/fcomp_rand"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    })
    .expect("add rule");

    // 32 distinct bytes: nothing repeats, so the LZ4 block cannot shrink and is
    // stored verbatim, and the frame then adds its fixed 27 bytes on top. The
    // result is necessarily larger than the input, so the skip is not a
    // "may or may not" — it is arithmetic, and this test now asserts it.
    //
    // It used to accept either answer, with the note "The important thing is it
    // doesn't panic". That gave the skip path no coverage at all, which mattered
    // more than it looks: the skip path is where `note_skipped` lives, and the
    // two inlined copies it replaced each took `STATE` twice in one statement
    // and would have deadlocked the moment they ran. A test that tolerates both
    // outcomes cannot fail, so it never ran them.
    let data: Vec<u8> = (0u8..32).collect();
    let before = stats().files_skipped;

    let result = compress_for_write("/tmp/fcomp_rand/random.bin", &data);
    assert!(
        result.is_none(),
        "32 incompressible bytes cannot fit in an LZ4 frame with 27 bytes of \
         fixed overhead, so the write must be skipped"
    );
    assert_eq!(
        stats().files_skipped,
        before.saturating_add(1),
        "a skipped write must be counted"
    );

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
        path_prefix: PathBuf::from("/var/log"),
        extensions: alloc::vec![String::from("log")],
        algorithm: Algorithm::Gzip,
    })
    .expect("add rule");

    // Comfortably compressible, so a `None` here can only mean the rule failed
    // to match. With a ~110-byte payload the `is_some()` below would have been
    // partly a bet on the gzip ratio clearing its 18-byte header — a second
    // reason to fail, in a test that is about prefix and extension matching.
    let data = compressible_sample();

    // Should match.
    let r1 = compress_for_write("/var/log/syslog.log", &data);
    assert!(r1.is_some(), ".log under /var/log should match");

    // Should NOT match (wrong extension).
    let r2 = compress_for_write("/var/log/data.bin", &data);
    assert!(r2.is_none(), ".bin under /var/log should not match");

    // Should NOT match (wrong prefix).
    let r3 = compress_for_write("/home/user/file.log", &data);
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
        path_prefix: PathBuf::from("/tmp/fcomp_min"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    })
    .expect("add rule");

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
        path_prefix: PathBuf::from("/tmp/fcomp_stats"),
        extensions: Vec::new(),
        algorithm: Algorithm::Lz4,
    })
    .expect("add rule");

    // Must be big enough to beat the LZ4 frame header — see `compressible_sample`.
    let data = compressible_sample();
    let compressed = compress_for_write("/tmp/fcomp_stats/test.txt", &data);
    assert!(compressed.is_some(), "should have compressed");
    let _ = decompress_for_read(&compressed.expect("checked"));

    let s = stats();
    assert!(
        s.files_compressed >= 1,
        "should count at least 1 compressed"
    );
    assert!(
        s.files_decompressed >= 1,
        "should count at least 1 decompressed"
    );
    assert!(s.bytes_original > 0);
    assert!(s.bytes_stored > 0);

    remove_rules("/tmp/fcomp_stats");
    set_min_size(DEFAULT_MIN_SIZE);
    set_enabled(was_enabled);

    serial_println!("[fcompress]   stats: ok");
}
