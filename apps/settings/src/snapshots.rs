//! System snapshot module for OS-level backup and recovery.
//!
//! Provides mutable data snapshots using copy-on-write at the filesystem level
//! with 64 KiB block granularity. Supports snapshot trees (branching like VMs),
//! selective includes, and rollback of OS updates.
//!
//! This module implements:
//! - Point-in-time captures of system state (OS, packages, config, user data)
//! - CoW block store with content-addressed deduplication
//! - Snapshot trees with branching and path traversal
//! - Automatic pre-update/pre-install snapshots with retention policies
//! - Rollback and update management (disable/retry)
//! - Settings UI rendering for snapshot management

#![allow(dead_code)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;

use core::fmt;

// ============================================================================
// Theme colors (same Catppuccin Mocha palette as main settings)
// ============================================================================

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
#[allow(dead_code)]
const COL_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const COL_ACCENT: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
#[allow(dead_code)]
const COL_TEAL: Color = Color::from_hex(0x94E2D5);

// ============================================================================
// Layout constants
// ============================================================================

const ROW_HEIGHT: f32 = 52.0;
const SECTION_SPACING: f32 = 24.0;
const TREE_INDENT: f32 = 28.0;
const TREE_NODE_HEIGHT: f32 = 32.0;

/// CoW block size: 64 KiB as specified in the design.
const COW_BLOCK_SIZE: usize = 64 * 1024;

/// Default maximum number of snapshots before pruning.
const DEFAULT_MAX_SNAPSHOTS: usize = 50;

/// Default maximum age in days before auto-deletion.
const DEFAULT_MAX_AGE_DAYS: u32 = 90;

/// Number of automatic pre-update snapshots to keep.
const AUTO_SNAPSHOT_KEEP_COUNT: usize = 5;

// ============================================================================
// Core types
// ============================================================================

/// Unique identifier for a snapshot, monotonically increasing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SnapshotId(pub u64);

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "snap-{}", self.0)
    }
}

/// Content hash of a 64 KiB block, used for deduplication.
/// In production this would be a cryptographic hash (e.g. BLAKE3);
/// here we use a simplified 256-bit representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockHash(pub [u8; 32]);

impl BlockHash {
    /// Create a hash from raw bytes (for testing / stub purposes).
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Simple non-cryptographic hash for testing. Production would use BLAKE3.
    pub fn compute(data: &[u8]) -> Self {
        // FNV-1a style mixing into 32 bytes for deterministic test hashing.
        let mut hash = [0u8; 32];
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &byte in data {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        // Spread across all 32 bytes by repeating with different seeds.
        for (i, slot) in hash.iter_mut().enumerate() {
            let seed = h.wrapping_add(i as u64);
            *slot = (seed ^ (seed >> 8) ^ (seed >> 16) ^ (seed >> 24)) as u8;
        }
        Self(hash)
    }
}

/// Type of snapshot, indicating what triggered its creation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotType {
    /// Full system state: OS binaries, packages, and configuration.
    System,
    /// User files only (home directories).
    UserData,
    /// Specific user-chosen paths.
    Custom(Vec<String>),
    /// Automatically created before an OS update.
    PreUpdate,
    /// Automatically created before a package installation.
    PreInstall,
}

impl SnapshotType {
    /// Human-readable label for UI display.
    pub fn label(&self) -> &str {
        match self {
            Self::System => "System",
            Self::UserData => "User Data",
            Self::Custom(_) => "Custom",
            Self::PreUpdate => "Pre-Update",
            Self::PreInstall => "Pre-Install",
        }
    }

    /// Color used for the type badge in the UI.
    pub fn badge_color(&self) -> Color {
        match self {
            Self::System => COL_ACCENT,
            Self::UserData => COL_GREEN,
            Self::Custom(_) => COL_TEAL,
            Self::PreUpdate => COL_PEACH,
            Self::PreInstall => COL_PEACH,
        }
    }
}

/// What data categories a snapshot includes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIncludes {
    /// System binaries: /usr, /bin, /lib
    pub system_files: bool,
    /// Configuration: /etc
    pub config_files: bool,
    /// User home directories: /home
    pub user_data: bool,
    /// Package manager state: /var/pkg
    pub package_state: bool,
    /// Additional user-specified paths.
    pub custom_paths: Vec<String>,
}

impl SnapshotIncludes {
    /// Everything included (full system snapshot).
    pub fn all() -> Self {
        Self {
            system_files: true,
            config_files: true,
            user_data: true,
            package_state: true,
            custom_paths: Vec::new(),
        }
    }

    /// Only user data.
    pub fn user_only() -> Self {
        Self {
            system_files: false,
            config_files: false,
            user_data: true,
            package_state: false,
            custom_paths: Vec::new(),
        }
    }

    /// System files and package state (for OS updates).
    pub fn system_only() -> Self {
        Self {
            system_files: true,
            config_files: true,
            user_data: false,
            package_state: true,
            custom_paths: Vec::new(),
        }
    }

    /// Custom paths only.
    pub fn custom(paths: Vec<String>) -> Self {
        Self {
            system_files: false,
            config_files: false,
            user_data: false,
            package_state: false,
            custom_paths: paths,
        }
    }

    /// Returns the list of root paths this snapshot covers.
    pub fn covered_paths(&self) -> Vec<&str> {
        let mut paths = Vec::new();
        if self.system_files {
            paths.extend_from_slice(&["/usr", "/bin", "/lib"]);
        }
        if self.config_files {
            paths.push("/etc");
        }
        if self.user_data {
            paths.push("/home");
        }
        if self.package_state {
            paths.push("/var/pkg");
        }
        for p in &self.custom_paths {
            paths.push(p.as_str());
        }
        paths
    }

    /// Check whether a given path falls under this snapshot's coverage.
    pub fn covers_path(&self, path: &str) -> bool {
        self.covered_paths()
            .iter()
            .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
    }
}

/// Current status of a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotStatus {
    /// Snapshot is complete and usable.
    Complete,
    /// Snapshot is currently being created.
    InProgress,
    /// Snapshot creation was interrupted or corrupted.
    Failed,
    /// Snapshot has been marked for deletion but not yet pruned.
    PendingDeletion,
}

impl SnapshotStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Complete => "Complete",
            Self::InProgress => "In Progress",
            Self::Failed => "Failed",
            Self::PendingDeletion => "Deleting",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Complete => COL_GREEN,
            Self::InProgress => COL_ACCENT,
            Self::Failed => COL_RED,
            Self::PendingDeletion => COL_OVERLAY0,
        }
    }
}

/// A point-in-time capture of system state.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Unique identifier.
    pub id: SnapshotId,
    /// Human-readable name.
    pub name: String,
    /// Optional longer description.
    pub description: String,
    /// Creation timestamp (seconds since epoch).
    pub created_at: u64,
    /// Parent snapshot for tree branching; `None` for root snapshots.
    pub parent_id: Option<SnapshotId>,
    /// What triggered this snapshot's creation.
    pub snapshot_type: SnapshotType,
    /// Disk space used by blocks unique to this snapshot (not shared).
    pub size_bytes: u64,
    /// What data categories are included.
    pub includes: SnapshotIncludes,
    /// Current status.
    pub status: SnapshotStatus,
    /// User-defined tags for organization.
    pub tags: Vec<String>,
}

