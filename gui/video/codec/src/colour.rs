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
//!
//! **A colour said whole** is taken as Chrome takes it, and converted as
//! Chrome converts it (design-decisions §1381): where one place -- the
//! bitstream, or the file -- says all four parts, each a code Chrome names,
//! that place's colour is the picture's, all of it, and its primaries and
//! curve are converted to sRGB's where they are not sRGB's (`yuv::managed`).
//! Chrome's decoders differ in which place they ask first: its libvpx one
//! (VP8, VP9) the file, then the bitstream, whose one colour-space field it
//! reads as all four; its dav1d one (AV1) the bitstream, then the file
//! (`vpx_video_decoder.cc`, `dav1d_video_decoder.cc`, measured in Chrome
//! 154). A colour said in pieces, or not at all, Chrome shows as BT.601 at
//! the studio range, unconverted; here it is pieced together and guessed as
//! above, and also unconverted -- which of the two SlateOS should do is
//! `open-questions/F-Q11.md`.

use rav1d::safe as av1;
use yuv::managed::{SdrCurve, Transfer};

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
    /// Said whole in one place, each part a code Chrome names: Chrome then
    /// shows the picture by exactly this colour, and converts its primaries
    /// and curve to the screen's where they are not sRGB's. A colour
    /// pieced together or guessed is shown unconverted.
    pub whole: bool,
}

impl Colour {
    /// Whether a picture of this colour is shown as Chrome shows it by its
    /// colour management (`yuv::managed`) rather than converted as its
    /// samples are sRGB's: HDR always; an SDR picture where its colour was
    /// said whole, its primaries or its curve are not sRGB's, and Chrome has
    /// a curve for its transfer (design-decisions §1381).
    #[must_use]
    pub fn converted(self) -> bool {
        self.managed().is_some()
    }

    /// The transfer a picture of this colour is converted by when it is
    /// [`Self::converted`].
    pub(crate) fn managed(self) -> Option<Transfer> {
        let transfer = Transfer::from_h273(self.transfer)?;
        let srgb = self.primaries == PRIMARIES_BT709 && transfer == Transfer::Sdr(SdrCurve::Srgb);
        (transfer.is_hdr() || (self.whole && !srgb)).then_some(transfer)
    }
}

/// Which of a stream's two places Chrome's decoder for it asks first for a
/// colour said whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prefer {
    /// Its dav1d decoder (AV1): the sequence header, then the file.
    Bitstream,
    /// Its libvpx decoder (VP8, VP9): the file, then the bitstream.
    File,
}

/// Primaries Chrome names (`ToGfxPrimaryID`): BT.709's, BT.470 M's and B
/// and G's, SMPTE 170M's and 240M's, film's, BT.2020's, XYZ's (ST 428-1),
/// DCI-P3's, Display P3's and H.273's 22.
const fn named_primaries(p: u16) -> bool {
    matches!(p, 1 | 4..=12 | 22)
}

/// Transfers Chrome names (`ToGfxTransferID`): every code from 4 to 18,
/// and BT.709's.
const fn named_transfer(t: u16) -> bool {
    matches!(t, 1 | 4..=18)
}

/// Matrices Chrome names (`ToGfxMatrixID`): RGB's, BT.709's, FCC's,
/// BT.601's two, SMPTE 240M's, YCgCo's, BT.2020's non-constant one and
/// YDZDX's. Not BT.2020's constant-luminance one, which it no longer
/// supports.
const fn named_matrix(m: u16) -> bool {
    matches!(m, 0 | 1 | 4..=9 | 11)
}

/// The colour `matrix`, `primaries`, `transfer` and range say whole, as
/// Chrome takes a colour (`gfx::ColorSpace::IsValid`): `None` unless each
/// is said and named.
fn whole(
    matrix: Option<u16>,
    primaries: Option<u16>,
    transfer: Option<u16>,
    full_range: Option<bool>,
) -> Option<Colour> {
    let (matrix, primaries, transfer, full_range) = (matrix?, primaries?, transfer?, full_range?);
    (named_matrix(matrix) && named_primaries(primaries) && named_transfer(transfer)).then_some(
        Colour {
            matrix,
            primaries,
            transfer,
            full_range,
            whole: true,
        },
    )
}

