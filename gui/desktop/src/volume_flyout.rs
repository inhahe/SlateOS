//! The volume flyout: what a press on the tray's speaker opens -- the master
//! volume's slider with its level, and a switch to mute it -- above the
//! taskbar, as every desktop's speaker opens one (`roadmap-detailed.md`
//! §3.4, "Volume icon popup"). Its level is the sound card's
//! ([`crate::volume`]); while the card is out of reach it says why where the
//! slider would be, and nothing in it moves (design-decisions §1485).
//!
//! At its foot, below a line, "Audio settings…" opens Settings on its Sound
//! page -- the one place for output and input both -- whether or not the card
//! can be reached, since that is when a user most wants to look. What the
//! roadmap's popup has besides -- a chooser of the output device, a mixer of
//! each program's volume -- waits on what the kernel's mixer offers programs,
//! which is the master alone today.
//!
//! The flyout holds the gesture -- a drag of the slider in progress -- and
//! nothing about the level, which the shell passes in each time; it answers
//! an input with what the user asked for ([`Action`]), which the shell
//! carries out on the card.

use guitk::color::Color;
use guitk::event::KeyEvent;
use guitk::frame::Rect;
use guitk::palette::Palette;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::slider::{Look, Placement, Slider};
use guitk::style::CornerRadii;

/// The flyout's width, at a scale of 1.
pub const WIDTH: f32 = 280.0;
/// Its height, at a scale of 1: the caption, the slider's row and the mute
/// switch's, the line, and the settings row, inside the padding.
pub const HEIGHT: f32 = PADDING * 2.0 + CAPTION_ROW + ROW * 3.0 + SEPARATOR;
/// What "Audio settings…" says.
pub const SETTINGS_LABEL: &str = "Audio settings\u{2026}";
/// The space round its contents.
const PADDING: f32 = 14.0;
/// The caption's size.
const CAPTION_SIZE: f32 = 12.0;
/// The level's and the mute label's size.
const TEXT_SIZE: f32 = 13.0;
/// Its corners.
const RADIUS: f32 = 8.0;
/// The slider's track thickness.
const TRACK: f32 = 6.0;
/// The slider's thumb.
const THUMB: f32 = 16.0;
/// Room right of the slider for its level, "100%".
const LEVEL_ROOM: f32 = 44.0;
/// The mute switch's width and height.
const SWITCH: (f32, f32) = (40.0, 20.0);
/// Each row's height below the caption.
const ROW: f32 = 28.0;
/// The caption's height.
const CAPTION_ROW: f32 = 20.0;
/// The room the line above the settings row takes, the line in its middle.
const SEPARATOR: f32 = 9.0;

/// What a press, a drag or a key in the flyout asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Set the master volume to this level, 0 to 100.
    Level(u8),
    /// Mute it if it sounds, let it sound if it is muted.
    ToggleMute,
    /// Open Settings on its Sound page, and close the flyout: "Audio
    /// settings…".
    OpenSettings,
}

/// Where everything in a flyout is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The flyout's panel.
    pub panel: Rect,
    /// The scale it is laid out at.
    pub scale: f32,
    /// The slider.
    pub slider: Placement,
    /// Where the level is written, right of the slider.
    pub level: Rect,
    /// The mute switch.
    pub mute: Rect,
    /// The "Audio settings…" row: a press anywhere on it opens Settings.
    pub settings: Rect,
}