impl Snapshot {
    /// Whether this snapshot was automatically created (pre-update or pre-install).
    pub fn is_automatic(&self) -> bool {
        matches!(
            self.snapshot_type,
            SnapshotType::PreUpdate | SnapshotType::PreInstall
        )
    }

    /// Short summary for list display.
    pub fn summary(&self) -> String {
        if self.description.is_empty() {
            format!("{} ({})", self.name, self.snapshot_type.label())
        } else {
            format!("{} — {}", self.name, self.description)
        }
    }
}

// ============================================================================
// CoW block store
// ============================================================================

/// A reference to a region within a file, mapped to a content-addressed block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRef {
    /// Path of the file this block belongs to.
    pub file_path: String,
    /// Byte offset within the file where this block starts.
    pub offset: u64,
    /// Actual size of data in this block (<= `COW_BLOCK_SIZE`; last block may be smaller).
    pub length: u32,
    /// Content hash of the block data.
    pub hash: BlockHash,
}

/// A difference between two snapshots at the block level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedBlock {
    /// File containing the changed block.
    pub file_path: String,
    /// Offset within the file.
    pub offset: u64,
    /// Block hash in the older snapshot (if the block existed).
    pub old_hash: Option<BlockHash>,
    /// Block hash in the newer snapshot (if the block exists).
    pub new_hash: Option<BlockHash>,
    /// Nature of the change.
    pub change_type: BlockChangeType,
}

/// What kind of change occurred to a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockChangeType {
    /// Block was added (not present in older snapshot).
    Added,
    /// Block was removed (not present in newer snapshot).
    Removed,
    /// Block content was modified.
    Modified,
}

/// Mapping from file regions to content-addressed blocks for a single snapshot.
#[derive(Clone, Debug, Default)]
pub struct BlockMap {
    /// All block references in this mapping, ordered by (file_path, offset).
    pub blocks: Vec<BlockRef>,
}

impl BlockMap {
    pub fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    /// Add a block reference to the map.
    pub fn insert(&mut self, block_ref: BlockRef) {
        self.blocks.push(block_ref);
    }

    /// Look up all blocks for a specific file path.
    pub fn blocks_for_file(&self, path: &str) -> Vec<&BlockRef> {
        self.blocks.iter().filter(|b| b.file_path == path).collect()
    }

    /// Total number of blocks in this map.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Total data size across all blocks.
    pub fn total_size(&self) -> u64 {
        self.blocks.iter().map(|b| u64::from(b.length)).sum()
    }
}

/// Content-addressed block store with deduplication.
///
/// Blocks are stored once and referenced by their content hash. When a snapshot
/// is created, file data is split into 64 KiB blocks, each hashed. If a block
/// with the same hash already exists, no new storage is consumed. When a file
/// is modified after a snapshot, the old block is preserved (CoW semantics)
/// and the new block is written separately.
pub struct BlockStore {
    /// All unique blocks stored, keyed by content hash.
    /// Value is the block data (up to 64 KiB).
    stored_blocks: Vec<(BlockHash, Vec<u8>)>,
    /// Per-snapshot block maps.
    snapshot_maps: Vec<(SnapshotId, BlockMap)>,
}

impl BlockStore {
    pub fn new() -> Self {
        Self {
            stored_blocks: Vec::new(),
            snapshot_maps: Vec::new(),
        }
    }

    /// Store a block if it doesn't already exist (deduplication).
    /// Returns the hash of the stored block.
    pub fn store_block(&mut self, data: &[u8]) -> BlockHash {
        let hash = BlockHash::compute(data);
        // Only store if not already present (content-addressed dedup).
        if !self.stored_blocks.iter().any(|(h, _)| *h == hash) {
            self.stored_blocks.push((hash, data.to_vec()));
        }
        hash
    }

    /// Retrieve block data by its hash.
    pub fn get_block(&self, hash: &BlockHash) -> Option<&[u8]> {
        self.stored_blocks
            .iter()
            .find(|(h, _)| h == hash)
            .map(|(_, data)| data.as_slice())
    }

    /// Check whether a block with this hash is already stored.
    pub fn contains(&self, hash: &BlockHash) -> bool {
        self.stored_blocks.iter().any(|(h, _)| h == hash)
    }

    /// Number of unique blocks currently stored.
    pub fn unique_block_count(&self) -> usize {
        self.stored_blocks.len()
    }

    /// Total storage consumed by unique blocks.
    pub fn total_storage_bytes(&self) -> u64 {
        self.stored_blocks
            .iter()
            .map(|(_, data)| data.len() as u64)
            .sum()
    }

    /// Create a snapshot from file data. Splits each file into 64 KiB blocks,
    /// stores them with deduplication, and records the block map.
    ///
    /// `files` is an iterator of (path, data) pairs.
    pub fn snapshot<'a>(
        &mut self,
        snapshot_id: SnapshotId,
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> BlockMap {
        let mut map = BlockMap::new();

        for (path, data) in files {
            let mut offset: u64 = 0;
            // Split file data into COW_BLOCK_SIZE chunks.
            for chunk in data.chunks(COW_BLOCK_SIZE) {
                let hash = self.store_block(chunk);
                map.insert(BlockRef {
                    file_path: path.to_string(),
                    offset,
                    length: chunk.len() as u32,
                    hash,
                });
                offset += chunk.len() as u64;
            }
        }

        self.snapshot_maps.push((snapshot_id, map.clone()));
        map
    }

    /// Retrieve the block map for a snapshot.
    pub fn get_snapshot_map(&self, id: SnapshotId) -> Option<&BlockMap> {
        self.snapshot_maps
            .iter()
            .find(|(sid, _)| *sid == id)
            .map(|(_, map)| map)
    }

    /// Restore file data from a snapshot's block map.
    /// Returns a list of (file_path, reassembled_data) pairs.
    pub fn restore(&self, snapshot_id: SnapshotId) -> Option<Vec<(String, Vec<u8>)>> {
        let map = self.get_snapshot_map(snapshot_id)?;
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();

        // Collect unique file paths in order.
        let mut current_path: Option<&str> = None;
        let mut current_data = Vec::new();

        for block_ref in &map.blocks {
            if current_path != Some(&block_ref.file_path) {
                // Flush previous file.
                if let Some(path) = current_path {
                    files.push((path.to_string(), core::mem::take(&mut current_data)));
                }
                current_path = Some(&block_ref.file_path);
                current_data.clear();
            }
            // Retrieve block data and append.
            if let Some(data) = self.get_block(&block_ref.hash) {
                current_data.extend_from_slice(data);
            }
        }
        // Flush last file.
        if let Some(path) = current_path {
            files.push((path.to_string(), current_data));
        }

        Some(files)
    }

