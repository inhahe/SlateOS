//! An ext4 file's extent map as a sorted list, and the edits the driver
//! makes to it: cutting it at a new end, filling holes, turning unwritten
//! extents written.
//!
//! The driver reads a file's whole extent tree into this list
//! (`Ext4Driver::read_extent_map`), edits the list, and writes a tree built
//! afresh from it into newly allocated blocks (`Ext4Driver::write_extent_tree`).
//! Everything here is plain list manipulation, checkable without a disk
//! ([`self_test`]).
//!
//! # Why a rebuilt tree rather than in-place surgery
//!
//! ext4's own in-place operations -- split an extent inside a full leaf,
//! insert in the middle, grow the tree a level -- are where its subtlety
//! lives, and this driver writes no journal: its crash safety comes from
//! ordering alone (new blocks written before the inode points at them, old
//! ones freed after). A tree built afresh in new blocks has that ordering by
//! construction. The cost is rewriting one leaf block per 340 extents (4 KiB
//! blocks) on each change; a file of four extents or fewer -- nearly every
//! file -- keeps its whole map in the inode and rewrites no block at all.
//!
//! # `ee_len`
//!
//! The ext4 specification: a value up to 32768 is an *initialized* extent of
//! that many blocks; a value above it is *unwritten* (preallocated, reads as
//! zeros), of `ee_len - 32768` blocks. [`decode_len`] and [`encode_len`] are
//! the only places that know; the driver used to mask with `0x7FFF`, which
//! read a full-length initialized extent (32768 blocks) as an empty
//! unwritten one.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

/// Longest initialized extent `ee_len` can describe.
pub const INIT_MAX_LEN: u64 = 32_768;

/// Longest unwritten extent `ee_len` can describe.
pub const UNWRITTEN_MAX_LEN: u64 = 32_767;

/// Deepest tree ext4 allows (`EXT4_MAX_EXTENT_DEPTH`).
pub const MAX_DEPTH: u16 = 5;

/// Decode an on-disk `ee_len`: `(blocks, unwritten)`.
#[must_use]
pub const fn decode_len(ee_len: u16) -> (u64, bool) {
    if ee_len as u64 <= INIT_MAX_LEN {
        (ee_len as u64, false)
    } else {
        // > 32768, so the subtraction cannot underflow.
        ((ee_len as u64).wrapping_sub(INIT_MAX_LEN), true)
    }
}

/// Encode `(blocks, unwritten)` as an on-disk `ee_len`; `None` if `blocks`
/// is 0 or more than the kind of extent can hold.
#[must_use]
pub fn encode_len(blocks: u64, unwritten: bool) -> Option<u16> {
    if blocks == 0 {
        return None;
    }
    let max = if unwritten {
        UNWRITTEN_MAX_LEN
    } else {
        INIT_MAX_LEN
    };
    if blocks > max {
        return None;
    }
    let raw = if unwritten {
        blocks.checked_add(INIT_MAX_LEN)?
    } else {
        blocks
    };
    u16::try_from(raw).ok()
}

/// One extent: `len` blocks of the file from logical block `logical` live at
/// physical blocks `phys .. phys + len`. An `unwritten` extent's blocks are
/// allocated but read as zeros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    /// First logical block (file block number).
    pub logical: u64,
    /// Blocks covered.
    pub len: u64,
    /// First physical block.
    pub phys: u64,
    /// Preallocated and never written: reads as zeros.
    pub unwritten: bool,
}

impl Extent {
    /// One past the last logical block.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.logical.saturating_add(self.len)
    }

    /// The longest extent of this kind.
    #[must_use]
    pub const fn max_len(&self) -> u64 {
        if self.unwritten {
            UNWRITTEN_MAX_LEN
        } else {
            INIT_MAX_LEN
        }
    }
}

