//! A buddy allocator: kmalloc's memory for a driver process.
//!
//! One region, its size a power of two and its base aligned to it, split in
//! halves down to [`MIN_ORDER`] blocks. A block of order `k` is `2^k` bytes
//! and lies on a `2^k` boundary, so an allocation rounded up to a power of
//! two is aligned to its own size -- Linux's promise for kmalloc (since
//! 5.4), which drivers lean on. Freeing needs only the pointer: each block's
//! order is kept in a byte per minimum block (`orders`), apart from the
//! block, so an aligned block has no header. Free blocks are kept in one
//! doubly linked list per order, threaded through the free blocks
//! themselves, so a freed block's buddy can be taken off its list at once.
//!
//! The region and the order bytes are given by the caller: lazily mapped
//! memory in the driver process ([`crate::sys`]), a vector in the tests.

/// The smallest block: 16 bytes, room for a free block's two links.
pub const MIN_ORDER: u32 = 4;
/// Orders a region may have, from [`MIN_ORDER`] to 2^39 bytes.
const ORDERS: usize = 36;

/// An order byte: the order of the block that starts there, and whether it
/// is free; 0 for no block start.
const FREE: u8 = 0x80;

/// A free block's links, at its start.
#[repr(C)]
struct Links {
    prev: usize,
    next: usize,
}

/// The allocator over one region.
pub struct Buddy {
    /// The region: `base` aligned to its size, `2^top` bytes.
    base: usize,
    top: u32,
    /// One byte per minimum block, `2^(top - MIN_ORDER)` of them.
    orders: *mut u8,
    /// Each order's first free block, 0 for none.
    heads: [usize; ORDERS],
}

/// Why a region cannot be managed.
#[derive(Debug, PartialEq, Eq)]
pub enum RegionError {
    /// Its size is not a power of two between the minimum block and 2^39.
    BadSize,
    /// Its base is not aligned to its size.
    Misaligned,
}

impl Buddy {
    /// Manage `2^top` bytes at `base`, all free, the order of each minimum
    /// block kept in the `2^(top - MIN_ORDER)` bytes at `orders`.
    ///
    /// # Errors
    ///
    /// A region the allocator cannot manage ([`RegionError`]).
    ///
    /// # Safety
    ///
    /// Both ranges are writable memory, the allocator's alone for as long
    /// as it lives; the order bytes read 0 (fresh mapped memory does).
    pub unsafe fn new(base: usize, top: u32, orders: *mut u8) -> Result<Self, RegionError> {
        let size = 1usize.checked_shl(top).ok_or(RegionError::BadSize)?;
        let levels = top
            .checked_sub(MIN_ORDER)
            .and_then(|l| usize::try_from(l).ok())
            .ok_or(RegionError::BadSize)?;
        if levels >= ORDERS {
            return Err(RegionError::BadSize);
        }
        if !base.is_multiple_of(size) {
            return Err(RegionError::Misaligned);
        }
        let mut buddy = Buddy {
            base,
            top,
            orders,
            heads: [0; ORDERS],
        };
        // SAFETY: the whole region is one free block, the caller's memory.
        unsafe { buddy.push(base, top) };
        Ok(buddy)
    }

    /// The order an allocation of `size` bytes takes: the smallest whose
    /// block holds it, and not below [`MIN_ORDER`]; `None` past the region.
    #[must_use]
    pub fn order_for(&self, size: usize) -> Option<u32> {
        let order = size
            .max(1)
            .checked_next_power_of_two()?
            .trailing_zeros()
            .max(MIN_ORDER);
        (order <= self.top).then_some(order)
    }

    /// A block of `size` bytes, aligned to `size` rounded up to a power of
    /// two (at least 16): `None` when no free block is that large.
    pub fn alloc(&mut self, size: usize) -> Option<usize> {
        let order = self.order_for(size)?;
        let mut k = (order..=self.top).find(|&k| self.head(k) != 0)?;
        let block = self.head(k);
        // SAFETY: `block` is the head of order k's free list, a free block
        // of the region.
        unsafe { self.unlink(block, k) };
        while k > order {
            // Above `order`, which is at least MIN_ORDER.
            k = k.wrapping_sub(1);
            // The upper half of the split block, free at order k.
            // SAFETY: inside the block just taken, which is the region's.
            unsafe { self.push(block.wrapping_add(1 << k), k) };
        }
        self.set_meta(block, u8::try_from(order).unwrap_or(0));
        Some(block)
    }

