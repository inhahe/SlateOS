//! A PCI function's BARs as a driver process's Linux sees them
//! (design-decisions 1584): shadowed, because the kernel's device handle
//! refuses to write them -- the kernel sized and placed them when it opened
//! the function.
//!
//! Linux sizes a BAR as hardware is sized: it writes all ones and reads the
//! register back, and the bits that stayed 0 below the address are the size.
//! So the registers answer here as the hardware would after what Linux wrote:
//! a memory BAR's address bits below its size read 0, its type bits are the
//! device's, and the rest is what Linux last wrote -- kept here, never
//! reaching the device, so that the BAR stays where the kernel put it. A BAR
//! that Linux has written somewhere else reads so, and [`Bars::window`] says
//! it was moved: the device would not follow it. An I/O BAR,
//! an absent one, and the expansion ROM read as 0: the handle offers none.

/// How many BARs a type-0 function has.
pub const COUNT: usize = 6;
/// The first BAR's offset in configuration space, and the ROM's.
pub const FIRST: u16 = 0x10;
pub const ROM: u16 = 0x30;
/// A memory BAR's type bits: below the address, read-only.
const TYPE_BITS: u32 = 0xf;
/// The type field's value for a 64-bit memory BAR (bits 2:1 = 0b10).
const TYPE_64: u32 = 0b100;
const TYPE_MASK: u32 = 0b110;

/// One function's BARs: what the registers hold, the memory BARs' sizes, and
/// what Linux has written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bars {
    real: [u32; COUNT],
    shadow: [u32; COUNT],
    /// A memory BAR's size; 0 for none, an I/O BAR, or a 64-bit BAR's upper
    /// half.
    size: [u64; COUNT],
    /// Whether BAR `i` is the upper half of the 64-bit BAR `i - 1`.
    upper: [bool; COUNT],
}

impl Bars {
    /// The BARs of a function whose registers hold `real` and whose memory
    /// BARs are `size` bytes (0 where there is none): as the kernel reports
    /// them, each a power of two of at least 16.
    #[must_use]
    pub fn new(real: [u32; COUNT], size: [u64; COUNT]) -> Self {
        let mut bars = Self {
            real,
            shadow: real,
            size,
            upper: [false; COUNT],
        };
        let mut i = 0;
        while i < COUNT {
            let next = i.saturating_add(1);
            if bars.is_64(i)
                && let (Some(upper), Some(size)) =
                    (bars.upper.get_mut(next), bars.size.get_mut(next))
            {
                *upper = true;
                *size = 0;
                i = next;
            }
            i = i.saturating_add(1);
        }
        bars
    }

    fn is_64(&self, i: usize) -> bool {
        self.size.get(i).is_some_and(|&s| s != 0)
            && self.real.get(i).is_some_and(|&r| r & TYPE_MASK == TYPE_64)
    }

    /// BAR `i`'s register as the hardware would read it after what was
    /// written.
    fn register(&self, i: usize) -> u32 {
        let (Some(&shadow), Some(&real)) = (self.shadow.get(i), self.real.get(i)) else {
            return 0;
        };
        if self.upper.get(i).copied().unwrap_or(false) {
            let below = i
                .checked_sub(1)
                .and_then(|b| self.size.get(b))
                .copied()
                .unwrap_or(0);
            // The address bits below the size, above bit 32.
            let fixed = u32::try_from(below.saturating_sub(1) >> 32).unwrap_or(u32::MAX);
            return shadow & !fixed;
        }
        let size = self.size.get(i).copied().unwrap_or(0);
        if size == 0 {
            return 0;
        }
        let fixed = u32::try_from(size.saturating_sub(1)).unwrap_or(u32::MAX);
        (shadow & !fixed & !TYPE_BITS) | (real & TYPE_BITS)
    }

    /// The register at `offset` this answers for -- a BAR's, or the ROM's --
    /// and the shift of `offset` within it; `None` for any other offset.
    fn slot(offset: u16) -> Option<(Option<usize>, u32)> {
        let shift = u32::from(offset & 3).saturating_mul(8);
        let aligned = offset & !3;
        if aligned == ROM {
            return Some((None, shift));
        }
        let index = usize::from(aligned.checked_sub(FIRST)? / 4);
        (index < COUNT).then_some((Some(index), shift))
    }

    /// A configuration read of `width` bytes (1, 2 or 4) at `offset`, if it
    /// is of the registers kept here: the value; `None` for any other.
    #[must_use]
    pub fn read(&self, offset: u16, width: u32) -> Option<u32> {
        let (index, shift) = Self::slot(offset)?;
        let dword = index.map_or(0, |i| self.register(i));
        Some((dword >> shift) & mask(width))
    }

