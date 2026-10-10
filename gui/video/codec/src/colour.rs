//! Which colour a picture is converted with: what its own bitstream says;
//! where the bitstream is silent, what the file says of its track; and where
//! both are, a guess from the picture's size, as players make it.
//!
//! The colour is ITU-T H.273's numbers -- the matrix that turns Y, U and V
//! into R, G and B, the primaries, the transfer function (what light each
//! sample stands for: BT.709's, or HDR's PQ and HLG), and whether samples use
//! the full range or the studio range -- and with it the HDR metadata, the
//! [`Light`] a stream says it holds: its own, kind by kind, else its file's,
//! as FFmpeg gives it with each frame.
//!
//! **The guess** is mpv's (`mp_image_params_guess_csp`, with
//! `mp_csp_guess_colorspace` and `mp_csp_guess_primaries`), the same in VLC
//! and Kodi: video of HD size (1280 wide or more, or taller than 576) was
//! made for BT.709, and smaller video for BT.601. Primaries unsaid are the
//! matrix's -- BT.2020's for BT.2020's matrix, BT.709's for BT.709's -- and
//! for BT.601's the size's: the 625-line primaries at a height of 576, the
//! 525-line ones at 480 or 486, else BT.709's. Range unsaid is the studio
//! range, as video nearly always is, and a transfer unsaid is BT.709's --
//! ordinary video's, as mpv takes it. libavif's own fallback -- BT.601 for
//! everything -- is right for pictures and wrong for most HD video, which is
//! encoded BT.709 and often not tagged (design-decisions §1346).
//!
//! **Except for HDR**, which is shown as Chrome shows it (design-decisions
//! §1378), and so guessed as Chrome guesses it
//! (`VideoColorSpace::GuessGfxColorSpace`): a transfer of BT.2020's -- PQ,
//! HLG, or its two ordinary ones -- makes an unsaid matrix and primaries
//! BT.2020's, unless the matrix or the primaries say BT.709's, which Chrome
//! ranks above it and then takes for both.

use rav1d::safe as av1;

use crate::{Chromaticities, ColourHint, ContentLightLevel, Light, Luminance, MasteringDisplay};

/// H.273's "unspecified", for the matrix, primaries and transfer alike.
const UNSPECIFIED: u16 = 2;
/// H.273's matrix 3 and primaries 0 and 3, "reserved": read as unspecified.
const RESERVED: u16 = 3;
const MATRIX_BT709: u16 = 1;
const MATRIX_BT601: u16 = 6;
const MATRIX_BT2020_NCL: u16 = 9;
const MATRIX_BT2020_CL: u16 = 10;
const PRIMARIES_BT709: u16 = 1;
const PRIMARIES_BT470BG: u16 = 5;
const PRIMARIES_SMPTE170M: u16 = 6;
const PRIMARIES_BT2020: u16 = 9;
const TRANSFER_BT709: u16 = 1;
/// BT.2020's transfers: its ordinary ones at 10 and 12 bits, PQ and HLG --
/// those `GuessGfxColorSpace` guesses BT.2020 from.
const TRANSFERS_BT2020: [u16; 4] = [14, 15, 16, 18];

