//! A program that draws the desktop's background, and the desktop: what the
//! program writes, and what the desktop tells it.
//!
//! `design.txt` asks for a background that is "not just a video, but a
//! program constantly changing it, with various input events such as
//! anything that's happening on the desktop". Both are one thing here: the
//! desktop starts a background program, shows each picture it writes, and
//! tells it what happens on the desktop. A video is the background program
//! `wallvideo`, which plays a file; any other program speaking this is
//! a background too (`design-decisions.md` §1489).
//!
//! # The pictures
//!
//! The program writes pictures on its standard output, one after another,
//! each the whole background ([`write_frame`]): a 16-byte head -- `SBG1`,
//! the width and the height as little-endian `u32`s, and a `u32` that is
//! nought -- then `width * height` pixels, four bytes each, `0xAARRGGBB` as a
//! little-endian `u32` (the compositor's `Argb8888`, `imagecodec`'s and
//! `videocodec`'s pixels). It writes a picture when it should be shown; the
//! desktop shows the newest it has read ([`read_frame`]). A picture larger
//! than [`MAX_PIXELS`] ends the program's background, as a picture file that
//! large is refused.
//!
//! # The events
//!
//! The desktop writes events on the program's standard input, a line each
//! ([`Event::line`], [`Event::parse`]). Words, not a binary record, so a
//! background can be a script:
//!
//! | Line | Says |
//! |---|---|
//! | `size W H` | the background is `W` by `H` pixels: draw at that size (first, before anything else, and on every change) |
//! | `pause` | nobody can see the background: drawing is wasted |
//! | `resume` | it is seen again |
//! | `pointer X Y` | the pointer is over the background at `(X, Y)` |
//! | `desktop N` | virtual desktop `N` (from 0) is the one shown |
//! | `windows X,Y,W,H ...` | where the windows on the shown desktop are, topmost first |
//! | `theme dark RRGGBB` / `theme light RRGGBB` | the desktop's mode and accent |
//!
//! **What a background is not told** is as deliberate: no window's title, no
//! program's name, nothing typed. A background program is a program the
//! user installed to look at, and nothing about what they are doing in
//! their windows is its business. Where the windows are is enough for a
//! background that moves out of their way. A line a program does not know
//! it should pass over: events may be added.

use std::io::{self, Read, Write};

/// The four bytes a picture starts with.
pub const MAGIC: [u8; 4] = *b"SBG1";

/// The picture's head: the magic, the width, the height, and a reserved
/// word.
pub const HEAD_LEN: usize = 16;

/// The most pixels a picture may have: the compositor's largest buffer,
/// 7680 by 4320, as for a picture file.
pub const MAX_PIXELS: u64 = 7680 * 4320;

/// A picture a background program wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Its width in pixels; never zero.
    pub width: u32,
    /// Its height in pixels; never zero.
    pub height: u32,
    /// `width * height` pixels, row by row, `0xAARRGGBB`.
    pub pixels: Vec<u32>,
}

/// Why a picture could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// Reading failed.
    Io(io::ErrorKind),
    /// It did not start with [`MAGIC`]: not a background program's output.
    NotAPicture,
    /// A width or a height of nought.
    Empty,
    /// More than [`MAX_PIXELS`].
    TooLarge {
        /// The width it said.
        width: u32,
        /// The height it said.
        height: u32,
    },
    /// The stream ended inside a picture.
    CutShort,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "its pictures could not be read ({kind})"),
            Self::NotAPicture => f.write_str("it wrote something that is not a picture"),
            Self::Empty => f.write_str("it wrote a picture of nothing"),
            Self::TooLarge { width, height } => {
                write!(
                    f,
                    "it wrote a picture of {width} by {height}, larger than a screen"
                )
            }
            Self::CutShort => f.write_str("it stopped in the middle of a picture"),
        }
    }
}

impl std::error::Error for FrameError {}

/// How many pixels `width` by `height` is, if a picture may be that size.
fn pixel_count(width: u32, height: u32) -> Result<usize, FrameError> {
    if width == 0 || height == 0 {
        return Err(FrameError::Empty);
    }
    let count = u64::from(width).saturating_mul(u64::from(height));
    if count > MAX_PIXELS {
        return Err(FrameError::TooLarge { width, height });
    }
    usize::try_from(count).map_err(|_| FrameError::TooLarge { width, height })
}

/// Write one picture, `width` by `height` -- `pixels` holding exactly that
/// many, `0xAARRGGBB` -- to `out`.
///
/// # Errors
///
/// `InvalidInput` for a size no picture may have or pixels that do not fill
/// it; otherwise as `out`'s write.
pub fn write_frame(
    out: &mut impl Write,
    width: u32,
    height: u32,
    pixels: &[u32],
) -> io::Result<()> {
    let count = pixel_count(width, height)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    if pixels.len() != count {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the pixels do not fill the picture",
        ));
    }
    let mut head = [0u8; HEAD_LEN];
    for (to, from) in head.iter_mut().zip(
        MAGIC
            .iter()
            .chain(width.to_le_bytes().iter())
            .chain(height.to_le_bytes().iter()),
    ) {
        *to = *from;
    }
    out.write_all(&head)?;
    let mut bytes = Vec::with_capacity(count.saturating_mul(4));
    for px in pixels {
        bytes.extend_from_slice(&px.to_le_bytes());
    }
    out.write_all(&bytes)
}

