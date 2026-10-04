//! The uncompressed start of a frame: the three-byte frame tag every frame
//! begins with and, on a key frame, the start code and picture size after it.
//!
//! Everything after these bytes is bool-coded and read by `decodeframe`.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/vp8_dx_iface.c`
//! (`vp8_peek_si_internal`), `vp8/decoder/decodeframe.c` (the start of
//! `vp8_decode_frame`) and `vp8/common/alloccommon.c`
//! (`vp8_setup_version`), copyright the WebM project authors, used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

use crate::Error;

/// What a frame's first bytes say, before anything is decoded: libvpx's
/// `vpx_codec_stream_info_t` as `vp8_peek_si_internal` fills it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    /// Whether the frame is a key frame.
    pub is_kf: bool,
    /// Its size in pixels, if it declares one (key frames do; 0 otherwise).
    pub width: u32,
    pub height: u32,
}

/// The outcome of peeking at a frame, as libvpx's decoder uses it: the error
/// the peek returned, if any, and what it learned. A failed peek can still
/// have learned that the frame is a key frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Peek {
    pub(crate) result: Result<(), Error>,
    pub(crate) is_kf: bool,
    /// The size, when the frame is a key frame whose start code is right.
    pub(crate) size: Option<(u32, u32)>,
}

/// libvpx's `vp8_peek_si_internal` on non-empty `data`. A frame shorter
/// than ten bytes is never taken for a key frame, whatever its first bit
/// says.
pub(crate) fn peek(data: &[u8]) -> Peek {
    match data {
        [b0, _, _, s0, s1, s2, w0, w1, h0, h1, ..] if b0 & 1 == 0 => {
            if [*s0, *s1, *s2] != START_CODE {
                return Peek {
                    result: Err(Error::Unsupported("a key frame without VP8's start code")),
                    is_kf: true,
                    size: None,
                };
            }
            let w = u32::from(u16::from_le_bytes([*w0, *w1]) & 0x3fff);
            let h = u32::from(u16::from_le_bytes([*h0, *h1]) & 0x3fff);
            if w == 0 || h == 0 {
                Peek {
                    result: Err(Error::Corrupt("a key frame of no width or height")),
                    is_kf: true,
                    size: Some((0, 0)),
                }
            } else {
                Peek {
                    result: Ok(()),
                    is_kf: true,
                    size: Some((w, h)),
                }
            }
        }
        _ => Peek {
            result: Err(Error::Unsupported("not the start of a VP8 key frame")),
            is_kf: false,
            size: None,
        },
    }
}

/// Peek at a frame: whether it is a key frame and, if so, its size, as
/// libvpx's `vpx_codec_peek_stream_info` reports them. An inter frame is not
/// an error here; a key frame whose start code is wrong, or whose size is 0,
/// is.
///
/// # Errors
///
/// [`Error::Unsupported`] for a key frame without VP8's start code, and
/// [`Error::Corrupt`] for one of no width or height.
pub fn peek_stream_info(data: &[u8]) -> Result<StreamInfo, Error> {
    let p = peek(data);
    if !p.is_kf {
        return Ok(StreamInfo {
            is_kf: false,
            width: 0,
            height: 0,
        });
    }
    p.result?;
    let (width, height) = p.size.unwrap_or((0, 0));
    Ok(StreamInfo {
        is_kf: true,
        width,
        height,
    })
}

/// The bytes after a key frame's tag: `vp8_decode_frame`'s sync code.
pub(crate) const START_CODE: [u8; 3] = [0x9d, 0x01, 0x2a];

/// A frame's three-byte tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tag {
    pub(crate) key_frame: bool,
    /// 0 to 7; 4 and up are reserved and decode as 0 does.
    pub(crate) version: u8,
    pub(crate) show_frame: bool,
    /// The length of the first partition, which holds the frame header and
    /// every macroblock's modes.
    pub(crate) first_partition_len: u32,
}

