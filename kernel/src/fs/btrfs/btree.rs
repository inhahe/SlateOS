//! Reading and walking a Btrfs B-tree.
//!
//! Every tree in the filesystem — chunk, root, and each subvolume's file
//! tree — is the same structure: a block of `nodesize` bytes with a common
//! [`Header`], followed by either an array of `btrfs_key_ptr` (an internal
//! node, `level > 0`) or an array of `btrfs_item` (a leaf, `level == 0`).
//! One parser therefore serves all of them, and the only per-tree input is
//! the logical address of the root.
//!
//! # Layout
//!
//! ```text
//!  internal node                        leaf
//!  +--------------------------+         +--------------------------+
//!  | header (101 bytes)       |         | header (101 bytes)       |
//!  +--------------------------+         +--------------------------+
//!  | key_ptr[0]  (33 bytes)   |         | item[0] (25 bytes)       |
//!  | key_ptr[1]               |         | item[1]                  | grows down
//!  | ...                      |         | ...                      |
//!  +--------------------------+         +--------------------------+
//!  |         free             |         |         free             |
//!  |                          |         +--------------------------+
//!  |                          |         | ...                      |
//!  |                          |         | data[1]                  | grows up
//!  |                          |         | data[0]                  |
//!  +--------------------------+         +--------------------------+
//! ```
//!
//! A leaf's item array grows from the front and its data grows from the back,
//! meeting in the middle — which is why `item.offset` is measured from the end
//! of the header rather than from the start of the block, and why nothing may
//! assume item `n`'s data follows item `n-1`'s.
//!
//! # What is validated, and why each check earns its place
//!
//! A tree block is read by following a 64-bit pointer out of another block. If
//! that pointer is wrong — corruption, a stale mapping, a bug in this driver's
//! own chunk arithmetic — the bytes at the destination are *something*, and
//! parsing them as a node yields item offsets and sizes that index wherever
//! they like. Four checks stop that at the door:
//!
//! - **the checksum**, which catches an arbitrary block landing here;
//! - **`bytenr` equals the address we followed**, which catches a *valid*
//!   block that is simply not the one wanted — the case a checksum cannot
//!   catch, because the wrong block checksums perfectly well;
//! - **`generation` equals what the parent said to expect**, which catches a
//!   correct block from the *wrong point in time*, the specific hazard of a
//!   copy-on-write filesystem where the previous version of a node is still
//!   intact on disk and still checksums;
//! - **`nritems` fits the block**, which is what stops every subsequent slot
//!   read from being an out-of-bounds index.
//!
//! Individually each looks like belt-and-braces; together they are the reason
//! a bad pointer surfaces as an error at the point of the mistake rather than
//! as nonsense several layers up.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{SectorSource, read_bytes};

use super::chunk::ChunkMap;
use super::raw::{HEADER_LEN, Header, ITEM_LEN, KEY_LEN, KEY_PTR_LEN, Key, read_u32, read_u64};
use super::sb::verify_csum;

/// Deepest tree Btrfs will build (`BTRFS_MAX_LEVEL`).
///
/// Used as a hard bound on descent. A corrupt block claiming `level = 200`
/// would otherwise drive an unbounded recursion, and a corrupt block claiming
/// a level that does not decrease would drive an infinite one.
pub const MAX_LEVEL: u8 = 8;

/// One tree block, read and validated.
#[derive(Debug, Clone)]
pub struct Node {
    /// The parsed header.
    pub header: Header,
    /// The whole block, `nodesize` bytes.
    pub data: Vec<u8>,
}

impl Node {
    /// Number of slots, as a `usize`.
    pub fn nritems(&self) -> usize {
        self.header.nritems as usize
    }

    /// The key of slot `slot`, in either kind of node.
    pub fn key_at(&self, slot: usize) -> KernelResult<Key> {
        if slot >= self.nritems() {
            return Err(KernelError::InvalidArgument);
        }
        let stride = if self.header.is_leaf() {
            ITEM_LEN
        } else {
            KEY_PTR_LEN
        };
        let off = slot
            .checked_mul(stride)
            .and_then(|v| v.checked_add(HEADER_LEN))
            .ok_or(KernelError::InvalidArgument)?;
        Key::parse(&self.data, off)
    }

