//! The filters a `.xz` block may chain before LZMA2: liblzma's `delta/` and
//! `simple/` (the branch converters, "BCJ"), and the chain rules of
//! `filter_common.c` and the property decoding of `filter_decoder.c`.
//!
//! A branch converter rewrites the relative addresses in one architecture's
//! call and jump instructions as absolute ones, which repeat far more often
//! and so compress better; decoding converts them back. Each is a pure
//! function of the bytes and their position, so it is run over a whole
//! decoded block at once -- liblzma runs it over whatever its buffers hold,
//! keeps back the few bytes that might begin an instruction, and gets the
//! same answer, which is what makes the result independent of buffer sizes.
//! The last few bytes of a stream, too short to hold an instruction, are left
//! as they are by both.

use crate::lzma2;
use crate::{Error, Result};

/// The filter IDs liblzma 5.2.5 knows.
pub(crate) const ID_LZMA2: u64 = 0x21;
pub(crate) const ID_DELTA: u64 = 0x03;
pub(crate) const ID_X86: u64 = 0x04;
pub(crate) const ID_POWERPC: u64 = 0x05;
pub(crate) const ID_IA64: u64 = 0x06;
pub(crate) const ID_ARM: u64 = 0x07;
pub(crate) const ID_ARMTHUMB: u64 = 0x08;
pub(crate) const ID_SPARC: u64 = 0x09;

/// `LZMA_FILTER_RESERVED_START`: IDs from here on are liblzma-private and
/// may not appear in a file.
pub(crate) const ID_RESERVED_START: u64 = 1 << 62;

/// A branch converter's architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bcj {
    /// x86 and x86-64 `call`/`jmp` (E8/E9).
    X86,
    /// Big-endian PowerPC `bl`.
    PowerPc,
    /// Itanium.
    Ia64,
    /// Little-endian ARM `bl`.
    Arm,
    /// ARM Thumb `bl`.
    ArmThumb,
    /// SPARC `call`.
    Sparc,
}

impl Bcj {
    /// The alignment a start offset must keep (`simple_coder.c`'s
    /// `alignment`).
    pub(crate) const fn alignment(self) -> u32 {
        match self {
            Self::X86 => 1,
            Self::ArmThumb => 2,
            Self::PowerPc | Self::Arm | Self::Sparc => 4,
            Self::Ia64 => 16,
        }
    }
}

/// One filter of a block's chain, with its properties decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    /// LZMA2 with this dictionary size: always the last.
    Lzma2 { dict_size: u32 },
    /// Delta with this distance, 1 to 256.
    Delta { distance: u32 },
    /// A branch converter, numbering positions from `start`.
    Bcj { arch: Bcj, start: u32 },
}

impl Filter {
    /// `lzma_properties_decode`: the filter with ID `id` and properties
    /// `props`. An ID liblzma does not know, or properties it does not
    /// accept, is [`Error::Unsupported`] (`LZMA_OPTIONS_ERROR`).
    pub(crate) fn decode(id: u64, props: &[u8]) -> Result<Self> {
        let arch = match id {
            ID_LZMA2 => {
                let &[prop] = props else {
                    return Err(Error::Unsupported);
                };
                let dict_size = lzma2::dict_size_from_prop(prop).ok_or(Error::Unsupported)?;
                return Ok(Self::Lzma2 { dict_size });
            }
            ID_DELTA => {
                let &[d] = props else {
                    return Err(Error::Unsupported);
                };
                return Ok(Self::Delta {
                    distance: u32::from(d) + 1,
                });
            }
            ID_X86 => Bcj::X86,
            ID_POWERPC => Bcj::PowerPc,
            ID_IA64 => Bcj::Ia64,
            ID_ARM => Bcj::Arm,
            ID_ARMTHUMB => Bcj::ArmThumb,
            ID_SPARC => Bcj::Sparc,
            _ => return Err(Error::Unsupported),
        };
        // `lzma_simple_props_decode`: no properties, or a 4-byte start
        // offset, which `lzma_simple_coder_init` requires to keep the
        // architecture's alignment.
        let start = match props {
            [] => 0,
            &[a, b, c, d] => u32::from_le_bytes([a, b, c, d]),
            _ => return Err(Error::Unsupported),
        };
        if start & arch.alignment().wrapping_sub(1) != 0 {
            return Err(Error::Unsupported);
        }
        Ok(Self::Bcj { arch, start })
    }

