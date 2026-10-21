//! A packet decoded, and pictures handed out in display order
//! (`decode_nal_units`, `h264_decode_frame`, `finalize_frame`,
//! `output_frame` and `send_next_delayed_frame` in `h264dec.c`, and the
//! container's configuration from `ff_h264_decode_extradata` in
//! `h264_parse.c`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on sizes and counts it checks first"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "slots and delay entries under their fixed sizes, as FFmpeg's asserts keep them"
)]

use std::sync::Arc;

use crate::decoder::{Context, DecodeError, SliceContext};
use crate::nal::{
    Flags, NAL_AUD, NAL_AUXILIARY_SLICE, NAL_DPA, NAL_DPB, NAL_DPC, NAL_END_SEQUENCE,
    NAL_END_STREAM, NAL_FILLER_DATA, NAL_IDR_SLICE, NAL_PPS, NAL_SEI, NAL_SLICE, NAL_SPS,
    NAL_SPS_EXT, Nal, Splitter,
};
use crate::picture::{AnyFrame, DELAYED_PIC_REF, FRAME_RECOVERED_SEI, FrameProps, Pixel};
use crate::ps::{decode_pps, decode_sps};
use ffbits::Bits;

/// `FF_DECODE_ERROR_DECODE_SLICES`: a slice could not be decoded.
const DECODE_ERROR_DECODE_SLICES: i32 = 8;

/// A picture handed out: its samples and what FFmpeg's frame carries with
/// them (`AVFrame`).
#[derive(Clone, Debug)]
pub(crate) struct Output {
    pub(crate) frame: Arc<AnyFrame>,
    pub(crate) props: FrameProps,
    /// The `stereo_mode` metadata: how a stereo pair is packed, by the
    /// frame packing SEI the decoder last read.
    pub(crate) stereo_mode: Option<&'static str>,
}

/// A packet, and what its container says of it that the frames it begins
/// carry (`AVPacket`'s pts, duration and `AV_PKT_FLAG_DISCARD`).
pub(crate) struct Packet<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) time: i64,
    pub(crate) duration: u64,
    pub(crate) discard: bool,
}

/// What a frame packing arrangement says as FFmpeg's metadata names it
/// (`ff_h264_sei_stereo_mode`): nothing until one has been read.
fn stereo_mode(
    arrangement_cancel_flag: i32,
    arrangement_type: u32,
    content_interpretation_type: u32,
) -> Option<&'static str> {
    let rl = content_interpretation_type == 2;
    match arrangement_cancel_flag {
        0 => Some(match arrangement_type {
            0 => {
                if rl {
                    "checkerboard_rl"
                } else {
                    "checkerboard_lr"
                }
            }
            1 => {
                if rl {
                    "col_interleaved_rl"
                } else {
                    "col_interleaved_lr"
                }
            }
            2 => {
                if rl {
                    "row_interleaved_rl"
                } else {
                    "row_interleaved_lr"
                }
            }
            3 => {
                if rl {
                    "right_left"
                } else {
                    "left_right"
                }
            }
            4 => {
                if rl {
                    "bottom_top"
                } else {
                    "top_bottom"
                }
            }
            5 => {
                if rl {
                    "block_rl"
                } else {
                    "block_lr"
                }
            }
            _ => "mono",
        }),
        1 => Some("mono"),
        _ => None,
    }
}

/// Walk the parameter sets an `avcC` record lists: its SPS list (units of
/// kind 7), then its PPS list (kind 8). `list(kind, count)` is told each
/// list's length before its units; `unit(kind, data, header)` is given each
/// unit, its two length bytes in front, and the byte after them, its NAL
/// header. Bytes past the record read as zeros, as FFmpeg reads its
/// padding. Stops, answering false, at a unit that runs past the record or
/// at a callback that answers false.
fn walk_avcc(
    buf: &[u8],
    mut list: impl FnMut(u8, u8) -> bool,
    mut unit: impl FnMut(u8, &[u8], u8) -> bool,
) -> bool {
    let byte = |i: usize| buf.get(i).copied().unwrap_or(0);
    let mut p = 6;
    for kind in [7u8, 8] {
        let cnt = if kind == 7 {
            byte(5) & 0x1f
        } else {
            p += 1;
            byte(p - 1)
        };
        if !list(kind, cnt) {
            return false;
        }
        for _ in 0..cnt {
            let nalsize = usize::from(u16::from_be_bytes([byte(p), byte(p + 1)])) + 2;
            let Some(data) = buf.get(p..p + nalsize) else {
                return false;
            };
            if !unit(kind, data, byte(p + 2)) {
                return false;
            }
            p += nalsize;
        }
    }
    true
}

