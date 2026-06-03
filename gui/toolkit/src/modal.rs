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
use crate::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::render::{FontWeightHint, RenderCommand, RenderTree};
use crate::style::CornerRadii;

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
        Self { buttons: vec![DialogButton::Ok] }
    }

    /// OK and Cancel buttons.
    pub fn ok_cancel() -> Self {
        Self { buttons: vec![DialogButton::Ok, DialogButton::Cancel] }
    }

    /// Yes and No buttons.
    pub fn yes_no() -> Self {
        Self { buttons: vec![DialogButton::Yes, DialogButton::No] }
    }

    /// Yes, No, and Cancel buttons.
    pub fn yes_no_cancel() -> Self {
        Self { buttons: vec![DialogButton::Yes, DialogButton::No, DialogButton::Cancel] }
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
            && self.dismiss_on_click_outside && !self.point_in_content(event.x, event.y) {
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
                if !self.buttons.is_empty() {
                    if event.modifiers.shift {
                        self.focused_button = if self.focused_button == 0 {
                            self.buttons.len() - 1
                        } else {
                            self.focused_button - 1
                        };
                    } else {
                        self.focused_button = (self.focused_button + 1) % self.buttons.len();
                    }
                }
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
                if point_in_rect(event.x, event.y, btn_rect.0, btn_rect.1, btn_rect.2, btn_rect.3)
                    && let Some(btn) = self.buttons.buttons.get(i) {
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
            });

            text_x = icon_x + ICON_SIZE + ICON_PADDING;
        }

        // Message text.
        let text_max_width = layout.x + layout.width - text_x - CONTENT_PADDING;
        tree.push(RenderCommand::Text {
            x: text_x,
            y: content_y + (ICON_SIZE - FONT_SIZE) / 2.0,
            text: self.message.clone(),
            color: COLOR_SUBTEXT1,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(text_max_width),
        });

        // Buttons (bottom-right aligned).
        let buttons_y = layout.y + layout.height - BUTTON_HEIGHT - CONTENT_PADDING;
        self.render_buttons(tree, &layout, buttons_y);
    }

    /// Render the button row.
    fn render_buttons(&self, tree: &mut RenderTree, layout: &DialogLayout, y: f32) {
        let total_width: f32 = self.buttons.buttons.iter().map(|_| BUTTON_MIN_WIDTH).sum::<f32>()
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
            let text_color = if btn.is_primary() { COLOR_CRUST } else { COLOR_TEXT };
            tree.push(RenderCommand::Text {
                x: btn_x + (BUTTON_MIN_WIDTH - (label.len() as f32 * 7.0)) / 2.0,
                y: y + (BUTTON_HEIGHT - FONT_SIZE) / 2.0,
                text: label.to_string(),
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    /// Compute dialog layout (position and size, centered in parent).
    fn compute_layout(&self, parent_width: f32, parent_height: f32) -> DialogLayout {
        let width = self.width.unwrap_or(DIALOG_MIN_WIDTH).clamp(DIALOG_MIN_WIDTH, DIALOG_MAX_WIDTH);
        let height = self.compute_height();
        let x = (parent_width - width) / 2.0;
        let y = (parent_height - height) / 2.0;

        // Compute button rects for hit testing.
        let buttons_y = y + height - BUTTON_HEIGHT - CONTENT_PADDING;
        let total_btn_width: f32 = self.buttons.buttons.iter().map(|_| BUTTON_MIN_WIDTH).sum::<f32>()
            + (self.buttons.len().saturating_sub(1) as f32) * BUTTON_SPACING;
        let start_x = x + width - CONTENT_PADDING - total_btn_width;

        let button_rects: Vec<(f32, f32, f32, f32)> = (0..self.buttons.len())
            .map(|i| {
                let bx = start_x + (i as f32) * (BUTTON_MIN_WIDTH + BUTTON_SPACING);
                (bx, buttons_y, BUTTON_MIN_WIDTH, BUTTON_HEIGHT)
            })
            .collect();

        DialogLayout { x, y, width, height, button_rects }
    }

    /// Compute the height needed for the dialog content.
    fn compute_height(&self) -> f32 {
        // Title bar + content padding + icon/message area + padding + buttons + padding
        let content_height = ICON_SIZE.max(FONT_SIZE * 3.0); // Estimate message height
        (TITLE_BAR_HEIGHT + CONTENT_PADDING + content_height + CONTENT_PADDING + BUTTON_HEIGHT + CONTENT_PADDING)
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
    cursor_pos: usize,
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
            cursor_pos: 0,
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
        self.cursor_pos = text.len();
        self
    }

    /// Show the dialog.
    pub fn show(&mut self) {
        self.result = None;
        self.validation_error = None;
        self.focused_element = InputFocus::TextField;
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
        self.cursor_pos = text.len();
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
            InputFocus::TextField => { self.handle_text_input(event); },
            InputFocus::OkButton | InputFocus::CancelButton => {
                match event.key {
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
                }
            }
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
            Key::Backspace => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.input_text.remove(self.cursor_pos);
                    self.validation_error = None;
                }
            }
            Key::Delete => {
                if self.cursor_pos < self.input_text.len() {
                    self.input_text.remove(self.cursor_pos);
                    self.validation_error = None;
                }
            }
            Key::Left => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                }
            }
            Key::Right => {
                if self.cursor_pos < self.input_text.len() {
                    self.cursor_pos += 1;
                }
            }
            Key::Home => {
                self.cursor_pos = 0;
            }
            Key::End => {
                self.cursor_pos = self.input_text.len();
            }
            _ => {
                if let Some(ch) = event.text
                    && !ch.is_control() {
                        self.input_text.insert(self.cursor_pos, ch);
                        self.cursor_pos += 1;
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

    /// Render the input dialog.
    pub fn render(&self, parent_width: f32, parent_height: f32, tree: &mut RenderTree) {
        if !self.overlay.active && self.overlay.opacity <= 0.0 {
            return;
        }

        self.overlay.render(parent_width, parent_height, tree);

        let width = DIALOG_MIN_WIDTH + 80.0;
        let has_error = self.validation_error.is_some();
        let height = TITLE_BAR_HEIGHT + CONTENT_PADDING + FONT_SIZE + 12.0
            + INPUT_HEIGHT + (if has_error { FONT_SIZE_SMALL + 8.0 } else { 0.0 })
            + CONTENT_PADDING + BUTTON_HEIGHT + CONTENT_PADDING;
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
        });

        // Message.
        let mut content_y = y + TITLE_BAR_HEIGHT + CONTENT_PADDING;
        tree.push(RenderCommand::Text {
            x: x + CONTENT_PADDING,
            y: content_y,
            text: self.message.clone(),
            color: COLOR_SUBTEXT1,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - CONTENT_PADDING * 2.0),
        });
        content_y += FONT_SIZE + 12.0;

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
        let display_text = if self.input_text.is_empty() {
            self.placeholder.clone()
        } else if self.password_mode {
            "*".repeat(self.input_text.len())
        } else {
            self.input_text.clone()
        };
        let text_color = if self.input_text.is_empty() {
            COLOR_OVERLAY0
        } else {
            COLOR_TEXT
        };
        tree.push(RenderCommand::Text {
            x: x + CONTENT_PADDING + 10.0,
            y: content_y + (INPUT_HEIGHT - FONT_SIZE) / 2.0,
            text: display_text,
            color: text_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(input_width - 20.0),
        });

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
        let cancel_height = if self.cancelable { BUTTON_HEIGHT + CONTENT_PADDING } else { 0.0 };
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
            });
        }

        // Detail text.
        if self.show_detail
            && let Some(ref detail) = self.detail_text {
                tree.push(RenderCommand::Text {
                    x: x + CONTENT_PADDING,
                    y: content_y,
                    text: detail.clone(),
                    color: COLOR_OVERLAY0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(bar_width),
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
                if point_in_rect(event.x, event.y, close_x, close_y, CLOSE_BUTTON_SIZE, CLOSE_BUTTON_SIZE) {
                    self.hide();
                    return EventResult::Consumed;
                }

                // Check title bar drag.
                if point_in_rect(event.x, event.y, self.x, self.y, self.width, TITLE_BAR_HEIGHT) {
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
                    event.x, event.y, close_x, close_y, CLOSE_BUTTON_SIZE, CLOSE_BUTTON_SIZE,
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
        });

        // Close button (X).
        let close_x = self.x + self.width - CONTENT_PADDING - CLOSE_BUTTON_SIZE;
        let close_y = self.y + (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0;
        let close_bg = if self.close_hovered { COLOR_SURFACE2 } else { COLOR_SURFACE0 };
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
            color: if self.close_hovered { COLOR_RED } else { COLOR_OVERLAY1 },
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
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
    use super::*;

    // --- DialogResult tests ---

    #[test]
    fn test_dialog_result_variants() {
        assert_eq!(DialogResult::Ok, DialogResult::Ok);
        assert_ne!(DialogResult::Ok, DialogResult::Cancel);
        assert_eq!(DialogResult::Text(String::from("hello")), DialogResult::Text(String::from("hello")));
        assert_ne!(DialogResult::Text(String::from("a")), DialogResult::Text(String::from("b")));
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
        let has_fill = tree.commands.iter().any(|cmd| matches!(cmd, RenderCommand::FillRect { .. }));
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

    #[test]
    fn test_alert_builder_custom_buttons() {
        let dialog = AlertDialog::info("Test", "Test")
            .with_buttons(ButtonSet::custom(vec![DialogButton::No, DialogButton::Yes]));
        assert_eq!(dialog.buttons.len(), 2);
        assert_eq!(dialog.buttons.buttons[0], DialogButton::No);
    }

    #[test]
    fn test_alert_builder_custom_icon() {
        let dialog = AlertDialog::info("Test", "Test")
            .with_icon(DialogIcon::Error);
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
        let dialog = InputDialog::prompt("Password", "Enter:", "")
            .with_password_mode(true);
        assert!(dialog.password_mode);
    }

    #[test]
    fn test_input_dialog_initial_text() {
        let dialog = InputDialog::prompt("Edit", "Edit value:", "")
            .with_initial_text("hello");
        assert_eq!(dialog.input_text(), "hello");
        assert_eq!(dialog.cursor_pos, 5);
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
        let mut dialog = InputDialog::prompt("Test", "Type:", "")
            .with_initial_text("hello");
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
        let mut dialog = InputDialog::prompt("Test", "Type:", "")
            .with_initial_text("hello");
        dialog.show();
        dialog.cursor_pos = 0;

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
        let mut dialog = InputDialog::prompt("Test", "Type:", "")
            .with_initial_text("hello");
        dialog.show();
        assert_eq!(dialog.cursor_pos, 5);

        // Left.
        let left = Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&left);
        assert_eq!(dialog.cursor_pos, 4);

        // Home.
        let home = Event::Key(KeyEvent {
            key: Key::Home,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&home);
        assert_eq!(dialog.cursor_pos, 0);

        // End.
        let end = Event::Key(KeyEvent {
            key: Key::End,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&end);
        assert_eq!(dialog.cursor_pos, 5);
    }

    #[test]
    fn test_input_dialog_enter_accepts() {
        let mut dialog = InputDialog::prompt("Test", "Type:", "")
            .with_initial_text("result");
        dialog.show();

        let enter = Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        dialog.handle_event(&enter);
        assert_eq!(dialog.result(), Some(&DialogResult::Text(String::from("result"))));
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
        let mut dialog = ProgressDialog::indeterminate("Test", "Status")
            .with_detail("Detail line");
        assert_eq!(dialog.detail_text, Some(String::from("Detail line")));
        assert!(dialog.show_detail);

        dialog.set_detail(Some("New detail"));
        assert_eq!(dialog.detail_text, Some(String::from("New detail")));

        dialog.set_detail(None);
        assert_eq!(dialog.detail_text, None);
    }

    #[test]
    fn test_progress_cancelable() {
        let mut dialog = ProgressDialog::indeterminate("Test", "Status")
            .with_cancel();
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
        let mut dialog = ProgressDialog::indeterminate("Test", "Status")
            .with_detail("Detail");
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
        let mut dialog = NonModalDialog::new("Test")
            .with_size(200.0, 150.0);
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
        let content = vec![
            RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                color: COLOR_BLUE,
                corner_radii: CornerRadii::ZERO,
            },
        ];
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
