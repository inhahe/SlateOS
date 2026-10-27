#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::field_reassign_with_default,
    reason = "tests build their contexts a field at a time"
)]

use super::*;

fn sps(poc_type: i32) -> Sps {
    let mut s = Sps::zeroed();
    s.log2_max_frame_num = 4;
    s.log2_max_poc_lsb = 6;
    s.poc_type = poc_type;
    s
}

/// The counts of one picture whose header gave `frame_num` and `lsb`, set
/// into the context as `h264_field_start` sets them, fields starting at
/// `[99, 99]` so what is left alone shows.
fn picture(
    pc: &mut PocContext,
    sps: &Sps,
    frame_num: i32,
    lsb: i32,
    structure: i32,
    ref_idc: u32,
) -> Result<([i32; 2], i32), PocOverflow> {
    pc.frame_num = frame_num;
    pc.poc_lsb = lsb;
    let mut field_poc = [99, 99];
    let mut poc = 0;
    init_poc(&mut field_poc, &mut poc, sps, pc, structure, ref_idc)?;
    Ok((field_poc, poc))
}

#[test]
fn type_2_counts_frames_twice_and_non_references_one_less() {
    let sps = sps(2);
    let mut pc = PocContext::default();
    assert_eq!(picture(&mut pc, &sps, 0, 0, PICT_FRAME, 1), Ok(([0, 0], 0)));
    pc.prev_frame_num = 0;
    assert_eq!(picture(&mut pc, &sps, 3, 0, PICT_FRAME, 1), Ok(([6, 6], 6)));
    assert_eq!(picture(&mut pc, &sps, 3, 0, PICT_FRAME, 0), Ok(([5, 5], 5)));
    // frame_num wrapping past 15 carries 16 into the offset.
    pc.prev_frame_num = 15;
    assert_eq!(
        picture(&mut pc, &sps, 0, 0, PICT_FRAME, 1),
        Ok(([32, 32], 32))
    );
    assert_eq!(pc.frame_num_offset, 16);
    pc.prev_frame_num_offset = 16;
    pc.prev_frame_num = 0;
    assert_eq!(
        picture(&mut pc, &sps, 1, 0, PICT_FRAME, 1),
        Ok(([34, 34], 34))
    );
}

#[test]
fn a_field_sets_its_own_count_and_the_lower_is_the_pictures() {
    let sps = sps(2);
    let mut pc = PocContext::default();
    pc.prev_frame_num = 0;
    assert_eq!(
        picture(&mut pc, &sps, 2, 0, PICT_TOP_FIELD, 1),
        Ok(([4, 99], 4))
    );
    assert_eq!(
        picture(&mut pc, &sps, 2, 0, PICT_BOTTOM_FIELD, 1),
        Ok(([99, 4], 4))
    );
    assert_eq!(
        picture(&mut pc, &sps, 60, 0, PICT_BOTTOM_FIELD, 1),
        Ok(([99, 120], 99))
    );
}

#[test]
fn type_0_carries_the_lsb_into_the_msb_both_ways() {
    let sps = sps(0);
    let mut pc = PocContext::default();
    // The first picture after an IDR: the count starts at 1 << 16.
    assert_eq!(
        picture(&mut pc, &sps, 0, 10, PICT_FRAME, 1),
        Ok(([65546, 65546], 65546))
    );
    assert_eq!(pc.prev_poc_lsb, 10);

    pc.prev_poc_msb = 0;
    pc.prev_poc_lsb = 60;
    // 60 then 2, a gap of 58 against a range of 64: wrapped forward.
    assert_eq!(
        picture(&mut pc, &sps, 0, 2, PICT_FRAME, 1),
        Ok(([66, 66], 66))
    );
    assert_eq!(pc.poc_msb, 64);
    // 60 then 29: under half the range, not wrapped.
    assert_eq!(
        picture(&mut pc, &sps, 0, 29, PICT_FRAME, 1),
        Ok(([29, 29], 29))
    );
    // ...but 28 is half of it exactly.
    assert_eq!(
        picture(&mut pc, &sps, 0, 28, PICT_FRAME, 1),
        Ok(([92, 92], 92))
    );
    // 2 then 60: wrapped back.
    pc.prev_poc_lsb = 2;
    assert_eq!(
        picture(&mut pc, &sps, 0, 60, PICT_FRAME, 1),
        Ok(([-4, -4], -4))
    );
    assert_eq!(pc.poc_msb, -64);
    // 2 then 34: a gap of exactly half is not wrapped back.
    assert_eq!(
        picture(&mut pc, &sps, 0, 34, PICT_FRAME, 1),
        Ok(([34, 34], 34))
    );

    // A frame's bottom field is offset by its delta; a field's is not.
    pc.prev_poc_lsb = 10;
    pc.delta_poc_bottom = -3;
    assert_eq!(
        picture(&mut pc, &sps, 0, 12, PICT_FRAME, 1),
        Ok(([12, 9], 9))
    );
    assert_eq!(
        picture(&mut pc, &sps, 0, 12, PICT_BOTTOM_FIELD, 1),
        Ok(([99, 12], 12))
    );
}

