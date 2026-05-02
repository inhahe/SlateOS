//! ext4 filesystem driver — core read logic.
//!
//! Ties together the superblock parser, block I/O, block group descriptor
//! reading, and inode lookup.  This is the main entry point for mounting
//! and reading an ext4 filesystem.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::io::BlockReader;
use super::ondisk::{
    Ext4DirEntry2, Ext4ExtentHeader, Ext4Extent, Ext4GroupDesc, Ext4Inode,
    EXT4_EXTENT_MAGIC, EXT4_ROOT_INO,
    file_type, inode_flags,
};
use super::superblock::{self, ParsedSuperblock};

// ---------------------------------------------------------------------------
// Directory entry cache (ext4-level dcache)
// ---------------------------------------------------------------------------

/// Number of entries in the ext4 directory entry cache.
///
/// Caches `(dir_inode, name) → child_inode` to avoid linear directory
/// scans on repeated lookups.  512 entries covers typical desktop
/// working sets (open project with dozens of files, navigating dirs).
pub(super) const EXT4_DCACHE_SIZE: usize = 512;

/// A single directory entry cache entry.
struct Ext4DcacheEntry {
    /// Directory inode number (key part 1).
    dir_ino: u32,
    /// Child name within the directory (key part 2).
    name: String,
    /// Resolved child inode number (cached result).
    child_ino: u32,
    /// File type byte from the directory entry.
    file_type: u8,
    /// LRU access counter.
    last_access: u64,
    /// Whether this entry is valid.
    valid: bool,
}

impl Ext4DcacheEntry {
    const fn empty() -> Self {
        Self {
            dir_ino: 0,
            name: String::new(),
            child_ino: 0,
            file_type: 0,
            last_access: 0,
            valid: false,
        }
    }
}

/// Directory entry cache for ext4.
///
/// Avoids linear O(n) directory scans in `dir_lookup()` by caching
/// recent name→inode mappings per directory.
pub(super) struct Ext4Dcache {
    entries: Vec<Ext4DcacheEntry>,
    counter: u64,
    hits: u64,
    misses: u64,
}

impl Ext4Dcache {
    fn new() -> Self {
        let mut entries = Vec::with_capacity(EXT4_DCACHE_SIZE);
        for _ in 0..EXT4_DCACHE_SIZE {
            entries.push(Ext4DcacheEntry::empty());
        }
        Self {
            entries,
            counter: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Look up a child inode by directory inode + name.
    fn lookup(&mut self, dir_ino: u32, name: &str) -> Option<(u32, u8)> {
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.dir_ino == dir_ino && entry.name == name {
                self.counter = self.counter.wrapping_add(1);
                entry.last_access = self.counter;
                self.hits = self.hits.wrapping_add(1);
                return Some((entry.child_ino, entry.file_type));
            }
        }
        self.misses = self.misses.wrapping_add(1);
        None
    }

    /// Insert a name→inode mapping.
    fn insert(&mut self, dir_ino: u32, name: &str, child_ino: u32, file_type: u8) {
        self.counter = self.counter.wrapping_add(1);

        // Check for existing entry (update in place).
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.dir_ino == dir_ino && entry.name == name {
                entry.child_ino = child_ino;
                entry.file_type = file_type;
                entry.last_access = self.counter;
                return;
            }
        }

        // Find empty slot.
        for entry in self.entries.iter_mut() {
            if !entry.valid {
                entry.dir_ino = dir_ino;
                entry.name = String::from(name);
                entry.child_ino = child_ino;
                entry.file_type = file_type;
                entry.last_access = self.counter;
                entry.valid = true;
                return;
            }
        }

        // Evict LRU.
        let mut lru_idx = 0;
        let mut lru_access = u64::MAX;
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.last_access < lru_access {
                lru_access = entry.last_access;
                lru_idx = i;
            }
        }

        let e = &mut self.entries[lru_idx];
        e.dir_ino = dir_ino;
        e.name.clear();
        e.name.push_str(name);
        e.child_ino = child_ino;
        e.file_type = file_type;
        e.last_access = self.counter;
        e.valid = true;
    }

    /// Invalidate all entries for a specific directory.
    ///
    /// Used when a directory's on-disk data changes in a way that could
    /// affect multiple entries (e.g., directory compaction, crash recovery).
    #[allow(dead_code)]
    fn invalidate_dir(&mut self, dir_ino: u32) {
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.dir_ino == dir_ino {
                entry.valid = false;
            }
        }
    }

    /// Invalidate a specific entry.
    pub(super) fn invalidate_entry(&mut self, dir_ino: u32, name: &str) {
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.dir_ino == dir_ino && entry.name == name {
                entry.valid = false;
                return;
            }
        }
    }

    /// Return (hits, misses, valid_count).
    pub(super) fn stats(&self) -> (u64, u64, usize) {
        let valid = self.entries.iter().filter(|e| e.valid).count();
        (self.hits, self.misses, valid)
    }
}

// ---------------------------------------------------------------------------
// Ext4 driver
// ---------------------------------------------------------------------------

/// An ext4 filesystem instance.
///
/// Holds the parsed superblock, block reader, cached block group
/// descriptor table, and directory entry cache.
pub struct Ext4Driver {
    /// Parsed superblock with derived values.
    sb: ParsedSuperblock,
    /// Block I/O layer.
    reader: BlockReader,
    /// Cached block group descriptor table.
    group_descs: Vec<Ext4GroupDesc>,
    /// Directory entry cache for fast name→inode lookups.
    pub(super) dcache: Ext4Dcache,
}

impl Ext4Driver {
    /// Open an ext4 filesystem on the given device.
    ///
    /// Reads and validates the superblock, then loads the block group
    /// descriptor table.
    pub fn open(device: &str) -> KernelResult<Self> {
        // Step 1: Read the raw superblock (1024 bytes at byte offset 1024).
        //
        // We use a temporary reader with a conservative 512-byte "block size"
        // just to read the superblock bytes.  After parsing, we create the
        // real reader with the correct ext4 block size.
        let temp_reader = BlockReader::new(device, 512)?;
        let sb_bytes = temp_reader.read_bytes(
            superblock::superblock_device_offset(),
            1024,
        )?;

        // Step 2: Parse and validate the superblock.
        let sb = superblock::parse(&sb_bytes)?;

        serial_println!("[ext4] {}", sb.summary());

        // Step 3: Create the real block reader with the correct block size.
        let reader = BlockReader::new(device, sb.block_size)?;

        // Step 4: Read the block group descriptor table.
        let group_descs = read_group_descs(&sb, &reader)?;

        serial_println!(
            "[ext4] Loaded {} block group descriptors",
            group_descs.len()
        );

        Ok(Self {
            sb,
            reader,
            group_descs,
            dcache: Ext4Dcache::new(),
        })
    }

    /// Access the parsed superblock.
    #[must_use]
    pub fn superblock(&self) -> &ParsedSuperblock {
        &self.sb
    }

    /// Read an inode by number.
    ///
    /// Inode numbers are 1-based (inode 0 is invalid, inode 2 is root).
    pub fn read_inode(&self, inode_nr: u32) -> KernelResult<Ext4Inode> {
        if inode_nr == 0 {
            return Err(KernelError::InvalidArgument);
        }

        let group = self.sb.inode_group(inode_nr);
        let index = self.sb.inode_index_in_group(inode_nr);

        // Get the inode table block for this group.
        let gd = self.group_descs.get(group as usize)
            .ok_or(KernelError::InvalidArgument)?;

        let inode_table_block = if self.sb.is_64bit {
            u64::from(gd.bg_inode_table_lo)
                | (u64::from(gd.bg_inode_table_hi) << 32)
        } else {
            u64::from(gd.bg_inode_table_lo)
        };

        // Calculate the byte offset of this inode on disk.
        let inode_byte_offset = inode_table_block
            .saturating_mul(u64::from(self.sb.block_size))
            .saturating_add(
                u64::from(index).saturating_mul(u64::from(self.sb.inode_size))
            );

        // Read the inode bytes.
        let inode_bytes = self.reader.read_bytes(
            inode_byte_offset,
            self.sb.inode_size as usize,
        )?;

        // Parse the core 128-byte inode.
        if inode_bytes.len() < core::mem::size_of::<Ext4Inode>() {
            return Err(KernelError::IoError);
        }

        let inode = read_struct::<Ext4Inode>(&inode_bytes)?;
        Ok(inode)
    }

    /// Read the contents of a file given its inode.
    ///
    /// Currently supports extent-based files only (the standard ext4 format).
    pub fn read_file_data(&self, inode: &Ext4Inode) -> KernelResult<Vec<u8>> {
        let file_size = self.inode_size(inode);

        if file_size == 0 {
            return Ok(Vec::new());
        }

        // Check if the inode uses extents.
        if (inode.i_flags & inode_flags::EXTENTS) == 0 {
            // Indirect block mapping — not yet supported.
            return Err(KernelError::NotSupported);
        }

        // Read data via extent tree.
        self.read_extent_data(inode, file_size)
    }

