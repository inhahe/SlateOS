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
    FlexAlign, FlexDirection, FlexItem, FlexJustify, FlexLayout,
    LayoutBox, Size, SizeConstraint, flex_layout,
};
use crate::render::{FontWeightHint, RenderCommand, RenderTree};
use crate::style::{Borders, CornerRadii, Edges, FontWeight, Style};

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
        cursor_pos: usize,
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
    Image { image_id: u64, width: f32, height: f32 },
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
                cursor_pos: value.len(),
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

    /// Compute intrinsic content size for this widget.
    pub fn intrinsic_size(&self) -> Size {
        match &self.kind {
            WidgetKind::Label { text } => {
                // Approximate text size (proper measurement needs font metrics)
                let char_width = self.style.font_size * 0.6;
                let width = text.len() as f32 * char_width;
                let height = self.style.font_size * self.style.line_height;
                Size::new(
                    width + self.style.padding.horizontal(),
                    height + self.style.padding.vertical(),
                )
            }
            WidgetKind::Button { text, .. } => {
                let char_width = self.style.font_size * 0.6;
                let width = text.len() as f32 * char_width;
                let height = self.style.font_size * self.style.line_height;
                Size::new(
                    width + self.style.padding.horizontal(),
                    height + self.style.padding.vertical(),
                )
            }
            WidgetKind::TextInput { .. } => {
                Size::new(
                    self.style.min_width.unwrap_or(120.0),
                    self.style.min_height.unwrap_or(28.0),
                )
            }
            WidgetKind::Checkbox { label, .. } => {
                let char_width = self.style.font_size * 0.6;
                let checkbox_size = self.style.font_size;
                let width = checkbox_size + 8.0 + label.len() as f32 * char_width;
                let height = self.style.font_size * self.style.line_height;
                Size::new(
                    width + self.style.padding.horizontal(),
                    height + self.style.padding.vertical(),
                )
            }
            WidgetKind::ProgressBar { .. } => {
                Size::new(
                    self.style.min_width.unwrap_or(200.0),
                    self.style.min_height.unwrap_or(20.0),
                )
            }
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
            && !self.children.is_empty() {
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
                    let max_x = layouts
                        .iter()
                        .map(|l| l.x + l.width)
                        .fold(0.0f32, f32::max);
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
            WidgetKind::TextInput {
                value, cursor_pos, ..
            } => {
                if let Some(ch) = key.text {
                    value.insert(*cursor_pos, ch);
                    *cursor_pos += ch.len_utf8();
                    return EventResult::Consumed;
                }
                match key.key {
                    crate::event::Key::Backspace => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                            value.remove(*cursor_pos);
                        }
                        EventResult::Consumed
                    }
                    crate::event::Key::Left => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                        }
                        EventResult::Consumed
                    }
                    crate::event::Key::Right => {
                        if *cursor_pos < value.len() {
                            *cursor_pos += 1;
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
}
