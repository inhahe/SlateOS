//! Which way up a picture is shown: a file's display matrix -- MP4's, the
//! track's after the movie's (`gui/video/mp4`); Matroska's, built from its
//! projection's pose as FFmpeg builds it (`gui/video/matroska`) -- read as one
//! of the eight ways of turning and mirroring a picture, as ffmpeg and ffplay read it (`fftools/cmdutils.c`'s
//! `get_rotation`, and the filters `ffmpeg_filter.c` and `ffplay.c` put in for
//! it), and applied to each converted frame.
//!
//! A matrix that turns by other than a quarter turn is not obeyed: ffmpeg
//! turns such a picture with its `rotate` filter, filling the corners with
//! black, and mpv shows it as it is, as this does.

use crate::Frame;

/// One of the eight ways a picture can be turned and mirrored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// As it is.
    #[default]
    Upright,
    /// Turned a quarter turn clockwise (ffmpeg's `transpose=clock`).
    Clockwise,
    /// Turned half round (`hflip`, then `vflip`).
    HalfTurn,
    /// Turned a quarter turn anticlockwise (`transpose=cclock`).
    Anticlockwise,
    /// Mirrored, left for right (`hflip`).
    Mirrored,
    /// Flipped, top for bottom (`vflip`).
    Flipped,
    /// Mirrored across the diagonal from its top left corner, rows becoming
    /// columns (`transpose=cclock_flip`).
    Transposed,
    /// Mirrored across the other diagonal (`transpose=clock_flip`).
    AntiTransposed,
}

impl Orientation {
    /// Whether it swaps a picture's width and height.
    pub fn swaps_sides(self) -> bool {
        matches!(
            self,
            Self::Clockwise | Self::Anticlockwise | Self::Transposed | Self::AntiTransposed
        )
    }

    /// What ffmpeg makes of display matrix `m` (row by row, 16.16 fixed
    /// point but for the third column): its angle, rounded to a degree and
    /// brought into [0, 360) (`get_rotation`), and then, within a degree of
    /// a quarter turn, the filters ffmpeg puts in -- whose mirroring it reads
    /// from the signs of the matrix's terms.
    pub(crate) fn from_matrix(m: &[i32; 9]) -> Self {
        let [a, _, _, c, d, ..] = *m;
        let theta = angle(m);
        let near = |degrees: f64| (theta - degrees).abs() < 1.0;
        if near(90.0) {
            if c > 0 {
                Self::Transposed
            } else {
                Self::Clockwise
            }
        } else if near(180.0) {
            match (a < 0, d < 0) {
                (true, true) => Self::HalfTurn,
                (true, false) => Self::Mirrored,
                (false, true) => Self::Flipped,
                (false, false) => Self::Upright,
            }
        } else if near(270.0) {
            if c < 0 {
                Self::AntiTransposed
            } else {
                Self::Anticlockwise
            }
        } else if theta.abs() < 1.0 && d < 0 {
            // Upright but for its rows: ffmpeg's `vflip`. (An angle further
            // from 0 is ffmpeg's `rotate`, not done here; and a matrix that
            // gives no angle at all is left alone, as there.)
            Self::Flipped
        } else {
            Self::Upright
        }
    }

    /// `frame` turned and mirrored this way.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "every coordinate is below the frame's width or height, both at least 1 and their product the pixel count, checked first"
    )]
    pub(crate) fn apply(self, frame: Frame) -> Frame {
        if self == Self::Upright {
            return frame;
        }
        let (Ok(w), Ok(h)) = (usize::try_from(frame.width), usize::try_from(frame.height)) else {
            return frame;
        };
        if frame.pixels.len() != w.saturating_mul(h) || w == 0 || h == 0 {
            return frame;
        }
        let (out_w, out_h) = if self.swaps_sides() { (h, w) } else { (w, h) };
        // The last column and row of the picture as it was.
        let (lx, ly) = (w - 1, h - 1);
        let mut pixels = Vec::with_capacity(frame.pixels.len());
        for y in 0..out_h {
            for x in 0..out_w {
                // Where the pixel at (x, y) of the result comes from.
                let (sx, sy) = match self {
                    Self::Upright => (x, y),
                    Self::Clockwise => (y, ly - x),
                    Self::HalfTurn => (lx - x, ly - y),
                    Self::Anticlockwise => (lx - y, x),
                    Self::Mirrored => (lx - x, y),
                    Self::Flipped => (x, ly - y),
                    Self::Transposed => (y, x),
                    Self::AntiTransposed => (lx - y, ly - x),
                };
                pixels.push(frame.pixels.get(sy * w + sx).copied().unwrap_or(0));
            }
        }
        let (Ok(width), Ok(height)) = (u32::try_from(out_w), u32::try_from(out_h)) else {
            return frame;
        };
        Frame {
            width,
            height,
            pixels,
            ..frame
        }
    }
}