    /// Read a byte range from a file's extent tree.
    ///
    /// Only reads the blocks that overlap `[offset, offset+len)`,
    /// avoiding reading the entire file for large-file partial reads.
    pub fn read_file_range(
        &self,
        inode: &Ext4Inode,
        offset: u64,
        len: usize,
    ) -> KernelResult<Vec<u8>> {
        let file_size = self.inode_size(inode);

        if offset >= file_size {
            return Ok(Vec::new());
        }

        // Clamp to file size.
        let actual_len = len.min(file_size.saturating_sub(offset) as usize);
        if actual_len == 0 {
            return Ok(Vec::new());
        }

        if (inode.i_flags & inode_flags::EXTENTS) == 0 {
            return Err(KernelError::NotSupported);
        }

        let block_size = u64::from(self.sb.block_size);

        // Determine which logical blocks we need.
        let first_logical = offset / block_size;
        let last_logical = (offset.saturating_add(actual_len as u64).saturating_sub(1)) / block_size;

        // Walk the extent tree and read only blocks in our range.
        let block_bytes = inode_block_as_bytes(inode);
        let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;
        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Err(KernelError::IoError);
        }

        let mut result = Vec::with_capacity(actual_len);

        // Walk the extent tree and read only blocks in our range.
        self.read_range_from_tree(
            block_bytes,
            &header,
            first_logical,
            last_logical,
            offset,
            actual_len,
            &mut result,
        )?;