/// What a run of logical blocks holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    /// No blocks: reads as zeros, a write must allocate.
    Hole {
        /// First logical block.
        logical: u64,
        /// Blocks.
        len: u64,
    },
    /// Initialized blocks.
    Written {
        /// First logical block.
        logical: u64,
        /// Blocks.
        len: u64,
        /// First physical block.
        phys: u64,
    },
    /// Allocated, reads as zeros; a write must mark them written.
    Unwritten {
        /// First logical block.
        logical: u64,
        /// Blocks.
        len: u64,
        /// First physical block.
        phys: u64,
    },
}

/// Check that `map` is sorted by logical block and that no two extents
/// overlap or are empty: what every function here assumes of its input, and
/// what an extent tree read from disk must satisfy before the driver trusts
/// it (`IoError` otherwise -- the tree is corrupt).
///
/// # Errors
///
/// [`KernelError::IoError`] if the map is not a valid extent map.
pub fn validate(map: &[Extent]) -> KernelResult<()> {
    let mut prev_end = 0u64;
    for (i, e) in map.iter().enumerate() {
        if e.len == 0 || e.phys == 0 || e.logical.checked_add(e.len).is_none() {
            return Err(KernelError::IoError);
        }
        if i > 0 && e.logical < prev_end {
            return Err(KernelError::IoError);
        }
        prev_end = e.end();
    }
    Ok(())
}

/// Classify logical blocks `[first, first + count)`, in order.
#[must_use]
pub fn classify(map: &[Extent], first: u64, count: u64) -> Vec<Span> {
    let end = first.saturating_add(count);
    let mut out = Vec::new();
    let mut cur = first;
    let mut i = map.partition_point(|e| e.end() <= first);
    while cur < end {
        match map.get(i) {
            Some(e) if e.logical <= cur => {
                let stop = e.end().min(end);
                let len = stop.saturating_sub(cur);
                let phys = e.phys.saturating_add(cur.saturating_sub(e.logical));
                out.push(if e.unwritten {
                    Span::Unwritten {
                        logical: cur,
                        len,
                        phys,
                    }
                } else {
                    Span::Written {
                        logical: cur,
                        len,
                        phys,
                    }
                });
                cur = stop;
                i = i.saturating_add(1);
            }
            Some(e) => {
                let stop = e.logical.min(end);
                out.push(Span::Hole {
                    logical: cur,
                    len: stop.saturating_sub(cur),
                });
                cur = stop;
            }
            None => {
                out.push(Span::Hole {
                    logical: cur,
                    len: end.saturating_sub(cur),
                });
                cur = end;
            }
        }
    }
    out
}

/// Cut the map so that nothing reaches logical block `new_blocks` or past
/// it. Returns the physical ranges `(start, blocks)` no longer mapped, for
/// the caller to free once the shorter tree is on disk.
pub fn truncate(map: &mut Vec<Extent>, new_blocks: u64) -> Vec<(u64, u64)> {
    let mut freed = Vec::new();
    map.retain_mut(|e| {
        if e.logical >= new_blocks {
            freed.push((e.phys, e.len));
            false
        } else if e.end() > new_blocks {
            let keep = new_blocks.saturating_sub(e.logical);
            freed.push((e.phys.saturating_add(keep), e.len.saturating_sub(keep)));
            e.len = keep;
            true
        } else {
            true
        }
    });
    freed
}

/// Map `new`'s logical range to `new`'s blocks, replacing whatever covered
/// any part of it -- a hole being filled, or an unwritten extent being
/// marked written (same physical blocks, `unwritten: false`). The callers
/// only ever replace holes and unwritten ranges; the blocks of a replaced
/// unwritten range are `new`'s own, so nothing is freed here.
pub fn set(map: &mut Vec<Extent>, new: Extent) {
    let new_end = new.end();
    let mut out: Vec<Extent> = Vec::with_capacity(map.len().saturating_add(2));
    for e in map.drain(..) {
        if e.end() <= new.logical || e.logical >= new_end {
            out.push(e);
            continue;
        }
        if e.logical < new.logical {
            out.push(Extent {
                logical: e.logical,
                len: new.logical.saturating_sub(e.logical),
                phys: e.phys,
                unwritten: e.unwritten,
            });
        }
        if e.end() > new_end {
            let skip = new_end.saturating_sub(e.logical);
            out.push(Extent {
                logical: new_end,
                len: e.end().saturating_sub(new_end),
                phys: e.phys.saturating_add(skip),
                unwritten: e.unwritten,
            });
        }
    }
    out.push(new);
    out.sort_unstable_by_key(|e| e.logical);
    *map = out;
    normalize(map);
}

