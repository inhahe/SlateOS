//! The DMA window's chunks (design-decisions 1586): which of the window's
//! chunks are backed by a device's DMA memory, and where the device sees
//! each.
//!
//! A driver process's DMA window is a range of its address space beside its
//! heap, inside Linux's direct map. It is backed a chunk at a time, in
//! order, as the process's allocations need it: each chunk is one arena of
//! the device's (`SYS_PCI_DEVICE_DMA_ALLOC_AT`), contiguous where the device
//! sees it, so a range inside one chunk has one device address. A range
//! crossing chunks does not -- two arenas need not be adjacent for the
//! device -- and has none.

/// How many chunks a window has, at most.
pub const CHUNKS: usize = 64;

/// The window's chunks: where they begin, their size, and the device
/// address of each one backed so far.
#[derive(Debug, Clone)]
pub struct Chunks {
    base: usize,
    order: u32,
    device: [u64; CHUNKS],
    backed: usize,
}

impl Chunks {
    /// A window at `base` of [`CHUNKS`] chunks of `2^order` bytes, none
    /// backed.
    #[must_use]
    pub const fn new(base: usize, order: u32) -> Self {
        Self {
            base,
            order,
            device: [0; CHUNKS],
            backed: 0,
        }
    }

    /// A chunk's size in bytes.
    #[must_use]
    pub fn chunk_bytes(&self) -> usize {
        1usize.checked_shl(self.order).unwrap_or(0)
    }

    /// Where the next chunk to back begins: `None` once all are.
    #[must_use]
    pub fn next(&self) -> Option<usize> {
        (self.backed < CHUNKS)
            .then(|| self.backed.checked_mul(self.chunk_bytes()))
            .flatten()
            .and_then(|offset| self.base.checked_add(offset))
    }

    /// The next chunk ([`Self::next`]) backed, the device seeing it at
    /// `device_addr`.
    pub fn record(&mut self, device_addr: u64) {
        if let Some(slot) = self.device.get_mut(self.backed) {
            *slot = device_addr;
            self.backed = self.backed.saturating_add(1);
        }
    }

    /// The device address of the `len` bytes at `at`: `None` unless they lie
    /// in one chunk already backed.
    #[must_use]
    pub fn device_address(&self, at: usize, len: usize) -> Option<u64> {
        let offset = at.checked_sub(self.base)?;
        let last = offset.checked_add(len.checked_sub(1)?)?;
        let index = offset >> self.order;
        if last >> self.order != index || index >= self.backed {
            return None;
        }
        let within = offset.checked_sub(index.checked_mul(self.chunk_bytes())?)?;
        self.device
            .get(index)?
            .checked_add(u64::try_from(within).ok()?)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

    use super::*;

    const BASE: usize = 0x5000_0000;
    const ORDER: u32 = 22;
    const MIB4: usize = 1 << 22;

    #[test]
    fn chunks_are_backed_in_order() {
        let mut c = Chunks::new(BASE, ORDER);
        assert_eq!(c.chunk_bytes(), MIB4);
        assert_eq!(c.next(), Some(BASE));
        c.record(0x1_0000_0000);
        assert_eq!(c.next(), Some(BASE + MIB4));
        c.record(0x8000_0000);
        assert_eq!(c.next(), Some(BASE + 2 * MIB4));
        for _ in 2..CHUNKS {
            c.record(0x9000_0000);
        }
        assert_eq!(c.next(), None, "all backed");
        c.record(0xdead);
        assert_eq!(c.next(), None, "a record past the last changes nothing");
    }

    #[test]
    fn a_range_in_one_backed_chunk_has_its_device_address() {
        let mut c = Chunks::new(BASE, ORDER);
        c.record(0x1_0000_0000);
        c.record(0x8000_0000);
        assert_eq!(c.device_address(BASE, 1), Some(0x1_0000_0000));
        assert_eq!(c.device_address(BASE + 0x1234, 64), Some(0x1_0000_1234));
        assert_eq!(
            c.device_address(BASE + MIB4 - 8, 8),
            Some(0x1_0000_0000 + MIB4 as u64 - 8)
        );
        assert_eq!(
            c.device_address(BASE + MIB4 + 16, 16),
            Some(0x8000_0010),
            "the second chunk's own"
        );
    }

    #[test]
    fn anything_else_has_none() {
        let mut c = Chunks::new(BASE, ORDER);
        c.record(0x1_0000_0000);
        assert_eq!(
            c.device_address(BASE + MIB4 - 8, 16),
            None,
            "crossing into the next"
        );
        assert_eq!(c.device_address(BASE + MIB4, 8), None, "a chunk not backed");
        assert_eq!(c.device_address(BASE - 1, 1), None, "below the window");
        assert_eq!(c.device_address(BASE, 0), None, "nothing");
        assert_eq!(
            c.device_address(usize::MAX, 2),
            None,
            "past the address space"
        );
    }
}
