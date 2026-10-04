//! A decoded picture, and its conversion to pixels.
//!
//! A [`Picture`] holds the decoder's planes as the decoder made them --
//! VP9's in place, without a copy -- so that a player that has fallen
//! behind can drop pictures without paying for their conversion, and
//! converts only the ones it shows ([`Picture::to_frame`]).
//!
//! The conversion is `gui/video/yuv`'s port of libavif's, by the colour
//! [`crate::colour`] settles on. A frame large enough to be worth it is
//! converted in bands of rows, one to each core (`to_argb_rows`, whose every
//! band is exactly those rows of the whole picture). VP9's 4:4:0 -- chroma
//! at full width and half height, which neither libyuv nor libavif converts
//! -- has its chroma brought to the full height first, as libyuv brings
//! 4:2:0's chroma down its columns, and is then converted as 4:4:4.

use std::num::NonZeroUsize;
use std::sync::OnceLock;
use std::thread;

use rav1d::safe as av1;
use yuv::reformat::{self, Format, Reformat};
use yuv::{Plane, PlaneBuf};

use crate::colour::{self, Colour};
use crate::decoder::Packet;
use crate::{ColourHint, Error, Frame};

/// A decoded picture, not yet converted.
pub struct Picture {
    /// When it is shown, in nanoseconds on the file's clock.
    pub time: i64,
    /// How long, in nanoseconds; 0 if the file does not say.
    pub duration: u64,
    /// Whether it is a key frame.
    pub keyframe: bool,
    planes: Planes,
    /// What the file says of the track's colour.
    hint: ColourHint,
}

/// The decoder's planes.
pub(crate) enum Planes {
    Vp9 {
        picture: vp9::Picture,
        /// The alpha stream's picture, whose luma is the alpha.
        alpha: Option<vp9::Picture>,
    },
    Av1(av1::Picture),
}

impl Picture {
    /// A picture `packet` showed.
    pub(crate) fn new(packet: &Packet<'_>, planes: Planes, hint: ColourHint) -> Self {
        Self {
            time: packet.time,
            duration: packet.duration,
            keyframe: packet.keyframe,
            planes,
            hint,
        }
    }

    /// An AV1 picture, timed by the packet it came from.
    pub(crate) fn av1(picture: av1::Picture, hint: ColourHint) -> Self {
        Self {
            // Every packet is sent with its time; only the track's
            // configuration is not, and it carries no frame to show.
            time: picture.timestamp().unwrap_or(0),
            duration: u64::try_from(picture.duration()).unwrap_or(0),
            keyframe: picture.is_key_frame(),
            planes: Planes::Av1(picture),
            hint,
        }
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> (u32, u32) {
        match &self.planes {
            Planes::Vp9 { picture, .. } => (picture.width(), picture.height()),
            Planes::Av1(p) => p.size(),
        }
    }

    /// The colour it is converted by: its bitstream's, the file's where the
    /// bitstream is silent, and a guess from its size where both are.
    pub fn colour(&self) -> Colour {
        let (width, height) = self.size();
        let said = match &self.planes {
            Planes::Vp9 { picture, .. } => {
                let (space, full_range) = picture.color();
                ColourHint::vp9(space, full_range)
            }
            Planes::Av1(p) => ColourHint::av1(p.colour()),
        };
        colour::resolve(said, self.hint, width, height)
    }

    /// The picture as pixels.
    ///
    /// # Errors
    ///
    /// [`Error::Colour`] for a colour description libavif does not convert.
    pub fn to_frame(&self) -> Result<Frame, Error> {
        let mut pixels = Vec::new();
        self.to_pixels(&mut pixels)?;
        let (width, height) = self.size();
        Ok(Frame {
            time: self.time,
            duration: self.duration,
            keyframe: self.keyframe,
            width,
            height,
            pixels,
        })
    }

    /// The picture's pixels into `out`, which is cleared first: for a player
    /// that converts frame after frame into one buffer.
    ///
    /// # Errors
    ///
    /// As [`Self::to_frame`]; `out` is then empty.
    pub fn to_pixels(&self, out: &mut Vec<u32>) -> Result<(), Error> {
        let colour = self.colour();
        let result = match &self.planes {
            Planes::Vp9 { picture, alpha } => vp9_pixels(picture, alpha.as_ref(), colour, out),
            Planes::Av1(p) => av1_pixels(p, colour, out),
        };
        if result.is_err() {
            out.clear();
        }
        result
    }
}

/// A sample `yuv` converts, which a weighted average can be written back
/// into, and which threads converting bands of one picture can share.
trait Sample: Reformat + Sync {
    fn narrow(v: u32) -> Self;
}

impl Sample for u8 {
    fn narrow(v: u32) -> Self {
        Self::try_from(v).unwrap_or(Self::MAX)
    }
}

impl Sample for u16 {
    fn narrow(v: u32) -> Self {
        Self::try_from(v).unwrap_or(Self::MAX)
    }
}

/// The planes and size of a picture to convert, with its colour.
struct Planar<'a, T> {
    width: usize,
    height: usize,
    depth: u8,
    /// Chroma's subsampling across and down: VP9's `ss_x` and `ss_y`.
    subsampling: (u8, u8),
    y: Plane<'a, T>,
    /// U and V; absent for monochrome.
    chroma: Option<(Plane<'a, T>, Plane<'a, T>)>,
    alpha: Option<Plane<'a, T>>,
}