    /// Whether the filter may stand last in a chain (`last_ok`): only LZMA2
    /// knows where its data ends.
    const fn last_ok(self) -> bool {
        matches!(self, Self::Lzma2 { .. })
    }
}

/// `validate_chain`: one to four filters, every one but the last a filter
/// that may come before another, and the last one that may end a chain.
pub(crate) fn validate_chain(chain: &[Filter]) -> Result<()> {
    let Some((last, rest)) = chain.split_last() else {
        return Err(Error::Unsupported);
    };
    if chain.len() > 4 || !last.last_ok() || rest.iter().any(|f| f.last_ok()) {
        return Err(Error::Unsupported);
    }
    Ok(())
}

/// Undoes the non-last filters of `chain` over a block's `data`, last first.
pub(crate) fn undo(chain: &[Filter], data: &mut [u8]) {
    for filter in chain.iter().rev() {
        match *filter {
            Filter::Lzma2 { .. } => {}
            Filter::Delta { distance } => delta_decode(data, distance),
            Filter::Bcj { arch, start } => {
                bcj(arch, start, false, data);
            }
        }
    }
}

/// `delta_decoder.c`'s `decode_buffer`, over a block from its start: each
/// byte has the byte `distance` before it added.
fn delta_decode(data: &mut [u8], distance: u32) {
    let mut history = [0u8; 256];
    let mut pos = 0u8;
    let distance = usize::try_from(distance).unwrap_or(256);
    for b in data {
        let from = usize::from(pos).wrapping_add(distance) & 0xff;
        *b = b.wrapping_add(history.get(from).copied().unwrap_or(0));
        if let Some(slot) = history.get_mut(usize::from(pos)) {
            *slot = *b;
        }
        pos = pos.wrapping_sub(1);
    }
}

/// `delta_encoder.c`'s `copy_and_encode`/`encode_in_place`: each byte has
/// the byte `distance` before it subtracted.
pub(crate) fn delta_encode(data: &mut [u8], distance: u32) {
    let mut history = [0u8; 256];
    let mut pos = 0u8;
    let distance = usize::try_from(distance).unwrap_or(256);
    for b in data {
        let from = usize::from(pos).wrapping_add(distance) & 0xff;
        let tmp = history.get(from).copied().unwrap_or(0);
        if let Some(slot) = history.get_mut(usize::from(pos)) {
            *slot = *b;
        }
        pos = pos.wrapping_sub(1);
        *b = b.wrapping_sub(tmp);
    }
}

/// Runs the branch converter for `arch` over `data`, whose first byte is at
/// position `start`; returns how many bytes it converted (the rest being too
/// few to hold an instruction).
pub(crate) fn bcj(arch: Bcj, start: u32, encode: bool, data: &mut [u8]) -> usize {
    match arch {
        Bcj::X86 => x86(start, encode, data),
        Bcj::PowerPc => powerpc(start, encode, data),
        Bcj::Ia64 => ia64(start, encode, data),
        Bcj::Arm => arm(start, encode, data),
        Bcj::ArmThumb => armthumb(start, encode, data),
        Bcj::Sparc => sparc(start, encode, data),
    }
}

/// A position as the 32 bits the converters compute in: liblzma's
/// `now_pos + (uint32_t)(i)`, which wraps.
fn at(start: u32, i: usize) -> u32 {
    start.wrapping_add(i as u32)
}

/// Reads `N` bytes of `data` from `i`, which the callers' loop bounds keep
/// in range.
fn bytes<const N: usize>(data: &[u8], i: usize) -> [u8; N] {
    let mut out = [0u8; N];
    if let Some(src) = data.get(i..i.wrapping_add(N)) {
        out.copy_from_slice(src);
    }
    out
}

/// Writes `N` bytes into `data` at `i`.
fn put<const N: usize>(data: &mut [u8], i: usize, b: [u8; N]) {
    if let Some(dst) = data.get_mut(i..i.wrapping_add(N)) {
        dst.copy_from_slice(&b);
    }
}

