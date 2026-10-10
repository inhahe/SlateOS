//! A frame's four-byte header, the bit reader, and the search for a frame
//! in a byte stream -- minimp3's `hdr_*` functions, `get_bits`,
//! `mp3d_match_frame` and `mp3d_find_frame`, translated into Rust (minimp3,
//! CC0: `licenses/minimp3-LICENSE`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "minimp3's integer arithmetic on header fields of a few bits and frame sizes below 2304 bytes, translated as it is"
)]

use crate::tables::HALFRATE;

pub(crate) const HDR_SIZE: usize = 4;
pub(crate) const MAX_FREE_FORMAT_FRAME_SIZE: usize = 2304;
const MAX_FRAME_SYNC_MATCHES: usize = 10;

/// A header's fields (minimp3's `HDR_*` macros), over its four bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hdr(pub [u8; 4]);

impl Hdr {
    pub(crate) fn at(bytes: &[u8]) -> Option<Self> {
        Some(Self(bytes.get(..4)?.try_into().ok()?))
    }

    pub(crate) fn is_mono(self) -> bool {
        self.0[3] & 0xC0 == 0xC0
    }
    pub(crate) fn is_ms_stereo(self) -> bool {
        self.0[3] & 0xE0 == 0x60
    }
    pub(crate) fn is_free_format(self) -> bool {
        self.0[2] & 0xF0 == 0
    }
    pub(crate) fn is_crc(self) -> bool {
        self.0[1] & 1 == 0
    }
    pub(crate) fn test_padding(self) -> bool {
        self.0[2] & 0x2 != 0
    }
    pub(crate) fn test_mpeg1(self) -> bool {
        self.0[1] & 0x8 != 0
    }
    pub(crate) fn test_not_mpeg25(self) -> bool {
        self.0[1] & 0x10 != 0
    }
    /// Intensity stereo: joint stereo with the mode extension's intensity
    /// bit. **Differs from minimp3**, whose `HDR_TEST_I_STEREO` tests the
    /// bit in any mode -- in mono scaling the one channel by memory nothing
    /// wrote, in stereo or dual channel mixing one channel into the other.
    /// The standard gives the mode extension meaning only in joint stereo,
    /// mpg123 reads it so, FFmpeg processes stereo so, and minimp3's own
    /// mid/side test ([`Hdr::is_ms_stereo`]) already looks there.
    pub(crate) fn is_i_stereo(self) -> bool {
        self.0[3] & 0xD0 == 0x50
    }
    pub(crate) fn test_ms_stereo(self) -> bool {
        self.0[3] & 0x20 != 0
    }
    pub(crate) fn stereo_mode(self) -> u8 {
        (self.0[3] >> 6) & 3
    }
    pub(crate) fn stereo_mode_ext(self) -> u8 {
        (self.0[3] >> 4) & 3
    }
    pub(crate) fn layer(self) -> u8 {
        (self.0[1] >> 1) & 3
    }
    pub(crate) fn bitrate(self) -> u8 {
        self.0[2] >> 4
    }
    pub(crate) fn sample_rate(self) -> u8 {
        (self.0[2] >> 2) & 3
    }
    /// `HDR_GET_MY_SAMPLE_RATE`: 0 to 8, MPEG-1's three rates, then
    /// MPEG-2's, then MPEG-2.5's.
    pub(crate) fn my_sample_rate(self) -> u32 {
        u32::from(self.sample_rate())
            + (u32::from((self.0[1] >> 3) & 1) + u32::from((self.0[1] >> 4) & 1)) * 3
    }
    pub(crate) fn is_frame_576(self) -> bool {
        self.0[1] & 0b1110 == 0b0010
    }
    pub(crate) fn is_layer_1(self) -> bool {
        self.0[1] & 0b0110 == 0b0110
    }

    /// `hdr_valid`.
    pub(crate) fn valid(self) -> bool {
        self.0[0] == 0xff
            && (self.0[1] & 0xF0 == 0xf0 || self.0[1] & 0xFE == 0xe2)
            && self.layer() != 0
            && self.bitrate() != 15
            && self.sample_rate() != 3
    }

    /// `hdr_compare`: `other` is a valid header of the same stream.
    pub(crate) fn same_stream(self, other: Self) -> bool {
        other.valid()
            && (self.0[1] ^ other.0[1]) & 0xFE == 0
            && (self.0[2] ^ other.0[2]) & 0x0C == 0
            && self.is_free_format() == other.is_free_format()
    }

    /// `hdr_bitrate_kbps`. Like the rest, meaningful for a [valid](Self::valid)
    /// header only, which is all that is asked; 0 for another.
    pub(crate) fn bitrate_kbps(self) -> u32 {
        let row =
            usize::from(self.test_mpeg1()) * 45 + usize::from(self.layer().wrapping_sub(1)) * 15;
        2 * u32::from(
            HALFRATE
                .get(row + usize::from(self.bitrate()))
                .copied()
                .unwrap_or(0),
        )
    }

