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
//! Uses a tree of [`MemFsNode`] nodes.  Each node is either a
//! [`File`](MemFsNodeKind::File) (data: `Vec<u8>`) or a
//! [`Dir`](MemFsNodeKind::Dir) (children: `BTreeMap<name, node>`).
//!
//! Path resolution walks the tree component by component with
//! exact (case-sensitive) matching.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileAttr, FileMeta, FileSystem, Timestamp};

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Kind of a memory filesystem node.
enum MemFsNodeKind {
    /// A regular file with byte contents.
    File(Vec<u8>),
    /// A directory containing named children.
    Dir(BTreeMap<String, MemFsNode>),
}

/// A single node in the memory filesystem tree.
struct MemFsNode {
    kind: MemFsNodeKind,
    /// Timestamps (nanoseconds since boot).
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

impl MemFsNode {
    fn new_file(data: Vec<u8>) -> Self {
        let now = crate::hpet::elapsed_ns();
        Self {
            kind: MemFsNodeKind::File(data),
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
        let now = crate::hpet::elapsed_ns();
        Self {
            kind: MemFsNodeKind::Dir(BTreeMap::new()),
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

    fn is_dir(&self) -> bool {
        matches!(self.kind, MemFsNodeKind::Dir(_))
    }

    fn is_file(&self) -> bool {
        matches!(self.kind, MemFsNodeKind::File(_))
    }

    fn file_data(&self) -> Option<&Vec<u8>> {
        match &self.kind {
            MemFsNodeKind::File(data) => Some(data),
            MemFsNodeKind::Dir(_) => None,
        }
    }

    fn file_data_mut(&mut self) -> Option<&mut Vec<u8>> {
        match &mut self.kind {
            MemFsNodeKind::File(data) => Some(data),
            MemFsNodeKind::Dir(_) => None,
        }
    }

    fn children(&self) -> Option<&BTreeMap<String, MemFsNode>> {
        match &self.kind {
            MemFsNodeKind::Dir(children) => Some(children),
            MemFsNodeKind::File(_) => None,
        }
    }

    fn children_mut(&mut self) -> Option<&mut BTreeMap<String, MemFsNode>> {
        match &mut self.kind {
            MemFsNodeKind::Dir(children) => Some(children),
            MemFsNodeKind::File(_) => None,
        }
    }

    /// Size in bytes: file data length, or 0 for directories.
    fn size(&self) -> u64 {
        match &self.kind {
            MemFsNodeKind::File(data) => data.len() as u64,
            MemFsNodeKind::Dir(_) => 0,
        }
    }

    /// Convert to a VFS DirEntry.
    fn to_dir_entry(&self, name: &str) -> DirEntry {
        DirEntry {
            name: String::from(name),
            entry_type: if self.is_dir() {
                EntryType::Directory
            } else {
                EntryType::File
            },
            size: self.size(),
        }
    }

    /// Convert to rich FileMeta.
    fn to_file_meta(&self) -> FileMeta {
        FileMeta {
            size: self.size(),
            entry_type: if self.is_dir() {
                EntryType::Directory
            } else {
                EntryType::File
            },
            created_ns: self.created_ns,
            modified_ns: self.modified_ns,
            accessed_ns: self.accessed_ns,
            changed_ns: self.changed_ns,
            uid: self.uid,
            gid: self.gid,
            permissions: self.permissions,
            attributes: self.attributes,
            xattrs: self.xattrs.clone(),
            hash: Vec::new(),
        }
    }

    /// Update modification and change timestamps to now.
    fn touch_modified(&mut self) {
        let now = crate::hpet::elapsed_ns();
        self.modified_ns = now;
        self.changed_ns = now;
    }

    /// Update access timestamp with relatime semantics.
    ///
    /// Only updates if accessed_ns < modified_ns or if more than
    /// one day has elapsed since last access.
    fn touch_accessed_relatime(&mut self) {
        let now = crate::hpet::elapsed_ns();
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

    /// Resolve a path to a reference to the node.
    ///
    /// Returns `None` for the root directory (when path is "/").
    fn resolve(&self, path: &str) -> KernelResult<&MemFsNode> {
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

    /// Resolve a path to a mutable reference to the node.
    fn resolve_mut(&mut self, path: &str) -> KernelResult<&mut MemFsNode> {
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

    /// Resolve the parent directory of a path.
    ///
    /// Returns `(parent_node, filename)`.
    fn resolve_parent_mut<'a, 'b>(
        &'a mut self,
        path: &'b str,
    ) -> KernelResult<(&'a mut MemFsNode, &'b str)> {
        let components = Self::path_components(path);
        if components.is_empty() {
            return Err(KernelError::InvalidArgument); // Can't get parent of root.
        }

        let filename = components[components.len() - 1];
        let parent_components = &components[..components.len() - 1];

        let mut current = &mut self.root;
        for component in parent_components {
            let children = current.children_mut().ok_or(KernelError::NotADirectory)?;
            current = children.get_mut(*component).ok_or(KernelError::NotFound)?;
        }

        if !current.is_dir() {
            return Err(KernelError::NotADirectory);
        }

        Ok((current, filename))
    }

    /// Split a path into components, filtering out empty parts and ".".
    fn path_components(path: &str) -> Vec<&str> {
        path.split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .collect()
    }
}

impl FileSystem for MemFs {
    fn fs_type(&self) -> &str {
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
        let (parent, filename) = self.resolve_parent_mut(path)?;
        let children = parent.children_mut().ok_or(KernelError::NotADirectory)?;

        match children.get_mut(filename) {
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
                let file_data = existing.file_data_mut()
                    .ok_or(KernelError::IsADirectory)?;
                file_data.clear();
                file_data.extend_from_slice(data);
                // NLL: file_data's last use is above; existing is free here.
                drop(file_data);
                existing.touch_modified();
            }
            None => {
                // Create new file (constructor sets timestamps to now).
                children.insert(
                    String::from(filename),
                    MemFsNode::new_file(data.to_vec()),
                );
            }
        }
        // NLL: children's last use is inside the match; parent is free here.
        parent.touch_modified();
        Ok(())
    }

    fn remove(&mut self, path: &str) -> KernelResult<()> {
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
                // Create the file first.
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

        // Update timestamps (file_data borrow released by NLL here).
        drop(file_data);
        node.touch_modified();
        Ok(())
    }

    fn truncate(&mut self, path: &str, size: u64) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        // Check attributes before getting mutable data reference.
        if node.attributes.contains(FileAttr::IMMUTABLE) || node.attributes.contains(FileAttr::APPEND_ONLY) {
            return Err(KernelError::PermissionDenied);
        }
        let file_data = node.file_data_mut().ok_or(KernelError::IsADirectory)?;
        file_data.resize(size as usize, 0);
        // Update timestamps (file_data borrow released by NLL here).
        drop(file_data);
        node.touch_modified();
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> KernelResult<()> {
        // Strategy: remove the source node, insert at destination.
        // Must be done in two steps because we can't hold two mutable
        // references into the tree simultaneously.

        // Step 1: Remove the source node.
        let from_components = Self::path_components(from);
        if from_components.is_empty() {
            return Err(KernelError::InvalidArgument); // Can't rename root.
        }
        let from_name = from_components[from_components.len() - 1];

        let removed_node = {
            let parent_components = &from_components[..from_components.len() - 1];
            let mut current = &mut self.root;
            for component in parent_components {
                let children = current.children_mut().ok_or(KernelError::NotADirectory)?;
                current = children.get_mut(*component).ok_or(KernelError::NotFound)?;
            }
            let children = current.children_mut().ok_or(KernelError::NotADirectory)?;
            children.remove(from_name).ok_or(KernelError::NotFound)?
        };

        // Step 2: Insert at destination.
        let to_components = Self::path_components(to);
        if to_components.is_empty() {
            // Can't rename to root — re-insert the source and fail.
            // (This would require putting it back, which is complex.
            // In practice this path is unreachable because rename("/foo", "/")
            // doesn't make sense.)
            return Err(KernelError::InvalidArgument);
        }
        let to_name = to_components[to_components.len() - 1];

        let to_parent_components = &to_components[..to_components.len() - 1];
        let mut current = &mut self.root;
        for component in to_parent_components {
            let children = current.children_mut().ok_or(KernelError::NotADirectory)?;
            current = children.get_mut(*component).ok_or(KernelError::NotFound)?;
        }
        let children = current.children_mut().ok_or(KernelError::NotADirectory)?;

        if children.contains_key(to_name) {
            return Err(KernelError::AlreadyExists);
        }

        children.insert(String::from(to_name), removed_node);
        Ok(())
    }

    fn debug_stats(&self) -> String {
        fn count_nodes(node: &MemFsNode) -> (usize, usize, u64) {
            match &node.kind {
                MemFsNodeKind::File(data) => (1, 0, data.len() as u64),
                MemFsNodeKind::Dir(children) => {
                    let mut files = 0usize;
                    let mut dirs = 1usize; // Count this dir.
                    let mut bytes = 0u64;
                    for child in children.values() {
                        let (f, d, b) = count_nodes(child);
                        files = files.wrapping_add(f);
                        dirs = dirs.wrapping_add(d);
                        bytes = bytes.wrapping_add(b);
                    }
                    (files, dirs, bytes)
                }
            }
        }

        let (files, dirs, bytes) = count_nodes(&self.root);
        use core::fmt::Write;
        let mut s = String::new();
        let _ = write!(s, "memfs: {} files, {} dirs, {} bytes", files, dirs, bytes);
        s
    }

    // --- Extended metadata operations ---

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let node = self.resolve(path)?;
        Ok(node.to_file_meta())
    }

    fn set_attributes(&mut self, path: &str, attrs: FileAttr) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.attributes = attrs;
        node.changed_ns = crate::hpet::elapsed_ns();
        Ok(())
    }

    fn set_owner(&mut self, path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.uid = uid;
        node.gid = gid;
        node.changed_ns = crate::hpet::elapsed_ns();
        Ok(())
    }

    fn set_permissions(&mut self, path: &str, permissions: u16) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        node.permissions = permissions;
        node.changed_ns = crate::hpet::elapsed_ns();
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

    fn get_xattr(&mut self, path: &str, key: &str) -> KernelResult<Vec<u8>> {
        let node = self.resolve(path)?;
        for (k, v) in &node.xattrs {
            if k == key {
                return Ok(v.clone());
            }
        }
        Err(KernelError::NotFound)
    }

    fn set_xattr(&mut self, path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        // Enforce max key length (255 bytes) and max value size (64 KiB).
        if key.len() > 255 {
            return Err(KernelError::InvalidArgument);
        }
        if value.len() > 65536 {
            return Err(KernelError::InvalidArgument);
        }

        let node = self.resolve_mut(path)?;
        // Update existing or insert new.
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
        node.changed_ns = crate::hpet::elapsed_ns();
        Ok(())
    }

    fn remove_xattr(&mut self, path: &str, key: &str) -> KernelResult<()> {
        let node = self.resolve_mut(path)?;
        let orig_len = node.xattrs.len();
        node.xattrs.retain(|(k, _)| k != key);
        if node.xattrs.len() == orig_len {
            return Err(KernelError::NotFound);
        }
        node.changed_ns = crate::hpet::elapsed_ns();
        Ok(())
    }

    fn list_xattrs(&mut self, path: &str) -> KernelResult<Vec<String>> {
        let node = self.resolve(path)?;
        Ok(node.xattrs.iter().map(|(k, _)| k.clone()).collect())
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

/// Self-test: verify basic MemFs operations.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[memfs] Running self-test...");

    // Create a standalone MemFs instance (don't mount globally — avoid
    // interfering with the real VFS mount at /).
    let mut fs = MemFs::new();

    // Test mkdir.
    fs.mkdir("/testdir")?;
    let entries = fs.readdir("/")?;
    let has_testdir = entries.iter().any(|e| e.name == "testdir" && e.entry_type == EntryType::Directory);
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
            crate::serial_println!("[memfs]   Case sensitivity: OK (Hello.txt ≠ hello.txt)");
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

    crate::serial_println!("[memfs] Self-test PASSED");
    Ok(())
}
