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
//! **Names and objects are separate.** [`MemFs`] owns a flat inode table
//! (`BTreeMap<ino, `[`MemFsNode`]`>`); a directory holds
//! `BTreeMap<name, ino>`, not the objects themselves.  Each node is a
//! [`File`](MemFsNodeKind::File) (data: `Vec<u8>`), a
//! [`Dir`](MemFsNodeKind::Dir) (children: `BTreeMap<name, ino>`),
//! or a [`Symlink`](MemFsNodeKind::Symlink) (target path string).
//!
//! The obvious alternative — a tree of nodes owned by their parent
//! directory — is what this was until 2026-09-01, and it cannot express a
//! **hard link**, because a hard link is precisely two names for one object
//! and an owning tree gives every object exactly one name.  Nothing else
//! forced the change; but hard links are not an exotic feature to bolt on
//! later (`ln`, `cp -al`, `tar`'s link dedup, and every safe-rename dance
//! use them), so the representation has to admit them from the bottom.
//!
//! Two consequences worth knowing:
//!
//! - **`nlink` is real.** A file or symlink inode counts the directory
//!   entries naming it, and is freed when that count reaches zero.
//!   Directories still *report* the Unix `2 + subdirectories` convention —
//!   see [`MemFs::nlink_of`] — because that is what `find(1)`'s leaf
//!   optimisation reads.
//! - **`rename` moves a number, not a subtree.** Renaming a directory used
//!   to move every descendant node; now it moves one `u64` between two
//!   maps, which is both O(1) and impossible to half-complete.
//!
//! Hard links to *directories* are refused ([`KernelError::PermissionDenied`],
//! as on Linux): the resolver below assumes the directory graph is a tree,
//! and a second name for a directory would let a path walk loop forever.
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
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{
    DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo, Timestamp, metadata_now_ns,
    normalize_path,
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
    /// A directory: names to inode numbers.
    ///
    /// The map holds `ino`s rather than nodes so that two entries — in this
    /// directory or in different ones — can name the same object.  Look the
    /// number up in [`MemFs::inodes`].
    Dir(BTreeMap<PathBuf, u64>),
    /// A symbolic link storing a target path string.
    ///
    /// The target is stored as-is (not resolved).  It can be absolute
    /// (starts with `/`) or relative (resolved from the symlink's
    /// parent directory).  Resolution happens during path traversal.
    Symlink(PathBuf),
}

/// A single node in the memory filesystem tree.
struct MemFsNode {
    kind: MemFsNodeKind,
    /// Stable synthetic inode number (assigned at creation, never reused
    /// for the lifetime of this node).  Surfaced as `st_ino`, and the key
    /// this node is stored under in [`MemFs::inodes`].
    ///
    /// Kept in the node as well as in the key so that a `&MemFsNode` handed
    /// to a helper still knows its own identity; the two are set together at
    /// insertion and never diverge.
    ino: u64,
    /// Number of directory entries that name this node.
    ///
    /// This is the *link count in the namespace*, not the value reported as
    /// `st_nlink` — for a directory those differ, see [`MemFs::nlink_of`].
    /// A file or symlink whose count reaches zero has no name left and its
    /// inode is dropped; memfs has no open-file table keeping an unlinked
    /// inode alive, because every VFS operation here arrives by path.
    links: u32,
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
    ///
    /// The key is bytes, not a `String`: an xattr name is an opaque
    /// NUL-terminated byte string exactly as a path component is.
    xattrs: Vec<(Vec<u8>, Vec<u8>)>,
}

// --- Node-level xattr helpers (shared by follow / no-follow variants) ---
// These operate on an already-resolved node so the trait methods differ only
// in how the path is resolved (resolve/resolve_mut vs the no-follow pair).