    /// Give back a block [`Self::alloc`] gave, joining it with its buddy
    /// while that is free and of the same order. A pointer that is not an
    /// allocated block's start is refused (`false`) and changes nothing.
    pub fn free(&mut self, ptr: usize) -> bool {
        let Some(meta) = self.meta(ptr) else {
            return false;
        };
        if meta == 0 || meta & FREE != 0 {
            return false;
        }
        let mut block = ptr;
        let mut k = u32::from(meta);
        self.set_meta(block, 0);
        while k < self.top {
            // Inside the region: an offset below 2^top with a bit below
            // `top` flipped.
            let buddy = self
                .base
                .wrapping_add(block.wrapping_sub(self.base) ^ (1 << k));
            if self.meta(buddy) != Some(FREE | u8::try_from(k).unwrap_or(0)) {
                break;
            }
            // SAFETY: `buddy` is a free block of order k, on its list.
            unsafe { self.unlink(buddy, k) };
            self.set_meta(buddy, 0);
            block = block.min(buddy);
            // Below `top`.
            k = k.wrapping_add(1);
        }
        // SAFETY: the joined block is the region's, and no longer given out.
        unsafe { self.push(block, k) };
        true
    }

    /// The bytes the block [`Self::alloc`] gave at `ptr` holds -- what was
    /// asked for, rounded up to the block's size -- or `None` for a pointer
    /// that is not an allocated block's start.
    #[must_use]
    pub fn block_size(&self, ptr: usize) -> Option<usize> {
        let meta = self.meta(ptr)?;
        if meta == 0 || meta & FREE != 0 {
            return None;
        }
        1usize.checked_shl(u32::from(meta))
    }

    /// How many bytes are free, for the tests and for reporting.
    #[must_use]
    pub fn free_bytes(&self) -> usize {
        (MIN_ORDER..=self.top)
            .map(|k| {
                let mut n = 0usize;
                let mut at = self.head(k);
                while at != 0 {
                    // A sum of disjoint blocks of the region: below 2^top.
                    n = n.wrapping_add(1 << k);
                    // SAFETY: a free block on order k's list holds its links.
                    at = unsafe { (*(at as *const Links)).next };
                }
                n
            })
            .sum()
    }

    /// Order `k`'s place in `heads`: `None` below [`MIN_ORDER`].
    fn level(k: u32) -> Option<usize> {
        k.checked_sub(MIN_ORDER)
            .and_then(|l| usize::try_from(l).ok())
    }

    fn head(&self, k: u32) -> usize {
        Self::level(k)
            .and_then(|l| self.heads.get(l))
            .copied()
            .unwrap_or(0)
    }

    fn set_head(&mut self, k: u32, at: usize) {
        if let Some(head) = Self::level(k).and_then(|l| self.heads.get_mut(l)) {
            *head = at;
        }
    }

    /// The order byte of the minimum block at `at`: `None` outside the
    /// region or off a minimum-block boundary.
    fn meta(&self, at: usize) -> Option<u8> {
        let offset = at.checked_sub(self.base)?;
        if offset >> self.top != 0 || offset % (1 << MIN_ORDER) != 0 {
            return None;
        }
        // SAFETY: offset < 2^top, so the index is inside the order bytes.
        Some(unsafe { *self.orders.add(offset >> MIN_ORDER) })
    }

    fn set_meta(&mut self, at: usize, value: u8) {
        if self.meta(at).is_some() {
            // SAFETY: as in `meta`.
            unsafe { *self.orders.add(at.wrapping_sub(self.base) >> MIN_ORDER) = value };
        }
    }

    /// Put the block at `at` on order `k`'s list, marked free.
    ///
    /// # Safety
    ///
    /// `at` is a block of order `k` in the region, on no list and given to
    /// no one.
    unsafe fn push(&mut self, at: usize, k: u32) {
        let head = self.head(k);
        // SAFETY: the caller's: the block is the allocator's to write.
        unsafe {
            (at as *mut Links).write(Links {
                prev: 0,
                next: head,
            });
        }
        if head != 0 {
            // SAFETY: the old head is a free block on this list.
            unsafe { (*(head as *mut Links)).prev = at };
        }
        self.set_head(k, at);
        self.set_meta(at, FREE | u8::try_from(k).unwrap_or(0));
    }

    /// Take the free block at `at` off order `k`'s list.
    ///
    /// # Safety
    ///
    /// `at` is on order `k`'s list.
    unsafe fn unlink(&mut self, at: usize, k: u32) {
        // SAFETY: the caller's: `at` is a listed free block.
        let Links { prev, next } = unsafe { (at as *const Links).read() };
        if prev == 0 {
            self.set_head(k, next);
        } else {
            // SAFETY: a listed free block's neighbours are listed free blocks.
            unsafe { (*(prev as *mut Links)).next = next };
        }
        if next != 0 {
            // SAFETY: as above.
            unsafe { (*(next as *mut Links)).prev = prev };
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation
    )]

    use super::*;

    /// A region of `2^top` bytes aligned to its size, with its order bytes.
    struct Region {
        _memory: Vec<u8>,
        _orders: Vec<u8>,
        buddy: Buddy,
    }