impl<T: Sample> Planar<'_, T> {
    fn convert(&self, colour: Colour, out: &mut Vec<u32>) -> Result<(), Error> {
        let format = match (self.chroma.is_some(), self.subsampling) {
            (false, _) => Format::Yuv400,
            (true, (1, 1)) => Format::Yuv420,
            (true, (1, 0)) => Format::Yuv422,
            (true, (0, 0)) => Format::Yuv444,
            (true, (0, 1)) => return self.convert_440(colour, out),
            (true, _) => return Err(Error::Colour(reformat::Error::Unsupported)),
        };
        let (u, v) = self.chroma.unzip();
        let picture = reformat::Picture {
            width: self.width,
            height: self.height,
            depth: self.depth,
            format,
            matrix: colour.matrix,
            primaries: colour.primaries,
            full_range: colour.full_range,
            y: self.y,
            u,
            v,
            alpha: self.alpha,
            alpha_premultiplied: false,
        };
        in_bands(&picture, out)
    }

    /// 4:4:0: the chroma brought to the full height, then 4:4:4.
    fn convert_440(&self, colour: Colour, out: &mut Vec<u32>) -> Result<(), Error> {
        let Some((u, v)) = self.chroma else {
            return Err(Error::Colour(reformat::Error::Size));
        };
        let (u, v) = (rows_doubled(u, self.height), rows_doubled(v, self.height));
        let full = Planar {
            subsampling: (0, 0),
            chroma: Some((u.view(), v.view())),
            ..*self
        };
        full.convert(colour, out)
    }
}

/// The fewest pixels worth a thread of their own: about half a millisecond of
/// converting (some 4 ns a pixel, `gui/video/yuv/tests/bench.rs`), against
/// the tens of microseconds a thread takes to start. So a 1080p frame (2
/// million pixels) is split as many ways as there are cores, up to fifteen,
/// and a small one is converted where it is.
const MIN_BAND_PIXELS: usize = 1 << 17;

/// How many threads a picture's bands may use: the machine's cores, asked
/// once.
fn cores() -> usize {
    static CORES: OnceLock<usize> = OnceLock::new();
    *CORES.get_or_init(|| thread::available_parallelism().map_or(1, NonZeroUsize::get))
}

/// `picture`'s pixels into `out`, a band of rows to each of the machine's
/// cores (`yuv::reformat::to_argb_rows`, whose every band is exactly those
/// rows of the whole picture): 8 ms for an HD frame on one thread becomes a
/// fraction of that, for every frame a player shows.
///
/// # Errors
///
/// As `yuv::reformat::to_argb_into`; `out` is then empty.
fn in_bands<T: Sample>(
    picture: &reformat::Picture<'_, T>,
    out: &mut Vec<u32>,
) -> Result<(), Error> {
    let pixels = picture.width.saturating_mul(picture.height);
    // At least two rows a band, as split takes them.
    let bands = (pixels / MIN_BAND_PIXELS).min(picture.height / 2);
    split(picture, out, cores().min(bands))
}

