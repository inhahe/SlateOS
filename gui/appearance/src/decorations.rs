//! The shape of a window's frame: its title bar, the buttons on it, its
//! border and its shadow -- what a theme's `window-decorations` section sets
//! ([`crate::themes::DecorationTheme`]) and what every window's frame is
//! drawn and clicked from. Its colours are the palette's
//! (`Palette::title_bar` and the rest): this is the frame's geometry.
//!
//! [`DecorationStyle::AERO`] is the built-in theme's -- the frame the
//! compositor has drawn since it drew one: a 30-pixel bar, the title at its
//! left, minimise, maximise and close at its right end, rounded as the
//! window's corners are, a one-pixel border and an eight-pixel shadow.
//!
//! [`DecorationStyle::title_bar`] says where a window's buttons and title go
//! on its bar, so that whatever draws the frame and whatever hit-tests it --
//! and a theme preview drawn in Settings -- put them in the same places.

use guitk::frame::Rect;

/// A button on a window's title bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TitleButton {
    /// Takes the window off the screen, to the taskbar.
    Minimize,
    /// Fills the screen with it, or puts it back.
    Maximize,
    /// Asks it to close.
    Close,
}

impl TitleButton {
    /// Every button, in the built-in order.
    pub const ALL: [Self; 3] = [Self::Minimize, Self::Maximize, Self::Close];

    /// Its name in a theme's `buttons.order`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Close => "close",
        }
    }
}

/// Which end of the title bar the buttons are at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSide {
    /// The left end, the title after them.
    Left,
    /// The right end.
    Right,
}

/// What a title bar's buttons look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonShape {
    /// Rounded as the window's corners are -- the built-in look.
    Rounded,
    /// Circles.
    Circle,
    /// Square-cornered.
    Square,
    /// No face: each button's mark alone, lit under the pointer.
    Glyph,
}

/// Where a window's title is written on its bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleAlign {
    /// At the start of the room the buttons leave.
    Left,
    /// In the middle of the bar -- moved over as far as it must be to keep
    /// clear of the buttons.
    Center,
}

/// A window frame's shape. Sizes are pixels at a display scale of 1; see
/// [`title_bar`](Self::title_bar) for the scaled ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecorationStyle {
    /// How tall the title bar is: [`Self::MIN_TITLE_HEIGHT`] to
    /// [`Self::MAX_TITLE_HEIGHT`].
    pub title_height: u16,
    /// Where the title is written.
    pub title_align: TitleAlign,
    /// Whether the title is bold.
    pub title_bold: bool,
    /// Which end the buttons are at.
    pub button_side: ButtonSide,
    /// The buttons, left to right as drawn, each once.
    pub buttons: [TitleButton; 3],
    /// What they look like.
    pub button_shape: ButtonShape,
    /// How big each is, square: [`Self::MIN_BUTTON_SIZE`] to
    /// [`Self::MAX_BUTTON_SIZE`].
    pub button_size: u16,
    /// The room between neighbouring buttons, and between the end button and
    /// the bar's end: 0 to [`Self::MAX_BUTTON_GAP`].
    pub button_gap: u16,
    /// How wide the frame's border is: 0 to [`Self::MAX_BORDER`].
    pub border: u16,
    /// How far the shadow reaches past the frame: 0 to [`Self::MAX_SHADOW`].
    pub shadow: u16,
}

impl Default for DecorationStyle {
    fn default() -> Self {
        Self::AERO
    }
}

impl DecorationStyle {
    /// The built-in theme's frame: the one the compositor has always drawn.
    pub const AERO: Self = Self {
        title_height: 30,
        title_align: TitleAlign::Left,
        title_bold: false,
        button_side: ButtonSide::Right,
        buttons: TitleButton::ALL,
        button_shape: ButtonShape::Rounded,
        button_size: 20,
        button_gap: 4,
        border: 1,
        shadow: 8,
    };

    /// The shortest a title bar may be.
    pub const MIN_TITLE_HEIGHT: u16 = 20;
    /// The tallest.
    pub const MAX_TITLE_HEIGHT: u16 = 56;
    /// The smallest a button may be.
    pub const MIN_BUTTON_SIZE: u16 = 12;
    /// The largest.
    pub const MAX_BUTTON_SIZE: u16 = 40;
    /// The most room between buttons.
    pub const MAX_BUTTON_GAP: u16 = 16;
    /// The widest border.
    pub const MAX_BORDER: u16 = 8;
    /// The furthest-reaching shadow.
    pub const MAX_SHADOW: u16 = 48;

    /// The gap between the title and the end of the bar it starts from, at a
    /// scale of 1 -- the compositor's `TITLE_TEXT_INSET`.
    pub const TITLE_INSET: f32 = 8.0;

