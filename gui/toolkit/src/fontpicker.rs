//! A font picker: the families this machine has, each listed in its own
//! face; the weights a family can be drawn at; the size; and a line of
//! preview in the choice so far -- [`FontPickerDialog`].
//!
//! # Preview, keep, revert
//!
//! The picker reports what the user does as they do it, in the colour
//! picker's three events ([`FontPickerEvent`], as
//! [`ColorPickerEvent`](crate::colorpicker::ColorPickerEvent) has them).
//! [`Changed`](FontPickerEvent::Changed) comes with every tentative choice --
//! a family moved onto, a weight or a size picked, a search that lands on
//! another family -- and commits to nothing: a host may redraw its document's
//! selection in it while the user browses.
//! [`Confirmed`](FontPickerEvent::Confirmed) carries the choice to keep.
//! [`Cancelled`](FontPickerEvent::Cancelled) means put back what was there:
//! [`FontPickerDialog::choice`] is the one the dialog opened with again. A
//! host that previews nothing reads only the last two.
//!
//! # Each family in its own face
//!
//! A family's row is drawn in that family ([`FontFamily::Named`]), as a word
//! processor's font list is: the name set in the face is the quickest preview
//! of it there is, and lets a list of hundreds be read at a glance. So is
//! each weight's row, in the family chosen, and the preview line.
//!
//! # Only what can be drawn
//!
//! The weights offered are the ones a render tree draws -- regular and bold
//! ([`FontWeightHint`]), upright -- and bold only for a family with a bold
//! face, or a variable one that reaches bold ([`text::family_has_bold`]). A
//! render tree draws no other weight and no slant, and a picker that offered
//! "Light" or "Italic" would preview them in regular and a host would draw
//! them that way: a choice that changes nothing.
//! [`FontPickerDialog::weights`] is the one place that grows when a render
//! tree can draw more.
//!
//! Sizes are whole pixels, 1 to [`MAX_SIZE`]: what the font cache builds. It
//! rounds every size to a whole pixel and builds nothing larger, so a picker
//! that took 10.5 would draw 10 or 11 and promise what it did not draw.
//!
//! # The keyboard
//!
//! Tab and Shift+Tab move between the search field, the family list, the
//! weights, the size field, the size list and the two buttons. Typing in the
//! search field narrows the families -- those whose name starts with what was
//! typed first, then those that contain it -- and lands on the first of them
//! unless the family chosen is still listed; the arrows and Page Up/Down move
//! through the families from the field without leaving it. Typing in the
//! family list types into the search. In the size field the arrows step
//! through [`SIZES`], down the list for larger. Enter keeps the choice from
//! anywhere but Cancel; Escape turns the dialog down from anywhere.

use crate::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::{Frame, Rect};
use crate::listview::{ListKey, ListViewport};
use crate::palette::Palette;
use crate::render::{FontFamily, FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use crate::style::CornerRadii;
use crate::text::{self, scaled};
use crate::textedit::{self, SingleLine};
use crate::textinput::{KeyEdit, TextInput};
use crate::{button, field, scrollbar, wheel};

/// The sizes the size list offers, in pixels. Any whole size from 1 to
/// [`MAX_SIZE`] may be typed.
pub const SIZES: [u16; 17] = [
    8, 9, 10, 11, 12, 13, 14, 16, 18, 20, 22, 24, 28, 32, 36, 48, 72,
];

/// The largest size, in pixels: the largest the font cache builds.
pub const MAX_SIZE: u16 = osfont::system::MAX_PX;

/// What the preview says until a host gives it text of its own.
pub const DEFAULT_SAMPLE: &str = "The quick brown fox jumps over the lazy dog";

// The layout, in pixels at the default text size: each is drawn through
// `text::scaled`, so a larger text size makes a larger dialog.
const PADDING: f32 = 12.0;
const TITLE_HEIGHT: f32 = 32.0;
const TITLE_SIZE: f32 = 13.0;
const LABEL_HEIGHT: f32 = 20.0;
const LABEL_SIZE: f32 = 12.0;
const FIELD_HEIGHT: f32 = 28.0;
const FIELD_PADDING: f32 = 8.0;
const FIELD_TEXT_SIZE: f32 = 13.0;
const LIST_GAP: f32 = 4.0;
const FAMILY_ROW: f32 = 26.0;
const FAMILY_SIZE: f32 = 15.0;
const ROW: f32 = 22.0;
const ROW_SIZE: f32 = 13.0;
const ROW_PADDING: f32 = 8.0;
const COLUMN_GAP: f32 = 12.0;
const WEIGHT_WIDTH: f32 = 110.0;
const SIZE_WIDTH: f32 = 76.0;
const PREVIEW_HEIGHT: f32 = 84.0;
const BUTTON_WIDTH: f32 = 88.0;
const BUTTON_GAP: f32 = 8.0;
const BAR_WIDTH: f32 = 12.0;
const RADIUS: f32 = 6.0;
const WELL_RADIUS: f32 = 4.0;
const DIALOG_WIDTH: f32 = 560.0;
const DIALOG_HEIGHT: f32 = 440.0;

/// A font: a family, a weight and a size -- what the picker chooses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontChoice {
    /// The family, spelled as the font index spells it -- "Noto Serif".
    pub family: String,
    /// Regular or bold.
    pub weight: FontWeightHint,
    /// The size in whole pixels, 1 to [`MAX_SIZE`].
    pub size: u16,
}

impl FontChoice {
    /// `family` at `weight` and `size`, the size held to 1 to [`MAX_SIZE`].
    #[must_use]
    pub fn new(family: impl Into<String>, weight: FontWeightHint, size: u16) -> Self {
        Self {
            family: family.into(),
            weight,
            size: size.clamp(1, MAX_SIZE),
        }
    }

    /// The family to draw text in this choice in -- what a host puts in
    /// [`RenderCommand::PushFont`]: the family by its name, or the UI face
    /// for a name no family can have (empty, or too long to be one).
    #[must_use]
    pub fn font_family(&self) -> FontFamily {
        FontFamily::named(&self.family).unwrap_or(FontFamily::Ui)
    }