/// [`in_bands`] in `bands` bands, a thread each; one or none on the calling
/// thread.
fn split<T: Sample>(
    picture: &reformat::Picture<'_, T>,
    out: &mut Vec<u32>,
    bands: usize,
) -> Result<(), Error> {
    let (width, height) = (picture.width, picture.height);
    if bands <= 1 {
        return reformat::to_argb_into(picture, out).map_err(Error::Colour);
    }
    let size = Error::Colour(reformat::Error::Size);
    let count = width.checked_mul(height).ok_or(size)?;
    out.clear();
    out.try_reserve_exact(count).map_err(|_| size)?;
    out.resize(count, 0);
    // Whole pairs of rows to a band: 4:2:0's chroma comes a pair at a time.
    let rows = height.div_ceil(bands).next_multiple_of(2);
    let band = rows.checked_mul(width).ok_or(size)?;
    let results: Vec<Result<(), reformat::Error>> = thread::scope(|scope| {
        let workers: Vec<_> = out
            .chunks_mut(band)
            .zip((0..).step_by(rows))
            .map(|(pixels, first)| {
                scope.spawn(move || reformat::to_argb_rows(picture, first, pixels))
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    if let Some(Err(e)) = results.into_iter().find(Result::is_err) {
        out.clear();
        return Err(Error::Colour(e));
    }
    Ok(())
}

// Views copy whatever their samples are, so not derived.
impl<T> Clone for Planar<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Planar<'_, T> {}

/// A chroma plane of half the picture's `height` brought to all of it, as
/// libyuv's bilinear 4:2:0 conversion brings chroma down its columns
/// (`I420ToARGBMatrixBilinear`): the first row from the first chroma row
/// alone; each pair of rows after it 3:1 and 1:3 of the two chroma rows
/// around them; and for an even height, the last row from the last chroma
/// row alone.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "samples are at most 16 bits, so 3a + b + 2 is under 2^18"
)]
fn rows_doubled<T: Sample>(chroma: Plane<'_, T>, height: usize) -> PlaneBuf<T> {
    let width = chroma.width;
    let mut out = PlaneBuf {
        width,
        height,
        samples: Vec::with_capacity(width.saturating_mul(height)),
    };
    if height == 0 {
        return out;
    }
    let mut push = |a: &[T], b: &[T], weigh: fn(u32, u32) -> u32| {
        for i in 0..width {
            let (x, y): (u32, u32) = (
                a.get(i).copied().unwrap_or_default().into(),
                b.get(i).copied().unwrap_or_default().into(),
            );
            out.samples.push(T::narrow(weigh(x, y)));
        }
    };
    let near = |x: u32, _: u32| x;
    push(chroma.row(0), chroma.row(0), near);
    let mut c = 0usize;
    for _ in 0..(height - 1) / 2 {
        let (a, b) = (chroma.row(c), chroma.row(c + 1));
        push(a, b, |x, y| (x * 3 + y + 2) >> 2);
        push(a, b, |x, y| (x + y * 3 + 2) >> 2);
        c += 1;
    }
    if height.is_multiple_of(2) {
        push(chroma.row(c), chroma.row(c), near);
    }
    out
}

/// A VP9 plane as `yuv` reads it, in place.
fn view<P>(p: vp9::PlaneView<'_, P>) -> Plane<'_, P> {
    Plane {
        samples: p.data,
        stride: p.stride,
        width: p.width,
        height: p.height,
    }
}

/// The alpha stream's picture, if it fits the picture it belongs to: the
/// same size and depth. (FFmpeg drops a frame whose alpha does not fit; here
/// it is shown opaque.)
fn fitting<'a>(
    alpha: Option<&'a vp9::Picture>,
    picture: &vp9::Picture,
) -> Option<&'a vp9::Picture> {
    alpha.filter(|a| {
        (a.width(), a.height(), a.bit_depth())
            == (picture.width(), picture.height(), picture.bit_depth())
    })
}

fn vp9_pixels(
    picture: &vp9::Picture,
    alpha: Option<&vp9::Picture>,
    colour: Colour,
    out: &mut Vec<u32>,
) -> Result<(), Error> {
    let size = |n: u32| usize::try_from(n).map_err(|_| Error::Colour(reformat::Error::Size));
    let (width, height) = (size(picture.width())?, size(picture.height())?);
    let depth = picture.bit_depth();
    let subsampling = picture.subsampling();
    let alpha = fitting(alpha, picture);
    let missing = Error::Colour(reformat::Error::Size);
    if depth == 8 {
        let [Some(y), Some(u), Some(v)] = [0, 1, 2].map(|i| picture.plane8(i).map(view)) else {
            return Err(missing);
        };
        Planar {
            width,
            height,
            depth,
            subsampling,
            y,
            chroma: Some((u, v)),
            alpha: alpha.and_then(|a| a.plane8(0)).map(view),
        }
        .convert(colour, out)
    } else {
        let [Some(y), Some(u), Some(v)] = [0, 1, 2].map(|i| picture.plane16(i).map(view)) else {
            return Err(missing);
        };
        Planar {
            width,
            height,
            depth,
            subsampling,
            y,
            chroma: Some((u, v)),
            alpha: alpha.and_then(|a| a.plane16(0)).map(view),
        }
        .convert(colour, out)
    }
}

/// An AV1 plane as `yuv` reads it. (rav1d's safe interface hands out a copy
/// of each plane, packed -- `gui/video/rav1d/safe.rs` says why.)
fn packed<T>(p: &av1::Plane<T>) -> Plane<'_, T> {
    Plane {
        samples: &p.samples,
        stride: p.width,
        width: p.width,
        height: p.height,
    }
}

