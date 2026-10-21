//! Reference list modification at the edge of its range, which no
//! conformance stream reaches: a difference of exactly `max_pic_num` is
//! taken -- it comes back round to the picture it started from -- and one
//! more is refused, as `ff_h264_build_ref_list` does.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::sync::Arc;

use super::*;
use crate::picture::AnyFrame;
use crate::slice::{RefModification, TYPE_P};

/// A context whose current picture is frame 3 of a stream numbering its
/// frames modulo 16, with frames 2, 1 and 0 short-term references, newest
/// first; and a P slice of it with list 0 three long and modified by
/// `mods`, each an operation and its value as the header codes them.
fn three_refs(mods: &[(u8, u32)]) -> (Context, SliceContext) {
    let mut h = Context::default();
    for slot in 0..4 {
        let pic = &mut h.pics[slot];
        pic.f = Some(Arc::new(AnyFrame::new(16, 16, 1, 1, 8).unwrap()));
        pic.frame_num = i32::try_from(slot).unwrap();
        pic.poc = 2 * pic.frame_num;
        pic.reference = PICT_FRAME;
    }
    h.pics[3].reference = 0;
    h.cur_pic_ptr = Some(3);
    h.picture_structure = PICT_FRAME;
    h.short_ref[..3].copy_from_slice(&[Some(2), Some(1), Some(0)]);
    h.short_ref_count = 3;

    let mut sl = SliceContext::default();
    sl.hdr.slice_type_nos = TYPE_P;
    sl.hdr.list_count = 1;
    sl.hdr.ref_count = [3, 0];
    sl.hdr.curr_pic_num = 3;
    sl.hdr.max_pic_num = 16;
    for (i, &(op, val)) in mods.iter().enumerate() {
        sl.hdr.ref_modifications[0][i] = RefModification { op, val };
    }
    sl.hdr.nb_ref_modifications = [mods.len(), 0];
    (h, sl)
}

/// The slots list 0 holds.
fn list0(sl: &SliceContext) -> Vec<Option<usize>> {
    sl.ref_list[0][..3].iter().map(|r| r.parent).collect()
}

#[test]
fn modifications_move_pictures_to_the_front() {
    // No modification: the default order, newest first.
    let (mut h, mut sl) = three_refs(&[]);
    assert_eq!(h.build_ref_list(&mut sl), Ok(()));
    assert_eq!(list0(&sl), [Some(2), Some(1), Some(0)]);
    // Frame 0 (3 - 3) moved to the front pushes the others back.
    let (mut h, mut sl) = three_refs(&[(0, 2)]);
    assert_eq!(h.build_ref_list(&mut sl), Ok(()));
    assert_eq!(list0(&sl), [Some(0), Some(2), Some(1)]);
}

#[test]
fn a_difference_of_max_pic_num_comes_back_to_the_same_picture() {
    // Frame 1 (3 - 2) to the front, then 16 back from it: frame 1 again,
    // into the second place.
    let (mut h, mut sl) = three_refs(&[(0, 1), (0, 15)]);
    assert_eq!(h.build_ref_list(&mut sl), Ok(()));
    assert_eq!(list0(&sl), [Some(1), Some(1), Some(2)]);
    // And 16 forward.
    let (mut h, mut sl) = three_refs(&[(0, 1), (1, 15)]);
    assert_eq!(h.build_ref_list(&mut sl), Ok(()));
    assert_eq!(list0(&sl), [Some(1), Some(1), Some(2)]);
}

#[test]
fn a_difference_past_max_pic_num_is_refused() {
    let (mut h, mut sl) = three_refs(&[(0, 1), (0, 16)]);
    assert_eq!(h.build_ref_list(&mut sl), Err(RefError));
    let (mut h, mut sl) = three_refs(&[(1, 16)]);
    assert_eq!(h.build_ref_list(&mut sl), Err(RefError));
}
