//! The frame the shell's own dialogs wear: the theme's window frame.
//!
//! The shell draws some dialogs itself -- the run box first -- and they used
//! to wear a frame of their own, a rounded box with a bold heading, whatever
//! the theme said windows look like. They wear the theme's window frame now
//! ([`DecorationStyle`], `design-decisions.md` §1456 and §1461): its title
//! bar's height, where the title goes, whether it is bold and how a long one
//! is cut, which end the close button is at and its shape and size, the
//! border and the shadow -- in the frame's colours, taken from the palette the
//! dialog is drawn with ([`DecorationColors::from_palette`]), so a frame and
//! the content inside it cannot come from two themes. A dialog the shell
//! draws and a window the compositor draws then look alike under any theme,
//! and they are clicked alike: the geometry is
//! [`DecorationStyle::title_bar`], which the compositor is asked to use too.
//!
//! A dialog has a close button only -- it cannot be minimised or maximised
//! -- so `title_bar` packs that one at the theme's end of the bar.

use appearance::decorations::{DecorationStyle, TitleBarGeometry, TitleButton};
use appearance::{AppearanceSettings, DecorationColors, Palette};
use guitk::frame::Rect;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;
use guitk::text;

/// How a shell dialog's frame is drawn: the theme's frame, the windows'
/// corner radius, the interface's text size and the display's scale. The
/// colours are the palette's, at drawing time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DialogFrame {
    style: DecorationStyle,
    /// The windows' corner radius, at scale 1.
    corner: f32,
    /// The title's size, at scale 1: the interface's, as a window's title is.
    font_size: f32,
    scale: f32,
}

impl Default for DialogFrame {
    fn default() -> Self {
        Self::from_settings(&AppearanceSettings::default(), 1.0)
    }
}

