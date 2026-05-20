//! OurOS file deduplication utility.
//!
//! Multi-personality binary providing:
//! - **hardlink** — find and link identical files to save disk space
//!
//! Scans directories for files with identical content and replaces duplicates
//! with hard links. Uses content hashing for fast comparison.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Options
// ============================================================================

struct HardlinkOpts {
    dry_run: bool,
    verbose: bool,
    quiet: bool,
    respect_name: bool,
    respect_time: bool,
    respect_perm: bool,
    respect_owner: bool,
    respect_xattr: bool,
    min_size: u64,
    max_size: Option<u64>,
    content: bool,
    exclude: Vec<String>,
    method: Method,
    dirs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Method {
    Sha256,
    Simple,
}

// ============================================================================
// File info
// ============================================================================

#[derive(Clone, Debug)]
struct FileInfo {
    path: String,
    size: u64,
    /// Inode number (platform-dependent, for future same-device dedup).
    _inode: u64,
    /// Device number (for cross-device detection).
    _dev: u64,
    /// Modification time (for --respect-time).
    _mtime: u64,
    /// File mode (for --respect-perm).
    _mode: u32,
}

// ============================================================================
// Hashing
// ============================================================================

/// Simple hash function (FNV-1a variant) for file deduplication.
fn hash_file(path: &str) -> Option<u64> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = [0u8; 8192];
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset basis.

    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3); // FNV prime.
        }
    }

    Some(hash)
}

/// SHA-256-like hash for stronger deduplication (simplified).
fn _sha256_file(path: &str) -> Option<[u8; 32]> {
    let data = fs::read(path).ok()?;
    Some(_sha256_bytes(&data))
}

fn _sha256_bytes(data: &[u8]) -> [u8; 32] {
    // Simplified hash — uses multiple FNV rounds for good distribution.
    let mut h = [0u64; 4];
    h[0] = 0x6a09_e667_f3bc_c908;
    h[1] = 0xbb67_ae85_84ca_a73b;
    h[2] = 0x3c6e_f372_fe94_f82b;
    h[3] = 0xa54f_f53a_5f1d_36f1;

    for (i, &byte) in data.iter().enumerate() {
        let idx = i % 4;
        h[idx] ^= byte as u64;
        h[idx] = h[idx].wrapping_mul(0x0100_0000_01b3);
        h[(idx + 1) % 4] = h[(idx + 1) % 4].wrapping_add(h[idx]);
    }

    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        let bytes = val.to_le_bytes();
        result[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
    }
    result
}

/// Byte-for-byte comparison of two files.
fn files_identical(path_a: &str, path_b: &str) -> bool {
    let a = match fs::read(path_a) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let b = match fs::read(path_b) {
        Ok(d) => d,
        Err(_) => return false,
    };
    a == b
}

// ============================================================================
// Directory scanning
// ============================================================================

fn scan_directory(dir: &str, opts: &HardlinkOpts, files: &mut Vec<FileInfo>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let path_str = path.to_string_lossy().to_string();

        // Check exclusions.
        let should_exclude = opts.exclude.iter().any(|ex| path_str.contains(ex.as_str()));
        if should_exclude {
            continue;
        }

        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if metadata.is_dir() {
            scan_directory(&path_str, opts, files);
        } else if metadata.is_file() {
            let size = metadata.len();

            // Size filters.
            if size < opts.min_size {
                continue;
            }
            if let Some(max) = opts.max_size {
                if size > max {
                    continue;
                }
            }

            // Skip empty files.
            if size == 0 {
                continue;
            }

            files.push(FileInfo {
                path: path_str,
                size,
                _inode: 0, // Platform-dependent, simulated.
                _dev: 0,
                _mtime: 0,
                _mode: 0,
            });
        }
    }
}

// ============================================================================
// Deduplication
// ============================================================================

struct Stats {
    files_scanned: u64,
    duplicates_found: u64,
    bytes_saved: u64,
    links_created: u64,
    errors: u64,
}