/// `x86.c`: `call` and `jmp` (E8, E9) with a 32-bit displacement whose top
/// byte is 00 or FF, skipping some that follow other candidates closely.
// Positions are bounded by the slice's length, so their arithmetic cannot
// overflow; the address arithmetic wraps explicitly, as the C's `uint32_t`.
#[allow(clippy::arithmetic_side_effects)]
fn x86(start: u32, encode: bool, data: &mut [u8]) -> usize {
    const MASK_TO_ALLOWED_STATUS: [bool; 8] = [true, true, true, false, true, false, false, false];
    const MASK_TO_BIT_NUMBER: [u32; 8] = [0, 1, 2, 2, 3, 3, 3, 3];
    let test_ms_byte = |b: u8| b == 0 || b == 0xff;

    // `x86_coder_init`: `prev_mask = 0`, `prev_pos = (uint32_t)(-5)`.
    let mut prev_mask: u32 = 0;
    let mut prev_pos: u32 = 0u32.wrapping_sub(5);
    let now_pos = start;
    let size = data.len();
    if size < 5 {
        return 0;
    }
    if now_pos.wrapping_sub(prev_pos) > 5 {
        prev_pos = now_pos.wrapping_sub(5);
    }
    let limit = size - 5;
    let mut pos = 0usize;
    while pos <= limit {
        let b = data.get(pos).copied().unwrap_or(0);
        if b != 0xe8 && b != 0xe9 {
            pos += 1;
            continue;
        }
        let offset = at(now_pos, pos).wrapping_sub(prev_pos);
        prev_pos = at(now_pos, pos);
        if offset > 5 {
            prev_mask = 0;
        } else {
            for _ in 0..offset {
                prev_mask &= 0x77;
                prev_mask <<= 1;
            }
        }
        let b4 = data.get(pos + 4).copied().unwrap_or(0);
        let allowed = MASK_TO_ALLOWED_STATUS
            .get(((prev_mask >> 1) & 7) as usize)
            .copied()
            .unwrap_or(false);
        if test_ms_byte(b4) && allowed && (prev_mask >> 1) < 0x10 {
            let [_, b1, b2, b3, _] = bytes::<5>(data, pos);
            let mut src = u32::from_le_bytes([b1, b2, b3, b4]);
            let mut dest;
            loop {
                dest = if encode {
                    src.wrapping_add(at(now_pos, pos).wrapping_add(5))
                } else {
                    src.wrapping_sub(at(now_pos, pos).wrapping_add(5))
                };
                if prev_mask == 0 {
                    break;
                }
                let i = MASK_TO_BIT_NUMBER
                    .get((prev_mask >> 1) as usize)
                    .copied()
                    .unwrap_or(0);
                let b = (dest >> (24 - i * 8)) as u8;
                if !test_ms_byte(b) {
                    break;
                }
                src = dest ^ (1u32.wrapping_shl(32 - i * 8).wrapping_sub(1));
            }
            let top = !(((dest >> 24) & 1).wrapping_sub(1)) as u8;
            let [d0, d1, d2, _] = dest.to_le_bytes();
            put(data, pos + 1, [d0, d1, d2, top]);
            pos += 5;
            prev_mask = 0;
        } else {
            pos += 1;
            prev_mask |= 1;
            if test_ms_byte(b4) {
                prev_mask |= 0x10;
            }
        }
    }
    pos
}

/// `powerpc.c`: `bl` -- opcode 18 with the absolute bit clear and the link
/// bit set.
// Positions are bounded by the slice's length, so their arithmetic cannot
// overflow; the address arithmetic wraps explicitly, as the C's `uint32_t`.
#[allow(clippy::arithmetic_side_effects)]
fn powerpc(start: u32, encode: bool, data: &mut [u8]) -> usize {
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let [b0, b1, b2, b3] = bytes::<4>(data, i);
        if (b0 >> 2) == 0x12 && (b3 & 3) == 1 {
            let src = (u32::from(b0 & 3) << 24)
                | (u32::from(b1) << 16)
                | (u32::from(b2) << 8)
                | u32::from(b3 & !3);
            let dest = if encode {
                at(start, i).wrapping_add(src)
            } else {
                src.wrapping_sub(at(start, i))
            };
            put(
                data,
                i,
                [
                    0x48 | ((dest >> 24) & 0x03) as u8,
                    (dest >> 16) as u8,
                    (dest >> 8) as u8,
                    (b3 & 0x03) | dest as u8,
                ],
            );
        }
        i += 4;
    }
    i
}