impl Tag {
    /// Read a tag: the first three bytes of `data`, which has at least three.
    pub(crate) fn read(b: [u8; 3]) -> Self {
        let raw = u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16;
        Self {
            key_frame: b[0] & 1 == 0,
            version: (b[0] >> 1) & 7,
            show_frame: (b[0] >> 4) & 1 != 0,
            first_partition_len: raw >> 5,
        }
    }
}

/// How a version decodes: libvpx's `vp8_setup_version`, less what the frame
/// header overrides (the loop filter's type) and what nothing reads
/// (`no_lpf`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VersionSetup {
    /// Bilinear sub-pixel prediction instead of the six-tap filter.
    pub(crate) bilinear: bool,
    /// Chroma motion vectors are rounded to whole pixels.
    pub(crate) full_pixel: bool,
}

impl VersionSetup {
    pub(crate) fn of(version: u8) -> Self {
        match version {
            1 | 2 => Self {
                bilinear: true,
                full_pixel: false,
            },
            3 => Self {
                bilinear: true,
                full_pixel: true,
            },
            // 0, and 4 to 7, which are reserved for future use.
            _ => Self {
                bilinear: false,
                full_pixel: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, reason = "a test: a failure should be loud")]

    use super::*;

    fn key_frame(w: u16, h: u16) -> Vec<u8> {
        let mut f = vec![0x10, 0x02, 0x00]; // key, version 0, shown, partition 16
        f.extend_from_slice(&START_CODE);
        f.extend_from_slice(&w.to_le_bytes());
        f.extend_from_slice(&h.to_le_bytes());
        f.extend_from_slice(&[0; 8]);
        f
    }

    #[test]
    fn a_key_frame_tells_its_size_without_its_scaling_bits() {
        let f = key_frame(176 | 0x4000, 144 | 0xc000);
        assert_eq!(
            peek_stream_info(&f),
            Ok(StreamInfo {
                is_kf: true,
                width: 176,
                height: 144
            })
        );
    }

    #[test]
    fn an_inter_frame_peeks_as_not_a_key_frame() {
        let f = [0x31u8, 0x01, 0x00, 1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            peek_stream_info(&f),
            Ok(StreamInfo {
                is_kf: false,
                width: 0,
                height: 0
            })
        );
        let p = peek(&f);
        assert!(!p.is_kf);
        assert!(p.result.is_err(), "libvpx's peek refuses an inter frame");
    }

    #[test]
    fn a_short_key_frame_is_not_taken_for_one() {
        let f = key_frame(16, 16);
        assert!(!peek(&f[..9]).is_kf);
    }

    #[test]
    fn a_wrong_start_code_or_an_empty_size_is_refused() {
        let mut f = key_frame(16, 16);
        f[4] = 0;
        assert_eq!(
            peek_stream_info(&f),
            Err(Error::Unsupported("a key frame without VP8's start code"))
        );
        let f = key_frame(0, 16);
        assert_eq!(
            peek_stream_info(&f),
            Err(Error::Corrupt("a key frame of no width or height"))
        );
    }

    #[test]
    fn a_tag_reads_its_fields() {
        // Inter, version 3, hidden, first partition 0x12345.
        let raw: u32 = 1 | 3 << 1 | 0x12345 << 5;
        let b = raw.to_le_bytes();
        let t = Tag::read([b[0], b[1], b[2]]);
        assert_eq!(
            t,
            Tag {
                key_frame: false,
                version: 3,
                show_frame: false,
                first_partition_len: 0x12345
            }
        );
    }

    #[test]
    fn versions_choose_filters_as_libvpx_does() {
        assert!(!VersionSetup::of(0).bilinear);
        assert!(VersionSetup::of(1).bilinear && !VersionSetup::of(1).full_pixel);
        assert!(VersionSetup::of(2).bilinear && !VersionSetup::of(2).full_pixel);
        assert!(VersionSetup::of(3).bilinear && VersionSetup::of(3).full_pixel);
        for v in 4..8 {
            assert_eq!(VersionSetup::of(v), VersionSetup::of(0));
        }
    }
}
