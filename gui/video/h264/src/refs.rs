//! Reference pictures: the lists each slice predicts from, and the marking
//! that decides which decoded pictures stay references (`h264_refs.c`).
//!
//! A slice's lists start in the standard's default order -- for P slices
//! by descending picture number, for B slices by order count either side of
//! the current picture, long-term references after the short -- and the
//! slice header's modifications then move pictures to the front. After a
//! reference picture is decoded, its marking operations (or, without them,
//! the sliding window) retire references and make long-term ones.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on counts it bounds; picture numbers are written wrapping"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "list indices are under the fixed sizes FFmpeg's checks keep them under: \
              32 entries a list, 16 long-term indices, 16 frames of DPB"
)]

use crate::decoder::{Context, MAX_DPB_FRAMES, SliceContext};
use crate::picture::{DELAYED_PIC_REF, FRAME_RECOVERED_HEURISTIC, LAST_PIC_FOR_EC, Ref};
use crate::slice::{
    MMCO_LONG, MMCO_LONG2UNUSED, MMCO_RESET, MMCO_SET_MAX_LONG, MMCO_SHORT2LONG, MMCO_SHORT2UNUSED,
    Mmco, PICT_BOTTOM_FIELD, PICT_FRAME, PICT_TOP_FIELD, TYPE_B, TYPE_I,
};

/// A reference refused, where errors are to explode, or a list FFmpeg
/// cannot fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RefError;

/// `pic` as one of its fields (`pic_as_field`): a bottom field starts a row
/// down; either strides two rows and goes by its own order count.
fn pic_as_field(pic: &mut Ref, parity: i32, field_poc: [i32; 2]) {
    for i in 0..3 {
        if parity == PICT_BOTTOM_FIELD {
            pic.offset[i] += pic.stride[i];
        }
        pic.reference = parity;
        pic.stride[i] *= 2;
    }
    pic.poc = field_poc[usize::from(parity == PICT_BOTTOM_FIELD)];
}

impl Context {
    /// A reference to the whole of picture `slot` (`ref_from_h264pic`).
    pub(crate) fn ref_of(&self, slot: usize) -> Ref {
        let pic = &self.pics[slot];
        let stride = pic
            .f
            .as_deref()
            .map_or([0; 3], |f| [0, 1, 2].map(|p| f.stride(p)));
        Ref {
            parent: Some(slot),
            offset: [0; 3],
            stride,
            reference: pic.reference,
            poc: pic.poc,
            pic_id: pic.pic_id,
        }
    }

    /// `src` into `dest` if it is a reference in `parity`, made a field
    /// with a field's picture number when the parity is one
    /// (`split_field_copy`).
    fn split_field_copy(&self, dest: &mut Ref, src: usize, parity: i32, id_add: i32) -> bool {
        let matched = self.pics[src].reference & parity != 0;
        if matched {
            *dest = self.ref_of(src);
            if parity != PICT_FRAME {
                pic_as_field(dest, parity, self.pics[src].field_poc);
                dest.pic_id = dest.pic_id * 2 + id_add;
            }
        }
        matched
    }