/// `ia64.c`: the branch slots of an Itanium bundle, as the template says.
// Positions are bounded by the slice's length, so their arithmetic cannot
// overflow; the address arithmetic wraps explicitly, as the C's `uint32_t`.
#[allow(clippy::arithmetic_side_effects)]
fn ia64(start: u32, encode: bool, data: &mut [u8]) -> usize {
    const BRANCH_TABLE: [u32; 32] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 6, 6, 0, 0, 7, 7, 4, 4, 0, 0, 4, 4,
        0, 0,
    ];
    let mut i = 0usize;
    while i + 16 <= data.len() {
        let template = data.get(i).copied().unwrap_or(0) & 0x1f;
        let mask = BRANCH_TABLE
            .get(usize::from(template))
            .copied()
            .unwrap_or(0);
        let mut bit_pos = 5u32;
        for slot in 0..3 {
            if (mask >> slot) & 1 == 0 {
                bit_pos += 41;
                continue;
            }
            let byte_pos = (bit_pos >> 3) as usize;
            let bit_res = bit_pos & 7;
            let raw = bytes::<6>(data, i + byte_pos);
            let mut instruction = 0u64;
            for (j, &b) in raw.iter().enumerate() {
                instruction += u64::from(b) << (8 * j);
            }
            let mut inst_norm = instruction >> bit_res;
            if ((inst_norm >> 37) & 0xf) == 0x5 && ((inst_norm >> 9) & 0x7) == 0 {
                let mut src = ((inst_norm >> 13) & 0xf_ffff) as u32;
                src |= (((inst_norm >> 36) & 1) as u32) << 20;
                src <<= 4;
                let mut dest = if encode {
                    at(start, i).wrapping_add(src)
                } else {
                    src.wrapping_sub(at(start, i))
                };
                dest >>= 4;
                inst_norm &= !(0x8f_ffffu64 << 13);
                inst_norm |= u64::from(dest & 0xf_ffff) << 13;
                inst_norm |= u64::from(dest & 0x10_0000) << (36 - 20);
                instruction &= (1u64 << bit_res) - 1;
                instruction |= inst_norm << bit_res;
                let mut out = [0u8; 6];
                for (j, b) in out.iter_mut().enumerate() {
                    *b = (instruction >> (8 * j)) as u8;
                }
                put(data, i + byte_pos, out);
            }
            bit_pos += 41;
        }
        i += 16;
    }
    i
}

/// `arm.c`: `bl` (top byte EB), little-endian.
// Positions are bounded by the slice's length, so their arithmetic cannot
// overflow; the address arithmetic wraps explicitly, as the C's `uint32_t`.
#[allow(clippy::arithmetic_side_effects)]
fn arm(start: u32, encode: bool, data: &mut [u8]) -> usize {
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let [b0, b1, b2, b3] = bytes::<4>(data, i);
        if b3 == 0xeb {
            let src = ((u32::from(b2) << 16) | (u32::from(b1) << 8) | u32::from(b0)) << 2;
            let dest = if encode {
                at(start, i).wrapping_add(8).wrapping_add(src)
            } else {
                src.wrapping_sub(at(start, i).wrapping_add(8))
            } >> 2;
            put(data, i, [dest as u8, (dest >> 8) as u8, (dest >> 16) as u8]);
        }
        i += 4;
    }
    i
}

/// `armthumb.c`: the two halves of a Thumb `bl`.
// Positions are bounded by the slice's length, so their arithmetic cannot
// overflow; the address arithmetic wraps explicitly, as the C's `uint32_t`.
#[allow(clippy::arithmetic_side_effects)]
fn armthumb(start: u32, encode: bool, data: &mut [u8]) -> usize {
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let [b0, b1, b2, b3] = bytes::<4>(data, i);
        if (b1 & 0xf8) == 0xf0 && (b3 & 0xf8) == 0xf8 {
            let src = ((u32::from(b1 & 7) << 19)
                | (u32::from(b0) << 11)
                | (u32::from(b3 & 7) << 8)
                | u32::from(b2))
                << 1;
            let dest = if encode {
                at(start, i).wrapping_add(4).wrapping_add(src)
            } else {
                src.wrapping_sub(at(start, i).wrapping_add(4))
            } >> 1;
            put(
                data,
                i,
                [
                    (dest >> 11) as u8,
                    0xf0 | ((dest >> 19) & 0x7) as u8,
                    dest as u8,
                    0xf8 | ((dest >> 8) & 0x7) as u8,
                ],
            );
            i += 2;
        }
        i += 2;
    }
    i
}