    /// A configuration write of `width` bytes at `offset`: kept, and `true`,
    /// if it is of the registers kept here (a write to the ROM is dropped);
    /// `false` for any other, which is the device's.
    pub fn write(&mut self, offset: u16, width: u32, value: u32) -> bool {
        let Some((index, shift)) = Self::slot(offset) else {
            return false;
        };
        if let Some(shadow) = index.and_then(|i| self.shadow.get_mut(i)) {
            let bits = mask(width) << shift;
            *shadow = (*shadow & !bits) | ((value << shift) & bits);
        }
        true
    }

    /// Where memory BAR `i` is -- where the kernel placed it, which the
    /// device decodes -- how large, and whether Linux has written it
    /// elsewhere since (a mapping where Linux then thinks it is would reach
    /// nothing); `None` for no memory BAR.
    #[must_use]
    pub fn window(&self, i: usize) -> Option<Window> {
        let size = self.size.get(i).copied().unwrap_or(0);
        let real = *self.real.get(i).filter(|_| size != 0)?;
        let mut base = u64::from(real & !TYPE_BITS);
        let mut in_place = self.register(i) == real;
        if self.is_64(i) {
            let upper = i.saturating_add(1);
            base |= u64::from(self.real.get(upper).copied().unwrap_or(0)) << 32;
            in_place &= self.register(upper) == self.real.get(upper).copied().unwrap_or(0);
        }
        Some(Window {
            base,
            size,
            moved: !in_place,
        })
    }
}

/// A memory BAR's place ([`Bars::window`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Where the kernel placed it, and the device decodes it.
    pub base: u64,
    pub size: u64,
    /// Whether Linux has written it elsewhere since.
    pub moved: bool,
}

