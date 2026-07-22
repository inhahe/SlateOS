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

    /// Build rich [`FileMeta`] from an already-resolved inode number.
    ///
    /// Shared by [`metadata`](Self::metadata) (follow) and
    /// [`lmetadata`](Self::lmetadata) (no-follow); the only difference
    /// between them is which resolver produced `ino`.
    fn meta_from_ino(&mut self, ino: u32) -> KernelResult<FileMeta> {
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

        // ext4 extra inode fields provide nanosecond precision and epoch
        // extension bits.  Layout of each *_extra field (u32, LE):
        //   bits [31:2] = nanoseconds (0..999_999_999)
        //   bits [1:0]  = epoch extension (adds 0..3 × 2^32 seconds)
        //
        // Full timestamp = base_seconds + epoch_ext * 2^32, nanoseconds from upper 30 bits.
        //
        // Offsets in raw inode bytes:
        //   0x84 = i_ctime_extra    0x88 = i_mtime_extra
        //   0x8C = i_atime_extra    0x90 = i_crtime (base secs)
        //   0x94 = i_crtime_extra
        let raw_inode = if self.driver.ondisk_inode_size() > 128 {
            self.driver.read_inode_raw(ino).ok()
        } else {
            None
        };

        // Combine base seconds + extra field → nanoseconds since epoch.
        let combine_ts = |base_secs: u32, extra_offset: usize| -> u64 {
            let extra = raw_inode.as_ref()
                .and_then(|raw| raw.get(extra_offset..extra_offset.wrapping_add(4)))
                .and_then(|s| <[u8; 4]>::try_from(s).ok())
                .map_or(0u32, u32::from_le_bytes);
            // Epoch extension: lower 2 bits extend the 32-bit seconds.
            let epoch_ext = u64::from(extra & 3);
            let total_secs = u64::from(base_secs).saturating_add(epoch_ext.wrapping_shl(32));
            let ns = u64::from(extra >> 2);
            total_secs.saturating_mul(1_000_000_000).saturating_add(ns)
        };

        // Simple second→nanosecond fallback for when no extra fields exist.
        let sec_to_ns = |s: u32| u64::from(s).saturating_mul(1_000_000_000);

        // Creation time: base at 0x90, extra at 0x94.
        let created_ns = raw_inode.as_ref()
            .and_then(|raw| raw.get(0x90..0x94))
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
            .map_or(0u32, u32::from_le_bytes);
        let created_ns = if created_ns > 0 {
            combine_ts(created_ns, 0x94)
        } else {
            0
        };

        // For mtime/atime/ctime, use extra fields if raw bytes available.
        let modified_ns = if raw_inode.is_some() {
            combine_ts(inode.i_mtime, 0x88)
        } else {
            sec_to_ns(inode.i_mtime)
        };
        let accessed_ns = if raw_inode.is_some() {
            combine_ts(inode.i_atime, 0x8C)
        } else {
            sec_to_ns(inode.i_atime)
        };
        let changed_ns = if raw_inode.is_some() {
            combine_ts(inode.i_ctime, 0x84)
        } else {
            sec_to_ns(inode.i_ctime)
        };

        Ok(FileMeta {
            size,
            entry_type,
            ino: u64::from(ino),
            created_ns,
            modified_ns,
            accessed_ns,
            changed_ns,
            uid: inode_uid_32(&inode),
            gid: inode_gid_32(&inode),
            permissions,
            attributes: attrs,
            nlinks: u32::from(inode.i_links_count),
            blocks: inode_block_sectors(&inode, self.driver.superblock().block_size),
            xattrs: self.driver.read_all_xattrs(ino, &inode).unwrap_or_default(),
            hash: Vec::new(),
        })
    }
}

