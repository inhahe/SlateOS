//! On-Screen Display (OSD) overlay system.
//!
//! Renders transient popups for system feedback: volume/brightness changes,
//! media track notifications, caps/num lock indicators, and ejection notices.
//! OSD overlays appear centered near the bottom of the screen and auto-dismiss
//! after a configurable timeout.

use appearance::Palette;
use guitk::color::Color;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// ============================================================================
// Colour
// ============================================================================
//
// Eleven hardcoded Catppuccin Mocha constants used to live here. They are gone;
// both renderers in this file take the resolved `&Palette` and read roles off
// it, so the OSD follows the desktop's mode and accent. Four judgements were
// made in the process, and the first is the one worth reading:
//
// 1. **Nothing the overlay draws follows the accent.** An OSD is pure
//    feedback: there is nothing in it to select, nothing to drag, nothing that
//    says "you are here". Every colour it uses is a surface, a piece of text,
//    or a reading. So `p.accent` does not appear anywhere in `OsdManager` at
//    all, and `no_colour_the_overlay_draws_ever_follows_the_accent` says so
//    out loud rather than leaving it as an accident of the mapping.
//
// 2. **Volume and brightness are a category pair, so both freeze.** The volume
//    overlay's bar is the thing here that looks most like a slider, and it is
//    the thing that most needs not to follow the accent: blue means volume and
//    yellow means brightness, and two readings told apart by hue stop being
//    two readings the moment one accent claims both. This is module 22's meter
//    rule (`widgets.rs`), and it is the deliberate counterpart to module 23's
//    (`sound_settings.rs`), where the volume bar *does* take the accent —
//    because there you can drag it. One rule stated twice: the accent marks
//    what you can move, not what you are being told.
//
// 3. **Ink drawn on a coloured fill is derived, never named.** The tick inside
//    an enabled checkbox is `readable_on(p.green)` and the Preview button's
//    label is `p.on_accent()`. Both were Mocha `base` — a dark ink that is
//    invisible on a Latte green and on a pale accent. A colour drawn *on* a
//    fill has to be computed from that fill.
//
// 4. **On/off pairs stay frozen.** Green/`subtext0`, green/`surface1` and
//    green/red report a state; they do not decorate one. Module 19's rule.
//
// The settings panel below the overlay is a different surface with a different
// answer: it has three accent sites — the selected position dot, the timeout
// slider's fill and the Preview button — because those are, respectively, a
// selection, a drag and an invitation.
//
// The overlay's shadow stays `rgba(0, 0, 0, base_alpha / 2)` rather than
// becoming `Palette::shadow()`, for the reason module 22 gives for the
// per-widget shadow: its depth is a function of the overlay's own fade, so a
// half-faded OSD must cast a half-faded shadow.

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

/// Size the icon+text OSD draws its label at.
///
/// Named, with the two below, because a caller that has to fit something into
/// that label — the screenshot path, which is elided from its *start* so the
/// file name survives — has to measure it in the face it will be drawn in. A
/// caller measuring at a guessed size is the same bug as counting characters,
/// one level up.
const OSD_LABEL_SIZE: f32 = 14.0;
/// Weight it is drawn at. Bold glyphs are wider than regular ones at the same
/// size, so measuring bold text as regular under-measures it.
const OSD_LABEL_WEIGHT: FontWeightHint = FontWeightHint::Bold;
/// Padding from the overlay's edge to its content.
const OSD_PADDING: f32 = 16.0;

