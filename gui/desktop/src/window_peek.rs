//! Window peek / live preview module for the taskbar.
//!
//! When the user hovers over a taskbar button, this module renders a popup
//! showing thumbnail previews of all windows belonging to that application.
//! Features:
//!
//! - Scaled-down window previews (proportional to actual window size)
//! - Window title below each thumbnail
//! - Close button (X) on hover per thumbnail
//! - Side-by-side layout for grouped windows
//! - Hover highlight and selection
//! - Smooth fade-in/fade-out animation
//! - Configurable hover delay before popup appears
//! - Auto-dismiss when mouse leaves the popup and taskbar button

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_SURFACE2: Color = Color::from_hex(0x585B70);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);

// ============================================================================
// Configuration
// ============================================================================

/// How long (in ms) the mouse must hover before the peek popup appears.
const DEFAULT_HOVER_DELAY_MS: u64 = 400;
/// Maximum thumbnail width in pixels.
const MAX_THUMBNAIL_WIDTH: f32 = 200.0;
/// Maximum thumbnail height in pixels.
const MAX_THUMBNAIL_HEIGHT: f32 = 140.0;
/// Minimum thumbnail width.
const MIN_THUMBNAIL_WIDTH: f32 = 120.0;
/// Padding around each thumbnail.
const THUMBNAIL_PADDING: f32 = 8.0;
/// Gap between thumbnails.
const THUMBNAIL_GAP: f32 = 12.0;
/// Height of the title text area below each thumbnail.
const TITLE_HEIGHT: f32 = 24.0;
/// Close button size (square).
const CLOSE_BUTTON_SIZE: f32 = 18.0;
/// Popup corner radius.
const POPUP_RADIUS: f32 = 8.0;
/// Popup shadow offset.
const SHADOW_OFFSET: f32 = 4.0;
/// Animation duration in ms.
const FADE_DURATION_MS: u64 = 150;

/// Configuration for the window peek feature.
#[derive(Clone, Debug)]
pub struct PeekConfig {
    /// Delay in ms before showing the popup after hover begins.
    pub hover_delay_ms: u64,
    /// Maximum width per thumbnail.
    pub max_thumb_width: f32,
    /// Maximum height per thumbnail.
    pub max_thumb_height: f32,
    /// Whether to show close buttons on thumbnails.
    pub show_close_buttons: bool,
    /// Whether the peek feature is enabled.
    pub enabled: bool,
    /// Animation duration in ms.
    pub fade_duration_ms: u64,
}

impl Default for PeekConfig {
    fn default() -> Self {
        Self {
            hover_delay_ms: DEFAULT_HOVER_DELAY_MS,
            max_thumb_width: MAX_THUMBNAIL_WIDTH,
            max_thumb_height: MAX_THUMBNAIL_HEIGHT,
            show_close_buttons: true,
            enabled: true,
            fade_duration_ms: FADE_DURATION_MS,
        }
    }
}

impl PeekConfig {
    /// Parse config from key=value text lines.
    pub fn from_text(text: &str) -> Self {
        let mut config = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "hover_delay_ms" => {
                        if let Ok(v) = val.parse::<u64>() {
                            config.hover_delay_ms = v;
                        }
                    }
                    "max_thumb_width" => {
                        if let Ok(v) = val.parse::<f32>() {
                            config.max_thumb_width = v.max(60.0);
                        }
                    }
                    "max_thumb_height" => {
                        if let Ok(v) = val.parse::<f32>() {
                            config.max_thumb_height = v.max(40.0);
                        }
                    }
                    "show_close_buttons" => {
                        config.show_close_buttons = val == "true";
                    }
                    "enabled" => {
                        config.enabled = val == "true";
                    }
                    "fade_duration_ms" => {
                        if let Ok(v) = val.parse::<u64>() {
                            config.fade_duration_ms = v;
                        }
                    }
                    _ => {}
                }
            }
        }
        config
    }

    /// Serialize config to key=value text.
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("# Window peek configuration\n");
        out.push_str(&format!("hover_delay_ms={}\n", self.hover_delay_ms));
        out.push_str(&format!("max_thumb_width={}\n", self.max_thumb_width));
        out.push_str(&format!("max_thumb_height={}\n", self.max_thumb_height));
        out.push_str(&format!(
            "show_close_buttons={}\n",
            self.show_close_buttons
        ));
        out.push_str(&format!("enabled={}\n", self.enabled));
        out.push_str(&format!("fade_duration_ms={}\n", self.fade_duration_ms));
        out
    }
}

// ============================================================================
// Window snapshot — the data we need to render a preview
// ============================================================================

/// A snapshot of a window's state used for rendering the peek preview.
#[derive(Clone, Debug)]
pub struct WindowSnapshot {
    /// Unique window ID.
    pub window_id: u64,
    /// Application ID this window belongs to.
    pub app_id: String,
    /// Window title.
    pub title: String,
    /// Actual window width.
    pub window_width: f32,
    /// Actual window height.
    pub window_height: f32,
    /// Dominant color of the window content (for placeholder rendering).
    pub dominant_color: Color,
    /// Whether this window is currently focused.
    pub is_focused: bool,
    /// Whether this window is minimized.
    pub is_minimized: bool,
}

impl WindowSnapshot {
    /// Create a new window snapshot.
    pub fn new(
        window_id: u64,
        app_id: &str,
        title: &str,
        width: f32,
        height: f32,
    ) -> Self {
        Self {
            window_id,
            app_id: app_id.to_string(),
            title: title.to_string(),
            window_width: width,
            window_height: height,
            dominant_color: MOCHA_SURFACE1,
            is_focused: false,
            is_minimized: false,
        }
    }

    /// Compute the scaled thumbnail size to fit within max dimensions
    /// while preserving aspect ratio.
    pub fn thumbnail_size(&self, max_w: f32, max_h: f32) -> (f32, f32) {
        if self.window_width <= 0.0 || self.window_height <= 0.0 {
            return (max_w.min(MIN_THUMBNAIL_WIDTH), max_h);
        }
        let scale_x = max_w / self.window_width;
        let scale_y = max_h / self.window_height;
        let scale = scale_x.min(scale_y);
        let w = (self.window_width * scale).max(MIN_THUMBNAIL_WIDTH).min(max_w);
        let h = (self.window_height * scale).min(max_h);
        (w, h)
    }