    /// Where the buttons and the title go on a window's title bar `bar`, at
    /// the display's `scale`, for a window with the buttons `has` says it has
    /// (one that cannot be resized has no maximise). The buttons it has are
    /// packed in this style's order at its end of the bar, without a gap
    /// where one it lacks would be, each centred down the bar; the title gets
    /// the rest, less an inset at the end it starts from.
    #[must_use]
    pub fn title_bar(
        &self,
        bar: Rect,
        scale: f32,
        has: impl Fn(TitleButton) -> bool,
    ) -> TitleBarGeometry {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let size = f32::from(
            self.button_size
                .clamp(Self::MIN_BUTTON_SIZE, Self::MAX_BUTTON_SIZE),
        ) * scale;
        let gap = f32::from(self.button_gap.min(Self::MAX_BUTTON_GAP)) * scale;
        let present: Vec<TitleButton> = self.buttons.iter().copied().filter(|b| has(*b)).collect();
        #[allow(clippy::cast_precision_loss, reason = "three buttons at most")]
        let count = present.len() as f32;
        // The run of buttons: each a size, a gap between each and before the
        // first from the bar's end.
        let run = if present.is_empty() {
            0.0
        } else {
            count * size + count * gap
        };
        let top = bar.y + (bar.h - size) / 2.0;
        let start = match self.button_side {
            ButtonSide::Right => bar.x + bar.w - run,
            ButtonSide::Left => bar.x + gap,
        };
        let buttons = present
            .iter()
            .enumerate()
            .map(|(i, &button)| {
                #[allow(clippy::cast_precision_loss, reason = "three buttons at most")]
                let i = i as f32;
                (button, Rect::new(start + i * (size + gap), top, size, size))
            })
            .collect();
        let inset = Self::TITLE_INSET * scale;
        let title = match self.button_side {
            ButtonSide::Right => Rect::new(
                bar.x + inset,
                bar.y,
                (bar.w - run - inset * 2.0).max(0.0),
                bar.h,
            ),
            ButtonSide::Left => Rect::new(
                bar.x + run + inset,
                bar.y,
                (bar.w - run - inset * 2.0).max(0.0),
                bar.h,
            ),
        };
        TitleBarGeometry {
            bar,
            buttons,
            title,
            align: self.title_align,
        }
    }
}

/// Where a window's buttons and title go on its title bar: what
/// [`DecorationStyle::title_bar`] answers.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleBarGeometry {
    /// The bar.
    pub bar: Rect,
    /// Each button the window has, left to right, and where it is drawn and
    /// clicked.
    pub buttons: Vec<(TitleButton, Rect)>,
    /// The room the title may be written in.
    pub title: Rect,
    /// Where in it the title goes.
    pub align: TitleAlign,
}

impl TitleBarGeometry {
    /// Where the button `button` is, if the window has it.
    #[must_use]
    pub fn button(&self, button: TitleButton) -> Option<Rect> {
        self.buttons
            .iter()
            .find(|(b, _)| *b == button)
            .map(|(_, r)| *r)
    }

