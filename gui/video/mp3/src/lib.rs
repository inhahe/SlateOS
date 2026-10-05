//! MPEG-1, -2 and -2.5 audio, Layers I, II and III ("MP3"): minimp3's
//! decoder translated into Rust, held to minimp3 built without SIMD
//! (`MINIMP3_NO_SIMD`) sample for sample (minimp3, by lieff, CC0:
//! `licenses/minimp3-LICENSE`).
//!
//! [`Reader`] takes an `.mp3`, `.mp2` or `.mp1` file apart into packets as
//! FFmpeg does -- its demuxer and MPEG audio parser, ID3v2 tags, the
//! Xing/Info/VBRI frame and the LAME tag's gapless delay and padding, junk
//! between frames -- and times and trims them as FFmpeg does, held to
//! ffprobe's packets (`tests/reader.rs`); see the `reader` module. Each
//! packet's frame is then the decoder's to decode.
//!
//! [`Decoder::decode_frame`] is `mp3dec_decode_frame`: it finds the next
//! frame in the bytes it is given -- syncing as minimp3 does, on a header
//! that the following frames' headers agree with -- decodes it into 16-bit
//! interleaved samples, and says how many bytes it took. A Layer III frame
//! may need the bit reservoir (the end of the frames before it): until the
//! decoder has seen enough of them, a frame decodes to no samples, as in
//! minimp3.
//!
//! **Where it differs from minimp3** (`tools/patch_minimp3.py` applies the
//! same two changes to the reference the tests are held to):
//!
//! - The mode extension's intensity-stereo bit counts only in joint stereo,
//!   as the standard has it and as mpg123 reads it (FFmpeg's stereo
//!   processing too); minimp3 applies intensity stereo whatever the mode,
//!   while gating its mid/side bit on joint stereo. A mono frame with the
//!   bit set would have minimp3 scale its one channel by memory nothing
//!   wrote; a dual-channel one, mix a channel into the other.
//! - A Layer III frame's private bits do not reach the first granule's
//!   scale factor selection, where minimp3's shifting leaves them and a set
//!   bit copies scale factors from memory nothing wrote.
//!
//! Where a damaged frame's Huffman codes run past the end of the main data,
//! the decoder reads zeros, where minimp3 reads the fields after its buffer;
//! and the decoder's scratch memory persists from frame to frame, where
//! minimp3's is whatever its stack held. The rest is minimp3's, its quirks
//! included: the CRC is skipped, not checked; and a sample between -1.5 and
//! -0.5 comes out as 0, not -1.

#![forbid(unsafe_code)]

mod frame;
mod header;
mod l12;
mod l3;
mod reader;
mod synth;
mod tables;

use frame::{Outcome, Scratch, State};
use header::{HDR_SIZE, Hdr};
pub use reader::{
    DECODER_DELAY, Header, Info, Packet, Reader, TICKS_PER_SECOND, is_mpeg_audio, probe,
};

/// The most samples a frame decodes to: 1152 a channel, two channels.
pub const MAX_SAMPLES_PER_FRAME: usize = 1152 * 2;

/// What [`Decoder::decode_frame`] found: minimp3's `mp3dec_frame_info_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameInfo {
    /// The bytes taken from the input: any passed over before the frame,
    /// and the frame. Where no frame was found, the whole input -- minimp3
    /// keeps nothing back for a later call, so a frame cut off at the end
    /// of the input is passed over with the rest, and the input should hold
    /// whole frames -- and so 0 only for no input.
    pub frame_bytes: usize,
    /// Where in the input the frame began.
    pub frame_offset: usize,
    /// 1 or 2; 0 where no frame was found.
    pub channels: usize,
    /// The sample rate, 8000 to 48000.
    pub hz: u32,
    /// 1, 2 or 3.
    pub layer: u32,
    /// The bit rate in kbit/s; 0 for a free-format stream.
    pub bitrate_kbps: u32,
}

/// An MPEG audio decoder: minimp3's `mp3dec_t` and `mp3dec_decode_frame`.
pub struct Decoder {
    state: Box<State>,
    scratch: Box<Scratch>,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// A decoder that will sync on the first frame it is given
    /// (`mp3dec_init`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Box::new(State::new()),
            scratch: Box::new(Scratch::new()),
        }
    }

    /// Forgets the stream: the next frame is synced on afresh and decoded
    /// with no history -- no reservoir, no overlap (`mp3dec_init`). For a
    /// seek.
    pub fn reset(&mut self) {
        self.state.header[0] = 0;
    }

    /// Decodes the frame at or after the start of `mp3`
    /// (`mp3dec_decode_frame`): its samples into `pcm`, interleaved, and
    /// how many there are a channel -- 0 where there is no frame, where the
    /// reservoir does not reach back far enough yet, or where the frame
    /// cannot be decoded. With `pcm` `None`, only finds the frame and says
    /// how many samples it would give.
    ///
    /// `mp3` should hold the next several frames where it can: a frame is
    /// trusted when the headers of the frames after it agree with its own.
    pub fn decode_frame(
        &mut self,
        mp3: &[u8],
        pcm: Option<&mut [i16; MAX_SAMPLES_PER_FRAME]>,
    ) -> (usize, FrameInfo) {
        let mut info = FrameInfo::default();
        let st = &mut *self.state;
        let (i, frame_size) = match st.sync(mp3) {
            Ok(found) => found,
            Err(skip) => {
                info.frame_bytes = skip;
                return (0, info);
            }
        };
        let Some(hdr) = mp3.get(i..).and_then(Hdr::at) else {
            info.frame_bytes = i;
            return (0, info);
        };
        st.header = hdr.0;
        info.frame_bytes = i.saturating_add(frame_size);
        info.frame_offset = i;
        info.channels = if hdr.is_mono() { 1 } else { 2 };
        info.hz = hdr.sample_rate_hz();
        info.layer = 4u32.saturating_sub(u32::from(hdr.layer()));
        info.bitrate_kbps = hdr.bitrate_kbps();

        let samples = hdr.frame_samples() as usize;
        let Some(pcm) = pcm else {
            return (samples, info);
        };
        let frame = mp3
            .get(i.saturating_add(HDR_SIZE)..info.frame_bytes)
            .unwrap_or_default();
        let outcome = if info.layer == 3 {
            st.decode_layer3(&mut self.scratch, hdr, frame, pcm)
        } else {
            st.decode_layer12(&mut self.scratch, hdr, frame, pcm)
        };
        match outcome {
            Outcome::Decoded => (samples, info),
            Outcome::NoReservoir => (0, info),
            Outcome::Invalid => {
                st.header[0] = 0;
                (0, info)
            }
        }
    }
}