    /// Truncate title to fit a given width at an approximate character width.
    pub fn display_title(&self, max_chars: usize) -> &str {
        if self.title.len() <= max_chars {
            &self.title
        } else if max_chars >= 3 {
            // Can't return owned string, just truncate at char boundary
            let end = self
                .title
                .char_indices()
                .nth(max_chars.saturating_sub(3))
                .map(|(i, _)| i)
                .unwrap_or(self.title.len());
            &self.title[..end]
        } else {
            &self.title[..self.title.len().min(max_chars)]
        }
    }
}

// ============================================================================
// Thumbnail layout — positioned thumbnails
// ============================================================================

/// A positioned thumbnail in the peek popup.
#[derive(Clone, Debug)]
pub struct ThumbnailSlot {
    /// Index into the snapshots array.
    pub snapshot_index: usize,
    /// Window ID.
    pub window_id: u64,
    /// X position of the thumbnail (relative to popup).
    pub x: f32,
    /// Y position of the thumbnail (relative to popup).
    pub y: f32,
    /// Rendered width.
    pub width: f32,
    /// Rendered height.
    pub height: f32,
}

impl ThumbnailSlot {
    /// Check if a point is inside this thumbnail.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width
            && py >= self.y && py < self.y + self.height + TITLE_HEIGHT
    }

    /// Check if a point is inside the close button area (top-right corner).
    pub fn close_button_hit(&self, px: f32, py: f32) -> bool {
        let bx = self.x + self.width - CLOSE_BUTTON_SIZE - 4.0;
        let by = self.y + 4.0;
        px >= bx && px < bx + CLOSE_BUTTON_SIZE
            && py >= by && py < by + CLOSE_BUTTON_SIZE
    }
}

/// Compute the layout of thumbnails arranged side by side.
pub fn compute_thumbnail_layout(
    snapshots: &[WindowSnapshot],
    config: &PeekConfig,
) -> (Vec<ThumbnailSlot>, f32, f32) {
    if snapshots.is_empty() {
        return (Vec::new(), 0.0, 0.0);
    }

    let mut slots = Vec::with_capacity(snapshots.len());
    let mut cursor_x = THUMBNAIL_PADDING;
    let mut max_height: f32 = 0.0;

    for (i, snap) in snapshots.iter().enumerate() {
        let (tw, th) = snap.thumbnail_size(config.max_thumb_width, config.max_thumb_height);
        slots.push(ThumbnailSlot {
            snapshot_index: i,
            window_id: snap.window_id,
            x: cursor_x,
            y: THUMBNAIL_PADDING,
            width: tw,
            height: th,
        });
        cursor_x += tw + THUMBNAIL_GAP;
        if th > max_height {
            max_height = th;
        }
    }

    // Total popup size
    let total_width = cursor_x - THUMBNAIL_GAP + THUMBNAIL_PADDING;
    let total_height = THUMBNAIL_PADDING + max_height + TITLE_HEIGHT + THUMBNAIL_PADDING;

    (slots, total_width, total_height)
}

// ============================================================================
// Peek popup state
// ============================================================================

/// Current animation phase of the peek popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeekPhase {
    /// Not visible.
    Hidden,
    /// Waiting for hover delay to expire.
    Waiting,
    /// Fading in.
    FadingIn,
    /// Fully visible.
    Visible,
    /// Fading out.
    FadingOut,
}

/// Action requested by the peek popup in response to user interaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeekAction {
    /// Switch focus to this window.
    FocusWindow(u64),
    /// Close this window.
    CloseWindow(u64),
    /// No action.
    None,
}

/// State of the window peek popup.
#[derive(Clone, Debug)]
pub struct PeekPopup {
    /// Current phase.
    pub phase: PeekPhase,
    /// App ID being previewed.
    pub app_id: String,
    /// Window snapshots for the hovered app.
    pub snapshots: Vec<WindowSnapshot>,
    /// Computed thumbnail slots.
    pub slots: Vec<ThumbnailSlot>,
    /// Popup X position (screen coordinates).
    pub popup_x: f32,
    /// Popup Y position (screen coordinates).
    pub popup_y: f32,
    /// Popup width.
    pub popup_width: f32,
    /// Popup height.
    pub popup_height: f32,
    /// Which thumbnail the mouse is hovering over (by slot index).
    pub hovered_slot: Option<usize>,
    /// Whether the mouse is over the close button of the hovered slot.
    pub close_hovered: bool,
    /// Animation progress (0.0 to 1.0).
    pub opacity: f32,
    /// Timestamp when hover began (for delay).
    pub hover_start_ms: u64,
    /// Timestamp when the current animation phase started.
    pub phase_start_ms: u64,
    /// Configuration.
    pub config: PeekConfig,
}

impl PeekPopup {
    /// Create a new hidden popup.
    pub fn new(config: PeekConfig) -> Self {
        Self {
            phase: PeekPhase::Hidden,
            app_id: String::new(),
            snapshots: Vec::new(),
            slots: Vec::new(),
            popup_x: 0.0,
            popup_y: 0.0,
            popup_width: 0.0,
            popup_height: 0.0,
            hovered_slot: None,
            close_hovered: false,
            opacity: 0.0,
            hover_start_ms: 0,
            phase_start_ms: 0,
            config,
        }
    }

