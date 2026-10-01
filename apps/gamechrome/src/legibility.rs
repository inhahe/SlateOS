//! Every text a window draws, read against what is drawn under it.
//!
//! A game's palette test proves its colours are the user's; it cannot prove
//! a colour is legible where it lands. The palette's inks are made for the
//! page (`Palette::ink`), and a game writes on much else -- a selected square,
//! a card, a tinted cell -- which in a light theme is darker than the page and
//! in a dark one lighter. Sudoku's hints and clashes fell to 2.5:1 on its
//! selected square that way, and nothing that looked only at colours noticed.
//!
//! [`reads`] walks a frame's commands as the renderer paints them -- fills
//! stacked in order, each clipped to the clip in force and moved by the
//! translation in force -- and, for every text, composites the fills under
//! the middle of its line over the window's page. [`illegible`] holds each to
//! the WCAG floor for its size (1.4.3: 4.5:1, or 3:1 for large text) and
//! returns what falls short.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::text;
use guitk::theme::contrast_ratio;

/// One text as drawn, with the colour under the middle of its line.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    /// What it says.
    pub text: String,
    /// The colour it is written in.
    pub ink: Color,
    /// What is under its middle: the fills there, composited in the order
    /// they were painted over the page.
    pub ground: Color,
    /// Its size, in pixels.
    pub size: f32,
    /// Whether it is bold, which lowers the size WCAG calls large.
    pub bold: bool,
}

/// Ordinary text's contrast floor (WCAG 1.4.3).
pub const TEXT_FLOOR: f32 = 4.5;

/// Large text's contrast floor (WCAG 1.4.3).
pub const LARGE_TEXT_FLOOR: f32 = 3.0;

/// Whether WCAG counts text at `size` pixels as large -- 18pt, or 14pt bold,
/// which at 96 pixels to the inch is 24px, or 18.66px bold.
#[must_use]
pub fn is_large(size: f32, bold: bool) -> bool {
    size >= 24.0 || (bold && size >= 18.66)
}

impl Read {
    /// Whether WCAG counts this as large text ([`is_large`]).
    #[must_use]
    pub fn is_large(&self) -> bool {
        is_large(self.size, self.bold)
    }

    /// The floor this text is held to: 3:1 when large, 4.5:1 otherwise.
    #[must_use]
    pub fn floor(&self) -> f32 {
        if self.is_large() {
            LARGE_TEXT_FLOOR
        } else {
            TEXT_FLOOR
        }
    }

    /// Its contrast against its ground.
    #[must_use]
    pub fn ratio(&self) -> f32 {
        contrast_ratio(self.ink, self.ground)
    }
}

/// A rectangle in window coordinates.
#[derive(Clone, Copy, Debug)]
struct Area {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Area {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = (self.y + self.h).min(other.y + other.h);
        Self {
            x,
            y,
            w: (right - x).max(0.0),
            h: (bottom - y).max(0.0),
        }
    }
}

/// What is painted at a point: a colour, or a picture whose colour a test
/// cannot know.
#[derive(Clone, Copy, Debug)]
enum Paint {
    Fill(Color),
    Picture,
}

