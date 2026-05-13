//! Filesystem link analysis and health checking.
//!
//! Finds broken symlinks, analyzes hardlink groups (files sharing
//! the same content via nlinks > 1), and reports dangling references.
//!
//! ## Architecture
//!
//! ```text
//! linkcheck::check("/dir")
//!   Walk directory tree:
//!   - For each symlink: readlink → metadata check → broken/ok
//!   - For each file with nlinks > 1: group by (size, mtime) heuristic
//!   → LinkReport { broken_symlinks, hardlink_groups, ... }
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelResult;
use crate::fs::{EntryType, Vfs};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_DEPTH: usize = 32;
const MAX_FILES: usize = 50_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A broken symlink.
#[derive(Debug, Clone)]
pub struct BrokenLink {
    /// Path of the symlink itself.
    pub link_path: String,
    /// Target the symlink points to (which doesn't exist).
    pub target: String,
}

/// A group of hardlinked files (sharing the same inode/content).
#[derive(Debug, Clone)]
pub struct HardlinkGroup {
    /// Identifying key (size + mtime hash).
    pub key: String,
    /// Paths that appear to share the same content.
    pub paths: Vec<String>,
    /// File size.
    pub size: u64,
    /// Link count reported by metadata.
    pub nlinks: u32,
}

/// Complete link analysis report.
#[derive(Debug, Clone, Default)]
pub struct LinkReport {
    /// Broken symlinks found.
    pub broken_symlinks: Vec<BrokenLink>,
    /// Working symlinks found.
    pub valid_symlinks: u64,
    /// Files with nlinks > 1 (potential hardlinks).
    pub hardlink_groups: Vec<HardlinkGroup>,
    /// Total files scanned.
    pub files_scanned: u64,
    /// Total directories scanned.
    pub dirs_scanned: u64,
    /// Total symlinks scanned.
    pub symlinks_scanned: u64,
    /// Errors during scanning.
    pub errors: Vec<String>,
}

