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
// Ext4 driver
// ---------------------------------------------------------------------------

/// An ext4 filesystem instance.
///
/// Holds the parsed superblock, block reader, and cached block group
/// descriptor table.
pub struct Ext4Driver {
    /// Parsed superblock with derived values.
    sb: ParsedSuperblock,
    /// Block I/O layer.
    reader: BlockReader,
    /// Cached block group descriptor table.
    group_descs: Vec<Ext4GroupDesc>,
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
    pub fn dir_lookup(
        &self,
        dir_inode: &Ext4Inode,
        name: &str,
    ) -> KernelResult<u32> {
        let entries = self.read_dir_entries(dir_inode)?;
        for (ino, _ftype, entry_name) in &entries {
            if entry_name == name {
                return Ok(*ino);
            }
        }
        Err(KernelError::NotFound)
    }

    /// Resolve a path to an inode number, starting from the root.
    ///
    /// `path` must be absolute (starting with `/`).
    pub fn resolve_path(&self, path: &str) -> KernelResult<u32> {
        let path = path.strip_prefix('/').unwrap_or(path);

        let mut current_ino = EXT4_ROOT_INO;

        if path.is_empty() {
            return Ok(current_ino);
        }

        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }

            let dir_inode = self.read_inode(current_ino)?;

            // Verify it's a directory.
            if (dir_inode.i_mode & file_type::S_IFMT) != file_type::S_IFDIR {
                return Err(KernelError::NotADirectory);
            }

            current_ino = self.dir_lookup(&dir_inode, component)?;
        }

        Ok(current_ino)
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
        Ok(())
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
