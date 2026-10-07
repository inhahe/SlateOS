//! A frame from sync to samples: minimp3's `mp3dec_t`, the sync and frame
//! dispatch of `mp3dec_decode_frame`, and `L3_restore_reservoir` and
//! `L3_save_reservoir`, translated into Rust (minimp3, CC0:
//! `licenses/minimp3-LICENSE`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "sums of a position in the input and a frame's size (at most 2304 bytes), and of counts below the 2815-byte main data and the 2304-sample output"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "the main data is at most 511 reservoir bytes and a Layer III frame's 2300 bytes after its header (MAX_FREE_FORMAT_FRAME_SIZE bounds even a free-format frame), within its 2815; a frame's samples are within MAX_SAMPLES_PER_FRAME"
)]

use crate::header::{Bits, HDR_SIZE, Hdr, MAX_FREE_FORMAT_FRAME_SIZE, find_frame};
use crate::l3::GrInfo;
use crate::{MAX_SAMPLES_PER_FRAME, l3, l12, synth};

const MAX_BITRESERVOIR_BYTES: usize = 511;
const MAX_L3_FRAME_PAYLOAD_BYTES: usize = MAX_FREE_FORMAT_FRAME_SIZE;

/// The memory a frame is decoded in: minimp3's `mp3dec_scratch_t`, kept
/// from frame to frame (minimp3's is a local).
pub(crate) struct Scratch {
    maindata: [u8; MAX_BITRESERVOIR_BYTES + MAX_L3_FRAME_PAYLOAD_BYTES],
    gr_info: [GrInfo; 4],
    grbuf: [f32; 1152],
    scf: [f32; 40],
    syn: [f32; 33 * 64],
    ist_pos: [[u8; 39]; 2],
}

impl Scratch {
    pub(crate) fn new() -> Self {
        Self {
            maindata: [0; MAX_BITRESERVOIR_BYTES + MAX_L3_FRAME_PAYLOAD_BYTES],
            gr_info: [GrInfo::default(); 4],
            grbuf: [0.0; 1152],
            scf: [0.0; 40],
            syn: [0.0; 33 * 64],
            ist_pos: [[0; 39]; 2],
        }
    }
}

/// The state carried from frame to frame: minimp3's `mp3dec_t`.
pub(crate) struct State {
    mdct_overlap: [[f32; 288]; 2],
    /// minimp3's `qmf_state`: the synthesis filter's last 15 rows.
    qmf: [f32; 960],
    reserv: usize,
    free_format_bytes: usize,
    /// The last frame's header; its first byte 0 when the next frame is to
    /// be synced on afresh.
    pub header: [u8; 4],
    reserv_buf: [u8; MAX_BITRESERVOIR_BYTES],
}