/// `get_rotation`: the matrix's angle (`av_display_rotation_get`, negated),
/// rounded to a whole degree and brought into [0, 360); NaN for a matrix
/// that squashes an axis to nothing.
fn angle(m: &[i32; 9]) -> f64 {
    let [a, b, _, c, d, ..] = m.map(|v| f64::from(v) / 65536.0);
    let (scale_x, scale_y) = (a.hypot(c), b.hypot(d));
    if scale_x == 0.0 || scale_y == 0.0 {
        return f64::NAN;
    }
    let rotation = (b / scale_y).atan2(a / scale_x).to_degrees();
    // av_display_rotation_get gives -rotation, and get_rotation negates its
    // rounding: round(-rotation), negated.
    let theta = -(-rotation).round();
    theta - 360.0 * (theta / 360.0 + 0.9 / 360.0).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: i32 = 1 << 16;
    const W: i32 = 1 << 30;

    fn matrix(a: i32, b: i32, c: i32, d: i32) -> [i32; 9] {
        [a, b, 0, c, d, 0, 0, 0, W]
    }

    #[test]
    fn ffmpegs_own_matrices_read_as_ffmpeg_turns_them() {
        use Orientation::*;
        // What `ffmpeg -display_rotation 90` writes is a quarter turn
        // anticlockwise; -90 clockwise; 180 a half turn.
        assert_eq!(
            Orientation::from_matrix(&matrix(0, -ONE, ONE, 0)),
            Anticlockwise
        );
        assert_eq!(
            Orientation::from_matrix(&matrix(0, ONE, -ONE, 0)),
            Clockwise
        );
        assert_eq!(
            Orientation::from_matrix(&matrix(-ONE, 0, 0, -ONE)),
            HalfTurn
        );
        // -display_hflip, and a quarter turn with it.
        assert_eq!(Orientation::from_matrix(&matrix(-ONE, 0, 0, ONE)), Mirrored);
        assert_eq!(
            Orientation::from_matrix(&matrix(0, -ONE, -ONE, 0)),
            AntiTransposed
        );
        assert_eq!(
            Orientation::from_matrix(&matrix(0, ONE, ONE, 0)),
            Transposed
        );
        // A vertical flip alone.
        assert_eq!(Orientation::from_matrix(&matrix(ONE, 0, 0, -ONE)), Flipped);
        // The identity, a stretch, nothing at all, and an eighth of a turn.
        assert_eq!(Orientation::from_matrix(&matrix(ONE, 0, 0, ONE)), Upright);
        assert_eq!(
            Orientation::from_matrix(&matrix(2 * ONE, 0, 0, ONE)),
            Upright
        );
        assert_eq!(Orientation::from_matrix(&[0; 9]), Upright);
        assert_eq!(
            Orientation::from_matrix(&matrix(46341, -46341, 46341, 46341)),
            Upright
        );
    }

    /// A 3 x 2 frame of pixels 0..6, row by row.
    fn frame() -> Frame {
        Frame {
            time: 0,
            duration: 0,
            keyframe: true,
            width: 3,
            height: 2,
            pixels: (0..6).collect(),
        }
    }

    #[test]
    fn each_orientation_moves_the_pixels_it_says() {
        use Orientation::*;
        //   0 1 2
        //   3 4 5
        let cases: [(Orientation, u32, &[u32]); 8] = [
            (Upright, 3, &[0, 1, 2, 3, 4, 5]),
            (Clockwise, 2, &[3, 0, 4, 1, 5, 2]),
            (HalfTurn, 3, &[5, 4, 3, 2, 1, 0]),
            (Anticlockwise, 2, &[2, 5, 1, 4, 0, 3]),
            (Mirrored, 3, &[2, 1, 0, 5, 4, 3]),
            (Flipped, 3, &[3, 4, 5, 0, 1, 2]),
            (Transposed, 2, &[0, 3, 1, 4, 2, 5]),
            (AntiTransposed, 2, &[5, 2, 4, 1, 3, 0]),
        ];
        for (o, width, pixels) in cases {
            let f = o.apply(frame());
            assert_eq!((f.width, f.pixels.as_slice()), (width, pixels), "{o:?}");
            assert_eq!(f.width * f.height, 6);
            assert_eq!(o.swaps_sides(), width == 2, "{o:?}");
        }
    }
}
