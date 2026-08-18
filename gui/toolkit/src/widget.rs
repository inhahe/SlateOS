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

use crate::color::Color;
use crate::event::{Event, EventResult, KeyEvent, MouseEvent, MouseEventKind};
use crate::layout::{
    FlexAlign, FlexDirection, FlexItem, FlexJustify, FlexLayout, LayoutBox, Size, SizeConstraint,
    flex_layout,
};
use crate::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use crate::style::{Borders, CornerRadii, Edges, FontWeight, Style};
use crate::text::TextCursor;

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
        selection: Option<(usize, usize)>,
    },
    /// Multi-line text area.
    TextArea {
        value: String,
        placeholder: String,
        cursor_pos: usize,
        scroll_offset: f32,
    },
    /// Checkbox.
    Checkbox { checked: CheckState, label: String },
    /// Radio button.
    RadioButton { selected: bool, label: String },
    /// Scroll view wrapper.
    ScrollView {
        scroll_x: f32,
        scroll_y: f32,
        content_width: f32,
        content_height: f32,
    },
    /// Horizontal or vertical separator line.
    Separator { vertical: bool },
    /// Progress bar.
    ProgressBar { value: f32, max: f32 },
    /// Slider.
    Slider { value: f32, min: f32, max: f32 },
    /// Image display.
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

impl Widget {
    // ======================================================================
    // Constructors
    // ======================================================================

    pub fn container() -> Self {
        Self {
            id: WidgetId::new(),
            kind: WidgetKind::Container,
            style: Style::default(),
            flex_item: FlexItem::default(),
            flex_layout: Some(FlexLayout::default()),
            children: Vec::new(),
            layout: LayoutBox::default(),
            enabled: true,
            visible: true,
            tooltip: None,
        }
    }

    pub fn label(text: &str) -> Self {
        Self {
            id: WidgetId::new(),
            kind: WidgetKind::Label {
                text: text.to_string(),
            },
            style: Style::default(),
            flex_item: FlexItem::default(),
            flex_layout: None,
            children: Vec::new(),
            layout: LayoutBox::default(),
            enabled: true,
            visible: true,
            tooltip: None,
        }
    }

    pub fn button(text: &str) -> Self {
        Self {
            id: WidgetId::new(),
            kind: WidgetKind::Button {
                text: text.to_string(),
                pressed: false,
            },
            style: Style {
                background: Color::from_hex(0xE0E0E0),
                padding: Edges::symmetric(6.0, 16.0),
                border: Borders::all(1.0, Color::from_hex(0xA0A0A0)),
                border_radius: CornerRadii::all(4.0),
                ..Style::default()
            },
            flex_item: FlexItem::default(),
            flex_layout: None,
            children: Vec::new(),
            layout: LayoutBox::default(),
            enabled: true,
            visible: true,
            tooltip: None,
        }
    }

    pub fn text_input(value: &str, placeholder: &str) -> Self {
        Self {
            id: WidgetId::new(),
            kind: WidgetKind::TextInput {
                value: value.to_string(),
                placeholder: placeholder.to_string(),
                cursor: TextCursor::from(value.len()),
                selection: None,
            },
            style: Style {
                background: Color::WHITE,
                padding: Edges::symmetric(4.0, 8.0),
                border: Borders::all(1.0, Color::from_hex(0xC0C0C0)),
                border_radius: CornerRadii::all(3.0),
                min_width: Some(120.0),
                min_height: Some(28.0),
                ..Style::default()
            },
            flex_item: FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
            flex_layout: None,
            children: Vec::new(),
            layout: LayoutBox::default(),
            enabled: true,
            visible: true,
            tooltip: None,
        }
    }

    pub fn checkbox(label: &str, checked: bool) -> Self {
        Self {
            id: WidgetId::new(),
            kind: WidgetKind::Checkbox {
                checked: if checked {
                    CheckState::Checked
                } else {
                    CheckState::Unchecked
                },
                label: label.to_string(),
            },
            style: Style {
                padding: Edges::symmetric(4.0, 4.0),
                ..Style::default()
            },
            flex_item: FlexItem::default(),
            flex_layout: None,
            children: Vec::new(),
            layout: LayoutBox::default(),
            enabled: true,
            visible: true,
            tooltip: None,
        }
    }

