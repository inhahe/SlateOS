#![allow(clippy::indexing_slicing, clippy::unwrap_used)]

use super::*;

/// A 16 by 16 plane whose every row is `left` then `right`: a vertical
/// edge at column 8.
fn step(left: [u8; 8], right: [u8; 8]) -> Vec<u8> {
    let mut s = Vec::with_capacity(16 * 16);
    for _ in 0..16 {
        s.extend_from_slice(&left);
        s.extend_from_slice(&right);
    }
    s
}

#[test]
fn a_weak_edge_moves_its_samples_by_at_most_tc0() {
    // p2 p1 p0 | q0 q1 q2: 60 60 60 | 70 70 70. alpha 20, beta 6.
    let mut s = step([60; 8], [70; 8]);
    luma(&mut s, 8, 1, 16, 4, 20, 6, [1, 1, 1, 1], 8);
    // tc = 1 + 2 (both sides flat): delta = ((70 - 60) * 4 + (60 - 70) + 4)
    // >> 3 = 4, clipped to 3. p1 and q1 move by at most tc0 = 1 toward
    // the mean.
    for y in 0..16 {
        let row = &s[16 * y..16 * y + 16];
        assert_eq!(row[7], 63, "p0 row {y}");
        assert_eq!(row[8], 67, "q0 row {y}");
        assert_eq!(row[6], 61, "p1 row {y}");
        assert_eq!(row[9], 69, "q1 row {y}");
        assert_eq!(row[5], 60);
        assert_eq!(row[10], 70);
    }
}

#[test]
fn a_segment_whose_tc0_is_minus_one_is_skipped() {
    let mut s = step([60; 8], [70; 8]);
    luma(&mut s, 8, 1, 16, 4, 20, 6, [-1, 1, -1, 1], 8);
    assert_eq!(s[7], 60, "segment 0 untouched");
    assert_eq!(s[4 * 16 + 7], 63, "segment 1 filtered");
    assert_eq!(s[8 * 16 + 7], 60);
    assert_eq!(s[12 * 16 + 7], 63);
}

#[test]
fn a_step_as_big_as_alpha_is_a_real_edge() {
    let mut s = step([60; 8], [80; 8]);
    let before = s.clone();
    luma(&mut s, 8, 1, 16, 4, 20, 6, [4; 4], 8);
    assert_eq!(s, before);
    luma_intra(&mut s, 8, 1, 16, 4, 20, 6, 8);
    assert_eq!(s, before);
}

#[test]
fn the_intra_filter_smooths_a_small_step_three_samples_deep() {
    // A step under alpha / 4 + 2 with flat sides: the strong filter.
    let mut s = step([60; 8], [64; 8]);
    luma_intra(&mut s, 8, 1, 16, 4, 20, 6, 8);
    let row = &s[..16];
    // p0' = (p2 + 2p1 + 2p0 + 2q0 + q1 + 4) >> 3 = (60+120+120+128+64+4) >> 3 = 62
    assert_eq!(row[7], 62);
    // p1' = (p2 + p1 + p0 + q0 + 2) >> 2 = (60+60+60+64+2) >> 2 = 61
    assert_eq!(row[6], 61);
    // p2' = (2p3 + 3p2 + p1 + p0 + q0 + 4) >> 3 = (120+180+60+60+64+4) >> 3 = 61
    assert_eq!(row[5], 61);
    // q0' = (p1 + 2p0 + 2q0 + 2q1 + q2 + 4) >> 3 = (60+120+128+128+64+4) >> 3 = 63
    assert_eq!(row[8], 63);
    assert_eq!(row[9], 63);
    // q2' = (2q3 + 3q2 + q1 + q0 + p0 + 4) >> 3 = 512 >> 3
    assert_eq!(row[10], 64);
    assert_eq!(row[4], 60);
    assert_eq!(row[11], 64);
}