        result.truncate(actual_len);
        Ok(result)
    }

    /// Read directory entries from a directory inode.
    ///
    /// Returns a vector of (inode_number, file_type, name) tuples.
    pub fn read_dir_entries(
        &self,
        dir_inode: &Ext4Inode,
    ) -> KernelResult<Vec<(u32, u8, String)>> {
        // Read directory data.
        let data = self.read_file_data(dir_inode)?;
        parse_dir_entries(&data)
    }

    /// Look up a name in a directory and return the inode number.
    ///
    /// Checks the ext4 dcache first for an O(1) hit.  On miss, does a
    /// linear scan of the directory entries and caches the result.
    pub fn dir_lookup(
        &mut self,
        dir_inode: &Ext4Inode,
        dir_ino: u32,
        name: &str,
    ) -> KernelResult<u32> {
        // Check dcache first.
        if let Some((child_ino, _ftype)) = self.dcache.lookup(dir_ino, name) {
            return Ok(child_ino);
        }

        // Cache miss — linear scan.
        let entries = self.read_dir_entries(dir_inode)?;
        for (ino, ftype, entry_name) in &entries {
            if entry_name == name {
                // Cache this lookup for next time.
                self.dcache.insert(dir_ino, name, *ino, *ftype);
                return Ok(*ino);
            }
        }
        Err(KernelError::NotFound)
    }

    /// Maximum number of symlinks followed during a single path resolution.
    ///
    /// Matches Linux's `MAXSYMLINKS` (40) and our memfs implementation.
    /// Prevents infinite loops from circular symlinks.
    const MAX_SYMLINK_DEPTH: usize = 40;

    /// Resolve a path to an inode number, following all symlinks.
    ///
    /// `path` must be absolute (starting with `/`).
    pub fn resolve_path(&mut self, path: &str) -> KernelResult<u32> {
        self.resolve_path_from(EXT4_ROOT_INO, path, true, 0)
    }

    /// Resolve a path without following the final symlink.
    ///
    /// Intermediate symlinks ARE followed; only the last component is
    /// left unresolved if it happens to be a symlink.  Used for `lstat`.
    pub fn resolve_path_no_follow(&mut self, path: &str) -> KernelResult<u32> {
        self.resolve_path_from(EXT4_ROOT_INO, path, false, 0)
    }

    /// Core path resolution with symlink following.
    ///
    /// `start_ino` is the directory inode to start from.  For absolute
    /// paths this is `EXT4_ROOT_INO`; for relative symlink targets it
    /// is the directory containing the symlink.
    ///
    /// `follow_last` controls whether the final component is followed
    /// if it is a symlink.
    ///
    /// `depth` tracks symlink recursion to prevent infinite loops.
    fn resolve_path_from(
        &mut self,
        start_ino: u32,
        path: &str,
        follow_last: bool,
        depth: usize,
    ) -> KernelResult<u32> {
        if depth > Self::MAX_SYMLINK_DEPTH {
            return Err(KernelError::TooManyLinks);
        }

        // Handle absolute vs relative paths.
        let (mut current_ino, path) = if path.starts_with('/') {
            (EXT4_ROOT_INO, path.strip_prefix('/').unwrap_or(path))
        } else {
            (start_ino, path)
        };

        if path.is_empty() {
            return Ok(current_ino);
        }

        // Collect components so we can index into them for building
        // remaining paths when we encounter a symlink.
        let components: Vec<&str> = path
            .split('/')
            .filter(|c| !c.is_empty() && *c != ".")
            .collect();

        if components.is_empty() {
            return Ok(current_ino);
        }

        for (i, component) in components.iter().enumerate() {
            let is_last = i == components.len() - 1;

            let dir_inode = self.read_inode(current_ino)?;

            // Current inode must be a directory to traverse into.
            if (dir_inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
                return Err(KernelError::NotADirectory);
            }

            let child_ino = self.dir_lookup(&dir_inode, current_ino, component)?;
            let child_inode = self.read_inode(child_ino)?;

            // Check if the child is a symlink.
            if (child_inode.i_mode & file_type::S_IFMT) == file_type::S_IFLNK {
                if is_last && !follow_last {
                    // Don't follow the final component — return the
                    // symlink inode itself (for lstat/readlink).
                    return Ok(child_ino);
                }

                // Read the symlink target.
                let target = self.read_symlink_target(&child_inode)?;
                let target_str = core::str::from_utf8(&target)
                    .map_err(|_| KernelError::IoError)?;

                // Build the new path: target + remaining components.
                let mut new_path = String::from(target_str);
                for rem in &components[i + 1..] {
                    new_path.push('/');
                    new_path.push_str(rem);
                }

                // Recurse.  For absolute targets, start_ino is ignored
                // (resolve_path_from detects the leading `/`).  For
                // relative targets, continue from the current directory
                // (the symlink's parent).
                return self.resolve_path_from(
                    current_ino,
                    &new_path,
                    follow_last,
                    depth + 1,
                );
            }

            current_ino = child_ino;
        }

        Ok(current_ino)
    }

    /// Read a symlink's target bytes from its inode.
    ///
    /// Fast symlinks (≤60 bytes) store the target in `i_block`.
    /// Slow symlinks store it in data blocks via the extent tree.
    pub fn read_symlink_target(&self, inode: &Ext4Inode) -> KernelResult<Vec<u8>> {
        let size = self.inode_size(inode) as usize;

        if size <= 60 && (inode.i_flags & inode_flags::EXTENTS) == 0 {
            // Fast symlink: target stored directly in i_block.
            let block_bytes = inode_block_as_bytes(inode);
            let target = block_bytes.get(..size).ok_or(KernelError::IoError)?;
            Ok(target.to_vec())
        } else {
            // Slow symlink: target stored in data blocks.
            self.read_file_data(inode)
        }
    }

    // -----------------------------------------------------------------------
    // Write operations
    // -----------------------------------------------------------------------

    /// Write an inode to disk.
    ///
    /// Writes the 128-byte core inode structure back to its on-disk
    /// location.  Caller is responsible for modifying the inode fields
    /// before calling this.
    pub fn write_inode(&self, inode_nr: u32, inode: &Ext4Inode) -> KernelResult<()> {
        if inode_nr == 0 {
            return Err(KernelError::InvalidArgument);
        }

        let group = self.sb.inode_group(inode_nr);
        let index = self.sb.inode_index_in_group(inode_nr);

        let gd = self.group_descs.get(group as usize)
            .ok_or(KernelError::InvalidArgument)?;

        let inode_table_block = if self.sb.is_64bit {
            u64::from(gd.bg_inode_table_lo)
                | (u64::from(gd.bg_inode_table_hi) << 32)
        } else {
            u64::from(gd.bg_inode_table_lo)
        };

        let inode_byte_offset = inode_table_block
            .saturating_mul(u64::from(self.sb.block_size))
            .saturating_add(
                u64::from(index).saturating_mul(u64::from(self.sb.inode_size))
            );

        // Serialize the inode to bytes.
        let inode_bytes = struct_as_bytes(inode);
        self.reader.write_bytes(inode_byte_offset, inode_bytes)
    }

    /// Write the superblock back to disk.
    ///
    /// The superblock is at byte offset 1024 from partition start.
    pub fn write_superblock(&self) -> KernelResult<()> {
        let sb_bytes = struct_as_bytes(&self.sb.raw);
        self.reader.write_bytes(
            super::superblock::superblock_device_offset(),
            sb_bytes,
        )
    }

    /// Write all block group descriptors back to disk.
    pub fn write_group_descs(&self) -> KernelResult<()> {
        let gd_size = self.sb.desc_size as usize;
        let gdt_start = self.sb.group_desc_offset(0);

        for (i, gd) in self.group_descs.iter().enumerate() {
            let offset = gdt_start.saturating_add(
                (i as u64).saturating_mul(gd_size as u64)
            );
            let gd_bytes = struct_as_bytes(gd);
            // Write only desc_size bytes (may be 32 or 64).
            let write_len = gd_bytes.len().min(gd_size);
            if let Some(data) = gd_bytes.get(..write_len) {
                self.reader.write_bytes(offset, data)?;
            }
        }

        Ok(())
    }

    /// Write file data to an inode using extents.
    ///
    /// Allocates blocks as needed and sets up the extent tree.
    /// The inode's i_block is initialized with a single extent pointing
    /// to the allocated blocks.
    ///
    /// Returns the modified inode (caller should write it with `write_inode`).
    pub fn write_file_data(
        &mut self,
        inode: &mut Ext4Inode,
        data: &[u8],
    ) -> KernelResult<()> {
        let block_size = self.sb.block_size as usize;

        if data.is_empty() {
            // Empty file: no blocks needed.
            inode.i_size_lo = 0;
            inode.i_size_high = 0;
            inode.i_blocks_lo = 0;
            // Initialize extent header with 0 entries.
            self.init_extent_header(inode, 0);
            return Ok(());
        }

        // Calculate blocks needed.
        let blocks_needed = data.len()
            .saturating_add(block_size)
            .saturating_sub(1)
            / block_size;

        // Try to allocate contiguous blocks.
        // Goal: start of the inode's block group for locality.
        let goal = u64::from(self.sb.raw.s_first_data_block);

        let first_block = super::balloc::alloc_blocks(
            &self.reader,
            &mut self.sb,
            &mut self.group_descs,
            goal,
            blocks_needed as u32,
        )?;

        // Write data to the allocated blocks.
        let mut offset = 0usize;
        for i in 0..blocks_needed {
            let block_nr = first_block.saturating_add(i as u64);
            let end = (offset.saturating_add(block_size)).min(data.len());
            let chunk = data.get(offset..end).unwrap_or(&[]);

            // Pad the last block with zeros if needed.
            let mut buf = vec![0u8; block_size];
            if let Some(dest) = buf.get_mut(..chunk.len()) {
                dest.copy_from_slice(chunk);
            }
            self.reader.write_block(block_nr, &buf)?;

            offset = end;
        }

        // Set up the extent tree in the inode.
        self.init_extent_header(inode, 1);
        self.set_single_extent(
            inode,
            0, // logical block 0
            first_block,
            blocks_needed as u16,
        );

        // Update inode size and block count.
        let file_size = data.len() as u64;
        inode.i_size_lo = file_size as u32;
        inode.i_size_high = (file_size >> 32) as u32;

        // i_blocks_lo counts in 512-byte units.
        let sectors = (blocks_needed as u32)
            .saturating_mul(self.sb.block_size / 512);
        inode.i_blocks_lo = sectors;

        Ok(())
    }

    /// Create a new empty inode with the given mode and flags.
    ///
    /// Allocates an inode number, initializes the on-disk inode, and
    /// writes it.  Returns the inode number and the initialized inode.
    pub fn create_inode(
        &mut self,
        mode: u16,
        preferred_group: u32,
    ) -> KernelResult<(u32, Ext4Inode)> {
        let is_dir = (mode & file_type::S_IFMT) == file_type::S_IFDIR;

        let inode_nr = super::balloc::alloc_inode(
            &self.reader,
            &mut self.sb,
            &mut self.group_descs,
            preferred_group,
            is_dir,
        )?;

        // Initialize a blank inode.
        let mut inode = blank_inode();
        inode.i_mode = mode;
        inode.i_flags = inode_flags::EXTENTS; // Use extent tree.
        inode.i_links_count = if is_dir { 2 } else { 1 }; // . and .. for dirs.

        // Initialize the extent header (0 entries).
        self.init_extent_header(&mut inode, 0);

        // Write the new inode to disk.
        self.write_inode(inode_nr, &inode)?;

        if is_dir {
            // Increment the used_dirs count in the group descriptor.
            let group = self.sb.inode_group(inode_nr) as usize;
            if let Some(gd) = self.group_descs.get_mut(group) {
                gd.bg_used_dirs_count_lo = gd.bg_used_dirs_count_lo.saturating_add(1);
            }
        }

        Ok((inode_nr, inode))
    }

    /// Add a directory entry to a directory inode.
    ///
    /// Appends a new entry to the directory's data.  If the current
    /// last block has space, the entry is inserted there.  Otherwise,
    /// a new block is allocated.
    pub fn add_dir_entry(
        &mut self,
        dir_inode: &mut Ext4Inode,
        dir_inode_nr: u32,
        child_ino: u32,
        name: &str,
        file_type_byte: u8,
    ) -> KernelResult<()> {
        let name_bytes = name.as_bytes();
        if name_bytes.is_empty() || name_bytes.len() > 255 {
            return Err(KernelError::InvalidArgument);
        }

        // Read existing directory data.
        let mut dir_data = self.read_file_data(dir_inode)?;
        let block_size = self.sb.block_size as usize;

        // Calculate the new entry size (aligned to 4 bytes).
        let entry_header_size = 8usize; // inode(4) + rec_len(2) + name_len(1) + file_type(1)
        let entry_size = entry_header_size.saturating_add(name_bytes.len());
        let entry_size_aligned = (entry_size.saturating_add(3)) & !3;

        // Try to find space in the last block by compacting the last entry.
        let dir_len = dir_data.len();
        if dir_len > 0 {
            // Find the last entry in the last block.
            let last_block_start = (dir_len / block_size) * block_size;
            if last_block_start < dir_len {
                // Actually, we need to find the last entry by walking.
                // The last entry's rec_len extends to the end of the block.
                if let Some(space) = find_dir_insert_point(
                    &dir_data,
                    last_block_start,
                    block_size,
                    entry_size_aligned,
                ) {
                    // Insert the new entry by shrinking the previous entry's
                    // rec_len and writing the new entry at `space`.
                    insert_dir_entry(
                        &mut dir_data,
                        space,
                        child_ino,
                        name_bytes,
                        file_type_byte,
                        block_size.saturating_sub(space % block_size),
                    );

                    // Write the modified directory data back.
                    self.write_file_data(dir_inode, &dir_data)?;
                    self.write_inode(dir_inode_nr, dir_inode)?;
                    return Ok(());
                }
            }
        }

        // No space in existing blocks — allocate a new block.
        let goal = u64::from(self.sb.raw.s_first_data_block);
        let new_block = super::balloc::alloc_block(
            &self.reader,
            &mut self.sb,
            &mut self.group_descs,
            goal,
        )?;

        // Initialize the new block with a single entry spanning the whole block.
        let mut block_buf = vec![0u8; block_size];
        write_dir_entry_raw(
            &mut block_buf,
            0,
            child_ino,
            name_bytes,
            file_type_byte,
            block_size, // rec_len spans whole block
        );
        self.reader.write_block(new_block, &block_buf)?;

        // Update the directory inode to include the new block.
        // For simplicity, rebuild the extent tree to add one more block.
        // This works for small directories; a production implementation
        // would update the extent tree incrementally.
        let new_size = dir_data.len().saturating_add(block_size) as u64;
        dir_inode.i_size_lo = new_size as u32;
        dir_inode.i_size_high = (new_size >> 32) as u32;

        // Update block count.
        let total_blocks = new_size as u32 / self.sb.block_size;
        dir_inode.i_blocks_lo = total_blocks.saturating_mul(self.sb.block_size / 512);

        self.write_inode(dir_inode_nr, dir_inode)?;

        // Invalidate dcache for the parent directory so stale entries
        // don't hide the new child.
        self.dcache.invalidate_entry(dir_inode_nr, name);

        Ok(())
    }

    /// Collect all physical block ranges referenced by an inode's extent tree.
    ///
    /// Returns a list of `(physical_block, block_count)` pairs covering
    /// all data extents.  For multi-level trees, also includes the
    /// intermediate index (internal) blocks so they can be freed too.
    ///
    /// Only handles extent-based inodes.  Returns an empty list for
    /// inodes with no data or non-extent inodes.
    pub fn collect_extent_blocks(
        &self,
        inode: &Ext4Inode,
    ) -> KernelResult<Vec<(u64, u32)>> {
        let mut result = Vec::new();

        // Empty file or non-extent inode — nothing to free.
        if (inode.i_flags & inode_flags::EXTENTS) == 0 {
            return Ok(result);
        }

        let block_bytes = inode_block_as_bytes(inode);
        let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;
        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Ok(result);
        }
        if header.eh_entries == 0 {
            return Ok(result);
        }

        self.collect_extents_recursive(block_bytes, &header, &mut result)?;
        Ok(result)
    }

    /// Recursively walk an extent tree node and collect all block ranges.
    fn collect_extents_recursive(
        &self,
        node_data: &[u8],
        header: &Ext4ExtentHeader,
        result: &mut Vec<(u64, u32)>,
    ) -> KernelResult<()> {
        let header_size = core::mem::size_of::<Ext4ExtentHeader>();

        if header.eh_depth == 0 {
            // Leaf node — collect data extents.
            let extent_size = core::mem::size_of::<Ext4Extent>();
            for i in 0..header.eh_entries as usize {
                let offset = header_size.saturating_add(i.saturating_mul(extent_size));
                let ext_bytes = node_data
                    .get(offset..offset.saturating_add(extent_size))
                    .ok_or(KernelError::IoError)?;
                let extent = read_struct::<Ext4Extent>(ext_bytes)?;

                let phys = u64::from(extent.ee_start_lo)
                    | (u64::from(extent.ee_start_hi) << 32);
                // Mask off uninitialized-extent flag.
                let len = u32::from(extent.ee_len & 0x7FFF);

                if phys != 0 && len > 0 {
                    result.push((phys, len));
                }
            }
        } else {
            // Internal node — follow index entries, and remember that the
            // child blocks themselves need freeing too.
            let idx_size = core::mem::size_of::<super::ondisk::Ext4ExtentIdx>();
            let block_size = self.sb.block_size as usize;

            for i in 0..header.eh_entries as usize {
                let offset = header_size.saturating_add(i.saturating_mul(idx_size));
                let idx_bytes = node_data
                    .get(offset..offset.saturating_add(idx_size))
                    .ok_or(KernelError::IoError)?;
                let idx = read_struct::<super::ondisk::Ext4ExtentIdx>(idx_bytes)?;

                let child_block = u64::from(idx.ei_leaf_lo)
                    | (u64::from(idx.ei_leaf_hi) << 32);

                if child_block == 0 {
                    continue;
                }

                // Read the child block.
                let mut child_data = vec![0u8; block_size];
                self.reader.read_block(child_block, &mut child_data)?;

                let child_header = read_struct::<Ext4ExtentHeader>(&child_data)?;
                if child_header.eh_magic != EXT4_EXTENT_MAGIC {
                    continue;
                }

                // Recurse into the child.
                self.collect_extents_recursive(&child_data, &child_header, result)?;

                // The index block itself is also allocated and needs freeing.
                result.push((child_block, 1));
            }
        }

        Ok(())
    }

    /// Free all data blocks referenced by an inode's extent tree.
    ///
    /// Walks the extent tree, collects all block ranges, and frees them
    /// via the block allocator.  Does NOT free the inode itself — call
    /// `free_inode_number` separately.
    pub fn free_inode_data(&mut self, inode: &Ext4Inode) -> KernelResult<()> {
        let ranges = self.collect_extent_blocks(inode)?;

        for (start, count) in ranges {
            // Free each range.  We tolerate individual errors (e.g., double-free
            // from a corrupted extent tree) and continue freeing remaining ranges.
            if let Err(e) = super::balloc::free_blocks(
                &self.reader,
                &mut self.sb,
                &mut self.group_descs,
                start,
                count,
            ) {
                serial_println!(
                    "[ext4] warning: failed to free block range {}-{}: {:?}",
                    start,
                    start.saturating_add(u64::from(count)),
                    e,
                );
            }
        }

        Ok(())
    }

    /// Free the external xattr block if one exists.
    ///
    /// Called during inode deletion to reclaim the xattr block.
    pub fn free_xattr_block(&mut self, inode: &Ext4Inode) -> KernelResult<()> {
        let block_nr = self.xattr_block(inode);
        if block_nr == 0 {
            return Ok(());
        }
        super::balloc::free_block(
            &self.reader,
            &mut self.sb,
            &mut self.group_descs,
            block_nr,
        )
    }

    /// Free an inode number back to the inode bitmap.
    ///
    /// Also decrements the used_dirs count if the inode is a directory.
    pub fn free_inode_number(
        &mut self,
        inode_nr: u32,
        is_directory: bool,
    ) -> KernelResult<()> {
        super::balloc::free_inode(
            &self.reader,
            &mut self.sb,
            &mut self.group_descs,
            inode_nr,
        )?;

        // Decrement the used_dirs count in the group descriptor.
        if is_directory {
            let group = self.sb.inode_group(inode_nr) as usize;
            if let Some(gd) = self.group_descs.get_mut(group) {
                gd.bg_used_dirs_count_lo =
                    gd.bg_used_dirs_count_lo.saturating_sub(1);
            }
        }

        Ok(())
    }

    /// Look up the physical block number for a given logical block.
    ///
    /// Walks the extent tree and returns the physical block that
    /// corresponds to `logical_block`, or `None` if the block is
    /// sparse (not allocated, e.g., in a file with holes).
    pub fn lookup_physical_block(
        &self,
        inode: &Ext4Inode,
        logical_block: u64,
    ) -> KernelResult<Option<u64>> {
        if (inode.i_flags & inode_flags::EXTENTS) == 0 {
            return Err(KernelError::NotSupported);
        }

        let block_bytes = inode_block_as_bytes(inode);
        let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;
        if header.eh_magic != EXT4_EXTENT_MAGIC || header.eh_entries == 0 {
            return Ok(None);
        }

        self.lookup_in_tree(block_bytes, &header, logical_block)
    }

    /// Recursively look up a logical block in an extent tree node.
    fn lookup_in_tree(
        &self,
        node_data: &[u8],
        header: &Ext4ExtentHeader,
        logical_block: u64,
    ) -> KernelResult<Option<u64>> {
        let header_size = core::mem::size_of::<Ext4ExtentHeader>();
        let block_size_usize = self.sb.block_size as usize;

        if header.eh_depth == 0 {
            // Leaf — search extents.
            let extent_size = core::mem::size_of::<Ext4Extent>();
            for i in 0..header.eh_entries as usize {
                let off = header_size.saturating_add(i.saturating_mul(extent_size));
                let ext_bytes = node_data
                    .get(off..off.saturating_add(extent_size))
                    .ok_or(KernelError::IoError)?;
                let extent = read_struct::<Ext4Extent>(ext_bytes)?;

                let ext_logical = u64::from(extent.ee_block);
                let ext_len = u64::from(extent.ee_len & 0x7FFF);
                let ext_phys = u64::from(extent.ee_start_lo)
                    | (u64::from(extent.ee_start_hi) << 32);

                if logical_block >= ext_logical
                    && logical_block < ext_logical.saturating_add(ext_len)
                {
                    let offset_in_ext = logical_block.saturating_sub(ext_logical);
                    return Ok(Some(ext_phys.saturating_add(offset_in_ext)));
                }
            }
            Ok(None)
        } else {
            // Internal node — find the right child.
            let idx_size = core::mem::size_of::<super::ondisk::Ext4ExtentIdx>();
            // Find the index entry whose range includes our block.
            // Index entries are sorted by ei_block.  The right child is
            // the last one with ei_block <= logical_block.
            let mut best_idx: Option<super::ondisk::Ext4ExtentIdx> = None;
            for i in 0..header.eh_entries as usize {
                let off = header_size.saturating_add(i.saturating_mul(idx_size));
                let idx_bytes = node_data
                    .get(off..off.saturating_add(idx_size))
                    .ok_or(KernelError::IoError)?;
                let idx = read_struct::<super::ondisk::Ext4ExtentIdx>(idx_bytes)?;

                if u64::from(idx.ei_block) <= logical_block {
                    best_idx = Some(idx);
                } else {
                    break;
                }
            }

            if let Some(idx) = best_idx {
                let child_block = u64::from(idx.ei_leaf_lo)
                    | (u64::from(idx.ei_leaf_hi) << 32);
                if child_block == 0 {
                    return Ok(None);
                }

                let mut child_data = vec![0u8; block_size_usize];
                self.reader.read_block(child_block, &mut child_data)?;

                let child_header = read_struct::<Ext4ExtentHeader>(&child_data)?;
                if child_header.eh_magic != EXT4_EXTENT_MAGIC {
                    return Ok(None);
                }

                self.lookup_in_tree(&child_data, &child_header, logical_block)
            } else {
                Ok(None)
            }
        }
    }

    /// Write data at a byte offset within an existing file, in place.
    ///
    /// Only modifies the disk blocks that are affected by the write.
    /// Does NOT extend the file — writes past the end are truncated.
    /// Caller should fall back to read-modify-write for extending writes.
    pub fn write_at_inplace(
        &self,
        inode: &Ext4Inode,
        offset: u64,
        data: &[u8],
    ) -> KernelResult<usize> {
        let file_size = self.inode_size(inode);
        if offset >= file_size || data.is_empty() {
            return Ok(0);
        }

        let block_size = u64::from(self.sb.block_size);
        let block_size_usize = self.sb.block_size as usize;

        // Clamp write to file size.
        let actual_len = data.len().min(file_size.saturating_sub(offset) as usize);
        let mut written = 0usize;

        while written < actual_len {
            let cur_offset = offset.saturating_add(written as u64);
            let logical_block = cur_offset / block_size;
            let offset_in_block = (cur_offset % block_size) as usize;

            // Look up the physical block.
            let phys = self.lookup_physical_block(inode, logical_block)?
                .ok_or(KernelError::IoError)?;

            // Read the existing block.
            let mut buf = vec![0u8; block_size_usize];
            self.reader.read_block(phys, &mut buf)?;

            // Calculate how much to write in this block.
            let space_in_block = block_size_usize.saturating_sub(offset_in_block);
            let chunk_len = space_in_block.min(actual_len.saturating_sub(written));

            if let (Some(dest), Some(src)) = (
                buf.get_mut(offset_in_block..offset_in_block.saturating_add(chunk_len)),
                data.get(written..written.saturating_add(chunk_len)),
            ) {
                dest.copy_from_slice(src);
            }

            // Write the modified block back.
            self.reader.write_block(phys, &buf)?;

            written = written.saturating_add(chunk_len);
        }

        Ok(written)
    }

    /// Read data from an extent tree, only reading blocks in
    /// the logical block range `[first_logical, last_logical]`.
    fn read_range_from_tree(
        &self,
        node_data: &[u8],
        header: &Ext4ExtentHeader,
        first_logical: u64,
        last_logical: u64,
        byte_offset: u64,
        byte_len: usize,
        result: &mut Vec<u8>,
    ) -> KernelResult<()> {
        let block_size = u64::from(self.sb.block_size);
        let block_size_usize = self.sb.block_size as usize;
        let header_size = core::mem::size_of::<Ext4ExtentHeader>();

        if header.eh_depth == 0 {
            // Leaf node — read matching extents.
            let extent_size = core::mem::size_of::<Ext4Extent>();
            for i in 0..header.eh_entries as usize {
                if result.len() >= byte_len {
                    return Ok(());
                }

                let off = header_size.saturating_add(i.saturating_mul(extent_size));
                let ext_bytes = node_data
                    .get(off..off.saturating_add(extent_size))
                    .ok_or(KernelError::IoError)?;
                let extent = read_struct::<Ext4Extent>(ext_bytes)?;

                let ext_logical = u64::from(extent.ee_block);
                let ext_len = u64::from(extent.ee_len & 0x7FFF);
                let ext_phys = u64::from(extent.ee_start_lo)
                    | (u64::from(extent.ee_start_hi) << 32);
                let ext_end = ext_logical.saturating_add(ext_len);

                // Skip extents that don't overlap our range.
                if ext_end <= first_logical || ext_logical > last_logical {
                    continue;
                }

                // Read blocks within this extent that overlap our range.
                for b in 0..ext_len {
                    let logical = ext_logical.saturating_add(b);
                    if logical < first_logical || logical > last_logical {
                        continue;
                    }
                    if result.len() >= byte_len {
                        return Ok(());
                    }

                    let phys = ext_phys.saturating_add(b);
                    let mut buf = vec![0u8; block_size_usize];
                    self.reader.read_block(phys, &mut buf)?;

                    // Calculate how much of this block to copy.
                    let block_start_byte = logical.saturating_mul(block_size);
                    let copy_start = if block_start_byte < byte_offset {
                        (byte_offset.saturating_sub(block_start_byte)) as usize
                    } else {
                        0
                    };
                    let remaining = byte_len.saturating_sub(result.len());
                    let copy_end = block_size_usize.min(copy_start.saturating_add(remaining));

                    if let Some(data) = buf.get(copy_start..copy_end) {
                        result.extend_from_slice(data);
                    }
                }
            }
        } else {
            // Internal node — recurse into child blocks.
            let idx_size = core::mem::size_of::<super::ondisk::Ext4ExtentIdx>();
            for i in 0..header.eh_entries as usize {
                if result.len() >= byte_len {
                    return Ok(());
                }

                let off = header_size.saturating_add(i.saturating_mul(idx_size));
                let idx_bytes = node_data
                    .get(off..off.saturating_add(idx_size))
                    .ok_or(KernelError::IoError)?;
                let idx = read_struct::<super::ondisk::Ext4ExtentIdx>(idx_bytes)?;

                let child_block = u64::from(idx.ei_leaf_lo)
                    | (u64::from(idx.ei_leaf_hi) << 32);
                if child_block == 0 {
                    continue;
                }

                let mut child_data = vec![0u8; block_size_usize];
                self.reader.read_block(child_block, &mut child_data)?;

                let child_header = read_struct::<Ext4ExtentHeader>(&child_data)?;
                if child_header.eh_magic != EXT4_EXTENT_MAGIC {
                    continue;
                }

                self.read_range_from_tree(
                    &child_data,
                    &child_header,
                    first_logical,
                    last_logical,
                    byte_offset,
                    byte_len,
                    result,
                )?;
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Extended attribute operations
    // -----------------------------------------------------------------------

    /// Read the xattr block number from an inode.
    ///
    /// Returns 0 if the inode has no external xattr block.
    fn xattr_block(&self, inode: &Ext4Inode) -> u64 {
        let lo = u64::from(inode.i_file_acl_lo);
        // High 16 bits are in i_osd2 bytes 2..4 on Linux.
        let hi = u64::from(u16::from_le_bytes([
            *inode.i_osd2.get(4).unwrap_or(&0),
            *inode.i_osd2.get(5).unwrap_or(&0),
        ]));
        lo | (hi << 32)
    }

    /// Parse all xattr entries from an external xattr block.
    ///
    /// Returns a list of `(full_key, value)` pairs.  The key is
    /// reconstructed by prepending the namespace prefix (e.g., "user.").
    pub fn read_xattrs(&self, inode: &Ext4Inode) -> KernelResult<Vec<(String, Vec<u8>)>> {
        let block_nr = self.xattr_block(inode);
        if block_nr == 0 {
            return Ok(Vec::new());
        }

        let block_size = self.sb.block_size as usize;
        let mut block_data = vec![0u8; block_size];
        self.reader.read_block(block_nr, &mut block_data)?;

        // Validate the header.
        let header = read_struct::<super::ondisk::Ext4XattrHeader>(&block_data)?;
        if header.h_magic != super::ondisk::EXT4_XATTR_MAGIC {
            return Err(KernelError::IoError);
        }

        let header_size = core::mem::size_of::<super::ondisk::Ext4XattrHeader>();
        let entry_header_size = core::mem::size_of::<super::ondisk::Ext4XattrEntry>();

        let mut result = Vec::new();
        let mut offset = header_size;

        loop {
            // Check for end of entries (sentinel: 4 zero bytes).
            if offset.saturating_add(4) > block_size {
                break;
            }
            if block_data.get(offset..offset.saturating_add(4)) == Some(&[0, 0, 0, 0]) {
                break;
            }

            // Read entry header.
            if offset.saturating_add(entry_header_size) > block_size {
                break;
            }
            let entry_bytes = block_data.get(offset..offset.saturating_add(entry_header_size))
                .ok_or(KernelError::IoError)?;
            let entry = read_struct::<super::ondisk::Ext4XattrEntry>(entry_bytes)?;

            // Read the name.
            let name_start = offset.saturating_add(entry_header_size);
            let name_end = name_start.saturating_add(entry.e_name_len as usize);
            let name_bytes = block_data.get(name_start..name_end)
                .ok_or(KernelError::IoError)?;
            let name = core::str::from_utf8(name_bytes)
                .map_err(|_| KernelError::IoError)?;

            // Build the full key with namespace prefix.
            let full_key = xattr_full_key(entry.e_name_index, name);

            // Read the value.
            let val_start = entry.e_value_offs as usize;
            let val_end = val_start.saturating_add(entry.e_value_size as usize);
            let value = if entry.e_value_size > 0 && val_end <= block_size {
                block_data.get(val_start..val_end)
                    .unwrap_or(&[])
                    .to_vec()
            } else {
                Vec::new()
            };

            result.push((full_key, value));

            // Advance past entry header + name, aligned to 4 bytes.
            let entry_total = entry_header_size.saturating_add(entry.e_name_len as usize);
            let aligned = (entry_total.saturating_add(3)) & !3;
            offset = offset.saturating_add(aligned);
        }

        Ok(result)
    }

    /// Get a single xattr value by full key (e.g., "user.myattr").
    pub fn get_xattr(&self, inode: &Ext4Inode, key: &str) -> KernelResult<Vec<u8>> {
        let attrs = self.read_xattrs(inode)?;
        for (k, v) in &attrs {
            if k == key {
                return Ok(v.clone());
            }
        }
        Err(KernelError::NotFound)
    }

    /// Write the xattr block for an inode with the given set of attributes.
    ///
    /// Allocates a new xattr block if needed, or updates the existing one.
    /// The caller provides the full set of xattrs — this is a replace-all
    /// operation.  Returns the block number used (0 if attrs is empty).
    pub fn write_xattr_block(
        &mut self,
        inode: &mut Ext4Inode,
        inode_nr: u32,
        attrs: &[(String, Vec<u8>)],
    ) -> KernelResult<u64> {
        let old_block = self.xattr_block(inode);

        if attrs.is_empty() {
            // No xattrs — free the old block if it exists.
            if old_block != 0 {
                super::balloc::free_block(
                    &self.reader,
                    &mut self.sb,
                    &mut self.group_descs,
                    old_block,
                )?;
                inode.i_file_acl_lo = 0;
                // Clear high bits in i_osd2.
                if let Some(b) = inode.i_osd2.get_mut(4) { *b = 0; }
                if let Some(b) = inode.i_osd2.get_mut(5) { *b = 0; }
                self.write_inode(inode_nr, inode)?;
            }
            return Ok(0);
        }

        // Build the xattr block.
        let block_size = self.sb.block_size as usize;
        let mut block_data = vec![0u8; block_size];
        let header_size = core::mem::size_of::<super::ondisk::Ext4XattrHeader>();
        let entry_header_size = core::mem::size_of::<super::ondisk::Ext4XattrEntry>();

        // Write header.
        let header = super::ondisk::Ext4XattrHeader {
            h_magic: super::ondisk::EXT4_XATTR_MAGIC,
            h_refcount: 1,
            h_blocks: 1,
            h_hash: 0,
            h_checksum: 0,
            h_reserved: [0; 3],
        };
        let hdr_bytes = struct_as_bytes(&header);
        if let Some(dest) = block_data.get_mut(..hdr_bytes.len()) {
            dest.copy_from_slice(hdr_bytes);
        }

        // Write entries from the front, values from the back.
        let mut entry_offset = header_size;
        let mut value_end = block_size; // values grow backward from end

        for (key, value) in attrs {
            let (name_index, name) = xattr_split_key(key);
            let name_bytes = name.as_bytes();

            // Check that entry + value fit.
            let entry_total = entry_header_size.saturating_add(name_bytes.len());
            let entry_aligned = (entry_total.saturating_add(3)) & !3;
            let value_aligned = (value.len().saturating_add(3)) & !3;

            // Value goes at end of block.
            let value_start = value_end.saturating_sub(value_aligned);
            if entry_offset.saturating_add(entry_aligned) > value_start {
                // No room — xattr block is full.
                return Err(KernelError::DiskFull);
            }

            // Write the value.
            if let Some(dest) = block_data.get_mut(value_start..value_start.saturating_add(value.len())) {
                dest.copy_from_slice(value);
            }
            value_end = value_start;

            // Write the entry header.
            let entry = super::ondisk::Ext4XattrEntry {
                e_name_len: name_bytes.len() as u8,
                e_name_index: name_index,
                e_value_offs: value_start as u16,
                e_value_inum: 0,
                e_value_size: value.len() as u32,
                e_hash: 0,
            };
            let entry_bytes = struct_as_bytes(&entry);
            if let Some(dest) = block_data.get_mut(entry_offset..entry_offset.saturating_add(entry_bytes.len())) {
                dest.copy_from_slice(entry_bytes);
            }

            // Write the name.
            let name_start = entry_offset.saturating_add(entry_header_size);
            if let Some(dest) = block_data.get_mut(name_start..name_start.saturating_add(name_bytes.len())) {
                dest.copy_from_slice(name_bytes);
            }

            entry_offset = entry_offset.saturating_add(entry_aligned);
        }

        // Write sentinel (4 zero bytes after last entry — already zero).

        // Allocate or reuse a block.
        let block_nr = if old_block != 0 {
            // Reuse the existing block.
            old_block
        } else {
            // Allocate a new block.
            let goal = u64::from(self.sb.raw.s_first_data_block);
            super::balloc::alloc_block(
                &self.reader,
                &mut self.sb,
                &mut self.group_descs,
                goal,
            )?
        };

        // Write the xattr block to disk.
        self.reader.write_block(block_nr, &block_data)?;

        // Update the inode's i_file_acl field.
        inode.i_file_acl_lo = block_nr as u32;
        // High bits.
        let hi = (block_nr >> 32) as u16;
        let hi_bytes = hi.to_le_bytes();
        if let Some(b) = inode.i_osd2.get_mut(4) { *b = hi_bytes[0]; }
        if let Some(b) = inode.i_osd2.get_mut(5) { *b = hi_bytes[1]; }

        self.write_inode(inode_nr, inode)?;

        Ok(block_nr)
    }

    /// Flush all cached writes for this filesystem to disk.
    pub fn flush(&self) -> KernelResult<()> {
        self.reader.flush()
    }

    /// Mutable access to the parsed superblock.
    #[allow(dead_code)]
    pub fn superblock_mut(&mut self) -> &mut ParsedSuperblock {
        &mut self.sb
    }

    /// Mutable access to the group descriptor table.
    #[allow(dead_code)]
    pub fn group_descs_mut(&mut self) -> &mut Vec<Ext4GroupDesc> {
        &mut self.group_descs
    }

    /// Access the block reader.
    #[allow(dead_code)]
    pub fn reader(&self) -> &BlockReader {
        &self.reader
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Initialize the extent header in an inode's i_block field (public).
    ///
    /// Used by the VFS layer for truncate-to-zero (resets extent tree).
    pub fn init_extent_header_pub(&self, inode: &mut Ext4Inode, entries: u16) {
        self.init_extent_header(inode, entries);
    }

    /// Initialize the extent header in an inode's i_block field.
    fn init_extent_header(&self, inode: &mut Ext4Inode, entries: u16) {
        // The extent header occupies the first 12 bytes of i_block.
        // eh_magic(2) + eh_entries(2) + eh_max(2) + eh_depth(2) + eh_generation(4)
        inode.i_block[0] = u32::from(EXT4_EXTENT_MAGIC)
            | (u32::from(entries) << 16);
        // Max entries in i_block: (60 - 12) / 12 = 4 extents.
        let max_entries: u16 = 4;
        inode.i_block[1] = u32::from(max_entries); // eh_max + eh_depth(0)
        inode.i_block[2] = 0; // eh_generation
    }

    /// Set a single extent at the given index in the inode's i_block.
    fn set_single_extent(
        &self,
        inode: &mut Ext4Inode,
        logical_block: u32,
        physical_block: u64,
        block_count: u16,
    ) {
        // Extent header is 12 bytes = 3 u32s (i_block[0..3]).
        // First extent starts at i_block[3].
        // Each extent is 12 bytes = 3 u32s:
        //   ee_block(4) + ee_len(2) + ee_start_hi(2) + ee_start_lo(4)
        let base = 3; // offset in i_block for first extent
        inode.i_block[base] = logical_block;
        inode.i_block[base + 1] = u32::from(block_count)
            | ((physical_block >> 32) as u32) << 16;
        inode.i_block[base + 2] = physical_block as u32;
    }

    /// Get the full 64-bit size of an inode.
    fn inode_size(&self, inode: &Ext4Inode) -> u64 {
        let lo = u64::from(inode.i_size_lo);
        // For regular files, high 32 bits are in i_size_high.
        // For directories, i_size_high is the directory ACL.
        let is_file = (inode.i_mode & file_type::S_IFMT) == file_type::S_IFREG;
        if is_file {
            lo | (u64::from(inode.i_size_high) << 32)
        } else {
            lo
        }
    }

    /// Read file data using the extent tree.
    fn read_extent_data(&self, inode: &Ext4Inode, file_size: u64) -> KernelResult<Vec<u8>> {
        let block_size = u64::from(self.sb.block_size);

        // The extent tree root is in inode.i_block (60 bytes).
        // First 12 bytes = extent header, rest = extent entries.
        let block_bytes = inode_block_as_bytes(inode);

        // Parse the extent header.
        let header = read_struct::<Ext4ExtentHeader>(&block_bytes)?;
        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Err(KernelError::IoError);
        }

        let mut result = Vec::with_capacity(file_size as usize);

        if header.eh_depth == 0 {
            // Leaf node — extents are directly in i_block.
            let entries = header.eh_entries as usize;
            let header_size = core::mem::size_of::<Ext4ExtentHeader>();
            let extent_size = core::mem::size_of::<Ext4Extent>();

            for i in 0..entries {
                let offset = header_size.saturating_add(i.saturating_mul(extent_size));
                let ext_bytes = block_bytes.get(offset..offset.saturating_add(extent_size))
                    .ok_or(KernelError::IoError)?;
                let extent = read_struct::<Ext4Extent>(ext_bytes)?;

                let phys_block = u64::from(extent.ee_start_lo)
                    | (u64::from(extent.ee_start_hi) << 32);
                // Uninitialized extents have the high bit of ee_len set.
                let block_count = u64::from(extent.ee_len & 0x7FFF);

                for b in 0..block_count {
                    let block_nr = phys_block.saturating_add(b);
                    let mut buf = vec![0u8; block_size as usize];
                    self.reader.read_block(block_nr, &mut buf)?;

                    // Don't append past file_size.
                    let remaining = file_size.saturating_sub(result.len() as u64);
                    let copy_len = (block_size).min(remaining) as usize;
                    if let Some(data) = buf.get(..copy_len) {
                        result.extend_from_slice(data);
                    }
                }
            }
        } else {
            // Multi-level extent tree — follow index nodes.
            // For simplicity, handle depth=1 (one level of indirection).
            // Deeper trees are rare for files under ~340 MB.
            self.read_extent_tree_recursive(
                &block_bytes, &header, file_size, &mut result,
            )?;
        }

        // Truncate to exact file size.
        result.truncate(file_size as usize);
        Ok(result)
    }

    /// Recursively read data from an extent tree node.
    fn read_extent_tree_recursive(
        &self,
        node_data: &[u8],
        header: &Ext4ExtentHeader,
        file_size: u64,
        result: &mut Vec<u8>,
    ) -> KernelResult<()> {
        let block_size = self.sb.block_size as usize;
        let header_size = core::mem::size_of::<Ext4ExtentHeader>();

        if header.eh_depth == 0 {
            // Leaf: read extents.
            let extent_size = core::mem::size_of::<Ext4Extent>();
            for i in 0..header.eh_entries as usize {
                let offset = header_size.saturating_add(i.saturating_mul(extent_size));
                let ext_bytes = node_data.get(offset..offset.saturating_add(extent_size))
                    .ok_or(KernelError::IoError)?;
                let extent = read_struct::<Ext4Extent>(ext_bytes)?;

                let phys_block = u64::from(extent.ee_start_lo)
                    | (u64::from(extent.ee_start_hi) << 32);
                let block_count = u64::from(extent.ee_len & 0x7FFF);

                for b in 0..block_count {
                    if result.len() as u64 >= file_size {
                        return Ok(());
                    }
                    let block_nr = phys_block.saturating_add(b);
                    let mut buf = vec![0u8; block_size];
                    self.reader.read_block(block_nr, &mut buf)?;

                    let remaining = file_size.saturating_sub(result.len() as u64);
                    let copy_len = (block_size as u64).min(remaining) as usize;
                    if let Some(data) = buf.get(..copy_len) {
                        result.extend_from_slice(data);
                    }
                }
            }
        } else {
            // Internal node: follow index entries to child blocks.
            let idx_size = core::mem::size_of::<super::ondisk::Ext4ExtentIdx>();
            for i in 0..header.eh_entries as usize {
                if result.len() as u64 >= file_size {
                    return Ok(());
                }
                let offset = header_size.saturating_add(i.saturating_mul(idx_size));
                let idx_bytes = node_data.get(offset..offset.saturating_add(idx_size))
                    .ok_or(KernelError::IoError)?;
                let idx = read_struct::<super::ondisk::Ext4ExtentIdx>(idx_bytes)?;

                let child_block = u64::from(idx.ei_leaf_lo)
                    | (u64::from(idx.ei_leaf_hi) << 32);

                // Read the child block.
                let mut child_data = vec![0u8; block_size];
                self.reader.read_block(child_block, &mut child_data)?;

                // Parse child header.
                let child_header = read_struct::<Ext4ExtentHeader>(&child_data)?;
                if child_header.eh_magic != EXT4_EXTENT_MAGIC {
                    return Err(KernelError::IoError);
                }

                self.read_extent_tree_recursive(
                    &child_data, &child_header, file_size, result,
                )?;
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Block group descriptor reading
// ---------------------------------------------------------------------------

/// Read and parse all block group descriptors from the device.
fn read_group_descs(
    sb: &ParsedSuperblock,
    reader: &BlockReader,
) -> KernelResult<Vec<Ext4GroupDesc>> {
    let gd_size = sb.desc_size as usize;
    let count = sb.group_count as usize;

    // The block group descriptor table starts at the block after the
    // superblock block.
    let gdt_start = sb.group_desc_offset(0);

    // Total bytes needed for all descriptors.
    let total_bytes = count.saturating_mul(gd_size);
    let raw = reader.read_bytes(gdt_start, total_bytes)?;

    let mut descs = Vec::with_capacity(count);

    for i in 0..count {
        let offset = i.saturating_mul(gd_size);
        let end = offset.saturating_add(gd_size).min(raw.len());
        let slice = raw.get(offset..end).ok_or(KernelError::IoError)?;

        // We always parse a full 64-byte Ext4GroupDesc.
        // If desc_size is 32, the high fields will be zero (padding).
        let mut buf = [0u8; 64];
        let copy_len = slice.len().min(64);
        if let Some(dest) = buf.get_mut(..copy_len) {
            if let Some(src) = slice.get(..copy_len) {
                dest.copy_from_slice(src);
            }
        }

        let gd = read_struct::<Ext4GroupDesc>(&buf)?;
        descs.push(gd);
    }

    Ok(descs)
}

// ---------------------------------------------------------------------------
// Directory entry parsing
// ---------------------------------------------------------------------------

/// Parse linear directory entries from raw directory block data.
fn parse_dir_entries(data: &[u8]) -> KernelResult<Vec<(u32, u8, String)>> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    let dir_entry_header_size = core::mem::size_of::<Ext4DirEntry2>();

    while offset.saturating_add(dir_entry_header_size) <= data.len() {
        let hdr_bytes = data.get(offset..offset.saturating_add(dir_entry_header_size))
            .ok_or(KernelError::IoError)?;
        let hdr = read_struct::<Ext4DirEntry2>(hdr_bytes)?;

        if hdr.rec_len == 0 {
            // End of directory block.
            break;
        }

        if hdr.inode != 0 && hdr.name_len > 0 {
            let name_start = offset.saturating_add(dir_entry_header_size);
            let name_end = name_start.saturating_add(hdr.name_len as usize);
            if name_end <= data.len() {
                if let Some(name_bytes) = data.get(name_start..name_end) {
                    let name = String::from_utf8_lossy(name_bytes).into_owned();
                    entries.push((hdr.inode, hdr.file_type, name));
                }
            }
        }

        offset = offset.saturating_add(hdr.rec_len as usize);
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Utility: read a #[repr(C)] struct from a byte slice
// ---------------------------------------------------------------------------

/// Read a `#[repr(C)]` struct from raw bytes, handling alignment.
///
/// Copies bytes into an aligned local to avoid UB from unaligned reads.
/// Public so sibling modules can use it for on-disk structure parsing.
pub fn read_struct_pub<T: Copy>(data: &[u8]) -> KernelResult<T> {
    read_struct(data)
}

/// Read a `#[repr(C)]` struct from raw bytes, handling alignment.
///
/// Copies bytes into an aligned local to avoid UB from unaligned reads.
fn read_struct<T: Copy>(data: &[u8]) -> KernelResult<T> {
    let size = core::mem::size_of::<T>();
    if data.len() < size {
        return Err(KernelError::IoError);
    }

    // SAFETY: We copy exactly `size` bytes into a MaybeUninit<T>.
    // T is Copy and #[repr(C)], so any bit pattern from the disk
    // is a valid representation (all fields are integer types).
    unsafe {
        let mut val = core::mem::MaybeUninit::<T>::uninit();
        core::ptr::copy_nonoverlapping(
            data.as_ptr(),
            val.as_mut_ptr().cast::<u8>(),
            size,
        );
        Ok(val.assume_init())
    }
}

// ---------------------------------------------------------------------------
// Extended attribute key helpers
// ---------------------------------------------------------------------------

/// Build a full xattr key from a namespace index and name.
///
/// For example, index=1 + name="myattr" → "user.myattr".
fn xattr_full_key(name_index: u8, name: &str) -> String {
    use super::ondisk::xattr_index;
    match name_index {
        xattr_index::USER => {
            let mut key = String::from("user.");
            key.push_str(name);
            key
        }
        xattr_index::TRUSTED => {
            let mut key = String::from("trusted.");
            key.push_str(name);
            key
        }
        xattr_index::SECURITY => {
            let mut key = String::from("security.");
            key.push_str(name);
            key
        }
        xattr_index::SYSTEM => {
            let mut key = String::from("system.");
            key.push_str(name);
            key
        }
        _ => {
            // Unknown namespace — store with raw index prefix.
            String::from(name)
        }
    }
}

/// Split a full xattr key into namespace index and bare name.
///
/// For example, "user.myattr" → (1, "myattr").
/// Unknown prefixes get index 0 (raw).
fn xattr_split_key(key: &str) -> (u8, &str) {
    use super::ondisk::xattr_index;
    if let Some(rest) = key.strip_prefix("user.") {
        (xattr_index::USER, rest)
    } else if let Some(rest) = key.strip_prefix("trusted.") {
        (xattr_index::TRUSTED, rest)
    } else if let Some(rest) = key.strip_prefix("security.") {
        (xattr_index::SECURITY, rest)
    } else if let Some(rest) = key.strip_prefix("system.") {
        (xattr_index::SYSTEM, rest)
    } else {
        (xattr_index::NONE, key)
    }
}

// ---------------------------------------------------------------------------
// Inode byte helpers
// ---------------------------------------------------------------------------

/// Reinterpret the inode's i_block field as a byte slice.
///
/// The i_block field is 15 * u32 = 60 bytes, which holds either
/// block pointers (ext2) or an extent tree (ext4).
pub fn inode_block_as_bytes(inode: &Ext4Inode) -> &[u8] {
    // SAFETY: i_block is [u32; 15] inside a repr(C) struct.
    // Reinterpreting as bytes is safe on any platform.
    let ptr = inode.i_block.as_ptr().cast::<u8>();
    let len = core::mem::size_of_val(&inode.i_block);
    // SAFETY: ptr is valid for len bytes (it's part of the struct),
    // and the lifetime is tied to `inode`.
    unsafe { core::slice::from_raw_parts(ptr, len) }
}

/// Reinterpret the inode's i_block field as a mutable byte slice.
///
/// Used for writing fast symlinks (target stored directly in i_block).
pub fn inode_block_as_bytes_mut(inode: &mut Ext4Inode) -> &mut [u8] {
    let ptr = inode.i_block.as_mut_ptr().cast::<u8>();
    let len = core::mem::size_of_val(&inode.i_block);
    // SAFETY: Same as inode_block_as_bytes, but mutable.
    // The mutable borrow of `inode` ensures exclusive access.
    unsafe { core::slice::from_raw_parts_mut(ptr, len) }
}

/// Reinterpret a `#[repr(C)]` struct as a byte slice.
///
/// Used for writing structs to disk.
fn struct_as_bytes<T: Copy>(val: &T) -> &[u8] {
    let ptr = (val as *const T).cast::<u8>();
    let len = core::mem::size_of::<T>();
    // SAFETY: T is repr(C) and Copy.  The pointer is valid for the
    // lifetime of `val`, and we read exactly `size_of::<T>()` bytes.
    unsafe { core::slice::from_raw_parts(ptr, len) }
}

/// Create a blank (zeroed) inode.
fn blank_inode() -> Ext4Inode {
    // SAFETY: Ext4Inode is repr(C) with all-integer fields.
    // Zero-initialization is a valid state (empty inode).
    unsafe { core::mem::zeroed() }
}

// ---------------------------------------------------------------------------
// Directory entry helpers (for write path)
// ---------------------------------------------------------------------------

/// Find an insertion point in a directory block where a new entry of
/// `needed_size` bytes can fit.
///
/// Walks the directory entries in the block, looking for a gap between
/// the actual size of the last entry and its rec_len.  Returns the
/// byte offset where the new entry should be written, or `None` if
/// no space is available.
fn find_dir_insert_point(
    data: &[u8],
    block_start: usize,
    block_size: usize,
    needed_size: usize,
) -> Option<usize> {
    let block_end = block_start.saturating_add(block_size);
    let entry_header_size = core::mem::size_of::<Ext4DirEntry2>();
    let mut offset = block_start;
    let mut last_offset = block_start;
    let mut last_actual_size = 0usize;
    let mut last_rec_len = 0u16;

    // Walk all entries in this block.
    while offset.saturating_add(entry_header_size) <= block_end {
        let hdr_bytes = data.get(offset..offset.saturating_add(entry_header_size))?;
        let hdr = read_struct::<Ext4DirEntry2>(hdr_bytes).ok()?;

        if hdr.rec_len == 0 {
            break;
        }

        // The actual size of this entry (header + name, 4-byte aligned).
        let actual = if hdr.inode == 0 {
            // Deleted entry — the whole rec_len is free.
            0
        } else {
            let name_total = entry_header_size.saturating_add(hdr.name_len as usize);
            (name_total.saturating_add(3)) & !3
        };

        last_offset = offset;
        last_actual_size = actual;
        last_rec_len = hdr.rec_len;

        offset = offset.saturating_add(hdr.rec_len as usize);
    }

    // Check if there's space after the last entry.
    if last_rec_len as usize > last_actual_size {
        let free_space = (last_rec_len as usize).saturating_sub(last_actual_size);
        if free_space >= needed_size {
            return Some(last_offset.saturating_add(last_actual_size));
        }
    }

    None
}

/// Insert a directory entry by splitting the space at `offset`.
///
/// `remaining_in_block` is the number of bytes from `offset` to the
/// end of the block (used for the new entry's rec_len).
fn insert_dir_entry(
    data: &mut [u8],
    offset: usize,
    child_ino: u32,
    name: &[u8],
    file_type_byte: u8,
    remaining_in_block: usize,
) {
    // First, shrink the previous entry's rec_len.
    // The previous entry ends at `offset`, so find it and update its rec_len.
    let entry_header_size = core::mem::size_of::<Ext4DirEntry2>();

    // Walk backwards from offset to find the previous entry.
    // Since we know `offset` is the correct insertion point (from
    // find_dir_insert_point), the previous entry starts at some earlier
    // offset.  We need to update its rec_len.
    // Actually, find_dir_insert_point returns last_offset + last_actual_size.
    // So the previous entry is at last_offset.  We need to set its rec_len
    // to last_actual_size.

    // For now, we find the entry just before `offset` by scanning.
    // This is O(n) but directories are typically small.
    let block_start = (offset / remaining_in_block.max(1)) * remaining_in_block.max(1);

    // Actually, the simplest approach: we know the previous entry should have
    // rec_len equal to (offset - prev_entry_start).  But since we computed
    // the insertion point from find_dir_insert_point, let's just update the
    // previous rec_len to point exactly to our insertion offset.

    // Scan to find the entry whose rec_len reaches past `offset`.
    let mut pos = block_start.min(offset);
    // Only scan if we have a valid block start
    if pos < offset {
        while pos.saturating_add(entry_header_size) <= offset {
            if let Some(bytes) = data.get(pos..pos.saturating_add(entry_header_size)) {
                if let Ok(hdr) = read_struct::<Ext4DirEntry2>(bytes) {
                    let next = pos.saturating_add(hdr.rec_len as usize);
                    if next > offset || hdr.rec_len == 0 {
                        // This is the entry we need to shrink.
                        let new_rec_len = (offset.saturating_sub(pos)) as u16;
                        if let Some(rl_bytes) = data.get_mut(
                            pos.saturating_add(4)..pos.saturating_add(6)
                        ) {
                            rl_bytes.copy_from_slice(&new_rec_len.to_le_bytes());
                        }
                        break;
                    }
                    pos = next;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // Write the new entry at `offset`.
    write_dir_entry_raw(
        data,
        offset,
        child_ino,
        name,
        file_type_byte,
        remaining_in_block,
    );
}

/// Write a raw directory entry at the given offset.
fn write_dir_entry_raw(
    buf: &mut [u8],
    offset: usize,
    inode: u32,
    name: &[u8],
    file_type_byte: u8,
    rec_len: usize,
) {
    // inode (4 bytes, LE)
    if let Some(dest) = buf.get_mut(offset..offset.saturating_add(4)) {
        dest.copy_from_slice(&inode.to_le_bytes());
    }
    // rec_len (2 bytes, LE)
    if let Some(dest) = buf.get_mut(
        offset.saturating_add(4)..offset.saturating_add(6)
    ) {
        dest.copy_from_slice(&(rec_len as u16).to_le_bytes());
    }
    // name_len (1 byte)
    if let Some(b) = buf.get_mut(offset.saturating_add(6)) {
        *b = name.len() as u8;
    }
    // file_type (1 byte)
    if let Some(b) = buf.get_mut(offset.saturating_add(7)) {
        *b = file_type_byte;
    }
    // name (variable length)
    let name_start = offset.saturating_add(8);
    let name_end = name_start.saturating_add(name.len());
    if name_end <= buf.len() {
        if let Some(dest) = buf.get_mut(name_start..name_end) {
            dest.copy_from_slice(name);
        }
    }
}