    fn region(top: u32) -> Region {
        let size = 1usize << top;
        let mut memory = vec![0u8; size * 2];
        let base = (memory.as_mut_ptr() as usize).next_multiple_of(size);
        let mut orders = vec![0u8; size >> MIN_ORDER];
        // SAFETY: both vectors live as long as the region, untouched else.
        let buddy = unsafe { Buddy::new(base, top, orders.as_mut_ptr()) }.unwrap();
        Region {
            _memory: memory,
            _orders: orders,
            buddy,
        }
    }

    #[test]
    fn a_block_is_aligned_to_its_size_and_freeing_joins_them_again() {
        let mut r = region(16);
        let total = r.buddy.free_bytes();
        assert_eq!(total, 1 << 16);
        let sizes = [1, 16, 17, 100, 4096, 3000, 8192, 24];
        let blocks: Vec<usize> = sizes.iter().map(|&s| r.buddy.alloc(s).unwrap()).collect();
        for (&size, &block) in sizes.iter().zip(&blocks) {
            let align = size.max(16).next_power_of_two();
            assert_eq!(block % align, 0, "size {size}");
        }
        // No two overlap.
        let mut spans: Vec<(usize, usize)> = sizes
            .iter()
            .zip(&blocks)
            .map(|(&s, &b)| (b, b + s.max(16).next_power_of_two()))
            .collect();
        spans.sort_unstable();
        for pair in spans.windows(2) {
            assert!(pair[0].1 <= pair[1].0);
        }
        for &b in &blocks {
            assert!(r.buddy.free(b));
        }
        assert_eq!(r.buddy.free_bytes(), total);
        // Joined into the one block again: the whole region can be had.
        assert!(r.buddy.alloc(1 << 16).is_some());
    }

    #[test]
    fn a_block_s_size_is_what_its_allocation_was_rounded_to() {
        let mut r = region(16);
        for size in [1, 15, 16, 17, 100, 4096, 4097, 1 << 15] {
            let block = r.buddy.alloc(size).unwrap();
            let want = size.max(16).next_power_of_two();
            assert_eq!(r.buddy.block_size(block), Some(want), "size {size}");
            assert_eq!(
                r.buddy.order_for(size).map(|k| 1usize << k),
                Some(want),
                "size {size}"
            );
            assert!(r.buddy.free(block));
            assert_eq!(
                r.buddy.block_size(block),
                None,
                "a freed block, size {size}"
            );
        }
        let a = r.buddy.alloc(64).unwrap();
        assert_eq!(r.buddy.block_size(a + 16), None, "inside a block");
        assert_eq!(r.buddy.block_size(a + 8), None, "off a 16-byte boundary");
        assert_eq!(r.buddy.block_size(0), None, "outside the region");
        assert_eq!(r.buddy.order_for((1 << 16) + 1), None, "past the region");
    }

    #[test]
    fn what_is_not_an_allocated_block_is_refused() {
        let mut r = region(12);
        let a = r.buddy.alloc(64).unwrap();
        assert!(!r.buddy.free(a + 16), "inside a block");
        assert!(
            !r.buddy.free(a + 8),
            "inside a block, off a 16-byte boundary"
        );
        assert!(!r.buddy.free(a + (1 << 12)), "past the region");
        assert!(!r.buddy.free(a.wrapping_sub(1 << 20)), "before it");
        assert!(r.buddy.free(a));
        assert!(!r.buddy.free(a), "twice");
    }

    #[test]
    fn it_runs_out_and_says_so() {
        let mut r = region(10);
        assert!(r.buddy.alloc(2048).is_none(), "larger than the region");
        let all: Vec<usize> = (0..64).map(|_| r.buddy.alloc(16).unwrap()).collect();
        assert!(r.buddy.alloc(16).is_none());
        assert!(r.buddy.free(all[17]));
        assert_eq!(r.buddy.alloc(16), Some(all[17]));
    }

    #[test]
    fn a_region_it_cannot_manage_is_refused() {
        let mut orders = [0u8; 64];
        // SAFETY: refused before anything is written.
        assert_eq!(
            unsafe { Buddy::new(1 << 20, 3, orders.as_mut_ptr()) }.err(),
            Some(RegionError::BadSize)
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { Buddy::new((1 << 20) + 16, 10, orders.as_mut_ptr()) }.err(),
            Some(RegionError::Misaligned)
        );
    }

    #[test]
    fn a_long_mixed_run_keeps_every_byte() {
        let mut r = region(14);
        let total = r.buddy.free_bytes();
        let mut held: Vec<usize> = Vec::new();
        // A fixed pseudo-random walk: allocate, free every third.
        let mut x = 0x2545_F491_u64;
        for i in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let size = (x % 700) as usize + 1;
            if let Some(b) = r.buddy.alloc(size) {
                held.push(b);
            }
            if i % 3 == 0 && !held.is_empty() {
                let at = (x as usize) % held.len();
                assert!(r.buddy.free(held.swap_remove(at)));
            }
        }
        for b in held {
            assert!(r.buddy.free(b));
        }
        assert_eq!(r.buddy.free_bytes(), total);
    }
}