impl Layout {
    /// The layout of a flyout whose top-left corner is `origin`, at `scale`.
    #[must_use]
    pub fn new(origin: (f32, f32), scale: f32) -> Self {
        let px = |v: f32| v * scale;
        let (x, y) = origin;
        let panel = Rect::new(x, y, px(WIDTH), px(HEIGHT));
        let inner_x = x + px(PADDING);
        let inner_w = px(WIDTH - PADDING * 2.0);
        let slider_row = y + px(PADDING + CAPTION_ROW);
        let track_w = (inner_w - px(LEVEL_ROOM)).max(0.0);
        let slider = Placement::horizontal(
            Rect::new(
                inner_x + px(THUMB / 2.0),
                slider_row + (px(ROW) - px(TRACK)) / 2.0,
                (track_w - px(THUMB)).max(0.0),
                px(TRACK),
            ),
            px(THUMB),
        );
        let level = Rect::new(inner_x + track_w, slider_row, px(LEVEL_ROOM), px(ROW));
        let mute_row = slider_row + px(ROW);
        let mute = Rect::new(
            inner_x + inner_w - px(SWITCH.0),
            mute_row + (px(ROW) - px(SWITCH.1)) / 2.0,
            px(SWITCH.0),
            px(SWITCH.1),
        );
        let settings = Rect::new(inner_x, mute_row + px(ROW + SEPARATOR), inner_w, px(ROW));
        Self {
            panel,
            scale,
            slider,
            level,
            mute,
            settings,
        }
    }
}

/// The volume flyout.
#[derive(Clone, Debug)]
pub struct VolumeFlyout {
    visible: bool,
    slider: Slider,
}

impl Default for VolumeFlyout {
    fn default() -> Self {
        Self::new()
    }
}

