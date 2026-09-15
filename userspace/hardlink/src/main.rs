//! Slate OS file deduplication utility.
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
    /// Modification time in whole seconds, for `--respect-time`.
    ///
    /// `None` where the platform does not report one. Every one of these was
    /// previously a `_`-prefixed field hardcoded to `0` with the comment
    /// "Platform-dependent, simulated" -- documented for the flag it serves,
    /// carrying a fabricated value, and read by nothing. A zero that stands in
    /// for a real mtime compares EQUAL to every other zero, so had the flags
    /// ever been wired to these fields they would have permitted every merge.
    mtime: Option<u64>,
    /// Permission bits, for `--respect-perm`.
    mode: Option<u32>,
    /// Owner, for `--respect-owner`.
    uid: Option<u32>,
    /// Group, for `--respect-owner`.
    gid: Option<u32>,
}

/// Read the metadata the respect flags compare.
///
/// `None` means "this platform does not tell us", which is deliberately NOT
/// the same as "they differ" or "they match": a flag whose metadata is
/// unavailable refuses rather than silently permitting the merge. Silently
/// permitting is what the hardcoded zeros would have done.
fn file_meta(md: &fs::Metadata) -> (Option<u64>, Option<u32>, Option<u32>, Option<u32>) {
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (mtime, Some(md.mode()), Some(md.uid()), Some(md.gid()))
    }
    #[cfg(not(unix))]
    {
        // The dev host. Modes and owners are not comparable here, so the
        // flags that need them refuse rather than pass.
        (mtime, None, None, None)
    }
}