    /// The child pointer and expected generation of slot `slot`.
    pub fn child_at(&self, slot: usize) -> KernelResult<(u64, u64)> {
        if self.header.is_leaf() || slot >= self.nritems() {
            return Err(KernelError::InvalidArgument);
        }
        let base = slot
            .checked_mul(KEY_PTR_LEN)
            .and_then(|v| v.checked_add(HEADER_LEN))
            .and_then(|v| v.checked_add(KEY_LEN))
            .ok_or(KernelError::InvalidArgument)?;
        Ok((
            read_u64(&self.data, base)?,
            read_u64(
                &self.data,
                base.checked_add(8).ok_or(KernelError::InvalidArgument)?,
            )?,
        ))
    }

    /// The payload bytes of leaf slot `slot`.
    ///
    /// Returned as a slice into the block rather than a copy: an
    /// `EXTENT_DATA` item can be the better part of a node when the extent is
    /// inline, and every caller here reads fields out of it and discards it.
    pub fn item_data(&self, slot: usize) -> KernelResult<&[u8]> {
        if !self.header.is_leaf() || slot >= self.nritems() {
            return Err(KernelError::InvalidArgument);
        }
        let base = slot
            .checked_mul(ITEM_LEN)
            .and_then(|v| v.checked_add(HEADER_LEN))
            .ok_or(KernelError::InvalidArgument)?;
        let rel = read_u32(
            &self.data,
            base.checked_add(KEY_LEN)
                .ok_or(KernelError::InvalidArgument)?,
        )?;
        let size = read_u32(
            &self.data,
            base.checked_add(KEY_LEN)
                .and_then(|v| v.checked_add(4))
                .ok_or(KernelError::InvalidArgument)?,
        )?;

        let start = usize::try_from(rel)
            .ok()
            .and_then(|v| v.checked_add(HEADER_LEN))
            .ok_or(KernelError::InvalidArgument)?;
        let end = usize::try_from(size)
            .ok()
            .and_then(|v| start.checked_add(v))
            .ok_or(KernelError::InvalidArgument)?;

        self.data
            .get(start..end)
            .ok_or(KernelError::InvalidArgument)
    }
}

/// Reads tree blocks: a device, the chunk map that addresses it, and the
/// block size they are all read at.
pub struct TreeReader<'a> {
    src: &'a dyn SectorSource,
    map: &'a ChunkMap,
    nodesize: usize,
}

impl<'a> TreeReader<'a> {
    /// Build a reader.
    pub fn new(src: &'a dyn SectorSource, map: &'a ChunkMap, nodesize: usize) -> Self {
        Self { src, map, nodesize }
    }

    /// The chunk map this reader translates through.
    pub fn map(&self) -> &ChunkMap {
        self.map
    }

    /// The device underneath.
    pub fn source(&self) -> &'a dyn SectorSource {
        self.src
    }

    /// Read a logical byte range, requiring it to lie within one chunk.
    pub fn read_logical(&self, logical: u64, len: usize) -> KernelResult<Vec<u8>> {
        let len64 = u64::try_from(len).map_err(|_| KernelError::InvalidArgument)?;
        let physical = self.map.map_range(logical, len64)?;
        read_bytes(self.src, physical, len)
    }

    /// Read and validate the tree block at `logical`.
    ///
    /// `expect_generation` is `None` only for a root named by the superblock,
    /// where there is no parent slot to have recorded one.
    pub fn read_node(&self, logical: u64, expect_generation: Option<u64>) -> KernelResult<Node> {
        let data = self.read_logical(logical, self.nodesize)?;
        verify_csum(&data)?;
        let header = Header::parse(&data)?;

        if header.bytenr != logical {
            return Err(KernelError::IoError);
        }
        if let Some(g) = expect_generation
            && header.generation != g
        {
            return Err(KernelError::IoError);
        }
        if header.level > MAX_LEVEL {
            return Err(KernelError::InvalidArgument);
        }

        let stride = if header.is_leaf() {
            ITEM_LEN
        } else {
            KEY_PTR_LEN
        };
        let slots_end = (header.nritems as usize)
            .checked_mul(stride)
            .and_then(|v| v.checked_add(HEADER_LEN))
            .ok_or(KernelError::InvalidArgument)?;
        if slots_end > self.nodesize {
            return Err(KernelError::InvalidArgument);
        }

        Ok(Node { header, data })
    }
}

