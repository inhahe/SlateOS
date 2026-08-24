//! Overlay filesystem (overlayfs) — layered filesystem composition.
//!
//! Merges a read-only **lower** layer with a read-write **upper** layer,
//! presenting a unified view at the merge point.  All writes go to the
//! upper layer (copy-up semantics); the lower layer is never modified.
//!
//! ## Architecture
//!
//! ```text
//! merged view (/overlay/NAME)
//!        ↓
//!  OverlayFs engine
//!    ├── upper layer (rw)  — e.g. /tmp/overlay/NAME/upper
//!    └── lower layer (ro)  — e.g. /mnt/root (the real filesystem)
//! ```
//!
//! ## Whiteout semantics
//!
//! When a file that exists only in the lower layer is deleted in the
//! merged view, a **whiteout** entry is recorded.  Whiteouts make the
//! lower-layer entry invisible in the merged view without modifying
//! the lower layer.
//!
//! ## Copy-up
//!
//! When a lower-layer file is modified, its content is first copied
//! to the upper layer ("copy-up").  Subsequent operations use the
//! upper-layer copy.  The lower-layer original is unchanged.
//!
//! ## Opaque directories
//!
//! When a directory from the lower layer is removed and then recreated
//! in the merged view, the new directory is marked **opaque**.  An
//! opaque directory hides all lower-layer contents beneath it.
//!
//! ## Use cases
//!
//! - Safe system updates (read-only base + writable delta)
//! - Container/sandbox isolation (shared base image + per-container writes)
//! - Development environments (shared source tree + per-branch modifications)
//! - Live boot (read-only media + RAM overlay)
//!
//! ## Limitations
//!
//! This module provides a standalone overlay engine.  Full VFS-level
//! transparency (automatic routing through overlays during normal
//! path resolution) requires VFS integration that can be added later.
//! For now, the overlay API can be called directly or through the
//! kshell `overlay` command.

#![allow(dead_code)]

use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::pathutil::path_in_subtree;
use crate::fs::vfs::Vfs;
use crate::fs::{DirEntry, EntryType, FileMeta};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for an overlay mount.
pub type OverlayId = u64;

/// An overlay mount merging two directory trees.
#[derive(Debug)]
struct OverlayMount {
    /// Human-readable name for this overlay.
    name: String,
    /// Absolute path to the read-only lower layer.
    lower_path: PathBuf,
    /// Absolute path to the read-write upper layer.
    upper_path: PathBuf,
    /// Paths (relative to the overlay root) that have been whited out.
    /// A whiteout hides the corresponding lower-layer entry.
    whiteouts: BTreeSet<PathBuf>,
    /// Directories (relative to the overlay root) marked as opaque.
    /// An opaque directory hides all lower-layer content beneath it.
    opaque_dirs: BTreeSet<PathBuf>,
    /// Number of read operations through this overlay.
    reads: u64,
    /// Number of write operations (including copy-ups).
    writes: u64,
    /// Number of copy-up operations performed.
    copyups: u64,
    /// Number of whiteout entries created.
    whiteout_count: u64,
}

/// Summary statistics for an overlay.
#[derive(Debug, Clone)]
pub struct OverlayStats {
    pub name: String,
    pub lower_path: PathBuf,
    pub upper_path: PathBuf,
    pub whiteout_count: usize,
    pub opaque_dir_count: usize,
    pub reads: u64,
    pub writes: u64,
    pub copyups: u64,
}

/// Describes which layer a resolved path comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Entry exists in the upper (writable) layer.
    Upper,
    /// Entry exists only in the lower (read-only) layer.
    Lower,
    /// Entry exists in both layers (upper takes precedence).
    Both,
    /// Entry does not exist or is whited out.
    None,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct OverlayInner {
    /// Active overlay mounts, keyed by ID.
    mounts: BTreeMap<OverlayId, OverlayMount>,
    /// Next ID to assign.
    next_id: OverlayId,
}

static OVERLAYS: Mutex<OverlayInner> = Mutex::new(OverlayInner {
    mounts: BTreeMap::new(),
    next_id: 1,
});

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Join an overlay layer path with an overlay-relative sub-path.
///
/// `rel` must already have been through [`normalize_rel`], so it carries no
/// leading separator.  That matters: `PathBuf::push` lets an absolute argument
/// replace the whole buffer, which for an unnormalized `rel` would silently
/// escape the layer root and address the real filesystem instead.
fn layer_join(layer_path: &Path, rel: &Path) -> PathBuf {
    let mut out = layer_path.to_path_buf();
    for component in rel.components() {
        out.push(component);
    }
    out
}

/// Normalize an overlay-relative path for whiteout/opaque lookups.
///
/// Rebuilding from `components()` drops leading, trailing and repeated
/// separators in one pass, so every key stored in the whiteout/opaque sets
/// and every path handed to [`layer_join`] has a single canonical spelling.
/// The result is always relative.
fn normalize_rel(rel: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in rel.components() {
        out.push(component);
    }
    out
}

// ---------------------------------------------------------------------------
// Public API — lifecycle
// ---------------------------------------------------------------------------