    /// Begin tracking a hover over a taskbar button. Call with the app_id,
    /// the button's screen position, and the current timestamp.
    pub fn begin_hover(
        &mut self,
        app_id: &str,
        button_center_x: f32,
        button_top_y: f32,
        snapshots: Vec<WindowSnapshot>,
        now_ms: u64,
    ) {
        if !self.config.enabled || snapshots.is_empty() {
            return;
        }

        // If already showing this app, don't restart
        if self.app_id == app_id
            && (self.phase == PeekPhase::Visible || self.phase == PeekPhase::FadingIn)
        {
            return;
        }

        self.app_id = app_id.to_string();
        self.snapshots = snapshots;

        // Compute layout
        let (slots, width, height) =
            compute_thumbnail_layout(&self.snapshots, &self.config);
        self.slots = slots;
        self.popup_width = width;
        self.popup_height = height;

        // Center popup above the button
        self.popup_x = button_center_x - width / 2.0;
        self.popup_y = button_top_y - height - 8.0;

        self.hovered_slot = None;
        self.close_hovered = false;
        self.hover_start_ms = now_ms;
        self.phase = PeekPhase::Waiting;
        self.phase_start_ms = now_ms;
    }

    /// Cancel the peek (mouse left the button and popup area).
    pub fn cancel(&mut self, now_ms: u64) {
        match self.phase {
            PeekPhase::Hidden => {}
            PeekPhase::Waiting => {
                self.phase = PeekPhase::Hidden;
                self.opacity = 0.0;
            }
            PeekPhase::FadingIn | PeekPhase::Visible => {
                self.phase = PeekPhase::FadingOut;
                self.phase_start_ms = now_ms;
            }
            PeekPhase::FadingOut => {
                // Already fading out, let it finish
            }
        }
    }

    /// Force-hide immediately (e.g., when clicking a window).
    pub fn hide(&mut self) {
        self.phase = PeekPhase::Hidden;
        self.opacity = 0.0;
        self.snapshots.clear();
        self.slots.clear();
        self.hovered_slot = None;
        self.close_hovered = false;
    }