    /// The size as a render tree takes it.
    #[must_use]
    pub fn font_size(&self) -> f32 {
        f32::from(self.size)
    }
}

/// What the picker says happened -- see the module's "Preview, keep,
/// revert".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontPickerEvent {
    /// A tentative choice: to preview if the host likes, committing to
    /// nothing.
    Changed(FontChoice),
    /// The user kept this choice -- OK, Enter, or a double-click on a family.
    Confirmed(FontChoice),
    /// The user turned the dialog down -- Cancel or Escape. Put back what was
    /// there; [`FontPickerDialog::choice`] is the original again.
    Cancelled,
}

/// The parts of the dialog that take the keyboard, in Tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The search field over the family list.
    Search,
    /// The family list.
    Families,
    /// The weights.
    Weights,
    /// The size field.
    SizeField,
    /// The size list.
    Sizes,
    /// The OK button.
    Ok,
    /// The Cancel button.
    Cancel,
}

impl Part {
    const ORDER: [Self; 7] = [
        Self::Search,
        Self::Families,
        Self::Weights,
        Self::SizeField,
        Self::Sizes,
        Self::Ok,
        Self::Cancel,
    ];

    /// The part after this one in Tab order, or before it, round the ends.
    fn step(self, backwards: bool) -> Self {
        let at = Self::ORDER.iter().position(|&p| p == self).unwrap_or(0);
        let to = if backwards {
            at.checked_sub(1)
        } else {
            at.checked_add(1)
        };
        to.and_then(|i| Self::ORDER.get(i).copied())
            .unwrap_or(if backwards {
                Self::Cancel
            } else {
                Self::Search
            })
    }
}

/// One of the dialog's two scrolling lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    /// The families.
    Families,
    /// The sizes.
    Sizes,
}

/// What a point in the dialog reaches: the hit boxes its frame records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The dialog itself, wherever it is nothing else -- so a host can tell
    /// a click on the dialog from one past its edge.
    Dialog,
    /// The search field.
    Search,
    /// A family's row, by its place among the families shown.
    Family(usize),
    /// A weight's row.
    Weight(FontWeightHint),
    /// The size field.
    SizeField,
    /// A size's row, by its place in [`SIZES`].
    Size(usize),
    /// A list's scrollbar column, outside its thumb.
    Track(List),
    /// A list's scrollbar thumb.
    Thumb(List),
    /// The OK button.
    Ok,
    /// The Cancel button.
    Cancel,
}

/// A scrollbar thumb held by the pointer: which list's, and how far below
/// the thumb's top it was taken hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ThumbDrag {
    list: List,
    grab: f32,
}

/// Where everything is in a dialog `width` by `height`: the one computation
/// the drawing and the keyboard's paging both read.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    title: Rect,
    family_label: Rect,
    weight_label: Rect,
    size_label: Rect,
    search: Rect,
    families: Rect,
    weights: Rect,
    size_field: Rect,
    sizes: Rect,
    preview: Rect,
    ok: Rect,
    cancel: Rect,
}

impl Layout {
    fn new(width: f32, height: f32) -> Self {
        let pad = scaled(PADDING);
        let gap = scaled(COLUMN_GAP);
        let title = Rect::new(0.0, 0.0, width.max(0.0), scaled(TITLE_HEIGHT));

        let button_h = button::height();
        let button_w = scaled(BUTTON_WIDTH);
        let buttons_y = (height - pad - button_h).max(0.0);
        let cancel = Rect::new(width - pad - button_w, buttons_y, button_w, button_h);
        let ok = Rect::new(
            cancel.x - scaled(BUTTON_GAP) - button_w,
            buttons_y,
            button_w,
            button_h,
        );

        let preview_h = scaled(PREVIEW_HEIGHT);
        let preview = Rect::new(
            pad,
            buttons_y - pad - preview_h,
            (width - 2.0 * pad).max(0.0),
            preview_h,
        );

        let label_h = scaled(LABEL_HEIGHT);
        let label_y = title.bottom() + pad;
        let columns_y = label_y + label_h;
        let weight_w = scaled(WEIGHT_WIDTH);
        let size_w = scaled(SIZE_WIDTH);
        let family_w = (width - 2.0 * pad - weight_w - size_w - 2.0 * gap).max(0.0);
        let family_x = pad;
        let weight_x = family_x + family_w + gap;
        let size_x = weight_x + weight_w + gap;
        let field_h = scaled(FIELD_HEIGHT);
        let lists_y = columns_y + field_h + scaled(LIST_GAP);
        // A dialog too short for its lists gives them no rows, not a
        // negative height: a box whose bottom is above its top.
        let lists_bottom = (preview.y - pad).max(lists_y);

        Self {
            title,
            family_label: Rect::new(family_x, label_y, family_w, label_h),
            weight_label: Rect::new(weight_x, label_y, weight_w, label_h),
            size_label: Rect::new(size_x, label_y, size_w, label_h),
            search: Rect::new(family_x, columns_y, family_w, field_h),
            families: Rect::new(family_x, lists_y, family_w, lists_bottom - lists_y),
            weights: Rect::new(weight_x, columns_y, weight_w, lists_bottom - columns_y),
            size_field: Rect::new(size_x, columns_y, size_w, field_h),
            sizes: Rect::new(size_x, lists_y, size_w, lists_bottom - lists_y),
            preview,
            ok,
            cancel,
        }
    }

    /// How many family rows the list holds.
    fn family_rows(&self) -> usize {
        rows_in(self.families.h, scaled(FAMILY_ROW))
    }

    /// How many size rows the list holds.
    fn size_rows(&self) -> usize {
        rows_in(self.sizes.h, scaled(ROW))
    }
}

