//! On-Screen Display (OSD) overlay system.
//!
//! Renders transient popups for system feedback: volume/brightness changes,
//! media track notifications, caps/num lock indicators, and ejection notices.
//! OSD overlays appear centered near the bottom of the screen and auto-dismiss
//! after a configurable timeout.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme constants — Catppuccin Mocha
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// OSD types
// ============================================================================

/// The kind of information an OSD overlay shows.
#[derive(Clone, Debug, PartialEq)]
pub enum OsdKind {
    /// System volume changed (0–100, muted flag).
    Volume { level: u8, muted: bool },
    /// Display brightness changed (0–100).
    Brightness { level: u8 },
    /// Media track changed (title + artist + album, optional progress).
    MediaTrack {
        title: String,
        artist: String,
        album: String,
    },
    /// Media playback state toggled.
    MediaPlayPause { playing: bool },
    /// Keyboard lock indicator toggled.
    KeyboardLock { lock_type: LockType, active: bool },
    /// Disc/device ejected or inserted.
    DeviceEvent { device_name: String, ejected: bool },
    /// Screenshot captured.
    ScreenshotTaken { path: String },
    /// Microphone mute toggled.
    Microphone { muted: bool },
    /// Network connected or disconnected.
    NetworkStatus { connected: bool, name: String },
    /// Battery low warning.
    BatteryLow { percent: u8 },
    /// Custom text OSD for arbitrary notifications.
    Custom { icon: OsdIcon, message: String },
}

/// Keyboard lock types.
// Variants share the `Lock` postfix because the keys themselves are named
// "Caps Lock" / "Num Lock" / "Scroll Lock" — dropping it would obscure the
// reference to the physical keys.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockType {
    CapsLock,
    NumLock,
    ScrollLock,
}

/// Generic icons for custom OSD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsdIcon {
    Info,
    Success,
    Warning,
    Error,
    Speaker,
    Brightness,
    Network,
    Battery,
    Lock,
    Camera,
}

/// Where the OSD appears on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsdPosition {
    TopCenter,
    BottomCenter,
    Center,
    TopRight,
    BottomRight,
}

impl OsdPosition {
    /// Compute the top-left corner given screen size and OSD size.
    pub fn compute_origin(
        self,
        screen_w: f32,
        screen_h: f32,
        osd_w: f32,
        osd_h: f32,
        margin: f32,
    ) -> (f32, f32) {
        match self {
            Self::TopCenter => ((screen_w - osd_w) / 2.0, margin),
            Self::BottomCenter => ((screen_w - osd_w) / 2.0, screen_h - osd_h - margin),
            Self::Center => ((screen_w - osd_w) / 2.0, (screen_h - osd_h) / 2.0),
            Self::TopRight => (screen_w - osd_w - margin, margin),
            Self::BottomRight => (screen_w - osd_w - margin, screen_h - osd_h - margin),
        }
    }
}

// ============================================================================
// OSD configuration
// ============================================================================

/// Configurable OSD display settings.
#[derive(Clone, Debug)]
pub struct OsdConfig {
    /// How long (in milliseconds) before the OSD auto-dismisses.
    pub timeout_ms: u64,
    /// Fade-in duration in milliseconds.
    pub fade_in_ms: u64,
    /// Fade-out duration in milliseconds.
    pub fade_out_ms: u64,
    /// Position on screen.
    pub position: OsdPosition,
    /// Margin from screen edge in pixels.
    pub margin: f32,
    /// OSD panel width.
    pub width: f32,
    /// Background opacity (0–255).
    pub bg_opacity: u8,
    /// Corner radius.
    pub corner_radius: f32,
    /// Whether to show OSD at all.
    pub enabled: bool,
    /// Whether to show volume OSD.
    pub show_volume: bool,
    /// Whether to show brightness OSD.
    pub show_brightness: bool,
    /// Whether to show media OSD.
    pub show_media: bool,
    /// Whether to show lock key indicators.
    pub show_lock_keys: bool,
}

impl Default for OsdConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 2000,
            fade_in_ms: 150,
            fade_out_ms: 300,
            position: OsdPosition::BottomCenter,
            margin: 80.0,
            width: 320.0,
            bg_opacity: 220,
            corner_radius: 12.0,
            enabled: true,
            show_volume: true,
            show_brightness: true,
            show_media: true,
            show_lock_keys: true,
        }
    }
}

// ============================================================================
// OSD state machine
// ============================================================================

/// Animation phase of an OSD overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsdPhase {
    /// Fading in.
    FadingIn,
    /// Fully visible, counting down.
    Visible,
    /// Fading out.
    FadingOut,
    /// Done, should be removed.
    Dismissed,
}

/// A single active OSD overlay instance.
#[derive(Clone, Debug)]
pub struct OsdOverlay {
    /// What to show.
    pub kind: OsdKind,
    /// Current animation phase.
    pub phase: OsdPhase,
    /// Timestamp (ms) when this overlay was created.
    pub created_at: u64,
    /// Timestamp (ms) when the phase last changed.
    pub phase_start: u64,
    /// Current opacity (0.0–1.0) computed from phase timing.
    pub opacity: f32,
    /// Unique ID for this overlay instance.
    pub id: u64,
}

impl OsdOverlay {
    /// Create a new overlay in the FadingIn phase.
    pub fn new(kind: OsdKind, now_ms: u64, id: u64) -> Self {
        Self {
            kind,
            phase: OsdPhase::FadingIn,
            created_at: now_ms,
            phase_start: now_ms,
            opacity: 0.0,
            id,
        }
    }

