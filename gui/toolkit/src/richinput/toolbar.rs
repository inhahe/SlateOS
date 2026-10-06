//! A rich input's formatting toolbar: bold, italic, underline and
//! strikethrough -- each drawn pressed where all of the selection has it, or
//! with nothing selected, where what is typed next would -- a smaller and a
//! larger size, and taking the formatting off.
//!
//! Optional, as the roadmap has it: a program that wants its own toolbar, a
//! menu or keys alone calls [`RichInput::toggle`](super::RichInput::toggle)
//! and the rest itself. The buttons are the toolkit's own
//! ([`crate::button`]), so the toolbar follows the theme as every button
//! does.

use super::{Format, RichInput, Toggle};
use crate::button;
use crate::color::Color;
use crate::palette::Palette;
use crate::render::RenderTree;

/// One of the toolbar's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// A switch: bold, italic, underline, strikethrough.
    Switch(Toggle),
    /// A size smaller.
    Smaller,
    /// A size larger.
    Larger,
    /// Every format off.
    Clear,
}

/// The toolbar's buttons, in order.
pub const TOOLS: [Tool; 7] = [
    Tool::Switch(Toggle::Bold),
    Tool::Switch(Toggle::Italic),
    Tool::Switch(Toggle::Underline),
    Tool::Switch(Toggle::Strike),
    Tool::Smaller,
    Tool::Larger,
    Tool::Clear,
];

/// The space between two buttons.
const GAP: f32 = 4.0;

/// The sizes a size button steps through, in pixels.
pub const SIZES: [f32; 10] = [8.0, 10.0, 12.0, 13.0, 15.0, 18.0, 22.0, 28.0, 36.0, 48.0];

impl Tool {
    /// What its button says.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Switch(Toggle::Bold) => "B",
            Self::Switch(Toggle::Italic) => "I",
            Self::Switch(Toggle::Underline) => "U",
            Self::Switch(Toggle::Strike) => "S",
            Self::Smaller => "A-",
            Self::Larger => "A+",
            Self::Clear => "Clear",
        }
    }
}

/// Each button's box, laid out from `(x, y)` along a row.
#[must_use]
pub fn layout(palette: &Palette, (x, y): (f32, f32)) -> Vec<(Tool, (f32, f32, f32, f32))> {
    let style = &palette.widget_style.button;
    let h = button::height();
    let mut at = x;
    TOOLS
        .iter()
        .map(|&tool| {
            let w = button::width(style, tool.label());
            let rect = (at, y, w, h);
            at += w + GAP;
            (tool, rect)
        })
        .collect()
}

/// The button at `(px, py)`, if any, in a toolbar laid out from `origin`.
#[must_use]
pub fn hit(palette: &Palette, origin: (f32, f32), (px, py): (f32, f32)) -> Option<Tool> {
    layout(palette, origin)
        .into_iter()
        .find(|(_, (x, y, w, h))| px >= *x && px < x + w && py >= *y && py < y + h)
        .map(|(tool, _)| tool)
}

/// Draw the toolbar from `origin` on `ground`: a switch pressed where the
/// field shows it, the button `hovered` lit.
pub fn draw(
    tree: &mut RenderTree,
    palette: &Palette,
    input: &RichInput,
    origin: (f32, f32),
    ground: Color,
    hovered: Option<Tool>,
) {
    for (tool, rect) in layout(palette, origin) {
        let pressed = match tool {
            Tool::Switch(toggle) => input.shown_has(toggle),
            Tool::Smaller | Tool::Larger | Tool::Clear => false,
        };
        let state = button::State {
            hovered: hovered == Some(tool),
            pressed,
            disabled: false,
            focused: false,
        };
        button::draw(
            tree,
            palette,
            rect,
            tool.label(),
            button::Kind::Plain,
            state,
            ground,
            0.0,
        );
    }
}

/// Do what `tool` does to `input`, a field whose text is `base` pixels
/// unless a format says otherwise.
pub fn apply(input: &mut RichInput, tool: Tool, base: f32) {
    match tool {
        Tool::Switch(toggle) => input.toggle(toggle),
        Tool::Smaller => {
            let now = size_now(input, base);
            let next = SIZES.iter().rev().find(|&&s| s < now).copied();
            input.set_size(Some(next.unwrap_or(now)));
        }
        Tool::Larger => {
            let now = size_now(input, base);
            let next = SIZES.iter().find(|&&s| s > now).copied();
            input.set_size(Some(next.unwrap_or(now)));
        }
        Tool::Clear => input.clear_formatting(),
    }
}

/// The size the selection starts in -- or what is typed next would be.
fn size_now(input: &RichInput, base: f32) -> f32 {
    let format: Format = input
        .selection_range()
        .map_or_else(|| input.typing_format(), |(s, _)| input.doc().format_at(s));
    format.size.unwrap_or(base)
}

#[cfg(test)]
#[path = "toolbar_tests.rs"]
mod tests;
