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
use crate::fs::vfs::{DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo};

use super::driver::Ext4Driver;
use super::ondisk::{file_type, inode_flags};

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
                let inode = self.driver.read_inode(ino)?;

                // Only regular files can be written.
                let mode = inode.i_mode & file_type::S_IFMT;
                if mode != file_type::S_IFREG {
                    return Err(KernelError::NotSupported);
                }

                // Crash-safe overwrite ordering:
                // 1. Save old inode (holds extent tree pointing to old blocks)
                // 2. Write new data (allocates new blocks, updates inode)
                // 3. Free old blocks (safe: on-disk inode now points to new data)
                //
                // If we crash after step 2 but before step 3, old blocks
                // are leaked (not ideal) but no data is lost or corrupted.
                // The reverse order (free-then-write) risks pointing the
                // inode at freed blocks if the write fails.
                let old_inode = inode;

                let mut new_inode = old_inode;
                self.driver.write_file_data(&mut new_inode, data)?;
                self.driver.write_inode(ino, &new_inode)?;

                // Now safe to free old blocks — on-disk inode points to new data.
                // Use old_inode which still has the old extent tree.
                self.driver.free_inode_data(&old_inode)?;

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
            // Free the external xattr block if present.
            self.driver.free_xattr_block(&inode)?;

            inode.i_size_lo = 0;
            inode.i_size_high = 0;
            inode.i_blocks_lo = 0;
            inode.i_file_acl_lo = 0;

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

        // Free the directory's data blocks and external xattr block.
        self.driver.free_inode_data(&inode)?;
        self.driver.free_xattr_block(&inode)?;

        // Mark directory inode as deleted.
        let mut inode = inode;
        inode.i_links_count = 0;
        inode.i_size_lo = 0;
        inode.i_size_high = 0;
        inode.i_blocks_lo = 0;
        inode.i_file_acl_lo = 0;
        self.driver.write_inode(ino, &inode)?;

        // Free the inode itself (is_directory=true to update used_dirs count).
        self.driver.free_inode_number(ino, true)?;

        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> KernelResult<()> {
        // Resolve the source inode.
        let src_ino = self.driver.resolve_path(from)?;
        let src_inode = self.driver.read_inode(src_ino)?;
        let src_mode = src_inode.i_mode & file_type::S_IFMT;

        // Determine the directory entry file type for re-insertion.
        let ft_byte = match src_mode {
            file_type::S_IFDIR => super::ondisk::dir_type::DIR,
            file_type::S_IFREG => super::ondisk::dir_type::REG_FILE,
            file_type::S_IFLNK => super::ondisk::dir_type::SYMLINK,
            _ => super::ondisk::dir_type::UNKNOWN,
        };

        // Check that the destination doesn't already exist.
        if self.driver.resolve_path(to).is_ok() {
            return Err(KernelError::AlreadyExists);
        }

        // Split source and destination into parent + name.
        let (src_parent_path, src_name) = split_parent_name(from)?;
        let (dst_parent_path, dst_name) = split_parent_name(to)?;

        let src_parent_ino = self.driver.resolve_path(src_parent_path)?;
        let dst_parent_ino = self.driver.resolve_path(dst_parent_path)?;

        // Add the entry in the destination directory first (safer: if this
        // fails, the source is still intact).
        let mut dst_parent_inode = self.driver.read_inode(dst_parent_ino)?;
        self.driver.add_dir_entry(
            &mut dst_parent_inode,
            dst_parent_ino,
            src_ino,
            dst_name,
            ft_byte,
        )?;

        // Remove the entry from the source directory.
        let mut src_parent_inode = self.driver.read_inode(src_parent_ino)?;
        self.remove_dir_entry(&mut src_parent_inode, src_parent_ino, src_name)?;

        // If moving a directory to a different parent, update ".." entry
        // and adjust link counts.
        if src_mode == file_type::S_IFDIR && src_parent_ino != dst_parent_ino {
            // Update the ".." entry in the moved directory to point to
            // the new parent.
            let mut dir_data = self.driver.read_file_data(&src_inode)?;
            // ".." is the second entry (at offset 12 after the "." entry).
            // Its inode field is at bytes 12..16.
            if let Some(dest) = dir_data.get_mut(12..16) {
                dest.copy_from_slice(&dst_parent_ino.to_le_bytes());
            }
            let mut dir_inode_copy = src_inode;
            self.driver.write_file_data(&mut dir_inode_copy, &dir_data)?;

            // Old parent loses a link (the moved dir's ".." no longer
            // points here), new parent gains one.
            src_parent_inode.i_links_count =
                src_parent_inode.i_links_count.saturating_sub(1);
            self.driver.write_inode(src_parent_ino, &src_parent_inode)?;

            dst_parent_inode.i_links_count =
                dst_parent_inode.i_links_count.saturating_add(1);
            self.driver.write_inode(dst_parent_ino, &dst_parent_inode)?;
        }

        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode = inode.i_mode & file_type::S_IFMT;
        if mode == file_type::S_IFDIR {
            return Err(KernelError::IsADirectory);
        }

        // Use extent-aware range read — only reads the blocks spanning
        // the requested byte range, not the entire file.
        self.driver.read_file_range(&inode, offset, len)
    }

    fn write_at(&mut self, path: &str, offset: u64, data: &[u8]) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode = inode.i_mode & file_type::S_IFMT;
        if mode != file_type::S_IFREG {
            return Err(KernelError::NotSupported);
        }

        let file_size = inode_file_size(&inode);
        let end = offset.saturating_add(data.len() as u64);

        if end <= file_size {
            // Write is within existing file bounds — modify blocks in place.
            // No block allocation needed, no extent tree changes.
            self.driver.write_at_inplace(&inode, offset, data)?;
            self.driver.flush()?;
            Ok(())
        } else {
            // Write extends past file end — fall back to read-modify-write.
            // A production implementation would extend the extent tree
            // incrementally; for now, the safe approach avoids complexity.
            let mut contents = self.driver.read_file_data(&inode)?;
            let start = offset as usize;
            let end_usize = end as usize;

            if end_usize > contents.len() {
                contents.resize(end_usize, 0);
            }
            if let Some(dest) = contents.get_mut(start..start.saturating_add(data.len())) {
                dest.copy_from_slice(data);
            }
            self.write_file(path, &contents)
        }
    }

    fn truncate(&mut self, path: &str, size: u64) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode = inode.i_mode & file_type::S_IFMT;
        if mode != file_type::S_IFREG {
            return Err(KernelError::NotSupported);
        }

        let current_size = inode_file_size(&inode);

        if size == current_size {
            return Ok(());
        }

        if size == 0 {
            // Truncate to zero: free all data blocks, reset inode.
            let old_inode = inode;
            let mut new_inode = inode;

            // Clear size.
            new_inode.i_size_lo = 0;
            new_inode.i_size_high = 0;
            new_inode.i_blocks_lo = 0;

            // Initialize an empty extent header.
            self.driver.init_extent_header_pub(&mut new_inode, 0);

            // Write the new inode first (crash-safe: inode points to
            // nothing, so the old blocks are just leaked on crash).
            self.driver.write_inode(ino, &new_inode)?;

            // Now free the old blocks.
            self.driver.free_inode_data(&old_inode)?;

            self.driver.write_superblock()?;
            self.driver.write_group_descs()?;
            self.driver.flush()?;
            return Ok(());
        }

        if size < current_size {
            // Shrink: read existing data, truncate, rewrite.
            // A fully optimized version would walk the extent tree and
            // free trailing blocks, but that requires extent tree surgery
            // (splitting the last extent).  The read-truncate-rewrite
            // approach is correct and the data volume is bounded by the
            // current file size (which we're shrinking).
            //
            // Crash-safe ordering: write new data first, free old blocks.
            let old_inode = inode;
            let mut data = self.driver.read_file_data(&inode)?;
            data.truncate(size as usize);

            let mut new_inode = inode;
            self.driver.write_file_data(&mut new_inode, &data)?;
            self.driver.write_inode(ino, &new_inode)?;

            // Free old blocks now that inode points to new data.
            self.driver.free_inode_data(&old_inode)?;

            self.driver.write_superblock()?;
            self.driver.write_group_descs()?;
            self.driver.flush()?;
            Ok(())
        } else {
            // Extend: read, resize with zeros, rewrite.
            // Growing in place would require extending the extent tree,
            // which uses the same write_file_data path anyway.
            let mut data = self.driver.read_file_data(&inode)?;
            let old_inode = inode;

            data.resize(size as usize, 0);

            let mut new_inode = inode;
            self.driver.write_file_data(&mut new_inode, &data)?;
            self.driver.write_inode(ino, &new_inode)?;

            self.driver.free_inode_data(&old_inode)?;

            self.driver.write_superblock()?;
            self.driver.write_group_descs()?;
            self.driver.flush()?;
            Ok(())
        }
    }

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode_type = inode.i_mode & file_type::S_IFMT;
        let entry_type = mode_to_entry_type(mode_type);
        let size = inode_file_size(&inode);
        let permissions = inode.i_mode & 0o7777; // lower 12 bits

        // Map inode flags to our FileAttr.
        let mut attrs = FileAttr::NONE;
        if inode.i_flags & inode_flags::IMMUTABLE != 0 {
            attrs = attrs.union(FileAttr::IMMUTABLE);
        }
        if inode.i_flags & inode_flags::APPEND != 0 {
            attrs = attrs.union(FileAttr::APPEND_ONLY);
        }

        // ext4 stores Unix timestamps as seconds since epoch.
        // Our FileMeta uses nanoseconds — convert.
        let sec_to_ns = |s: u32| u64::from(s).saturating_mul(1_000_000_000);

        Ok(FileMeta {
            size,
            entry_type,
            created_ns: 0, // ext4 core inode doesn't have crtime (it's in extra)
            modified_ns: sec_to_ns(inode.i_mtime),
            accessed_ns: sec_to_ns(inode.i_atime),
            changed_ns: sec_to_ns(inode.i_ctime),
            uid: u32::from(inode.i_uid),
            gid: u32::from(inode.i_gid),
            permissions,
            attributes: attrs,
            xattrs: self.driver.read_xattrs(&inode).unwrap_or_default(),
            hash: Vec::new(),
        })
    }

    fn set_permissions(&mut self, path: &str, permissions: u16) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let mut inode = self.driver.read_inode(ino)?;

        // Preserve the file type bits, update only the permission bits.
        let type_bits = inode.i_mode & file_type::S_IFMT;
        inode.i_mode = type_bits | (permissions & 0o7777);

        self.driver.write_inode(ino, &inode)?;
        self.driver.flush()?;
        Ok(())
    }

    fn set_owner(&mut self, path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let mut inode = self.driver.read_inode(ino)?;

        // Note: ext4 supports 32-bit UIDs via i_uid + i_osd2 high bits.
        // We only set the low 16 bits for now.
        inode.i_uid = uid as u16;
        inode.i_gid = gid as u16;

        self.driver.write_inode(ino, &inode)?;
        self.driver.flush()?;
        Ok(())
    }

    fn set_times(
        &mut self,
        path: &str,
        accessed_ns: crate::fs::vfs::Timestamp,
        modified_ns: crate::fs::vfs::Timestamp,
    ) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let mut inode = self.driver.read_inode(ino)?;

        // Convert nanoseconds to seconds (ext4 core inode stores seconds).
        // Pass 0 to leave a timestamp unchanged.
        let ns_to_sec = |ns: u64| -> u32 {
            if ns == 0 { return 0; }
            (ns / 1_000_000_000) as u32
        };

        if accessed_ns != 0 {
            inode.i_atime = ns_to_sec(accessed_ns);
        }
        if modified_ns != 0 {
            inode.i_mtime = ns_to_sec(modified_ns);
            // Also update ctime (metadata change time) when mtime changes.
            inode.i_ctime = ns_to_sec(modified_ns);
        }

        self.driver.write_inode(ino, &inode)?;
        self.driver.flush()?;
        Ok(())
    }

    fn set_attributes(&mut self, path: &str, attrs: FileAttr) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let mut inode = self.driver.read_inode(ino)?;

        // Map our FileAttr flags to ext4 inode flags.
        // Preserve all other inode flags (like EXTENTS).
        let mut flags = inode.i_flags;

        // Clear the bits we manage, then set them if requested.
        flags &= !(inode_flags::IMMUTABLE | inode_flags::APPEND);
        if attrs.contains(FileAttr::IMMUTABLE) {
            flags |= inode_flags::IMMUTABLE;
        }
        if attrs.contains(FileAttr::APPEND_ONLY) {
            flags |= inode_flags::APPEND;
        }
        inode.i_flags = flags;

        self.driver.write_inode(ino, &inode)?;
        self.driver.flush()?;
        Ok(())
    }

    fn get_xattr(&mut self, path: &str, key: &str) -> KernelResult<Vec<u8>> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;
        self.driver.get_xattr(&inode, key)
    }

    fn set_xattr(&mut self, path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let mut inode = self.driver.read_inode(ino)?;

        // Read existing xattrs, update or add the new one.
        let mut attrs = self.driver.read_xattrs(&inode)?;

        // Check key length (255 bytes max per design spec).
        if key.len() > 255 {
            return Err(KernelError::InvalidArgument);
        }
        // Check value size (64 KiB max per design spec).
        if value.len() > 65536 {
            return Err(KernelError::InvalidArgument);
        }

        // Update existing or insert new.
        let mut found = false;
        for (k, v) in &mut attrs {
            if k == key {
                *v = value.to_vec();
                found = true;
                break;
            }
        }
        if !found {
            attrs.push((String::from(key), value.to_vec()));
        }

        self.driver.write_xattr_block(&mut inode, ino, &attrs)?;
        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn remove_xattr(&mut self, path: &str, key: &str) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let mut inode = self.driver.read_inode(ino)?;

        let mut attrs = self.driver.read_xattrs(&inode)?;
        let original_len = attrs.len();
        attrs.retain(|(k, _)| k != key);

        if attrs.len() == original_len {
            // Key wasn't present.
            return Err(KernelError::NotFound);
        }

        self.driver.write_xattr_block(&mut inode, ino, &attrs)?;
        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn list_xattrs(&mut self, path: &str) -> KernelResult<Vec<String>> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;
        let attrs = self.driver.read_xattrs(&inode)?;
        Ok(attrs.into_iter().map(|(k, _)| k).collect())
    }

    fn symlink(&mut self, path: &str, target: &str) -> KernelResult<()> {
        let (parent_path, name) = split_parent_name(path)?;
        let parent_ino = self.driver.resolve_path(parent_path)?;
        let mut parent_inode = self.driver.read_inode(parent_ino)?;

        if (parent_inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Check name doesn't already exist.
        if self.driver.dir_lookup(&parent_inode, name).is_ok() {
            return Err(KernelError::AlreadyExists);
        }

        let target_bytes = target.as_bytes();

        // Allocate the symlink inode.
        let preferred_group = self.driver.superblock().inode_group(parent_ino);
        let (sym_ino, mut sym_inode) = self.driver.create_inode(
            file_type::S_IFLNK | 0o777,
            preferred_group,
        )?;

        if target_bytes.len() <= 60 {
            // Fast symlink: store target in i_block directly (no data blocks).
            let block_bytes = super::driver::inode_block_as_bytes_mut(&mut sym_inode);
            if let Some(dest) = block_bytes.get_mut(..target_bytes.len()) {
                dest.copy_from_slice(target_bytes);
            }
            // Clear the EXTENTS flag — fast symlinks don't use extents.
            sym_inode.i_flags &= !inode_flags::EXTENTS;
        } else {
            // Slow symlink: store target in data blocks.
            self.driver.write_file_data(&mut sym_inode, target_bytes)?;
        }

        // Set the size to the target length.
        sym_inode.i_size_lo = target_bytes.len() as u32;
        sym_inode.i_size_high = 0;

        self.driver.write_inode(sym_ino, &sym_inode)?;

        // Add directory entry in parent.
        self.driver.add_dir_entry(
            &mut parent_inode,
            parent_ino,
            sym_ino,
            name,
            super::ondisk::dir_type::SYMLINK,
        )?;

        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn readlink(&mut self, path: &str) -> KernelResult<String> {
        // Use resolve_path_no_follow so we get the symlink inode itself,
        // not whatever it points to.
        let ino = self.driver.resolve_path_no_follow(path)?;
        let inode = self.driver.read_inode(ino)?;

        if (inode.i_mode & file_type::S_IFMT) != file_type::S_IFLNK {
            return Err(KernelError::InvalidArgument);
        }

        let target_bytes = self.driver.read_symlink_target(&inode)?;
        String::from_utf8(target_bytes)
            .map_err(|_| KernelError::IoError)
    }

    fn lstat(&mut self, path: &str) -> KernelResult<DirEntry> {
        // lstat doesn't follow the final symlink.  resolve_path_no_follow
        // follows all intermediate symlinks but stops at the last component.
        let ino = self.driver.resolve_path_no_follow(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode = inode.i_mode & file_type::S_IFMT;
        let entry_type = mode_to_entry_type(mode);
        let size = inode_file_size(&inode);

        let name = path.rsplit('/').next().unwrap_or(path);
        let name = if name.is_empty() { "/" } else { name };

        Ok(DirEntry {
            name: String::from(name),
            entry_type,
            size,
        })
    }

    fn debug_stats(&self) -> String {
        self.driver.superblock().summary()
    }

    /// Report ext4 filesystem capacity and free space.
    ///
    /// Reads block count and free block count from the superblock.
    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        let sb = self.driver.superblock();
        Ok(FsInfo {
            fs_type: String::from("ext4"),
            block_size: u64::from(sb.block_size),
            total_blocks: sb.block_count,
            free_blocks: sb.free_block_count,
            total_inodes: u64::from(sb.raw.s_inodes_count),
            free_inodes: u64::from(sb.raw.s_free_inodes_count),
            max_name_len: 255,
            read_only: !sb.can_write,
        })
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
                        let mut dir_inode_copy = *dir_inode;
                        self.driver.write_file_data(
                            &mut dir_inode_copy,
                            &dir_data,
                        )?;
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
    ///
    /// Delegates to the driver's `read_symlink_target` which handles
    /// both fast symlinks (≤60 bytes in i_block) and slow symlinks
    /// (target stored in data blocks).
    fn read_symlink_target(&self, inode: &super::ondisk::Ext4Inode) -> KernelResult<Vec<u8>> {
        self.driver.read_symlink_target(inode)
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KernelError;

    // --- split_parent_name tests ---

    #[test]
    fn test_split_parent_name_simple() {
        let (parent, name) = split_parent_name("/foo/bar").unwrap();
        assert_eq!(parent, "/foo");
        assert_eq!(name, "bar");
    }

    #[test]
    fn test_split_parent_name_root() {
        let (parent, name) = split_parent_name("/file.txt").unwrap();
        assert_eq!(parent, "/");
        assert_eq!(name, "file.txt");
    }

    #[test]
    fn test_split_parent_name_deep() {
        let (parent, name) = split_parent_name("/a/b/c/d").unwrap();
        assert_eq!(parent, "/a/b/c");
        assert_eq!(name, "d");
    }

    #[test]
    fn test_split_parent_name_trailing_slash() {
        // Trailing slash should be stripped.
        let (parent, name) = split_parent_name("/foo/bar/").unwrap();
        assert_eq!(parent, "/foo");
        assert_eq!(name, "bar");
    }

    #[test]
    fn test_split_parent_name_no_slash() {
        // Relative paths without a slash should fail.
        assert!(matches!(
            split_parent_name("file.txt"),
            Err(KernelError::InvalidArgument)
        ));
    }

    #[test]
    fn test_split_parent_name_root_only() {
        // Just "/" has no name component.
        assert!(matches!(
            split_parent_name("/"),
            Err(KernelError::InvalidArgument)
        ));
    }

    // --- dir_type_to_entry_type tests ---

    #[test]
    fn test_dir_type_to_entry_type() {
        use super::super::ondisk::dir_type;
        assert_eq!(dir_type_to_entry_type(dir_type::DIR), EntryType::Directory);
        assert_eq!(dir_type_to_entry_type(dir_type::REG_FILE), EntryType::File);
        assert_eq!(dir_type_to_entry_type(dir_type::SYMLINK), EntryType::Symlink);
        // Unknown types fall back to File.
        assert_eq!(dir_type_to_entry_type(dir_type::CHRDEV), EntryType::File);
        assert_eq!(dir_type_to_entry_type(dir_type::SOCK), EntryType::File);
        assert_eq!(dir_type_to_entry_type(dir_type::UNKNOWN), EntryType::File);
    }

    // --- mode_to_entry_type tests ---

    #[test]
    fn test_mode_to_entry_type() {
        assert_eq!(mode_to_entry_type(file_type::S_IFDIR), EntryType::Directory);
        assert_eq!(mode_to_entry_type(file_type::S_IFREG), EntryType::File);
        assert_eq!(mode_to_entry_type(file_type::S_IFLNK), EntryType::Symlink);
        // Unknown modes fall back to File.
        assert_eq!(mode_to_entry_type(file_type::S_IFBLK), EntryType::File);
        assert_eq!(mode_to_entry_type(file_type::S_IFIFO), EntryType::File);
    }

    // --- inode_file_size tests ---

    #[test]
    fn test_inode_file_size_regular_file() {
        let mut inode: super::super::ondisk::Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = file_type::S_IFREG | 0o644;
        inode.i_size_lo = 0x1234_5678;
        inode.i_size_high = 0x0000_0001;

        // Regular file uses the high bits.
        assert_eq!(inode_file_size(&inode), 0x0000_0001_1234_5678);
    }

    #[test]
    fn test_inode_file_size_directory() {
        let mut inode: super::super::ondisk::Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = file_type::S_IFDIR | 0o755;
        inode.i_size_lo = 4096;
        inode.i_size_high = 0xDEAD; // Should be ignored for directories.

        assert_eq!(inode_file_size(&inode), 4096);
    }

    #[test]
    fn test_inode_file_size_zero() {
        let inode: super::super::ondisk::Ext4Inode = unsafe { core::mem::zeroed() };
        assert_eq!(inode_file_size(&inode), 0);
    }

    // --- write_dot_entry / write_dotdot_entry tests ---

    #[test]
    fn test_write_dot_entry() {
        let mut buf = [0u8; 32];
        write_dot_entry(&mut buf, 0, 42, 12);

        // inode = 42 (LE)
        assert_eq!(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]), 42);
        // rec_len = 12
        assert_eq!(u16::from_le_bytes([buf[4], buf[5]]), 12);
        // name_len = 1
        assert_eq!(buf[6], 1);
        // file_type = DIR (2)
        assert_eq!(buf[7], super::super::ondisk::dir_type::DIR);
        // name = "."
        assert_eq!(buf[8], b'.');
    }

    #[test]
    fn test_write_dotdot_entry() {
        let mut buf = [0u8; 32];
        write_dotdot_entry(&mut buf, 0, 99, 1012);

        assert_eq!(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]), 99);
        assert_eq!(u16::from_le_bytes([buf[4], buf[5]]), 1012);
        assert_eq!(buf[6], 2); // name_len
        assert_eq!(buf[7], super::super::ondisk::dir_type::DIR);
        assert_eq!(buf[8], b'.');
        assert_eq!(buf[9], b'.');
    }
}
