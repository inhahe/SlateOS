//! The code view's find bar: what to find, what to replace it with, the three
//! switches -- case, whole words, regular expression -- and how many matches
//! there are, across the top of the view.
//!
//! Ctrl+F opens it on the text selected (when that is one line), Ctrl+H with
//! the replace row too. While it has the keyboard, typing searches as it
//! goes -- the view moves to the first match from where the caret was --
//! Enter goes to the next match and Shift+Enter to the one before, Alt+Enter
//! selects every match as a caret and hands the keyboard back to the text,
//! Alt+C, Alt+W and Alt+R flip the switches, Tab moves between the two
//! fields, and Escape closes it. In the replace field Enter replaces the
//! match and moves on, and Ctrl+Alt+Enter replaces every match as one step.
//!
//! Every match on screen is outlined, so the eye finds the rest while the
//! caret is on one; the bar says "3 of 12", or why the pattern is not one.

use core::ops::Range;

use crate::codeedit::{FindQuery, Finder};
use crate::event::{Key, KeyEvent};
use crate::field;
use crate::frame::Rect;
use crate::palette::Palette;
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::style::CornerRadii;
use crate::surface::CommandSink;
use crate::text::scaled;
use crate::textedit::{self, SingleLine};
use crate::textinput::{KeyEdit, TextInput};

/// The bar's rows' height.
pub(super) const ROW_HEIGHT: f32 = 28.0;
/// The text size in the bar.
const FONT_SIZE: f32 = 12.5;
/// Room around the bar's contents.
const PADDING: f32 = 6.0;
/// A switch's width.
const SWITCH_WIDTH: f32 = 26.0;
/// The width the match count is given.
const COUNT_WIDTH: f32 = 110.0;
/// The fields' share of the bar's width, at most.
const MAX_FIELD_WIDTH: f32 = 360.0;

/// Which field has the keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Field {
    #[default]
    Find,
    Replace,
}

/// A switch in the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Switch {
    Case,
    Word,
    Regex,
}

/// What a key in the bar asks the view to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BarAction {
    /// Nothing more than draw again.
    Redraw,
    /// The query changed: search again from where the caret was.
    Search,
    /// Go to the next match, or the one before.
    Next { backwards: bool },
    /// Select every match as a caret.
    SelectAll,
    /// Replace the selected match and go on.
    Replace,
    /// Replace every match.
    ReplaceAll,
    /// Close the bar.
    Close,
}

/// The find bar's state. See the module docs.
#[derive(Debug, Default)]
pub(super) struct FindBar {
    pub(super) open: bool,
    /// Whether the replace row shows.
    pub(super) replace: bool,
    /// Whether the bar, rather than the text, has the keyboard.
    pub(super) focused: bool,
    pub(super) field: Field,
    pub(super) find: TextInput,
    pub(super) with: TextInput,
    pub(super) case_sensitive: bool,
    pub(super) whole_word: bool,
    pub(super) regex: bool,
    /// The matches in the text, as of `revision`.
    pub(super) matches: Vec<Range<usize>>,
    /// Why the pattern is not one, when it is not.
    pub(super) error: Option<String>,
    /// The editor revision `matches` was found at; `None` before any search.
    pub(super) revision: Option<u64>,
}

impl FindBar {
    /// The query the bar holds.
    pub(super) fn query(&self) -> FindQuery {
        FindQuery {
            pattern: self.find.text().to_owned(),
            regex: self.regex,
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
        }
    }

    /// The query, compiled; `None` (with the reason kept) when it cannot be.
    pub(super) fn finder(&mut self) -> Option<Finder> {
        match Finder::new(&self.query()) {
            Ok(finder) => {
                self.error = None;
                Some(finder)
            }
            Err(crate::codeedit::FindError::Empty) => {
                self.error = None;
                None
            }
            Err(e) => {
                self.error = Some(e.to_string());
                None
            }
        }
    }

    /// How tall the bar is: one row, two with the replace row, nothing
    /// closed.
    pub(super) fn height(&self) -> f32 {
        if !self.open {
            return 0.0;
        }
        let rows = if self.replace { 2.0 } else { 1.0 };
        rows * ROW_HEIGHT + scaled(PADDING)
    }