/// Merge neighbours that continue each other -- logically and physically
/// adjacent, of the same kind -- then split any extent longer than its kind
/// can hold. The result is the fewest entries that describe the map.
pub fn normalize(map: &mut Vec<Extent>) {
    let mut merged: Vec<Extent> = Vec::with_capacity(map.len());
    for e in map.drain(..) {
        if let Some(prev) = merged.last_mut() {
            if prev.end() == e.logical
                && prev.phys.saturating_add(prev.len) == e.phys
                && prev.unwritten == e.unwritten
            {
                prev.len = prev.len.saturating_add(e.len);
                continue;
            }
        }
        merged.push(e);
    }
    for e in merged {
        let max = e.max_len();
        let mut done = 0u64;
        while done < e.len {
            let len = (e.len.saturating_sub(done)).min(max);
            map.push(Extent {
                logical: e.logical.saturating_add(done),
                len,
                phys: e.phys.saturating_add(done),
                unwritten: e.unwritten,
            });
            done = done.saturating_add(len);
        }
    }
}

/// The shape of a tree holding `n` extents: its depth (the root's
/// `eh_depth`) and the number of blocks on each level below the root,
/// leaves first. The root -- the inode's `i_block` -- holds `root_cap`
/// entries and every block `node_cap`. `None` if `n` needs a tree deeper
/// than ext4 allows.
#[must_use]
pub fn tree_shape(n: usize, root_cap: usize, node_cap: usize) -> Option<(u16, Vec<usize>)> {
    if n <= root_cap {
        return Some((0, Vec::new()));
    }
    if node_cap == 0 {
        return None;
    }
    let mut levels = Vec::new();
    let mut count = n.div_ceil(node_cap);
    levels.push(count);
    let mut depth: u16 = 1;
    while count > root_cap {
        count = count.div_ceil(node_cap);
        levels.push(count);
        depth = depth.saturating_add(1);
        if depth > MAX_DEPTH {
            return None;
        }
    }
    Some((depth, levels))
}