/// A descent from a tree's root to one leaf slot.
///
/// Held so that iteration can continue past the end of a leaf: reaching the
/// next item in key order means climbing to the deepest ancestor that still
/// has an unvisited child and descending its leftmost edge, which needs the
/// slots taken on the way down. Btrfs leaves carry no sibling pointer — CoW
/// would have to rewrite both siblings on every split — so the path *is* the
/// iterator's state.
pub struct Path {
    /// `(node, slot)` from root (index 0) to leaf (last).
    levels: Vec<(Node, usize)>,
}

impl Path {
    /// The leaf and the slot currently selected, if the slot is in range.
    ///
    /// `None` means the path has run off the end of the tree, which is the
    /// normal termination of a scan rather than an error.
    pub fn current(&self) -> Option<(&Node, usize)> {
        let (node, slot) = self.levels.last()?;
        if *slot < node.nritems() {
            Some((node, *slot))
        } else {
            None
        }
    }

    /// The key at the current position.
    pub fn current_key(&self) -> KernelResult<Key> {
        let (node, slot) = self.current().ok_or(KernelError::NotFound)?;
        node.key_at(slot)
    }

    /// The item payload at the current position.
    pub fn current_data(&self) -> KernelResult<&[u8]> {
        let (node, slot) = self.current().ok_or(KernelError::NotFound)?;
        node.item_data(slot)
    }
}

