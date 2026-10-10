#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;

/// Three macroblocks across, two down: `mb_stride` 4, as FFmpeg's is one
/// wider than the picture.
fn er() -> Er {
    let mut er = Er::new(3, 2, 4).unwrap();
    er.frame_start();
    er
}

#[test]
fn every_macroblock_is_lost_until_a_slice_decodes_it() {
    let er = er();
    assert_eq!(er.mb_index2xy, [0, 1, 2, 4, 5, 6, 7]);
    assert_eq!(er.error_count, 18);
    assert!(
        er.mb_index2xy[..6]
            .iter()
            .all(|&xy| er.error_status_table[xy] == ER_MB_ERROR | VP_START | ER_MB_END)
    );
    assert_eq!(er.dc_val_base.len(), 7 * 5 + 2 * 4 * 3);
    assert!(er.dc_val_base.iter().all(|&v| v == 1024));
}

/// A slice that ends well clears its macroblocks and marks its last one's
/// end; the whole picture decoded leaves nothing to conceal.
#[test]
fn slices_that_end_well_leave_nothing_to_conceal() {
    let mut er = er();
    // Macroblocks 0 to 3 (the second row's first), ending at 3.
    er.add_slice(0, 0, 0, 1, ER_MB_END);
    assert_eq!(er.error_status_table[0], VP_START);
    assert_eq!(er.error_status_table[1], 0);
    assert_eq!(er.error_status_table[4], ER_MB_END);
    assert!(!er.error_occurred);
    er.check_slice_start(1, 1);
    assert!(!er.error_occurred);
    er.add_slice(1, 1, 2, 1, ER_MB_END);
    assert_eq!(er.error_count, 0);
    assert!(!er.error_occurred);
}

/// A slice that fails marks the picture, and so does one that starts where
/// the slice before did not end.
#[test]
fn a_failed_or_missing_slice_marks_the_picture() {
    let mut e = er();
    e.add_slice(0, 0, 1, 0, ER_MB_ERROR);
    assert!(e.error_occurred);
    assert_eq!(e.error_count, i32::MAX);
    assert_eq!(e.error_status_table[1] & ER_MB_ERROR, ER_MB_ERROR);

    let mut e = er();
    e.add_slice(0, 0, 0, 0, ER_MB_END);
    // The next slice starts at macroblock 2: macroblock 1 is lost.
    e.check_slice_start(2, 0);
    assert!(e.error_occurred);
    // A slice whose end is before its start is ignored.
    let before = e.error_status_table.clone();
    e.add_slice(2, 1, 0, 0, ER_MB_END);
    assert_eq!(e.error_status_table, before);
}

/// At more than 8 bits a byte is half a sample, low byte first.
#[test]
fn deep_samples_are_read_and_written_as_bytes() {
    let mut f = AnyFrame::new(16, 16, 1, 1, 10).unwrap();
    f.fill([0x0102, 0x0304, 0x0506]);
    assert_eq!(f.er_linesize(0), 32);
    assert_eq!((f.er_byte(0, 0), f.er_byte(0, 1)), (2, 1));
    f.er_set_byte(0, 1, 0xAB);
    assert_eq!(f.er_byte(0, 1), 0xAB);
    let AnyFrame::High(b) = &f else { panic!() };
    assert_eq!(b.planes[0].data[0], 0xAB02);
    assert_eq!(b.planes[0].data[1], 0x0102);

    let mut f = AnyFrame::new(16, 16, 1, 1, 8).unwrap();
    f.er_set_byte(2, 5, 7);
    assert_eq!((f.er_byte(2, 5), f.er_linesize(2)), (7, 8));
}

/// A step at a damaged edge spread over the damaged side, beyond what the
/// neighbouring steps account for; over a sound side too, more gently.
#[test]
fn an_edge_step_is_spread_over_the_damaged_side() {
    let line = |f: &AnyFrame| (0..8).map(|i| f.er_byte(0, i)).collect::<Vec<_>>();
    let mut f = AnyFrame::new(16, 16, 1, 1, 8).unwrap();
    for (i, v) in [100, 100, 100, 100, 140, 140, 140, 140]
        .into_iter()
        .enumerate()
    {
        f.er_set_byte(0, i, v);
    }
    let at = [0, 1, 2, 3, 4, 5, 6, 7];
    let mut g = f.clone();
    // Both damaged: d = 40, spread 7, 5, 3, 1 sixteenths of it each way.
    edge(&mut g, 0, at, [true, true]);
    assert_eq!(line(&g), [102, 107, 112, 117, 123, 128, 133, 138]);
    // Only the left: d scaled by 16/9 (71), the left side alone.
    let mut g = f.clone();
    edge(&mut g, 0, at, [true, false]);
    assert_eq!(line(&g), [104, 113, 122, 131, 140, 140, 140, 140]);
    // A flat line has no step.
    let mut g = AnyFrame::new(16, 16, 1, 1, 8).unwrap();
    edge(&mut g, 0, at, [true, true]);
    assert_eq!(line(&g), [0; 8]);
}

/// A damaged intra block's DC is the sound neighbours' in each direction,
/// weighted by nearness; sound blocks and inter blocks keep theirs.
#[test]
fn a_damaged_intra_blocks_dc_is_guessed_from_its_neighbours() {
    let mut e = er();
    // Chroma: one DC per macroblock, all intra; macroblock 1 of the top
    // row damaged.
    let mb_type = [MB_TYPE_INTRA4X4; 8];
    for xy in [0, 1, 2, 4, 5, 6] {
        e.error_status_table[xy] = 0;
    }
    e.error_status_table[1] = ER_DC_ERROR;
    let dc = e.dc_val[1];
    for (xy, v) in [(0, 100), (1, 0), (2, 300), (4, 200), (5, 200), (6, 200)] {
        e.dc_val_base[dc + xy] = v;
    }
    e.guess_dc(&mb_type, 1, 3, 2, 4, false);
    // Left 100 and right 300 at distance 1, below 200 at 1; above none
    // (1024 at 9999, all but weightless).
    let w = 256 * 256 * 256 * 16;
    let weight_sum = 3 * w + w / 9999;
    let want = (w * (100 + 300 + 200) + (w / 9999) * 1024 + weight_sum / 2) / weight_sum;
    assert_eq!(i64::from(e.dc_val_base[dc + 1]), want);
    assert_eq!(e.dc_val_base[dc], 100);
    assert_eq!(e.dc_val_base[dc + 2], 300);
}

/// The luma DC plane smoothed inside its border, each value from the one
/// before as it was.
#[test]
fn filter181_smooths_inside_the_border() {
    // 3 by 3, stride 3: only the middle changes.
    let mut d = [10, 10, 10, 10, 70, 10, 10, 10, 10];
    filter181(&mut d, 3, 3, 3);
    // Across: (-10 + 8 * 70 - 10) / 6 rounded = 90; down: (-10 + 8 * 90 - 10) / 6 = 117.
    let across = ((-10 + 8 * 70 - 10) * 10923 + 32768) >> 16;
    let down = ((-10 + 8 * across - 10) * 10923 + 32768) >> 16;
    assert_eq!(i32::from(d[4]), down);
    assert_eq!(d[..4], [10, 10, 10, 10]);
    assert_eq!(d[5..], [10, 10, 10, 10]);
}
