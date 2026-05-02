//! ext4 filesystem driver — core read logic.
//!
//! Ties together the superblock parser, block I/O, block group descriptor
//! reading, and inode lookup.  This is the main entry point for mounting
//! and reading an ext4 filesystem.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use spin::Mutex;

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
// Extent cache — avoids re-walking the extent tree for sequential reads
// ---------------------------------------------------------------------------

/// Number of entries in the extent range cache.
///
/// Caches `(inode, logical_block_start) → (physical_block_start, length)`
/// so that sequential reads within the same extent range don't need to
/// walk the extent tree from scratch.  256 entries covers typical workloads
/// (multiple files being read concurrently, each with several extents).
pub(super) const EXTENT_CACHE_SIZE: usize = 256;

/// A cached extent range mapping.
struct ExtentCacheEntry {
    /// Inode number this extent belongs to.
    inode: u32,
    /// Starting logical block of the extent.
    logical_start: u64,
    /// Starting physical block of the extent.
    physical_start: u64,
    /// Number of contiguous blocks in the extent.
    length: u64,
    /// LRU access counter.
    last_access: u64,
    /// Whether this entry is valid.
    valid: bool,
}

impl ExtentCacheEntry {
    const fn empty() -> Self {
        Self {
            inode: 0,
            logical_start: 0,
            physical_start: 0,
            length: 0,
            last_access: 0,
            valid: false,
        }
    }
}

/// Interior-mutable state for the extent range cache.
struct ExtentCacheInner {
    entries: Vec<ExtentCacheEntry>,
    counter: u64,
    hits: u64,
    misses: u64,
}

/// Extent range cache for ext4.
///
/// When `lookup_physical_block()` finds a mapping by walking the extent
/// tree, we cache the full extent range.  Subsequent lookups for the same
/// inode check the cache first — if the logical block falls within a
/// cached extent, we compute the physical block with zero disk I/O.
///
/// This is especially effective for sequential reads (reading a file from
/// start to end), where every block in the same extent hits the cache.
///
/// Interior-mutable via a spin mutex so that `lookup_physical_block()`
/// can update the cache even through a `&self` reference (the htree
/// module calls it via immutable borrows).
pub(super) struct ExtentCache {
    inner: Mutex<ExtentCacheInner>,
}

impl ExtentCache {
    fn new() -> Self {
        let mut entries = Vec::with_capacity(EXTENT_CACHE_SIZE);
        for _ in 0..EXTENT_CACHE_SIZE {
            entries.push(ExtentCacheEntry::empty());
        }
        Self {
            inner: Mutex::new(ExtentCacheInner {
                entries,
                counter: 0,
                hits: 0,
                misses: 0,
            }),
        }
    }

    /// Look up a physical block for (inode, logical_block).
    ///
    /// Returns `Some(physical_block)` if the logical block falls within
    /// a cached extent range.  Interior-mutable: safe to call through
    /// `&self` (acquires spin lock internally).
    fn lookup(&self, inode: u32, logical_block: u64) -> Option<u64> {
        let mut inner = self.inner.lock();

        // Phase 1: immutable scan to find matching entry index + result.
        let found = inner.entries.iter()
            .enumerate()
            .find(|(_, e)| {
                e.valid
                    && e.inode == inode
                    && logical_block >= e.logical_start
                    && logical_block < e.logical_start.saturating_add(e.length)
            })
            .map(|(i, e)| {
                let offset = logical_block.saturating_sub(e.logical_start);
                (i, e.physical_start.saturating_add(offset))
            });

        // Phase 2: mutable update of LRU counter and stats.
        match found {
            Some((idx, phys)) => {
                inner.counter = inner.counter.wrapping_add(1);
                let c = inner.counter;
                inner.hits = inner.hits.wrapping_add(1);
                if let Some(e) = inner.entries.get_mut(idx) {
                    e.last_access = c;
                }
                Some(phys)
            }
            None => {
                inner.misses = inner.misses.wrapping_add(1);
                None
            }
        }
    }

    /// Insert a full extent range into the cache.
    ///
    /// Interior-mutable: safe to call through `&self`.
    fn insert(
        &self,
        inode: u32,
        logical_start: u64,
        physical_start: u64,
        length: u64,
    ) {
        let mut inner = self.inner.lock();
        inner.counter = inner.counter.wrapping_add(1);
        let c = inner.counter;

        // Check for existing entry covering the same range (update).
        let existing = inner.entries.iter()
            .position(|e| e.valid && e.inode == inode && e.logical_start == logical_start);

        if let Some(idx) = existing {
            if let Some(e) = inner.entries.get_mut(idx) {
                e.physical_start = physical_start;
                e.length = length;
                e.last_access = c;
            }
            return;
        }

        // Find empty slot.
        let empty = inner.entries.iter().position(|e| !e.valid);

        if let Some(idx) = empty {
            if let Some(e) = inner.entries.get_mut(idx) {
                e.inode = inode;
                e.logical_start = logical_start;
                e.physical_start = physical_start;
                e.length = length;
                e.last_access = c;
                e.valid = true;
            }
            return;
        }

        // Evict LRU: find entry with lowest last_access.
        let lru_idx = inner.entries.iter()
            .enumerate()
            .min_by_key(|(_, e)| e.last_access)
            .map(|(i, _)| i)
            .unwrap_or(0);

        if let Some(e) = inner.entries.get_mut(lru_idx) {
            e.inode = inode;
            e.logical_start = logical_start;
            e.physical_start = physical_start;
            e.length = length;
            e.last_access = c;
            e.valid = true;
        }
    }

    /// Invalidate all cached extents for a given inode.
    ///
    /// Called when the inode's extent tree changes (writes, truncate).
    /// Interior-mutable: safe to call through `&self`.
    fn invalidate_inode(&self, inode: u32) {
        let mut inner = self.inner.lock();
        for entry in inner.entries.iter_mut() {
            if entry.valid && entry.inode == inode {
                entry.valid = false;
            }
        }
    }

    /// Return (hits, misses, valid_count).
    fn stats(&self) -> (u64, u64, usize) {
        let inner = self.inner.lock();
        let valid = inner.entries.iter().filter(|e| e.valid).count();
        (inner.hits, inner.misses, valid)
    }
}

// ---------------------------------------------------------------------------
// Inode cache — avoids re-reading inodes from disk on repeated access
// ---------------------------------------------------------------------------

/// Number of entries in the inode cache.
///
/// Path resolution, directory lookups, and file metadata calls read the
/// same inodes repeatedly (especially directory inodes along a path).
/// 128 entries covers typical working sets with room for multiple open
/// files and their parent directories.
pub(super) const INODE_CACHE_SIZE: usize = 128;

/// A cached inode entry.
struct InodeCacheEntry {
    /// Inode number (cache key).
    inode_nr: u32,
    /// The cached inode data (128 bytes).
    inode: Ext4Inode,
    /// LRU access counter.
    last_access: u64,
    /// Whether this entry is valid.
    valid: bool,
}

/// Interior-mutable state for the inode cache.
struct InodeCacheInner {
    entries: Vec<InodeCacheEntry>,
    counter: u64,
    hits: u64,
    misses: u64,
}

/// Inode cache for ext4.
///
/// Caches recently read `Ext4Inode` structures keyed by inode number.
/// This eliminates redundant disk reads during path resolution (which
/// reads each parent directory's inode) and repeated metadata queries.
///
/// Interior-mutable via `spin::Mutex` for use through `&self` references.
///
/// Cache coherency: `write_inode()` invalidates the cached entry so
/// subsequent reads pick up the on-disk changes.
pub(super) struct InodeCache {
    inner: Mutex<InodeCacheInner>,
}

impl InodeCache {
    fn new() -> Self {
        let mut entries = Vec::with_capacity(INODE_CACHE_SIZE);
        for _ in 0..INODE_CACHE_SIZE {
            let entry = InodeCacheEntry {
                inode_nr: 0,
                // SAFETY: Ext4Inode is 128 bytes of integers, all-zero is a valid
                // (if meaningless) inode.  We only use entries where valid == true.
                inode: unsafe { core::mem::zeroed() },
                last_access: 0,
                valid: false,
            };
            entries.push(entry);
        }
        Self {
            inner: Mutex::new(InodeCacheInner {
                entries,
                counter: 0,
                hits: 0,
                misses: 0,
            }),
        }
    }

    /// Look up a cached inode by number.
    ///
    /// Returns a copy of the inode if cached, or `None` on miss.
    fn lookup(&self, inode_nr: u32) -> Option<Ext4Inode> {
        let mut inner = self.inner.lock();
        let found = inner.entries.iter()
            .enumerate()
            .find(|(_, e)| e.valid && e.inode_nr == inode_nr)
            .map(|(i, e)| (i, e.inode));

        match found {
            Some((idx, inode)) => {
                inner.counter = inner.counter.wrapping_add(1);
                let c = inner.counter;
                inner.hits = inner.hits.wrapping_add(1);
                if let Some(e) = inner.entries.get_mut(idx) {
                    e.last_access = c;
                }
                Some(inode)
            }
            None => {
                inner.misses = inner.misses.wrapping_add(1);
                None
            }
        }
    }

    /// Insert or update a cached inode.
    fn insert(&self, inode_nr: u32, inode: &Ext4Inode) {
        let mut inner = self.inner.lock();
        inner.counter = inner.counter.wrapping_add(1);
        let c = inner.counter;

        // Check for existing entry (update in place).
        let existing = inner.entries.iter()
            .position(|e| e.valid && e.inode_nr == inode_nr);
        if let Some(idx) = existing {
            if let Some(e) = inner.entries.get_mut(idx) {
                e.inode = *inode;
                e.last_access = c;
            }
            return;
        }

        // Find empty slot.
        let empty = inner.entries.iter().position(|e| !e.valid);
        if let Some(idx) = empty {
            if let Some(e) = inner.entries.get_mut(idx) {
                e.inode_nr = inode_nr;
                e.inode = *inode;
                e.last_access = c;
                e.valid = true;
            }
            return;
        }

        // Evict LRU.
        let lru_idx = inner.entries.iter()
            .enumerate()
            .min_by_key(|(_, e)| e.last_access)
            .map(|(i, _)| i)
            .unwrap_or(0);

        if let Some(e) = inner.entries.get_mut(lru_idx) {
            e.inode_nr = inode_nr;
            e.inode = *inode;
            e.last_access = c;
            e.valid = true;
        }
    }

    /// Invalidate a cached inode.
    ///
    /// Used when an inode is freed/deleted and should no longer be served
    /// from cache.  For normal writes, `insert` is preferred (updates the
    /// cache with the new data rather than forcing a re-read).
    #[allow(dead_code)]
    fn invalidate(&self, inode_nr: u32) {
        let mut inner = self.inner.lock();
        for entry in inner.entries.iter_mut() {
            if entry.valid && entry.inode_nr == inode_nr {
                entry.valid = false;
                return;
            }
        }
    }

    /// Return (hits, misses, valid_count).
    fn stats(&self) -> (u64, u64, usize) {
        let inner = self.inner.lock();
        let valid = inner.entries.iter().filter(|e| e.valid).count();
        (inner.hits, inner.misses, valid)
    }
}

// ---------------------------------------------------------------------------
// Ext4 driver
// ---------------------------------------------------------------------------

/// An ext4 filesystem instance.
///
/// Holds the parsed superblock, block reader, cached block group
/// descriptor table, directory entry cache, extent range cache, and
/// inode cache.
pub struct Ext4Driver {
    /// Parsed superblock with derived values.
    sb: ParsedSuperblock,
    /// Block I/O layer.
    reader: BlockReader,
    /// Cached block group descriptor table.
    group_descs: Vec<Ext4GroupDesc>,
    /// Directory entry cache for fast name→inode lookups.
    pub(super) dcache: Ext4Dcache,
    /// Extent range cache for fast logical→physical block mapping.
    extent_cache: ExtentCache,
    /// Inode cache for fast repeated inode reads.
    inode_cache: InodeCache,
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

        let mut driver = Self {
            sb,
            reader,
            group_descs,
            dcache: Ext4Dcache::new(),
            extent_cache: ExtentCache::new(),
            inode_cache: InodeCache::new(),
        };

        // Step 5: Journal recovery.
        //
        // If the filesystem has a journal and the RECOVER incompat flag is
        // set, committed transactions may not have been written to their
        // final locations before the previous unmount (crash, power loss).
        // Replay them now to restore consistency before allowing access.
        driver.recover_journal_if_needed()?;