impl VolumeFlyout {
    /// A closed flyout.
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: false,
            slider: Slider::new(0.0, 100.0, 0.0).with_step(1.0),
        }
    }

    /// Whether it is open.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Open it, or close it -- which lets go of a drag in progress, as the
    /// pointer leaving it would.
    pub fn set_visible(&mut self, visible: bool) {
        if !visible {
            // Closing is not a release: the level a drag reached is the one
            // already set.
            let _abandoned = self.slider.cancel();
        }
        self.visible = visible;
    }

    /// A press at `(x, y)` -- on the slider, moving its thumb there or taking
    /// hold of it; on the mute switch -- with the volume at `level`; or on
    /// "Audio settings…", which opens Settings whatever the card. Nothing
    /// else while the card is out of reach (`reachable` false), and nothing
    /// anywhere else in the panel.
    pub fn press(
        &mut self,
        layout: &Layout,
        (x, y): (f32, f32),
        level: u8,
        reachable: bool,
    ) -> Option<Action> {
        if !self.visible {
            return None;
        }
        if layout.settings.contains(x, y) {
            return Some(Action::OpenSettings);
        }
        if !reachable {
            return None;
        }
        if layout.mute.contains(x, y) {
            return Some(Action::ToggleMute);
        }
        self.slider.set_value(f64::from(level));
        level_of(self.slider.press(&layout.slider, x, y))
    }

    /// The pointer moved to `(x, y)` with the button held: the slider's
    /// thumb follows it, if a press took hold of it.
    pub fn drag(&mut self, layout: &Layout, (x, y): (f32, f32)) -> Option<Action> {
        level_of(self.slider.drag_to(&layout.slider, x, y))
    }

    /// The button was let go.
    pub fn release(&mut self) -> Option<Action> {
        level_of(self.slider.release())
    }

    /// Whether a drag of the slider is going on.
    #[must_use]
    pub const fn dragging(&self) -> bool {
        self.slider.is_dragging()
    }

    /// A key while the flyout is open, the volume at `level`: the arrows,
    /// Page Up and Down, Home and End move the slider, as on any slider.
    pub fn key(&mut self, key: &KeyEvent, level: u8, reachable: bool) -> Option<Action> {
        if !reachable || !self.visible {
            return None;
        }
        self.slider.set_value(f64::from(level));
        level_of(self.slider.handle_key(key))
    }

    /// Draw the flyout laid out at `layout`, the volume at `level`, `muted`
    /// or not -- or, with the card out of reach, `why` in the slider's place
    /// and no switch.
    #[must_use]
    pub fn render(
        &self,
        p: &Palette,
        layout: &Layout,
        level: u8,
        muted: bool,
        out_of_reach: Option<&str>,
    ) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }
        let s = layout.scale;
        let px = |v: f32| v * s;
        let panel = layout.panel;
        let radii = CornerRadii::all(px(RADIUS));
        let mut cmds = vec![
            RenderCommand::BoxShadow {
                x: panel.x,
                y: panel.y,
                width: panel.w,
                height: panel.h,
                offset_x: 0.0,
                offset_y: px(4.0),
                blur: px(16.0),
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 100),
                corner_radii: radii,
            },
            RenderCommand::FillRect {
                x: panel.x,
                y: panel.y,
                width: panel.w,
                height: panel.h,
                color: p.base,
                corner_radii: radii,
            },
            RenderCommand::StrokeRect {
                x: panel.x,
                y: panel.y,
                width: panel.w,
                height: panel.h,
                color: p.surface1,
                line_width: 1.0,
                corner_radii: radii,
            },
        ];
        let inner_x = panel.x + px(PADDING);
        let inner_w = px(WIDTH - PADDING * 2.0);
        let text = |cmds: &mut Vec<RenderCommand>, x, y, t: String, color, size, width| {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: t,
                color,
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
        };
        text(
            &mut cmds,
            inner_x,
            panel.y + px(PADDING),
            "Volume".to_owned(),
            p.subtext0,
            px(CAPTION_SIZE),
            inner_w,
        );
        let slider_row = panel.y + px(PADDING + CAPTION_ROW);
        let text_dy = (px(ROW) - px(TEXT_SIZE)) / 2.0;
        if let Some(why) = out_of_reach {
            text(
                &mut cmds,
                inner_x,
                slider_row + text_dy,
                why.to_owned(),
                p.subtext0,
                px(TEXT_SIZE),
                inner_w,
            );
        } else {
            let mut shown = self.slider.clone();
            shown.set_value(f64::from(level));
            shown.draw(
                &mut cmds,
                p,
                &layout.slider,
                Look::accent(p, p.surface2),
                false,
                0.0,
            );
            text(
                &mut cmds,
                layout.level.x + px(8.0),
                layout.level.y + text_dy,
                if muted {
                    "Muted".to_owned()
                } else {
                    format!("{level}%")
                },
                p.text,
                px(TEXT_SIZE),
                (layout.level.w - px(8.0)).max(0.0),
            );
            let mute_row = slider_row + px(ROW);
            text(
                &mut cmds,
                inner_x,
                mute_row + text_dy,
                "Mute".to_owned(),
                p.text,
                px(TEXT_SIZE),
                (layout.mute.x - inner_x).max(0.0),
            );
            cmds.extend(guitk::switch::shapes(
                p,
                layout.mute,
                muted,
                if muted { p.accent } else { p.surface2 },
            ));
        }
        // The foot, whatever the card: a line, and "Audio settings…" as a
        // link -- the link's colour, and underlined, since colour alone does
        // not mark one.
        let settings = layout.settings;
        cmds.push(RenderCommand::FillRect {
            x: inner_x,
            y: settings.y - px(SEPARATOR) / 2.0,
            width: inner_w,
            height: px(1.0),
            color: p.surface1,
            corner_radii: CornerRadii::ZERO,
        });
        let label_y = settings.y + text_dy;
        text(
            &mut cmds,
            settings.x,
            label_y,
            SETTINGS_LABEL.to_owned(),
            p.link,
            px(TEXT_SIZE),
            settings.w,
        );
        let label_w = guitk::text::measure(SETTINGS_LABEL, px(TEXT_SIZE), FontWeightHint::Regular)
            .min(settings.w);
        cmds.push(RenderCommand::FillRect {
            x: settings.x,
            y: label_y + px(TEXT_SIZE) + px(1.0),
            width: label_w,
            height: px(1.0),
            color: p.link,
            corner_radii: CornerRadii::ZERO,
        });
        cmds
    }
}

/// The level a slider's response sets, if it moved it.
fn level_of(response: guitk::slider::Response) -> Option<Action> {
    let value = response.event()?.value().round().clamp(0.0, 100.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded and clamped to 0..=100 first"
    )]
    let level = value as u8;
    Some(Action::Level(level))
}

#[cfg(test)]
#[path = "volume_flyout_tests.rs"]
mod tests;