    pub fn progress_bar(value: f32, max: f32) -> Self {
        Self {
            id: WidgetId::new(),
            kind: WidgetKind::ProgressBar { value, max },
            style: Style {
                background: Color::from_hex(0xE8E8E8),
                border: Borders::all(1.0, Color::from_hex(0xC0C0C0)),
                border_radius: CornerRadii::all(3.0),
                min_height: Some(20.0),
                ..Style::default()
            },
            flex_item: FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
            flex_layout: None,
            children: Vec::new(),
            layout: LayoutBox::default(),
            enabled: true,
            visible: true,
            tooltip: None,
        }
    }

    pub fn separator() -> Self {
        Self {
            id: WidgetId::new(),
            kind: WidgetKind::Separator { vertical: false },
            style: Style {
                background: Color::from_hex(0xD0D0D0),
                margin: Edges::symmetric(8.0, 0.0),
                min_height: Some(1.0),
                ..Style::default()
            },
            flex_item: FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
            flex_layout: None,
            children: Vec::new(),
            layout: LayoutBox::default(),
            enabled: true,
            visible: true,
            tooltip: None,
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

    /// Compute intrinsic content size for this widget.
    pub fn intrinsic_size(&self) -> Size {
        match &self.kind {
            WidgetKind::Label { text } => {
                let width = self.measure(text);
                let height = self.style.font_size * self.style.line_height;
                Size::new(
                    width + self.style.padding.horizontal(),
                    height + self.style.padding.vertical(),
                )
            }
            WidgetKind::Button { text, .. } => {
                let width = self.measure(text);
                let height = self.style.font_size * self.style.line_height;
                Size::new(
                    width + self.style.padding.horizontal(),
                    height + self.style.padding.vertical(),
                )
            }
            WidgetKind::TextInput { .. } => Size::new(
                self.style.min_width.unwrap_or(120.0),
                self.style.min_height.unwrap_or(28.0),
            ),
            WidgetKind::Checkbox { label, .. } => {
                let checkbox_size = self.style.font_size;
                let width = checkbox_size + 8.0 + self.measure(label);
                let height = self.style.font_size * self.style.line_height;
                Size::new(
                    width + self.style.padding.horizontal(),
                    height + self.style.padding.vertical(),
                )
            }
            WidgetKind::ProgressBar { .. } => Size::new(
                self.style.min_width.unwrap_or(200.0),
                self.style.min_height.unwrap_or(20.0),
            ),
            WidgetKind::Separator { vertical } => {
                if *vertical {
                    Size::new(1.0, 0.0)
                } else {
                    Size::new(0.0, 1.0)
                }
            }
            WidgetKind::Container => Size::ZERO,
            _ => Size::new(
                self.style.min_width.unwrap_or(0.0),
                self.style.min_height.unwrap_or(0.0),
            ),
        }
    }

    /// Perform layout on this widget and all children.
    pub fn do_layout(&mut self, constraint: SizeConstraint) {
        if !self.visible {
            return;
        }

        let content_size = constraint.constrain(self.intrinsic_size());
        self.layout.width = content_size.width;
        self.layout.height = content_size.height;
        self.layout.padding = self.style.padding;
        self.layout.margin = self.style.margin;
        self.layout.border_widths = Edges {
            top: self.style.border.top.width,
            right: self.style.border.right.width,
            bottom: self.style.border.bottom.width,
            left: self.style.border.left.width,
        };

        if let Some(ref flex) = self.flex_layout
            && !self.children.is_empty()
        {
            // Compute child intrinsic sizes
            let child_info: Vec<(Size, FlexItem)> = self
                .children
                .iter()
                .filter(|c| c.visible)
                .map(|c| (c.intrinsic_size(), c.flex_item.clone()))
                .collect();

            let container_size = Size::new(
                constraint.max_width - self.style.margin.horizontal(),
                constraint.max_height - self.style.margin.vertical(),
            );

            let layouts = flex_layout(container_size, flex, &child_info, &self.style.padding);

            // Apply layout results to children
            let mut visible_idx = 0;
            for child in &mut self.children {
                if !child.visible {
                    continue;
                }
                if visible_idx < layouts.len() {
                    let lb = &layouts[visible_idx];
                    child.layout.x = lb.x;
                    child.layout.y = lb.y;

                    // Recursively layout children with their computed size
                    let child_constraint = SizeConstraint {
                        min_width: 0.0,
                        max_width: lb.width,
                        min_height: 0.0,
                        max_height: lb.height,
                    };
                    child.do_layout(child_constraint);
                    child.layout.x = lb.x;
                    child.layout.y = lb.y;
                    child.layout.width = lb.width;
                    child.layout.height = lb.height;
                }
                visible_idx += 1;
            }

            // Update own size to fit content if unconstrained
            if constraint.max_width == f32::INFINITY {
                let max_x = layouts.iter().map(|l| l.x + l.width).fold(0.0f32, f32::max);
                self.layout.width = max_x + self.style.padding.right;
            }
            if constraint.max_height == f32::INFINITY {
                let max_y = layouts
                    .iter()
                    .map(|l| l.y + l.height)
                    .fold(0.0f32, f32::max);
                self.layout.height = max_y + self.style.padding.bottom;
            }
        }
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Render this widget and all children into a render tree.
    pub fn render(&self, tree: &mut RenderTree) {
        if !self.visible {
            return;
        }

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

        // Border
        let border_width = self.style.border.top.width;
        if border_width > 0.0 {
            tree.push(RenderCommand::StrokeRect {
                x,
                y,
                width: w,
                height: h,
                color: self.style.border.top.color,
                line_width: border_width,
                corner_radii: self.style.border_radius,
            });
        }

        // Content
        let cx = x + self.layout.border_widths.left + self.layout.padding.left;
        let cy = y + self.layout.border_widths.top + self.layout.padding.top;

        match &self.kind {
            WidgetKind::Label { text } => {
                tree.push(RenderCommand::Text {
                    x: cx,
                    y: cy,
                    text: text.clone(),
                    color: self.style.foreground,
                    font_size: self.style.font_size,
                    font_weight: weight_to_hint(self.style.font_weight),
                    max_width: Some(self.layout.width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            WidgetKind::Button { text, pressed } => {
                let bg = if *pressed {
                    Color::from_hex(0xC0C0C0)
                } else {
                    self.style.background
                };
                // Re-render background if pressed changes it
                if *pressed {
                    tree.push(RenderCommand::FillRect {
                        x,
                        y,
                        width: w,
                        height: h,
                        color: bg,
                        corner_radii: self.style.border_radius,
                    });
                }
                tree.push(RenderCommand::Text {
                    x: cx,
                    y: cy,
                    text: text.clone(),
                    color: self.style.foreground,
                    font_size: self.style.font_size,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(self.layout.width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            WidgetKind::TextInput {
                value, placeholder, ..
            } => {
                let display_text = if value.is_empty() {
                    placeholder.as_str()
                } else {
                    value.as_str()
                };
                let color = if value.is_empty() {
                    Color::GRAY
                } else {
                    self.style.foreground
                };
                tree.push(RenderCommand::Text {
                    x: cx,
                    y: cy,
                    text: display_text.to_string(),
                    color,
                    font_size: self.style.font_size,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(self.layout.width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            WidgetKind::Checkbox { checked, label } => {
                // Draw checkbox box
                let box_size = self.style.font_size;
                tree.push(RenderCommand::StrokeRect {
                    x: cx,
                    y: cy + 2.0,
                    width: box_size,
                    height: box_size,
                    color: Color::from_hex(0x606060),
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(2.0),
                });
                if *checked == CheckState::Checked {
                    tree.push(RenderCommand::FillRect {
                        x: cx + 3.0,
                        y: cy + 5.0,
                        width: box_size - 6.0,
                        height: box_size - 6.0,
                        color: Color::from_hex(0x0078D7),
                        corner_radii: CornerRadii::ZERO,
                    });
                }
                // Draw label
                tree.push(RenderCommand::Text {
                    x: cx + box_size + 8.0,
                    y: cy,
                    text: label.clone(),
                    color: self.style.foreground,
                    font_size: self.style.font_size,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            WidgetKind::ProgressBar { value, max } => {
                let fraction = if *max > 0.0 { value / max } else { 0.0 };
                let fill_width = self.layout.width * fraction.clamp(0.0, 1.0);
                tree.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: fill_width,
                    height: self.layout.height,
                    color: Color::from_hex(0x0078D7),
                    corner_radii: CornerRadii::all(2.0),
                });
            }
            WidgetKind::Separator { vertical } => {
                if *vertical {
                    tree.push(RenderCommand::Line {
                        x1: cx + w / 2.0,
                        y1: cy,
                        x2: cx + w / 2.0,
                        y2: cy + h,
                        color: self.style.background,
                        width: 1.0,
                    });
                } else {
                    tree.push(RenderCommand::Line {
                        x1: cx,
                        y1: cy + h / 2.0,
                        x2: cx + self.layout.width,
                        y2: cy + h / 2.0,
                        color: self.style.background,
                        width: 1.0,
                    });
                }
            }
            WidgetKind::Container => {} // Container renders through children
            _ => {}
        }

        // Render children with translation
        if !self.children.is_empty() {
            tree.push(RenderCommand::PushTranslate { dx: cx, dy: cy });
            tree.push(RenderCommand::PushClip {
                x: 0.0,
                y: 0.0,
                width: self.layout.width,
                height: self.layout.height,
            });

            for child in &self.children {
                child.render(tree);
            }

            tree.push(RenderCommand::PopClip);
            tree.push(RenderCommand::PopTranslate);
        }
    }

    // ======================================================================
    // Event handling
    // ======================================================================

    /// Dispatch an event to this widget. Returns whether it was consumed.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.enabled || !self.visible {
            return EventResult::Ignored;
        }

        // Try children first (front-to-back, last child is "on top")
        for child in self.children.iter_mut().rev() {
            if child.handle_event(event) == EventResult::Consumed {
                return EventResult::Consumed;
            }
        }

        // Handle at this widget level
        match event {
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Key(key) => self.handle_key(key),
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        // Check if mouse is within this widget's bounds
        let x = self.layout.x;
        let y = self.layout.y;
        let w = self.layout.outer_width();
        let h = self.layout.outer_height();

        if mouse.x < x || mouse.x > x + w || mouse.y < y || mouse.y > y + h {
            return EventResult::Ignored;
        }

        match &mut self.kind {
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
                    *checked = match *checked {
                        CheckState::Unchecked => CheckState::Checked,
                        CheckState::Checked => CheckState::Unchecked,
                        CheckState::Indeterminate => CheckState::Checked,
                    };
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

        match &mut self.kind {
            WidgetKind::TextInput { value, cursor, .. } => {
                if let Some(ch) = key.text {
                    value.insert(cursor.byte(), ch);
                    // Typing lands the caret after what was typed, which is the
                    // downstream side of the new boundary whichever way the
                    // surrounding text runs — and, now that the text contains
                    // the character, the next boundary the text itself names.
                    *cursor = cursor
                        .next_in(value)
                        .unwrap_or_else(|| TextCursor::from(value.len()));
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
                        if let Some(prev) = cursor.prev_in(value) {
                            value.remove(prev.byte());
                            *cursor = prev;
                        }
                        EventResult::Consumed
                    }
                    // The arrows step *logically* — one character later or
                    // earlier in the string — which on a line mixing
                    // directions is not the same as one step right or left on
                    // the screen. That is deliberate and is not an oversight:
                    // macOS, GTK and Qt move logically, Windows moves visually,
                    // both ship, and picking between them is a user-visible
                    // policy the operator has not yet decided. See
                    // `open-questions.md` → C-Q2.
                    //
                    // The visual alternative is fully built and tested —
                    // `text::caret_left`/`caret_right`, and this widget already
                    // stores the `TextCursor` they need. Answering C-Q2
                    // "visual" is two edits and nothing else: bind
                    // `let font_size = self.style.font_size;` *above* the
                    // `match &mut self.kind` (it has to be read before `kind`
                    // is borrowed mutably), then replace each arm's body with
                    // `if let Some(next) = crate::text::caret_left(value,
                    // *cursor, font_size, FontWeightHint::Regular) { *cursor =
                    // next; }`. Measuring at the size the text is *drawn* at is
                    // not optional — the gaps between glyphs are a property of
                    // the shaped run, so another size moves the caret to a
                    // place this widget never drew it. Do not make that switch
                    // without an answer.
                    crate::event::Key::Left => {
                        if let Some(prev) = cursor.prev_in(value) {
                            *cursor = prev;
                        }
                        EventResult::Consumed
                    }
                    crate::event::Key::Right => {
                        if let Some(next) = cursor.next_in(value) {
                            *cursor = next;
                        }
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
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
}

impl WidgetTree {
    pub fn new(root: Widget, width: f32, height: f32) -> Self {
        Self {
            root,
            window_width: width,
            window_height: height,
        }
    }

    /// Perform layout on the entire tree.
    pub fn layout(&mut self) {
        let constraint = SizeConstraint {
            min_width: self.window_width,
            max_width: self.window_width,
            min_height: self.window_height,
            max_height: self.window_height,
        };
        self.root.do_layout(constraint);
        self.root.layout.width = self.window_width;
        self.root.layout.height = self.window_height;
    }

    /// Render the entire tree into a render command list.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();
        self.root.render(&mut tree);
        tree
    }

    /// Dispatch an event to the widget tree.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
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

        // Root should be the full window size
        assert_eq!(tree.root.layout.width, 400.0);
        assert_eq!(tree.root.layout.height, 300.0);

        // Children should have positions set
        assert!(tree.root.children.len() == 3);
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
            text: Some(ch),
        }
    }

    fn pressed(key: crate::event::Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: None,
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

    /// The arrows move **logically** — one character earlier or later in the
    /// string — which on `ab` + two Hebrew letters + `cd` walks the byte
    /// offsets 7, 6, 4, 2, 1, 0 straight down, even though the line is drawn
    /// `a b <bet> <aleph> c d` and the caret therefore jumps sideways across
    /// the Hebrew rather than stepping through it.
    ///
    /// **This test pins a policy, not a truth.** Logical is what macOS, GTK and
    /// Qt do; Windows moves visually; the choice is `open-questions.md` → C-Q2
    /// and the operator has not answered it. The visual behaviour is built and
    /// tested in `text::caret_left`/`caret_right`, and this test is what will
    /// change — to 7, 6, 4, 6, 1, 0 — if C-Q2 answers "visual". Do not "fix"
    /// this test to match the visual sequence without that answer.
    #[test]
    fn the_arrows_move_by_the_string_pending_c_q2() {
        let text = "ab\u{05D0}\u{05D1}cd";
        let mut input = Widget::text_input(text, "");
        let mut seen = vec![];
        for _ in 0..6 {
            input.handle_key(&pressed(crate::event::Key::Left));
            seen.push(state(&input).1);
        }
        assert_eq!(seen, vec![7, 6, 4, 2, 1, 0]);
        // And back out again the other way.
        let mut seen = vec![];
        for _ in 0..6 {
            input.handle_key(&pressed(crate::event::Key::Right));
            seen.push(state(&input).1);
        }
        assert_eq!(seen, vec![1, 2, 4, 6, 7, 8]);
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
}