/// The low `width` bytes of a register.
fn mask(width: u32) -> u32 {
    match width {
        1 => 0xff,
        2 => 0xffff,
        _ => u32::MAX,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

    use super::*;

    /// What `window()` answers for a BAR where the kernel placed it.
    #[allow(clippy::unnecessary_wraps)] // Compared with `window()`'s own answer.
    fn placed(base: u64, size: u64) -> Option<Window> {
        Some(Window {
            base,
            size,
            moved: false,
        })
    }

    /// An 82574's BARs, as QEMU shows one: BAR0 and BAR1 32-bit memory,
    /// 128 KiB each; BAR2 I/O (no size from the kernel); BAR3 memory, 16 KiB.
    fn e1000e() -> Bars {
        Bars::new(
            [0x8916_0000, 0x8914_0000, 0xc001, 0x891c_8000, 0, 0],
            [0x2_0000, 0x2_0000, 0, 0x4000, 0, 0],
        )
    }

    /// A 64-bit prefetchable 8 GiB BAR at 0x4_0000_0000, at BAR0/1, and a
    /// 64-bit 64 KiB BAR at 0x1_2345_0000 at BAR2/3.
    fn wide() -> Bars {
        Bars::new(
            [0x0000_000c, 0x4, 0x2345_0004, 0x1, 0, 0],
            [1 << 33, 0, 0x1_0000, 0, 0, 0],
        )
    }

    /// Linux's own sizing of BAR `i`: all ones written, read back, restored.
    fn size_as_linux(bars: &mut Bars, i: u16) -> (u32, u32) {
        let at = FIRST + 4 * i;
        let was = bars.read(at, 4).unwrap();
        assert!(bars.write(at, 4, u32::MAX));
        let sized = bars.read(at, 4).unwrap();
        assert!(bars.write(at, 4, was));
        (was, sized)
    }

    #[test]
    fn a_memory_bar_reads_as_it_is_and_sizes_as_hardware_does() {
        let mut b = e1000e();
        let (was, sized) = size_as_linux(&mut b, 0);
        assert_eq!(was, 0x8916_0000);
        assert_eq!(sized, !(0x2_0000u32 - 1) & !0xf);
        assert_eq!(b.read(FIRST, 4), Some(0x8916_0000), "restored");
        assert_eq!(b.window(0), placed(0x8916_0000, 0x2_0000));
        let (_, sized) = size_as_linux(&mut b, 3);
        assert_eq!(sized, !(0x4000u32 - 1) & !0xf);
    }

    #[test]
    fn the_type_bits_are_the_device_s() {
        let mut b = Bars::new([0x8000_0008, 0, 0, 0, 0, 0], [0x1000, 0, 0, 0, 0, 0]);
        let (_, sized) = size_as_linux(&mut b, 0);
        assert_eq!(sized & 0xf, 0x8, "prefetchable stays so");
        assert!(b.write(FIRST, 4, 0));
        assert_eq!(b.read(FIRST, 4).unwrap() & 0xf, 0x8);
    }

    #[test]
    fn an_io_bar_an_absent_one_and_the_rom_read_as_nothing() {
        let mut b = e1000e();
        assert_eq!(b.read(FIRST + 8, 4), Some(0), "the I/O BAR");
        assert_eq!(size_as_linux(&mut b, 2), (0, 0));
        assert_eq!(b.read(FIRST + 16, 4), Some(0), "absent");
        assert_eq!(b.window(2), None);
        assert_eq!(b.window(4), None);
        assert!(b.write(ROM, 4, !1));
        assert_eq!(b.read(ROM, 4), Some(0), "the ROM, sized");
    }

    #[test]
    fn a_64_bit_bar_sizes_across_both_halves() {
        let mut b = wide();
        let (lo_was, lo) = size_as_linux(&mut b, 0);
        let (hi_was, hi) = size_as_linux(&mut b, 1);
        assert_eq!((lo_was, hi_was), (0xc, 0x4));
        // 8 GiB: every low address bit fixed, the upper's bit 0 too.
        assert_eq!(lo, 0xc);
        assert_eq!(hi, !1);
        let size = !((u64::from(hi) << 32) | u64::from(lo & !0xf)) + 1;
        assert_eq!(size, 1 << 33);
        assert_eq!(b.window(0), placed(0x4_0000_0000, 1 << 33));
        assert_eq!(b.window(1), None, "an upper half is no BAR of its own");
        let (_, lo) = size_as_linux(&mut b, 2);
        let (_, hi) = size_as_linux(&mut b, 3);
        assert_eq!(lo, !(0x1_0000u32 - 1) & !0xf | 0x4);
        assert_eq!(hi, u32::MAX);
        assert_eq!(b.window(2), placed(0x1_2345_0000, 0x1_0000));
    }

    #[test]
    fn a_bar_linux_moved_is_refused_and_a_write_back_restores_it() {
        let mut b = e1000e();
        assert!(b.write(FIRST, 4, 0xa000_0000));
        assert_eq!(b.read(FIRST, 4), Some(0xa000_0000));
        assert_eq!(
            b.window(0),
            Some(Window {
                base: 0x8916_0000,
                size: 0x2_0000,
                moved: true
            }),
            "where it really is, and that it moved"
        );
        assert!(b.write(FIRST, 4, 0x8916_0000));
        assert_eq!(b.window(0), placed(0x8916_0000, 0x2_0000));
        let mut w = wide();
        // Bit 0 of the upper half is below the 8 GiB size: written, it still
        // reads 0, and the BAR has not moved.
        assert!(w.write(FIRST + 4, 4, 0x5));
        assert_eq!(w.window(0), placed(0x4_0000_0000, 1 << 33));
        assert!(w.write(FIRST + 4, 4, 0x6));
        assert!(w.window(0).unwrap().moved, "the upper half moved");
    }

    #[test]
    fn narrow_accesses_reach_their_bytes() {
        let mut b = e1000e();
        assert_eq!(b.read(FIRST + 2, 2), Some(0x8916));
        assert_eq!(b.read(FIRST, 2), Some(0x0000), "the low word alone");
        assert_eq!(b.read(FIRST + 3, 1), Some(0x89));
        assert_eq!(b.read(FIRST + 2, 1), Some(0x16), "one byte alone");
        assert_eq!(b.read(FIRST + 1, 1), Some(0x00));
        assert!(b.write(FIRST + 2, 2, 0xa000));
        assert_eq!(b.read(FIRST, 4), Some(0xa000_0000));
        // A narrow write leaves the bytes beside it.
        assert!(b.write(FIRST + 2, 1, 0x5a));
        assert_eq!(b.read(FIRST, 4), Some(0xa05a_0000));
        assert!(b.write(FIRST, 2, 0xffff));
        assert_eq!(
            b.read(FIRST, 4),
            Some(0xa05a_0000),
            "the low word, below the size, fixed"
        );
    }

    #[test]
    fn other_registers_are_the_device_s() {
        let mut b = e1000e();
        assert_eq!(b.read(0x04, 2), None, "the command register");
        assert_eq!(b.read(FIRST - 4, 4), None);
        assert_eq!(
            b.read(FIRST + 24, 4),
            None,
            "the cardbus pointer, past the BARs"
        );
        assert!(!b.write(0x04, 2, 0x6));
        assert!(!b.write(0x2c, 4, 0));
    }
}
