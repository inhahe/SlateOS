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
    /// IGNORE flags, not RESPECT flags, and the difference is the default.
    ///
    /// util-linux spells these `-t, --ignore-time`, `-p, --ignore-mode` and
    /// `-o, --ignore-owner`: "Link and compare files even if their mode is
    /// different. Results may be slightly unpredictable." So upstream
    /// REFUSES to merge files whose mode, owner or mtime differ, and each
    /// flag relaxes one of those.
    ///
    /// This crate had them as `--respect-*`, defaulting to false -- meaning
    /// it merged files with differing mode, owner and time unless told not
    /// to. That is upstream's `-pot`, which its own manual calls
    /// unpredictable, taken as the default. On a tool that replaces two
    /// files with one inode, and which re-running cannot undo.
    ignore_time: bool,
    ignore_mode: bool,
    ignore_owner: bool,
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
            "--respect-xattrs: this build cannot read extended attributes, so it cannot honour the flag"
                .to_string(),
        );
    }

    // `!opts.ignore_*`: the check runs unless it was explicitly relaxed.
    // The flag name in each row is the one that would TURN THE CHECK OFF,
    // because that is what a reader of the error needs to know.
    let checks: [(bool, Option<u64>, Option<u64>, &str); 4] = [
        (!opts.ignore_time, a.mtime, b.mtime, "--ignore-time"),
        (
            !opts.ignore_mode,
            a.mode.map(u64::from),
            b.mode.map(u64::from),
            "--ignore-mode",
        ),
        (
            !opts.ignore_owner,
            a.uid.map(u64::from),
            b.uid.map(u64::from),
            "--ignore-owner",
        ),
        (
            !opts.ignore_owner,
            a.gid.map(u64::from),
            b.gid.map(u64::from),
            "--ignore-owner",
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
        // All false = every check ON, which is upstream's default.
        ignore_time: false,
        ignore_mode: false,
        ignore_owner: false,
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
                println!("  -f, --respect-name   Filenames have to be identical");
                println!("  -t, --ignore-time    Ignore timestamps when testing equality");
                println!("  -p, --ignore-mode    Link even if the file mode differs");
                println!("  -o, --ignore-owner   Link even if the owner differs");
                println!("  -X, --respect-xattrs Respect extended attributes");
                println!("  -s, --minimum-size N Minimum file size (default 1)");
                println!("  -S, --maximum-size N Maximum file size");
                println!("  -x, --exclude REGEX  Exclude files matching REGEX");
                println!("  --method METHOD      Hash method: simple, sha256");
                println!("  -c, --content        Compare only contents, same as -pot");
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
            "-t" | "--ignore-time" => opts.ignore_time = true,
            "-p" | "--ignore-mode" => opts.ignore_mode = true,
            "-o" | "--ignore-owner" => opts.ignore_owner = true,
            // `-X` and `-x` were SWAPPED with each other. Upstream:
            // `-x <regex>` excludes, `-X` respects xattrs. With them
            // reversed, `hardlink -x '\.git' dir` set the xattr flag and
            // left the regex to be read as a PATH to scan -- so the files
            // the caller named to exclude were linked instead.
            "-X" | "--respect-xattrs" => opts.respect_xattr = true,
            // "compare only file contents, same as -pot" -- util-linux's own
            // words. It RELAXES all three checks rather than selecting a
            // comparison method.
            "-c" | "--content" => {
                opts.content = true;
                opts.ignore_time = true;
                opts.ignore_mode = true;
                opts.ignore_owner = true;
            }
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
            "-x" | "--exclude" => {
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

    /// BY DEFAULT a difference in time, mode or owner refuses the merge,
    /// and the matching `--ignore-*` flag relaxes exactly that one.
    ///
    /// The default is the half that matters. This crate used to default to
    /// merging files that differed in all three -- upstream's `-pot`, which
    /// its own manual calls "slightly unpredictable" -- so the old version of
    /// this test switched each check ON and proved it worked when asked for.
    /// It could not see that nobody was asking.
    #[test]
    fn differences_refuse_by_default_and_each_ignore_flag_relaxes_one() {
        type SetFlag = fn(&mut HardlinkOpts);
        type MakeDiffer = fn(&mut FileInfo);
        let cases: [(SetFlag, MakeDiffer); 4] = [
            (|o| o.ignore_time = true, |f| f.mtime = Some(2_000)),
            (|o| o.ignore_mode = true, |f| f.mode = Some(0o600)),
            (|o| o.ignore_owner = true, |f| f.uid = Some(0)),
            (|o| o.ignore_owner = true, |f| f.gid = Some(0)),
        ];
        for (relax, differ) in cases {
            let a = info("/a/x");
            let mut b = info("/b/x");
            differ(&mut b);

            // No flags: the difference is refused.
            let plain = opts_for_test();
            assert_eq!(
                may_link(&a, &b, &plain),
                Ok(false),
                "a difference must refuse the merge with no flags given"
            );
            // Identical files are still permitted, or the assertion above
            // would pass against a `may_link` that refused everything.
            assert_eq!(may_link(&a, &info("/b/x"), &plain), Ok(true), "identical");

            // The matching --ignore-* flag relaxes it.
            let mut relaxed = opts_for_test();
            relax(&mut relaxed);
            assert_eq!(
                may_link(&a, &b, &relaxed),
                Ok(true),
                "the --ignore-* flag must permit its own difference"
            );
        }
    }

    /// `-c` is "compare only file contents, same as -pot" -- it relaxes all
    /// three checks rather than selecting a comparison method.
    #[test]
    fn content_relaxes_mode_owner_and_time_together() {
        let opts = parse_args(&["-c".to_string(), "/tmp".to_string()]);
        assert!(opts.ignore_time && opts.ignore_mode && opts.ignore_owner);

        let a = info("/a/x");
        let mut b = info("/b/x");
        b.mtime = Some(2_000);
        b.mode = Some(0o600);
        b.uid = Some(0);
        let mut o = opts_for_test();
        o.ignore_time = true;
        o.ignore_mode = true;
        o.ignore_owner = true;
        assert_eq!(may_link(&a, &b, &o), Ok(true));
    }

    /// **The property the hardcoded zeros would have destroyed.** An attribute
    /// this build cannot read is an ERROR, not a match. Had the flags been
    /// wired to the old `_mode: 0` fields, every file would have compared
    /// equal to every other and every merge would have been permitted -- the
    /// flag would have looked implemented and prevented nothing.
    #[test]
    fn an_unreadable_attribute_refuses_rather_than_permits() {
        // No flag needed: the mode check is on by default now.
        let o = opts_for_test();

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
        // `-x`, not `-X`. This test asserted the swap it was meant to guard:
        // it proved the exclude option was reachable, using the letter that
        // upstream gives to --respect-xattrs.
        let args = vec!["-x".to_string(), ".git".to_string(), "/tmp".to_string()];
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
        assert!(opts.ignore_time);
        assert!(opts.ignore_mode);
    }

    /// `-x` excludes and `-X` respects xattrs, as upstream binds them.
    ///
    /// They were swapped. With `-X` as the exclude option, `hardlink -x
    /// '\.git' dir` set the xattr flag and left `\.git` to be read as a PATH
    /// to scan -- so the files the caller named to exclude were candidates
    /// for linking instead.
    #[test]
    fn exclude_is_lowercase_x_and_xattrs_is_uppercase() {
        let opts = parse_args(&["-x".to_string(), "\\.git".to_string(), "/tmp".to_string()]);
        assert_eq!(opts.exclude, vec!["\\.git".to_string()]);
        assert!(!opts.respect_xattr, "-x must not set the xattr flag");
        assert_eq!(
            opts.dirs,
            vec!["/tmp".to_string()],
            "the regex must not be taken as a directory to scan"
        );

        let opts = parse_args(&["-X".to_string(), "/tmp".to_string()]);
        assert!(opts.respect_xattr, "-X respects extended attributes");
        assert!(opts.exclude.is_empty(), "-X takes no argument");
    }

    #[test]
    fn test_scan_nonexistent_dir() {
        let opts = HardlinkOpts {
            dry_run: true,
            verbose: false,
            quiet: true,
            respect_name: false,
            ignore_time: false,
            ignore_mode: false,
            ignore_owner: false,
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