    /// Update animation state. Call each frame with the current timestamp.
    /// Returns true if the popup needs a repaint.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        match self.phase {
            PeekPhase::Hidden => false,
            PeekPhase::Waiting => {
                let elapsed = now_ms.saturating_sub(self.hover_start_ms);
                if elapsed >= self.config.hover_delay_ms {
                    self.phase = PeekPhase::FadingIn;
                    self.phase_start_ms = now_ms;
                    self.opacity = 0.0;
                    true
                } else {
                    false
                }
            }
            PeekPhase::FadingIn => {
                let elapsed = now_ms.saturating_sub(self.phase_start_ms);
                let duration = self.config.fade_duration_ms.max(1);
                let progress = (elapsed as f32) / (duration as f32);
                if progress >= 1.0 {
                    self.opacity = 1.0;
                    self.phase = PeekPhase::Visible;
                } else {
                    self.opacity = progress;
                }
                true
            }
            PeekPhase::Visible => false,
            PeekPhase::FadingOut => {
                let elapsed = now_ms.saturating_sub(self.phase_start_ms);
                let duration = self.config.fade_duration_ms.max(1);
                let progress = (elapsed as f32) / (duration as f32);
                if progress >= 1.0 {
                    self.hide();
                } else {
                    self.opacity = 1.0 - progress;
                }
                true
            }
        }
    }

    /// Handle mouse movement within the popup area. Coordinates are screen-relative.
    /// Returns true if hovered state changed.
    pub fn on_mouse_move(&mut self, screen_x: f32, screen_y: f32) -> bool {
        if self.phase != PeekPhase::Visible && self.phase != PeekPhase::FadingIn {
            return false;
        }

        // Convert to popup-relative coordinates
        let local_x = screen_x - self.popup_x;
        let local_y = screen_y - self.popup_y;

        let old_slot = self.hovered_slot;
        let old_close = self.close_hovered;

        self.hovered_slot = None;
        self.close_hovered = false;

        for (i, slot) in self.slots.iter().enumerate() {
            if slot.contains(local_x, local_y) {
                self.hovered_slot = Some(i);
                if self.config.show_close_buttons {
                    self.close_hovered = slot.close_button_hit(local_x, local_y);
                }
                break;
            }
        }

        self.hovered_slot != old_slot || self.close_hovered != old_close
    }

    /// Handle a click inside the popup. Returns an action to perform.
    pub fn on_click(&mut self, screen_x: f32, screen_y: f32) -> PeekAction {
        if self.phase != PeekPhase::Visible && self.phase != PeekPhase::FadingIn {
            return PeekAction::None;
        }

        let local_x = screen_x - self.popup_x;
        let local_y = screen_y - self.popup_y;

        for slot in &self.slots {
            if slot.contains(local_x, local_y) {
                if self.config.show_close_buttons && slot.close_button_hit(local_x, local_y)
                {
                    return PeekAction::CloseWindow(slot.window_id);
                }
                return PeekAction::FocusWindow(slot.window_id);
            }
        }

        PeekAction::None
    }

    /// Check if a screen point is inside the popup area.
    pub fn contains_point(&self, screen_x: f32, screen_y: f32) -> bool {
        screen_x >= self.popup_x
            && screen_x < self.popup_x + self.popup_width
            && screen_y >= self.popup_y
            && screen_y < self.popup_y + self.popup_height
    }

    /// Whether the popup is currently showing or animating.
    pub fn is_active(&self) -> bool {
        self.phase != PeekPhase::Hidden
    }

    /// Whether the popup is visible enough to render.
    pub fn is_rendering(&self) -> bool {
        matches!(
            self.phase,
            PeekPhase::FadingIn | PeekPhase::Visible | PeekPhase::FadingOut
        )
    }

    /// Render the peek popup to a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.is_rendering() {
            return Vec::new();
        }

        let mut cmds = Vec::with_capacity(self.slots.len() * 6 + 4);
        let alpha = self.opacity;

        // Shadow
        cmds.push(RenderCommand::BoxShadow {
            x: self.popup_x,
            y: self.popup_y,
            width: self.popup_width,
            height: self.popup_height,
            offset_x: SHADOW_OFFSET,
            offset_y: SHADOW_OFFSET,
            blur: 12.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, (80.0 * alpha) as u8),
            corner_radii: CornerRadii::all(POPUP_RADIUS),
        });

        // Background
        let bg_alpha = (230.0 * alpha) as u8;
        cmds.push(RenderCommand::FillRect {
            x: self.popup_x,
            y: self.popup_y,
            width: self.popup_width,
            height: self.popup_height,
            color: Color::rgba(
                MOCHA_BASE.r,
                MOCHA_BASE.g,
                MOCHA_BASE.b,
                bg_alpha,
            ),
            corner_radii: CornerRadii::all(POPUP_RADIUS),
        });

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x: self.popup_x,
            y: self.popup_y,
            width: self.popup_width,
            height: self.popup_height,
            color: Color::rgba(
                MOCHA_SURFACE2.r,
                MOCHA_SURFACE2.g,
                MOCHA_SURFACE2.b,
                (180.0 * alpha) as u8,
            ),
            line_width: 1.0,
            corner_radii: CornerRadii::all(POPUP_RADIUS),
        });

        // Render each thumbnail
        for (i, slot) in self.slots.iter().enumerate() {
            let is_hovered = self.hovered_slot == Some(i);
            self.render_thumbnail(&mut cmds, slot, i, is_hovered, alpha);
        }

        cmds
    }

    /// Render a single thumbnail slot.
    fn render_thumbnail(
        &self,
        cmds: &mut Vec<RenderCommand>,
        slot: &ThumbnailSlot,
        index: usize,
        is_hovered: bool,
        alpha: f32,
    ) {
        let abs_x = self.popup_x + slot.x;
        let abs_y = self.popup_y + slot.y;
        let a = (255.0 * alpha) as u8;

        // Hover highlight background
        if is_hovered {
            cmds.push(RenderCommand::FillRect {
                x: abs_x - 4.0,
                y: abs_y - 4.0,
                width: slot.width + 8.0,
                height: slot.height + TITLE_HEIGHT + 8.0,
                color: Color::rgba(
                    MOCHA_SURFACE0.r,
                    MOCHA_SURFACE0.g,
                    MOCHA_SURFACE0.b,
                    (200.0 * alpha) as u8,
                ),
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Thumbnail background (representing window content)
        let snap = match self.snapshots.get(index) {
            Some(s) => s,
            None => return,
        };

        let content_color = if snap.is_minimized {
            // Dimmer for minimized windows
            MOCHA_SURFACE0
        } else {
            snap.dominant_color
        };

        cmds.push(RenderCommand::FillRect {
            x: abs_x,
            y: abs_y,
            width: slot.width,
            height: slot.height,
            color: Color::rgba(
                content_color.r,
                content_color.g,
                content_color.b,
                a,
            ),
            corner_radii: CornerRadii::all(4.0),
        });

        // Border around thumbnail
        let border_color = if snap.is_focused {
            MOCHA_BLUE
        } else if is_hovered {
            MOCHA_OVERLAY0
        } else {
            MOCHA_SURFACE2
        };

        cmds.push(RenderCommand::StrokeRect {
            x: abs_x,
            y: abs_y,
            width: slot.width,
            height: slot.height,
            color: Color::rgba(
                border_color.r,
                border_color.g,
                border_color.b,
                a,
            ),
            line_width: if snap.is_focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });

        // Minimized indicator
        if snap.is_minimized {
            cmds.push(RenderCommand::Text {
                x: abs_x + slot.width / 2.0 - 30.0,
                y: abs_y + slot.height / 2.0 - 6.0,
                text: "Minimized".to_string(),
                font_size: 11.0,
                color: Color::rgba(
                    MOCHA_SUBTEXT0.r,
                    MOCHA_SUBTEXT0.g,
                    MOCHA_SUBTEXT0.b,
                    a,
                ),
                font_weight: FontWeightHint::Regular,
                max_width: Some(slot.width - 8.0),
            });
        }

        // Title below thumbnail
        let title_y = abs_y + slot.height + 4.0;
        let max_title_chars = (slot.width / 7.0) as usize;
        let display_title = snap.display_title(max_title_chars);

        cmds.push(RenderCommand::Text {
            x: abs_x + 2.0,
            y: title_y,
            text: display_title.to_string(),
            font_size: 11.0,
            color: Color::rgba(
                MOCHA_TEXT.r,
                MOCHA_TEXT.g,
                MOCHA_TEXT.b,
                a,
            ),
            font_weight: FontWeightHint::Regular,
            max_width: Some(slot.width - 4.0),
        });

        // Close button (only when hovering this specific thumbnail)
        if is_hovered && self.config.show_close_buttons {
            let bx = abs_x + slot.width - CLOSE_BUTTON_SIZE - 4.0;
            let by = abs_y + 4.0;

            // Close button background
            let close_bg = if self.close_hovered {
                MOCHA_RED
            } else {
                MOCHA_SURFACE2
            };

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: by,
                width: CLOSE_BUTTON_SIZE,
                height: CLOSE_BUTTON_SIZE,
                color: Color::rgba(
                    close_bg.r,
                    close_bg.g,
                    close_bg.b,
                    a,
                ),
                corner_radii: CornerRadii::all(3.0),
            });

            // X symbol via two crossed lines
            let margin = 4.0;
            let line_color = Color::rgba(
                MOCHA_TEXT.r,
                MOCHA_TEXT.g,
                MOCHA_TEXT.b,
                a,
            );
            cmds.push(RenderCommand::Line {
                x1: bx + margin,
                y1: by + margin,
                x2: bx + CLOSE_BUTTON_SIZE - margin,
                y2: by + CLOSE_BUTTON_SIZE - margin,
                color: line_color,
                width: 1.5,
            });
            cmds.push(RenderCommand::Line {
                x1: bx + CLOSE_BUTTON_SIZE - margin,
                y1: by + margin,
                x2: bx + margin,
                y2: by + CLOSE_BUTTON_SIZE - margin,
                color: line_color,
                width: 1.5,
            });
        }
    }
}

// ============================================================================
// Peek manager — integrates with the taskbar
// ============================================================================

/// Manages the peek popup lifecycle for the entire taskbar.
///
/// The taskbar should call `on_button_hover()` when the mouse enters a button,
/// `on_button_leave()` when it exits, and `tick()` each frame.
pub struct PeekManager {
    /// The popup state.
    pub popup: PeekPopup,
    /// Currently hovered button's app_id (if any).
    hovered_app: Option<String>,
    /// Timestamp of last tick.
    last_tick_ms: u64,
}