/// How many whole rows `row` tall fit in `height`: none for a height or a row
/// that is not a positive number.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive, finite quotient of two lengths on a screen, floored"
)]
fn rows_in(height: f32, row: f32) -> usize {
    if row > 0.0 && height > 0.0 && height.is_finite() {
        (height / row).floor() as usize
    } else {
        0
    }
}

/// The top of row `slot` in a list whose first row is at `top`.
#[allow(
    clippy::cast_precision_loss,
    reason = "a row on screen: a few hundred at most, far inside f32's range"
)]
fn row_top(top: f32, slot: usize, row: f32) -> f32 {
    top + slot as f32 * row
}

/// The top of a line of text `size` pixels at `weight` in `family`, centred
/// in `rect` -- measured in that family, whose line may be taller or shorter
/// than the UI face's.
fn centred(rect: Rect, size: f32, weight: FontWeightHint, family: FontFamily) -> f32 {
    rect.y + (rect.h - text::line_height_in(size, weight, family)) / 2.0
}

/// The rows of a list `len` long that show, `capacity` at a time, from
/// `view` -- revealing its selection as the list's own viewport will at the
/// next event, so the first frame, drawn before any event has told the
/// viewport its height, already shows the family chosen.
fn shown_rows(view: ListViewport, len: usize, capacity: usize) -> std::ops::Range<usize> {
    let mut view = view;
    if view.height() != capacity {
        view.set_height(capacity, len);
    }
    view.visible_range(len)
}

/// A weight's name in the weight list.
fn weight_name(weight: FontWeightHint) -> &'static str {
    match weight {
        FontWeightHint::Bold => "Bold",
        FontWeightHint::Regular => "Regular",
        FontWeightHint::Light => "Light",
    }
}

/// `text` as a size, if it is a whole number of pixels from 1 to
/// [`MAX_SIZE`]; spaces round it are allowed.
#[must_use]
pub fn parse_size(text: &str) -> Option<u16> {
    let px: u16 = text.trim().parse().ok()?;
    (1..=MAX_SIZE).contains(&px).then_some(px)
}

/// The font picker. Shown in a window or a modal overlay of its host's: the
/// host forwards every key and mouse event, draws [`render`](Self::render) at
/// the size it passes to the handlers, and acts on the events they return.
#[derive(Clone, Debug)]
pub struct FontPickerDialog {
    /// The choice the dialog opened with, for Cancel to put back.
    original: FontChoice,
    /// The choice so far.
    choice: FontChoice,
    /// Every family the dialog offers, in the order listed.
    families: Vec<String>,
    /// The families the search leaves, as places in `families`.
    shown: Vec<usize>,
    /// What has been typed in the search field.
    search: TextInput,
    /// The family list's selection and scroll, over `shown`.
    family_view: ListViewport,
    /// What is in the size field, which may not be a size while it is typed.
    size_text: TextInput,
    /// The size list's selection and scroll, over [`SIZES`].
    size_view: ListViewport,
    /// Which part has the keyboard.
    focus: Part,
    /// What the pointer is over.
    hovered: Option<Target>,
    /// A scrollbar thumb the pointer holds.
    drag: Option<ThumbDrag>,
    /// The scroll wheel's part-rows, between notches.
    wheel: wheel::Accumulator,
    /// What the preview says.
    sample: String,
    /// The user's focus-ring width.
    focus_ring: f32,
    /// The user's caret width.
    caret_width: f32,
    /// Whether a family can be drawn bold.
    has_bold: fn(&str) -> bool,
    /// Whether the family chosen can: `has_bold` asked once per family.
    bold: bool,
    /// OK was pressed.
    confirmed: bool,
    /// Cancel was pressed.
    cancelled: bool,
}

impl FontPickerDialog {
    /// A picker over every family installed here, starting at `initial`.
    #[must_use]
    pub fn new(initial: FontChoice) -> Self {
        Self::over(initial, text::available_families(), text::family_has_bold)
    }

    /// A picker over the fixed-pitch families only -- a terminal's font, or a
    /// code editor's -- starting at `initial`.
    #[must_use]
    pub fn fixed_pitch(initial: FontChoice) -> Self {
        Self::over(
            initial,
            text::available_mono_families(),
            text::family_has_bold,
        )
    }

    /// A picker over `families`, in the order given, asking `has_bold`
    /// whether a family can be drawn bold. For a host that offers families
    /// of its own choosing, and for tests.
    #[must_use]
    pub fn over(initial: FontChoice, families: Vec<String>, has_bold: fn(&str) -> bool) -> Self {
        let mut initial = initial;
        initial.size = initial.size.clamp(1, MAX_SIZE);
        let mut size_text = TextInput::new();
        size_text.set_text(&initial.size.to_string());
        let mut picker = Self {
            bold: has_bold(&initial.family),
            original: initial.clone(),
            choice: initial,
            shown: (0..families.len()).collect(),
            families,
            search: TextInput::new(),
            family_view: ListViewport::new(0),
            size_text,
            size_view: ListViewport::new(0),
            focus: Part::Search,
            hovered: None,
            drag: None,
            wheel: wheel::Accumulator::default(),
            sample: DEFAULT_SAMPLE.to_string(),
            focus_ring: crate::style::FOCUS_RING_WIDTH,
            caret_width: textedit::CARET_WIDTH,
            has_bold,
            confirmed: false,
            cancelled: false,
        };
        let at = picker.position_of_choice();
        picker.family_view.select(at, picker.shown.len());
        let size_at = SIZES.iter().position(|&s| s == picker.choice.size);
        picker.size_view.select(size_at, SIZES.len());
        // Back to the top: selecting in a list with no rows yet scrolls the
        // choice to the top row, so the first frame would open the size list
        // at 13 with 8 to 12 hidden above it. From the top, the first height
        // a list is given scrolls only as far as its choice needs.
        picker.family_view.scroll_to(0, picker.shown.len());
        picker.size_view.scroll_to(0, SIZES.len());
        picker
    }