    /// Compute the differences between two snapshots at the block level.
    pub fn diff(&self, older: SnapshotId, newer: SnapshotId) -> Option<Vec<ChangedBlock>> {
        let old_map = self.get_snapshot_map(older)?;
        let new_map = self.get_snapshot_map(newer)?;

        let mut changes = Vec::new();

        // Index old blocks by (path, offset) for lookup.
        let old_index: Vec<(&str, u64, &BlockHash)> = old_map
            .blocks
            .iter()
            .map(|b| (b.file_path.as_str(), b.offset, &b.hash))
            .collect();

        // Check each block in the newer snapshot against the older one.
        for new_block in &new_map.blocks {
            let old_entry = old_index
                .iter()
                .find(|(p, o, _)| *p == new_block.file_path && *o == new_block.offset);

            match old_entry {
                Some((_, _, old_hash)) if **old_hash != new_block.hash => {
                    // Block existed but content changed.
                    changes.push(ChangedBlock {
                        file_path: new_block.file_path.clone(),
                        offset: new_block.offset,
                        old_hash: Some(**old_hash),
                        new_hash: Some(new_block.hash),
                        change_type: BlockChangeType::Modified,
                    });
                }
                None => {
                    // Block is new (not in older snapshot).
                    changes.push(ChangedBlock {
                        file_path: new_block.file_path.clone(),
                        offset: new_block.offset,
                        old_hash: None,
                        new_hash: Some(new_block.hash),
                        change_type: BlockChangeType::Added,
                    });
                }
                Some(_) => {
                    // Same hash — unchanged, skip.
                }
            }
        }

        // Check for blocks in the old snapshot that are absent in the new one.
        let new_index: Vec<(&str, u64)> = new_map
            .blocks
            .iter()
            .map(|b| (b.file_path.as_str(), b.offset))
            .collect();

        for old_block in &old_map.blocks {
            let still_exists = new_index
                .iter()
                .any(|(p, o)| *p == old_block.file_path && *o == old_block.offset);

            if !still_exists {
                changes.push(ChangedBlock {
                    file_path: old_block.file_path.clone(),
                    offset: old_block.offset,
                    old_hash: Some(old_block.hash),
                    new_hash: None,
                    change_type: BlockChangeType::Removed,
                });
            }
        }

        Some(changes)
    }

    /// Remove all block data associated with a snapshot. Blocks that are shared
    /// with other snapshots are retained; only blocks unique to this snapshot
    /// are freed.
    pub fn remove_snapshot(&mut self, id: SnapshotId) {
        // Find and remove the snapshot's block map.
        let removed_map = {
            let pos = self.snapshot_maps.iter().position(|(sid, _)| *sid == id);
            match pos {
                Some(i) => self.snapshot_maps.remove(i).1,
                None => return,
            }
        };

        // Collect all hashes still referenced by other snapshots.
        let mut still_referenced: Vec<BlockHash> = Vec::new();
        for (_, map) in &self.snapshot_maps {
            for block in &map.blocks {
                if !still_referenced.contains(&block.hash) {
                    still_referenced.push(block.hash);
                }
            }
        }

        // Remove blocks unique to the deleted snapshot.
        for block in &removed_map.blocks {
            if !still_referenced.contains(&block.hash) {
                self.stored_blocks.retain(|(h, _)| *h != block.hash);
            }
        }
    }
}

// ============================================================================
// Snapshot tree
// ============================================================================

/// Tree structure organizing snapshots with parent-child relationships.
/// Supports branching (like VM snapshots or git) so users can explore
/// alternate system states without losing the original timeline.
pub struct SnapshotTree {
    /// All snapshots, in insertion order.
    snapshots: Vec<Snapshot>,
    /// Next ID to assign.
    next_id: u64,
}

impl SnapshotTree {
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
            next_id: 1,
        }
    }

    /// Allocate the next unique snapshot ID.
    fn alloc_id(&mut self) -> SnapshotId {
        let id = SnapshotId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Create a new root snapshot (no parent).
    pub fn create_root(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        snapshot_type: SnapshotType,
        includes: SnapshotIncludes,
        created_at: u64,
    ) -> SnapshotId {
        let id = self.alloc_id();
        self.snapshots.push(Snapshot {
            id,
            name: name.into(),
            description: description.into(),
            created_at,
            parent_id: None,
            snapshot_type,
            size_bytes: 0,
            includes,
            status: SnapshotStatus::Complete,
            tags: Vec::new(),
        });
        id
    }

    /// Create a branch from an existing snapshot.
    pub fn create_branch(
        &mut self,
        parent_id: SnapshotId,
        name: impl Into<String>,
        description: impl Into<String>,
        snapshot_type: SnapshotType,
        includes: SnapshotIncludes,
        created_at: u64,
    ) -> Option<SnapshotId> {
        // Verify parent exists.
        if !self.snapshots.iter().any(|s| s.id == parent_id) {
            return None;
        }
        let id = self.alloc_id();
        self.snapshots.push(Snapshot {
            id,
            name: name.into(),
            description: description.into(),
            created_at,
            parent_id: Some(parent_id),
            snapshot_type,
            size_bytes: 0,
            includes,
            status: SnapshotStatus::Complete,
            tags: Vec::new(),
        });
        Some(id)
    }

    /// Look up a snapshot by ID.
    pub fn get(&self, id: SnapshotId) -> Option<&Snapshot> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    /// Mutable access to a snapshot.
    pub fn get_mut(&mut self, id: SnapshotId) -> Option<&mut Snapshot> {
        self.snapshots.iter_mut().find(|s| s.id == id)
    }

    /// List direct children of a snapshot.
    pub fn list_children(&self, id: SnapshotId) -> Vec<SnapshotId> {
        self.snapshots
            .iter()
            .filter(|s| s.parent_id == Some(id))
            .map(|s| s.id)
            .collect()
    }

    /// List all root snapshots (those without a parent).
    pub fn list_roots(&self) -> Vec<SnapshotId> {
        self.snapshots
            .iter()
            .filter(|s| s.parent_id.is_none())
            .map(|s| s.id)
            .collect()
    }

    /// Trace the path from a snapshot back to its root ancestor.
    /// Returns IDs from the given snapshot (first) to the root (last).
    pub fn path_to_root(&self, id: SnapshotId) -> Vec<SnapshotId> {
        let mut path = Vec::new();
        let mut current = Some(id);
        // Guard against cycles (should not happen but defensive).
        let max_depth = self.snapshots.len();
        let mut depth = 0;

        while let Some(cid) = current {
            if depth > max_depth {
                break; // Cycle detected, bail out.
            }
            path.push(cid);
            current = self.get(cid).and_then(|s| s.parent_id);
            depth += 1;
        }
        path
    }

    /// All snapshots, ordered by creation time.
    pub fn all_snapshots(&self) -> &[Snapshot] {
        &self.snapshots
    }

    /// Total number of snapshots.
    pub fn count(&self) -> usize {
        self.snapshots.len()
    }

    /// Remove a snapshot by ID. Returns the removed snapshot if found.
    /// Does not remove children — they become new roots.
    pub fn remove(&mut self, id: SnapshotId) -> Option<Snapshot> {
        let pos = self.snapshots.iter().position(|s| s.id == id)?;
        let removed = self.snapshots.remove(pos);

        // Orphaned children become roots.
        for snap in &mut self.snapshots {
            if snap.parent_id == Some(id) {
                snap.parent_id = None;
            }
        }

        Some(removed)
    }

    /// Collect all descendant IDs of a snapshot (recursive).
    pub fn descendants(&self, id: SnapshotId) -> Vec<SnapshotId> {
        let mut result = Vec::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            for child_id in self.list_children(current) {
                result.push(child_id);
                stack.push(child_id);
            }
        }
        result
    }

    /// Build an ASCII-art tree visualization, returning lines of text.
    pub fn render_tree_ascii(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let roots = self.list_roots();
        for (i, root_id) in roots.iter().enumerate() {
            let is_last_root = i == roots.len() - 1;
            self.render_subtree_ascii(&mut lines, *root_id, "", is_last_root);
        }
        lines
    }

    fn render_subtree_ascii(
        &self,
        lines: &mut Vec<String>,
        id: SnapshotId,
        prefix: &str,
        is_last: bool,
    ) {
        let snap = match self.get(id) {
            Some(s) => s,
            None => return,
        };

        let connector = if prefix.is_empty() {
            ""
        } else if is_last {
            "\u{2514}\u{2500}\u{2500} " // "└── "
        } else {
            "\u{251C}\u{2500}\u{2500} " // "├── "
        };

        let status_marker = match snap.status {
            SnapshotStatus::Complete => "\u{2713}",        // checkmark
            SnapshotStatus::InProgress => "\u{2026}",      // ellipsis
            SnapshotStatus::Failed => "\u{2717}",          // cross
            SnapshotStatus::PendingDeletion => "\u{2205}", // empty set
        };

        lines.push(format!(
            "{prefix}{connector}[{status_marker}] {} ({})",
            snap.name,
            snap.snapshot_type.label(),
        ));

        let children = self.list_children(id);
        let child_prefix = if prefix.is_empty() {
            String::new()
        } else if is_last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}\u{2502}   ") // "│   "
        };

        for (i, child_id) in children.iter().enumerate() {
            let is_last_child = i == children.len() - 1;
            self.render_subtree_ascii(lines, *child_id, &child_prefix, is_last_child);
        }
    }
}