/// Every text in `cmds`, read against what is painted under the middle of its
/// line, over `page` -- the colour under everything. A text over a picture is
/// left out: what is under it is not a colour.
#[must_use]
pub fn reads(cmds: &[RenderCommand], page: Color) -> Vec<Read> {
    let mut painted: Vec<(Area, Paint)> = Vec::new();
    let mut offsets: Vec<(f32, f32)> = vec![(0.0, 0.0)];
    let everything = Area {
        x: f32::MIN / 4.0,
        y: f32::MIN / 4.0,
        w: f32::MAX / 2.0,
        h: f32::MAX / 2.0,
    };
    let mut clips: Vec<Area> = vec![everything];
    let mut out = Vec::new();
    for cmd in cmds {
        let (dx, dy) = offsets.last().copied().unwrap_or((0.0, 0.0));
        let clip = clips.last().copied().unwrap_or(everything);
        match cmd {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } => {
                let area = Area {
                    x: x + dx,
                    y: y + dy,
                    w: *width,
                    h: *height,
                }
                .intersect(clip);
                painted.push((area, Paint::Fill(*color)));
            }
            RenderCommand::Image {
                x,
                y,
                width,
                height,
                ..
            } => {
                let area = Area {
                    x: x + dx,
                    y: y + dy,
                    w: *width,
                    h: *height,
                }
                .intersect(clip);
                painted.push((area, Paint::Picture));
            }
            RenderCommand::Text {
                x,
                y,
                text: s,
                color,
                font_size,
                font_weight,
                max_width,
                ..
            }
            | RenderCommand::RichText {
                x,
                y,
                text: s,
                color,
                font_size,
                font_weight,
                max_width,
                ..
            } => {
                if s.is_empty() || *font_size <= 0.0 {
                    continue;
                }
                let wide = text::measure(s, *font_size, *font_weight);
                let wide = max_width.map_or(wide, |room| wide.min(room.max(0.0)));
                let mid_x = x + dx + wide / 2.0;
                let mid_y = y + dy + text::line_height(*font_size, *font_weight) / 2.0;
                if !clip.contains(mid_x, mid_y) {
                    // Clipped away: nothing is read.
                    continue;
                }
                let mut ground = Some(page);
                for (area, paint) in &painted {
                    if !area.contains(mid_x, mid_y) {
                        continue;
                    }
                    ground = match paint {
                        // An opaque fill is the ground, whatever was under it.
                        Paint::Fill(c) if c.a == u8::MAX => Some(*c),
                        Paint::Fill(c) => ground.map(|g| c.over(g)),
                        // An opaque picture hides what was under it, and what
                        // is painted over it lands on something unknown.
                        Paint::Picture => None,
                    };
                }
                if let Some(ground) = ground {
                    out.push(Read {
                        text: s.clone(),
                        ink: *color,
                        ground,
                        size: *font_size,
                        bold: *font_weight == FontWeightHint::Bold,
                    });
                }
            }
            RenderCommand::PushClip {
                x,
                y,
                width,
                height,
            } => clips.push(
                Area {
                    x: x + dx,
                    y: y + dy,
                    w: *width,
                    h: *height,
                }
                .intersect(clip),
            ),
            RenderCommand::PopClip => {
                if clips.len() > 1 {
                    clips.pop();
                }
            }
            RenderCommand::PushTranslate { dx: tx, dy: ty } => offsets.push((dx + tx, dy + ty)),
            RenderCommand::PopTranslate => {
                if offsets.len() > 1 {
                    offsets.pop();
                }
            }
            _ => {}
        }
    }
    out
}

/// The texts in `cmds` that fall below the WCAG floor for their size on what
/// is under them, over `page`, leaving out any whose ink `exempt` says is
/// exempt -- a switched-off control's label, which WCAG 1.4.3 exempts and a
/// palette deliberately draws faint.
#[must_use]
pub fn illegible(cmds: &[RenderCommand], page: Color, exempt: impl Fn(&Read) -> bool) -> Vec<Read> {
    reads(cmds, page)
        .into_iter()
        .filter(|r| !exempt(r) && r.ratio() < r.floor())
        .collect()
}