/// Read the next picture from `input`: `None` where the stream ends between
/// two pictures, as a program that has stopped drawing ends it.
///
/// # Errors
///
/// A stream that is not pictures, a picture too large or empty, one cut
/// short, or a read that failed.
pub fn read_frame(input: &mut impl Read) -> Result<Option<Frame>, FrameError> {
    let mut head = [0u8; HEAD_LEN];
    if !fill(input, &mut head)? {
        return Ok(None);
    }
    if head.get(..4) != Some(&MAGIC[..]) {
        return Err(FrameError::NotAPicture);
    }
    let word = |at: usize| {
        head.get(at..at.saturating_add(4))
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0, u32::from_le_bytes)
    };
    let (width, height) = (word(4), word(8));
    let count = pixel_count(width, height)?;
    let mut bytes = vec![0u8; count.saturating_mul(4)];
    if !fill(input, &mut bytes)? {
        return Err(FrameError::CutShort);
    }
    let pixels = bytes
        .chunks_exact(4)
        .map(|c| <[u8; 4]>::try_from(c).map_or(0, u32::from_le_bytes))
        .collect();
    Ok(Some(Frame {
        width,
        height,
        pixels,
    }))
}

/// Fill `buf` from `input`: `false` where the stream ended before the first
/// byte of it.
///
/// # Errors
///
/// [`FrameError::CutShort`] where it ended part of the way, or the read's
/// own failure.
fn fill(input: &mut impl Read, buf: &mut [u8]) -> Result<bool, FrameError> {
    let mut got = 0;
    while got < buf.len() {
        let Some(rest) = buf.get_mut(got..) else {
            break;
        };
        match input.read(rest) {
            Ok(0) if got == 0 => return Ok(false),
            Ok(0) => return Err(FrameError::CutShort),
            Ok(n) => got = got.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FrameError::Io(e.kind())),
        }
    }
    Ok(true)
}

/// Where a window is, in the background's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Its left edge.
    pub x: i32,
    /// Its top edge.
    pub y: i32,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
}

/// Something the desktop tells a background program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The background is this many pixels: draw at this size.
    Size {
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
    },
    /// Nobody can see the background.
    Pause,
    /// It is seen again.
    Resume,
    /// The pointer is over the background here.
    Pointer {
        /// Across, from the left.
        x: i32,
        /// Down, from the top.
        y: i32,
    },
    /// The virtual desktop shown, counting from nought.
    Desktop(u32),
    /// Where the windows on the shown desktop are, topmost first.
    Windows(Vec<Rect>),
    /// The desktop's mode and accent colour (`0xRRGGBB`).
    Theme {
        /// Whether the mode is dark.
        dark: bool,
        /// The accent, `0xRRGGBB`.
        accent: u32,
    },
}

impl Event {
    /// It as the line the desktop writes, without its newline.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            Self::Size { width, height } => format!("size {width} {height}"),
            Self::Pause => "pause".to_owned(),
            Self::Resume => "resume".to_owned(),
            Self::Pointer { x, y } => format!("pointer {x} {y}"),
            Self::Desktop(n) => format!("desktop {n}"),
            Self::Windows(rects) => {
                let mut line = "windows".to_owned();
                for r in rects {
                    line.push_str(&format!(" {},{},{},{}", r.x, r.y, r.width, r.height));
                }
                line
            }
            Self::Theme { dark, accent } => format!(
                "theme {} {:06x}",
                if *dark { "dark" } else { "light" },
                accent & 0x00FF_FFFF
            ),
        }
    }

    /// The event a line says, or `None` for one this does not know -- which
    /// a program passes over, as events may be added.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let mut words = line.split_ascii_whitespace();
        let event = match words.next()? {
            "size" => Self::Size {
                width: words.next()?.parse().ok()?,
                height: words.next()?.parse().ok()?,
            },
            "pause" => Self::Pause,
            "resume" => Self::Resume,
            "pointer" => Self::Pointer {
                x: words.next()?.parse().ok()?,
                y: words.next()?.parse().ok()?,
            },
            "desktop" => Self::Desktop(words.next()?.parse().ok()?),
            "windows" => {
                let mut rects = Vec::new();
                for word in words.by_ref() {
                    let mut parts = word.split(',');
                    rects.push(Rect {
                        x: parts.next()?.parse().ok()?,
                        y: parts.next()?.parse().ok()?,
                        width: parts.next()?.parse().ok()?,
                        height: parts.next()?.parse().ok()?,
                    });
                    if parts.next().is_some() {
                        return None;
                    }
                }
                Self::Windows(rects)
            }
            "theme" => Self::Theme {
                dark: match words.next()? {
                    "dark" => true,
                    "light" => false,
                    _ => return None,
                },
                accent: u32::from_str_radix(words.next()?, 16)
                    .ok()
                    .filter(|a| *a <= 0x00FF_FFFF)?,
            },
            _ => return None,
        };
        // A line with words left over is not this event.
        words.next().is_none().then_some(event)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