// ============================================================================
// Rollback system
// ============================================================================

/// Identifier for an OS update, used to track rollback/disable/retry state.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UpdateId(pub String);

/// Current disposition of an update relative to rollback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateDisposition {
    /// Update is applied normally.
    Applied,
    /// Update has been disabled and will be excluded on next restore.
    Disabled,
    /// Update is queued to be retried from its pre-update snapshot.
    PendingRetry,
}

/// An update tracked by the rollback system.
#[derive(Clone, Debug)]
pub struct TrackedUpdate {
    /// Unique update identifier (e.g. KB number).
    pub update_id: UpdateId,
    /// Description of the update.
    pub description: String,
    /// Snapshot taken before this update was applied.
    pub pre_update_snapshot: SnapshotId,
    /// Current disposition.
    pub disposition: UpdateDisposition,
}

/// Manages rollback of system state and update tracking.
pub struct RollbackManager {
    /// Updates tracked for rollback purposes.
    tracked_updates: Vec<TrackedUpdate>,
}

impl RollbackManager {
    pub fn new() -> Self {
        Self {
            tracked_updates: Vec::new(),
        }
    }

    /// Register an update with its pre-update snapshot.
    pub fn track_update(
        &mut self,
        update_id: UpdateId,
        description: impl Into<String>,
        pre_snapshot: SnapshotId,
    ) {
        self.tracked_updates.push(TrackedUpdate {
            update_id,
            description: description.into(),
            pre_update_snapshot: pre_snapshot,
            disposition: UpdateDisposition::Applied,
        });
    }

    /// Mark an update as disabled. On next restore, this update's changes
    /// will be excluded.
    pub fn disable_update(&mut self, update_id: &UpdateId) -> bool {
        if let Some(entry) = self
            .tracked_updates
            .iter_mut()
            .find(|t| t.update_id == *update_id)
        {
            entry.disposition = UpdateDisposition::Disabled;
            true
        } else {
            false
        }
    }

    /// Mark an update for retry. The system will re-apply it from the
    /// pre-update snapshot point.
    pub fn retry_update(&mut self, update_id: &UpdateId) -> bool {
        if let Some(entry) = self
            .tracked_updates
            .iter_mut()
            .find(|t| t.update_id == *update_id)
        {
            entry.disposition = UpdateDisposition::PendingRetry;
            true
        } else {
            false
        }
    }

    /// Get the pre-update snapshot for a rollback target.
    pub fn rollback_target(&self, update_id: &UpdateId) -> Option<SnapshotId> {
        self.tracked_updates
            .iter()
            .find(|t| t.update_id == *update_id)
            .map(|t| t.pre_update_snapshot)
    }

    /// List all tracked updates.
    pub fn tracked_updates(&self) -> &[TrackedUpdate] {
        &self.tracked_updates
    }

    /// List updates with a specific disposition.
    pub fn updates_with_disposition(&self, disp: UpdateDisposition) -> Vec<&TrackedUpdate> {
        self.tracked_updates
            .iter()
            .filter(|t| t.disposition == disp)
            .collect()
    }

    /// Remove tracking for an update (e.g. when its snapshot is pruned).
    pub fn remove_tracking(&mut self, update_id: &UpdateId) -> bool {
        let len_before = self.tracked_updates.len();
        self.tracked_updates.retain(|t| t.update_id != *update_id);
        self.tracked_updates.len() < len_before
    }
}

// ============================================================================
// Retention policy
// ============================================================================

/// Policy controlling automatic pruning of old snapshots.
#[derive(Clone, Debug)]
pub struct SnapshotRetention {
    /// Maximum number of snapshots to keep.
    pub max_snapshots: usize,
    /// Maximum age in days before a snapshot is eligible for deletion.
    pub max_age_days: u32,
    /// Snapshot IDs that are protected from automatic deletion.
    pub protected: Vec<SnapshotId>,
}

impl SnapshotRetention {
    /// Default retention policy.
    pub fn default_policy() -> Self {
        Self {
            max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            max_age_days: DEFAULT_MAX_AGE_DAYS,
            protected: Vec::new(),
        }
    }

    /// Protect a snapshot from automatic deletion.
    pub fn protect(&mut self, id: SnapshotId) {
        if !self.protected.contains(&id) {
            self.protected.push(id);
        }
    }

    /// Remove protection from a snapshot.
    pub fn unprotect(&mut self, id: SnapshotId) {
        self.protected.retain(|pid| *pid != id);
    }

    /// Check whether a snapshot is protected.
    pub fn is_protected(&self, id: SnapshotId) -> bool {
        self.protected.contains(&id)
    }

    /// Apply the retention policy to a snapshot tree. Returns the IDs of
    /// snapshots that should be deleted.
    ///
    /// `now_epoch` is the current time in seconds since epoch.
    pub fn apply(&self, tree: &SnapshotTree, now_epoch: u64) -> Vec<SnapshotId> {
        let mut candidates: Vec<&Snapshot> = tree
            .all_snapshots()
            .iter()
            .filter(|s| s.status == SnapshotStatus::Complete && !self.protected.contains(&s.id))
            .collect();

        // Sort by creation time, oldest first.
        candidates.sort_by_key(|s| s.created_at);

        let mut to_delete = Vec::new();

        // Phase 1: delete snapshots older than max_age_days.
        let max_age_secs = u64::from(self.max_age_days) * 86_400;
        for snap in &candidates {
            if now_epoch.saturating_sub(snap.created_at) > max_age_secs {
                to_delete.push(snap.id);
            }
        }

        // Phase 2: if still over max_snapshots, delete oldest non-protected.
        let remaining = tree.count().saturating_sub(to_delete.len());
        if remaining > self.max_snapshots {
            let excess = remaining - self.max_snapshots;
            let mut deleted_in_phase2 = 0;
            for snap in &candidates {
                if deleted_in_phase2 >= excess {
                    break;
                }
                if !to_delete.contains(&snap.id) {
                    to_delete.push(snap.id);
                    deleted_in_phase2 += 1;
                }
            }
        }

        to_delete
    }
}