/// Whether `buf` is an `avcC` record sent in a packet rather than as the
/// container's configuration (`is_avcc_extradata`): both lists have units,
/// every one in bounds and of its list's kind.
fn is_avcc_extradata(buf: &[u8]) -> bool {
    walk_avcc(
        buf,
        |_, count| count != 0,
        |kind, _, header| header & 0x9F == kind,
    )
}

impl Context {
    /// The parameter sets of a container's configuration (`avcC`, or Annex B
    /// units), and how long each unit's length is in the packets after it
    /// (`ff_h264_decode_extradata`).
    pub(crate) fn decode_extradata(&mut self, data: &[u8]) -> Result<(), DecodeError> {
        if data.is_empty() {
            return Err(DecodeError);
        }
        if data[0] == 1 {
            self.is_avc = true;
            if data.len() < 7 {
                return Err(DecodeError);
            }
            // Each set in turn: one that runs past the record, or that is
            // refused, refuses the rest.
            if !walk_avcc(
                data,
                |_, _| true,
                |_, unit, _| self.decode_extradata_ps_mp4(unit).is_ok(),
            ) {
                return Err(DecodeError);
            }
            self.nal_length_size = usize::from(data[4] & 0x03) + 1;
        } else {
            self.is_avc = false;
            self.decode_extradata_ps(data, false)?;
        }
        Ok(())
    }