/// A picture's colour, settled: what it is converted to pixels with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    /// H.273 `MatrixCoefficients`: 1 BT.709, 5 and 6 BT.601, 9 BT.2020, 0
    /// the identity (RGB stored as G, B, R), ...
    pub matrix: u16,
    /// H.273 `ColourPrimaries`: 1 BT.709, 9 BT.2020, ...
    pub primaries: u16,
    /// H.273 `TransferCharacteristics`: 1 BT.709 (and 6 and 14, the same
    /// curve), 16 PQ (HDR10), 18 HLG, ...
    pub transfer: u16,
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
            full_range: Some(clamping_type == 1),
            ..Self::default()
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
            full_range: Some(full_range),
            ..Self::default()
        }
    }

    /// What an AV1 picture says: its sequence header's colour, and the HDR
    /// metadata OBUs in force for it.
    pub(crate) fn av1(picture: &av1::Picture) -> Self {
        let light = Light {
            mastering: picture.mastering_display().map(|m| {
                // As FFmpeg's libdav1d wrapper scales them: 0.16
                // chromaticities, a 24.8 peak and an 18.14 black.
                let xy = |p: [u16; 2]| p.map(|v| f64::from(v) / 65536.0);
                let [red, green, blue] = m.primaries.map(xy);
                MasteringDisplay {
                    chromaticities: Some(Chromaticities {
                        red,
                        green,
                        blue,
                        white: xy(m.white_point),
                    }),
                    luminance: Some(Luminance {
                        max: f64::from(m.max_luminance) / 256.0,
                        min: f64::from(m.min_luminance) / 16384.0,
                    }),
                }
            }),
            content: picture.content_light().map(|c| ContentLightLevel {
                max_cll: u32::from(c.max_content_light_level),
                max_fall: u32::from(c.max_frame_average_light_level),
            }),
        };
        let Some(c) = picture.colour() else {
            return Self {
                light,
                ..Self::default()
            };
        };
        Self {
            matrix: said(u16::from(c.matrix), true),
            primaries: said(u16::from(c.primaries), false),
            transfer: said(u16::from(c.transfer), false),
            full_range: Some(c.full_range),
            light,
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
            transfer: u16::try_from(c.transfer_characteristics)
                .ok()
                .and_then(|t| said(t, false)),
            // 0 unspecified, 1 broadcast, 2 full; 3 "defined by the matrix
            // and transfer", which says nothing about a range by itself.
            full_range: match c.range {
                1 => Some(false),
                2 => Some(true),
                _ => None,
            },
            light: Light {
                mastering: c.mastering.map(|m| MasteringDisplay {
                    chromaticities: m.chromaticities.map(|p| Chromaticities {
                        red: p.red,
                        green: p.green,
                        blue: p.blue,
                        white: p.white,
                    }),
                    luminance: m.luminance.map(|l| Luminance {
                        max: l.max,
                        min: l.min,
                    }),
                }),
                content: c.content_light.map(|l| ContentLightLevel {
                    max_cll: as_c_unsigned(l.max_cll),
                    max_fall: as_c_unsigned(l.max_fall),
                }),
            },
        }
    }

    /// What an MP4 track's `colr` and `vpcC` say, as `gui/video/mp4` keeps
    /// it -- FFmpeg's numbers, a code point it has no name for already made
    /// unspecified -- and its `mdcv` or `SmDm`, and `clli` or `CoLL`, each
    /// number over its box's scale, as FFmpeg's rationals are.
    pub(crate) fn mp4(video: &mp4::Video) -> Self {
        let light = Light {
            mastering: video.mastering.map(|m| {
                let over = |v: u32, scale: u32| f64::from(v) / f64::from(scale);
                let [rx, ry, gx, gy, bx, by, wx, wy] = m
                    .chromaticities
                    .map(|v| over(u32::from(v), m.chromaticity_scale));
                MasteringDisplay {
                    chromaticities: Some(Chromaticities {
                        red: [rx, ry],
                        green: [gx, gy],
                        blue: [bx, by],
                        white: [wx, wy],
                    }),
                    luminance: Some(Luminance {
                        max: over(m.max_luminance, m.max_luminance_scale),
                        min: over(m.min_luminance, m.min_luminance_scale),
                    }),
                }
            }),
            content: video.content_light.map(|l| ContentLightLevel {
                max_cll: u32::from(l.max_cll),
                max_fall: u32::from(l.max_fall),
            }),
        };
        let Some(c) = video.colour else {
            return Self {
                light,
                ..Self::default()
            };
        };
        Self {
            matrix: said(c.matrix, true),
            primaries: said(c.primaries, false),
            transfer: said(c.transfer, false),
            full_range: c.full_range,
            light,
        }
    }
}

