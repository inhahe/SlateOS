#![allow(dead_code)]
//! Modal and non-modal dialog widgets.
//!
//! Provides comprehensive dialog infrastructure including:
//! - `ModalOverlay` — semi-transparent backdrop that blocks parent input
//! - `AlertDialog` — simple message dialogs (info, warning, error, confirm, yes/no)
//! - `InputDialog` — text input with validation
//! - `ProgressDialog` — progress feedback (determinate and indeterminate)
//! - `NonModalDialog` — floating draggable dialog windows
//! - `DialogResult` — return values from dialog interactions
//!
//! All dialogs use a Catppuccin Mocha dark theme and render to `RenderTree`.

use crate::color::Color;
#[allow(unused_imports)]
use crate::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crate::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use crate::style::CornerRadii;
use crate::text::TextCursor;

// --- Catppuccin Mocha palette ---

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_CRUST: Color = Color::from_hex(0x11111B);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COLOR_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COLOR_OVERLAY1: Color = Color::from_hex(0x7F849C);
const COLOR_LAVENDER: Color = Color::from_hex(0xB4BEFE);

// Overlay scrim color (semi-transparent black)
const COLOR_SCRIM: Color = Color::rgba(0, 0, 0, 160);

// --- Layout constants ---

const DIALOG_MIN_WIDTH: f32 = 320.0;
const DIALOG_MAX_WIDTH: f32 = 600.0;
/// Fixed outer width of an [`InputDialog`], which does not auto-size.
const INPUT_DIALOG_WIDTH: f32 = DIALOG_MIN_WIDTH + 80.0;
const DIALOG_MIN_HEIGHT: f32 = 160.0;
const DIALOG_MAX_HEIGHT: f32 = 500.0;
const DIALOG_CORNER_RADIUS: f32 = 12.0;
const TITLE_BAR_HEIGHT: f32 = 44.0;
const BUTTON_HEIGHT: f32 = 34.0;
const BUTTON_MIN_WIDTH: f32 = 80.0;
const BUTTON_PADDING_H: f32 = 16.0;
const BUTTON_SPACING: f32 = 8.0;
const BUTTON_CORNER_RADIUS: f32 = 6.0;
const CONTENT_PADDING: f32 = 24.0;
const ICON_SIZE: f32 = 40.0;
const ICON_PADDING: f32 = 16.0;
const INPUT_HEIGHT: f32 = 36.0;
const INPUT_CORNER_RADIUS: f32 = 6.0;
const PROGRESS_BAR_HEIGHT: f32 = 8.0;
const PROGRESS_BAR_RADIUS: f32 = 4.0;
const FONT_SIZE: f32 = 14.0;
const FONT_SIZE_TITLE: f32 = 16.0;
const FONT_SIZE_SMALL: f32 = 12.0;
/// Baseline-to-baseline spacing of a wrapped dialog message, in pixels.
///
/// Named because the height calculation and the per-line draw both depend on
/// it; as two separate literals they could disagree and clip the last line.
const MESSAGE_LINE_HEIGHT: f32 = 20.0;
const SHADOW_BLUR: f32 = 24.0;
const SHADOW_OFFSET_Y: f32 = 8.0;
const SHADOW_COLOR: Color = Color::rgba(0, 0, 0, 100);
const CLOSE_BUTTON_SIZE: f32 = 28.0;

// --- DialogResult ---

/// Result value returned by dialog interactions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogResult {
    /// User pressed OK / confirmed.
    Ok,
    /// User pressed Cancel.
    Cancel,
    /// User pressed Yes.
    Yes,
    /// User pressed No.
    No,
    /// User provided text input (from InputDialog).
    Text(String),
    /// Dialog was dismissed (clicked outside or pressed Escape).
    Dismissed,
}

// --- Button configuration ---

/// Identifier for standard dialog buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButton {
    Ok,
    Cancel,
    Yes,
    No,
}

impl DialogButton {
    /// Display label for this button.
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Cancel => "Cancel",
            Self::Yes => "Yes",
            Self::No => "No",
        }
    }

    /// Map button to its corresponding DialogResult.
    fn to_result(self) -> DialogResult {
        match self {
            Self::Ok => DialogResult::Ok,
            Self::Cancel => DialogResult::Cancel,
            Self::Yes => DialogResult::Yes,
            Self::No => DialogResult::No,
        }
    }

    /// Whether this is a "primary" (accent-colored) button.
    fn is_primary(self) -> bool {
        matches!(self, Self::Ok | Self::Yes)
    }
}

/// Configuration for the set of buttons in a dialog.
#[derive(Clone, Debug)]
pub struct ButtonSet {
    buttons: Vec<DialogButton>,
}

impl ButtonSet {
    /// Single OK button.
    pub fn ok() -> Self {
        Self {
            buttons: vec![DialogButton::Ok],
        }
    }

    /// OK and Cancel buttons.
    pub fn ok_cancel() -> Self {
        Self {
            buttons: vec![DialogButton::Ok, DialogButton::Cancel],
        }
    }

    /// Yes and No buttons.
    pub fn yes_no() -> Self {
        Self {
            buttons: vec![DialogButton::Yes, DialogButton::No],
        }
    }

    /// Yes, No, and Cancel buttons.
    pub fn yes_no_cancel() -> Self {
        Self {
            buttons: vec![DialogButton::Yes, DialogButton::No, DialogButton::Cancel],
        }
    }

    /// Custom button set.
    pub fn custom(buttons: Vec<DialogButton>) -> Self {
        Self { buttons }
    }

    /// Number of buttons.
    pub fn len(&self) -> usize {
        self.buttons.len()
    }

    /// Whether the button set is empty.
    pub fn is_empty(&self) -> bool {
        self.buttons.is_empty()
    }
}

// --- Icon type for alert dialogs ---

/// Icon displayed in alert dialogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogIcon {
    /// Informational (i) circle.
    Info,
    /// Warning triangle (!).
    Warning,
    /// Error (X) circle.
    Error,
    /// No icon.
    None,
}

impl DialogIcon {
    /// Glyph character representing this icon.
    fn glyph(self) -> Option<&'static str> {
        match self {
            Self::Info => Some("i"),
            Self::Warning => Some("!"),
            Self::Error => Some("X"),
            Self::None => None,
        }
    }

    /// Color for the icon glyph.
    fn color(self) -> Color {
        match self {
            Self::Info => COLOR_BLUE,
            Self::Warning => COLOR_YELLOW,
            Self::Error => COLOR_RED,
            Self::None => Color::TRANSPARENT,
        }
    }

    /// Background color for the icon circle.
    fn bg_color(self) -> Color {
        match self {
            Self::Info => Color::rgba(137, 180, 250, 30),
            Self::Warning => Color::rgba(249, 226, 175, 30),
            Self::Error => Color::rgba(243, 139, 168, 30),
            Self::None => Color::TRANSPARENT,
        }
    }
}

// --- ModalOverlay ---

/// Semi-transparent dark overlay that covers the parent area and blocks input.
///
/// Used as the backdrop for modal dialogs. Supports:
/// - Configurable click-outside-to-dismiss behavior
/// - Escape key to close (configurable)
/// - Fade-in/out animation state via opacity transitions
#[derive(Clone, Debug)]
pub struct ModalOverlay {
    /// Whether the overlay is currently active/visible.
    pub active: bool,
    /// Current opacity (0.0 = fully transparent, 1.0 = fully opaque).
    pub opacity: f32,
    /// Target opacity for animation.
    target_opacity: f32,
    /// Animation speed (opacity change per millisecond).
    fade_speed: f32,
    /// Whether clicking outside the dialog content dismisses it.
    pub dismiss_on_click_outside: bool,
    /// Whether pressing Escape dismisses the overlay.
    pub dismiss_on_escape: bool,
    /// The area occupied by the dialog content (clicks inside are forwarded).
    content_rect: (f32, f32, f32, f32),
}

impl ModalOverlay {
    /// Create a new modal overlay with default settings.
    pub fn new() -> Self {
        Self {
            active: false,
            opacity: 0.0,
            target_opacity: 0.0,
            fade_speed: 0.004, // Full fade in ~250ms
            dismiss_on_click_outside: true,
            dismiss_on_escape: true,
            content_rect: (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Show the overlay (begin fade-in).
    pub fn show(&mut self) {
        self.active = true;
        self.target_opacity = 1.0;
    }

    /// Hide the overlay (begin fade-out).
    pub fn hide(&mut self) {
        self.target_opacity = 0.0;
    }

    /// Whether the overlay has fully faded out and can be deactivated.
    pub fn is_fully_hidden(&self) -> bool {
        self.opacity <= 0.0 && self.target_opacity <= 0.0
    }

    /// Whether the overlay is fully visible.
    pub fn is_fully_visible(&self) -> bool {
        self.opacity >= 1.0
    }

    /// Set the content rectangle (the area that the dialog occupies).
    pub fn set_content_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.content_rect = (x, y, width, height);
    }

    /// Update animation state. Call each frame with elapsed milliseconds.
    pub fn tick(&mut self, elapsed_ms: u64) {
        let delta = self.fade_speed * elapsed_ms as f32;
        if self.opacity < self.target_opacity {
            self.opacity = (self.opacity + delta).min(self.target_opacity);
        } else if self.opacity > self.target_opacity {
            self.opacity = (self.opacity - delta).max(self.target_opacity);
        }

        // Deactivate once fully faded out.
        if self.opacity <= 0.0 && self.target_opacity <= 0.0 {
            self.active = false;
        }
    }

    /// Handle a mouse event. Returns `Dismissed` if click-outside triggered.
    pub fn handle_mouse(&self, event: &MouseEvent) -> Option<DialogResult> {
        if !self.active {
            return None;
        }

        if let MouseEventKind::Press(MouseButton::Left) = event.kind
            && self.dismiss_on_click_outside
            && !self.point_in_content(event.x, event.y)
        {
            return Some(DialogResult::Dismissed);
        }
        None
    }

    /// Handle a key event. Returns `Dismissed` if Escape triggered.
    pub fn handle_key(&self, event: &KeyEvent) -> Option<DialogResult> {
        if !self.active || !event.pressed {
            return None;
        }
        if self.dismiss_on_escape && event.key == Key::Escape {
            return Some(DialogResult::Dismissed);
        }
        None
    }

    /// Render the overlay scrim.
    pub fn render(&self, width: f32, height: f32, tree: &mut RenderTree) {
        if self.opacity <= 0.0 {
            return;
        }
        let alpha = (160.0 * self.opacity) as u8;
        let scrim_color = Color::rgba(0, 0, 0, alpha);
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: scrim_color,
            corner_radii: CornerRadii::ZERO,
        });
    }

    /// Check if a point is inside the content rectangle.
    fn point_in_content(&self, x: f32, y: f32) -> bool {
        let (cx, cy, cw, ch) = self.content_rect;
        x >= cx && x <= cx + cw && y >= cy && y <= cy + ch
    }
}

impl Default for ModalOverlay {
    fn default() -> Self {
        Self::new()
    }
}

// --- AlertDialog ---

/// Simple message dialog for displaying alerts, confirmations, and choices.
///
/// Provides factory methods for common patterns:
/// - `info` — informational message with OK button
/// - `warning` — warning message with OK button
/// - `error` — error message with OK button
/// - `confirm` — confirmation with OK + Cancel
/// - `yes_no` — choice with Yes + No
/// - `yes_no_cancel` — choice with Yes + No + Cancel
#[derive(Clone, Debug)]
pub struct AlertDialog {
    title: String,
    message: String,
    icon: DialogIcon,
    buttons: ButtonSet,
    focused_button: usize,
    result: Option<DialogResult>,
    overlay: ModalOverlay,
    /// Custom width (if set, overrides auto-sizing).
    width: Option<f32>,
}

impl AlertDialog {
    /// Create an informational dialog.
    pub fn info(title: &str, message: &str) -> Self {
        Self::new(title, message, DialogIcon::Info, ButtonSet::ok())
    }

    /// Create a warning dialog.
    pub fn warning(title: &str, message: &str) -> Self {
        Self::new(title, message, DialogIcon::Warning, ButtonSet::ok())
    }

    /// Create an error dialog.
    pub fn error(title: &str, message: &str) -> Self {
        Self::new(title, message, DialogIcon::Error, ButtonSet::ok())
    }

    /// Create a confirmation dialog (OK + Cancel).
    pub fn confirm(title: &str, message: &str) -> Self {
        Self::new(title, message, DialogIcon::Info, ButtonSet::ok_cancel())
    }

    /// Create a Yes/No dialog.
    pub fn yes_no(title: &str, message: &str) -> Self {
        Self::new(title, message, DialogIcon::Info, ButtonSet::yes_no())
    }

    /// Create a Yes/No/Cancel dialog.
    pub fn yes_no_cancel(title: &str, message: &str) -> Self {
        Self::new(title, message, DialogIcon::Info, ButtonSet::yes_no_cancel())
    }

    /// Builder: set custom button set.
    #[must_use]
    pub fn with_buttons(mut self, buttons: ButtonSet) -> Self {
        self.buttons = buttons;
        self
    }

    /// Builder: set custom icon.
    #[must_use]
    pub fn with_icon(mut self, icon: DialogIcon) -> Self {
        self.icon = icon;
        self
    }

    /// Builder: set fixed width.
    #[must_use]
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Builder: configure escape-to-dismiss behavior.
    #[must_use]
    pub fn with_escape_dismiss(mut self, enabled: bool) -> Self {
        self.overlay.dismiss_on_escape = enabled;
        self
    }

    /// Builder: configure click-outside-to-dismiss behavior.
    #[must_use]
    pub fn with_click_outside_dismiss(mut self, enabled: bool) -> Self {
        self.overlay.dismiss_on_click_outside = enabled;
        self
    }

    /// Show the dialog (activate overlay, begin fade-in).
    pub fn show(&mut self) {
        self.result = None;
        self.focused_button = 0;
        self.overlay.show();
    }

    /// Whether the dialog is currently active.
    pub fn is_active(&self) -> bool {
        self.overlay.active
    }

    /// Get the result (if the dialog has been dismissed or a button pressed).
    pub fn result(&self) -> Option<&DialogResult> {
        self.result.as_ref()
    }

