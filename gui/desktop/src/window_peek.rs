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
//!
//! # Colour
//!
//! Every colour here is read from the [`Palette`] the renderers are handed;
//! the nine `MOCHA_*` constants this module used to own are gone. Three of the
//! decisions that conversion forced are worth stating, because a later reader
//! will otherwise assume the obvious answer was taken.
//!
//! ## The close button's X was illegible — in the theme the shell ships
//!
//! The X was drawn in `TEXT` on a close button that turns `RED` under the
//! cursor. Light-grey lettering on a pastel red measures **1.60:1 in Mocha**
//! and 1.47:1 in Latte: not "below the 4.5:1 floor" but very nearly invisible,
//! and — uniquely among the forty modules converted before this one — broken
//! in the *dark* theme, which is the one the shell actually starts in. Every
//! previous instance of this bug was a light-mode failure that Mocha happened
//! to hide. The idle state was failing too, more quietly: `TEXT` on `SURFACE2`
//! is 4.62:1 in Mocha but 3.69:1 in Latte.
//!
//! The X is now [`readable_on`] of whatever the button is actually filled
//! with, which fixes all four cases at once: 8.10:1 and 4.80:1 hovered, 5.90:1
//! and 8.67:1 idle. This is the same fix as in the previous four modules, and
//! it keeps being the same fix because the underlying mistake keeps being the
//! same one — a constant named for a *value* (`TEXT`) cannot express the thing
//! the site needs, which is "the ink that can be read on this fill".
//!
//! ## A window's own colour is not the shell's to theme
//!
//! [`WindowSnapshot::dominant_color`] is a sample of what a real window is
//! displaying. It is data, like the pixels of a [`RenderCommand::Image`], and
//! it does not belong to any palette — so it is `Option<Color>`, and the
//! renderer supplies `p.surface1` when it is `None`. Before this change the
//! field was *initialised* to Mocha `SURFACE1`, which conflated two different
//! things: "this window has not been sampled yet" and "this window happens to
//! be that exact grey". The first must follow the theme and the second must
//! not, so they cannot be the same value. Tests that install a sample declare
//! it to the membership sweep as `derived`, which is the sweep's way of
//! recording that a colour was claimed by someone rather than leaked.
//!
//! ## A focused window is a *selection*, so its border follows the accent
//!
//! The focused thumbnail's border was `BLUE`, which in this module means "this
//! is the one you are on" rather than any kind of status — the same
//! state-versus-selection split the accessibility and focus-assist pages
//! settled. It is now `p.accent`, so a user who chooses a green accent sees
//! the focused window ringed in green. The *hover* ring stays `p.overlay0` and
//! the resting ring `p.surface2`: those are depth, not choice.

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

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
        out.push_str(&format!("show_close_buttons={}\n", self.show_close_buttons));
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
    /// The window content's own dominant colour, once something has sampled
    /// it — `None` until then.
    ///
    /// A real window's colour is not a palette role and must not become one:
    /// it is a property of what the application is displaying, in the same way
    /// the pixels of a [`RenderCommand::Image`] are. So the renderer draws
    /// this verbatim when it is `Some`, and falls back to `p.surface1` when it
    /// is `None`.
    ///
    /// The `Option` is the point rather than ceremony. This field used to be
    /// initialised to Mocha `SURFACE1`, which made "nobody has looked at this
    /// window yet" indistinguishable from "this window really is that grey" —
    /// and the first of those has to follow the theme while the second must
    /// not.
    pub dominant_color: Option<Color>,
    /// Whether this window is currently focused.
    pub is_focused: bool,
    /// Whether this window is minimized.
    pub is_minimized: bool,
}

