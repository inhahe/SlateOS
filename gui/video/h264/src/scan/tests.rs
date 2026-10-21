#![allow(clippy::indexing_slicing, clippy::unwrap_used)]

use super::*;

/// Each scan is a permutation of its block's positions.
#[test]
fn every_scan_visits_every_position_once() {
    for bypass in [false, true] {
        let s = ScanTables::new(bypass);
        for base in [0, Q0] {
            for (at, n) in [
                (ZIGZAG, 16),
                (FIELD, 16),
                (ZIGZAG8X8, 64),
                (ZIGZAG8X8_CAVLC, 64),
                (FIELD8X8, 64),
                (FIELD8X8_CAVLC, 64),
            ] {
                let mut seen = s.all[base + at..base + at + n].to_vec();
                seen.sort_unstable();
                assert_eq!(seen, (0..u8::try_from(n).unwrap()).collect::<Vec<_>>());
            }
        }
    }
}

#[test]
fn scans_are_stored_transposed_except_at_qp0_bypassing_the_transform() {
    let s = ScanTables::new(false);
    // Zigzag's third coefficient is row 1, column 0: raster 4, transposed 1.
    assert_eq!(s.all[ZIGZAG + 2], 1);
    assert_eq!(s.all[ZIGZAG + 1], 4);
    // Without bypass, QP 0 reads the same transposed scans.
    assert_eq!(s.all[ZIGZAG..ZIGZAG + Q0], s.all[ZIGZAG + Q0..]);
    let b = ScanTables::new(true);
    assert_eq!(b.all[..ZIGZAG + Q0], s.all[..ZIGZAG + Q0]);
    assert_eq!(b.all[Q0 + ZIGZAG..Q0 + ZIGZAG + 16], ZIGZAG_SCAN);
    assert_eq!(b.all[Q0 + FIELD8X8_CAVLC + 1], FIELD_SCAN8X8_CAVLC[1]);
}

/// Up to 16 places before the first scan are FFmpeg's padding, zeros: a
/// CAVLC block whose zeros run out before its scan's start puts its
/// remaining coefficients at position 0. Before that is refused.
#[test]
fn before_the_first_scan_is_sixteen_zeros() {
    let s = ScanTables::new(false);
    let scan = Scan {
        tables: &s.all,
        start: ZIGZAG,
    };
    for at in -16..0 {
        assert_eq!(scan.get(at), Ok(0));
    }
    assert_eq!(scan.get(-17), Err(DecodeError));
    assert_eq!(scan.get(0), Ok(usize::from(s.all[ZIGZAG])));
    // Past the last table is refused too.
    let last = Scan {
        tables: &s.all,
        start: Q0 + FIELD8X8_CAVLC,
    };
    assert_eq!(last.get(63), Ok(63));
    assert_eq!(last.get(64), Err(DecodeError));
}

/// A 4x4 run one past its scan reads the first entry of the 8x8 table that
/// follows it, as FFmpeg's context lays the tables out.
#[test]
fn one_past_a_4x4_scan_is_the_next_tables_first_entry() {
    let s = ScanTables::new(true);
    for base in [0, Q0] {
        assert_eq!(s.all[base + ZIGZAG + 16], s.all[base + ZIGZAG8X8]);
        assert_eq!(s.all[base + FIELD + 16], s.all[base + FIELD8X8]);
        assert_eq!(s.all[base + FIELD + 16], 0);
    }
}

#[test]
fn a_macroblock_reads_its_field_and_qp0_scans() {
    assert_eq!(ScanTables::cavlc(false, false), (ZIGZAG, ZIGZAG8X8_CAVLC));
    assert_eq!(ScanTables::cavlc(true, false), (FIELD, FIELD8X8_CAVLC));
    assert_eq!(
        ScanTables::cavlc(true, true),
        (Q0 + FIELD, Q0 + FIELD8X8_CAVLC)
    );
}