#[cfg(test)]
// A test that panics on bad data is a test reporting a fault.
#[allow(clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::*;
    use guitk::render::TextOverflow;
    use guitk::style::CornerRadii;

    const PAGE: Color = Color::from_hex(0xFFFFFF);

    fn fill(x: f32, y: f32, w: f32, h: f32, color: Color) -> RenderCommand {
        RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color,
            corner_radii: CornerRadii::ZERO,
        }
    }

    fn text(x: f32, y: f32, s: &str, color: Color, size: f32) -> RenderCommand {
        RenderCommand::Text {
            x,
            y,
            text: s.to_owned(),
            color,
            font_size: size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Ellipsis,
        }
    }

    #[test]
    fn a_text_is_read_against_the_fill_under_it_or_the_page() {
        let grey = Color::from_hex(0x777777);
        let cmds = [
            fill(0.0, 0.0, 100.0, 40.0, grey),
            text(4.0, 4.0, "on the fill", PAGE, 12.0),
            text(4.0, 60.0, "on the page", grey, 12.0),
        ];
        let read = reads(&cmds, PAGE);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].ground, grey);
        assert_eq!(read[1].ground, PAGE);
    }

    #[test]
    fn a_text_is_read_at_the_middle_of_its_line_not_its_corner() {
        // The fill starts a little right of the text and below its top: the
        // corner the text is placed by is on the page, its middle on the fill.
        let grey = Color::from_hex(0x777777);
        let size = 20.0;
        let wide = text::measure("middle", size, FontWeightHint::Regular);
        let tall = text::line_height(size, FontWeightHint::Regular);
        let cmds = [
            fill(10.0 + wide * 0.25, 10.0 + tall * 0.25, 200.0, 200.0, grey),
            text(10.0, 10.0, "middle", PAGE, size),
        ];
        assert_eq!(reads(&cmds, PAGE)[0].ground, grey);
    }

    #[test]
    fn a_see_through_fill_is_composited_over_what_it_covers() {
        let veil = Color::rgba(0, 0, 0, 128);
        let cmds = [
            fill(0.0, 0.0, 100.0, 100.0, veil),
            text(4.0, 4.0, "x", PAGE, 12.0),
        ];
        let read = reads(&cmds, PAGE);
        assert_eq!(read[0].ground, veil.over(PAGE));
        assert_ne!(read[0].ground, PAGE);
    }

    #[test]
    fn a_translation_moves_fills_and_texts_alike() {
        let grey = Color::from_hex(0x777777);
        let cmds = [
            RenderCommand::PushTranslate { dx: 200.0, dy: 0.0 },
            fill(0.0, 0.0, 100.0, 40.0, grey),
            RenderCommand::PopTranslate,
            // At x 4 there is nothing: the fill was moved to x 200.
            text(4.0, 4.0, "left", PAGE, 12.0),
            text(204.0, 4.0, "right", PAGE, 12.0),
        ];
        let read = reads(&cmds, PAGE);
        assert_eq!(read[0].ground, PAGE, "the fill was not moved");
        assert_eq!(
            read[1].ground, grey,
            "the fill was not found where it was moved"
        );
    }

    #[test]
    fn a_fill_clipped_away_is_not_under_anything() {
        let grey = Color::from_hex(0x777777);
        let cmds = [
            RenderCommand::PushClip {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 20.0,
            },
            fill(0.0, 0.0, 100.0, 100.0, grey),
            RenderCommand::PopClip,
            text(4.0, 50.0, "below the clip", Color::from_hex(0x000000), 12.0),
        ];
        let read = reads(&cmds, PAGE);
        assert_eq!(read[0].ground, PAGE);
    }

    #[test]
    fn a_text_over_a_picture_is_not_read() {
        let cmds = [
            RenderCommand::Image {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                image_id: 1,
            },
            text(4.0, 4.0, "caption", PAGE, 12.0),
        ];
        assert!(reads(&cmds, PAGE).is_empty());
        // An opaque fill over the picture is a ground again; a see-through
        // one is not.
        let grey = Color::from_hex(0x777777);
        let mut cmds = cmds.to_vec();
        cmds.push(fill(0.0, 0.0, 50.0, 50.0, grey));
        cmds.push(text(4.0, 4.0, "on the fill", PAGE, 12.0));
        cmds.push(fill(60.0, 0.0, 40.0, 50.0, Color::rgba(0, 0, 0, 100)));
        cmds.push(text(62.0, 4.0, "veiled", PAGE, 8.0));
        let read = reads(&cmds, PAGE);
        assert_eq!(read.len(), 1, "{read:?}");
        assert_eq!(read[0].ground, grey);
    }

    #[test]
    fn a_faint_text_is_illegible_unless_exempt_and_large_text_has_the_lower_floor() {
        let pale = Color::from_hex(0x999999);
        // 2.85:1 on white: below both floors.
        let faint = [text(4.0, 4.0, "faint", pale, 12.0)];
        assert_eq!(illegible(&faint, PAGE, |_| false).len(), 1);
        assert!(illegible(&faint, PAGE, |r| r.ink == pale).is_empty());
        // 3.5:1 on white: fails small, passes large.
        let mid = Color::from_hex(0x888888);
        let small = [text(4.0, 4.0, "small", mid, 12.0)];
        let large = [text(4.0, 4.0, "large", mid, 30.0)];
        assert_eq!(illegible(&small, PAGE, |_| false).len(), 1);
        assert!(illegible(&large, PAGE, |_| false).is_empty());
    }
}
