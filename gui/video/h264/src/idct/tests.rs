#![allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

const EIGHT: Depth = Depth { bits: 8 };
const TEN: Depth = Depth { bits: 10 };

/// A 4x4 (or 8x8) patch of samples in a 16-wide plane, every sample `v`.
fn plane<P: Pixel>(v: i32) -> Vec<P> {
    vec![P::from_int(v); 16 * 16]
}

#[test]
fn a_dc_only_block_transforms_as_its_shortcut_does() {
    for dc in [-2000, -100, -33, -31, 0, 31, 32, 95, 96, 1000, 4000] {
        for (d, start) in [(EIGHT, 128), (TEN, 512)] {
            let mut full = plane::<u16>(start);
            let mut short = plane::<u16>(start);
            let mut a = [0i32; 16];
            let mut b = [0i32; 16];
            a[0] = dc;
            b[0] = dc;
            idct_add(d, &mut full, 17, 16, &mut a);
            idct_dc_add(d, &mut short, 17, 16, &mut b);
            assert_eq!(full, short, "dc {dc} at {} bits", d.bits);
            assert_eq!(a, [0; 16]);
            assert_eq!(b, [0; 16]);

            let mut full = plane::<u16>(start);
            let mut short = plane::<u16>(start);
            let mut a = [0i32; 64];
            let mut b = [0i32; 64];
            a[0] = dc;
            b[0] = dc;
            idct8_add(d, &mut full, 17, 16, &mut a);
            idct8_dc_add(d, &mut short, 17, 16, &mut b);
            assert_eq!(full, short, "8x8 dc {dc} at {} bits", d.bits);
            assert_eq!(a, [0; 64]);
        }
    }
}

#[test]
fn only_the_block_is_touched_and_samples_clip_to_the_depth() {
    let mut p = plane::<u8>(250);
    let mut block = [0i32; 16];
    block[0] = 64 * 20;
    idct_add(EIGHT, &mut p, 16 + 4, 16, &mut block);
    for (k, &s) in p.iter().enumerate() {
        let (y, x) = (k / 16, k % 16);
        let inside = (1..5).contains(&y) && (4..8).contains(&x);
        assert_eq!(s, if inside { 255 } else { 250 }, "({x}, {y})");
    }
    let mut p = plane::<u16>(1000);
    let mut block = [0i32; 16];
    block[0] = 64 * 50;
    idct_add(TEN, &mut p, 0, 16, &mut block);
    assert_eq!(p[0], 1023);
    assert_eq!(p[4], 1000);
}

/// The 8-bit build keeps coefficients in 16 bits: a DC at the top of the
/// range wraps negative when its rounding is added, where the deeper
/// builds' 32 bits do not.
#[test]
fn eight_bit_coefficients_wrap_at_16_bits() {
    let mut p = plane::<u8>(200);
    let mut block = [0i32; 16];
    block[0] = i32::from(i16::MAX);
    idct_add(EIGHT, &mut p, 0, 16, &mut block);
    // 32767 + 32 is -32737 in 16 bits: every sample pulled to 0.
    assert_eq!(p[0], 0);
    assert_eq!(p[3 * 16 + 3], 0);
    let mut p = plane::<u16>(200);
    let mut block = [0i32; 16];
    block[0] = i32::from(i16::MAX);
    idct_add(TEN, &mut p, 0, 16, &mut block);
    assert_eq!(p[0], 200 + (32799 >> 6));
}

/// A block worked through FFmpeg's arithmetic by hand. Blocks are stored
/// transposed: entry 4 (second row, first column) is the first horizontal
/// frequency. With 96 + 32 rounding at DC and 64 there, the first pass
/// leaves column 0 as 192, 160, 96, 64 (z0 = z1 = 128, z2 = 32, z3 = 64),
/// and the second spreads each down one column of samples: 3, 2, 1, 1.
#[test]
fn a_known_block_transforms_to_its_samples() {
    let mut block = [0i32; 16];
    block[0] = 96;
    block[4] = 64;
    let mut p = plane::<u8>(100);
    idct_add(EIGHT, &mut p, 0, 16, &mut block);
    for y in 0..4 {
        assert_eq!(p[y * 16..y * 16 + 4], [103, 102, 101, 101], "row {y}");
    }
}

#[test]
fn the_luma_dc_transform_spreads_one_coefficient_to_every_block() {
    let mut input = [0i32; 16];
    input[0] = 10;
    let mut output = [0i32; 256];
    luma_dc_dequant_idct(EIGHT, &mut output, &input, 64);
    // (10 * 64 + 128) >> 8 = 3 at each block's DC, nothing elsewhere.
    for b in 0..16 {
        assert_eq!(output[16 * b], 3, "block {b}");
        assert!(output[16 * b + 1..16 * b + 16].iter().all(|&v| v == 0));
    }
}

#[test]
fn chroma_dc_transforms_dequantize_in_place() {
    let mut block = [0i32; 64];
    block[0] = 4;
    chroma_dc_dequant_idct(EIGHT, &mut block, 128);
    // (4 * 128) >> 7 = 4 in each of the four DC places.
    for at in [0, 16, 32, 48] {
        assert_eq!(block[at], 4);
    }
    let mut block = [0i32; 128];
    block[0] = 8;
    chroma422_dc_dequant_idct(EIGHT, &mut block, 64);
    // (8 * 64 + 128) >> 8 = 2 in each of the eight.
    for at in [0, 16, 32, 48, 64, 80, 96, 112] {
        assert_eq!(block[at], 2, "{at}");
    }
}

#[test]
fn the_add_loops_pick_the_transform_by_count() {
    let offsets: Vec<usize> = (0..16).map(|i| (i % 4) * 4 + (i / 4) * 4 * 16).collect();
    let mut nnz = [0u8; 120];
    let mut block = [0i32; 16 * 48];
    // Block 0: one coefficient, its DC. Block 1: none counted but a DC
    // (what Intra16x16 leaves). Block 2: counted, AC only.
    nnz[SCAN8[0]] = 1;
    block[0] = 64;
    block[16] = 64;
    nnz[SCAN8[2]] = 1;
    block[2 * 16 + 1] = 64;
    let mut inter = plane::<u8>(100);
    let mut b = block;
    idct_add16(EIGHT, &mut inter, 0, &offsets, &mut b, 16, &nnz);
    // Inter: block 1's uncounted DC is ignored.
    assert_eq!(inter[0], 101);
    assert_eq!(inter[4], 100);
    assert_eq!(b[16], 64);
    let mut intra = plane::<u8>(100);
    let mut b = block;
    idct_add16intra(EIGHT, &mut intra, 0, &offsets, &mut b, 16, &nnz);
    // Intra: block 1's DC is added.
    assert_eq!(intra[4], 101);
    assert_eq!(b[16], 0);
}