/// Room the icon+text OSD's label has, given the overlay's width.
fn osd_label_width(osd_w: f32) -> f32 {
    (osd_w - OSD_PADDING * 2.0 - 36.0).max(0.0)
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
    /// Source of overlay IDs.
    ids: IdSeq,
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
            ids: IdSeq::new(),
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

        let id = self.ids.issue_infallible();
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
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
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
                color: Color::rgba(p.base.r, p.base.g, p.base.b, base_alpha),
                corner_radii: CornerRadii::all(self.config.corner_radius),
            });

            // Border.
            commands.push(RenderCommand::StrokeRect {
                x: ox,
                y: oy,
                width: osd_w,
                height: osd_h,
                color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, base_alpha),
                line_width: 1.0,
                corner_radii: CornerRadii::all(self.config.corner_radius),
            });

            // Content.
            self.render_content(p, overlay, ox, oy, osd_w, osd_h, text_alpha, &mut commands);
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
        self.overlays
            .iter_mut()
            .find(|o| same_category(&o.kind, kind))
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
        p: &Palette,
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
                    p,
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    if *muted { "Muted" } else { "Volume" },
                    if *muted {
                        volume_muted_icon()
                    } else {
                        volume_icon(*level)
                    },
                    *level,
                    if *muted { p.red } else { p.blue },
                    commands,
                );
            }
            OsdKind::Brightness { level } => {
                self.render_slider_osd(
                    p,
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    "Brightness",
                    brightness_icon(*level),
                    *level,
                    p.yellow,
                    commands,
                );
            }
            OsdKind::MediaTrack {
                title,
                artist,
                album,
            } => {
                self.render_media_osd(p, ox, oy, osd_w, text_alpha, title, artist, album, commands);
            }
            OsdKind::MediaPlayPause { playing } => {
                let label = if *playing { "Playing" } else { "Paused" };
                let icon = if *playing { "\u{25B6}" } else { "\u{23F8}" };
                self.render_icon_text_osd(
                    p, ox, oy, osd_w, text_alpha, icon, label, p.lavender, commands,
                );
            }
            OsdKind::KeyboardLock { lock_type, active } => {
                let name = match lock_type {
                    LockType::CapsLock => "Caps Lock",
                    LockType::NumLock => "Num Lock",
                    LockType::ScrollLock => "Scroll Lock",
                };
                let status = if *active { "ON" } else { "OFF" };
                let label = format!("{name}: {status}");
                let color = if *active { p.green } else { p.subtext0 };
                self.render_icon_text_osd(
                    p,
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    "\u{1F512}",
                    &label,
                    color,
                    commands,
                );
            }
            OsdKind::DeviceEvent {
                device_name,
                ejected,
            } => {
                let label = if *ejected {
                    format!("{device_name} ejected")
                } else {
                    format!("{device_name} connected")
                };
                let color = if *ejected { p.subtext0 } else { p.green };
                self.render_icon_text_osd(
                    p, ox, oy, osd_w, text_alpha, "\u{23CF}", &label, color, commands,
                );
            }
            OsdKind::ScreenshotTaken { path } => {
                // The path is shortened from its *start*: the file name is
                // what identifies the shot, and a path elided the usual way
                // reads "/home/user/pictures/scree…", which names nothing.
                //
                // It used to be `&path[path.len() - 27..]` — a *byte* offset
                // into a `str`, which aborts the process when it lands inside a
                // character. A screenshot path contains the user's home
                // directory, so any account named in a non-Latin script took
                // the desktop shell down on every screenshot. The 27 was also a
                // count with no relation to the width beside it.
                let prefix = "Screenshot: ";
                let room = (osd_label_width(osd_w)
                    - text::measure(prefix, OSD_LABEL_SIZE, OSD_LABEL_WEIGHT))
                .max(0.0);
                let display_path =
                    text::elide_start(path, room, "\u{2026}", OSD_LABEL_SIZE, OSD_LABEL_WEIGHT);
                let label = format!("{prefix}{display_path}");
                self.render_icon_text_osd(
                    p,
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    "\u{1F4F7}",
                    &label,
                    p.green,
                    commands,
                );
            }
            OsdKind::Microphone { muted } => {
                let label = if *muted { "Mic: Muted" } else { "Mic: Active" };
                let color = if *muted { p.red } else { p.green };
                self.render_icon_text_osd(
                    p,
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    "\u{1F3A4}",
                    label,
                    color,
                    commands,
                );
            }
            OsdKind::NetworkStatus { connected, name } => {
                let label = if *connected {
                    format!("Connected: {name}")
                } else {
                    format!("Disconnected: {name}")
                };
                let color = if *connected { p.green } else { p.red };
                self.render_icon_text_osd(
                    p,
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    "\u{1F310}",
                    &label,
                    color,
                    commands,
                );
            }
            OsdKind::BatteryLow { percent } => {
                let label = format!("Battery Low: {percent}%");
                self.render_icon_text_osd(
                    p,
                    ox,
                    oy,
                    osd_w,
                    text_alpha,
                    "\u{1F50B}",
                    &label,
                    p.red,
                    commands,
                );
            }
            OsdKind::Custom { icon, message } => {
                let (icon_str, color) = icon_info(p, *icon);
                self.render_icon_text_osd(
                    p, ox, oy, osd_w, text_alpha, icon_str, message, color, commands,
                );
            }
        }
    }

    /// Render a slider-style OSD (volume, brightness).
    fn render_slider_osd(
        &self,
        p: &Palette,
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
            overflow: TextOverflow::Clip,
        });

        // Label and percentage.
        let pct_str = format!("{label}  {level}%");
        commands.push(RenderCommand::Text {
            x: ox + padding + icon_size + 12.0,
            y: oy + 16.0,
            text: pct_str,
            font_size: 14.0,
            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Slider track.
        let track_x = ox + padding;
        let track_y = oy + 48.0;
        let track_w = osd_w - padding * 2.0;
        let track_h = 6.0;

        crate::slider::Slider {
            x: track_x,
            y: track_y,
            width: track_w,
            height: track_h,
            frac: level.min(100) as f32 / 100.0,
            thumb: 10.0,
            track: p.surface0,
            fill: accent,
            p,
            // The overlay fades in and out as one thing, so the slider takes
            // the same opacity as the label above it.
            alpha: text_alpha,
        }
        .draw(commands);
    }

    /// Render a media track OSD with title/artist/album.
    fn render_media_osd(
        &self,
        p: &Palette,
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
            color: Color::rgba(p.lavender.r, p.lavender.g, p.lavender.b, text_alpha),
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let text_x = ox + padding + 40.0;
        let max_text_w = osd_w - padding * 2.0 - 44.0;

        // Title.
        commands.push(RenderCommand::Text {
            x: text_x,
            y: oy + 14.0,
            text: title.to_string(),
            font_size: 14.0,
            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_text_w),
            overflow: TextOverflow::Ellipsis,
        });

        // Artist.
        commands.push(RenderCommand::Text {
            x: text_x,
            y: oy + 38.0,
            text: artist.to_string(),
            font_size: 12.0,
            color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha),
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_text_w),
            overflow: TextOverflow::Ellipsis,
        });

        // Album (dimmer).
        if !album.is_empty() {
            commands.push(RenderCommand::Text {
                x: text_x,
                y: oy + 58.0,
                text: album.to_string(),
                font_size: 11.0,
                color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha / 2),
                font_weight: FontWeightHint::Light,
                max_width: Some(max_text_w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Decorative bar at bottom.
        commands.push(RenderCommand::FillRect {
            x: ox + padding,
            y: oy + 84.0,
            width: osd_w - padding * 2.0,
            height: 2.0,
            color: Color::rgba(p.lavender.r, p.lavender.g, p.lavender.b, text_alpha / 3),
            corner_radii: CornerRadii::all(1.0),
        });
    }

    /// Render a simple icon + text OSD.
    fn render_icon_text_osd(
        &self,
        p: &Palette,
        ox: f32,
        oy: f32,
        osd_w: f32,
        text_alpha: u8,
        icon: &str,
        label: &str,
        accent: Color,
        commands: &mut Vec<RenderCommand>,
    ) {
        let padding = OSD_PADDING;
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
            overflow: TextOverflow::Clip,
        });

        // Label.
        commands.push(RenderCommand::Text {
            x: ox + padding + 32.0,
            y: center_y + 2.0,
            text: label.to_string(),
            font_size: OSD_LABEL_SIZE,
            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),
            font_weight: OSD_LABEL_WEIGHT,
            max_width: Some(osd_label_width(osd_w)),
            overflow: TextOverflow::Ellipsis,
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
            | (
                OsdKind::MediaPlayPause { .. },
                OsdKind::MediaPlayPause { .. }
            )
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
fn icon_info(p: &Palette, icon: OsdIcon) -> (&'static str, Color) {
    match icon {
        OsdIcon::Info => ("\u{2139}", p.blue),
        OsdIcon::Success => ("\u{2705}", p.green),
        OsdIcon::Warning => ("\u{26A0}", p.yellow),
        OsdIcon::Error => ("\u{274C}", p.red),
        OsdIcon::Speaker => ("\u{1F50A}", p.blue),
        OsdIcon::Brightness => ("\u{2600}", p.yellow),
        OsdIcon::Network => ("\u{1F310}", p.green),
        OsdIcon::Battery => ("\u{1F50B}", p.peach),
        OsdIcon::Lock => ("\u{1F512}", p.lavender),
        OsdIcon::Camera => ("\u{1F4F7}", p.green),
    }
}

// A `truncate_str(s, max_chars)` used to cut the title, artist, album and
// label to a fixed character budget — 35 or 40 — before handing them over.
// It is gone, and the strings go to the renderer whole.
//
// Every one of its four call sites already passed `max_width` and
// `TextOverflow::Ellipsis`, which is the instruction to fit the text and mark
// the cut, carried out by measuring the face the text is drawn in. A character
// budget cannot express that: 35 characters is a different width in every
// string, and none of the four budgets bore any relation to the `max_text_w`
// sitting three lines below it. The two answers disagreed in both directions —
// the budget cut "Symphony No. 9 in D minor, Op. 125" that fitted, and let a
// run of capitals past that did not — and when they disagreed the budget won,
// because it cut first. It also appended a three-dot ASCII "..." where the
// renderer marks with a single "…", so the same OSD showed both.

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
    pub fn render(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let padding = 12.0;
        let mut cy = y + padding - self.scroll_y;

        // Title.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "On-Screen Display Settings".to_string(),
            font_size: 18.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 36.0;

        // Enable toggle.
        let enable_color = if self.config.enabled {
            p.green
        } else {
            p.subtext0
        };
        commands.extend(crate::switch::switch(
            x + padding,
            cy,
            40.0,
            20.0,
            self.config.enabled,
            enable_color,
        ));
        commands.push(RenderCommand::Text {
            x: x + padding + 52.0,
            y: cy + 2.0,
            text: "Enable OSD overlays".to_string(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 36.0;

        // Position selector.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Position".to_string(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            let dot_color = if selected { p.accent } else { p.surface1 };
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
                color: if selected { p.text } else { p.subtext0 },
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
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
            color: p.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 22.0;
        let timeout_frac = (self.config.timeout_ms as f32 - 500.0) / 4500.0;
        let track_w = width - padding * 2.0 - 20.0;
        commands.push(RenderCommand::FillRect {
            x: x + padding,
            y: cy,
            width: track_w,
            height: 4.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(2.0),
        });
        commands.push(RenderCommand::FillRect {
            x: x + padding,
            y: cy,
            width: track_w * timeout_frac.clamp(0.0, 1.0),
            height: 4.0,
            color: p.accent,
            corner_radii: CornerRadii::all(2.0),
        });
        cy += 20.0;

        // Per-kind toggles.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Show OSD for:".to_string(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 22.0;

        let toggles = [
            ("Volume changes", self.config.show_volume),
            ("Brightness changes", self.config.show_brightness),
            ("Media track info", self.config.show_media),
            ("Lock key indicators", self.config.show_lock_keys),
        ];
        for (label, enabled) in &toggles {
            let check_color = if *enabled { p.green } else { p.surface1 };
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
                    color: appearance::readable_on(p.green),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            commands.push(RenderCommand::Text {
                x: x + padding + 28.0,
                y: cy + 1.0,
                text: label.to_string(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
            color: p.accent,
            corner_radii: CornerRadii::all(6.0),
        });
        commands.push(RenderCommand::Text {
            x: x + padding + 20.0,
            y: cy + 8.0,
            text: "Preview OSD".to_string(),
            font_size: 13.0,
            color: p.on_accent(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        commands
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

    use super::*;
    use crate::palette_check::assert_drawn_from;

    /// The dark palette, which is what every constant deleted from this module
    /// used to be: the tests below that predate the conversion asserted against
    /// Mocha values, and asking for Mocha explicitly keeps them saying exactly
    /// what they said before rather than quietly re-baselining onto whatever
    /// the default happens to become.
    fn mocha() -> Palette {
        Palette::for_mode(false)
    }

    fn rgb(c: Color) -> (u8, u8, u8) {
        (c.r, c.g, c.b)
    }

    /// Accents that are not a role of either palette, so that "this followed
    /// the accent" and "this is a leftover constant" can never be confused.
    const SAFE_ACCENTS: [Color; 4] = [
        appearance::MAUVE,
        appearance::TEAL,
        appearance::SAPPHIRE,
        appearance::PINK,
    ];

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
        let o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            1000,
            1,
        );
        assert_eq!(o.phase, OsdPhase::FadingIn);
        assert_eq!(o.created_at, 1000);
        assert!((o.opacity - 0.0).abs() < 0.01);
    }

    #[test]
    fn overlay_fade_in_progresses() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
            1,
        );
        o.tick(75, &config); // halfway through 150ms fade-in
        assert_eq!(o.phase, OsdPhase::FadingIn);
        assert!((o.opacity - 0.5).abs() < 0.01);
    }

    #[test]
    fn overlay_transitions_to_visible() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
            1,
        );
        o.tick(150, &config);
        assert_eq!(o.phase, OsdPhase::Visible);
        assert!((o.opacity - 1.0).abs() < 0.01);
    }

    #[test]
    fn overlay_visible_stays_until_timeout() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
            1,
        );
        o.tick(150, &config); // now Visible
        o.tick(1000, &config); // still within 2000ms timeout
        assert_eq!(o.phase, OsdPhase::Visible);
    }

    #[test]
    fn overlay_transitions_to_fading_out() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
            1,
        );
        o.tick(150, &config); // Visible
        o.tick(2200, &config); // past timeout
        assert_eq!(o.phase, OsdPhase::FadingOut);
    }

    #[test]
    fn overlay_fading_out_progresses() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
            1,
        );
        o.tick(150, &config); // Visible
        o.tick(2200, &config); // FadingOut starts at ~2200
        o.tick(2350, &config); // 150ms into 300ms fade-out
        assert_eq!(o.phase, OsdPhase::FadingOut);
        assert!(o.opacity > 0.0 && o.opacity < 1.0);
    }

    #[test]
    fn overlay_dismissed_after_fade_out() {
        let config = OsdConfig::default();
        let mut o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
            1,
        );
        o.tick(150, &config);
        o.tick(2200, &config);
        let alive = o.tick(2600, &config);
        assert!(!alive);
        assert_eq!(o.phase, OsdPhase::Dismissed);
    }

    #[test]
    fn overlay_dismiss_starts_fade_out() {
        let mut o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
            1,
        );
        o.phase = OsdPhase::Visible;
        o.opacity = 1.0;
        o.dismiss(500);
        assert_eq!(o.phase, OsdPhase::FadingOut);
        assert_eq!(o.phase_start, 500);
    }

    #[test]
    fn overlay_reset_timer_goes_visible() {
        let mut o = OsdOverlay::new(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
            1,
        );
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
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
        assert_eq!(mgr.active_count(), 1);
        assert!(mgr.has_visible());
    }

    #[test]
    fn manager_disabled_ignores_show() {
        let mut mgr = make_manager();
        mgr.config.enabled = false;
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn manager_per_kind_toggle() {
        let mut mgr = make_manager();
        mgr.config.show_volume = false;
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
        assert_eq!(mgr.active_count(), 0);

        mgr.show(OsdKind::Brightness { level: 80 }, 0);
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn manager_same_category_updates_inplace() {
        let mut mgr = make_manager();
        mgr.show(
            OsdKind::Volume {
                level: 30,
                muted: false,
            },
            0,
        );
        assert_eq!(mgr.active_count(), 1);

        mgr.show(
            OsdKind::Volume {
                level: 60,
                muted: false,
            },
            100,
        );
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
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
        mgr.show(OsdKind::Brightness { level: 80 }, 0);
        assert_eq!(mgr.active_count(), 2);
    }

    #[test]
    fn manager_max_overlays_enforced() {
        let mut mgr = make_manager();
        mgr.max_overlays = 2;
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
        mgr.show(OsdKind::Brightness { level: 80 }, 0);
        mgr.show(
            OsdKind::KeyboardLock {
                lock_type: LockType::CapsLock,
                active: true,
            },
            0,
        );
        assert!(mgr.active_count() <= 2);
    }

    #[test]
    fn manager_tick_removes_dismissed() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.config.timeout_ms = 100;
        mgr.config.fade_out_ms = 0;
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
        assert_eq!(mgr.active_count(), 1);

        mgr.tick(0); // goes to Visible
        mgr.tick(200); // past timeout, fades out instantly
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn manager_dismiss_all() {
        let mut mgr = make_manager();
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
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
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(!cmds.is_empty());
    }

    #[test]
    fn manager_render_empty_when_no_overlays() {
        let mgr = make_manager();
        let cmds = mgr.render(&mocha());
        assert!(cmds.is_empty());
    }

    #[test]
    fn manager_render_brightness() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Brightness { level: 100 }, 0);
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
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
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_media_play_pause() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::MediaPlayPause { playing: true }, 0);
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_keyboard_lock() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::KeyboardLock {
                lock_type: LockType::CapsLock,
                active: true,
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_device_event() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::DeviceEvent {
                device_name: "USB Drive".into(),
                ejected: false,
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_screenshot() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::ScreenshotTaken {
                path: "/home/user/screenshot.png".into(),
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_microphone() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::Microphone { muted: true }, 0);
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_network_status() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::NetworkStatus {
                connected: true,
                name: "WiFi".into(),
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_battery_low() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(OsdKind::BatteryLow { percent: 5 }, 0);
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    #[test]
    fn manager_render_custom() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::Custom {
                icon: OsdIcon::Warning,
                message: "Disk full".into(),
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    // ---- same_category ----

    #[test]
    fn same_category_volume() {
        assert!(same_category(
            &OsdKind::Volume {
                level: 30,
                muted: false
            },
            &OsdKind::Volume {
                level: 60,
                muted: true
            },
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
            &OsdKind::Volume {
                level: 50,
                muted: false
            },
            &OsdKind::Brightness { level: 50 },
        ));
    }

    #[test]
    fn keyboard_locks_not_same_category() {
        // Lock events don't merge — each lock toggle is independent.
        assert!(!same_category(
            &OsdKind::KeyboardLock {
                lock_type: LockType::CapsLock,
                active: true
            },
            &OsdKind::KeyboardLock {
                lock_type: LockType::NumLock,
                active: true
            },
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
            let (s, _c) = icon_info(&mocha(), v);
            assert!(!s.is_empty());
        }
    }

    // ---- text is bounded by width, not by a character budget ----

    /// Every string the OSD draws reaches the renderer whole, with a width to
    /// fit inside and a mark to use if it does not. The OSD used to cut them to
    /// 35 or 40 characters first, a budget unrelated to the width beside it.
    #[test]
    fn osd_text_is_bounded_by_width_not_pre_truncated() {
        let long = "Symphony No. 9 in D minor, Op. 125 — Ode an die Freude (remastered)";
        let mut osd = OsdManager::new(1920.0, 1080.0);
        osd.show(
            OsdKind::MediaTrack {
                title: long.to_string(),
                artist: long.to_string(),
                album: long.to_string(),
            },
            0,
        );
        let cmds = osd.render(&mocha());
        let bounded: Vec<_> = cmds
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
        assert_eq!(bounded.len(), 3, "title, artist and album, each uncut");
        for (max_width, overflow) in bounded {
            assert!(max_width.is_some());
            assert_eq!(overflow, TextOverflow::Ellipsis);
        }
    }

    // ---- height_for_kind ----

    #[test]
    fn height_volume() {
        let mgr = make_manager();
        assert!(
            (mgr.height_for_kind(&OsdKind::Volume {
                level: 50,
                muted: false
            }) - 72.0)
                .abs()
                < 0.01
        );
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
        let h = mgr.height_for_kind(&OsdKind::KeyboardLock {
            lock_type: LockType::NumLock,
            active: true,
        });
        assert!((h - 56.0).abs() < 0.01);
    }

    // ---- Settings UI ----

    #[test]
    fn settings_ui_render_not_empty() {
        let ui = OsdSettingsUI::new(OsdConfig::default());
        let cmds = ui.render(&mocha(), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn settings_ui_render_with_disabled() {
        let mut config = OsdConfig::default();
        config.enabled = false;
        let ui = OsdSettingsUI::new(config);
        let cmds = ui.render(&mocha(), 0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    // ---- Multiple overlays stacking ----

    #[test]
    fn stacked_overlays_render_multiple_panels() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::Volume {
                level: 50,
                muted: false,
            },
            0,
        );
        mgr.show(
            OsdKind::KeyboardLock {
                lock_type: LockType::CapsLock,
                active: true,
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        // Should have commands for both overlays (at least 6 each: shadow + bg + border + content).
        assert!(cmds.len() >= 10);
    }

    // ---- Muted volume rendering ----

    #[test]
    fn volume_muted_renders() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::Volume {
                level: 0,
                muted: true,
            },
            0,
        );
        mgr.tick(0);
        let cmds = mgr.render(&mocha());
        assert!(cmds.len() > 3);
    }

    // ---- Edge cases ----

    #[test]
    fn volume_level_clamped_at_100() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::Volume {
                level: 150,
                muted: false,
            },
            0,
        );
        mgr.tick(0);
        // Should not panic during render.
        let cmds = mgr.render(&mocha());
        assert!(!cmds.is_empty());
    }

    /// A screenshot path is shortened from its start, so what survives is the
    /// *end* of the path — the part that says which shot — and the result fits
    /// the label it is drawn into.
    ///
    /// The test asserts a suffix, not the whole file name: the label box has
    /// room for about twenty characters, and this file name alone is thirty.
    /// Demanding the whole name would be demanding that the box be wider,
    /// which is a different claim from the one this code makes.
    #[test]
    fn screenshot_path_keeps_the_file_name_and_fits() {
        let path =
            "/home/user/very/long/path/to/some/nested/directory/screenshot_2026_05_17_12345.png";
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::ScreenshotTaken {
                path: path.to_string(),
            },
            0,
        );
        mgr.tick(0);
        let label = mgr
            .render(&mocha())
            .into_iter()
            .find_map(|cmd| match cmd {
                RenderCommand::Text { text, .. } if text.starts_with("Screenshot: ") => Some(text),
                _ => None,
            })
            .expect("the OSD names the screenshot");
        let shown = label
            .strip_prefix("Screenshot: ")
            .expect("the prefix is what we matched on");
        let tail = shown
            .strip_prefix('\u{2026}')
            .expect("a path too long for the box is marked as cut: {label}");
        assert!(
            path.ends_with(tail),
            "what is shown is the end of the path, not the start: {label}"
        );
        assert!(
            tail.ends_with(".png"),
            "and it reaches at least the extension: {label}"
        );
        assert!(
            text::measure(&label, OSD_LABEL_SIZE, OSD_LABEL_WEIGHT)
                <= osd_label_width(mgr.config.width) + 0.5,
            "and it fits the label box: {label}"
        );
    }

    /// A path under a home directory named in a non-Latin script must not take
    /// the shell down. The removed truncation sliced a `str` at
    /// `len() - 27` — a byte offset — which panics inside a character.
    #[test]
    fn a_non_ascii_screenshot_path_renders() {
        let mut mgr = make_manager();
        mgr.config.fade_in_ms = 0;
        mgr.show(
            OsdKind::ScreenshotTaken {
                path: "/home/ユーザー/画像/スクリーンショット_2026_05_17.png".to_string(),
            },
            0,
        );
        mgr.tick(0);
        assert!(!mgr.render(&mocha()).is_empty());
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
        let cmds = mgr.render(&mocha());
        assert!(!cmds.is_empty());
    }

    #[test]
    fn rapid_volume_updates_keep_single_overlay() {
        let mut mgr = make_manager();
        for i in 0..20 {
            mgr.show(
                OsdKind::Volume {
                    level: i * 5,
                    muted: false,
                },
                i as u64 * 50,
            );
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

    // ========================================================================
    // The palette conversion
    // ========================================================================
    //
    // Part 2 of `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-
    // PALETTE`. Eleven Mocha constants were deleted from this module; the
    // tests below are what stands between that edit and a leftover.
    //
    // The sweep (`every_colour_the_osd_draws_comes_from_its_palette`) finds a
    // leftover *constant*. It cannot find a wrong *role*, because every role
    // is a member of both palettes — so everything this module promises about
    // *which* role goes where needs a test of its own, and the eight tests
    // after the sweep are those promises written down.

    /// One fixture per arm of `render_content`'s `match`, and both sides of
    /// every `if` inside those arms.
    ///
    /// The sweep is only as wide as the render it is given, so this list is
    /// enumerated from the renderer's branches rather than from its colours:
    /// a branch missing here is a branch nothing below checks.
    fn every_kind() -> Vec<(String, OsdKind)> {
        let mut v: Vec<(String, OsdKind)> = vec![
            (
                "volume".into(),
                OsdKind::Volume {
                    level: 60,
                    muted: false,
                },
            ),
            (
                "volume muted".into(),
                OsdKind::Volume {
                    level: 60,
                    muted: true,
                },
            ),
            // level 0 takes the `fill_w > 0.0` branch the other way, and the
            // first arm of `volume_icon`.
            (
                "volume silent".into(),
                OsdKind::Volume {
                    level: 0,
                    muted: false,
                },
            ),
            (
                "volume low".into(),
                OsdKind::Volume {
                    level: 20,
                    muted: false,
                },
            ),
            (
                "volume medium".into(),
                OsdKind::Volume {
                    level: 50,
                    muted: false,
                },
            ),
            // `level.min(100)` clamps, so a level past 100 is its own branch.
            (
                "volume over".into(),
                OsdKind::Volume {
                    level: 200,
                    muted: false,
                },
            ),
            ("brightness dark".into(), OsdKind::Brightness { level: 10 }),
            (
                "brightness medium".into(),
                OsdKind::Brightness { level: 50 },
            ),
            (
                "brightness bright".into(),
                OsdKind::Brightness { level: 90 },
            ),
            (
                "media track".into(),
                OsdKind::MediaTrack {
                    title: "Clair de Lune".into(),
                    artist: "Debussy".into(),
                    album: "Suite bergamasque".into(),
                },
            ),
            // The album line is guarded on a non-empty album.
            (
                "media track without album".into(),
                OsdKind::MediaTrack {
                    title: "Clair de Lune".into(),
                    artist: "Debussy".into(),
                    album: String::new(),
                },
            ),
            ("playing".into(), OsdKind::MediaPlayPause { playing: true }),
            ("paused".into(), OsdKind::MediaPlayPause { playing: false }),
            (
                "device connected".into(),
                OsdKind::DeviceEvent {
                    device_name: "SanDisk".into(),
                    ejected: false,
                },
            ),
            (
                "device ejected".into(),
                OsdKind::DeviceEvent {
                    device_name: "SanDisk".into(),
                    ejected: true,
                },
            ),
            (
                "screenshot".into(),
                OsdKind::ScreenshotTaken {
                    path: "/home/user/pictures/shot.png".into(),
                },
            ),
            ("mic active".into(), OsdKind::Microphone { muted: false }),
            ("mic muted".into(), OsdKind::Microphone { muted: true }),
            (
                "network up".into(),
                OsdKind::NetworkStatus {
                    connected: true,
                    name: "home".into(),
                },
            ),
            (
                "network down".into(),
                OsdKind::NetworkStatus {
                    connected: false,
                    name: "home".into(),
                },
            ),
            ("battery low".into(), OsdKind::BatteryLow { percent: 7 }),
        ];
        for (lock_type, name) in [
            (LockType::CapsLock, "caps"),
            (LockType::NumLock, "num"),
            (LockType::ScrollLock, "scroll"),
        ] {
            for active in [true, false] {
                v.push((
                    format!("{name} lock {}", if active { "on" } else { "off" }),
                    OsdKind::KeyboardLock { lock_type, active },
                ));
            }
        }
        for icon in every_icon() {
            v.push((
                format!("custom {icon:?}"),
                OsdKind::Custom {
                    icon,
                    message: "hello".into(),
                },
            ));
        }
        v
    }

    /// Every arm of `icon_info`'s `match`.
    fn every_icon() -> [OsdIcon; 10] {
        [
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
        ]
    }

    /// One fully-visible overlay of `kind`, rendered from `p`.
    ///
    /// `tick(150)` is exactly `fade_in_ms`, which takes the overlay to
    /// `Visible` at opacity 1.0 — so `text_alpha` is 255 and the assertions
    /// below can compare whole colours where they want to.
    fn overlay(kind: OsdKind, p: &Palette) -> Vec<RenderCommand> {
        let mut mgr = make_manager();
        mgr.show(kind, 0);
        mgr.tick(150);
        mgr.render(p)
    }

    /// The settings panel at its defaults, 400px wide at the origin.
    fn settings(p: &Palette) -> Vec<RenderCommand> {
        OsdSettingsUI::new(OsdConfig::default()).render(p, 0.0, 0.0, 400.0)
    }

    fn texts_of(cmds: &[RenderCommand]) -> Vec<(String, f32, Color)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    font_size,
                    color,
                    ..
                } => Some((text.clone(), *font_size, *color)),
                _ => None,
            })
            .collect()
    }

    /// The colour of the one text drawn at `size`.
    ///
    /// Panics unless there is exactly one, which is the point: an assertion
    /// that silently picked the first of several would stop meaning anything
    /// the moment the renderer grew a second text that size.
    fn text_at(cmds: &[RenderCommand], size: f32) -> Color {
        let hits: Vec<Color> = texts_of(cmds)
            .into_iter()
            .filter(|(_, s, _)| (*s - size).abs() < 0.01)
            .map(|(_, _, c)| c)
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "expected exactly one text at size {size}, found {}",
            hits.len()
        );
        hits[0]
    }

    /// The colour every text whose content is exactly `want` is drawn in.
    ///
    /// Panics if there are none, and panics if they disagree. Four ticks come
    /// out of one source site, so "the tick's colour" is a well-formed
    /// question only as long as all four answer the same — and if they ever
    /// stop, the site has grown a branch that needs naming here rather than
    /// being silently represented by whichever one this picked first.
    fn text_saying(cmds: &[RenderCommand], want: &str) -> Color {
        let hits: Vec<Color> = texts_of(cmds)
            .into_iter()
            .filter(|(t, _, _)| t == want)
            .map(|(_, _, c)| c)
            .collect();
        assert!(!hits.is_empty(), "no text {want:?} is drawn");
        assert!(
            hits.iter().all(|c| rgb(*c) == rgb(hits[0])),
            "the {} texts saying {want:?} are not all the same colour",
            hits.len()
        );
        hits[0]
    }

    fn says(cmds: &[RenderCommand], want: &str) -> usize {
        texts_of(cmds).iter().filter(|(t, _, _)| t == want).count()
    }

    /// Colours of every `FillRect` of exactly `w` x `h`, in draw order.
    fn fills(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if (*width - w).abs() < 0.01 && (*height - h).abs() < 0.01 => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Colours of every `FillRect` of height `h`, whatever its width.
    ///
    /// The one way to pick out a track and the fill sitting on top of it,
    /// whose whole difference is that the fill's width moves.
    fn fills_h(cmds: &[RenderCommand], h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { height, color, .. } if (*height - h).abs() < 0.01 => {
                    Some(*color)
                }
                _ => None,
            })
            .collect()
    }

    fn every_color(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::BoxShadow { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// A settings panel whose config the caller has adjusted.
    fn settings_with(p: &Palette, f: impl FnOnce(&mut OsdConfig)) -> Vec<RenderCommand> {
        let mut cfg = OsdConfig::default();
        f(&mut cfg);
        OsdSettingsUI::new(cfg).render(p, 0.0, 0.0, 400.0)
    }

    /// The sweep: in the *light* palette a surviving Mocha constant is a dark
    /// value Latte does not contain, so it names itself.
    ///
    /// `derived` names the only two colours this module computes: the ink on
    /// the lit lock-key pill, and the lettering on an accent-filled control.
    /// Both are `readable_on` answers, and a `readable_on` answer is declared
    /// rather than exempt — each of the two values that function can return is
    /// also a role of one of the two palettes, so a blanket allowance would
    /// un-check Latte `base` and Mocha `crust` everywhere.
    #[test]
    fn every_colour_the_osd_draws_comes_from_its_palette() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let ink = [appearance::readable_on(p.green), p.on_accent()];
                for (what, kind) in every_kind() {
                    assert_drawn_from(&p, &overlay(kind, &p), &ink, &format!("osd ({what})"));
                }
                assert_drawn_from(&p, &settings(&p), &ink, "osd settings");
                assert_drawn_from(
                    &p,
                    &settings_with(&p, |c| {
                        c.enabled = false;
                        c.show_volume = false;
                        c.show_brightness = false;
                        c.show_media = false;
                        c.show_lock_keys = false;
                        c.position = OsdPosition::Center;
                    }),
                    &ink,
                    "osd settings (all off)",
                );
            }
        }
    }

    /// The fixtures actually reach every branch the two renderers have.
    ///
    /// The sweep is only as wide as the render it is given. Each assertion
    /// here names the `if` or `match` arm it stands for; a branch that stops
    /// being drawn stops being checked by everything above, silently, unless
    /// this fails first.
    #[test]
    fn the_fixtures_take_every_branch_the_osd_has() {
        let p = mocha();

        // render_content: Volume, both sides of `muted`.
        assert_eq!(
            says(
                &overlay(
                    OsdKind::Volume {
                        level: 60,
                        muted: false
                    },
                    &p
                ),
                "Volume  60%"
            ),
            1,
            "the unmuted volume label is not drawn"
        );
        assert_eq!(
            says(
                &overlay(
                    OsdKind::Volume {
                        level: 60,
                        muted: true
                    },
                    &p
                ),
                "Muted  60%"
            ),
            1,
            "the muted volume label is not drawn"
        );

        // render_slider_osd: `if fill_w > 0.0`, both ways. A track with no
        // fill draws one 6px-high rect; a filled one draws two.
        assert_eq!(
            fills_h(
                &overlay(
                    OsdKind::Volume {
                        level: 0,
                        muted: false
                    },
                    &p
                ),
                6.0
            )
            .len(),
            1,
            "a zero-level slider still draws a fill"
        );
        assert_eq!(
            fills_h(
                &overlay(
                    OsdKind::Volume {
                        level: 60,
                        muted: false
                    },
                    &p
                ),
                6.0
            )
            .len(),
            2,
            "a filled slider does not draw its fill"
        );

        // volume_icon: all four arms, and brightness_icon: all three.
        for (level, icon) in [
            (0_u8, "\u{1F507}"),
            (20, "\u{1F508}"),
            (50, "\u{1F509}"),
            (80, "\u{1F50A}"),
        ] {
            assert_eq!(
                says(
                    &overlay(
                        OsdKind::Volume {
                            level,
                            muted: false
                        },
                        &p
                    ),
                    icon
                ),
                1,
                "volume level {level} does not draw its own icon"
            );
        }
        for (level, icon) in [(10_u8, "\u{1F315}"), (50, "\u{2600}"), (90, "\u{2B50}")] {
            assert_eq!(
                says(&overlay(OsdKind::Brightness { level }, &p), icon),
                1,
                "brightness level {level} does not draw its own icon"
            );
        }

        // render_media_osd: `if !album.is_empty()`, both ways. The album is
        // the only 11px text in the overlay.
        let with_album = overlay(
            OsdKind::MediaTrack {
                title: "T".into(),
                artist: "A".into(),
                album: "Album".into(),
            },
            &p,
        );
        assert_eq!(says(&with_album, "Album"), 1, "the album line is not drawn");
        let no_album = overlay(
            OsdKind::MediaTrack {
                title: "T".into(),
                artist: "A".into(),
                album: String::new(),
            },
            &p,
        );
        assert_eq!(
            texts_of(&no_album)
                .iter()
                .filter(|(_, s, _)| (*s - 11.0).abs() < 0.01)
                .count(),
            0,
            "an empty album still draws a line"
        );

        // render_content: MediaPlayPause, KeyboardLock (three types x two
        // states), DeviceEvent, Microphone, NetworkStatus, BatteryLow.
        for (kind, want) in [
            (OsdKind::MediaPlayPause { playing: true }, "Playing"),
            (OsdKind::MediaPlayPause { playing: false }, "Paused"),
            (
                OsdKind::KeyboardLock {
                    lock_type: LockType::CapsLock,
                    active: true,
                },
                "Caps Lock: ON",
            ),
            (
                OsdKind::KeyboardLock {
                    lock_type: LockType::CapsLock,
                    active: false,
                },
                "Caps Lock: OFF",
            ),
            (
                OsdKind::KeyboardLock {
                    lock_type: LockType::NumLock,
                    active: true,
                },
                "Num Lock: ON",
            ),
            (
                OsdKind::KeyboardLock {
                    lock_type: LockType::ScrollLock,
                    active: true,
                },
                "Scroll Lock: ON",
            ),
            (
                OsdKind::DeviceEvent {
                    device_name: "SanDisk".into(),
                    ejected: false,
                },
                "SanDisk connected",
            ),
            (
                OsdKind::DeviceEvent {
                    device_name: "SanDisk".into(),
                    ejected: true,
                },
                "SanDisk ejected",
            ),
            (OsdKind::Microphone { muted: false }, "Mic: Active"),
            (OsdKind::Microphone { muted: true }, "Mic: Muted"),
            (
                OsdKind::NetworkStatus {
                    connected: true,
                    name: "home".into(),
                },
                "Connected: home",
            ),
            (
                OsdKind::NetworkStatus {
                    connected: false,
                    name: "home".into(),
                },
                "Disconnected: home",
            ),
            (OsdKind::BatteryLow { percent: 7 }, "Battery Low: 7%"),
        ] {
            assert_eq!(
                says(&overlay(kind, &p), want),
                1,
                "the {want:?} branch is not drawn"
            );
        }

        // render_content: ScreenshotTaken. The path is elided from its start,
        // so assert on the prefix rather than the whole label.
        let shot = overlay(
            OsdKind::ScreenshotTaken {
                path: "/home/user/pictures/shot.png".into(),
            },
            &p,
        );
        assert_eq!(
            texts_of(&shot)
                .iter()
                .filter(|(t, _, _)| t.starts_with("Screenshot: "))
                .count(),
            1,
            "the screenshot branch is not drawn"
        );

        // icon_info: all ten arms, each identified by its own icon string.
        for icon in every_icon() {
            let (want, _) = icon_info(&p, icon);
            assert_eq!(
                says(
                    &overlay(
                        OsdKind::Custom {
                            icon,
                            message: "hello".into()
                        },
                        &p
                    ),
                    want
                ),
                1,
                "the {icon:?} arm of icon_info is not drawn"
            );
        }

        // OsdSettingsUI::render: `if self.config.enabled`, both ways — the
        // pill knob moves, which is the only difference in geometry.
        assert_eq!(fills(&settings(&p), 16.0, 16.0).len(), 1);
        assert_eq!(
            fills(&settings_with(&p, |c| c.enabled = false), 16.0, 16.0).len(),
            1
        );

        // OsdSettingsUI::render: `let selected = …`, every position.
        for pos in [
            OsdPosition::TopCenter,
            OsdPosition::BottomCenter,
            OsdPosition::Center,
            OsdPosition::TopRight,
            OsdPosition::BottomRight,
        ] {
            let cmds = settings_with(&p, |c| c.position = pos);
            let dots = fills(&cmds, 12.0, 12.0);
            assert_eq!(dots.len(), 5, "the position selector is not five dots");
            assert_eq!(
                dots.iter().filter(|c| rgb(**c) == rgb(p.accent)).count(),
                1,
                "{pos:?} is not the only selected position"
            );
        }

        // OsdSettingsUI::render: `if *enabled` in the toggle loop, both ways.
        // The tick is drawn only when the toggle is on.
        assert_eq!(
            says(&settings(&p), "\u{2713}"),
            4,
            "four enabled toggles do not draw four ticks"
        );
        assert_eq!(
            says(
                &settings_with(&p, |c| {
                    c.show_volume = false;
                    c.show_brightness = false;
                    c.show_media = false;
                    c.show_lock_keys = false;
                }),
                "\u{2713}"
            ),
            0,
            "a disabled toggle still draws a tick"
        );
    }

    /// Every text site, one entry per site in the source — not per rendered
    /// instance, and not per *kind* of site.
    ///
    /// Do not shorten this to "a representative sample": a table that lists
    /// one entry per kind leaves the sites it did not list checked by
    /// nothing, which is how two defects escaped module 22's version of it.
    /// Eight overlay sites and ten panel sites; if the renderer grows a
    /// nineteenth, this grows with it.
    #[test]
    fn every_text_the_osd_draws_is_in_the_role_it_claims() {
        for light in [false, true] {
            let mut p = Palette::for_mode(light);
            p.accent = SAFE_ACCENTS[0];

            // ---- render_slider_osd ----
            let vol = overlay(
                OsdKind::Volume {
                    level: 60,
                    muted: false,
                },
                &p,
            );
            // S4: the slider's icon takes the kind's own colour.
            assert_eq!(rgb(text_at(&vol, 24.0)), rgb(p.blue), "slider icon");
            // S5: the label and percentage are plain text.
            assert_eq!(rgb(text_at(&vol, 14.0)), rgb(p.text), "slider label");

            // ---- render_media_osd ----
            let media = overlay(
                OsdKind::MediaTrack {
                    title: "T".into(),
                    artist: "A".into(),
                    album: "B".into(),
                },
                &p,
            );
            // S9: the music note.
            assert_eq!(rgb(text_at(&media, 28.0)), rgb(p.lavender), "media note");
            // S10: the title.
            assert_eq!(rgb(text_at(&media, 14.0)), rgb(p.text), "media title");
            // S11: the artist, one step down.
            assert_eq!(rgb(text_at(&media, 12.0)), rgb(p.subtext0), "media artist");
            // S12: the album, same role, drawn at half alpha.
            assert_eq!(rgb(text_at(&media, 11.0)), rgb(p.subtext0), "media album");

            // ---- render_icon_text_osd ----
            let batt = overlay(OsdKind::BatteryLow { percent: 7 }, &p);
            // S14: the icon takes the kind's own colour.
            assert_eq!(rgb(text_at(&batt, 20.0)), rgb(p.red), "icon-text icon");
            // S15: the label is plain text.
            assert_eq!(rgb(text_at(&batt, 14.0)), rgb(p.text), "icon-text label");

            // ---- OsdSettingsUI::render ----
            let s = settings(&p);
            // T1: the panel title.
            assert_eq!(
                rgb(text_saying(&s, "On-Screen Display Settings")),
                rgb(p.text),
                "panel title"
            );
            // T4: the enable toggle's label.
            assert_eq!(
                rgb(text_saying(&s, "Enable OSD overlays")),
                rgb(p.text),
                "enable label"
            );
            // T5, T8, T11: the three section headings are all secondary.
            for heading in ["Position", "Timeout: 2000ms", "Show OSD for:"] {
                assert_eq!(
                    rgb(text_saying(&s, heading)),
                    rgb(p.subtext0),
                    "heading {heading:?}"
                );
            }
            // T7: a position label, selected and not. This is one source site
            // with an `if` in it, so both sides are named.
            assert_eq!(
                rgb(text_saying(&s, "Bottom Center")),
                rgb(p.text),
                "the selected position's label"
            );
            assert_eq!(
                rgb(text_saying(&s, "Top Center")),
                rgb(p.subtext0),
                "an unselected position's label"
            );
            // T13: the tick sits *on* the green box, so it is derived from it.
            assert_eq!(
                rgb(text_saying(&s, "\u{2713}")),
                rgb(appearance::readable_on(p.green)),
                "the checkbox tick"
            );
            // T14: the toggle labels.
            assert_eq!(
                rgb(text_saying(&s, "Volume changes")),
                rgb(p.text),
                "a toggle label"
            );
            // T16: the Preview button's label sits on the accent.
            assert_eq!(
                rgb(text_saying(&s, "Preview OSD")),
                rgb(p.on_accent()),
                "the preview button's label"
            );
        }
    }

    /// Every rectangle site, one entry per site in the source.
    ///
    /// Same rule as the text table above: per source site, not per rendered
    /// instance and not per kind. Seven overlay sites and seven panel sites.
    #[test]
    fn every_rectangle_the_osd_draws_is_in_the_role_it_claims() {
        for light in [false, true] {
            let mut p = Palette::for_mode(light);
            p.accent = SAFE_ACCENTS[0];

            let vol = overlay(
                OsdKind::Volume {
                    level: 60,
                    muted: false,
                },
                &p,
            );
            // S1: the drop shadow is black at the overlay's own fade, not a
            // role — a shadow is an absence of light, and its depth follows
            // the thing casting it.
            let shadows: Vec<Color> = vol
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::BoxShadow { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(shadows.len(), 1, "the overlay draws one shadow");
            assert_eq!(rgb(shadows[0]), (0, 0, 0), "the shadow is black");
            assert_eq!(shadows[0].a, 110, "the shadow is half the panel's alpha");
            // S2: the panel.
            assert_eq!(
                fills(&vol, 320.0, 72.0),
                vec![Color::rgba(p.base.r, p.base.g, p.base.b, 220)]
            );
            // S3: the border.
            let borders: Vec<Color> = vol
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(borders.len(), 1, "the overlay draws one border");
            assert_eq!(rgb(borders[0]), rgb(p.surface1), "the border");
            // S6, S7: the slider's track and the fill on top of it. The fill
            // takes the kind's own colour, which the track must not.
            let bar = fills_h(&vol, 6.0);
            assert_eq!(bar.len(), 2, "the slider is a track and a fill");
            assert_eq!(rgb(bar[0]), rgb(p.surface0), "the slider track");
            assert_eq!(rgb(bar[1]), rgb(p.blue), "the slider fill");
            // S8: the knob.
            assert_eq!(
                fills(&vol, 10.0, 10.0)
                    .iter()
                    .map(|c| rgb(*c))
                    .collect::<Vec<_>>(),
                vec![rgb(p.text)],
                "the slider knob"
            );

            // S13: the media overlay's decorative bar.
            let media = overlay(
                OsdKind::MediaTrack {
                    title: "T".into(),
                    artist: "A".into(),
                    album: "B".into(),
                },
                &p,
            );
            assert_eq!(
                fills_h(&media, 2.0)
                    .iter()
                    .map(|c| rgb(*c))
                    .collect::<Vec<_>>(),
                vec![rgb(p.lavender)],
                "the media bar"
            );

            // ---- OsdSettingsUI::render ----
            let s = settings(&p);
            // T2: the enable pill, both sides of its `if`.
            assert_eq!(
                fills(&s, 40.0, 20.0)
                    .iter()
                    .map(|c| rgb(*c))
                    .collect::<Vec<_>>(),
                vec![rgb(p.green)],
                "the enable pill, on"
            );
            assert_eq!(
                fills(&settings_with(&p, |c| c.enabled = false), 40.0, 20.0)
                    .iter()
                    .map(|c| rgb(*c))
                    .collect::<Vec<_>>(),
                vec![rgb(p.subtext0)],
                "the enable pill, off"
            );
            // T3: the pill's knob — `readable_on` the pill it sits on, not a
            // role. `s` is the enabled fixture, so the pill is `green`.
            assert_eq!(
                fills(&s, 16.0, 16.0)
                    .iter()
                    .map(|c| rgb(*c))
                    .collect::<Vec<_>>(),
                vec![rgb(appearance::readable_on(p.green))],
                "the pill knob"
            );
            // T6: the position dots — one selected, four not.
            let dots = fills(&s, 12.0, 12.0);
            assert_eq!(dots.len(), 5);
            assert_eq!(
                dots.iter().filter(|c| rgb(**c) == rgb(p.accent)).count(),
                1,
                "the selected dot"
            );
            assert_eq!(
                dots.iter().filter(|c| rgb(**c) == rgb(p.surface1)).count(),
                4,
                "the unselected dots"
            );
            // T9, T10: the timeout slider's track and its accent fill.
            let track = fills_h(&s, 4.0);
            assert_eq!(track.len(), 2, "the timeout slider is a track and a fill");
            assert_eq!(rgb(track[0]), rgb(p.surface0), "the timeout track");
            assert_eq!(rgb(track[1]), rgb(p.accent), "the timeout fill");
            // T12: the checkboxes, both sides of their `if`.
            assert_eq!(
                fills(&s, 14.0, 14.0)
                    .iter()
                    .filter(|c| rgb(**c) == rgb(p.green))
                    .count(),
                4,
                "the enabled checkboxes"
            );
            assert_eq!(
                fills(
                    &settings_with(&p, |c| {
                        c.show_volume = false;
                        c.show_brightness = false;
                        c.show_media = false;
                        c.show_lock_keys = false;
                    }),
                    14.0,
                    14.0
                )
                .iter()
                .filter(|c| rgb(**c) == rgb(p.surface1))
                .count(),
                4,
                "the disabled checkboxes"
            );
            // T15: the Preview button.
            assert_eq!(
                fills(&s, 120.0, 32.0)
                    .iter()
                    .map(|c| rgb(*c))
                    .collect::<Vec<_>>(),
                vec![rgb(p.accent)],
                "the preview button"
            );
        }
    }

    /// Every kind draws its icon in the colour that kind claims — one entry
    /// per colour *decision* in `render_content` and `icon_info`, not one per
    /// kind and not one per representative.
    ///
    /// `render_content` chooses fifteen colours and `icon_info` chooses ten.
    /// Those twenty-five decisions are the module's whole vocabulary, and
    /// nothing else here checks them: the sweep cannot, because every one of
    /// them is a role of both palettes, and the role tables above see only the
    /// two fixtures they happen to render. Written out as roles rather than as
    /// `icon_info(p, icon).1`, which would only prove the function agrees with
    /// itself.
    #[test]
    fn every_kind_draws_its_icon_in_the_colour_that_kind_claims() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mode = if light { "light" } else { "dark" };

            // The two slider kinds draw their icon at 24px.
            for (what, kind, want) in [
                (
                    "volume",
                    OsdKind::Volume {
                        level: 60,
                        muted: false,
                    },
                    p.blue,
                ),
                (
                    "a muted volume",
                    OsdKind::Volume {
                        level: 60,
                        muted: true,
                    },
                    p.red,
                ),
                ("brightness", OsdKind::Brightness { level: 60 }, p.yellow),
            ] {
                assert_eq!(
                    rgb(text_at(&overlay(kind, &p), 24.0)),
                    rgb(want),
                    "{mode}: {what} does not draw its icon in its own colour"
                );
            }

            // Every other kind draws its icon at 20px.
            let mut cases: Vec<(String, OsdKind, Color)> = vec![
                (
                    "play/pause".into(),
                    OsdKind::MediaPlayPause { playing: true },
                    p.lavender,
                ),
                (
                    "a lock that is on".into(),
                    OsdKind::KeyboardLock {
                        lock_type: LockType::CapsLock,
                        active: true,
                    },
                    p.green,
                ),
                (
                    "a lock that is off".into(),
                    OsdKind::KeyboardLock {
                        lock_type: LockType::CapsLock,
                        active: false,
                    },
                    p.subtext0,
                ),
                (
                    "a device that connected".into(),
                    OsdKind::DeviceEvent {
                        device_name: "d".into(),
                        ejected: false,
                    },
                    p.green,
                ),
                (
                    "a device that ejected".into(),
                    OsdKind::DeviceEvent {
                        device_name: "d".into(),
                        ejected: true,
                    },
                    p.subtext0,
                ),
                (
                    "a screenshot".into(),
                    OsdKind::ScreenshotTaken {
                        path: "/p/s.png".into(),
                    },
                    p.green,
                ),
                (
                    "a live mic".into(),
                    OsdKind::Microphone { muted: false },
                    p.green,
                ),
                (
                    "a muted mic".into(),
                    OsdKind::Microphone { muted: true },
                    p.red,
                ),
                (
                    "a network that is up".into(),
                    OsdKind::NetworkStatus {
                        connected: true,
                        name: "n".into(),
                    },
                    p.green,
                ),
                (
                    "a network that is down".into(),
                    OsdKind::NetworkStatus {
                        connected: false,
                        name: "n".into(),
                    },
                    p.red,
                ),
                (
                    "a low battery".into(),
                    OsdKind::BatteryLow { percent: 7 },
                    p.red,
                ),
            ];
            for (icon, want) in [
                (OsdIcon::Info, p.blue),
                (OsdIcon::Success, p.green),
                (OsdIcon::Warning, p.yellow),
                (OsdIcon::Error, p.red),
                (OsdIcon::Speaker, p.blue),
                (OsdIcon::Brightness, p.yellow),
                (OsdIcon::Network, p.green),
                (OsdIcon::Battery, p.peach),
                (OsdIcon::Lock, p.lavender),
                (OsdIcon::Camera, p.green),
            ] {
                cases.push((
                    format!("the {icon:?} icon"),
                    OsdKind::Custom {
                        icon,
                        message: "hello".into(),
                    },
                    want,
                ));
            }
            for (what, kind, want) in cases {
                assert_eq!(
                    rgb(text_at(&overlay(kind, &p), 20.0)),
                    rgb(want),
                    "{mode}: {what} does not draw its icon in its own colour"
                );
            }
        }
    }

    /// Nothing the overlay draws follows the accent.
    ///
    /// An OSD is pure feedback: there is nothing in it to select, nothing to
    /// drag, nothing that says "you are here". The accent marks what you can
    /// move, not what you are being told — so `p.accent` must not appear
    /// anywhere in `OsdManager`, and this says so rather than leaving it to be
    /// an accident of the mapping that the next edit quietly undoes.
    #[test]
    fn no_colour_the_overlay_draws_ever_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                for (what, kind) in every_kind() {
                    for c in every_color(&overlay(kind, &p)) {
                        assert_ne!(
                            rgb(c),
                            rgb(accent),
                            "the {what} overlay draws the accent; an OSD is \
                             feedback, and nothing in it is yours to move"
                        );
                    }
                }
            }
        }
    }

    /// Volume and brightness are a category pair, and a pair told apart by
    /// hue stops being a pair the moment one colour claims both.
    ///
    /// This is the deliberate counterpart to `sound_settings.rs`, where the
    /// volume bar *does* take the accent — because there you can drag it.
    #[test]
    fn volume_and_brightness_stay_a_pair_you_can_tell_apart() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let vol = overlay(
                OsdKind::Volume {
                    level: 60,
                    muted: false,
                },
                &p,
            );
            let bright = overlay(OsdKind::Brightness { level: 60 }, &p);
            let vol_fill = fills_h(&vol, 6.0)[1];
            let bright_fill = fills_h(&bright, 6.0)[1];
            assert_eq!(rgb(vol_fill), rgb(p.blue), "volume is blue");
            assert_eq!(rgb(bright_fill), rgb(p.yellow), "brightness is yellow");
            assert_ne!(
                rgb(vol_fill),
                rgb(bright_fill),
                "volume and brightness are the same colour, so the overlay no \
                 longer says which one changed"
            );
            // Muting is a reading, not a level, and it overrides the hue.
            let muted = overlay(
                OsdKind::Volume {
                    level: 60,
                    muted: true,
                },
                &p,
            );
            assert_eq!(rgb(fills_h(&muted, 6.0)[1]), rgb(p.red), "muted is red");
            assert_ne!(
                rgb(fills_h(&muted, 6.0)[1]),
                rgb(vol_fill),
                "a muted volume bar looks exactly like an unmuted one"
            );
        }
    }

    /// The settings panel is a different surface with a different answer: it
    /// has exactly three accent sites, and they are a selection, a drag and an
    /// invitation.
    ///
    /// Counted rather than spot-checked, so that a fourth site added later has
    /// to be argued for here instead of appearing by itself.
    #[test]
    fn the_settings_panel_has_exactly_three_accent_sites() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let hits = every_color(&settings(&p))
                    .into_iter()
                    .filter(|c| rgb(*c) == rgb(accent))
                    .count();
                assert_eq!(
                    hits, 3,
                    "the OSD settings panel has {hits} accent sites, not the \
                     three it claims: the selected position dot, the timeout \
                     slider's fill and the Preview button"
                );
            }
        }
    }

    /// Ink drawn on a coloured fill is derived from that fill, never named.
    ///
    /// Both sites were Mocha `base` — a dark ink that vanishes on a Latte
    /// green and on a pale accent. The proof that they are *computed* is that
    /// they move: a dark accent must flip the Preview label to the light
    /// endpoint, which no constant would do. All four `SAFE_ACCENTS` are
    /// pastel and would all answer the same way, so this adds one deliberately
    /// dark accent and one deliberately pale one.
    #[test]
    fn ink_drawn_on_a_coloured_fill_is_readable_in_both_modes() {
        const DARK: Color = Color::from_hex(0x0020_3050);
        const PALE: Color = Color::from_hex(0x00F5_D0E0);

        for light in [false, true] {
            let mut p = Palette::for_mode(light);

            // The tick sits on `p.green`, whose value differs by mode.
            let tick = text_saying(&settings(&p), "\u{2713}");
            assert_eq!(rgb(tick), rgb(appearance::readable_on(p.green)));
            assert_ne!(rgb(tick), rgb(p.green), "the tick is invisible on its box");

            // The Preview label sits on the accent, which the user chooses.
            p.accent = DARK;
            let on_dark = text_saying(&settings(&p), "Preview OSD");
            p.accent = PALE;
            let on_pale = text_saying(&settings(&p), "Preview OSD");
            assert_ne!(
                rgb(on_dark),
                rgb(on_pale),
                "the Preview label is the same colour on a dark accent and a \
                 pale one, so it is a constant rather than a derivation"
            );
            assert!(
                on_dark.r > 0x80,
                "a dark accent wants light ink, got #{:02X}{:02X}{:02X}",
                on_dark.r,
                on_dark.g,
                on_dark.b
            );
            assert!(
                on_pale.r < 0x80,
                "a pale accent wants dark ink, got #{:02X}{:02X}{:02X}",
                on_pale.r,
                on_pale.g,
                on_pale.b
            );
        }
    }

    /// Nothing that reports a state follows the accent.
    ///
    /// A lock that is on, a device that ejected, a mic that is muted, a
    /// network that dropped, a setting that is off — these are readings. The
    /// accent is the user's choice of decoration, and a reading painted in it
    /// stops being a reading. Covers the settings panel's two state widgets
    /// as well as the overlay's, which is why it is not subsumed by
    /// `no_colour_the_overlay_draws_ever_follows_the_accent`.
    #[test]
    fn nothing_that_reports_a_state_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                // The enable pill, both states.
                for (on, what) in [(true, "on"), (false, "off")] {
                    let pill = fills(&settings_with(&p, |c| c.enabled = on), 40.0, 20.0);
                    assert_eq!(pill.len(), 1);
                    assert_ne!(
                        rgb(pill[0]),
                        rgb(accent),
                        "the enable pill reports {what} in the accent"
                    );
                }
                // The checkboxes, both states.
                for (on, what) in [(true, "checked"), (false, "unchecked")] {
                    let boxes = fills(
                        &settings_with(&p, |c| {
                            c.show_volume = on;
                            c.show_brightness = on;
                            c.show_media = on;
                            c.show_lock_keys = on;
                        }),
                        14.0,
                        14.0,
                    );
                    assert_eq!(boxes.len(), 4);
                    for b in boxes {
                        assert_ne!(
                            rgb(b),
                            rgb(accent),
                            "a {what} checkbox reports its state in the accent"
                        );
                    }
                }
            }
        }
    }

    /// Every pair this module uses to tell two things apart stays apart in
    /// both modes.
    ///
    /// A role mapping can be wrong without leaving a constant behind: two
    /// states mapped to the same role compile, render, and say nothing. The
    /// sweep cannot see it, because both roles are members of both palettes.
    #[test]
    fn every_pair_this_module_uses_to_tell_things_apart_stays_apart() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mode = if light { "light" } else { "dark" };
            for (a, b, what) in [
                (p.green, p.subtext0, "a lock that is on vs one that is off"),
                (p.green, p.red, "a mic that is live vs one that is muted"),
                (p.green, p.surface1, "a checked box vs an unchecked one"),
                (p.blue, p.yellow, "volume vs brightness"),
                (p.text, p.subtext0, "a selected label vs an unselected one"),
                (p.base, p.surface1, "the overlay's fill vs its border"),
                (p.surface0, p.text, "a slider's track vs its knob"),
            ] {
                assert_ne!(rgb(a), rgb(b), "{mode}: {what} are the same colour");
            }
            // icon_info's five distinguishable severities. Info/Speaker share
            // blue, Warning/Brightness share yellow and Success/Network/Camera
            // share green on purpose — those are the same thing said twice —
            // but these five must never collapse into one another.
            let severities = [
                ("Info", icon_info(&p, OsdIcon::Info).1),
                ("Warning", icon_info(&p, OsdIcon::Warning).1),
                ("Error", icon_info(&p, OsdIcon::Error).1),
                ("Success", icon_info(&p, OsdIcon::Success).1),
                ("Battery", icon_info(&p, OsdIcon::Battery).1),
                ("Lock", icon_info(&p, OsdIcon::Lock).1),
            ];
            for i in 0..severities.len() {
                for j in (i + 1)..severities.len() {
                    assert_ne!(
                        rgb(severities[i].1),
                        rgb(severities[j].1),
                        "{mode}: the {} and {} icons are the same colour",
                        severities[i].0,
                        severities[j].0
                    );
                }
            }
        }
    }
}