impl FileSystem for Ext4Fs {
    fn fs_type(&self) -> &'static str {
        "ext4"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        // Verify it's a directory.
        if (inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        let raw_entries = self.driver.read_dir_entries(ino, &inode)?;

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

    fn readdir_at(
        &mut self,
        path: &str,
        offset: usize,
        count: usize,
    ) -> KernelResult<(Vec<DirEntry>, usize)> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        if (inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Read raw directory entries (cheap — just parses names/types, no inode reads).
        let raw_entries = self.driver.read_dir_entries(ino, &inode)?;

        // Filter . and .. and count total.
        let filtered: Vec<_> = raw_entries
            .into_iter()
            .filter(|(_, _, name)| name != "." && name != "..")
            .collect();
        let total = filtered.len();

        // Only read child inodes for the entries in the requested window.
        // This is the key optimization: for a 10,000-entry directory with
        // offset=20, count=20, we only read 20 inodes instead of 10,000.
        let start = offset.min(total);
        let end = start.saturating_add(count).min(total);

        let page = filtered
            .into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|(child_ino, ftype, name)| {
                let entry_type = dir_type_to_entry_type(ftype);
                let size = self.driver.read_inode(child_ino)
                    .map(|ci| inode_file_size(&ci))
                    .unwrap_or(0);
                DirEntry { name, entry_type, size }
            })
            .collect();

        Ok((page, total))
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode = inode.i_mode & file_type::S_IFMT;
        match mode {
            file_type::S_IFREG => {
                self.driver.read_file_data(ino, &inode)
            }
            file_type::S_IFLNK => {
                // For symlinks, the target path is stored:
                // - In i_block if the target is <= 60 bytes (fast symlink)
                // - In data blocks otherwise
                self.read_symlink_target(ino, &inode)
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
                // Invalidate cached extent mappings before rebuilding
                // the extent tree — old ranges become stale.
                self.driver.invalidate_extent_cache(ino);
                self.driver.write_file_data(&mut new_inode, data)?;
                // Update mtime/ctime so `stat`/`ls -l` reflect the write.
                stamp_inode_mtime(&mut new_inode);
                self.driver.write_inode(ino, &new_inode)?;

                // Now safe to free old blocks — on-disk inode points to new data.
                // Use old_inode which still has the old extent tree.
                self.driver.free_inode_data(ino, &old_inode)?;

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
        // `unlink(2)` never dereferences a trailing symlink: it removes the
        // symlink inode itself, not its target.  Resolving with follow here
        // both corrupts the wrong inode (decrementing the *target's* link
        // count while deleting the symlink's dir entry) and fails outright
        // when the target is unreachable (e.g. a symlink whose stored target
        // is an absolute VFS path on another mount).  Resolve no-follow.
        let ino = self.driver.resolve_path_no_follow(path)?;
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
            self.driver.invalidate_extent_cache(ino);
            self.driver.free_inode_data(ino, &inode)?;
            // Free the external xattr block if present.
            self.driver.free_xattr_block(&inode)?;

            inode.i_size_lo = 0;
            inode.i_size_high = 0;
            set_inode_blocks_48(&mut inode, 0);
            inode.i_file_acl_lo = 0;
            // Clear i_file_acl_high in i_osd2[2..4].
            if let Some(b) = inode.i_osd2.get_mut(2) { *b = 0; }
            if let Some(b) = inode.i_osd2.get_mut(3) { *b = 0; }

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
        if self.driver.dir_lookup(&parent_inode, parent_ino, name).is_ok() {
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
        // Like `unlink`, `rmdir(2)` never dereferences a trailing symlink:
        // resolving with follow would let `rmdir("symlink-to-dir")` destroy the
        // *target* directory while unlinking the symlink's name.  Resolve
        // no-follow so a final symlink reads as `S_IFLNK` and is rejected with
        // `NotADirectory` (ENOTDIR), matching POSIX.
        let ino = self.driver.resolve_path_no_follow(path)?;
        let inode = self.driver.read_inode(ino)?;

        // Must be a directory.
        if (inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Check that the directory is empty (only . and ..).
        let entries = self.driver.read_dir_entries(ino, &inode)?;
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
        self.driver.invalidate_extent_cache(ino);
        self.driver.free_inode_data(ino, &inode)?;
        self.driver.free_xattr_block(&inode)?;

        // Mark directory inode as deleted.
        let mut inode = inode;
        inode.i_links_count = 0;
        inode.i_size_lo = 0;
        inode.i_size_high = 0;
        set_inode_blocks_48(&mut inode, 0);
        inode.i_file_acl_lo = 0;
        if let Some(b) = inode.i_osd2.get_mut(2) { *b = 0; }
        if let Some(b) = inode.i_osd2.get_mut(3) { *b = 0; }
        self.driver.write_inode(ino, &inode)?;

        // Free the inode itself (is_directory=true to update used_dirs count).
        self.driver.free_inode_number(ino, true)?;

        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> KernelResult<()> {
        // `rename(2)` never dereferences a trailing symlink on either operand:
        // it renames the *symlink itself* (and replaces an existing symlink
        // destination, not its target).  Resolve the final components no-follow
        // — following would rename/replace the wrong inode and mis-stamp the
        // re-inserted dir entry's type byte (the `S_IFLNK` arm below).  Parent
        // directories are still resolved with follow (correct: the path *to* a
        // parent follows intermediate symlinks).
        let src_ino = self.driver.resolve_path_no_follow(from)?;
        let src_inode = self.driver.read_inode(src_ino)?;
        let src_mode = src_inode.i_mode & file_type::S_IFMT;

        // Determine the directory entry file type for re-insertion.
        let ft_byte = match src_mode {
            file_type::S_IFDIR => super::ondisk::dir_type::DIR,
            file_type::S_IFREG => super::ondisk::dir_type::REG_FILE,
            file_type::S_IFLNK => super::ondisk::dir_type::SYMLINK,
            _ => super::ondisk::dir_type::UNKNOWN,
        };

        // POSIX rename semantics: if the destination already exists and is a
        // regular file, replace it.  If it's a directory, return error.
        if let Ok(dest_ino) = self.driver.resolve_path_no_follow(to) {
            let dest_inode = self.driver.read_inode(dest_ino)?;
            let dest_mode = dest_inode.i_mode & file_type::S_IFMT;
            if dest_mode == file_type::S_IFDIR {
                return Err(KernelError::IsADirectory);
            }
            // Remove the existing destination's dir entry from its parent.
            let (dp_path, dp_name) = split_parent_name(to)?;
            let dp_ino = self.driver.resolve_path(dp_path)?;
            let mut dp_inode = self.driver.read_inode(dp_ino)?;
            self.remove_dir_entry(&mut dp_inode, dp_ino, dp_name)?;
            // Decrement link count; fully free inode if orphaned (mirrors
            // the remove() path).
            let mut dest_inode = self.driver.read_inode(dest_ino)?;
            dest_inode.i_links_count = dest_inode.i_links_count.saturating_sub(1);
            if dest_inode.i_links_count == 0 {
                self.driver.invalidate_extent_cache(dest_ino);
                self.driver.free_inode_data(dest_ino, &dest_inode)?;
                self.driver.free_xattr_block(&dest_inode)?;
                dest_inode.i_size_lo = 0;
                dest_inode.i_size_high = 0;
                set_inode_blocks_48(&mut dest_inode, 0);
                dest_inode.i_file_acl_lo = 0;
                if let Some(b) = dest_inode.i_osd2.get_mut(2) { *b = 0; }
                if let Some(b) = dest_inode.i_osd2.get_mut(3) { *b = 0; }
                self.driver.write_inode(dest_ino, &dest_inode)?;
                self.driver.free_inode_number(dest_ino, false)?;
            } else {
                self.driver.write_inode(dest_ino, &dest_inode)?;
            }
            self.driver.write_superblock()?;
            self.driver.write_group_descs()?;
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
            let mut dir_data = self.driver.read_file_data(src_ino, &src_inode)?;
            // ".." is the second entry (at offset 12 after the "." entry).
            // Its inode field is at bytes 12..16.
            if let Some(dest) = dir_data.get_mut(12..16) {
                dest.copy_from_slice(&dst_parent_ino.to_le_bytes());
            }

            // Stamp directory block checksums after modifying ".." entry.
            super::driver::stamp_dir_data_checksums(
                self.driver.superblock(),
                src_ino,
                src_inode.i_generation,
                &mut dir_data,
            );

            let mut dir_inode_copy = src_inode;
            // Write modified data to existing blocks — only the ".."
            // entry changed, no size change, so no reallocation needed.
            match self.driver.write_to_existing_blocks(src_ino, &dir_inode_copy, &dir_data) {
                Ok(()) => {},
                Err(KernelError::NotSupported) => {
                    // Deep extent tree — fall back to full rewrite.
                    let old_inode = dir_inode_copy;
                    self.driver.invalidate_extent_cache(src_ino);
                    self.driver.write_file_data(&mut dir_inode_copy, &dir_data)?;
                    self.driver.free_inode_data(src_ino, &old_inode)?;
                },
                Err(e) => return Err(e),
            }

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
        self.driver.read_file_range(ino, &inode, offset, len)
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
            self.driver.write_at_inplace(ino, &inode, offset, data)?;
            // Block data changed in place but the inode itself was untouched;
            // still bump mtime/ctime so the modification is observable.
            if !data.is_empty() {
                let mut new_inode = inode;
                stamp_inode_mtime(&mut new_inode);
                self.driver.write_inode(ino, &new_inode)?;
            }
            self.driver.flush()?;
            Ok(())
        } else if offset == file_size {
            // Append at EOF — try the efficient extend path.
            // This avoids reading/rewriting the entire file for the
            // common case of growing a log file, database, etc.
            let mut new_inode = inode;
            match self.driver.extend_file_data(ino, &mut new_inode, data) {
                Ok(()) => {
                    stamp_inode_mtime(&mut new_inode);
                    self.driver.write_inode(ino, &new_inode)?;
                    self.driver.invalidate_extent_cache(ino);
                    self.driver.write_superblock()?;
                    self.driver.write_group_descs()?;
                    self.driver.flush()?;
                    Ok(())
                }
                Err(KernelError::NotSupported) => {
                    // Deep extent tree or extent entries full — fall back
                    // to read-modify-write.
                    let mut contents = self.driver.read_file_data(ino, &inode)?;
                    contents.extend_from_slice(data);
                    self.write_file(path, &contents)
                }
                Err(e) => Err(e),
            }
        } else if offset < file_size {
            // Write starts within the file but extends past EOF.
            // Optimization: write the in-bounds portion in place, then
            // append the remainder using extend_file_data.  This avoids
            // reading the entire file for the common case of overwriting
            // the tail + appending new data.
            let in_bounds_len = file_size.saturating_sub(offset) as usize;
            let in_bounds = data.get(..in_bounds_len).unwrap_or(data);
            let past_eof = data.get(in_bounds_len..).unwrap_or(&[]);

            // Step 1: write the in-bounds portion in place.
            if !in_bounds.is_empty() {
                self.driver.write_at_inplace(ino, &inode, offset, in_bounds)?;
            }

            // Step 2: append the past-EOF portion.
            if !past_eof.is_empty() {
                // Re-read inode (write_at_inplace doesn't change it, but
                // extend_file_data needs the current state).
                let mut new_inode = self.driver.read_inode(ino)?;
                match self.driver.extend_file_data(ino, &mut new_inode, past_eof) {
                    Ok(()) => {
                        stamp_inode_mtime(&mut new_inode);
                        self.driver.write_inode(ino, &new_inode)?;
                        self.driver.invalidate_extent_cache(ino);
                        self.driver.write_superblock()?;
                        self.driver.write_group_descs()?;
                    }
                    Err(KernelError::NotSupported) => {
                        // Fall back to full read-modify-write.
                        let mut contents = self.driver.read_file_data(ino, &inode)?;
                        let start = offset as usize;
                        let end_usize = end as usize;
                        if end_usize > contents.len() {
                            contents.resize(end_usize, 0);
                        }
                        if let Some(dest) = contents.get_mut(
                            start..start.saturating_add(data.len())
                        ) {
                            dest.copy_from_slice(data);
                        }
                        return self.write_file(path, &contents);
                    }
                    Err(e) => return Err(e),
                }
            }

            self.driver.flush()?;
            Ok(())
        } else {
            // Write starts past EOF — zero-fill gap then write.
            // This is an unusual case; fall back to read-modify-write.
            let mut contents = self.driver.read_file_data(ino, &inode)?;
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

    fn fallocate(&mut self, path: &str, size: u64) -> KernelResult<()> {
        if size == 0 {
            return Ok(());
        }

        let ino = self.driver.resolve_path(path)?;
        let inode = self.driver.read_inode(ino)?;

        let mode = inode.i_mode & file_type::S_IFMT;
        if mode != file_type::S_IFREG {
            return Err(KernelError::NotSupported);
        }

        let current_size = inode_file_size(&inode);
        let block_size = u64::from(self.driver.superblock().block_size);
        if block_size == 0 {
            return Err(KernelError::IoError);
        }

        // Calculate blocks currently allocated vs needed.
        let current_blocks = current_size
            .saturating_add(block_size.saturating_sub(1)) / block_size;
        let needed_blocks = size
            .saturating_add(block_size.saturating_sub(1)) / block_size;

        if needed_blocks <= current_blocks {
            // Already have enough allocated blocks.
            return Ok(());
        }

        // For non-empty files, append an UNWRITTEN extent covering the
        // new blocks.  This requires a depth-0 extent tree with room for
        // one more entry.  If that fails (depth>0 or full), silently
        // succeed — blocks will be allocated on demand when data is written.
        if current_size != 0 {
            let extra_blocks = needed_blocks.saturating_sub(current_blocks);
            if extra_blocks == 0 || extra_blocks > 0x7FFF_u64 {
                return Ok(());
            }

            let mut new_inode = inode;

            // Goal: allocate adjacent to last existing extent for contiguity.
            // Parse the extent tree to find the last physical block.
            let last_extent_end = self.driver.last_extent_end(&new_inode)
                .unwrap_or(u64::from(self.driver.superblock().raw.s_first_data_block));

            #[allow(clippy::cast_possible_truncation)]
            let extra_u16 = extra_blocks as u16;
            #[allow(clippy::cast_possible_truncation)]
            let logical_start = current_blocks as u32;

            match self.driver.append_unwritten_extent(
                &mut new_inode,
                logical_start,
                extra_u16,
                last_extent_end,
            ) {
                Ok(_first_block) => {
                    // Update block count (not file size — that's the point
                    // of fallocate).
                    let total_sectors = needed_blocks
                        .saturating_mul(u64::from(self.driver.superblock().block_size / 512));
                    set_inode_blocks_48(&mut new_inode, total_sectors);

                    stamp_inode_mtime(&mut new_inode);
                    self.driver.write_inode(ino, &new_inode)?;
                    self.driver.invalidate_extent_cache(ino);
                    self.driver.write_superblock()?;
                    self.driver.write_group_descs()?;
                    self.driver.flush()?;
                }
                Err(KernelError::NotSupported) => {
                    // Can't add extent (tree too deep or full) — silently
                    // succeed; blocks will be allocated on write.
                }
                Err(e) => return Err(e),
            }

            return Ok(());
        }

        // Allocate contiguous blocks via the driver (avoids split borrows).
        let blocks_to_alloc = needed_blocks.min(0x7FFF_u64);

        #[allow(clippy::cast_possible_truncation)]
        let blocks_u32 = blocks_to_alloc as u32;
        let goal = u64::from(self.driver.superblock().raw.s_first_data_block);

        let first_block = self.driver.fallocate_blocks(goal, blocks_u32)?;

        // Set up the extent tree with an UNWRITTEN extent.
        // Unwritten extents have bit 15 set in ee_len, causing reads to
        // return zeros instead of reading actual block data.
        let mut new_inode = inode;
        self.driver.init_extent_header_pub(&mut new_inode, 1);

        // Set extent with UNWRITTEN flag (0x8000 | block_count).
        #[allow(clippy::cast_possible_truncation)]
        let block_count_u16 = blocks_to_alloc as u16;
        self.driver.set_single_extent_unwritten(
            &mut new_inode,
            0,
            first_block,
            block_count_u16,
        );

        // Update block count (in 512-byte sectors) but NOT file size.
        // File size stays 0 — reads past logical EOF return zeros.
        let sectors = u64::from(blocks_u32)
            .saturating_mul(u64::from(self.driver.superblock().block_size / 512));
        set_inode_blocks_48(&mut new_inode, sectors);

        stamp_inode_mtime(&mut new_inode);
        self.driver.write_inode(ino, &new_inode)?;
        self.driver.invalidate_extent_cache(ino);

        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
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
            set_inode_blocks_48(&mut new_inode, 0);

            // Initialize an empty extent header.
            self.driver.init_extent_header_pub(&mut new_inode, 0);

            stamp_inode_mtime(&mut new_inode);
            // Write the new inode first (crash-safe: inode points to
            // nothing, so the old blocks are just leaked on crash).
            self.driver.write_inode(ino, &new_inode)?;

            // Now free the old blocks.
            self.driver.free_inode_data(ino, &old_inode)?;

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
            let mut data = self.driver.read_file_data(ino, &inode)?;
            data.truncate(size as usize);

            let mut new_inode = inode;
            self.driver.invalidate_extent_cache(ino);
            self.driver.write_file_data(&mut new_inode, &data)?;
            stamp_inode_mtime(&mut new_inode);
            self.driver.write_inode(ino, &new_inode)?;

            // Free old blocks now that inode points to new data.
            self.driver.free_inode_data(ino, &old_inode)?;

            self.driver.write_superblock()?;
            self.driver.write_group_descs()?;
            self.driver.flush()?;
            Ok(())
        } else {
            // Extend: read, resize with zeros, rewrite.
            // Growing in place would require extending the extent tree,
            // which uses the same write_file_data path anyway.
            let mut data = self.driver.read_file_data(ino, &inode)?;
            let old_inode = inode;

            data.resize(size as usize, 0);

            let mut new_inode = inode;
            self.driver.invalidate_extent_cache(ino);
            self.driver.write_file_data(&mut new_inode, &data)?;
            stamp_inode_mtime(&mut new_inode);
            self.driver.write_inode(ino, &new_inode)?;

            self.driver.free_inode_data(ino, &old_inode)?;

            self.driver.write_superblock()?;
            self.driver.write_group_descs()?;
            self.driver.flush()?;
            Ok(())
        }
    }

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        // Follows the trailing symlink (resolve_path).
        let ino = self.driver.resolve_path(path)?;
        self.meta_from_ino(ino)
    }

    fn lmetadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        // No-follow: resolve_path_no_follow follows intermediate
        // symlinks but stops at the final component, so a trailing
        // symlink reports its own inode rather than the target's.
        let ino = self.driver.resolve_path_no_follow(path)?;
        self.meta_from_ino(ino)
    }

    fn set_permissions(&mut self, path: &str, permissions: u16) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        let mut inode = self.driver.read_inode(ino)?;

        // Preserve the file type bits, update only the permission bits.
        let type_bits = inode.i_mode & file_type::S_IFMT;
        inode.i_mode = type_bits | (permissions & 0o7777);

        // chmod advances ctime (metadata change), not mtime.
        stamp_inode_ctime(&mut inode);
        self.driver.write_inode(ino, &inode)?;
        self.driver.flush()?;
        Ok(())
    }

    fn set_owner(&mut self, path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        self.set_owner_ino(ino, uid, gid)
    }

    /// `lchown`/`fchownat(AT_SYMLINK_NOFOLLOW)`: chown the link inode itself.
    /// Resolves the final component WITHOUT following a trailing symlink.
    fn set_owner_no_follow(&mut self, path: &str, uid: u32, gid: u32) -> KernelResult<()> {
        let ino = self.driver.resolve_path_no_follow(path)?;
        self.set_owner_ino(ino, uid, gid)
    }

    fn set_times(
        &mut self,
        path: &str,
        accessed_ns: crate::fs::vfs::Timestamp,
        modified_ns: crate::fs::vfs::Timestamp,
    ) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        self.set_times_ino(ino, accessed_ns, modified_ns)
    }

    /// `lutimes`/`utimensat(AT_SYMLINK_NOFOLLOW)`: stamp the link inode itself.
    fn set_times_no_follow(
        &mut self,
        path: &str,
        accessed_ns: crate::fs::vfs::Timestamp,
        modified_ns: crate::fs::vfs::Timestamp,
    ) -> KernelResult<()> {
        let ino = self.driver.resolve_path_no_follow(path)?;
        self.set_times_ino(ino, accessed_ns, modified_ns)
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

        // Attribute changes advance ctime (metadata change), not mtime.
        stamp_inode_ctime(&mut inode);
        self.driver.write_inode(ino, &inode)?;
        self.driver.flush()?;
        Ok(())
    }

    fn get_xattr(&mut self, path: &str, key: &str) -> KernelResult<Vec<u8>> {
        let ino = self.driver.resolve_path(path)?;
        self.get_xattr_ino(ino, key)
    }

    fn set_xattr(&mut self, path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        self.set_xattr_ino(ino, key, value)
    }

    fn remove_xattr(&mut self, path: &str, key: &str) -> KernelResult<()> {
        let ino = self.driver.resolve_path(path)?;
        self.remove_xattr_ino(ino, key)
    }

    fn list_xattrs(&mut self, path: &str) -> KernelResult<Vec<String>> {
        let ino = self.driver.resolve_path(path)?;
        self.list_xattrs_ino(ino)
    }

    // --- No-follow xattr variants (lgetxattr/lsetxattr/lremovexattr/
    // llistxattr): resolve the final component WITHOUT following a symlink,
    // so the link inode's own xattrs are targeted. ---

    fn get_xattr_no_follow(&mut self, path: &str, key: &str) -> KernelResult<Vec<u8>> {
        let ino = self.driver.resolve_path_no_follow(path)?;
        self.get_xattr_ino(ino, key)
    }

    fn set_xattr_no_follow(&mut self, path: &str, key: &str, value: &[u8]) -> KernelResult<()> {
        let ino = self.driver.resolve_path_no_follow(path)?;
        self.set_xattr_ino(ino, key, value)
    }

    fn remove_xattr_no_follow(&mut self, path: &str, key: &str) -> KernelResult<()> {
        let ino = self.driver.resolve_path_no_follow(path)?;
        self.remove_xattr_ino(ino, key)
    }

    fn list_xattrs_no_follow(&mut self, path: &str) -> KernelResult<Vec<String>> {
        let ino = self.driver.resolve_path_no_follow(path)?;
        self.list_xattrs_ino(ino)
    }

    fn symlink(&mut self, path: &str, target: &str) -> KernelResult<()> {
        let (parent_path, name) = split_parent_name(path)?;
        let parent_ino = self.driver.resolve_path(parent_path)?;
        let mut parent_inode = self.driver.read_inode(parent_ino)?;

        if (parent_inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Check name doesn't already exist.
        if self.driver.dir_lookup(&parent_inode, parent_ino, name).is_ok() {
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

        let target_bytes = self.driver.read_symlink_target(ino, &inode)?;
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
        let mut s = self.driver.superblock().summary();

        // Directory entry cache stats.
        let (hits, misses, valid) = self.driver.dcache.stats();
        let total = hits.saturating_add(misses);
        let rate = if total > 0 {
            hits.saturating_mul(100) / total
        } else {
            0
        };
        s.push_str(&alloc::format!(
            "\ndcache: {}/{} slots, {} hits, {} misses ({}% hit rate)",
            valid, super::driver::EXT4_DCACHE_SIZE, hits, misses, rate,
        ));

        // Extent range cache stats.
        let (ehits, emisses, evalid) = self.driver.extent_cache_stats();
        let etotal = ehits.saturating_add(emisses);
        let erate = if etotal > 0 {
            ehits.saturating_mul(100) / etotal
        } else {
            0
        };
        s.push_str(&alloc::format!(
            "\nextent_cache: {}/{} slots, {} hits, {} misses ({}% hit rate)",
            evalid, super::driver::EXTENT_CACHE_SIZE, ehits, emisses, erate,
        ));

        // Inode cache stats.
        let (ihits, imisses, ivalid) = self.driver.inode_cache_stats();
        let itotal = ihits.saturating_add(imisses);
        let irate = if itotal > 0 {
            ihits.saturating_mul(100) / itotal
        } else {
            0
        };
        s.push_str(&alloc::format!(
            "\ninode_cache: {}/{} slots, {} hits, {} misses ({}% hit rate)",
            ivalid, super::driver::INODE_CACHE_SIZE, ihits, imisses, irate,
        ));

        s
    }

    /// Create a hard link to an existing file.
    ///
    /// Creates a new directory entry in `new_path`'s parent directory
    /// pointing to the same inode as `existing`.  Increments the inode's
    /// link count.
    ///
    /// Restrictions:
    /// - Cannot hard-link directories (prevents cycles in the directory tree).
    /// - The target must exist.
    /// - The new name must not already exist.
    #[allow(clippy::arithmetic_side_effects)]
    fn link(&mut self, existing: &str, new_path: &str) -> KernelResult<()> {
        // `link`/`linkat(AT_SYMLINK_FOLLOW)`: follow a trailing symlink in
        // `existing` so the hard link points at the underlying file.
        let existing_ino = self.driver.resolve_path(existing)?;
        self.link_ino_checked(existing_ino, new_path)
    }

    fn link_no_follow(&mut self, existing: &str, new_path: &str) -> KernelResult<()> {
        // Plain `link(2)` / `linkat` without AT_SYMLINK_FOLLOW: do NOT follow
        // a trailing symlink — hard-link the symlink inode itself.
        let existing_ino = self.driver.resolve_path_no_follow(existing)?;
        self.link_ino_checked(existing_ino, new_path)
    }

    /// Report ext4 filesystem capacity and free space.
    ///
    /// Reads block count and free block count from the superblock.
    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        let sb = self.driver.superblock();
        Ok(FsInfo {
            fs_type: String::from("ext4"),
            volume_label: sb.volume_name.clone(),
            block_size: u64::from(sb.block_size),
            total_blocks: sb.block_count,
            free_blocks: sb.free_block_count,
            total_inodes: u64::from(sb.raw.s_inodes_count),
            free_inodes: u64::from(sb.raw.s_free_inodes_count),
            max_name_len: 255,
            read_only: !sb.can_write,
        })
    }

    /// Flush all pending writes to the block device.
    ///
    /// Writes the superblock and flushes the block cache.
    fn sync(&mut self) -> KernelResult<()> {
        self.driver.flush()
    }
}

impl Ext4Fs {
    /// Shared body for [`link`]/[`link_no_follow`]: create a new directory
    /// entry (`new_path`) referencing an already-resolved inode.  The follow
    /// vs no-follow distinction lives entirely in how `existing_ino` was
    /// resolved by the caller; from here the two paths are identical.
    ///
    /// Regular files and symlinks may be hard-linked (a symlink inode is the
    /// no-follow `link(2)` target); directories are rejected (EISDIR), as is
    /// any other inode type (EINVAL).
    fn link_ino_checked(&mut self, existing_ino: u32, new_path: &str) -> KernelResult<()> {
        let mut inode = self.driver.read_inode(existing_ino)?;

        let mode_type = inode.i_mode & file_type::S_IFMT;
        if mode_type == file_type::S_IFDIR {
            return Err(KernelError::IsADirectory);
        }
        // Regular files and symlinks are hard-linkable; nothing else.
        if mode_type != file_type::S_IFREG && mode_type != file_type::S_IFLNK {
            return Err(KernelError::InvalidArgument);
        }

        // Resolve the parent of the new path.
        let (parent_path, name) = split_parent_name(new_path)?;
        let parent_ino = self.driver.resolve_path(parent_path)?;
        let mut parent_inode = self.driver.read_inode(parent_ino)?;

        // Verify parent is a directory.
        if (parent_inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Check that the new name doesn't already exist.
        if self.driver.dir_lookup(&parent_inode, parent_ino, name).is_ok() {
            return Err(KernelError::AlreadyExists);
        }

        // Reject if link count would overflow u16.  Adding the directory
        // entry before checking would leave a dangling hard link if we
        // refused to increment the count afterward.
        if inode.i_links_count == u16::MAX {
            return Err(KernelError::InvalidArgument);
        }

        // Determine the directory entry file type byte.
        let ftype_byte = match mode_type {
            file_type::S_IFREG => super::ondisk::dir_type::REG_FILE,
            file_type::S_IFLNK => super::ondisk::dir_type::SYMLINK,
            _ => super::ondisk::dir_type::UNKNOWN,
        };

        // Add the directory entry.
        self.driver.add_dir_entry(
            &mut parent_inode, parent_ino,
            existing_ino, name, ftype_byte,
        )?;
        self.driver.write_inode(parent_ino, &parent_inode)?;

        // Increment the link count.  Overflow already rejected above.
        inode.i_links_count = inode.i_links_count.saturating_add(1);
        self.driver.write_inode(existing_ino, &inode)?;

        Ok(())
    }

    /// Shared body for [`set_owner`]/[`set_owner_no_follow`]: chown an
    /// already-resolved inode.  Writes the full 32-bit UID/GID and advances
    /// ctime (chown is a metadata change, not a data change).
    fn set_owner_ino(&mut self, ino: u32, uid: u32, gid: u32) -> KernelResult<()> {
        let mut inode = self.driver.read_inode(ino)?;
        // Write the full 32-bit UID/GID (low 16 in i_uid/i_gid, high 16 in i_osd2).
        set_inode_uid_32(&mut inode, uid);
        set_inode_gid_32(&mut inode, gid);
        stamp_inode_ctime(&mut inode);
        self.driver.write_inode(ino, &inode)?;
        self.driver.flush()?;
        Ok(())
    }

    /// Shared body for [`set_times`]/[`set_times_no_follow`]: stamp an
    /// already-resolved inode.  Pass 0 for a timestamp to leave it unchanged.
    fn set_times_ino(
        &mut self,
        ino: u32,
        accessed_ns: crate::fs::vfs::Timestamp,
        modified_ns: crate::fs::vfs::Timestamp,
    ) -> KernelResult<()> {
        let mut inode = self.driver.read_inode(ino)?;

        // Convert nanoseconds to seconds (ext4 core inode stores seconds).
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

    /// Shared body for [`get_xattr`]/[`get_xattr_no_follow`]: read an xattr
    /// from an already-resolved inode.
    fn get_xattr_ino(&mut self, ino: u32, key: &str) -> KernelResult<Vec<u8>> {
        let inode = self.driver.read_inode(ino)?;
        // Search both inline and external xattrs.
        let attrs = self.driver.read_all_xattrs(ino, &inode)?;
        for (k, v) in &attrs {
            if k == key {
                return Ok(v.clone());
            }
        }
        Err(KernelError::NotFound)
    }

    /// Shared body for [`set_xattr`]/[`set_xattr_no_follow`]: insert/replace an
    /// xattr on an already-resolved inode.
    fn set_xattr_ino(&mut self, ino: u32, key: &str, value: &[u8]) -> KernelResult<()> {
        let mut inode = self.driver.read_inode(ino)?;

        // Read all xattrs (inline + external), then write back to external block.
        // We always write to the external block because modifying inline xattrs
        // requires careful inode body manipulation that risks corrupting the
        // extra inode fields.
        let mut attrs = self.driver.read_all_xattrs(ino, &inode)?;

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

        // Setting an xattr advances ctime (metadata change).
        stamp_inode_ctime(&mut inode);
        self.driver.write_xattr_block(&mut inode, ino, &attrs)?;
        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    /// Shared body for [`remove_xattr`]/[`remove_xattr_no_follow`].
    fn remove_xattr_ino(&mut self, ino: u32, key: &str) -> KernelResult<()> {
        let mut inode = self.driver.read_inode(ino)?;

        let mut attrs = self.driver.read_all_xattrs(ino, &inode)?;
        let original_len = attrs.len();
        attrs.retain(|(k, _)| k != key);

        if attrs.len() == original_len {
            // Key wasn't present.
            return Err(KernelError::NotFound);
        }

        // Removing an xattr advances ctime (metadata change).
        stamp_inode_ctime(&mut inode);
        self.driver.write_xattr_block(&mut inode, ino, &attrs)?;
        self.driver.write_superblock()?;
        self.driver.write_group_descs()?;
        self.driver.flush()?;
        Ok(())
    }

    /// Shared body for [`list_xattrs`]/[`list_xattrs_no_follow`].
    fn list_xattrs_ino(&mut self, ino: u32) -> KernelResult<Vec<String>> {
        let inode = self.driver.read_inode(ino)?;
        let attrs = self.driver.read_all_xattrs(ino, &inode)?;
        Ok(attrs.into_iter().map(|(k, _)| k).collect())
    }

    /// Create a new file at `path` with the given data.
    fn create_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        let (parent_path, name) = split_parent_name(path)?;
        let parent_ino = self.driver.resolve_path(parent_path)?;
        let mut parent_inode = self.driver.read_inode(parent_ino)?;

        if (parent_inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
            return Err(KernelError::NotADirectory);
        }

        // Check that name doesn't already exist.
        if self.driver.dir_lookup(&parent_inode, parent_ino, name).is_ok() {
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
        dir_ino: u32,
        name: &str,
    ) -> KernelResult<()> {
        // Invalidate the dcache entry for this name.
        self.driver.dcache.invalidate_entry(dir_ino, name);
        let mut dir_data = self.driver.read_file_data(dir_ino, dir_inode)?;
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

                        // Stamp directory block checksums before writing.
                        super::driver::stamp_dir_data_checksums(
                            self.driver.superblock(),
                            dir_ino,
                            dir_inode.i_generation,
                            &mut dir_data,
                        );

                        // Write modified data to existing blocks — only
                        // an entry was zeroed/merged, no size change.
                        match self.driver.write_to_existing_blocks(
                            dir_ino, dir_inode, &dir_data,
                        ) {
                            Ok(()) => {},
                            Err(KernelError::NotSupported) => {
                                // Deep extent tree — fall back to full rewrite.
                                let old_inode = *dir_inode;
                                let mut dir_inode_copy = *dir_inode;
                                self.driver.invalidate_extent_cache(dir_ino);
                                self.driver.write_file_data(
                                    &mut dir_inode_copy,
                                    &dir_data,
                                )?;
                                self.driver.free_inode_data(dir_ino, &old_inode)?;
                            },
                            Err(e) => return Err(e),
                        }
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
    fn read_symlink_target(&self, inode_nr: u32, inode: &super::ondisk::Ext4Inode) -> KernelResult<Vec<u8>> {
        self.driver.read_symlink_target(inode_nr, inode)
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

/// Read the full 32-bit UID from an ext4 inode.
///
/// Linux ext4 splits UIDs into `i_uid` (low 16 bits) and `i_osd2[4..6]`
/// (high 16 bits).  UIDs > 65535 are common on modern systems (container
/// environments, NFS, etc.).
fn inode_uid_32(inode: &super::ondisk::Ext4Inode) -> u32 {
    let lo = u32::from(inode.i_uid);
    let hi = u32::from(u16::from_le_bytes([
        *inode.i_osd2.get(4).unwrap_or(&0),
        *inode.i_osd2.get(5).unwrap_or(&0),
    ]));
    lo | (hi << 16)
}

/// Read the full 32-bit GID from an ext4 inode.
fn inode_gid_32(inode: &super::ondisk::Ext4Inode) -> u32 {
    let lo = u32::from(inode.i_gid);
    let hi = u32::from(u16::from_le_bytes([
        *inode.i_osd2.get(6).unwrap_or(&0),
        *inode.i_osd2.get(7).unwrap_or(&0),
    ]));
    lo | (hi << 16)
}

/// Write a 32-bit UID into an ext4 inode (low 16 in `i_uid`, high 16 in `i_osd2`).
fn set_inode_uid_32(inode: &mut super::ondisk::Ext4Inode, uid: u32) {
    inode.i_uid = uid as u16;
    let hi = (uid >> 16) as u16;
    let hi_bytes = hi.to_le_bytes();
    if let Some(slot) = inode.i_osd2.get_mut(4) { *slot = hi_bytes[0]; }
    if let Some(slot) = inode.i_osd2.get_mut(5) { *slot = hi_bytes[1]; }
}

/// Write a 32-bit GID into an ext4 inode.
fn set_inode_gid_32(inode: &mut super::ondisk::Ext4Inode, gid: u32) {
    inode.i_gid = gid as u16;
    let hi = (gid >> 16) as u16;
    let hi_bytes = hi.to_le_bytes();
    if let Some(slot) = inode.i_osd2.get_mut(6) { *slot = hi_bytes[0]; }
    if let Some(slot) = inode.i_osd2.get_mut(7) { *slot = hi_bytes[1]; }
}

/// Read the inode block count and return 512-byte sectors.
///
/// Combines `i_blocks_lo` (32 bits) with `i_osd2[0..2]` (high 16 bits)
/// for a 48-bit raw value.  If the inode has `HUGE_FILE` flag set, the
/// raw value is in filesystem block units — multiply by `block_size / 512`
/// to convert to 512-byte sectors.
///
/// This matters for files larger than ~2 TB on 4K-block filesystems (or
/// ~32 TB on our 16K pages).  The HUGE_FILE conversion handles the rare
/// case where Linux stored the count in fs-block units because the
/// 48-bit sector count would overflow.
///
/// Based on Linux `ext4_inode_blocks()` in `fs/ext4/inode.c`.
fn inode_block_sectors(inode: &super::ondisk::Ext4Inode, block_size: u32) -> u64 {
    let lo = u64::from(inode.i_blocks_lo);
    let hi = u64::from(u16::from_le_bytes([
        *inode.i_osd2.first().unwrap_or(&0),
        *inode.i_osd2.get(1).unwrap_or(&0),
    ]));
    let raw = lo | (hi << 32);

    if (inode.i_flags & super::ondisk::inode_flags::HUGE_FILE) != 0 {
        // Raw value is in filesystem blocks — convert to 512-byte sectors.
        let sectors_per_block = u64::from(block_size / 512);
        raw.saturating_mul(sectors_per_block)
    } else {
        // Raw value is already in 512-byte sectors.
        raw
    }
}

/// Write the 48-bit block count into an inode (in 512-byte sectors).
///
/// Clears the `HUGE_FILE` inode flag since we always store in sector
/// units (the 48-bit range supports up to 128 PiB, far beyond any
/// practical file size).
///
/// Based on Linux `ext4_inode_blocks_set()` in `fs/ext4/inode.c`.
fn set_inode_blocks_48(inode: &mut super::ondisk::Ext4Inode, sectors: u64) {
    inode.i_blocks_lo = sectors as u32;
    let hi = ((sectors >> 32) as u16).to_le_bytes();
    if let Some(slot) = inode.i_osd2.get_mut(0) { *slot = hi[0]; }
    if let Some(slot) = inode.i_osd2.get_mut(1) { *slot = hi[1]; }
    // Always clear HUGE_FILE since we store sectors, not fs blocks.
    inode.i_flags &= !super::ondisk::inode_flags::HUGE_FILE;
}

/// Stamp an inode's modification and change times with the current wall clock.
///
/// Call this on any data- or size-mutating operation (write, append,
/// truncate, fallocate) just before `write_inode`, so that `stat`/`ls -l`
/// reflect the change. Times are 32-bit Unix epoch seconds in the inode core
/// and are persisted (with checksum) by `write_inode`.
fn stamp_inode_mtime(inode: &mut super::ondisk::Ext4Inode) {
    let now_secs = super::driver::epoch_secs_u32();
    inode.i_mtime = now_secs;
    inode.i_ctime = now_secs;
}

/// Stamp an inode's change time (`i_ctime`) only, leaving mtime alone.
///
/// Call this on metadata-only changes — `chmod`, `chown`, attribute and
/// xattr updates — where POSIX requires the change time to advance but the
/// modification time (data) must be preserved.
fn stamp_inode_ctime(inode: &mut super::ondisk::Ext4Inode) {
    inode.i_ctime = super::driver::epoch_secs_u32();
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
// Self-test (in-kernel)
// ---------------------------------------------------------------------------

/// VFS integration layer tests — exercises pure helper functions that
/// convert between ext4 on-disk formats and VFS types.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ext4-vfs] Running self-test...");

    test_split_parent_name()?;
    test_inode_file_size()?;
    test_inode_uid_gid_32()?;
    test_dir_type_conversions()?;
    test_write_dot_entries()?;

    crate::serial_println!("[ext4-vfs] Self-test PASSED (5 tests)");
    Ok(())
}

/// Test split_parent_name for various path patterns.
fn test_split_parent_name() -> KernelResult<()> {
    // Simple path.
    let (parent, name) = split_parent_name("/foo/bar")?;
    if parent != "/foo" || name != "bar" {
        crate::serial_println!(
            "[ext4-vfs]   FAIL: split('/foo/bar') = ('{}', '{}')", parent, name
        );
        return Err(KernelError::InternalError);
    }

    // Root-level file.
    let (parent, name) = split_parent_name("/file.txt")?;
    if parent != "/" || name != "file.txt" {
        crate::serial_println!(
            "[ext4-vfs]   FAIL: split('/file.txt') = ('{}', '{}')", parent, name
        );
        return Err(KernelError::InternalError);
    }

    // Deep path.
    let (parent, name) = split_parent_name("/a/b/c/d")?;
    if parent != "/a/b/c" || name != "d" {
        crate::serial_println!("[ext4-vfs]   FAIL: split deep path");
        return Err(KernelError::InternalError);
    }

    // Trailing slash should be stripped.
    let (parent, name) = split_parent_name("/foo/bar/")?;
    if parent != "/foo" || name != "bar" {
        crate::serial_println!("[ext4-vfs]   FAIL: split trailing slash");
        return Err(KernelError::InternalError);
    }

    // No slash → error.
    if split_parent_name("file.txt").is_ok() {
        crate::serial_println!("[ext4-vfs]   FAIL: no-slash should fail");
        return Err(KernelError::InternalError);
    }

    // Root only → error.
    if split_parent_name("/").is_ok() {
        crate::serial_println!("[ext4-vfs]   FAIL: root-only should fail");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-vfs]   split_parent_name: OK");
    Ok(())
}

/// Test inode_file_size for regular files, directories, and zeroed.
fn test_inode_file_size() -> KernelResult<()> {
    // SAFETY: Ext4Inode is all integer fields.
    let mut inode: super::ondisk::Ext4Inode = unsafe { core::mem::zeroed() };

    // Regular file: uses both lo and hi.
    inode.i_mode = file_type::S_IFREG | 0o644;
    inode.i_size_lo = 0x1234_5678;
    inode.i_size_high = 0x0000_0001;
    let size = inode_file_size(&inode);
    if size != 0x0000_0001_1234_5678 {
        crate::serial_println!(
            "[ext4-vfs]   FAIL: regular file size = {:#x}", size
        );
        return Err(KernelError::InternalError);
    }

    // Directory: ignores hi field.
    inode.i_mode = file_type::S_IFDIR | 0o755;
    inode.i_size_lo = 4096;
    inode.i_size_high = 0xDEAD;
    let size = inode_file_size(&inode);
    if size != 4096 {
        crate::serial_println!(
            "[ext4-vfs]   FAIL: dir size = {} (should ignore hi)", size
        );
        return Err(KernelError::InternalError);
    }

    // Zeroed inode.
    // SAFETY: Ext4Inode is all integer fields; zeroed is a valid representation.
    let zero: super::ondisk::Ext4Inode = unsafe { core::mem::zeroed() };
    if inode_file_size(&zero) != 0 {
        crate::serial_println!("[ext4-vfs]   FAIL: zero inode size != 0");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-vfs]   inode_file_size: OK");
    Ok(())
}

/// Test 32-bit UID/GID read/write (split across i_uid/i_gid and i_osd2).
fn test_inode_uid_gid_32() -> KernelResult<()> {
    // SAFETY: Ext4Inode is all integer fields; zeroed is a valid representation.
    let mut inode: super::ondisk::Ext4Inode = unsafe { core::mem::zeroed() };

    // Set UID = 70000 (0x0001_1170) — exceeds 16-bit range.
    set_inode_uid_32(&mut inode, 70000);
    let uid = inode_uid_32(&inode);
    if uid != 70000 {
        crate::serial_println!(
            "[ext4-vfs]   FAIL: UID readback = {}, expected 70000", uid
        );
        return Err(KernelError::InternalError);
    }

    // Verify lo/hi split: lo = 70000 & 0xFFFF = 0x1170, hi = 1.
    if inode.i_uid != 0x1170 {
        crate::serial_println!(
            "[ext4-vfs]   FAIL: i_uid = {:#x}, expected 0x1170", inode.i_uid
        );
        return Err(KernelError::InternalError);
    }

    // Set GID = 100000 (0x0001_86A0).
    set_inode_gid_32(&mut inode, 100000);
    let gid = inode_gid_32(&inode);
    if gid != 100000 {
        crate::serial_println!(
            "[ext4-vfs]   FAIL: GID readback = {}, expected 100000", gid
        );
        return Err(KernelError::InternalError);
    }

    // 16-bit range: UID = 1000. Hi should be 0.
    set_inode_uid_32(&mut inode, 1000);
    if inode_uid_32(&inode) != 1000 {
        crate::serial_println!("[ext4-vfs]   FAIL: 16-bit UID");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-vfs]   inode uid/gid 32-bit: OK");
    Ok(())
}

/// Test dir_type_to_entry_type and mode_to_entry_type conversions.
fn test_dir_type_conversions() -> KernelResult<()> {
    use super::ondisk::dir_type;
    use crate::fs::vfs::EntryType;

    if dir_type_to_entry_type(dir_type::DIR) != EntryType::Directory {
        crate::serial_println!("[ext4-vfs]   FAIL: DIR type");
        return Err(KernelError::InternalError);
    }
    if dir_type_to_entry_type(dir_type::REG_FILE) != EntryType::File {
        crate::serial_println!("[ext4-vfs]   FAIL: REG_FILE type");
        return Err(KernelError::InternalError);
    }
    if dir_type_to_entry_type(dir_type::SYMLINK) != EntryType::Symlink {
        crate::serial_println!("[ext4-vfs]   FAIL: SYMLINK type");
        return Err(KernelError::InternalError);
    }
    // Unknown types fall back to File.
    if dir_type_to_entry_type(dir_type::CHRDEV) != EntryType::File {
        crate::serial_println!("[ext4-vfs]   FAIL: CHRDEV fallback");
        return Err(KernelError::InternalError);
    }

    // mode_to_entry_type.
    if mode_to_entry_type(file_type::S_IFDIR) != EntryType::Directory {
        crate::serial_println!("[ext4-vfs]   FAIL: mode DIR");
        return Err(KernelError::InternalError);
    }
    if mode_to_entry_type(file_type::S_IFREG) != EntryType::File {
        crate::serial_println!("[ext4-vfs]   FAIL: mode REG");
        return Err(KernelError::InternalError);
    }
    if mode_to_entry_type(file_type::S_IFLNK) != EntryType::Symlink {
        crate::serial_println!("[ext4-vfs]   FAIL: mode LNK");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-vfs]   dir type conversions: OK");
    Ok(())
}

/// Test write_dot_entry and write_dotdot_entry encoding.
fn test_write_dot_entries() -> KernelResult<()> {
    let mut buf = [0u8; 32];

    // Write "." entry.
    write_dot_entry(&mut buf, 0, 42, 12);
    let ino = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let rec_len = u16::from_le_bytes([buf[4], buf[5]]);
    if ino != 42 || rec_len != 12 || buf[6] != 1 || buf[8] != b'.' {
        crate::serial_println!("[ext4-vfs]   FAIL: dot entry encoding");
        return Err(KernelError::InternalError);
    }
    if buf[7] != super::ondisk::dir_type::DIR {
        crate::serial_println!("[ext4-vfs]   FAIL: dot entry file_type");
        return Err(KernelError::InternalError);
    }

    // Write ".." entry.
    let mut buf2 = [0u8; 32];
    write_dotdot_entry(&mut buf2, 0, 99, 1012);
    let ino = u32::from_le_bytes([buf2[0], buf2[1], buf2[2], buf2[3]]);
    let rec_len = u16::from_le_bytes([buf2[4], buf2[5]]);
    if ino != 99 || rec_len != 1012 || buf2[6] != 2 {
        crate::serial_println!("[ext4-vfs]   FAIL: dotdot entry encoding");
        return Err(KernelError::InternalError);
    }
    if buf2[8] != b'.' || buf2[9] != b'.' {
        crate::serial_println!("[ext4-vfs]   FAIL: dotdot name bytes");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-vfs]   write dot entries: OK");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests (#[cfg(test)] — only runs with `cargo test`, not in kernel)
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
        // SAFETY: Ext4Inode is all integer fields; zeroed is valid.
        let mut inode: super::super::ondisk::Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = file_type::S_IFREG | 0o644;
        inode.i_size_lo = 0x1234_5678;
        inode.i_size_high = 0x0000_0001;

        // Regular file uses the high bits.
        assert_eq!(inode_file_size(&inode), 0x0000_0001_1234_5678);
    }

    #[test]
    fn test_inode_file_size_directory() {
        // SAFETY: Ext4Inode is all integer fields; zeroed is valid.
        let mut inode: super::super::ondisk::Ext4Inode = unsafe { core::mem::zeroed() };
        inode.i_mode = file_type::S_IFDIR | 0o755;
        inode.i_size_lo = 4096;
        inode.i_size_high = 0xDEAD; // Should be ignored for directories.

        assert_eq!(inode_file_size(&inode), 4096);
    }

    #[test]
    fn test_inode_file_size_zero() {
        // SAFETY: Ext4Inode is all integer fields; zeroed is valid.
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
