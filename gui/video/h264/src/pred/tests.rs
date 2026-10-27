#![allow(clippy::indexing_slicing, clippy::unwrap_used)]

use super::*;

/// A cache whose macroblock's top row and left column of 4x4 modes are
/// `top` and `left` (the corner shared), everything else DC.
fn cache(top: [i8; 4], left: [i8; 4]) -> [i8; 40] {
    let mut c = [DC_PRED; 40];
    for i in 0..4 {
        c[SCAN8[0] + i] = top[i];
        c[SCAN8[0] + 8 * i] = left[i];
    }
    c
}

fn row(c: &[i8; 40]) -> [i8; 4] {
    [0, 1, 2, 3].map(|i| c[SCAN8[0] + i])
}

fn column(c: &[i8; 40]) -> [i8; 4] {
    [0, 1, 2, 3].map(|i| c[SCAN8[0] + 8 * i])
}

const ALL: u32 = 0xFFFF;

#[test]
fn a_4x4_mode_with_its_neighbours_is_left_alone() {
    let mut c = cache(
        [VERT_PRED, DIAG_DOWN_LEFT_PRED, VERT_LEFT_PRED, DC_PRED],
        [VERT_PRED, HOR_PRED, HOR_UP_PRED, DC_PRED],
    );
    let before = c;
    assert_eq!(check_intra4x4_pred_mode(&mut c, ALL, ALL), Ok(()));
    assert_eq!(c, before);
}

#[test]
fn without_the_top_row_dc_is_left_dc_and_vertical_is_refused() {
    let mut c = cache(
        [HOR_PRED, DC_PRED, HOR_UP_PRED, DC_PRED],
        [HOR_PRED, HOR_PRED, HOR_PRED, HOR_PRED],
    );
    assert_eq!(check_intra4x4_pred_mode(&mut c, 0x7FFF, ALL), Ok(()));
    assert_eq!(row(&c), [HOR_PRED, LEFT_DC_PRED, HOR_UP_PRED, LEFT_DC_PRED]);
    for refused in [
        VERT_PRED,
        DIAG_DOWN_LEFT_PRED,
        DIAG_DOWN_RIGHT_PRED,
        VERT_RIGHT_PRED,
        HOR_DOWN_PRED,
        VERT_LEFT_PRED,
    ] {
        let mut c = cache([HOR_PRED, HOR_PRED, refused, HOR_PRED], [HOR_PRED; 4]);
        assert_eq!(
            check_intra4x4_pred_mode(&mut c, 0x7FFF, ALL),
            Err(DecodeError),
            "{refused}"
        );
    }
}

#[test]
fn without_the_left_column_dc_is_top_dc_and_horizontal_is_refused() {
    let mut c = cache(
        [VERT_PRED; 4],
        [VERT_PRED, DC_PRED, DIAG_DOWN_LEFT_PRED, VERT_LEFT_PRED],
    );
    assert_eq!(check_intra4x4_pred_mode(&mut c, ALL, 0x5F5F), Ok(()));
    assert_eq!(
        column(&c),
        [VERT_PRED, TOP_DC_PRED, DIAG_DOWN_LEFT_PRED, VERT_LEFT_PRED]
    );
    for refused in [
        HOR_PRED,
        DIAG_DOWN_RIGHT_PRED,
        VERT_RIGHT_PRED,
        HOR_DOWN_PRED,
        HOR_UP_PRED,
    ] {
        let mut c = cache([VERT_PRED; 4], [VERT_PRED, VERT_PRED, VERT_PRED, refused]);
        assert_eq!(
            check_intra4x4_pred_mode(&mut c, ALL, 0x5F5F),
            Err(DecodeError),
            "{refused}"
        );
    }
}

#[test]
fn without_either_the_corner_dc_is_128() {
    let mut c = cache([DC_PRED; 4], [DC_PRED; 4]);
    assert_eq!(check_intra4x4_pred_mode(&mut c, 0, 0), Ok(()));
    assert_eq!(
        row(&c),
        [DC_128_PRED, LEFT_DC_PRED, LEFT_DC_PRED, LEFT_DC_PRED]
    );
    assert_eq!(
        column(&c),
        [DC_128_PRED, TOP_DC_PRED, TOP_DC_PRED, TOP_DC_PRED]
    );
}