/// A Matroska number as FFmpeg's `unsigned` field holds it: its low 32
/// bits, as C's assignment keeps them. (MaxCLL and MaxFALL are cd/m2, of
/// which no display gives four billion; a file saying so is wrong either
/// way, and is read as FFmpeg reads it.)
fn as_c_unsigned(v: u64) -> u32 {
    u32::try_from(v & u64::from(u32::MAX)).unwrap_or(u32::MAX)
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
/// four from the first that says it, else the guess (the module's).
pub(crate) fn resolve(
    bitstream: ColourHint,
    container: ColourHint,
    width: u32,
    height: u32,
) -> Colour {
    let said_matrix = bitstream.matrix.or(container.matrix);
    let said_primaries = bitstream.primaries.or(container.primaries);
    let said_transfer = bitstream.transfer.or(container.transfer);
    let (matrix, primaries) = if said_transfer.is_some_and(|t| TRANSFERS_BT2020.contains(&t)) {
        // Chrome's guess: BT.2020's, or BT.709's where either says it.
        let bt709 = said_matrix == Some(MATRIX_BT709) || said_primaries == Some(PRIMARIES_BT709);
        let (m, p) = if bt709 {
            (MATRIX_BT709, PRIMARIES_BT709)
        } else {
            (MATRIX_BT2020_NCL, PRIMARIES_BT2020)
        };
        (said_matrix.unwrap_or(m), said_primaries.unwrap_or(p))
    } else {
        // mpv's: the matrix by the size, the primaries by the matrix.
        let hd = is_hd(width, height);
        let matrix = said_matrix.unwrap_or(if hd { MATRIX_BT709 } else { MATRIX_BT601 });
        let primaries = said_primaries.unwrap_or(match matrix {
            MATRIX_BT2020_NCL | MATRIX_BT2020_CL => PRIMARIES_BT2020,
            MATRIX_BT709 => PRIMARIES_BT709,
            _ => match height {
                _ if hd => PRIMARIES_BT709,
                576 => PRIMARIES_BT470BG,
                480 | 486 => PRIMARIES_SMPTE170M,
                _ => PRIMARIES_BT709,
            },
        });
        (matrix, primaries)
    };
    Colour {
        matrix,
        primaries,
        transfer: said_transfer.unwrap_or(TRANSFER_BT709),
        full_range: bitstream
            .full_range
            .or(container.full_range)
            .unwrap_or(false),
    }
}

/// The light of a picture whose bitstream says `bitstream` and whose file
/// says `container`: each kind the bitstream's where it says it, else the
/// file's -- as FFmpeg gives a frame its stream's side data only of the
/// kinds the frame has none of.
pub(crate) fn light(bitstream: Light, container: Light) -> Light {
    Light {
        mastering: bitstream.mastering.or(container.mastering),
        content: bitstream.content.or(container.content),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]

    use super::*;

    const NOTHING: ColourHint = ColourHint {
        matrix: None,
        primaries: None,
        transfer: None,
        full_range: None,
        light: Light {
            mastering: None,
            content: None,
        },
    };

    /// A Matroska `Colour` of these numbers and no light.
    fn mkv(matrix: u64, primaries: u64, transfer: u64, range: u64) -> matroska::Colour {
        matroska::Colour {
            matrix_coefficients: matrix,
            range,
            primaries,
            transfer_characteristics: transfer,
            content_light: None,
            mastering: None,
        }
    }

    /// An MP4 track's picture of this colour and light.
    fn mp4_video(
        colour: Option<mp4::Colour>,
        mastering: Option<mp4::Mastering>,
        content_light: Option<mp4::ContentLight>,
    ) -> mp4::Video {
        mp4::Video {
            width: 64,
            height: 48,
            track_width: 64,
            track_height: 48,
            pixel_aspect: None,
            colour,
            mastering,
            content_light,
            matrix: None,
            crop: [0; 4],
            frame_duration: None,
        }
    }

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
        // NTSC's professional height is mpv's 486, not 488.
        assert_eq!(resolve(NOTHING, NOTHING, 720, 486).primaries, 6);
        assert_eq!(resolve(NOTHING, NOTHING, 720, 488).primaries, 1);
    }

    /// mpv takes unsaid primaries from the matrix before the size:
    /// BT.2020's and BT.709's matrices name their primaries, BT.601's
    /// leaves them to the size.
    #[test]
    fn unsaid_primaries_are_the_matrix_s() {
        let matrix = |m| ColourHint {
            matrix: Some(m),
            ..NOTHING
        };
        assert_eq!(resolve(matrix(9), NOTHING, 1920, 1080).primaries, 9);
        assert_eq!(resolve(matrix(10), NOTHING, 720, 576).primaries, 9);
        assert_eq!(resolve(matrix(1), NOTHING, 720, 576).primaries, 1);
        assert_eq!(resolve(matrix(5), NOTHING, 720, 576).primaries, 5);
        assert_eq!(resolve(matrix(6), NOTHING, 720, 480).primaries, 6);
        assert_eq!(resolve(matrix(6), NOTHING, 1920, 1080).primaries, 1);
        // Said primaries stand.
        let both = ColourHint {
            primaries: Some(12),
            ..matrix(9)
        };
        assert_eq!(resolve(both, NOTHING, 1920, 1080).primaries, 12);
    }

    /// A transfer of BT.2020's guesses BT.2020 for what is unsaid, at any
    /// size, as Chrome guesses it -- unless BT.709 is said, which Chrome
    /// ranks above it.
    #[test]
    fn hdr_is_guessed_as_chrome_guesses_it() {
        let hint = |transfer, matrix, primaries| ColourHint {
            transfer: Some(transfer),
            matrix,
            primaries,
            ..NOTHING
        };
        for transfer in [14, 15, 16, 18] {
            let c = resolve(hint(transfer, None, None), NOTHING, 720, 576);
            assert_eq!((c.matrix, c.primaries), (9, 9), "transfer {transfer}");
        }
        // BT.601's matrix ranks below BT.2020: the primaries are still
        // BT.2020's, where mpv would take the size's.
        let c = resolve(hint(16, Some(6), None), NOTHING, 720, 576);
        assert_eq!((c.matrix, c.primaries), (6, 9));
        let c = resolve(hint(18, None, Some(5)), NOTHING, 720, 576);
        assert_eq!((c.matrix, c.primaries), (9, 5));
        // BT.709 said by either is taken for the other.
        let c = resolve(hint(16, Some(1), None), NOTHING, 3840, 2160);
        assert_eq!((c.matrix, c.primaries), (1, 1));
        let c = resolve(hint(16, None, Some(1)), NOTHING, 720, 576);
        assert_eq!((c.matrix, c.primaries), (1, 1));
        // An ordinary transfer is mpv's guess.
        let c = resolve(hint(1, None, None), NOTHING, 720, 576);
        assert_eq!((c.matrix, c.primaries), (6, 5));
        // The bitstream's transfer and the file's matrix combine.
        let file = ColourHint {
            matrix: Some(6),
            ..NOTHING
        };
        let c = resolve(hint(16, None, None), file, 1920, 1080);
        assert_eq!((c.matrix, c.primaries, c.transfer), (6, 9, 16));
    }

    #[test]
    fn the_bitstream_wins_then_the_file() {
        let file = ColourHint {
            matrix: Some(9),
            primaries: Some(9),
            full_range: Some(true),
            ..NOTHING
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
            ..NOTHING
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
        let file = |m, p, t, r| ColourHint::matroska(Some(&mkv(m, p, t, r)));
        assert_eq!(file(2, 2, 2, 0), NOTHING);
        assert_eq!(file(3, 3, 3, 3), NOTHING);
        // The transfer's reserved 0, as the primaries'.
        assert_eq!(file(256, 0, 0, 0), NOTHING);
        assert_eq!(
            file(0, 1, 16, 2),
            ColourHint {
                matrix: Some(0),
                primaries: Some(1),
                transfer: Some(16),
                full_range: Some(true),
                ..NOTHING
            }
        );
        assert_eq!(file(1, 1, 1, 1).full_range, Some(false));
        assert_eq!(ColourHint::matroska(None), NOTHING);
        // MP4's: the same words, and `nclc`'s silence on the range.
        let mp4 = |p, t, m, full_range| {
            ColourHint::mp4(&mp4_video(
                Some(mp4::Colour {
                    primaries: p,
                    transfer: t,
                    matrix: m,
                    full_range,
                }),
                None,
                None,
            ))
        };
        assert_eq!(mp4(2, 2, 2, None), NOTHING);
        assert_eq!(mp4(0, 0, 3, None), NOTHING);
        assert_eq!(
            mp4(9, 18, 9, Some(true)),
            ColourHint {
                matrix: Some(9),
                primaries: Some(9),
                transfer: Some(18),
                full_range: Some(true),
                ..NOTHING
            }
        );
        assert_eq!(mp4(1, 1, 0, Some(false)).matrix, Some(0));
        assert_eq!(ColourHint::mp4(&mp4_video(None, None, None)), NOTHING);
    }

    /// An untagged transfer is BT.709's, ordinary video's; a tag stands.
    #[test]
    fn unsaid_transfer_is_bt709s() {
        assert_eq!(resolve(NOTHING, NOTHING, 1920, 1080).transfer, 1);
        let pq = ColourHint {
            transfer: Some(16),
            ..NOTHING
        };
        assert_eq!(resolve(NOTHING, pq, 1920, 1080).transfer, 16);
    }

    /// MP4's light: each number over its box's scale, exactly FFmpeg's
    /// rational; and Matroska's, as the file gives it, a MaxCLL past 32 bits
    /// cut to them as FFmpeg's C cuts it.
    #[test]
    fn the_light_is_scaled_as_ffmpeg_scales_it() {
        let mdcv = mp4::Mastering {
            chromaticities: [35400, 14600, 8500, 39850, 6550, 2300, 15635, 16450],
            chromaticity_scale: 50_000,
            max_luminance: 10_000_000,
            max_luminance_scale: 10_000,
            min_luminance: 1,
            min_luminance_scale: 10_000,
        };
        let light = mp4::ContentLight {
            max_cll: 1000,
            max_fall: 400,
        };
        let hint = ColourHint::mp4(&mp4_video(None, Some(mdcv), Some(light)));
        let m = hint.light.mastering.unwrap();
        let c = m.chromaticities.unwrap();
        assert_eq!((c.red, c.white), ([0.708, 0.292], [0.3127, 0.329]));
        let l = m.luminance.unwrap();
        assert_eq!((l.max, l.min), (1000.0, 0.0001));
        assert_eq!(
            hint.light.content,
            Some(ContentLightLevel {
                max_cll: 1000,
                max_fall: 400
            })
        );
        let mut huge = mkv(2, 2, 16, 0);
        huge.content_light = Some(matroska::ContentLight {
            max_cll: (1 << 32) + 1000,
            max_fall: 400,
        });
        let content = ColourHint::matroska(Some(&huge)).light.content.unwrap();
        assert_eq!((content.max_cll, content.max_fall), (1000, 400));
    }

    /// Each kind of light the bitstream's where it says it, else the file's.
    #[test]
    fn the_bitstream_s_light_stands_kind_by_kind() {
        let display = |max| MasteringDisplay {
            chromaticities: None,
            luminance: Some(Luminance { max, min: 0.0 }),
        };
        let level = |max_cll| ContentLightLevel {
            max_cll,
            max_fall: 1,
        };
        let file = Light {
            mastering: Some(display(4000.0)),
            content: Some(level(2000)),
        };
        // The bitstream's display over the file's; the file's level where
        // the bitstream has none.
        let stream = Light {
            mastering: Some(display(1000.0)),
            content: None,
        };
        assert_eq!(
            light(stream, file),
            Light {
                mastering: Some(display(1000.0)),
                content: Some(level(2000)),
            }
        );
        // And the other way about: the bitstream's level over the file's.
        let stream = Light {
            mastering: None,
            content: Some(level(1000)),
        };
        assert_eq!(
            light(stream, file),
            Light {
                mastering: Some(display(4000.0)),
                content: Some(level(1000)),
            }
        );
        assert_eq!(light(Light::default(), file), file);
        assert_eq!(light(file, Light::default()), file);
    }
}