/// Create a new overlay mount.
///
/// - `name`: human-readable label
/// - `lower_path`: absolute path to the read-only lower directory
/// - `upper_path`: absolute path to the read-write upper directory
///
/// Both paths must exist and be directories.  Returns the overlay ID.
pub fn create(
    name: &str,
    lower_path: impl AsRef<Path>,
    upper_path: impl AsRef<Path>,
) -> KernelResult<OverlayId> {
    let (lower_path, upper_path) = (lower_path.as_ref(), upper_path.as_ref());
    // Validate paths exist and are directories.
    if !Vfs::is_directory(lower_path) {
        return Err(KernelError::NotADirectory);
    }
    if !Vfs::is_directory(upper_path) {
        return Err(KernelError::NotADirectory);
    }

    // Prevent nesting (lower inside upper or vice versa).  The check is
    // component-aligned: a raw byte-prefix test would reject the perfectly
    // legal pairing of `/tmp/upper` with `/tmp/upper2`, and — worse, once
    // either path picks up a trailing slash — would fail to reject a genuine
    // nesting.
    if path_in_subtree(lower_path, upper_path) || path_in_subtree(upper_path, lower_path) {
        return Err(KernelError::InvalidArgument);
    }

    let mut inner = OVERLAYS.lock();

    // Check for duplicate name.
    for mount in inner.mounts.values() {
        if mount.name == name {
            return Err(KernelError::AlreadyExists);
        }
    }

    let id = inner.next_id;
    inner.next_id = inner.next_id.wrapping_add(1);

    inner.mounts.insert(
        id,
        OverlayMount {
            name: name.into(),
            lower_path: lower_path.to_path_buf(),
            upper_path: upper_path.to_path_buf(),
            whiteouts: BTreeSet::new(),
            opaque_dirs: BTreeSet::new(),
            reads: 0,
            writes: 0,
            copyups: 0,
            whiteout_count: 0,
        },
    );

    Ok(id)
}