fn av1_pixels(picture: &av1::Picture, colour: Colour, out: &mut Vec<u32>) -> Result<(), Error> {
    let size = |n: u32| usize::try_from(n).map_err(|_| Error::Colour(reformat::Error::Size));
    let (w, h) = picture.size();
    let (width, height) = (size(w)?, size(h)?);
    let depth = picture.bit_depth();
    let subsampling = match picture.layout() {
        av1::Layout::I420 => (1, 1),
        av1::Layout::I422 => (1, 0),
        av1::Layout::I444 | av1::Layout::I400 => (0, 0),
    };
    let missing = Error::Colour(reformat::Error::Size);
    if depth == 8 {
        let y = picture.plane_u8(0).ok_or(missing)?;
        let (u, v) = (picture.plane_u8(1), picture.plane_u8(2));
        Planar {
            width,
            height,
            depth,
            subsampling,
            y: packed(&y),
            chroma: u
                .as_ref()
                .zip(v.as_ref())
                .map(|(u, v)| (packed(u), packed(v))),
            alpha: None,
        }
        .convert(colour, out)
    } else {
        let y = picture.plane_u16(0).ok_or(missing)?;
        let (u, v) = (picture.plane_u16(1), picture.plane_u16(2));
        Planar {
            width,
            height,
            depth,
            subsampling,
            y: packed(&y),
            chroma: u
                .as_ref()
                .zip(v.as_ref())
                .map(|(u, v)| (packed(u), packed(v))),
            alpha: None,
        }
        .convert(colour, out)
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    fn plane(width: usize, rows: &[&[u8]]) -> PlaneBuf<u8> {
        PlaneBuf {
            width,
            height: rows.len(),
            samples: rows.concat(),
        }
    }

    /// 4:4:0's chroma comes down the columns as libyuv's 4:2:0 chroma does:
    /// the first row alone, 3:1 and 1:3 between, and the last alone again
    /// for an even height.
    #[test]
    fn four_four_zero_chroma_doubles_its_rows_as_libyuv_would() {
        let c = plane(2, &[&[0, 100], &[40, 200], &[80, 0]]);
        let even = rows_doubled(c.view(), 6);
        assert_eq!(
            even.samples,
            [
                0, 100, // row 0: chroma row 0
                10, 125, // (3 * 0 + 40 + 2) >> 2, (300 + 200 + 2) >> 2
                30, 175, // (0 + 120 + 2) >> 2, (100 + 600 + 2) >> 2
                50, 150, // between chroma rows 1 and 2
                70, 50, //
                80, 0, // row 5, the even height's last: chroma row 2
            ]
        );
        let odd = rows_doubled(c.view(), 5);
        assert_eq!(odd.samples, even.samples[..10]);
        assert_eq!(rows_doubled(c.view(), 1).samples, [0, 100]);
        assert!(rows_doubled(c.view(), 0).samples.is_empty());
    }

    /// A 4:4:0 picture converts as the 4:4:4 picture of its doubled chroma.
    #[test]
    fn a_four_four_zero_picture_is_its_doubled_four_four_four() {
        let y = plane(2, &[&[16, 50], &[100, 235], &[60, 70], &[200, 30]]);
        let u = plane(2, &[&[90, 160], &[128, 30]]);
        let v = plane(2, &[&[240, 16], &[70, 128]]);
        let colour = Colour {
            matrix: 1,
            primaries: 1,
            full_range: false,
        };
        let planar = Planar {
            width: 2,
            height: 4,
            depth: 8,
            subsampling: (0, 1),
            y: y.view(),
            chroma: Some((u.view(), v.view())),
            alpha: None,
        };
        let mut got = Vec::new();
        planar.convert(colour, &mut got).unwrap();
        let (u4, v4) = (rows_doubled(u.view(), 4), rows_doubled(v.view(), 4));
        let mut want = Vec::new();
        Planar {
            subsampling: (0, 0),
            chroma: Some((u4.view(), v4.view())),
            ..planar
        }
        .convert(colour, &mut want)
        .unwrap();
        assert_eq!(got.len(), 8);
        assert_eq!(got, want);
    }

    /// Monochrome has no chroma and converts as grey.
    #[test]
    fn a_picture_without_chroma_is_grey() {
        let y = plane(2, &[&[16, 235]]);
        let mut out = Vec::new();
        Planar {
            width: 2,
            height: 1,
            depth: 8,
            subsampling: (0, 0),
            y: y.view(),
            chroma: None,
            alpha: None,
        }
        .convert(
            Colour {
                matrix: 1,
                primaries: 1,
                full_range: false,
            },
            &mut out,
        )
        .unwrap();
        assert_eq!(out, [0xff00_0000, 0xffff_ffff]);
    }

    /// A picture split across threads in any number of bands -- more bands
    /// than rows to spare, bands of odd and even heights, 4:2:0 and 4:2:2 at
    /// two depths -- is the picture converted whole.
    #[test]
    fn bands_on_threads_are_the_whole_picture() {
        fn check<T: Sample + TryFrom<u32> + Default>(subsampling: (u8, u8), depth: u8) {
            let (width, height): (usize, usize) = (9, 23);
            let half = |n: usize, ss: u8| if ss == 1 { n.div_ceil(2) } else { n };
            let (cw, ch) = (half(width, subsampling.0), half(height, subsampling.1));
            let fill = |w: usize, h: usize, seed: u32| PlaneBuf::<T> {
                width: w,
                height: h,
                samples: (0..w * h)
                    .map(|i| {
                        let v = (u32::try_from(i).unwrap().wrapping_mul(2_654_435_761) ^ seed) >> 7;
                        T::try_from(v % (1 << depth)).ok().unwrap_or_default()
                    })
                    .collect(),
            };
            let (y, u, v) = (fill(width, height, 1), fill(cw, ch, 2), fill(cw, ch, 3));
            let picture = reformat::Picture {
                width,
                height,
                depth,
                format: if subsampling.1 == 1 {
                    Format::Yuv420
                } else {
                    Format::Yuv422
                },
                matrix: 1,
                primaries: 1,
                full_range: false,
                y: y.view(),
                u: Some(u.view()),
                v: Some(v.view()),
                alpha: None,
                alpha_premultiplied: false,
            };
            let whole = reformat::to_argb(&picture).unwrap();
            for bands in [2, 3, 5, 11, 12, 40] {
                let mut out = Vec::new();
                split(&picture, &mut out, bands).unwrap();
                assert_eq!(out, whole, "{depth}-bit {subsampling:?} in {bands} bands");
            }
        }
        check::<u8>((1, 1), 8);
        check::<u8>((1, 0), 8);
        check::<u16>((1, 1), 10);
        check::<u16>((1, 0), 12);
    }

    /// How much the bands save, measured: a 1920x1080 8-bit 4:2:0 frame
    /// converted on one thread and in bands on every core, back to back, the
    /// fastest of five each -- the ratio holds on a busy machine where the
    /// absolute times do not. Run with `--release --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement: run with --release --ignored --nocapture"]
    fn bench_bands() {
        let (width, height) = (1920usize, 1080usize);
        let fill = |w: usize, h: usize| PlaneBuf::<u8> {
            width: w,
            height: h,
            samples: (0..w * h).map(|i| (i * 131 % 251) as u8).collect(),
        };
        let (y, u, v) = (fill(width, height), fill(960, 540), fill(960, 540));
        let picture = reformat::Picture {
            width,
            height,
            depth: 8,
            format: Format::Yuv420,
            matrix: 1,
            primaries: 1,
            full_range: false,
            y: y.view(),
            u: Some(u.view()),
            v: Some(v.view()),
            alpha: None,
            alpha_premultiplied: false,
        };
        let mut out = Vec::new();
        let mut time = |bands: usize| {
            (0..5)
                .map(|_| {
                    let start = std::time::Instant::now();
                    split(&picture, &mut out, bands).unwrap();
                    start.elapsed().as_secs_f64() * 1000.0
                })
                .fold(f64::MAX, f64::min)
        };
        let one = time(1);
        let many = time(cores().min(width * height / MIN_BAND_PIXELS));
        println!(
            "1920x1080 4:2:0: {one:.2} ms on one thread, {many:.2} ms in bands on {} cores",
            cores()
        );
    }
}
