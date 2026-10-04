//! Which colour a picture is converted with: what its own bitstream says;
//! where the bitstream is silent, what the file says of its track; and where
//! both are, a guess from the picture's size, as players make it.
//!
//! The colour is ITU-T H.273's three numbers -- the matrix that turns Y, U
//! and V into R, G and B, the primaries (which the conversion needs only for
//! the matrices derived from them, 12 and 13), and whether samples use the
//! full range or the studio range. The transfer function is not among them:
//! nothing in SlateOS manages colour or maps HDR down yet, so the conversion
//! takes no account of it (`known-issues/F-video-is-shown-without-colour-management.md`).
//!
//! **The guess** is mpv's (`mp_csp_guess_colorspace`,
//! `mp_csp_guess_primaries`), the same in VLC and Kodi: video of HD size
//! (1280 wide or more, or taller than 576) was made for BT.709, and smaller
//! video for BT.601 -- the 625-line primaries at a height of 576, the
//! 525-line ones at 480 or 488. Range unsaid is the studio range, as video
//! nearly always is. libavif's own fallback -- BT.601 for everything -- is
//! right for pictures and wrong for most HD video, which is encoded BT.709
//! and often not tagged (design-decisions §1346).

use crate::ColourHint;

/// H.273's "unspecified", for the matrix, primaries and transfer alike.
const UNSPECIFIED: u16 = 2;
/// H.273's matrix 3 and primaries 0 and 3, "reserved": read as unspecified.
const RESERVED: u16 = 3;
const MATRIX_BT709: u16 = 1;
const MATRIX_BT601: u16 = 6;
const PRIMARIES_BT709: u16 = 1;
const PRIMARIES_BT470BG: u16 = 5;
const PRIMARIES_SMPTE170M: u16 = 6;

/// A picture's colour, settled: what it is converted to pixels with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    /// H.273 `MatrixCoefficients`: 1 BT.709, 5 and 6 BT.601, 9 BT.2020, 0
    /// the identity (RGB stored as G, B, R), ...
    pub matrix: u16,
    /// H.273 `ColourPrimaries`.
    pub primaries: u16,
    /// Samples span every code rather than the studio range.
    pub full_range: bool,
}

impl ColourHint {
    /// What a VP8 key frame says, as FFmpeg reads it (`libavcodec/vp8.c`):
    /// its colour space bit 0 -- the only value the specification defines,
    /// YUV "similar to BT.601" -- is BT.601 (FFmpeg's `BT470BG` matrix, 5),
    /// and 1 says nothing; its clamping type bit 1 is the full range, and 0
    /// the studio range. libvpx reads both bits and goes by neither. FFmpeg's
    /// reading is what players built on it show, so a VP8 picture is BT.601
    /// whatever its size or its file says -- unlike untagged VP9, whose HD
    /// sizes are guessed BT.709.
    pub(crate) fn vp8(color_space: u8, clamping_type: u8) -> Self {
        Self {
            matrix: (color_space == 0).then_some(5),
            primaries: None,
            full_range: Some(clamping_type == 1),
        }
    }

    /// What a VP9 frame says: libvpx's `vpx_color_space_t` and its range
    /// bit. VP9 has no primaries, and an unknown or reserved space says
    /// nothing. The numbers are FFmpeg's (`libavcodec/vp9.c`): BT.601 is
    /// BT.470BG, and sRGB the identity matrix.
    pub(crate) fn vp9(color_space: u8, full_range: bool) -> Self {
        let matrix = match color_space {
            1 => Some(5),
            2 => Some(MATRIX_BT709),
            3 => Some(MATRIX_BT601),
            4 => Some(7),
            5 => Some(9),
            7 => Some(0),
            // 0, unknown, and 6, reserved.
            _ => None,
        };
        Self {
            matrix,
            primaries: None,
            full_range: Some(full_range),
        }
    }

    /// What an AV1 sequence header says.
    pub(crate) fn av1(colour: Option<rav1d::safe::Colour>) -> Self {
        let Some(c) = colour else {
            return Self::default();
        };
        Self {
            matrix: said(u16::from(c.matrix), true),
            primaries: said(u16::from(c.primaries), false),
            full_range: Some(c.full_range),
        }
    }

    /// What a Matroska track's `Colour` says.
    pub(crate) fn matroska(colour: Option<&matroska::Colour>) -> Self {
        let Some(c) = colour else {
            return Self::default();
        };
        Self {
            matrix: u16::try_from(c.matrix_coefficients)
                .ok()
                .and_then(|m| said(m, true)),
            primaries: u16::try_from(c.primaries).ok().and_then(|p| said(p, false)),
            // 0 unspecified, 1 broadcast, 2 full; 3 "defined by the matrix
            // and transfer", which says nothing about a range by itself.
            full_range: match c.range {
                1 => Some(false),
                2 => Some(true),
                _ => None,
            },
        }
    }
}

/// `value` if it says something: not unspecified, not reserved, and an H.273
/// code point at all (under 256).
fn said(value: u16, matrix: bool) -> Option<u16> {
    let reserved = value == RESERVED || (!matrix && value == 0);
    (value != UNSPECIFIED && !reserved && value < 256).then_some(value)
}

/// Whether a picture this size was made for HD television: mpv's test.
const fn is_hd(width: u32, height: u32) -> bool {
    width >= 1280 || height > 576
}