/// Destroy an overlay mount.  Does not delete upper-layer files.
pub fn destroy(id: OverlayId) -> KernelResult<()> {
    let mut inner = OVERLAYS.lock();
    if inner.mounts.remove(&id).is_some() {
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Look up an overlay by name.
pub fn find_by_name(name: &str) -> Option<OverlayId> {
    let inner = OVERLAYS.lock();
    for (&id, mount) in &inner.mounts {
        if mount.name == name {
            return Some(id);
        }
    }
    None
}

/// List all active overlays.
pub fn list() -> Vec<(OverlayId, OverlayStats)> {
    let inner = OVERLAYS.lock();
    inner
        .mounts
        .iter()
        .map(|(&id, m)| {
            (
                id,
                OverlayStats {
                    name: m.name.clone(),
                    lower_path: m.lower_path.clone(),
                    upper_path: m.upper_path.clone(),
                    whiteout_count: m.whiteouts.len(),
                    opaque_dir_count: m.opaque_dirs.len(),
                    reads: m.reads,
                    writes: m.writes,
                    copyups: m.copyups,
                },
            )
        })
        .collect()
}

/// Get stats for a single overlay.
pub fn stats(id: OverlayId) -> KernelResult<OverlayStats> {
    let inner = OVERLAYS.lock();
    let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
    Ok(OverlayStats {
        name: m.name.clone(),
        lower_path: m.lower_path.clone(),
        upper_path: m.upper_path.clone(),
        whiteout_count: m.whiteouts.len(),
        opaque_dir_count: m.opaque_dirs.len(),
        reads: m.reads,
        writes: m.writes,
        copyups: m.copyups,
    })
}

/// Return the absolute upper-layer (read-write) path for `id`.
///
/// Used by callers that need to enumerate the container's scratch layer
/// directly — e.g. `container diff`, which walks the upper layer to report
/// added/changed files relative to the read-only image.
pub fn upper_path(id: OverlayId) -> KernelResult<PathBuf> {
    let inner = OVERLAYS.lock();
    let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
    Ok(m.upper_path.clone())
}

/// Return the overlay's whiteout entries — normalized relative paths (no
/// leading slash) that are hidden from the merged view because they were
/// deleted after being present in the lower layer.
///
/// The result is sorted (the backing set is a `BTreeSet`). Used by
/// `container diff` to report `D` (deleted) entries.
pub fn whiteouts(id: OverlayId) -> KernelResult<Vec<PathBuf>> {
    let inner = OVERLAYS.lock();
    let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
    Ok(m.whiteouts.iter().cloned().collect())
}

// ---------------------------------------------------------------------------
// Public API — resolution
// ---------------------------------------------------------------------------

/// Determine which layer a path exists in.
///
/// `rel_path` is relative to the overlay root (no leading slash needed).
pub fn which_layer(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<Layer> {
    let rel_path = rel_path.as_ref();

    // Everything that consults the overlay's own tables -- whiteouts, opaque
    // ancestors, the layer roots -- happens under the lock; the two existence
    // probes happen after it is released. `Vfs::exists` resolves through the
    // filesystem's lock, and the VFS holds that lock across content
    // generation, which for procfs reaches back into arbitrary module-global
    // state: `OVERLAYS -> filesystem lock` inverts the live order and
    // deadlocks. `scripts/check-vfs-under-lock.py` enforces this.
    //
    // The tables are therefore read at one instant and the disk at a slightly
    // later one, so a whiteout added in between is missed by this call. That
    // race already existed against the filesystem itself -- a file can be
    // deleted immediately after `exists` returns true -- and the answer is a
    // snapshot either way.
    let (upper_full, lower_full) = {
        let inner = OVERLAYS.lock();
        let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        // Check whiteout first — if whited out, it doesn't exist.
        if m.whiteouts.contains(&rel) {
            return Ok(Layer::None);
        }

        // Check if an ancestor directory is opaque — hides lower-layer content.
        let lower_hidden = is_opaque_ancestor(&m.opaque_dirs, &rel);

        (
            layer_join(&m.upper_path, &rel),
            // `None` means the lower layer is hidden and must not be probed at
            // all, which is not the same as probing it and finding nothing.
            if lower_hidden {
                None
            } else {
                Some(layer_join(&m.lower_path, &rel))
            },
        )
    };

    let in_upper = Vfs::exists(&upper_full);
    let in_lower = lower_full.is_some_and(|lower| Vfs::exists(&lower));

    Ok(match (in_upper, in_lower) {
        (true, true) => Layer::Both,
        (true, false) => Layer::Upper,
        (false, true) => Layer::Lower,
        (false, false) => Layer::None,
    })
}

/// Check whether `rel` itself or any ancestor of it is marked opaque.
fn is_opaque_ancestor(opaque_dirs: &BTreeSet<PathBuf>, rel: &Path) -> bool {
    // Walk the components, testing each successive ancestor prefix.
    let mut prefix = PathBuf::new();
    for component in rel.components() {
        prefix.push(component);
        if opaque_dirs.contains(&prefix) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Public API — file operations
// ---------------------------------------------------------------------------

/// Read a file through the overlay.
///
/// Returns data from the upper layer if the file exists there,
/// otherwise from the lower layer.
pub fn read_file(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<Vec<u8>> {
    let rel_path = rel_path.as_ref();
    let (upper_full, lower_full, lower_hidden) = {
        let mut inner = OVERLAYS.lock();
        let m = inner.mounts.get_mut(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        if m.whiteouts.contains(&rel) {
            return Err(KernelError::NotFound);
        }

        m.reads = m.reads.saturating_add(1);
        let hidden = is_opaque_ancestor(&m.opaque_dirs, &rel);
        (
            layer_join(&m.upper_path, &rel),
            layer_join(&m.lower_path, &rel),
            hidden,
        )
    };

    // Try upper first.
    if let Ok(data) = Vfs::read_file(&upper_full) {
        return Ok(data);
    }

    // Fall through to lower if not hidden.
    if lower_hidden {
        return Err(KernelError::NotFound);
    }

    Vfs::read_file(&lower_full)
}

/// Write a file through the overlay.
///
/// If the file exists only in the lower layer, a copy-up is performed
/// first (parent directories are created in upper as needed).  The
/// write always goes to the upper layer.
pub fn write_file(id: OverlayId, rel_path: impl AsRef<Path>, data: &[u8]) -> KernelResult<()> {
    let rel_path = rel_path.as_ref();
    let (upper_full, needs_copyup) = {
        let mut inner = OVERLAYS.lock();
        let m = inner.mounts.get_mut(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        // Remove whiteout if present (file is being recreated).
        m.whiteouts.remove(&rel);
        m.writes = m.writes.saturating_add(1);

        let upper_p = layer_join(&m.upper_path, &rel);
        let in_upper = Vfs::exists(&upper_p);
        (upper_p, !in_upper)
    };

    if needs_copyup {
        // Ensure parent directories exist in upper.
        ensure_upper_parents(id, rel_path)?;
    }

    Vfs::write_file(&upper_full, data)
}

/// Stat a path through the overlay.
///
/// Returns metadata from the upper layer if present, otherwise lower.
pub fn stat(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<DirEntry> {
    let rel_path = rel_path.as_ref();
    let (upper_full, lower_full, lower_hidden) = {
        let inner = OVERLAYS.lock();
        let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        if m.whiteouts.contains(&rel) {
            return Err(KernelError::NotFound);
        }

        let hidden = is_opaque_ancestor(&m.opaque_dirs, &rel);
        (
            layer_join(&m.upper_path, &rel),
            layer_join(&m.lower_path, &rel),
            hidden,
        )
    };

    // Try upper first.
    if let Ok(entry) = Vfs::stat(&upper_full) {
        return Ok(entry);
    }

    if lower_hidden {
        return Err(KernelError::NotFound);
    }

    Vfs::stat(&lower_full)
}

/// Get full metadata through the overlay.
pub fn metadata(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<FileMeta> {
    let rel_path = rel_path.as_ref();
    let (upper_full, lower_full, lower_hidden) = {
        let inner = OVERLAYS.lock();
        let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        if m.whiteouts.contains(&rel) {
            return Err(KernelError::NotFound);
        }

        let hidden = is_opaque_ancestor(&m.opaque_dirs, &rel);
        (
            layer_join(&m.upper_path, &rel),
            layer_join(&m.lower_path, &rel),
            hidden,
        )
    };

    if let Ok(meta) = Vfs::metadata(&upper_full) {
        return Ok(meta);
    }

    if lower_hidden {
        return Err(KernelError::NotFound);
    }

    Vfs::metadata(&lower_full)
}

/// Remove a file or empty directory through the overlay.
///
/// If the entry exists in the lower layer, a whiteout is added.
/// If it exists only in the upper layer, it is removed directly.
pub fn remove(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<()> {
    let rel_path = rel_path.as_ref();
    let (upper_full, lower_full, lower_hidden) = {
        let inner = OVERLAYS.lock();
        let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        if m.whiteouts.contains(&rel) {
            return Err(KernelError::NotFound);
        }

        let hidden = is_opaque_ancestor(&m.opaque_dirs, &rel);
        (
            layer_join(&m.upper_path, &rel),
            layer_join(&m.lower_path, &rel),
            hidden,
        )
    };

    let in_upper = Vfs::exists(&upper_full);
    let in_lower = if lower_hidden {
        false
    } else {
        Vfs::exists(&lower_full)
    };

    if !in_upper && !in_lower {
        return Err(KernelError::NotFound);
    }

    // Remove from upper if present.
    if in_upper {
        // Check if it's a directory or file.
        if Vfs::is_directory(&upper_full) {
            Vfs::rmdir(&upper_full)?;
        } else {
            Vfs::remove(&upper_full)?;
        }
    }

    // If it was in the lower layer, add a whiteout.
    if in_lower {
        let mut inner = OVERLAYS.lock();
        if let Some(m) = inner.mounts.get_mut(&id) {
            let rel = normalize_rel(rel_path);
            m.whiteouts.insert(rel);
            m.whiteout_count = m.whiteout_count.saturating_add(1);
        }
    }

    Ok(())
}

/// Create a directory through the overlay.
pub fn mkdir(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<()> {
    let rel_path = rel_path.as_ref();
    let upper_full = {
        let mut inner = OVERLAYS.lock();
        let m = inner.mounts.get_mut(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        // Remove whiteout if present.
        m.whiteouts.remove(&rel);
        m.writes = m.writes.saturating_add(1);

        layer_join(&m.upper_path, &rel)
    };

    // Ensure parent directories exist in upper.
    ensure_upper_parents(id, rel_path)?;

    Vfs::mkdir(&upper_full)
}

/// Remove a directory through the overlay.
///
/// If the directory has lower-layer content, it becomes opaque.
pub fn rmdir(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<()> {
    let rel_path = rel_path.as_ref();
    let (upper_full, lower_full, lower_hidden) = {
        let inner = OVERLAYS.lock();
        let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        if m.whiteouts.contains(&rel) {
            return Err(KernelError::NotFound);
        }

        let hidden = is_opaque_ancestor(&m.opaque_dirs, &rel);
        (
            layer_join(&m.upper_path, &rel),
            layer_join(&m.lower_path, &rel),
            hidden,
        )
    };

    let in_upper = Vfs::is_directory(&upper_full);
    let in_lower = if lower_hidden {
        false
    } else {
        Vfs::is_directory(&lower_full)
    };

    if !in_upper && !in_lower {
        return Err(KernelError::NotFound);
    }

    // Remove from upper if present.
    if in_upper {
        Vfs::rmdir(&upper_full)?;
    }

    // Add whiteout for lower content.
    if in_lower {
        let mut inner = OVERLAYS.lock();
        if let Some(m) = inner.mounts.get_mut(&id) {
            let rel = normalize_rel(rel_path);
            m.whiteouts.insert(rel.clone());
            m.whiteout_count = m.whiteout_count.saturating_add(1);
            // Also mark as opaque so recreating the dir doesn't
            // resurface lower-layer children.
            m.opaque_dirs.insert(rel);
        }
    }

    Ok(())
}

/// Read directory entries through the overlay.
///
/// Merges entries from upper and lower layers, excluding whiteouts.
/// Upper-layer entries take precedence over lower-layer entries with
/// the same name.
pub fn readdir(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<Vec<DirEntry>> {
    let rel_path = rel_path.as_ref();
    let (upper_full, lower_full, lower_hidden, whiteouts_snapshot) = {
        let mut inner = OVERLAYS.lock();
        let m = inner.mounts.get_mut(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);

        // Collect the whiteouts that name a *direct child* of this directory,
        // reduced to the bare child name so they can be matched against the
        // names `Vfs::readdir` returns for the lower layer.  Component-aligned
        // stripping is what keeps `d2/x` out of the listing of `d`.
        let wo: BTreeSet<PathBuf> = m
            .whiteouts
            .iter()
            .filter_map(|w| w.strip_prefix(&rel))
            .filter(|child| child.components().count() == 1)
            .collect();

        m.reads = m.reads.saturating_add(1);

        let hidden = is_opaque_ancestor(&m.opaque_dirs, &rel) || m.opaque_dirs.contains(&rel);

        (
            layer_join(&m.upper_path, &rel),
            layer_join(&m.lower_path, &rel),
            hidden,
            wo,
        )
    };

    // Collect upper entries.
    let mut merged: BTreeMap<PathBuf, DirEntry> = BTreeMap::new();

    if let Ok(entries) = Vfs::readdir(&upper_full) {
        for e in entries {
            merged.insert(e.name.clone(), e);
        }
    }

    // Merge lower entries (if not hidden by opaque dir).
    if !lower_hidden {
        if let Ok(entries) = Vfs::readdir(&lower_full) {
            for e in entries {
                // Skip whiteouts.
                if whiteouts_snapshot.contains(&e.name) {
                    continue;
                }
                // Upper takes precedence — only insert if not already present.
                merged.entry(e.name.clone()).or_insert(e);
            }
        }
    }

    Ok(merged.into_values().collect())
}

/// Rename a file or directory through the overlay.
///
/// Copy-up from lower if needed, then rename within upper.
pub fn rename(
    id: OverlayId,
    from_rel: impl AsRef<Path>,
    to_rel: impl AsRef<Path>,
) -> KernelResult<()> {
    let (from_rel, to_rel) = (from_rel.as_ref(), to_rel.as_ref());
    let layer = which_layer(id, from_rel)?;

    match layer {
        Layer::None => return Err(KernelError::NotFound),
        Layer::Lower => {
            // Copy-up the source first.
            copy_up(id, from_rel)?;
        }
        Layer::Upper | Layer::Both => {
            // Already in upper, can rename directly.
        }
    }

    let (upper_from, upper_to) = {
        let mut inner = OVERLAYS.lock();
        let m = inner.mounts.get_mut(&id).ok_or(KernelError::NotFound)?;
        let from = normalize_rel(from_rel);
        let to = normalize_rel(to_rel);

        m.writes = m.writes.saturating_add(1);

        // Remove whiteout on target if exists.
        m.whiteouts.remove(&to);

        // Add whiteout on source if it was in lower.
        if layer == Layer::Lower || layer == Layer::Both {
            m.whiteouts.insert(from.clone());
            m.whiteout_count = m.whiteout_count.saturating_add(1);
        }

        (
            layer_join(&m.upper_path, &from),
            layer_join(&m.upper_path, &to),
        )
    };

    // Ensure parent directories for destination.
    ensure_upper_parents(id, to_rel)?;

    Vfs::rename(&upper_from, &upper_to)
}

/// Check if a path exists in the merged view.
pub fn exists(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<bool> {
    let layer = which_layer(id, rel_path)?;
    Ok(layer != Layer::None)
}

/// Copy a file from the lower layer to the upper layer.
///
/// This is the "copy-up" operation.  If the file is already in the
/// upper layer, this is a no-op.
pub fn copy_up(id: OverlayId, rel_path: impl AsRef<Path>) -> KernelResult<()> {
    let rel_path = rel_path.as_ref();
    // Take the two layer paths and get out of the lock -- the rest of this
    // function is nothing but VFS calls, which `OVERLAYS` may not be held
    // across (see `which_layer`).
    let (upper_full, lower_full) = {
        let inner = OVERLAYS.lock();
        let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
        let rel = normalize_rel(rel_path);
        (
            layer_join(&m.upper_path, &rel),
            layer_join(&m.lower_path, &rel),
        )
    };

    // Already in upper? No-op.
    if Vfs::exists(&upper_full) {
        return Ok(());
    }

    // Count the copy-up now, as the original did, rather than on success: the
    // statistic is "how many copy-ups were attempted against this overlay",
    // and a failed read below leaves the upper layer untouched anyway. The
    // mount is looked up again by id because the lock was released in between.
    {
        let mut inner = OVERLAYS.lock();
        let m = inner.mounts.get_mut(&id).ok_or(KernelError::NotFound)?;
        m.copyups = m.copyups.saturating_add(1);
    }

    // Read from lower.
    let data = Vfs::read_file(&lower_full)?;

    // Ensure parent directories in upper.
    ensure_upper_parents(id, rel_path)?;

    // Write to upper.
    Vfs::write_file(&upper_full, &data)?;

    // Copy metadata if possible.
    if let Ok(meta) = Vfs::metadata(&lower_full) {
        // Best-effort metadata copy — don't fail the copy-up if metadata
        // can't be set (e.g., filesystem doesn't support it).
        let _ = Vfs::set_permissions(&upper_full, meta.permissions);
        let _ = Vfs::set_owner(&upper_full, meta.uid, meta.gid);
    }

    Ok(())
}

/// Discard all upper-layer changes and whiteouts for this overlay.
///
/// This effectively "resets" the overlay to show only the lower layer.
/// **Warning**: this deletes all files in the upper directory.
pub fn reset(id: OverlayId) -> KernelResult<u64> {
    let upper_path = {
        let mut inner = OVERLAYS.lock();
        let m = inner.mounts.get_mut(&id).ok_or(KernelError::NotFound)?;

        // Clear overlay metadata.
        m.whiteouts.clear();
        m.opaque_dirs.clear();
        m.whiteout_count = 0;
        m.copyups = 0;

        m.upper_path.clone()
    };

    // Remove all content in the upper directory (but keep the dir itself).
    let entries = Vfs::readdir(&upper_path).unwrap_or_default();
    let mut removed = 0u64;
    for entry in &entries {
        let full = upper_path.join(&entry.name);
        if entry.entry_type == EntryType::Directory {
            if let Ok(count) = Vfs::remove_recursive(&full) {
                removed = removed.saturating_add(count);
            }
        } else if Vfs::remove(&full).is_ok() {
            removed = removed.saturating_add(1);
        }
    }

    Ok(removed)
}

/// Commit upper-layer changes by merging them into the lower layer.
///
/// This copies all upper-layer files to the lower layer, applies
/// deletions (removes whited-out files from lower), and then resets
/// the overlay.
///
/// **Warning**: this modifies the lower layer, which is normally
/// read-only.  Use with caution.
pub fn commit(id: OverlayId) -> KernelResult<u64> {
    let (upper_path, lower_path, whiteouts) = {
        let inner = OVERLAYS.lock();
        let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
        (
            m.upper_path.clone(),
            m.lower_path.clone(),
            m.whiteouts.iter().cloned().collect::<Vec<_>>(),
        )
    };

    let mut applied = 0u64;

    // Apply whiteouts (delete from lower).
    for rel in &whiteouts {
        let lower_full = layer_join(&lower_path, rel);
        if Vfs::is_directory(&lower_full) {
            if Vfs::remove_recursive(&lower_full).is_ok() {
                applied = applied.saturating_add(1);
            }
        } else if Vfs::remove(&lower_full).is_ok() {
            applied = applied.saturating_add(1);
        }
    }

    // Copy upper-layer files to lower.
    applied = applied.saturating_add(merge_dir_to_lower(&upper_path, &lower_path, Path::new(""))?);

    // Reset the overlay.
    reset(id)?;

    Ok(applied)
}

/// Recursively merge upper directory contents into lower.
fn merge_dir_to_lower(upper_base: &Path, lower_base: &Path, rel: &Path) -> KernelResult<u64> {
    let upper_dir = layer_join(upper_base, rel);
    let entries = Vfs::readdir(&upper_dir)?;
    let mut count = 0u64;

    for entry in &entries {
        // `Path::join` on an empty `rel` yields the bare name, so the two
        // cases the old `format!` distinguished collapse into one.
        let child_rel = rel.join(&entry.name);

        let lower_full = layer_join(lower_base, &child_rel);
        let upper_full = layer_join(upper_base, &child_rel);

        if entry.entry_type == EntryType::Directory {
            // Ensure directory exists in lower.
            if !Vfs::is_directory(&lower_full) {
                let _ = Vfs::mkdir(&lower_full);
            }
            count = count.saturating_add(merge_dir_to_lower(upper_base, lower_base, &child_rel)?);
        } else {
            // Copy file content.
            if let Ok(data) = Vfs::read_file(&upper_full) {
                Vfs::write_file(&lower_full, &data)?;
                count = count.saturating_add(1);
            }
        }
    }

    Ok(count)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Ensure all parent directories for `rel_path` exist in the upper layer.
fn ensure_upper_parents(id: OverlayId, rel_path: &Path) -> KernelResult<()> {
    let upper_path = {
        let inner = OVERLAYS.lock();
        let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
        m.upper_path.clone()
    };

    let rel = normalize_rel(rel_path);
    let parts: Vec<&Path> = rel.components().collect();

    // Create each parent component (skip the last one which is the file/dir itself).
    let mut prefix = PathBuf::new();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        prefix.push(part);

        let full = layer_join(&upper_path, &prefix);
        if !Vfs::is_directory(&full) {
            Vfs::mkdir(&full)?;
        }
    }

    Ok(())
}

/// List all whiteout entries for an overlay.
pub fn list_whiteouts(id: OverlayId) -> KernelResult<Vec<PathBuf>> {
    let inner = OVERLAYS.lock();
    let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
    Ok(m.whiteouts.iter().cloned().collect())
}

/// List all opaque directories for an overlay.
pub fn list_opaque_dirs(id: OverlayId) -> KernelResult<Vec<PathBuf>> {
    let inner = OVERLAYS.lock();
    let m = inner.mounts.get(&id).ok_or(KernelError::NotFound)?;
    Ok(m.opaque_dirs.iter().cloned().collect())
}

/// Get the number of active overlays.
pub fn count() -> usize {
    OVERLAYS.lock().mounts.len()
}

// ---------------------------------------------------------------------------
// VFS adapter — mount an overlay into the path tree
// ---------------------------------------------------------------------------

/// A [`FileSystem`](crate::fs::vfs::FileSystem) adapter that exposes an
/// existing overlay (by ID) at a VFS mount point.
///
/// This gives the otherwise ID-addressed overlay engine *path
/// transparency*: once mounted at, say, `/containers/<id>/rootfs`, normal
/// path resolution under that prefix routes through the overlay, so reads
/// see the merged lower+upper view and writes copy-up into the upper layer.
/// This is what a container rootfs jail needs for real copy-on-write
/// isolation (the jail re-anchors absolute paths under the mount point;
/// see `ipc::namespace` and known-issues TD32).
///
/// The adapter holds only the `OverlayId`; the overlay's lifetime is
/// managed independently via [`create`]/[`destroy`].  Unmounting the VFS
/// mount does not destroy the overlay, and vice versa — the caller is
/// responsible for ordering teardown (unmount before destroy).
pub struct OverlayFs {
    id: OverlayId,
}

impl OverlayFs {
    /// Wrap an existing overlay so it can be mounted into the VFS.
    ///
    /// Returns `NotFound` if no overlay with this ID exists.
    pub fn new(id: OverlayId) -> KernelResult<Self> {
        // Validate the overlay exists up front so a bad ID fails at mount
        // time rather than on the first path operation.
        let inner = OVERLAYS.lock();
        if !inner.mounts.contains_key(&id) {
            return Err(KernelError::NotFound);
        }
        drop(inner);
        Ok(Self { id })
    }

    /// The wrapped overlay's ID.
    #[must_use]
    pub fn id(&self) -> OverlayId {
        self.id
    }
}

impl crate::fs::vfs::FileSystem for OverlayFs {
    fn fs_type(&self) -> &'static str {
        "overlay"
    }

    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>> {
        readdir(self.id, path)
    }

    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>> {
        read_file(self.id, path)
    }

    fn stat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        stat(self.id, path)
    }

    fn metadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        metadata(self.id, path)
    }

    fn write_file(&mut self, path: &Path, data: &[u8]) -> KernelResult<()> {
        write_file(self.id, path, data)
    }

    fn remove(&mut self, path: &Path) -> KernelResult<()> {
        remove(self.id, path)
    }

    fn mkdir(&mut self, path: &Path) -> KernelResult<()> {
        mkdir(self.id, path)
    }

    fn rmdir(&mut self, path: &Path) -> KernelResult<()> {
        rmdir(self.id, path)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> KernelResult<()> {
        rename(self.id, from, to)
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for the overlay filesystem module.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[overlay] Running self-test...");

    // Use /tmp as our test ground (memfs, always available).
    let test_base = "/tmp/overlay_test";
    let lower = "/tmp/overlay_test/lower";
    let upper = "/tmp/overlay_test/upper";

    // Clean up from any previous test run.
    let _ = Vfs::remove_recursive(test_base);
    Vfs::mkdir(test_base)?;
    Vfs::mkdir(lower)?;
    Vfs::mkdir(upper)?;

    // Populate lower layer with test files.
    Vfs::write_file(alloc::format!("{}/file_a.txt", lower), b"lower content A")?;
    Vfs::write_file(alloc::format!("{}/file_b.txt", lower), b"lower content B")?;
    Vfs::write_file(alloc::format!("{}/shared.txt", lower), b"from lower")?;
    Vfs::mkdir(alloc::format!("{}/subdir", lower))?;
    Vfs::write_file(alloc::format!("{}/subdir/deep.txt", lower), b"deep file")?;

    // --- Test 1: Create overlay ---
    let id = create("test_overlay", lower, upper)?;
    serial_println!("[overlay]   create: OK (id={})", id);

    // --- Test 2: Duplicate name rejected ---
    {
        let result = create("test_overlay", lower, upper);
        if result.is_ok() {
            serial_println!("[overlay]   ERROR: duplicate name allowed");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        serial_println!("[overlay]   duplicate name rejected: OK");
    }

    // --- Test 3: Read from lower layer ---
    {
        let data = read_file(id, "file_a.txt")?;
        if data != b"lower content A" {
            serial_println!("[overlay]   ERROR: lower read mismatch");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        serial_println!("[overlay]   read lower: OK");
    }

    // --- Test 4: Write creates in upper, read returns upper version ---
    {
        write_file(id, "shared.txt", b"from upper")?;
        let data = read_file(id, "shared.txt")?;
        if data != b"from upper" {
            serial_println!("[overlay]   ERROR: upper override failed");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        // Lower is unchanged.
        let lower_data = Vfs::read_file(alloc::format!("{}/shared.txt", lower))?;
        if lower_data != b"from lower" {
            serial_println!("[overlay]   ERROR: lower was modified!");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        serial_println!("[overlay]   write + upper override: OK");
    }

    // --- Test 5: Create new file (upper only) ---
    {
        write_file(id, "new_file.txt", b"brand new")?;
        let data = read_file(id, "new_file.txt")?;
        if data != b"brand new" {
            serial_println!("[overlay]   ERROR: new file read failed");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        serial_println!("[overlay]   create new file: OK");
    }

    // --- Test 6: Remove lower file (whiteout) ---
    {
        remove(id, "file_b.txt")?;
        let result = read_file(id, "file_b.txt");
        if result.is_ok() {
            serial_println!("[overlay]   ERROR: whiteout file still readable");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        // Lower still has the file.
        let lower_data = Vfs::read_file(alloc::format!("{}/file_b.txt", lower))?;
        if lower_data != b"lower content B" {
            serial_println!("[overlay]   ERROR: lower file was deleted!");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        let whiteouts = list_whiteouts(id)?;
        if !whiteouts.contains(&PathBuf::from("file_b.txt")) {
            serial_println!("[overlay]   ERROR: whiteout not recorded");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        serial_println!("[overlay]   remove + whiteout: OK");
    }

    // --- Test 7: Readdir merges layers ---
    {
        let entries = readdir(id, "")?;
        let names: Vec<&Path> = entries.iter().map(|e| e.name.as_path()).collect();

        // Should have: file_a.txt, shared.txt (upper), new_file.txt, subdir
        // Should NOT have: file_b.txt (whited out)
        if names.contains(&Path::new("file_b.txt")) {
            serial_println!("[overlay]   ERROR: whited-out file in readdir");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        if !names.contains(&Path::new("file_a.txt"))
            || !names.contains(&Path::new("shared.txt"))
            || !names.contains(&Path::new("subdir"))
        {
            serial_println!("[overlay]   ERROR: missing expected entries: {:?}", names);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        serial_println!("[overlay]   readdir merge: OK");
    }

    // --- Test 8: which_layer resolution ---
    {
        let l1 = which_layer(id, "file_a.txt")?;
        if l1 != Layer::Lower {
            serial_println!("[overlay]   ERROR: file_a expected Lower, got {:?}", l1);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        let l2 = which_layer(id, "shared.txt")?;
        if l2 != Layer::Both {
            serial_println!("[overlay]   ERROR: shared expected Both, got {:?}", l2);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        let l3 = which_layer(id, "new_file.txt")?;
        if l3 != Layer::Upper {
            serial_println!("[overlay]   ERROR: new_file expected Upper, got {:?}", l3);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        let l4 = which_layer(id, "file_b.txt")?;
        if l4 != Layer::None {
            serial_println!("[overlay]   ERROR: file_b expected None, got {:?}", l4);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[overlay]   which_layer: OK");
    }

    // --- Test 9: Subdirectory copy-up ---
    {
        write_file(id, "subdir/deep.txt", b"modified deep")?;
        let data = read_file(id, "subdir/deep.txt")?;
        if data != b"modified deep" {
            serial_println!("[overlay]   ERROR: subdir copy-up failed");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        // Lower unchanged.
        let lower_deep = Vfs::read_file(alloc::format!("{}/subdir/deep.txt", lower))?;
        if lower_deep != b"deep file" {
            serial_println!("[overlay]   ERROR: lower subdir was modified");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        serial_println!("[overlay]   subdir copy-up: OK");
    }

    // --- Test 10: Stats tracking ---
    {
        let s = stats(id)?;
        if s.reads == 0 || s.writes == 0 {
            serial_println!(
                "[overlay]   ERROR: stats not tracked (r={} w={})",
                s.reads,
                s.writes
            );
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        if s.whiteout_count == 0 {
            serial_println!("[overlay]   ERROR: whiteout count is 0");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[overlay]   stats: OK (reads={} writes={} copyups={} whiteouts={})",
            s.reads,
            s.writes,
            s.copyups,
            s.whiteout_count
        );
    }

    // --- Test 11: Reset discards upper changes ---
    {
        let removed = reset(id)?;
        serial_println!("[overlay]   reset: removed {} entries", removed);

        // After reset, should read from lower again.
        let data = read_file(id, "file_a.txt")?;
        if data != b"lower content A" {
            serial_println!("[overlay]   ERROR: reset didn't restore lower view");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        // file_b should be visible again (whiteout cleared).
        let data_b = read_file(id, "file_b.txt")?;
        if data_b != b"lower content B" {
            serial_println!("[overlay]   ERROR: reset didn't clear whiteout");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[overlay]   reset + restore: OK");
    }

    // --- Test 12: Commit merges upper into lower ---
    {
        // Write something to upper, then commit.
        write_file(id, "committed.txt", b"committed data")?;
        let applied = commit(id)?;

        if applied == 0 {
            serial_println!("[overlay]   ERROR: commit applied 0 changes");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        // After commit, lower should have the committed file.
        let lower_data = Vfs::read_file(alloc::format!("{}/committed.txt", lower))?;
        if lower_data != b"committed data" {
            serial_println!("[overlay]   ERROR: commit didn't write to lower");
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        serial_println!("[overlay]   commit: OK (applied {} changes)", applied);
    }

    // --- Test 13: VFS mount adapter (path-transparent overlay) ---
    {
        let mount_path = "/mnt/ovl-cow-test";

        // The mount point's parent must exist for the mount to be reachable
        // by path lookup — `Vfs::mount` now enforces this rather than
        // registering an unreachable mount.  Nothing creates `/mnt` at boot.
        if !Vfs::exists("/mnt") {
            Vfs::mkdir("/mnt")?;
        }

        // Wrap the live overlay and mount it into the path tree.
        let ovl_fs = OverlayFs::new(id)?;
        Vfs::mount(mount_path, alloc::boxed::Box::new(ovl_fs))?;

        // Every fallible step below is reported with the step name and the
        // error, then unwound.  A bare `?` here would abandon the mount and
        // the scratch tree *and* tell the reader only
        // "Overlay filesystem self-test failed: NotFound" — with thirteen
        // tests and four `?`s in this block, that names neither the
        // operation that failed nor the path it failed on.
        macro_rules! step {
            ($what:expr, $e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(err) => {
                        serial_println!("[overlay]   ERROR: {} failed: {:?}", $what, err);
                        let _ = Vfs::unmount(mount_path);
                        let _ = Vfs::remove_recursive(test_base);
                        destroy(id).ok();
                        return Err(err);
                    }
                }
            };
        }

        // Read through a normal VFS path → merged view (lower layer).
        let via_vfs = step!(
            "read of file_a.txt through the mount",
            Vfs::read_file(alloc::format!("{}/file_a.txt", mount_path))
        );
        if via_vfs != b"lower content A" {
            serial_println!("[overlay]   ERROR: VFS-mounted read mismatch");
            let _ = Vfs::unmount(mount_path);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        // Write through a normal VFS path → copy-up into the upper layer.
        step!(
            "write of vfs_new.txt through the mount",
            Vfs::write_file(alloc::format!("{}/vfs_new.txt", mount_path), b"via vfs")
        );
        let back = step!(
            "read-back of vfs_new.txt through the mount",
            Vfs::read_file(alloc::format!("{}/vfs_new.txt", mount_path))
        );
        if back != b"via vfs" {
            serial_println!("[overlay]   ERROR: VFS-mounted write/read mismatch");
            let _ = Vfs::unmount(mount_path);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        // The write landed in the upper layer (copy-on-write), not lower.
        if step!("which_layer(vfs_new.txt)", which_layer(id, "vfs_new.txt")) != Layer::Upper {
            serial_println!("[overlay]   ERROR: VFS write did not go to upper layer");
            let _ = Vfs::unmount(mount_path);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }
        if Vfs::exists(alloc::format!("{}/vfs_new.txt", lower)) {
            serial_println!("[overlay]   ERROR: VFS write leaked into lower layer");
            let _ = Vfs::unmount(mount_path);
            let _ = Vfs::remove_recursive(test_base);
            destroy(id).ok();
            return Err(KernelError::InternalError);
        }

        step!("unmount", Vfs::unmount(mount_path));
        serial_println!("[overlay]   VFS mount adapter (CoW routing): OK");
    }

    // --- Cleanup ---
    destroy(id)?;
    let _ = Vfs::remove_recursive(test_base);

    serial_println!("[overlay] Self-test passed (13 tests).");
    Ok(())
}