    /// Advance the overlay's animation state given the current time and config.
    /// Returns true if the overlay should be kept, false if dismissed.
    ///
    /// Phase transitions inside a single tick re-enter the match loop so
    /// that zero-duration phases (e.g. `fade_out_ms = 0`) collapse in one
    /// call instead of needing a second tick to be observed.
    pub fn tick(&mut self, now_ms: u64, config: &OsdConfig) -> bool {
        loop {
            let elapsed = now_ms.saturating_sub(self.phase_start);
            let prev_phase = self.phase;

            match self.phase {
                OsdPhase::FadingIn => {
                    if config.fade_in_ms == 0 || elapsed >= config.fade_in_ms {
                        self.opacity = 1.0;
                        self.phase = OsdPhase::Visible;
                        self.phase_start = now_ms;
                    } else {
                        self.opacity = elapsed as f32 / config.fade_in_ms as f32;
                    }
                }
                OsdPhase::Visible => {
                    self.opacity = 1.0;
                    if elapsed >= config.timeout_ms {
                        self.phase = OsdPhase::FadingOut;
                        self.phase_start = now_ms;
                    }
                }
                OsdPhase::FadingOut => {
                    if config.fade_out_ms == 0 || elapsed >= config.fade_out_ms {
                        self.opacity = 0.0;
                        self.phase = OsdPhase::Dismissed;
                    } else {
                        self.opacity = 1.0 - (elapsed as f32 / config.fade_out_ms as f32);
                    }
                }
                OsdPhase::Dismissed => {}
            }

            if self.phase == prev_phase {
                break;
            }
        }

        self.phase != OsdPhase::Dismissed
    }

    /// Immediately dismiss (start fading out).
    pub fn dismiss(&mut self, now_ms: u64) {
        if self.phase != OsdPhase::FadingOut && self.phase != OsdPhase::Dismissed {
            self.phase = OsdPhase::FadingOut;
            self.phase_start = now_ms;
        }
    }

    /// Reset the timer (e.g., when the same OSD kind fires again).
    pub fn reset_timer(&mut self, now_ms: u64) {
        self.phase = OsdPhase::Visible;
        self.phase_start = now_ms;
        self.opacity = 1.0;
    }
}

// ============================================================================
// OSD manager
// ============================================================================

/// Manages the currently active OSD overlays.
pub struct OsdManager {
    /// Active overlays (newest last).
    overlays: Vec<OsdOverlay>,
    /// Configuration.
    pub config: OsdConfig,
    /// Screen dimensions.
    pub screen_width: f32,
    pub screen_height: f32,
    /// Next overlay ID.
    next_id: u64,
    /// Maximum number of simultaneous overlays.
    pub max_overlays: usize,
}