    /// `input`'s references into `def` in the default order: for a field,
    /// the same parity and the other taken in turn (`build_def_list`). Each
    /// picture is given its picture number on the way (`pic_id`).
    fn build_def_list(
        &mut self,
        def: &mut [Ref],
        input: &[Option<usize>],
        is_long: bool,
        sel: i32,
    ) -> usize {
        let len = input.len();
        let has = |me: &Self, k: usize, mask: i32| {
            input[k].is_some_and(|p| me.pics[p].reference & mask != 0)
        };
        let mut i = [0usize; 2];
        let mut index = 0;
        while i[0] < len || i[1] < len {
            while i[0] < len && !has(self, i[0], sel) {
                i[0] += 1;
            }
            while i[1] < len && !has(self, i[1], sel ^ 3) {
                i[1] += 1;
            }
            for (side, parity, id_add) in [(0, sel, 1), (1, sel ^ 3, 0)] {
                if i[side] < len {
                    let Some(p) = input[i[side]] else { continue };
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_possible_wrap,
                        reason = "under 32"
                    )]
                    let id = if is_long {
                        i[side] as i32
                    } else {
                        self.pics[p].frame_num
                    };
                    self.pics[p].pic_id = id;
                    // FFmpeg asserts there is room: references are capped
                    // at 16 frames, 32 fields.
                    if let Some(slot) = def.get_mut(index) {
                        self.split_field_copy(slot, p, parity, id_add);
                        index += 1;
                    }
                    i[side] += 1;
                }
            }
        }
        index
    }

    /// The short-term references in order count from `limit` -- up when
    /// `dir` is 0, down when it is 1 -- appended to `sorted`
    /// (`add_sorted`). A picture of order count `INT_MIN` going down is
    /// never taken, as FFmpeg's sentinel makes it.
    fn add_sorted(&self, sorted: &mut Vec<usize>, mut limit: i32, dir: i32) {
        let up = dir == 0;
        loop {
            let mut best_poc = if up { i32::MAX } else { i32::MIN };
            let mut best = None;
            for &p in self.short_ref[..self.short_ref_count].iter().flatten() {
                let poc = self.pics[p].poc;
                let past = (poc > limit) ^ !up;
                let better = (poc < best_poc) ^ !up;
                if past && better {
                    best_poc = poc;
                    best = Some(p);
                }
            }
            if best_poc == (if up { i32::MAX } else { i32::MIN }) {
                break;
            }
            let Some(p) = best else { break };
            sorted.push(p);
            limit = self.pics[p].poc.wrapping_sub(dir);
        }
    }

    /// Whether picture `slot`'s frame is not the size or layout of the
    /// current one's (`mismatches_ref`).
    pub(crate) fn mismatches_ref(&self, slot: usize) -> bool {
        self.cur_pic_ptr
            .is_none_or(|cur| !self.pics[cur].same_shape(&self.pics[slot]))
    }

    /// The default lists (`h264_initialise_ref_list`).
    fn initialise_ref_list(&mut self, sl: &mut SliceContext) {
        let hdr = &sl.hdr;
        if hdr.slice_type_nos == TYPE_B {
            let Some(cur) = self.cur_pic_ptr else { return };
            let cur_poc = if self.field_picture() {
                self.pics[cur].field_poc[usize::from(self.picture_structure == PICT_BOTTOM_FIELD)]
            } else {
                self.pics[cur].poc
            };
            let mut lens = [0usize; 2];
            for list in 0..2 {
                let mut sorted = Vec::with_capacity(32);
                self.add_sorted(&mut sorted, cur_poc, 1 ^ list);
                self.add_sorted(&mut sorted, cur_poc, list);
                let sorted: Vec<Option<usize>> = sorted.into_iter().map(Some).collect();
                let structure = self.picture_structure;
                let mut l = std::mem::replace(
                    &mut sl.ref_list[list as usize],
                    Box::new([Ref::default(); 48]),
                );
                let mut len = self.build_def_list(&mut l[..32], &sorted, false, structure);
                let long = self.long_ref[..16].to_vec();
                len += self.build_def_list(&mut l[len..32], &long, true, structure);
                for r in &mut l[len..32] {
                    *r = Ref::default();
                }
                sl.ref_list[list as usize] = l;
                lens[list as usize] = len;
            }
            if lens[0] == lens[1] && lens[1] > 1 {
                let same =
                    (0..lens[0]).all(|i| self.same_buffer(sl.ref_list[0][i], sl.ref_list[1][i]));
                if same {
                    sl.ref_list[1].swap(0, 1);
                }
            }
        } else {
            let structure = self.picture_structure;
            let short = self.short_ref[..self.short_ref_count].to_vec();
            let long = self.long_ref[..16].to_vec();
            let mut l = std::mem::replace(&mut sl.ref_list[0], Box::new([Ref::default(); 48]));
            let mut len = self.build_def_list(&mut l[..32], &short, false, structure);
            len += self.build_def_list(&mut l[len..32], &long, true, structure);
            for r in &mut l[len..32] {
                *r = Ref::default();
            }
            sl.ref_list[0] = l;
        }

        let lists = 1 + usize::from(sl.hdr.slice_type_nos == TYPE_B);
        for j in 0..lists {
            for i in 0..sl.hdr.ref_count[j] as usize {
                if sl.ref_list[j][i]
                    .parent
                    .is_some_and(|p| self.mismatches_ref(p))
                {
                    sl.ref_list[j][i] = Ref::default();
                }
            }
        }
        for i in 0..sl.hdr.list_count as usize {
            self.default_ref[i] = sl.ref_list[i][0];
        }
    }

    /// Whether two references' pictures share one frame buffer
    /// (`parent->f->buf[0]->buffer` compared).
    fn same_buffer(&self, a: Ref, b: Ref) -> bool {
        let buf = |r: Ref| r.parent.and_then(|p| self.pics[p].f.as_ref());
        match (buf(a), buf(b)) {
            (Some(x), Some(y)) => std::sync::Arc::ptr_eq(x, y),
            _ => false,
        }
    }

    /// `pic_num` as a frame number or long-term index, and the field it
    /// names (`pic_num_extract`): in a field picture an even number is the
    /// other parity's.
    fn pic_num_extract(&self, mut pic_num: i32) -> (i32, i32) {
        let mut structure = self.picture_structure;
        if self.field_picture() {
            if pic_num & 1 == 0 {
                structure ^= PICT_FRAME;
            }
            pic_num >>= 1;
        }
        (pic_num, structure)
    }

    /// Each list's MBAFF field entries from its frame entries
    /// (`h264_fill_mbaff_ref_list`).
    fn fill_mbaff_ref_list(&self, sl: &mut SliceContext) {
        for list in 0..sl.hdr.list_count as usize {
            for i in 0..sl.hdr.ref_count[list] as usize {
                let frame = sl.ref_list[list][i];
                let Some(parent) = frame.parent else {
                    continue;
                };
                let pic = &self.pics[parent];
                let mut top = frame;
                for j in 0..3 {
                    top.stride[j] <<= 1;
                }
                top.reference = PICT_TOP_FIELD;
                top.poc = pic.field_poc[0];
                let mut bottom = top;
                let strides = pic
                    .f
                    .as_deref()
                    .map_or([0; 3], |f| [0, 1, 2].map(|p| f.stride(p)));
                for j in 0..3 {
                    bottom.offset[j] += strides[j];
                }
                bottom.reference = PICT_BOTTOM_FIELD;
                bottom.poc = pic.field_poc[1];
                sl.ref_list[list][16 + 2 * i] = top;
                sl.ref_list[list][16 + 2 * i + 1] = bottom;
            }
        }
    }

    /// Each list of the slice: the default order, the header's
    /// modifications, a missing entry filled from the default and a gray
    /// stand-in passed over where there is better (`ff_h264_build_ref_list`).
    pub(crate) fn build_ref_list(&mut self, sl: &mut SliceContext) -> Result<(), RefError> {
        #[cfg(test)]
        self.debug_lists();
        self.initialise_ref_list(sl);

        for list in 0..sl.hdr.list_count as usize {
            let mut pred = sl.hdr.curr_pic_num;
            for index in 0..sl.hdr.nb_ref_modifications[list] {
                let m = sl.hdr.ref_modifications[list][index];
                let (found, pic_id, structure) = match m.op {
                    0 | 1 => {
                        let abs_diff_pic_num = m.val.wrapping_add(1);
                        #[allow(clippy::cast_sign_loss, reason = "a power of 2 up to 1 << 17")]
                        if abs_diff_pic_num > sl.hdr.max_pic_num as u32 {
                            return Err(RefError);
                        }
                        #[allow(
                            clippy::cast_possible_wrap,
                            reason = "FFmpeg's unsigned added to int"
                        )]
                        let diff = abs_diff_pic_num as i32;
                        pred = if m.op == 0 {
                            pred.wrapping_sub(diff)
                        } else {
                            pred.wrapping_add(diff)
                        };
                        pred &= sl.hdr.max_pic_num - 1;
                        let (frame_num, structure) = self.pic_num_extract(pred);
                        let found = self.short_ref[..self.short_ref_count]
                            .iter()
                            .rev()
                            .flatten()
                            .copied()
                            .find(|&p| {
                                self.pics[p].frame_num == frame_num
                                    && self.pics[p].reference & structure != 0
                            });
                        (found, pred, structure)
                    }
                    _ => {
                        #[allow(
                            clippy::cast_possible_wrap,
                            reason = "FFmpeg's unsigned taken as int"
                        )]
                        let pic_id = m.val as i32;
                        let (long_idx, structure) = self.pic_num_extract(pic_id);
                        #[allow(
                            clippy::cast_sign_loss,
                            reason = "compared unsigned, as FFmpeg does"
                        )]
                        if long_idx as u32 > 31 {
                            return Err(RefError);
                        }
                        let found = self.long_ref[long_idx as usize]
                            .filter(|&p| self.pics[p].reference & structure != 0);
                        (found, pic_id, structure)
                    }
                };

                match found {
                    Some(r) if !self.mismatches_ref(r) => {
                        let count = sl.hdr.ref_count[list] as usize;
                        let mut i = index;
                        while i + 1 < count {
                            let e = sl.ref_list[list][i];
                            if e.parent
                                .is_some_and(|p| self.pics[r].long_ref == self.pics[p].long_ref)
                                && pic_id == e.pic_id
                            {
                                break;
                            }
                            i += 1;
                        }
                        while i > index {
                            sl.ref_list[list][i] = sl.ref_list[list][i - 1];
                            i -= 1;
                        }
                        let mut entry = self.ref_of(r);
                        if self.field_picture() {
                            pic_as_field(&mut entry, structure, self.pics[r].field_poc);
                        }
                        sl.ref_list[list][index] = entry;
                    }
                    _ => {
                        if self.explode {
                            return Err(RefError);
                        }
                        sl.ref_list[list][index] = Ref::default();
                    }
                }
            }
        }

        // A frame's entry must be both fields.
        let field = self.field_picture();
        let not_frame = |r: Ref| !field && r.reference & 3 != 3;
        for list in 0..sl.hdr.list_count as usize {
            for index in 0..sl.hdr.ref_count[list] as usize {
                let e = sl.ref_list[list][index];
                if e.parent.is_none() || not_frame(e) {
                    if self.explode {
                        return Err(RefError);
                    }
                    self.last_pocs = [i32::MIN; MAX_DPB_FRAMES];
                    let d = self.default_ref[list];
                    if d.parent.is_some() && !not_frame(d) {
                        sl.ref_list[list][index] = d;
                    } else {
                        return Err(RefError);
                    }
                }
                let gray = |me: &Self, r: Ref| r.parent.is_some_and(|p| me.pics[p].gray != 0);
                if self.noref_gray > 0 && gray(self, sl.ref_list[list][index]) && self.non_gray != 0
                {
                    for j in 0..sl.hdr.list_count as usize {
                        let d = self.default_ref[(list + j) & 1];
                        if d.parent.is_some() && !gray(self, d) && !not_frame(d) {
                            sl.ref_list[list][index] = d;
                            break;
                        }
                    }
                }
            }
        }

        if self.mb_aff_frame {
            self.fill_mbaff_ref_list(sl);
        }
        Ok(())
    }

    /// Picture `pic` no longer a reference in the fields `refmask` clears
    /// (`unreference_pic`): `true` once it is none at all, when it is kept
    /// only if it is still to be shown.
    fn unreference_pic(&mut self, pic: usize, refmask: i32) -> bool {
        self.pics[pic].reference &= refmask;
        if self.pics[pic].reference != 0 {
            return false;
        }
        if self.delayed_pic.iter().map_while(|d| *d).any(|d| d == pic) {
            self.pics[pic].reference = DELAYED_PIC_REF;
        }
        true
    }

    /// The short-term reference of `frame_num` and where it is in the list
    /// (`find_short`).
    fn find_short(&mut self, frame_num: i32) -> Option<(usize, usize)> {
        for i in 0..self.short_ref_count {
            let p = self.short_ref[i]?;
            #[cfg(test)]
            self.debug_mmco
                .push(format!("{i} {}", self.pics[p].frame_num));
            if self.pics[p].frame_num == frame_num {
                return Some((p, i));
            }
        }
        None
    }

    /// FFmpeg's `-debug mmco` view of the reference lists
    /// (`print_short_term`, `print_long_term`), for the tests.
    #[cfg(test)]
    fn debug_lists(&mut self) {
        self.debug_mmco.push("short term list:".to_owned());
        for i in 0..self.short_ref_count {
            if let Some(p) = self.short_ref[i] {
                let (f, poc) = (self.pics[p].frame_num, self.pics[p].poc);
                self.debug_mmco.push(format!("{i} fn:{f} poc:{poc}"));
            }
        }
        self.debug_mmco.push("long term list:".to_owned());
        for i in 0..16 {
            if let Some(p) = self.long_ref[i] {
                let (f, poc) = (self.pics[p].frame_num, self.pics[p].poc);
                self.debug_mmco.push(format!("{i} fn:{f} poc:{poc}"));
            }
        }
    }

    /// The short-term list without its entry `i` (`remove_short_at_index`).
    fn remove_short_at_index(&mut self, i: usize) {
        self.short_ref[i] = None;
        self.short_ref_count -= 1;
        if self.short_ref_count != 0 {
            self.short_ref.copy_within(i + 1..=self.short_ref_count, i);
        }
    }

    /// The short-term reference of `frame_num` unreferenced in the fields
    /// `ref_mask` clears, out of the list once it is no reference at all
    /// (`remove_short`).
    fn remove_short(&mut self, frame_num: i32, ref_mask: i32) -> Option<usize> {
        #[cfg(test)]
        self.debug_mmco.push(format!(
            "remove short {frame_num} count {}",
            self.short_ref_count
        ));
        let (pic, i) = self.find_short(frame_num)?;
        if self.unreference_pic(pic, ref_mask) {
            self.remove_short_at_index(i);
        }
        Some(pic)
    }

    /// The same for long-term index `i` (`remove_long`).
    fn remove_long(&mut self, i: usize, ref_mask: i32) -> Option<usize> {
        let pic = self.long_ref[i]?;
        if self.unreference_pic(pic, ref_mask) {
            self.pics[pic].long_ref = 0;
            self.long_ref[i] = None;
            self.long_ref_count -= 1;
        }
        Some(pic)
    }

    /// No references left at all (`ff_h264_remove_all_refs`): the newest
    /// short-term one kept aside first, to conceal a lost picture with.
    pub(crate) fn remove_all_refs(&mut self) {
        for i in 0..16 {
            self.remove_long(i, 0);
        }
        if self.short_ref_count != 0 && !self.pics[LAST_PIC_FOR_EC].in_use() {
            if let Some(first) = self.short_ref[0] {
                self.pics[LAST_PIC_FOR_EC].unref();
                let src = self.pics[first].clone();
                self.pics[LAST_PIC_FOR_EC].ref_from(&src);
            }
        }
        for i in 0..self.short_ref_count {
            if let Some(p) = self.short_ref[i] {
                self.unreference_pic(p, 0);
            }
            self.short_ref[i] = None;
        }
        self.short_ref_count = 0;
        self.default_ref = [Ref::default(); 2];
    }

    /// Without operations in the header, the oldest short-term reference
    /// let go once the references fill the DPB -- both its fields, for a
    /// field picture (`generate_sliding_window_mmcos`).
    fn generate_sliding_window_mmcos(&mut self) {
        let mut nb_mmco = 0;
        let Some(sps) = self.ps.sps.as_deref() else {
            return;
        };
        let second_field_of_reference = self.field_picture()
            && self.first_field == 0
            && self
                .cur_pic_ptr
                .is_some_and(|c| self.pics[c].reference != 0);
        #[allow(clippy::cast_possible_wrap, reason = "at most 32")]
        let refs = (self.long_ref_count + self.short_ref_count) as i32;
        if self.short_ref_count != 0 && refs >= sps.ref_frame_count && !second_field_of_reference {
            if let Some(oldest) = self.short_ref[self.short_ref_count - 1] {
                self.mmco[0].opcode = MMCO_SHORT2UNUSED;
                self.mmco[0].short_pic_num = self.pics[oldest].frame_num;
                nb_mmco = 1;
                if self.field_picture() {
                    self.mmco[0].short_pic_num *= 2;
                    self.mmco[1].opcode = MMCO_SHORT2UNUSED;
                    self.mmco[1].short_pic_num = self.mmco[0].short_pic_num + 1;
                    nb_mmco = 2;
                }
            }
        }
        self.nb_mmco = nb_mmco;
    }

    /// The current picture's marking carried out (`ff_h264_execute_ref_pic_marking`):
    /// its operations, or the sliding window, then the current picture made
    /// a reference; one reference let go if there are too many; and a
    /// random access point no SEI marks recognised by its shape. An
    /// inconsistency is an error only where errors are to explode.
    pub(crate) fn execute_ref_pic_marking(&mut self) -> Result<(), RefError> {
        let mut err = false;
        let mut current_ref_assigned = false;
        let (Some(sps), Some(cur)) = (self.ps.sps.clone(), self.cur_pic_ptr) else {
            return if self.explode { Err(RefError) } else { Ok(()) };
        };

        if self.explicit_ref_marking == 0 {
            self.generate_sliding_window_mmcos();
        }
        let ops: Vec<Mmco> = self.mmco[..self.nb_mmco].to_vec();
        #[cfg(test)]
        if ops.is_empty() {
            self.debug_mmco.push("no mmco here".to_owned());
        }
        for m in ops {
            #[cfg(test)]
            self.debug_mmco.push(format!(
                "mmco:{} {} {}",
                m.opcode, m.short_pic_num, m.long_arg
            ));
            #[allow(clippy::cast_sign_loss, reason = "checked under 32 where it was read")]
            let long_arg = m.long_arg as usize;
            match m.opcode {
                MMCO_SHORT2UNUSED | MMCO_SHORT2LONG => {
                    let (frame_num, structure) = self.pic_num_extract(m.short_pic_num);
                    let Some((pic, j)) = self.find_short(frame_num) else {
                        let already = m.opcode == MMCO_SHORT2LONG
                            && self.long_ref[long_arg]
                                .is_some_and(|p| self.pics[p].frame_num == frame_num);
                        if !already {
                            err = true;
                        }
                        continue;
                    };
                    if m.opcode == MMCO_SHORT2UNUSED {
                        #[cfg(test)]
                        self.debug_mmco.push(format!(
                            "mmco: unref short {} count {}",
                            m.short_pic_num, self.short_ref_count
                        ));
                        self.remove_short(frame_num, structure ^ PICT_FRAME);
                    } else {
                        if self.long_ref[long_arg] != Some(pic) {
                            self.remove_long(long_arg, 0);
                        }
                        self.remove_short_at_index(j);
                        self.long_ref[long_arg] = Some(pic);
                        self.pics[pic].long_ref = 1;
                        self.long_ref_count += 1;
                    }
                }
                MMCO_LONG2UNUSED => {
                    let (j, structure) = self.pic_num_extract(m.long_arg);
                    #[allow(clippy::cast_sign_loss, reason = "a long-term index, under 16")]
                    let j = j as usize;
                    #[cfg(test)]
                    if self.long_ref[j].is_none() {
                        self.debug_mmco.push("mmco: unref long failure".to_owned());
                    }
                    if self.long_ref[j].is_some() {
                        self.remove_long(j, structure ^ PICT_FRAME);
                    }
                }
                MMCO_LONG => {
                    if self.short_ref[0] == Some(cur) {
                        self.remove_short_at_index(0);
                    }
                    if self.pics[cur].long_ref != 0 {
                        for j in 0..self.long_ref.len() {
                            if self.long_ref[j] == Some(cur) {
                                self.remove_long(j, 0);
                            }
                        }
                    }
                    if self.long_ref[long_arg] != Some(cur) {
                        self.remove_long(long_arg, 0);
                        self.long_ref[long_arg] = Some(cur);
                        self.pics[cur].long_ref = 1;
                        self.long_ref_count += 1;
                    }
                    self.pics[cur].reference |= self.picture_structure;
                    current_ref_assigned = true;
                }
                MMCO_SET_MAX_LONG => {
                    for j in long_arg..16 {
                        self.remove_long(j, 0);
                    }
                }
                MMCO_RESET => {
                    while self.short_ref_count != 0 {
                        if let Some(p) = self.short_ref[0] {
                            let frame_num = self.pics[p].frame_num;
                            self.remove_short(frame_num, 0);
                        }
                    }
                    for j in 0..16 {
                        self.remove_long(j, 0);
                    }
                    self.poc.frame_num = 0;
                    self.pics[cur].frame_num = 0;
                    self.mmco_reset = 1;
                    self.pics[cur].mmco_reset = 1;
                    self.last_pocs = [i32::MIN; MAX_DPB_FRAMES];
                }
                _ => {}
            }
        }

        if !current_ref_assigned {
            if self.short_ref_count != 0 && self.short_ref[0] == Some(cur) {
                // The second field of a pair whose first is short-term.
                self.pics[cur].reference |= self.picture_structure;
            } else if self.pics[cur].long_ref != 0 {
                err = true;
            } else {
                let frame_num = self.pics[cur].frame_num;
                if self.remove_short(frame_num, 0).is_some() {
                    err = true;
                }
                if self.short_ref_count != 0 {
                    self.short_ref.copy_within(0..self.short_ref_count, 1);
                }
                self.short_ref[0] = Some(cur);
                self.short_ref_count += 1;
                self.pics[cur].reference |= self.picture_structure;
            }
        }

        #[allow(clippy::cast_possible_wrap, reason = "at most 32")]
        let refs = (self.long_ref_count + self.short_ref_count) as i32;
        if refs > sps.ref_frame_count.max(1) {
            err = true;
            if self.long_ref_count != 0 && self.short_ref_count == 0 {
                if let Some(i) = (0..16).find(|&i| self.long_ref[i].is_some()) {
                    self.remove_long(i, 0);
                }
            } else if let Some(oldest) = self.short_ref[self.short_ref_count - 1] {
                let frame_num = self.pics[oldest].frame_num;
                self.remove_short(frame_num, 0);
            }
        }

        let mut i = 0;
        while i < self.short_ref_count {
            if let Some(p) = self.short_ref[i] {
                if self.pics[p].invalid_gap != 0 {
                    let mask = (1i32 << sps.log2_max_frame_num) - 1;
                    let d = (self.pics[cur].frame_num - self.pics[p].frame_num) & mask;
                    if d > sps.ref_frame_count {
                        let frame_num = self.pics[p].frame_num;
                        self.remove_short(frame_num, 0);
                    }
                }
            }
            i += 1;
        }

        #[cfg(test)]
        self.debug_lists();
        let mut pps_ref_count = [0u32; 2];
        for pps in self.ps.pps_list.iter().flatten() {
            pps_ref_count[0] = pps_ref_count[0].max(pps.ref_count[0]);
            pps_ref_count[1] = pps_ref_count[1].max(pps.ref_count[1]);
        }
        let field = u32::from(self.picture_structure != PICT_FRAME);
        let has_b_frames = self.avctx.has_b_frames;
        if !err
            && self.long_ref_count == 0
            && (self.short_ref_count <= 2
                || (pps_ref_count[0] <= 2 && pps_ref_count[1] <= 1 && has_b_frames != 0)
                || (pps_ref_count[0] <= 1 + field && pps_ref_count[1] <= 1))
            && pps_ref_count[0] <= 2 + field + 2 * u32::from(!self.has_recovery_point)
            && self.pics[cur].props.pict_type == TYPE_I
        {
            self.pics[cur].recovered |= FRAME_RECOVERED_HEURISTIC;
            if has_b_frames == 0 {
                self.frame_recovered |= FRAME_RECOVERED_HEURISTIC;
            }
        }

        if err && self.explode {
            Err(RefError)
        } else {
            Ok(())
        }
    }
}
