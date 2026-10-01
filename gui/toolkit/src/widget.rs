//! Widget system — the UI building blocks.
//!
//! Widgets form a tree. Each widget has:
//! - An ID (stable across frames for state tracking)
//! - A style
//! - Layout properties (flex item + optional flex container)
//! - Content (text, children, etc.)
//! - Event handlers
//!
//! The widget tree is rebuilt each frame (immediate-mode-inspired),
//! but widget state (focus, text cursor, scroll position) persists
//! via the WidgetId.
//!
//! # Drawing
//!
//! A [`WidgetTree`] draws in the palette its program gives it
//! ([`WidgetTree::set_palette`]), and every control in it through the
//! toolkit's own component module for that control -- `button`, `checkbox`,
//! `radio`, `slider`, `field` and `textedit`, `textarea`, `scrollbar` (the
//! `draw` submodule) -- so a control looks the same in a tree as anywhere
//! else, and follows the user's theme. A colour a widget's [`Style`] sets is
//! its program's choice; one it leaves unset is the palette's. The theme's
//! widget style sizes the controls too: a button is as wide as
//! [`crate::button::width`] makes it.
//!
//! A [`WidgetKind::ScrollView`] lays what it holds out at its own width and as
//! tall as it needs, scrolls it under the wheel (sideways under its tilt),
//! cuts it to its box, and draws its bars: down its right edge for content
//! taller than it, across its foot for content wider.

mod draw;
#[cfg(test)]
mod tree_tests;

use crate::color::Color;
use crate::event::{Event, EventResult, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::layout::{
    FlexAlign, FlexDirection, FlexItem, FlexJustify, FlexLayout, LayoutBox, Size, SizeConstraint,
    flex_layout,
};
use crate::palette::Palette;
use crate::render::{FontWeightHint, RenderCommand, RenderTree};
use crate::style::{CornerRadii, Edges, FontWeight, Style};
use crate::text::TextCursor;

/// How tall a progress bar is by default.
const PROGRESS_HEIGHT: f32 = 12.0;

/// A slider's thumb, across, and its track's thickness: the sizes the shell's
/// own sliders are drawn at.
const SLIDER_THUMB: f32 = 14.0;
const SLIDER_TRACK: f32 = 4.0;

/// How tall a slider's row is: its thumb with room for the light round it.
const SLIDER_HEIGHT: f32 = 20.0;

/// Unique widget identifier. Used to track persistent state across frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId(pub u64);

impl WidgetId {
    /// Generate a new unique ID.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for WidgetId {
    fn default() -> Self {
        Self::new()
    }
}

/// A widget in the UI tree.
#[derive(Clone, Debug)]
pub struct Widget {
    pub id: WidgetId,
    pub kind: WidgetKind,
    pub style: Style,
    pub flex_item: FlexItem,
    pub flex_layout: Option<FlexLayout>,
    pub children: Vec<Widget>,
    /// Computed layout (set by layout pass).
    pub layout: LayoutBox,
    /// Whether this widget is enabled (accepts input).
    pub enabled: bool,
    /// Whether this widget is visible.
    pub visible: bool,
    /// Whether this widget holds the keyboard focus.
    ///
    /// Read it with [`Widget::is_focused`]; set it through
    /// [`WidgetTree::focus`], never by hand. The tree owns the invariant that
    /// at most one widget in it is focused, and a field written directly can
    /// break that invariant in a way nothing detects until two carets blink at
    /// the user.
    ///
    /// It is a field rather than an id held by the tree because `render` takes
    /// `&self` and gets no tree, so a widget that could not answer "am I
    /// focused?" from its own state could not decide whether to draw a caret
    /// without threading the focused id through every render call.
    focused: bool,
    /// Whether the pointer is over it: what lights a button or a box. Kept
    /// by [`WidgetTree::handle_event`] from the pointer's moves, as `focused`
    /// is from its presses.
    hovered: bool,
    /// Tooltip text.
    pub tooltip: Option<String>,
}

/// Widget content type.
#[derive(Clone, Debug)]
pub enum WidgetKind {
    /// A container (panel, group) — just holds children with layout.
    Container,
    /// Text label.
    Label { text: String },
    /// Clickable button.
    Button { text: String, pressed: bool },
    /// Single-line text input.
    TextInput {
        value: String,
        placeholder: String,
        /// Where the caret is — a byte offset *and* which side of a direction
        /// boundary it belongs to. Not a bare `usize`: an offset where two
        /// directions meet names two different places on the screen, and a
        /// caret rebuilt from the offset alone steps straight over a
        /// right-to-left word instead of through it. See
        /// `text::dropping_the_affinity_between_steps_skips_the_reordered_run`.
        cursor: TextCursor,
        /// Where a selection started, if one is being made — a plain byte
        /// offset, deliberately, because a selection is a *range of text* and
        /// a range has no side of a direction boundary to be on. The other end
        /// is always `cursor`, so the two cannot drift apart; the selected
        /// range is `anchor..cursor.byte()` in either order. This is the shape
        /// the Run dialog already uses (`gui/desktop/src/run_dialog.rs`).
        ///
        /// It replaces a `selection: Option<(usize, usize)>` that was declared
        /// and never once read or written. A pair can disagree with the caret;
        /// an anchor cannot.
        selection_anchor: Option<usize>,
    },
    /// Multi-line text: the toolkit's text area ([`crate::textarea`]) holds the
    /// text, the caret, the selection, the undo history and the scroll.
    TextArea {
        area: crate::textarea::TextArea,
        /// Shown while it is empty: what it is for.
        placeholder: String,
        /// Whether a press in it is held: the pointer's moves select text
        /// until it is let go.
        selecting: bool,
    },
    /// Checkbox.
    Checkbox { checked: CheckState, label: String },
    /// One of a group of choices. Its group is its siblings that are radio
    /// buttons: choosing it clears theirs.
    RadioButton { selected: bool, label: String },
    /// What it holds, scrolled: laid out as wide as this widget's content and
    /// as tall as it needs, and shown through this widget's box.
    ScrollView {
        scroll_x: f32,
        scroll_y: f32,
        /// How wide and tall what it holds is, from the last layout.
        content_width: f32,
        content_height: f32,
    },
    /// Horizontal or vertical separator line.
    Separator { vertical: bool },
    /// Progress bar.
    ProgressBar { value: f32, max: f32 },
    /// A value between two bounds: the toolkit's slider ([`crate::slider`])
    /// holds it and any drag changing it.
    Slider { slider: crate::slider::Slider },
    /// A picture the host has uploaded under `image_id`, shown at `width` by
    /// `height`.
    Image {
        image_id: u64,
        width: f32,
        height: f32,
    },
}

/// Checkbox state (supports tristate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Unchecked,
    Checked,
    Indeterminate,
}

impl CheckState {
    /// The state a click leaves the box in: ticked becomes unticked, and
    /// anything else — unticked, or partly ticked — becomes ticked.
    ///
    /// The one statement of the rule. The checkbox widget's mouse and keyboard
    /// handlers each spelled it out, and a tree's boxes (`treeview`) need it
    /// too; three copies of a rule is three chances for one of them to decide
    /// that a click on a partial box clears it.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Checked => Self::Unchecked,
            Self::Unchecked | Self::Indeterminate => Self::Checked,
        }
    }
}

impl Widget {
    // ======================================================================
    // Constructors
    // ======================================================================

    /// The fields every widget has regardless of what it draws: a fresh id, no
    /// children, no layout yet, enabled, visible, unfocused.
    ///
    /// Every constructor below builds on this rather than spelling the tail
    /// out, because the alternative had already cost something: with seven
    /// copies of the same eight lines, adding one field to `Widget` meant seven
    /// edits, and missing one is a compile error only until someone reaches for
    /// `..Default::default()` to silence it. `focused` was the field that made
    /// the point — it has to start `false` in all seven, and there is exactly
    /// one place here that says so.
    fn bare(kind: WidgetKind) -> Self {
        Self {
            id: WidgetId::new(),
            kind,
            style: Style::default(),
            flex_item: FlexItem::default(),
            flex_layout: None,
            children: Vec::new(),
            layout: LayoutBox::default(),
            enabled: true,
            visible: true,
            focused: false,
            hovered: false,
            tooltip: None,
        }
    }

    pub fn container() -> Self {
        Self {
            flex_layout: Some(FlexLayout::default()),
            ..Self::bare(WidgetKind::Container)
        }
    }

    pub fn label(text: &str) -> Self {
        Self::bare(WidgetKind::Label {
            text: text.to_string(),
        })
    }

    /// A button, drawn by the toolkit's buttons ([`crate::button`]) in the
    /// theme's shape and the palette's colours, and as wide as they make it.
    pub fn button(text: &str) -> Self {
        Self::bare(WidgetKind::Button {
            text: text.to_string(),
            pressed: false,
        })
    }

    /// A one-line text field, drawn in the theme's field ([`crate::field`]).
    pub fn text_input(value: &str, placeholder: &str) -> Self {
        Self {
            style: Style {
                padding: Edges::symmetric(4.0, 8.0),
                min_width: Some(120.0),
                min_height: Some(28.0),
                ..Style::default()
            },
            flex_item: FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
            ..Self::bare(WidgetKind::TextInput {
                value: value.to_string(),
                placeholder: placeholder.to_string(),
                cursor: TextCursor::from(value.len()),
                selection_anchor: None,
            })
        }
    }

    /// A check box with its label, drawn by the toolkit's check boxes
    /// ([`crate::checkbox`]).
    pub fn checkbox(label: &str, checked: bool) -> Self {
        Self::bare(WidgetKind::Checkbox {
            checked: if checked {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            },
            label: label.to_string(),
        })
    }

    /// A choice among its sibling radio buttons, drawn by the toolkit's
    /// ([`crate::radio`]).
    pub fn radio(label: &str, selected: bool) -> Self {
        Self::bare(WidgetKind::RadioButton {
            selected,
            label: label.to_string(),
        })
    }

    /// A multi-line text field holding `text`, with `placeholder` shown while
    /// it is empty.
    pub fn text_area(text: &str, placeholder: &str) -> Self {
        Self {
            style: Style {
                padding: Edges::symmetric(4.0, 8.0),
                min_width: Some(200.0),
                min_height: Some(80.0),
                ..Style::default()
            },
            flex_item: FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
            ..Self::bare(WidgetKind::TextArea {
                area: crate::textarea::TextArea::with_text(text),
                placeholder: placeholder.to_string(),
                selecting: false,
            })
        }
    }

    /// A box that scrolls what it holds, laid out in a column: add the
    /// content with [`with_child`](Self::with_child).
    pub fn scroll_view() -> Self {
        Self {
            flex_layout: Some(FlexLayout {
                direction: FlexDirection::Column,
                ..FlexLayout::default()
            }),
            flex_item: FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
            ..Self::bare(WidgetKind::ScrollView {
                scroll_x: 0.0,
                scroll_y: 0.0,
                content_width: 0.0,
                content_height: 0.0,
            })
        }
    }

    /// A slider from `min` to `max` at `value`, drawn and moved by the
    /// toolkit's sliders ([`crate::slider`]).
    pub fn slider(min: f64, max: f64, value: f64) -> Self {
        Self {
            style: Style {
                min_width: Some(120.0),
                ..Style::default()
            },
            ..Self::bare(WidgetKind::Slider {
                slider: crate::slider::Slider::new(min, max, value),
            })
        }
    }