/// Validate an xattr key/value shape before touching the node.
fn node_validate_xattr(key: &[u8], value: &[u8]) -> KernelResult<()> {
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
fn node_get_xattr(node: &MemFsNode, key: &[u8]) -> KernelResult<Vec<u8>> {
    for (k, v) in &node.xattrs {
        if k == key {
            return Ok(v.clone());
        }
    }
    // The node was found; only the attribute is missing.  `NotFound` here
    // would tell the caller the *file* is gone.
    Err(KernelError::NoAttribute)
}

/// Insert or replace an xattr on a resolved node.  Assumes the key/value have
/// already passed [`node_validate_xattr`].
fn node_set_xattr(node: &mut MemFsNode, key: &[u8], value: &[u8]) -> KernelResult<()> {
    let mut found = false;
    for (k, v) in &mut node.xattrs {
        if k == key {
            *v = value.to_vec();
            found = true;
            break;
        }
    }
    if !found {
        node.xattrs.push((key.to_vec(), value.to_vec()));
    }
    node.changed_ns = metadata_now_ns();
    Ok(())
}

/// Remove an xattr from a resolved node; `NoAttribute` if the key is absent.
fn node_remove_xattr(node: &mut MemFsNode, key: &[u8]) -> KernelResult<()> {
    let orig_len = node.xattrs.len();
    node.xattrs.retain(|(k, _)| k != key);
    if node.xattrs.len() == orig_len {
        return Err(KernelError::NoAttribute);
    }
    node.changed_ns = metadata_now_ns();
    Ok(())
}

/// List all xattr keys on a resolved node.
fn node_list_xattrs(node: &MemFsNode) -> Vec<Vec<u8>> {
    node.xattrs.iter().map(|(k, _)| k.clone()).collect()
}

impl MemFsNode {
    /// Build a fresh node of `kind` with mode `permissions`, a newly
    /// allocated inode number and **one** link.
    ///
    /// One link, not zero, because a node is only ever created in order to
    /// be named: the three callers below each insert it under exactly one
    /// name in the same operation.  Additional names go through
    /// [`MemFs::add_link`], which is the only other place `links` grows.
    fn new(kind: MemFsNodeKind, permissions: u16) -> Self {
        let now = metadata_now_ns();
        Self {
            kind,
            ino: alloc_memfs_ino(),
            links: 1,
            created_ns: now,
            modified_ns: now,
            accessed_ns: now,
            changed_ns: now,
            uid: 0,
            gid: 0,
            permissions,
            attributes: FileAttr::NONE,
            xattrs: Vec::new(),
        }
    }

    fn new_file(data: Vec<u8>) -> Self {
        Self::new(MemFsNodeKind::File(data), 0o644)
    }

    fn new_dir() -> Self {
        Self::new(MemFsNodeKind::Dir(BTreeMap::new()), 0o755)
    }

    fn new_symlink(target: PathBuf) -> Self {
        // Symlinks are always 0o777 (permissions are on the target).
        Self::new(MemFsNodeKind::Symlink(target), 0o777)
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

    /// This directory's `name -> ino` map, or `None` if it is not a
    /// directory.  Resolve the numbers through [`MemFs::node`].
    fn children(&self) -> Option<&BTreeMap<PathBuf, u64>> {
        match &self.kind {
            MemFsNodeKind::Dir(children) => Some(children),
            _ => None,
        }
    }

    fn children_mut(&mut self) -> Option<&mut BTreeMap<PathBuf, u64>> {
        match &mut self.kind {
            MemFsNodeKind::Dir(children) => Some(children),
            _ => None,
        }
    }

    /// Symlink target string, if this is a symlink.
    fn symlink_target(&self) -> Option<&Path> {
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

    /// Entry type for this node.
    fn entry_type(&self) -> EntryType {
        match &self.kind {
            MemFsNodeKind::File(_) => EntryType::File,
            MemFsNodeKind::Dir(_) => EntryType::Directory,
            MemFsNodeKind::Symlink(_) => EntryType::Symlink,
        }
    }

    /// Convert to a VFS DirEntry.
    fn to_dir_entry(&self, name: &Path) -> DirEntry {
        DirEntry {
            name: PathBuf::from(name),
            entry_type: self.entry_type(),
            size: self.size(),
            // Same value `to_file_meta` reports as `st_ino`, from the same
            // field, so a listing and a stat of the same object never
            // disagree about its identity.
            ino: self.ino,
        }
    }

    /// Convert to rich `FileMeta`.
    ///
    /// `nlinks` is passed in rather than read off the node because a
    /// directory's reported count depends on how many of its children are
    /// themselves directories, which only [`MemFs::nlink_of`] can see — the
    /// node holds `ino`s, not nodes.
    fn to_file_meta(&self, nlinks: u32) -> FileMeta {
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
            nlinks,
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
    /// Every object in this filesystem, keyed by its inode number.
    ///
    /// This is the only owner of a node.  Directories name their children by
    /// `ino` (see [`MemFsNodeKind::Dir`]), which is what lets two names refer
    /// to one object.  An entry is removed exactly when the last name for it
    /// goes; [`MemFs::drop_link`] is the only place that happens.
    inodes: BTreeMap<u64, MemFsNode>,
    /// Inode number of the root directory.
    ///
    /// The root has no parent entry naming it, so it is reachable only from
    /// here.  It is never removed for the lifetime of the mount.
    root_ino: u64,
}

impl MemFs {
    /// Create a new empty in-memory filesystem.
    pub fn new() -> Self {
        let root = MemFsNode::new_dir();
        let root_ino = root.ino;
        let mut inodes = BTreeMap::new();
        inodes.insert(root_ino, root);
        Self { inodes, root_ino }
    }

    // -----------------------------------------------------------------------
    // Inode-table access
    // -----------------------------------------------------------------------

    /// Look up a node by inode number.
    ///
    /// `NotFound` rather than a panic on a missing number: a stale `ino` is a
    /// bug in this file, but a kernel that panics on it is worse than one
    /// that reports the object as gone.
    fn node(&self, ino: u64) -> KernelResult<&MemFsNode> {
        self.inodes.get(&ino).ok_or(KernelError::NotFound)
    }

    /// Look up a node by inode number, mutably.
    fn node_mut(&mut self, ino: u64) -> KernelResult<&mut MemFsNode> {
        self.inodes.get_mut(&ino).ok_or(KernelError::NotFound)
    }

    /// The value to report as `st_nlink` for `ino`.
    ///
    /// Files and symlinks report the number of directory entries naming
    /// them, which is what `links` counts.  Directories follow the Unix
    /// convention instead: 2 (the name in the parent, plus the directory's
    /// own `.`) plus one for each immediate subdirectory, each of which
    /// contributes a `..` pointing back here.
    ///
    /// Reporting the directory form honestly matters for tools that exploit
    /// it: `find(1)`'s leaf optimisation treats `nlink == 2` as "no
    /// subdirectories, do not bother stat'ing the children".  A hardcoded 1
    /// both defeats that and is a count no real filesystem reports.
    ///
    /// A directory's `links` field is therefore *not* what it reports, and
    /// stays 1 — there is exactly one name for it, since hard links to
    /// directories are refused.
    fn nlink_of(&self, ino: u64) -> u32 {
        let Ok(node) = self.node(ino) else {
            return 0;
        };
        match node.children() {
            Some(children) => {
                let subdirs = children
                    .values()
                    .filter(|child_ino| self.node(**child_ino).is_ok_and(MemFsNode::is_dir))
                    .count();
                // `saturating_add` / `try_from(..).unwrap_or` keep this
                // arithmetic-side-effect free and clamp the (practically
                // unreachable) > u32::MAX case.
                u32::try_from(subdirs.saturating_add(2)).unwrap_or(u32::MAX)
            }
            None => node.links,
        }
    }

    /// Add one name for `ino` and account for it.
    ///
    /// The only place `links` grows after node creation.  The caller has
    /// already validated that `name` is free in `parent_ino` and that the
    /// link is permitted.
    fn add_link(&mut self, parent_ino: u64, name: PathBuf, ino: u64) -> KernelResult<()> {
        {
            let parent = self.node_mut(parent_ino)?;
            let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;
            children.insert(name, ino);
            parent.touch_modified();
        }
        let node = self.node_mut(ino)?;
        node.links = node.links.saturating_add(1);
        // ctime, not mtime: linking changes the inode's metadata (its link
        // count), never its contents.
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    /// Create `node` as a new object and give it its first name.
    ///
    /// Returns the new inode number.  `node.links` is already 1 (see
    /// [`MemFsNode::new`]), so this does not go through
    /// [`add_link`](Self::add_link).
    ///
    /// The name is inserted before the node, so a `parent_ino` that is not a
    /// directory fails without having left an unreachable inode behind.
    /// Nothing can observe the gap: `&mut self` means no other caller is in
    /// the filesystem between the two statements.
    fn insert_new(&mut self, parent_ino: u64, name: PathBuf, node: MemFsNode) -> KernelResult<u64> {
        let ino = node.ino;
        {
            let parent = self.node_mut(parent_ino)?;
            let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;
            children.insert(name, ino);
            parent.touch_modified();
        }
        self.inodes.insert(ino, node);
        Ok(ino)
    }

    /// Remove `name` from directory `parent_ino` and return the inode it
    /// named, or `NotFound`.
    ///
    /// Does **not** touch the inode's link count — pair it with
    /// [`drop_link`](Self::drop_link) to delete a name, or with
    /// [`add_link`](Self::add_link) elsewhere to move one.
    fn take_child(&mut self, parent_ino: u64, name: &Path) -> KernelResult<u64> {
        let parent = self.node_mut(parent_ino)?;
        let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;
        let ino = children.remove(name).ok_or(KernelError::NotFound)?;
        parent.touch_modified();
        Ok(ino)
    }

    /// Give the object `ino` an additional name at `new_path`.
    ///
    /// Shared by [`FileSystem::link`] and [`FileSystem::link_no_follow`],
    /// which differ only in how they turn `existing` into an inode number —
    /// once they have one, there is nothing left for the two to disagree
    /// about, so there is one implementation of the rules rather than two.
    ///
    /// # Errors
    /// - `PermissionDenied` if `ino` is a directory.  Directory hard links
    ///   would make the namespace a graph rather than a tree, which
    ///   [`resolve_path_str`](Self::resolve_path_str) assumes it is not:
    ///   a cycle would make it loop without the symlink-depth counter ever
    ///   firing, because no symlink is involved.  Linux refuses these for the
    ///   same reason (`EPERM`), so refusing costs no compatibility.
    /// - `PermissionDenied` if `ino` or the new parent is immutable.
    /// - `AlreadyExists` if `new_path`'s final component is taken.  `link(2)`
    ///   never replaces — unlike `rename`, which does.
    fn link_ino(&mut self, ino: u64, new_path: &Path) -> KernelResult<()> {
        if self.node(ino)?.is_dir() {
            return Err(KernelError::PermissionDenied);
        }
        if self.node(ino)?.attributes.contains(FileAttr::IMMUTABLE) {
            return Err(KernelError::PermissionDenied);
        }

        let (parent_ino, name) = self.resolve_parent(new_path)?;
        if self
            .node(parent_ino)?
            .attributes
            .contains(FileAttr::IMMUTABLE)
        {
            return Err(KernelError::PermissionDenied);
        }
        if self.child_ino(parent_ino, name)?.is_some() {
            return Err(KernelError::AlreadyExists);
        }

        let name = name.to_path_buf();
        self.add_link(parent_ino, name, ino)
    }

    /// The inode `name` refers to in directory `parent_ino`, if any.
    fn child_ino(&self, parent_ino: u64, name: &Path) -> KernelResult<Option<u64>> {
        let children = self
            .node(parent_ino)?
            .children()
            .ok_or(KernelError::NotADirectory)?;
        Ok(children.get(name).copied())
    }

    /// Account for one name for `ino` going away, freeing the inode when the
    /// last one does.
    ///
    /// This is the only place a node is removed from [`MemFs::inodes`].  A
    /// directory being removed by `rmdir` also comes through here: its
    /// `links` is 1, so the first drop frees it.
    ///
    /// The caller has already removed the name from its parent's map; this
    /// takes only the inode side, so the two halves cannot get out of step
    /// by one half being forgotten in a new call site.
    fn drop_link(&mut self, ino: u64) {
        let Some(node) = self.inodes.get_mut(&ino) else {
            return;
        };
        node.links = node.links.saturating_sub(1);
        if node.links == 0 {
            self.inodes.remove(&ino);
        } else {
            node.changed_ns = metadata_now_ns();
        }
    }

    // -----------------------------------------------------------------------
    // Path helpers
    // -----------------------------------------------------------------------

    /// Split a path into components, filtering out empty parts and ".".
    ///
    /// [`Path::components`] already drops the empty parts (leading, repeated
    /// and trailing separators); the extra filter here drops `.`, which the
    /// lexer deliberately yields verbatim because whether it is meaningful is
    /// the filesystem's decision, not the lexer's.
    fn path_components(path: &Path) -> Vec<&Path> {
        path.components().filter(|c| c.as_bytes() != b".").collect()
    }

    /// Build the parent path from a set of components.
    ///
    /// `["a", "b", "c"]` → `"/a/b"`.  `["a"]` → `"/"`.
    fn parent_path_of(comps: &[&Path]) -> PathBuf {
        if comps.len() <= 1 {
            return PathBuf::from("/");
        }
        let mut p = PathBuf::new();
        for c in comps.get(..comps.len().saturating_sub(1)).unwrap_or(&[]) {
            p.extend_bytes(b"/");
            p.extend_bytes(c.as_bytes());
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
    /// Returns the fully resolved path as an owned [`PathBuf`].
    fn resolve_path_str(&self, path: &Path, follow_last: bool) -> KernelResult<PathBuf> {
        let mut resolved = normalize_path(path);
        let mut depth = 0usize;

        loop {
            // `resolved` is owned and mutated below, so the components must be
            // copied out before the loop body can reassign it.
            let components: Vec<PathBuf> = resolved.components().map(Path::to_path_buf).collect();

            let Some(last_index) = components.len().checked_sub(1) else {
                return Ok(PathBuf::from("/"));
            };

            let mut current = self.node(self.root_ino)?;
            let mut hit_symlink = false;

            for (i, component) in components.iter().enumerate() {
                let is_last = i == last_index;
                let children = current.children().ok_or(KernelError::NotADirectory)?;
                let child_ino = *children
                    .get(component.as_path())
                    .ok_or(KernelError::NotFound)?;
                let node = self.node(child_ino)?;

                if let MemFsNodeKind::Symlink(ref target) = node.kind {
                    if is_last && !follow_last {
                        // Don't follow the final component.
                        return Ok(resolved);
                    }

                    depth = depth.wrapping_add(1);
                    if depth > MAX_SYMLINK_DEPTH {
                        return Err(KernelError::TooManyLinks);
                    }

                    // Rebuild the path with this symlink replaced by its
                    // target: [components before it] + target + [components
                    // after it].  `PathBuf::push` inserts the separator, and
                    // clears the buffer when the pushed piece is absolute —
                    // which is exactly the "absolute target discards the
                    // parent" rule, so it needs no special case here.
                    let mut rebuilt = PathBuf::from("/");
                    for c in components.get(..i).unwrap_or(&[]) {
                        rebuilt.push(c);
                    }
                    rebuilt.push(target);
                    for c in components.get(i.saturating_add(1)..).unwrap_or(&[]) {
                        rebuilt.push(c);
                    }

                    resolved = normalize_path(&rebuilt);
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

    /// Walk a path WITHOUT following symlinks, returning the inode number it
    /// names.
    ///
    /// Used after [`resolve_path_str`] has already resolved all symlinks.
    ///
    /// There is no `walk_mut`: with names and objects separated, a walk needs
    /// only shared access, and the caller reaches the node it found through
    /// [`node_mut`](Self::node_mut).  That is what removes the nested-borrow
    /// contortions the owning-tree version needed, where walking mutably
    /// borrowed every ancestor for as long as the descendant was held.
    fn walk(&self, path: &Path) -> KernelResult<u64> {
        let components = Self::path_components(path);
        let mut current = self.root_ino;
        for component in &components {
            let children = self
                .node(current)?
                .children()
                .ok_or(KernelError::NotADirectory)?;
            current = *children.get(*component).ok_or(KernelError::NotFound)?;
        }
        Ok(current)
    }

    // -----------------------------------------------------------------------
    // Public resolve helpers (used by FileSystem trait impls)
    // -----------------------------------------------------------------------

    /// Resolve a path to an inode number, following ALL symlinks (including
    /// the final one).
    fn resolve_ino(&self, path: &Path) -> KernelResult<u64> {
        let resolved = self.resolve_path_str(path, true)?;
        self.walk(&resolved)
    }

    /// Resolve a path, following ALL symlinks (including the final one).
    fn resolve(&self, path: &Path) -> KernelResult<&MemFsNode> {
        let ino = self.resolve_ino(path)?;
        self.node(ino)
    }

    /// Resolve a path mutably, following ALL symlinks.
    fn resolve_mut(&mut self, path: &Path) -> KernelResult<&mut MemFsNode> {
        let ino = self.resolve_ino(path)?;
        self.node_mut(ino)
    }

    /// Resolve a path to an inode number, following intermediate symlinks
    /// but NOT the final component.
    fn resolve_ino_no_follow(&self, path: &Path) -> KernelResult<u64> {
        let resolved = self.resolve_path_str(path, false)?;
        self.walk(&resolved)
    }

    /// Resolve a path, following intermediate symlinks but NOT the
    /// final component.  Used by `lstat` and `readlink`.
    fn resolve_no_follow(&self, path: &Path) -> KernelResult<&MemFsNode> {
        let ino = self.resolve_ino_no_follow(path)?;
        self.node(ino)
    }

    /// Resolve a path mutably, following intermediate symlinks but NOT the
    /// final component.  Used by `lchown`/`lutimes`-style operations that
    /// must mutate the symlink inode itself, not its target.
    fn resolve_no_follow_mut(&mut self, path: &Path) -> KernelResult<&mut MemFsNode> {
        let ino = self.resolve_ino_no_follow(path)?;
        self.node_mut(ino)
    }

    /// Resolve the parent directory of a path (following symlinks in
    /// intermediate components) and return `(parent_ino, filename)`.
    ///
    /// The filename is the last component of the original path (not
    /// followed if it's a symlink).  The parent path IS fully resolved.
    fn resolve_parent<'b>(&self, path: &'b Path) -> KernelResult<(u64, &'b Path)> {
        let components = Self::path_components(path);
        let filename = *components.last().ok_or(KernelError::InvalidArgument)?;
        let parent_path = Self::parent_path_of(&components);

        // Resolve the parent (following all symlinks in the parent path).
        let resolved_parent = self.resolve_path_str(&parent_path, true)?;

        let parent_ino = self.walk(&resolved_parent)?;
        if !self.node(parent_ino)?.is_dir() {
            return Err(KernelError::NotADirectory);
        }

        Ok((parent_ino, filename))
    }

    /// Resolve the write target for a file operation.
    ///
    /// Follows symlinks on all components (including the final one).
    /// If the final component doesn't exist, resolves the parent and
    /// returns the parent's resolved path + the original filename so
    /// the caller can create a new entry.
    ///
    /// Returns `(resolved_parent_path, filename)`.
    fn resolve_write_path(&self, path: &Path) -> KernelResult<(PathBuf, PathBuf)> {
        let mut current_path = normalize_path(path);
        let mut depth = 0usize;

        loop {
            let comps = Self::path_components(&current_path);
            let filename = (*comps.last().ok_or(KernelError::InvalidArgument)?).to_path_buf();
            let parent_path = Self::parent_path_of(&comps);

            // Resolve the parent path (following all symlinks).
            let resolved_parent = self.resolve_path_str(&parent_path, true)?;

            // Check if filename exists in the resolved parent.
            let parent_ino = self.walk(&resolved_parent)?;
            let children = self
                .node(parent_ino)?
                .children()
                .ok_or(KernelError::NotADirectory)?;

            match children.get(filename.as_path()).copied() {
                Some(child_ino) => {
                    if let MemFsNodeKind::Symlink(ref target) = self.node(child_ino)?.kind {
                        // Follow the symlink.
                        depth = depth.wrapping_add(1);
                        if depth > MAX_SYMLINK_DEPTH {
                            return Err(KernelError::TooManyLinks);
                        }
                        // `push` clears the buffer for an absolute argument,
                        // so an absolute target correctly discards the parent
                        // without a separate branch.
                        let mut next = resolved_parent;
                        next.push(target);
                        current_path = normalize_path(&next);
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

    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>> {
        let dir_ino = self.resolve_ino(path)?;
        let children = self
            .node(dir_ino)?
            .children()
            .ok_or(KernelError::NotADirectory)?;

        // A name whose inode has gone is a bug in this file rather than
        // something a caller can cause, but listing it as a broken entry
        // would be worse than omitting it: every consumer of a `DirEntry`
        // assumes the object exists.
        let entries: Vec<DirEntry> = children
            .iter()
            .filter_map(|(name, child_ino)| {
                self.node(*child_ino).ok().map(|n| n.to_dir_entry(name))
            })
            .collect();

        Ok(entries)
    }

    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>> {
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

    fn stat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        let components = Self::path_components(path);
        if components.is_empty() {
            // Root directory.  Its inode comes from the root node rather than
            // a literal, so `stat("/")` agrees with the `..` entry a child
            // directory listing reports for it.
            return Ok(DirEntry {
                name: PathBuf::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
                ino: self.root_ino,
            });
        }

        let name = components[components.len() - 1];
        let node = self.resolve(path)?;
        Ok(node.to_dir_entry(name))
    }

    fn write_file(&mut self, path: &Path, data: &[u8]) -> KernelResult<()> {
        // Follow symlinks to find the actual write target.
        let (parent_path, filename) = self.resolve_write_path(path)?;

        let parent_ino = self.walk(&parent_path)?;

        match self.child_ino(parent_ino, &filename)? {
            Some(existing_ino) => {
                let existing = self.node_mut(existing_ino)?;
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
                let file_data = existing.file_data_mut().ok_or(KernelError::IsADirectory)?;
                file_data.clear();
                file_data.extend_from_slice(data);
                // NLL: file_data borrow ends here (last use above).
                existing.touch_modified();
                // Only the inode changed, so the directory's mtime is left
                // alone — and every other name for this inode now reads the
                // new bytes, which is what a hard link is for.  (The `None`
                // arm below does add an entry, and `insert_new` stamps the
                // directory there.)
            }
            None => {
                // Create new file (constructor sets timestamps to now).
                self.insert_new(parent_ino, filename, MemFsNode::new_file(data.to_vec()))?;
            }
        }
        Ok(())
    }

    fn remove(&mut self, path: &Path) -> KernelResult<()> {
        // remove() does NOT follow the final component — it removes the
        // entry itself (file or symlink).  Intermediate symlinks ARE followed.
        let (parent_ino, filename) = self.resolve_parent(path)?;

        let ino = self
            .child_ino(parent_ino, filename)?
            .ok_or(KernelError::NotFound)?;
        {
            let node = self.node(ino)?;
            if node.is_dir() {
                return Err(KernelError::IsADirectory);
            }
            if node.attributes.contains(FileAttr::IMMUTABLE) {
                return Err(KernelError::PermissionDenied);
            }
        }
        self.take_child(parent_ino, filename)?;
        // Unlink, not delete: the object survives while another name refers
        // to it, and only the last `remove` frees it.
        self.drop_link(ino);
        Ok(())
    }

    fn mkdir(&mut self, path: &Path) -> KernelResult<()> {
        // mkdir does NOT follow the final component — if the name
        // already exists (even as a symlink), it returns AlreadyExists.
        let (parent_ino, dirname) = self.resolve_parent(path)?;
        if self
            .node(parent_ino)?
            .attributes
            .contains(FileAttr::IMMUTABLE)
        {
            return Err(KernelError::PermissionDenied);
        }
        if self.child_ino(parent_ino, dirname)?.is_some() {
            return Err(KernelError::AlreadyExists);
        }

        self.insert_new(parent_ino, dirname.to_path_buf(), MemFsNode::new_dir())?;
        Ok(())
    }

    fn rmdir(&mut self, path: &Path) -> KernelResult<()> {
        // rmdir does NOT follow the final component — a symlink at the
        // end returns NotADirectory (like Linux).
        let (parent_ino, dirname) = self.resolve_parent(path)?;

        let ino = self
            .child_ino(parent_ino, dirname)?
            .ok_or(KernelError::NotFound)?;
        {
            let node = self.node(ino)?;
            let children = node.children().ok_or(KernelError::NotADirectory)?;
            if node.attributes.contains(FileAttr::IMMUTABLE) {
                return Err(KernelError::PermissionDenied);
            }
            // Must be empty.
            if !children.is_empty() {
                return Err(KernelError::InvalidArgument); // Directory not empty.
            }
        }

        self.take_child(parent_ino, dirname)?;
        // A directory has exactly one name (hard links to directories are
        // refused), so this drop is always the last one and frees the inode.
        self.drop_link(ino);
        Ok(())
    }

    fn read_at(&mut self, path: &Path, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
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

    fn write_at(&mut self, path: &Path, offset: u64, data: &[u8]) -> KernelResult<()> {
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

    fn truncate(&mut self, path: &Path, size: u64) -> KernelResult<()> {
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

    fn rename(&mut self, from: &Path, to: &Path) -> KernelResult<()> {
        // rename() does NOT follow the final component for either source
        // or destination — it moves the entry itself (including symlinks).
        // Intermediate components ARE resolved through symlinks.

        // Resolve both parents (following intermediate symlinks).
        let from_comps = Self::path_components(from);
        let to_comps = Self::path_components(to);
        if from_comps.is_empty() || to_comps.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        let from_name = (*from_comps.last().ok_or(KernelError::InvalidArgument)?).to_path_buf();
        let to_name = (*to_comps.last().ok_or(KernelError::InvalidArgument)?).to_path_buf();

        let from_parent_path = Self::parent_path_of(&from_comps);
        let to_parent_path = Self::parent_path_of(&to_comps);

        let resolved_from_parent = self.resolve_path_str(&from_parent_path, true)?;
        let resolved_to_parent = self.resolve_path_str(&to_parent_path, true)?;

        // POSIX rename semantics.  All three checks below run *before* the
        // source is detached, so a rejected rename leaves the tree untouched.
        //
        // 1. Renaming an entry onto itself is a no-op success, not an error —
        //    and above all not a removal.
        if resolved_from_parent == resolved_to_parent && from_name == to_name {
            return Ok(());
        }

        // 2. Moving a directory into its own subtree (`/a` -> `/a/b/c`) is
        //    `InvalidArgument` (POSIX EINVAL).  This used to detach the source
        //    first and only then walk to the destination parent — which had
        //    just gone with it — so the walk failed with `NotFound` and the
        //    entire moved subtree was dropped on the floor.
        let from_full = resolved_from_parent.join(from_name.as_path());
        if resolved_to_parent.starts_with(&from_full) {
            return Err(KernelError::InvalidArgument);
        }

        // 3. An existing destination is *replaced*, unless it is a directory
        //    (`IsADirectory`, matching both ext4 and FAT).
        //
        //    This used to refuse any existing destination with `AlreadyExists`,
        //    which is `RENAME_NOREPLACE` behaviour baked into the plain
        //    operation.  The VFS implements that flag itself — `rename_inner`
        //    stats the destination under the same per-mount lock it then
        //    renames under (see `Vfs::rename_noreplace`) — so a filesystem that
        //    hardcodes it leaves callers no way to get the replacing form at
        //    all.  `Vfs::atomic_write` is exactly such a caller: its last step
        //    renames a temp file over the target, so the standard safe-write
        //    pattern could never replace an existing file on memfs, which is
        //    what `/tmp` always is.
        let to_parent_ino = self.walk(&resolved_to_parent)?;
        let displaced = self.child_ino(to_parent_ino, to_name.as_path())?;
        if let Some(existing_ino) = displaced {
            if self.node(existing_ino)?.entry_type() == EntryType::Directory {
                return Err(KernelError::IsADirectory);
            }
        }

        let from_parent_ino = self.walk(&resolved_from_parent)?;
        let source_ino = self
            .child_ino(from_parent_ino, from_name.as_path())?
            .ok_or(KernelError::NotFound)?;

        // 4. Both names already denote the *same object* — two hard links to
        //    one file.  POSIX: "rename() shall return successfully and perform
        //    no other action."  Without this the move below would drop `from`
        //    and leave only `to`, i.e. silently unlink one of the two names the
        //    caller asked to keep.  The case became reachable the moment memfs
        //    could hold two names for one inode, and is invisible to any test
        //    written before it could.
        if displaced == Some(source_ino) {
            return Ok(());
        }

        // Move the *name*: one `u64` leaves one map and enters another.  The
        // object never moves and its link count never changes, so renaming a
        // directory no longer relocates its whole subtree — which is why this
        // is now O(1) and cannot leave a half-moved tree behind.
        let moved_ino = self.take_child(from_parent_ino, from_name.as_path())?;

        {
            let to_parent = self.node_mut(to_parent_ino)?;
            let children = to_parent.children_mut().ok_or(KernelError::NotADirectory)?;
            children.insert(to_name, moved_ino);
            to_parent.touch_modified();
        }

        // Replacement (case 3): the displaced entry lost its name, so its
        // inode loses a link — and is freed only if that was its last one.
        // The owning-tree version relied on `BTreeMap::insert` dropping the
        // node it evicted, which silently destroys a file that another hard
        // link still names.
        if let Some(existing_ino) = displaced {
            self.drop_link(existing_ino);
        }
        Ok(())
    }

    fn rename_exchange(&mut self, a: &Path, b: &Path) -> KernelResult<()> {
        // Atomically swap two existing entries. Like rename(), the final
        // component is NOT followed for either path; intermediate components
        // ARE resolved through symlinks. Both entries must exist.
        let a_comps = Self::path_components(a);
        let b_comps = Self::path_components(b);
        if a_comps.is_empty() || b_comps.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        let a_name = (*a_comps.last().ok_or(KernelError::InvalidArgument)?).to_path_buf();
        let b_name = (*b_comps.last().ok_or(KernelError::InvalidArgument)?).to_path_buf();

        let a_parent_path = Self::parent_path_of(&a_comps);
        let b_parent_path = Self::parent_path_of(&b_comps);

        let resolved_a_parent = self.resolve_path_str(&a_parent_path, true)?;
        let resolved_b_parent = self.resolve_path_str(&b_parent_path, true)?;

        // Exchanging an entry with itself is a no-op (but the entry must
        // still exist, else ENOENT).
        let a_parent_ino = self.walk(&resolved_a_parent)?;
        if resolved_a_parent == resolved_b_parent && a_name == b_name {
            if self.child_ino(a_parent_ino, a_name.as_path())?.is_none() {
                return Err(KernelError::NotFound);
            }
            return Ok(());
        }
        let b_parent_ino = self.walk(&resolved_b_parent)?;

        // Both names must exist *before* anything moves.  Looking them up
        // first is what makes the exchange all-or-nothing: the owning-tree
        // version had to detach `a`, discover `b` was missing, and put `a`
        // back — a rollback that could itself fail.  Here nothing has been
        // touched yet when the second lookup returns `NotFound`.
        let ino_a = self
            .child_ino(a_parent_ino, a_name.as_path())?
            .ok_or(KernelError::NotFound)?;
        let ino_b = self
            .child_ino(b_parent_ino, b_name.as_path())?
            .ok_or(KernelError::NotFound)?;

        // Swap the two numbers.  No link count changes: each object still has
        // exactly the names it had, and neither object moves.
        {
            let parent_a = self.node_mut(a_parent_ino)?;
            let children = parent_a.children_mut().ok_or(KernelError::NotADirectory)?;
            children.insert(a_name, ino_b);
            parent_a.touch_modified();
        }
        {
            let parent_b = self.node_mut(b_parent_ino)?;
            let children = parent_b.children_mut().ok_or(KernelError::NotADirectory)?;
            children.insert(b_name, ino_a);
            parent_b.touch_modified();
        }
        Ok(())
    }

    fn debug_stats(&self) -> String {
        // Count *objects*, not names: a file with three hard links is one
        // file and its bytes are stored once, so counting the namespace
        // would triple both.
        let mut files = 0usize;
        let mut dirs = 0usize;
        let mut links = 0usize;
        let mut bytes = 0u64;
        for node in self.inodes.values() {
            match &node.kind {
                MemFsNodeKind::File(data) => {
                    files = files.wrapping_add(1);
                    bytes = bytes.wrapping_add(data.len() as u64);
                }
                MemFsNodeKind::Dir(_) => dirs = dirs.wrapping_add(1),
                MemFsNodeKind::Symlink(_) => links = links.wrapping_add(1),
            }
        }

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

    fn metadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        let ino = self.resolve_ino(path)?;
        let nlinks = self.nlink_of(ino);
        Ok(self.node(ino)?.to_file_meta(nlinks))
    }

    fn lmetadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        // No-follow: return the trailing symlink's own metadata rather
        // than its target's.  Mirrors `metadata` but uses the
        // non-following resolver.
        let ino = self.resolve_ino_no_follow(path)?;
        let nlinks = self.nlink_of(ino);
        Ok(self.node(ino)?.to_file_meta(nlinks))
    }

    fn set_attributes(&mut self, path: &Path, attrs: FileAttr) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.attributes = attrs;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    fn set_owner(&mut self, path: &Path, uid: u32, gid: u32) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.uid = uid;
        node.gid = gid;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    fn set_permissions(&mut self, path: &Path, permissions: u16) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.permissions = permissions;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    fn set_times(
        &mut self,
        path: &Path,
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
    fn set_owner_no_follow(&mut self, path: &Path, uid: u32, gid: u32) -> KernelResult<()> {
        let node = self.resolve_no_follow_mut(path)?;
        node.uid = uid;
        node.gid = gid;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    /// `fchmodat2(AT_SYMLINK_NOFOLLOW)`: set mode bits on the link inode
    /// itself.  Same as [`set_permissions`](Self::set_permissions) but the
    /// final path component is resolved WITHOUT following a symlink.
    fn set_permissions_no_follow(&mut self, path: &Path, permissions: u16) -> KernelResult<()> {
        let node = self.resolve_no_follow_mut(path)?;
        node.permissions = permissions;
        node.changed_ns = metadata_now_ns();
        Ok(())
    }

    /// `lutimes`/`utimensat(AT_SYMLINK_NOFOLLOW)`: stamp the link inode
    /// itself.  Same as [`set_times`](Self::set_times) but no-follow.
    fn set_times_no_follow(
        &mut self,
        path: &Path,
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

    fn get_xattr(&mut self, path: &Path, key: &[u8]) -> KernelResult<Vec<u8>> {
        node_get_xattr(self.resolve(path)?, key)
    }

    fn set_xattr(&mut self, path: &Path, key: &[u8], value: &[u8]) -> KernelResult<()> {
        // Validation happens before path resolution so a bad key/value shape
        // is rejected identically regardless of follow mode.
        node_validate_xattr(key, value)?;
        node_set_xattr(self.resolve_mut(path)?, key, value)
    }

    fn remove_xattr(&mut self, path: &Path, key: &[u8]) -> KernelResult<()> {
        node_remove_xattr(self.resolve_mut(path)?, key)
    }

    fn list_xattrs(&mut self, path: &Path) -> KernelResult<Vec<Vec<u8>>> {
        Ok(node_list_xattrs(self.resolve(path)?))
    }

    // --- No-follow xattr variants (l-prefixed: lgetxattr/lsetxattr/etc.) ---
    // Operate on the symlink inode itself rather than its target.  Identical
    // to the following versions but the final component is not followed.

    fn get_xattr_no_follow(&mut self, path: &Path, key: &[u8]) -> KernelResult<Vec<u8>> {
        node_get_xattr(self.resolve_no_follow(path)?, key)
    }

    fn set_xattr_no_follow(&mut self, path: &Path, key: &[u8], value: &[u8]) -> KernelResult<()> {
        node_validate_xattr(key, value)?;
        node_set_xattr(self.resolve_no_follow_mut(path)?, key, value)
    }

    fn remove_xattr_no_follow(&mut self, path: &Path, key: &[u8]) -> KernelResult<()> {
        node_remove_xattr(self.resolve_no_follow_mut(path)?, key)
    }

    fn list_xattrs_no_follow(&mut self, path: &Path) -> KernelResult<Vec<Vec<u8>>> {
        Ok(node_list_xattrs(self.resolve_no_follow(path)?))
    }

    // --- Symlink operations ---

    fn symlink(&mut self, path: &Path, target: &Path) -> KernelResult<()> {
        if target.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        // Validate target length (symlink targets use the same limit as
        // path components).
        if target.len() > 4096 {
            return Err(KernelError::InvalidArgument);
        }

        let (parent_ino, linkname) = self.resolve_parent(path)?;
        if self
            .node(parent_ino)?
            .attributes
            .contains(FileAttr::IMMUTABLE)
        {
            return Err(KernelError::PermissionDenied);
        }
        if self.child_ino(parent_ino, linkname)?.is_some() {
            return Err(KernelError::AlreadyExists);
        }

        self.insert_new(
            parent_ino,
            linkname.to_path_buf(),
            MemFsNode::new_symlink(target.to_path_buf()),
        )?;
        Ok(())
    }

    fn readlink(&mut self, path: &Path) -> KernelResult<PathBuf> {
        // readlink does NOT follow the final component.
        let node = self.resolve_no_follow(path)?;
        match node.symlink_target() {
            Some(target) => Ok(target.to_path_buf()),
            None => Err(KernelError::InvalidArgument), // Not a symlink.
        }
    }

    // --- Hard link operations ---

    fn link(&mut self, existing: &Path, new_path: &Path) -> KernelResult<()> {
        let ino = self.resolve_ino(existing)?;
        self.link_ino(ino, new_path)
    }

    fn link_no_follow(&mut self, existing: &Path, new_path: &Path) -> KernelResult<()> {
        // `link(2)` and `linkat` without `AT_SYMLINK_FOLLOW`: a trailing
        // symlink in `existing` is itself the thing being linked, so the new
        // name is a second name for the *symlink* inode.  Both names then
        // dangle or resolve together, which is the point.
        let ino = self.resolve_ino_no_follow(existing)?;
        self.link_ino(ino, new_path)
    }

    fn lstat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        let components = Self::path_components(path);
        if components.is_empty() {
            return Ok(DirEntry {
                name: PathBuf::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
                ino: self.root_ino,
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
        // The inode table *is* the object count, so this no longer walks the
        // namespace.  That is not merely cheaper (O(1) rather than O(tree)):
        // it is now correct in a case the walk could not express — a file
        // reachable under two names is one inode, and the recursive count
        // would have reported it twice.
        let node_count = self.inodes.len() as u64;

        Ok(FsInfo {
            fs_type: String::from("memfs"),
            volume_label: String::new(),
            block_size: 1,   // Byte-granular allocation.
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
///
/// # Errors
/// Any error from [`crate::fs::Vfs::mount`] (e.g. the mountpoint is already
/// occupied or its parent does not exist).
pub fn mount<P: AsRef<crate::fs::path::Path> + ?Sized>(mount_path: &P) -> KernelResult<()> {
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
    fs.mkdir(Path::new("/testdir"))?;
    let entries = fs.readdir(Path::new("/"))?;
    let has_testdir = entries
        .iter()
        .any(|e| e.name.as_path() == Path::new("testdir") && e.entry_type == EntryType::Directory);
    if !has_testdir {
        crate::serial_println!("[memfs]   FAILED: testdir not in root");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   mkdir: OK");

    // Test write_file + read_file.
    let test_data = b"Hello from MemFs!";
    fs.write_file(Path::new("/testdir/hello.txt"), test_data)?;
    let readback = fs.read_file(Path::new("/testdir/hello.txt"))?;
    if readback.as_slice() != test_data.as_slice() {
        crate::serial_println!("[memfs]   FAILED: write/read mismatch");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   write_file + read_file: OK");

    // Test stat.
    let stat = fs.stat(Path::new("/testdir/hello.txt"))?;
    if stat.size != test_data.len() as u64 || stat.entry_type != EntryType::File {
        crate::serial_println!("[memfs]   FAILED: stat mismatch");
        return Err(KernelError::IoError);
    }

    // Test case sensitivity: "Hello.txt" should NOT find "hello.txt".
    match fs.read_file(Path::new("/testdir/Hello.txt")) {
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
    let partial = fs.read_at(Path::new("/testdir/hello.txt"), 6, 4)?;
    if partial.as_slice() != b"from" {
        crate::serial_println!(
            "[memfs]   FAILED: read_at expected 'from', got {:?}",
            core::str::from_utf8(&partial)
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   read_at: OK");

    // Test write_at (extend).
    fs.write_at(Path::new("/testdir/hello.txt"), 17, b" Extended!")?;
    let extended = fs.read_file(Path::new("/testdir/hello.txt"))?;
    if extended.as_slice() != b"Hello from MemFs! Extended!" {
        crate::serial_println!("[memfs]   FAILED: write_at extend");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   write_at: OK");

    // Test truncate.
    fs.truncate(Path::new("/testdir/hello.txt"), 5)?;
    let truncated = fs.read_file(Path::new("/testdir/hello.txt"))?;
    if truncated.as_slice() != b"Hello" {
        crate::serial_println!("[memfs]   FAILED: truncate");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   truncate: OK");

    // Test rename.
    fs.rename(
        Path::new("/testdir/hello.txt"),
        Path::new("/testdir/renamed.txt"),
    )?;
    match fs.read_file(Path::new("/testdir/hello.txt")) {
        Err(KernelError::NotFound) => {}
        _ => {
            crate::serial_println!("[memfs]   FAILED: old name still exists after rename");
            return Err(KernelError::IoError);
        }
    }
    let renamed_data = fs.read_file(Path::new("/testdir/renamed.txt"))?;
    if renamed_data.as_slice() != b"Hello" {
        crate::serial_println!("[memfs]   FAILED: renamed file data mismatch");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   rename: OK");

    // Test remove.
    fs.remove(Path::new("/testdir/renamed.txt"))?;
    match fs.read_file(Path::new("/testdir/renamed.txt")) {
        Err(KernelError::NotFound) => {}
        _ => {
            crate::serial_println!("[memfs]   FAILED: file still exists after remove");
            return Err(KernelError::IoError);
        }
    }

    // Test rmdir.
    fs.rmdir(Path::new("/testdir"))?;
    match fs.readdir(Path::new("/testdir")) {
        Err(KernelError::NotFound) => {}
        _ => {
            crate::serial_println!("[memfs]   FAILED: dir still exists after rmdir");
            return Err(KernelError::IoError);
        }
    }
    crate::serial_println!("[memfs]   remove + rmdir: OK");

    // Test rmdir on non-empty directory.
    fs.mkdir(Path::new("/notempty"))?;
    fs.write_file(Path::new("/notempty/file.txt"), b"data")?;
    match fs.rmdir(Path::new("/notempty")) {
        Err(KernelError::InvalidArgument) => {
            crate::serial_println!("[memfs]   rmdir non-empty: correctly rejected");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: rmdir non-empty should fail");
            return Err(KernelError::IoError);
        }
    }
    // Clean up.
    fs.remove(Path::new("/notempty/file.txt"))?;
    fs.rmdir(Path::new("/notempty"))?;

    // Test debug_stats.
    fs.write_file(Path::new("/a.txt"), b"aaa")?;
    fs.write_file(Path::new("/b.txt"), b"bbb")?;
    let stats = fs.debug_stats();
    crate::serial_println!("[memfs]   {}", stats);
    fs.remove(Path::new("/a.txt"))?;
    fs.remove(Path::new("/b.txt"))?;

    // --- Metadata tests ---

    // Test metadata timestamps are set.
    fs.write_file(Path::new("/meta.txt"), b"metadata test")?;
    let meta = fs.metadata(Path::new("/meta.txt"))?;
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
    fs.set_permissions(Path::new("/meta.txt"), 0o755)?;
    let meta2 = fs.metadata(Path::new("/meta.txt"))?;
    if meta2.permissions != 0o755 {
        crate::serial_println!("[memfs]   FAILED: permissions not updated");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   set_permissions: OK");

    // Test set_owner.
    fs.set_owner(Path::new("/meta.txt"), 1000, 1000)?;
    let meta3 = fs.metadata(Path::new("/meta.txt"))?;
    if meta3.uid != 1000 || meta3.gid != 1000 {
        crate::serial_println!("[memfs]   FAILED: owner not updated");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   set_owner: OK");

    // Test immutable attribute.
    fs.set_attributes(Path::new("/meta.txt"), FileAttr::IMMUTABLE)?;
    match fs.write_file(Path::new("/meta.txt"), b"should fail") {
        Err(KernelError::PermissionDenied) => {
            crate::serial_println!("[memfs]   immutable write rejected: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: immutable write should fail");
            return Err(KernelError::IoError);
        }
    }
    match fs.remove(Path::new("/meta.txt")) {
        Err(KernelError::PermissionDenied) => {
            crate::serial_println!("[memfs]   immutable remove rejected: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: immutable remove should fail");
            return Err(KernelError::IoError);
        }
    }
    // Clear immutable to clean up.
    fs.set_attributes(Path::new("/meta.txt"), FileAttr::NONE)?;

    // Test append-only attribute.
    fs.set_attributes(Path::new("/meta.txt"), FileAttr::APPEND_ONLY)?;
    match fs.truncate(Path::new("/meta.txt"), 0) {
        Err(KernelError::PermissionDenied) => {
            crate::serial_println!("[memfs]   append-only truncate rejected: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: append-only truncate should fail");
            return Err(KernelError::IoError);
        }
    }
    fs.set_attributes(Path::new("/meta.txt"), FileAttr::NONE)?;

    // Test extended attributes.
    fs.set_xattr(Path::new("/meta.txt"), b"user.tag", b"important")?;
    let xval = fs.get_xattr(Path::new("/meta.txt"), b"user.tag")?;
    if xval.as_slice() != b"important" {
        crate::serial_println!("[memfs]   FAILED: xattr value mismatch");
        return Err(KernelError::IoError);
    }
    let xkeys = fs.list_xattrs(Path::new("/meta.txt"))?;
    if xkeys.len() != 1 || xkeys[0] != b"user.tag" {
        crate::serial_println!("[memfs]   FAILED: xattr list mismatch");
        return Err(KernelError::IoError);
    }
    fs.remove_xattr(Path::new("/meta.txt"), b"user.tag")?;
    let xkeys2 = fs.list_xattrs(Path::new("/meta.txt"))?;
    if !xkeys2.is_empty() {
        crate::serial_println!("[memfs]   FAILED: xattr not removed");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   extended attributes: OK");

    // Clean up.
    fs.remove(Path::new("/meta.txt"))?;

    // --- Symlink tests ---
    test_symlinks(&mut fs)?;

    // --- Non-UTF-8 name tests ---
    test_non_utf8_names(&mut fs)?;

    crate::serial_println!("[memfs] Self-test PASSED");
    Ok(())
}

/// A file whose name is not valid UTF-8 must be fully usable.
///
/// This is the end-to-end case the byte-`Path` conversion exists for.  Under
/// the old `&str` API such a file could not even be *named* from kernel code,
/// so every operation on it was unreachable; the only way one could come into
/// existence was through a filesystem driver reading it off disk, and the
/// drivers responded by *skipping* the entry — making the file invisible to
/// `readdir` and its parent directory permanently un-`rmdir`-able (the entry
/// is still on disk, so the directory is never empty, but nothing can name it
/// to delete it).
///
/// The bytes chosen are deliberately hostile: `\xff` is never legal anywhere
/// in a UTF-8 sequence, and `\xc3` is a legal *lead* byte whose continuation
/// is missing — the two ways a lossy decoder fails.  Two names differing only
/// in one such byte must stay distinct, which is what a `from_utf8_lossy`
/// scheme cannot guarantee (both would become the same replacement char).
#[allow(clippy::arithmetic_side_effects)]
fn test_non_utf8_names(fs: &mut MemFs) -> KernelResult<()> {
    let dir = Path::new(b"/nonutf8");
    let a = Path::new(b"/nonutf8/na\xffme.txt");
    let b = Path::new(b"/nonutf8/na\xfeme.txt");
    let lossy_twin = Path::new(b"/nonutf8/na\xc3me.txt");

    fs.mkdir(dir)?;

    // Create, read back.
    fs.write_file(a, b"alpha")?;
    fs.write_file(b, b"beta")?;
    fs.write_file(lossy_twin, b"gamma")?;

    for (path, want) in [
        (a, b"alpha".as_slice()),
        (b, b"beta".as_slice()),
        (lossy_twin, b"gamma".as_slice()),
    ] {
        if fs.read_file(path)? != want {
            crate::serial_println!("[memfs]   FAILED: non-UTF-8 read-back mismatch");
            return Err(KernelError::IoError);
        }
    }

    // All three are distinct entries — a lossy decoder would have collapsed
    // them into one.
    let entries = fs.readdir(dir)?;
    if entries.len() != 3 {
        crate::serial_println!(
            "[memfs]   FAILED: expected 3 non-UTF-8 entries, got {}",
            entries.len()
        );
        return Err(KernelError::IoError);
    }
    let listed_a = entries.iter().any(|e| e.name.as_bytes() == b"na\xffme.txt");
    if !listed_a {
        crate::serial_println!("[memfs]   FAILED: non-UTF-8 name missing from readdir");
        return Err(KernelError::IoError);
    }

    // stat by the exact bytes.
    if fs.stat(a)?.size != 5 {
        crate::serial_println!("[memfs]   FAILED: non-UTF-8 stat size");
        return Err(KernelError::IoError);
    }

    // A symlink whose *target* is non-UTF-8 resolves.
    fs.symlink(Path::new(b"/nonutf8/link"), Path::new(b"na\xffme.txt"))?;
    if fs.read_file(Path::new(b"/nonutf8/link"))? != b"alpha" {
        crate::serial_println!("[memfs]   FAILED: symlink to non-UTF-8 target");
        return Err(KernelError::IoError);
    }
    if fs.readlink(Path::new(b"/nonutf8/link"))?.as_bytes() != b"na\xffme.txt" {
        crate::serial_println!("[memfs]   FAILED: readlink lost non-UTF-8 target bytes");
        return Err(KernelError::IoError);
    }

    // Rename between two non-UTF-8 names.
    let renamed = Path::new(b"/nonutf8/re\xf0named");
    fs.rename(a, renamed)?;
    if fs.read_file(renamed)? != b"alpha" {
        crate::serial_println!("[memfs]   FAILED: non-UTF-8 rename lost data");
        return Err(KernelError::IoError);
    }
    if fs.read_file(a).is_ok() {
        crate::serial_println!("[memfs]   FAILED: old non-UTF-8 name survived rename");
        return Err(KernelError::IoError);
    }

    // Delete everything and rmdir.  If any name were un-nameable the rmdir
    // below would fail with "not empty" — that is the exact production
    // symptom this test guards.
    fs.remove(renamed)?;
    fs.remove(b)?;
    fs.remove(lossy_twin)?;
    fs.remove(Path::new(b"/nonutf8/link"))?;
    fs.rmdir(dir)?;

    crate::serial_println!("[memfs]   non-UTF-8 names: OK");
    Ok(())
}

/// Symlink-specific tests.
#[allow(clippy::arithmetic_side_effects)]
fn test_symlinks(fs: &mut MemFs) -> KernelResult<()> {
    // Create a file and a symlink to it.
    fs.write_file(Path::new("/target.txt"), b"symlink target data")?;
    fs.symlink(Path::new("/link.txt"), Path::new("target.txt"))?;

    // readlink returns the stored target.
    let target = fs.readlink(Path::new("/link.txt"))?;
    if target.as_path() != Path::new("target.txt") {
        crate::serial_println!("[memfs]   FAILED: readlink got '{}'", target.display());
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   symlink + readlink: OK");

    // stat follows the symlink (returns the target's info).
    let st = fs.stat(Path::new("/link.txt"))?;
    if st.entry_type != EntryType::File || st.size != 19 {
        crate::serial_println!("[memfs]   FAILED: stat through symlink");
        return Err(KernelError::IoError);
    }

    // lstat does NOT follow (returns the symlink's own info).
    let lst = fs.lstat(Path::new("/link.txt"))?;
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
    let data = fs.read_file(Path::new("/link.txt"))?;
    if data.as_slice() != b"symlink target data" {
        crate::serial_println!("[memfs]   FAILED: read through symlink");
        return Err(KernelError::IoError);
    }

    // write_file through symlink overwrites the target.
    fs.write_file(Path::new("/link.txt"), b"overwritten")?;
    let data2 = fs.read_file(Path::new("/target.txt"))?;
    if data2.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: write through symlink");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   read/write through symlink: OK");

    // remove on the symlink removes the link, not the target.
    fs.remove(Path::new("/link.txt"))?;
    let target_data = fs.read_file(Path::new("/target.txt"))?;
    if target_data.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: remove symlink deleted target");
        return Err(KernelError::IoError);
    }
    match fs.read_file(Path::new("/link.txt")) {
        Err(KernelError::NotFound) => {}
        _ => {
            crate::serial_println!("[memfs]   FAILED: symlink still exists after remove");
            return Err(KernelError::IoError);
        }
    }
    crate::serial_println!("[memfs]   remove symlink (not target): OK");

    // Symlink to a directory.
    fs.mkdir(Path::new("/realdir"))?;
    fs.write_file(Path::new("/realdir/file.txt"), b"in realdir")?;
    fs.symlink(Path::new("/dirlink"), Path::new("realdir"))?;
    let entries = fs.readdir(Path::new("/dirlink"))?;
    let has_file = entries
        .iter()
        .any(|e| e.name.as_path() == Path::new("file.txt"));
    if !has_file {
        crate::serial_println!("[memfs]   FAILED: readdir through dir symlink");
        return Err(KernelError::IoError);
    }
    // Access file through the dir symlink.
    let nested = fs.read_file(Path::new("/dirlink/file.txt"))?;
    if nested.as_slice() != b"in realdir" {
        crate::serial_println!("[memfs]   FAILED: read file through dir symlink");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   directory symlink traversal: OK");

    // Symlink chain: a → b → target.txt
    fs.symlink(Path::new("/chain_b"), Path::new("target.txt"))?;
    fs.symlink(Path::new("/chain_a"), Path::new("chain_b"))?;
    let chain_data = fs.read_file(Path::new("/chain_a"))?;
    if chain_data.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: symlink chain");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   symlink chain (a->b->file): OK");

    // Circular symlink detection.
    fs.symlink(Path::new("/circ_a"), Path::new("circ_b"))?;
    fs.symlink(Path::new("/circ_b"), Path::new("circ_a"))?;
    match fs.read_file(Path::new("/circ_a")) {
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
    fs.symlink(Path::new("/dangling"), Path::new("nonexistent.txt"))?;
    match fs.read_file(Path::new("/dangling")) {
        Err(KernelError::NotFound) => {
            crate::serial_println!("[memfs]   dangling symlink -> NotFound: OK");
        }
        _ => {
            crate::serial_println!("[memfs]   FAILED: dangling symlink should be NotFound");
            return Err(KernelError::IoError);
        }
    }

    // Absolute symlink within the filesystem.
    fs.symlink(Path::new("/abs_link"), Path::new("/target.txt"))?;
    let abs_data = fs.read_file(Path::new("/abs_link"))?;
    if abs_data.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: absolute symlink");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   absolute symlink: OK");

    // Relative symlink with .. traversal.
    fs.mkdir(Path::new("/subdir"))?;
    fs.symlink(Path::new("/subdir/up_link"), Path::new("../target.txt"))?;
    let up_data = fs.read_file(Path::new("/subdir/up_link"))?;
    if up_data.as_slice() != b"overwritten" {
        crate::serial_println!("[memfs]   FAILED: relative symlink with ..");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   relative symlink (..): OK");

    // Symlinks appear as Symlink type in readdir.
    let root_entries = fs.readdir(Path::new("/"))?;
    let link_entry = root_entries
        .iter()
        .find(|e| e.name.as_path() == Path::new("abs_link"));
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
    fs.mkdir(Path::new("/nlinkdir"))?;
    let m_empty = fs.metadata(Path::new("/nlinkdir"))?;
    if m_empty.nlinks != 2 {
        crate::serial_println!(
            "[memfs]   FAILED: empty dir nlink expected 2, got {}",
            m_empty.nlinks
        );
        return Err(KernelError::IoError);
    }
    // A regular file and a symlink must NOT bump the parent's link count.
    fs.write_file(Path::new("/nlinkdir/file.txt"), b"x")?;
    fs.symlink(Path::new("/nlinkdir/lnk"), Path::new("file.txt"))?;
    let m_file = fs.metadata(Path::new("/nlinkdir"))?;
    if m_file.nlinks != 2 {
        crate::serial_println!(
            "[memfs]   FAILED: dir nlink with file+symlink expected 2, got {}",
            m_file.nlinks
        );
        return Err(KernelError::IoError);
    }
    // Two subdirectories bring it to 4.
    fs.mkdir(Path::new("/nlinkdir/sub1"))?;
    fs.mkdir(Path::new("/nlinkdir/sub2"))?;
    let m_subs = fs.metadata(Path::new("/nlinkdir"))?;
    if m_subs.nlinks != 4 {
        crate::serial_println!(
            "[memfs]   FAILED: dir nlink with 2 subdirs expected 4, got {}",
            m_subs.nlinks
        );
        return Err(KernelError::IoError);
    }
    // Removing one subdirectory drops it back to 3.
    fs.rmdir(Path::new("/nlinkdir/sub1"))?;
    let m_after = fs.metadata(Path::new("/nlinkdir"))?;
    if m_after.nlinks != 3 {
        crate::serial_println!(
            "[memfs]   FAILED: dir nlink after rmdir expected 3, got {}",
            m_after.nlinks
        );
        return Err(KernelError::IoError);
    }
    // A regular file still reports a single link.
    let m_regfile = fs.metadata(Path::new("/nlinkdir/file.txt"))?;
    if m_regfile.nlinks != 1 {
        crate::serial_println!(
            "[memfs]   FAILED: file nlink expected 1, got {}",
            m_regfile.nlinks
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   directory link count (st_nlink): OK");
    // Clean up the nlink fixtures.
    fs.remove(Path::new("/nlinkdir/lnk"))?;
    fs.remove(Path::new("/nlinkdir/file.txt"))?;
    fs.rmdir(Path::new("/nlinkdir/sub2"))?;
    fs.rmdir(Path::new("/nlinkdir"))?;

    // --- Hard links ---
    //
    // The property under test is that a hard link is a second *name*, not a
    // second *object*.  Every assertion below is one that an implementation
    // which copied instead of linking would fail, which is why they check
    // shared data and shared inode numbers rather than merely that `link`
    // returned Ok.
    let inodes_before = fs.statvfs()?.total_inodes;

    fs.write_file(Path::new("/hl_orig.txt"), b"shared")?;
    fs.link(Path::new("/hl_orig.txt"), Path::new("/hl_second.txt"))?;

    // Same object: same inode number from both names, and nlink is 2.
    let a = fs.stat(Path::new("/hl_orig.txt"))?;
    let b = fs.stat(Path::new("/hl_second.txt"))?;
    if a.ino != b.ino || a.ino == 0 {
        crate::serial_println!(
            "[memfs]   FAILED: hard link inodes differ ({} vs {})",
            a.ino,
            b.ino
        );
        return Err(KernelError::IoError);
    }
    if fs.metadata(Path::new("/hl_orig.txt"))?.nlinks != 2 {
        crate::serial_println!("[memfs]   FAILED: nlink after link != 2");
        return Err(KernelError::IoError);
    }
    // A second object holding equal bytes would pass the checks above; only
    // a write through one name showing up under the other rules it out.
    fs.write_file(Path::new("/hl_second.txt"), b"written via the second name")?;
    if fs.read_file(Path::new("/hl_orig.txt"))?.as_slice() != b"written via the second name" {
        crate::serial_println!("[memfs]   FAILED: hard links do not share data");
        return Err(KernelError::IoError);
    }
    // Renaming one name moves that name and nothing else.
    fs.rename(Path::new("/hl_second.txt"), Path::new("/hl_renamed.txt"))?;
    if fs.metadata(Path::new("/hl_orig.txt"))?.nlinks != 2 {
        crate::serial_println!("[memfs]   FAILED: rename changed a link count");
        return Err(KernelError::IoError);
    }
    // Renaming one hard link onto another name of the *same* object is a
    // no-op success, not a move: POSIX requires both names to survive.  A
    // plain detach-and-reattach passes every other assertion here and fails
    // this one by leaving a single name behind.
    fs.link(Path::new("/hl_orig.txt"), Path::new("/hl_third.txt"))?;
    fs.rename(Path::new("/hl_third.txt"), Path::new("/hl_renamed.txt"))?;
    if fs.stat(Path::new("/hl_third.txt")).is_err()
        || fs.stat(Path::new("/hl_renamed.txt")).is_err()
    {
        crate::serial_println!("[memfs]   FAILED: rename between two names of one object lost one");
        return Err(KernelError::IoError);
    }
    if fs.metadata(Path::new("/hl_orig.txt"))?.nlinks != 3 {
        crate::serial_println!("[memfs]   FAILED: same-object rename changed the link count");
        return Err(KernelError::IoError);
    }
    fs.remove(Path::new("/hl_third.txt"))?;
    crate::serial_println!("[memfs]   rename between two names of one object is a no-op: OK");

    // Unlinking one name must decrement, not delete: the surviving name still
    // reads the data.  This is the case the old owning-tree memfs could not
    // represent at all.
    fs.remove(Path::new("/hl_renamed.txt"))?;
    if fs.read_file(Path::new("/hl_orig.txt"))?.as_slice() != b"written via the second name" {
        crate::serial_println!("[memfs]   FAILED: unlinking one name destroyed the object");
        return Err(KernelError::IoError);
    }
    if fs.metadata(Path::new("/hl_orig.txt"))?.nlinks != 1 {
        crate::serial_println!("[memfs]   FAILED: nlink after unlink != 1");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   hard link (shared inode, shared data, nlink): OK");

    // link never replaces an existing name — unlike rename, which does.
    match fs.link(Path::new("/hl_orig.txt"), Path::new("/hl_orig.txt")) {
        Err(KernelError::AlreadyExists) => {}
        other => {
            crate::serial_println!("[memfs]   FAILED: link onto existing name gave {:?}", other);
            return Err(KernelError::IoError);
        }
    }
    // Directories are refused, so the namespace stays a tree and the path
    // resolver's no-cycles assumption holds.
    fs.mkdir(Path::new("/hl_dir"))?;
    match fs.link(Path::new("/hl_dir"), Path::new("/hl_dir_alias")) {
        Err(KernelError::PermissionDenied) => {}
        other => {
            crate::serial_println!("[memfs]   FAILED: hard link to directory gave {:?}", other);
            return Err(KernelError::IoError);
        }
    }
    fs.rmdir(Path::new("/hl_dir"))?;
    crate::serial_println!("[memfs]   link refuses directories and existing names: OK");

    // Follow vs no-follow on a symlink source.  `link` dereferences and links
    // the *target*; `link_no_follow` links the *symlink inode*.  The two are
    // distinguishable only by asking whether the new name is itself a symlink.
    fs.symlink(Path::new("/hl_sym"), Path::new("hl_orig.txt"))?;
    fs.link(Path::new("/hl_sym"), Path::new("/hl_followed"))?;
    fs.link_no_follow(Path::new("/hl_sym"), Path::new("/hl_unfollowed"))?;
    if fs.lstat(Path::new("/hl_followed"))?.entry_type != EntryType::File {
        crate::serial_println!(
            "[memfs]   FAILED: link() through a symlink did not link the target"
        );
        return Err(KernelError::IoError);
    }
    if fs.lstat(Path::new("/hl_unfollowed"))?.entry_type != EntryType::Symlink {
        crate::serial_println!(
            "[memfs]   FAILED: link_no_follow() did not link the symlink itself"
        );
        return Err(KernelError::IoError);
    }
    if fs.readlink(Path::new("/hl_unfollowed"))?.as_path() != Path::new("hl_orig.txt") {
        crate::serial_println!("[memfs]   FAILED: linked symlink lost its target");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   link follow vs no-follow on a symlink: OK");

    // Removing every name frees the object: the inode table returns to the
    // size it had before this block, which no per-name bookkeeping would
    // give if a link had leaked an inode.
    fs.remove(Path::new("/hl_unfollowed"))?;
    fs.remove(Path::new("/hl_followed"))?;
    fs.remove(Path::new("/hl_sym"))?;
    fs.remove(Path::new("/hl_orig.txt"))?;
    match fs.read_file(Path::new("/hl_orig.txt")) {
        Err(KernelError::NotFound) => {}
        other => {
            crate::serial_println!(
                "[memfs]   FAILED: last unlink left the file readable: {:?}",
                other.map(|d| d.len())
            );
            return Err(KernelError::IoError);
        }
    }
    let inodes_after = fs.statvfs()?.total_inodes;
    if inodes_after != inodes_before {
        crate::serial_println!(
            "[memfs]   FAILED: hard link test leaked inodes ({} -> {})",
            inodes_before,
            inodes_after
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[memfs]   last unlink frees the inode: OK");

    // Clean up.
    fs.remove(Path::new("/target.txt"))?;
    fs.remove(Path::new("/realdir/file.txt"))?;
    fs.rmdir(Path::new("/realdir"))?;
    fs.remove(Path::new("/dirlink"))?;
    fs.remove(Path::new("/chain_a"))?;
    fs.remove(Path::new("/chain_b"))?;
    fs.remove(Path::new("/circ_a"))?;
    fs.remove(Path::new("/circ_b"))?;
    fs.remove(Path::new("/dangling"))?;
    fs.remove(Path::new("/abs_link"))?;
    fs.remove(Path::new("/subdir/up_link"))?;
    fs.rmdir(Path::new("/subdir"))?;

    Ok(())
}
