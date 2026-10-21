#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::sync::Arc;

use super::*;
use crate::picture::AnyFrame;

const SIZES: TableSizes = TableSizes {
    qscale_table: 3,
    mb_type: 3,
    motion_val: 5,
    ref_index: 4,
};

/// A context with pictures of a frame each, in slots `slots`, each slot's
/// tables marked with its number.
fn context(slots: &[usize]) -> Context {
    let mut h = Context::default();
    for &s in slots {
        let mut t = h.pool.get(SIZES).unwrap();
        let mark = i8::try_from(s).unwrap();
        t.qscale_table[0] = mark;
        t.motion_val[0][0] = [i16::from(mark), 0];
        t.motion_val[1][0] = [i16::from(mark), 1];
        h.pics[s].f = Some(Arc::new(AnyFrame::new(16, 16, 1, 1, 8).unwrap()));
        h.pics[s].put_tables(t);
    }
    h
}

/// New tables are zeroed; reused ones come back as they were left, the
/// last given back first, each list's buffers swapped.
#[test]
fn tables_are_reused_last_first_with_their_lists_swapped() {
    let mut h = context(&[0, 1]);
    h.unref_pic(0);
    h.unref_pic(1);
    let t = h.pool.get(SIZES).unwrap();
    assert_eq!(t.qscale_table[0], 1);
    assert_eq!((t.motion_val[0][0], t.motion_val[1][0]), ([1, 1], [1, 0]));
    let t = h.pool.get(SIZES).unwrap();
    assert_eq!(t.qscale_table[0], 0);
    let t = h.pool.get(SIZES).unwrap();
    assert_eq!(t.qscale_table, [0; 3]);
    assert_eq!(t.motion_val[0], [[0; 2]; 5]);
}

/// While the decoder's view of the picture being decoded holds a slot's
/// tables, letting the slot go keeps them from the pool until the view
/// moves on.
#[test]
fn a_held_slots_tables_wait_for_the_holder() {
    let mut h = context(&[0, 1]);
    h.hold(HOLD_CUR, 0);
    h.unref_pic(0);
    h.unref_pic(1);
    // Only slot 1's are free.
    assert_eq!(h.pool.get(SIZES).unwrap().qscale_table[0], 1);
    assert_eq!(h.pool.get(SIZES).unwrap().qscale_table[0], 0);
    let mut h = context(&[0, 1]);
    h.hold(HOLD_CUR, 0);
    h.unref_pic(0);
    h.unref_pic(1);
    h.hold(HOLD_CUR, 1);
    // Slot 0's go back as the hold moves; slot 1 is in use no longer, and
    // its tables are the pool's already.
    assert_eq!(h.pool.get(SIZES).unwrap().qscale_table[0], 0);
    assert_eq!(h.pool.get(SIZES).unwrap().qscale_table[0], 1);
}

/// Two holders of one slot's tables: they go back when both have let go.
#[test]
fn tables_held_twice_go_back_after_both_holders() {
    let mut h = context(&[0]);
    h.hold(HOLD_CUR, 0);
    h.hold(HOLD_EC, 0);
    h.unref_pic(0);
    h.release_hold(HOLD_EC);
    assert_eq!(h.pool.get(SIZES).unwrap().qscale_table, [0; 3]);
    let mut h = context(&[0]);
    h.hold(HOLD_CUR, 0);
    h.hold(HOLD_EC, 0);
    h.unref_pic(0);
    h.release_hold(HOLD_EC);
    h.release_hold(HOLD_CUR);
    let t = h.pool.get(SIZES).unwrap();
    assert_eq!((t.qscale_table[0], t.motion_val[0][0]), (0, [0, 1]));
}

/// The picture kept for concealment reads the vectors of the picture it
/// holds, before and after that picture's slot lets go.
#[test]
fn the_kept_picture_reads_the_tables_it_holds() {
    let mut h = context(&[3]);
    assert!(h.list0_tables(LAST_PIC_FOR_EC).is_none());
    h.hold(HOLD_EC, 3);
    assert_eq!(h.list0_tables(LAST_PIC_FOR_EC).unwrap().0[0], [3, 0]);
    h.unref_pic(3);
    assert_eq!(h.list0_tables(LAST_PIC_FOR_EC).unwrap().0[0], [3, 0]);
    assert!(h.list0_tables(3).is_none());
    h.unref_pic(LAST_PIC_FOR_EC);
    assert!(h.list0_tables(LAST_PIC_FOR_EC).is_none());
    assert_eq!(h.pool.get(SIZES).unwrap().qscale_table[0], 3);
}

/// Tables from before the pools were freed are dropped, not reused.
#[test]
fn a_reset_pool_drops_tables_from_before() {
    let mut h = context(&[0]);
    h.pool.reset();
    h.unref_pic(0);
    assert_eq!(h.pool.get(SIZES).unwrap().qscale_table, [0; 3]);
}