    /// `hdr_sample_rate_hz`; 44100 for an invalid header's rate, never 0
    /// (a frame's size is divided by it).
    pub(crate) fn sample_rate_hz(self) -> u32 {
        const HZ: [u32; 3] = [44100, 48000, 32000];
        HZ.get(usize::from(self.sample_rate()))
            .copied()
            .unwrap_or(44100)
            >> u32::from(!self.test_mpeg1())
            >> u32::from(!self.test_not_mpeg25())
    }

    /// `hdr_frame_samples`.
    pub(crate) fn frame_samples(self) -> u32 {
        if self.is_layer_1() {
            384
        } else {
            1152 >> u32::from(self.is_frame_576())
        }
    }

    /// `hdr_frame_bytes`: a free-format frame's size given.
    pub(crate) fn frame_bytes(self, free_format_size: usize) -> usize {
        let mut bytes =
            (self.frame_samples() * self.bitrate_kbps() * 125 / self.sample_rate_hz()) as usize;
        if self.is_layer_1() {
            bytes &= !3;
        }
        if bytes == 0 { free_format_size } else { bytes }
    }

    /// `hdr_padding`.
    pub(crate) fn padding(self) -> usize {
        if self.test_padding() {
            if self.is_layer_1() { 4 } else { 1 }
        } else {
            0
        }
    }
}

/// minimp3's `bs_t`: a bit reader whose reads past its limit give 0, and
/// whose position goes on past it, for the caller to test.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bits<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
    pub limit: usize,
}

impl<'a> Bits<'a> {
    pub(crate) fn new(buf: &'a [u8], bytes: usize) -> Self {
        Self {
            buf,
            pos: 0,
            limit: bytes * 8,
        }
    }

    /// `get_bits`: `n` bits (up to 32), most significant first; 0 where the
    /// read passes the limit (the position moving on regardless).
    pub(crate) fn get(&mut self, n: u32) -> u32 {
        let s = (self.pos & 7) as u32;
        let mut shl = n as i32 + s as i32;
        let mut p = self.pos >> 3;
        self.pos += n as usize;
        if self.pos > self.limit {
            return 0;
        }
        let byte = |i: usize| -> u32 { u32::from(self.buf.get(i).copied().unwrap_or(0)) };
        let mut next = byte(p) & (255 >> s);
        p += 1;
        let mut cache = 0u32;
        loop {
            shl -= 8;
            if shl <= 0 {
                break;
            }
            cache |= next << shl;
            next = byte(p);
            p += 1;
        }
        cache | (next >> (-shl))
    }
}

/// `mp3d_match_frame`: the frames after the one at the start of `data` are
/// of its stream, as far as can be seen (up to ten).
fn match_frame(data: &[u8], frame_bytes: usize) -> bool {
    let mut i = 0usize;
    for nmatch in 0..MAX_FRAME_SYNC_MATCHES {
        let Some(h) = data.get(i..).and_then(Hdr::at) else {
            return nmatch > 0;
        };
        i += h.frame_bytes(frame_bytes) + h.padding();
        if i + HDR_SIZE > data.len() {
            return nmatch > 0;
        }
        let (Some(first), Some(next)) = (Hdr::at(data), data.get(i..).and_then(Hdr::at)) else {
            return false;
        };
        if !first.same_stream(next) {
            return false;
        }
    }
    true
}

/// `mp3d_find_frame`: where the first frame in `data` starts, and its size
/// with its padding (0 where none is found); a free-format stream's frame
/// size learned into `free_format_bytes`.
pub(crate) fn find_frame(data: &[u8], free_format_bytes: &mut usize) -> (usize, usize) {
    let n = data.len();
    let mut i = 0usize;
    while i + HDR_SIZE < n {
        let Some(mp3) = data.get(i..) else { break };
        let Some(h) = Hdr::at(mp3) else { break };
        if h.valid() {
            let mut frame_bytes = h.frame_bytes(*free_format_bytes);
            let mut frame_and_padding = frame_bytes + h.padding();
            let mut k = HDR_SIZE;
            while frame_bytes == 0 && k < MAX_FREE_FORMAT_FRAME_SIZE && i + 2 * k < n - HDR_SIZE {
                if let Some(hk) = mp3.get(k..).and_then(Hdr::at)
                    && h.same_stream(hk)
                {
                    let fb = k - h.padding();
                    let nextfb = fb + hk.padding();
                    let far = mp3.get(k + nextfb..).and_then(Hdr::at);
                    if i + k + nextfb + HDR_SIZE <= n && far.is_some_and(|f| h.same_stream(f)) {
                        frame_and_padding = k;
                        frame_bytes = fb;
                        *free_format_bytes = fb;
                    }
                }
                k += 1;
            }
            if (frame_bytes != 0 && i + frame_and_padding <= n && match_frame(mp3, frame_bytes))
                || (i == 0 && frame_and_padding == n)
            {
                return (i, frame_and_padding);
            }
            *free_format_bytes = 0;
        }
        i += 1;
    }
    (n, 0)
}