impl ColourHint {
    /// What a VP8 key frame says, as FFmpeg reads it (`libavcodec/vp8.c`):
    /// its colour space bit 0 -- the only value the specification defines,
    /// YUV "similar to BT.601" -- is BT.601 (FFmpeg's `BT470BG` matrix, 5),
    /// and 1 says nothing; its clamping type bit 1 is the full range, and 0
    /// the studio range. libvpx reads both bits and goes by neither. FFmpeg's
    /// reading is what players built on it show, so a VP8 picture is BT.601
    /// whatever its size or its file says -- unlike untagged VP9, whose HD
    /// sizes are guessed BT.709. Chrome's libvpx decoder reads neither bit
    /// (libvpx leaves a VP8 picture's colour space unknown): VP8 says no
    /// colour whole.
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
    ///
    /// Chrome reads the one field as a colour whole
    /// (`VpxVideoDecoder::CopyVpxImageToVideoFrame`): BT.601 and SMPTE
    /// 170M are SMPTE 170M's primaries, curve and matrix; SMPTE 240M is
    /// 240M's three; BT.709 BT.709's; BT.2020 BT.2020's primaries and
    /// matrix, with its curve for 10 and 12 bits (`depth`) and BT.709's for
    /// 8; sRGB BT.709's primaries, sRGB's curve and RGB.
    pub(crate) fn vp9(color_space: u8, full_range: bool, depth: u8) -> Self {
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
        let bt2020_transfer = match depth {
            12.. => 15,
            10.. => 14,
            _ => TRANSFER_BT709,
        };
        // Chrome's primaries, transfer and matrix for each space.
        let chrome = match color_space {
            1 | 3 => Some((PRIMARIES_SMPTE170M, 6, MATRIX_BT601)),
            4 => Some((7, 7, 7)),
            2 => Some((PRIMARIES_BT709, TRANSFER_BT709, MATRIX_BT709)),
            5 => Some((PRIMARIES_BT2020, bt2020_transfer, MATRIX_BT2020_NCL)),
            7 => Some((PRIMARIES_BT709, 13, 0)),
            _ => None,
        };
        Self {
            matrix,
            full_range: Some(full_range),
            whole: chrome.map(|(primaries, transfer, matrix)| Colour {
                matrix,
                primaries,
                transfer,
                full_range,
                whole: true,
            }),
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
            light,
            ..Self::cicp(
                u16::from(c.matrix),
                u16::from(c.primaries),
                u16::from(c.transfer),
                c.full_range,
            )
        }
    }

    /// What H.273's code points say as a bitstream carries them -- AV1's
    /// sequence header, every part given, unspecified where unknown:
    /// unspecified and reserved values nothing, and the colour whole where
    /// every part is a code Chrome names.
    fn cicp(matrix: u16, primaries: u16, transfer: u16, full_range: bool) -> Self {
        let matrix = said(matrix, true);
        let primaries = said(primaries, false);
        let transfer = said(transfer, false);
        Self {
            matrix,
            primaries,
            transfer,
            full_range: Some(full_range),
            light: Light::default(),
            whole: whole(matrix, primaries, transfer, Some(full_range)),
        }
    }