/// How a frame's decoding went.
pub(crate) enum Outcome {
    /// Its samples are out.
    Decoded,
    /// No samples: the reservoir does not reach back far enough yet.
    NoReservoir,
    /// No samples, and the next frame is to be synced on afresh.
    Invalid,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self {
            mdct_overlap: [[0.0; 288]; 2],
            qmf: [0.0; 960],
            reserv: 0,
            free_format_bytes: 0,
            header: [0; 4],
            reserv_buf: [0; MAX_BITRESERVOIR_BYTES],
        }
    }

    /// Where the frame to decode is in `mp3`, and its size with padding:
    /// at the start if it continues the last frame's stream and the next
    /// frame's header agrees (or it ends the input), else found afresh --
    /// forgetting everything -- by `mp3d_find_frame`. `Err` with the bytes
    /// to skip where there is none.
    pub(crate) fn sync(&mut self, mp3: &[u8]) -> Result<(usize, usize), usize> {
        let n = mp3.len();
        let mut frame_size = 0usize;
        if n > 4
            && self.header[0] == 0xff
            && let Some(h) = Hdr::at(mp3)
            && Hdr(self.header).same_stream(h)
        {
            frame_size = h.frame_bytes(self.free_format_bytes) + h.padding();
            let next = mp3.get(frame_size..).and_then(Hdr::at);
            if frame_size != n
                && (frame_size + HDR_SIZE > n || !next.is_some_and(|next| h.same_stream(next)))
            {
                frame_size = 0;
            }
        }
        if frame_size != 0 {
            return Ok((0, frame_size));
        }
        *self = Self::new();
        let (i, frame_size) = find_frame(mp3, &mut self.free_format_bytes);
        if frame_size == 0 || i + frame_size > n {
            return Err(i);
        }
        Ok((i, frame_size))
    }

    /// One Layer III frame, `frame` its bytes after the header.
    pub(crate) fn decode_layer3(
        &mut self,
        s: &mut Scratch,
        hdr: Hdr,
        frame: &[u8],
        pcm: &mut [i16; MAX_SAMPLES_PER_FRAME],
    ) -> Outcome {
        let nch = if hdr.is_mono() { 1 } else { 2 };
        let mut bs_frame = Bits::new(frame, frame.len());
        if hdr.is_crc() {
            bs_frame.get(16);
        }
        let main_data_begin = l3::read_side_info(&mut bs_frame, &mut s.gr_info, hdr);
        let Some(main_data_begin) = main_data_begin.filter(|_| bs_frame.pos <= bs_frame.limit)
        else {
            return Outcome::Invalid;
        };
        let (success, bytes) = self.restore_reservoir(&bs_frame, &mut s.maindata, main_data_begin);
        let mut bs = Bits::new(&s.maindata, bytes);
        if success {
            let granules = if hdr.test_mpeg1() { 2 } else { 1 };
            for igr in 0..granules {
                s.grbuf.fill(0.0);
                let mut granule = l3::Granule {
                    gr_info: &s.gr_info[igr * nch..],
                    grbuf: &mut s.grbuf,
                    scf: &mut s.scf,
                    syn: &mut s.syn,
                    ist_pos: &mut s.ist_pos,
                };
                l3::decode(hdr, &mut self.mdct_overlap, &mut bs, &mut granule, nch);
                synth::synth_granule(
                    &mut self.qmf,
                    &mut s.grbuf,
                    18,
                    nch,
                    &mut pcm[igr * 576 * nch..],
                    &mut s.syn,
                );
            }
        }
        self.save_reservoir(&bs);
        if success {
            Outcome::Decoded
        } else {
            Outcome::NoReservoir
        }
    }

    /// One Layer I or II frame, `frame` its bytes after the header.
    pub(crate) fn decode_layer12(
        &mut self,
        s: &mut Scratch,
        hdr: Hdr,
        frame: &[u8],
        pcm: &mut [i16; MAX_SAMPLES_PER_FRAME],
    ) -> Outcome {
        let nch = if hdr.is_mono() { 1 } else { 2 };
        let mut bs_frame = Bits::new(frame, frame.len());
        if hdr.is_crc() {
            bs_frame.get(16);
        }
        let mut sci = l12::ScaleInfo::default();
        l12::read_scale_info(hdr, &mut bs_frame, &mut sci);
        s.grbuf.fill(0.0);
        // minimp3's `info->layer | 1`: Layer I's groups are of one sample,
        // Layer II's of three.
        let group_size = if hdr.is_layer_1() { 1 } else { 3 };
        let mut at = 0usize;
        let mut out = 0usize;
        for igr in 0..3 {
            at += l12::dequantize_granule(&mut s.grbuf, at, &mut bs_frame, &sci, group_size);
            if at == 12 {
                at = 0;
                l12::apply_scf_384(&sci, &sci.scf[igr..], &mut s.grbuf);
                synth::synth_granule(
                    &mut self.qmf,
                    &mut s.grbuf,
                    12,
                    nch,
                    &mut pcm[out..],
                    &mut s.syn,
                );
                s.grbuf.fill(0.0);
                out += 384 * nch;
            }
            if bs_frame.pos > bs_frame.limit {
                return Outcome::Invalid;
            }
        }
        Outcome::Decoded
    }

    /// `L3_restore_reservoir`: the main data -- the reservoir's last
    /// `main_data_begin` bytes, then the frame's after its side information
    /// -- into `maindata`; whether the reservoir held them all, and the
    /// bytes.
    fn restore_reservoir(
        &self,
        bs: &Bits<'_>,
        maindata: &mut [u8],
        main_data_begin: usize,
    ) -> (bool, usize) {
        let frame_bytes = (bs.limit - bs.pos) / 8;
        let bytes_have = self.reserv.min(main_data_begin);
        let from = self.reserv.saturating_sub(main_data_begin);
        maindata[..bytes_have].copy_from_slice(&self.reserv_buf[from..from + bytes_have]);
        let start = bs.pos / 8;
        maindata[bytes_have..bytes_have + frame_bytes]
            .copy_from_slice(&bs.buf[start..start + frame_bytes]);
        (self.reserv >= main_data_begin, bytes_have + frame_bytes)
    }

    /// `L3_save_reservoir`: what the frame's granules left unread of the
    /// main data, its last 511 bytes at most, kept for the frames after.
    fn save_reservoir(&mut self, bs: &Bits<'_>) {
        let mut pos = bs.pos.div_ceil(8);
        let mut remains = (bs.limit / 8).saturating_sub(pos);
        if remains > MAX_BITRESERVOIR_BYTES {
            pos += remains - MAX_BITRESERVOIR_BYTES;
            remains = MAX_BITRESERVOIR_BYTES;
        }
        if remains > 0 {
            self.reserv_buf[..remains].copy_from_slice(&bs.buf[pos..pos + remains]);
        }
        self.reserv = remains;
    }
}