impl OsdManager {
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            overlays: Vec::new(),
            config: OsdConfig::default(),
            screen_width,
            screen_height,
            next_id: 1,
            max_overlays: 3,
        }
    }

    /// Show an OSD. If the same kind of OSD is already showing, update it
    /// in-place (e.g., volume slider moves — reset timer and update value).
    pub fn show(&mut self, kind: OsdKind, now_ms: u64) {
        if !self.config.enabled {
            return;
        }

        // Check per-kind toggle.
        if !self.is_kind_enabled(&kind) {
            return;
        }

        // Check if there's an existing overlay of the same "category" to update.
        if let Some(existing) = self.find_same_category_mut(&kind) {
            existing.kind = kind;
            existing.reset_timer(now_ms);
            return;
        }

        // Enforce max overlays — dismiss oldest if necessary.
        while self.overlays.len() >= self.max_overlays {
            if let Some(oldest) = self.overlays.first_mut() {
                oldest.dismiss(now_ms);
            }
            // Remove any dismissed.
            self.overlays.retain(|o| o.phase != OsdPhase::Dismissed);
            if self.overlays.len() >= self.max_overlays {
                // Force-remove the oldest.
                self.overlays.remove(0);
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        self.overlays.push(OsdOverlay::new(kind, now_ms, id));
    }

    /// Dismiss all current overlays.
    pub fn dismiss_all(&mut self, now_ms: u64) {
        for overlay in &mut self.overlays {
            overlay.dismiss(now_ms);
        }
    }

    /// Tick all overlays, removing dismissed ones.
    pub fn tick(&mut self, now_ms: u64) {
        for overlay in &mut self.overlays {
            overlay.tick(now_ms, &self.config);
        }
        self.overlays.retain(|o| o.phase != OsdPhase::Dismissed);
    }

    /// Whether any overlay is currently visible.
    pub fn has_visible(&self) -> bool {
        !self.overlays.is_empty()
    }

    /// Number of active overlays.
    pub fn active_count(&self) -> usize {
        self.overlays.len()
    }

    /// Render all active overlays into render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let osd_w = self.config.width;

        for (i, overlay) in self.overlays.iter().enumerate() {
            let osd_h = self.height_for_kind(&overlay.kind);
            let base_alpha = (overlay.opacity * self.config.bg_opacity as f32) as u8;
            let text_alpha = (overlay.opacity * 255.0) as u8;

            // Stack overlays vertically from the position anchor.
            let stack_offset = i as f32 * (osd_h + 8.0);
            let (base_x, base_y) = self.config.position.compute_origin(
                self.screen_width,
                self.screen_height,
                osd_w,
                osd_h,
                self.config.margin,
            );
            let (ox, oy) = match self.config.position {
                OsdPosition::BottomCenter | OsdPosition::BottomRight => {
                    (base_x, base_y - stack_offset)
                }
                _ => (base_x, base_y + stack_offset),
            };

            // Background shadow.
            commands.push(RenderCommand::BoxShadow {
                x: ox,
                y: oy,
                width: osd_w,
                height: osd_h,
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 16.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, base_alpha / 2),
                corner_radii: CornerRadii::all(self.config.corner_radius),
            });

            // Background panel.
            commands.push(RenderCommand::FillRect {
                x: ox,
                y: oy,
                width: osd_w,
                height: osd_h,
                color: Color::rgba(BASE.r, BASE.g, BASE.b, base_alpha),
                corner_radii: CornerRadii::all(self.config.corner_radius),
            });

            // Border.
            commands.push(RenderCommand::StrokeRect {
                x: ox,
                y: oy,
                width: osd_w,
                height: osd_h,
                color: Color::rgba(SURFACE1.r, SURFACE1.g, SURFACE1.b, base_alpha),
                line_width: 1.0,
                corner_radii: CornerRadii::all(self.config.corner_radius),
            });

            // Content.
            self.render_content(overlay, ox, oy, osd_w, osd_h, text_alpha, &mut commands);
        }

        commands
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    fn is_kind_enabled(&self, kind: &OsdKind) -> bool {
        match kind {
            OsdKind::Volume { .. } => self.config.show_volume,
            OsdKind::Brightness { .. } => self.config.show_brightness,
            OsdKind::MediaTrack { .. } | OsdKind::MediaPlayPause { .. } => self.config.show_media,
            OsdKind::KeyboardLock { .. } => self.config.show_lock_keys,
            _ => true,
        }
    }

    /// Check if an existing overlay is the same "category" as the new one.
    fn find_same_category_mut(&mut self, kind: &OsdKind) -> Option<&mut OsdOverlay> {
        self.overlays.iter_mut().find(|o| same_category(&o.kind, kind))
    }

    /// Height of the OSD panel for the given kind.
    fn height_for_kind(&self, kind: &OsdKind) -> f32 {
        match kind {
            OsdKind::Volume { .. } | OsdKind::Brightness { .. } => 72.0,
            OsdKind::MediaTrack { .. } => 100.0,
            OsdKind::MediaPlayPause { .. } => 64.0,
            OsdKind::KeyboardLock { .. } => 56.0,
            OsdKind::DeviceEvent { .. } => 64.0,
            OsdKind::ScreenshotTaken { .. } => 64.0,
            OsdKind::Microphone { .. } => 56.0,
            OsdKind::NetworkStatus { .. } => 64.0,
            OsdKind::BatteryLow { .. } => 64.0,
            OsdKind::Custom { .. } => 64.0,
        }
    }

    /// Render the content for a specific overlay.
    fn render_content(
        &self,
        overlay: &OsdOverlay,
        ox: f32,
        oy: f32,
        osd_w: f32,
        _osd_h: f32,
        text_alpha: u8,
        commands: &mut Vec<RenderCommand>,
    ) {
        match &overlay.kind {
            OsdKind::Volume { level, muted } => {
                self.render_slider_osd(
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    if *muted { "Muted" } else { "Volume" },
                    if *muted { volume_muted_icon() } else { volume_icon(*level) },
                    *level,
                    if *muted { RED } else { BLUE },
                    commands,
                );
            }
            OsdKind::Brightness { level } => {
                self.render_slider_osd(
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    "Brightness",
                    brightness_icon(*level),
                    *level,
                    YELLOW,
                    commands,
                );
            }
            OsdKind::MediaTrack { title, artist, album } => {
                self.render_media_osd(ox, oy, osd_w, text_alpha, title, artist, album, commands);
            }
            OsdKind::MediaPlayPause { playing } => {
                let label = if *playing { "Playing" } else { "Paused" };
                let icon = if *playing { "\u{25B6}" } else { "\u{23F8}" };
                self.render_icon_text_osd(ox, oy, osd_w, text_alpha, icon, label, LAVENDER, commands);
            }
            OsdKind::KeyboardLock { lock_type, active } => {
                let name = match lock_type {
                    LockType::CapsLock => "Caps Lock",
                    LockType::NumLock => "Num Lock",
                    LockType::ScrollLock => "Scroll Lock",
                };
                let status = if *active { "ON" } else { "OFF" };
                let label = format!("{name}: {status}");
                let color = if *active { GREEN } else { SUBTEXT0 };
                self.render_icon_text_osd(ox, oy, osd_w, text_alpha, "\u{1F512}", &label, color, commands);
            }
            OsdKind::DeviceEvent { device_name, ejected } => {
                let label = if *ejected {
                    format!("{device_name} ejected")
                } else {
                    format!("{device_name} connected")
                };
                let color = if *ejected { SUBTEXT0 } else { GREEN };
                self.render_icon_text_osd(ox, oy, osd_w, text_alpha, "\u{23CF}", &label, color, commands);
            }
            OsdKind::ScreenshotTaken { path } => {
                // Truncate path for display.
                let display_path = if path.len() > 30 {
                    format!("...{}", &path[path.len() - 27..])
                } else {
                    path.clone()
                };
                let label = format!("Screenshot: {display_path}");
                self.render_icon_text_osd(ox, oy, osd_w, text_alpha, "\u{1F4F7}", &label, GREEN, commands);
            }
            OsdKind::Microphone { muted } => {
                let label = if *muted { "Mic: Muted" } else { "Mic: Active" };
                let color = if *muted { RED } else { GREEN };
                self.render_icon_text_osd(ox, oy, osd_w, text_alpha, "\u{1F3A4}", label, color, commands);
            }
            OsdKind::NetworkStatus { connected, name } => {
                let label = if *connected {
                    format!("Connected: {name}")
                } else {
                    format!("Disconnected: {name}")
                };
                let color = if *connected { GREEN } else { RED };
                self.render_icon_text_osd(ox, oy, osd_w, text_alpha, "\u{1F310}", &label, color, commands);
            }
            OsdKind::BatteryLow { percent } => {
                let label = format!("Battery Low: {percent}%");
                self.render_icon_text_osd(ox, oy, osd_w, text_alpha, "\u{1F50B}", &label, RED, commands);
            }
            OsdKind::Custom { icon, message } => {
                let (icon_str, color) = icon_info(*icon);
                self.render_icon_text_osd(ox, oy, osd_w, text_alpha, icon_str, message, color, commands);
            }
        }
    }

    /// Render a slider-style OSD (volume, brightness).
    fn render_slider_osd(
        &self,
        ox: f32,
        oy: f32,
        osd_w: f32,
        text_alpha: u8,
        label: &str,
        icon: &str,
        level: u8,
        accent: Color,
        commands: &mut Vec<RenderCommand>,
    ) {
        let padding = 16.0;
        let icon_size = 24.0;

        // Icon.
        commands.push(RenderCommand::Text {
            x: ox + padding,
            y: oy + 14.0,
            text: icon.to_string(),
            font_size: icon_size,
            color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Label and percentage.
        let pct_str = format!("{label}  {level}%");
        commands.push(RenderCommand::Text {
            x: ox + padding + icon_size + 12.0,
            y: oy + 16.0,
            text: pct_str,
            font_size: 14.0,
            color: Color::rgba(TEXT.r, TEXT.g, TEXT.b, text_alpha),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Slider track.
        let track_x = ox + padding;
        let track_y = oy + 48.0;
        let track_w = osd_w - padding * 2.0;
        let track_h = 6.0;

        commands.push(RenderCommand::FillRect {
            x: track_x,
            y: track_y,
            width: track_w,
            height: track_h,
            color: Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, text_alpha),
            corner_radii: CornerRadii::all(3.0),
        });

        // Filled portion.
        let fill_w = track_w * (level.min(100) as f32 / 100.0);
        if fill_w > 0.0 {
            commands.push(RenderCommand::FillRect {
                x: track_x,
                y: track_y,
                width: fill_w,
                height: track_h,
                color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),
                corner_radii: CornerRadii::all(3.0),
            });
        }

        // Knob.
        let knob_x = track_x + fill_w - 5.0;
        let knob_y = track_y - 2.0;
        commands.push(RenderCommand::FillRect {
            x: knob_x,
            y: knob_y,
            width: 10.0,
            height: 10.0,
            color: Color::rgba(TEXT.r, TEXT.g, TEXT.b, text_alpha),
            corner_radii: CornerRadii::all(5.0),
        });
    }

    /// Render a media track OSD with title/artist/album.
    fn render_media_osd(
        &self,
        ox: f32,
        oy: f32,
        osd_w: f32,
        text_alpha: u8,
        title: &str,
        artist: &str,
        album: &str,
        commands: &mut Vec<RenderCommand>,
    ) {
        let padding = 16.0;

        // Music note icon.
        commands.push(RenderCommand::Text {
            x: ox + padding,
            y: oy + 14.0,
            text: "\u{266B}".to_string(),
            font_size: 28.0,
            color: Color::rgba(LAVENDER.r, LAVENDER.g, LAVENDER.b, text_alpha),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let text_x = ox + padding + 40.0;
        let max_text_w = osd_w - padding * 2.0 - 44.0;

        // Title.
        commands.push(RenderCommand::Text {
            x: text_x,
            y: oy + 14.0,
            text: truncate_str(title, 35),
            font_size: 14.0,
            color: Color::rgba(TEXT.r, TEXT.g, TEXT.b, text_alpha),
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_text_w),
        });

        // Artist.
        commands.push(RenderCommand::Text {
            x: text_x,
            y: oy + 38.0,
            text: truncate_str(artist, 40),
            font_size: 12.0,
            color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, text_alpha),
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_text_w),
        });

        // Album (dimmer).
        if !album.is_empty() {
            commands.push(RenderCommand::Text {
                x: text_x,
                y: oy + 58.0,
                text: truncate_str(album, 40),
                font_size: 11.0,
                color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, text_alpha / 2),
                font_weight: FontWeightHint::Light,
                max_width: Some(max_text_w),
            });
        }

        // Decorative bar at bottom.
        commands.push(RenderCommand::FillRect {
            x: ox + padding,
            y: oy + 84.0,
            width: osd_w - padding * 2.0,
            height: 2.0,
            color: Color::rgba(LAVENDER.r, LAVENDER.g, LAVENDER.b, text_alpha / 3),
            corner_radii: CornerRadii::all(1.0),
        });
    }

    /// Render a simple icon + text OSD.
    fn render_icon_text_osd(
        &self,
        ox: f32,
        oy: f32,
        osd_w: f32,
        text_alpha: u8,
        icon: &str,
        label: &str,
        accent: Color,
        commands: &mut Vec<RenderCommand>,
    ) {
        let padding = 16.0;
        let osd_h = self.height_for_kind(&OsdKind::Custom {
            icon: OsdIcon::Info,
            message: String::new(),
        });
        let center_y = oy + (osd_h - 20.0) / 2.0;

        // Icon.
        commands.push(RenderCommand::Text {
            x: ox + padding,
            y: center_y,
            text: icon.to_string(),
            font_size: 20.0,
            color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Label.
        commands.push(RenderCommand::Text {
            x: ox + padding + 32.0,
            y: center_y + 2.0,
            text: truncate_str(label, 35),
            font_size: 14.0,
            color: Color::rgba(TEXT.r, TEXT.g, TEXT.b, text_alpha),
            font_weight: FontWeightHint::Bold,
            max_width: Some(osd_w - padding * 2.0 - 36.0),
        });
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Whether two OSD kinds are the same "category" (should replace each other).
fn same_category(a: &OsdKind, b: &OsdKind) -> bool {
    matches!(
        (a, b),
        (OsdKind::Volume { .. }, OsdKind::Volume { .. })
            | (OsdKind::Brightness { .. }, OsdKind::Brightness { .. })
            | (OsdKind::MediaTrack { .. }, OsdKind::MediaTrack { .. })
            | (OsdKind::MediaPlayPause { .. }, OsdKind::MediaPlayPause { .. })
            | (OsdKind::Microphone { .. }, OsdKind::Microphone { .. })
            | (OsdKind::BatteryLow { .. }, OsdKind::BatteryLow { .. })
    )
}

/// Volume icon based on level.
fn volume_icon(level: u8) -> &'static str {
    if level == 0 {
        "\u{1F507}" // muted
    } else if level < 33 {
        "\u{1F508}" // low
    } else if level < 66 {
        "\u{1F509}" // medium
    } else {
        "\u{1F50A}" // high
    }
}

/// Muted volume icon.
fn volume_muted_icon() -> &'static str {
    "\u{1F507}"
}

/// Brightness icon based on level.
fn brightness_icon(level: u8) -> &'static str {
    if level < 25 {
        "\u{1F315}" // dark
    } else if level < 75 {
        "\u{2600}" // medium
    } else {
        "\u{2B50}" // bright
    }
}

/// Get icon string and color for a generic OsdIcon.
fn icon_info(icon: OsdIcon) -> (&'static str, Color) {
    match icon {
        OsdIcon::Info => ("\u{2139}", BLUE),
        OsdIcon::Success => ("\u{2705}", GREEN),
        OsdIcon::Warning => ("\u{26A0}", YELLOW),
        OsdIcon::Error => ("\u{274C}", RED),
        OsdIcon::Speaker => ("\u{1F50A}", BLUE),
        OsdIcon::Brightness => ("\u{2600}", YELLOW),
        OsdIcon::Network => ("\u{1F310}", GREEN),
        OsdIcon::Battery => ("\u{1F50B}", PEACH),
        OsdIcon::Lock => ("\u{1F512}", LAVENDER),
        OsdIcon::Camera => ("\u{1F4F7}", GREEN),
    }
}

/// Truncate a string to max chars, appending "..." if needed.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

// ============================================================================
// OSD settings UI
// ============================================================================

/// Settings panel for configuring OSD behavior.
pub struct OsdSettingsUI {
    /// Current config being edited.
    pub config: OsdConfig,
    /// Which section is expanded.
    pub expanded_section: usize,
    /// Preview overlay (for testing).
    pub preview_active: bool,
    /// Scroll offset.
    pub scroll_y: f32,
}

impl OsdSettingsUI {
    pub fn new(config: OsdConfig) -> Self {
        Self {
            config,
            expanded_section: 0,
            preview_active: false,
            scroll_y: 0.0,
        }
    }

    /// Render the settings panel.
    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let padding = 12.0;
        let mut cy = y + padding - self.scroll_y;

        // Title.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "On-Screen Display Settings".to_string(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 36.0;

        // Enable toggle.
        let enable_color = if self.config.enabled { GREEN } else { SUBTEXT0 };
        commands.push(RenderCommand::FillRect {
            x: x + padding,
            y: cy,
            width: 40.0,
            height: 20.0,
            color: enable_color,
            corner_radii: CornerRadii::all(10.0),
        });
        let toggle_x = if self.config.enabled {
            x + padding + 22.0
        } else {
            x + padding + 2.0
        };
        commands.push(RenderCommand::FillRect {
            x: toggle_x,
            y: cy + 2.0,
            width: 16.0,
            height: 16.0,
            color: TEXT,
            corner_radii: CornerRadii::all(8.0),
        });
        commands.push(RenderCommand::Text {
            x: x + padding + 52.0,
            y: cy + 2.0,
            text: "Enable OSD overlays".to_string(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cy += 36.0;

        // Position selector.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Position".to_string(),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 22.0;

        let positions = [
            ("Top Center", OsdPosition::TopCenter),
            ("Bottom Center", OsdPosition::BottomCenter),
            ("Center", OsdPosition::Center),
            ("Top Right", OsdPosition::TopRight),
            ("Bottom Right", OsdPosition::BottomRight),
        ];
        for (label, pos) in &positions {
            let selected = self.config.position == *pos;
            let dot_color = if selected { BLUE } else { SURFACE1 };
            commands.push(RenderCommand::FillRect {
                x: x + padding + 4.0,
                y: cy + 2.0,
                width: 12.0,
                height: 12.0,
                color: dot_color,
                corner_radii: CornerRadii::all(6.0),
            });
            commands.push(RenderCommand::Text {
                x: x + padding + 24.0,
                y: cy,
                text: label.to_string(),
                font_size: 12.0,
                color: if selected { TEXT } else { SUBTEXT0 },
                font_weight: if selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: None,
            });
            cy += 22.0;
        }
        cy += 8.0;

        // Timeout slider.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: format!("Timeout: {}ms", self.config.timeout_ms),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 22.0;
        let timeout_frac = (self.config.timeout_ms as f32 - 500.0) / 4500.0;
        let track_w = width - padding * 2.0 - 20.0;
        commands.push(RenderCommand::FillRect {
            x: x + padding,
            y: cy,
            width: track_w,
            height: 4.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(2.0),
        });
        commands.push(RenderCommand::FillRect {
            x: x + padding,
            y: cy,
            width: track_w * timeout_frac.clamp(0.0, 1.0),
            height: 4.0,
            color: BLUE,
            corner_radii: CornerRadii::all(2.0),
        });
        cy += 20.0;

        // Per-kind toggles.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Show OSD for:".to_string(),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 22.0;

        let toggles = [
            ("Volume changes", self.config.show_volume),
            ("Brightness changes", self.config.show_brightness),
            ("Media track info", self.config.show_media),
            ("Lock key indicators", self.config.show_lock_keys),
        ];
        for (label, enabled) in &toggles {
            let check_color = if *enabled { GREEN } else { SURFACE1 };
            commands.push(RenderCommand::FillRect {
                x: x + padding + 4.0,
                y: cy + 1.0,
                width: 14.0,
                height: 14.0,
                color: check_color,
                corner_radii: CornerRadii::all(3.0),
            });
            if *enabled {
                commands.push(RenderCommand::Text {
                    x: x + padding + 6.0,
                    y: cy,
                    text: "\u{2713}".to_string(),
                    font_size: 11.0,
                    color: BASE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            commands.push(RenderCommand::Text {
                x: x + padding + 28.0,
                y: cy + 1.0,
                text: label.to_string(),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cy += 24.0;
        }
        cy += 12.0;

        // Preview button.
        commands.push(RenderCommand::FillRect {
            x: x + padding,
            y: cy,
            width: 120.0,
            height: 32.0,
            color: BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        commands.push(RenderCommand::Text {
            x: x + padding + 20.0,
            y: cy + 8.0,
            text: "Preview OSD".to_string(),
            font_size: 13.0,
            color: BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        commands
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: create a default manager.
    fn make_manager() -> OsdManager {
        OsdManager::new(1920.0, 1080.0)
    }

    // ---- OsdPosition ----

    #[test]
    fn position_top_center() {
        let (x, y) = OsdPosition::TopCenter.compute_origin(1920.0, 1080.0, 320.0, 72.0, 80.0);
        assert!((x - 800.0).abs() < 0.01);
        assert!((y - 80.0).abs() < 0.01);
    }

    #[test]
    fn position_bottom_center() {
        let (x, y) = OsdPosition::BottomCenter.compute_origin(1920.0, 1080.0, 320.0, 72.0, 80.0);
        assert!((x - 800.0).abs() < 0.01);
        assert!((y - 928.0).abs() < 0.01);
    }

    #[test]
    fn position_center() {
        let (x, y) = OsdPosition::Center.compute_origin(1920.0, 1080.0, 320.0, 72.0, 80.0);
        assert!((x - 800.0).abs() < 0.01);
        assert!((y - 504.0).abs() < 0.01);
    }

    #[test]
    fn position_top_right() {
        let (x, y) = OsdPosition::TopRight.compute_origin(1920.0, 1080.0, 320.0, 72.0, 80.0);
        assert!((x - 1520.0).abs() < 0.01);
        assert!((y - 80.0).abs() < 0.01);
    }

    #[test]
    fn position_bottom_right() {
        let (x, y) = OsdPosition::BottomRight.compute_origin(1920.0, 1080.0, 320.0, 72.0, 80.0);
        assert!((x - 1520.0).abs() < 0.01);
        assert!((y - 928.0).abs() < 0.01);
    }

    // ---- OsdConfig defaults ----

    #[test]
    fn default_config() {
        let c = OsdConfig::default();
        assert_eq!(c.timeout_ms, 2000);
        assert_eq!(c.fade_in_ms, 150);
        assert_eq!(c.fade_out_ms, 300);
        assert_eq!(c.position, OsdPosition::BottomCenter);
        assert!(c.enabled);
        assert!(c.show_volume);
        assert!(c.show_brightness);
        assert!(c.show_media);
        assert!(c.show_lock_keys);
    }

    // ---- OsdOverlay lifecycle ----

    #[test]
    fn overlay_new_starts_fading_in() {
        let o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 1000, 1);
        assert_eq!(o.phase, OsdPhase::FadingIn);
        assert_eq!(o.created_at, 1000);
        assert!((o.opacity - 0.0).abs() < 0.01);
    }

    #[test]
    fn overlay_fade_in_progresses() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 0, 1);
        o.tick(75, &config); // halfway through 150ms fade-in
        assert_eq!(o.phase, OsdPhase::FadingIn);
        assert!((o.opacity - 0.5).abs() < 0.01);
    }

    #[test]
    fn overlay_transitions_to_visible() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 0, 1);
        o.tick(150, &config);
        assert_eq!(o.phase, OsdPhase::Visible);
        assert!((o.opacity - 1.0).abs() < 0.01);
    }

    #[test]
    fn overlay_visible_stays_until_timeout() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 0, 1);
        o.tick(150, &config); // now Visible
        o.tick(1000, &config); // still within 2000ms timeout
        assert_eq!(o.phase, OsdPhase::Visible);
    }

    #[test]
    fn overlay_transitions_to_fading_out() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 0, 1);
        o.tick(150, &config); // Visible
        o.tick(2200, &config); // past timeout
        assert_eq!(o.phase, OsdPhase::FadingOut);
    }

    #[test]
    fn overlay_fading_out_progresses() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 0, 1);
        o.tick(150, &config); // Visible
        o.tick(2200, &config); // FadingOut starts at ~2200
        o.tick(2350, &config); // 150ms into 300ms fade-out
        assert_eq!(o.phase, OsdPhase::FadingOut);
        assert!(o.opacity > 0.0 && o.opacity < 1.0);
    }

    #[test]
    fn overlay_dismissed_after_fade_out() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 0, 1);
        o.tick(150, &config);
        o.tick(2200, &config);
        let alive = o.tick(2600, &config);
        assert!(!alive);
        assert_eq!(o.phase, OsdPhase::Dismissed);
    }

    #[test]
    fn overlay_dismiss_starts_fade_out() {
        let mut o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 0, 1);
        o.phase = OsdPhase::Visible;
        o.opacity = 1.0;
        o.dismiss(500);
        assert_eq!(o.phase, OsdPhase::FadingOut);
        assert_eq!(o.phase_start, 500);
    }

    #[test]
    fn overlay_reset_timer_goes_visible() {
        let mut o = OsdOverlay::new(OsdKind::Volume { level: 50, muted: false }, 0, 1);
        o.phase = OsdPhase::FadingOut;
        o.opacity = 0.3;
        o.reset_timer(1000);
        assert_eq!(o.phase, OsdPhase::Visible);
        assert!((o.opacity - 1.0).abs() < 0.01);
        assert_eq!(o.phase_start, 1000);
    }

    #[test]
    fn overlay_zero_fade_in_skips_to_visible() {
        let mut config = OsdConfig::default();
        config.fade_in_ms = 0;
        let mut o = OsdOverlay::new(OsdKind::Brightness { level: 80 }, 0, 1);
        o.tick(0, &config);
        assert_eq!(o.phase, OsdPhase::Visible);
        assert!((o.opacity - 1.0).abs() < 0.01);
    }

    #[test]
    fn overlay_zero_fade_out_dismisses_immediately() {
        let mut config = OsdConfig::default();
        config.fade_out_ms = 0;
        let mut o = OsdOverlay::new(OsdKind::Brightness { level: 80 }, 0, 1);
        o.phase = OsdPhase::FadingOut;
        o.phase_start = 100;
        let alive = o.tick(100, &config);
        assert!(!alive);
        assert_eq!(o.phase, OsdPhase::Dismissed);
    }

    // ---- OsdManager ----

    #[test]
    fn manager_show_adds_overlay() {
        let mut mgr = make_manager();
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        assert_eq!(mgr.active_count(), 1);
        assert!(mgr.has_visible());
    }

    #[test]
    fn manager_disabled_ignores_show() {
        let mut mgr = make_manager();
        mgr.config.enabled = false;
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn manager_per_kind_toggle() {
        let mut mgr = make_manager();
        mgr.config.show_volume = false;
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        assert_eq!(mgr.active_count(), 0);

        mgr.show(OsdKind::Brightness { level: 80 }, 0);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn manager_same_category_updates_inplace() {
        let mut mgr = make_manager();
        mgr.show(OsdKind::Volume { level: 30, muted: false }, 0);
        assert_eq!(mgr.active_count(), 1);

        mgr.show(OsdKind::Volume { level: 60, muted: false }, 100);
        assert_eq!(mgr.active_count(), 1); // still 1, updated in place
        if let OsdKind::Volume { level, .. } = &mgr.overlays[0].kind {
            assert_eq!(*level, 60);
        } else {
            panic!("Expected Volume kind");
        }
    }

    #[test]
    fn manager_different_categories_stack() {
        let mut mgr = make_manager();
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        mgr.show(OsdKind::Brightness { level: 80 }, 0);
        assert_eq!(mgr.active_count(), 2);
    }

    #[test]
    fn manager_max_overlays_enforced() {
        let mut mgr = make_manager();
        mgr.max_overlays = 2;
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        mgr.show(OsdKind::Brightness { level: 80 }, 0);
        mgr.show(OsdKind::KeyboardLock { lock_type: LockType::CapsLock, active: true }, 0);
        assert!(mgr.active_count() <= 2);
    }

    #[test]
    fn manager_tick_removes_dismissed() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.config.timeout_ms = 100;
        mgr.config.fade_out_ms = 0;
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        assert_eq!(mgr.active_count(), 1);

        mgr.tick(0); // goes to Visible
        mgr.tick(200); // past timeout, fades out instantly
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn manager_dismiss_all() {
        let mut mgr = make_manager();
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        mgr.show(OsdKind::Brightness { level: 80 }, 0);
        mgr.dismiss_all(100);
        for o in &mgr.overlays {
            assert_eq!(o.phase, OsdPhase::FadingOut);
        }
    }

    #[test]
    fn manager_render_returns_commands() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn manager_render_empty_when_no_overlays() {
        let mgr = make_manager();
        let cmds = mgr.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn manager_render_brightness() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Brightness { level: 100 }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_media_track() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::MediaTrack {
                title: "Test Song".into(),
                artist: "Test Artist".into(),
                album: "Test Album".into(),
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_media_play_pause() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::MediaPlayPause { playing: true }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_keyboard_lock() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::KeyboardLock { lock_type: LockType::CapsLock, active: true }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_device_event() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::DeviceEvent { device_name: "USB Drive".into(), ejected: false }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_screenshot() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::ScreenshotTaken { path: "/home/user/screenshot.png".into() }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_microphone() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Microphone { muted: true }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_network_status() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::NetworkStatus { connected: true, name: "WiFi".into() }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_battery_low() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::BatteryLow { percent: 5 }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_custom() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Custom { icon: OsdIcon::Warning, message: "Disk full".into() }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    // ---- same_category ----

    #[test]
    fn same_category_volume() {
        assert!(same_category(
            &OsdKind::Volume { level: 30, muted: false },
            &OsdKind::Volume { level: 60, muted: true },
        ));
    }

    #[test]
    fn same_category_brightness() {
        assert!(same_category(
            &OsdKind::Brightness { level: 10 },
            &OsdKind::Brightness { level: 90 },
        ));
    }

    #[test]
    fn different_categories() {
        assert!(!same_category(
            &OsdKind::Volume { level: 50, muted: false },
            &OsdKind::Brightness { level: 50 },
        ));
    }

    #[test]
    fn keyboard_locks_not_same_category() {
        // Lock events don't merge — each lock toggle is independent.
        assert!(!same_category(
            &OsdKind::KeyboardLock { lock_type: LockType::CapsLock, active: true },
            &OsdKind::KeyboardLock { lock_type: LockType::NumLock, active: true },
        ));
    }

    // ---- Icon helpers ----

    #[test]
    fn volume_icon_levels() {
        assert_eq!(volume_icon(0), "\u{1F507}");
        assert_eq!(volume_icon(10), "\u{1F508}");
        assert_eq!(volume_icon(50), "\u{1F509}");
        assert_eq!(volume_icon(100), "\u{1F50A}");
    }

    #[test]
    fn brightness_icon_levels() {
        assert_eq!(brightness_icon(10), "\u{1F315}");
        assert_eq!(brightness_icon(50), "\u{2600}");
        assert_eq!(brightness_icon(90), "\u{2B50}");
    }

    #[test]
    fn icon_info_all_variants() {
        let variants = [
            OsdIcon::Info,
            OsdIcon::Success,
            OsdIcon::Warning,
            OsdIcon::Error,
            OsdIcon::Speaker,
            OsdIcon::Brightness,
            OsdIcon::Network,
            OsdIcon::Battery,
            OsdIcon::Lock,
            OsdIcon::Camera,
        ];
        for v in variants {
            let (s, _c) = icon_info(v);
            assert!(!s.is_empty());
        }
    }

    // ---- truncate_str ----

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let result = truncate_str("this is a really long string", 15);
        assert!(result.ends_with("..."));
        assert!(result.chars().count() <= 15);
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate_str("", 10), "");
    }

    // ---- height_for_kind ----

    #[test]
    fn height_volume() {
        let mgr = make_manager();
        assert!((mgr.height_for_kind(&OsdKind::Volume { level: 50, muted: false }) - 72.0).abs() < 0.01);
    }

    #[test]
    fn height_brightness() {
        let mgr = make_manager();
        assert!((mgr.height_for_kind(&OsdKind::Brightness { level: 50 }) - 72.0).abs() < 0.01);
    }

    #[test]
    fn height_media_track() {
        let mgr = make_manager();
        let h = mgr.height_for_kind(&OsdKind::MediaTrack {
            title: "T".into(),
            artist: "A".into(),
            album: "B".into(),
        });
        assert!((h - 100.0).abs() < 0.01);
    }

    #[test]
    fn height_keyboard_lock() {
        let mgr = make_manager();
        let h = mgr.height_for_kind(&OsdKind::KeyboardLock { lock_type: LockType::NumLock, active: true });
        assert!((h - 56.0).abs() < 0.01);
    }

    // ---- Settings UI ----

    #[test]
    fn settings_ui_render_not_empty() {
        let ui = OsdSettingsUI::new(OsdConfig::default());
        let cmds = ui.render(0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn settings_ui_render_with_disabled() {
        let mut config = OsdConfig::default();
        config.enabled = false;
        let ui = OsdSettingsUI::new(config);
        let cmds = ui.render(0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    // ---- Multiple overlays stacking ----

    #[test]
    fn stacked_overlays_render_multiple_panels() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Volume { level: 50, muted: false }, 0);
        mgr.show(OsdKind::KeyboardLock { lock_type: LockType::CapsLock, active: true }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        // Should have commands for both overlays (at least 6 each: shadow + bg + border + content).
        assert!(cmds.len() >= 10);
    }

    // ---- Muted volume rendering ----

    #[test]
    fn volume_muted_renders() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Volume { level: 0, muted: true }, 0);
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(cmds.len() > 3);
    }

    // ---- Edge cases ----

    #[test]
    fn volume_level_clamped_at_100() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Volume { level: 150, muted: false }, 0);
        mgr.tick(0);
        // Should not panic during render.
        let cmds = mgr.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn screenshot_long_path_truncated() {
        let long_path = "/home/user/very/long/path/to/some/nested/directory/screenshot_2026_05_17_12345.png";
        let result = truncate_str(long_path, 30);
        assert!(result.chars().count() <= 30);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn media_track_empty_album() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::MediaTrack {
                title: "Song".into(),
                artist: "Artist".into(),
                album: String::new(),
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn rapid_volume_updates_keep_single_overlay() {
        let mut mgr = make_manager();
        for i in 0..20 {
            mgr.show(OsdKind::Volume { level: i * 5, muted: false }, i as u64 * 50);
        }
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn overlay_full_lifecycle() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 100;
        mgr.config.timeout_ms = 200;
        mgr.config.fade_out_ms = 100;

        mgr.show(OsdKind::Brightness { level: 75 }, 0);
        assert_eq!(mgr.active_count(), 1);

        mgr.tick(50); // fading in
        assert!(mgr.has_visible());

        mgr.tick(100); // visible
        assert!(mgr.has_visible());

        mgr.tick(300); // still visible (200ms timeout from phase_start ~100)
        // Should be transitioning to FadingOut around 300.
        mgr.tick(350); // fading out
        assert!(mgr.has_visible()); // still fading

        mgr.tick(500); // should be dismissed
        assert!(!mgr.has_visible());
    }
}