/// Prune automatic pre-update/pre-install snapshots, keeping only the most
/// recent `keep_count`.
pub fn prune_auto_snapshots(tree: &SnapshotTree, keep_count: usize) -> Vec<SnapshotId> {
    let mut auto_snaps: Vec<&Snapshot> = tree
        .all_snapshots()
        .iter()
        .filter(|s| s.is_automatic() && s.status == SnapshotStatus::Complete)
        .collect();

    // Sort newest first.
    auto_snaps.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    auto_snaps.iter().skip(keep_count).map(|s| s.id).collect()
}

// ============================================================================
// Snapshot manager (orchestrates all subsystems)
// ============================================================================

/// High-level manager coordinating snapshot creation, storage, rollback,
/// and retention.
pub struct SnapshotManager {
    pub tree: SnapshotTree,
    pub block_store: BlockStore,
    pub rollback: RollbackManager,
    pub retention: SnapshotRetention,
}

impl SnapshotManager {
    pub fn new() -> Self {
        Self {
            tree: SnapshotTree::new(),
            block_store: BlockStore::new(),
            rollback: RollbackManager::new(),
            retention: SnapshotRetention::default_policy(),
        }
    }

    /// Create a snapshot with the given metadata, storing file data in the
    /// block store.
    pub fn create_snapshot<'a>(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        snapshot_type: SnapshotType,
        includes: SnapshotIncludes,
        created_at: u64,
        parent_id: Option<SnapshotId>,
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> SnapshotId {
        let id = match parent_id {
            Some(pid) => self
                .tree
                .create_branch(
                    pid,
                    name,
                    description,
                    snapshot_type,
                    includes.clone(),
                    created_at,
                )
                .unwrap_or_else(|| {
                    // Parent not found — create as root instead.
                    self.tree.create_root(
                        "orphaned",
                        "parent not found",
                        SnapshotType::System,
                        includes.clone(),
                        created_at,
                    )
                }),
            None => self.tree.create_root(
                name,
                description,
                snapshot_type,
                includes.clone(),
                created_at,
            ),
        };

        let block_map = self.block_store.snapshot(id, files);

        // Record size (unique bytes for this snapshot).
        if let Some(snap) = self.tree.get_mut(id) {
            snap.size_bytes = block_map.total_size();
        }

        id
    }

    /// Create an automatic pre-update snapshot and register the update
    /// for rollback tracking.
    pub fn create_pre_update_snapshot<'a>(
        &mut self,
        update_id: UpdateId,
        update_description: impl Into<String>,
        created_at: u64,
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> SnapshotId {
        let desc_string: String = update_description.into();
        let snap_name = format!("Pre-update: {}", desc_string);

        let id = self.create_snapshot(
            snap_name,
            format!("Automatic snapshot before {desc_string}"),
            SnapshotType::PreUpdate,
            SnapshotIncludes::system_only(),
            created_at,
            None,
            files,
        );

        self.rollback.track_update(update_id, desc_string, id);
        id
    }

    /// Apply retention policy and prune expired snapshots.
    /// Returns the IDs that were deleted.
    pub fn apply_retention(&mut self, now_epoch: u64) -> Vec<SnapshotId> {
        let mut to_delete = self.retention.apply(&self.tree, now_epoch);

        // Also prune old automatic snapshots.
        let auto_prune = prune_auto_snapshots(&self.tree, AUTO_SNAPSHOT_KEEP_COUNT);
        for id in auto_prune {
            if !to_delete.contains(&id) {
                to_delete.push(id);
            }
        }

        // Perform deletion.
        for &id in &to_delete {
            self.block_store.remove_snapshot(id);
            self.tree.remove(id);
        }

        to_delete
    }
}

// ============================================================================
// Settings UI rendering
// ============================================================================