#[test]
fn type_1_follows_the_cycle_of_expected_increments() {
    let mut sps = sps(1);
    sps.poc_cycle_length = 2;
    sps.offset_for_ref_frame[0] = 2;
    sps.offset_for_ref_frame[1] = 4;
    sps.offset_for_ref_frame[2] = 1000; // past the cycle: never added
    sps.offset_for_non_ref_pic = -3;
    sps.offset_for_top_to_bottom_field = 1;
    let mut pc = PocContext::default();
    pc.prev_frame_num = 0;
    pc.delta_poc = [5, 7];

    // Frame 0 is no way into the cycle.
    assert_eq!(
        picture(&mut pc, &sps, 0, 0, PICT_FRAME, 1),
        Ok(([5, 13], 5))
    );
    // Frame 3: one whole cycle (6) and the first offset of the next (2).
    assert_eq!(
        picture(&mut pc, &sps, 3, 0, PICT_FRAME, 1),
        Ok(([13, 21], 13))
    );
    // Not a reference: counted as frame 2 -- 2 + 4 -- and offset by -3.
    assert_eq!(
        picture(&mut pc, &sps, 3, 0, PICT_FRAME, 0),
        Ok(([8, 16], 8))
    );
    // A field takes no second delta.
    assert_eq!(
        picture(&mut pc, &sps, 3, 0, PICT_BOTTOM_FIELD, 1),
        Ok(([99, 14], 14))
    );

    // With no cycle every frame counts as frame 0.
    sps.poc_cycle_length = 0;
    assert_eq!(
        picture(&mut pc, &sps, 9, 0, PICT_FRAME, 1),
        Ok(([5, 13], 5))
    );
    assert_eq!(
        picture(&mut pc, &sps, 9, 0, PICT_FRAME, 0),
        Ok(([2, 10], 2))
    );
}

#[test]
fn a_count_past_an_int_is_refused_and_changes_nothing() {
    let sps0 = sps(0);
    let mut pc = PocContext::default();
    pc.prev_poc_msb = i32::MAX - 63;
    pc.prev_poc_lsb = 0;
    pc.delta_poc_bottom = 100;
    let mut field_poc = [1, 2];
    let mut poc = 3;
    let r = init_poc(&mut field_poc, &mut poc, &sps0, &mut pc, PICT_FRAME, 1);
    assert_eq!(r, Err(PocOverflow));
    assert_eq!((field_poc, poc), ([1, 2], 3));
    // Without the delta it fits.
    pc.delta_poc_bottom = 0;
    let r = init_poc(&mut field_poc, &mut poc, &sps0, &mut pc, PICT_FRAME, 1);
    assert_eq!(r, Ok(()));
    assert_eq!(poc, i32::MAX - 63);

    let mut sps1 = sps(1);
    sps1.offset_for_top_to_bottom_field = i32::MAX;
    let mut pc = PocContext::default();
    pc.delta_poc = [1, 0];
    let r = init_poc(&mut field_poc, &mut poc, &sps1, &mut pc, PICT_TOP_FIELD, 1);
    assert_eq!(
        r,
        Err(PocOverflow),
        "the bottom field's count is checked for a top field too"
    );
}