    /// The parameter sets in `data` (`decode_extradata_ps`): a set FFmpeg
    /// refuses refuses the rest.
    fn decode_extradata_ps(&mut self, data: &[u8], is_avc: bool) -> Result<(), DecodeError> {
        let mut pkt = Splitter::default();
        let flags = Flags {
            nalff: is_avc,
            small_padding: true,
        };
        if pkt.split(data, 2, flags).is_err() {
            return Ok(());
        }
        for nal in pkt.nals().to_vec() {
            match nal.kind {
                NAL_SPS => {
                    if !self.decode_sps_unit(&pkt, data, &nal) {
                        return Err(DecodeError);
                    }
                }
                NAL_PPS => {
                    let mut gb = pkt.reader(data, &nal).ok_or(DecodeError)?;
                    decode_pps(&mut gb, &mut self.ps, nal.size_bits).map_err(|_| DecodeError)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The same, and where a set stored without its escapes fails, again
    /// with them put back (`decode_extradata_ps_mp4`): files like that are
    /// about.
    fn decode_extradata_ps_mp4(&mut self, buf: &[u8]) -> Result<(), DecodeError> {
        if self.decode_extradata_ps(buf, true).is_ok() || self.explode {
            return Ok(());
        }
        if buf.len() / 2 >= (i16::MAX as usize - 64) / 3 {
            return Err(DecodeError);
        }
        let mut escaped = Vec::with_capacity(buf.len() * 3 / 2 + 2);
        let mut i = 0;
        while i < buf.len() {
            if buf.len() - i >= 3 && u32::from_be_bytes([0, buf[i], buf[i + 1], buf[i + 2]]) <= 3 {
                escaped.extend_from_slice(&[0, 0, 3]);
                i += 2;
            } else {
                escaped.push(buf[i]);
                i += 1;
            }
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "under i16::MAX, checked above"
        )]
        let len = (escaped.len() - 2) as u16;
        escaped[..2].copy_from_slice(&len.to_be_bytes());
        // A file whose configuration fails both ways may still decode.
        let _ = self.decode_extradata_ps(&escaped, true);
        Ok(())
    }

    /// A unit's SPS read as `decode_nal_units` reads it: as unescaped, then
    /// from its raw bytes, then once more with truncation ignored.
    fn decode_sps_unit(&mut self, pkt: &Splitter, packet: &[u8], nal: &Nal) -> bool {
        if let Some(mut gb) = pkt.reader(packet, nal) {
            if decode_sps(&mut gb, &mut self.ps, false).is_ok() {
                return true;
            }
        }
        let raw = packet
            .get(nal.raw_start + 1..nal.raw_start + nal.raw_size)
            .unwrap_or(&[]);
        if let Some(mut gb) = Bits::whole(raw) {
            if decode_sps(&mut gb, &mut self.ps, false).is_ok() {
                return true;
            }
        }
        pkt.reader(packet, nal)
            .is_some_and(|mut gb| decode_sps(&mut gb, &mut self.ps, true).is_ok())
    }

    /// Every unit of a packet decoded (`decode_nal_units`).
    pub(crate) fn decode_nal_units(
        &mut self,
        sl: &mut SliceContext,
        pkt: &mut Splitter,
        buf: &[u8],
    ) -> Result<(), DecodeError> {
        let mut idr_cleared = false;
        self.has_slice = false;
        self.nal_unit_type = 0;
        self.current_slice = 0;
        if self.first_field == 0 {
            self.cur_pic_ptr = None;
            self.sei.uninit();
        }

        if self.nal_length_size == 4 {
            let be32 = |i: usize| {
                buf.get(i..i + 4)
                    .map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            };
            #[allow(
                clippy::cast_possible_truncation,
                reason = "FFmpeg compares with the int size as unsigned"
            )]
            let size = buf.len() as u32;
            if buf.len() > 8 && be32(0) == 1 && be32(5) > size {
                self.is_avc = false;
            } else if buf.len() > 3 && be32(0) > 1 && be32(0) <= size {
                self.is_avc = true;
            }
        }
        let flags = Flags {
            nalff: self.is_avc,
            small_padding: false,
        };
        pkt.split(buf, self.nal_length_size, flags).map_err(|e| {
            self.refusal = Some(e);
            DecodeError
        })?;

        let mut ret = Ok(());
        for nal in pkt.nals().to_vec() {
            self.nal_unit_type = nal.kind;
            match nal.kind {
                NAL_IDR_SLICE | NAL_SLICE => {
                    if nal.kind == NAL_IDR_SLICE {
                        // An IDR whose slice type, P, is no IDR's.
                        if pkt.data(buf, &nal).get(1).is_some_and(|b| b & 0xFC == 0x98) {
                            self.next_outputed_poc = i32::MIN;
                            ret = Err(DecodeError);
                            break;
                        }
                        if !idr_cleared {
                            self.idr();
                        }
                        idr_cleared = true;
                        self.has_recovery_point = true;
                    }
                    self.has_slice = true;
                    let Some(mut gb) = pkt.reader(buf, &nal) else {
                        continue;
                    };
                    if self.queue_decode_slice(sl, &nal, &mut gb).is_err() {
                        sl.hdr.ref_count = [0, 0];
                        if self.explode {
                            ret = Err(DecodeError);
                            break;
                        }
                        continue;
                    }
                    let r = self.execute_decode_slices(sl, &mut gb);
                    if r.is_err() && self.explode {
                        ret = r;
                        break;
                    }
                }
                NAL_DPA | NAL_DPB | NAL_DPC => {}
                NAL_SEI => {
                    if self.setup_finished {
                        continue;
                    }
                    let Some(gb) = pkt.reader(buf, &nal) else {
                        continue;
                    };
                    let r = self.sei.decode(&gb, &self.ps);
                    self.has_recovery_point =
                        self.has_recovery_point || self.sei.recovery_frame_cnt != -1;
                    if r.is_err() && self.explode {
                        ret = Err(DecodeError);
                        break;
                    }
                }
                NAL_SPS => {
                    self.decode_sps_unit(pkt, buf, &nal);
                }
                NAL_PPS => {
                    let r = pkt
                        .reader(buf, &nal)
                        .map(|mut gb| decode_pps(&mut gb, &mut self.ps, nal.size_bits));
                    if !matches!(r, Some(Ok(()))) && self.explode {
                        ret = Err(DecodeError);
                        break;
                    }
                }
                NAL_AUD | NAL_END_SEQUENCE | NAL_END_STREAM | NAL_FILLER_DATA | NAL_SPS_EXT
                | NAL_AUXILIARY_SLICE => {}
                _ => {}
            }
        }

        // FFmpeg decodes what is still queued here; with one slice context
        // every slice was decoded as it was queued. A slice that failed, or
        // one missing before a slice, marks the picture -- unless decoding
        // stopped short of here.
        if ret.is_ok() && self.er.error_occurred {
            if let Some(cur) = self.cur_pic_ptr {
                self.pics[cur].props.decode_error_flags |= DECODE_ERROR_DECODE_SLICES;
            }
        }
        self.conceal(sl);
        #[cfg(test)]
        if crate::probe::PROBES.pic {
            self.debug_pic();
        }
        ret
    }

    /// Under H264_PIC, the line `tools/ffdbg`'s FFDBG_PIC prints in FFmpeg,
    /// for `tools/h264cmp.py`: the picture being decoded once a packet's
    /// slices are decoded and concealed -- shown or not -- each plane's
    /// samples summed as bytes, coded size.
    #[cfg(test)]
    fn debug_pic(&self) {
        fn sum<P: crate::picture::Pixel>(p: &crate::picture::Plane<P>) -> u32 {
            let wide = std::mem::size_of::<P>() > 1;
            let (mut a, mut b) = (1u32, 0u32);
            for y in 0..p.height {
                for x in 0..p.width {
                    let v = p.data[y * p.stride + x].int() as u32;
                    let bytes: &[u32] = if wide { &[v & 0xFF, v >> 8] } else { &[v] };
                    for &byte in bytes {
                        a = (a + byte) % 65521;
                        b = (b + a) % 65521;
                    }
                }
            }
            (b << 16) | a
        }
        let Some(cur) = self.cur_pic_ptr else {
            return;
        };
        let pic = &self.pics[cur];
        let Some(f) = pic.f.as_deref() else {
            return;
        };
        let s: [u32; 3] = match f {
            crate::picture::AnyFrame::Eight(f) => std::array::from_fn(|p| sum(&f.planes[p])),
            crate::picture::AnyFrame::High(f) => std::array::from_fn(|p| sum(&f.planes[p])),
        };
        eprintln!(
            "PIC poc={} fpoc={},{} struct={} sum={:08x},{:08x},{:08x}",
            pic.poc, pic.field_poc[0], pic.field_poc[1], self.picture_structure, s[0], s[1], s[2]
        );
    }

    /// Picture `out` handed out, if it can be shown (`finalize_frame`): a
    /// field never decoded copied from the other, and a gray stand-in left
    /// out where asked.
    fn finalize_frame(&mut self, out: usize) -> Option<Output> {
        if !self.output_corrupt && self.pics[out].recovered == 0 {
            return None;
        }
        if self.skip_gray > 0 && self.non_gray != 0 && self.pics[out].gray != 0 {
            return None;
        }
        let field_poc = self.pics[out].field_poc;
        if field_poc[0] == i32::MAX || field_poc[1] == i32::MAX {
            let missing_top = field_poc[0] == i32::MAX;
            if let Some(f) = self.pics[out].f.as_mut() {
                duplicate_field(Arc::make_mut(f), missing_top);
            }
        }
        #[cfg(test)]
        if crate::probe::PROBES.out {
            self.debug_out(out);
        }
        self.output_frame(out)
    }

    /// Under H264_OUT, the line `tools/ffdbg`'s FFDBG_OUT prints in FFmpeg,
    /// for `tools/h264cmp.py`: picture `out`'s POCs and an Adler-32 of each
    /// field's luma, coded size.
    #[cfg(test)]
    fn debug_out(&self, out: usize) {
        fn fields<P: crate::picture::Pixel>(
            p: &crate::picture::FrameBuf<P>,
            w: usize,
            h: usize,
        ) -> [u32; 2] {
            let plane = &p.planes[0];
            let wide = std::mem::size_of::<P>() > 1;
            std::array::from_fn(|par| {
                let (mut a, mut b) = (1u32, 0u32);
                for y in (par..h).step_by(2) {
                    for x in 0..w {
                        let v = plane.data[y * plane.stride + x].int() as u32;
                        let bytes: &[u32] = if wide { &[v & 0xFF, v >> 8] } else { &[v] };
                        for &byte in bytes {
                            a = (a + byte) % 65521;
                            b = (b + a) % 65521;
                        }
                    }
                }
                (b << 16) | a
            })
        }
        let pic = &self.pics[out];
        let (w, h) = (
            (16 * self.mb_width) as usize,
            (16 * self.mb_height) as usize,
        );
        let s = match pic.f.as_deref() {
            Some(crate::picture::AnyFrame::Eight(f)) => fields(f, w, h),
            Some(crate::picture::AnyFrame::High(f)) => fields(f, w, h),
            None => [0, 0],
        };
        eprintln!(
            "OUT poc={} fpoc={},{} top={:08x} bot={:08x}",
            pic.poc, pic.field_poc[0], pic.field_poc[1], s[0], s[1]
        );
    }

    /// The frame and its properties as handed out (`output_frame`).
    fn output_frame(&self, out: usize) -> Option<Output> {
        let pic = &self.pics[out];
        let frame = pic.f.clone()?;
        let mut props = pic.props.clone();
        if pic.sei_recovery_frame_cnt == 0 {
            props.key = true;
        }
        let fp = &self.sei.frame_packing;
        Some(Output {
            frame,
            props,
            stereo_mode: stereo_mode(
                fp.arrangement_cancel_flag,
                fp.arrangement_type,
                fp.content_interpretation_type,
            ),
        })
    }

    /// At the end of the stream, the next picture still held
    /// (`send_next_delayed_frame`).
    pub(crate) fn send_next_delayed_frame(&mut self) -> Option<Output> {
        self.cur_pic_ptr = None;
        self.first_field = 0;
        while let Some(first) = self.delayed_pic[0] {
            let (mut out, mut out_idx) = (first, 0);
            let mut i = 1;
            while let Some(d) = self.delayed_pic[i] {
                if self.pics[d].props.key || self.pics[d].mmco_reset != 0 {
                    break;
                }
                if self.pics[d].poc < self.pics[out].poc {
                    out = d;
                    out_idx = i;
                }
                i += 1;
            }
            self.delayed_pic.copy_within(out_idx + 1.., out_idx);
            if let Some(last) = self.delayed_pic.last_mut() {
                *last = None;
            }
            self.frame_recovered |= self.pics[out].recovered;
            self.pics[out].recovered |= self.frame_recovered & FRAME_RECOVERED_SEI;
            self.pics[out].reference &= !DELAYED_PIC_REF;
            if let Some(o) = self.finalize_frame(out) {
                return Some(o);
            }
        }
        None
    }

    /// One packet decoded, and the picture it lets out if there is one
    /// (`h264_decode_frame`). An empty packet drains what is held.
    pub(crate) fn decode_frame(
        &mut self,
        sl: &mut SliceContext,
        pkt: &mut Splitter,
        packet: &Packet<'_>,
    ) -> Result<Option<Output>, DecodeError> {
        let buf = packet.data;
        self.setup_finished = false;
        self.nb_slice_ctx_queued = 0;
        self.unref_pic(crate::picture::LAST_PIC_FOR_EC);
        if buf.is_empty() {
            return Ok(self.send_next_delayed_frame());
        }
        self.pkt_time = packet.time;
        self.pkt_duration = packet.duration;
        self.pkt_discard = packet.discard;

        if self.is_avc
            && buf.len() >= 9
            && buf[0] == 1
            && buf[2] == 0
            && buf[4] & 0xFC == 0xFC
            && is_avcc_extradata(buf)
        {
            self.decode_extradata(buf)?;
            return Ok(None);
        }

        self.decode_nal_units(sl, pkt, buf)?;

        if self.cur_pic_ptr.is_none() && self.nal_unit_type == NAL_END_SEQUENCE {
            return Ok(self.send_next_delayed_frame());
        }
        if self.cur_pic_ptr.is_none() || !self.has_slice {
            if buf.starts_with(b"Q264") {
                return Ok(None);
            }
            // "no frame!"
            return Err(DecodeError);
        }

        self.field_end()?;
        let out = match self.next_output_pic {
            Some(out) => self.finalize_frame(out),
            None => None,
        };
        self.unref_pic(crate::picture::LAST_PIC_FOR_EC);
        Ok(out)
    }
}

/// The rows of the field never decoded filled from the other's
/// (`av_image_copy` in `finalize_frame`).
fn duplicate_field(f: &mut AnyFrame, missing_top: bool) {
    fn dup<P: Pixel>(f: &mut crate::picture::FrameBuf<P>, missing_top: bool) {
        let from = usize::from(missing_top);
        for plane in &mut f.planes {
            let (stride, width) = (plane.stride, plane.width);
            for pair in 0..plane.height / 2 {
                let src = (2 * pair + from) * stride;
                let dst = (2 * pair + (from ^ 1)) * stride;
                plane.data.copy_within(src..src + width, dst);
            }
        }
    }
    match f {
        AnyFrame::Eight(f) => dup(f, missing_top),
        AnyFrame::High(f) => dup(f, missing_top),
    }
}