/// In an MBAFF frame half the left column can be there: only the rows
/// without it change.
#[test]
fn half_a_left_column_changes_only_its_rows() {
    let mut c = cache([VERT_PRED; 4], [DC_PRED; 4]);
    // Rows 0 and 2 (masks 0x8000, 0x80) are there; 1 and 3 are not.
    assert_eq!(check_intra4x4_pred_mode(&mut c, ALL, 0x8080), Ok(()));
    assert_eq!(column(&c), [DC_PRED, TOP_DC_PRED, DC_PRED, TOP_DC_PRED]);
    let mut c = cache([VERT_PRED; 4], [DC_PRED; 4]);
    assert_eq!(check_intra4x4_pred_mode(&mut c, ALL, 0x8800), Ok(()));
    assert_eq!(column(&c), [DC_PRED, TOP_DC_PRED, TOP_DC_PRED, TOP_DC_PRED]);
}

#[test]
fn a_16x16_or_chroma_mode_is_made_one_the_neighbours_allow() {
    let check = |top, left, mode, chroma| check_intra_pred_mode(top, left, mode, chroma);
    for mode in 0..4 {
        assert_eq!(
            check(ALL, ALL, mode, false),
            Ok(i32::try_from(mode).unwrap())
        );
    }
    assert_eq!(check(ALL, ALL, 4, false), Err(DecodeError));
    // No top row: DC is left DC, horizontal stays, vertical and plane go.
    assert_eq!(check(0, ALL, 0, false), Ok(LEFT_DC_PRED8X8));
    assert_eq!(check(0, ALL, 1, false), Ok(HOR_PRED8X8));
    assert_eq!(check(0, ALL, 2, false), Err(DecodeError));
    assert_eq!(check(0, ALL, 3, false), Err(DecodeError));
    // No left column: DC is top DC, vertical stays, horizontal and plane go.
    assert_eq!(check(ALL, 0, 0, false), Ok(TOP_DC_PRED8X8));
    assert_eq!(check(ALL, 0, 2, false), Ok(VERT_PRED8X8));
    assert_eq!(check(ALL, 0, 1, false), Err(DecodeError));
    assert_eq!(check(ALL, 0, 3, false), Err(DecodeError));
    // Neither: DC is 128.
    assert_eq!(check(0, 0, 0, false), Ok(DC_128_PRED8X8));
}

#[test]
fn chroma_beside_half_a_left_column_takes_an_alzheimer_dc() {
    let check = |top, left| check_intra_pred_mode(top, left, 0, true);
    assert_eq!(check(ALL, 0x8000), Ok(ALZHEIMER_DC_L0T_PRED8X8));
    assert_eq!(check(ALL, 0x0080), Ok(ALZHEIMER_DC_0LT_PRED8X8));
    assert_eq!(check(0, 0x8000), Ok(ALZHEIMER_DC_L00_PRED8X8));
    assert_eq!(check(0, 0x0080), Ok(ALZHEIMER_DC_0L0_PRED8X8));
    // Luma beside the same keeps its plain DC.
    assert_eq!(
        check_intra_pred_mode(ALL, 0x8000, 0, false),
        Ok(TOP_DC_PRED8X8)
    );
    // Chroma with no left column at all is plain too.
    assert_eq!(check(ALL, 0), Ok(TOP_DC_PRED8X8));
}

/// A 32x32 plane of `v`, and where a block 8 samples in from its top-left
/// starts: room for every neighbour above, left and above-right.
fn field<P: Pixel>(v: i32) -> (Vec<P>, usize) {
    (vec![P::from_int(v); 32 * 32], 8 * 32 + 8)
}

const STRIDE: usize = 32;

/// The w by h block from `at`.
fn block_of<P: Pixel>(s: &[P], at: usize, w: usize, h: usize) -> Vec<i32> {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| s[at + x + y * STRIDE].int()))
        .collect()
}

#[test]
fn every_mode_predicts_a_flat_neighbourhood_flat() {
    for bits in [8, 10, 14] {
        let c = (1 << bits) / 3;
        for mode in 0..=DC_128_PRED {
            let (mut s, at) = field::<u16>(c);
            pred4x4(mode, &mut s, at, STRIDE, [c; 4], bits);
            let want = if mode == DC_128_PRED {
                1 << (bits - 1)
            } else {
                c
            };
            assert!(
                block_of(&s, at, 4, 4).iter().all(|&v| v == want),
                "4x4 mode {mode} at {bits}"
            );
            for (tl, tr) in [(false, false), (true, true), (true, false)] {
                let (mut s, at) = field::<u16>(c);
                pred8x8l(mode, &mut s, at, STRIDE, tl, tr, bits);
                assert!(
                    block_of(&s, at, 8, 8).iter().all(|&v| v == want),
                    "8x8 mode {mode}"
                );
            }
        }
        for mode in 0..=DC_128_PRED8X8 {
            let want = if mode == DC_128_PRED8X8 {
                1 << (bits - 1)
            } else {
                c
            };
            let (mut s, at) = field::<u16>(c);
            pred16x16(mode, &mut s, at, STRIDE, bits);
            assert!(
                block_of(&s, at, 16, 16).iter().all(|&v| v == want),
                "16x16 mode {mode}"
            );
            for tall in [false, true] {
                let (mut s, at) = field::<u16>(c);
                pred_chroma(mode, &mut s, at, STRIDE, tall, bits);
                let rows = if tall { 16 } else { 8 };
                assert!(
                    block_of(&s, at, 8, rows).iter().all(|&v| v == want),
                    "chroma mode {mode}"
                );
            }
        }
    }
}