/// A horizontal edge is filtered down the columns: the same numbers, the
/// strides swapped.
#[test]
fn a_horizontal_edge_filters_across_rows() {
    let mut s = vec![0u8; 16 * 16];
    for y in 0..16 {
        let v = if y < 8 { 60 } else { 70 };
        s[16 * y..16 * y + 16].fill(v);
    }
    luma(&mut s, 8 * 16, 16, 1, 4, 20, 6, [1; 4], 8);
    for x in 0..16 {
        assert_eq!(s[7 * 16 + x], 63);
        assert_eq!(s[8 * 16 + x], 67);
    }
}

/// Chroma's tc0 is luma's plus one, and FFmpeg's "skip" (255 in its uint8
/// table) wraps to 0, which the chroma filter skips.
#[test]
fn chroma_takes_one_more_than_luma_and_wraps_skip_to_zero() {
    let lf = Lf {
        a: 52,
        b: 52,
        bits: 8,
    };
    // QP 40: the standard's alpha 80 and beta 13 (Table 8-16), tc0 for
    // bS 1 to 3 of 4, 5, 7 (Table 8-17).
    let (index_a, alpha, beta) = lf.params(40).unwrap();
    assert_eq!((index_a, alpha, beta), (92, 80, 13));
    assert_eq!(TC0_TABLE[92][1..], [4, 5, 7]);
    assert_eq!(TC0_TABLE[92][0], 255);
    let mut s = step([60; 8], [70; 8]);
    lf.chroma_edge(&mut s, 8, 16, Dir::Vertical, 2, [0, 1, 0, 1], 40, false);
    // bS 0: untouched; bS 1: tc = 4 + 1 = 5, delta (40 - 10 + 4) >> 3 =
    // 4 inside it.
    assert_eq!(s[7], 60);
    assert_eq!(s[2 * 16 + 7], 64);
    assert_eq!(s[2 * 16 + 8], 66);
    // QP low enough that alpha is 0: nothing.
    assert!(lf.params(10).is_none());
}

#[test]
fn ten_bit_samples_scale_alpha_beta_and_tc() {
    // The 8-bit step of 10 is 40 at 10 bits; alpha 20 and beta 6 scale by 4.
    let mut s: Vec<u16> = (0..16 * 16)
        .map(|i| if i % 16 < 8 { 240 } else { 280 })
        .collect();
    luma(&mut s, 8, 1, 16, 4, 20, 6, [1; 4], 10);
    // tc0 1 scales to 4, plus 2: delta (40 * 4 - 40 + 4) >> 3 = 15,
    // clipped to 6.
    assert_eq!(s[7], 246);
    assert_eq!(s[8], 274);
}

#[test]
fn neighbouring_vectors_need_filtering_a_sample_apart() {
    let mut sl = SliceContext::default();
    sl.hdr.list_count = 1;
    let (b, bn) = (SCAN8[1], SCAN8[0]);
    sl.ref_cache[0][b] = 0;
    sl.ref_cache[0][bn] = 0;
    sl.mv_cache[0][b] = [3, 0];
    sl.mv_cache[0][bn] = [0, 0];
    assert!(!check_mv(&sl, b, bn, 4), "3 quarter samples apart");
    sl.mv_cache[0][b] = [4, 0];
    assert!(check_mv(&sl, b, bn, 4), "a whole sample apart");
    sl.mv_cache[0][b] = [0, -4];
    assert!(check_mv(&sl, b, bn, 4));
    // Three quarter samples apart vertically: under a frame's limit of 4,
    // over a field's of 2.
    sl.mv_cache[0][b] = [0, -3];
    assert!(!check_mv(&sl, b, bn, 4));
    assert!(check_mv(&sl, b, bn, 2));
    sl.mv_cache[0][b] = [0, 0];
    sl.ref_cache[0][b] = 1;
    assert!(check_mv(&sl, b, bn, 4), "another reference");
}