    /// What a Matroska track's `Colour` says.
    pub(crate) fn matroska(colour: Option<&matroska::Colour>) -> Self {
        let Some(c) = colour else {
            return Self::default();
        };
        let matrix = u16::try_from(c.matrix_coefficients)
            .ok()
            .and_then(|m| said(m, true));
        let primaries = u16::try_from(c.primaries).ok().and_then(|p| said(p, false));
        let transfer = u16::try_from(c.transfer_characteristics)
            .ok()
            .and_then(|t| said(t, false));
        // Chrome's reading of the range (`WebMColourParser`): 3, "defined by
        // the matrix and transfer", is the studio range there
        // (`ColorSpace::GetRangeAdjustMatrix`), and says the range.
        let chrome_range = match c.range {
            1 | 3 => Some(false),
            2 => Some(true),
            _ => None,
        };
        Self {
            matrix,
            primaries,
            transfer,
            // 0 unspecified, 1 broadcast, 2 full; 3 "defined by the matrix
            // and transfer", which says nothing about a range by itself.
            full_range: match c.range {
                1 => Some(false),
                2 => Some(true),
                _ => None,
            },
            whole: whole(matrix, primaries, transfer, chrome_range),
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
        let (matrix, primaries, transfer) = (
            said(c.matrix, true),
            said(c.primaries, false),
            said(c.transfer, false),
        );
        Self {
            matrix,
            primaries,
            transfer,
            full_range: c.full_range,
            light,
            whole: whole(matrix, primaries, transfer, c.full_range),
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
/// `bitstream` and whose track's description says `container`: the colour
/// one of them says whole, the one Chrome's decoder asks first (`prefer`);
/// else each of the four from the first that says it, else the guess (the
/// module's).
pub(crate) fn resolve(
    prefer: Prefer,
    bitstream: ColourHint,
    container: ColourHint,
    width: u32,
    height: u32,
) -> Colour {
    let whole = match prefer {
        Prefer::Bitstream => bitstream.whole.or(container.whole),
        Prefer::File => container.whole.or(bitstream.whole),
    };
    if let Some(colour) = whole {
        return colour;
    }
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
        whole: false,
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
        whole: None,
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
        let sd = resolve(Prefer::File, NOTHING, NOTHING, 720, 576);
        assert_eq!((sd.matrix, sd.primaries, sd.full_range), (6, 5, false));
        let ntsc = resolve(Prefer::File, NOTHING, NOTHING, 720, 480);
        assert_eq!((ntsc.matrix, ntsc.primaries), (6, 6));
        let small = resolve(Prefer::File, NOTHING, NOTHING, 320, 240);
        assert_eq!((small.matrix, small.primaries), (6, 1));
        // 1280 wide is HD however short; 577 tall is HD however narrow.
        for (w, h) in [(1280, 96), (720, 577), (1920, 1080)] {
            let hd = resolve(Prefer::File, NOTHING, NOTHING, w, h);
            assert_eq!((hd.matrix, hd.primaries), (1, 1), "{w}x{h}");
        }
        assert_eq!(resolve(Prefer::File, NOTHING, NOTHING, 1279, 576).matrix, 6);
        // NTSC's professional height is mpv's 486, not 488.
        assert_eq!(
            resolve(Prefer::File, NOTHING, NOTHING, 720, 486).primaries,
            6
        );
        assert_eq!(
            resolve(Prefer::File, NOTHING, NOTHING, 720, 488).primaries,
            1
        );
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
        assert_eq!(
            resolve(Prefer::File, matrix(9), NOTHING, 1920, 1080).primaries,
            9
        );
        assert_eq!(
            resolve(Prefer::File, matrix(10), NOTHING, 720, 576).primaries,
            9
        );
        assert_eq!(
            resolve(Prefer::File, matrix(1), NOTHING, 720, 576).primaries,
            1
        );
        assert_eq!(
            resolve(Prefer::File, matrix(5), NOTHING, 720, 576).primaries,
            5
        );
        assert_eq!(
            resolve(Prefer::File, matrix(6), NOTHING, 720, 480).primaries,
            6
        );
        assert_eq!(
            resolve(Prefer::File, matrix(6), NOTHING, 1920, 1080).primaries,
            1
        );
        // Said primaries stand.
        let both = ColourHint {
            primaries: Some(12),
            ..matrix(9)
        };
        assert_eq!(
            resolve(Prefer::File, both, NOTHING, 1920, 1080).primaries,
            12
        );
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
            let c = resolve(Prefer::File, hint(transfer, None, None), NOTHING, 720, 576);
            assert_eq!((c.matrix, c.primaries), (9, 9), "transfer {transfer}");
        }
        // BT.601's matrix ranks below BT.2020: the primaries are still
        // BT.2020's, where mpv would take the size's.
        let c = resolve(Prefer::File, hint(16, Some(6), None), NOTHING, 720, 576);
        assert_eq!((c.matrix, c.primaries), (6, 9));
        let c = resolve(Prefer::File, hint(18, None, Some(5)), NOTHING, 720, 576);
        assert_eq!((c.matrix, c.primaries), (9, 5));
        // BT.709 said by either is taken for the other.
        let c = resolve(Prefer::File, hint(16, Some(1), None), NOTHING, 3840, 2160);
        assert_eq!((c.matrix, c.primaries), (1, 1));
        let c = resolve(Prefer::File, hint(16, None, Some(1)), NOTHING, 720, 576);
        assert_eq!((c.matrix, c.primaries), (1, 1));
        // An ordinary transfer is mpv's guess.
        let c = resolve(Prefer::File, hint(1, None, None), NOTHING, 720, 576);
        assert_eq!((c.matrix, c.primaries), (6, 5));
        // The bitstream's transfer and the file's matrix combine.
        let file = ColourHint {
            matrix: Some(6),
            ..NOTHING
        };
        let c = resolve(Prefer::File, hint(16, None, None), file, 1920, 1080);
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
        let vp9_unknown = ColourHint::vp9(0, false, 8);
        let c = resolve(Prefer::File, vp9_unknown, file, 320, 240);
        assert_eq!((c.matrix, c.primaries, c.full_range), (9, 9, false));
        // ...and the bitstream's own word stands over the file's: VP9's
        // colour space says a colour whole, which stands over the file's
        // pieces entire, as Chrome takes it.
        let vp9_709 = ColourHint::vp9(2, true, 8);
        let c = resolve(Prefer::File, vp9_709, file, 320, 240);
        assert_eq!(
            (c.matrix, c.primaries, c.full_range, c.whole),
            (1, 1, true, true)
        );
        // Neither place says a colour whole: part by part, the bitstream's
        // word stands over the file's.
        let bitstream = ColourHint {
            transfer: Some(16),
            ..NOTHING
        };
        let file = ColourHint {
            transfer: Some(1),
            ..NOTHING
        };
        let c = resolve(Prefer::File, bitstream, file, 1920, 1080);
        assert_eq!((c.transfer, c.whole), (16, false));
        // A tag stands over the size's guess.
        assert_eq!(
            resolve(
                Prefer::File,
                ColourHint::vp9(3, false, 8),
                NOTHING,
                1920,
                1080
            )
            .matrix,
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
        let c = resolve(Prefer::File, ColourHint::vp8(0, 0), file, 1920, 1080);
        assert_eq!((c.matrix, c.primaries, c.full_range), (5, 1, false));
        let c = resolve(Prefer::File, ColourHint::vp8(0, 1), NOTHING, 176, 144);
        assert_eq!((c.matrix, c.full_range), (5, true));
        // The reserved colour space says nothing: the file's word, or the
        // size's guess.
        assert_eq!(
            resolve(Prefer::File, ColourHint::vp8(1, 0), file, 176, 144).matrix,
            1
        );
        assert_eq!(
            resolve(Prefer::File, ColourHint::vp8(1, 0), NOTHING, 176, 144).matrix,
            6
        );
    }

    #[test]
    fn vp9_colour_spaces_are_ffmpegs_numbers() {
        let matrix = |cs| ColourHint::vp9(cs, false, 8).matrix;
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
                whole: Some(Colour {
                    matrix: 0,
                    primaries: 1,
                    transfer: 16,
                    full_range: true,
                    whole: true,
                }),
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
                whole: Some(Colour {
                    matrix: 9,
                    primaries: 9,
                    transfer: 18,
                    full_range: true,
                    whole: true,
                }),
                ..NOTHING
            }
        );
        assert_eq!(mp4(1, 1, 0, Some(false)).matrix, Some(0));
        assert_eq!(ColourHint::mp4(&mp4_video(None, None, None)), NOTHING);
    }

    /// An untagged transfer is BT.709's, ordinary video's; a tag stands.
    #[test]
    fn unsaid_transfer_is_bt709s() {
        assert_eq!(
            resolve(Prefer::File, NOTHING, NOTHING, 1920, 1080).transfer,
            1
        );
        let pq = ColourHint {
            transfer: Some(16),
            ..NOTHING
        };
        assert_eq!(resolve(Prefer::File, NOTHING, pq, 1920, 1080).transfer, 16);
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

    /// A colour said whole is Chrome's: all four parts, each a code Chrome
    /// names, in one place. A part unsaid, unnamed or unspecified -- the
    /// range above all, which a Matroska file often leaves out -- leaves
    /// none; Matroska's "derived" range is the studio range there.
    #[test]
    fn a_colour_is_whole_when_every_part_is_said_and_named() {
        let file = |m, p, t, r| ColourHint::matroska(Some(&mkv(m, p, t, r))).whole;
        let whole = |matrix, primaries, transfer, full_range| {
            Some(Colour {
                matrix,
                primaries,
                transfer,
                full_range,
                whole: true,
            })
        };
        assert_eq!(file(6, 6, 6, 1), whole(6, 6, 6, false));
        assert_eq!(file(1, 12, 13, 2), whole(1, 12, 13, true));
        assert_eq!(file(9, 9, 14, 3), whole(9, 9, 14, false));
        // A part missing or unspecified: the range, the transfer.
        assert_eq!(file(6, 6, 6, 0), None);
        assert_eq!(file(6, 6, 2, 1), None);
        // Codes Chrome has no name for: BT.2020's constant-luminance matrix,
        // the chromaticity-derived ones and ICtCp; primaries 13 to 21; a
        // transfer past 18.
        for m in [10, 12, 13, 14] {
            assert_eq!(file(m, 9, 14, 1), None, "matrix {m}");
        }
        for p in [13, 21, 23] {
            assert_eq!(file(1, p, 1, 1), None, "primaries {p}");
        }
        assert_eq!(file(1, 1, 19, 1), None);
        // Every code Chrome names, each with parts it names.
        for m in [0, 1, 4, 5, 6, 7, 8, 9, 11] {
            assert!(file(m, 1, 1, 1).is_some(), "matrix {m}");
        }
        for p in [1, 4, 5, 6, 7, 8, 9, 10, 11, 12, 22] {
            assert!(file(1, p, 1, 1).is_some(), "primaries {p}");
        }
        for t in [1, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18] {
            assert!(file(1, 1, t, 1).is_some(), "transfer {t}");
        }
    }

    /// VP9's one colour-space field is a colour whole as Chrome reads it;
    /// BT.2020's curve is its own at 10 and 12 bits and BT.709's at 8.
    #[test]
    fn vp9_colour_spaces_are_chrome_s_wholes() {
        let whole = |cs, depth| {
            ColourHint::vp9(cs, false, depth)
                .whole
                .map(|c| (c.primaries, c.transfer, c.matrix))
        };
        assert_eq!(whole(1, 8), Some((6, 6, 6)));
        assert_eq!(whole(3, 8), Some((6, 6, 6)));
        assert_eq!(whole(4, 8), Some((7, 7, 7)));
        assert_eq!(whole(2, 8), Some((1, 1, 1)));
        assert_eq!(whole(5, 8), Some((9, 1, 9)));
        assert_eq!(whole(5, 10), Some((9, 14, 9)));
        assert_eq!(whole(5, 12), Some((9, 15, 9)));
        assert_eq!(whole(7, 8), Some((1, 13, 0)));
        assert_eq!(whole(0, 8), None);
        assert_eq!(whole(6, 8), None);
        let full = ColourHint::vp9(2, true, 8).whole.map(|c| c.full_range);
        assert_eq!(full, Some(true));
        // VP8's bits say no colour whole: Chrome reads neither.
        assert_eq!(ColourHint::vp8(0, 1).whole, None);
    }

    /// AV1's code points say what they name: unspecified (2) and reserved
    /// values nothing, and a colour is whole only where every part is named.
    #[test]
    fn av1_s_code_points_say_what_they_name() {
        let c = ColourHint::cicp(2, 2, 2, false);
        assert_eq!(
            (c.matrix, c.primaries, c.transfer, c.whole),
            (None, None, None, None)
        );
        let c = ColourHint::cicp(3, 0, 0, false);
        assert_eq!((c.matrix, c.primaries, c.transfer), (None, None, None));
        let c = ColourHint::cicp(9, 9, 16, false);
        assert_eq!(
            (c.matrix, c.primaries, c.transfer),
            (Some(9), Some(9), Some(16))
        );
        assert_eq!(
            c.whole.map(|w| (w.transfer, w.full_range)),
            Some((16, false))
        );
        // One part unspecified: the rest said, nothing whole.
        let c = ColourHint::cicp(1, 1, 2, true);
        assert_eq!((c.matrix, c.transfer, c.whole), (Some(1), None, None));
    }

    /// What Chrome converts: HDR however its colour was found; an SDR
    /// colour said whole in primaries or a curve not sRGB's, and only where
    /// Chrome has a curve for its transfer; never an SDR colour pieced
    /// together or guessed.
    #[test]
    fn chrome_converts_whole_colours_that_are_not_srgb_s() {
        let colour = |primaries, transfer, whole| Colour {
            matrix: 6,
            primaries,
            transfer,
            full_range: false,
            whole,
        };
        // Said whole: other primaries, or another curve.
        assert!(colour(6, 6, true).converted());
        assert!(colour(9, 14, true).converted());
        assert!(colour(1, 4, true).converted());
        // Said whole, but sRGB's: BT.709's primaries by any of the curves
        // Chrome takes for the sRGB one.
        for transfer in [1, 6, 13, 14, 15] {
            assert!(!colour(1, transfer, true).converted(), "{transfer}");
        }
        // A curve Chrome has none for: nothing converted, primaries or not.
        assert!(!colour(9, 11, true).converted());
        // Pieced together or guessed: not converted, unless HDR.
        assert!(!colour(6, 6, false).converted());
        assert!(!colour(9, 1, false).converted());
        assert!(colour(9, 16, false).converted());
        assert!(colour(9, 18, true).converted());
    }

    /// Which place's whole colour stands is Chrome's decoder's choice:
    /// libvpx's (VP8, VP9) the file's, dav1d's (AV1) the bitstream's. Either
    /// stands over pieces in the other, and a colour said whole nowhere is
    /// pieced together and guessed, and not whole.
    #[test]
    fn whose_whole_colour_stands_is_the_decoder_s() {
        let bitstream = ColourHint::vp9(2, false, 8);
        let file = ColourHint::matroska(Some(&mkv(6, 6, 6, 1)));
        let vpx = resolve(Prefer::File, bitstream, file, 1920, 1080);
        assert_eq!((vpx.matrix, vpx.primaries, vpx.transfer), (6, 6, 6));
        let av1 = resolve(Prefer::Bitstream, bitstream, file, 1920, 1080);
        assert_eq!((av1.matrix, av1.primaries, av1.transfer), (1, 1, 1));
        assert!(vpx.whole && av1.whole);
        // The file's pieces give way to the bitstream's whole colour,
        // whichever is asked first.
        let pieces = ColourHint::matroska(Some(&mkv(6, 6, 6, 0)));
        let c = resolve(Prefer::File, bitstream, pieces, 1920, 1080);
        assert_eq!((c.matrix, c.primaries, c.whole), (1, 1, true));
        // Pieces alone: per part, and guessed, and not whole.
        let unknown = ColourHint::vp9(0, false, 8);
        let c = resolve(Prefer::File, unknown, pieces, 1920, 1080);
        assert_eq!(
            (c.matrix, c.primaries, c.transfer, c.whole),
            (6, 6, 6, false)
        );
    }
}