/// A plane whose samples are `f(x, y)` around a block at (8, 8).
fn ramp(f: impl Fn(i32, i32) -> i32) -> (Vec<u8>, usize) {
    let s = (0..32 * 32)
        .map(|i| u8::try_from(f(i % 32 - 8, i / 32 - 8)).unwrap())
        .collect();
    (s, 8 * 32 + 8)
}

#[test]
fn vertical_and_horizontal_copy_their_edge() {
    let (mut s, at) = ramp(|x, y| 100 + x + 3 * y);
    pred4x4(VERT_PRED, &mut s, at, STRIDE, [0; 4], 8);
    for y in 0..4 {
        assert_eq!(block_of(&s, at + y * STRIDE, 4, 1), [97, 98, 99, 100]);
    }
    let (mut s, at) = ramp(|x, y| 100 + x + 3 * y);
    pred16x16(HOR_PRED8X8, &mut s, at, STRIDE, 8);
    for y in 0..16 {
        assert!(
            block_of(&s, at + y * STRIDE, 16, 1)
                .iter()
                .all(|&v| v == 99 + 3 * y as i32)
        );
    }
}

/// A linear ramp's edges fit a plane that is the ramp: plane prediction
/// gives it back exactly, 16x16 and chroma alike.
#[test]
fn plane_prediction_reproduces_a_ramp() {
    let f = |x: i32, y: i32| 60 + 2 * x + 3 * y;
    let (mut s, at) = ramp(f);
    pred16x16(PLANE_PRED8X8, &mut s, at, STRIDE, 8);
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(
                s[at + x + y * STRIDE].int(),
                f(x as i32, y as i32),
                "({x}, {y})"
            );
        }
    }
    let (mut s, at) = ramp(f);
    pred_chroma(PLANE_PRED8X8, &mut s, at, STRIDE, false, 8);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                s[at + x + y * STRIDE].int(),
                f(x as i32, y as i32),
                "chroma ({x}, {y})"
            );
        }
    }
}

/// The diagonals worked by hand on a top row 0, 4, 8 ... 28 (top-right
/// included): down-left's corner is (t0 + 2 t1 + t2 + 2) >> 2 = 4, its
/// far corner (t6 + 3 t7 + 2) >> 2 = 27.
#[test]
fn down_left_runs_along_the_top_row() {
    let (mut s, at) = ramp(|x, _| 4 * x.max(0));
    pred4x4(DIAG_DOWN_LEFT_PRED, &mut s, at, STRIDE, [16, 20, 24, 28], 8);
    let b = block_of(&s, at, 4, 4);
    assert_eq!(b[0], 4);
    assert_eq!(b[15], 27);
    // Each anti-diagonal one value.
    assert_eq!(b[1], b[4]);
    assert_eq!(b[3], b[6]);
    assert_eq!(b[6], b[9]);
    assert_eq!(b[9], b[12]);
}

#[test]
fn lossless_vertical_prediction_sums_the_residual_down_each_column() {
    let (mut s, at) = field::<u8>(250);
    let mut block = [0i32; 16];
    for (k, v) in block.iter_mut().enumerate() {
        *v = 1 + i32::try_from(k).unwrap() % 4;
    }
    pred4x4_add(true, &mut s, at, STRIDE, &mut block);
    // Column x: 250 + (x + 1) per row, wrapping past 255 as uint8 does.
    for y in 0..4u8 {
        for x in 0..4u8 {
            let want = 250u8.wrapping_add((x + 1) * (y + 1));
            assert_eq!(
                s[at + usize::from(x) + usize::from(y) * STRIDE],
                want,
                "({x}, {y})"
            );
        }
    }
    assert_eq!(block, [0; 16]);
}