    fn input_mut(&mut self) -> &mut TextInput {
        match self.field {
            Field::Find => &mut self.find,
            Field::Replace => &mut self.with,
        }
    }

    /// Act on a key while the bar has the keyboard.
    pub(super) fn handle_key(&mut self, key: &KeyEvent) -> Option<BarAction> {
        let m = key.modifiers;
        let action = match key.key {
            Key::Escape => BarAction::Close,
            Key::Enter if m.ctrl && m.alt && self.replace => BarAction::ReplaceAll,
            Key::Enter if m.alt => BarAction::SelectAll,
            Key::Enter if self.field == Field::Replace => BarAction::Replace,
            Key::Enter => BarAction::Next { backwards: m.shift },
            Key::Tab if self.replace => {
                self.field = match self.field {
                    Field::Find => Field::Replace,
                    Field::Replace => Field::Find,
                };
                BarAction::Redraw
            }
            Key::C if m.alt && !m.ctrl => {
                self.flip(Switch::Case);
                BarAction::Search
            }
            Key::W if m.alt && !m.ctrl => {
                self.flip(Switch::Word);
                BarAction::Search
            }
            Key::R if m.alt && !m.ctrl => {
                self.flip(Switch::Regex);
                BarAction::Search
            }
            _ => {
                let field = self.field;
                match self
                    .input_mut()
                    .edit_key(key, scaled(FONT_SIZE), FontWeightHint::Regular)
                {
                    KeyEdit::Unhandled => return None,
                    KeyEdit::Handled => BarAction::Redraw,
                    // A change to the replacement is not a new search.
                    KeyEdit::Changed if field == Field::Replace => BarAction::Redraw,
                    KeyEdit::Changed => BarAction::Search,
                }
            }
        };
        Some(action)
    }

    /// Flip a switch.
    pub(super) fn flip(&mut self, switch: Switch) {
        let slot = match switch {
            Switch::Case => &mut self.case_sensitive,
            Switch::Word => &mut self.whole_word,
            Switch::Regex => &mut self.regex,
        };
        *slot = !*slot;
    }

    /// Where the parts of the bar are, for a bar across `bounds`' top: the
    /// find field, the replace field (when shown), and the three switches.
    pub(super) fn layout(&self, bounds: Rect) -> Layout {
        let field_w =
            (bounds.w - scaled(PADDING) * 5.0 - scaled(SWITCH_WIDTH) * 3.0 - scaled(COUNT_WIDTH))
                .clamp(40.0, scaled(MAX_FIELD_WIDTH));
        let x = bounds.x + scaled(PADDING);
        let y = bounds.y + scaled(PADDING) / 2.0;
        let h = ROW_HEIGHT - scaled(PADDING);
        let find = Rect::new(x, y, field_w, h);
        let replace = self
            .replace
            .then(|| Rect::new(x, y + ROW_HEIGHT, field_w, h));
        let switch_x = find.right() + scaled(PADDING);
        let switches = [Switch::Case, Switch::Word, Switch::Regex];
        let mut placed = [(Switch::Case, Rect::EMPTY); 3];
        for (i, (slot, switch)) in placed.iter_mut().zip(switches).enumerate() {
            #[allow(clippy::cast_precision_loss, reason = "three switches")]
            let offset = i as f32 * (scaled(SWITCH_WIDTH) + scaled(2.0));
            *slot = (
                switch,
                Rect::new(switch_x + offset, y, scaled(SWITCH_WIDTH), h),
            );
        }
        let count = Rect::new(
            switch_x + 3.0 * (scaled(SWITCH_WIDTH) + scaled(2.0)) + scaled(PADDING),
            y,
            scaled(COUNT_WIDTH),
            h,
        );
        Layout {
            find,
            replace,
            switches: placed,
            count,
        }
    }

