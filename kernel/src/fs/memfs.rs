//! In-memory filesystem (ramfs / tmpfs).
//!
//! A volatile, heap-backed filesystem that stores all data in RAM.
//! Contents are lost on reboot.  Case-sensitive (per design spec).
//!
//! ## Use cases
//!
//! - `/tmp` for temporary files
//! - Foundation for pseudo-filesystems (procfs, sysfs, devfs)
//! - Testing VFS operations without a real block device
//!
//! ## Design
//!
//! Uses a tree of [`MemFsNode`] nodes.  Each node is a
//! [`File`](MemFsNodeKind::File) (data: `Vec<u8>`), a
//! [`Dir`](MemFsNodeKind::Dir) (children: `BTreeMap<name, node>`),
//! or a [`Symlink`](MemFsNodeKind::Symlink) (target path string).
//!
//! Path resolution walks the tree component by component with
//! exact (case-sensitive) matching.  Symlinks are followed
//! transparently during resolution (up to [`MAX_SYMLINK_DEPTH`]
//! hops to prevent loops).

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{
    metadata_now_ns, normalize_path, DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo,
    Timestamp,
};

/// Maximum number of symlinks followed during a single path resolution.
///
/// Matches Linux's `MAXSYMLINKS` (40).  Prevents infinite loops from
/// circular symlinks like `a → b → a`.
const MAX_SYMLINK_DEPTH: usize = 40;

/// Monotonic source of synthetic inode numbers for memfs nodes.
///
/// memfs has no on-disk inode table, but `stat()` callers (and programs
/// that detect file identity, e.g. hard-link dedup in `cp -a`/`tar`)
/// expect a stable, unique `st_ino` per object.  We assign one at node
/// creation from this global counter.  Starts at 1 so 0 stays reserved
/// for "not available" everywhere in the VFS.  The counter is process-
/// global across all memfs mounts; uniqueness within a single mount (all
/// that POSIX requires) is therefore guaranteed.  Wraparound after 2^64
/// allocations is not a practical concern.
static NEXT_MEMFS_INO: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