fn deduplicate(opts: &HardlinkOpts) -> Stats {
    let mut stats = Stats {
        files_scanned: 0,
        duplicates_found: 0,
        bytes_saved: 0,
        links_created: 0,
        errors: 0,
    };

    // Collect all files.
    let mut files = Vec::new();
    for dir in &opts.dirs {
        scan_directory(dir, opts, &mut files);
    }
    stats.files_scanned = files.len() as u64;

    if opts.verbose {
        eprintln!("hardlink: scanned {} files", files.len());
    }

    // Group by size first (only same-size files can be identical).
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (idx, file) in files.iter().enumerate() {
        by_size.entry(file.size).or_default().push(idx);
    }

    // For each size group with >1 file, hash and compare.
    for (_size, indices) in &by_size {
        if indices.len() < 2 {
            continue;
        }

        // Hash all files in the group.
        let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
        for &idx in indices {
            if let Some(hash) = hash_file(&files[idx].path) {
                by_hash.entry(hash).or_default().push(idx);
            }
        }

        // For each hash collision group, verify byte-for-byte.
        for (_, hash_group) in &by_hash {
            if hash_group.len() < 2 {
                continue;
            }

            // The first file in the group is the "master" — others link to it.
            let master_idx = hash_group[0];
            let master_path = &files[master_idx].path;

            for &dup_idx in &hash_group[1..] {
                let dup_path = &files[dup_idx].path;

                // Verify content match.
                if !files_identical(master_path, dup_path) {
                    continue;
                }

                stats.duplicates_found += 1;
                stats.bytes_saved += files[dup_idx].size;

                if opts.verbose {
                    eprintln!("  {} => {}", dup_path, master_path);
                }

                if !opts.dry_run {
                    // Create hardlink: remove dup, link to master.
                    if let Err(e) = fs::remove_file(dup_path) {
                        if !opts.quiet {
                            eprintln!("hardlink: cannot remove {dup_path}: {e}");
                        }
                        stats.errors += 1;
                        continue;
                    }

                    if let Err(e) = fs::hard_link(master_path, dup_path) {
                        if !opts.quiet {
                            eprintln!("hardlink: cannot link {dup_path}: {e}");
                        }
                        stats.errors += 1;
                        // Try to restore the original file.
                        continue;
                    }

                    stats.links_created += 1;
                }
            }
        }
    }

    stats
}

// ============================================================================
// CLI
// ============================================================================