    /// Preview `sample` rather than [`DEFAULT_SAMPLE`] -- the host's own
    /// selected text, say. An empty sample keeps the default.
    #[must_use]
    pub fn with_sample(mut self, sample: impl Into<String>) -> Self {
        let sample = sample.into();
        if !sample.is_empty() {
            self.sample = sample;
        }
        self
    }

    /// Draw the focus ring this wide: the user's setting
    /// (`AppearanceSettings::focus_ring_width`).
    #[must_use]
    pub fn with_focus_ring(mut self, width: f32) -> Self {
        if width.is_finite() && width > 0.0 {
            self.focus_ring = width;
        }
        self
    }

    /// Draw the fields' caret this wide: the user's setting.
    #[must_use]
    pub fn with_caret_width(mut self, width: f32) -> Self {
        if width.is_finite() && width > 0.0 {
            self.caret_width = width;
        }
        self
    }

    /// The size the dialog is laid out for at the user's text size, for a
    /// host that sizes its window or overlay to it.
    #[must_use]
    pub fn preferred_size() -> (f32, f32) {
        (scaled(DIALOG_WIDTH), scaled(DIALOG_HEIGHT))
    }

    /// The choice so far -- after Cancel, the original again.
    #[must_use]
    pub fn choice(&self) -> &FontChoice {
        &self.choice
    }

    /// The choice the dialog opened with.
    #[must_use]
    pub fn original(&self) -> &FontChoice {
        &self.original
    }

    /// Whether the choice was kept.
    #[must_use]
    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// Whether the dialog was turned down.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Which part has the keyboard.
    #[must_use]
    pub fn focus(&self) -> Part {
        self.focus
    }

    /// What has been typed in the search field.
    #[must_use]
    pub fn search_text(&self) -> &str {
        self.search.text()
    }

    /// The families the search leaves, in the order listed.
    pub fn shown(&self) -> impl Iterator<Item = &str> {
        self.shown
            .iter()
            .filter_map(|&i| self.families.get(i).map(String::as_str))
    }

