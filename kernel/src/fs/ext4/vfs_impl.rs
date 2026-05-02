//! ext4 `FileSystem` trait implementation for VFS integration.
//!
//! Bridges the ext4 driver to the VFS layer, allowing ext4 filesystems
//! to be mounted alongside FAT, memfs, procfs, etc.
//!
//! Supports full read-write operations: file create/overwrite/delete,
//! directory create/delete, with proper block and inode reclamation
//! via the bitmap allocator.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileSystem};

use super::driver::Ext4Driver;
use super::ondisk::file_type;

// ---------------------------------------------------------------------------
// FileSystem implementation
// ---------------------------------------------------------------------------

/// ext4 filesystem that implements the VFS [`FileSystem`] trait.
///
/// Wraps an [`Ext4Driver`] and provides the standard VFS interface.
/// The VFS layer handles mount-point resolution; this implementation
/// receives paths relative to the ext4 mount point.
pub struct Ext4Fs {
    driver: Ext4Driver,
}

impl Ext4Fs {
    /// Create a new ext4 VFS wrapper from an opened driver.
    pub fn new(driver: Ext4Driver) -> Self {
        Self { driver }
    }

    /// Open and mount an ext4 filesystem from a block device.
    pub fn open(device: &str) -> KernelResult<Self> {
        let driver = Ext4Driver::open(device)?;
        Ok(Self::new(driver))
    }
}