impl PeekManager {
    /// Create a new peek manager with the given configuration.
    pub fn new(config: PeekConfig) -> Self {
        Self {
            popup: PeekPopup::new(config),
            hovered_app: None,
            last_tick_ms: 0,
        }
    }

    /// Notify that the mouse is hovering over a taskbar button for an app.
    /// `snapshots_fn` is called to get the window snapshots only if needed.
    pub fn on_button_hover<F>(
        &mut self,
        app_id: &str,
        button_center_x: f32,
        button_top_y: f32,
        now_ms: u64,
        snapshots_fn: F,
    ) where
        F: FnOnce() -> Vec<WindowSnapshot>,
    {
        if !self.popup.config.enabled {
            return;
        }

        let same_app = self
            .hovered_app
            .as_ref()
            .is_some_and(|a| a == app_id);

        if same_app && self.popup.is_active() {
            // Already showing or waiting for this app
            return;
        }

        self.hovered_app = Some(app_id.to_string());
        let snapshots = snapshots_fn();
        self.popup.begin_hover(
            app_id,
            button_center_x,
            button_top_y,
            snapshots,
            now_ms,
        );
    }

    /// Notify that the mouse left the taskbar button area.
    /// Only dismiss if the mouse is not over the popup itself.
    pub fn on_button_leave(&mut self, screen_x: f32, screen_y: f32, now_ms: u64) {
        if self.popup.contains_point(screen_x, screen_y) {
            // Mouse moved into the popup — keep it alive
            return;
        }
        self.hovered_app = None;
        self.popup.cancel(now_ms);
    }

    /// Notify that the mouse left the popup area (and isn't over the button).
    pub fn on_popup_leave(&mut self, now_ms: u64) {
        self.hovered_app = None;
        self.popup.cancel(now_ms);
    }

    /// Handle mouse movement. Returns true if state changed.
    pub fn on_mouse_move(
        &mut self,
        screen_x: f32,
        screen_y: f32,
        _now_ms: u64,
    ) -> bool {
        self.popup.on_mouse_move(screen_x, screen_y)
    }

    /// Handle a mouse click. Returns the action to perform.
    pub fn on_click(&mut self, screen_x: f32, screen_y: f32) -> PeekAction {
        let action = self.popup.on_click(screen_x, screen_y);
        if action != PeekAction::None {
            // After clicking, hide the popup
            if matches!(action, PeekAction::FocusWindow(_)) {
                self.popup.hide();
                self.hovered_app = None;
            }
        }
        action
    }