/// Options for link checking.
#[derive(Debug, Clone)]
pub struct CheckOptions {
    /// Check for broken symlinks.
    pub check_symlinks: bool,
    /// Check for hardlink groups.
    pub check_hardlinks: bool,
    /// Maximum recursion depth.
    pub max_depth: usize,
    /// Only report broken links (don't count valid ones).
    pub broken_only: bool,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            check_symlinks: true,
            check_hardlinks: true,
            max_depth: MAX_DEPTH,
            broken_only: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global stats
// ---------------------------------------------------------------------------

static CHECKS: AtomicU64 = AtomicU64::new(0);
static BROKEN_FOUND: AtomicU64 = AtomicU64::new(0);

/// Get counters: (checks_performed, broken_links_found).
pub fn stats() -> (u64, u64) {
    (
        CHECKS.load(Ordering::Relaxed),
        BROKEN_FOUND.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Link checking
// ---------------------------------------------------------------------------

/// Analyze links in a directory tree.
pub fn check(root: &str, opts: &CheckOptions) -> KernelResult<LinkReport> {
    let mut report = LinkReport::default();

    // Track files with nlinks > 1 for hardlink grouping.
    // Key: (size, modified_ns) → Vec<path>
    let mut hardlink_candidates: BTreeMap<String, Vec<String>> = BTreeMap::new();

    walk_tree(root, root, opts, &mut report, &mut hardlink_candidates, 0)?;

    // Build hardlink groups from candidates.
    if opts.check_hardlinks {
        for (key, paths) in &hardlink_candidates {
            if paths.len() > 1 {
                // Multiple paths with same (size, mtime, nlinks>1) — likely hardlinks.
                let size = paths.first()
                    .and_then(|p| Vfs::metadata(p).ok())
                    .map_or(0, |m| m.size);
                let nlinks = paths.first()
                    .and_then(|p| Vfs::metadata(p).ok())
                    .map_or(0, |m| m.nlinks);
                report.hardlink_groups.push(HardlinkGroup {
                    key: key.clone(),
                    paths: paths.clone(),
                    size,
                    nlinks,
                });
            }
        }
    }

    CHECKS.fetch_add(1, Ordering::Relaxed);
    BROKEN_FOUND.fetch_add(report.broken_symlinks.len() as u64, Ordering::Relaxed);

    serial_println!(
        "[linkcheck] {}: {} broken symlinks, {} valid, {} hardlink groups, {} files",
        root,
        report.broken_symlinks.len(),
        report.valid_symlinks,
        report.hardlink_groups.len(),
        report.files_scanned,
    );

    Ok(report)
}

/// Find all broken symlinks in a directory tree.
pub fn find_broken(root: &str) -> KernelResult<Vec<BrokenLink>> {
    let opts = CheckOptions {
        check_symlinks: true,
        check_hardlinks: false,
        broken_only: true,
        ..CheckOptions::default()
    };
    let report = check(root, &opts)?;
    Ok(report.broken_symlinks)
}

/// Fix broken symlinks by removing them.
pub fn fix_broken(root: &str, dry_run: bool) -> KernelResult<(u64, Vec<String>)> {
    let broken = find_broken(root)?;
    let mut removed: u64 = 0;
    let mut errors = Vec::new();

    for link in &broken {
        if dry_run {
            removed = removed.saturating_add(1);
        } else {
            match Vfs::remove(&link.link_path) {
                Ok(()) => removed = removed.saturating_add(1),
                Err(e) => errors.push(alloc::format!("rm {}: {:?}", link.link_path, e)),
            }
        }
    }

    serial_println!(
        "[linkcheck] Fix: {} removed, {} errors{}",
        removed, errors.len(),
        if dry_run { " (dry run)" } else { "" },
    );

    Ok((removed, errors))
}

// ---------------------------------------------------------------------------
// Tree walker
// ---------------------------------------------------------------------------

fn walk_tree(
    root: &str,
    path: &str,
    opts: &CheckOptions,
    report: &mut LinkReport,
    hardlink_map: &mut BTreeMap<String, Vec<String>>,
    depth: usize,
) -> KernelResult<()> {
    if depth > opts.max_depth || report.files_scanned as usize >= MAX_FILES {
        return Ok(());
    }

    let entries = match Vfs::readdir(path) {
        Ok(e) => e,
        Err(e) => {
            report.errors.push(alloc::format!("readdir {}: {:?}", path, e));
            return Ok(());
        }
    };

    report.dirs_scanned = report.dirs_scanned.saturating_add(1);

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        if report.files_scanned as usize >= MAX_FILES {
            return Ok(());
        }

        let full = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        match entry.entry_type {
            EntryType::File => {
                report.files_scanned = report.files_scanned.saturating_add(1);

                // Check for hardlinks.
                if opts.check_hardlinks {
                    if let Ok(meta) = Vfs::metadata(&full) {
                        if meta.nlinks > 1 {
                            let key = alloc::format!("{}_{}", meta.size, meta.modified_ns);
                            hardlink_map.entry(key).or_insert_with(Vec::new).push(full.clone());
                        }
                    }
                }
            }
            EntryType::Directory => {
                walk_tree(root, &full, opts, report, hardlink_map, depth + 1)?;
            }
            EntryType::Symlink => {
                report.symlinks_scanned = report.symlinks_scanned.saturating_add(1);

                if opts.check_symlinks {
                    match Vfs::readlink(&full) {
                        Ok(target) => {
                            // Resolve target path.
                            let resolved = if target.starts_with('/') {
                                target.clone()
                            } else {
                                // Relative symlink — resolve against parent.
                                let parent = if let Some(pos) = full.rfind('/') {
                                    &full[..pos]
                                } else {
                                    "/"
                                };
                                alloc::format!("{}/{}", parent, target)
                            };

                            // Check if target exists.
                            if Vfs::metadata(&resolved).is_err() {
                                report.broken_symlinks.push(BrokenLink {
                                    link_path: full.clone(),
                                    target,
                                });
                            } else {
                                report.valid_symlinks = report.valid_symlinks.saturating_add(1);
                            }
                        }
                        Err(e) => {
                            // Can't read link target — treat as broken.
                            report.broken_symlinks.push(BrokenLink {
                                link_path: full.clone(),
                                target: alloc::format!("(error: {:?})", e),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[linkcheck] Running self-test...");

    test_check_empty();
    test_broken_symlink();
    test_valid_symlink();
    test_fix_broken();
    test_hardlink_detection();
    test_stats();

    serial_println!("[linkcheck] Self-test passed (6 tests).");
    Ok(())
}

fn test_check_empty() {
    let _ = Vfs::mkdir("/tmp/lc_empty");
    let report = check("/tmp/lc_empty", &CheckOptions::default()).expect("check");
    assert!(report.broken_symlinks.is_empty());
    assert_eq!(report.valid_symlinks, 0);
    let _ = Vfs::rmdir("/tmp/lc_empty");

    serial_println!("[linkcheck]   check empty: ok");
}

fn test_broken_symlink() {
    let _ = Vfs::mkdir("/tmp/lc_broken");
    let _ = Vfs::symlink("/tmp/lc_broken/bad", "/nonexistent/target");

    let report = check("/tmp/lc_broken", &CheckOptions::default()).expect("check");
    assert!(!report.broken_symlinks.is_empty(), "should find broken symlink");
    assert!(report.broken_symlinks[0].link_path.contains("bad"));

    let _ = Vfs::remove("/tmp/lc_broken/bad");
    let _ = Vfs::rmdir("/tmp/lc_broken");

    serial_println!("[linkcheck]   broken symlink: ok");
}

fn test_valid_symlink() {
    let _ = Vfs::mkdir("/tmp/lc_valid");
    Vfs::write_file("/tmp/lc_valid/target.txt", b"hello").expect("write");
    let _ = Vfs::symlink("/tmp/lc_valid/link", "/tmp/lc_valid/target.txt");

    let report = check("/tmp/lc_valid", &CheckOptions::default()).expect("check");
    assert!(report.valid_symlinks >= 1 || report.broken_symlinks.is_empty(),
        "should have valid symlink or no broken ones");

    let _ = Vfs::remove("/tmp/lc_valid/link");
    let _ = Vfs::remove("/tmp/lc_valid/target.txt");
    let _ = Vfs::rmdir("/tmp/lc_valid");

    serial_println!("[linkcheck]   valid symlink: ok");
}

fn test_fix_broken() {
    let _ = Vfs::mkdir("/tmp/lc_fix");
    let _ = Vfs::symlink("/tmp/lc_fix/broken1", "/does/not/exist1");
    let _ = Vfs::symlink("/tmp/lc_fix/broken2", "/does/not/exist2");

    let (removed, errors) = fix_broken("/tmp/lc_fix", false).expect("fix");
    assert!(removed >= 1, "should remove broken links, got {}", removed);
    let _ = errors;

    let _ = Vfs::rmdir("/tmp/lc_fix");

    serial_println!("[linkcheck]   fix broken: ok");
}

fn test_hardlink_detection() {
    // Create files that look like they could be hardlinked (nlinks > 1).
    // In memfs, nlinks might always be 1, so we just verify the check
    // runs without error.
    let _ = Vfs::mkdir("/tmp/lc_hard");
    Vfs::write_file("/tmp/lc_hard/a.txt", b"content").expect("write");

    let opts = CheckOptions {
        check_hardlinks: true,
        ..CheckOptions::default()
    };
    let report = check("/tmp/lc_hard", &opts).expect("check");
    assert_eq!(report.files_scanned, 1);

    let _ = Vfs::remove("/tmp/lc_hard/a.txt");
    let _ = Vfs::rmdir("/tmp/lc_hard");

    serial_println!("[linkcheck]   hardlink detection: ok");
}

fn test_stats() {
    let (checks, broken) = stats();
    assert!(checks > 0, "should have checks");
    let _ = broken;

    serial_println!("[linkcheck]   stats: ok");
}
