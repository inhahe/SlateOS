//! The ARM64 and RISC-V branch converters as 7-Zip decodes them in a 7z
//! archive (methods `0A` and `0B`): `z7_BranchConv_ARM64_Dec` and
//! `z7_BranchConv_RISCV_Dec` of `C/Bra.c`, from the LZMA SDK 26.00 (Igor
//! Pavlov, public domain), ported. (The older converters -- x86, PowerPC,
//! IA-64, ARM, ARM Thumb, SPARC -- are liblzma's, through the `xz` crate.)
//!
//! A branch converter makes machine code compress better by turning the
//! relative targets of calls and jumps into absolute ones, which repeat;
//! decoding turns them back. Each function converts as much of `data` as
//! it can and returns how much: 7-Zip's filter wrapper writes the bytes
//! after that unconverted when the stream ends there, and so does the 7z
//! reader (`decode.rs`).

// `pc`, positions and the instruction fields are `Bra.c`'s `UInt32`
// arithmetic, wrapping where it wraps (`wrapping_*`); positions index
// `data` checked.
#![allow(clippy::arithmetic_side_effects)]

/// The little-endian `u32` at `i`.
fn get32(data: &[u8], i: usize) -> u32 {
    match data.get(i..i + 4) {
        Some(&[a, b, c, d]) => u32::from_le_bytes([a, b, c, d]),
        _ => 0,
    }
}

/// The big-endian `u32` at `i`.
fn get32_be(data: &[u8], i: usize) -> u32 {
    get32(data, i).swap_bytes()
}

/// The little-endian `u16` at `i`.
fn get16(data: &[u8], i: usize) -> u32 {
    match data.get(i..i + 2) {
        Some(&[a, b]) => u32::from(u16::from_le_bytes([a, b])),
        _ => 0,
    }
}

fn set32(data: &mut [u8], i: usize, v: u32) {
    if let Some(d) = data.get_mut(i..i + 4) {
        d.copy_from_slice(&v.to_le_bytes());
    }
}

/// A position as `Bra.c`'s 32-bit `pc` arithmetic takes it.
#[allow(clippy::cast_possible_truncation)]
const fn pos32(i: usize) -> u32 {
    i as u32
}

/// `z7_BranchConv_ARM64_Dec`: `BL` and `ADRP` targets back to relative,
/// `pc` being `data`'s address. Converts whole instructions: returns
/// `data.len()` rounded down to 4.
pub(crate) fn arm64_decode(data: &mut [u8], pc: u32) -> usize {
    let size = data.len() & !3;
    let flag = 1u32 << (24 - 4);
    let mask = (1u32 << 24) - (flag << 1);
    let mut i = 0;
    while i < size {
        let mut v = get32(data, i);
        let at = pc.wrapping_add(pos32(i));
        if v.wrapping_sub(0x9400_0000) & 0xFC00_0000 == 0 {
            // BL
            v = v.wrapping_sub(at >> 2);
            v &= 0x03FF_FFFF;
            v |= 0x9400_0000;
            set32(data, i, v);
        } else {
            v = v.wrapping_sub(0x9000_0000);
            if v & 0x9F00_0000 == 0 {
                // ADRP, if its page offset is within +-4 GiB
                v = v.wrapping_add(flag);
                if v & mask == 0 {
                    let mut z = (v & 0xFFFF_FFE0) | (v >> 26);
                    let c = (at >> (12 - 3)) & !7u32;
                    z = z.wrapping_sub(c);
                    v &= 0x1F;
                    v |= 0x9000_0000;
                    v |= z << 26;
                    v |= 0x00FF_FFE0 & (z & ((flag << 1) - 1)).wrapping_sub(flag);
                    set32(data, i, v);
                }
            }
        }
        i += 4;
    }
    size
}

/// `RISCV_CHECK_1`
const fn riscv_check_1(v: u32, b: u32) -> bool {
    ((b.wrapping_sub(3) ^ (v << 8)) & (0xF8000 + 3)) == 0
}

/// `RISCV_CHECK_2`
const fn riscv_check_2(v: u32, r: u32) -> bool {
    (v.wrapping_sub((3 << 12) | (2 << 7) | 8) << 18) < (r & 0x1D)
}

/// `z7_BranchConv_RISCV_Dec`: `JAL` and `AUIPC` pairs back to relative,
/// `pc` being `data`'s address. Returns how much was converted: up to the
/// last 6 bytes, which it needs to look at a pair.
pub(crate) fn riscv_decode(data: &mut [u8], pc: u32) -> usize {
    let size = data.len() & !1;
    if size <= 6 {
        return 0;
    }
    let lim = size - 6;
    let mut p = 0usize;
    loop {
        // RISCV_SCAN_LOOP: on to the next JAL or AUIPC.
        let mut a;
        loop {
            if p >= lim {
                return p;
            }
            a = (get16(data, p) ^ 0x10) + 1;
            if a & 0x77 == 0 {
                break;
            }
            a = (get16(data, p + 2) ^ 0x10) + 1;
            p += 4;
            if a & 0x77 == 0 {
                p -= 2;
                if p >= lim {
                    return p;
                }
                break;
            }
        }
        let at = pc.wrapping_add(pos32(p));

        if a & 8 == 0 {
            // JAL
            a = a.wrapping_sub(0x100 - 0x7F);
            if a & 0xD80 != 0 {
                p += 2;
                continue;
            }
            let a_old = (a + (0xEF - 0x7F)) & 0xFFF;
            let b2 = u32::from(data.get(p + 2).copied().unwrap_or(0));
            let b3 = u32::from(data.get(p + 3).copied().unwrap_or(0));
            let mut v = (b3 << 1) | (b2 << 9) | ((a & 0xF000) << 5);
            v = v.wrapping_sub(at);
            a = a_old
                | ((v << 11) & (1u32 << 31))
                | ((v << 20) & (0x3FF << 21))
                | ((v << 9) & (1 << 20))
                | (v & (0xFF << 12));
            set32(data, p, a);
            p += 4;
            continue;
        }

        // AUIPC
        let v = a;
        let a = get32(data, p);
        if v & 0xE80 == 0 {
            // with x0 or x2
            let r = a >> 27;
            if riscv_check_2(v, r) {
                let b = get32_be(data, p + 4).wrapping_sub(at);
                let mut lo = a >> 12;
                let hi = ((r << 7) + 0x17).wrapping_add(b.wrapping_add(0x800) & 0xFFFF_F000);
                lo |= b << 20;
                set32(data, p, hi);
                set32(data, p + 4, lo);
                p += 8;
            } else {
                p += 4;
            }
        } else {
            let b = get32(data, p + 4);
            if riscv_check_1(v, b) {
                let lo = (a & 0xFFFF_F000) | (b >> 20);
                let hi = (b << 12) | (0x17 + (2 << 7));
                set32(data, p, hi);
                set32(data, p + 4, lo);
                p += 8;
            } else {
                p += 4 + 2;
            }
        }
    }
}