    /// Advance animation state. Returns true if a repaint is needed.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.last_tick_ms = now_ms;
        self.popup.tick(now_ms)
    }

    /// Get the currently hovered app ID, if any.
    pub fn hovered_app(&self) -> Option<&str> {
        self.hovered_app.as_deref()
    }

    /// Render the popup. Returns an empty vec if hidden.
    pub fn render(&self) -> Vec<RenderCommand> {
        self.popup.render()
    }

    /// Check if a point is inside the popup.
    pub fn hit_test(&self, screen_x: f32, screen_y: f32) -> bool {
        self.popup.is_rendering() && self.popup.contains_point(screen_x, screen_y)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(id: u64, title: &str, w: f32, h: f32) -> WindowSnapshot {
        WindowSnapshot::new(id, "test-app", title, w, h)
    }

    fn make_config() -> PeekConfig {
        PeekConfig {
            hover_delay_ms: 100,
            fade_duration_ms: 50,
            ..PeekConfig::default()
        }
    }

    // ---- Config tests ----

    #[test]
    fn test_config_default() {
        let c = PeekConfig::default();
        assert_eq!(c.hover_delay_ms, DEFAULT_HOVER_DELAY_MS);
        assert!(c.enabled);
        assert!(c.show_close_buttons);
    }

    #[test]
    fn test_config_roundtrip() {
        let c = PeekConfig {
            hover_delay_ms: 300,
            max_thumb_width: 180.0,
            max_thumb_height: 120.0,
            show_close_buttons: false,
            enabled: true,
            fade_duration_ms: 200,
        };
        let text = c.to_text();
        let c2 = PeekConfig::from_text(&text);
        assert_eq!(c2.hover_delay_ms, 300);
        assert!(!c2.show_close_buttons);
        assert_eq!(c2.fade_duration_ms, 200);
    }

    #[test]
    fn test_config_parse_empty() {
        let c = PeekConfig::from_text("");
        assert_eq!(c.hover_delay_ms, DEFAULT_HOVER_DELAY_MS);
    }

    #[test]
    fn test_config_parse_comments() {
        let c = PeekConfig::from_text("# comment\nhover_delay_ms=250\n# another");
        assert_eq!(c.hover_delay_ms, 250);
    }

    #[test]
    fn test_config_parse_invalid_values() {
        let c = PeekConfig::from_text("hover_delay_ms=abc\nmax_thumb_width=nan");
        assert_eq!(c.hover_delay_ms, DEFAULT_HOVER_DELAY_MS);
    }

    #[test]
    fn test_config_min_thumb_width() {
        let c = PeekConfig::from_text("max_thumb_width=10");
        // Clamped to 60.0 minimum
        assert!(c.max_thumb_width >= 60.0);
    }

    // ---- WindowSnapshot tests ----

    #[test]
    fn test_snapshot_new() {
        let s = make_snapshot(1, "Hello", 800.0, 600.0);
        assert_eq!(s.window_id, 1);
        assert_eq!(s.title, "Hello");
        assert!(!s.is_focused);
        assert!(!s.is_minimized);
    }

    #[test]
    fn test_snapshot_thumbnail_size_landscape() {
        let s = make_snapshot(1, "Wide", 1920.0, 1080.0);
        let (w, h) = s.thumbnail_size(200.0, 140.0);
        // Should fit within max dimensions
        assert!(w <= 200.0);
        assert!(h <= 140.0);
        // Should maintain aspect ratio approximately
        let ratio = w / h;
        let expected = 1920.0 / 1080.0;
        assert!((ratio - expected).abs() < 0.5);
    }

    #[test]
    fn test_snapshot_thumbnail_size_portrait() {
        let s = make_snapshot(1, "Tall", 600.0, 1200.0);
        let (w, h) = s.thumbnail_size(200.0, 140.0);
        assert!(w <= 200.0);
        assert!(h <= 140.0);
    }

    #[test]
    fn test_snapshot_thumbnail_size_zero_dimensions() {
        let s = make_snapshot(1, "Zero", 0.0, 0.0);
        let (w, h) = s.thumbnail_size(200.0, 140.0);
        assert!(w > 0.0);
        assert!(h > 0.0);
    }

    #[test]
    fn test_snapshot_display_title_short() {
        let s = make_snapshot(1, "Hello", 800.0, 600.0);
        assert_eq!(s.display_title(20), "Hello");
    }

    #[test]
    fn test_snapshot_display_title_truncate() {
        let s = make_snapshot(1, "This is a very long window title that should be truncated", 800.0, 600.0);
        let truncated = s.display_title(20);
        assert!(truncated.len() <= 20);
    }

    #[test]
    fn test_snapshot_display_title_tiny_max() {
        let s = make_snapshot(1, "Hello World", 800.0, 600.0);
        let t = s.display_title(2);
        assert!(t.len() <= 2);
    }

    // ---- Layout tests ----

    #[test]
    fn test_layout_empty() {
        let config = make_config();
        let (slots, w, h) = compute_thumbnail_layout(&[], &config);
        assert!(slots.is_empty());
        assert_eq!(w, 0.0);
        assert_eq!(h, 0.0);
    }

    #[test]
    fn test_layout_single_window() {
        let config = make_config();
        let snaps = vec![make_snapshot(1, "Win1", 800.0, 600.0)];
        let (slots, w, h) = compute_thumbnail_layout(&snaps, &config);
        assert_eq!(slots.len(), 1);
        assert!(w > 0.0);
        assert!(h > 0.0);
        assert_eq!(slots[0].window_id, 1);
    }

    #[test]
    fn test_layout_multiple_windows() {
        let config = make_config();
        let snaps = vec![
            make_snapshot(1, "Win1", 800.0, 600.0),
            make_snapshot(2, "Win2", 800.0, 600.0),
            make_snapshot(3, "Win3", 800.0, 600.0),
        ];
        let (slots, w, _h) = compute_thumbnail_layout(&snaps, &config);
        assert_eq!(slots.len(), 3);
        // Each slot should be positioned to the right of the previous one
        assert!(slots[1].x > slots[0].x);
        assert!(slots[2].x > slots[1].x);
        // Total width should accommodate all three
        assert!(w > slots[2].x + slots[2].width);
    }

    #[test]
    fn test_layout_different_sizes() {
        let config = make_config();
        let snaps = vec![
            make_snapshot(1, "Wide", 1920.0, 1080.0),
            make_snapshot(2, "Square", 600.0, 600.0),
        ];
        let (slots, _w, _h) = compute_thumbnail_layout(&snaps, &config);
        assert_eq!(slots.len(), 2);
        // Wide window should have different aspect ratio than square
        let ratio1 = slots[0].width / slots[0].height;
        let ratio2 = slots[1].width / slots[1].height;
        assert!((ratio1 - ratio2).abs() > 0.1);
    }

    // ---- ThumbnailSlot tests ----

    #[test]
    fn test_slot_contains_inside() {
        let slot = ThumbnailSlot {
            snapshot_index: 0,
            window_id: 1,
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 80.0,
        };
        assert!(slot.contains(50.0, 50.0));
    }

    #[test]
    fn test_slot_contains_outside() {
        let slot = ThumbnailSlot {
            snapshot_index: 0,
            window_id: 1,
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 80.0,
        };
        assert!(!slot.contains(5.0, 5.0));
        assert!(!slot.contains(200.0, 200.0));
    }

    #[test]
    fn test_slot_contains_title_area() {
        let slot = ThumbnailSlot {
            snapshot_index: 0,
            window_id: 1,
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 80.0,
        };
        // Should include the title area below
        assert!(slot.contains(50.0, 95.0));
    }

    #[test]
    fn test_slot_close_button_hit() {
        let slot = ThumbnailSlot {
            snapshot_index: 0,
            window_id: 1,
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 80.0,
        };
        // Close button is at top-right: x=10+100-18-4=88, y=10+4=14
        assert!(slot.close_button_hit(92.0, 18.0));
        // Outside close button
        assert!(!slot.close_button_hit(50.0, 50.0));
    }

    // ---- PeekPopup tests ----

    #[test]
    fn test_popup_new_is_hidden() {
        let popup = PeekPopup::new(make_config());
        assert_eq!(popup.phase, PeekPhase::Hidden);
        assert!(!popup.is_active());
        assert!(!popup.is_rendering());
    }

    #[test]
    fn test_popup_begin_hover_starts_waiting() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 100.0, 50.0, snaps, 1000);
        assert_eq!(popup.phase, PeekPhase::Waiting);
        assert!(popup.is_active());
    }

    #[test]
    fn test_popup_begin_hover_empty_snaps_stays_hidden() {
        let mut popup = PeekPopup::new(make_config());
        popup.begin_hover("test-app", 100.0, 50.0, vec![], 1000);
        assert_eq!(popup.phase, PeekPhase::Hidden);
    }

    #[test]
    fn test_popup_begin_hover_disabled_stays_hidden() {
        let mut config = make_config();
        config.enabled = false;
        let mut popup = PeekPopup::new(config);
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 100.0, 50.0, snaps, 1000);
        assert_eq!(popup.phase, PeekPhase::Hidden);
    }

    #[test]
    fn test_popup_tick_waiting_to_fading_in() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 100.0, 50.0, snaps, 1000);
        assert_eq!(popup.phase, PeekPhase::Waiting);

        // Not enough time
        let changed = popup.tick(1050);
        assert!(!changed);
        assert_eq!(popup.phase, PeekPhase::Waiting);

        // Enough time
        let changed = popup.tick(1101);
        assert!(changed);
        assert_eq!(popup.phase, PeekPhase::FadingIn);
    }

    #[test]
    fn test_popup_tick_fading_in_to_visible() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 100.0, 50.0, snaps, 1000);
        popup.tick(1101); // → FadingIn
        assert_eq!(popup.phase, PeekPhase::FadingIn);

        popup.tick(1200); // fade done (50ms duration)
        assert_eq!(popup.phase, PeekPhase::Visible);
        assert!((popup.opacity - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_popup_cancel_from_waiting() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 100.0, 50.0, snaps, 1000);
        popup.cancel(1050);
        assert_eq!(popup.phase, PeekPhase::Hidden);
    }

    #[test]
    fn test_popup_cancel_from_visible() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 100.0, 50.0, snaps, 1000);
        popup.tick(1101); // → FadingIn
        popup.tick(1200); // → Visible
        popup.cancel(1200);
        assert_eq!(popup.phase, PeekPhase::FadingOut);
    }

    #[test]
    fn test_popup_fade_out_completes() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 100.0, 50.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);
        popup.cancel(1200);
        popup.tick(1300); // fade out done (50ms)
        assert_eq!(popup.phase, PeekPhase::Hidden);
        assert!(popup.snapshots.is_empty());
    }

    #[test]
    fn test_popup_hide_immediate() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 100.0, 50.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);
        popup.hide();
        assert_eq!(popup.phase, PeekPhase::Hidden);
        assert_eq!(popup.opacity, 0.0);
    }

    #[test]
    fn test_popup_mouse_move_hover_detection() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![
            make_snapshot(1, "Win1", 800.0, 600.0),
            make_snapshot(2, "Win2", 800.0, 600.0),
        ];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);

        // Move mouse over first thumbnail
        let slot0_x = popup.popup_x + popup.slots[0].x + 10.0;
        let slot0_y = popup.popup_y + popup.slots[0].y + 10.0;
        popup.on_mouse_move(slot0_x, slot0_y);
        assert_eq!(popup.hovered_slot, Some(0));
    }

    #[test]
    fn test_popup_click_focus_window() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(42, "Target", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);

        // Click on the thumbnail
        let x = popup.popup_x + popup.slots[0].x + 10.0;
        let y = popup.popup_y + popup.slots[0].y + 10.0;
        let action = popup.on_click(x, y);
        assert_eq!(action, PeekAction::FocusWindow(42));
    }

    #[test]
    fn test_popup_click_close_window() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(7, "Closeable", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);

        // Click on close button (top-right of thumbnail)
        let slot = &popup.slots[0];
        let bx = popup.popup_x + slot.x + slot.width - CLOSE_BUTTON_SIZE / 2.0 - 4.0;
        let by = popup.popup_y + slot.y + CLOSE_BUTTON_SIZE / 2.0 + 4.0;
        let action = popup.on_click(bx, by);
        assert_eq!(action, PeekAction::CloseWindow(7));
    }

    #[test]
    fn test_popup_click_outside() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);

        let action = popup.on_click(0.0, 0.0);
        assert_eq!(action, PeekAction::None);
    }

    #[test]
    fn test_popup_contains_point() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);

        let cx = popup.popup_x + popup.popup_width / 2.0;
        let cy = popup.popup_y + popup.popup_height / 2.0;
        assert!(popup.contains_point(cx, cy));
        assert!(!popup.contains_point(-100.0, -100.0));
    }

    #[test]
    fn test_popup_render_hidden_empty() {
        let popup = PeekPopup::new(make_config());
        let cmds = popup.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_popup_render_visible_has_commands() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);

        let cmds = popup.render();
        // Should have: shadow, background, border + per-thumbnail commands
        assert!(cmds.len() >= 5);
    }

    #[test]
    fn test_popup_render_with_hovered_thumbnail() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);

        let x = popup.popup_x + popup.slots[0].x + 10.0;
        let y = popup.popup_y + popup.slots[0].y + 10.0;
        popup.on_mouse_move(x, y);

        let cmds = popup.render();
        // Should have extra commands for hover highlight and close button
        assert!(cmds.len() >= 8);
    }

    #[test]
    fn test_popup_render_minimized_window() {
        let mut popup = PeekPopup::new(make_config());
        let mut snap = make_snapshot(1, "Win", 800.0, 600.0);
        snap.is_minimized = true;
        popup.begin_hover("test-app", 200.0, 200.0, vec![snap], 1000);
        popup.tick(1101);
        popup.tick(1200);

        let cmds = popup.render();
        // Should contain a "Minimized" text command
        let has_minimized = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Minimized"
            } else {
                false
            }
        });
        assert!(has_minimized);
    }

    #[test]
    fn test_popup_no_double_start_same_app() {
        let mut popup = PeekPopup::new(make_config());
        let snaps1 = vec![make_snapshot(1, "Win1", 800.0, 600.0)];
        let snaps2 = vec![
            make_snapshot(1, "Win1", 800.0, 600.0),
            make_snapshot(2, "Win2", 800.0, 600.0),
        ];
        popup.begin_hover("test-app", 200.0, 200.0, snaps1, 1000);
        popup.tick(1101);
        popup.tick(1200);
        assert_eq!(popup.snapshots.len(), 1);

        // Hovering the same app again should not restart
        popup.begin_hover("test-app", 200.0, 200.0, snaps2, 1300);
        assert_eq!(popup.snapshots.len(), 1);
    }

    // ---- PeekManager tests ----

    #[test]
    fn test_manager_new() {
        let mgr = PeekManager::new(make_config());
        assert!(!mgr.popup.is_active());
        assert!(mgr.hovered_app().is_none());
    }

    #[test]
    fn test_manager_hover_and_show() {
        let mut mgr = PeekManager::new(make_config());
        mgr.on_button_hover("app1", 100.0, 50.0, 1000, || {
            vec![make_snapshot(1, "Win", 800.0, 600.0)]
        });
        assert_eq!(mgr.hovered_app(), Some("app1"));
        assert!(mgr.popup.is_active());
    }

    #[test]
    fn test_manager_hover_disabled() {
        let mut config = make_config();
        config.enabled = false;
        let mut mgr = PeekManager::new(config);
        mgr.on_button_hover("app1", 100.0, 50.0, 1000, || {
            vec![make_snapshot(1, "Win", 800.0, 600.0)]
        });
        assert!(!mgr.popup.is_active());
    }

    #[test]
    fn test_manager_leave_button_dismisses() {
        let mut mgr = PeekManager::new(make_config());
        mgr.on_button_hover("app1", 100.0, 50.0, 1000, || {
            vec![make_snapshot(1, "Win", 800.0, 600.0)]
        });
        mgr.popup.tick(1101);
        mgr.popup.tick(1200);
        // Pick a leave point demonstrably outside the popup rectangle.
        // The popup is positioned above the button (popup_y = 50 - height - 8,
        // which is negative for any non-trivial popup height), so the point
        // (0, 0) can actually fall *inside* the popup rectangle. Use a point
        // far below the popup instead.
        let outside_y = mgr.popup.popup_y + mgr.popup.popup_height + 100.0;
        mgr.on_button_leave(mgr.popup.popup_x - 100.0, outside_y, 1200);
        assert_eq!(mgr.popup.phase, PeekPhase::FadingOut);
    }

    #[test]
    fn test_manager_leave_button_into_popup_stays() {
        let mut mgr = PeekManager::new(make_config());
        mgr.on_button_hover("app1", 200.0, 200.0, 1000, || {
            vec![make_snapshot(1, "Win", 800.0, 600.0)]
        });
        mgr.popup.tick(1101);
        mgr.popup.tick(1200);

        // Move mouse into popup area
        let px = mgr.popup.popup_x + 10.0;
        let py = mgr.popup.popup_y + 10.0;
        mgr.on_button_leave(px, py, 1200);
        assert_eq!(mgr.popup.phase, PeekPhase::Visible);
    }

    #[test]
    fn test_manager_click_focus_hides() {
        let mut mgr = PeekManager::new(make_config());
        mgr.on_button_hover("app1", 200.0, 200.0, 1000, || {
            vec![make_snapshot(42, "Win", 800.0, 600.0)]
        });
        mgr.popup.tick(1101);
        mgr.popup.tick(1200);

        let x = mgr.popup.popup_x + mgr.popup.slots[0].x + 10.0;
        let y = mgr.popup.popup_y + mgr.popup.slots[0].y + 10.0;
        let action = mgr.on_click(x, y);
        assert_eq!(action, PeekAction::FocusWindow(42));
        assert!(!mgr.popup.is_active());
    }

    #[test]
    fn test_manager_hit_test() {
        let mut mgr = PeekManager::new(make_config());
        mgr.on_button_hover("app1", 200.0, 200.0, 1000, || {
            vec![make_snapshot(1, "Win", 800.0, 600.0)]
        });
        mgr.popup.tick(1101);
        mgr.popup.tick(1200);

        let cx = mgr.popup.popup_x + 10.0;
        let cy = mgr.popup.popup_y + 10.0;
        assert!(mgr.hit_test(cx, cy));
        assert!(!mgr.hit_test(-100.0, -100.0));
    }

    #[test]
    fn test_manager_tick_returns_repaint() {
        let mut mgr = PeekManager::new(make_config());
        mgr.on_button_hover("app1", 200.0, 200.0, 1000, || {
            vec![make_snapshot(1, "Win", 800.0, 600.0)]
        });
        let changed = mgr.tick(1050);
        assert!(!changed); // Still waiting

        let changed = mgr.tick(1101);
        assert!(changed); // Transition to FadingIn
    }

    #[test]
    fn test_manager_render_empty_when_hidden() {
        let mgr = PeekManager::new(make_config());
        let cmds = mgr.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_popup_position_centered_above_button() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 300.0, 500.0, snaps, 1000);

        // Popup should be approximately centered horizontally on button_center_x=300
        let center = popup.popup_x + popup.popup_width / 2.0;
        assert!((center - 300.0).abs() < 1.0);
        // Popup should be above button_top_y=500
        assert!(popup.popup_y + popup.popup_height < 500.0);
    }

    #[test]
    fn test_peek_action_equality() {
        assert_eq!(PeekAction::None, PeekAction::None);
        assert_eq!(PeekAction::FocusWindow(1), PeekAction::FocusWindow(1));
        assert_ne!(PeekAction::FocusWindow(1), PeekAction::FocusWindow(2));
        assert_ne!(PeekAction::FocusWindow(1), PeekAction::CloseWindow(1));
    }

    #[test]
    fn test_opacity_during_fade_in() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101); // → FadingIn at t=1101

        // Midway through fade (25ms into 50ms fade)
        popup.tick(1126);
        assert!(popup.opacity > 0.0);
        assert!(popup.opacity < 1.0);
    }

    #[test]
    fn test_opacity_during_fade_out() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200); // → Visible
        popup.cancel(1200); // → FadingOut

        popup.tick(1225); // midway
        assert!(popup.opacity > 0.0);
        assert!(popup.opacity < 1.0);
    }

    #[test]
    fn test_mouse_move_when_hidden() {
        let mut popup = PeekPopup::new(make_config());
        let changed = popup.on_mouse_move(100.0, 100.0);
        assert!(!changed);
    }

    #[test]
    fn test_click_when_hidden() {
        let mut popup = PeekPopup::new(make_config());
        let action = popup.on_click(100.0, 100.0);
        assert_eq!(action, PeekAction::None);
    }

    #[test]
    fn test_popup_multiple_windows_layout() {
        let config = make_config();
        let snaps = vec![
            make_snapshot(1, "Win1", 1920.0, 1080.0),
            make_snapshot(2, "Win2", 800.0, 600.0),
            make_snapshot(3, "Win3", 1024.0, 768.0),
            make_snapshot(4, "Win4", 600.0, 400.0),
        ];
        let (slots, total_w, total_h) = compute_thumbnail_layout(&snaps, &config);
        assert_eq!(slots.len(), 4);
        assert!(total_w > 0.0);
        assert!(total_h > 0.0);

        // All slots should have positive dimensions
        for slot in &slots {
            assert!(slot.width > 0.0);
            assert!(slot.height > 0.0);
        }
    }
}