impl FileSystem for Ext4Fs {
    fn fs_type(&self) -> &str {
        "ext4"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        // Verify it's a directory.
        if (inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        let raw_entries = self.driver.read_dir_entries(&inode)?;

        let entries = raw_entries
            .into_iter()
            .filter(|(_, _, name)| name != "." && name != "..")
            .map(|(child_ino, ftype, name)| {
                let entry_type = dir_type_to_entry_type(ftype);
                // Try to get the file size from the child inode.
                let size = self.driver.read_inode(child_ino)
                    .map(|ci| inode_file_size(&ci))
                    .unwrap_or(0);
                DirEntry {
                    name,
                    entry_type,
                    size,
                }
            })
            .collect();

        Ok(entries)
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode = inode.i_mode & file_type::S_IFMT;
        match mode {
            file_type::S_IFREG => {
                self.driver.read_file_data(&inode)
            }
            file_type::S_IFLNK => {
                // For symlinks, the target path is stored:
                // - In i_block if the target is <= 60 bytes (fast symlink)
                // - In data blocks otherwise
                self.read_symlink_target(&inode)
            }
            file_type::S_IFDIR => Err(KernelError::IsADirectory),
            _ => Err(KernelError::NotSupported),
        }
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode = inode.i_mode & file_type::S_IFMT;
        let entry_type = mode_to_entry_type(mode);
        let size = inode_file_size(&inode);

        // Extract the name from the path.
        let name = path.rsplit('/').next().unwrap_or(path);
        let name = if name.is_empty() { "/" } else { name };

        Ok(DirEntry {
            name: String::from(name),
            entry_type,
            size,
        })
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        // Check if the file already exists.
        match self.driver.resolve_path(path) {
            Ok(ino) => {
                // File exists — overwrite its contents.
                let mut inode = self.driver.read_inode(ino)?;

                // Only regular files can be written.
                let mode = inode.i_mode & file_type::S_IFMT;
                if mode != file_type::S_IFREG {
                    return Err(KernelError::NotSupported);
                }

                // Free the old blocks before writing new ones.
                // This prevents block leaks on overwrite.
                self.driver.free_inode_data(&inode)?;

                self.driver.write_file_data(&mut inode, data)?;
                self.driver.write_inode(ino, &inode)?;
                self.driver.write_superblock()?;
                self.driver.write_group_descs()?;
                self.driver.flush()?;
                Ok(())
            }
            Err(KernelError::NotFound) => {
                // File doesn't exist — create it.
                self.create_file(path, data)
            }
            Err(e) => Err(e),
        }
    }

    fn remove(&mut self, path: &str) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        // Can't remove directories with remove() — use rmdir().
        let mode = inode.i_mode & file_type::S_IFMT;
        if mode == file_type::S_IFDIR {
            return Err(KernelError::IsADirectory);
        }

        // Remove the directory entry from the parent.
        let (parent_path, name) = split_parent_name(path)?;
        let parent_ino = self.driver.resolve_path(parent_path)?;
        let mut parent_inode = self.driver.read_inode(parent_ino)?;

        self.remove_dir_entry(&mut parent_inode, parent_ino, name)?;

        // Decrement the link count and mark the inode as deleted if it reaches 0.
        let mut inode = inode;
        inode.i_links_count = inode.i_links_count.saturating_sub(1);
        if inode.i_links_count == 0 {
            // Free all data blocks owned by this file.
            self.driver.free_inode_data(&inode)?;

            inode.i_size_lo = 0;
            inode.i_size_high = 0;
            inode.i_blocks_lo = 0;

            // Write the zeroed inode first, then free the inode number.
            self.driver.write_inode(ino, &inode)?;
            self.driver.free_inode_number(ino, false)?;
        } else {
            self.driver.write_inode(ino, &inode)?;
        }
        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn mkdir(&mut self, path: &str) -> KernelResult<()> {
        // Verify parent exists and is a directory.
        let (parent_path, name) = split_parent_name(path)?;
        let parent_ino = self.driver.resolve_path(parent_path)?;
        let mut parent_inode = self.driver.read_inode(parent_ino)?;

        if (parent_inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Check that the name doesn't already exist.
        if self.driver.dir_lookup(&parent_inode, name).is_ok() {
            return Err(KernelError::AlreadyExists);
        }

        // Create the directory inode.
        let preferred_group = self.driver.superblock().inode_group(parent_ino);
        let (dir_ino, mut dir_inode) = self.driver.create_inode(
            file_type::S_IFDIR | 0o755,
            preferred_group,
        )?;

        // Create the initial directory data with . and .. entries.
        let block_size = self.driver.superblock().block_size as usize;
        let mut dir_block = alloc::vec![0u8; block_size];

        // Entry for "." (self).
        write_dot_entry(&mut dir_block, 0, dir_ino, 12);
        // Entry for ".." (parent) — rec_len extends to end of block.
        write_dotdot_entry(&mut dir_block, 12, parent_ino, block_size - 12);

        self.driver.write_file_data(&mut dir_inode, &dir_block)?;
        self.driver.write_inode(dir_ino, &dir_inode)?;

        // Add entry in parent directory.
        self.driver.add_dir_entry(
            &mut parent_inode,
            parent_ino,
            dir_ino,
            name,
            super::ondisk::dir_type::DIR,
        )?;

        // Increment parent's link count (for the new "..").
        parent_inode.i_links_count = parent_inode.i_links_count.saturating_add(1);
        self.driver.write_inode(parent_ino, &parent_inode)?;

        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn rmdir(&mut self, path: &str) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        // Must be a directory.
        if (inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Check that the directory is empty (only . and ..).
        let entries = self.driver.read_dir_entries(&inode)?;
        let real_entries = entries.iter()
            .filter(|(_, _, name)| name != "." && name != "..")
            .count();
        if real_entries > 0 {
            return Err(KernelError::NotEmpty);
        }

        // Remove from parent.
        let (parent_path, name) = split_parent_name(path)?;
        let parent_ino = self.driver.resolve_path(parent_path)?;
        let mut parent_inode = self.driver.read_inode(parent_ino)?;

        self.remove_dir_entry(&mut parent_inode, parent_ino, name)?;

        // Decrement parent's link count (for removed "..").
        parent_inode.i_links_count = parent_inode.i_links_count.saturating_sub(1);
        self.driver.write_inode(parent_ino, &parent_inode)?;

        // Free the directory's data blocks.
        self.driver.free_inode_data(&inode)?;

        // Mark directory inode as deleted.
        let mut inode = inode;
        inode.i_links_count = 0;
        inode.i_size_lo = 0;
        inode.i_size_high = 0;
        inode.i_blocks_lo = 0;
        self.driver.write_inode(ino, &inode)?;

        // Free the inode itself (is_directory=true to update used_dirs count).
        self.driver.free_inode_number(ino, true)?;

        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn debug_stats(&self) -> String {
        self.driver.superblock().summary()
    }
}

impl Ext4Fs {
    /// Create a new file at `path` with the given data.
    fn create_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        let (parent_path, name) = split_parent_name(path)?;
        let parent_ino = self.driver.resolve_path(parent_path)?;
        let mut parent_inode = self.driver.read_inode(parent_ino)?;

        if (parent_inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Check that name doesn't already exist.
        if self.driver.dir_lookup(&parent_inode, name).is_ok() {
            return Err(KernelError::AlreadyExists);
        }

        // Allocate inode in same group as parent for locality.
        let preferred_group = self.driver.superblock().inode_group(parent_ino);
        let (file_ino, mut file_inode) = self.driver.create_inode(
            file_type::S_IFREG | 0o644,
            preferred_group,
        )?;

        // Write file data.
        self.driver.write_file_data(&mut file_inode, data)?;
        self.driver.write_inode(file_ino, &file_inode)?;

        // Add directory entry in parent.
        self.driver.add_dir_entry(
            &mut parent_inode,
            parent_ino,
            file_ino,
            name,
            super::ondisk::dir_type::REG_FILE,
        )?;

        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    /// Remove a directory entry by name from a directory.
    fn remove_dir_entry(
        &mut self,
        dir_inode: &mut super::ondisk::Ext4Inode,
        _dir_ino: u32,
        name: &str,
    ) -> KernelResult<()> {
        let mut dir_data = self.driver.read_file_data(dir_inode)?;
        let entry_header_size = core::mem::size_of::<super::ondisk::Ext4DirEntry2>();
        let mut offset = 0usize;
        let mut prev_offset: Option<usize> = None;

        while offset.saturating_add(entry_header_size) <= dir_data.len() {
            let hdr_bytes = dir_data.get(offset..offset.saturating_add(entry_header_size))
                .ok_or(KernelError::IoError)?;
            let hdr = super::driver::read_struct_pub::<super::ondisk::Ext4DirEntry2>(hdr_bytes)?;

            if hdr.rec_len == 0 {
                break;
            }

            if hdr.inode != 0 && hdr.name_len > 0 {
                let name_start = offset.saturating_add(entry_header_size);
                let name_end = name_start.saturating_add(hdr.name_len as usize);
                if let Some(name_bytes) = dir_data.get(name_start..name_end) {
                    if name_bytes == name.as_bytes() {
                        // Found the entry. Remove it by setting inode to 0
                        // and merging with the previous entry if possible.
                        if let Some(prev_off) = prev_offset {
                            // Merge: add this entry's rec_len to the previous entry's.
                            let prev_rec_bytes = dir_data.get(
                                prev_off.saturating_add(4)..prev_off.saturating_add(6)
                            ).ok_or(KernelError::IoError)?;
                            let prev_rec = u16::from_le_bytes([
                                *prev_rec_bytes.first().ok_or(KernelError::IoError)?,
                                *prev_rec_bytes.get(1).ok_or(KernelError::IoError)?,
                            ]);
                            let new_rec = prev_rec.saturating_add(hdr.rec_len);
                            if let Some(dest) = dir_data.get_mut(
                                prev_off.saturating_add(4)..prev_off.saturating_add(6)
                            ) {
                                dest.copy_from_slice(&new_rec.to_le_bytes());
                            }
                        } else {
                            // First entry in block: just zero the inode field.
                            if let Some(dest) = dir_data.get_mut(offset..offset.saturating_add(4)) {
                                dest.copy_from_slice(&0u32.to_le_bytes());
                            }
                        }

                        // Write modified directory data back.
                        self.driver.write_file_data(
                            &mut dir_inode.clone(),
                            &dir_data,
                        ).ok(); // Best-effort for now.
                        return Ok(());
                    }
                }
            }

            prev_offset = Some(offset);
            offset = offset.saturating_add(hdr.rec_len as usize);
        }

        Err(KernelError::NotFound)
    }

    /// Read a symlink target from an inode.
    fn read_symlink_target(&self, inode: &super::ondisk::Ext4Inode) -> KernelResult<Vec<u8>> {
        let size = inode_file_size(inode) as usize;

        if size <= 60 {
            // Fast symlink: target stored in i_block.
            let block_bytes = super::driver::inode_block_as_bytes(inode);
            let target = block_bytes.get(..size).ok_or(KernelError::IoError)?;
            Ok(target.to_vec())
        } else {
            // Slow symlink: target stored in data blocks.
            self.driver.read_file_data(inode)
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert ext4 directory entry file type to VFS EntryType.
fn dir_type_to_entry_type(ftype: u8) -> EntryType {
    use super::ondisk::dir_type;
    match ftype {
        dir_type::DIR => EntryType::Directory,
        dir_type::REG_FILE => EntryType::File,
        dir_type::SYMLINK => EntryType::Symlink,
        _ => EntryType::File, // Fallback for block/char/fifo/socket.
    }
}

/// Convert inode mode to VFS EntryType.
fn mode_to_entry_type(mode: u16) -> EntryType {
    match mode {
        file_type::S_IFDIR => EntryType::Directory,
        file_type::S_IFREG => EntryType::File,
        file_type::S_IFLNK => EntryType::Symlink,
        _ => EntryType::File,
    }
}

/// Get the full 64-bit file size from an inode.
fn inode_file_size(inode: &super::ondisk::Ext4Inode) -> u64 {
    let lo = u64::from(inode.i_size_lo);
    let is_file = (inode.i_mode & file_type::S_IFMT) == file_type::S_IFREG;
    if is_file {
        lo | (u64::from(inode.i_size_high) << 32)
    } else {
        lo
    }
}

/// Split a path into parent directory and final name component.
///
/// e.g., `"/foo/bar/baz"` → `("/foo/bar", "baz")`
fn split_parent_name(path: &str) -> KernelResult<(&str, &str)> {
    let path = path.strip_suffix('/').unwrap_or(path);
    match path.rfind('/') {
        Some(pos) => {
            let parent = if pos == 0 { "/" } else { &path[..pos] };
            let name = &path[pos + 1..];
            if name.is_empty() {
                Err(KernelError::InvalidArgument)
            } else {
                Ok((parent, name))
            }
        }
        None => Err(KernelError::InvalidArgument),
    }
}

/// Write a "." directory entry at the given offset.
fn write_dot_entry(buf: &mut [u8], offset: usize, inode: u32, rec_len: usize) {
    // inode (4 bytes)
    if let Some(dest) = buf.get_mut(offset..offset + 4) {
        dest.copy_from_slice(&inode.to_le_bytes());
    }
    // rec_len (2 bytes)
    if let Some(dest) = buf.get_mut(offset + 4..offset + 6) {
        dest.copy_from_slice(&(rec_len as u16).to_le_bytes());
    }
    // name_len (1 byte)
    if let Some(b) = buf.get_mut(offset + 6) {
        *b = 1; // "."
    }
    // file_type (1 byte)
    if let Some(b) = buf.get_mut(offset + 7) {
        *b = super::ondisk::dir_type::DIR;
    }
    // name
    if let Some(b) = buf.get_mut(offset + 8) {
        *b = b'.';
    }
}

/// Write a ".." directory entry at the given offset.
fn write_dotdot_entry(buf: &mut [u8], offset: usize, inode: u32, rec_len: usize) {
    if let Some(dest) = buf.get_mut(offset..offset + 4) {
        dest.copy_from_slice(&inode.to_le_bytes());
    }
    if let Some(dest) = buf.get_mut(offset + 4..offset + 6) {
        dest.copy_from_slice(&(rec_len as u16).to_le_bytes());
    }
    if let Some(b) = buf.get_mut(offset + 6) {
        *b = 2; // ".."
    }
    if let Some(b) = buf.get_mut(offset + 7) {
        *b = super::ondisk::dir_type::DIR;
    }
    if let Some(b) = buf.get_mut(offset + 8) {
        *b = b'.';
    }
    if let Some(b) = buf.get_mut(offset + 9) {
        *b = b'.';
    }
}