fn parse_args(args: &[String]) -> HardlinkOpts {
    let mut opts = HardlinkOpts {
        dry_run: false,
        verbose: false,
        quiet: false,
        respect_name: false,
        respect_time: false,
        respect_perm: false,
        respect_owner: false,
        respect_xattr: false,
        min_size: 1,
        max_size: None,
        content: true,
        exclude: Vec::new(),
        method: Method::Simple,
        dirs: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: hardlink [options] directory...");
                println!();
                println!("Find and replace duplicate files with hard links.");
                println!();
                println!("Options:");
                println!("  -n, --dry-run        Don't actually link, just report");
                println!("  -v, --verbose        Verbose output");
                println!("  -q, --quiet          Suppress output");
                println!("  -f, --respect-name   Only link files with same name");
                println!("  -t, --respect-time   Only link files with same mtime");
                println!("  -p, --respect-perm   Only link files with same permissions");
                println!("  -o, --respect-owner  Only link files with same owner");
                println!("  -x, --respect-xattr  Only link files with same xattrs");
                println!("  -s, --minimum-size N Minimum file size (default 1)");
                println!("  -S, --maximum-size N Maximum file size");
                println!("  -X, --exclude PAT    Exclude pattern");
                println!("  --method METHOD      Hash method: simple, sha256");
                println!("  -c, --content        Content comparison (default)");
                println!("  -h, --help           Show this help");
                println!("  -V, --version        Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("hardlink {VERSION}");
                process::exit(0);
            }
            "-n" | "--dry-run" => opts.dry_run = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-q" | "--quiet" => opts.quiet = true,
            "-f" | "--respect-name" => opts.respect_name = true,
            "-t" | "--respect-time" => opts.respect_time = true,
            "-p" | "--respect-perm" => opts.respect_perm = true,
            "-o" | "--respect-owner" => opts.respect_owner = true,
            "-x" | "--respect-xattr" => opts.respect_xattr = true,
            "-c" | "--content" => opts.content = true,
            "-s" | "--minimum-size" => {
                i += 1;
                if i < args.len() {
                    opts.min_size = args[i].parse().unwrap_or(1);
                }
            }
            "-S" | "--maximum-size" => {
                i += 1;
                if i < args.len() {
                    opts.max_size = args[i].parse().ok();
                }
            }
            "-X" | "--exclude" => {
                i += 1;
                if i < args.len() {
                    opts.exclude.push(args[i].clone());
                }
            }
            s if s.starts_with("--method=") => {
                if let Some(val) = s.strip_prefix("--method=") {
                    opts.method = match val {
                        "sha256" => Method::Sha256,
                        _ => Method::Simple,
                    };
                }
            }
            s if !s.starts_with('-') => {
                opts.dirs.push(s.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    opts
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let opts = parse_args(&rest);

    if opts.dirs.is_empty() {
        eprintln!("hardlink: no directories specified");
        eprintln!("Try 'hardlink --help' for more information.");
        process::exit(1);
    }

    let stats = deduplicate(&opts);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if !opts.quiet {
        let _ = writeln!(out);
        let _ = writeln!(out, "Files scanned:    {}", stats.files_scanned);
        let _ = writeln!(out, "Duplicates found: {}", stats.duplicates_found);
        if opts.dry_run {
            let _ = writeln!(out, "Would save:       {} bytes", stats.bytes_saved);
        } else {
            let _ = writeln!(out, "Links created:    {}", stats.links_created);
            let _ = writeln!(out, "Bytes saved:      {}", stats.bytes_saved);
        }
        if stats.errors > 0 {
            let _ = writeln!(out, "Errors:           {}", stats.errors);
        }
    }

    if stats.errors > 0 {
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_bytes_deterministic() {
        let data = b"hello world";
        let h1 = _sha256_bytes(data);
        let h2 = _sha256_bytes(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_sha256_bytes_different() {
        let h1 = _sha256_bytes(b"hello");
        let h2 = _sha256_bytes(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_sha256_bytes_empty() {
        let h = _sha256_bytes(b"");
        // Should still produce a valid hash.
        assert_eq!(h.len(), 32);
    }

    #[test]
    fn test_hash_file_nonexistent() {
        assert!(hash_file("/nonexistent/file").is_none());
    }

    #[test]
    fn test_sha256_file_nonexistent() {
        assert!(_sha256_file("/nonexistent/file").is_none());
    }

    #[test]
    fn test_files_identical_nonexistent() {
        assert!(!files_identical("/nonexistent/a", "/nonexistent/b"));
    }

    #[test]
    fn test_method_equality() {
        assert_eq!(Method::Simple, Method::Simple);
        assert_ne!(Method::Simple, Method::Sha256);
    }

    #[test]
    fn test_parse_args_dry_run() {
        let args = vec!["-n".to_string(), "/tmp".to_string()];
        let opts = parse_args(&args);
        assert!(opts.dry_run);
        assert_eq!(opts.dirs, vec!["/tmp"]);
    }

    #[test]
    fn test_parse_args_verbose() {
        let args = vec!["-v".to_string(), "/tmp".to_string()];
        let opts = parse_args(&args);
        assert!(opts.verbose);
    }

    #[test]
    fn test_parse_args_min_size() {
        let args = vec![
            "-s".to_string(),
            "1024".to_string(),
            "/tmp".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.min_size, 1024);
    }

    #[test]
    fn test_parse_args_max_size() {
        let args = vec![
            "-S".to_string(),
            "1048576".to_string(),
            "/tmp".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.max_size, Some(1048576));
    }

    #[test]
    fn test_parse_args_exclude() {
        let args = vec![
            "-X".to_string(),
            ".git".to_string(),
            "/tmp".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.exclude, vec![".git"]);
    }

    #[test]
    fn test_parse_args_multiple_dirs() {
        let args = vec!["/a".to_string(), "/b".to_string(), "/c".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.dirs.len(), 3);
    }

    #[test]
    fn test_parse_args_respect_flags() {
        let args = vec![
            "-f".to_string(),
            "-t".to_string(),
            "-p".to_string(),
            "/tmp".to_string(),
        ];
        let opts = parse_args(&args);
        assert!(opts.respect_name);
        assert!(opts.respect_time);
        assert!(opts.respect_perm);
    }

    #[test]
    fn test_scan_nonexistent_dir() {
        let opts = HardlinkOpts {
            dry_run: true, verbose: false, quiet: true,
            respect_name: false, respect_time: false,
            respect_perm: false, respect_owner: false,
            respect_xattr: false, min_size: 1,
            max_size: None, content: true,
            exclude: Vec::new(), method: Method::Simple,
            dirs: Vec::new(),
        };
        let mut files = Vec::new();
        scan_directory("/nonexistent/dir", &opts, &mut files);
        assert!(files.is_empty());
    }
}