/// The final path component, which is what `--respect-name` compares.
fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Whether `opts` permits these two files to be merged.
///
/// `Err` is "this build cannot answer", not "no". It is returned rather than
/// swallowed because the alternative -- treating an unanswerable comparison as
/// permission -- is exactly the defect these flags were added to prevent, and
/// hardlink's merge is destructive.
fn may_link(a: &FileInfo, b: &FileInfo, opts: &HardlinkOpts) -> Result<bool, String> {
    if opts.respect_name && base_name(&a.path) != base_name(&b.path) {
        return Ok(false);
    }

    // Extended attributes are not readable from this build at all -- there is
    // no `getxattr` to call -- so the answer is "cannot tell", never "they
    // match". Refusing here rather than at parse time keeps every
    // unanswerable comparison on one path, and means the flag still parses for
    // a script that passes it unconditionally.
    if opts.respect_xattr {
        return Err(
            "--respect-xattr: this build cannot read extended attributes, so it cannot honour the flag"
                .to_string(),
        );
    }

    let checks: [(bool, Option<u64>, Option<u64>, &str); 4] = [
        (opts.respect_time, a.mtime, b.mtime, "--respect-time"),
        (
            opts.respect_perm,
            a.mode.map(u64::from),
            b.mode.map(u64::from),
            "--respect-perm",
        ),
        (
            opts.respect_owner,
            a.uid.map(u64::from),
            b.uid.map(u64::from),
            "--respect-owner",
        ),
        (
            opts.respect_owner,
            a.gid.map(u64::from),
            b.gid.map(u64::from),
            "--respect-owner",
        ),
    ];

    for (wanted, lhs, rhs, flag) in checks {
        if !wanted {
            continue;
        }
        match (lhs, rhs) {
            (Some(x), Some(y)) if x == y => {}
            (Some(_), Some(_)) => return Ok(false),
            _ => {
                return Err(format!(
                    "{flag}: this build cannot read that attribute, so it \
cannot honour the flag"
                ));
            }
        }
    }

    Ok(true)
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

/// Replace `dup_path` with a hard link to `master_path`, without ever
/// leaving the duplicate's data unreachable.
///
/// **This replaces a sequence that destroyed files.** It was:
///
/// ```text
/// fs::remove_file(dup_path)?;          // the data is now gone
/// fs::hard_link(master_path, dup_path) // ...and if THIS fails,
///     // Try to restore the original file.
///     continue;                        // it stays gone
/// ```
///
/// The comment said "Try to restore the original file" and no restore was
/// written — the next line was `continue`. So any failure between the unlink
/// and the link lost the duplicate outright: a full disk, a cross-device
/// master, a read-only directory, the link count already at its maximum, or
/// simply losing the race with another writer. The content was identical to
/// the master's *at the moment it was compared*, which is the only reason this
/// was survivable at all, and it is not a guarantee: `files_identical` reads
/// both files separately and the master can change afterwards.
///
/// Link-then-rename has no such window. `hard_link` creates a NEW name, so
/// nothing is removed until it has succeeded, and `rename` over the duplicate
/// is atomic on both filesystems this ships on. If the link fails there is
/// nothing to undo; if the rename fails, the temporary is cleaned up and the
/// duplicate is still there.
///
/// Returns whether the link was made, and counts its own errors.
fn link_over(master_path: &str, dup_path: &str, opts: &HardlinkOpts, stats: &mut Stats) -> bool {
    // The temporary must be in the SAME directory as the duplicate: `rename`
    // is only atomic within a filesystem, and a `/tmp` staging path would
    // cross one on any normal layout.
    let tmp_path = format!("{dup_path}.hardlink-tmp");

    // A leftover from an interrupted earlier run would make `hard_link` fail
    // with EEXIST forever. Removing it is safe precisely because this name is
    // ours: it is derived from the duplicate's own path.
    let _ = fs::remove_file(&tmp_path);

    if let Err(e) = fs::hard_link(master_path, &tmp_path) {
        if !opts.quiet {
            eprintln!("hardlink: cannot link {dup_path}: {e}");
        }
        stats.errors += 1;
        return false;
    }

    if let Err(e) = fs::rename(&tmp_path, dup_path) {
        if !opts.quiet {
            eprintln!("hardlink: cannot replace {dup_path}: {e}");
        }
        // The duplicate is untouched; only our temporary needs clearing.
        let _ = fs::remove_file(&tmp_path);
        stats.errors += 1;
        return false;
    }

    stats.links_created += 1;
    true
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
            if let Some(max) = opts.max_size
                && size > max
            {
                continue;
            }

            // Skip empty files.
            if size == 0 {
                continue;
            }

            let (mtime, mode, uid, gid) = file_meta(&metadata);
            files.push(FileInfo {
                path: path_str,
                size,
                mtime,
                mode,
                uid,
                gid,
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
    for indices in by_size.values() {
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
        for hash_group in by_hash.values() {
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

                // ...and that the caller allows THESE two to be merged.
                // Content equality is not sufficient: two files can hold the
                // same bytes and differ in owner or mode, and merging them
                // collapses both onto the master's.
                match may_link(&files[master_idx], &files[dup_idx], opts) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(why) => {
                        if !opts.quiet {
                            eprintln!("hardlink: {why}");
                        }
                        stats.errors += 1;
                        continue;
                    }
                }

                stats.duplicates_found += 1;
                stats.bytes_saved += files[dup_idx].size;

                if opts.verbose {
                    eprintln!("  {} => {}", dup_path, master_path);
                }

                if !opts.dry_run && !link_over(master_path, dup_path, opts, &mut stats) {
                    continue;
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

    // ---------------- may_link / the respect flags ----------------

    fn info(path: &str) -> FileInfo {
        FileInfo {
            path: path.to_string(),
            size: 10,
            mtime: Some(1_000),
            mode: Some(0o644),
            uid: Some(1000),
            gid: Some(1000),
        }
    }

    /// With no respect flag, content equality is the whole test -- which is
    /// the behaviour that shipped, and is correct only when nobody asked for
    /// more.
    #[test]
    fn without_a_respect_flag_anything_may_link() {
        let o = opts_for_test();
        assert_eq!(may_link(&info("/a/x"), &info("/b/y"), &o), Ok(true));
    }

    /// `--respect-name` compares the final component, not the whole path:
    /// deduplicating identical files across directories is the point of the
    /// tool, and requiring equal paths would permit nothing at all.
    #[test]
    fn respect_name_compares_the_basename_not_the_path() {
        let mut o = opts_for_test();
        o.respect_name = true;
        assert_eq!(
            may_link(&info("/a/same.txt"), &info("/b/same.txt"), &o),
            Ok(true),
            "same name in different directories still links"
        );
        assert_eq!(
            may_link(&info("/a/one.txt"), &info("/a/two.txt"), &o),
            Ok(false)
        );
    }

    /// Each attribute flag refuses a pair that differs in exactly that
    /// attribute, and permits one that does not.
    #[test]
    fn each_respect_flag_refuses_its_own_difference() {
        type SetFlag = fn(&mut HardlinkOpts);
        type MakeDiffer = fn(&mut FileInfo);
        let cases: [(SetFlag, MakeDiffer); 4] = [
            (|o| o.respect_time = true, |f| f.mtime = Some(2_000)),
            (|o| o.respect_perm = true, |f| f.mode = Some(0o600)),
            (|o| o.respect_owner = true, |f| f.uid = Some(0)),
            (|o| o.respect_owner = true, |f| f.gid = Some(0)),
        ];
        for (set, differ) in cases {
            let mut o = opts_for_test();
            set(&mut o);

            let a = info("/a/x");
            assert_eq!(may_link(&a, &info("/b/x"), &o), Ok(true), "identical");

            let mut b = info("/b/x");
            differ(&mut b);
            assert_eq!(may_link(&a, &b, &o), Ok(false), "differing");
        }
    }

    /// **The property the hardcoded zeros would have destroyed.** An attribute
    /// this build cannot read is an ERROR, not a match. Had the flags been
    /// wired to the old `_mode: 0` fields, every file would have compared
    /// equal to every other and every merge would have been permitted -- the
    /// flag would have looked implemented and prevented nothing.
    #[test]
    fn an_unreadable_attribute_refuses_rather_than_permits() {
        let mut o = opts_for_test();
        o.respect_perm = true;

        let mut a = info("/a/x");
        let mut b = info("/b/x");
        a.mode = None;
        b.mode = None;

        let r = may_link(&a, &b, &o);
        assert!(r.is_err(), "unknown must not read as equal: {r:?}");
    }

    /// `--respect-xattr` cannot be honoured at all, so it always refuses.
    #[test]
    fn respect_xattr_always_refuses() {
        let mut o = opts_for_test();
        o.respect_xattr = true;
        assert!(may_link(&info("/a/x"), &info("/a/x"), &o).is_err());
    }

    // ---------------- link_over ----------------

    fn scratch(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "slateos-hardlink-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&p);
        p
    }

    fn opts_for_test() -> HardlinkOpts {
        let mut o = parse_args(&[]);
        o.quiet = true;
        o
    }

    fn zero_stats() -> Stats {
        Stats {
            files_scanned: 0,
            duplicates_found: 0,
            bytes_saved: 0,
            links_created: 0,
            errors: 0,
        }
    }

    /// THE PROPERTY THE OLD CODE DID NOT HAVE. A link that cannot be made must
    /// leave the duplicate exactly where it was. The previous sequence removed
    /// the duplicate FIRST and carried a comment promising a restore that was
    /// never written, so every failure between the two steps destroyed the
    /// file.
    #[test]
    fn a_failed_link_leaves_the_duplicate_intact() {
        let dir = scratch("failed-link");
        let dup = dir.join("dup.txt");
        fs::write(&dup, b"irreplaceable").expect("scratch write");

        let mut stats = zero_stats();
        let ok = link_over(
            // A master that does not exist: `hard_link` must fail.
            &dir.join("no-such-master").to_string_lossy(),
            &dup.to_string_lossy(),
            &opts_for_test(),
            &mut stats,
        );

        assert!(!ok, "it must report failure");
        assert_eq!(stats.links_created, 0);
        assert_eq!(stats.errors, 1);
        assert!(dup.exists(), "THE DUPLICATE MUST STILL EXIST");
        assert_eq!(
            fs::read(&dup).expect("read back"),
            b"irreplaceable",
            "and still hold its own bytes"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// A failed link must not leave its scratch name behind either, or the
    /// next run trips over `EEXIST` on a path it chose itself.
    #[test]
    fn a_failed_link_leaves_no_temporary_behind() {
        let dir = scratch("no-temp");
        let dup = dir.join("dup.txt");
        fs::write(&dup, b"x").expect("scratch write");

        let mut stats = zero_stats();
        let _ = link_over(
            &dir.join("no-such-master").to_string_lossy(),
            &dup.to_string_lossy(),
            &opts_for_test(),
            &mut stats,
        );

        let tmp = dir.join("dup.txt.hardlink-tmp");
        assert!(!tmp.exists(), "left {tmp:?} behind");

        let _ = fs::remove_dir_all(&dir);
    }

    /// The ordinary path: the duplicate ends up sharing the master's content,
    /// and the master is untouched.
    #[test]
    fn a_successful_link_replaces_the_duplicate_and_spares_the_master() {
        let dir = scratch("success");
        let master = dir.join("master.txt");
        let dup = dir.join("dup.txt");
        fs::write(&master, b"shared bytes").expect("scratch write");
        fs::write(&dup, b"shared bytes").expect("scratch write");

        let mut stats = zero_stats();
        let ok = link_over(
            &master.to_string_lossy(),
            &dup.to_string_lossy(),
            &opts_for_test(),
            &mut stats,
        );

        assert!(ok);
        assert_eq!(stats.links_created, 1);
        assert_eq!(stats.errors, 0);
        assert!(master.exists(), "the master is never the one removed");
        assert_eq!(fs::read(&dup).expect("read back"), b"shared bytes");
        assert!(
            !dir.join("dup.txt.hardlink-tmp").exists(),
            "the temporary is renamed away, not left"
        );

        let _ = fs::remove_dir_all(&dir);
    }
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
        let args = vec!["-s".to_string(), "1024".to_string(), "/tmp".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.min_size, 1024);
    }

    #[test]
    fn test_parse_args_max_size() {
        let args = vec!["-S".to_string(), "1048576".to_string(), "/tmp".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.max_size, Some(1048576));
    }

    #[test]
    fn test_parse_args_exclude() {
        let args = vec!["-X".to_string(), ".git".to_string(), "/tmp".to_string()];
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
            dry_run: true,
            verbose: false,
            quiet: true,
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
        let mut files = Vec::new();
        scan_directory("/nonexistent/dir", &opts, &mut files);
        assert!(files.is_empty());
    }
}