    /// The focused button index.
    pub fn focused_button(&self) -> usize {
        self.focused_button
    }

    /// The button set for this dialog.
    pub fn buttons(&self) -> &ButtonSet {
        &self.buttons
    }

    /// Update animation state.
    pub fn tick(&mut self, elapsed_ms: u64) {
        self.overlay.tick(elapsed_ms);
    }

    /// Handle an event. Returns EventResult indicating consumption.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.overlay.active {
            return EventResult::Ignored;
        }

        match event {
            Event::Key(key_event) => self.handle_key(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            Event::Tick { elapsed_ms } => {
                self.tick(*elapsed_ms);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a key event.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Consumed;
        }

        // Check overlay-level escape handling.
        if let Some(result) = self.overlay.handle_key(event) {
            self.result = Some(result);
            self.overlay.hide();
            return EventResult::Consumed;
        }

        match event.key {
            Key::Tab => {
                // Cycle focus through buttons.
                // The fifth copy of the wrap `step` was extracted to own; see
                // its module doc. Both ends of the list are inside the helper,
                // so an empty button row needs no guard here either.
                self.focused_button = if event.modifiers.shift {
                    crate::step::wrapping_before(self.buttons.len(), self.focused_button)
                } else {
                    crate::step::wrapping_after(self.buttons.len(), self.focused_button)
                };
                EventResult::Consumed
            }
            Key::Enter | Key::Space => {
                // Activate focused button.
                if let Some(btn) = self.buttons.buttons.get(self.focused_button) {
                    self.result = Some(btn.to_result());
                    self.overlay.hide();
                }
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    /// Handle a mouse event.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        // Check overlay dismiss.
        if let Some(result) = self.overlay.handle_mouse(event) {
            self.result = Some(result);
            self.overlay.hide();
            return EventResult::Consumed;
        }

        // Check button clicks.
        if let MouseEventKind::Press(MouseButton::Left) = event.kind {
            let layout = self.compute_layout(800.0, 600.0);
            for (i, btn_rect) in layout.button_rects.iter().enumerate() {
                if point_in_rect(
                    event.x, event.y, btn_rect.0, btn_rect.1, btn_rect.2, btn_rect.3,
                ) && let Some(btn) = self.buttons.buttons.get(i)
                {
                    self.result = Some(btn.to_result());
                    self.overlay.hide();
                    return EventResult::Consumed;
                }
            }
        }

        EventResult::Consumed
    }

    /// Render the dialog within the given parent area.
    pub fn render(&self, parent_width: f32, parent_height: f32, tree: &mut RenderTree) {
        if !self.overlay.active && self.overlay.opacity <= 0.0 {
            return;
        }

        // Render overlay scrim.
        self.overlay.render(parent_width, parent_height, tree);

        let layout = self.compute_layout(parent_width, parent_height);

        // Box shadow.
        tree.push(RenderCommand::BoxShadow {
            x: layout.x,
            y: layout.y,
            width: layout.width,
            height: layout.height,
            offset_x: 0.0,
            offset_y: SHADOW_OFFSET_Y,
            blur: SHADOW_BLUR,
            spread: 0.0,
            color: SHADOW_COLOR,
            corner_radii: CornerRadii::all(DIALOG_CORNER_RADIUS),
        });

        // Dialog background.
        tree.push(RenderCommand::FillRect {
            x: layout.x,
            y: layout.y,
            width: layout.width,
            height: layout.height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(DIALOG_CORNER_RADIUS),
        });

        // Title bar.
        tree.push(RenderCommand::FillRect {
            x: layout.x,
            y: layout.y,
            width: layout.width,
            height: TITLE_BAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii {
                top_left: DIALOG_CORNER_RADIUS,
                top_right: DIALOG_CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Title text.
        tree.push(RenderCommand::Text {
            x: layout.x + CONTENT_PADDING,
            y: layout.y + (TITLE_BAR_HEIGHT - FONT_SIZE_TITLE) / 2.0,
            text: self.title.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_TITLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(layout.width - CONTENT_PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Content area.
        let content_y = layout.y + TITLE_BAR_HEIGHT + CONTENT_PADDING;
        let mut text_x = layout.x + CONTENT_PADDING;

        // Icon (if any).
        if let Some(glyph) = self.icon.glyph() {
            let icon_x = layout.x + CONTENT_PADDING;
            let icon_y = content_y;

            // Icon background circle.
            tree.push(RenderCommand::FillRect {
                x: icon_x,
                y: icon_y,
                width: ICON_SIZE,
                height: ICON_SIZE,
                color: self.icon.bg_color(),
                corner_radii: CornerRadii::all(ICON_SIZE / 2.0),
            });

            // Icon glyph.
            tree.push(RenderCommand::Text {
                x: icon_x + (ICON_SIZE - FONT_SIZE_TITLE) / 2.0,
                y: icon_y + (ICON_SIZE - FONT_SIZE_TITLE) / 2.0,
                text: glyph.to_string(),
                color: self.icon.color(),
                font_size: FONT_SIZE_TITLE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            text_x = icon_x + ICON_SIZE + ICON_PADDING;
        }

        let buttons_y = layout.y + layout.height - BUTTON_HEIGHT - CONTENT_PADDING;

        // Message text, one command per wrapped line.
        let text_max_width = self.message_max_width();
        let lines = self.message_lines();
        // Centred against the icon while the message is the shorter of the
        // two, then top-aligned once it is taller — so a one-line message
        // still sits level with its icon.
        let block_height = lines.len() as f32 * MESSAGE_LINE_HEIGHT;
        let first_line_y = content_y + (ICON_SIZE - block_height).max(0.0) / 2.0;
        for (n, line) in lines.iter().enumerate() {
            let line_y = first_line_y + n as f32 * MESSAGE_LINE_HEIGHT;
            // The dialog height is clamped at `DIALOG_MAX_HEIGHT`, so a message
            // can be longer than the box it is given. Lines that do not fit are
            // dropped rather than drawn over the button row: text on top of the
            // controls that dismiss the dialog is worse than text not shown.
            if line_y + MESSAGE_LINE_HEIGHT > buttons_y {
                break;
            }
            tree.push(RenderCommand::Text {
                x: text_x,
                y: line_y,
                text: line.clone(),
                color: COLOR_SUBTEXT1,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(text_max_width),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Buttons (bottom-right aligned).
        self.render_buttons(tree, &layout, buttons_y);
    }

    /// Render the button row.
    fn render_buttons(&self, tree: &mut RenderTree, layout: &DialogLayout, y: f32) {
        let total_width: f32 = self
            .buttons
            .buttons
            .iter()
            .map(|_| BUTTON_MIN_WIDTH)
            .sum::<f32>()
            + (self.buttons.len().saturating_sub(1) as f32) * BUTTON_SPACING;
        let start_x = layout.x + layout.width - CONTENT_PADDING - total_width;

        for (i, btn) in self.buttons.buttons.iter().enumerate() {
            let btn_x = start_x + (i as f32) * (BUTTON_MIN_WIDTH + BUTTON_SPACING);
            let is_focused = i == self.focused_button;

            // Button background.
            let bg_color = if btn.is_primary() {
                COLOR_BLUE
            } else {
                COLOR_SURFACE1
            };
            tree.push(RenderCommand::FillRect {
                x: btn_x,
                y,
                width: BUTTON_MIN_WIDTH,
                height: BUTTON_HEIGHT,
                color: bg_color,
                corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS),
            });

            // Focus ring.
            if is_focused {
                tree.push(RenderCommand::StrokeRect {
                    x: btn_x - 2.0,
                    y: y - 2.0,
                    width: BUTTON_MIN_WIDTH + 4.0,
                    height: BUTTON_HEIGHT + 4.0,
                    color: COLOR_LAVENDER,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS + 2.0),
                });
            }

            // Button label.
            let label = btn.label();
            let text_color = if btn.is_primary() {
                COLOR_CRUST
            } else {
                COLOR_TEXT
            };
            tree.push(RenderCommand::Text {
                // Centred on the label's measured width. The flat 7px-per-byte
                // guess this replaces drifted further off-centre the longer the
                // label was, and mis-centred non-ASCII labels badly.
                x: btn_x
                    + (BUTTON_MIN_WIDTH
                        - crate::text::measure(label, FONT_SIZE, FontWeightHint::Bold))
                        / 2.0,
                y: y + (BUTTON_HEIGHT - FONT_SIZE) / 2.0,
                text: label.to_string(),
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    /// The dialog's outer width.
    fn dialog_width(&self) -> f32 {
        self.width
            .unwrap_or(DIALOG_MIN_WIDTH)
            .clamp(DIALOG_MIN_WIDTH, DIALOG_MAX_WIDTH)
    }

    /// Horizontal room the message has, after the padding and any icon.
    fn message_max_width(&self) -> f32 {
        let text_offset = if self.icon.glyph().is_some() {
            CONTENT_PADDING + ICON_SIZE + ICON_PADDING
        } else {
            CONTENT_PADDING
        };
        self.dialog_width() - text_offset - CONTENT_PADDING
    }

    /// The message, broken into the lines it will be drawn as.
    ///
    /// `RenderCommand::Text` does not wrap — the compositor truncates at
    /// `max_width` — so a message longer than one line has to be broken up
    /// here and drawn a line at a time. `compute_height` sizes the dialog from
    /// this same list, so the box is always as tall as the text it holds.
    fn message_lines(&self) -> Vec<String> {
        crate::text::wrap(
            &self.message,
            self.message_max_width(),
            FONT_SIZE,
            FontWeightHint::Regular,
        )
    }

    /// Compute dialog layout (position and size, centered in parent).
    fn compute_layout(&self, parent_width: f32, parent_height: f32) -> DialogLayout {
        let width = self.dialog_width();
        let height = self.compute_height();
        let x = (parent_width - width) / 2.0;
        let y = (parent_height - height) / 2.0;

        // Compute button rects for hit testing.
        let buttons_y = y + height - BUTTON_HEIGHT - CONTENT_PADDING;
        let total_btn_width: f32 = self
            .buttons
            .buttons
            .iter()
            .map(|_| BUTTON_MIN_WIDTH)
            .sum::<f32>()
            + (self.buttons.len().saturating_sub(1) as f32) * BUTTON_SPACING;
        let start_x = x + width - CONTENT_PADDING - total_btn_width;

        let button_rects: Vec<(f32, f32, f32, f32)> = (0..self.buttons.len())
            .map(|i| {
                let bx = start_x + (i as f32) * (BUTTON_MIN_WIDTH + BUTTON_SPACING);
                (bx, buttons_y, BUTTON_MIN_WIDTH, BUTTON_HEIGHT)
            })
            .collect();

        DialogLayout {
            x,
            y,
            width,
            height,
            button_rects,
        }
    }

    /// Compute the height needed for the dialog content.
    fn compute_height(&self) -> f32 {
        // Title bar + content padding + icon/message area + padding + buttons + padding
        //
        // The message area is as tall as the message's own wrapped lines. It
        // used to be a flat three-line guess, which left a band of empty space
        // under a one-line message and clipped anything longer than three.
        let message_height = self.message_lines().len() as f32 * MESSAGE_LINE_HEIGHT;
        let content_height = ICON_SIZE.max(message_height);
        (TITLE_BAR_HEIGHT
            + CONTENT_PADDING
            + content_height
            + CONTENT_PADDING
            + BUTTON_HEIGHT
            + CONTENT_PADDING)
            .clamp(DIALOG_MIN_HEIGHT, DIALOG_MAX_HEIGHT)
    }

    fn new(title: &str, message: &str, icon: DialogIcon, buttons: ButtonSet) -> Self {
        let mut overlay = ModalOverlay::new();
        overlay.dismiss_on_escape = true;
        overlay.dismiss_on_click_outside = true;

        Self {
            title: title.to_string(),
            message: message.to_string(),
            icon,
            buttons,
            focused_button: 0,
            result: None,
            overlay,
            width: None,
        }
    }
}

// --- InputDialog ---

/// Text input dialog for prompting the user for text.
///
/// Supports:
/// - Single-line text input with placeholder
/// - Optional validation function (displays error below input)
/// - Password mode (displays masked characters)
/// - OK/Cancel button pair
#[derive(Clone, Debug)]
pub struct InputDialog {
    title: String,
    message: String,
    placeholder: String,
    input_text: String,
    /// The caret: a byte offset *and* which side of a direction boundary it is
    /// on. Not a bare offset — where a left-to-right stretch meets a
    /// right-to-left one, a single offset names two different places on the
    /// screen, and a caret rebuilt from the offset alone steps over a
    /// right-to-left word rather than through it.
    cursor: TextCursor,
    /// Where a selection started, as a byte offset into `input_text`, or `None`
    /// if nothing is selected. The other end of the selection is the caret —
    /// deliberately, rather than a stored `(start, end)` pair, which is a third
    /// number that can disagree with the other two. A plain offset and not a
    /// `TextCursor`: a selection is a range of *text*, so it has no side of a
    /// direction boundary to be on. See `design-decisions.md` §546.
    selection_anchor: Option<usize>,
    password_mode: bool,
    validation_error: Option<String>,
    /// Validation function stored as a flag; actual validation is done via `validate()`.
    has_validator: bool,
    buttons: ButtonSet,
    focused_element: InputFocus,
    result: Option<DialogResult>,
    overlay: ModalOverlay,
}

/// Which element has focus in the input dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputFocus {
    TextField,
    OkButton,
    CancelButton,
}

impl InputDialog {
    /// Create a new input dialog.
    pub fn prompt(title: &str, message: &str, placeholder: &str) -> Self {
        let mut overlay = ModalOverlay::new();
        overlay.dismiss_on_escape = true;
        overlay.dismiss_on_click_outside = false;

        Self {
            title: title.to_string(),
            message: message.to_string(),
            placeholder: placeholder.to_string(),
            input_text: String::new(),
            cursor: TextCursor::default(),
            selection_anchor: None,
            password_mode: false,
            validation_error: None,
            has_validator: false,
            buttons: ButtonSet::ok_cancel(),
            focused_element: InputFocus::TextField,
            result: None,
            overlay,
        }
    }

    /// Builder: enable password mode.
    #[must_use]
    pub fn with_password_mode(mut self, enabled: bool) -> Self {
        self.password_mode = enabled;
        self
    }

    /// Builder: mark that this dialog has a validator.
    /// Callers should use `validate()` to check input before accepting.
    #[must_use]
    pub fn with_validation(mut self) -> Self {
        self.has_validator = true;
        self
    }

    /// Builder: set initial text.
    #[must_use]
    pub fn with_initial_text(mut self, text: &str) -> Self {
        self.input_text = text.to_string();
        self.cursor = TextCursor::from(text.len());
        self.selection_anchor = None;
        self
    }

    /// Show the dialog.
    pub fn show(&mut self) {
        self.result = None;
        self.validation_error = None;
        self.focused_element = InputFocus::TextField;
        self.selection_anchor = None;
        self.overlay.show();
    }

    /// Whether the dialog is active.
    pub fn is_active(&self) -> bool {
        self.overlay.active
    }

    /// Get the result.
    pub fn result(&self) -> Option<&DialogResult> {
        self.result.as_ref()
    }

    /// Get the current input text.
    pub fn input_text(&self) -> &str {
        &self.input_text
    }

    /// Set the input text programmatically.
    pub fn set_input_text(&mut self, text: &str) {
        self.input_text = text.to_string();
        self.cursor = TextCursor::from(text.len());
        // The anchor names offsets in the string that has just been replaced,
        // so it can be past the end of the new one.
        self.selection_anchor = None;
    }

    /// Set a validation error message (shown below the input field).
    pub fn set_validation_error(&mut self, error: Option<&str>) {
        self.validation_error = error.map(|s| s.to_string());
    }

    /// Check whether the current input has a validation error set.
    pub fn has_validation_error(&self) -> bool {
        self.validation_error.is_some()
    }

    /// Update animation state.
    pub fn tick(&mut self, elapsed_ms: u64) {
        self.overlay.tick(elapsed_ms);
    }

    /// Handle an event.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.overlay.active {
            return EventResult::Ignored;
        }

        match event {
            Event::Key(key_event) => self.handle_key(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            Event::Tick { elapsed_ms } => {
                self.tick(*elapsed_ms);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a key event.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Consumed;
        }

        // Escape handling.
        if event.key == Key::Escape {
            self.result = Some(DialogResult::Cancel);
            self.overlay.hide();
            return EventResult::Consumed;
        }

        match self.focused_element {
            InputFocus::TextField => {
                self.handle_text_input(event);
            }
            InputFocus::OkButton | InputFocus::CancelButton => match event.key {
                Key::Enter | Key::Space => {
                    if self.focused_element == InputFocus::OkButton {
                        self.try_accept();
                    } else {
                        self.result = Some(DialogResult::Cancel);
                        self.overlay.hide();
                    }
                }
                Key::Tab => {
                    self.cycle_focus(event.modifiers.shift);
                }
                _ => {}
            },
        }

        EventResult::Consumed
    }

    /// Handle text input when the text field is focused.
    fn handle_text_input(&mut self, event: &KeyEvent) -> EventResult {
        match event.key {
            Key::Tab => {
                self.cycle_focus(event.modifiers.shift);
            }
            Key::Enter => {
                self.try_accept();
            }
            // The caret's offset is a *byte* offset: `String::insert` and
            // `String::remove` index by bytes, and both panic outright on an
            // offset that is not a character boundary. So every edit below moves
            // the caret with `TextCursor::prev_in`/`next_in`, which answer with
            // an offset the text itself named. Spelling that out here as "slice
            // the text before me, take the last character, subtract its UTF-8
            // width" is the same answer reached by a route with two hazards on
            // it — the slice panics on a cursor that has drifted off a boundary,
            // and the subtraction is safe only because of a test in an earlier
            // statement — which is why the toolkit owns the step and this
            // dialog no longer computes one.
            //
            // Deleting is a *logical* edit and stays logical: backspace removes
            // the character before this one in the string, which is what a
            // reader of that script means, even where that character is drawn
            // on the right. The arrows below are the opposite — they are about
            // the screen — and that asymmetry is deliberate.
            //
            // Every deleting arm below asks the selection first. A key that
            // would remove one character removes the whole selection instead
            // when there is one, which is what "selected" means everywhere
            // else; a field that deleted one character out of a highlighted run
            // would leave the user staring at a highlight that no longer
            // matches the text under it.
            Key::Backspace => {
                if crate::textedit::delete_selection(
                    &mut self.input_text,
                    &mut self.cursor,
                    &mut self.selection_anchor,
                ) {
                    self.validation_error = None;
                } else if let Some(prev) = self.cursor.prev_in(&self.input_text) {
                    self.input_text.remove(prev.byte());
                    self.cursor = prev;
                    self.validation_error = None;
                }
            }
            Key::Delete => {
                if crate::textedit::delete_selection(
                    &mut self.input_text,
                    &mut self.cursor,
                    &mut self.selection_anchor,
                ) {
                    self.validation_error = None;
                } else if self.cursor.byte() < self.input_text.len() {
                    // `remove` takes the whole character at the offset, so no
                    // width arithmetic is needed here — only the guard that the
                    // offset is inside the string.
                    self.input_text.remove(self.cursor.byte());
                    self.validation_error = None;
                }
            }
            // Each arrow plants the anchor before it moves, if Shift is down,
            // and drops it if not: a bare arrow on a selection means "put the
            // caret here and forget the selection", not "extend it silently".
            Key::Left => {
                self.begin_or_end_selection(event.modifiers.shift);
                self.move_caret(false);
            }
            Key::Right => {
                self.begin_or_end_selection(event.modifiers.shift);
                self.move_caret(true);
            }
            Key::Home => {
                self.begin_or_end_selection(event.modifiers.shift);
                self.cursor = TextCursor::default();
            }
            Key::End => {
                self.begin_or_end_selection(event.modifiers.shift);
                self.cursor = TextCursor::from(self.input_text.len());
            }
            _ => {
                if let Some(ch) = event.text
                    && !ch.is_control()
                {
                    // Typing over a selection replaces it, so the character
                    // lands where the selection was rather than beside it.
                    crate::textedit::delete_selection(
                        &mut self.input_text,
                        &mut self.cursor,
                        &mut self.selection_anchor,
                    );
                    self.input_text.insert(self.cursor.byte(), ch);
                    // The boundary after the caret, once the text has the new
                    // character in it, is where the typing left the caret.
                    self.cursor = self
                        .cursor
                        .next_in(&self.input_text)
                        .unwrap_or_else(|| TextCursor::from(self.input_text.len()));
                    self.validation_error = None;
                }
            }
        }
        EventResult::Consumed
    }

    /// Handle mouse event.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        if let Some(result) = self.overlay.handle_mouse(event) {
            self.result = Some(result);
            self.overlay.hide();
            return EventResult::Consumed;
        }
        EventResult::Consumed
    }

    /// Step the caret one position left or right **on the screen**.
    ///
    /// On a line that mixes directions that is not the same as one character
    /// earlier or later in the string: the caret walks *through* a
    /// right-to-left word rather than jumping across it. macOS, GTK and Qt move
    /// logically and Windows moves visually; the operator chose visual, and the
    /// reasoning is `design-decisions.md` §541.
    ///
    /// **A password field is the documented exception and stays logical.** What
    /// it draws is a row of asterisks, so its drawn order is its string order
    /// whatever was typed. Moving by the layout of the *hidden* text would
    /// scatter the caret among identical marks with nothing on screen to
    /// explain the jumps — and would leak the shape of the secret to anyone
    /// watching the caret, which is the one thing the masking exists to
    /// prevent.
    fn move_caret(&mut self, right: bool) {
        // A whole character at a time, never a byte: `String::remove` and
        // `insert` panic on an offset inside one. Both paths below return
        // offsets the text named, so there is no width to add or subtract.
        //
        // Note the visual arms assign the returned cursor whole rather than
        // just its byte. Where two directions meet, one byte offset names two
        // screen positions; the affinity carried in `TextCursor` is what tells
        // them apart, and dropping it would skip a whole word per keypress.
        let stepped = if self.password_mode {
            if right {
                self.cursor.next_in(&self.input_text)
            } else {
                self.cursor.prev_in(&self.input_text)
            }
        } else if right {
            crate::text::caret_right(
                &self.input_text,
                self.cursor,
                FONT_SIZE,
                FontWeightHint::Regular,
            )
        } else {
            crate::text::caret_left(
                &self.input_text,
                self.cursor,
                FONT_SIZE,
                FontWeightHint::Regular,
            )
        };
        if let Some(next) = stepped {
            self.cursor = next;
        }
    }

    /// Plant or drop the selection anchor for an arrow key that is about to
    /// move the caret.
    fn begin_or_end_selection(&mut self, shift: bool) {
        crate::textedit::begin_or_end_selection(shift, self.cursor, &mut self.selection_anchor);
    }

    /// The selected range as offsets into the *drawn* string.
    ///
    /// For an ordinary field that is the range as stored. For a password field
    /// the drawn string is a row of one-byte marks, one per character, so both
    /// ends have to be converted from byte offsets in the secret to mark counts
    /// — otherwise a selection over an accented letter would highlight two
    /// marks for one character, and a selection over an emoji four, which is
    /// exactly the byte-count leak the masking exists to prevent.
    fn drawn_offset(&self, byte: usize) -> usize {
        if !self.password_mode {
            return byte;
        }
        self.input_text.get(..byte).map_or_else(
            || self.input_text.chars().count(),
            |head| head.chars().count(),
        )
    }

    /// Cycle focus between text field, OK, and Cancel.
    fn cycle_focus(&mut self, reverse: bool) {
        self.focused_element = if reverse {
            match self.focused_element {
                InputFocus::TextField => InputFocus::CancelButton,
                InputFocus::OkButton => InputFocus::TextField,
                InputFocus::CancelButton => InputFocus::OkButton,
            }
        } else {
            match self.focused_element {
                InputFocus::TextField => InputFocus::OkButton,
                InputFocus::OkButton => InputFocus::CancelButton,
                InputFocus::CancelButton => InputFocus::TextField,
            }
        };
    }

    /// Try to accept the input (set result to Text if no validation error).
    fn try_accept(&mut self) {
        if self.validation_error.is_none() {
            self.result = Some(DialogResult::Text(self.input_text.clone()));
            self.overlay.hide();
        }
    }

    /// The prompt, broken into the lines it will be drawn as.
    ///
    /// `RenderCommand::Text` clips at `max_width` rather than wrapping, so a
    /// prompt longer than one line has to be broken up here. `render` derives
    /// both the dialog height and the input field's position from this list, so
    /// the field cannot land on top of the prompt above it.
    fn message_lines(&self) -> Vec<String> {
        crate::text::wrap(
            &self.message,
            INPUT_DIALOG_WIDTH - CONTENT_PADDING * 2.0,
            FONT_SIZE,
            FontWeightHint::Regular,
        )
    }

    /// Render the input dialog.
    pub fn render(&self, parent_width: f32, parent_height: f32, tree: &mut RenderTree) {
        if !self.overlay.active && self.overlay.opacity <= 0.0 {
            return;
        }

        self.overlay.render(parent_width, parent_height, tree);

        let width = INPUT_DIALOG_WIDTH;
        let has_error = self.validation_error.is_some();
        // The prompt is wrapped rather than clipped, so the room reserved for
        // it is the height of its own lines. A flat one-line allowance used to
        // push the input field up over the second line of any longer prompt.
        let message_lines = self.message_lines();
        let message_height = message_lines.len() as f32 * MESSAGE_LINE_HEIGHT;
        let height = TITLE_BAR_HEIGHT
            + CONTENT_PADDING
            + message_height
            + 12.0
            + INPUT_HEIGHT
            + (if has_error {
                FONT_SIZE_SMALL + 8.0
            } else {
                0.0
            })
            + CONTENT_PADDING
            + BUTTON_HEIGHT
            + CONTENT_PADDING;
        let x = (parent_width - width) / 2.0;
        let y = (parent_height - height) / 2.0;

        // Shadow.
        tree.push(RenderCommand::BoxShadow {
            x,
            y,
            width,
            height,
            offset_x: 0.0,
            offset_y: SHADOW_OFFSET_Y,
            blur: SHADOW_BLUR,
            spread: 0.0,
            color: SHADOW_COLOR,
            corner_radii: CornerRadii::all(DIALOG_CORNER_RADIUS),
        });

        // Background.
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(DIALOG_CORNER_RADIUS),
        });

        // Title bar.
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: TITLE_BAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii {
                top_left: DIALOG_CORNER_RADIUS,
                top_right: DIALOG_CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Title text.
        tree.push(RenderCommand::Text {
            x: x + CONTENT_PADDING,
            y: y + (TITLE_BAR_HEIGHT - FONT_SIZE_TITLE) / 2.0,
            text: self.title.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_TITLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - CONTENT_PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Message, one command per wrapped line.
        let mut content_y = y + TITLE_BAR_HEIGHT + CONTENT_PADDING;
        for (n, line) in message_lines.iter().enumerate() {
            tree.push(RenderCommand::Text {
                x: x + CONTENT_PADDING,
                y: content_y + n as f32 * MESSAGE_LINE_HEIGHT,
                text: line.clone(),
                color: COLOR_SUBTEXT1,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - CONTENT_PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        content_y += message_height + 12.0;

        // Input field.
        let input_width = width - CONTENT_PADDING * 2.0;
        let input_border_color = if self.focused_element == InputFocus::TextField {
            COLOR_BLUE
        } else if self.validation_error.is_some() {
            COLOR_RED
        } else {
            COLOR_SURFACE2
        };

        tree.push(RenderCommand::FillRect {
            x: x + CONTENT_PADDING,
            y: content_y,
            width: input_width,
            height: INPUT_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(INPUT_CORNER_RADIUS),
        });

        tree.push(RenderCommand::StrokeRect {
            x: x + CONTENT_PADDING,
            y: content_y,
            width: input_width,
            height: INPUT_HEIGHT,
            color: input_border_color,
            line_width: 1.5,
            corner_radii: CornerRadii::all(INPUT_CORNER_RADIUS),
        });

        // Input text or placeholder.
        //
        // Geometry shared by both paths, so the caret in an empty field sits
        // exactly where the first character will be drawn.
        let text_x = x + CONTENT_PADDING + 10.0;
        let text_y = content_y + (INPUT_HEIGHT - FONT_SIZE) / 2.0;
        let text_avail = input_width - 20.0;
        let field_focused = self.focused_element == InputFocus::TextField;

        let display_text = if self.input_text.is_empty() {
            self.placeholder.clone()
        } else if self.password_mode {
            // One mark per *caret stop*, not per byte. `len()` is the UTF-8
            // byte count, so a password with any non-ASCII character in it drew
            // more asterisks than it has characters — two for an accented
            // letter, four for an emoji. That is wrong twice over: the row of
            // marks no longer lines up with the positions the caret can occupy
            // (`caret_offsets` walks characters), and the width of the row
            // leaks how many bytes the secret encodes to, which for a password
            // typed in a non-Latin script is most of what an observer wants.
            "*".repeat(self.input_text.chars().count())
        } else {
            self.input_text.clone()
        };
        if self.input_text.is_empty() {
            // The placeholder is not editable text: it has no caret positions
            // in it and nothing can be selected in it, so it is drawn plainly
            // and the caret — if the field has the focus — goes at the left,
            // where the first character the user types will appear. A focused
            // empty field with no caret is one the user cannot tell is ready.
            tree.push(RenderCommand::Text {
                x: text_x,
                y: text_y,
                text: display_text,
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(text_avail),
                overflow: TextOverflow::Ellipsis,
            });
            if field_focused {
                crate::textedit::push_caret(tree, text_x, text_y, FONT_SIZE, COLOR_TEXT);
            }
        } else {
            // Offsets into the *drawn* string: for a password field that is the
            // row of marks, not the secret. See `drawn_offset`.
            let drawn_cursor = TextCursor::from(self.drawn_offset(self.cursor.byte()));
            let drawn_anchor = self.selection_anchor.map(|a| self.drawn_offset(a));
            crate::textedit::draw(
                tree,
                &crate::textedit::SingleLine {
                    text: &display_text,
                    cursor: drawn_cursor,
                    selection_anchor: drawn_anchor,
                    focused: field_focused,
                    x: text_x,
                    y: text_y,
                    width: text_avail,
                    line_height: FONT_SIZE,
                    font_size: FONT_SIZE,
                    weight: FontWeightHint::Regular,
                    color: COLOR_TEXT,
                },
            );
        }

        content_y += INPUT_HEIGHT;

        // Validation error.
        if let Some(ref error) = self.validation_error {
            content_y += 4.0;
            tree.push(RenderCommand::Text {
                x: x + CONTENT_PADDING,
                y: content_y,
                text: error.clone(),
                color: COLOR_RED,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_width),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Buttons.
        let buttons_y = y + height - BUTTON_HEIGHT - CONTENT_PADDING;
        let btn_start_x = x + width - CONTENT_PADDING - BUTTON_MIN_WIDTH * 2.0 - BUTTON_SPACING;

        // OK button.
        let ok_focused = self.focused_element == InputFocus::OkButton;
        tree.push(RenderCommand::FillRect {
            x: btn_start_x,
            y: buttons_y,
            width: BUTTON_MIN_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_BLUE,
            corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS),
        });
        if ok_focused {
            tree.push(RenderCommand::StrokeRect {
                x: btn_start_x - 2.0,
                y: buttons_y - 2.0,
                width: BUTTON_MIN_WIDTH + 4.0,
                height: BUTTON_HEIGHT + 4.0,
                color: COLOR_LAVENDER,
                line_width: 2.0,
                corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS + 2.0),
            });
        }
        tree.push(RenderCommand::Text {
            x: btn_start_x + (BUTTON_MIN_WIDTH - 18.0) / 2.0,
            y: buttons_y + (BUTTON_HEIGHT - FONT_SIZE) / 2.0,
            text: String::from("OK"),
            color: COLOR_CRUST,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Cancel button.
        let cancel_x = btn_start_x + BUTTON_MIN_WIDTH + BUTTON_SPACING;
        let cancel_focused = self.focused_element == InputFocus::CancelButton;
        tree.push(RenderCommand::FillRect {
            x: cancel_x,
            y: buttons_y,
            width: BUTTON_MIN_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS),
        });
        if cancel_focused {
            tree.push(RenderCommand::StrokeRect {
                x: cancel_x - 2.0,
                y: buttons_y - 2.0,
                width: BUTTON_MIN_WIDTH + 4.0,
                height: BUTTON_HEIGHT + 4.0,
                color: COLOR_LAVENDER,
                line_width: 2.0,
                corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS + 2.0),
            });
        }
        tree.push(RenderCommand::Text {
            x: cancel_x + (BUTTON_MIN_WIDTH - 42.0) / 2.0,
            y: buttons_y + (BUTTON_HEIGHT - FONT_SIZE) / 2.0,
            text: String::from("Cancel"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

// --- ProgressDialog ---

/// Progress dialog for long-running operations.
///
/// Supports:
/// - Indeterminate mode (animated, no specific percentage)
/// - Determinate mode (0.0 to 1.0 progress)
/// - Status text and optional detail text
/// - Optional cancel button
#[derive(Clone, Debug)]
pub struct ProgressDialog {
    title: String,
    status_text: String,
    detail_text: Option<String>,
    show_detail: bool,
    progress: ProgressMode,
    cancelable: bool,
    cancelled: bool,
    /// Animation tick counter for indeterminate mode.
    anim_tick: u64,
    overlay: ModalOverlay,
}

/// Progress mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProgressMode {
    /// Indeterminate — shows an animated bar.
    Indeterminate,
    /// Determinate — shows a specific percentage (0.0 to 1.0).
    Determinate(f32),
}

impl ProgressDialog {
    /// Create a new indeterminate progress dialog.
    pub fn indeterminate(title: &str, status: &str) -> Self {
        let mut overlay = ModalOverlay::new();
        overlay.dismiss_on_escape = false;
        overlay.dismiss_on_click_outside = false;

        Self {
            title: title.to_string(),
            status_text: status.to_string(),
            detail_text: None,
            show_detail: false,
            progress: ProgressMode::Indeterminate,
            cancelable: false,
            cancelled: false,
            anim_tick: 0,
            overlay,
        }
    }

    /// Create a new determinate progress dialog.
    pub fn determinate(title: &str, status: &str) -> Self {
        let mut overlay = ModalOverlay::new();
        overlay.dismiss_on_escape = false;
        overlay.dismiss_on_click_outside = false;

        Self {
            title: title.to_string(),
            status_text: status.to_string(),
            detail_text: None,
            show_detail: false,
            progress: ProgressMode::Determinate(0.0),
            cancelable: false,
            cancelled: false,
            anim_tick: 0,
            overlay,
        }
    }

    /// Builder: make the dialog cancelable.
    #[must_use]
    pub fn with_cancel(mut self) -> Self {
        self.cancelable = true;
        self.overlay.dismiss_on_escape = true;
        self
    }

    /// Builder: set initial detail text.
    #[must_use]
    pub fn with_detail(mut self, detail: &str) -> Self {
        self.detail_text = Some(detail.to_string());
        self.show_detail = true;
        self
    }

    /// Show the dialog.
    pub fn show(&mut self) {
        self.cancelled = false;
        self.overlay.show();
    }

    /// Hide the dialog (operation complete).
    pub fn hide(&mut self) {
        self.overlay.hide();
    }

    /// Whether the dialog is active.
    pub fn is_active(&self) -> bool {
        self.overlay.active
    }

    /// Whether the user cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Update the progress value (for determinate mode, 0.0 to 1.0).
    pub fn set_progress(&mut self, value: f32) {
        self.progress = ProgressMode::Determinate(value.clamp(0.0, 1.0));
    }

    /// Update the status text.
    pub fn set_status(&mut self, status: &str) {
        self.status_text = status.to_string();
    }

    /// Update the detail text.
    pub fn set_detail(&mut self, detail: Option<&str>) {
        self.detail_text = detail.map(|s| s.to_string());
    }

    /// Toggle detail text visibility.
    pub fn toggle_detail(&mut self) {
        self.show_detail = !self.show_detail;
    }

    /// Get the current progress mode.
    pub fn progress(&self) -> ProgressMode {
        self.progress
    }

    /// Update animation state.
    pub fn tick(&mut self, elapsed_ms: u64) {
        self.anim_tick = self.anim_tick.wrapping_add(elapsed_ms);
        self.overlay.tick(elapsed_ms);
    }

    /// Handle an event.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.overlay.active {
            return EventResult::Ignored;
        }

        match event {
            Event::Key(key_event) => {
                if key_event.pressed && key_event.key == Key::Escape && self.cancelable {
                    self.cancelled = true;
                    self.overlay.hide();
                }
                EventResult::Consumed
            }
            Event::Tick { elapsed_ms } => {
                self.tick(*elapsed_ms);
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    /// Render the progress dialog.
    pub fn render(&self, parent_width: f32, parent_height: f32, tree: &mut RenderTree) {
        if !self.overlay.active && self.overlay.opacity <= 0.0 {
            return;
        }

        self.overlay.render(parent_width, parent_height, tree);

        let width = DIALOG_MIN_WIDTH + 40.0;
        let detail_height = if self.show_detail && self.detail_text.is_some() {
            FONT_SIZE_SMALL + 8.0
        } else {
            0.0
        };
        let cancel_height = if self.cancelable {
            BUTTON_HEIGHT + CONTENT_PADDING
        } else {
            0.0
        };
        let height = TITLE_BAR_HEIGHT + CONTENT_PADDING
            + FONT_SIZE + 12.0 // status text
            + PROGRESS_BAR_HEIGHT + 12.0 // progress bar
            + detail_height
            + cancel_height
            + CONTENT_PADDING;
        let x = (parent_width - width) / 2.0;
        let y = (parent_height - height) / 2.0;

        // Shadow.
        tree.push(RenderCommand::BoxShadow {
            x,
            y,
            width,
            height,
            offset_x: 0.0,
            offset_y: SHADOW_OFFSET_Y,
            blur: SHADOW_BLUR,
            spread: 0.0,
            color: SHADOW_COLOR,
            corner_radii: CornerRadii::all(DIALOG_CORNER_RADIUS),
        });

        // Background.
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(DIALOG_CORNER_RADIUS),
        });

        // Title bar.
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: TITLE_BAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii {
                top_left: DIALOG_CORNER_RADIUS,
                top_right: DIALOG_CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Title.
        tree.push(RenderCommand::Text {
            x: x + CONTENT_PADDING,
            y: y + (TITLE_BAR_HEIGHT - FONT_SIZE_TITLE) / 2.0,
            text: self.title.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_TITLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - CONTENT_PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Status text.
        let mut content_y = y + TITLE_BAR_HEIGHT + CONTENT_PADDING;
        tree.push(RenderCommand::Text {
            x: x + CONTENT_PADDING,
            y: content_y,
            text: self.status_text.clone(),
            color: COLOR_SUBTEXT1,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - CONTENT_PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
        content_y += FONT_SIZE + 12.0;

        // Progress bar.
        let bar_width = width - CONTENT_PADDING * 2.0;
        let bar_x = x + CONTENT_PADDING;

        // Bar background.
        tree.push(RenderCommand::FillRect {
            x: bar_x,
            y: content_y,
            width: bar_width,
            height: PROGRESS_BAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(PROGRESS_BAR_RADIUS),
        });

        // Bar fill.
        match self.progress {
            ProgressMode::Determinate(value) => {
                let fill_width = bar_width * value;
                if fill_width > 0.0 {
                    tree.push(RenderCommand::FillRect {
                        x: bar_x,
                        y: content_y,
                        width: fill_width,
                        height: PROGRESS_BAR_HEIGHT,
                        color: COLOR_BLUE,
                        corner_radii: CornerRadii::all(PROGRESS_BAR_RADIUS),
                    });
                }
            }
            ProgressMode::Indeterminate => {
                // Animate a sliding segment.
                let cycle = (self.anim_tick % 2000) as f32 / 2000.0;
                let segment_width = bar_width * 0.3;
                let segment_x = bar_x + (bar_width - segment_width) * cycle;
                tree.push(RenderCommand::FillRect {
                    x: segment_x,
                    y: content_y,
                    width: segment_width,
                    height: PROGRESS_BAR_HEIGHT,
                    color: COLOR_BLUE,
                    corner_radii: CornerRadii::all(PROGRESS_BAR_RADIUS),
                });
            }
        }

        content_y += PROGRESS_BAR_HEIGHT + 12.0;

        // Percentage text (for determinate mode).
        if let ProgressMode::Determinate(value) = self.progress {
            let pct = (value * 100.0) as u32;
            tree.push(RenderCommand::Text {
                x: x + width - CONTENT_PADDING - 40.0,
                y: content_y - PROGRESS_BAR_HEIGHT - 10.0 - FONT_SIZE_SMALL,
                text: format!("{pct}%"),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Detail text.
        if self.show_detail
            && let Some(ref detail) = self.detail_text
        {
            tree.push(RenderCommand::Text {
                x: x + CONTENT_PADDING,
                y: content_y,
                text: detail.clone(),
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bar_width),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Cancel button.
        if self.cancelable {
            let btn_y = y + height - BUTTON_HEIGHT - CONTENT_PADDING;
            let btn_x = x + width - CONTENT_PADDING - BUTTON_MIN_WIDTH;
            tree.push(RenderCommand::FillRect {
                x: btn_x,
                y: btn_y,
                width: BUTTON_MIN_WIDTH,
                height: BUTTON_HEIGHT,
                color: COLOR_SURFACE1,
                corner_radii: CornerRadii::all(BUTTON_CORNER_RADIUS),
            });
            tree.push(RenderCommand::Text {
                x: btn_x + (BUTTON_MIN_WIDTH - 42.0) / 2.0,
                y: btn_y + (BUTTON_HEIGHT - FONT_SIZE) / 2.0,
                text: String::from("Cancel"),
                color: COLOR_RED,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }
}

// --- NonModalDialog ---

/// Floating non-modal dialog window.
///
/// Unlike modal dialogs, non-modal dialogs do not block input to the parent.
/// They feature:
/// - Title bar with close button (X)
/// - Draggable by title bar
/// - Optional resize behavior
/// - Stays on top but allows parent interaction
/// - Builder pattern for content
#[derive(Clone, Debug)]
pub struct NonModalDialog {
    title: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    min_width: f32,
    min_height: f32,
    max_width: f32,
    max_height: f32,
    visible: bool,
    /// Whether the dialog is currently being dragged.
    dragging: bool,
    /// Offset from the mouse to the dialog origin during drag.
    drag_offset: (f32, f32),
    /// Whether the dialog is resizable.
    resizable: bool,
    /// Whether a resize is in progress.
    resizing: bool,
    /// Content render callback produces commands for the body.
    content_commands: Vec<RenderCommand>,
    /// Whether the close button is hovered.
    close_hovered: bool,
}

impl NonModalDialog {
    /// Create a new non-modal dialog with a title.
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            x: 100.0,
            y: 100.0,
            width: 400.0,
            height: 300.0,
            min_width: 200.0,
            min_height: 120.0,
            max_width: 1200.0,
            max_height: 900.0,
            visible: false,
            dragging: false,
            drag_offset: (0.0, 0.0),
            resizable: false,
            resizing: false,
            content_commands: Vec::new(),
            close_hovered: false,
        }
    }

    /// Builder: set initial position.
    #[must_use]
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    /// Builder: set initial size.
    #[must_use]
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.width = width.clamp(self.min_width, self.max_width);
        self.height = height.clamp(self.min_height, self.max_height);
        self
    }

    /// Builder: set minimum size constraints.
    #[must_use]
    pub fn with_min_size(mut self, min_width: f32, min_height: f32) -> Self {
        self.min_width = min_width;
        self.min_height = min_height;
        self
    }

    /// Builder: set maximum size constraints.
    #[must_use]
    pub fn with_max_size(mut self, max_width: f32, max_height: f32) -> Self {
        self.max_width = max_width;
        self.max_height = max_height;
        self
    }

    /// Builder: enable resizing.
    #[must_use]
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Show the dialog.
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Hide (close) the dialog.
    pub fn hide(&mut self) {
        self.visible = false;
        self.dragging = false;
        self.resizing = false;
    }

    /// Whether the dialog is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Get the dialog position.
    pub fn position(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// Get the dialog size.
    pub fn size(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    /// Set position programmatically.
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
    }

    /// Set the content commands to render inside the dialog body.
    pub fn set_content(&mut self, commands: Vec<RenderCommand>) {
        self.content_commands = commands;
    }

    /// Center the dialog within the given area.
    pub fn center_in(&mut self, area_width: f32, area_height: f32) {
        self.x = (area_width - self.width) / 2.0;
        self.y = (area_height - self.height) / 2.0;
    }

    /// Handle an event. Returns whether the event was consumed.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.visible {
            return EventResult::Ignored;
        }

        match event {
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            Event::Key(key_event) => self.handle_key(key_event),
            _ => EventResult::Ignored,
        }
    }

    /// Handle mouse events (drag, close button, resize).
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                // Check close button hit.
                let close_x = self.x + self.width - CONTENT_PADDING - CLOSE_BUTTON_SIZE;
                let close_y = self.y + (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0;
                if point_in_rect(
                    event.x,
                    event.y,
                    close_x,
                    close_y,
                    CLOSE_BUTTON_SIZE,
                    CLOSE_BUTTON_SIZE,
                ) {
                    self.hide();
                    return EventResult::Consumed;
                }

                // Check title bar drag.
                if point_in_rect(
                    event.x,
                    event.y,
                    self.x,
                    self.y,
                    self.width,
                    TITLE_BAR_HEIGHT,
                ) {
                    self.dragging = true;
                    self.drag_offset = (event.x - self.x, event.y - self.y);
                    return EventResult::Consumed;
                }

                // Check resize handle (bottom-right corner).
                if self.resizable {
                    let resize_area = 12.0;
                    let rx = self.x + self.width - resize_area;
                    let ry = self.y + self.height - resize_area;
                    if point_in_rect(event.x, event.y, rx, ry, resize_area, resize_area) {
                        self.resizing = true;
                        return EventResult::Consumed;
                    }
                }

                // Check if click is within dialog body.
                if point_in_rect(event.x, event.y, self.x, self.y, self.width, self.height) {
                    return EventResult::Consumed;
                }

                EventResult::Ignored
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if self.dragging || self.resizing {
                    self.dragging = false;
                    self.resizing = false;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            MouseEventKind::Move => {
                if self.dragging {
                    self.x = event.x - self.drag_offset.0;
                    self.y = event.y - self.drag_offset.1;
                    return EventResult::Consumed;
                }
                if self.resizing {
                    let new_width = (event.x - self.x).clamp(self.min_width, self.max_width);
                    let new_height = (event.y - self.y).clamp(self.min_height, self.max_height);
                    self.width = new_width;
                    self.height = new_height;
                    return EventResult::Consumed;
                }

                // Update close button hover state.
                let close_x = self.x + self.width - CONTENT_PADDING - CLOSE_BUTTON_SIZE;
                let close_y = self.y + (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0;
                self.close_hovered = point_in_rect(
                    event.x,
                    event.y,
                    close_x,
                    close_y,
                    CLOSE_BUTTON_SIZE,
                    CLOSE_BUTTON_SIZE,
                );

                if point_in_rect(event.x, event.y, self.x, self.y, self.width, self.height) {
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle key events.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }
        // Non-modal dialogs don't typically capture keyboard; pass through.
        EventResult::Ignored
    }

    /// Render the non-modal dialog.
    pub fn render(&self, tree: &mut RenderTree) {
        if !self.visible {
            return;
        }

        // Shadow.
        tree.push(RenderCommand::BoxShadow {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            offset_x: 0.0,
            offset_y: SHADOW_OFFSET_Y,
            blur: SHADOW_BLUR,
            spread: 0.0,
            color: SHADOW_COLOR,
            corner_radii: CornerRadii::all(DIALOG_CORNER_RADIUS),
        });

        // Background.
        tree.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(DIALOG_CORNER_RADIUS),
        });

        // Title bar.
        tree.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: TITLE_BAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii {
                top_left: DIALOG_CORNER_RADIUS,
                top_right: DIALOG_CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Title text.
        tree.push(RenderCommand::Text {
            x: self.x + CONTENT_PADDING,
            y: self.y + (TITLE_BAR_HEIGHT - FONT_SIZE_TITLE) / 2.0,
            text: self.title.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_TITLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - CONTENT_PADDING * 2.0 - CLOSE_BUTTON_SIZE - 8.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Close button (X).
        let close_x = self.x + self.width - CONTENT_PADDING - CLOSE_BUTTON_SIZE;
        let close_y = self.y + (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0;
        let close_bg = if self.close_hovered {
            COLOR_SURFACE2
        } else {
            COLOR_SURFACE0
        };
        tree.push(RenderCommand::FillRect {
            x: close_x,
            y: close_y,
            width: CLOSE_BUTTON_SIZE,
            height: CLOSE_BUTTON_SIZE,
            color: close_bg,
            corner_radii: CornerRadii::all(CLOSE_BUTTON_SIZE / 2.0),
        });
        tree.push(RenderCommand::Text {
            x: close_x + (CLOSE_BUTTON_SIZE - 8.0) / 2.0,
            y: close_y + (CLOSE_BUTTON_SIZE - FONT_SIZE) / 2.0,
            text: String::from("X"),
            color: if self.close_hovered {
                COLOR_RED
            } else {
                COLOR_OVERLAY1
            },
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Content area with clipping.
        let content_y = self.y + TITLE_BAR_HEIGHT;
        let content_height = self.height - TITLE_BAR_HEIGHT;
        tree.push(RenderCommand::PushClip {
            x: self.x,
            y: content_y,
            width: self.width,
            height: content_height,
        });
        tree.push(RenderCommand::PushTranslate {
            dx: self.x,
            dy: content_y,
        });

        // Render content commands.
        for cmd in &self.content_commands {
            tree.push(cmd.clone());
        }

        tree.push(RenderCommand::PopTranslate);
        tree.push(RenderCommand::PopClip);

        // Resize handle indicator (bottom-right corner).
        if self.resizable {
            let handle_size = 12.0;
            let hx = self.x + self.width - handle_size;
            let hy = self.y + self.height - handle_size;
            // Draw two small diagonal lines as resize grip.
            tree.push(RenderCommand::Line {
                x1: hx + 4.0,
                y1: hy + handle_size - 2.0,
                x2: hx + handle_size - 2.0,
                y2: hy + 4.0,
                color: COLOR_OVERLAY0,
                width: 1.0,
            });
            tree.push(RenderCommand::Line {
                x1: hx + 8.0,
                y1: hy + handle_size - 2.0,
                x2: hx + handle_size - 2.0,
                y2: hy + 8.0,
                color: COLOR_OVERLAY0,
                width: 1.0,
            });
        }
    }
}

// --- Internal helpers ---

/// Layout information for a dialog (computed position/size + button hit areas).
struct DialogLayout {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    button_rects: Vec<(f32, f32, f32, f32)>,
}

/// Point-in-rectangle hit test.
fn point_in_rect(px: f32, py: f32, rx: f32, ry: f32, rw: f32, rh: f32) -> bool {
    px >= rx && px <= rx + rw && py >= ry && py <= ry + rh
}

// --- Tests ---

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

    // --- DialogResult tests ---

    #[test]
    fn test_dialog_result_variants() {
        assert_eq!(DialogResult::Ok, DialogResult::Ok);
        assert_ne!(DialogResult::Ok, DialogResult::Cancel);
        assert_eq!(
            DialogResult::Text(String::from("hello")),
            DialogResult::Text(String::from("hello"))
        );
        assert_ne!(
            DialogResult::Text(String::from("a")),
            DialogResult::Text(String::from("b"))
        );
    }

    // --- ButtonSet tests ---

    #[test]
    fn test_button_set_ok() {
        let bs = ButtonSet::ok();
        assert_eq!(bs.len(), 1);
        assert!(!bs.is_empty());
        assert_eq!(bs.buttons[0], DialogButton::Ok);
    }

    #[test]
    fn test_button_set_ok_cancel() {
        let bs = ButtonSet::ok_cancel();
        assert_eq!(bs.len(), 2);
        assert_eq!(bs.buttons[0], DialogButton::Ok);
        assert_eq!(bs.buttons[1], DialogButton::Cancel);
    }

    #[test]
    fn test_button_set_yes_no() {
        let bs = ButtonSet::yes_no();
        assert_eq!(bs.len(), 2);
        assert_eq!(bs.buttons[0], DialogButton::Yes);
        assert_eq!(bs.buttons[1], DialogButton::No);
    }

    #[test]
    fn test_button_set_yes_no_cancel() {
        let bs = ButtonSet::yes_no_cancel();
        assert_eq!(bs.len(), 3);
        assert_eq!(bs.buttons[0], DialogButton::Yes);
        assert_eq!(bs.buttons[1], DialogButton::No);
        assert_eq!(bs.buttons[2], DialogButton::Cancel);
    }

    #[test]
    fn test_button_set_custom() {
        let bs = ButtonSet::custom(vec![DialogButton::No, DialogButton::Ok]);
        assert_eq!(bs.len(), 2);
        assert_eq!(bs.buttons[0], DialogButton::No);
        assert_eq!(bs.buttons[1], DialogButton::Ok);
    }

    #[test]
    fn test_button_to_result() {
        assert_eq!(DialogButton::Ok.to_result(), DialogResult::Ok);
        assert_eq!(DialogButton::Cancel.to_result(), DialogResult::Cancel);
        assert_eq!(DialogButton::Yes.to_result(), DialogResult::Yes);
        assert_eq!(DialogButton::No.to_result(), DialogResult::No);
    }

    #[test]
    fn test_button_is_primary() {
        assert!(DialogButton::Ok.is_primary());
        assert!(DialogButton::Yes.is_primary());
        assert!(!DialogButton::Cancel.is_primary());
        assert!(!DialogButton::No.is_primary());
    }

    // --- ModalOverlay tests ---

    #[test]
    fn test_overlay_initial_state() {
        let overlay = ModalOverlay::new();
        assert!(!overlay.active);
        assert_eq!(overlay.opacity, 0.0);
        assert!(overlay.dismiss_on_click_outside);
        assert!(overlay.dismiss_on_escape);
    }

    #[test]
    fn test_overlay_show_hide() {
        let mut overlay = ModalOverlay::new();
        overlay.show();
        assert!(overlay.active);
        assert_eq!(overlay.target_opacity, 1.0);

        overlay.hide();
        assert_eq!(overlay.target_opacity, 0.0);
    }

    #[test]
    fn test_overlay_fade_in() {
        let mut overlay = ModalOverlay::new();
        overlay.show();

        // Tick enough to approach full opacity.
        for _ in 0..300 {
            overlay.tick(1);
        }
        assert!(overlay.opacity > 0.9);
    }

    #[test]
    fn test_overlay_fade_out_deactivates() {
        let mut overlay = ModalOverlay::new();
        overlay.show();
        overlay.opacity = 1.0; // Skip fade-in.
        overlay.hide();

        // Tick enough to fade out.
        for _ in 0..300 {
            overlay.tick(1);
        }
        assert!(overlay.is_fully_hidden());
        assert!(!overlay.active);
    }

    #[test]
    fn test_overlay_click_outside_dismisses() {
        let mut overlay = ModalOverlay::new();
        overlay.show();
        overlay.opacity = 1.0;
        overlay.set_content_rect(100.0, 100.0, 200.0, 200.0);

        // Click outside content rect.
        let mouse = MouseEvent {
            x: 50.0,
            y: 50.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        let result = overlay.handle_mouse(&mouse);
        assert_eq!(result, Some(DialogResult::Dismissed));
    }

    #[test]
    fn test_overlay_click_inside_does_not_dismiss() {
        let mut overlay = ModalOverlay::new();
        overlay.show();
        overlay.opacity = 1.0;
        overlay.set_content_rect(100.0, 100.0, 200.0, 200.0);

        let mouse = MouseEvent {
            x: 150.0,
            y: 150.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        let result = overlay.handle_mouse(&mouse);
        assert_eq!(result, None);
    }

    #[test]
    fn test_overlay_escape_dismisses() {
        let mut overlay = ModalOverlay::new();
        overlay.show();
        overlay.opacity = 1.0;

        let key = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = overlay.handle_key(&key);
        assert_eq!(result, Some(DialogResult::Dismissed));
    }

    #[test]
    fn test_overlay_escape_disabled() {
        let mut overlay = ModalOverlay::new();
        overlay.dismiss_on_escape = false;
        overlay.show();
        overlay.opacity = 1.0;

        let key = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = overlay.handle_key(&key);
        assert_eq!(result, None);
    }

    #[test]
    fn test_overlay_render_produces_scrim() {
        let mut overlay = ModalOverlay::new();
        overlay.show();
        overlay.opacity = 1.0;

        let mut tree = RenderTree::new();
        overlay.render(800.0, 600.0, &mut tree);

        assert!(!tree.is_empty());
        // Should contain a FillRect for the scrim.
        let has_fill = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::FillRect { .. }));
        assert!(has_fill);
    }

    // --- AlertDialog tests ---

    #[test]
    fn test_alert_info_creation() {
        let dialog = AlertDialog::info("Title", "Message");
        assert_eq!(dialog.title, "Title");
        assert_eq!(dialog.message, "Message");
        assert_eq!(dialog.icon, DialogIcon::Info);
        assert_eq!(dialog.buttons.len(), 1);
    }

    #[test]
    fn test_alert_warning_creation() {
        let dialog = AlertDialog::warning("Warn", "Something");
        assert_eq!(dialog.icon, DialogIcon::Warning);
    }

    #[test]
    fn test_alert_error_creation() {
        let dialog = AlertDialog::error("Err", "Bad thing");
        assert_eq!(dialog.icon, DialogIcon::Error);
    }

    #[test]
    fn test_alert_confirm_has_two_buttons() {
        let dialog = AlertDialog::confirm("Confirm", "Are you sure?");
        assert_eq!(dialog.buttons.len(), 2);
    }

    #[test]
    fn test_alert_yes_no_has_two_buttons() {
        let dialog = AlertDialog::yes_no("Choice", "Pick one");
        assert_eq!(dialog.buttons.len(), 2);
        assert_eq!(dialog.buttons.buttons[0], DialogButton::Yes);
        assert_eq!(dialog.buttons.buttons[1], DialogButton::No);
    }

    #[test]
    fn test_alert_yes_no_cancel_has_three_buttons() {
        let dialog = AlertDialog::yes_no_cancel("Choice", "Pick one");
        assert_eq!(dialog.buttons.len(), 3);
    }

    #[test]
    fn test_alert_show_activates() {
        let mut dialog = AlertDialog::info("Test", "Test");
        assert!(!dialog.is_active());
        dialog.show();
        assert!(dialog.is_active());
    }

    #[test]
    fn test_alert_enter_confirms_focused_button() {
        let mut dialog = AlertDialog::confirm("Test", "Test");
        dialog.show();

        // Focused button starts at 0 (OK).
        let event = Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&event);
        assert_eq!(dialog.result(), Some(&DialogResult::Ok));
    }

    #[test]
    fn test_alert_tab_cycles_focus() {
        let mut dialog = AlertDialog::yes_no_cancel("Test", "Test");
        dialog.show();
        assert_eq!(dialog.focused_button(), 0);

        // Tab forward.
        let tab = Event::Key(KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&tab);
        assert_eq!(dialog.focused_button(), 1);

        dialog.handle_event(&tab);
        assert_eq!(dialog.focused_button(), 2);

        // Wraps around.
        dialog.handle_event(&tab);
        assert_eq!(dialog.focused_button(), 0);
    }

    #[test]
    fn test_alert_shift_tab_cycles_backwards() {
        let mut dialog = AlertDialog::yes_no_cancel("Test", "Test");
        dialog.show();

        let shift_tab = Event::Key(KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: None,
        });
        dialog.handle_event(&shift_tab);
        assert_eq!(dialog.focused_button(), 2); // Wraps to last.
    }

    #[test]
    fn test_alert_escape_dismisses() {
        let mut dialog = AlertDialog::info("Test", "Test");
        dialog.show();

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&esc);
        assert_eq!(dialog.result(), Some(&DialogResult::Dismissed));
    }

    #[test]
    fn test_alert_escape_dismiss_disabled() {
        let mut dialog = AlertDialog::info("Test", "Test").with_escape_dismiss(false);
        dialog.show();

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&esc);
        // Should not have dismissed.
        assert_eq!(dialog.result(), None);
    }

    #[test]
    fn test_alert_render_produces_output() {
        let mut dialog = AlertDialog::info("Hello", "World");
        dialog.show();
        dialog.overlay.opacity = 1.0;

        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);

        // Should produce multiple commands (scrim, shadow, bg, title bar, text, buttons).
        assert!(tree.len() > 5);
    }

    /// Every message line an alert drew, as (y, text), in draw order.
    fn alert_message_lines(dialog: &AlertDialog) -> Vec<(f32, String)> {
        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);
        tree.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    text,
                    font_size,
                    color,
                    ..
                } if (*font_size - FONT_SIZE).abs() < 0.01 && *color == COLOR_SUBTEXT1 => {
                    Some((*y, text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_long_alert_message_is_wrapped_not_truncated() {
        // The compositor truncates at `max_width` instead of wrapping, so a
        // message sent as one command lost everything past its first line.
        let message = "The file could not be saved because the destination \
                       volume is read-only. Choose another location and try \
                       again, or unlock the volume first.";
        let mut dialog = AlertDialog::error("Save failed", message);
        dialog.show();
        dialog.overlay.opacity = 1.0;

        let lines = alert_message_lines(&dialog);
        assert!(
            lines.len() > 1,
            "a {} character message was drawn as {} line(s)",
            message.len(),
            lines.len()
        );
        assert_eq!(
            lines
                .iter()
                .flat_map(|(_, l)| l.split_whitespace())
                .collect::<Vec<_>>(),
            message.split_whitespace().collect::<Vec<_>>(),
            "the drawn lines are not the message"
        );
    }

    #[test]
    fn an_alert_message_stays_inside_its_dialog() {
        // Both directions: every line fits the width it was wrapped for, and
        // the block of lines fits between the title bar and the buttons.
        let message = "Deleting this project removes every file it contains, \
                       including any work that has not been backed up, and this \
                       cannot be undone afterwards.";
        let mut dialog = AlertDialog::warning("Delete project?", message);
        dialog.show();
        dialog.overlay.opacity = 1.0;
        let layout = dialog.compute_layout(800.0, 600.0);

        let lines = alert_message_lines(&dialog);
        for (_, line) in &lines {
            if line.split_whitespace().count() < 2 {
                continue;
            }
            assert!(
                crate::text::width(line, FONT_SIZE) <= dialog.message_max_width(),
                "{line:?} is wider than the dialog that contains it"
            );
        }
        let last_y = lines.iter().map(|&(y, _)| y).fold(f32::MIN, f32::max);
        let buttons_y = layout.y + layout.height - BUTTON_HEIGHT - CONTENT_PADDING;
        assert!(
            last_y + MESSAGE_LINE_HEIGHT <= buttons_y,
            "the last message line at {last_y} runs into the buttons at {buttons_y}"
        );
    }

    #[test]
    fn an_input_dialog_field_clears_a_wrapped_prompt() {
        // The prompt used to get a flat one-line allowance, so the input field
        // was drawn over the second line of anything longer.
        let prompt = "Enter the full path of the directory to index, including \
                      any network share you want covered by the search.";
        let mut dialog = InputDialog::prompt("Index", prompt, "");
        dialog.show();
        dialog.overlay.opacity = 1.0;

        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);

        let lines: Vec<f32> = tree
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    font_size,
                    color,
                    ..
                } if (*font_size - FONT_SIZE).abs() < 0.01 && *color == COLOR_SUBTEXT1 => Some(*y),
                _ => None,
            })
            .collect();
        assert!(lines.len() > 1, "the prompt was drawn as one line");

        // The input field is the first surface-coloured box of INPUT_HEIGHT.
        let field_y = tree
            .commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect { y, height, .. }
                    if (*height - INPUT_HEIGHT).abs() < 0.01 =>
                {
                    Some(*y)
                }
                _ => None,
            })
            .expect("the input dialog drew no input field");
        let prompt_bottom = lines.iter().fold(f32::MIN, |a, &b| a.max(b)) + MESSAGE_LINE_HEIGHT;
        assert!(
            prompt_bottom <= field_y,
            "the prompt ends at {prompt_bottom}, below the input field at {field_y}"
        );
    }

    #[test]
    fn an_oversized_message_is_not_drawn_over_the_buttons() {
        // The dialog height is clamped, so a message can be longer than any box
        // it can be given. It must lose its tail rather than cover the controls
        // that dismiss it.
        let message = "word ".repeat(600);
        let mut dialog = AlertDialog::error("Failed", &message);
        dialog.show();
        dialog.overlay.opacity = 1.0;
        let layout = dialog.compute_layout(800.0, 600.0);
        let buttons_y = layout.y + layout.height - BUTTON_HEIGHT - CONTENT_PADDING;

        let lines = alert_message_lines(&dialog);
        assert!(!lines.is_empty(), "the message vanished entirely");
        for (y, line) in &lines {
            assert!(
                y + MESSAGE_LINE_HEIGHT <= buttons_y,
                "{line:?} at {y} is drawn into the button row at {buttons_y}"
            );
        }
    }

    #[test]
    fn an_alert_grows_for_a_longer_message() {
        // The height used to be a flat three-line guess, so these were equal.
        let short = AlertDialog::info("T", "Done.");
        let long = AlertDialog::info(
            "T",
            "The operation finished, but several items were skipped because \
             they were already present at the destination and the overwrite \
             option was not enabled for this run.",
        );
        assert!(
            long.compute_height() > short.compute_height(),
            "a long message ({}) got no more room than a short one ({})",
            long.compute_height(),
            short.compute_height()
        );
    }

    #[test]
    fn test_alert_builder_custom_buttons() {
        let dialog = AlertDialog::info("Test", "Test")
            .with_buttons(ButtonSet::custom(vec![DialogButton::No, DialogButton::Yes]));
        assert_eq!(dialog.buttons.len(), 2);
        assert_eq!(dialog.buttons.buttons[0], DialogButton::No);
    }

    #[test]
    fn test_alert_builder_custom_icon() {
        let dialog = AlertDialog::info("Test", "Test").with_icon(DialogIcon::Error);
        assert_eq!(dialog.icon, DialogIcon::Error);
    }

    // --- InputDialog tests ---

    #[test]
    fn test_input_dialog_creation() {
        let dialog = InputDialog::prompt("Name", "Enter your name:", "John Doe");
        assert_eq!(dialog.title, "Name");
        assert_eq!(dialog.message, "Enter your name:");
        assert_eq!(dialog.placeholder, "John Doe");
        assert!(dialog.input_text.is_empty());
        assert!(!dialog.password_mode);
    }

    #[test]
    fn test_input_dialog_password_mode() {
        let dialog = InputDialog::prompt("Password", "Enter:", "").with_password_mode(true);
        assert!(dialog.password_mode);
    }

    #[test]
    fn test_input_dialog_initial_text() {
        let dialog = InputDialog::prompt("Edit", "Edit value:", "").with_initial_text("hello");
        assert_eq!(dialog.input_text(), "hello");
        assert_eq!(dialog.cursor.byte(), 5);
    }

    #[test]
    fn test_input_dialog_typing() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "");
        dialog.show();

        // Type 'h'.
        let event = Event::Key(KeyEvent {
            key: Key::H,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('h'),
        });
        dialog.handle_event(&event);
        assert_eq!(dialog.input_text(), "h");

        // Type 'i'.
        let event = Event::Key(KeyEvent {
            key: Key::I,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('i'),
        });
        dialog.handle_event(&event);
        assert_eq!(dialog.input_text(), "hi");
    }

    #[test]
    fn test_input_dialog_backspace() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "").with_initial_text("hello");
        dialog.show();

        let bs = Event::Key(KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&bs);
        assert_eq!(dialog.input_text(), "hell");
    }

    #[test]
    fn test_input_dialog_delete() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "").with_initial_text("hello");
        dialog.show();
        dialog.cursor = TextCursor::default();

        let del = Event::Key(KeyEvent {
            key: Key::Delete,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&del);
        assert_eq!(dialog.input_text(), "ello");
    }

    #[test]
    fn test_input_dialog_cursor_movement() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "").with_initial_text("hello");
        dialog.show();
        assert_eq!(dialog.cursor.byte(), 5);

        // Left.
        let left = Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&left);
        assert_eq!(dialog.cursor.byte(), 4);

        // Home.
        let home = Event::Key(KeyEvent {
            key: Key::Home,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&home);
        assert_eq!(dialog.cursor.byte(), 0);

        // End.
        let end = Event::Key(KeyEvent {
            key: Key::End,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&end);
        assert_eq!(dialog.cursor.byte(), 5);
    }

    /// Typing a character that is more than one byte long, then editing around
    /// it, used to abort the process: the cursor moved one *byte* per keypress
    /// while `String::insert`/`remove` index by bytes and panic on an offset
    /// inside a character. Every non-ASCII name a user could type — `café`, any
    /// Cyrillic or CJK text — was a crash one keystroke later.
    #[test]
    fn a_multi_byte_character_can_be_typed_moved_over_and_deleted() {
        fn typed(ch: char) -> Event {
            Event::Key(KeyEvent {
                // The `key` code is irrelevant here: a character arrives in
                // `text`, and `Key` has no per-character variant. `é` has no
                // key code at all on most layouts, which is the point.
                key: Key::Unknown(0),
                pressed: true,
                modifiers: Modifiers::NONE,
                text: Some(ch),
            })
        }
        fn pressed(key: Key) -> Event {
            Event::Key(KeyEvent {
                key,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            })
        }

        let mut dialog = InputDialog::prompt("Test", "Type:", "");
        dialog.show();
        for ch in "café".chars() {
            dialog.handle_event(&typed(ch));
        }
        assert_eq!(dialog.input_text(), "café");
        // Five bytes for four characters: the cursor counts bytes.
        assert_eq!(dialog.cursor.byte(), 5);

        // Stepping over the two-byte `é` must land on its start, not inside it.
        dialog.handle_event(&pressed(Key::Left));
        assert_eq!(dialog.cursor.byte(), 3);
        dialog.handle_event(&pressed(Key::Right));
        assert_eq!(dialog.cursor.byte(), 5);

        // Backspace removes the whole character, not one of its two bytes.
        dialog.handle_event(&pressed(Key::Backspace));
        assert_eq!(dialog.input_text(), "caf");
        assert_eq!(dialog.cursor.byte(), 3);

        // Delete from before a multi-byte character takes all of it.
        dialog.handle_event(&typed('é'));
        dialog.handle_event(&pressed(Key::Left));
        dialog.handle_event(&pressed(Key::Delete));
        assert_eq!(dialog.input_text(), "caf");
    }

    fn key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    /// The arrows move **visually** — one position left or right on the screen
    /// — so on `ab` + two Hebrew letters + `cd`, drawn `a b <bet> <aleph> c d`,
    /// the caret steps through the Hebrew rather than jumping across it.
    /// `design-decisions.md` §541.
    ///
    /// Byte 6 is visited twice on the way left and byte 2 twice on the way
    /// right, at the two opposite ends of the Hebrew both times: each of those
    /// gaps answers to both offsets, and only the affinity inside `TextCursor`
    /// says which. See the fuller account on the toolkit's own
    /// `the_arrows_move_by_the_screen_and_keep_the_side_they_are_on`.
    #[test]
    fn a_plain_input_dialog_moves_its_caret_by_the_screen() {
        let text = "ab\u{05D0}\u{05D1}cd";
        let mut dialog = InputDialog::prompt("Test", "Path:", "").with_initial_text(text);
        dialog.show();
        let mut seen = vec![];
        for _ in 0..6 {
            dialog.handle_event(&key(Key::Left));
            seen.push(dialog.cursor.byte());
        }
        assert_eq!(seen, vec![7, 6, 4, 6, 1, 0]);
        let mut seen = vec![];
        for _ in 0..6 {
            dialog.handle_event(&key(Key::Right));
            seen.push(dialog.cursor.byte());
        }
        assert_eq!(seen, vec![1, 2, 4, 2, 7, 8]);
        // Past the end it stays put rather than wrapping.
        dialog.handle_event(&key(Key::Right));
        assert_eq!(dialog.cursor.byte(), text.len());
    }

    /// A password field keeps stepping **logically**, and is the one documented
    /// exception to §541's visual arrows. What it draws is a row of asterisks:
    /// its drawn order is its string order whatever was typed, so moving by the
    /// layout of the hidden text would scatter the caret among identical marks
    /// with nothing on screen to explain the jumps — and would leak the shape
    /// of the secret to anyone watching, which is the one thing masking exists
    /// to prevent.
    ///
    /// This is now a live contrast rather than a hypothetical one: the plain
    /// dialog next door really does walk 7, 6, 4, 6, 1, 0 on this same text,
    /// and this one must not.
    #[test]
    fn a_password_field_steps_through_its_mask_not_its_secret() {
        let text = "ab\u{05D0}\u{05D1}cd";
        let mut hidden = InputDialog::prompt("Test", "Password:", "")
            .with_password_mode(true)
            .with_initial_text(text);
        hidden.show();
        let mut seen = vec![];
        for _ in 0..6 {
            hidden.handle_event(&key(Key::Left));
            seen.push(hidden.cursor.byte());
        }
        assert_eq!(
            seen,
            vec![7, 6, 4, 2, 1, 0],
            "asterisks are crossed in string order"
        );
        // What the *visual* walk would have done with the same text, for the
        // contrast: it enters the Hebrew from the other end. This is the
        // sequence a password field must not adopt.
        let mut visual = vec![];
        let mut at = crate::text::TextCursor::from(text.len());
        for _ in 0..6 {
            let Some(next) = crate::text::caret_left(text, at, FONT_SIZE, FontWeightHint::Regular)
            else {
                break;
            };
            visual.push(next.byte());
            at = next;
        }
        assert_eq!(visual, vec![7, 6, 4, 6, 1, 0]);
    }

    #[test]
    fn test_input_dialog_enter_accepts() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "").with_initial_text("result");
        dialog.show();

        let enter = Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&enter);
        assert_eq!(
            dialog.result(),
            Some(&DialogResult::Text(String::from("result")))
        );
    }

    #[test]
    fn test_input_dialog_escape_cancels() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "");
        dialog.show();

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&esc);
        assert_eq!(dialog.result(), Some(&DialogResult::Cancel));
    }

    #[test]
    fn test_input_dialog_validation_error_blocks_accept() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "")
            .with_validation()
            .with_initial_text("bad");
        dialog.show();
        dialog.set_validation_error(Some("Input is invalid"));

        let enter = Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&enter);
        // Should not accept because there is a validation error.
        assert_eq!(dialog.result(), None);
        assert!(dialog.has_validation_error());
    }

    #[test]
    fn test_input_dialog_typing_clears_validation() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "");
        dialog.show();
        dialog.set_validation_error(Some("Error"));
        assert!(dialog.has_validation_error());

        let event = Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        });
        dialog.handle_event(&event);
        assert!(!dialog.has_validation_error());
    }

    #[test]
    fn tab_walks_the_button_row_and_wraps_at_both_ends() {
        fn tab(shift: bool) -> Event {
            Event::Key(KeyEvent {
                key: Key::Tab,
                pressed: true,
                modifiers: if shift {
                    Modifiers {
                        shift: true,
                        ..Modifiers::NONE
                    }
                } else {
                    Modifiers::NONE
                },
                text: None,
            })
        }

        let mut dialog = AlertDialog::yes_no_cancel("Save?", "Save before closing?");
        dialog.show();
        assert_eq!(dialog.buttons().len(), 3);
        assert_eq!(dialog.focused_button(), 0);

        // Forward off the end comes back to the first — the wrap that used to
        // be `(focused + 1) % len`, guarded by an emptiness test one statement
        // above it.
        for expected in [1, 2, 0, 1] {
            dialog.handle_event(&tab(false));
            assert_eq!(dialog.focused_button(), expected);
        }
        // And backward off the front reaches the last.
        for expected in [0, 2, 1, 0] {
            dialog.handle_event(&tab(true));
            assert_eq!(dialog.focused_button(), expected);
        }
    }

    #[test]
    fn tabbing_a_dialog_with_no_buttons_is_not_fatal() {
        // `step` answers 0 for an empty list, which is the only index that
        // could mean anything; what matters is that neither direction
        // subtracts from a zero length.
        let mut dialog = AlertDialog::info("Note", "No buttons here")
            .with_buttons(ButtonSet::custom(Vec::new()));
        dialog.show();
        for shift in [false, true] {
            dialog.handle_event(&Event::Key(KeyEvent {
                key: Key::Tab,
                pressed: true,
                modifiers: if shift {
                    Modifiers {
                        shift: true,
                        ..Modifiers::NONE
                    }
                } else {
                    Modifiers::NONE
                },
                text: None,
            }));
            assert_eq!(dialog.focused_button(), 0);
        }
    }

    #[test]
    fn test_input_dialog_tab_cycles_focus() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "");
        dialog.show();
        assert_eq!(dialog.focused_element, InputFocus::TextField);

        let tab = Event::Key(KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&tab);
        assert_eq!(dialog.focused_element, InputFocus::OkButton);

        dialog.handle_event(&tab);
        assert_eq!(dialog.focused_element, InputFocus::CancelButton);

        dialog.handle_event(&tab);
        assert_eq!(dialog.focused_element, InputFocus::TextField);
    }

    /// The mask has one asterisk per caret stop, whatever the secret encodes to.
    ///
    /// It used to be `"*".repeat(text.len())` — the UTF-8 *byte* count. An
    /// accented letter therefore drew two marks, an emoji four, and a password
    /// typed in Greek or Hebrew or Japanese drew a row two or three times its
    /// own length. Two things break at once:
    ///
    /// - the marks stop lining up with the places the caret can be. The caret
    ///   steps by character (`caret_offsets` walks `char_indices`), so in an
    ///   eight-character password of eleven bytes the caret has nine stops
    ///   spread across twelve asterisks and points between the wrong ones.
    /// - the width of the row leaks the secret's *encoded* length rather than
    ///   its typed length, which for a non-Latin password narrows it far more
    ///   than a character count does. Masking exists to stop exactly that.
    ///
    /// **A failure here counting more marks than characters is that bug back.**
    #[test]
    fn the_mask_has_one_mark_per_character_not_per_byte() {
        // 8 characters; 16 bytes — the old code drew exactly twice as many
        // marks as the user typed. ASCII, Latin-1, Greek, Hebrew, CJK, emoji:
        // one, two, two, two, three and four bytes respectively.
        let secret = "ab\u{00E9}\u{03B1}\u{05D0}\u{4E2D}\u{1F600}c";
        assert_eq!(secret.chars().count(), 8);
        assert_eq!(secret.len(), 16);

        let mut dialog = InputDialog::prompt("Test", "Password:", "")
            .with_password_mode(true)
            .with_initial_text(secret);
        dialog.show();
        dialog.overlay.opacity = 1.0;

        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);
        // `RichText`, not `Text`: the field's contents are drawn by
        // `textedit::draw`, which colours the selection per glyph.
        let masks: Vec<&String> = tree
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::RichText { text, .. } if text.starts_with('*') => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(masks.len(), 1, "the field is drawn once");
        assert_eq!(masks[0].len(), 8, "one mark per character, not per byte");
        assert!(
            !masks[0].contains(|c| c != '*'),
            "nothing of the secret itself is drawn"
        );
    }

    // --- InputDialog caret, selection and scrolling ---
    //
    // The dialog tracked a caret from the day it was written and never drew
    // one, so every test above could assert where the caret *went* and none
    // could assert where it is *shown* -- see `known-issues.md`,
    // TD-C-TWO-TOOLKIT-TEXT-FIELDS-DRAW-NO-CARET-AT-ALL. These are the tests
    // that can.

    /// Every vertical `Line` in a rendered dialog, by x. The caret is the only
    /// thing in this dialog drawn as a zero-width line.
    fn caret_xs(dialog: &InputDialog) -> Vec<f32> {
        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);
        tree.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Line { x1, x2, .. } if (x1 - x2).abs() < f32::EPSILON => Some(*x1),
                _ => None,
            })
            .collect()
    }

    /// The dialog, shown and fully faded in, ready to render.
    fn opened(mut dialog: InputDialog) -> InputDialog {
        dialog.show();
        dialog.overlay.opacity = 1.0;
        dialog
    }

    fn shifted(k: Key, shift: bool) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers {
                shift,
                ..Modifiers::NONE
            },
            text: None,
        })
    }

    #[test]
    fn the_field_draws_a_caret_when_it_has_the_focus_and_not_when_it_does_not() {
        let dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text("abc"));
        assert_eq!(
            caret_xs(&dialog).len(),
            1,
            "the focused field must show where the next character will go"
        );

        // Tab to the OK button. A caret still drawn here would say the typing
        // goes to the text field when Space would in fact press the button.
        let mut moved = dialog;
        moved.handle_event(&shifted(Key::Tab, false));
        assert_eq!(moved.focused_element, InputFocus::OkButton);
        assert!(
            caret_xs(&moved).is_empty(),
            "an unfocused field must not draw a caret"
        );
    }

    #[test]
    fn an_empty_input_dialog_still_shows_where_typing_will_land() {
        // The empty field draws its placeholder, which is not editable text --
        // but the caret still belongs at the left, or a user cannot tell a
        // ready field from a dead one.
        let dialog = opened(InputDialog::prompt("T", "P:", "type here"));
        assert_eq!(caret_xs(&dialog).len(), 1);
    }

    #[test]
    fn the_input_dialogs_drawn_caret_follows_the_arrow_keys() {
        // The point of the whole exercise: the caret the dialog tracks and the
        // caret it draws have to be the same caret.
        let mut dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text("abcdef"));
        let at_end = caret_xs(&dialog)[0];

        dialog.handle_event(&shifted(Key::Home, false));
        let at_start = caret_xs(&dialog)[0];
        assert!(
            at_start < at_end,
            "Home must move the drawn caret left of where End leaves it, got {at_start} then {at_end}"
        );

        dialog.handle_event(&shifted(Key::Right, false));
        let after_one = caret_xs(&dialog)[0];
        assert!(
            after_one > at_start,
            "one Right must move the drawn caret rightwards in left-to-right text"
        );
    }

    #[test]
    fn shift_and_an_arrow_select_in_the_input_dialog_and_a_bare_arrow_gives_it_up() {
        let mut dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text("abcdef"));
        assert_eq!(dialog.selection_anchor, None);

        dialog.handle_event(&shifted(Key::Left, true));
        dialog.handle_event(&shifted(Key::Left, true));
        assert_eq!(
            dialog.selection_anchor,
            Some(6),
            "the anchor stays where the selection began, not where the caret is now"
        );
        assert_eq!(dialog.cursor.byte(), 4);

        dialog.handle_event(&shifted(Key::Left, false));
        assert_eq!(
            dialog.selection_anchor, None,
            "a bare arrow means 'put the caret here and forget the selection'"
        );
    }

    #[test]
    fn an_input_dialogs_selection_is_painted_behind_the_text() {
        let mut dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text("abcdef"));
        dialog.handle_event(&shifted(Key::Home, false));
        dialog.handle_event(&shifted(Key::Right, true));
        dialog.handle_event(&shifted(Key::Right, true));

        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);
        let painted = tree.commands.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { color, .. }
                if *color == crate::textedit::SELECTION_BACKGROUND)
        });
        assert!(painted, "a selection nobody can see is not a selection");
    }

    #[test]
    fn typing_over_an_input_dialogs_selection_replaces_it() {
        let mut dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text("abcdef"));
        dialog.handle_event(&shifted(Key::Home, false));
        for _ in 0..3 {
            dialog.handle_event(&shifted(Key::Right, true));
        }
        dialog.handle_event(&Event::Key(KeyEvent {
            key: Key::Z,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('Z'),
        }));
        assert_eq!(dialog.input_text, "Zdef");
        assert_eq!(dialog.cursor.byte(), 1);
        assert_eq!(dialog.selection_anchor, None);
    }

    #[test]
    fn backspace_over_an_input_dialogs_selection_takes_it_and_nothing_more() {
        // The bug this guards is an off-by-one that is easy to write and hard
        // to see: deleting the selection *and then* the character before it.
        let mut dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text("abcdef"));
        for _ in 0..2 {
            dialog.handle_event(&shifted(Key::Left, true));
        }
        dialog.handle_event(&shifted(Key::Backspace, false));
        assert_eq!(dialog.input_text, "abcd");
        assert_eq!(dialog.cursor.byte(), 4);

        // And with no selection it still deletes exactly one character.
        dialog.handle_event(&shifted(Key::Backspace, false));
        assert_eq!(dialog.input_text, "abc");
    }

    #[test]
    fn delete_over_a_selection_takes_the_selection_rather_than_the_next_character() {
        let mut dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text("abcdef"));
        dialog.handle_event(&shifted(Key::Home, false));
        for _ in 0..2 {
            dialog.handle_event(&shifted(Key::Right, true));
        }
        dialog.handle_event(&shifted(Key::Delete, false));
        assert_eq!(dialog.input_text, "cdef");
        assert_eq!(dialog.cursor.byte(), 0);
    }

    #[test]
    fn an_input_dialog_selection_spanning_a_multi_byte_character_is_cut_on_a_boundary() {
        // `String::drain` panics on an offset inside a character, and a panic
        // in a dialog takes the application down over a keystroke.
        let mut dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text("aébc"));
        dialog.handle_event(&shifted(Key::Home, false));
        for _ in 0..2 {
            dialog.handle_event(&shifted(Key::Right, true));
        }
        dialog.handle_event(&shifted(Key::Backspace, false));
        assert_eq!(dialog.input_text, "bc");
    }

    #[test]
    fn replacing_the_text_from_code_drops_a_selection_that_named_the_old_text() {
        // The anchor is an offset into a string that no longer exists; left
        // behind it can point past the end of the new one, and the next
        // backspace would try to cut a range that is not there.
        let mut dialog =
            opened(InputDialog::prompt("T", "P:", "").with_initial_text("a long value"));
        for _ in 0..4 {
            dialog.handle_event(&shifted(Key::Left, true));
        }
        assert!(dialog.selection_anchor.is_some());
        dialog.set_input_text("hi");
        assert_eq!(dialog.selection_anchor, None);
        assert_eq!(dialog.cursor.byte(), 2);
    }

    #[test]
    fn a_password_selection_is_measured_in_marks_and_not_in_bytes() {
        // The mask is one byte per character, the secret is not, so a selection
        // carried over unconverted would highlight two marks for an accented
        // letter -- redrawing exactly the byte-length leak the masking exists
        // to prevent, this time in the shape of the highlight.
        let secret = "aébc"; // 4 characters, 5 bytes
        let dialog = InputDialog::prompt("T", "P:", "")
            .with_password_mode(true)
            .with_initial_text(secret);
        assert_eq!(dialog.drawn_offset(secret.len()), 4);
        assert_eq!(dialog.drawn_offset(3), 2, "'a' and 'é' are two marks");
        assert_eq!(dialog.drawn_offset(0), 0);
    }

    #[test]
    fn an_input_dialog_longer_than_its_box_scrolls_to_keep_the_caret_in_view() {
        // Without a scroll offset the caret is painted past the right edge of
        // the field, over whatever is beside it.
        let long = "x".repeat(400);
        let dialog = opened(InputDialog::prompt("T", "P:", "").with_initial_text(&long));
        let caret = caret_xs(&dialog)[0];

        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);
        let clip = tree.commands.iter().find_map(|c| match c {
            RenderCommand::PushClip {
                x, width, height, ..
            } if *height <= FONT_SIZE => Some((*x, *width)),
            _ => None,
        });
        let (clip_x, clip_w) = clip.expect("the field's text must be clipped to the field");
        assert!(
            caret >= clip_x && caret <= clip_x + clip_w,
            "the caret at {caret} must be inside the field's box {clip_x}..{}",
            clip_x + clip_w
        );
    }

    #[test]
    fn test_input_dialog_render() {
        let mut dialog = InputDialog::prompt("Name", "Enter name:", "placeholder");
        dialog.show();
        dialog.overlay.opacity = 1.0;

        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);
        assert!(tree.len() > 5);
    }

    // --- ProgressDialog tests ---

    #[test]
    fn test_progress_indeterminate_creation() {
        let dialog = ProgressDialog::indeterminate("Loading", "Please wait...");
        assert_eq!(dialog.title, "Loading");
        assert_eq!(dialog.status_text, "Please wait...");
        assert_eq!(dialog.progress, ProgressMode::Indeterminate);
        assert!(!dialog.cancelable);
    }

    #[test]
    fn test_progress_determinate_creation() {
        let dialog = ProgressDialog::determinate("Downloading", "0%");
        assert_eq!(dialog.progress, ProgressMode::Determinate(0.0));
    }

    #[test]
    fn test_progress_set_progress() {
        let mut dialog = ProgressDialog::determinate("Test", "Status");
        dialog.set_progress(0.5);
        assert_eq!(dialog.progress(), ProgressMode::Determinate(0.5));

        // Clamps to 0..1.
        dialog.set_progress(1.5);
        assert_eq!(dialog.progress(), ProgressMode::Determinate(1.0));
        dialog.set_progress(-0.5);
        assert_eq!(dialog.progress(), ProgressMode::Determinate(0.0));
    }

    #[test]
    fn test_progress_set_status() {
        let mut dialog = ProgressDialog::indeterminate("Test", "Initial");
        dialog.set_status("Updated");
        assert_eq!(dialog.status_text, "Updated");
    }

    #[test]
    fn test_progress_set_detail() {
        let mut dialog = ProgressDialog::indeterminate("Test", "Status").with_detail("Detail line");
        assert_eq!(dialog.detail_text, Some(String::from("Detail line")));
        assert!(dialog.show_detail);

        dialog.set_detail(Some("New detail"));
        assert_eq!(dialog.detail_text, Some(String::from("New detail")));

        dialog.set_detail(None);
        assert_eq!(dialog.detail_text, None);
    }

    #[test]
    fn test_progress_cancelable() {
        let mut dialog = ProgressDialog::indeterminate("Test", "Status").with_cancel();
        assert!(dialog.cancelable);
        dialog.show();

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&esc);
        assert!(dialog.is_cancelled());
    }

    #[test]
    fn test_progress_not_cancelable_ignores_escape() {
        let mut dialog = ProgressDialog::indeterminate("Test", "Status");
        dialog.show();

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&esc);
        assert!(!dialog.is_cancelled());
    }

    #[test]
    fn test_progress_tick_advances_animation() {
        let mut dialog = ProgressDialog::indeterminate("Test", "Status");
        dialog.show();
        let initial = dialog.anim_tick;
        dialog.tick(16);
        assert_eq!(dialog.anim_tick, initial + 16);
    }

    #[test]
    fn test_progress_toggle_detail() {
        let mut dialog = ProgressDialog::indeterminate("Test", "Status").with_detail("Detail");
        assert!(dialog.show_detail);
        dialog.toggle_detail();
        assert!(!dialog.show_detail);
        dialog.toggle_detail();
        assert!(dialog.show_detail);
    }

    #[test]
    fn test_progress_render() {
        let mut dialog = ProgressDialog::determinate("Downloading", "50%");
        dialog.show();
        dialog.overlay.opacity = 1.0;
        dialog.set_progress(0.5);

        let mut tree = RenderTree::new();
        dialog.render(800.0, 600.0, &mut tree);
        assert!(tree.len() > 5);
    }

    // --- NonModalDialog tests ---

    #[test]
    fn test_nonmodal_creation() {
        let dialog = NonModalDialog::new("Properties");
        assert_eq!(dialog.title, "Properties");
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_nonmodal_show_hide() {
        let mut dialog = NonModalDialog::new("Test");
        dialog.show();
        assert!(dialog.is_visible());
        dialog.hide();
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_nonmodal_position_and_size() {
        let dialog = NonModalDialog::new("Test")
            .with_position(50.0, 75.0)
            .with_size(300.0, 250.0);
        assert_eq!(dialog.position(), (50.0, 75.0));
        assert_eq!(dialog.size(), (300.0, 250.0));
    }

    #[test]
    fn test_nonmodal_size_clamped() {
        let dialog = NonModalDialog::new("Test")
            .with_min_size(100.0, 80.0)
            .with_max_size(500.0, 400.0)
            .with_size(50.0, 50.0); // Below min.
        assert_eq!(dialog.size(), (100.0, 80.0));
    }

    #[test]
    fn test_nonmodal_center_in() {
        let mut dialog = NonModalDialog::new("Test").with_size(200.0, 150.0);
        dialog.center_in(800.0, 600.0);
        assert_eq!(dialog.position(), (300.0, 225.0));
    }

    #[test]
    fn test_nonmodal_drag() {
        let mut dialog = NonModalDialog::new("Test")
            .with_position(100.0, 100.0)
            .with_size(300.0, 200.0);
        dialog.show();

        // Press in title bar.
        let press = Event::Mouse(MouseEvent {
            x: 150.0,
            y: 110.0, // Within title bar (y=100 to y=144).
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        let result = dialog.handle_event(&press);
        assert_eq!(result, EventResult::Consumed);
        assert!(dialog.dragging);

        // Move.
        let move_event = Event::Mouse(MouseEvent {
            x: 200.0,
            y: 160.0,
            kind: MouseEventKind::Move,
        });
        dialog.handle_event(&move_event);
        // Dialog should have moved.
        assert_ne!(dialog.position(), (100.0, 100.0));

        // Release.
        let release = Event::Mouse(MouseEvent {
            x: 200.0,
            y: 160.0,
            kind: MouseEventKind::Release(MouseButton::Left),
        });
        dialog.handle_event(&release);
        assert!(!dialog.dragging);
    }

    #[test]
    fn test_nonmodal_close_button() {
        let mut dialog = NonModalDialog::new("Test")
            .with_position(0.0, 0.0)
            .with_size(400.0, 300.0);
        dialog.show();

        // Click close button (top-right area).
        let close_x = 400.0 - CONTENT_PADDING - CLOSE_BUTTON_SIZE + CLOSE_BUTTON_SIZE / 2.0;
        let close_y = (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0 + CLOSE_BUTTON_SIZE / 2.0;
        let press = Event::Mouse(MouseEvent {
            x: close_x,
            y: close_y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        dialog.handle_event(&press);
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_nonmodal_resize() {
        let mut dialog = NonModalDialog::new("Test")
            .with_position(0.0, 0.0)
            .with_size(400.0, 300.0)
            .with_resizable(true);
        dialog.show();

        // Press in bottom-right corner (resize handle).
        let press = Event::Mouse(MouseEvent {
            x: 396.0,
            y: 296.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        dialog.handle_event(&press);
        assert!(dialog.resizing);

        // Drag to new size.
        let move_event = Event::Mouse(MouseEvent {
            x: 500.0,
            y: 400.0,
            kind: MouseEventKind::Move,
        });
        dialog.handle_event(&move_event);
        assert_eq!(dialog.size(), (500.0, 400.0));

        // Release.
        let release = Event::Mouse(MouseEvent {
            x: 500.0,
            y: 400.0,
            kind: MouseEventKind::Release(MouseButton::Left),
        });
        dialog.handle_event(&release);
        assert!(!dialog.resizing);
    }

    #[test]
    fn test_nonmodal_click_outside_ignored() {
        let mut dialog = NonModalDialog::new("Test")
            .with_position(100.0, 100.0)
            .with_size(200.0, 150.0);
        dialog.show();

        // Click outside the dialog bounds.
        let press = Event::Mouse(MouseEvent {
            x: 50.0,
            y: 50.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        let result = dialog.handle_event(&press);
        // Non-modal dialogs don't consume clicks outside.
        assert_eq!(result, EventResult::Ignored);
        assert!(dialog.is_visible()); // Still visible.
    }

    #[test]
    fn test_nonmodal_set_content() {
        let mut dialog = NonModalDialog::new("Test");
        let content = vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            color: COLOR_BLUE,
            corner_radii: CornerRadii::ZERO,
        }];
        dialog.set_content(content);
        assert_eq!(dialog.content_commands.len(), 1);
    }

    #[test]
    fn test_nonmodal_render() {
        let mut dialog = NonModalDialog::new("Test Dialog")
            .with_position(50.0, 50.0)
            .with_size(300.0, 200.0);
        dialog.show();

        let mut tree = RenderTree::new();
        dialog.render(&mut tree);

        // Should have shadow, bg, title bar, title text, close button.
        assert!(tree.len() >= 5);
    }

    #[test]
    fn test_nonmodal_render_hidden_is_empty() {
        let dialog = NonModalDialog::new("Test");
        let mut tree = RenderTree::new();
        dialog.render(&mut tree);
        assert!(tree.is_empty());
    }

    // --- Helper function tests ---

    #[test]
    fn test_point_in_rect() {
        assert!(point_in_rect(5.0, 5.0, 0.0, 0.0, 10.0, 10.0));
        assert!(point_in_rect(0.0, 0.0, 0.0, 0.0, 10.0, 10.0)); // Edge.
        assert!(point_in_rect(10.0, 10.0, 0.0, 0.0, 10.0, 10.0)); // Edge.
        assert!(!point_in_rect(11.0, 5.0, 0.0, 0.0, 10.0, 10.0)); // Outside.
        assert!(!point_in_rect(5.0, 11.0, 0.0, 0.0, 10.0, 10.0)); // Outside.
        assert!(!point_in_rect(-1.0, 5.0, 0.0, 0.0, 10.0, 10.0)); // Outside.
    }

    // --- Icon tests ---

    #[test]
    fn test_dialog_icon_glyphs() {
        assert_eq!(DialogIcon::Info.glyph(), Some("i"));
        assert_eq!(DialogIcon::Warning.glyph(), Some("!"));
        assert_eq!(DialogIcon::Error.glyph(), Some("X"));
        assert_eq!(DialogIcon::None.glyph(), None);
    }

    #[test]
    fn test_dialog_icon_colors_distinct() {
        assert_ne!(DialogIcon::Info.color(), DialogIcon::Warning.color());
        assert_ne!(DialogIcon::Warning.color(), DialogIcon::Error.color());
    }
}