/// Helper: draw a filled rounded rectangle.
fn fill_rounded(tree: &mut RenderTree, x: f32, y: f32, w: f32, h: f32, color: Color, radius: f32) {
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

/// Helper: draw bold text.
fn text_bold(tree: &mut RenderTree, x: f32, y: f32, content: &str, color: Color, size: f32) {
    tree.text(x, y, content, color, size);
    // Simulate bold by drawing slightly offset (until proper font weight support).
    tree.push(RenderCommand::Text {
        x: x + 0.5,
        y,
        text: content.to_string(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
}

/// Helper: section header with underline.
fn render_section_header(tree: &mut RenderTree, x: f32, y: f32, title: &str) -> f32 {
    text_bold(tree, x, y, title, COL_TEXT, 16.0);
    tree.push(RenderCommand::FillRect {
        x,
        y: y + 24.0,
        width: 580.0,
        height: 1.0,
        color: COL_SURFACE1,
        corner_radii: CornerRadii::ZERO,
    });
    y + 36.0
}

/// Helper: render a small colored badge.
fn render_badge(tree: &mut RenderTree, x: f32, y: f32, label: &str, color: Color) {
    let width = label.len() as f32 * 7.0 + 16.0;
    fill_rounded(tree, x, y, width, 20.0, color, 4.0);
    tree.text(x + 8.0, y + 3.0, label, COL_BASE, 11.0);
}

/// Helper: render a clickable button.
fn render_button(tree: &mut RenderTree, x: f32, y: f32, label: &str, color: Color) {
    let width = label.len() as f32 * 7.5 + 24.0;
    fill_rounded(tree, x, y, width, 32.0, color, 6.0);
    tree.text(x + 12.0, y + 8.0, label, COL_BASE, 13.0);
}

/// Render the full snapshots settings page.
///
/// Displays a list of snapshots with metadata, action buttons, a tree
/// visualization, and retention settings.
pub fn render_snapshots_page(
    tree: &mut RenderTree,
    x: f32,
    start_y: f32,
    manager: &SnapshotManager,
) {
    let mut y = start_y;
    let right_x = x + 400.0;

    // Section: System Snapshots
    y = render_section_header(tree, x, y, "System Snapshots");
    tree.text(
        x,
        y + 4.0,
        "Point-in-time captures for safe rollback and recovery:",
        COL_SUBTEXT0,
        13.0,
    );
    y += 28.0;

    // Snapshot list.
    let snapshots = manager.tree.all_snapshots();
    if snapshots.is_empty() {
        tree.text(x + 16.0, y + 12.0, "No snapshots yet.", COL_OVERLAY0, 13.0);
        y += ROW_HEIGHT;
    } else {
        for snap in snapshots {
            let bg = match snap.status {
                SnapshotStatus::InProgress => COL_SURFACE2,
                _ => COL_SURFACE0,
            };
            fill_rounded(tree, x, y, 580.0, ROW_HEIGHT, bg, 6.0);

            // Name and type badge.
            text_bold(tree, x + 12.0, y + 8.0, &snap.name, COL_TEXT, 13.0);
            render_badge(
                tree,
                x + 12.0 + snap.name.len() as f32 * 8.0 + 8.0,
                y + 8.0,
                snap.snapshot_type.label(),
                snap.snapshot_type.badge_color(),
            );

            // Description.
            if !snap.description.is_empty() {
                tree.text(x + 12.0, y + 30.0, &snap.description, COL_SUBTEXT0, 11.0);
            }

            // Size.
            let size_label = format_size(snap.size_bytes);
            tree.text(right_x, y + 8.0, &size_label, COL_SUBTEXT0, 12.0);

            // Status.
            let status_color = snap.status.color();
            tree.text(
                right_x + 100.0,
                y + 8.0,
                snap.status.label(),
                status_color,
                12.0,
            );

            // Tags.
            if !snap.tags.is_empty() {
                let tags_str = snap.tags.join(", ");
                tree.text(right_x, y + 28.0, &tags_str, COL_OVERLAY0, 10.0);
            }

            y += ROW_HEIGHT + 4.0;
        }
    }

    y += SECTION_SPACING;

    // Action buttons row.
    render_button(tree, x, y, "Create Snapshot", COL_ACCENT);
    render_button(tree, x + 150.0, y, "Restore", COL_GREEN);
    render_button(tree, x + 240.0, y, "Delete", COL_RED);
    render_button(tree, x + 320.0, y, "Diff", COL_PEACH);
    y += 44.0 + SECTION_SPACING;

    // Section: Snapshot Tree.
    y = render_section_header(tree, x, y, "Snapshot Tree");
    let tree_lines = manager.tree.render_tree_ascii();
    if tree_lines.is_empty() {
        tree.text(x + 16.0, y + 4.0, "(empty)", COL_OVERLAY0, 12.0);
        y += TREE_NODE_HEIGHT;
    } else {
        for line in &tree_lines {
            // Use monospace-style rendering for the tree.
            tree.text(x + 16.0, y + 4.0, line, COL_TEXT, 12.0);
            y += TREE_NODE_HEIGHT / 1.5;
        }
    }

    y += SECTION_SPACING;

    // Section: Retention Settings.
    y = render_section_header(tree, x, y, "Retention Settings");

    // Max snapshots.
    fill_rounded(tree, x, y, 580.0, 40.0, COL_SURFACE0, 6.0);
    tree.text(x + 12.0, y + 12.0, "Maximum snapshots", COL_TEXT, 13.0);
    tree.text(
        right_x + 80.0,
        y + 12.0,
        &manager.retention.max_snapshots.to_string(),
        COL_ACCENT,
        13.0,
    );
    y += 48.0;

    // Max age.
    fill_rounded(tree, x, y, 580.0, 40.0, COL_SURFACE0, 6.0);
    tree.text(
        x + 12.0,
        y + 12.0,
        "Auto-delete after (days)",
        COL_TEXT,
        13.0,
    );
    tree.text(
        right_x + 80.0,
        y + 12.0,
        &manager.retention.max_age_days.to_string(),
        COL_ACCENT,
        13.0,
    );
    y += 48.0;

    // Protected count.
    fill_rounded(tree, x, y, 580.0, 40.0, COL_SURFACE0, 6.0);
    tree.text(x + 12.0, y + 12.0, "Protected snapshots", COL_TEXT, 13.0);
    tree.text(
        right_x + 80.0,
        y + 12.0,
        &manager.retention.protected.len().to_string(),
        COL_GREEN,
        13.0,
    );
    y += 48.0;

    // Auto-prune count.
    fill_rounded(tree, x, y, 580.0, 40.0, COL_SURFACE0, 6.0);
    tree.text(x + 12.0, y + 12.0, "Keep auto-snapshots", COL_TEXT, 13.0);
    tree.text(
        right_x + 80.0,
        y + 12.0,
        &AUTO_SNAPSHOT_KEEP_COUNT.to_string(),
        COL_ACCENT,
        13.0,
    );

    // Trailing space indicator (unused y, but available for future sections).
    let _ = y;
}

/// Format a byte count as a human-readable string (KiB, MiB, GiB).
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;

    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: create a simple test snapshot in a manager.
    fn make_test_manager() -> SnapshotManager {
        SnapshotManager::new()
    }

    // ---- Snapshot creation with metadata ----

    #[test]
    fn snapshot_creation_metadata() {
        let mut mgr = make_test_manager();

        let id = mgr.create_snapshot(
            "test-snap",
            "A test snapshot",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_700_000_000,
            None,
            [("/etc/config.yaml", b"key: value" as &[u8])],
        );

        let snap = mgr.tree.get(id).expect("snapshot should exist");
        assert_eq!(snap.name, "test-snap");
        assert_eq!(snap.description, "A test snapshot");
        assert_eq!(snap.created_at, 1_700_000_000);
        assert_eq!(snap.snapshot_type, SnapshotType::System);
        assert_eq!(snap.status, SnapshotStatus::Complete);
        assert!(snap.parent_id.is_none());
        assert!(snap.size_bytes > 0);
    }

    // ---- Block store deduplication ----

    #[test]
    fn block_store_dedup_identical_blocks() {
        let mut store = BlockStore::new();

        let data = vec![0xABu8; COW_BLOCK_SIZE];
        let h1 = store.store_block(&data);
        let h2 = store.store_block(&data);

        assert_eq!(h1, h2, "identical data should produce the same hash");
        assert_eq!(
            store.unique_block_count(),
            1,
            "identical blocks should only be stored once"
        );
    }

    #[test]
    fn block_store_different_blocks_stored_separately() {
        let mut store = BlockStore::new();

        let data_a = vec![0x11u8; COW_BLOCK_SIZE];
        let data_b = vec![0x22u8; COW_BLOCK_SIZE];
        let h1 = store.store_block(&data_a);
        let h2 = store.store_block(&data_b);

        assert_ne!(h1, h2);
        assert_eq!(store.unique_block_count(), 2);
    }

    // ---- CoW: modify block, old preserved ----

    #[test]
    fn cow_old_block_preserved_on_modify() {
        let mut mgr = make_test_manager();

        let original_data = b"original content for CoW test block";
        let snap1 = mgr.create_snapshot(
            "v1",
            "original",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
            None,
            [("/data/file.txt", original_data as &[u8])],
        );

        let modified_data = b"modified content for CoW test block";
        let snap2 = mgr.create_snapshot(
            "v2",
            "modified",
            SnapshotType::System,
            SnapshotIncludes::all(),
            2_000,
            Some(snap1),
            [("/data/file.txt", modified_data as &[u8])],
        );

        // Both snapshots should be restorable independently.
        let restored_v1 = mgr.block_store.restore(snap1).expect("v1 restorable");
        let restored_v2 = mgr.block_store.restore(snap2).expect("v2 restorable");

        assert_eq!(restored_v1.len(), 1);
        assert_eq!(restored_v2.len(), 1);
        assert_eq!(restored_v1[0].1, original_data);
        assert_eq!(restored_v2[0].1, modified_data);
    }

    // ---- Tree branching and path traversal ----

    #[test]
    fn tree_branching() {
        let mut tree = SnapshotTree::new();

        let root = tree.create_root(
            "root",
            "initial",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
        );

        let branch_a = tree
            .create_branch(
                root,
                "branch-a",
                "first branch",
                SnapshotType::System,
                SnapshotIncludes::all(),
                2_000,
            )
            .expect("branch from root");

        let branch_b = tree
            .create_branch(
                root,
                "branch-b",
                "second branch",
                SnapshotType::UserData,
                SnapshotIncludes::user_only(),
                3_000,
            )
            .expect("branch from root");

        assert_eq!(tree.list_children(root), vec![branch_a, branch_b]);
        assert!(tree.list_children(branch_a).is_empty());
        assert!(tree.list_children(branch_b).is_empty());
    }

    #[test]
    fn tree_path_to_root() {
        let mut tree = SnapshotTree::new();

        let root = tree.create_root(
            "root",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
        );

        let child = tree
            .create_branch(
                root,
                "child",
                "",
                SnapshotType::System,
                SnapshotIncludes::all(),
                2_000,
            )
            .unwrap();

        let grandchild = tree
            .create_branch(
                child,
                "grandchild",
                "",
                SnapshotType::System,
                SnapshotIncludes::all(),
                3_000,
            )
            .unwrap();

        let path = tree.path_to_root(grandchild);
        assert_eq!(path, vec![grandchild, child, root]);
    }

    #[test]
    fn tree_branch_from_nonexistent_parent_fails() {
        let mut tree = SnapshotTree::new();
        let result = tree.create_branch(
            SnapshotId(999),
            "orphan",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
        );
        assert!(result.is_none());
    }

    #[test]
    fn tree_remove_reparents_children() {
        let mut tree = SnapshotTree::new();

        let root = tree.create_root(
            "root",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
        );
        let child = tree
            .create_branch(
                root,
                "child",
                "",
                SnapshotType::System,
                SnapshotIncludes::all(),
                2_000,
            )
            .unwrap();
        let grandchild = tree
            .create_branch(
                child,
                "gc",
                "",
                SnapshotType::System,
                SnapshotIncludes::all(),
                3_000,
            )
            .unwrap();

        // Remove the middle node.
        tree.remove(child);

        // Grandchild should now be a root.
        let gc = tree.get(grandchild).expect("grandchild still exists");
        assert!(gc.parent_id.is_none());
        assert!(tree.list_roots().contains(&grandchild));
    }

    #[test]
    fn tree_descendants() {
        let mut tree = SnapshotTree::new();

        let root = tree.create_root(
            "root",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
        );
        let c1 = tree
            .create_branch(
                root,
                "c1",
                "",
                SnapshotType::System,
                SnapshotIncludes::all(),
                2_000,
            )
            .unwrap();
        let c2 = tree
            .create_branch(
                root,
                "c2",
                "",
                SnapshotType::System,
                SnapshotIncludes::all(),
                3_000,
            )
            .unwrap();
        let gc1 = tree
            .create_branch(
                c1,
                "gc1",
                "",
                SnapshotType::System,
                SnapshotIncludes::all(),
                4_000,
            )
            .unwrap();

        let desc = tree.descendants(root);
        assert_eq!(desc.len(), 3);
        assert!(desc.contains(&c1));
        assert!(desc.contains(&c2));
        assert!(desc.contains(&gc1));
    }

    // ---- Retention policy ----

    #[test]
    fn retention_by_age() {
        let mut tree = SnapshotTree::new();

        // Snapshot created 100 days ago.
        let old = tree.create_root(
            "old",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000_000,
        );

        // Snapshot created recently.
        let recent = tree.create_root(
            "recent",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            10_000_000,
        );

        let policy = SnapshotRetention {
            max_snapshots: 100,
            max_age_days: 90,
            protected: Vec::new(),
        };

        // "now" is 10_000_000 + a little bit. 10_000_000 - 1_000_000 = 9_000_000 seconds
        // = ~104 days, which is > 90 days.
        let to_delete = policy.apply(&tree, 10_000_000);
        assert!(to_delete.contains(&old), "old snapshot should be pruned");
        assert!(
            !to_delete.contains(&recent),
            "recent snapshot should be kept"
        );
    }

    #[test]
    fn retention_by_count() {
        let mut tree = SnapshotTree::new();

        // Create 5 snapshots, but max is 3.
        for i in 0..5 {
            tree.create_root(
                format!("snap-{i}"),
                "",
                SnapshotType::System,
                SnapshotIncludes::all(),
                1_000_000 + i * 1_000,
            );
        }

        let policy = SnapshotRetention {
            max_snapshots: 3,
            max_age_days: 9999, // No age-based pruning.
            protected: Vec::new(),
        };

        let to_delete = policy.apply(&tree, 1_100_000);
        assert_eq!(
            to_delete.len(),
            2,
            "should delete 2 oldest to get down to 3"
        );
    }

    #[test]
    fn retention_protected_not_deleted() {
        let mut tree = SnapshotTree::new();

        let old = tree.create_root(
            "protected-old",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
        );
        tree.create_root(
            "unprotected",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            2_000,
        );

        let policy = SnapshotRetention {
            max_snapshots: 100,
            max_age_days: 1, // Very aggressive — would delete both by age.
            protected: vec![old],
        };

        let to_delete = policy.apply(&tree, 1_000_000);
        assert!(
            !to_delete.contains(&old),
            "protected snapshot must not be deleted"
        );
    }

    // ---- Diff between snapshots ----

    #[test]
    fn diff_detects_modifications() {
        let mut mgr = make_test_manager();

        let snap1 = mgr.create_snapshot(
            "before",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
            None,
            [("/file.txt", b"hello world" as &[u8])],
        );

        let snap2 = mgr.create_snapshot(
            "after",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            2_000,
            Some(snap1),
            [("/file.txt", b"hello WORLD" as &[u8])],
        );

        let changes = mgr.block_store.diff(snap1, snap2).expect("diff succeeds");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change_type, BlockChangeType::Modified);
        assert_eq!(changes[0].file_path, "/file.txt");
    }

    #[test]
    fn diff_detects_additions_and_removals() {
        let mut mgr = make_test_manager();

        let snap1 = mgr.create_snapshot(
            "v1",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
            None,
            [("/a.txt", b"aaa" as &[u8])],
        );

        let snap2 = mgr.create_snapshot(
            "v2",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            2_000,
            Some(snap1),
            [("/b.txt", b"bbb" as &[u8])],
        );

        let changes = mgr.block_store.diff(snap1, snap2).expect("diff succeeds");

        let added: Vec<_> = changes
            .iter()
            .filter(|c| c.change_type == BlockChangeType::Added)
            .collect();
        let removed: Vec<_> = changes
            .iter()
            .filter(|c| c.change_type == BlockChangeType::Removed)
            .collect();

        assert_eq!(added.len(), 1);
        assert_eq!(added[0].file_path, "/b.txt");
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].file_path, "/a.txt");
    }

    // ---- Rollback state tracking ----

    #[test]
    fn rollback_track_and_disable() {
        let mut rollback = RollbackManager::new();

        let snap_id = SnapshotId(1);
        let update = UpdateId("KB5032100".into());

        rollback.track_update(update.clone(), "Security patch", snap_id);

        assert_eq!(rollback.tracked_updates().len(), 1);
        assert_eq!(
            rollback.tracked_updates()[0].disposition,
            UpdateDisposition::Applied
        );

        assert!(rollback.disable_update(&update));
        assert_eq!(
            rollback.tracked_updates()[0].disposition,
            UpdateDisposition::Disabled
        );
    }

    #[test]
    fn rollback_retry_update() {
        let mut rollback = RollbackManager::new();

        let snap_id = SnapshotId(2);
        let update = UpdateId("KB5031980".into());

        rollback.track_update(update.clone(), "Feature update", snap_id);
        assert!(rollback.retry_update(&update));
        assert_eq!(
            rollback.tracked_updates()[0].disposition,
            UpdateDisposition::PendingRetry
        );

        // Rollback target should be the pre-update snapshot.
        assert_eq!(rollback.rollback_target(&update), Some(snap_id));
    }

    #[test]
    fn rollback_nonexistent_update_returns_false() {
        let mut rollback = RollbackManager::new();
        let bogus = UpdateId("nonexistent".into());
        assert!(!rollback.disable_update(&bogus));
        assert!(!rollback.retry_update(&bogus));
        assert!(rollback.rollback_target(&bogus).is_none());
    }

    // ---- SnapshotIncludes filtering ----

    #[test]
    fn includes_covers_path() {
        let includes = SnapshotIncludes::all();
        assert!(includes.covers_path("/usr/bin/app"));
        assert!(includes.covers_path("/etc/config.yaml"));
        assert!(includes.covers_path("/home/user/file.txt"));
        assert!(includes.covers_path("/var/pkg/db"));
    }

    #[test]
    fn includes_user_only_does_not_cover_system() {
        let includes = SnapshotIncludes::user_only();
        assert!(includes.covers_path("/home/user/file.txt"));
        assert!(!includes.covers_path("/usr/bin/app"));
        assert!(!includes.covers_path("/etc/config.yaml"));
        assert!(!includes.covers_path("/var/pkg/db"));
    }

    #[test]
    fn includes_custom_paths() {
        let includes = SnapshotIncludes::custom(vec!["/opt/myapp".into(), "/srv/data".into()]);
        assert!(includes.covers_path("/opt/myapp/bin/run"));
        assert!(includes.covers_path("/srv/data/file.db"));
        assert!(!includes.covers_path("/home/user/file.txt"));
        assert!(!includes.covers_path("/usr/bin/app"));
    }

    // ---- Block store snapshot and restore round-trip ----

    #[test]
    fn snapshot_restore_roundtrip() {
        let mut store = BlockStore::new();

        let file_a = b"content of file A with enough data to be interesting";
        let file_b = b"content of file B, different from A";

        let id = SnapshotId(1);
        store.snapshot(
            id,
            [("/a.txt", file_a as &[u8]), ("/b.txt", file_b as &[u8])],
        );

        let restored = store.restore(id).expect("restore succeeds");
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].0, "/a.txt");
        assert_eq!(restored[0].1, file_a);
        assert_eq!(restored[1].0, "/b.txt");
        assert_eq!(restored[1].1, file_b);
    }

    #[test]
    fn snapshot_large_file_multi_block() {
        let mut store = BlockStore::new();

        // Create data spanning 3 blocks.
        let data = vec![0x42u8; COW_BLOCK_SIZE * 2 + 100];
        let id = SnapshotId(1);
        let map = store.snapshot(id, [("/big.bin", data.as_slice())]);

        assert_eq!(map.block_count(), 3, "should split into 3 blocks");

        let restored = store.restore(id).expect("restore succeeds");
        assert_eq!(restored[0].1, data);
    }

    // ---- Auto-snapshot pruning ----

    #[test]
    fn prune_auto_snapshots_keeps_recent() {
        let mut tree = SnapshotTree::new();

        // Create 7 auto snapshots.
        for i in 0..7 {
            tree.create_root(
                format!("auto-{i}"),
                "",
                SnapshotType::PreUpdate,
                SnapshotIncludes::system_only(),
                1_000 + i * 100,
            );
        }

        let to_prune = prune_auto_snapshots(&tree, AUTO_SNAPSHOT_KEEP_COUNT);
        // 7 auto snaps, keep 5 => prune 2 oldest.
        assert_eq!(to_prune.len(), 2);

        // The pruned ones should be the oldest (lowest created_at).
        for &id in &to_prune {
            let snap = tree.get(id).unwrap();
            assert!(snap.created_at < 1_000 + 5 * 100);
        }
    }

    // ---- Tree ASCII rendering ----

    #[test]
    fn tree_ascii_rendering() {
        let mut tree = SnapshotTree::new();

        let root = tree.create_root(
            "Initial",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
        );
        tree.create_branch(
            root,
            "Update-1",
            "",
            SnapshotType::PreUpdate,
            SnapshotIncludes::system_only(),
            2_000,
        )
        .unwrap();
        tree.create_branch(
            root,
            "Branch-A",
            "",
            SnapshotType::UserData,
            SnapshotIncludes::user_only(),
            3_000,
        )
        .unwrap();

        let lines = tree.render_tree_ascii();
        assert!(!lines.is_empty());
        // Root should be first line.
        assert!(lines[0].contains("Initial"));
        // Children should appear.
        let all_text = lines.join("\n");
        assert!(all_text.contains("Update-1"));
        assert!(all_text.contains("Branch-A"));
    }

    // ---- Format size ----

    #[test]
    fn format_size_display() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }

    // ---- Snapshot is_automatic ----

    #[test]
    fn snapshot_is_automatic() {
        let mut tree = SnapshotTree::new();

        let manual = tree.create_root(
            "manual",
            "",
            SnapshotType::System,
            SnapshotIncludes::all(),
            1_000,
        );
        let pre_update = tree.create_root(
            "pre-upd",
            "",
            SnapshotType::PreUpdate,
            SnapshotIncludes::system_only(),
            2_000,
        );
        let pre_install = tree.create_root(
            "pre-inst",
            "",
            SnapshotType::PreInstall,
            SnapshotIncludes::system_only(),
            3_000,
        );

        assert!(!tree.get(manual).unwrap().is_automatic());
        assert!(tree.get(pre_update).unwrap().is_automatic());
        assert!(tree.get(pre_install).unwrap().is_automatic());
    }

    // ---- Manager: pre-update snapshot integrates rollback ----

    #[test]
    fn manager_pre_update_snapshot_tracks_rollback() {
        let mut mgr = make_test_manager();

        let update_id = UpdateId("KB5032200".into());
        let snap_id = mgr.create_pre_update_snapshot(
            update_id.clone(),
            "May 2026 security patch",
            1_700_000_000,
            [("/usr/lib/libfoo.so", b"old library" as &[u8])],
        );

        // Rollback should know about this update.
        assert_eq!(mgr.rollback.rollback_target(&update_id), Some(snap_id));

        // Snapshot should exist in the tree.
        let snap = mgr.tree.get(snap_id).unwrap();
        assert_eq!(snap.snapshot_type, SnapshotType::PreUpdate);
        assert!(snap.name.contains("Pre-update"));
    }

    // ---- Block removal on snapshot delete ----

    #[test]
    fn block_store_remove_snapshot_frees_unique_blocks() {
        let mut store = BlockStore::new();

        let shared_data = b"shared content";
        let unique_data = b"unique to snap 1 only";

        let id1 = SnapshotId(1);
        let id2 = SnapshotId(2);

        store.snapshot(
            id1,
            [
                ("/shared.txt", shared_data as &[u8]),
                ("/unique.txt", unique_data as &[u8]),
            ],
        );
        store.snapshot(id2, [("/shared.txt", shared_data as &[u8])]);

        let blocks_before = store.unique_block_count();
        store.remove_snapshot(id1);

        // Shared block should still exist; unique block should be gone.
        assert!(store.unique_block_count() < blocks_before);
        assert!(
            store.contains(&BlockHash::compute(shared_data)),
            "shared block should still be stored"
        );
        assert!(
            !store.contains(&BlockHash::compute(unique_data)),
            "unique block should be freed"
        );
    }
}