    /// The picture the host uploaded under `image_id`, `width` by `height`.
    pub fn image(image_id: u64, width: f32, height: f32) -> Self {
        Self::bare(WidgetKind::Image {
            image_id,
            width,
            height,
        })
    }

    /// A bar `value` of `max` full, in the palette's accent.
    pub fn progress_bar(value: f32, max: f32) -> Self {
        Self {
            style: Style {
                min_height: Some(PROGRESS_HEIGHT),
                ..Style::default()
            },
            flex_item: FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
            ..Self::bare(WidgetKind::ProgressBar { value, max })
        }
    }

    /// A line across its container, in the palette's line colour.
    pub fn separator() -> Self {
        Self {
            style: Style {
                margin: Edges::symmetric(8.0, 0.0),
                min_height: Some(1.0),
                ..Style::default()
            },
            flex_item: FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
            ..Self::bare(WidgetKind::Separator { vertical: false })
        }
    }

    // ======================================================================
    // Builder methods
    // ======================================================================

    pub fn with_id(mut self, id: WidgetId) -> Self {
        self.id = id;
        self
    }

    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn with_padding(mut self, padding: Edges) -> Self {
        self.style.padding = padding;
        self
    }

    pub fn with_margin(mut self, margin: Edges) -> Self {
        self.style.margin = margin;
        self
    }

    pub fn with_background(mut self, color: Color) -> Self {
        self.style.background = color;
        self
    }

    pub fn with_flex_grow(mut self, grow: f32) -> Self {
        self.flex_item.grow = grow;
        self
    }

    pub fn with_flex_direction(mut self, direction: FlexDirection) -> Self {
        if let Some(ref mut layout) = self.flex_layout {
            layout.direction = direction;
        } else {
            self.flex_layout = Some(FlexLayout {
                direction,
                ..FlexLayout::default()
            });
        }
        self
    }

    pub fn with_gap(mut self, gap: f32) -> Self {
        if let Some(ref mut layout) = self.flex_layout {
            layout.gap = gap;
        } else {
            self.flex_layout = Some(FlexLayout {
                gap,
                ..FlexLayout::default()
            });
        }
        self
    }

    pub fn with_justify(mut self, justify: FlexJustify) -> Self {
        if let Some(ref mut layout) = self.flex_layout {
            layout.justify = justify;
        }
        self
    }

    pub fn with_align(mut self, align: FlexAlign) -> Self {
        if let Some(ref mut layout) = self.flex_layout {
            layout.align_items = align;
        }
        self
    }

    pub fn with_child(mut self, child: Widget) -> Self {
        self.children.push(child);
        self
    }

    pub fn with_children(mut self, children: Vec<Widget>) -> Self {
        self.children = children;
        self
    }

    pub fn with_tooltip(mut self, tip: &str) -> Self {
        self.tooltip = Some(tip.to_string());
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }

    // ======================================================================
    // Focus
    // ======================================================================
    //
    // There was none of this before, and the absence was not a missing feature
    // but a live bug. `handle_event` walked the children back to front and gave
    // the event to the first that took it, for *every* kind of event — so on a
    // form with two text fields, every keystroke went to whichever field came
    // last in the tree, no matter which one the user had clicked. Typing into
    // the first field of a two-field dialog put the characters in the second.
    //
    // Focus is the fix, and it also answers the question the caret could not be
    // drawn without: *which* field draws one.

    /// Whether this widget can take the keyboard focus.
    ///
    /// Buttons and checkboxes are included, not just text fields, because
    /// otherwise Tab has nothing to stop at between two text fields and a
    /// keyboard-only user cannot press the button they just filled the form
    /// for. A widget that is disabled or hidden is skipped: focus that lands
    /// somewhere the user cannot see is focus that has vanished.
    #[must_use]
    pub const fn accepts_focus(&self) -> bool {
        self.enabled
            && self.visible
            && matches!(
                self.kind,
                WidgetKind::TextInput { .. }
                    | WidgetKind::TextArea { .. }
                    | WidgetKind::Button { .. }
                    | WidgetKind::Checkbox { .. }
                    | WidgetKind::RadioButton { .. }
                    | WidgetKind::Slider { .. }
            )
    }

    /// Whether this widget currently holds the keyboard focus.
    #[must_use]
    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// The id of the focused widget in this subtree, if any.
    #[must_use]
    pub fn focused_id(&self) -> Option<WidgetId> {
        if self.focused {
            return Some(self.id);
        }
        self.children.iter().find_map(Widget::focused_id)
    }

    /// Drop focus everywhere in this subtree.
    fn clear_focus(&mut self) {
        self.focused = false;
        for child in &mut self.children {
            child.clear_focus();
        }
    }

    /// Give focus to `id` if this subtree contains it and it will take it.
    ///
    /// Returns whether it did. The caller is responsible for having cleared the
    /// old focus first — see [`WidgetTree::focus`], which is the only thing
    /// that should be calling this.
    fn focus_by_id(&mut self, id: WidgetId) -> bool {
        if self.id == id && self.accepts_focus() {
            self.focused = true;
            return true;
        }
        self.children.iter_mut().any(|c| c.focus_by_id(id))
    }

    /// Every focusable widget in this subtree, in tab order.
    ///
    /// Tab order is tree order — depth first, parents before children, siblings
    /// in the order they were added — which for a form built top to bottom is
    /// the order the user reads it in. A hidden or disabled subtree is skipped
    /// whole: its children are no more reachable than it is.
    #[must_use]
    pub fn focus_order(&self) -> Vec<WidgetId> {
        let mut out = Vec::new();
        self.collect_focus_order(&mut out);
        out
    }

    fn collect_focus_order(&self, out: &mut Vec<WidgetId>) {
        if !self.enabled || !self.visible {
            return;
        }
        if self.accepts_focus() {
            out.push(self.id);
        }
        for child in &self.children {
            child.collect_focus_order(out);
        }
    }

    /// The topmost focusable widget under the point, if any.
    ///
    /// "Topmost" is the same rule event dispatch uses — later siblings are
    /// drawn over earlier ones, so they are searched first. Hit-testing for the
    /// *focus* target separately from dispatching the click is what lets a
    /// press both do its own job and move the focus, without every widget kind
    /// having to remember to report that it was clicked.
    #[must_use]
    pub fn focus_target_at(&self, px: f32, py: f32) -> Option<WidgetId> {
        if !self.enabled || !self.visible || !self.contains(px, py) {
            return None;
        }
        // The children were laid out in this widget's content space -- a
        // scroll view's scrolled, and shown only inside its box.
        if !self.scrolls() || self.shows_children_at(px, py) {
            let (ox, oy) = self.children_origin();
            for child in self.children.iter().rev() {
                if let Some(hit) = child.focus_target_at(px - ox, py - oy) {
                    return Some(hit);
                }
            }
        }
        self.accepts_focus().then_some(self.id)
    }

    /// Whether the pointer is over this widget, as its last move left it.
    #[must_use]
    pub const fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// Note where the pointer is, in this widget's parent's content space:
    /// over this widget or not, and over which of its children.
    fn set_hover(&mut self, px: f32, py: f32) {
        self.hovered = self.visible && self.contains(px, py);
        let reaches = self.hovered && (!self.scrolls() || self.shows_children_at(px, py));
        let (ox, oy) = self.children_origin();
        for child in &mut self.children {
            if reaches {
                child.set_hover(px - ox, py - oy);
            } else {
                child.clear_hover();
            }
        }
    }

    /// The pointer is over nothing in this subtree.
    fn clear_hover(&mut self) {
        self.hovered = false;
        for child in &mut self.children {
            child.clear_hover();
        }
    }