/// `sparc.c`: `call`, sign-extended to 22 bits.
// Positions are bounded by the slice's length, so their arithmetic cannot
// overflow; the address arithmetic wraps explicitly, as the C's `uint32_t`.
#[allow(clippy::arithmetic_side_effects)]
fn sparc(start: u32, encode: bool, data: &mut [u8]) -> usize {
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let [b0, b1, b2, b3] = bytes::<4>(data, i);
        if (b0 == 0x40 && (b1 & 0xc0) == 0x00) || (b0 == 0x7f && (b1 & 0xc0) == 0xc0) {
            let src = u32::from_be_bytes([b0, b1, b2, b3]) << 2;
            let mut dest = if encode {
                at(start, i).wrapping_add(src)
            } else {
                src.wrapping_sub(at(start, i))
            } >> 2;
            dest = ((0u32.wrapping_sub((dest >> 22) & 1) << 22) & 0x3fff_ffff)
                | (dest & 0x3f_ffff)
                | 0x4000_0000;
            put(data, i, dest.to_be_bytes());
        }
        i += 4;
    }
    i
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn delta_round_trips() {
        let original: Vec<u8> = (0..1000u32).map(|i| (i * 7 + i / 13) as u8).collect();
        for distance in [1u32, 2, 4, 255, 256] {
            let mut data = original.clone();
            delta_encode(&mut data, distance);
            assert_ne!(data, original);
            delta_decode(&mut data, distance);
            assert_eq!(data, original, "distance {distance}");
        }
    }

    #[test]
    fn delta_subtracts_the_byte_distance_back() {
        let mut data = vec![10, 20, 30, 45];
        delta_encode(&mut data, 2);
        assert_eq!(data, [10, 20, 20, 25]);
    }

    #[test]
    fn every_branch_converter_round_trips() {
        let mut seed = 1u64;
        let original: Vec<u8> = (0..4096)
            .map(|_| {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                (seed >> 56) as u8
            })
            .collect();
        for arch in [
            Bcj::X86,
            Bcj::PowerPc,
            Bcj::Ia64,
            Bcj::Arm,
            Bcj::ArmThumb,
            Bcj::Sparc,
        ] {
            for start in [0u32, 16, 0x1000] {
                let mut data = original.clone();
                bcj(arch, start, true, &mut data);
                bcj(arch, start, false, &mut data);
                assert_eq!(data, original, "{arch:?} from {start}");
            }
        }
    }

    #[test]
    fn x86_makes_a_call_absolute() {
        // A call at position 0x10 to displacement 0x100: absolute 0x115.
        let mut data = vec![0x90; 0x20];
        data[0x10..0x15].copy_from_slice(&[0xe8, 0x00, 0x01, 0x00, 0x00]);
        bcj(Bcj::X86, 0, true, &mut data);
        assert_eq!(&data[0x10..0x15], &[0xe8, 0x15, 0x01, 0x00, 0x00]);
    }

    #[test]
    fn a_start_offset_must_keep_the_alignment() {
        assert_eq!(
            Filter::decode(ID_ARM, &4u32.to_le_bytes()),
            Ok(Filter::Bcj {
                arch: Bcj::Arm,
                start: 4
            })
        );
        assert_eq!(
            Filter::decode(ID_ARM, &2u32.to_le_bytes()),
            Err(Error::Unsupported)
        );
        assert_eq!(Filter::decode(ID_X86, &[1, 2]), Err(Error::Unsupported));
        assert_eq!(
            Filter::decode(ID_DELTA, &[255]),
            Ok(Filter::Delta { distance: 256 })
        );
        assert_eq!(Filter::decode(0x0a, &[]), Err(Error::Unsupported));
    }

    #[test]
    fn chains_end_in_lzma2_and_hold_at_most_four() {
        let l = Filter::Lzma2 { dict_size: 1 << 20 };
        let x = Filter::Bcj {
            arch: Bcj::X86,
            start: 0,
        };
        let d = Filter::Delta { distance: 1 };
        assert!(validate_chain(&[l]).is_ok());
        assert!(validate_chain(&[x, d, x, l]).is_ok());
        assert!(validate_chain(&[x, d, x, d, l]).is_err());
        assert!(validate_chain(&[x]).is_err());
        assert!(validate_chain(&[l, l]).is_err());
        assert!(validate_chain(&[]).is_err());
    }
}