        Ok(driver)
    }

    /// Access the parsed superblock.
    #[must_use]
    pub fn superblock(&self) -> &ParsedSuperblock {
        &self.sb
    }

    /// Invalidate all cached extent mappings for an inode.
    ///
    /// Must be called whenever an inode's extent tree is modified
    /// (e.g., `write_file_data`, `truncate`, file deletion) so that
    /// stale physical block mappings are not returned from the cache.
    pub(super) fn invalidate_extent_cache(&self, inode_nr: u32) {
        self.extent_cache.invalidate_inode(inode_nr);
    }

    /// Return extent cache statistics: (hits, misses, valid_entries).
    pub(super) fn extent_cache_stats(&self) -> (u64, u64, usize) {
        self.extent_cache.stats()
    }

    /// Return inode cache statistics: (hits, misses, valid_entries).
    pub(super) fn inode_cache_stats(&self) -> (u64, u64, usize) {
        self.inode_cache.stats()
    }

    // -----------------------------------------------------------------------
    // Journal recovery
    // -----------------------------------------------------------------------

    /// Check if the filesystem needs journal recovery and perform it.
    ///
    /// Called during `open()` after the driver is constructed but before
    /// it's returned to callers.  If the RECOVER incompat flag is set,
    /// the filesystem was not cleanly unmounted and the journal may
    /// contain committed transactions that need replaying.
    ///
    /// After successful replay, clears the RECOVER flag and writes the
    /// superblock back to disk so subsequent mounts don't re-replay.
    fn recover_journal_if_needed(&mut self) -> KernelResult<()> {
        use super::ondisk::incompat;

        // No journal → nothing to do.
        if !self.sb.has_journal {
            return Ok(());
        }

        let needs_recovery =
            (self.sb.raw.s_feature_incompat & incompat::RECOVER) != 0;

        if !needs_recovery {
            serial_println!("[ext4] Journal present, no recovery needed.");
            return Ok(());
        }

        serial_println!("[ext4] RECOVER flag set — replaying journal...");

        // Read the journal inode.
        let journal_ino = self.sb.raw.s_journal_inum;
        if journal_ino == 0 {
            serial_println!("[ext4] Warning: RECOVER set but s_journal_inum=0, skipping.");
            return Ok(());
        }

        let journal_inode = self.read_inode(journal_ino)?;

        // Resolve the journal inode's extent tree to a flat list of
        // physical block numbers.  The journal module needs this to
        // map journal-relative offsets to device blocks.
        let journal_blocks = self.resolve_inode_block_list(journal_ino, &journal_inode)?;
        if journal_blocks.is_empty() {
            serial_println!("[ext4] Warning: journal inode has no blocks, skipping recovery.");
            return Ok(());
        }

        serial_println!(
            "[ext4] Journal inode {}: {} blocks",
            journal_ino, journal_blocks.len()
        );

        // Open the journal and replay any committed transactions.
        let mut journal = super::journal::Journal::open(
            &self.reader,
            journal_ino,
            journal_blocks,
            self.sb.block_size,
        )?;

        let replayed = journal.replay()?;

        if replayed > 0 {
            serial_println!(
                "[ext4] Journal recovery complete: {} blocks replayed.",
                replayed
            );

            // Re-read the block group descriptors — journal replay may
            // have updated them.
            self.group_descs = read_group_descs(&self.sb, &self.reader)?;
        } else {
            serial_println!("[ext4] Journal was clean, no blocks to replay.");
        }

        // Clear the RECOVER flag so subsequent mounts skip replay.
        self.sb.raw.s_feature_incompat &= !incompat::RECOVER;
        self.write_superblock()?;
        self.reader.flush()?;

        serial_println!("[ext4] RECOVER flag cleared.");
        Ok(())
    }

    /// Resolve an inode's extent tree to a flat, ordered list of physical
    /// block numbers.
    ///
    /// Used to map the journal inode's logical blocks to physical device
    /// blocks.  Walks the extent tree and expands each extent into
    /// individual block numbers in logical order.
    fn resolve_inode_block_list(&self, inode_nr: u32, inode: &Ext4Inode) -> KernelResult<Vec<u64>> {
        let file_size = self.inode_size(inode);
        let block_size = u64::from(self.sb.block_size);
        if file_size == 0 || block_size == 0 {
            return Ok(Vec::new());
        }

        let total_blocks = file_size.saturating_add(block_size - 1) / block_size;
        let mut blocks = Vec::with_capacity(total_blocks as usize);

        // We can't use lookup_physical_block here because it requires
        // an inode_nr for caching, and we don't want to pollute the
        // extent cache with journal inode entries.  Walk the tree directly
        // using collect_extent_blocks ranges instead.
        //
        // Build a sorted list of (logical_start, phys_start, len) from
        // the extent tree leaves.
        let leaf_extents = self.collect_leaf_extents(inode_nr, inode)?;

        for logical_block in 0..total_blocks {
            // Binary search in the sorted leaf extents for the range
            // containing this logical block.
            let phys = find_in_leaf_extents(&leaf_extents, logical_block);
            match phys {
                Some(p) => blocks.push(p),
                None => {
                    // Sparse hole in the journal — shouldn't happen but
                    // use 0 as a sentinel (journal code will error if
                    // it tries to read block 0).
                    blocks.push(0);
                }
            }
        }

        Ok(blocks)
    }

    /// Collect leaf extents from an inode's extent tree, returning
    /// (logical_start, physical_start, length) tuples sorted by logical
    /// block number.
    ///
    /// Unlike `collect_extent_blocks`, this only returns data extents
    /// (not index node blocks) and preserves the logical block mapping.
    fn collect_leaf_extents(
        &self,
        inode_nr: u32,
        inode: &Ext4Inode,
    ) -> KernelResult<Vec<(u64, u64, u64)>> {
        let mut result = Vec::new();

        if (inode.i_flags & inode_flags::EXTENTS) == 0 {
            return Ok(result);
        }

        let block_bytes = inode_block_as_bytes(inode);
        let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;
        if header.eh_magic != EXT4_EXTENT_MAGIC || header.eh_entries == 0 {
            return Ok(result);
        }

        let ino_seed = inode_csum_seed(&self.sb, inode_nr, inode.i_generation);
        self.collect_leaf_extents_recursive(ino_seed, block_bytes, &header, &mut result)?;

        // Sort by logical start block for binary search.
        result.sort_by_key(|&(logical, _, _)| logical);
        Ok(result)
    }

    /// Recursively walk extent tree nodes, collecting only leaf extents
    /// with their logical block mappings.
    fn collect_leaf_extents_recursive(
        &self,
        ino_seed: u32,
        node_data: &[u8],
        header: &Ext4ExtentHeader,
        result: &mut Vec<(u64, u64, u64)>,
    ) -> KernelResult<()> {
        let header_size = core::mem::size_of::<Ext4ExtentHeader>();

        if header.eh_depth == 0 {
            // Leaf — collect (logical, physical, length) tuples.
            let extent_size = core::mem::size_of::<Ext4Extent>();
            for i in 0..header.eh_entries as usize {
                let offset = header_size.saturating_add(i.saturating_mul(extent_size));
                let ext_bytes = node_data
                    .get(offset..offset.saturating_add(extent_size))
                    .ok_or(KernelError::IoError)?;
                let extent = read_struct::<Ext4Extent>(ext_bytes)?;

                let logical = u64::from(extent.ee_block);
                let phys = u64::from(extent.ee_start_lo)
                    | (u64::from(extent.ee_start_hi) << 32);
                let len = u64::from(extent.ee_len & 0x7FFF);

                if phys != 0 && len > 0 {
                    result.push((logical, phys, len));
                }
            }
        } else {
            // Internal node — recurse into children.
            let idx_size = core::mem::size_of::<super::ondisk::Ext4ExtentIdx>();
            let block_size = self.sb.block_size as usize;

            for i in 0..header.eh_entries as usize {
                let offset = header_size.saturating_add(i.saturating_mul(idx_size));
                let idx_bytes = node_data
                    .get(offset..offset.saturating_add(idx_size))
                    .ok_or(KernelError::IoError)?;
                let idx = read_struct::<super::ondisk::Ext4ExtentIdx>(idx_bytes)?;

                let child_block = u64::from(idx.ei_leaf_lo)
                    | (u64::from(idx.ei_leaf_hi) << 16);

                let mut child_data = vec![0u8; block_size];
                self.reader.read_block(child_block, &mut child_data)?;

                let child_header = read_struct::<Ext4ExtentHeader>(&child_data)?;
                if child_header.eh_magic != EXT4_EXTENT_MAGIC {
                    continue;
                }

                // Validate extent block checksum.
                validate_extent_block_checksum(
                    self.sb.has_metadata_csum,
                    ino_seed,
                    &child_data,
                    &child_header,
                )?;

                self.collect_leaf_extents_recursive(ino_seed, &child_data, &child_header, result)?;
            }
        }

        Ok(())
    }

    /// Read a single ext4 block by physical block number.
    ///
    /// Returns a newly allocated buffer containing the block data.
    /// Used by the htree module to read dx_root / dx_node / leaf blocks.
    pub(super) fn read_block(&self, phys_block: u64) -> KernelResult<Vec<u8>> {
        let bs = self.sb.block_size as usize;
        let mut buf = vec![0u8; bs];
        self.reader.read_block(phys_block, &mut buf)?;
        Ok(buf)
    }

    /// Map a logical block number to a physical block number for an inode.
    ///
    /// Wrapper around [`lookup_physical_block`] for the htree module.
    /// `inode_nr` is the inode number (needed for the extent cache key).
    pub(super) fn logical_to_physical(
        &self,
        inode_nr: u32,
        inode: &Ext4Inode,
        logical_block: u64,
    ) -> KernelResult<Option<u64>> {
        self.lookup_physical_block(inode_nr, inode, logical_block)
    }

    /// Read an inode by number.
    ///
    /// Checks the inode cache first.  On miss, reads from disk and
    /// inserts the result into the cache for future lookups.
    ///
    /// Inode numbers are 1-based (inode 0 is invalid, inode 2 is root).
    pub fn read_inode(&self, inode_nr: u32) -> KernelResult<Ext4Inode> {
        if inode_nr == 0 {
            return Err(KernelError::InvalidArgument);
        }

        // Fast path: check the inode cache.
        if let Some(inode) = self.inode_cache.lookup(inode_nr) {
            return Ok(inode);
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

        // Validate inode checksum (if metadata checksumming enabled).
        if self.sb.has_metadata_csum {
            validate_inode_checksum(
                &self.sb,
                inode_nr,
                &inode,
                &inode_bytes,
            )?;
        }

        // Cache for future lookups.
        self.inode_cache.insert(inode_nr, &inode);

        Ok(inode)
    }

    /// Read the raw on-disk bytes for an inode (all `sb.inode_size` bytes).
    ///
    /// This is used to access the inline xattr area that lives after the
    /// 128-byte core + i_extra_isize extra fields, up to the full inode
    /// size on disk.  The parsed [`Ext4Inode`] only covers the first 128
    /// bytes; this returns everything.
    pub fn read_inode_raw(&self, inode_nr: u32) -> KernelResult<Vec<u8>> {
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

        self.reader.read_bytes(inode_byte_offset, self.sb.inode_size as usize)
    }

    /// Parse inline xattrs from raw inode bytes.
    ///
    /// In ext4, xattrs can be stored in the inode body between
    /// `128 + i_extra_isize` and `inode_size`.  The inline area has a
    /// 4-byte magic header (`EXT4_XATTR_MAGIC`), followed by the same
    /// entry format as external xattr blocks.  Values grow backward from
    /// the end of the inline area.
    ///
    /// Returns an empty Vec if the inode has no inline xattr area or the
    /// magic doesn't match.
    ///
    /// Based on Linux `fs/ext4/xattr.c:ext4_xattr_ibody_list()`.
    fn parse_inline_xattrs(&self, raw: &[u8]) -> Vec<(String, Vec<u8>)> {
        // Inline xattrs require inode_size > 128 (need space for the area).
        let inode_size = self.sb.inode_size as usize;
        if inode_size <= 128 {
            return Vec::new();
        }

        // Read i_extra_isize from the raw bytes at offset 0x80 (2 bytes, LE).
        let i_extra_isize = raw.get(0x80..0x82)
            .and_then(|s| <[u8; 2]>::try_from(s).ok())
            .map_or(0u16, u16::from_le_bytes) as usize;

        // The inline xattr area starts at 128 + i_extra_isize.
        let ibody_start = 128usize.saturating_add(i_extra_isize);

        // Need at least 4 bytes for the magic header.
        if ibody_start.saturating_add(4) > inode_size || ibody_start.saturating_add(4) > raw.len() {
            return Vec::new();
        }

        // Check the magic number.
        let magic = raw.get(ibody_start..ibody_start.saturating_add(4))
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
            .map_or(0u32, u32::from_le_bytes);

        if magic != super::ondisk::EXT4_XATTR_MAGIC {
            return Vec::new(); // No inline xattrs.
        }

        let entry_header_size = core::mem::size_of::<super::ondisk::Ext4XattrEntry>();
        let entries_start = ibody_start.saturating_add(4); // After magic.
        let area_end = inode_size.min(raw.len());

        let mut result = Vec::new();
        let mut offset = entries_start;

        loop {
            // Check for end sentinel (4 zero bytes).
            if offset.saturating_add(4) > area_end {
                break;
            }
            if raw.get(offset..offset.saturating_add(4)) == Some(&[0, 0, 0, 0]) {
                break;
            }

            // Parse entry header.
            if offset.saturating_add(entry_header_size) > area_end {
                break;
            }
            let entry_bytes = match raw.get(offset..offset.saturating_add(entry_header_size)) {
                Some(b) => b,
                None => break,
            };
            let entry = match read_struct::<super::ondisk::Ext4XattrEntry>(entry_bytes) {
                Ok(e) => e,
                Err(_) => break,
            };

            // Read the name.
            let name_start = offset.saturating_add(entry_header_size);
            let name_end = name_start.saturating_add(entry.e_name_len as usize);
            let name = match raw.get(name_start..name_end) {
                Some(bytes) => match core::str::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(_) => break,
                },
                None => break,
            };

            let full_key = xattr_full_key(entry.e_name_index, name);

            // For inline xattrs, e_value_offs is relative to the first
            // entry position (entries_start), not the block start.
            let val_start = entries_start.saturating_add(entry.e_value_offs as usize);
            let val_end = val_start.saturating_add(entry.e_value_size as usize);
            let value = if entry.e_value_size > 0 && val_end <= area_end {
                raw.get(val_start..val_end)
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

        result
    }

    /// Read all xattrs for an inode: both inline (in-inode) and external block.
    ///
    /// Linux ext4 stores xattrs in two places:
    /// 1. **Inline** — in the inode body after `128 + i_extra_isize`, up to
    ///    `inode_size`.  Used for small attrs (e.g., security.selinux).
    /// 2. **External** — in a separate block pointed to by `i_file_acl`.
    ///    Used when the inline area is full.
    ///
    /// This method reads from both locations and merges them.  External
    /// attrs take precedence if the same key appears in both (shouldn't
    /// happen in practice, but defensive).
    pub fn read_all_xattrs(&self, inode_nr: u32, inode: &Ext4Inode) -> KernelResult<Vec<(String, Vec<u8>)>> {
        // Read inline xattrs first.
        let mut attrs = match self.read_inode_raw(inode_nr) {
            Ok(raw) => self.parse_inline_xattrs(&raw),
            Err(_) => Vec::new(),
        };

        // Read external xattr block.
        let block_nr = self.xattr_block(inode);
        if block_nr != 0 {
            let external = self.read_xattrs(inode)?;
            // Merge: external attrs override any inline attrs with the same key.
            for (key, value) in external {
                if let Some(existing) = attrs.iter_mut().find(|(k, _)| *k == key) {
                    existing.1 = value;
                } else {
                    attrs.push((key, value));
                }
            }
        }

        Ok(attrs)
    }

    /// Read the contents of a file given its inode.
    ///
    /// Supports both extent-based (modern ext4) and indirect-block-based
    /// (ext2/ext3 compatibility) inodes.
    pub fn read_file_data(&self, inode_nr: u32, inode: &Ext4Inode) -> KernelResult<Vec<u8>> {
        let file_size = self.inode_size(inode);

        if file_size == 0 {
            return Ok(Vec::new());
        }

        // Inline data: file content stored directly in the inode's i_block[].
        if (inode.i_flags & inode_flags::INLINE_DATA) != 0 {
            return self.read_inline_data(inode, file_size);
        }

        if (inode.i_flags & inode_flags::EXTENTS) != 0 {
            // Extent-based file.
            self.read_extent_data(inode_nr, inode, file_size)
        } else {
            // Indirect-block-based file (ext2/ext3 compat).
            self.read_indirect_data(inode, file_size)
        }
    }

    /// Read inline data from an inode's i_block[] field.
    ///
    /// ext4 inline data stores up to 60 bytes of file content directly
    /// in the inode's `i_block[0..14]` array (which is 60 bytes when
    /// interpreted as raw data instead of block pointers).
    ///
    /// Files larger than 60 bytes with inline data also store overflow
    /// in a "system.data" extended attribute, but we don't support that
    /// yet — only the first 60 bytes are handled.
    fn read_inline_data(&self, inode: &Ext4Inode, file_size: u64) -> KernelResult<Vec<u8>> {
        // i_block is [u32; 15] = 60 bytes of raw data.
        // Reinterpret as a byte array.
        let raw_bytes: &[u8] = {
            let ptr = inode.i_block.as_ptr().cast::<u8>();
            // SAFETY: i_block is 15 * 4 = 60 bytes, all initialized.
            // We're reinterpreting the same memory as bytes.
            unsafe { core::slice::from_raw_parts(ptr, 60) }
        };

        let data_len = file_size.min(60) as usize;
        let data = raw_bytes.get(..data_len)
            .ok_or(KernelError::IoError)?;

        Ok(Vec::from(data))
    }

    /// Read a byte range from a file's extent tree.
    ///
    /// Only reads the blocks that overlap `[offset, offset+len)`,
    /// avoiding reading the entire file for large-file partial reads.
    pub fn read_file_range(
        &self,
        inode_nr: u32,
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

        // Inline data: slice into the inode's i_block[] interpreted as bytes.
        if (inode.i_flags & inode_flags::INLINE_DATA) != 0 {
            let full = self.read_inline_data(inode, file_size)?;
            let start = offset as usize;
            let end = start.saturating_add(actual_len).min(full.len());
            return Ok(Vec::from(full.get(start..end).unwrap_or(&[])));
        }

        let block_size = u64::from(self.sb.block_size);
        let block_size_usize = self.sb.block_size as usize;

        if (inode.i_flags & inode_flags::EXTENTS) != 0 {
            // Extent-based: efficient tree walk.
            let first_logical = offset / block_size;
            let last_logical = (offset.saturating_add(actual_len as u64).saturating_sub(1)) / block_size;

            let block_bytes = inode_block_as_bytes(inode);
            let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;
            if header.eh_magic != EXT4_EXTENT_MAGIC {
                return Err(KernelError::IoError);
            }

            let ino_seed = inode_csum_seed(&self.sb, inode_nr, inode.i_generation);
            let mut result = Vec::with_capacity(actual_len);
            self.read_range_from_tree(
                ino_seed,
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
        } else {
            // Indirect-block-based: read each logical block via lookup.
            let first_logical = offset / block_size;
            let last_logical = (offset.saturating_add(actual_len as u64).saturating_sub(1)) / block_size;

            let mut result = Vec::with_capacity(actual_len);
            let mut block_buf = vec![0u8; block_size_usize];

            for logical in first_logical..=last_logical {
                let phys = self.lookup_indirect_block(inode, logical)?;
                match phys {
                    Some(p) => {
                        self.reader.read_block(p, &mut block_buf)?;
                    }
                    None => {
                        // Sparse hole — zero block.
                        for b in block_buf.iter_mut() {
                            *b = 0;
                        }
                    }
                }

                // Determine which portion of this block is relevant.
                let block_start_byte = logical * block_size;
                let copy_start = if block_start_byte < offset {
                    (offset - block_start_byte) as usize
                } else {
                    0
                };
                let copy_end = block_size_usize.min(
                    (offset.saturating_add(actual_len as u64) - block_start_byte) as usize,
                );

                if let Some(slice) = block_buf.get(copy_start..copy_end) {
                    result.extend_from_slice(slice);
                }
            }

            result.truncate(actual_len);
            Ok(result)
        }
    }

    /// Read directory entries from a directory inode.
    ///
    /// Returns a vector of (inode_number, file_type, name) tuples.
    /// If metadata checksums are enabled, validates each directory block's
    /// CRC32C checksum before parsing entries.
    pub fn read_dir_entries(
        &self,
        dir_ino: u32,
        dir_inode: &Ext4Inode,
    ) -> KernelResult<Vec<(u32, u8, String)>> {
        // Read directory data.
        let data = self.read_file_data(dir_ino, dir_inode)?;

        // Validate per-block checksums if metadata_csum is enabled.
        if self.sb.has_metadata_csum {
            let bs = self.sb.block_size as usize;
            if bs > 0 {
                let mut block_start: usize = 0;
                while block_start.saturating_add(bs) <= data.len() {
                    if let Some(block) = data.get(block_start..block_start.saturating_add(bs)) {
                        validate_dirent_checksum(
                            &self.sb,
                            dir_ino,
                            dir_inode.i_generation,
                            block,
                        )?;
                    }
                    block_start = block_start.saturating_add(bs);
                }
            }
        }

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
        // Check dcache first (fastest path — O(1) with no I/O).
        if let Some((child_ino, _ftype)) = self.dcache.lookup(dir_ino, name) {
            return Ok(child_ino);
        }

        // Try htree-accelerated lookup if the directory uses hash indexing.
        // This avoids reading all directory blocks for large directories.
        if dir_inode.i_flags & inode_flags::INDEX != 0 {
            if let Ok(Some((child_ino, ftype))) =
                super::htree::htree_lookup(self, dir_ino, name)
            {
                // Cache the result for next time.
                self.dcache.insert(dir_ino, name, child_ino, ftype);
                return Ok(child_ino);
            }
            // htree lookup failed or returned None — fall through to
            // linear scan as a fallback (htree may be corrupt or the
            // directory is being converted).
        }

        // Fallback: linear scan of all directory entries.
        let entries = self.read_dir_entries(dir_ino, dir_inode)?;
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
                let target = self.read_symlink_target(child_ino, &child_inode)?;
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
    pub fn read_symlink_target(&self, inode_nr: u32, inode: &Ext4Inode) -> KernelResult<Vec<u8>> {
        let size = self.inode_size(inode) as usize;

        if size <= 60 && (inode.i_flags & inode_flags::EXTENTS) == 0 {
            // Fast symlink: target stored directly in i_block.
            let block_bytes = inode_block_as_bytes(inode);
            let target = block_bytes.get(..size).ok_or(KernelError::IoError)?;
            Ok(target.to_vec())
        } else {
            // Slow symlink: target stored in data blocks.
            self.read_file_data(inode_nr, inode)
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

        // Build the full on-disk inode image.
        let inode_sz = self.sb.inode_size as usize;
        let core_bytes = struct_as_bytes(inode);

        if self.sb.has_metadata_csum && inode_sz > core_bytes.len() {
            // Read the existing full on-disk inode so we preserve the
            // extra area (creation time, checksum_hi, etc.).
            let mut buf = self.reader.read_bytes(inode_byte_offset, inode_sz)?;

            // Overwrite the core 128-byte portion with the new data.
            let copy_len = core_bytes.len().min(buf.len());
            if let (Some(dst), Some(src)) = (buf.get_mut(..copy_len), core_bytes.get(..copy_len)) {
                dst.copy_from_slice(src);
            }

            // Compute and embed the checksum.
            stamp_inode_checksum(&self.sb, inode_nr, inode, &mut buf);

            self.reader.write_bytes(inode_byte_offset, &buf)?;
        } else if self.sb.has_metadata_csum {
            // 128-byte inodes with metadata_csum: only lo checksum.
            let mut buf = Vec::from(core_bytes);
            stamp_inode_checksum(&self.sb, inode_nr, inode, &mut buf);
            self.reader.write_bytes(inode_byte_offset, &buf)?;
        } else {
            // No checksumming — write the core inode directly.
            self.reader.write_bytes(inode_byte_offset, core_bytes)?;
        }

        // Update the inode cache with the new data.
        self.inode_cache.insert(inode_nr, inode);

        Ok(())
    }

    /// Zero the full on-disk inode (including extra area beyond 128 bytes).
    ///
    /// Used when allocating a brand-new inode to prevent stale data from
    /// a previously deleted inode from persisting in the extra fields.
    fn zero_ondisk_inode(&self, inode_nr: u32) -> KernelResult<()> {
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
        let offset = inode_table_block
            .saturating_mul(u64::from(self.sb.block_size))
            .saturating_add(
                u64::from(index).saturating_mul(u64::from(self.sb.inode_size))
            );
        let zeros = vec![0u8; self.sb.inode_size as usize];
        self.reader.write_bytes(offset, &zeros)
    }

    /// Write the `i_extra_isize` field at offset 0x80 in the on-disk inode.
    ///
    /// This field is part of `Ext4InodeExtra` and tells how many bytes of
    /// extra data follow the 128-byte core.
    fn write_extra_isize(&self, inode_nr: u32, extra_isize: u16) -> KernelResult<()> {
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
        let inode_offset = inode_table_block
            .saturating_mul(u64::from(self.sb.block_size))
            .saturating_add(
                u64::from(index).saturating_mul(u64::from(self.sb.inode_size))
            );
        // i_extra_isize is at offset 0x80 (u16, little-endian).
        let field_offset = inode_offset.saturating_add(0x80);
        self.reader.write_bytes(field_offset, &extra_isize.to_le_bytes())
    }

    /// Write the superblock back to disk.
    ///
    /// If metadata checksumming is enabled, computes and embeds the CRC32C
    /// checksum before writing.  The superblock is at byte offset 1024
    /// from partition start.
    pub fn write_superblock(&self) -> KernelResult<()> {
        if self.sb.has_metadata_csum {
            let mut buf = Vec::from(struct_as_bytes(&self.sb.raw));
            stamp_superblock_checksum(&mut buf);
            self.reader.write_bytes(
                super::superblock::superblock_device_offset(),
                &buf,
            )
        } else {
            let sb_bytes = struct_as_bytes(&self.sb.raw);
            self.reader.write_bytes(
                super::superblock::superblock_device_offset(),
                sb_bytes,
            )
        }
    }

    /// Write all block group descriptors back to disk.
    ///
    /// If metadata checksumming is enabled, computes and embeds the CRC32C
    /// checksum for each descriptor before writing.
    pub fn write_group_descs(&self) -> KernelResult<()> {
        let gd_size = self.sb.desc_size as usize;
        let gdt_start = self.sb.group_desc_offset(0);

        for (i, gd) in self.group_descs.iter().enumerate() {
            let offset = gdt_start.saturating_add(
                (i as u64).saturating_mul(gd_size as u64)
            );
            let gd_bytes = struct_as_bytes(gd);
            let write_len = gd_bytes.len().min(gd_size);

            if self.sb.has_metadata_csum {
                // Copy descriptor bytes and stamp checksum.
                let source = gd_bytes.get(..write_len).unwrap_or(&[]);
                let mut buf = Vec::from(source);
                #[allow(clippy::cast_possible_truncation)]
                stamp_gd_checksum(&self.sb, i as u32, &mut buf);
                self.reader.write_bytes(offset, &buf)?;
            } else if let Some(data) = gd_bytes.get(..write_len) {
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
            set_inode_blocks_48(inode, 0);
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

        // Block count in 512-byte units (48-bit field).
        let sectors = (blocks_needed as u64)
            .saturating_mul(u64::from(self.sb.block_size / 512));
        set_inode_blocks_48(inode, sectors);

        Ok(())
    }

    /// Extend a file by appending data at the current end.
    ///
    /// Much more efficient than `write_file_data` for append operations
    /// on existing files: only allocates and writes the new blocks instead
    /// of reading and rewriting the entire file.
    ///
    /// Handles depth-0 extent trees natively (adding extents to the root)
    /// and depth>0 trees by finding the last leaf block and modifying it.
    /// Returns `Err(NotSupported)` only when the last leaf block is full
    /// and the new blocks are not adjacent (would require extent tree
    /// splitting, which is not yet implemented).
    ///
    /// `append_data` is the bytes to append starting at the current EOF.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub fn extend_file_data(
        &mut self,
        inode_nr: u32,
        inode: &mut Ext4Inode,
        append_data: &[u8],
    ) -> KernelResult<()> {
        if append_data.is_empty() {
            return Ok(());
        }

        let block_size = self.sb.block_size as usize;
        if block_size == 0 {
            return Err(KernelError::IoError);
        }
        let block_size_u64 = self.sb.block_size as u64;

        let current_size = {
            let lo = u64::from(inode.i_size_lo);
            let hi = u64::from(inode.i_size_high);
            lo | (hi << 32)
        };

        // Parse the existing extent tree root in the inode.
        let block_bytes = inode_block_as_bytes(inode);
        let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;

        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Err(KernelError::IoError);
        }

        // Collect all leaf extents from the tree (any depth).
        let ino_seed = inode_csum_seed(&self.sb, inode_nr, inode.i_generation);
        let mut all_extents: Vec<(u64, u64, u64)> = Vec::new();
        self.collect_leaf_extents_recursive(ino_seed, block_bytes, &header, &mut all_extents)?;

        // Sort by logical block number and find the last extent.
        all_extents.sort_by_key(|&(logical, _, _)| logical);
        let (last_logical_start, last_phys_start, last_block_count) =
            all_extents.last().copied().unwrap_or((0, 0, 0));

        // Calculate the partial block at EOF (if the file doesn't end on
        // a block boundary, we need to read-modify-write the last block).
        let tail_bytes_in_last_block = if current_size > 0 {
            let rem = current_size % block_size_u64;
            if rem == 0 { 0 } else { rem as usize }
        } else {
            0
        };

        // Build the combined data: partial last-block content + append_data.
        let combined = if tail_bytes_in_last_block > 0 {
            // Read the current tail block, patch in the new data.
            let last_logical_block = current_size.saturating_sub(1) / block_size_u64;
            let phys = last_phys_start.saturating_add(
                last_logical_block.saturating_sub(last_logical_start)
            );
            let mut buf = vec![0u8; block_size];
            self.reader.read_block(phys, &mut buf)?;

            // Write the existing partial block back with the start of append_data.
            let space_in_block = block_size.saturating_sub(tail_bytes_in_last_block);
            let fill = append_data.len().min(space_in_block);
            if let (Some(dest), Some(src)) = (
                buf.get_mut(tail_bytes_in_last_block..tail_bytes_in_last_block + fill),
                append_data.get(..fill),
            ) {
                dest.copy_from_slice(src);
            }
            self.reader.write_block(phys, &buf)?;

            // Return the remaining data that needs new blocks.
            append_data.get(fill..).unwrap_or(&[]).to_vec()
        } else {
            append_data.to_vec()
        };

        // If all append data fit in the existing last block, just update size.
        if combined.is_empty() {
            let new_size = current_size.saturating_add(append_data.len() as u64);
            inode.i_size_lo = new_size as u32;
            inode.i_size_high = (new_size >> 32) as u32;
            return Ok(());
        }

        // Calculate new blocks needed for the remaining data.
        let new_blocks_needed = combined.len()
            .saturating_add(block_size)
            .saturating_sub(1)
            / block_size;

        if new_blocks_needed == 0 {
            return Ok(());
        }

        // Goal: allocate adjacent to the last extent's end for contiguity.
        let last_extent_end = last_phys_start.saturating_add(last_block_count);
        let goal = if last_extent_end > 0 { last_extent_end } else {
            u64::from(self.sb.raw.s_first_data_block)
        };

        let first_new_block = super::balloc::alloc_blocks(
            &self.reader,
            &mut self.sb,
            &mut self.group_descs,
            goal,
            new_blocks_needed as u32,
        )?;

        // Write new data to the allocated blocks.
        let mut data_offset = 0usize;
        for i in 0..new_blocks_needed {
            let block_nr = first_new_block.saturating_add(i as u64);
            let end = data_offset.saturating_add(block_size).min(combined.len());
            let chunk = combined.get(data_offset..end).unwrap_or(&[]);

            let mut buf = vec![0u8; block_size];
            if let Some(dest) = buf.get_mut(..chunk.len()) {
                dest.copy_from_slice(chunk);
            }
            self.reader.write_block(block_nr, &buf)?;

            data_offset = end;
        }

        // Update the extent tree — strategy depends on tree depth.
        let new_logical_start = if current_size > 0 {
            let current_blocks = current_size
                .saturating_add(block_size_u64.saturating_sub(1))
                / block_size_u64;
            current_blocks
        } else {
            0
        };

        let is_adjacent = !all_extents.is_empty()
            && first_new_block == last_extent_end
            && last_block_count.saturating_add(new_blocks_needed as u64) <= 0x7FFF;

        if header.eh_depth == 0 {
            // Depth-0: modify extents directly in the inode's i_block.
            let entries = header.eh_entries as usize;
            let max_entries = header.eh_max as usize;

            if is_adjacent {
                // Extend the last extent's block count in-place.
                let new_len = (last_block_count as u16)
                    .saturating_add(new_blocks_needed as u16);
                let idx = entries.saturating_sub(1);
                let base = 3 + idx * 3; // each extent is 3 u32s, header is 3 u32s
                if let Some(word) = inode.i_block.get(base + 1).copied() {
                    let hi_bits = word & 0xFFFF_0000;
                    inode.i_block[base + 1] = hi_bits | u32::from(new_len);
                }
            } else if entries < max_entries {
                // Add a new extent entry.
                let new_entries = (entries as u16).saturating_add(1);
                inode.i_block[0] = u32::from(EXT4_EXTENT_MAGIC)
                    | (u32::from(new_entries) << 16);

                let base = 3 + entries * 3;
                if base + 2 < inode.i_block.len() {
                    inode.i_block[base] = new_logical_start as u32;
                    inode.i_block[base + 1] = (new_blocks_needed as u32 & 0x7FFF)
                        | (((first_new_block >> 32) as u32) << 16);
                    inode.i_block[base + 2] = first_new_block as u32;
                } else {
                    self.free_contiguous_blocks(first_new_block, new_blocks_needed);
                    return Err(KernelError::NotSupported);
                }
            } else {
                self.free_contiguous_blocks(first_new_block, new_blocks_needed);
                return Err(KernelError::NotSupported);
            }
        } else {
            // Depth>0: find the last leaf block and modify it.
            let result = self.extend_in_last_leaf(
                inode_nr,
                inode,
                is_adjacent,
                new_blocks_needed,
                new_logical_start,
                first_new_block,
            );
            if result.is_err() {
                self.free_contiguous_blocks(first_new_block, new_blocks_needed);
                return result;
            }
        }

        // Update inode size and block count.
        let new_size = current_size.saturating_add(append_data.len() as u64);
        inode.i_size_lo = new_size as u32;
        inode.i_size_high = (new_size >> 32) as u32;

        // Recalculate total block count in 512-byte sectors.
        let total_blocks = new_size
            .saturating_add(block_size_u64.saturating_sub(1))
            / block_size_u64;
        let sectors = total_blocks
            .saturating_mul(u64::from(self.sb.block_size / 512));
        set_inode_blocks_48(inode, sectors);

        Ok(())
    }

    /// Free `count` contiguous blocks starting at `start`.
    /// Shared error-cleanup helper for [`extend_file_data`].
    fn free_contiguous_blocks(&mut self, start: u64, count: usize) {
        for i in 0..count {
            let block_nr = start.saturating_add(i as u64);
            let _ = super::balloc::free_block(
                &self.reader, &mut self.sb, &mut self.group_descs, block_nr,
            );
        }
    }

    /// Extend or append an extent in the last leaf block of a depth>0 tree.
    ///
    /// Reads the last leaf block from disk, modifies the last extent
    /// (adjacent case) or adds a new entry (room in leaf), writes back,
    /// and stamps the extent block checksum if enabled.
    ///
    /// Returns `Err(NotSupported)` if the last leaf is full and the new
    /// blocks are not adjacent — tree splitting is not yet implemented.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn extend_in_last_leaf(
        &self,
        inode_nr: u32,
        inode: &Ext4Inode,
        is_adjacent: bool,
        new_blocks_needed: usize,
        new_logical_start: u64,
        first_new_block: u64,
    ) -> KernelResult<()> {
        let block_size = self.sb.block_size as usize;

        // Walk index nodes to find the last leaf block address.
        // "Last" = the rightmost index entry at each level.
        let block_bytes = inode_block_as_bytes(inode);
        let root_header = read_struct::<Ext4ExtentHeader>(block_bytes)?;
        let ino_seed = inode_csum_seed(&self.sb, inode_nr, inode.i_generation);

        let leaf_block_nr = self.find_last_leaf_block(
            block_bytes, &root_header,
        )?;

        // Read the leaf block.
        let mut leaf_data = vec![0u8; block_size];
        self.reader.read_block(leaf_block_nr, &mut leaf_data)?;

        let leaf_header = read_struct::<Ext4ExtentHeader>(&leaf_data)?;
        if leaf_header.eh_magic != EXT4_EXTENT_MAGIC || leaf_header.eh_depth != 0 {
            return Err(KernelError::IoError);
        }

        let header_size = core::mem::size_of::<Ext4ExtentHeader>();
        let extent_size = core::mem::size_of::<Ext4Extent>();
        let entries = leaf_header.eh_entries as usize;
        let max_entries = leaf_header.eh_max as usize;

        if is_adjacent && entries > 0 {
            // Extend the last extent in this leaf block.
            let idx = entries.saturating_sub(1);
            let off = header_size.saturating_add(idx.saturating_mul(extent_size));
            let ee_len_off = off + 4; // ee_len starts after ee_block(4 bytes)
            if ee_len_off + 2 > leaf_data.len() {
                return Err(KernelError::IoError);
            }
            let old_len = u16::from_le_bytes([
                *leaf_data.get(ee_len_off).ok_or(KernelError::IoError)?,
                *leaf_data.get(ee_len_off + 1).ok_or(KernelError::IoError)?,
            ]);
            let new_len = (old_len & 0x8000) // preserve unwritten flag
                | ((old_len & 0x7FFF).saturating_add(new_blocks_needed as u16));
            let new_len_bytes = new_len.to_le_bytes();
            if let Some(b) = leaf_data.get_mut(ee_len_off) { *b = new_len_bytes[0]; }
            if let Some(b) = leaf_data.get_mut(ee_len_off + 1) { *b = new_len_bytes[1]; }
        } else if entries < max_entries {
            // Add a new extent entry in this leaf block.
            let new_entries = (entries as u16).saturating_add(1);
            // Update eh_entries in the leaf header.
            let eh_entries_bytes = new_entries.to_le_bytes();
            if let Some(b) = leaf_data.get_mut(2) { *b = eh_entries_bytes[0]; }
            if let Some(b) = leaf_data.get_mut(3) { *b = eh_entries_bytes[1]; }

            // Write the new extent at index `entries`.
            let off = header_size.saturating_add(entries.saturating_mul(extent_size));
            if off + extent_size > leaf_data.len() {
                return Err(KernelError::IoError);
            }
            // ee_block (4 bytes)
            let logical_bytes = (new_logical_start as u32).to_le_bytes();
            for (i, &b) in logical_bytes.iter().enumerate() {
                if let Some(slot) = leaf_data.get_mut(off + i) { *slot = b; }
            }
            // ee_len (2 bytes)
            let len_bytes = (new_blocks_needed as u16).to_le_bytes();
            if let Some(slot) = leaf_data.get_mut(off + 4) { *slot = len_bytes[0]; }
            if let Some(slot) = leaf_data.get_mut(off + 5) { *slot = len_bytes[1]; }
            // ee_start_hi (2 bytes)
            let start_hi = ((first_new_block >> 32) as u16).to_le_bytes();
            if let Some(slot) = leaf_data.get_mut(off + 6) { *slot = start_hi[0]; }
            if let Some(slot) = leaf_data.get_mut(off + 7) { *slot = start_hi[1]; }
            // ee_start_lo (4 bytes)
            let start_lo = (first_new_block as u32).to_le_bytes();
            for (i, &b) in start_lo.iter().enumerate() {
                if let Some(slot) = leaf_data.get_mut(off + 8 + i) { *slot = b; }
            }
        } else {
            // Leaf is full and blocks not adjacent — would need tree split.
            return Err(KernelError::NotSupported);
        }

        // Stamp extent block checksum.
        // Re-read header after modifications.
        let updated_header = read_struct::<Ext4ExtentHeader>(&leaf_data)?;
        stamp_extent_block_checksum(
            self.sb.has_metadata_csum, ino_seed, &mut leaf_data, &updated_header,
        );

        // Write the modified leaf block back.
        self.reader.write_block(leaf_block_nr, &leaf_data)?;

        Ok(())
    }

    /// Find the physical block number of the last (rightmost) leaf block
    /// in a depth>0 extent tree.  Follows the rightmost index entry at
    /// each level until reaching a leaf.
    fn find_last_leaf_block(
        &self,
        node_data: &[u8],
        header: &Ext4ExtentHeader,
    ) -> KernelResult<u64> {
        if header.eh_depth == 0 {
            // Should not be called for depth-0 trees.
            return Err(KernelError::InvalidArgument);
        }

        let header_size = core::mem::size_of::<Ext4ExtentHeader>();
        let idx_size = core::mem::size_of::<super::ondisk::Ext4ExtentIdx>();
        let block_size = self.sb.block_size as usize;

        // Find the last (rightmost) index entry.
        let last_idx = header.eh_entries.saturating_sub(1) as usize;
        let off = header_size.saturating_add(last_idx.saturating_mul(idx_size));
        let idx_bytes = node_data
            .get(off..off.saturating_add(idx_size))
            .ok_or(KernelError::IoError)?;
        let idx = read_struct::<super::ondisk::Ext4ExtentIdx>(idx_bytes)?;

        let child_block = u64::from(idx.ei_leaf_lo)
            | (u64::from(idx.ei_leaf_hi) << 16);

        if header.eh_depth == 1 {
            // Child is a leaf block — return its address.
            Ok(child_block)
        } else {
            // Child is another internal node — recurse.
            let mut child_data = vec![0u8; block_size];
            self.reader.read_block(child_block, &mut child_data)?;
            let child_header = read_struct::<Ext4ExtentHeader>(&child_data)?;
            if child_header.eh_magic != EXT4_EXTENT_MAGIC {
                return Err(KernelError::IoError);
            }
            self.find_last_leaf_block(&child_data, &child_header)
        }
    }

    /// Write data back to blocks already mapped by the inode's extent tree.
    ///
    /// Unlike `write_file_data` which allocates entirely new blocks and
    /// rebuilds the extent tree, this writes to the existing physical
    /// blocks.  Use when only the block CONTENTS changed, not the file
    /// size (e.g., inserting a directory entry in an existing block).
    ///
    /// Handles any extent tree depth (walks the tree to find leaf extents).
    /// Sorts leaf extents by logical block number and writes data blocks
    /// sequentially using the physical mapping.
    pub fn write_to_existing_blocks(
        &self,
        inode_nr: u32,
        inode: &Ext4Inode,
        data: &[u8],
    ) -> KernelResult<()> {
        let block_size = self.sb.block_size as usize;

        // Parse the extent header from the inode's i_block.
        let block_bytes = inode_block_as_bytes(inode);
        let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;

        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Err(KernelError::IoError);
        }

        // Collect all leaf extents regardless of tree depth.
        let ino_seed = inode_csum_seed(&self.sb, inode_nr, inode.i_generation);
        let mut extents: Vec<(u64, u64, u64)> = Vec::new();
        self.collect_leaf_extents_recursive(ino_seed, block_bytes, &header, &mut extents)?;

        // Sort by logical block number (should already be sorted, but
        // enforce for correctness).
        extents.sort_by_key(|&(logical, _, _)| logical);

        let mut data_offset = 0usize;
        for &(_, phys_block, len) in &extents {
            for j in 0..len as usize {
                if data_offset >= data.len() {
                    return Ok(());
                }

                let block_nr = phys_block.saturating_add(j as u64);
                let end = (data_offset.saturating_add(block_size)).min(data.len());
                let chunk = data.get(data_offset..end).unwrap_or(&[]);

                let mut buf = vec![0u8; block_size];
                if let Some(dest) = buf.get_mut(..chunk.len()) {
                    dest.copy_from_slice(chunk);
                }
                self.reader.write_block(block_nr, &buf)?;

                data_offset = end;
            }
        }

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

        // Zero the full on-disk inode (including extra area) before writing.
        // This prevents stale data from a previously deleted inode from
        // persisting in the extra fields (timestamps, checksums, xattrs).
        if self.sb.inode_size > 128 {
            self.zero_ondisk_inode(inode_nr)?;

            // Set i_extra_isize so that inline xattrs and extra fields
            // (crtime, checksum_hi, etc.) are properly located.
            let extra_isize = self.sb.want_extra_isize;
            if extra_isize > 0 {
                self.write_extra_isize(inode_nr, extra_isize)?;
            }
        }

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
        let mut dir_data = self.read_file_data(dir_inode_nr, dir_inode)?;
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

                    // Stamp directory block checksums before writing.
                    stamp_dir_data_checksums(
                        &self.sb, dir_inode_nr, dir_inode.i_generation, &mut dir_data,
                    );

                    // Write modified data to existing blocks (no reallocation).
                    // For depth-0 extent trees this avoids the leak that
                    // write_file_data causes by always allocating new blocks.
                    match self.write_to_existing_blocks(dir_inode_nr, dir_inode, &dir_data) {
                        Ok(()) => {},
                        Err(KernelError::NotSupported) => {
                            // Deep extent tree — fall back to full rewrite.
                            let old_inode = *dir_inode;
                            self.invalidate_extent_cache(dir_inode_nr);
                            self.write_file_data(dir_inode, &dir_data)?;
                            self.free_inode_data(dir_inode_nr, &old_inode)?;
                        },
                        Err(e) => return Err(e),
                    }
                    self.write_inode(dir_inode_nr, dir_inode)?;
                    return Ok(());
                }
            }
        }

        // No space in existing blocks — need to grow the directory by one block.
        // Build the new block data in memory, then use write_file_data to
        // reallocate and rebuild the extent tree (crash-safe: old blocks
        // are freed only after new ones are committed).

        // Initialize the new block with a single entry.
        // If metadata checksums are enabled, reserve 12 bytes at the end
        // for the dirent tail and reduce the entry's rec_len accordingly.
        let mut block_buf = vec![0u8; block_size];
        let tail_size = core::mem::size_of::<super::ondisk::Ext4DirEntryTail>();
        let entry_rec_len = if self.sb.has_metadata_csum {
            block_size.saturating_sub(tail_size)
        } else {
            block_size
        };
        write_dir_entry_raw(
            &mut block_buf,
            0,
            child_ino,
            name_bytes,
            file_type_byte,
            entry_rec_len,
        );

        // Initialize and stamp the dirent tail if checksums are enabled.
        if self.sb.has_metadata_csum {
            init_dirent_tail(&mut block_buf);
            stamp_dirent_checksum(
                &self.sb,
                dir_inode_nr,
                dir_inode.i_generation,
                &mut block_buf,
            );
        }

        // Append the new block to existing directory data.
        dir_data.extend_from_slice(&block_buf);

        // Stamp checksums on all blocks (the existing ones may not need
        // re-stamping, but it's safe and simple).
        stamp_dir_data_checksums(
            &self.sb, dir_inode_nr, dir_inode.i_generation, &mut dir_data,
        );

        // Save old inode for crash-safe block freeing.
        let old_inode = *dir_inode;

        // Invalidate cached extent mappings — they'll be rebuilt.
        self.invalidate_extent_cache(dir_inode_nr);

        // Rebuild extent tree with the full directory data (old + new block).
        self.write_file_data(dir_inode, &dir_data)?;
        self.write_inode(dir_inode_nr, dir_inode)?;

        // Free old blocks now that on-disk inode points to new data.
        self.free_inode_data(dir_inode_nr, &old_inode)?;

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
        inode_nr: u32,
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

        let ino_seed = inode_csum_seed(&self.sb, inode_nr, inode.i_generation);
        self.collect_extents_recursive(ino_seed, block_bytes, &header, &mut result)?;
        Ok(result)
    }

    /// Recursively walk an extent tree node and collect all block ranges.
    fn collect_extents_recursive(
        &self,
        ino_seed: u32,
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

                // Validate extent block checksum.
                validate_extent_block_checksum(
                    self.sb.has_metadata_csum,
                    ino_seed,
                    &child_data,
                    &child_header,
                )?;

                // Recurse into the child.
                self.collect_extents_recursive(ino_seed, &child_data, &child_header, result)?;

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
    pub fn free_inode_data(&mut self, inode_nr: u32, inode: &Ext4Inode) -> KernelResult<()> {
        let ranges = self.collect_extent_blocks(inode_nr, inode)?;

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
    /// Supports both extent-based and indirect-block-based inodes:
    /// - **Extents** (modern ext4): walks the extent tree with LRU caching.
    /// - **Indirect blocks** (ext2/ext3 compat): follows the classic
    ///   12 direct + single/double/triple indirect block pointer scheme.
    ///
    /// `inode_nr` is the inode number (cache key for extent-based lookups).
    pub fn lookup_physical_block(
        &self,
        inode_nr: u32,
        inode: &Ext4Inode,
        logical_block: u64,
    ) -> KernelResult<Option<u64>> {
        if (inode.i_flags & inode_flags::EXTENTS) != 0 {
            // Extent-based inode.
            // Fast path: check the extent cache.
            if let Some(phys) = self.extent_cache.lookup(inode_nr, logical_block) {
                return Ok(Some(phys));
            }

            let block_bytes = inode_block_as_bytes(inode);
            let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;
            if header.eh_magic != EXT4_EXTENT_MAGIC || header.eh_entries == 0 {
                return Ok(None);
            }

            let ino_seed = inode_csum_seed(&self.sb, inode_nr, inode.i_generation);
            self.lookup_in_tree(inode_nr, ino_seed, block_bytes, &header, logical_block)
        } else {
            // Indirect-block-based inode (ext2/ext3 compatibility).
            self.lookup_indirect_block(inode, logical_block)
        }
    }

    /// Recursively look up a logical block in an extent tree node.
    ///
    /// When a matching leaf extent is found, it is inserted into the
    /// extent cache so that subsequent lookups within the same range
    /// are served from cache without any I/O.
    fn lookup_in_tree(
        &self,
        inode_nr: u32,
        ino_seed: u32,
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
                    // Cache the full extent range for future lookups
                    // within the same contiguous run.
                    self.extent_cache.insert(
                        inode_nr,
                        ext_logical,
                        ext_phys,
                        ext_len,
                    );

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

                // Validate extent block checksum (non-root blocks have a tail).
                validate_extent_block_checksum(
                    self.sb.has_metadata_csum,
                    ino_seed,
                    &child_data,
                    &child_header,
                )?;

                self.lookup_in_tree(inode_nr, ino_seed, &child_data, &child_header, logical_block)
            } else {
                Ok(None)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Indirect block mapping (ext2/ext3 compatibility)
    // -----------------------------------------------------------------------

    /// Look up a logical block via the classic indirect block scheme.
    ///
    /// The 15 `i_block` entries in the inode are:
    /// - `[0..12]`  — 12 direct block pointers
    /// - `[12]`     — single indirect (points to a block of `u32` pointers)
    /// - `[13]`     — double indirect (points to a block of single-indirect blocks)
    /// - `[14]`     — triple indirect (points to a block of double-indirect blocks)
    ///
    /// A pointer value of 0 means "not allocated" (sparse hole).
    ///
    /// Based on Linux `fs/ext4/indirect.c`.
    fn lookup_indirect_block(
        &self,
        inode: &Ext4Inode,
        logical_block: u64,
    ) -> KernelResult<Option<u64>> {
        // Number of u32 pointers that fit in one block.
        let ptrs_per_block = u64::from(self.sb.block_size) / 4;
        if ptrs_per_block == 0 {
            return Err(KernelError::InvalidArgument);
        }

        // Direct blocks: logical 0..11.
        if logical_block < 12 {
            let ptr = inode.i_block[logical_block as usize];
            return Ok(if ptr == 0 { None } else { Some(u64::from(ptr)) });
        }

        // Single indirect: logical 12 .. (12 + ptrs_per_block - 1).
        let single_max = 12 + ptrs_per_block;
        if logical_block < single_max {
            let indirect_block = u64::from(inode.i_block[12]);
            if indirect_block == 0 {
                return Ok(None);
            }
            let index = logical_block - 12;
            return self.read_indirect_ptr(indirect_block, index);
        }

        // Double indirect: logical single_max .. (single_max + ptrs_per_block^2 - 1).
        let double_max = single_max + ptrs_per_block * ptrs_per_block;
        if logical_block < double_max {
            let dind_block = u64::from(inode.i_block[13]);
            if dind_block == 0 {
                return Ok(None);
            }
            let offset = logical_block - single_max;
            let dind_index = offset / ptrs_per_block;
            let sind_index = offset % ptrs_per_block;

            // Read the double-indirect block to get the single-indirect ptr.
            let sind_block = match self.read_indirect_ptr(dind_block, dind_index)? {
                Some(b) => b,
                None => return Ok(None),
            };
            return self.read_indirect_ptr(sind_block, sind_index);
        }

        // Triple indirect: logical double_max .. (double_max + ptrs_per_block^3 - 1).
        let triple_max = double_max + ptrs_per_block * ptrs_per_block * ptrs_per_block;
        if logical_block < triple_max {
            let tind_block = u64::from(inode.i_block[14]);
            if tind_block == 0 {
                return Ok(None);
            }
            let offset = logical_block - double_max;
            let tind_index = offset / (ptrs_per_block * ptrs_per_block);
            let remainder = offset % (ptrs_per_block * ptrs_per_block);
            let dind_index = remainder / ptrs_per_block;
            let sind_index = remainder % ptrs_per_block;

            // Triple → double → single → data.
            let dind_block = match self.read_indirect_ptr(tind_block, tind_index)? {
                Some(b) => b,
                None => return Ok(None),
            };
            let sind_block = match self.read_indirect_ptr(dind_block, dind_index)? {
                Some(b) => b,
                None => return Ok(None),
            };
            return self.read_indirect_ptr(sind_block, sind_index);
        }

        // Logical block exceeds the maximum addressable by the indirect scheme.
        Ok(None)
    }

    /// Read a single `u32` block pointer from an indirect block on disk.
    ///
    /// `indirect_block` is the physical block number of the indirect block.
    /// `index` is the 0-based index into the array of `u32` pointers.
    ///
    /// Returns `Ok(None)` if the pointer is 0 (sparse hole).
    fn read_indirect_ptr(
        &self,
        indirect_block: u64,
        index: u64,
    ) -> KernelResult<Option<u64>> {
        let byte_offset = index.saturating_mul(4);
        let block_size = u64::from(self.sb.block_size);
        if byte_offset.saturating_add(4) > block_size {
            return Err(KernelError::IoError);
        }

        // Read the indirect block.
        let bs = self.sb.block_size as usize;
        let mut buf = vec![0u8; bs];
        self.reader.read_block(indirect_block, &mut buf)?;

        // Extract the u32 pointer at the given index.
        let off = byte_offset as usize;
        let ptr_bytes = buf.get(off..off + 4).ok_or(KernelError::IoError)?;
        let ptr = u32::from_le_bytes([
            ptr_bytes[0], ptr_bytes[1], ptr_bytes[2], ptr_bytes[3],
        ]);

        Ok(if ptr == 0 { None } else { Some(u64::from(ptr)) })
    }

    /// Write data at a byte offset within an existing file, in place.
    ///
    /// Only modifies the disk blocks that are affected by the write.
    /// Does NOT extend the file — writes past the end are truncated.
    /// Caller should fall back to read-modify-write for extending writes.
    ///
    /// `inode_nr` is passed through to `lookup_physical_block` for the
    /// extent cache.
    pub fn write_at_inplace(
        &self,
        inode_nr: u32,
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

            // Look up the physical block (extent cache accelerated).
            let phys = self.lookup_physical_block(inode_nr, inode, logical_block)?
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
        ino_seed: u32,
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
                let ext_unwritten = (extent.ee_len & 0x8000) != 0;
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

                    let mut buf = vec![0u8; block_size_usize];
                    if !ext_unwritten {
                        let phys = ext_phys.saturating_add(b);
                        self.reader.read_block(phys, &mut buf)?;
                    }
                    // Unwritten extents: buf stays zeroed.

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

                // Validate extent block checksum.
                validate_extent_block_checksum(
                    self.sb.has_metadata_csum,
                    ino_seed,
                    &child_data,
                    &child_header,
                )?;

                self.read_range_from_tree(
                    ino_seed,
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
        // i_file_acl_high is in i_osd2 bytes 2..4 on Linux.
        let hi = u64::from(u16::from_le_bytes([
            *inode.i_osd2.get(2).unwrap_or(&0),
            *inode.i_osd2.get(3).unwrap_or(&0),
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

        // Validate checksum if metadata checksums are enabled.
        validate_xattr_block_checksum(&self.sb, block_nr, &block_data)?;

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
                // Clear i_file_acl_high in i_osd2[2..4].
                if let Some(b) = inode.i_osd2.get_mut(2) { *b = 0; }
                if let Some(b) = inode.i_osd2.get_mut(3) { *b = 0; }
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

        // Stamp the checksum before writing.
        stamp_xattr_block_checksum(&self.sb, block_nr, &mut block_data);

        // Write the xattr block to disk.
        self.reader.write_block(block_nr, &block_data)?;

        // Update the inode's i_file_acl field.
        inode.i_file_acl_lo = block_nr as u32;
        // i_file_acl_high is at i_osd2[2..4].
        let hi = (block_nr >> 32) as u16;
        let hi_bytes = hi.to_le_bytes();
        if let Some(b) = inode.i_osd2.get_mut(2) { *b = hi_bytes[0]; }
        if let Some(b) = inode.i_osd2.get_mut(3) { *b = hi_bytes[1]; }

        self.write_inode(inode_nr, inode)?;

        Ok(block_nr)
    }

    /// Pre-allocate blocks for fallocate without writing data.
    ///
    /// Allocates `block_count` contiguous blocks starting near `goal`.
    /// Returns the physical block number of the first allocated block.
    /// The caller is responsible for setting up the extent tree and inode.
    pub fn fallocate_blocks(
        &mut self,
        goal: u64,
        block_count: u32,
    ) -> KernelResult<u64> {
        super::balloc::alloc_blocks(
            &self.reader,
            &mut self.sb,
            &mut self.group_descs,
            goal,
            block_count,
        )
    }

    /// Return the physical block number one past the last extent in the
    /// inode.  Used as allocation goal for adjacency.  Handles any extent
    /// tree depth.
    pub fn last_extent_end(&self, inode: &Ext4Inode) -> KernelResult<u64> {
        let block_bytes = inode_block_as_bytes(inode);
        let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;

        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Err(KernelError::NotSupported);
        }

        // Collect all leaf extents and find the rightmost one.
        let ino_seed = inode_csum_seed(&self.sb, 0, inode.i_generation);
        let mut extents: Vec<(u64, u64, u64)> = Vec::new();
        self.collect_leaf_extents_recursive(ino_seed, block_bytes, &header, &mut extents)?;

        if extents.is_empty() {
            return Err(KernelError::NotFound);
        }

        // Find the extent with the highest logical start.
        let (_, phys, len) = extents.iter()
            .max_by_key(|&&(logical, _, _)| logical)
            .copied()
            .ok_or(KernelError::NotFound)?;
        Ok(phys.saturating_add(len))
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

    /// Set a single UNWRITTEN extent in the inode's i_block.
    ///
    /// Like `set_single_extent`, but marks the extent as uninitialized
    /// (pre-allocated).  Reads from unwritten extents return zeros without
    /// touching the disk blocks.  The UNWRITTEN flag is bit 15 of `ee_len`.
    ///
    /// Used by `fallocate()` to reserve disk space without committing data.
    pub fn set_single_extent_unwritten(
        &self,
        inode: &mut Ext4Inode,
        logical_block: u32,
        physical_block: u64,
        block_count: u16,
    ) {
        let base = 3; // offset in i_block for first extent
        inode.i_block[base] = logical_block;
        // Set UNWRITTEN flag: bit 15 of ee_len (0x8000 | count).
        let ee_len_with_uninit = u32::from(block_count | 0x8000);
        inode.i_block[base + 1] = ee_len_with_uninit
            | ((physical_block >> 32) as u32) << 16;
        inode.i_block[base + 2] = physical_block as u32;
    }

    /// Append an UNWRITTEN extent to an existing depth-0 extent tree.
    ///
    /// Pre-allocates `block_count` blocks starting at `logical_start`
    /// and adds them as an UNWRITTEN extent.  Returns the physical
    /// start block of the allocation.
    ///
    /// Requirements:
    /// - Inode must have a valid depth-0 extent tree
    /// - There must be room for one more extent entry
    ///
    /// Returns `NotSupported` if the tree is depth>0 or full.
    /// Does NOT update file size — caller must update block count via set_inode_blocks_48.
    pub fn append_unwritten_extent(
        &mut self,
        inode: &mut Ext4Inode,
        logical_start: u32,
        block_count: u16,
        goal: u64,
    ) -> KernelResult<u64> {
        let block_bytes = inode_block_as_bytes(inode);
        let header = read_struct::<Ext4ExtentHeader>(block_bytes)?;

        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Err(KernelError::IoError);
        }
        if header.eh_depth != 0 {
            return Err(KernelError::NotSupported);
        }

        let entries = header.eh_entries as usize;
        let max_entries = header.eh_max as usize;

        if entries >= max_entries {
            return Err(KernelError::NotSupported);
        }

        // Allocate physical blocks.
        let first_block = self.fallocate_blocks(goal, u32::from(block_count))?;

        // Add new UNWRITTEN extent at index `entries`.
        let new_entries = (entries as u16).saturating_add(1);
        // Update eh_entries in the header (i_block[0]).
        inode.i_block[0] = u32::from(EXT4_EXTENT_MAGIC)
            | (u32::from(new_entries) << 16);

        let base = 3_usize.saturating_add(entries.saturating_mul(3));
        if base.saturating_add(2) < inode.i_block.len() {
            inode.i_block[base] = logical_start;
            // ee_len with UNWRITTEN flag (bit 15) | ee_start_hi in high 16 bits
            let ee_len_unwritten = u32::from(block_count | 0x8000);
            inode.i_block[base + 1] =
                ee_len_unwritten | (((first_block >> 32) as u32) << 16);
            inode.i_block[base + 2] = first_block as u32;
        } else {
            // Out of i_block space — free blocks and bail.
            for i in 0..u64::from(block_count) {
                let _ = super::balloc::free_block(
                    &self.reader,
                    &mut self.sb,
                    &mut self.group_descs,
                    first_block.saturating_add(i),
                );
            }
            return Err(KernelError::NotSupported);
        }

        Ok(first_block)
    }

    /// Get the on-disk inode size in bytes (from superblock).
    ///
    /// This is the total space allocated for each inode on disk, including
    /// the 128-byte core, extra fields, and inline xattr area.
    pub fn ondisk_inode_size(&self) -> u32 {
        self.sb.inode_size
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
    fn read_extent_data(&self, inode_nr: u32, inode: &Ext4Inode, file_size: u64) -> KernelResult<Vec<u8>> {
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
                // Uninitialized (unwritten) extents have bit 15 of ee_len set.
                // These are pre-allocated but not yet written — reads return zeros.
                let unwritten = (extent.ee_len & 0x8000) != 0;
                let block_count = u64::from(extent.ee_len & 0x7FFF);

                for b in 0..block_count {
                    let mut buf = vec![0u8; block_size as usize];
                    if !unwritten {
                        let block_nr = phys_block.saturating_add(b);
                        self.reader.read_block(block_nr, &mut buf)?;
                    }
                    // Unwritten extents: buf stays zeroed (correct behavior).

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
            let ino_seed = inode_csum_seed(&self.sb, inode_nr, inode.i_generation);
            self.read_extent_tree_recursive(
                ino_seed, &block_bytes, &header, file_size, &mut result,
            )?;
        }

        // Truncate to exact file size.
        result.truncate(file_size as usize);
        Ok(result)
    }

    /// Recursively read data from an extent tree node.
    fn read_extent_tree_recursive(
        &self,
        ino_seed: u32,
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
                let unwritten = (extent.ee_len & 0x8000) != 0;
                let block_count = u64::from(extent.ee_len & 0x7FFF);

                for b in 0..block_count {
                    if result.len() as u64 >= file_size {
                        return Ok(());
                    }
                    let mut buf = vec![0u8; block_size];
                    if !unwritten {
                        let block_nr = phys_block.saturating_add(b);
                        self.reader.read_block(block_nr, &mut buf)?;
                    }

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

                // Validate extent block checksum.
                validate_extent_block_checksum(
                    self.sb.has_metadata_csum,
                    ino_seed,
                    &child_data,
                    &child_header,
                )?;

                self.read_extent_tree_recursive(
                    ino_seed, &child_data, &child_header, file_size, result,
                )?;
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Indirect block data reading
    // -----------------------------------------------------------------------

    /// Read the full contents of a file using indirect block mapping.
    ///
    /// Walks logical blocks 0..N, looking up each physical block via
    /// the direct/single/double/triple indirect scheme, and assembles
    /// the file data.
    fn read_indirect_data(&self, inode: &Ext4Inode, file_size: u64) -> KernelResult<Vec<u8>> {
        let block_size = u64::from(self.sb.block_size);
        let block_size_usize = self.sb.block_size as usize;
        let total_blocks = file_size.saturating_add(block_size - 1) / block_size;

        let mut result = Vec::with_capacity(file_size as usize);
        let mut block_buf = vec![0u8; block_size_usize];

        for logical in 0..total_blocks {
            let phys = self.lookup_indirect_block(inode, logical)?;
            match phys {
                Some(p) => {
                    self.reader.read_block(p, &mut block_buf)?;
                }
                None => {
                    // Sparse hole — zero fill.
                    for b in block_buf.iter_mut() {
                        *b = 0;
                    }
                }
            }

            let remaining = file_size.saturating_sub(result.len() as u64);
            let copy_len = block_size.min(remaining) as usize;
            if let Some(data) = block_buf.get(..copy_len) {
                result.extend_from_slice(data);
            }
        }

        result.truncate(file_size as usize);
        Ok(result)
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

        // Validate group descriptor checksum (if metadata checksumming enabled).
        if sb.has_metadata_csum {
            let stored = gd.bg_checksum;
            let computed = compute_gd_checksum(sb, i as u32, &buf, gd_size);
            if stored != computed {
                crate::serial_println!(
                    "[ext4] group {} descriptor checksum mismatch: stored={:#06X}, computed={:#06X}",
                    i, stored, computed,
                );
                return Err(KernelError::CorruptedData);
            }
        }

        descs.push(gd);
    }

    if sb.has_metadata_csum {
        crate::serial_println!("[ext4] all {} group descriptor checksums valid", count);
    }

    Ok(descs)
}

/// Compute the CRC32C checksum for a block group descriptor.
///
/// Algorithm (from Linux `ext4_group_desc_csum()`):
/// 1. Start with `sb.csum_seed` (raw CRC accumulator, no inversion).
/// 2. Feed in the group number as little-endian u32.
/// 3. Feed in the descriptor bytes with `bg_checksum` (offset 0x1E, 2 bytes) zeroed.
/// 4. Return the lower 16 bits of the final CRC32C.
fn compute_gd_checksum(sb: &ParsedSuperblock, group: u32, raw: &[u8], desc_size: usize) -> u16 {
    let group_le = group.to_le_bytes();

    // Start with the UUID-derived seed and fold in the group number.
    let crc = crate::crypto::crc32c_raw(sb.csum_seed, &group_le);

    // Feed descriptor bytes, but zero out the bg_checksum field (2 bytes at offset 0x1E).
    const CSUM_OFFSET: usize = 0x1E;
    const CSUM_SIZE: usize = 2;

    let before = raw.get(..CSUM_OFFSET).unwrap_or(&[]);
    let after_start = CSUM_OFFSET.saturating_add(CSUM_SIZE);
    let after = raw.get(after_start..desc_size.min(raw.len())).unwrap_or(&[]);

    let crc = crate::crypto::crc32c_raw(crc, before);
    let crc = crate::crypto::crc32c_raw(crc, &[0u8; CSUM_SIZE]);
    // Final segment with inversion.
    let final_crc = crate::crypto::crc32c_seed(crc, after);

    // Only the lower 16 bits are stored.
    #[allow(clippy::cast_possible_truncation)]
    { final_crc as u16 }
}

/// Validate an inode's CRC32C checksum.
///
/// Algorithm (from Linux `ext4_inode_csum()`):
/// 1. Start with `sb.csum_seed` (raw CRC accumulator).
/// 2. Feed in the inode number as little-endian u32.
/// 3. Feed in the inode generation as little-endian u32.
/// 4. Feed in the inode bytes with `i_checksum_lo` and `i_checksum_hi` zeroed.
/// 5. Compare: lower 16 bits → `i_checksum_lo`, upper 16 bits → `i_checksum_hi`.
///
/// The `i_checksum_lo` field is at offset 0x7C within the inode (within i_osd2),
/// and `i_checksum_hi` is at offset 0x82 (in the extra area, if inode_size >= 256).
fn validate_inode_checksum(
    sb: &ParsedSuperblock,
    inode_nr: u32,
    _inode: &Ext4Inode,
    raw_bytes: &[u8],
) -> KernelResult<()> {
    // i_checksum_lo is at offset 0x7C within the inode (i_osd2 + 8).
    const CKSUM_LO_OFFSET: usize = 0x7C;
    // i_checksum_hi is at offset 0x82 within the inode (extra area + 2).
    const CKSUM_HI_OFFSET: usize = 0x82;

    let inode_size = sb.inode_size as usize;

    // Read the stored checksum low 16 bits.
    let stored_lo = if raw_bytes.len() > CKSUM_LO_OFFSET.saturating_add(1) {
        u16::from_le_bytes([
            raw_bytes[CKSUM_LO_OFFSET],
            raw_bytes[CKSUM_LO_OFFSET.saturating_add(1)],
        ])
    } else {
        return Ok(()); // Can't validate — inode too small.
    };

    // Read the stored checksum high 16 bits (only if inode_size >= 256).
    let stored_hi = if inode_size >= 256 && raw_bytes.len() > CKSUM_HI_OFFSET.saturating_add(1) {
        u16::from_le_bytes([
            raw_bytes[CKSUM_HI_OFFSET],
            raw_bytes[CKSUM_HI_OFFSET.saturating_add(1)],
        ])
    } else {
        0u16
    };

    let stored = u32::from(stored_lo) | (u32::from(stored_hi) << 16);

    // Compute the checksum.
    let ino_le = inode_nr.to_le_bytes();
    let gen_le = _inode.i_generation.to_le_bytes();

    let crc = crate::crypto::crc32c_raw(sb.csum_seed, &ino_le);
    let crc = crate::crypto::crc32c_raw(crc, &gen_le);

    // Feed inode bytes, zeroing checksum fields.
    // We need to handle up to 3 segments:
    //   [0..CKSUM_LO_OFFSET] + [0,0] + [CKSUM_LO_OFFSET+2..CKSUM_HI_OFFSET] + [0,0] + [CKSUM_HI_OFFSET+2..inode_size]
    let end = inode_size.min(raw_bytes.len());

    let seg1 = raw_bytes.get(..CKSUM_LO_OFFSET).unwrap_or(&[]);
    let crc = crate::crypto::crc32c_raw(crc, seg1);
    let crc = crate::crypto::crc32c_raw(crc, &[0u8; 2]); // zero i_checksum_lo

    let seg2_start = CKSUM_LO_OFFSET.saturating_add(2);
    if inode_size >= 256 && end > CKSUM_HI_OFFSET.saturating_add(1) {
        // Large inode: also zero i_checksum_hi.
        let seg2 = raw_bytes.get(seg2_start..CKSUM_HI_OFFSET).unwrap_or(&[]);
        let crc = crate::crypto::crc32c_raw(crc, seg2);
        let crc = crate::crypto::crc32c_raw(crc, &[0u8; 2]); // zero i_checksum_hi
        let seg3_start = CKSUM_HI_OFFSET.saturating_add(2);
        let seg3 = raw_bytes.get(seg3_start..end).unwrap_or(&[]);
        let computed = crate::crypto::crc32c_seed(crc, seg3);

        if stored != computed {
            crate::serial_println!(
                "[ext4] inode {} checksum mismatch: stored={:#010X}, computed={:#010X}",
                inode_nr, stored, computed,
            );
            return Err(KernelError::CorruptedData);
        }
    } else {
        // Small inode (128 bytes): only i_checksum_lo.
        let seg2 = raw_bytes.get(seg2_start..end).unwrap_or(&[]);
        let computed = crate::crypto::crc32c_seed(crc, seg2);

        let stored_lo_only = u32::from(stored_lo);
        let computed_lo_only = computed & 0xFFFF;
        if stored_lo_only != computed_lo_only {
            crate::serial_println!(
                "[ext4] inode {} checksum mismatch: stored={:#06X}, computed={:#06X}",
                inode_nr, stored_lo_only, computed_lo_only,
            );
            return Err(KernelError::CorruptedData);
        }
    }

    Ok(())
}

/// Compute and embed an inode checksum into a mutable raw inode buffer.
///
/// The inode buffer must be at least 128 bytes.  If the inode is 256+
/// bytes, both `i_checksum_lo` (offset 0x7C) and `i_checksum_hi`
/// (offset 0x82) are written.  Otherwise only `i_checksum_lo`.
fn stamp_inode_checksum(
    sb: &ParsedSuperblock,
    inode_nr: u32,
    inode: &Ext4Inode,
    buf: &mut [u8],
) {
    const CKSUM_LO_OFFSET: usize = 0x7C;
    const CKSUM_HI_OFFSET: usize = 0x82;
    let inode_sz = buf.len();

    // Zero the checksum fields before computing.
    if inode_sz > CKSUM_LO_OFFSET.saturating_add(1) {
        buf[CKSUM_LO_OFFSET] = 0;
        buf[CKSUM_LO_OFFSET.saturating_add(1)] = 0;
    }
    if inode_sz > CKSUM_HI_OFFSET.saturating_add(1) {
        buf[CKSUM_HI_OFFSET] = 0;
        buf[CKSUM_HI_OFFSET.saturating_add(1)] = 0;
    }

    // Compute CRC32C(seed + inode_nr + generation + inode_bytes).
    let ino_le = inode_nr.to_le_bytes();
    let gen_le = inode.i_generation.to_le_bytes();

    let crc = crate::crypto::crc32c_raw(sb.csum_seed, &ino_le);
    let crc = crate::crypto::crc32c_raw(crc, &gen_le);
    let computed = crate::crypto::crc32c_seed(crc, buf);

    // Write checksum back into the buffer.
    #[allow(clippy::cast_possible_truncation)]
    let lo = computed as u16;
    let lo_bytes = lo.to_le_bytes();
    if inode_sz > CKSUM_LO_OFFSET.saturating_add(1) {
        buf[CKSUM_LO_OFFSET] = lo_bytes[0];
        buf[CKSUM_LO_OFFSET.saturating_add(1)] = lo_bytes[1];
    }

    if inode_sz > CKSUM_HI_OFFSET.saturating_add(1) {
        #[allow(clippy::cast_possible_truncation)]
        let hi = (computed >> 16) as u16;
        let hi_bytes = hi.to_le_bytes();
        buf[CKSUM_HI_OFFSET] = hi_bytes[0];
        buf[CKSUM_HI_OFFSET.saturating_add(1)] = hi_bytes[1];
    }
}

/// Compute and embed a group descriptor checksum into a mutable raw descriptor buffer.
fn stamp_gd_checksum(sb: &ParsedSuperblock, group: u32, buf: &mut [u8]) {
    const CSUM_OFFSET: usize = 0x1E;

    // Zero the checksum field.
    if buf.len() > CSUM_OFFSET.saturating_add(1) {
        buf[CSUM_OFFSET] = 0;
        buf[CSUM_OFFSET.saturating_add(1)] = 0;
    }

    let group_le = group.to_le_bytes();
    let crc = crate::crypto::crc32c_raw(sb.csum_seed, &group_le);
    let computed = crate::crypto::crc32c_seed(crc, buf);

    #[allow(clippy::cast_possible_truncation)]
    let csum = computed as u16;
    let csum_bytes = csum.to_le_bytes();
    if buf.len() > CSUM_OFFSET.saturating_add(1) {
        buf[CSUM_OFFSET] = csum_bytes[0];
        buf[CSUM_OFFSET.saturating_add(1)] = csum_bytes[1];
    }
}

/// Compute and embed the superblock checksum into a mutable raw superblock buffer.
fn stamp_superblock_checksum(buf: &mut [u8]) {
    const CSUM_OFFSET: usize = 0x3FC;
    const SB_SIZE: usize = 1024;

    if buf.len() < SB_SIZE {
        return;
    }

    // Zero the checksum field.
    buf[CSUM_OFFSET] = 0;
    buf[CSUM_OFFSET.saturating_add(1)] = 0;
    buf[CSUM_OFFSET.saturating_add(2)] = 0;
    buf[CSUM_OFFSET.saturating_add(3)] = 0;

    let computed = crate::crypto::crc32c(buf.get(..SB_SIZE).unwrap_or(&[]));

    let csum_bytes = computed.to_le_bytes();
    buf[CSUM_OFFSET] = csum_bytes[0];
    buf[CSUM_OFFSET.saturating_add(1)] = csum_bytes[1];
    buf[CSUM_OFFSET.saturating_add(2)] = csum_bytes[2];
    buf[CSUM_OFFSET.saturating_add(3)] = csum_bytes[3];
}

// ---------------------------------------------------------------------------
// Per-inode checksum seed
// ---------------------------------------------------------------------------

/// Compute the per-inode checksum seed used for extent blocks and directory
/// blocks.  This is `crc32c_raw(crc32c_raw(sb.csum_seed, &ino_le), &gen_le)`.
///
/// Returns 0 if metadata checksums are disabled.
///
/// Based on Linux `ext4_inode_csum_set()` / `ext4_inode_csum_init()`.
fn inode_csum_seed(sb: &ParsedSuperblock, inode_nr: u32, inode_gen: u32) -> u32 {
    if !sb.has_metadata_csum {
        return 0;
    }
    let crc = crate::crypto::crc32c_raw(sb.csum_seed, &inode_nr.to_le_bytes());
    crate::crypto::crc32c_raw(crc, &inode_gen.to_le_bytes())
}

// ---------------------------------------------------------------------------
// Extent block checksums
// ---------------------------------------------------------------------------

/// Validate a non-root extent tree block's CRC32C checksum.
///
/// ext4 with metadata_csum stores a 4-byte `ext4_extent_tail` after the
/// maximum extent entries (at offset `header_size + eh_max * entry_size`).
/// The root extent block (in the inode's i_block) does not have a tail —
/// it is covered by the inode checksum.
///
/// The checksum is CRC32C(inode_csum_seed + block_data[..tail_offset]).
///
/// Returns Ok(()) if checksums are disabled or if the checksum matches.
fn validate_extent_block_checksum(
    has_metadata_csum: bool,
    ino_seed: u32,
    block_data: &[u8],
    header: &Ext4ExtentHeader,
) -> KernelResult<()> {
    if !has_metadata_csum {
        return Ok(());
    }

    // Tail offset: header_size + eh_max * entry_size.
    // Both Ext4Extent and Ext4ExtentIdx are 12 bytes.
    let entry_size = core::mem::size_of::<Ext4Extent>();
    let tail_offset = core::mem::size_of::<Ext4ExtentHeader>()
        .saturating_add((header.eh_max as usize).saturating_mul(entry_size));

    let tail_end = tail_offset.saturating_add(4);
    if block_data.len() < tail_end {
        return Ok(()); // Block too small for a checksum tail.
    }

    // Read the stored checksum.
    let tail_bytes = block_data.get(tail_offset..tail_end)
        .ok_or(KernelError::IoError)?;
    let stored = u32::from_le_bytes([
        tail_bytes[0], tail_bytes[1], tail_bytes[2], tail_bytes[3],
    ]);

    // Compute: CRC32C(inode_csum_seed, block_data[..tail_offset]).
    let data = block_data.get(..tail_offset).ok_or(KernelError::IoError)?;
    let computed = crate::crypto::crc32c_seed(ino_seed, data);

    if computed != stored {
        serial_println!(
            "[ext4] extent block checksum MISMATCH: stored={:#010x} computed={:#010x}",
            stored, computed,
        );
        return Err(KernelError::CorruptedData);
    }

    Ok(())
}

/// Compute and stamp a non-root extent block's checksum tail.
///
/// Updates the 4-byte `et_checksum` field in-place at the tail offset.
#[allow(dead_code)]
fn stamp_extent_block_checksum(
    has_metadata_csum: bool,
    ino_seed: u32,
    block_data: &mut [u8],
    header: &Ext4ExtentHeader,
) {
    if !has_metadata_csum {
        return;
    }

    let entry_size = core::mem::size_of::<Ext4Extent>();
    let tail_offset = core::mem::size_of::<Ext4ExtentHeader>()
        .saturating_add((header.eh_max as usize).saturating_mul(entry_size));

    let tail_end = tail_offset.saturating_add(4);
    if block_data.len() < tail_end {
        return;
    }

    let computed = crate::crypto::crc32c_seed(
        ino_seed,
        block_data.get(..tail_offset).unwrap_or(&[]),
    );
    let csum_bytes = computed.to_le_bytes();
    if let Some(dest) = block_data.get_mut(tail_offset..tail_end) {
        dest.copy_from_slice(&csum_bytes);
    }
}

// ---------------------------------------------------------------------------
// Directory block checksums
// ---------------------------------------------------------------------------

/// Validate a directory data block's checksum (if present).
///
/// ext4 with metadata_csum places a 12-byte `ext4_dir_entry_tail` at the
/// end of each directory data block.  The tail is identified by
/// `inode == 0, rec_len == 12, name_len == 0, file_type == 0xDE`.
///
/// The checksum is CRC32C(csum_seed + inode_nr_le32 + gen_le32 + block_data),
/// where block_data has the tail's checksum field zeroed.
///
/// Returns Ok(()) if no tail is present or if the checksum matches.
pub(super) fn validate_dirent_checksum(
    sb: &ParsedSuperblock,
    dir_inode_nr: u32,
    dir_inode_gen: u32,
    block_data: &[u8],
) -> KernelResult<()> {
    if !sb.has_metadata_csum {
        return Ok(());
    }

    let tail_size = core::mem::size_of::<super::ondisk::Ext4DirEntryTail>();
    if block_data.len() < tail_size {
        return Ok(()); // Block too small for a tail.
    }

    // Check if the last 12 bytes look like a dirent tail.
    let tail_offset = block_data.len().saturating_sub(tail_size);
    let tail_bytes = block_data.get(tail_offset..).ok_or(KernelError::IoError)?;
    let tail = read_struct::<super::ondisk::Ext4DirEntryTail>(tail_bytes)?;

    if tail.det_reserved_zero1 != 0
        || tail.det_rec_len != 12
        || tail.det_reserved_zero2 != 0
        || tail.det_reserved_ft != super::ondisk::EXT4_DIRENT_TAIL_MARKER
    {
        // Not a dirent tail — block doesn't have a checksum.
        return Ok(());
    }

    // Compute the checksum.
    let ino_le = dir_inode_nr.to_le_bytes();
    let gen_le = dir_inode_gen.to_le_bytes();

    let crc = crate::crypto::crc32c_raw(sb.csum_seed, &ino_le);
    let crc = crate::crypto::crc32c_raw(crc, &gen_le);

    // Feed the block data, zeroing the checksum field (last 4 bytes of the tail).
    let data_before_csum = block_data.get(..tail_offset.saturating_add(8))
        .ok_or(KernelError::IoError)?;
    let crc = crate::crypto::crc32c_raw(crc, data_before_csum);
    // Final segment with inversion (consistent with inode/GD checksum convention).
    let computed = crate::crypto::crc32c_seed(crc, &[0u8; 4]);

    if computed != tail.det_checksum {
        serial_println!(
            "[ext4] directory block checksum MISMATCH for inode {}: stored={:#010x} computed={:#010x}",
            dir_inode_nr, tail.det_checksum, computed,
        );
        return Err(KernelError::CorruptedData);
    }

    Ok(())
}

/// Compute and stamp a directory block checksum tail.
///
/// The block must have a valid dirent tail structure at the end.
/// Updates the `det_checksum` field in-place.
fn stamp_dirent_checksum(
    sb: &ParsedSuperblock,
    dir_inode_nr: u32,
    dir_inode_gen: u32,
    block_data: &mut [u8],
) {
    if !sb.has_metadata_csum {
        return;
    }

    let tail_size = core::mem::size_of::<super::ondisk::Ext4DirEntryTail>();
    if block_data.len() < tail_size {
        return;
    }

    let tail_offset = block_data.len().saturating_sub(tail_size);

    // Check that this block has a tail.
    if let Some(tail_bytes) = block_data.get(tail_offset..) {
        if let Ok(tail) = read_struct::<super::ondisk::Ext4DirEntryTail>(tail_bytes) {
            if tail.det_reserved_ft != super::ondisk::EXT4_DIRENT_TAIL_MARKER {
                return; // No tail to stamp.
            }
        } else {
            return;
        }
    } else {
        return;
    }

    // Compute checksum.
    let ino_le = dir_inode_nr.to_le_bytes();
    let gen_le = dir_inode_gen.to_le_bytes();

    let crc = crate::crypto::crc32c_raw(sb.csum_seed, &ino_le);
    let crc = crate::crypto::crc32c_raw(crc, &gen_le);

    // Feed block data with zeroed checksum field.
    let data_before_csum = block_data.get(..tail_offset.saturating_add(8)).unwrap_or(&[]);
    let crc = crate::crypto::crc32c_raw(crc, data_before_csum);
    // Final segment with inversion (consistent with inode/GD checksum convention).
    let computed = crate::crypto::crc32c_seed(crc, &[0u8; 4]);

    // Stamp the checksum into the tail.
    let csum_offset = tail_offset.saturating_add(8);
    let csum_bytes = computed.to_le_bytes();
    if let Some(dest) = block_data.get_mut(csum_offset..csum_offset.saturating_add(4)) {
        dest.copy_from_slice(&csum_bytes);
    }
}

/// Stamp directory block checksums on all block-sized chunks in a buffer.
///
/// Used before writing modified directory data back to disk.
/// Each block that has a valid dirent tail gets its checksum recomputed.
pub(super) fn stamp_dir_data_checksums(
    sb: &ParsedSuperblock,
    dir_inode_nr: u32,
    dir_inode_gen: u32,
    dir_data: &mut [u8],
) {
    if !sb.has_metadata_csum {
        return;
    }
    let bs = sb.block_size as usize;
    if bs == 0 {
        return;
    }
    let mut offset: usize = 0;
    while offset.saturating_add(bs) <= dir_data.len() {
        if let Some(block) = dir_data.get_mut(offset..offset.saturating_add(bs)) {
            stamp_dirent_checksum(sb, dir_inode_nr, dir_inode_gen, block);
        }
        offset = offset.saturating_add(bs);
    }
}

/// Initialize a 12-byte dirent tail at the end of a directory block.
///
/// When metadata_csum is enabled, new directory blocks must include a
/// fake dirent entry at the end that holds the CRC32C checksum.  The
/// previous real entry's `rec_len` must be reduced to leave room for
/// the 12-byte tail.
fn init_dirent_tail(block_data: &mut [u8]) {
    let tail_size = core::mem::size_of::<super::ondisk::Ext4DirEntryTail>();
    if block_data.len() < tail_size {
        return;
    }

    let tail_offset = block_data.len().saturating_sub(tail_size);

    // Write the dirent tail structure.
    // det_reserved_zero1 (4 bytes) = 0
    if let Some(dest) = block_data.get_mut(tail_offset..tail_offset.saturating_add(4)) {
        dest.copy_from_slice(&0u32.to_le_bytes());
    }
    // det_rec_len (2 bytes) = 12
    if let Some(dest) = block_data.get_mut(
        tail_offset.saturating_add(4)..tail_offset.saturating_add(6)
    ) {
        dest.copy_from_slice(&12u16.to_le_bytes());
    }
    // det_reserved_zero2 (1 byte) = 0
    if let Some(dest) = block_data.get_mut(tail_offset.saturating_add(6)..tail_offset.saturating_add(7)) {
        dest[0] = 0;
    }
    // det_reserved_ft (1 byte) = 0xDE
    if let Some(dest) = block_data.get_mut(tail_offset.saturating_add(7)..tail_offset.saturating_add(8)) {
        dest[0] = super::ondisk::EXT4_DIRENT_TAIL_MARKER;
    }
    // det_checksum (4 bytes) = 0 (will be stamped separately)
    if let Some(dest) = block_data.get_mut(
        tail_offset.saturating_add(8)..tail_offset.saturating_add(12)
    ) {
        dest.copy_from_slice(&0u32.to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// Extended attribute block checksums
// ---------------------------------------------------------------------------

/// Validate an xattr block's CRC32C checksum.
///
/// ext4 with metadata_csum stores a checksum in `h_checksum` of the
/// `Ext4XattrHeader` at the start of each standalone xattr block.
/// The checksum covers `csum_seed + block_nr_le64 + block_data` with
/// the `h_checksum` field zeroed during computation.
///
/// Based on Linux `ext4_xattr_block_csum()` in `fs/ext4/xattr.c`.
fn validate_xattr_block_checksum(
    sb: &ParsedSuperblock,
    block_nr: u64,
    block_data: &[u8],
) -> KernelResult<()> {
    if !sb.has_metadata_csum {
        return Ok(());
    }

    // h_checksum is at offset 16 within Ext4XattrHeader (after h_magic,
    // h_refcount, h_blocks, h_hash — each u32).
    const CSUM_OFFSET: usize = 16;
    const CSUM_SIZE: usize = 4;

    if block_data.len() < CSUM_OFFSET.saturating_add(CSUM_SIZE) {
        return Err(KernelError::InvalidArgument);
    }

    // Read stored checksum.
    let stored = u32::from_le_bytes([
        block_data[CSUM_OFFSET],
        block_data[CSUM_OFFSET.saturating_add(1)],
        block_data[CSUM_OFFSET.saturating_add(2)],
        block_data[CSUM_OFFSET.saturating_add(3)],
    ]);

    let computed = compute_xattr_block_checksum(sb, block_nr, block_data);
    if stored != computed {
        crate::serial_println!(
            "[ext4] xattr block {} checksum MISMATCH: stored={:#010x} computed={:#010x}",
            block_nr, stored, computed,
        );
        return Err(KernelError::CorruptedData);
    }

    Ok(())
}

/// Compute and stamp the checksum for an xattr block.
///
/// Writes the CRC32C into the `h_checksum` field at offset 16.
///
/// Based on Linux `ext4_xattr_block_csum_set()` in `fs/ext4/xattr.c`.
fn stamp_xattr_block_checksum(
    sb: &ParsedSuperblock,
    block_nr: u64,
    block_data: &mut [u8],
) {
    if !sb.has_metadata_csum {
        return;
    }

    const CSUM_OFFSET: usize = 16;
    const CSUM_SIZE: usize = 4;

    if block_data.len() < CSUM_OFFSET.saturating_add(CSUM_SIZE) {
        return;
    }

    // Zero the checksum field before computing.
    if let Some(dest) = block_data.get_mut(CSUM_OFFSET..CSUM_OFFSET.saturating_add(CSUM_SIZE)) {
        dest.copy_from_slice(&[0u8; CSUM_SIZE]);
    }

    let computed = compute_xattr_block_checksum(sb, block_nr, block_data);
    let csum_bytes = computed.to_le_bytes();
    if let Some(dest) = block_data.get_mut(CSUM_OFFSET..CSUM_OFFSET.saturating_add(CSUM_SIZE)) {
        dest.copy_from_slice(&csum_bytes);
    }
}

/// Compute the CRC32C checksum for an xattr block.
///
/// The xattr block checksum differs from extent/directory checksums:
/// it uses `csum_seed + block_nr` as the seed (not the per-inode seed),
/// because a single xattr block can be shared by multiple inodes.
///
/// Algorithm: `crc32c(csum_seed, block_nr_le64 || block_data)`
/// with h_checksum (4 bytes at offset 16) replaced by zeros.
fn compute_xattr_block_checksum(
    sb: &ParsedSuperblock,
    block_nr: u64,
    block_data: &[u8],
) -> u32 {
    const CSUM_OFFSET: usize = 16;
    const CSUM_SIZE: usize = 4;

    let block_nr_le = block_nr.to_le_bytes();

    // Feed: csum_seed + block_nr_le64
    let crc = crate::crypto::crc32c_raw(sb.csum_seed, &block_nr_le);

    // Feed: block data before h_checksum (bytes 0..16).
    let before = block_data.get(..CSUM_OFFSET).unwrap_or(&[]);
    let crc = crate::crypto::crc32c_raw(crc, before);

    // Feed: 4 zero bytes in place of h_checksum.
    let crc = crate::crypto::crc32c_raw(crc, &[0u8; CSUM_SIZE]);

    // Feed: block data after h_checksum (bytes 20..block_size).
    let after_start = CSUM_OFFSET.saturating_add(CSUM_SIZE);
    let after = block_data.get(after_start..).unwrap_or(&[]);

    // Final segment with inversion (consistent with our other checksums).
    crate::crypto::crc32c_seed(crc, after)
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

/// Read the inode block count and return 512-byte sectors.
///
/// Combines `i_blocks_lo` (32 bits) with `i_osd2[0..2]` (high 16 bits)
/// for a 48-bit raw value.  If the inode has `HUGE_FILE` flag set, the
/// raw value is in filesystem block units — multiply by `block_size / 512`
/// to convert to sectors.
///
/// Based on Linux `ext4_inode_blocks()` in `fs/ext4/inode.c`.
#[allow(dead_code)]
fn inode_block_sectors(inode: &Ext4Inode, block_size: u32) -> u64 {
    let lo = u64::from(inode.i_blocks_lo);
    let hi = u64::from(u16::from_le_bytes([
        *inode.i_osd2.get(0).unwrap_or(&0),
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
fn set_inode_blocks_48(inode: &mut Ext4Inode, sectors: u64) {
    inode.i_blocks_lo = sectors as u32;
    let hi = ((sectors >> 32) as u16).to_le_bytes();
    if let Some(slot) = inode.i_osd2.get_mut(0) { *slot = hi[0]; }
    if let Some(slot) = inode.i_osd2.get_mut(1) { *slot = hi[1]; }
    // Always clear HUGE_FILE since we store sectors, not fs blocks.
    inode.i_flags &= !super::ondisk::inode_flags::HUGE_FILE;
}

/// Binary search a sorted list of (logical_start, phys_start, length)
/// extent tuples for the physical block corresponding to `logical_block`.
///
/// Returns `Some(physical_block)` if the logical block falls within an
/// extent, or `None` if it's in a sparse hole.
fn find_in_leaf_extents(extents: &[(u64, u64, u64)], logical_block: u64) -> Option<u64> {
    // Binary search for the extent containing logical_block.
    // Extents are sorted by logical_start.
    let idx = extents.partition_point(|&(start, _, _)| start <= logical_block);
    if idx == 0 {
        return None;
    }
    let (start, phys, len) = extents[idx - 1];
    let offset = logical_block.checked_sub(start)?;
    if offset < len {
        Some(phys.saturating_add(offset))
    } else {
        None
    }
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

        // Skip the dirent tail (metadata_csum checksum entry).
        // It looks like a deleted entry but must not be reclaimed.
        if hdr.inode == 0
            && hdr.rec_len == 12
            && hdr.name_len == 0
            && hdr.file_type == super::ondisk::EXT4_DIRENT_TAIL_MARKER
        {
            // Don't update last_offset — the tail is not usable space.
            offset = offset.saturating_add(hdr.rec_len as usize);
            continue;
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