/// The colour of a `width` x `height` picture whose bitstream says
/// `bitstream` and whose track's description says `container`: each of the
/// three from the first that says it, else the guess.
pub(crate) fn resolve(
    bitstream: ColourHint,
    container: ColourHint,
    width: u32,
    height: u32,
) -> Colour {
    let hd = is_hd(width, height);
    let matrix = bitstream.matrix.or(container.matrix).unwrap_or(if hd {
        MATRIX_BT709
    } else {
        MATRIX_BT601
    });
    let primaries = bitstream
        .primaries
        .or(container.primaries)
        .unwrap_or(match height {
            _ if hd => PRIMARIES_BT709,
            576 => PRIMARIES_BT470BG,
            480 | 488 => PRIMARIES_SMPTE170M,
            _ => PRIMARIES_BT709,
        });
    Colour {
        matrix,
        primaries,
        full_range: bitstream
            .full_range
            .or(container.full_range)
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTHING: ColourHint = ColourHint {
        matrix: None,
        primaries: None,
        full_range: None,
    };

    #[test]
    fn unsaid_colour_is_guessed_from_the_size() {
        let sd = resolve(NOTHING, NOTHING, 720, 576);
        assert_eq!((sd.matrix, sd.primaries, sd.full_range), (6, 5, false));
        let ntsc = resolve(NOTHING, NOTHING, 720, 480);
        assert_eq!((ntsc.matrix, ntsc.primaries), (6, 6));
        let small = resolve(NOTHING, NOTHING, 320, 240);
        assert_eq!((small.matrix, small.primaries), (6, 1));
        // 1280 wide is HD however short; 577 tall is HD however narrow.
        for (w, h) in [(1280, 96), (720, 577), (1920, 1080)] {
            let hd = resolve(NOTHING, NOTHING, w, h);
            assert_eq!((hd.matrix, hd.primaries), (1, 1), "{w}x{h}");
        }
        assert_eq!(resolve(NOTHING, NOTHING, 1279, 576).matrix, 6);
    }

    #[test]
    fn the_bitstream_wins_then_the_file() {
        let file = ColourHint {
            matrix: Some(9),
            primaries: Some(9),
            full_range: Some(true),
        };
        // The file fills in what the bitstream leaves unsaid...
        let vp9_unknown = ColourHint::vp9(0, false);
        let c = resolve(vp9_unknown, file, 320, 240);
        assert_eq!((c.matrix, c.primaries, c.full_range), (9, 9, false));
        // ...and the bitstream's own word stands over the file's.
        let vp9_709 = ColourHint::vp9(2, true);
        let c = resolve(vp9_709, file, 320, 240);
        assert_eq!((c.matrix, c.primaries, c.full_range), (1, 9, true));
        // A tag stands over the size's guess.
        assert_eq!(
            resolve(ColourHint::vp9(3, false), NOTHING, 1920, 1080).matrix,
            6
        );
    }

    #[test]
    fn vp8_is_bt601_at_any_size_and_says_its_range() {
        // HD VP8 stays BT.601, where HD VP9 that says nothing is guessed
        // BT.709; the file's matrix gives way to the bitstream's word.
        let file = ColourHint {
            matrix: Some(1),
            primaries: Some(1),
            full_range: Some(true),
        };
        let c = resolve(ColourHint::vp8(0, 0), file, 1920, 1080);
        assert_eq!((c.matrix, c.primaries, c.full_range), (5, 1, false));
        let c = resolve(ColourHint::vp8(0, 1), NOTHING, 176, 144);
        assert_eq!((c.matrix, c.full_range), (5, true));
        // The reserved colour space says nothing: the file's word, or the
        // size's guess.
        assert_eq!(resolve(ColourHint::vp8(1, 0), file, 176, 144).matrix, 1);
        assert_eq!(resolve(ColourHint::vp8(1, 0), NOTHING, 176, 144).matrix, 6);
    }

    #[test]
    fn vp9_colour_spaces_are_ffmpegs_numbers() {
        let matrix = |cs| ColourHint::vp9(cs, false).matrix;
        assert_eq!(
            [0, 1, 2, 3, 4, 5, 6, 7].map(matrix),
            [
                None,
                Some(5),
                Some(1),
                Some(6),
                Some(7),
                Some(9),
                None,
                Some(0)
            ]
        );
    }

    #[test]
    fn unspecified_and_reserved_say_nothing() {
        let file = |m, p, r| {
            ColourHint::matroska(Some(&matroska::Colour {
                matrix_coefficients: m,
                range: r,
                primaries: p,
            }))
        };
        assert_eq!(file(2, 2, 0), NOTHING);
        assert_eq!(file(3, 3, 3), NOTHING);
        assert_eq!(file(256, 0, 0), NOTHING);
        assert_eq!(
            file(0, 1, 2),
            ColourHint {
                matrix: Some(0),
                primaries: Some(1),
                full_range: Some(true),
            }
        );
        assert_eq!(file(1, 1, 1).full_range, Some(false));
        assert_eq!(ColourHint::matroska(None), NOTHING);
        assert_eq!(ColourHint::av1(None), NOTHING);
        let av1 = ColourHint::av1(Some(rav1d::safe::Colour {
            primaries: 2,
            transfer: 2,
            matrix: 2,
            full_range: false,
            chroma_sample_position: 0,
        }));
        assert_eq!(
            av1,
            ColourHint {
                full_range: Some(false),
                ..NOTHING
            }
        );
    }
}