    /// The button at `(x, y)`, if one is.
    #[must_use]
    pub fn button_at(&self, x: f32, y: f32) -> Option<TitleButton> {
        self.buttons
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(b, _)| *b)
    }

    /// Where a title `text_width` wide starts: at the start of its room, or
    /// in the middle of the bar -- moved over as far as it must be to stay
    /// in its room. A title wider than its room starts at the room's start,
    /// for the text's end to be cut with a mark.
    #[must_use]
    pub fn title_x(&self, text_width: f32) -> f32 {
        let room = self.title;
        match self.align {
            TitleAlign::Left => room.x,
            TitleAlign::Center => {
                let centred = self.bar.x + (self.bar.w - text_width) / 2.0;
                let last = room.x + room.w - text_width;
                if last < room.x {
                    room.x
                } else {
                    centred.clamp(room.x, last)
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    const BAR: Rect = Rect {
        x: 100.0,
        y: 50.0,
        w: 400.0,
        h: 30.0,
    };

    fn all(_: TitleButton) -> bool {
        true
    }

    /// **The built-in frame puts the buttons where the compositor always
    /// has**: close at the right end, a gap from it, then maximise and
    /// minimise, each 20 square, centred down the bar.
    #[test]
    fn the_built_in_frame_is_the_compositors() {
        let g = DecorationStyle::AERO.title_bar(BAR, 1.0, all);
        assert_eq!(
            g.button(TitleButton::Close),
            Some(Rect::new(476.0, 55.0, 20.0, 20.0))
        );
        assert_eq!(
            g.button(TitleButton::Maximize),
            Some(Rect::new(452.0, 55.0, 20.0, 20.0))
        );
        assert_eq!(
            g.button(TitleButton::Minimize),
            Some(Rect::new(428.0, 55.0, 20.0, 20.0))
        );
        // The title from 8 in, to 8 short of the buttons.
        assert_eq!(g.title, Rect::new(108.0, 50.0, 400.0 - 72.0 - 16.0, 30.0));
    }

    /// **A button a window lacks leaves no gap**: the rest close up.
    #[test]
    fn a_missing_button_leaves_no_gap() {
        let g = DecorationStyle::AERO.title_bar(BAR, 1.0, |b| b != TitleButton::Maximize);
        assert_eq!(g.buttons.len(), 2);
        assert_eq!(g.button(TitleButton::Maximize), None);
        assert_eq!(g.button(TitleButton::Close).unwrap().x, 476.0);
        assert_eq!(g.button(TitleButton::Minimize).unwrap().x, 452.0);
        assert_eq!(g.title.w, 400.0 - 48.0 - 16.0);
    }

    /// **Buttons on the left come first, in their order, the title after
    /// them**; a window with none has the whole bar for its title.
    #[test]
    fn buttons_on_the_left_come_first() {
        let style = DecorationStyle {
            button_side: ButtonSide::Left,
            buttons: [
                TitleButton::Close,
                TitleButton::Minimize,
                TitleButton::Maximize,
            ],
            ..DecorationStyle::AERO
        };
        let g = style.title_bar(BAR, 1.0, all);
        let order: Vec<TitleButton> = g.buttons.iter().map(|(b, _)| *b).collect();
        assert_eq!(
            order,
            [
                TitleButton::Close,
                TitleButton::Minimize,
                TitleButton::Maximize
            ]
        );
        assert_eq!(g.button(TitleButton::Close).unwrap().x, 104.0);
        assert_eq!(g.title.x, 100.0 + 72.0 + 8.0);
        let bare = style.title_bar(BAR, 1.0, |_| false);
        assert!(bare.buttons.is_empty());
        assert_eq!(bare.title, Rect::new(108.0, 50.0, 384.0, 30.0));
    }

    /// **Everything grows with the display's scale**, and a scale that is no
    /// scale (nought, negative, NaN) is taken as 1.
    #[test]
    fn the_frame_grows_with_the_scale() {
        let bar = Rect::new(0.0, 0.0, 800.0, 60.0);
        let g = DecorationStyle::AERO.title_bar(bar, 2.0, all);
        let close = g.button(TitleButton::Close).unwrap();
        assert_eq!((close.w, close.x), (40.0, 800.0 - 48.0));
        assert_eq!(close.y, 10.0);
        for bad in [0.0, -2.0, f32::NAN] {
            let g = DecorationStyle::AERO.title_bar(BAR, bad, all);
            assert_eq!(g.button(TitleButton::Close).unwrap().w, 20.0, "{bad}");
        }
    }

    /// **A centred title sits in the middle of the bar, moved over only as
    /// far as the buttons need**; one too wide for its room starts at it.
    #[test]
    fn a_centred_title_keeps_clear_of_the_buttons() {
        let style = DecorationStyle {
            title_align: TitleAlign::Center,
            ..DecorationStyle::AERO
        };
        let g = style.title_bar(BAR, 1.0, all);
        // 100 wide: the bar's middle is 300, so it starts at 250.
        assert_eq!(g.title_x(100.0), 250.0);
        // 300 wide: centred it would end at 450, past its room's end (420,
        // 8 short of the buttons): moved left to end there.
        assert_eq!(g.title_x(300.0), 420.0 - 300.0);
        // Wider than the room: from its start.
        assert_eq!(g.title_x(1000.0), g.title.x);
        let left = DecorationStyle::AERO.title_bar(BAR, 1.0, all);
        assert_eq!(left.title_x(100.0), 108.0);
    }

    /// **The button under the pointer is found by where it is drawn.**
    #[test]
    fn the_button_under_the_pointer() {
        let g = DecorationStyle::AERO.title_bar(BAR, 1.0, all);
        assert_eq!(g.button_at(486.0, 65.0), Some(TitleButton::Close));
        assert_eq!(g.button_at(474.0, 65.0), None);
        assert_eq!(g.button_at(200.0, 65.0), None);
    }

    /// **Sizes out of range are held to it when laid out**: a style built in
    /// code with a 0 button is drawn at the smallest.
    #[test]
    fn sizes_out_of_range_are_held() {
        let style = DecorationStyle {
            button_size: 0,
            button_gap: 500,
            ..DecorationStyle::AERO
        };
        let g = style.title_bar(Rect::new(0.0, 0.0, 2000.0, 30.0), 1.0, all);
        let close = g.button(TitleButton::Close).unwrap();
        assert_eq!(close.w, f32::from(DecorationStyle::MIN_BUTTON_SIZE));
        assert_eq!(
            2000.0 - (close.x + close.w),
            f32::from(DecorationStyle::MAX_BUTTON_GAP)
        );
    }
}