    /// The weights the family chosen can be drawn at: regular, and bold if
    /// it has a bold face. The one place that grows when a render tree can
    /// draw more weights (the module's "Only what can be drawn").
    #[must_use]
    pub fn weights(&self) -> &'static [FontWeightHint] {
        if self.bold {
            &[FontWeightHint::Regular, FontWeightHint::Bold]
        } else {
            &[FontWeightHint::Regular]
        }
    }

    /// Whether the size field holds a size. While it does not -- half typed,
    /// or out of range -- its edge is red and the size chosen is the last
    /// one it did hold.
    #[must_use]
    pub fn size_is_valid(&self) -> bool {
        parse_size(self.size_text.text()).is_some()
    }

    /// Whether the dialog is done with, kept or turned down: it takes no
    /// more events.
    fn finished(&self) -> bool {
        self.confirmed || self.cancelled
    }

    // ---------------------------------------------------------------------
    // Choosing
    // ---------------------------------------------------------------------

    /// Where the family chosen is among those shown, whatever its capitals.
    fn position_of_choice(&self) -> Option<usize> {
        let wanted = self.choice.family.to_lowercase();
        self.shown.iter().position(|&i| {
            self.families
                .get(i)
                .is_some_and(|name| name.to_lowercase() == wanted)
        })
    }

    /// The `i`th family shown is the choice now. A family with no bold takes
    /// the choice to regular in the same step: one change, one event.
    fn pick_family(&mut self, i: usize) -> Option<FontPickerEvent> {
        let name = self
            .shown
            .get(i)
            .and_then(|&f| self.families.get(f))?
            .clone();
        self.family_view.select(Some(i), self.shown.len());
        if name == self.choice.family {
            return None;
        }
        self.bold = (self.has_bold)(&name);
        if !self.bold && self.choice.weight == FontWeightHint::Bold {
            self.choice.weight = FontWeightHint::Regular;
        }
        self.choice.family = name;
        Some(self.changed())
    }

    /// `weight` is the choice now, if the family can be drawn at it.
    fn pick_weight(&mut self, weight: FontWeightHint) -> Option<FontPickerEvent> {
        if weight == self.choice.weight || !self.weights().contains(&weight) {
            return None;
        }
        self.choice.weight = weight;
        Some(self.changed())
    }

    /// `px` is the size now; `write` puts it in the size field too, which a
    /// size typed there does not want -- the field already says it.
    fn pick_size(&mut self, px: u16, write: bool) -> Option<FontPickerEvent> {
        if write {
            self.size_text.set_text(&px.to_string());
        }
        let at = SIZES.iter().position(|&s| s == px);
        self.size_view.select(at, SIZES.len());
        if px == self.choice.size {
            return None;
        }
        self.choice.size = px;
        Some(self.changed())
    }

    /// The next listed size above the size chosen, or below it.
    fn step_size(&mut self, larger: bool) -> Option<FontPickerEvent> {
        let now = self.choice.size;
        let to = if larger {
            SIZES.iter().copied().find(|&s| s > now)
        } else {
            SIZES.iter().rev().copied().find(|&s| s < now)
        }?;
        self.pick_size(to, true)
    }

    fn changed(&self) -> FontPickerEvent {
        FontPickerEvent::Changed(self.choice.clone())
    }

    fn confirm(&mut self) -> FontPickerEvent {
        self.confirmed = true;
        FontPickerEvent::Confirmed(self.choice.clone())
    }

    fn cancel(&mut self) -> FontPickerEvent {
        self.cancelled = true;
        self.choice = self.original.clone();
        FontPickerEvent::Cancelled
    }

    /// The families the search text leaves: those whose name starts with it
    /// first, then those that contain it, each in the order listed -- typing
    /// "ar" reaches Arial before Candara.
    fn filter(&mut self) {
        let query = self.search.text().trim().to_lowercase();
        let mut starting = Vec::new();
        let mut containing = Vec::new();
        for (i, name) in self.families.iter().enumerate() {
            let name = name.to_lowercase();
            if name.starts_with(&query) {
                starting.push(i);
            } else if name.contains(&query) {
                containing.push(i);
            }
        }
        starting.extend(containing);
        self.shown = starting;
    }

    /// After the search text changed: list what it leaves, keep the family
    /// chosen while it is listed, and otherwise land on the first listed --
    /// typing a family's name chooses it.
    fn searched(&mut self) -> Option<FontPickerEvent> {
        self.filter();
        // A new list, not an edited one: a scroll position into the old one
        // means nothing.
        self.family_view.reset();
        let len = self.shown.len();
        match self.position_of_choice() {
            Some(i) => {
                self.family_view.select(Some(i), len);
                None
            }
            None if len == 0 => None,
            None => self.pick_family(0),
        }
    }

    /// The viewports learn how many rows `layout` gives them -- only when
    /// that changed, because telling one its height scrolls it back to its
    /// selection, which would undo a wheel scroll at the next pointer move.
    fn fit(&mut self, layout: &Layout) {
        let families = layout.family_rows();
        if self.family_view.height() != families {
            self.family_view.set_height(families, self.shown.len());
        }
        let sizes = layout.size_rows();
        if self.size_view.height() != sizes {
            self.size_view.set_height(sizes, SIZES.len());
        }
    }

    // ---------------------------------------------------------------------
    // The keyboard
    // ---------------------------------------------------------------------

    /// Handle a key for a dialog drawn `width` by `height` -- the size, so
    /// Page Up and Page Down move by the rows on screen.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        width: f32,
        height: f32,
    ) -> Option<FontPickerEvent> {
        if !key.pressed || self.finished() {
            return None;
        }
        self.fit(&Layout::new(width, height));
        match key.key {
            Key::Escape => return Some(self.cancel()),
            Key::Enter => {
                return Some(if self.focus == Part::Cancel {
                    self.cancel()
                } else {
                    self.confirm()
                });
            }
            Key::Tab if !key.modifiers.ctrl && !key.modifiers.alt => {
                self.focus = self.focus.step(key.modifiers.shift);
                self.entered();
                return None;
            }
            _ => {}
        }
        match self.focus {
            Part::Search => self.search_key(key),
            Part::Families => self.families_key(key),
            Part::Weights => self.weights_key(key),
            Part::SizeField => self.size_field_key(key),
            Part::Sizes => self.sizes_key(key),
            Part::Ok => (key.key == Key::Space).then(|| self.confirm()),
            Part::Cancel => (key.key == Key::Space).then(|| self.cancel()),
        }
    }

    /// A field the keyboard has just come to holds its text selected, so
    /// what is typed replaces it: the size field's "13" by "14", not "1314".
    fn entered(&mut self) {
        match self.focus {
            Part::Search => self.search.select_all(),
            Part::SizeField => self.size_text.select_all(),
            Part::Families | Part::Weights | Part::Sizes | Part::Ok | Part::Cancel => {}
        }
    }

    fn search_key(&mut self, key: &KeyEvent) -> Option<FontPickerEvent> {
        // Up, Down and the page keys move through the families from the
        // field; Home and End stay the field's, for its caret.
        if matches!(key.key, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
            return self.move_family(ListKey::of(key)?);
        }
        match self
            .search
            .edit_key(key, scaled(FIELD_TEXT_SIZE), FontWeightHint::Regular)
        {
            KeyEdit::Changed => self.searched(),
            KeyEdit::Handled | KeyEdit::Unhandled => None,
        }
    }

    fn families_key(&mut self, key: &KeyEvent) -> Option<FontPickerEvent> {
        if let Some(list_key) = ListKey::of(key) {
            return self.move_family(list_key);
        }
        // Typing finds a family, as in any font list: the text goes to the
        // search, and the keyboard with it.
        let typed = !key.text.is_empty()
            && !key.modifiers.is_ctrl_chord()
            && !key.text.chars().any(char::is_control);
        if !typed {
            return None;
        }
        self.focus = Part::Search;
        match self
            .search
            .edit_key(key, scaled(FIELD_TEXT_SIZE), FontWeightHint::Regular)
        {
            KeyEdit::Changed => self.searched(),
            KeyEdit::Handled | KeyEdit::Unhandled => None,
        }
    }

    fn move_family(&mut self, key: ListKey) -> Option<FontPickerEvent> {
        self.family_view.go(key, self.shown.len());
        let i = self.family_view.selected()?;
        self.pick_family(i)
    }

    fn weights_key(&mut self, key: &KeyEvent) -> Option<FontPickerEvent> {
        let weights = self.weights();
        let from = weights.iter().position(|&w| w == self.choice.weight);
        let to = ListKey::of(key)?.target(from, weights.len(), weights.len())?;
        self.pick_weight(*weights.get(to)?)
    }

    fn size_field_key(&mut self, key: &KeyEvent) -> Option<FontPickerEvent> {
        match key.key {
            // Down the list for larger, as the list below the field runs.
            Key::Down => return self.step_size(true),
            Key::Up => return self.step_size(false),
            _ => {}
        }
        match self
            .size_text
            .edit_key(key, scaled(FIELD_TEXT_SIZE), FontWeightHint::Regular)
        {
            KeyEdit::Changed => {
                let px = parse_size(self.size_text.text())?;
                self.pick_size(px, false)
            }
            KeyEdit::Handled | KeyEdit::Unhandled => None,
        }
    }

    fn sizes_key(&mut self, key: &KeyEvent) -> Option<FontPickerEvent> {
        match ListKey::of(key)? {
            // From a typed size the list does not have, as from one it has:
            // the next listed size above it, or below.
            ListKey::Next => self.step_size(true),
            ListKey::Previous => self.step_size(false),
            list_key => {
                self.size_view.go(list_key, SIZES.len());
                let px = *SIZES.get(self.size_view.selected()?)?;
                self.pick_size(px, true)
            }
        }
    }

    // ---------------------------------------------------------------------
    // The pointer
    // ---------------------------------------------------------------------

    /// Handle a mouse event at coordinates relative to a dialog drawn `width`
    /// by `height` -- the size the host draws it at, which the event is
    /// tested against. Forward every event while the dialog is up, moves and
    /// releases too: a scrollbar is dragged by them.
    pub fn handle_mouse(
        &mut self,
        event: &MouseEvent,
        width: f32,
        height: f32,
    ) -> Option<FontPickerEvent> {
        if self.finished() {
            return None;
        }
        self.fit(&Layout::new(width, height));
        // Walked for its hit boxes: they do not depend on the colours, so
        // any palette answers alike (as `FileDialog::handle_mouse` explains).
        let frame = self.frame(&Palette::for_mode(false), width, height);
        let target = frame.hit_test(event.x, event.y);
        match event.kind {
            MouseEventKind::Move | MouseEventKind::Enter => {
                self.hovered = target;
                if let Some(drag) = self.drag {
                    self.drag_thumb(&frame, drag, event.y);
                }
                None
            }
            MouseEventKind::Leave => {
                self.hovered = None;
                None
            }
            MouseEventKind::Press(MouseButton::Left) => self.press(&frame, target, event.y),
            MouseEventKind::Release(MouseButton::Left) => {
                self.drag = None;
                None
            }
            // The second click of a double-click on a family keeps it; the
            // first has already chosen it.
            MouseEventKind::DoubleClick(MouseButton::Left) => match target {
                Some(Target::Family(i)) => {
                    let _changed = self.pick_family(i);
                    Some(self.confirm())
                }
                _ => None,
            },
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.wheel.rows(dy);
                match target {
                    Some(
                        Target::Family(_)
                        | Target::Track(List::Families)
                        | Target::Thumb(List::Families),
                    ) => {
                        self.family_view.scroll_by(rows, self.shown.len());
                    }
                    Some(
                        Target::Size(_) | Target::Track(List::Sizes) | Target::Thumb(List::Sizes),
                    ) => {
                        self.size_view.scroll_by(rows, SIZES.len());
                    }
                    _ => {}
                }
                None
            }
            _ => None,
        }
    }

    fn press(
        &mut self,
        frame: &Frame<Target>,
        target: Option<Target>,
        y: f32,
    ) -> Option<FontPickerEvent> {
        match target? {
            Target::Search => {
                self.focus = Part::Search;
                None
            }
            Target::Family(i) => {
                self.focus = Part::Families;
                self.pick_family(i)
            }
            Target::Weight(weight) => {
                self.focus = Part::Weights;
                self.pick_weight(weight)
            }
            Target::SizeField => {
                // A press selects the size, as Tab does: a size field is
                // retyped whole far more often than edited.
                if self.focus != Part::SizeField {
                    self.focus = Part::SizeField;
                    self.entered();
                }
                None
            }
            Target::Size(i) => {
                self.focus = Part::Sizes;
                let px = *SIZES.get(i)?;
                self.pick_size(px, true)
            }
            Target::Thumb(list) => {
                let thumb = frame.rect_of(|t| *t == Target::Thumb(list))?;
                self.drag = Some(ThumbDrag {
                    list,
                    grab: y - thumb.y,
                });
                None
            }
            Target::Track(list) => {
                // A press beside the thumb pages towards the press.
                let thumb = frame.rect_of(|t| *t == Target::Thumb(list))?;
                let (view, len) = self.view_of(list);
                let page = isize::try_from(view.height().max(1)).unwrap_or(isize::MAX);
                let delta = if y < thumb.y {
                    page.saturating_neg()
                } else {
                    page
                };
                view.scroll_by(delta, len);
                None
            }
            Target::Ok => Some(self.confirm()),
            Target::Cancel => Some(self.cancel()),
            Target::Dialog => None,
        }
    }

    /// A list's viewport and its length.
    fn view_of(&mut self, list: List) -> (&mut ListViewport, usize) {
        match list {
            List::Families => (&mut self.family_view, self.shown.len()),
            List::Sizes => (&mut self.size_view, SIZES.len()),
        }
    }

    fn drag_thumb(&mut self, frame: &Frame<Target>, drag: ThumbDrag, y: f32) {
        let (Some(track), Some(thumb)) = (
            frame.rect_of(|t| *t == Target::Track(drag.list)),
            frame.rect_of(|t| *t == Target::Thumb(drag.list)),
        ) else {
            return;
        };
        let (view, len) = self.view_of(drag.list);
        let capacity = view.height();
        if let Some(first) = scrollbar::first_from_drag(track, thumb.h, drag.grab, y, len, capacity)
        {
            view.scroll_to(first, len);
        }
    }

    // ---------------------------------------------------------------------
    // Drawing
    // ---------------------------------------------------------------------

    /// Draw the dialog `width` by `height`.
    #[must_use]
    pub fn render(&self, palette: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(palette, width, height).into_tree().commands
    }

    /// Draw the dialog `width` by `height`, recording what each part of it
    /// is clicked to reach -- the boxes the handlers test the pointer
    /// against, so nothing is drawn in one place and clicked in another
    /// ([`crate::frame`]).
    #[must_use]
    pub fn frame(&self, palette: &Palette, width: f32, height: f32) -> Frame<Target> {
        let layout = Layout::new(width, height);
        let mut frame = Frame::new(width, height);
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: palette.base,
            corner_radii: CornerRadii::all(scaled(RADIUS)),
        });
        frame.hit(Target::Dialog, Rect::new(0.0, 0.0, width, height));
        self.draw_title(palette, &mut frame, &layout);
        for (rect, label) in [
            (layout.family_label, "Family"),
            (layout.weight_label, "Weight"),
            (layout.size_label, "Size"),
        ] {
            let size = scaled(LABEL_SIZE);
            frame.push(RenderCommand::Text {
                x: rect.x,
                y: centred(rect, size, FontWeightHint::Regular, FontFamily::Ui),
                text: label.to_string(),
                color: palette.ink(palette.subtext0),
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(rect.w),
                overflow: TextOverflow::Ellipsis,
            });
        }
        self.draw_field(palette, &mut frame, layout.search, Target::Search);
        self.draw_families(palette, &mut frame, layout.families);
        self.draw_weights(palette, &mut frame, layout.weights);
        self.draw_field(palette, &mut frame, layout.size_field, Target::SizeField);
        self.draw_sizes(palette, &mut frame, layout.sizes);
        self.draw_preview(palette, &mut frame, layout.preview);
        self.draw_buttons(palette, &mut frame, &layout);
        frame
    }

    fn draw_title(&self, palette: &Palette, frame: &mut Frame<Target>, layout: &Layout) {
        let r = scaled(RADIUS);
        let bar = layout.title;
        frame.push(RenderCommand::FillRect {
            x: bar.x,
            y: bar.y,
            width: bar.w,
            height: bar.h,
            color: palette.surface0,
            corner_radii: CornerRadii {
                top_left: r,
                top_right: r,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
        });
        let size = scaled(TITLE_SIZE);
        let pad = scaled(PADDING);
        let title = if self.families.is_empty() {
            "Font -- no fonts are installed"
        } else {
            "Font"
        };
        frame.push(RenderCommand::Text {
            x: bar.x + pad,
            y: centred(bar, size, FontWeightHint::Bold, FontFamily::Ui),
            text: title.to_string(),
            color: palette.ink(palette.text),
            font_size: size,
            font_weight: FontWeightHint::Bold,
            max_width: Some((bar.w - 2.0 * pad).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The search field or the size field, with what is typed in it, its
    /// selection and its caret.
    fn draw_field(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect, target: Target) {
        let (input, part, invalid, placeholder) = if target == Target::Search {
            (&self.search, Part::Search, false, "Search")
        } else {
            (&self.size_text, Part::SizeField, !self.size_is_valid(), "")
        };
        let focused = self.focus == part;
        field::draw(
            frame,
            palette,
            rect,
            field::State {
                hovered: self.hovered == Some(target),
                focused,
                disabled: false,
                invalid,
            },
            self.focus_ring,
        );
        frame.hit(target, rect);
        let size = scaled(FIELD_TEXT_SIZE);
        let pad = scaled(FIELD_PADDING);
        let line = text::line_height(size, FontWeightHint::Regular);
        let inner = Rect::new(
            rect.x + pad,
            rect.y + (rect.h - line) / 2.0,
            (rect.w - 2.0 * pad).max(0.0),
            line,
        );
        if input.text().is_empty() && !placeholder.is_empty() {
            frame.push(RenderCommand::Text {
                x: inner.x,
                y: inner.y,
                text: placeholder.to_string(),
                color: palette.ink(palette.subtext0),
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(inner.w),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let mut tree = RenderTree::new();
        textedit::draw(
            &mut tree,
            &SingleLine {
                text: input.text(),
                cursor: input.cursor(),
                selection_anchor: input.selection_anchor(),
                focused,
                x: inner.x,
                y: inner.y,
                width: inner.w,
                line_height: line,
                font_size: size,
                weight: FontWeightHint::Regular,
                color: palette.ink(palette.text),
                selection_bg: palette.accent,
                selection_fg: crate::palette::readable_on(palette.accent),
                caret_width: self.caret_width,
            },
        );
        for command in tree.commands {
            frame.push(command);
        }
    }

    /// A list's well, and the focus ring round it when it has the keyboard.
    fn draw_well(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect, focused: bool) {
        let radii = CornerRadii::all(scaled(WELL_RADIUS));
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: palette.crust,
            corner_radii: radii,
        });
        let (edge, width) = if focused {
            (palette.accent, self.focus_ring)
        } else {
            (palette.surface1, 1.0)
        };
        frame.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: edge,
            line_width: width,
            corner_radii: radii,
        });
    }

    /// A row's ground: the selection's, the pointer's, or none.
    fn draw_row_ground(
        palette: &Palette,
        frame: &mut Frame<Target>,
        row: Rect,
        selected: bool,
        hovered: bool,
    ) {
        let color = if selected {
            palette.selection_fill()
        } else if hovered {
            palette.surface0
        } else {
            return;
        };
        frame.push(RenderCommand::FillRect {
            x: row.x,
            y: row.y,
            width: row.w,
            height: row.h,
            color,
            corner_radii: CornerRadii::ZERO,
        });
    }

    /// A list's scrollbar, if it needs one, in a column at the right of
    /// `rect`; returns the column's width, for the rows to stop short of.
    fn draw_bar(
        &self,
        palette: &Palette,
        frame: &mut Frame<Target>,
        rect: Rect,
        list: List,
        (len, capacity, first): (usize, usize, usize),
    ) -> f32 {
        if !scrollbar::needed(len, capacity) {
            return 0.0;
        }
        let w = scaled(BAR_WIDTH).min(rect.w);
        let track = Rect::new(rect.right() - w, rect.y, w, rect.h);
        let thumb = scrollbar::thumb(track, len, capacity, first);
        scrollbar::draw(
            frame,
            palette,
            track,
            thumb,
            scrollbar::BarState {
                hovered: matches!(self.hovered, Some(Target::Track(l) | Target::Thumb(l)) if l == list),
                dragging: self.drag.is_some_and(|d| d.list == list),
            },
        );
        frame.hit(Target::Track(list), track);
        frame.hit(Target::Thumb(list), thumb);
        w
    }

    fn draw_families(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect) {
        let focused = self.focus == Part::Families;
        self.draw_well(palette, frame, rect, focused);
        let row_h = scaled(FAMILY_ROW);
        let len = self.shown.len();
        let capacity = rows_in(rect.h, row_h);
        let rows = shown_rows(self.family_view, len, capacity);
        if len == 0 {
            let size = scaled(ROW_SIZE);
            let pad = scaled(ROW_PADDING);
            let message = if self.families.is_empty() {
                "No fonts are installed"
            } else {
                "No family matches"
            };
            frame.push(RenderCommand::Text {
                x: rect.x + pad,
                y: rect.y + pad,
                text: message.to_string(),
                color: palette.ink(palette.subtext0),
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some((rect.w - 2.0 * pad).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }
        let bar = self.draw_bar(
            palette,
            frame,
            rect,
            List::Families,
            (len, capacity, rows.start),
        );
        let size = scaled(FAMILY_SIZE);
        let pad = scaled(ROW_PADDING);
        frame.clip(rect);
        for (slot, i) in rows.enumerate() {
            let Some(name) = self.shown.get(i).and_then(|&f| self.families.get(f)) else {
                continue;
            };
            let row = Rect::new(
                rect.x,
                row_top(rect.y, slot, row_h),
                (rect.w - bar).max(0.0),
                row_h,
            );
            Self::draw_row_ground(
                palette,
                frame,
                row,
                self.family_view.selected() == Some(i),
                self.hovered == Some(Target::Family(i)),
            );
            // Each name in its own face, clipped to its row: a face with a
            // tall line must not draw into the rows beside it.
            let family = FontFamily::named(name).unwrap_or(FontFamily::Ui);
            frame.clip(row);
            frame.push(RenderCommand::PushFont { family });
            frame.push(RenderCommand::Text {
                x: row.x + pad,
                y: centred(row, size, FontWeightHint::Regular, family),
                text: name.clone(),
                color: palette.ink(palette.text),
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some((row.w - 2.0 * pad).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.push(RenderCommand::PopFont);
            frame.unclip();
            frame.hit(Target::Family(i), row);
        }
        frame.unclip();
    }

    fn draw_weights(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect) {
        self.draw_well(palette, frame, rect, self.focus == Part::Weights);
        let row_h = scaled(ROW);
        let size = scaled(ROW_SIZE);
        let pad = scaled(ROW_PADDING);
        let family = self.choice.font_family();
        frame.clip(rect);
        for (slot, &weight) in self.weights().iter().enumerate() {
            let row = Rect::new(rect.x, row_top(rect.y, slot, row_h), rect.w, row_h);
            Self::draw_row_ground(
                palette,
                frame,
                row,
                self.choice.weight == weight,
                self.hovered == Some(Target::Weight(weight)),
            );
            // Each weight's name at that weight, in the family chosen.
            frame.push(RenderCommand::PushFont { family });
            frame.push(RenderCommand::Text {
                x: row.x + pad,
                y: centred(row, size, weight, family),
                text: weight_name(weight).to_string(),
                color: palette.ink(palette.text),
                font_size: size,
                font_weight: weight,
                max_width: Some((row.w - 2.0 * pad).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.push(RenderCommand::PopFont);
            frame.hit(Target::Weight(weight), row);
        }
        frame.unclip();
    }

    fn draw_sizes(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect) {
        self.draw_well(palette, frame, rect, self.focus == Part::Sizes);
        let row_h = scaled(ROW);
        let len = SIZES.len();
        let capacity = rows_in(rect.h, row_h);
        let rows = shown_rows(self.size_view, len, capacity);
        let bar = self.draw_bar(
            palette,
            frame,
            rect,
            List::Sizes,
            (len, capacity, rows.start),
        );
        let size = scaled(ROW_SIZE);
        let pad = scaled(ROW_PADDING);
        frame.clip(rect);
        for (slot, i) in rows.enumerate() {
            let Some(px) = SIZES.get(i) else {
                continue;
            };
            let row = Rect::new(
                rect.x,
                row_top(rect.y, slot, row_h),
                (rect.w - bar).max(0.0),
                row_h,
            );
            Self::draw_row_ground(
                palette,
                frame,
                row,
                self.choice.size == *px,
                self.hovered == Some(Target::Size(i)),
            );
            frame.push(RenderCommand::Text {
                x: row.x + pad,
                y: centred(row, size, FontWeightHint::Regular, FontFamily::Ui),
                text: px.to_string(),
                color: palette.ink(palette.text),
                font_size: size,
                font_weight: FontWeightHint::Regular,
                max_width: Some((row.w - 2.0 * pad).max(0.0)),
                overflow: TextOverflow::Clip,
            });
            frame.hit(Target::Size(i), row);
        }
        frame.unclip();
    }

    /// The sample in the choice so far, at its own size -- not the user's
    /// text size, which scales the dialog and not the font being chosen --
    /// centred, and cut at the box: a preview larger than its box shows what
    /// fits, which is still the face.
    fn draw_preview(&self, palette: &Palette, frame: &mut Frame<Target>, rect: Rect) {
        self.draw_well(palette, frame, rect, false);
        let family = self.choice.font_family();
        let size = self.choice.font_size();
        let pad = scaled(ROW_PADDING);
        frame.clip(rect);
        frame.push(RenderCommand::PushFont { family });
        frame.push(RenderCommand::Text {
            x: rect.x + pad,
            y: centred(rect, size, self.choice.weight, family),
            text: self.sample.clone(),
            color: palette.ink(palette.text),
            font_size: size,
            font_weight: self.choice.weight,
            max_width: Some((rect.w - 2.0 * pad).max(0.0)),
            overflow: TextOverflow::Clip,
        });
        frame.push(RenderCommand::PopFont);
        frame.unclip();
    }

    fn draw_buttons(&self, palette: &Palette, frame: &mut Frame<Target>, layout: &Layout) {
        for (rect, label, kind, part, target) in [
            (layout.ok, "OK", button::Kind::Primary, Part::Ok, Target::Ok),
            (
                layout.cancel,
                "Cancel",
                button::Kind::Plain,
                Part::Cancel,
                Target::Cancel,
            ),
        ] {
            button::draw(
                frame,
                palette,
                (rect.x, rect.y, rect.w, rect.h),
                label,
                kind,
                button::State {
                    hovered: self.hovered == Some(target),
                    pressed: false,
                    disabled: false,
                    focused: self.focus == part,
                },
                palette.base,
                self.focus_ring,
            );
            frame.hit(target, rect);
        }
    }
}

#[cfg(test)]
#[path = "fontpicker_tests.rs"]
mod tests;