    /// Draw the bar across the top of `bounds`.
    pub(super) fn draw(
        &self,
        sink: &mut impl CommandSink,
        p: &Palette,
        bounds: Rect,
        current: Option<usize>,
        caret_width: f32,
    ) {
        if !self.open {
            return;
        }
        let strip = Rect::new(bounds.x, bounds.y, bounds.w, self.height());
        sink.emit(RenderCommand::FillRect {
            x: strip.x,
            y: strip.y,
            width: strip.w,
            height: strip.h,
            color: p.mantle,
            corner_radii: CornerRadii::ZERO,
        });
        let layout = self.layout(bounds);
        self.draw_field(
            sink,
            p,
            layout.find,
            &self.find,
            self.focused && self.field == Field::Find,
            self.error.is_some(),
            caret_width,
        );
        if let Some(rect) = layout.replace {
            self.draw_field(
                sink,
                p,
                rect,
                &self.with,
                self.focused && self.field == Field::Replace,
                false,
                caret_width,
            );
        }
        for (switch, rect) in layout.switches {
            let on = match switch {
                Switch::Case => self.case_sensitive,
                Switch::Word => self.whole_word,
                Switch::Regex => self.regex,
            };
            let label = match switch {
                Switch::Case => "Aa",
                Switch::Word => "W",
                Switch::Regex => ".*",
            };
            sink.emit(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: if on { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(3.0),
            });
            let w = crate::text::measure(label, scaled(FONT_SIZE), FontWeightHint::Bold);
            sink.emit(RenderCommand::Text {
                x: rect.x + (rect.w - w) / 2.0,
                y: rect.y + (rect.h - scaled(FONT_SIZE)) / 2.0,
                text: label.to_owned(),
                color: if on { p.on_accent() } else { p.text },
                font_size: scaled(FONT_SIZE),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        let (status, color) = if let Some(error) = &self.error {
            (error.clone(), p.ink(p.red))
        } else if self.find.text().is_empty() {
            (String::new(), p.subtext0)
        } else if self.matches.is_empty() {
            ("No matches".to_owned(), p.subtext0)
        } else {
            let of = self.matches.len();
            match current {
                Some(i) => (format!("{} of {of}", i.saturating_add(1)), p.subtext0),
                None => (format!("{of} matches"), p.subtext0),
            }
        };
        if !status.is_empty() {
            sink.emit(RenderCommand::Text {
                x: layout.count.x,
                y: layout.count.y + (layout.count.h - scaled(FONT_SIZE)) / 2.0,
                text: status,
                color,
                font_size: scaled(FONT_SIZE),
                font_weight: FontWeightHint::Regular,
                max_width: Some((bounds.right() - layout.count.x - scaled(PADDING)).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "where, what, and the three states a field shows"
    )]
    fn draw_field(
        &self,
        sink: &mut impl CommandSink,
        p: &Palette,
        rect: Rect,
        input: &TextInput,
        focused: bool,
        invalid: bool,
        caret_width: f32,
    ) {
        field::draw(
            sink,
            p,
            rect,
            field::State {
                focused,
                invalid,
                ..field::State::default()
            },
            crate::style::FOCUS_RING_WIDTH,
        );
        let mut tree = crate::render::RenderTree::new();
        textedit::draw(
            &mut tree,
            &SingleLine {
                text: input.text(),
                cursor: input.cursor(),
                selection_anchor: input.selection_anchor(),
                focused,
                x: rect.x + scaled(6.0),
                y: rect.y + (rect.h - scaled(FONT_SIZE)) / 2.0,
                width: (rect.w - scaled(12.0)).max(0.0),
                line_height: scaled(FONT_SIZE),
                font_size: scaled(FONT_SIZE),
                weight: FontWeightHint::Regular,
                color: p.text,
                selection_bg: p.accent,
                selection_fg: p.on_accent(),
                caret_width,
            },
        );
        for cmd in tree.commands {
            sink.emit(cmd);
        }
    }
}

/// Where the bar's parts are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Layout {
    pub(super) find: Rect,
    pub(super) replace: Option<Rect>,
    pub(super) switches: [(Switch, Rect); 3],
    pub(super) count: Rect,
}