/// Allocate the next unique synthetic inode number for a memfs node.
fn alloc_memfs_ino() -> u64 {
    NEXT_MEMFS_INO.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Kind of a memory filesystem node.
enum MemFsNodeKind {
    /// A regular file with byte contents.
    File(Vec<u8>),
    /// A directory containing named children.
    Dir(BTreeMap<String, MemFsNode>),
    /// A symbolic link storing a target path string.
    ///
    /// The target is stored as-is (not resolved).  It can be absolute
    /// (starts with `/`) or relative (resolved from the symlink's
    /// parent directory).  Resolution happens during path traversal.
    Symlink(String),
}

/// A single node in the memory filesystem tree.
struct MemFsNode {
    kind: MemFsNodeKind,
    /// Stable synthetic inode number (assigned at creation, never reused
    /// for the lifetime of this node).  Surfaced as `st_ino`.
    ino: u64,
    /// Timestamps (wall-clock: nanoseconds since the Unix epoch).
    created_ns: Timestamp,
    modified_ns: Timestamp,
    accessed_ns: Timestamp,
    changed_ns: Timestamp,
    /// Ownership.
    uid: u32,
    gid: u32,
    /// Unix permission bits (rwxrwxrwx).
    permissions: u16,
    /// File attribute flags.
    attributes: FileAttr,
    /// Extended attributes (key-value pairs).
    xattrs: Vec<(String, Vec<u8>)>,
}

// --- Node-level xattr helpers (shared by follow / no-follow variants) ---
// These operate on an already-resolved node so the trait methods differ only
// in how the path is resolved (resolve/resolve_mut vs the no-follow pair).

/// Validate an xattr key/value shape before touching the node.
fn node_validate_xattr(key: &str, value: &[u8]) -> KernelResult<()> {
    // Enforce max key length (255 bytes) and max value size (64 KiB).
    if key.len() > 255 {
        return Err(KernelError::InvalidArgument);
    }
    if value.len() > 65536 {
        return Err(KernelError::InvalidArgument);
    }
    Ok(())
}

/// Read an xattr value from a resolved node.
fn node_get_xattr(node: &MemFsNode, key: &str) -> KernelResult<Vec<u8>> {
    for (k, v) in &node.xattrs {
        if k == key {
            return Ok(v.clone());
        }
    }
    Err(KernelError::NotFound)
}

/// Insert or replace an xattr on a resolved node.  Assumes the key/value have
/// already passed [`node_validate_xattr`].
fn node_set_xattr(node: &mut MemFsNode, key: &str, value: &[u8]) -> KernelResult<()> {
    let mut found = false;
    for (k, v) in &mut node.xattrs {
        if k == key {
            *v = value.to_vec();
            found = true;
            break;
        }
    }
    if !found {
        node.xattrs.push((String::from(key), value.to_vec()));
    }
    node.changed_ns = metadata_now_ns();
    Ok(())
}

/// Remove an xattr from a resolved node; `NotFound` if the key is absent.
fn node_remove_xattr(node: &mut MemFsNode, key: &str) -> KernelResult<()> {
    let orig_len = node.xattrs.len();
    node.xattrs.retain(|(k, _)| k != key);
    if node.xattrs.len() == orig_len {
        return Err(KernelError::NotFound);
    }
    node.changed_ns = metadata_now_ns();
    Ok(())
}

/// List all xattr keys on a resolved node.
fn node_list_xattrs(node: &MemFsNode) -> Vec<String> {
    node.xattrs.iter().map(|(k, _)| k.clone()).collect()
}

impl MemFsNode {
    fn new_file(data: Vec<u8>) -> Self {
        let now = metadata_now_ns();
        Self {
            kind: MemFsNodeKind::File(data),
            ino: alloc_memfs_ino(),
            created_ns: now,
            modified_ns: now,
            accessed_ns: now,
            changed_ns: now,
            uid: 0,
            gid: 0,
            permissions: 0o644,
            attributes: FileAttr::NONE,
            xattrs: Vec::new(),
        }
    }

    fn new_dir() -> Self {
        let now = metadata_now_ns();
        Self {
            kind: MemFsNodeKind::Dir(BTreeMap::new()),
            ino: alloc_memfs_ino(),
            created_ns: now,
            modified_ns: now,
            accessed_ns: now,
            changed_ns: now,
            uid: 0,
            gid: 0,
            permissions: 0o755,
            attributes: FileAttr::NONE,
            xattrs: Vec::new(),
        }
    }

    fn new_symlink(target: String) -> Self {
        let now = metadata_now_ns();
        Self {
            kind: MemFsNodeKind::Symlink(target),
            ino: alloc_memfs_ino(),
            created_ns: now,
            modified_ns: now,
            accessed_ns: now,
            changed_ns: now,
            uid: 0,
            gid: 0,
            // Symlinks are always 0o777 (permissions are on the target).
            permissions: 0o777,
            attributes: FileAttr::NONE,
            xattrs: Vec::new(),
        }
    }

    fn is_dir(&self) -> bool {
        matches!(self.kind, MemFsNodeKind::Dir(_))
    }

    fn is_file(&self) -> bool {
        matches!(self.kind, MemFsNodeKind::File(_))
    }

    #[allow(dead_code)] // Part of the MemFsNode type-query API, used as subsystems mature.
    fn is_symlink(&self) -> bool {
        matches!(self.kind, MemFsNodeKind::Symlink(_))
    }

    fn file_data(&self) -> Option<&Vec<u8>> {
        match &self.kind {
            MemFsNodeKind::File(data) => Some(data),
            _ => None,
        }
    }

    fn file_data_mut(&mut self) -> Option<&mut Vec<u8>> {
        match &mut self.kind {
            MemFsNodeKind::File(data) => Some(data),
            _ => None,
        }
    }

    fn children(&self) -> Option<&BTreeMap<String, MemFsNode>> {
        match &self.kind {
            MemFsNodeKind::Dir(children) => Some(children),
            _ => None,
        }
    }

    fn children_mut(&mut self) -> Option<&mut BTreeMap<String, MemFsNode>> {
        match &mut self.kind {
            MemFsNodeKind::Dir(children) => Some(children),
            _ => None,
        }
    }

    /// Symlink target string, if this is a symlink.
    fn symlink_target(&self) -> Option<&str> {
        match &self.kind {
            MemFsNodeKind::Symlink(target) => Some(target),
            _ => None,
        }
    }

    /// Size in bytes.
    ///
    /// - Files: data length.
    /// - Directories: 0.
    /// - Symlinks: length of the target path string (like Linux `lstat`).
    fn size(&self) -> u64 {
        match &self.kind {
            MemFsNodeKind::File(data) => data.len() as u64,
            MemFsNodeKind::Dir(_) => 0,
            MemFsNodeKind::Symlink(target) => target.len() as u64,
        }
    }

    /// Link count surfaced as `st_nlink`.
    ///
    /// memfs does not implement file hard links yet, so regular files and
    /// symlinks always report a single link.  Directories follow the Unix
    /// convention: a directory's link count is 2 (its own name in the
    /// parent directory plus its own `.` entry) plus one for each immediate
    /// subdirectory (each contributes a `..` entry pointing back to here).
    ///
    /// Reporting this honestly matters for tools that exploit it: `find(1)`
    /// uses the "leaf optimisation" — a directory whose `nlink == 2` has no
    /// subdirectories, so `find` can skip stat'ing its entries to decide
    /// whether to descend.  Hardcoding `1` defeated that optimisation and
    /// produced a link count no real filesystem ever reports for a directory.
    fn nlink_count(&self) -> u32 {
        match &self.kind {
            MemFsNodeKind::Dir(children) => {
                let subdirs = children.values().filter(|c| c.is_dir()).count();
                // 2 ("." + the name in the parent) + one ".." per immediate
                // subdirectory.  `saturating_add`/`try_from(..).unwrap_or`
                // keep this arithmetic-side-effect free and clamp the
                // (practically unreachable) > u32::MAX case.
                u32::try_from(subdirs.saturating_add(2)).unwrap_or(u32::MAX)
            }
            MemFsNodeKind::File(_) | MemFsNodeKind::Symlink(_) => 1,
        }
    }

    /// Entry type for this node.
    fn entry_type(&self) -> EntryType {
        match &self.kind {
            MemFsNodeKind::File(_) => EntryType::File,
            MemFsNodeKind::Dir(_) => EntryType::Directory,
            MemFsNodeKind::Symlink(_) => EntryType::Symlink,
        }
    }

    /// Convert to a VFS DirEntry.
    fn to_dir_entry(&self, name: &str) -> DirEntry {
        DirEntry {
            name: String::from(name),
            entry_type: self.entry_type(),
            size: self.size(),
        }
    }

    /// Convert to rich FileMeta.
    fn to_file_meta(&self) -> FileMeta {
        FileMeta {
            size: self.size(),
            entry_type: self.entry_type(),
            ino: self.ino,
            created_ns: self.created_ns,
            modified_ns: self.modified_ns,
            accessed_ns: self.accessed_ns,
            changed_ns: self.changed_ns,
            uid: self.uid,
            gid: self.gid,
            permissions: self.permissions,
            attributes: self.attributes,
            // Directories report 2 + immediate-subdir count (Unix `.`/`..`
            // convention); files and symlinks report 1 (no file hard links
            // yet).  See `nlink_count`.
            nlinks: self.nlink_count(),
            blocks: 0,
            xattrs: self.xattrs.clone(),
            hash: Vec::new(),
        }
    }

    /// Update modification and change timestamps to now.
    fn touch_modified(&mut self) {
        let now = metadata_now_ns();
        self.modified_ns = now;
        self.changed_ns = now;
    }

    /// Update access timestamp with relatime semantics.
    ///
    /// Only updates if accessed_ns < modified_ns or if more than
    /// one day has elapsed since last access.
    fn touch_accessed_relatime(&mut self) {
        let now = metadata_now_ns();
        // Relatime: only update if atime < mtime or older than 1 day.
        if self.accessed_ns < self.modified_ns
            || now.saturating_sub(self.accessed_ns) > 86_400_000_000_000
        {
            self.accessed_ns = now;
        }
    }
}

// ---------------------------------------------------------------------------
// MemFs filesystem
// ---------------------------------------------------------------------------

/// In-memory filesystem instance.
pub struct MemFs {
    /// Root directory node.
    root: MemFsNode,
}

impl MemFs {
    /// Create a new empty in-memory filesystem.
    pub fn new() -> Self {
        Self {
            root: MemFsNode::new_dir(),
        }
    }

    // -----------------------------------------------------------------------
    // Path helpers
    // -----------------------------------------------------------------------

    /// Split a path into components, filtering out empty parts and ".".
    fn path_components(path: &str) -> Vec<&str> {
        path.split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .collect()
    }

    /// Build the parent path from a set of components.
    ///
    /// `["a", "b", "c"]` → `"/a/b"`.  `["a"]` → `"/"`.
    fn parent_path_of(comps: &[&str]) -> String {
        if comps.len() <= 1 {
            return String::from("/");
        }
        let mut p = String::new();
        for c in &comps[..comps.len() - 1] {
            p.push('/');
            p.push_str(c);
        }
        p
    }

    // -----------------------------------------------------------------------
    // Symlink-aware path resolution
    // -----------------------------------------------------------------------

    /// Resolve a path to its canonical form, following symlinks.
    ///
    /// Walks the tree component by component.  When a symlink is
    /// encountered, substitutes the target and restarts from the
    /// appropriate point.
    ///
    /// `follow_last`: if `true`, follow the final component if it
    /// is a symlink.  If `false`, follow only intermediate symlinks.
    ///
    /// Returns the fully resolved path as an owned `String`.
    fn resolve_path_str(&self, path: &str, follow_last: bool) -> KernelResult<String> {
        let mut resolved = normalize_path(path);
        let mut depth = 0usize;

        loop {
            let components: Vec<&str> = resolved
                .split('/')
                .filter(|s| !s.is_empty())
                .collect();

            if components.is_empty() {
                return Ok(String::from("/"));
            }

            let mut current = &self.root;
            let mut hit_symlink = false;

            for (i, component) in components.iter().enumerate() {
                let is_last = i == components.len() - 1;
                let children = current.children().ok_or(KernelError::NotADirectory)?;
                let node = children.get(*component).ok_or(KernelError::NotFound)?;

                if let MemFsNodeKind::Symlink(ref target) = node.kind {
                    if is_last && !follow_last {
                        // Don't follow the final component.
                        return Ok(resolved);
                    }

                    depth = depth.wrapping_add(1);
                    if depth > MAX_SYMLINK_DEPTH {
                        return Err(KernelError::TooManyLinks);
                    }

                    // Build parent path (components before this symlink).
                    let parent = if i == 0 {
                        String::from("/")
                    } else {
                        let mut p = String::new();
                        for c in &components[..i] {
                            p.push('/');
                            p.push_str(c);
                        }
                        p
                    };

                    // Resolve target: absolute targets are used directly;
                    // relative targets are resolved from the parent directory.
                    let new_base = if target.starts_with('/') {
                        target.clone()
                    } else if parent == "/" {
                        let mut s = String::from("/");
                        s.push_str(target);
                        s
                    } else {
                        let mut s = parent;
                        s.push('/');
                        s.push_str(target);
                        s
                    };

                    // Append remaining path components after the symlink.
                    let remaining = &components[i + 1..];
                    resolved = if remaining.is_empty() {
                        normalize_path(&new_base)
                    } else {
                        let mut full = new_base;
                        for r in remaining {
                            full.push('/');
                            full.push_str(r);
                        }
                        normalize_path(&full)
                    };

                    hit_symlink = true;
                    break;
                }

                current = node;
            }

            if !hit_symlink {
                return Ok(resolved);
            }
        }
    }

    /// Walk a path WITHOUT following symlinks.
    ///
    /// Used after [`resolve_path_str`] has already resolved all symlinks.
    fn walk(&self, path: &str) -> KernelResult<&MemFsNode> {
        let components = Self::path_components(path);
        if components.is_empty() {
            return Ok(&self.root);
        }
        let mut current = &self.root;
        for component in &components {
            let children = current.children().ok_or(KernelError::NotADirectory)?;
            current = children.get(*component).ok_or(KernelError::NotFound)?;
        }
        Ok(current)
    }

    /// Walk a path without following symlinks (mutable).
    fn walk_mut(&mut self, path: &str) -> KernelResult<&mut MemFsNode> {
        let components = Self::path_components(path);
        if components.is_empty() {
            return Ok(&mut self.root);
        }
        let mut current = &mut self.root;
        for component in &components {
            let children = current.children_mut().ok_or(KernelError::NotADirectory)?;
            current = children.get_mut(*component).ok_or(KernelError::NotFound)?;
        }
        Ok(current)
    }

    // -----------------------------------------------------------------------
    // Public resolve helpers (used by FileSystem trait impls)
    // -----------------------------------------------------------------------

    /// Resolve a path, following ALL symlinks (including the final one).
    fn resolve(&self, path: &str) -> KernelResult<&MemFsNode> {
        let resolved = self.resolve_path_str(path, true)?;
        self.walk(&resolved)
    }

    /// Resolve a path mutably, following ALL symlinks.
    fn resolve_mut(&mut self, path: &str) -> KernelResult<&mut MemFsNode> {
        // Phase 1: resolve symlinks immutably → owned String.
        let resolved = self.resolve_path_str(path, true)?;
        // Phase 2: walk the resolved path (no symlinks left).
        self.walk_mut(&resolved)
    }

    /// Resolve a path, following intermediate symlinks but NOT the
    /// final component.  Used by `lstat` and `readlink`.
    fn resolve_no_follow(&self, path: &str) -> KernelResult<&MemFsNode> {
        let resolved = self.resolve_path_str(path, false)?;
        self.walk(&resolved)
    }

    /// Resolve a path mutably, following intermediate symlinks but NOT the
    /// final component.  Used by `lchown`/`lutimes`-style operations that
    /// must mutate the symlink inode itself, not its target.
    fn resolve_no_follow_mut(&mut self, path: &str) -> KernelResult<&mut MemFsNode> {
        // Phase 1: resolve intermediate symlinks immutably → owned String
        // whose final component is left unfollowed.
        let resolved = self.resolve_path_str(path, false)?;
        // Phase 2: walk the resolved path without following the final link.
        self.walk_mut(&resolved)
    }

    /// Resolve the parent directory of a path (following symlinks in
    /// intermediate components) and return `(parent_node, filename)`.
    ///
    /// The filename is the last component of the original path (not
    /// followed if it's a symlink).  The parent path IS fully resolved.
    fn resolve_parent_mut<'a, 'b>(
        &'a mut self,
        path: &'b str,
    ) -> KernelResult<(&'a mut MemFsNode, &'b str)> {
        let components = Self::path_components(path);
        if components.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        let filename = components[components.len() - 1];
        let parent_path = Self::parent_path_of(&components);

        // Resolve the parent (following all symlinks in the parent path).
        let resolved_parent = self.resolve_path_str(&parent_path, true)?;

        let parent = self.walk_mut(&resolved_parent)?;
        if !parent.is_dir() {
            return Err(KernelError::NotADirectory);
        }

        Ok((parent, filename))
    }

    /// Resolve the write target for a file operation.
    ///
    /// Follows symlinks on all components (including the final one).
    /// If the final component doesn't exist, resolves the parent and
    /// returns the parent's resolved path + the original filename so
    /// the caller can create a new entry.
    ///
    /// Returns `(resolved_parent_path, filename)`.
    fn resolve_write_path(&self, path: &str) -> KernelResult<(String, String)> {
        let mut current_path = normalize_path(path);
        let mut depth = 0usize;

        loop {
            let comps = Self::path_components(&current_path);
            if comps.is_empty() {
                return Err(KernelError::InvalidArgument);
            }

            let filename = String::from(comps[comps.len() - 1]);
            let parent_path = Self::parent_path_of(&comps);

            // Resolve the parent path (following all symlinks).
            let resolved_parent = self.resolve_path_str(&parent_path, true)?;

            // Check if filename exists in the resolved parent.
            let parent_node = self.walk(&resolved_parent)?;
            let children = parent_node.children().ok_or(KernelError::NotADirectory)?;

            match children.get(&*filename) {
                Some(node) => {
                    if let MemFsNodeKind::Symlink(ref target) = node.kind {
                        // Follow the symlink.
                        depth = depth.wrapping_add(1);
                        if depth > MAX_SYMLINK_DEPTH {
                            return Err(KernelError::TooManyLinks);
                        }
                        current_path = if target.starts_with('/') {
                            normalize_path(target)
                        } else if resolved_parent == "/" {
                            let mut s = String::from("/");
                            s.push_str(target);
                            normalize_path(&s)
                        } else {
                            let mut s = resolved_parent;
                            s.push('/');
                            s.push_str(target);
                            normalize_path(&s)
                        };
                        continue;
                    }
                    // Not a symlink — write here.
                    return Ok((resolved_parent, filename));
                }
                None => {
                    // Doesn't exist — create here.
                    return Ok((resolved_parent, filename));
                }
            }
        }
    }
}

impl FileSystem for MemFs {
    fn fs_type(&self) -> &'static str {
        "memfs"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let node = self.resolve(path)?;
        let children = node.children().ok_or(KernelError::NotADirectory)?;

        let entries: Vec<DirEntry> = children
            .iter()
            .map(|(name, child)| child.to_dir_entry(name))
            .collect();

        Ok(entries)
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        // Two-phase: resolve immutably to get data, then update atime.
        let data = {
            let node = self.resolve(path)?;
            let d = node.file_data().ok_or(KernelError::IsADirectory)?;
            d.clone()
        };
        // Relatime: update access timestamp if stale.
        if let Ok(node) = self.resolve_mut(path) {
            node.touch_accessed_relatime();
        }
        Ok(data)
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let components = Self::path_components(path);
        if components.is_empty() {
            // Root directory.
            return Ok(DirEntry {
                name: String::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            });
        }

        let name = components[components.len() - 1];
        let node = self.resolve(path)?;
        Ok(node.to_dir_entry(name))
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        // Follow symlinks to find the actual write target.
        let (parent_path, filename) = self.resolve_write_path(path)?;

        let parent = self.walk_mut(&parent_path)?;
        let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;

        match children.get_mut(&*filename) {
            Some(existing) => {
                // Enforce attribute restrictions.
                if existing.attributes.contains(FileAttr::IMMUTABLE) {
                    return Err(KernelError::PermissionDenied);
                }
                if existing.is_dir() {
                    return Err(KernelError::IsADirectory);
                }
                // Append-only: reject full overwrites (use write_at for appends).
                if existing.attributes.contains(FileAttr::APPEND_ONLY) {
                    return Err(KernelError::PermissionDenied);
                }
                let file_data = existing
                    .file_data_mut()
                    .ok_or(KernelError::IsADirectory)?;
                file_data.clear();
                file_data.extend_from_slice(data);
                // NLL: file_data borrow ends here (last use above).
                existing.touch_modified();
            }
            None => {
                // Create new file (constructor sets timestamps to now).
                children.insert(filename, MemFsNode::new_file(data.to_vec()));
            }
        }
        parent.touch_modified();
        Ok(())
    }

    fn remove(&mut self, path: &str) -> KernelResult<()> {
        // remove() does NOT follow the final component — it removes the
        // entry itself (file or symlink).  Intermediate symlinks ARE followed.
        let (parent, filename) = self.resolve_parent_mut(path)?;
        let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;

        let node = children.get(filename).ok_or(KernelError::NotFound)?;
        if node.is_dir() {
            return Err(KernelError::IsADirectory);
        }
        if node.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::PermissionDenied);
        }
        children.remove(filename);
        parent.touch_modified();
        Ok(())
    }

    fn mkdir(&mut self, path: &str) -> KernelResult<()> {
        // mkdir does NOT follow the final component — if the name
        // already exists (even as a symlink), it returns AlreadyExists.
        let (parent, dirname) = self.resolve_parent_mut(path)?;
        if parent.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::PermissionDenied);
        }
        let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;

        if children.contains_key(dirname) {
            return Err(KernelError::AlreadyExists);
        }

        children.insert(String::from(dirname), MemFsNode::new_dir());
        parent.touch_modified();
        Ok(())
    }

    fn rmdir(&mut self, path: &str) -> KernelResult<()> {
        // rmdir does NOT follow the final component — a symlink at the
        // end returns NotADirectory (like Linux).
        let (parent, dirname) = self.resolve_parent_mut(path)?;
        let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;

        let node = children.get(dirname).ok_or(KernelError::NotFound)?;
        if !node.is_dir() {
            return Err(KernelError::NotADirectory);
        }
        if node.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::PermissionDenied);
        }

        // Must be empty.
        if let Some(grandchildren) = node.children() {
            if !grandchildren.is_empty() {
                return Err(KernelError::InvalidArgument); // Directory not empty.
            }
        }

        children.remove(dirname);
        parent.touch_modified();
        Ok(())
    }

    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let result = {
            let node = self.resolve(path)?;
            let data = node.file_data().ok_or(KernelError::IsADirectory)?;
            let start = (offset as usize).min(data.len());
            let end = (start.saturating_add(len)).min(data.len());
            data.get(start..end).map_or_else(Vec::new, |s| s.to_vec())
        };
        // Relatime: update access timestamp if stale.
        if let Ok(node) = self.resolve_mut(path) {
            node.touch_accessed_relatime();
        }
        Ok(result)
    }

    fn write_at(&mut self, path: &str, offset: u64, data: &[u8]) -> KernelResult<()> {
        let node = match self.resolve_mut(path) {
            Ok(n) => n,
            Err(KernelError::NotFound) => {
                // Create the file first (follows symlinks for creation target).
                self.write_file(path, &[])?;
                self.resolve_mut(path)?
            }
            Err(e) => return Err(e),
        };

        // Enforce attribute restrictions before borrowing file_data.
        let attrs = node.attributes;
        if attrs.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::PermissionDenied);
        }
        if !node.is_file() {
            return Err(KernelError::IsADirectory);
        }
        // Check append-only: get current length before mutable borrow.
        let current_len = node.size() as usize;
        if attrs.contains(FileAttr::APPEND_ONLY) && (offset as usize) != current_len {
            return Err(KernelError::PermissionDenied);
        }

        // Now perform the write.
        let file_data = node.file_data_mut().ok_or(KernelError::IsADirectory)?;

        let start = offset as usize;
        let end = start.saturating_add(data.len());

        // Extend if writing past current end.
        if end > file_data.len() {
            file_data.resize(end, 0);
        }

        if let Some(dest) = file_data.get_mut(start..end) {
            dest.copy_from_slice(data);
        }

        // NLL: file_data borrow ends here (last use is the copy above).
        node.touch_modified();
        Ok(())
    }

    fn truncate(&mut self, path: &str, size: u64) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        // Check attributes before getting mutable data reference.
        if node.attributes.contains(FileAttr::IMMUTABLE)
            || node.attributes.contains(FileAttr::APPEND_ONLY)
        {
            return Err(KernelError::PermissionDenied);
        }
        let file_data = node.file_data_mut().ok_or(KernelError::IsADirectory)?;
        file_data.resize(size as usize, 0);
        // NLL: file_data borrow ends here (last use is the resize above).
        node.touch_modified();
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> KernelResult<()> {
        // rename() does NOT follow the final component for either source
        // or destination — it moves the entry itself (including symlinks).
        // Intermediate components ARE resolved through symlinks.

        // Resolve both parents (following intermediate symlinks).
        let from_comps = Self::path_components(from);
        let to_comps = Self::path_components(to);
        if from_comps.is_empty() || to_comps.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        let from_name = String::from(from_comps[from_comps.len() - 1]);
        let to_name = String::from(to_comps[to_comps.len() - 1]);

        let from_parent_path = Self::parent_path_of(&from_comps);
        let to_parent_path = Self::parent_path_of(&to_comps);

        let resolved_from_parent = self.resolve_path_str(&from_parent_path, true)?;
        let resolved_to_parent = self.resolve_path_str(&to_parent_path, true)?;

        // Check that destination doesn't already exist (before removing source).
        {
            let to_parent = self.walk(&resolved_to_parent)?;
            let to_children = to_parent.children().ok_or(KernelError::NotADirectory)?;
            if to_children.contains_key(&*to_name) {
                return Err(KernelError::AlreadyExists);
            }
        }

        // Remove source node.
        let removed_node = {
            let from_parent = self.walk_mut(&resolved_from_parent)?;
            let children = from_parent
                .children_mut()
                .ok_or(KernelError::NotADirectory)?;
            children.remove(&*from_name).ok_or(KernelError::NotFound)?
        };

        // Insert at destination.
        let to_parent = self.walk_mut(&resolved_to_parent)?;
        let children = to_parent
            .children_mut()
            .ok_or(KernelError::NotADirectory)?;
        children.insert(to_name, removed_node);
        Ok(())
    }

    fn rename_exchange(&mut self, a: &str, b: &str) -> KernelResult<()> {
        // Atomically swap two existing entries. Like rename(), the final
        // component is NOT followed for either path; intermediate components
        // ARE resolved through symlinks. Both entries must exist.
        let a_comps = Self::path_components(a);
        let b_comps = Self::path_components(b);
        if a_comps.is_empty() || b_comps.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        let a_name = String::from(a_comps[a_comps.len() - 1]);
        let b_name = String::from(b_comps[b_comps.len() - 1]);

        let a_parent_path = Self::parent_path_of(&a_comps);
        let b_parent_path = Self::parent_path_of(&b_comps);

        let resolved_a_parent = self.resolve_path_str(&a_parent_path, true)?;
        let resolved_b_parent = self.resolve_path_str(&b_parent_path, true)?;

        // Exchanging an entry with itself is a no-op (but the entry must
        // still exist, else ENOENT).
        if resolved_a_parent == resolved_b_parent && a_name == b_name {
            let parent = self.walk(&resolved_a_parent)?;
            let children = parent.children().ok_or(KernelError::NotADirectory)?;
            if !children.contains_key(&*a_name) {
                return Err(KernelError::NotFound);
            }
            return Ok(());
        }

        // Detach a's node (must exist).
        let node_a = {
            let parent = self.walk_mut(&resolved_a_parent)?;
            let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;
            children.remove(&*a_name).ok_or(KernelError::NotFound)?
        };

        // Detach b's node; if it does not exist, restore a and fail so the
        // exchange is all-or-nothing.
        let node_b_result = match self.walk_mut(&resolved_b_parent) {
            Ok(parent) => match parent.children_mut() {
                Some(children) => children.remove(&*b_name).ok_or(KernelError::NotFound),
                None => Err(KernelError::NotADirectory),
            },
            Err(e) => Err(e),
        };
        let node_b = match node_b_result {
            Ok(n) => n,
            Err(e) => {
                // Roll back the detach of a (its parent existed a moment ago).
                if let Ok(parent) = self.walk_mut(&resolved_a_parent) {
                    if let Some(children) = parent.children_mut() {
                        children.insert(a_name, node_a);
                    }
                }
                return Err(e);
            }
        };

        // Re-attach swapped: b's node at a's location, a's node at b's.
        {
            let parent_a = self.walk_mut(&resolved_a_parent)?;
            let children = parent_a.children_mut().ok_or(KernelError::NotADirectory)?;
            children.insert(a_name, node_b);
        }
        {
            let parent_b = self.walk_mut(&resolved_b_parent)?;
            let children = parent_b.children_mut().ok_or(KernelError::NotADirectory)?;
            children.insert(b_name, node_a);
        }
        Ok(())
    }

    fn debug_stats(&self) -> String {
        fn count_nodes(node: &MemFsNode) -> (usize, usize, usize, u64) {
            match &node.kind {
                MemFsNodeKind::File(data) => (1, 0, 0, data.len() as u64),
                MemFsNodeKind::Dir(children) => {
                    let mut files = 0usize;
                    let mut dirs = 1usize; // Count this dir.
                    let mut links = 0usize;
                    let mut bytes = 0u64;
                    for child in children.values() {
                        let (f, d, l, b) = count_nodes(child);
                        files = files.wrapping_add(f);
                        dirs = dirs.wrapping_add(d);
                        links = links.wrapping_add(l);
                        bytes = bytes.wrapping_add(b);
                    }
                    (files, dirs, links, bytes)
                }
                MemFsNodeKind::Symlink(_) => (0, 0, 1, 0),
            }
        }

        let (files, dirs, links, bytes) = count_nodes(&self.root);
        use core::fmt::Write;
        let mut s = String::new();
        let _ = write!(
            s,
            "memfs: {} files, {} dirs, {} symlinks, {} bytes",
            files, dirs, links, bytes
        );
        s
    }

    // --- Extended metadata operations ---

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let node = self.resolve(path)?;
        Ok(node.to_file_meta())
    }

    fn lmetadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        // No-follow: return the trailing symlink's own metadata rather
        // than its target's.  Mirrors `metadata` but uses the
        // non-following resolver.
        let node = self.resolve_no_follow(path)?;
        Ok(node.to_file_meta())
    }

    fn set_attributes(&mut self, path: &str, attrs: FileAttr) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.attributes = attrs;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    fn set_owner(&mut self, path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.uid = uid;
        node.gid = gid;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    fn set_permissions(&mut self, path: &str, permissions: u16) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.permissions = permissions;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    fn set_times(
        &mut self,
        path: &str,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        if accessed_ns != 0 {
            node.accessed_ns = accessed_ns;
        }
        if modified_ns != 0 {
            node.modified_ns = modified_ns;
        }
        Ok(())
    }

    /// `lchown`/`fchownat(AT_SYMLINK_NOFOLLOW)`: chown the link inode itself,
    /// not its target.  Identical to [`set_owner`](Self::set_owner) but the
    /// final path component is resolved WITHOUT following a symlink.
    fn set_owner_no_follow(&mut self, path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        let node = self.resolve_no_follow_mut(path)?;
        node.uid = uid;
        node.gid = gid;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    /// `fchmodat2(AT_SYMLINK_NOFOLLOW)`: set mode bits on the link inode
    /// itself.  Same as [`set_permissions`](Self::set_permissions) but the
    /// final path component is resolved WITHOUT following a symlink.
    fn set_permissions_no_follow(&mut self, path: &str, permissions: u16) -> KernelResult<()> {
        let node = self.resolve_no_follow_mut(path)?;
        node.permissions = permissions;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    /// `lutimes`/`utimensat(AT_SYMLINK_NOFOLLOW)`: stamp the link inode
    /// itself.  Same as [`set_times`](Self::set_times) but no-follow.
    fn set_times_no_follow(
        &mut self,
        path: &str,
        accessed_ns: Timestamp,
        modified_ns: Timestamp,
    ) -> KernelResult<()> {
        let node = self.resolve_no_follow_mut(path)?;
        if accessed_ns != 0 {
            node.accessed_ns = accessed_ns;
        }
        if modified_ns != 0 {
            node.modified_ns = modified_ns;
        }
        Ok(())
    }

    fn get_xattr(&mut self, path: &str, key: &str) -> KernelResult<Vec<u8>> {
        node_get_xattr(self.resolve(path)?, key)
    }

    fn set_xattr(&mut self, path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        // Validation happens before path resolution so a bad key/value shape
        // is rejected identically regardless of follow mode.
        node_validate_xattr(key, value)?;
        node_set_xattr(self.resolve_mut(path)?, key, value)
    }

    fn remove_xattr(&mut self, path: &str, key: &str) -> KernelResult<()> {
        node_remove_xattr(self.resolve_mut(path)?, key)
    }

    fn list_xattrs(&mut self, path: &str) -> KernelResult<Vec<String>> {
        Ok(node_list_xattrs(self.resolve(path)?))
    }

    // --- No-follow xattr variants (l-prefixed: lgetxattr/lsetxattr/etc.) ---
    // Operate on the symlink inode itself rather than its target.  Identical
    // to the following versions but the final component is not followed.

    fn get_xattr_no_follow(&mut self, path: &str, key: &str) -> KernelResult<Vec<u8>> {
        node_get_xattr(self.resolve_no_follow(path)?, key)
    }

    fn set_xattr_no_follow(&mut self, path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        node_validate_xattr(key, value)?;
        node_set_xattr(self.resolve_no_follow_mut(path)?, key, value)
    }

    fn remove_xattr_no_follow(&mut self, path: &str, key: &str) -> KernelResult<()> {
        node_remove_xattr(self.resolve_no_follow_mut(path)?, key)
    }

    fn list_xattrs_no_follow(&mut self, path: &str) -> KernelResult<Vec<String>> {
        Ok(node_list_xattrs(self.resolve_no_follow(path)?))
    }

    // --- Symlink operations ---

    fn symlink(&mut self, path: &str, target: &str) -> KernelResult<()> {
        if target.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        // Validate target length (symlink targets use the same limit as
        // path components).
        if target.len() > 4096 {
            return Err(KernelError::InvalidArgument);
        }

        let (parent, linkname) = self.resolve_parent_mut(path)?;
        if parent.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::PermissionDenied);
        }
        let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;

        if children.contains_key(linkname) {
            return Err(KernelError::AlreadyExists);
        }

        children.insert(
            String::from(linkname),
            MemFsNode::new_symlink(String::from(target)),
        );
        parent.touch_modified();
        Ok(())
    }

    fn readlink(&mut self, path: &str) -> KernelResult<String> {
        // readlink does NOT follow the final component.
        let node = self.resolve_no_follow(path)?;
        match node.symlink_target() {
            Some(target) => Ok(String::from(target)),
            None => Err(KernelError::InvalidArgument), // Not a symlink.
        }
    }

    fn lstat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let components = Self::path_components(path);
        if components.is_empty() {
            return Ok(DirEntry {
                name: String::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            });
        }

        let name = components[components.len() - 1];
        let node = self.resolve_no_follow(path)?;
        Ok(node.to_dir_entry(name))
    }

    /// Report memfs usage.
    ///
    /// Since memfs is RAM-backed, total capacity is essentially unlimited
    /// (bounded by heap size).  We report the current used byte count.
    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        fn count_nodes(node: &MemFsNode) -> (u64, u64) {
            match &node.kind {
                MemFsNodeKind::File(data) => (data.len() as u64, 1),
                MemFsNodeKind::Dir(children) => {
                    let mut bytes = 0u64;
                    let mut count = 1u64; // Count this dir.
                    for child in children.values() {
                        let (b, c) = count_nodes(child);
                        bytes = bytes.wrapping_add(b);
                        count = count.wrapping_add(c);
                    }
                    (bytes, count)
                }
                MemFsNodeKind::Symlink(_) => (0, 1),
            }
        }

        let (_used_bytes, node_count) = count_nodes(&self.root);

        Ok(FsInfo {
            fs_type: String::from("memfs"),
            volume_label: String::new(),
            block_size: 1, // Byte-granular allocation.
            total_blocks: 0, // Unlimited (bounded by heap).
            free_blocks: 0,
            total_inodes: node_count,
            free_inodes: 0, // Unlimited.
            max_name_len: 255,
            read_only: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Constructor and self-test
// ---------------------------------------------------------------------------

/// Mount a new in-memory filesystem at the given path.
pub fn mount(mount_path: &str) -> KernelResult<()> {
    let fs = MemFs::new();
    crate::fs::Vfs::mount(mount_path, Box::new(fs))?;
    Ok(())
}

/// Self-test: verify basic MemFs operations including symlinks.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[memfs] Running self-test...");

    // Create a standalone MemFs instance (don't mount globally — avoid
    // interfering with the real VFS mount at /).
    let mut fs = MemFs::new();

    // Test mkdir.
    fs.mkdir("/testdir")?;
    let entries = fs.readdir("/")?;
    let has_testdir = entries
        .iter()
        .any(|e| e.name == "testdir" && e.entry_type == EntryType::Directory);
    if !has_testdir {
        crate::serial_println!("[memfs]   FAILED: testdir not in root");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   mkdir: OK");

    // Test write_file + read_file.
    let test_data = b"Hello from MemFs!";
    fs.write_file("/testdir/hello.txt", test_data)?;
    let readback = fs.read_file("/testdir/hello.txt")?;
    if readback.as_slice() != test_data.as_slice() {
        crate::serial_println!("[memfs]   FAILED: write/read mismatch");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   write_file + read_file: OK");

    // Test stat.
    let stat = fs.stat("/testdir/hello.txt")?;
    if stat.size != test_data.len() as u64 || stat.entry_type != EntryType::File {
        crate::serial_println!("[memfs]   FAILED: stat mismatch");
        return Err(KernelError::IoError);
    }

    // Test case sensitivity: "Hello.txt" should NOT find "hello.txt".
    match fs.read_file("/testdir/Hello.txt") {
        Err(KernelError::NotFound) => {
            crate::serial_println!("[memfs]   Case sensitivity: OK (Hello.txt != hello.txt)");
        }
        Ok(_) => {
            crate::serial_println!("[memfs]   FAILED: case-insensitive match");
            return Err(KernelError::IoError);
        }
        Err(e) => return Err(e),
    }

    // Test read_at.
    let partial = fs.read_at("/testdir/hello.txt", 6, 4)?;
    if partial.as_slice() != b"from" {
        crate::serial_println!(
            "[memfs]   FAILED: read_at expected 'from', got {:?}",
            core::str::from_utf8(&partial)
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   read_at: OK");

    // Test write_at (extend).
    fs.write_at("/testdir/hello.txt", 17, b" Extended!")?;
    let extended = fs.read_file("/testdir/hello.txt")?;
    if extended.as_slice() != b"Hello from MemFs! Extended!" {
        crate::serial_println!("[memfs]   FAILED: write_at extend");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   write_at: OK");

    // Test truncate.
    fs.truncate("/testdir/hello.txt", 5)?;
    let truncated = fs.read_file("/testdir/hello.txt")?;
    if truncated.as_slice() != b"Hello" {
        crate::serial_println!("[memfs]   FAILED: truncate");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   truncate: OK");

    // Test rename.
    fs.rename("/testdir/hello.txt", "/testdir/renamed.txt")?;
    match fs.read_file("/testdir/hello.txt") {
        Err(KernelError::NotFound) => {}
        _ => {
            crate::serial_println!("[memfs]   FAILED: old name still exists after rename");
            return Err(KernelError::IoError);
        }
    }
    let renamed_data = fs.read_file("/testdir/renamed.txt")?;
    if renamed_data.as_slice() != b"Hello" {
        crate::serial_println!("[memfs]   FAILED: renamed file data mismatch");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   rename: OK");

    // Test remove.
    fs.remove("/testdir/renamed.txt")?;
    match fs.read_file("/testdir/renamed.txt") {
        Err(KernelError::NotFound) => {}
        _ => {
            crate::serial_println!("[memfs]   FAILED: file still exists after remove");
            return Err(KernelError::IoError);
        }
    }

    // Test rmdir.
    fs.rmdir("/testdir")?;
    match fs.readdir("/testdir") {
        Err(KernelError::NotFound) => {}
        _ => {
            crate::serial_println!("[memfs]   FAILED: dir still exists after rmdir");
            return Err(KernelError::IoError);
        }
    }
    crate::serial_println!("[memfs]   remove + rmdir: OK");

    // Test rmdir on non-empty directory.
    fs.mkdir("/notempty")?;
    fs.write_file("/notempty/file.txt", b"data")?;
    match fs.rmdir("/notempty") {
        Err(KernelError::InvalidArgument) => {
            crate::serial_println!("[memfs]   rmdir non-empty: correctly rejected");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: rmdir non-empty should fail");
            return Err(KernelError::IoError);
        }
    }
    // Clean up.
    fs.remove("/notempty/file.txt")?;
    fs.rmdir("/notempty")?;

    // Test debug_stats.
    fs.write_file("/a.txt", b"aaa")?;
    fs.write_file("/b.txt", b"bbb")?;
    let stats = fs.debug_stats();
    crate::serial_println!("[memfs]   {}", stats);
    fs.remove("/a.txt")?;
    fs.remove("/b.txt")?;

    // --- Metadata tests ---

    // Test metadata timestamps are set.
    fs.write_file("/meta.txt", b"metadata test")?;
    let meta = fs.metadata("/meta.txt")?;
    if meta.created_ns == 0 || meta.modified_ns == 0 || meta.accessed_ns == 0 {
        crate::serial_println!("[memfs]   FAILED: timestamps not set");
        return Err(KernelError::IoError);
    }
    if meta.entry_type != EntryType::File || meta.size != 13 {
        crate::serial_println!("[memfs]   FAILED: metadata type/size mismatch");
        return Err(KernelError::IoError);
    }
    if meta.permissions != 0o644 {
        crate::serial_println!("[memfs]   FAILED: file permissions not 0644");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   metadata (timestamps, permissions): OK");

    // Test set_permissions.
    fs.set_permissions("/meta.txt", 0o755)?;
    let meta2 = fs.metadata("/meta.txt")?;
    if meta2.permissions != 0o755 {
        crate::serial_println!("[memfs]   FAILED: permissions not updated");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   set_permissions: OK");

    // Test set_owner.
    fs.set_owner("/meta.txt", 1000, 1000)?;
    let meta3 = fs.metadata("/meta.txt")?;
    if meta3.uid != 1000 || meta3.gid != 1000 {
        crate::serial_println!("[memfs]   FAILED: owner not updated");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   set_owner: OK");

    // Test immutable attribute.
    fs.set_attributes("/meta.txt", FileAttr::IMMUTABLE)?;
    match fs.write_file("/meta.txt", b"should fail") {
        Err(KernelError::PermissionDenied) => {
            crate::serial_println!("[memfs]   immutable write rejected: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: immutable write should fail");
            return Err(KernelError::IoError);
        }
    }
    match fs.remove("/meta.txt") {
        Err(KernelError::PermissionDenied) => {
            crate::serial_println!("[memfs]   immutable remove rejected: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: immutable remove should fail");
            return Err(KernelError::IoError);
        }
    }
    // Clear immutable to clean up.
    fs.set_attributes("/meta.txt", FileAttr::NONE)?;

    // Test append-only attribute.
    fs.set_attributes("/meta.txt", FileAttr::APPEND_ONLY)?;
    match fs.truncate("/meta.txt", 0) {
        Err(KernelError::PermissionDenied) => {
            crate::serial_println!("[memfs]   append-only truncate rejected: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: append-only truncate should fail");
            return Err(KernelError::IoError);
        }
    }
    fs.set_attributes("/meta.txt", FileAttr::NONE)?;

    // Test extended attributes.
    fs.set_xattr("/meta.txt", "user.tag", b"important")?;
    let xval = fs.get_xattr("/meta.txt", "user.tag")?;
    if xval.as_slice() != b"important" {
        crate::serial_println!("[memfs]   FAILED: xattr value mismatch");
        return Err(KernelError::IoError);
    }
    let xkeys = fs.list_xattrs("/meta.txt")?;
    if xkeys.len() != 1 || xkeys[0] != "user.tag" {
        crate::serial_println!("[memfs]   FAILED: xattr list mismatch");
        return Err(KernelError::IoError);
    }
    fs.remove_xattr("/meta.txt", "user.tag")?;
    let xkeys2 = fs.list_xattrs("/meta.txt")?;
    if !xkeys2.is_empty() {
        crate::serial_println!("[memfs]   FAILED: xattr not removed");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   extended attributes: OK");

    // Clean up.
    fs.remove("/meta.txt")?;

    // --- Symlink tests ---
    test_symlinks(&mut fs)?;

    crate::serial_println!("[memfs] Self-test PASSED");
    Ok(())
}

/// Symlink-specific tests.
#[allow(clippy::arithmetic_side_effects)]
fn test_symlinks(fs: &mut MemFs) -> KernelResult<()> {
    // Create a file and a symlink to it.
    fs.write_file("/target.txt", b"symlink target data")?;
    fs.symlink("/link.txt", "target.txt")?;

    // readlink returns the stored target.
    let target = fs.readlink("/link.txt")?;
    if target != "target.txt" {
        crate::serial_println!("[memfs]   FAILED: readlink got '{}'", target);
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   symlink + readlink: OK");

    // stat follows the symlink (returns the target's info).
    let st = fs.stat("/link.txt")?;
    if st.entry_type != EntryType::File || st.size != 19 {
        crate::serial_println!("[memfs]   FAILED: stat through symlink");
        return Err(KernelError::IoError);
    }

    // lstat does NOT follow (returns the symlink's own info).
    let lst = fs.lstat("/link.txt")?;
    if lst.entry_type != EntryType::Symlink {
        crate::serial_println!("[memfs]   FAILED: lstat type not Symlink");
        return Err(KernelError::IoError);
    }
    // Symlink size = target string length.
    if lst.size != 10 {
        crate::serial_println!("[memfs]   FAILED: lstat size {} != 10", lst.size);
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   stat vs lstat: OK");

    // read_file through symlink.
    let data = fs.read_file("/link.txt")?;
    if data.as_slice() != b"symlink target data" {
        crate::serial_println!("[memfs]   FAILED: read through symlink");
        return Err(KernelError::IoError);
    }

    // write_file through symlink overwrites the target.
    fs.write_file("/link.txt", b"overwritten")?;
    let data2 = fs.read_file("/target.txt")?;
    if data2.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: write through symlink");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   read/write through symlink: OK");

    // remove on the symlink removes the link, not the target.
    fs.remove("/link.txt")?;
    let target_data = fs.read_file("/target.txt")?;
    if target_data.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: remove symlink deleted target");
        return Err(KernelError::IoError);
    }
    match fs.read_file("/link.txt") {
        Err(KernelError::NotFound) => {}
        _ => {
            crate::serial_println!("[memfs]   FAILED: symlink still exists after remove");
            return Err(KernelError::IoError);
        }
    }
    crate::serial_println!("[memfs]   remove symlink (not target): OK");

    // Symlink to a directory.
    fs.mkdir("/realdir")?;
    fs.write_file("/realdir/file.txt", b"in realdir")?;
    fs.symlink("/dirlink", "realdir")?;
    let entries = fs.readdir("/dirlink")?;
    let has_file = entries.iter().any(|e| e.name == "file.txt");
    if !has_file {
        crate::serial_println!("[memfs]   FAILED: readdir through dir symlink");
        return Err(KernelError::IoError);
    }
    // Access file through the dir symlink.
    let nested = fs.read_file("/dirlink/file.txt")?;
    if nested.as_slice() != b"in realdir" {
        crate::serial_println!("[memfs]   FAILED: read file through dir symlink");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   directory symlink traversal: OK");

    // Symlink chain: a → b → target.txt
    fs.symlink("/chain_b", "target.txt")?;
    fs.symlink("/chain_a", "chain_b")?;
    let chain_data = fs.read_file("/chain_a")?;
    if chain_data.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: symlink chain");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   symlink chain (a->b->file): OK");

    // Circular symlink detection.
    fs.symlink("/circ_a", "circ_b")?;
    fs.symlink("/circ_b", "circ_a")?;
    match fs.read_file("/circ_a") {
        Err(KernelError::TooManyLinks) => {
            crate::serial_println!("[memfs]   circular symlink detected: OK");
        }
        Ok(_) => {
            crate::serial_println!("[memfs]   FAILED: circular symlink not detected");
            return Err(KernelError::IoError);
        }
        Err(e) => {
            crate::serial_println!("[memfs]   FAILED: circular symlink got {:?}", e);
            return Err(KernelError::IoError);
        }
    }

    // Dangling symlink.
    fs.symlink("/dangling", "nonexistent.txt")?;
    match fs.read_file("/dangling") {
        Err(KernelError::NotFound) => {
            crate::serial_println!("[memfs]   dangling symlink -> NotFound: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: dangling symlink should be NotFound");
            return Err(KernelError::IoError);
        }
    }

    // Absolute symlink within the filesystem.
    fs.symlink("/abs_link", "/target.txt")?;
    let abs_data = fs.read_file("/abs_link")?;
    if abs_data.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: absolute symlink");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   absolute symlink: OK");

    // Relative symlink with .. traversal.
    fs.mkdir("/subdir")?;
    fs.symlink("/subdir/up_link", "../target.txt")?;
    let up_data = fs.read_file("/subdir/up_link")?;
    if up_data.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: relative symlink with ..");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   relative symlink (..): OK");

    // Symlinks appear as Symlink type in readdir.
    let root_entries = fs.readdir("/")?;
    let link_entry = root_entries.iter().find(|e| e.name == "abs_link");
    match link_entry {
        Some(e) if e.entry_type == EntryType::Symlink => {
            crate::serial_println!("[memfs]   symlink in readdir: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: symlink not listed as Symlink in readdir");
            return Err(KernelError::IoError);
        }
    }

    // --- Directory link-count (st_nlink) ---
    //
    // A fresh directory with no subdirectories reports nlink == 2 ("." plus
    // its name in the parent).  Each immediate subdirectory adds one (its
    // ".." back-reference); files and symlinks inside it do NOT.  Removing a
    // subdirectory decrements the count again.
    fs.mkdir("/nlinkdir")?;
    let m_empty = fs.metadata("/nlinkdir")?;
    if m_empty.nlinks != 2 {
        crate::serial_println!(
            "[memfs]   FAILED: empty dir nlink expected 2, got {}",
            m_empty.nlinks
        );
        return Err(KernelError::IoError);
    }
    // A regular file and a symlink must NOT bump the parent's link count.
    fs.write_file("/nlinkdir/file.txt", b"x")?;
    fs.symlink("/nlinkdir/lnk", "file.txt")?;
    let m_file = fs.metadata("/nlinkdir")?;
    if m_file.nlinks != 2 {
        crate::serial_println!(
            "[memfs]   FAILED: dir nlink with file+symlink expected 2, got {}",
            m_file.nlinks
        );
        return Err(KernelError::IoError);
    }
    // Two subdirectories bring it to 4.
    fs.mkdir("/nlinkdir/sub1")?;
    fs.mkdir("/nlinkdir/sub2")?;
    let m_subs = fs.metadata("/nlinkdir")?;
    if m_subs.nlinks != 4 {
        crate::serial_println!(
            "[memfs]   FAILED: dir nlink with 2 subdirs expected 4, got {}",
            m_subs.nlinks
        );
        return Err(KernelError::IoError);
    }
    // Removing one subdirectory drops it back to 3.
    fs.rmdir("/nlinkdir/sub1")?;
    let m_after = fs.metadata("/nlinkdir")?;
    if m_after.nlinks != 3 {
        crate::serial_println!(
            "[memfs]   FAILED: dir nlink after rmdir expected 3, got {}",
            m_after.nlinks
        );
        return Err(KernelError::IoError);
    }
    // A regular file still reports a single link.
    let m_regfile = fs.metadata("/nlinkdir/file.txt")?;
    if m_regfile.nlinks != 1 {
        crate::serial_println!(
            "[memfs]   FAILED: file nlink expected 1, got {}",
            m_regfile.nlinks
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   directory link count (st_nlink): OK");
    // Clean up the nlink fixtures.
    fs.remove("/nlinkdir/lnk")?;
    fs.remove("/nlinkdir/file.txt")?;
    fs.rmdir("/nlinkdir/sub2")?;
    fs.rmdir("/nlinkdir")?;

    // Clean up.
    fs.remove("/target.txt")?;
    fs.remove("/realdir/file.txt")?;
    fs.rmdir("/realdir")?;
    fs.remove("/dirlink")?;
    fs.remove("/chain_a")?;
    fs.remove("/chain_b")?;
    fs.remove("/circ_a")?;
    fs.remove("/circ_b")?;
    fs.remove("/dangling")?;
    fs.remove("/abs_link")?;
    fs.remove("/subdir/up_link")?;
    fs.rmdir("/subdir")?;

    Ok(())
}