impl DialogFrame {
    /// The frame `settings` give a dialog, at the display scale `scale`.
    #[must_use]
    pub fn from_settings(settings: &AppearanceSettings, scale: f32) -> Self {
        Self {
            style: settings.decorations(),
            corner: settings.window_corners.radius(),
            font_size: settings.fonts.ui_size,
            scale: if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            },
        }
    }

    /// A length at scale 1, at this frame's scale.
    fn scaled(&self, length: f32) -> f32 {
        length * self.scale
    }

    /// The border's width, scaled.
    fn border(&self) -> f32 {
        self.scaled(f32::from(self.style.border))
    }

    /// The title bar's height, scaled.
    fn bar_height(&self) -> f32 {
        self.scaled(f32::from(self.style.title_height))
    }

    /// The whole dialog's size for content `width` by `height`: the content,
    /// the border on every side, and the title bar on top.
    #[must_use]
    pub fn outer_size(&self, width: f32, height: f32) -> (f32, f32) {
        let border = self.border();
        (
            width + 2.0 * border,
            height + self.bar_height() + 2.0 * border,
        )
    }

    /// Where the parts of a dialog whose outside is `outer` go.
    #[must_use]
    pub fn layout(&self, outer: Rect) -> FrameLayout {
        let border = self.border();
        let inner_w = (outer.w - 2.0 * border).max(0.0);
        let bar = Rect::new(
            outer.x + border,
            outer.y + border,
            inner_w,
            self.bar_height(),
        );
        let geometry = self
            .style
            .title_bar(bar, self.scale, |b| b == TitleButton::Close);
        let content = Rect::new(
            bar.x,
            bar.y + bar.h,
            inner_w,
            (outer.h - bar.h - 2.0 * border).max(0.0),
        );
        FrameLayout {
            outer,
            content,
            geometry,
        }
    }

    /// The weight the title is written in.
    fn title_weight(&self) -> FontWeightHint {
        if self.style.title_bold {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        }
    }

    /// The frame of a dialog laid out as `layout`, titled `title`, in the
    /// palette `p` -- its body the palette's `base`, the frame in the frame's
    /// colours for `p` -- the close button lit while `close_lit`, the pointer
    /// on it. Drawn below the content, which goes on top.
    #[must_use]
    pub fn render(
        &self,
        layout: &FrameLayout,
        title: &str,
        p: &Palette,
        close_lit: bool,
    ) -> Vec<RenderCommand> {
        let colors = DecorationColors::from_palette(p);
        let mut out = Vec::with_capacity(8);
        let outer = layout.outer;
        let round = self.scaled(self.corner);
        let radii = CornerRadii::all(round);
        // The shadow reaches as far as the theme says: the run box's own
        // 4-down, 16-soft, 2-wide shadow at the built-in 8.
        let reach = self.scaled(f32::from(self.style.shadow));
        if reach > 0.0 {
            out.push(RenderCommand::BoxShadow {
                x: outer.x,
                y: outer.y,
                width: outer.w,
                height: outer.h,
                offset_x: 0.0,
                offset_y: reach / 2.0,
                blur: reach * 2.0,
                spread: reach / 4.0,
                color: colors.shadow,
                corner_radii: radii,
            });
        }
        out.push(RenderCommand::FillRect {
            x: outer.x,
            y: outer.y,
            width: outer.w,
            height: outer.h,
            color: p.base,
            corner_radii: radii,
        });
        let bar = layout.geometry.bar;
        out.push(RenderCommand::FillRect {
            x: bar.x,
            y: bar.y,
            width: bar.w,
            height: bar.h,
            color: colors.title_focused_bg,
            corner_radii: CornerRadii::top(round),
        });
        let border = self.border();
        if border > 0.0 {
            out.push(RenderCommand::StrokeRect {
                x: outer.x,
                y: outer.y,
                width: outer.w,
                height: outer.h,
                color: colors.border_focused,
                line_width: border,
                corner_radii: radii,
            });
        }
        self.render_title(&mut out, &layout.geometry, title, &colors);
        if let Some(close) = layout.close() {
            self.render_close(&mut out, close, close_lit, &colors);
        }
        out
    }

    /// The title, in its room on the bar, cut as the theme says.
    fn render_title(
        &self,
        out: &mut Vec<RenderCommand>,
        geometry: &TitleBarGeometry,
        title: &str,
        colors: &DecorationColors,
    ) {
        let size = self.scaled(self.font_size).max(1.0);
        let weight = self.title_weight();
        let room = geometry.title;
        let (shown, overflow) =
            text::fit_line(title, room.w, size, weight, self.style.title_overflow);
        let width = text::measure(&shown, size, weight).min(room.w);
        let line = text::line_height(size, weight);
        out.push(RenderCommand::Text {
            x: geometry.title_x(width),
            y: geometry.bar.y + (geometry.bar.h - line) / 2.0,
            text: shown,
            color: colors.title_focused_fg,
            font_size: size,
            font_weight: weight,
            max_width: Some(room.w),
            overflow,
        });
    }

    /// The close button at `rect`: its face in the theme's shape -- none for
    /// a glyph at rest, whose mark alone is drawn -- lit under the pointer.
    fn render_close(
        &self,
        out: &mut Vec<RenderCommand>,
        rect: Rect,
        lit: bool,
        colors: &DecorationColors,
    ) {
        let face = self
            .style
            .button_shape
            .face_radius(rect.w, self.scaled(self.corner), lit);
        if let Some(radius) = face {
            let color = if lit {
                guitk::palette::emphasized(colors.close_button)
            } else {
                colors.close_button
            };
            out.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color,
                corner_radii: CornerRadii::all(radius),
            });
        }
        if self.style.button_shape == appearance::decorations::ButtonShape::Glyph {
            // The mark: a cross in the middle two-fifths of the button, in
            // the title's colour -- or the face's ink when lit, on the face.
            let ink = if lit {
                guitk::palette::readable_on(colors.close_button)
            } else {
                colors.title_focused_fg
            };
            let arm = rect.w * 0.2;
            let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            let width = self.scaled(1.5).max(1.0);
            for (dx, dy) in [(arm, arm), (arm, -arm)] {
                out.push(RenderCommand::Line {
                    x1: cx - dx,
                    y1: cy - dy,
                    x2: cx + dx,
                    y2: cy + dy,
                    color: ink,
                    width,
                });
            }
        }
    }
}

/// Where a framed dialog's parts are: what [`DialogFrame::layout`] answers.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameLayout {
    /// The whole dialog, frame and all.
    pub outer: Rect,
    /// The room inside the frame, under the title bar.
    pub content: Rect,
    geometry: TitleBarGeometry,
}

impl FrameLayout {
    /// The title bar.
    #[must_use]
    pub fn title_bar(&self) -> Rect {
        self.geometry.bar
    }

    /// The close button.
    #[must_use]
    pub fn close(&self) -> Option<Rect> {
        self.geometry.button(TitleButton::Close)
    }

    /// Whether `(x, y)` is on the close button.
    #[must_use]
    pub fn is_close(&self, x: f32, y: f32) -> bool {
        self.geometry.button_at(x, y) == Some(TitleButton::Close)
    }
}

#[cfg(test)]
#[path = "dialog_frame_tests.rs"]
mod tests;