impl WindowSnapshot {
    /// Create a new window snapshot, with no content colour sampled yet.
    pub fn new(window_id: u64, app_id: &str, title: &str, width: f32, height: f32) -> Self {
        Self {
            window_id,
            app_id: app_id.to_string(),
            title: title.to_string(),
            window_width: width,
            window_height: height,
            dominant_color: None,
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
        let w = (self.window_width * scale)
            .max(MIN_THUMBNAIL_WIDTH)
            .min(max_w);
        let h = (self.window_height * scale).min(max_h);
        (w, h)
    }
}

// A `display_title(max_chars)` used to live here, cutting the title to a
// character budget derived from `slot.width / 7.0`. It is gone rather than
// fixed, for three reasons that are worth keeping written down:
//
//   * It was **redundant**. Its one call site already passed `max_width` and
//     `TextOverflow::Ellipsis`, so the renderer was going to fit the title
//     anyway — by measuring the face it draws in, which is the only way to get
//     it right. Cutting first only guaranteed the measured pass never saw the
//     text it was meant to fit.
//   * It **panicked**. The `max_chars < 3` branch was
//     `&self.title[..self.title.len().min(max_chars)]` — a *byte* offset into a
//     `str`. A narrow thumbnail plus a window titled in any non-Latin script
//     aborts the desktop shell, and a window title is attacker-supplied in the
//     sense that any application chooses its own.
//   * It compared `self.title.len()` (bytes) against a character budget, so
//     even the non-panicking branch cut early on accented text.
//
// The lesson generalises past this function: a caller that pre-truncates for a
// renderer that already elides is not being careful, it is adding a second,
// worse answer to a question that was already answered.

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
        px >= self.x
            && px < self.x + self.width
            && py >= self.y
            && py < self.y + self.height + TITLE_HEIGHT
    }

    /// Check if a point is inside the close button area (top-right corner).
    pub fn close_button_hit(&self, px: f32, py: f32) -> bool {
        let bx = self.x + self.width - CLOSE_BUTTON_SIZE - 4.0;
        let by = self.y + 4.0;
        px >= bx && px < bx + CLOSE_BUTTON_SIZE && py >= by && py < by + CLOSE_BUTTON_SIZE
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
        let (slots, width, height) = compute_thumbnail_layout(&self.snapshots, &self.config);
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
                if self.config.show_close_buttons && slot.close_button_hit(local_x, local_y) {
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
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
        if !self.is_rendering() {
            return Vec::new();
        }

        let mut cmds = Vec::with_capacity(self.slots.len().saturating_mul(6).saturating_add(4));
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
        let bg = p.base;
        cmds.push(RenderCommand::FillRect {
            x: self.popup_x,
            y: self.popup_y,
            width: self.popup_width,
            height: self.popup_height,
            color: Color::rgba(bg.r, bg.g, bg.b, bg_alpha),
            corner_radii: CornerRadii::all(POPUP_RADIUS),
        });

        // Border
        let border = p.surface2;
        cmds.push(RenderCommand::StrokeRect {
            x: self.popup_x,
            y: self.popup_y,
            width: self.popup_width,
            height: self.popup_height,
            color: Color::rgba(border.r, border.g, border.b, (180.0 * alpha) as u8),
            line_width: 1.0,
            corner_radii: CornerRadii::all(POPUP_RADIUS),
        });

        // Render each thumbnail
        for (i, slot) in self.slots.iter().enumerate() {
            let is_hovered = self.hovered_slot == Some(i);
            self.render_thumbnail(&mut cmds, p, slot, i, is_hovered, alpha);
        }

        cmds
    }

    /// Render a single thumbnail slot.
    fn render_thumbnail(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
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
            let wash = p.surface0;
            cmds.push(RenderCommand::FillRect {
                x: abs_x - 4.0,
                y: abs_y - 4.0,
                width: slot.width + 8.0,
                height: slot.height + TITLE_HEIGHT + 8.0,
                color: Color::rgba(wash.r, wash.g, wash.b, (200.0 * alpha) as u8),
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Thumbnail background (representing window content)
        let snap = match self.snapshots.get(index) {
            Some(s) => s,
            None => return,
        };

        // A minimized window is showing nothing, so whatever was sampled from
        // it is stale — the placeholder is the honest answer, and a dimmer one
        // than the un-sampled case so the two states still read apart.
        let content_color = if snap.is_minimized {
            p.surface0
        } else {
            snap.dominant_color.unwrap_or(p.surface1)
        };

        cmds.push(RenderCommand::FillRect {
            x: abs_x,
            y: abs_y,
            width: slot.width,
            height: slot.height,
            color: Color::rgba(content_color.r, content_color.g, content_color.b, a),
            corner_radii: CornerRadii::all(4.0),
        });

        // Border around thumbnail. Focus is a *selection* — "this is the one
        // you are on" — so it wears the accent, while hover and rest are
        // depth and stay on the neutral ramp.
        let border_color = if snap.is_focused {
            p.accent
        } else if is_hovered {
            p.overlay0
        } else {
            p.surface2
        };

        cmds.push(RenderCommand::StrokeRect {
            x: abs_x,
            y: abs_y,
            width: slot.width,
            height: slot.height,
            color: Color::rgba(border_color.r, border_color.g, border_color.b, a),
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
                color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, a),
                font_weight: FontWeightHint::Regular,
                max_width: Some(slot.width - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Title below thumbnail. Handed over whole: `max_width` and
        // `TextOverflow::Ellipsis` below are the instruction to fit it, and the
        // renderer carries it out by measuring. See the note on `Snapshot` for
        // the `display_title` that used to cut it here and why it is gone.
        let title_y = abs_y + slot.height + 4.0;

        cmds.push(RenderCommand::Text {
            x: abs_x + 2.0,
            y: title_y,
            text: snap.title.clone(),
            font_size: 11.0,
            color: Color::rgba(p.text.r, p.text.g, p.text.b, a),
            font_weight: FontWeightHint::Regular,
            max_width: Some(slot.width - 4.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Close button (only when hovering this specific thumbnail)
        if is_hovered && self.config.show_close_buttons {
            let bx = abs_x + slot.width - CLOSE_BUTTON_SIZE - 4.0;
            let by = abs_y + 4.0;

            // Close button background
            let close_bg = if self.close_hovered {
                p.red
            } else {
                p.surface2
            };

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: by,
                width: CLOSE_BUTTON_SIZE,
                height: CLOSE_BUTTON_SIZE,
                color: Color::rgba(close_bg.r, close_bg.g, close_bg.b, a),
                corner_radii: CornerRadii::all(3.0),
            });

            // X symbol via two crossed lines, inked for the fill underneath
            // it. `TEXT` here measured 1.60:1 hovered in Mocha — the one
            // contrast failure in this series that the dark theme did not
            // hide. See the module docs.
            let margin = 4.0;
            let ink = readable_on(close_bg);
            let line_color = Color::rgba(ink.r, ink.g, ink.b, a);
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

        let same_app = self.hovered_app.as_ref().is_some_and(|a| a == app_id);

        if same_app && self.popup.is_active() {
            // Already showing or waiting for this app
            return;
        }

        self.hovered_app = Some(app_id.to_string());
        let snapshots = snapshots_fn();
        self.popup
            .begin_hover(app_id, button_center_x, button_top_y, snapshots, now_ms);
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
    pub fn on_mouse_move(&mut self, screen_x: f32, screen_y: f32, _now_ms: u64) -> bool {
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
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
        self.popup.render(p)
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
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check::assert_drawn_from;
    use appearance::AccentColor;

    fn make_snapshot(id: u64, title: &str, w: f32, h: f32) -> WindowSnapshot {
        WindowSnapshot::new(id, "test-app", title, w, h)
    }

    /// A popup driven all the way to `Visible` showing one window with `title`.
    fn peek_showing(title: &str) -> PeekPopup {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, title, 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);
        popup
    }

    fn make_config() -> PeekConfig {
        PeekConfig {
            hover_delay_ms: 100,
            fade_duration_ms: 50,
            ..PeekConfig::default()
        }
    }

    /// A window content colour that belongs to no palette — what a real
    /// sample of a real window would look like.
    const SAMPLED: Color = Color::from_hex(0x00C0_FFEE);

    /// A palette wearing an accent that is in no palette, so "this site
    /// followed the accent" and "this site read a role" cannot be confused.
    ///
    /// The guards below are the fixture doing instrument duty: if the accent
    /// were equal to a role, every accent assertion would pass for the wrong
    /// reason, and if `SAMPLED` were equal to a role the membership sweep
    /// would accept a leaked constant as a window's own colour.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0x00FF_00FF);
        assert!(
            !p.roles()
                .iter()
                .any(|(n, r)| *n != "accent" && *r == p.accent),
            "the fixture accent is a role of the {} palette, so it cannot \
             distinguish following the accent from reading that role",
            if light { "light" } else { "dark" }
        );
        assert!(
            !p.roles().iter().any(|(_, r)| *r == SAMPLED),
            "the fixture's sampled window colour is a role of the {} palette, \
             so the membership sweep could not tell a leaked constant from a \
             window's own pixels",
            if light { "light" } else { "dark" }
        );
        p
    }

    /// A palette wearing a *real* accent, which resolves to a different value
    /// per mode.
    ///
    /// [`accented`] installs one fixed magenta in both modes on purpose — that
    /// is what makes it a good instrument for "did this site follow the
    /// accent". It is therefore useless for "did this site move when the mode
    /// did", because the accent legitimately would not move. A real
    /// [`AccentColor`] has a dark and a light form, so it moves the way every
    /// other role does.
    fn wearing(light: bool, accent: AccentColor) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = if light {
            accent.color_light()
        } else {
            accent.color()
        };
        p
    }

    /// Every colour `cmds` puts on the screen, at full alpha.
    ///
    /// Alpha is stripped because it is animation and depth here — the fade
    /// scales every colour, and the popup's own chrome is drawn at 230/180/200
    /// — none of which is a claim about the palette. Comparing it would make
    /// every pin an assertion about the fade curve as well.
    fn colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::Line { color, .. }
                | RenderCommand::BoxShadow { color, .. } => {
                    Some(Color::rgba(color.r, color.g, color.b, 255))
                }
                _ => None,
            })
            .collect()
    }

    /// A visible popup over `snaps`, with slot 0 hovered when `hover`.
    fn peek_over(snaps: Vec<WindowSnapshot>, hover: bool) -> PeekPopup {
        let mut popup = PeekPopup::new(make_config());
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);
        if hover {
            popup.hovered_slot = Some(0);
        }
        popup
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

    /// The title reaches the renderer whole, with the width it has to fit in.
    /// It used to be cut here first, to a character budget guessed from the
    /// thumbnail's width — which both under- and over-shot on any text that is
    /// not average-width ASCII, and aborted the process outright on a narrow
    /// thumbnail with a non-Latin title.
    #[test]
    fn a_title_is_bounded_by_width_not_pre_truncated() {
        let long = "This is a very long window title — Café über 日本語 — that cannot fit";
        let peek = peek_showing(long);
        let titles: Vec<_> = peek
            .render(&accented(false))
            .into_iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    text,
                    max_width,
                    overflow,
                    ..
                } if text == long => Some((max_width, overflow)),
                _ => None,
            })
            .collect();
        assert!(!titles.is_empty(), "the title is drawn uncut");
        for (max_width, overflow) in titles {
            assert!(max_width.is_some(), "with a width to fit inside");
            assert_eq!(
                overflow,
                TextOverflow::Ellipsis,
                "and a mark when it does not"
            );
        }
    }

    /// A title in a non-Latin script, in a thumbnail too narrow for even an
    /// ellipsis, must render. The removed `display_title` sliced a `str` at a
    /// byte offset in exactly that case.
    #[test]
    fn a_narrow_thumbnail_with_a_non_latin_title_renders() {
        let peek = peek_showing("日本語のウィンドウ");
        assert!(!peek.render(&accented(false)).is_empty());
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
        let cmds = popup.render(&accented(false));
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_popup_render_visible_has_commands() {
        let mut popup = PeekPopup::new(make_config());
        let snaps = vec![make_snapshot(1, "Win", 800.0, 600.0)];
        popup.begin_hover("test-app", 200.0, 200.0, snaps, 1000);
        popup.tick(1101);
        popup.tick(1200);

        let cmds = popup.render(&accented(false));
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

        let cmds = popup.render(&accented(false));
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

        let cmds = popup.render(&accented(false));
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
        let cmds = mgr.render(&accented(false));
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

    // ---- Colour: the conversion off this module's own palette ----

    /// The four window states this module can draw, as one popup.
    ///
    /// Focused, hovered, minimized and sampled are separate branches in
    /// `render_thumbnail`, and a fixture that omits one leaves that branch
    /// unchecked by every test below — the empty-state lesson from the focus
    /// assist page, applied up front rather than after a sweep found it.
    fn four_states() -> Vec<WindowSnapshot> {
        let mut plain = make_snapshot(1, "Plain", 800.0, 600.0);
        let mut focused = make_snapshot(2, "Focused", 800.0, 600.0);
        focused.is_focused = true;
        let mut minimized = make_snapshot(3, "Minimized", 800.0, 600.0);
        minimized.is_minimized = true;
        let mut sampled = make_snapshot(4, "Sampled", 800.0, 600.0);
        sampled.dominant_color = Some(SAMPLED);
        plain.dominant_color = None;
        vec![plain, focused, minimized, sampled]
    }

    /// Nothing either renderer draws is outside the palette it was handed.
    ///
    /// Rendered in *both* modes, because a leftover Mocha constant is a legal
    /// colour in the dark render and only names itself in the light one. The
    /// window's own sampled colour is declared `derived`: it is the one value
    /// here that is deliberately not a role, and naming it at the call site is
    /// how the sweep records that somebody claimed it.
    #[test]
    fn every_colour_the_module_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            for hover in [false, true] {
                for close in [false, true] {
                    let mut popup = peek_over(four_states(), hover);
                    popup.close_hovered = close;
                    assert_drawn_from(&p, &popup.render(&p), &[SAMPLED], "window peek");
                }
            }
        }
    }

    /// None of the nine deleted constants is still drawn.
    ///
    /// The membership sweep above cannot catch these on its own: in the *dark*
    /// render every one of them is a legitimate role, so only the light render
    /// can say a Mocha value survived.
    #[test]
    fn none_of_the_nine_deleted_constants_is_still_drawn() {
        let deleted = [
            ("BASE", 0x001E_1E2E),
            ("SURFACE0", 0x0031_3244),
            ("SURFACE1", 0x0045_475A),
            ("SURFACE2", 0x0058_5B70),
            ("OVERLAY0", 0x006C_7086),
            ("TEXT", 0x00CD_D6F4),
            ("SUBTEXT0", 0x00A6_ADC8),
            ("BLUE", 0x0089_B4FA),
            ("RED", 0x00F3_8BA8),
        ];
        let p = accented(true);
        for hover in [false, true] {
            for close in [false, true] {
                let mut popup = peek_over(four_states(), hover);
                popup.close_hovered = close;
                let drawn = colors(&popup.render(&p));
                for (name, hex) in deleted {
                    assert!(
                        !drawn.contains(&Color::from_hex(hex)),
                        "the light render still draws Mocha {name} \
                         (#{hex:06X}), so that constant survived the \
                         conversion (hover = {hover}, close = {close})"
                    );
                }
            }
        }
    }

    /// Every colour of the popup, in order, is the role that site claims.
    ///
    /// The expectation is written out by hand rather than derived from
    /// anything the renderer walks, which is what lets it see a site moving as
    /// well as a site changing colour.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            for close in [false, true] {
                let mut popup = peek_over(four_states(), true);
                popup.close_hovered = close;
                let black = Color::rgba(0, 0, 0, 255);
                // Chrome: shadow, background, border.
                let mut want = vec![black, p.base, p.surface2];
                // Slot 0: hovered, unsampled, unfocused — highlight, then the
                // placeholder fill, the hover ring and the title. Hovering is
                // also what makes the close button appear, so its fill and the
                // two strokes of its X land here too — the pin catching that
                // omission is the reason it is written out by hand.
                //
                // Both states of that button are rendered because only the
                // *ink* is asserted elsewhere, and the ink is a step function:
                // `peach` and `red` take the same side of the step in both
                // modes, so a hovered button quietly repainted `peach` was a
                // site with no test at all until this loop existed.
                let close_bg = if close { p.red } else { p.surface2 };
                let x_ink = readable_on(close_bg);
                want.extend([
                    p.surface0, p.surface1, p.overlay0, p.text, close_bg, x_ink, x_ink,
                ]);
                // Slot 1: focused, unsampled.
                want.extend([p.surface1, p.accent, p.text]);
                // Slot 2: minimized — placeholder fill, resting ring, the
                // "Minimized" legend, then the title.
                want.extend([p.surface0, p.surface2, p.subtext0, p.text]);
                // Slot 3: a real sample, which is not a role at all.
                want.extend([SAMPLED, p.surface2, p.text]);
                assert_eq!(
                    colors(&popup.render(&p)),
                    want,
                    "the peek popup's colours are not the roles it claims, in \
                     order (light = {light}, close hovered = {close})"
                );
            }
        }
    }

    /// The close button's X is legible on the button it is drawn on.
    ///
    /// This is the module's real bug fix, and the one contrast failure in this
    /// series that the dark theme did not hide: `TEXT` on the hovered red
    /// measured **1.60:1 in Mocha** and 1.47:1 in Latte. `readable_on` is a
    /// *step* function of the fill's brightness, so a test that sampled one
    /// state in one theme would still pass against a hard-coded ink — the
    /// assertion at the end is what makes this a claim about the function
    /// rather than about a sample.
    #[test]
    fn the_close_button_x_is_legible_on_the_button_it_marks() {
        let mut saw_dark_ink = false;
        let mut saw_light_ink = false;
        for light in [false, true] {
            let p = accented(light);
            for close in [false, true] {
                let mut popup = peek_over(four_states(), true);
                popup.close_hovered = close;
                let cmds = popup.render(&p);
                let fill = if close { p.red } else { p.surface2 };
                let ink: Vec<Color> = cmds
                    .iter()
                    .filter_map(|c| match c {
                        RenderCommand::Line { color, .. } => {
                            Some(Color::rgba(color.r, color.g, color.b, 255))
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(ink.len(), 2, "the X is two crossed lines");
                for got in ink {
                    assert_eq!(
                        got,
                        readable_on(fill),
                        "the close X is not inked for its own button \
                         (light = {light}, hovered = {close})"
                    );
                    if got == Color::from_hex(0x0011_111B) {
                        saw_dark_ink = true;
                    } else {
                        saw_light_ink = true;
                    }
                }
            }
        }
        assert!(
            saw_dark_ink && saw_light_ink,
            "every case landed on the same side of readable_on's step, so \
             this test would pass against a hard-coded ink and proves nothing"
        );
    }

    /// The focused window's ring is the accent, and it is the only one.
    ///
    /// Focus here marks *which window you are on*, not a status, so it follows
    /// the user's accent the way a selected row does. Hover and rest are depth
    /// and must stay on the neutral ramp — if they drifted onto the accent the
    /// popup would claim three windows were focused.
    #[test]
    fn only_the_focused_thumbnail_wears_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            let popup = peek_over(four_states(), true);
            let rings: Vec<Color> = popup
                .render(&p)
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. } => {
                        Some(Color::rgba(color.r, color.g, color.b, 255))
                    }
                    _ => None,
                })
                .collect();
            // The popup's own border, then one ring per thumbnail.
            assert_eq!(
                rings,
                vec![p.surface2, p.overlay0, p.accent, p.surface2, p.surface2],
                "the thumbnail rings are wrong (light = {light})"
            );
        }
    }

    /// Hovering the focused window does not take its focus ring away.
    ///
    /// [`four_states`] gives the hovered slot and the focused slot to two
    /// different windows, so nothing above it exercises the precedence between
    /// the two branches — swap them and every test on this page still passes.
    /// The pointer sits on the focused window constantly in real use, and it
    /// is exactly then that "which window am I on" matters most.
    #[test]
    fn a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it() {
        for light in [false, true] {
            let p = accented(light);
            let mut snap = make_snapshot(1, "Both", 800.0, 600.0);
            snap.is_focused = true;
            let rings: Vec<Color> = peek_over(vec![snap], true)
                .render(&p)
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. } => {
                        Some(Color::rgba(color.r, color.g, color.b, 255))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                rings,
                vec![p.surface2, p.accent],
                "hovering the focused window replaced its focus ring with the \
                 hover ring, so the popup no longer says which window you are \
                 on (light = {light})"
            );
        }
    }

    /// The manager draws with the palette it was handed, not one of its own.
    ///
    /// Every other colour test here calls [`PeekPopup::render`] directly — but
    /// the desktop calls [`PeekManager::render`], and that one-line delegate is
    /// the only place a palette could be swapped for a hard-coded one without a
    /// single test on this page noticing.
    #[test]
    fn the_manager_renders_with_the_palette_it_was_given() {
        let mut mgr = PeekManager::new(make_config());
        mgr.popup
            .begin_hover("test-app", 200.0, 200.0, four_states(), 1000);
        mgr.popup.tick(1101);
        mgr.popup.tick(1200);
        mgr.popup.hovered_slot = Some(0);
        let light = accented(true);
        assert_drawn_from(&light, &mgr.render(&light), &[SAMPLED], "peek manager");
        assert_ne!(
            colors(&mgr.render(&accented(false))),
            colors(&mgr.render(&light)),
            "the manager drew the same popup in both modes, so it is not \
             rendering with the palette it was given"
        );
    }

    /// An unsampled window gets the palette's placeholder; a sampled one gets
    /// its own colour, untouched by the theme.
    ///
    /// These are the two halves of why `dominant_color` is an `Option`. Before
    /// it was one, "not sampled" *was* Mocha `SURFACE1` — so in Latte the
    /// placeholder stayed dark, and a window that genuinely was that grey
    /// could not be told from one nobody had looked at.
    #[test]
    fn an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not() {
        let mut placeholders = Vec::new();
        for light in [false, true] {
            let p = accented(light);
            let popup = peek_over(four_states(), false);
            let fills = colors(&popup.render(&p));
            // Slot 0 is unsampled, slot 3 carries a real sample. Chrome is
            // three commands; each unhovered slot draws fill, ring, title,
            // and the minimized one an extra legend — and with nothing
            // hovered there is no close button anywhere.
            assert_eq!(
                fills[3], p.surface1,
                "the unsampled window (light = {light})"
            );
            assert_eq!(
                fills[13], SAMPLED,
                "the sampled window's own colour was themed away \
                 (light = {light})"
            );
            placeholders.push(fills[3]);
        }
        assert_ne!(
            placeholders[0], placeholders[1],
            "the placeholder is the same in both modes, so it is not being \
             read from the palette"
        );
    }

    /// A minimized window shows the placeholder, not a stale sample.
    ///
    /// What was sampled from a window before it was minimized is no longer
    /// what that window is showing, so drawing it would be a confident lie.
    #[test]
    fn a_minimized_window_ignores_whatever_was_sampled_from_it() {
        let p = accented(false);
        let mut snap = make_snapshot(1, "Gone", 800.0, 600.0);
        snap.dominant_color = Some(SAMPLED);
        snap.is_minimized = true;
        let drawn = colors(&peek_over(vec![snap], false).render(&p));
        assert!(
            !drawn.contains(&SAMPLED),
            "a minimized window drew the colour it had before it was minimized"
        );
        assert_eq!(drawn[3], p.surface0, "the minimized placeholder");
    }

    /// Changing the mode changes what is drawn — at every site, not most.
    ///
    /// The one site that legitimately does not move is the shadow, which is an
    /// absence of light rather than a colour, and the sampled window, which is
    /// not the shell's to theme. Both are excluded by name rather than by a
    /// tolerance, so a site that stopped following the palette cannot hide
    /// among them.
    ///
    /// This is the one test that must not use [`accented`]: that fixture pins
    /// the same magenta in both modes, so the focused ring would fail here for
    /// a reason that is not a defect. A real [`AccentColor`] moves.
    ///
    /// The close button is rendered in both of its states for the same reason
    /// the ordered pin loops over them: the hovered fill and the ink chosen for
    /// it are two distinct sites, and a fixture that leaves `close_hovered`
    /// alone renders neither. A site nothing renders is a site nothing checks,
    /// so the loop is what makes this test's claim — *every* site — true.
    #[test]
    fn every_site_changes_when_the_mode_does() {
        for close in [false, true] {
            let mut popup = peek_over(four_states(), true);
            popup.close_hovered = close;
            assert_modes_differ(&popup, close);
        }
    }

    /// The body of [`every_site_changes_when_the_mode_does`], for one state of
    /// the close button.
    fn assert_modes_differ(popup: &PeekPopup, close: bool) {
        let dark = wearing(false, AccentColor::Mauve);
        let light = wearing(true, AccentColor::Mauve);
        let a = colors(&popup.render(&dark));
        let b = colors(&popup.render(&light));
        assert_eq!(a.len(), b.len(), "the two modes drew different popups");
        let black = Color::rgba(0, 0, 0, 255);
        for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            if *x == black || *x == SAMPLED {
                assert_eq!(
                    x, y,
                    "colour {i} is the shadow or the sample and must not move \
                     (close hovered = {close})"
                );
                continue;
            }
            assert_ne!(
                x, y,
                "colour {i} is the same in both modes (close hovered = {close})"
            );
        }
    }
}