impl TreeReader<'_> {
    /// Position a path at the first item with key >= `key`.
    ///
    /// This is `btrfs_search_slot`'s read-only half. It never fails when the
    /// key is absent — it returns the insertion point, because every caller
    /// here either wants an exact item or wants to scan forward from a
    /// prefix, and both are expressed as "start here and walk".
    pub fn search(&self, root: u64, root_generation: Option<u64>, key: &Key) -> KernelResult<Path> {
        let mut levels: Vec<(Node, usize)> = Vec::new();
        let mut logical = root;
        let mut generation = root_generation;

        for _ in 0..=MAX_LEVEL {
            let node = self.read_node(logical, generation)?;

            if node.header.is_leaf() {
                let slot = self.slot_ge(&node, key)?;
                levels.push((node, slot));
                return Ok(Path { levels });
            }

            // In an internal node the target lives under the last child whose
            // key is <= the search key: a child's key is the *smallest* key in
            // its subtree, so the first child with a larger key has already
            // gone past. An empty internal node is corruption — a node with no
            // children cannot be on a path to anything.
            let slot = self.slot_le(&node, key)?;
            let (child, child_gen) = node.child_at(slot)?;
            levels.push((node, slot));
            logical = child;
            generation = Some(child_gen);
        }

        // Fell out of the loop: more than MAX_LEVEL internal nodes, so the
        // levels are not decreasing and the tree is cyclic or corrupt.
        Err(KernelError::InvalidArgument)
    }

    /// Advance to the next item in key order.
    ///
    /// Returns `false` once the tree is exhausted.
    pub fn next(&self, path: &mut Path) -> KernelResult<bool> {
        // Advance within the current leaf if possible.
        if let Some((node, slot)) = path.levels.last_mut() {
            let next = slot.saturating_add(1);
            if next < node.nritems() {
                *slot = next;
                return Ok(true);
            }
            // Mark the leaf exhausted so `current()` is honest even if the
            // climb below fails to find another subtree.
            *slot = node.nritems();
        } else {
            return Ok(false);
        }

        // Climb to the deepest ancestor with an unvisited child.
        let mut depth = path.levels.len().saturating_sub(1);
        loop {
            if depth == 0 {
                return Ok(false);
            }
            depth = depth.saturating_sub(1);
            let (node, slot) = path.levels.get(depth).ok_or(KernelError::InternalError)?;
            let next = slot.saturating_add(1);
            if next < node.nritems() {
                let (child, child_gen) = node.child_at(next)?;
                let entry = path
                    .levels
                    .get_mut(depth)
                    .ok_or(KernelError::InternalError)?;
                entry.1 = next;
                path.levels.truncate(depth.saturating_add(1));
                self.descend_leftmost(path, child, child_gen)?;
                return Ok(true);
            }
        }
    }

    /// Step back to the previous item in key order.
    ///
    /// The mirror of [`Self::next`], and needed for one specific job: a read
    /// at file offset *N* must start from the extent that **contains** N, and
    /// a forward search for `(ino, EXTENT_DATA, N)` lands on the first extent
    /// starting at or after N — which is the wrong one whenever N falls in the
    /// middle of an extent, i.e. almost always. Backing up one item is how
    /// that is corrected. Scanning forward from offset 0 instead would be a
    /// linear walk of every extent before the read.
    ///
    /// Note this is well-defined when the path has run off the end of a leaf
    /// (`slot == nritems`): stepping back from "past the last item" selects
    /// the last item, which is what a caller that overshot wants.
    pub fn prev(&self, path: &mut Path) -> KernelResult<bool> {
        if let Some((_, slot)) = path.levels.last_mut() {
            if *slot > 0 {
                *slot = slot.saturating_sub(1);
                return Ok(true);
            }
        } else {
            return Ok(false);
        }

        let mut depth = path.levels.len().saturating_sub(1);
        loop {
            if depth == 0 {
                return Ok(false);
            }
            depth = depth.saturating_sub(1);
            let (node, slot) = path.levels.get(depth).ok_or(KernelError::InternalError)?;
            if *slot > 0 {
                let prev_slot = slot.saturating_sub(1);
                let (child, child_gen) = node.child_at(prev_slot)?;
                let entry = path
                    .levels
                    .get_mut(depth)
                    .ok_or(KernelError::InternalError)?;
                entry.1 = prev_slot;
                path.levels.truncate(depth.saturating_add(1));
                self.descend_rightmost(path, child, child_gen)?;
                return Ok(true);
            }
        }
    }

    /// Extend `path` from a child pointer down its rightmost edge to a leaf.
    fn descend_rightmost(&self, path: &mut Path, child: u64, generation: u64) -> KernelResult<()> {
        let mut logical = child;
        let mut expect = generation;
        for _ in 0..=MAX_LEVEL {
            let node = self.read_node(logical, Some(expect))?;
            let last = node
                .nritems()
                .checked_sub(1)
                .ok_or(KernelError::InvalidArgument)?;
            if node.header.is_leaf() {
                path.levels.push((node, last));
                return Ok(());
            }
            let (next, next_gen) = node.child_at(last)?;
            path.levels.push((node, last));
            logical = next;
            expect = next_gen;
        }
        Err(KernelError::InvalidArgument)
    }

    /// Extend `path` from a child pointer down its leftmost edge to a leaf.
    fn descend_leftmost(&self, path: &mut Path, child: u64, generation: u64) -> KernelResult<()> {
        let mut logical = child;
        let mut expect = generation;
        for _ in 0..=MAX_LEVEL {
            let node = self.read_node(logical, Some(expect))?;
            if node.header.is_leaf() {
                path.levels.push((node, 0));
                return Ok(());
            }
            let (next, next_gen) = node.child_at(0)?;
            path.levels.push((node, 0));
            logical = next;
            expect = next_gen;
        }
        Err(KernelError::InvalidArgument)
    }

    /// First slot whose key is >= `key` (may equal `nritems`).
    fn slot_ge(&self, node: &Node, key: &Key) -> KernelResult<usize> {
        let n = node.nritems();
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
            if node.key_at(mid)?.sort_tuple() < key.sort_tuple() {
                lo = mid.saturating_add(1);
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    /// Last slot whose key is <= `key`, clamped to 0.
    ///
    /// Clamping matters: a search for a key smaller than everything in the
    /// tree must still descend, because the leftmost leaf is where an
    /// iteration from that key has to begin. Returning "no slot" instead would
    /// make a scan-from-the-start impossible to express.
    fn slot_le(&self, node: &Node, key: &Key) -> KernelResult<usize> {
        let n = node.nritems();
        if n == 0 {
            return Err(KernelError::InvalidArgument);
        }
        let ge = self.slot_ge(node, key)?;
        if ge < n && node.key_at(ge)?.sort_tuple() == key.sort_tuple() {
            return Ok(ge);
        }
        Ok(ge.saturating_sub(1))
    }

    /// Find an item by exact key, or `None`.
    pub fn find(
        &self,
        root: u64,
        root_generation: Option<u64>,
        key: &Key,
    ) -> KernelResult<Option<Path>> {
        let path = self.search(root, root_generation, key)?;
        match path.current() {
            Some((node, slot)) if node.key_at(slot)? == *key => Ok(Some(path)),
            _ => Ok(None),
        }
    }
}