    /// Whether the point is inside this widget's outer box.
    #[must_use]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        let x = self.layout.x;
        let y = self.layout.y;
        px >= x
            && px <= x + self.layout.outer_width()
            && py >= y
            && py <= y + self.layout.outer_height()
    }

    // ======================================================================
    // Layout
    // ======================================================================

    /// Width of `text` in this widget's own font, in pixels.
    ///
    /// Layout used to guess at `text.len() as f32 * font_size * 0.6`. That was
    /// two errors at once: `len` counts bytes, so any non-ASCII label was sized
    /// two to four times too wide, and even for ASCII the 0.6 was a constant
    /// picked for a fixed-cell font the compositor no longer draws. A button
    /// sized by a different rule than its label is drawn by is a button whose
    /// text hangs off the end of it.
    fn measure(&self, text: &str) -> f32 {
        crate::text::measure(
            text,
            self.style.font_size,
            weight_to_hint(self.style.font_weight),
        )
    }

    /// Compute intrinsic content size for this widget, under the palette `p`
    /// -- whose theme says how much room a button's padding takes.
    ///
    /// A control the toolkit's component modules draw is as big as they make
    /// it ([`crate::button::width`], [`crate::checkbox::width`], ...), so
    /// what is laid out and what is drawn agree.
    pub fn intrinsic_size(&self, p: &Palette) -> Size {
        let padded = |w: f32, h: f32| {
            Size::new(
                w + self.style.padding.horizontal(),
                h + self.style.padding.vertical(),
            )
        };
        let line = self.style.font_size * self.style.line_height;
        let least = |w: f32, h: f32| {
            Size::new(
                self.style.min_width.unwrap_or(w),
                self.style.min_height.unwrap_or(h),
            )
        };
        match &self.kind {
            WidgetKind::Label { text } => padded(self.measure(text), line),
            WidgetKind::Button { text, .. } => padded(
                crate::button::width(&p.widget_style.button, text),
                crate::button::HEIGHT,
            ),
            WidgetKind::TextInput { .. } => least(120.0, 28.0),
            WidgetKind::TextArea { .. } => least(200.0, 80.0),
            WidgetKind::Checkbox { label, .. } => {
                padded(crate::checkbox::width(label), crate::checkbox::HEIGHT)
            }
            WidgetKind::RadioButton { label, .. } => {
                padded(crate::radio::width(label), crate::checkbox::HEIGHT)
            }
            WidgetKind::ProgressBar { .. } => least(200.0, PROGRESS_HEIGHT),
            WidgetKind::Slider { .. } => Size::new(
                self.style.min_width.unwrap_or(120.0),
                SLIDER_HEIGHT.max(self.style.min_height.unwrap_or(0.0)),
            ),
            WidgetKind::Image { width, height, .. } => {
                let side = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
                Size::new(side(*width), side(*height))
            }
            WidgetKind::Separator { vertical } => {
                if *vertical {
                    Size::new(1.0, 0.0)
                } else {
                    Size::new(0.0, 1.0)
                }
            }
            // A scroll view is as big as its style asks: what it holds is
            // what it scrolls, and is not what sizes it.
            WidgetKind::ScrollView { .. } => least(0.0, 0.0),
            WidgetKind::Container => Size::ZERO,
        }
    }

    /// The widths of this widget's border, from its style.
    fn border_edges(&self) -> Edges {
        Edges {
            top: self.style.border.top.width,
            right: self.style.border.right.width,
            bottom: self.style.border.bottom.width,
            left: self.style.border.left.width,
        }
    }

    /// What lies between this widget's border box and its content, across
    /// and down: its padding and its border.
    fn frame(&self) -> (f32, f32) {
        let border = self.border_edges();
        (
            self.style.padding.horizontal() + border.horizontal(),
            self.style.padding.vertical() + border.vertical(),
        )
    }

    /// Where this widget's content starts, in its parent's content space --
    /// the origin its children are laid out, drawn and hit from.
    fn content_origin(&self) -> (f32, f32) {
        (
            self.layout.x
                + self.layout.margin.left
                + self.layout.border_widths.left
                + self.layout.padding.left,
            self.layout.y
                + self.layout.margin.top
                + self.layout.border_widths.top
                + self.layout.padding.top,
        )
    }

    /// The visible children as a flex container lays them out: each one's
    /// border box with room to spare, and its item.
    fn flex_children(&self, p: &Palette) -> Vec<(Size, FlexItem)> {
        self.children
            .iter()
            .filter(|c| c.visible)
            .map(|c| (c.border_box_size(p), c.flex_item_in_layout()))
            .collect()
    }

    /// How big this widget's border box is with room to spare: its content,
    /// padding and border. A flex container's content is what its children
    /// take laid out unbounded -- a column of labels is as wide as its widest
    /// -- and no less than its own size from its style. A scroll view's is
    /// its own: what it holds is what it scrolls.
    fn border_box_size(&self, p: &Palette) -> Size {
        let (frame_h, frame_v) = self.frame();
        let own = self.intrinsic_size(p);
        let border = self.border_edges();
        let mut size = Size::new(
            own.width + border.horizontal(),
            own.height + border.vertical(),
        );
        if let Some(ref flex) = self.flex_layout
            && !self.scrolls()
            && self.children.iter().any(|c| c.visible)
        {
            let unbounded = Size::new(f32::INFINITY, f32::INFINITY);
            let boxes = flex_layout(unbounded, flex, &self.flex_children(p), &Edges::ZERO);
            let wide = boxes
                .iter()
                .map(|b| b.x + b.outer_width())
                .fold(0.0_f32, f32::max);
            let tall = boxes
                .iter()
                .map(|b| b.y + b.outer_height())
                .fold(0.0_f32, f32::max);
            size.width = size.width.max(wide + frame_h);
            size.height = size.height.max(tall + frame_v);
        }
        size
    }

    /// This widget as an item of the flex container it is in: its flex
    /// properties, with the room it keeps (`margin`) and the least and most
    /// it may be (`min_*`, `max_*`) from its style -- so a style's limits hold
    /// in a row or a column as they hold alone. The sizes are its border
    /// box's.
    fn flex_item_in_layout(&self) -> FlexItem {
        let s = &self.style;
        let item = &self.flex_item;
        FlexItem {
            min: Size::new(
                s.min_width.unwrap_or(0.0).max(item.min.width),
                s.min_height.unwrap_or(0.0).max(item.min.height),
            ),
            max: Size::new(
                s.max_width.unwrap_or(f32::INFINITY).min(item.max.width),
                s.max_height.unwrap_or(f32::INFINITY).min(item.max.height),
            ),
            margin: s.margin,
            ..item.clone()
        }
    }

    /// Lay this widget and its children out, within `constraint` -- which
    /// bounds its border box: what its container's flex layout gave it, or
    /// the window's room for the root.
    ///
    /// Afterwards `layout.width` and `layout.height` are its content size
    /// (padding and border are outside them, as [`LayoutBox`] has it), and
    /// each child's `layout.x` and `layout.y` are the corner of its margin
    /// box in this widget's content space -- the space [`render`](Self::render)
    /// draws the children in and [`handle_event`](Self::handle_event) hands
    /// them the pointer in. A container sized by nothing (an unbounded
    /// constraint) takes what its children take.
    ///
    /// Until 2026-10-01 the three disagreed: the children were laid out from
    /// the container's padding edge and then drawn from its content edge (the
    /// padding counted twice), a child's width was its border box though
    /// `width` is a content size (its padding twice again), and margins moved
    /// a widget's drawing without the layout leaving room for them.
    pub fn do_layout(&mut self, constraint: SizeConstraint, p: &Palette) {
        if !self.visible {
            return;
        }
        let (frame_h, frame_v) = self.frame();
        let outer = constraint.constrain(self.border_box_size(p));
        self.layout.width = (outer.width - frame_h).max(0.0);
        self.layout.height = (outer.height - frame_v).max(0.0);
        self.layout.padding = self.style.padding;
        self.layout.margin = self.style.margin;
        self.layout.border_widths = self.border_edges();

        if let Some(ref flex) = self.flex_layout
            && self.children.iter().any(|c| c.visible)
        {
            let scrolls = self.scrolls();
            // The room inside: what the constraint allows less the frame,
            // unbounded where the constraint is -- and for a scroll view, its
            // own width and as tall as what it holds needs.
            let room = if scrolls {
                Size::new(self.layout.width, f32::INFINITY)
            } else {
                Size::new(
                    (constraint.max_width - frame_h).max(0.0),
                    (constraint.max_height - frame_v).max(0.0),
                )
            };
            let boxes = flex_layout(room, flex, &self.flex_children(p), &Edges::ZERO);

            // `flex_children` takes the visible children in order and
            // `flex_layout` answers one box each, so the visible children
            // and the boxes pair off one to one -- which is what `zip` says.
            for (child, lb) in self.children.iter_mut().filter(|c| c.visible).zip(&boxes) {
                // Its border box is exactly what the flex layout gave it.
                child.do_layout(SizeConstraint::tight(Size::new(lb.width, lb.height)), p);
                child.layout.x = lb.x;
                child.layout.y = lb.y;
            }

            let wide = boxes
                .iter()
                .map(|b| b.x + b.outer_width())
                .fold(0.0_f32, f32::max);
            let tall = boxes
                .iter()
                .map(|b| b.y + b.outer_height())
                .fold(0.0_f32, f32::max);
            let (view_w, view_h) = (self.layout.width, self.layout.height);
            if let WidgetKind::ScrollView {
                scroll_x,
                scroll_y,
                content_width,
                content_height,
            } = &mut self.kind
            {
                // What it holds may have shrunk under the scroll: keep the
                // scroll within it.
                *content_width = wide;
                *content_height = tall;
                *scroll_x = scroll_x.clamp(0.0, (wide - view_w).max(0.0));
                *scroll_y = scroll_y.clamp(0.0, (tall - view_h).max(0.0));
                return;
            }

            if !constraint.max_width.is_finite() {
                self.layout.width = wide;
            }
            if !constraint.max_height.is_finite() {
                self.layout.height = tall;
            }
        }
    }

    /// Whether this widget scrolls what it holds.
    const fn scrolls(&self) -> bool {
        matches!(self.kind, WidgetKind::ScrollView { .. })
    }

    /// Where this widget's children are, in its parent's content space: its
    /// content origin, less how far a scroll view has scrolled. The one
    /// statement of it, for drawing, the pointer and focus alike.
    fn children_origin(&self) -> (f32, f32) {
        let (ox, oy) = self.content_origin();
        match self.kind {
            WidgetKind::ScrollView {
                scroll_x, scroll_y, ..
            } => (ox - scroll_x, oy - scroll_y),
            _ => (ox, oy),
        }
    }

    /// Whether the point, in this widget's parent's content space, is where
    /// its children show: inside its content box. A scroll view's children
    /// scrolled out of it are neither seen nor hit.
    fn shows_children_at(&self, px: f32, py: f32) -> bool {
        let (ox, oy) = self.content_origin();
        px >= ox && py >= oy && px <= ox + self.layout.width && py <= oy + self.layout.height
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Render this widget and all children into a render tree -- faded by
    /// its style's `opacity`, and its children by theirs on top of it, so
    /// a half-transparent panel holding a half-transparent label draws the
    /// label at a quarter. Fully transparent, it draws nothing (it is still
    /// laid out, and still takes its room).
    ///
    /// The fade is each colour's: overlapping parts of one widget show
    /// through each other where a compositor's group opacity would not, and
    /// pictures are drawn unfaded (see [`RenderCommand::faded`]).
    ///
    /// Every control is drawn by the toolkit's own component module for it
    /// -- [`crate::button`], [`crate::checkbox`], [`crate::radio`],
    /// [`crate::slider`], [`crate::field`], [`crate::textarea`] -- in the
    /// palette `p`, so a control in a widget tree looks as it does anywhere
    /// else and follows the user's theme. A colour the widget's style sets is
    /// the program's choice and wins; one it leaves unset is the palette's.
    ///
    /// A control is drawn knowing what it is drawn on -- a button computes
    /// its face and its label's contrast against that -- which is the
    /// background of the nearest widget round it that has one, laid over
    /// what that one is drawn on, and at the root the palette's `base`, the
    /// colour the toolkit's windows clear to.
    pub fn render(&self, p: &Palette, tree: &mut RenderTree) {
        self.render_on(p, p.base, tree);
    }

    /// [`render`](Self::render), on `ground`: the colour behind this widget.
    fn render_on(&self, p: &Palette, ground: Color, tree: &mut RenderTree) {
        if !self.visible {
            return;
        }
        // A NaN opacity is a style nobody set on purpose: drawn as the
        // default, opaque, rather than vanishing.
        let opacity = self.style.opacity;
        if opacity.is_nan() || opacity >= 1.0 {
            self.render_opaque(p, ground, tree);
        } else if opacity > 0.0 {
            // Faded as a group: drawn opaque, on the same ground, and then
            // every command faded alike -- so a child is drawn on its
            // parent's background as it is before the fade.
            let mut own = RenderTree::new();
            self.render_opaque(p, ground, &mut own);
            tree.commands
                .extend(own.commands.into_iter().map(|c| c.faded(opacity)));
        }
    }

    /// [`render_on`](Self::render_on), at full opacity.
    fn render_opaque(&self, p: &Palette, ground: Color, tree: &mut RenderTree) {
        let x = self.layout.x + self.layout.margin.left;
        let y = self.layout.y + self.layout.margin.top;
        let w = self.layout.border_box_width();
        let h = self.layout.border_box_height();

        // Background
        if self.style.background.a > 0 {
            tree.push(RenderCommand::FillRect {
                x,
                y,
                width: w,
                height: h,
                color: self.style.background,
                corner_radii: self.style.border_radius,
            });
        }

        // Border: one stroke round a box whose four sides agree, as most do,
        // round corners and all; otherwise each side its own strip, square
        // -- where it used to be the top side's width and colour all round.
        let b = &self.style.border;
        let same = |s: &crate::style::Border| {
            (s.width - b.top.width).abs() < f32::EPSILON && s.color == b.top.color
        };
        if same(&b.right) && same(&b.bottom) && same(&b.left) {
            if b.top.width > 0.0 {
                tree.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: b.top.color,
                    line_width: b.top.width,
                    corner_radii: self.style.border_radius,
                });
            }
        } else {
            let strips = [
                (x, y, w, b.top.width, b.top.color),
                (x, y + h - b.bottom.width, w, b.bottom.width, b.bottom.color),
                (x, y, b.left.width, h, b.left.color),
                (x + w - b.right.width, y, b.right.width, h, b.right.color),
            ];
            for (sx, sy, sw, sh, color) in strips {
                if sw > 0.0 && sh > 0.0 {
                    tree.push(RenderCommand::FillRect {
                        x: sx,
                        y: sy,
                        width: sw,
                        height: sh,
                        color,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }

        // What the widget is, drawn by the component module for it, and its
        // children, on its background -- or, where it has none, on what it
        // is itself drawn on.
        let ground = self.style.background.over(ground);
        self.draw_kind(p, ground, tree, (x, y, w, h));

        // Its children, from where they are -- a scroll view's scrolled -- and
        // cut to its content box.
        if !self.children.is_empty() {
            let (ox, oy) = self.children_origin();
            let (cx, cy) = self.content_origin();
            tree.push(RenderCommand::PushTranslate { dx: ox, dy: oy });
            tree.push(RenderCommand::PushClip {
                x: cx - ox,
                y: cy - oy,
                width: self.layout.width,
                height: self.layout.height,
            });

            for child in &self.children {
                child.render_on(p, ground, tree);
            }

            tree.push(RenderCommand::PopClip);
            tree.push(RenderCommand::PopTranslate);
        }
        // A scroll view's bars, over what it holds.
        self.draw_scrollbars(p, tree);
    }

    // ======================================================================
    // Event handling
    // ======================================================================

    /// Dispatch an event to this widget. Returns whether it was consumed.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.enabled || !self.visible {
            return EventResult::Ignored;
        }

        // Try children first (front-to-back, last child is "on top") -- the
        // pointer in this widget's content space, where they were laid out
        // and are drawn, a scroll view's scrolled; and only where they show,
        // for a scroll view, which cuts them to its box.
        let translated;
        let mut offer = true;
        let inner = if let Event::Mouse(mouse) = event {
            offer = !self.scrolls() || self.shows_children_at(mouse.x, mouse.y);
            let (ox, oy) = self.children_origin();
            translated = Event::Mouse(MouseEvent {
                x: mouse.x - ox,
                y: mouse.y - oy,
                kind: mouse.kind.clone(),
            });
            &translated
        } else {
            event
        };
        if offer {
            for index in (0..self.children.len()).rev() {
                let Some(child) = self.children.get_mut(index) else {
                    continue;
                };
                if child.handle_event(inner) == EventResult::Consumed {
                    self.choose_radio(index);
                    return EventResult::Consumed;
                }
            }
        }

        // Handle at this widget level
        match event {
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            // A keystroke goes to the focused widget and to nothing else. The
            // recursion above has already offered it to the children, so
            // reaching here means none of them was focused; if this one is not
            // either, the event is not ours.
            //
            // Before focus existed this line read `self.handle_key(key)`
            // unconditionally, which meant the *last* text field in the tree
            // consumed every keystroke in the window regardless of where the
            // user had clicked. On a two-field dialog, typing into the first
            // field filled in the second.
            Event::Key(key) if self.focused => self.handle_key(key),
            _ => EventResult::Ignored,
        }
    }

    /// After the child at `index` took an event: if it is a radio button and
    /// is now chosen, its sibling radio buttons -- its group -- are not.
    fn choose_radio(&mut self, index: usize) {
        let chosen = matches!(
            self.children.get(index).map(|child| &child.kind),
            Some(WidgetKind::RadioButton { selected: true, .. })
        );
        if !chosen {
            return;
        }
        for (at, sibling) in self.children.iter_mut().enumerate() {
            if at != index
                && let WidgetKind::RadioButton { selected, .. } = &mut sibling.kind
            {
                *selected = false;
            }
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        let inside = self.contains(mouse.x, mouse.y);
        let font_size = self.style.font_size;
        let row = font_size * self.style.line_height;
        let (content_x, content_y) = self.content_origin();
        let (content_w, content_h) = (self.layout.width, self.layout.height);
        let focused = self.focused;
        let metrics = self.text_metrics();
        let placement = self.slider_placement();
        let taken = |yes: bool| {
            if yes {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        };

        match &mut self.kind {
            // A slider follows its drag wherever the pointer goes, and decides
            // itself what of the pointer is its; the wheel moves it only while
            // it has the keyboard, as its module asks.
            WidgetKind::Slider { slider } => match mouse.kind {
                MouseEventKind::Scroll { dy, .. } if focused && inside => {
                    taken(slider.wheel(dy).is_taken())
                }
                _ => taken(slider.handle_mouse(&placement, mouse).is_taken()),
            },
            // A text area selects while a press in it is held, wherever the
            // pointer goes; and scrolls under the wheel.
            WidgetKind::TextArea {
                area, selecting, ..
            } => match mouse.kind {
                MouseEventKind::Press(MouseButton::Left) if inside => {
                    area.press(mouse.x - content_x, mouse.y - content_y, 1, false, &metrics);
                    *selecting = true;
                    EventResult::Consumed
                }
                MouseEventKind::DoubleClick(MouseButton::Left) if inside => {
                    area.press(mouse.x - content_x, mouse.y - content_y, 2, false, &metrics);
                    *selecting = true;
                    EventResult::Consumed
                }
                MouseEventKind::Move if *selecting => {
                    area.drag_to(mouse.x - content_x, mouse.y - content_y, &metrics);
                    EventResult::Consumed
                }
                MouseEventKind::Release(_) if *selecting => {
                    *selecting = false;
                    EventResult::Consumed
                }
                MouseEventKind::Scroll { dy, .. } if inside => {
                    area.scroll_by(crate::wheel::pixels(dy, metrics.line_height()), &metrics);
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            _ if !inside => EventResult::Ignored,
            // The wheel scrolls a scroll view, as far as it goes; a turn
            // that moves nothing is left for a scroll view round it.
            WidgetKind::ScrollView {
                scroll_x,
                scroll_y,
                content_width,
                content_height,
            } => match mouse.kind {
                MouseEventKind::Scroll { dx, dy } => {
                    let before = (*scroll_x, *scroll_y);
                    *scroll_y = (*scroll_y + crate::wheel::pixels(dy, row))
                        .clamp(0.0, (*content_height - content_h).max(0.0));
                    *scroll_x = (*scroll_x + crate::wheel::pixels_x(dx, row))
                        .clamp(0.0, (*content_width - content_w).max(0.0));
                    taken((*scroll_x, *scroll_y) != before)
                }
                _ => EventResult::Ignored,
            },
            // A click chooses a radio button; its parent clears the rest of
            // its group (`choose_radio`).
            WidgetKind::RadioButton { selected, .. } => {
                if matches!(mouse.kind, MouseEventKind::Release(_)) {
                    *selected = true;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // A click in a text field puts the caret where it landed. The
            // offset is resolved by the shaper (`cursor_at`), not by dividing
            // by an average character width, so it lands on the boundary the
            // user aimed at even where the text mixes directions -- and it
            // round-trips with `caret_x`, which is what draws it.
            WidgetKind::TextInput {
                value,
                cursor,
                selection_anchor,
                ..
            } => match &mouse.kind {
                MouseEventKind::Press(_) => {
                    *cursor = crate::textedit::cursor_at_click(
                        value,
                        *cursor,
                        content_w,
                        font_size,
                        FontWeightHint::Regular,
                        mouse.x - content_x,
                    );
                    // A click starts a new selection wherever it lands, so
                    // whatever was selected stops being selected. Dragging is
                    // not wired up yet, so the anchor is dropped rather than
                    // set: an anchor with no drag to move it would leave an
                    // empty selection behind that only looks like state.
                    *selection_anchor = None;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            WidgetKind::Button { pressed, .. } => match &mouse.kind {
                MouseEventKind::Press(_) => {
                    *pressed = true;
                    EventResult::Consumed
                }
                MouseEventKind::Release(_) => {
                    *pressed = false;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            WidgetKind::Checkbox { checked, .. } => {
                if matches!(&mouse.kind, MouseEventKind::Release(_)) {
                    *checked = checked.toggled();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        // Read before `kind` is borrowed mutably below. The arrows measure the
        // text to find where the caret sits on screen, and they have to measure
        // it at the size it is *drawn* at (see the `Left` arm), which lives in
        // `style` — a different field of the same `self`. A text area's box is
        // read for the same reason.
        let font_size = self.style.font_size;
        let metrics = self.text_metrics();

        let shift = key.modifiers.shift;

        match &mut self.kind {
            WidgetKind::TextInput {
                value,
                cursor,
                selection_anchor,
                ..
            } => {
                // `types_text`, not `!key.text.is_empty()`: our own compositor
                // reports no text for Enter, Tab and Escape, but this widget
                // also serves events that arrived over `guiremote` from a peer
                // whose keymap is not ours, and on most systems those keys
                // produce `\r`, `\t` and `\x1b`. Claiming such a keystroke as
                // text would put an unprintable byte in the field *and* stop
                // the `match` below from ever seeing the Enter that submits it.
                if key.types_text() {
                    // Typing over a selection replaces it. Doing this *before*
                    // the insert is what makes the caret land in the right
                    // place: the deletion moves every offset after it, so an
                    // insert positioned first would end up somewhere else.
                    crate::textedit::delete_selection(value, cursor, selection_anchor);
                    // *Every* character the keystroke produced, not just the
                    // first: a dead key whose composition failed types two
                    // (`´` then `x` gives `´x`), and taking one of them would
                    // silently eat half of what the user typed.
                    for ch in key.typed() {
                        value.insert(cursor.byte(), ch);
                        // Typing lands the caret after what was typed, which is
                        // the downstream side of the new boundary whichever way
                        // the surrounding text runs — and, now that the text
                        // contains the character, the next boundary the text
                        // itself names.
                        *cursor = cursor
                            .next_in(value)
                            .unwrap_or_else(|| TextCursor::from(value.len()));
                    }
                    return EventResult::Consumed;
                }
                match key.key {
                    // Backspace is a *logical* edit, not a visual move: it
                    // deletes the character before this one in the string,
                    // which is what a reader of that script expects even where
                    // that character is drawn to the right. `prev_in` is that
                    // step, and it is the toolkit's rather than this widget's
                    // because `String::remove` panics on an offset
                    // mid-character and there is no reason for four widgets to
                    // each own a way of not producing one.
                    crate::event::Key::Backspace => {
                        // With a selection up, Backspace deletes the selection
                        // and stops. Deleting the selection *and then* the
                        // character before it removes something the user never
                        // pointed at, which is the one editing mistake that
                        // cannot be seen happening.
                        if !crate::textedit::delete_selection(value, cursor, selection_anchor) {
                            if let Some(prev) = cursor.prev_in(value) {
                                value.remove(prev.byte());
                                *cursor = prev;
                            }
                        }
                        EventResult::Consumed
                    }
                    crate::event::Key::Delete => {
                        if !crate::textedit::delete_selection(value, cursor, selection_anchor) {
                            // Forward delete is the logical *next* character,
                            // for the same reason Backspace is the logical
                            // previous one, and `next_in` is what refuses to
                            // hand back an offset inside one.
                            if cursor.byte() < value.len() {
                                value.remove(cursor.byte());
                            }
                        }
                        EventResult::Consumed
                    }
                    // The arrows step *visually* — one position left or right on
                    // the screen — which on a line mixing directions is not the
                    // same as one character earlier or later in the string.
                    // macOS, GTK and Qt move logically and Windows moves
                    // visually; the operator chose visual, and the reasoning is
                    // `design-decisions.md` §541.
                    //
                    // Two things this depends on, both easy to break:
                    //
                    // 1. **Measure at the size the text is drawn at.** The gaps
                    //    between glyphs are a property of the shaped run, so
                    //    measuring at another size moves the caret to a place
                    //    this widget never drew it. `font_size` above is the
                    //    same value the `Text` command below is pushed with.
                    // 2. **Keep the whole `TextCursor`, not just its byte.**
                    //    Where a left-to-right and a right-to-left run meet, one
                    //    place on screen answers to two byte offsets, and one
                    //    byte offset answers to two places on screen. On
                    //    `ab` + two Hebrew letters + `cd` the caret visits byte
                    //    6 *twice* on a single walk leftwards, at two different
                    //    screen positions, and only the affinity distinguishes
                    //    them. A widget that stored the byte and rebuilt the
                    //    rest each keypress would skip the whole Hebrew word in
                    //    one press. Assigning the returned cursor whole, as
                    //    below, is what keeps that bit alive.
                    //
                    // Shift held, the caret drags a selection behind it: the
                    // anchor is planted at the caret's *old* position the first
                    // time and left alone afterwards, so a run of Shift+Left
                    // extends one selection rather than restarting it each
                    // press. Shift released, the selection is dropped — an
                    // arrow with no Shift is a move, and a move that left the
                    // highlight standing would make the next keystroke delete
                    // text the user had stopped selecting.
                    crate::event::Key::Left => {
                        crate::textedit::begin_or_end_selection(shift, *cursor, selection_anchor);
                        if let Some(prev) = crate::text::caret_left(
                            value,
                            *cursor,
                            font_size,
                            FontWeightHint::Regular,
                        ) {
                            *cursor = prev;
                        }
                        EventResult::Consumed
                    }
                    crate::event::Key::Right => {
                        crate::textedit::begin_or_end_selection(shift, *cursor, selection_anchor);
                        if let Some(next) = crate::text::caret_right(
                            value,
                            *cursor,
                            font_size,
                            FontWeightHint::Regular,
                        ) {
                            *cursor = next;
                        }
                        EventResult::Consumed
                    }
                    // Home and End are *string* ends, not screen ends, and so
                    // take no affinity argument: byte 0 and byte len are each
                    // one place on the screen whichever way the text runs,
                    // because there is nothing on the far side of them for a
                    // boundary to have two sides of.
                    crate::event::Key::Home => {
                        crate::textedit::begin_or_end_selection(shift, *cursor, selection_anchor);
                        *cursor = TextCursor::from(0);
                        EventResult::Consumed
                    }
                    crate::event::Key::End => {
                        crate::textedit::begin_or_end_selection(shift, *cursor, selection_anchor);
                        *cursor = TextCursor::from(value.len());
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            // A focused button is pressed by Space or Enter, and a focused
            // checkbox is toggled by Space. Without this, Tab could move the
            // focus onto a button that then had no way to be activated, which
            // is worse than not being in the tab order at all: the user would
            // see the focus land somewhere it could not act.
            WidgetKind::Button { pressed, .. } => match key.key {
                crate::event::Key::Space | crate::event::Key::Enter => {
                    *pressed = !*pressed;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            WidgetKind::Checkbox { checked, .. } => {
                if key.key == crate::event::Key::Space {
                    *checked = checked.toggled();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // Space chooses a radio button, as a click does.
            WidgetKind::RadioButton { selected, .. } => {
                if key.key == crate::event::Key::Space {
                    *selected = true;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            WidgetKind::Slider { slider } => {
                if slider.handle_key(key).is_taken() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // A text area's editing keys are its module's: typing, deleting,
            // the arrows and their selections, undo, the clipboard. What it
            // leaves -- Escape, Tab -- is the window's.
            WidgetKind::TextArea { area, .. } => match area.edit_key(key, &metrics) {
                crate::textinput::KeyEdit::Unhandled => EventResult::Ignored,
                crate::textinput::KeyEdit::Handled | crate::textinput::KeyEdit::Changed => {
                    EventResult::Consumed
                }
            },
            _ => EventResult::Ignored,
        }
    }
}

fn weight_to_hint(w: FontWeight) -> FontWeightHint {
    match w {
        FontWeight::Thin | FontWeight::Light => FontWeightHint::Light,
        FontWeight::Bold | FontWeight::ExtraBold | FontWeight::SemiBold => FontWeightHint::Bold,
        _ => FontWeightHint::Regular,
    }
}

/// The root widget tree — manages the top-level window.
pub struct WidgetTree {
    pub root: Widget,
    pub window_width: f32,
    pub window_height: f32,
    /// What the tree draws in: the user's palette, from its program
    /// ([`set_palette`](Self::set_palette)). Its theme's widget style also
    /// sizes the controls.
    palette: Palette,
}

impl WidgetTree {
    /// A tree for a window `width` by `height`, drawn in the light palette
    /// until its program gives it the user's: this crate cannot read the
    /// user's settings, its programs can.
    pub fn new(root: Widget, width: f32, height: f32) -> Self {
        Self {
            root,
            window_width: width,
            window_height: height,
            palette: Palette::for_mode(true),
        }
    }

    /// Draw in `palette` from now on -- the user's, and again when the user
    /// changes theme -- and lay the tree out again: a theme's buttons are as
    /// wide as its padding makes them.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
        self.layout();
    }

    /// The palette the tree draws in.
    #[must_use]
    pub const fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Perform layout on the entire tree.
    pub fn layout(&mut self) {
        // The root's border box is the window less the root's own margins:
        // the window is its margin box, at the window's corner.
        let margin = self.root.style.margin;
        self.root.do_layout(
            SizeConstraint::tight(Size::new(
                (self.window_width - margin.horizontal()).max(0.0),
                (self.window_height - margin.vertical()).max(0.0),
            )),
            &self.palette,
        );
        self.root.layout.x = 0.0;
        self.root.layout.y = 0.0;
    }

    /// Render the entire tree into a render command list, in its palette.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();
        self.root.render(&self.palette, &mut tree);
        tree
    }

    /// The focused widget's id, if anything is focused.
    #[must_use]
    pub fn focused_id(&self) -> Option<WidgetId> {
        self.root.focused_id()
    }

    /// Move the focus to `id`, or nowhere if `id` is `None`.
    ///
    /// Returns whether the focus ended up where it was asked to go — `false`
    /// if `id` is not in this tree, or is disabled, hidden, or of a kind that
    /// does not take focus. The old focus is dropped in every case, including
    /// the failing ones: an attempt to focus a widget that has been disabled
    /// since the caller last looked must not leave the focus on the widget
    /// before it, which would silently redirect the next keystroke.
    pub fn focus(&mut self, id: Option<WidgetId>) -> bool {
        self.root.clear_focus();
        id.is_some_and(|id| self.root.focus_by_id(id))
    }

    /// Focus the first widget that will take it, if any. Returns whether one
    /// did.
    pub fn focus_first(&mut self) -> bool {
        let first = self.root.focus_order().first().copied();
        self.focus(first)
    }

    /// Move the focus to the next focusable widget in tab order, wrapping.
    ///
    /// With nothing focused it takes the first — which is what makes Tab work
    /// on a freshly-opened window that nobody has clicked in yet.
    pub fn focus_next(&mut self) -> bool {
        self.step_focus(true)
    }

    /// Move the focus to the previous focusable widget in tab order, wrapping.
    pub fn focus_prev(&mut self) -> bool {
        self.step_focus(false)
    }

    fn step_focus(&mut self, forward: bool) -> bool {
        let order = self.root.focus_order();
        if order.is_empty() {
            return false;
        }
        let next = match self
            .focused_id()
            .and_then(|id| order.iter().position(|&candidate| candidate == id))
        {
            Some(at) if forward => crate::step::wrapping_after(order.len(), at),
            Some(at) => crate::step::wrapping_before(order.len(), at),
            // Nothing focused: forward starts at the top of the order,
            // backward at the bottom, which is what Shift+Tab into a window
            // means everywhere else.
            None if forward => 0,
            None => order.len().saturating_sub(1),
        };
        self.focus(order.get(next).copied())
    }

    /// Dispatch an event to the widget tree.
    ///
    /// Two things happen here that cannot happen inside a widget, because both
    /// need to see the whole tree:
    ///
    /// - **Tab moves the focus**, and is taken before the tree sees it. A text
    ///   field that received Tab would have no way to know what comes next.
    /// - **A press moves the focus to whatever was clicked**, resolved by
    ///   hit-testing rather than by asking each widget to report that it was
    ///   hit. The focus moves first, so the click is delivered to a widget that
    ///   is already focused — otherwise the first click into a field would
    ///   position a caret that was not yet being drawn.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) if key.pressed && key.key == crate::event::Key::Tab => {
                if key.modifiers.shift {
                    self.focus_prev();
                } else {
                    self.focus_next();
                }
                return EventResult::Consumed;
            }
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Press(_)) => {
                // A press on nothing focusable clears the focus, rather than
                // leaving it where it was. Clicking the background of a dialog
                // is how a user says "not that field any more", and a caret
                // still blinking in a field the user has clicked away from is
                // a caret that lies about where the next keystroke goes.
                self.focus(self.root.focus_target_at(mouse.x, mouse.y));
            }
            // The pointer's moves say what it is over, which lights a button
            // or a box as the pointer crosses it.
            Event::Mouse(mouse)
                if matches!(mouse.kind, MouseEventKind::Move | MouseEventKind::Enter) =>
            {
                self.root.set_hover(mouse.x, mouse.y);
            }
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Leave) => {
                self.root.clear_hover();
            }
            _ => {}
        }
        self.root.handle_event(event)
    }

    /// Resize the window and re-layout.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.window_width = width;
        self.window_height = height;
        self.layout();
    }
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    #[test]
    fn test_widget_tree_layout() {
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_padding(Edges::all(10.0))
            .with_child(Widget::label("Hello"))
            .with_child(Widget::button("Click me"))
            .with_child(Widget::text_input("", "Type here..."));

        let mut tree = WidgetTree::new(root, 400.0, 300.0);
        tree.layout();

        // The root's border box is the window; `width` and `height` are its
        // content, inside the padding, as `LayoutBox` says they are.
        assert_eq!(tree.root.layout.border_box_width(), 400.0);
        assert_eq!(tree.root.layout.border_box_height(), 300.0);
        assert_eq!(tree.root.layout.width, 380.0);
        assert_eq!(tree.root.layout.height, 280.0);

        // A column stacks its children from the content's corner, each as
        // wide as the content.
        let kids = &tree.root.children;
        assert_eq!(kids.len(), 3);
        assert_eq!((kids[0].layout.x, kids[0].layout.y), (0.0, 0.0));
        assert_eq!(kids[1].layout.y, kids[0].layout.outer_height());
        assert_eq!(
            kids[2].layout.y,
            kids[1].layout.y + kids[1].layout.outer_height()
        );
        for kid in kids {
            assert_eq!(kid.layout.border_box_width(), 380.0);
        }
    }

    /// The `x` of the first text a tree draws.
    fn first_text_x(tree: &WidgetTree) -> f32 {
        tree.render()
            .commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { x, .. } => Some(*x),
                _ => None,
            })
            .expect("some text is drawn")
    }

    /// The sum of the translations in force where the first text is drawn.
    fn translation_at_first_text(tree: &WidgetTree) -> f32 {
        let mut stack: Vec<f32> = Vec::new();
        for c in &tree.render().commands {
            match c {
                RenderCommand::PushTranslate { dx, .. } => stack.push(*dx),
                RenderCommand::PopTranslate => {
                    stack.pop();
                }
                RenderCommand::Text { .. } => return stack.iter().sum(),
                _ => {}
            }
        }
        panic!("no text is drawn")
    }

    /// **A padded container's child is drawn inside the padding once** --
    /// it was twice: laid out from the padding edge, then drawn from the
    /// content edge.
    #[test]
    fn a_padded_containers_child_is_drawn_inside_the_padding_once() {
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_padding(Edges::all(10.0))
            .with_child(Widget::label("Hello"));
        let mut tree = WidgetTree::new(root, 300.0, 200.0);
        tree.layout();
        let label = &tree.root.children[0];
        let drawn = translation_at_first_text(&tree) + first_text_x(&tree);
        // The label's own text sits inside its own padding.
        let expected = 10.0 + label.layout.margin.left + label.layout.padding.left;
        assert!(
            (drawn - expected).abs() < 0.01,
            "the label is drawn at {drawn}, not {expected}"
        );
    }

    /// **A margin keeps its room**: the next child in a row starts past it,
    /// where it used to be drawn over.
    #[test]
    fn a_margin_keeps_its_room_in_a_row() {
        let root = Widget::container()
            .with_child(Widget::label("One").with_margin(Edges {
                top: 0.0,
                right: 8.0,
                bottom: 0.0,
                left: 4.0,
            }))
            .with_child(Widget::label("Two"));
        let mut tree = WidgetTree::new(root, 300.0, 100.0);
        tree.layout();
        let kids = &tree.root.children;
        assert_eq!(kids[0].layout.x, 0.0);
        assert_eq!(kids[1].layout.x, kids[0].layout.outer_width());
        assert_eq!(
            kids[0].layout.outer_width(),
            4.0 + kids[0].layout.border_box_width() + 8.0
        );
    }

    /// **A column inside a row is as wide as its widest child**: a container
    /// takes its children's size, where it used to be nothing wide and its
    /// children overflowed it.
    #[test]
    fn a_nested_column_takes_its_widest_childs_width() {
        let column = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_child(Widget::label("short"))
            .with_child(Widget::label("a much longer label"));
        let root = Widget::container()
            .with_child(column)
            .with_child(Widget::label("beside"));
        let mut tree = WidgetTree::new(root, 600.0, 100.0);
        tree.layout();
        let column = &tree.root.children[0];
        let widest = column
            .children
            .iter()
            .map(|c| c.border_box_size(tree.palette()).width)
            .fold(0.0_f32, f32::max);
        assert!(widest > 0.0);
        assert_eq!(column.layout.border_box_width(), widest);
        assert_eq!(tree.root.children[1].layout.x, widest);
    }

    /// **A style's `min_width` holds in a row**: an item squeezed by its
    /// neighbours stops at it.
    #[test]
    fn a_styles_min_width_holds_in_a_row() {
        let mut keep = Widget::label("kept at its minimum");
        keep.style.min_width = Some(150.0);
        let root = Widget::container()
            .with_child(keep)
            .with_child(Widget::label("a neighbour that would squeeze it"));
        let mut tree = WidgetTree::new(root, 160.0, 100.0);
        tree.layout();
        assert!(tree.root.children[0].layout.border_box_width() >= 150.0);
    }

    /// **A click reaches a field nested in padded containers** -- the pointer
    /// handed to each child in the space it was laid out in. It used to be
    /// handed on unchanged, so the deeper a field sat, the further from it a
    /// click had to land.
    #[test]
    fn a_click_reaches_a_field_nested_in_padded_containers() {
        let inner = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_padding(Edges::all(12.0))
            .with_child(Widget::text_input("", "nested"));
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_padding(Edges::all(20.0))
            .with_child(inner);
        let mut tree = WidgetTree::new(root, 400.0, 300.0);
        tree.layout();
        let field_id = tree.root.children[0].children[0].id;
        // The field's corner on the window: each container's content origin
        // in turn, then the field's own margin box corner.
        let (rx, ry) = tree.root.content_origin();
        let (ix, iy) = tree.root.children[0].content_origin();
        let field = &tree.root.children[0].children[0];
        let (fx, fy) = (
            rx + ix + field.layout.x + 2.0,
            ry + iy + field.layout.y + 2.0,
        );
        assert_eq!((fx, fy), (34.0, 34.0));
        tree.handle_event(&clicked(fx, fy));
        assert_eq!(tree.focused_id(), Some(field_id));

        // And a click in the inner container's padding, beside the field,
        // is not the field's.
        assert!(!tree.focus(None));
        tree.handle_event(&clicked(25.0, 25.0));
        assert_ne!(tree.focused_id(), Some(field_id));
    }

    /// **A nested widget's own handler is given the pointer in its own
    /// space, too**: a checkbox two padded containers deep toggles when the
    /// click lands on it, and not when it lands where the checkbox would be
    /// if the containers' offsets were forgotten -- the window's corner.
    #[test]
    fn a_click_toggles_a_checkbox_nested_in_padded_containers() {
        let inner = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_padding(Edges::all(12.0))
            .with_child(Widget::checkbox("nested", false));
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_padding(Edges::all(20.0))
            .with_child(inner);
        let mut tree = WidgetTree::new(root, 400.0, 300.0);
        tree.layout();
        let released = |x, y| {
            Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Release(crate::event::MouseButton::Left),
            })
        };
        let checked = |tree: &WidgetTree| match &tree.root.children[0].children[0].kind {
            WidgetKind::Checkbox { checked, .. } => *checked,
            other => panic!("not a checkbox: {other:?}"),
        };
        // Its corner on the window is 20 + 12 in from the window's.
        tree.handle_event(&released(5.0, 5.0));
        assert_eq!(checked(&tree), CheckState::Unchecked);
        tree.handle_event(&released(36.0, 36.0));
        assert_eq!(checked(&tree), CheckState::Checked);
    }

    /// The colours of the first command of each kind a tree draws.
    fn first_fill(tree: &WidgetTree) -> Option<Color> {
        tree.render().commands.iter().find_map(|c| match c {
            RenderCommand::FillRect { color, .. } => Some(*color),
            _ => None,
        })
    }

    /// **A widget's opacity fades everything it draws, and a child's on top
    /// of its parent's**: a half-transparent panel's half-transparent label
    /// is drawn at a quarter. Fully transparent draws nothing; a NaN is the
    /// default, opaque. The style's `opacity` used to be stored and never
    /// read.
    #[test]
    fn opacity_fades_a_widget_and_its_children() {
        let mut label = Widget::label("faint");
        label.style.opacity = 0.5;
        let mut panel = Widget::container()
            .with_background(Color::WHITE)
            .with_child(label);
        panel.style.opacity = 0.5;
        let mut tree = WidgetTree::new(panel, 200.0, 100.0);
        tree.layout();
        let text_alpha = |tree: &WidgetTree| {
            tree.render().commands.iter().find_map(|c| match c {
                RenderCommand::Text { color, .. } => Some(color.a),
                _ => None,
            })
        };
        assert_eq!(first_fill(&tree).map(|c| c.a), Some(128));
        // 255 halved is 128 by the label, and 64 by the panel after it.
        assert_eq!(text_alpha(&tree), Some(64));

        tree.root.style.opacity = 0.0;
        assert!(tree.render().commands.is_empty());

        tree.root.style.opacity = f32::NAN;
        assert_eq!(first_fill(&tree).map(|c| c.a), Some(255));
    }

    /// **A border whose sides differ is drawn side by side**, each side in
    /// its own width and colour -- it used to be the top side's all round.
    /// Four sides alike are still one stroke, round corners and all.
    #[test]
    fn a_border_whose_sides_differ_is_drawn_side_by_side() {
        let red = Color::from_hex(0xFF_0000);
        let blue = Color::from_hex(0x00_00FF);
        let mut panel = Widget::container();
        panel.style.border.top = crate::style::Border {
            width: 2.0,
            color: red,
        };
        panel.style.border.left = crate::style::Border {
            width: 1.0,
            color: blue,
        };
        let mut tree = WidgetTree::new(panel, 200.0, 100.0);
        tree.layout();
        let strips: Vec<(f32, f32, f32, f32, Color)> = tree
            .render()
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => Some((*x, *y, *width, *height, *color)),
                _ => None,
            })
            .collect();
        assert_eq!(
            strips,
            [(0.0, 0.0, 200.0, 2.0, red), (0.0, 0.0, 1.0, 100.0, blue)]
        );

        let all_round = crate::style::Border {
            width: 3.0,
            color: red,
        };
        tree.root.style.border = crate::style::Borders {
            top: all_round,
            right: all_round,
            bottom: all_round,
            left: all_round,
        };
        let strokes = tree
            .render()
            .commands
            .iter()
            .filter(
                |c| matches!(c, RenderCommand::StrokeRect { line_width, .. } if *line_width == 3.0),
            )
            .count();
        assert_eq!(strokes, 1);
    }

    #[test]
    fn test_render_produces_commands() {
        let root = Widget::container()
            .with_background(Color::WHITE)
            .with_child(Widget::label("Test"));

        let mut tree = WidgetTree::new(root, 200.0, 100.0);
        tree.layout();
        let render = tree.render();

        // Should have at least a fill rect (background) and text command
        assert!(!render.is_empty());
    }

    #[test]
    fn test_button_press() {
        let root = Widget::container().with_child(Widget::button("OK"));

        let mut tree = WidgetTree::new(root, 200.0, 100.0);
        tree.layout();

        // Simulate click within bounds
        let event = Event::Mouse(MouseEvent {
            x: 5.0,
            y: 5.0,
            kind: MouseEventKind::Press(crate::event::MouseButton::Left),
        });
        tree.handle_event(&event);
    }

    /// Editing around a character that occupies more than one byte used to
    /// abort the process: the cursor moved one *byte* per keypress, while
    /// `String::remove` indexes by bytes and panics on an offset inside a
    /// character. Any non-ASCII input — `café`, Cyrillic, CJK — was a crash one
    /// keystroke later.
    fn typed(ch: char) -> KeyEvent {
        KeyEvent {
            // The `key` code is irrelevant: a character arrives in `text`, and
            // `Key` has no per-character variant.
            key: crate::event::Key::Unknown(0),
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: ch.to_string(),
        }
    }

    fn pressed(key: crate::event::Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: String::new(),
        }
    }

    fn state(w: &Widget) -> (&str, usize) {
        match &w.kind {
            WidgetKind::TextInput { value, cursor, .. } => (value.as_str(), cursor.byte()),
            _ => panic!("not a text input"),
        }
    }

    #[test]
    fn a_text_input_survives_a_multi_byte_character() {
        let mut input = Widget::text_input("caf", "");
        input.handle_key(&typed('é'));
        assert_eq!(state(&input), ("café", 5));

        // One press must cross the whole two-byte character.
        input.handle_key(&pressed(crate::event::Key::Left));
        assert_eq!(state(&input), ("café", 3));
        input.handle_key(&pressed(crate::event::Key::Right));
        assert_eq!(state(&input), ("café", 5));

        // And backspace must remove it whole rather than half of it.
        input.handle_key(&pressed(crate::event::Key::Backspace));
        assert_eq!(state(&input), ("caf", 3));

        // The empty-string edges stay no-ops rather than underflowing.
        let mut empty = Widget::text_input("", "");
        empty.handle_key(&pressed(crate::event::Key::Backspace));
        empty.handle_key(&pressed(crate::event::Key::Left));
        empty.handle_key(&pressed(crate::event::Key::Right));
        assert_eq!(state(&empty), ("", 0));
    }

    /// The arrows move **visually** — one position left or right on the screen
    /// — so on `ab` + two Hebrew letters + `cd`, drawn `a b <bet> <aleph> c d`,
    /// the caret steps *through* the Hebrew instead of jumping across it.
    /// `design-decisions.md` §541; the operator chose this over logical motion.
    ///
    /// **One byte offset is visited twice per walk, and that is the point.**
    /// The two gaps where the directions meet — `b|<bet>` and `<aleph>|c` — sit
    /// at opposite ends of the Hebrew on screen, yet each answers to *both* of
    /// the byte offsets 2 and 6. Which one the caret reports depends on the
    /// direction it arrived from: walking left it reads 6 at both gaps
    /// (`7, 6, 4, 6, 1, 0`), walking right it reads 2 at both
    /// (`1, 2, 4, 2, 7, 8`). Only the affinity carried inside `TextCursor`
    /// separates the two, and the screen position moves strictly one step each
    /// press in both walks.
    ///
    /// So this is also the regression test for §541's measured trap: a widget
    /// that kept the byte and rebuilt the rest each keypress cannot tell the
    /// second 6 from the first, and skips the entire Hebrew word in one press —
    /// worse than the logical motion this replaced. **A failure here showing a
    /// _shorter_ sequence, or one without the repeat, is that bug.**
    #[test]
    fn the_arrows_move_by_the_screen_and_keep_the_side_they_are_on() {
        let text = "ab\u{05D0}\u{05D1}cd";
        let mut input = Widget::text_input(text, "");
        let mut seen = vec![];
        for _ in 0..6 {
            input.handle_key(&pressed(crate::event::Key::Left));
            seen.push(state(&input).1);
        }
        assert_eq!(seen, vec![7, 6, 4, 6, 1, 0]);
        // And back out again the other way, which reports the *other* offset of
        // each ambiguous pair for the same six screen positions.
        let mut seen = vec![];
        for _ in 0..6 {
            input.handle_key(&pressed(crate::event::Key::Right));
            seen.push(state(&input).1);
        }
        assert_eq!(seen, vec![1, 2, 4, 2, 7, 8]);
        // Pressed past the end it stays put rather than wrapping or panicking.
        input.handle_key(&pressed(crate::event::Key::Right));
        assert_eq!(state(&input), (text, text.len()));
    }

    /// Backspace removes the character before this one in the string, and would
    /// keep doing so even if C-Q2 made the arrows visual: "the previous
    /// character" is what a reader of that script means regardless of which
    /// side of the caret it is drawn on. Deleting and moving are allowed to
    /// disagree.
    #[test]
    fn backspace_deletes_the_previous_character_in_the_string() {
        let mut input = Widget::text_input("ab\u{05D0}\u{05D1}cd", "");
        // Caret at the end; backspace twice takes `d` then `c`.
        input.handle_key(&pressed(crate::event::Key::Backspace));
        assert_eq!(state(&input), ("ab\u{05D0}\u{05D1}c", 7));
        input.handle_key(&pressed(crate::event::Key::Backspace));
        assert_eq!(state(&input), ("ab\u{05D0}\u{05D1}", 6));
        // Now the Hebrew, whole characters at a time and never half of one.
        input.handle_key(&pressed(crate::event::Key::Backspace));
        assert_eq!(state(&input), ("ab\u{05D0}", 4));
    }

    // ======================================================================
    // Focus
    // ======================================================================

    fn shifted(key: crate::event::Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: crate::event::Modifiers {
                shift: true,
                ..crate::event::Modifiers::NONE
            },
            text: String::new(),
        }
    }

    fn clicked(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(crate::event::MouseButton::Left),
        })
    }

    /// Two text fields in one window, and the user clicks the first one.
    ///
    /// This is the bug focus was introduced to fix, and it was not subtle:
    /// `handle_event` walked the children back to front and gave the event to
    /// the first that took it, for keystrokes as well as clicks. A text field
    /// takes every keystroke, so the *last* field in the tree consumed all of
    /// them — typing into the first field of a two-field dialog filled in the
    /// second, whatever the user had clicked.
    #[test]
    fn typing_goes_to_the_field_that_was_clicked_and_not_the_last_one_in_the_tree() {
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_child(Widget::text_input("", "first"))
            .with_child(Widget::text_input("", "second"));
        let mut tree = WidgetTree::new(root, 300.0, 200.0);
        tree.layout();

        let first = tree.root.children[0].id;
        let (fx, fy) = (
            tree.root.children[0].layout.x + 2.0,
            tree.root.children[0].layout.y + 2.0,
        );
        tree.handle_event(&clicked(fx, fy));
        assert_eq!(tree.focused_id(), Some(first));

        for ch in "hello".chars() {
            tree.handle_event(&Event::Key(typed(ch)));
        }
        assert_eq!(state(&tree.root.children[0]).0, "hello");
        assert_eq!(
            state(&tree.root.children[1]).0,
            "",
            "the field the user did not click must not receive a single character"
        );
    }

    /// With nothing focused a keystroke has nowhere to go, and must go nowhere
    /// — not to whichever field happens to be last.
    #[test]
    fn a_window_nobody_has_clicked_in_swallows_typing_rather_than_guessing() {
        let root = Widget::container().with_child(Widget::text_input("", "name"));
        let mut tree = WidgetTree::new(root, 300.0, 200.0);
        tree.layout();

        assert_eq!(tree.focused_id(), None);
        assert_eq!(
            tree.handle_event(&Event::Key(typed('x'))),
            EventResult::Ignored
        );
        assert_eq!(state(&tree.root.children[0]).0, "");
    }

    #[test]
    fn tab_visits_every_control_in_reading_order_and_wraps() {
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_child(Widget::label("not focusable"))
            .with_child(Widget::text_input("", "name"))
            .with_child(Widget::checkbox("remember me", false))
            .with_child(Widget::button("OK"));
        let mut tree = WidgetTree::new(root, 300.0, 200.0);
        tree.layout();

        let want: Vec<_> = [1_usize, 2, 3]
            .iter()
            .map(|&i| tree.root.children[i].id)
            .collect();
        assert_eq!(
            tree.root.focus_order(),
            want,
            "the label is not in the order, and the other three are in the order they are read"
        );

        let mut seen = Vec::new();
        for _ in 0..4 {
            tree.handle_event(&Event::Key(pressed(crate::event::Key::Tab)));
            seen.push(tree.focused_id().unwrap());
        }
        assert_eq!(
            seen,
            vec![want[0], want[1], want[2], want[0]],
            "the fourth Tab must come back to the first, not stop at the last"
        );

        tree.handle_event(&Event::Key(shifted(crate::event::Key::Tab)));
        assert_eq!(
            tree.focused_id(),
            Some(want[2]),
            "Shift+Tab wraps backwards"
        );
    }

    /// Focus that lands somewhere the user cannot see or use is focus that has
    /// disappeared: Tab would seem to do nothing, once, at random.
    #[test]
    fn tab_steps_over_a_control_that_is_disabled_or_hidden() {
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_child(Widget::text_input("", "one"))
            .with_child(Widget::text_input("", "off").disabled())
            .with_child(Widget::text_input("", "gone").hidden())
            .with_child(Widget::text_input("", "two"));
        let mut tree = WidgetTree::new(root, 300.0, 400.0);
        tree.layout();

        let first = tree.root.children[0].id;
        let last = tree.root.children[3].id;
        assert_eq!(tree.root.focus_order(), vec![first, last]);

        tree.handle_event(&Event::Key(pressed(crate::event::Key::Tab)));
        tree.handle_event(&Event::Key(pressed(crate::event::Key::Tab)));
        assert_eq!(tree.focused_id(), Some(last));
    }

    /// Focus given by id is the path Tab and the mouse do not take, and it is
    /// the one that can be handed an id the caller looked up before the widget
    /// was disabled. Both halves of `WidgetTree::focus`'s promise are tested
    /// here because the dangerous failure is the second: a caller told `false`
    /// while the focus quietly stayed where it was would send the next
    /// keystroke to a field it believes nothing is focused in.
    #[test]
    fn focusing_by_id_refuses_what_tab_would_have_skipped_and_still_lets_go() {
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_child(Widget::text_input("", "typed-in"))
            .with_child(Widget::label("just a label"))
            .with_child(Widget::text_input("", "off").disabled())
            .with_child(Widget::text_input("", "gone").hidden());
        let mut tree = WidgetTree::new(root, 300.0, 400.0);
        tree.layout();

        let field = tree.root.children[0].id;
        let refused = [
            ("a label", tree.root.children[1].id),
            ("a disabled field", tree.root.children[2].id),
            ("a hidden field", tree.root.children[3].id),
        ];

        for (what, id) in refused {
            assert!(tree.focus(Some(field)), "the field itself must take focus");
            assert_eq!(tree.focused_id(), Some(field));

            assert!(!tree.focus(Some(id)), "focus must refuse {what}");
            assert_eq!(
                tree.focused_id(),
                None,
                "a refused focus ({what}) must still let go of the widget that \
                 had it — otherwise the caller is told the focus did not move \
                 while the next keystroke goes to the field it was on"
            );
        }
    }

    /// Clicking the background is how a user says "not that field any more". A
    /// caret still blinking in a field that has been clicked away from is a
    /// caret that lies about where the next keystroke will go.
    #[test]
    fn clicking_away_from_every_control_takes_the_focus_with_it() {
        let root = Widget::container()
            .with_padding(Edges::all(40.0))
            .with_child(Widget::text_input("", "name"));
        let mut tree = WidgetTree::new(root, 300.0, 200.0);
        tree.layout();

        tree.focus_first();
        assert!(tree.focused_id().is_some());

        tree.handle_event(&clicked(2.0, 2.0));
        assert_eq!(tree.focused_id(), None);
    }

    /// A focused button has to be pressable from the keyboard, or putting it in
    /// the tab order is worse than leaving it out: the user watches the focus
    /// land somewhere it cannot act.
    #[test]
    fn the_keyboard_can_work_a_control_it_has_tabbed_to() {
        let root = Widget::container()
            .with_flex_direction(FlexDirection::Column)
            .with_child(Widget::checkbox("remember me", false))
            .with_child(Widget::button("OK"));
        let mut tree = WidgetTree::new(root, 300.0, 200.0);
        tree.layout();

        tree.handle_event(&Event::Key(pressed(crate::event::Key::Tab)));
        tree.handle_event(&Event::Key(pressed(crate::event::Key::Space)));
        assert!(
            matches!(
                tree.root.children[0].kind,
                WidgetKind::Checkbox {
                    checked: CheckState::Checked,
                    ..
                }
            ),
            "Space must tick the focused checkbox"
        );

        tree.handle_event(&Event::Key(pressed(crate::event::Key::Tab)));
        tree.handle_event(&Event::Key(pressed(crate::event::Key::Enter)));
        assert!(
            matches!(
                tree.root.children[1].kind,
                WidgetKind::Button { pressed: true, .. }
            ),
            "Enter must press the focused button"
        );
        assert!(
            matches!(
                tree.root.children[0].kind,
                WidgetKind::Checkbox {
                    checked: CheckState::Checked,
                    ..
                }
            ),
            "and must not also reach the control the focus has left"
        );
    }

    // ======================================================================
    // The caret, the selection and the scroll offset
    // ======================================================================

    /// The palette the tests draw and lay out in: a tree's own default.
    fn test_palette() -> Palette {
        Palette::for_mode(true)
    }

    fn laid_out(mut w: Widget, width: f32) -> Widget {
        w.do_layout(
            SizeConstraint {
                min_width: width,
                max_width: width,
                min_height: 28.0,
                max_height: 28.0,
            },
            &test_palette(),
        );
        w
    }

    fn drawn(w: &Widget) -> RenderTree {
        let mut tree = RenderTree::new();
        w.render(&test_palette(), &mut tree);
        tree
    }

    /// Where the vertical rules are. The caret is the only vertical line a text
    /// field draws, so this finds it without the test needing to know which
    /// command index it lands at.
    fn caret_xs(w: &Widget) -> Vec<f32> {
        drawn(w)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Line { x1, x2, .. } if (x1 - x2).abs() < f32::EPSILON => Some(*x1),
                _ => None,
            })
            .collect()
    }

    fn content_left(w: &Widget) -> f32 {
        w.layout.x + w.layout.margin.left + w.layout.border_widths.left + w.layout.padding.left
    }

    fn selection_of(w: &Widget) -> Option<(usize, usize)> {
        match &w.kind {
            WidgetKind::TextInput {
                cursor,
                selection_anchor,
                ..
            } => selection_anchor.map(|a| (a.min(cursor.byte()), a.max(cursor.byte()))),
            _ => panic!("not a text input"),
        }
    }

    /// The field tracked a caret and moved it and never drew it, so a user
    /// typing into it saw the text change with no sign of where the next
    /// character would go, and the arrow keys appeared to do nothing at all.
    /// See `known-issues.md` →
    /// `TD-C-TWO-TOOLKIT-TEXT-FIELDS-DRAW-NO-CARET-AT-ALL`.
    #[test]
    fn a_focused_field_draws_a_caret_and_an_unfocused_one_does_not() {
        let unfocused = laid_out(Widget::text_input("abc", ""), 200.0);
        assert!(
            caret_xs(&unfocused).is_empty(),
            "an unfocused field must not draw a caret: two carets in one window \
             are two claims about where typing goes"
        );

        let mut focused = laid_out(Widget::text_input("abc", ""), 200.0);
        focused.focused = true;
        assert_eq!(caret_xs(&focused).len(), 1);
    }

    /// An empty field is the one a user is most likely to be about to type
    /// into, so it is the one that most needs to say so.
    #[test]
    fn an_empty_field_still_shows_where_typing_will_land() {
        let mut w = laid_out(Widget::text_input("", "Type here"), 200.0);
        w.focused = true;
        assert_eq!(caret_xs(&w).len(), 1);
    }

    /// The caret has to be drawn where the cursor *is*, which is a question for
    /// the shaper. Measuring the prefix would answer it wrongly wherever the
    /// text changes direction — and the arrows already move by the shaper, so a
    /// caret placed any other way would drift away from the text it is in.
    #[test]
    fn the_drawn_caret_follows_the_arrow_keys() {
        let mut w = laid_out(Widget::text_input("iiiWWW", ""), 200.0);
        w.focused = true;

        let at_end = caret_xs(&w)[0];
        w.handle_key(&pressed(crate::event::Key::Left));
        let one_back = caret_xs(&w)[0];
        w.handle_key(&pressed(crate::event::Key::Left));
        let two_back = caret_xs(&w)[0];

        assert!(
            two_back < one_back && one_back < at_end,
            "each Left must move the drawn caret leftwards: \
             {two_back} < {one_back} < {at_end}"
        );
    }

    /// Without a scroll offset, a caret past the right-hand edge is drawn
    /// outside the field. The text has to move under it instead.
    #[test]
    fn a_string_longer_than_its_box_scrolls_to_keep_the_caret_in_view() {
        let long = "the quick brown fox jumps over the lazy dog, twice over";
        let mut w = laid_out(Widget::text_input(long, ""), 80.0);
        w.focused = true;

        let left = content_left(&w);
        let right = left + w.layout.width;

        let at_end = caret_xs(&w)[0];
        assert!(
            at_end >= left && at_end <= right,
            "with the caret at the end of a long string it must still be inside \
             the field: {left} <= {at_end} <= {right}"
        );

        // And back at the start of the string the view returns, rather than
        // leaving the caret pinned off the left-hand edge.
        w.handle_key(&pressed(crate::event::Key::Home));
        let at_start = caret_xs(&w)[0];
        assert!(
            at_start >= left && at_start <= right,
            "and at the start too: {left} <= {at_start} <= {right}"
        );
    }

    /// Scrolling draws the head of the string to the left of the field. Without
    /// a clip that ink lands on whatever else is there — so the clip is part of
    /// the feature, not a decoration on it.
    #[test]
    fn a_scrolled_field_clips_what_it_pushes_outside_itself() {
        let long = "the quick brown fox jumps over the lazy dog, twice over";
        let mut w = laid_out(Widget::text_input(long, ""), 80.0);
        w.focused = true;

        let tree = drawn(&w);
        let pushes = tree
            .commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::PushClip { .. }))
            .count();
        let pops = tree
            .commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::PopClip))
            .count();
        assert!(pushes > 0, "a scrolled field must clip its own contents");
        assert_eq!(pushes, pops, "every clip must be popped");
    }

    #[test]
    fn shift_and_an_arrow_select_and_a_bare_arrow_gives_the_selection_up() {
        let mut w = Widget::text_input("hello", "");
        w.handle_key(&shifted(crate::event::Key::Left));
        w.handle_key(&shifted(crate::event::Key::Left));
        assert_eq!(
            selection_of(&w),
            Some((3, 5)),
            "a run of Shift+Left must extend one selection, not restart it each press"
        );

        w.handle_key(&pressed(crate::event::Key::Left));
        assert_eq!(
            selection_of(&w),
            None,
            "an arrow without Shift is a move, and a move ends the selection — \
             otherwise the next keystroke deletes text the user stopped selecting"
        );
    }

    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut w = Widget::text_input("hello", "");
        for _ in 0..3 {
            w.handle_key(&shifted(crate::event::Key::Left));
        }
        w.handle_key(&typed('p'));
        assert_eq!(state(&w), ("hep", 3));
        assert_eq!(selection_of(&w), None);
    }

    /// Backspace with a selection up deletes the selection and stops. Deleting
    /// the selection *and* the character before it removes something the user
    /// never pointed at, which is the one editing mistake that cannot be seen
    /// happening.
    #[test]
    fn backspace_over_a_selection_takes_the_selection_and_nothing_more() {
        let mut w = Widget::text_input("hello", "");
        for _ in 0..2 {
            w.handle_key(&shifted(crate::event::Key::Left));
        }
        w.handle_key(&pressed(crate::event::Key::Backspace));
        assert_eq!(state(&w), ("hel", 3));
    }

    #[test]
    fn a_selection_that_spans_a_multi_byte_character_is_cut_on_a_boundary() {
        // Deleting three characters back from the end of `ab` + two Hebrew
        // letters + `cd`. Each Hebrew letter is two bytes, so a selection
        // measured in characters and applied in bytes would cut one in half —
        // and `String::drain` panics on an offset inside a character, taking
        // the application down over a keystroke.
        let mut w = Widget::text_input("ab\u{05D0}\u{05D1}cd", "");
        for _ in 0..3 {
            w.handle_key(&shifted(crate::event::Key::Left));
        }
        w.handle_key(&pressed(crate::event::Key::Delete));
        let (text, at) = state(&w);
        assert_eq!(
            text.chars().count(),
            3,
            "three characters selected, three characters gone: {text:?}"
        );
        assert!(
            text.is_char_boundary(at),
            "and the caret must be left on a character boundary, not inside one"
        );
    }

    #[test]
    fn a_selection_is_painted_and_its_text_is_drawn_over_the_paint() {
        let mut w = laid_out(Widget::text_input("hello", ""), 200.0);
        w.focused = true;
        for _ in 0..2 {
            w.handle_key(&shifted(crate::event::Key::Left));
        }

        let tree = drawn(&w);
        // A style that says no selection colours takes the palette's: the
        // user's accent, and the ink that reads on it.
        let accent = test_palette().accent;
        let highlighted = tree
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == accent));
        assert!(highlighted, "the selected range must be painted");

        let on_accent = crate::palette::readable_on(accent);
        let recoloured = tree.commands.iter().any(|c| {
            matches!(c, RenderCommand::RichText { spans, .. }
                if spans.iter().any(|s| s.color == on_accent))
        });
        assert!(
            recoloured,
            "and the selected text must be drawn in a colour that shows against it"
        );
    }

    /// A click puts the caret where it landed, resolved by the shaper. Dividing
    /// by an average character width would put it somewhere else in any string
    /// whose glyphs are not all one width, which is every string.
    #[test]
    fn clicking_in_the_text_puts_the_caret_where_the_click_landed() {
        let mut w = laid_out(Widget::text_input("WWWiii", ""), 200.0);
        w.focused = true;
        let left = content_left(&w);

        w.handle_mouse(&MouseEvent {
            x: left + 0.5,
            y: w.layout.y + 4.0,
            kind: MouseEventKind::Press(crate::event::MouseButton::Left),
        });
        assert_eq!(
            state(&w).1,
            0,
            "a click at the left edge lands before the text"
        );

        let full = crate::text::measure("WWWiii", w.style.font_size, FontWeightHint::Regular);
        w.handle_mouse(&MouseEvent {
            x: left + full,
            y: w.layout.y + 4.0,
            kind: MouseEventKind::Press(crate::event::MouseButton::Left),
        });
        assert_eq!(
            state(&w).1,
            6,
            "and one past the last glyph lands after all of it"
        );
    }

    /// The test above uses a string that fits, so the field's scroll offset is
    /// zero and a click that forgot to add it still lands in the right place.
    /// This one uses a string that does not fit. A text input opens with its
    /// caret at the end, so a long value opens scrolled to its tail: the glyph
    /// under the left edge of the box is somewhere in the middle of the string,
    /// not its first character.
    ///
    /// The two clicks are made on separate widgets on purpose. The scroll
    /// offset is recomputed from the caret on every frame, so the first click
    /// changes where the second one would land.
    #[test]
    fn clicking_in_a_scrolled_field_accounts_for_what_has_scrolled_off() {
        let long = "the quick brown fox jumps over the lazy dog, twice over";

        let mut at_left = laid_out(Widget::text_input(long, ""), 80.0);
        at_left.focused = true;
        assert_eq!(
            state(&at_left).1,
            long.len(),
            "a field opens with its caret at the end, which is what scrolls it"
        );

        at_left.handle_mouse(&MouseEvent {
            x: content_left(&at_left) + 0.5,
            y: at_left.layout.y + 4.0,
            kind: MouseEventKind::Press(crate::event::MouseButton::Left),
        });
        assert!(
            state(&at_left).1 > long.len() / 2,
            "the left edge of a field scrolled to its tail shows the far end of \
             the string, so a click there must land there — a click that ignores \
             the scroll offset lands at byte 0 instead, got {}",
            state(&at_left).1
        );

        let mut at_right = laid_out(Widget::text_input(long, ""), 80.0);
        at_right.focused = true;
        at_right.handle_mouse(&MouseEvent {
            x: content_left(&at_right) + at_right.layout.width,
            y: at_right.layout.y + 4.0,
            kind: MouseEventKind::Press(crate::event::MouseButton::Left),
        });
        assert_eq!(
            state(&at_right).1,
            long.len(),
            "and the right edge shows the last character, not one a box-width in"
        );
    }

    #[test]
    fn a_click_gives_up_the_selection_it_landed_on() {
        let mut w = laid_out(Widget::text_input("hello", ""), 200.0);
        w.focused = true;
        for _ in 0..3 {
            w.handle_key(&shifted(crate::event::Key::Left));
        }
        assert!(selection_of(&w).is_some());

        w.handle_mouse(&MouseEvent {
            x: content_left(&w) + 1.0,
            y: w.layout.y + 4.0,
            kind: MouseEventKind::Press(crate::event::MouseButton::Left),
        });
        assert_eq!(selection_of(&w), None);
    }
}