/// The first offset at or after `offset` that is in data (`want_data`) or in
/// a hole, for `SEEK_DATA` / `SEEK_HOLE`, in a file of `size` bytes with
/// `block_size`-byte blocks. Unwritten extents are holes, as Linux reports
/// them: they read as zeros and hold no data. The end of the file counts as
/// a hole. `None` for `SEEK_DATA` with no data at or after `offset` (Linux's
/// `ENXIO`).
#[must_use]
pub fn seek_data_hole(
    map: &[Extent],
    block_size: u64,
    size: u64,
    offset: u64,
    want_data: bool,
) -> Option<u64> {
    if offset >= size || block_size == 0 {
        return None;
    }
    let mut block = offset / block_size;
    let last = size.saturating_sub(1) / block_size;
    let in_data = |b: u64| -> Option<&Extent> {
        let i = map.partition_point(|e| e.end() <= b);
        map.get(i).filter(|e| e.logical <= b && !e.unwritten)
    };
    loop {
        if block > last {
            return if want_data { None } else { Some(size) };
        }
        match in_data(block) {
            Some(_) if want_data => {
                return Some((block.saturating_mul(block_size)).max(offset));
            }
            Some(e) => block = e.end(),
            None if !want_data => {
                return Some((block.saturating_mul(block_size)).max(offset).min(size));
            }
            None => {
                // Skip to the next written extent at or after `block`.
                let i = map.partition_point(|e| e.end() <= block);
                match map
                    .get(i..)
                    .and_then(|rest| rest.iter().find(|e| !e.unwritten))
                {
                    Some(e) => block = e.logical.max(block),
                    None => return None,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test (pure: no disk)
// ---------------------------------------------------------------------------

fn ext(logical: u64, len: u64, phys: u64, unwritten: bool) -> Extent {
    Extent {
        logical,
        len,
        phys,
        unwritten,
    }
}

fn check(ok: bool, what: &str) -> KernelResult<()> {
    if ok {
        Ok(())
    } else {
        serial_println!("[ext4]   FAIL: extent map: {}", what);
        Err(KernelError::InternalError)
    }
}

/// Pure checks of the extent-map edits, run at boot.
///
/// # Errors
///
/// [`KernelError::InternalError`] if an edit is wrong.
#[allow(clippy::too_many_lines)]
pub fn self_test() -> KernelResult<()> {
    // ee_len, as the specification has it -- 32768 is a full initialized
    // extent, not an empty unwritten one.
    check(decode_len(1) == (1, false), "ee_len 1")?;
    check(
        decode_len(32_768) == (32_768, false),
        "ee_len 32768 is initialized",
    )?;
    check(
        decode_len(32_769) == (1, true),
        "ee_len 32769 is unwritten, 1 block",
    )?;
    check(decode_len(65_535) == (32_767, true), "ee_len 65535")?;
    check(
        encode_len(32_768, false) == Some(32_768),
        "encode 32768 written",
    )?;
    check(
        encode_len(32_767, true) == Some(65_535),
        "encode 32767 unwritten",
    )?;
    check(
        encode_len(32_768, true).is_none(),
        "32768 unwritten does not fit",
    )?;
    check(
        encode_len(0, false).is_none(),
        "an empty extent is not encodable",
    )?;
    for (blocks, unwritten) in [(1, false), (77, true), (32_768, false), (32_767, true)] {
        let round = encode_len(blocks, unwritten).map(decode_len);
        check(
            round == Some((blocks, unwritten)),
            "encode/decode round trip",
        )?;
    }

    // classify: hole, written, unwritten, hole.
    let map = [ext(2, 3, 100, false), ext(5, 2, 200, true)];
    let spans = classify(&map, 0, 9);
    check(
        spans
            == [
                Span::Hole { logical: 0, len: 2 },
                Span::Written {
                    logical: 2,
                    len: 3,
                    phys: 100,
                },
                Span::Unwritten {
                    logical: 5,
                    len: 2,
                    phys: 200,
                },
                Span::Hole { logical: 7, len: 2 },
            ],
        "classify a hole, a written and an unwritten extent",
    )?;
    check(
        classify(&map, 3, 1)
            == [Span::Written {
                logical: 3,
                len: 1,
                phys: 101,
            }],
        "classify from inside an extent",
    )?;

    // truncate: keep a prefix of the straddling extent, drop the rest.
    let mut m = alloc::vec![
        ext(0, 4, 100, false),
        ext(10, 6, 300, false),
        ext(20, 2, 400, true)
    ];
    let freed = truncate(&mut m, 12);
    check(
        m == [ext(0, 4, 100, false), ext(10, 2, 300, false)],
        "truncate keeps a prefix",
    )?;
    check(
        freed == [(302, 4), (400, 2)],
        "truncate frees the tail and the rest",
    )?;
    let freed = truncate(&mut m, 0);
    check(
        m.is_empty() && freed == [(100, 4), (300, 2)],
        "truncate to zero frees all",
    )?;

    // set: fill a hole between two extents that it continues -- one extent.
    let mut m = alloc::vec![ext(0, 2, 100, false), ext(4, 2, 104, false)];
    set(&mut m, ext(2, 2, 102, false));
    check(
        m == [ext(0, 6, 100, false)],
        "a filled hole merges with both neighbours",
    )?;

    // set: mark the middle of an unwritten extent written -- three pieces.
    let mut m = alloc::vec![ext(0, 10, 500, true)];
    set(&mut m, ext(3, 2, 503, false));
    check(
        m == [
            ext(0, 3, 500, true),
            ext(3, 2, 503, false),
            ext(5, 5, 505, true),
        ],
        "writing inside an unwritten extent splits it in three",
    )?;
    // ...and writing the rest makes it one written extent again.
    set(&mut m, ext(0, 3, 500, false));
    set(&mut m, ext(5, 5, 505, false));
    check(
        m == [ext(0, 10, 500, false)],
        "writing all of it merges back",
    )?;

    // normalize: merge adjacent, split an overlong one at the kind's limit.
    let mut m = alloc::vec![
        ext(0, 32_000, 1_000, false),
        ext(32_000, 1_000, 33_000, false),
    ];
    normalize(&mut m);
    check(
        m == [
            ext(0, 32_768, 1_000, false),
            ext(32_768, 232, 33_768, false),
        ],
        "normalize merges and splits at 32768",
    )?;
    let mut m = alloc::vec![ext(0, 40_000, 7_000, true)];
    normalize(&mut m);
    check(
        m == [
            ext(0, 32_767, 7_000, true),
            ext(32_767, 7_233, 39_767, true),
        ],
        "an unwritten extent splits at 32767",
    )?;
    let mut m = alloc::vec![ext(0, 2, 100, false), ext(2, 2, 102, true)];
    normalize(&mut m);
    check(m.len() == 2, "written and unwritten neighbours stay apart")?;

    // validate: overlap and disorder are corruption.
    check(
        validate(&[ext(0, 4, 1, false), ext(4, 1, 9, false)]).is_ok(),
        "valid map",
    )?;
    check(
        validate(&[ext(0, 4, 1, false), ext(3, 1, 9, false)]).is_err(),
        "overlap",
    )?;
    check(
        validate(&[ext(5, 1, 1, false), ext(0, 1, 9, false)]).is_err(),
        "disorder",
    )?;
    check(
        validate(&[ext(0, 1, 0, false)]).is_err(),
        "physical block 0",
    )?;

    // tree_shape: 4 in the inode; 340 per block (4 KiB blocks).
    check(
        tree_shape(4, 4, 340) == Some((0, Vec::new())),
        "four extents fit the inode",
    )?;
    check(
        tree_shape(5, 4, 340) == Some((1, alloc::vec![1])),
        "five need a leaf",
    )?;
    check(
        tree_shape(1_360, 4, 340) == Some((1, alloc::vec![4])),
        "four full leaves, depth 1",
    )?;
    check(
        tree_shape(1_361, 4, 340) == Some((2, alloc::vec![5, 1])),
        "a fifth leaf, depth 2",
    )?;

    // seek_data_hole: data at blocks 2..5, an unwritten 5..7, size 9 blocks.
    let bs = 4_096u64;
    let map = [ext(2, 3, 100, false), ext(5, 2, 200, true)];
    let size = 9 * bs;
    check(
        seek_data_hole(&map, bs, size, 0, true) == Some(2 * bs),
        "SEEK_DATA from 0",
    )?;
    check(
        seek_data_hole(&map, bs, size, 0, false) == Some(0),
        "SEEK_HOLE from 0",
    )?;
    check(
        seek_data_hole(&map, bs, size, 2 * bs + 10, false) == Some(5 * bs),
        "SEEK_HOLE from inside data: the unwritten extent is a hole",
    )?;
    check(
        seek_data_hole(&map, bs, size, 5 * bs, true).is_none(),
        "no data after 5 blocks",
    )?;
    check(
        seek_data_hole(&map, bs, size, size, false).is_none(),
        "at EOF: ENXIO",
    )?;
    check(
        seek_data_hole(&[], bs, 3 * bs, 0, false) == Some(0),
        "a sparse file is a hole",
    )?;
    check(
        seek_data_hole(&[], bs, 3 * bs, 0, true).is_none(),
        "and has no data",
    )?;
    let full = [ext(0, 3, 100, false)];
    check(
        seek_data_hole(&full, bs, 3 * bs, bs, false) == Some(3 * bs),
        "a dense file's only hole is its end",
    )?;

    serial_println!(
        "[ext4]   extent map edits (ee_len, classify, truncate, set, normalize, shape, seek): OK"
    );
    Ok(())
}
