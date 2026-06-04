//! Notification Pane — Action Center style slide-out panel.
//!
//! Slides in from the right edge of the screen, showing notification history
//! grouped by time (Today, Yesterday, This Week, Older), quick settings toggles,
//! and per-app notification configuration.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut pane = NotificationPane::new();
//!
//! // Toggle on system tray click or Win+N:
//! pane.toggle();
//!
//! // Push incoming notifications:
//! pane.push_notification(notif);
//!
//! // Each frame:
//! pane.tick(dt);
//! let commands = pane.render(screen_width, screen_height);
//!
//! // Drain events to act on:
//! for event in pane.drain_events() {
//!     match event {
//!         NotifPaneEvent::NotificationClicked(id) => { /* open app */ }
//!         NotifPaneEvent::ClearAll => { /* acknowledged */ }
//!         NotifPaneEvent::Closed => { /* pane dismissed */ }
//!         _ => {}
//!     }
//! }
//! ```

use guitk::color::Color;
use guitk::event::{EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const OVERLAY1: Color = Color::from_hex(0x7F849C);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const SHADOW: Color = Color::rgba(0, 0, 0, 120);
    pub const PANE_BG: Color = Color::from_hex(0x1E1E2E);
    pub const CARD_BG: Color = Color::from_hex(0x313244);
    pub const HOVER_BG: Color = Color::from_hex(0x45475A);
}

// ============================================================================
// Constants
// ============================================================================

/// Width of the notification pane in pixels.
const PANE_WIDTH: f32 = 380.0;

/// Maximum number of stored notifications.
const MAX_NOTIFICATIONS: usize = 50;

/// Padding inside the pane.
const PANE_PADDING: f32 = 16.0;

/// Height of a single notification card.
const NOTIF_CARD_HEIGHT: f32 = 80.0;

/// Spacing between notification cards.
const NOTIF_CARD_SPACING: f32 = 8.0;

/// Height of the quick settings section.
const QUICK_SETTINGS_HEIGHT: f32 = 200.0;

/// Height of the header row (title + clear all).
const HEADER_HEIGHT: f32 = 44.0;

/// Height of a time-group header ("Today", "Yesterday", etc.).
const GROUP_HEADER_HEIGHT: f32 = 28.0;

/// Corner radius for cards.
const CARD_RADIUS: f32 = 8.0;

/// Dismiss button size.
const DISMISS_BTN_SIZE: f32 = 20.0;

/// Toggle pill dimensions.
const TOGGLE_WIDTH: f32 = 40.0;
const TOGGLE_HEIGHT: f32 = 22.0;

/// Slider dimensions.
const SLIDER_WIDTH: f32 = 140.0;
const SLIDER_HEIGHT: f32 = 6.0;

/// Quick-setting row height.
const QS_ROW_HEIGHT: f32 = 36.0;

// ============================================================================
// Time grouping helpers
// ============================================================================

/// Seconds in a day.
const SECS_PER_DAY: u64 = 86400;

/// Time group for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TimeGroup {
    Today,
    Yesterday,
    ThisWeek,
    Older,
}

impl TimeGroup {
    fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This Week",
            Self::Older => "Older",
        }
    }

    /// Classify a timestamp relative to `now`.
    fn classify(timestamp: u64, now: u64) -> Self {
        if now < timestamp {
            return Self::Today;
        }
        let age = now - timestamp;
        if age < SECS_PER_DAY {
            Self::Today
        } else if age < 2 * SECS_PER_DAY {
            Self::Yesterday
        } else if age < 7 * SECS_PER_DAY {
            Self::ThisWeek
        } else {
            Self::Older
        }
    }
}

// ============================================================================
// Core types
// ============================================================================

/// Notification priority level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NotifPriority {
    Low,
    Normal,
    High,
    Urgent,
}

impl NotifPriority {
    fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Urgent => "Urgent",
        }
    }

    fn accent_color(self) -> Color {
        match self {
            Self::Low => theme::OVERLAY0,
            Self::Normal => theme::BLUE,
            Self::High => theme::PEACH,
            Self::Urgent => theme::RED,
        }
    }
}

/// A single notification.
#[derive(Clone, Debug)]
pub struct Notification {
    pub id: u64,
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub timestamp: u64,
    pub priority: NotifPriority,
    pub read: bool,
    pub action: Option<String>,
}

/// Quick-setting toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuickSetting {
    DoNotDisturb,
    NightLight,
    WiFi,
    Bluetooth,
    FocusMode,
}

impl QuickSetting {
    fn label(self) -> &'static str {
        match self {
            Self::DoNotDisturb => "Do Not Disturb",
            Self::NightLight => "Night Light",
            Self::WiFi => "Wi-Fi",
            Self::Bluetooth => "Bluetooth",
            Self::FocusMode => "Focus Mode",
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::DoNotDisturb,
            Self::NightLight,
            Self::WiFi,
            Self::Bluetooth,
            Self::FocusMode,
        ]
    }
}

/// Per-app notification settings.
#[derive(Clone, Debug)]
pub struct AppNotifSettings {
    pub app_name: String,
    pub enabled: bool,
    pub priority: NotifPriority,
    pub sound: bool,
    pub banner: bool,
}

impl AppNotifSettings {
    fn new(app_name: String) -> Self {
        Self {
            app_name,
            enabled: true,
            priority: NotifPriority::Normal,
            sound: true,
            banner: true,
        }
    }
}

/// Per-app setting that can be changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppSettingKind {
    Enabled,
    Priority,
    Sound,
    Banner,
}

/// Setting value for per-app changes.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Priority(NotifPriority),
}

/// Events emitted by the notification pane.
#[derive(Clone, Debug)]
pub enum NotifPaneEvent {
    /// User clicked a notification (wants to open the related app/action).
    NotificationClicked(u64),
    /// User dismissed a single notification.
    NotificationDismissed(u64),
    /// User clicked "Clear all".
    ClearAll,
    /// Per-app setting changed.
    SettingChanged {
        app: String,
        setting: AppSettingKind,
        value: SettingValue,
    },
    /// Quick setting toggled.
    QuickSettingToggled(QuickSetting),
    /// Pane was closed.
    Closed,
}

// ============================================================================
// Animation state
// ============================================================================

/// Pane visibility state with animation progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaneState {
    /// Fully hidden (off-screen).
    Hidden,
    /// Sliding in from the right; progress goes 0.0 -> 1.0.
    SlideIn(f32),
    /// Fully visible.
    Visible,
    /// Sliding out to the right; progress goes 0.0 -> 1.0.
    SlideOut(f32),
}

impl PaneState {
    /// Returns the fraction of the pane that is currently visible (0.0 = hidden, 1.0 = full).
    fn visibility(self) -> f32 {
        match self {
            Self::Hidden => 0.0,
            Self::SlideIn(p) => p,
            Self::Visible => 1.0,
            Self::SlideOut(p) => 1.0 - p,
        }
    }

    fn is_visible(self) -> bool {
        !matches!(self, Self::Hidden)
    }
}

// ============================================================================
// Quick settings state
// ============================================================================

/// Quick settings values.
#[derive(Clone, Debug)]
struct QuickSettingsState {
    do_not_disturb: bool,
    night_light: bool,
    wifi: bool,
    bluetooth: bool,
    focus_mode: bool,
    /// Volume 0..=100
    volume: u8,
    /// Brightness 0..=100
    brightness: u8,
}

impl Default for QuickSettingsState {
    fn default() -> Self {
        Self {
            do_not_disturb: false,
            night_light: false,
            wifi: true,
            bluetooth: true,
            focus_mode: false,
            volume: 75,
            brightness: 80,
        }
    }
}

impl QuickSettingsState {
    fn get(&self, setting: QuickSetting) -> bool {
        match setting {
            QuickSetting::DoNotDisturb => self.do_not_disturb,
            QuickSetting::NightLight => self.night_light,
            QuickSetting::WiFi => self.wifi,
            QuickSetting::Bluetooth => self.bluetooth,
            QuickSetting::FocusMode => self.focus_mode,
        }
    }

    fn toggle(&mut self, setting: QuickSetting) {
        match setting {
            QuickSetting::DoNotDisturb => self.do_not_disturb = !self.do_not_disturb,
            QuickSetting::NightLight => self.night_light = !self.night_light,
            QuickSetting::WiFi => self.wifi = !self.wifi,
            QuickSetting::Bluetooth => self.bluetooth = !self.bluetooth,
            QuickSetting::FocusMode => self.focus_mode = !self.focus_mode,
        }
    }
}

// ============================================================================
// NotificationPane
// ============================================================================

/// The notification pane / action center.
pub struct NotificationPane {
    /// Current animation state.
    state: PaneState,
    /// Stored notifications (newest first).
    notifications: Vec<Notification>,
    /// Next notification ID to assign.
    next_id: u64,
    /// Quick settings state.
    quick_settings: QuickSettingsState,
    /// Per-app notification settings.
    app_settings: Vec<AppNotifSettings>,
    /// Scroll offset in the notification list (pixels).
    scroll_offset: f32,
    /// Pending output events.
    events: Vec<NotifPaneEvent>,
    /// Current "now" timestamp for grouping (updated on show/push).
    current_time: u64,
    /// Hover state: index of notification card being hovered (-1 = none).
    hovered_notif: Option<usize>,
    /// Whether the settings sub-view is showing.
    show_settings: bool,
    /// Animation speed (fraction per second).
    anim_speed: f32,
}

impl NotificationPane {
    /// Create a new notification pane (starts hidden).
    pub fn new() -> Self {
        Self {
            state: PaneState::Hidden,
            notifications: Vec::new(),
            next_id: 1,
            quick_settings: QuickSettingsState::default(),
            app_settings: Vec::new(),
            scroll_offset: 0.0,
            events: Vec::new(),
            current_time: 0,
            hovered_notif: None,
            show_settings: false,
            anim_speed: 5.0, // complete slide in ~0.2s
        }
    }

    // ========================================================================
    // Public API
    // ========================================================================

    /// Show the pane (begin slide-in animation).
    pub fn show(&mut self) {
        match self.state {
            PaneState::Hidden => {
                self.state = PaneState::SlideIn(0.0);
            }
            PaneState::SlideOut(p) => {
                // Reverse: convert remaining slide-out progress to slide-in.
                self.state = PaneState::SlideIn(1.0 - p);
            }
            _ => {}
        }
        self.show_settings = false;
    }

    /// Hide the pane (begin slide-out animation).
    pub fn hide(&mut self) {
        match self.state {
            PaneState::Visible => {
                self.state = PaneState::SlideOut(0.0);
            }
            PaneState::SlideIn(p) => {
                // Reverse: convert remaining slide-in progress to slide-out.
                self.state = PaneState::SlideOut(1.0 - p);
            }
            _ => {}
        }
    }

    /// Toggle visibility.
    pub fn toggle(&mut self) {
        match self.state {
            PaneState::Hidden | PaneState::SlideOut(_) => self.show(),
            PaneState::Visible | PaneState::SlideIn(_) => self.hide(),
        }
    }

    /// Push a new notification into the pane.
    ///
    /// If the pane already has `MAX_NOTIFICATIONS`, the oldest is evicted.
    /// Returns the assigned notification ID.
    pub fn push_notification(&mut self, mut notif: Notification) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        notif.id = id;

        // Ensure per-app settings exist.
        if !self.app_settings.iter().any(|s| s.app_name == notif.app_name) {
            self.app_settings
                .push(AppNotifSettings::new(notif.app_name.clone()));
        }

        // Insert at front (newest first).
        self.notifications.insert(0, notif);

        // Evict overflow.
        if self.notifications.len() > MAX_NOTIFICATIONS {
            self.notifications.truncate(MAX_NOTIFICATIONS);
        }

        id
    }

    /// Advance animation by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        let step = self.anim_speed * dt;
        match self.state {
            PaneState::SlideIn(p) => {
                let next = (p + step).min(1.0);
                if next >= 1.0 {
                    self.state = PaneState::Visible;
                } else {
                    self.state = PaneState::SlideIn(next);
                }
            }
            PaneState::SlideOut(p) => {
                let next = (p + step).min(1.0);
                if next >= 1.0 {
                    self.state = PaneState::Hidden;
                    self.events.push(NotifPaneEvent::Closed);
                } else {
                    self.state = PaneState::SlideOut(next);
                }
            }
            _ => {}
        }
    }

    /// Handle a mouse event. Coordinates are in screen space.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent, screen_width: f32, screen_height: f32) -> EventResult {
        if !self.state.is_visible() {
            return EventResult::Ignored;
        }

        let vis = self.state.visibility();
        let pane_x = screen_width - PANE_WIDTH * vis;

        // Click outside pane dismisses it.
        if event.x < pane_x {
            if matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
                self.hide();
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        // Relative coordinates within the pane.
        let rx = event.x - pane_x;
        let ry = event.y;

        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.handle_click(rx, ry, screen_height);
                EventResult::Consumed
            }
            MouseEventKind::Scroll { dy, .. } => {
                self.scroll_offset = (self.scroll_offset - dy * 30.0).max(0.0);
                EventResult::Consumed
            }
            MouseEventKind::Move => {
                self.update_hover(rx, ry, screen_height);
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    /// Handle a key event.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> EventResult {
        if !self.state.is_visible() {
            return EventResult::Ignored;
        }

        if event.pressed && event.key == Key::Escape {
            self.hide();
            return EventResult::Consumed;
        }

        // Scroll with arrow keys.
        if event.pressed {
            match event.key {
                Key::Down => {
                    self.scroll_offset += 40.0;
                    return EventResult::Consumed;
                }
                Key::Up => {
                    self.scroll_offset = (self.scroll_offset - 40.0).max(0.0);
                    return EventResult::Consumed;
                }
                Key::PageDown => {
                    self.scroll_offset += 200.0;
                    return EventResult::Consumed;
                }
                Key::PageUp => {
                    self.scroll_offset = (self.scroll_offset - 200.0).max(0.0);
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        EventResult::Consumed
    }

    /// Drain all pending events.
    pub fn drain_events(&mut self) -> Vec<NotifPaneEvent> {
        core::mem::take(&mut self.events)
    }

    /// Number of unread notifications.
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    /// Set the current timestamp (for time grouping).
    pub fn set_current_time(&mut self, now: u64) {
        self.current_time = now;
    }

    /// Get current pane state.
    pub fn pane_state(&self) -> PaneState {
        self.state
    }

    /// Get per-app settings (read-only).
    pub fn app_settings(&self) -> &[AppNotifSettings] {
        &self.app_settings
    }

    /// Get quick settings state for a specific toggle.
    pub fn quick_setting_value(&self, qs: QuickSetting) -> bool {
        self.quick_settings.get(qs)
    }

    /// Get volume (0..=100).
    pub fn volume(&self) -> u8 {
        self.quick_settings.volume
    }

    /// Get brightness (0..=100).
    pub fn brightness(&self) -> u8 {
        self.quick_settings.brightness
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the pane. Returns draw commands in screen space.
    // The render body builds up its command list incrementally with helper
    // calls between pushes; vec![...] would require relocating all of those.
    #[allow(clippy::vec_init_then_push)]
    pub fn render(&self, screen_width: f32, screen_height: f32) -> Vec<RenderCommand> {
        if !self.state.is_visible() {
            return Vec::new();
        }

        let vis = self.state.visibility();
        let pane_x = screen_width - PANE_WIDTH * vis;
        let mut cmds = Vec::new();

        // Dim overlay behind pane.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: screen_width,
            height: screen_height,
            color: Color::rgba(0, 0, 0, (60.0 * vis) as u8),
            corner_radii: CornerRadii::ZERO,
        });

        // Pane shadow.
        cmds.push(RenderCommand::BoxShadow {
            x: pane_x,
            y: 0.0,
            width: PANE_WIDTH,
            height: screen_height,
            offset_x: -4.0,
            offset_y: 0.0,
            blur: 16.0,
            spread: 0.0,
            color: theme::SHADOW,
            corner_radii: CornerRadii::ZERO,
        });

        // Pane background.
        cmds.push(RenderCommand::FillRect {
            x: pane_x,
            y: 0.0,
            width: PANE_WIDTH,
            height: screen_height,
            color: theme::PANE_BG,
            corner_radii: CornerRadii::ZERO,
        });

        // Clip to pane area.
        cmds.push(RenderCommand::PushClip {
            x: pane_x,
            y: 0.0,
            width: PANE_WIDTH,
            height: screen_height,
        });
        cmds.push(RenderCommand::PushTranslate { dx: pane_x, dy: 0.0 });

        // Render sections.
        let mut y = PANE_PADDING;

        // Header.
        y += self.render_header(&mut cmds, y);

        // Quick settings.
        y += self.render_quick_settings(&mut cmds, y);

        // Separator.
        y += 8.0;
        cmds.push(RenderCommand::Line {
            x1: PANE_PADDING,
            y1: y,
            x2: PANE_WIDTH - PANE_PADDING,
            y2: y,
            color: theme::SURFACE1,
            width: 1.0,
        });
        y += 8.0;

        if self.show_settings {
            self.render_app_settings(&mut cmds, y, screen_height - y);
        } else {
            self.render_notifications(&mut cmds, y, screen_height - y);
        }

        cmds.push(RenderCommand::PopTranslate);
        cmds.push(RenderCommand::PopClip);

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>, y: f32) -> f32 {
        // Title.
        let title = if self.show_settings {
            "Notification Settings"
        } else {
            "Notifications"
        };
        cmds.push(RenderCommand::Text {
            x: PANE_PADDING,
            y: y + 4.0,
            text: title.to_string(),
            color: theme::TEXT,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Unread badge.
        let unread = self.unread_count();
        if unread > 0 && !self.show_settings {
            let badge_text = if unread > 99 {
                "99+".to_string()
            } else {
                unread.to_string()
            };
            cmds.push(RenderCommand::FillRect {
                x: PANE_PADDING + 120.0,
                y: y + 4.0,
                width: 24.0,
                height: 18.0,
                color: theme::BLUE,
                corner_radii: CornerRadii::all(9.0),
            });
            cmds.push(RenderCommand::Text {
                x: PANE_PADDING + 126.0,
                y: y + 6.0,
                text: badge_text,
                color: theme::CRUST,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(20.0),
            });
        }

        if !self.show_settings {
            // "Clear all" button.
            let clear_x = PANE_WIDTH - PANE_PADDING - 60.0;
            cmds.push(RenderCommand::Text {
                x: clear_x,
                y: y + 6.0,
                text: "Clear all".to_string(),
                color: theme::BLUE,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(60.0),
            });

            // Settings gear link.
            let gear_x = PANE_WIDTH - PANE_PADDING - 130.0;
            cmds.push(RenderCommand::Text {
                x: gear_x,
                y: y + 6.0,
                text: "Settings".to_string(),
                color: theme::SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(60.0),
            });
        } else {
            // "Back" link.
            let back_x = PANE_WIDTH - PANE_PADDING - 40.0;
            cmds.push(RenderCommand::Text {
                x: back_x,
                y: y + 6.0,
                text: "Back".to_string(),
                color: theme::BLUE,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
            });
        }

        HEADER_HEIGHT
    }

    fn render_quick_settings(&self, cmds: &mut Vec<RenderCommand>, start_y: f32) -> f32 {
        let mut y = start_y;

        // Section title.
        cmds.push(RenderCommand::Text {
            x: PANE_PADDING,
            y,
            text: "Quick Settings".to_string(),
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });
        y += 20.0;

        // Toggle rows.
        for qs in QuickSetting::all() {
            let enabled = self.quick_settings.get(*qs);
            self.render_toggle_row(cmds, PANE_PADDING, y, qs.label(), enabled);
            y += QS_ROW_HEIGHT;
        }

        // Volume slider.
        y += 4.0;
        self.render_slider_row(cmds, PANE_PADDING, y, "Volume", self.quick_settings.volume);
        y += QS_ROW_HEIGHT;

        // Brightness slider.
        self.render_slider_row(cmds, PANE_PADDING, y, "Brightness", self.quick_settings.brightness);
        y += QS_ROW_HEIGHT;

        y - start_y
    }

    fn render_toggle_row(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, label: &str, enabled: bool) {
        // Label.
        cmds.push(RenderCommand::Text {
            x,
            y: y + 8.0,
            text: label.to_string(),
            color: theme::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(180.0),
        });

        // Toggle pill.
        let pill_x = PANE_WIDTH - PANE_PADDING - TOGGLE_WIDTH - PANE_PADDING;
        let pill_bg = if enabled { theme::BLUE } else { theme::SURFACE2 };
        cmds.push(RenderCommand::FillRect {
            x: pill_x,
            y: y + 6.0,
            width: TOGGLE_WIDTH,
            height: TOGGLE_HEIGHT,
            color: pill_bg,
            corner_radii: CornerRadii::all(TOGGLE_HEIGHT / 2.0),
        });

        // Toggle knob.
        let knob_radius = (TOGGLE_HEIGHT - 4.0) / 2.0;
        let knob_x = if enabled {
            pill_x + TOGGLE_WIDTH - knob_radius * 2.0 - 2.0
        } else {
            pill_x + 2.0
        };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 8.0,
            width: knob_radius * 2.0,
            height: knob_radius * 2.0,
            color: theme::TEXT,
            corner_radii: CornerRadii::all(knob_radius),
        });
    }

    fn render_slider_row(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, label: &str, value: u8) {
        // Label + value.
        cmds.push(RenderCommand::Text {
            x,
            y: y + 8.0,
            text: format!("{label}  {value}%"),
            color: theme::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(180.0),
        });

        // Slider track.
        let track_x = PANE_WIDTH - PANE_PADDING - SLIDER_WIDTH - PANE_PADDING;
        let track_y = y + 14.0;
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: track_y,
            width: SLIDER_WIDTH,
            height: SLIDER_HEIGHT,
            color: theme::SURFACE2,
            corner_radii: CornerRadii::all(SLIDER_HEIGHT / 2.0),
        });

        // Slider filled portion.
        let filled_width = SLIDER_WIDTH * (value as f32 / 100.0);
        cmds.push(RenderCommand::FillRect {
            x: track_x,
            y: track_y,
            width: filled_width,
            height: SLIDER_HEIGHT,
            color: theme::BLUE,
            corner_radii: CornerRadii::all(SLIDER_HEIGHT / 2.0),
        });

        // Slider thumb.
        let thumb_x = track_x + filled_width - 6.0;
        cmds.push(RenderCommand::FillRect {
            x: thumb_x,
            y: track_y - 3.0,
            width: 12.0,
            height: 12.0,
            color: theme::TEXT,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    fn render_notifications(&self, cmds: &mut Vec<RenderCommand>, start_y: f32, available_height: f32) {
        // Clip notifications to available area.
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: start_y,
            width: PANE_WIDTH,
            height: available_height,
        });

        if self.notifications.is_empty() {
            cmds.push(RenderCommand::Text {
                x: PANE_WIDTH / 2.0 - 80.0,
                y: start_y + 60.0,
                text: "No notifications".to_string(),
                color: theme::OVERLAY0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(160.0),
            });
            cmds.push(RenderCommand::PopClip);
            return;
        }

        let mut y = start_y - self.scroll_offset;
        let mut current_group: Option<TimeGroup> = None;

        for (idx, notif) in self.notifications.iter().enumerate() {
            let group = TimeGroup::classify(notif.timestamp, self.current_time);

            // Render group header if changed.
            if current_group != Some(group) {
                current_group = Some(group);
                if y + GROUP_HEADER_HEIGHT > start_y - 20.0 {
                    cmds.push(RenderCommand::Text {
                        x: PANE_PADDING,
                        y: y + 6.0,
                        text: group.label().to_string(),
                        color: theme::SUBTEXT0,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(100.0),
                    });
                }
                y += GROUP_HEADER_HEIGHT;
            }

            // Skip rendering if above visible area.
            if y + NOTIF_CARD_HEIGHT < start_y {
                y += NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING;
                continue;
            }
            // Stop rendering if below visible area.
            if y > start_y + available_height {
                break;
            }

            self.render_notification_card(cmds, idx, notif, PANE_PADDING, y);
            y += NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING;
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_notification_card(
        &self,
        cmds: &mut Vec<RenderCommand>,
        idx: usize,
        notif: &Notification,
        x: f32,
        y: f32,
    ) {
        let card_width = PANE_WIDTH - 2.0 * PANE_PADDING;
        let is_hovered = self.hovered_notif == Some(idx);
        let bg = if is_hovered { theme::HOVER_BG } else { theme::CARD_BG };

        // Card background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: card_width,
            height: NOTIF_CARD_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });

        // Unread indicator (left accent bar).
        if !notif.read {
            cmds.push(RenderCommand::FillRect {
                x,
                y: y + 4.0,
                width: 3.0,
                height: NOTIF_CARD_HEIGHT - 8.0,
                color: notif.priority.accent_color(),
                corner_radii: CornerRadii::all(1.5),
            });
        }

        // App name.
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 8.0,
            text: notif.app_name.clone(),
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card_width - 50.0),
        });

        // Timestamp (relative).
        let time_str = self.format_relative_time(notif.timestamp);
        cmds.push(RenderCommand::Text {
            x: x + card_width - 60.0,
            y: y + 8.0,
            text: time_str,
            color: theme::OVERLAY0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(55.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 26.0,
            text: notif.title.clone(),
            color: theme::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(card_width - 40.0),
        });

        // Body (truncated).
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 46.0,
            text: Self::truncate_body(&notif.body, 60),
            color: theme::SUBTEXT1,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card_width - 24.0),
        });

        // Dismiss button (X) — shown on hover.
        if is_hovered {
            let btn_x = x + card_width - DISMISS_BTN_SIZE - 8.0;
            let btn_y = y + 6.0;
            cmds.push(RenderCommand::FillRect {
                x: btn_x,
                y: btn_y,
                width: DISMISS_BTN_SIZE,
                height: DISMISS_BTN_SIZE,
                color: theme::SURFACE2,
                corner_radii: CornerRadii::all(DISMISS_BTN_SIZE / 2.0),
            });
            // "X" glyph.
            cmds.push(RenderCommand::Text {
                x: btn_x + 5.0,
                y: btn_y + 2.0,
                text: "x".to_string(),
                color: theme::TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_app_settings(&self, cmds: &mut Vec<RenderCommand>, start_y: f32, available_height: f32) {
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: start_y,
            width: PANE_WIDTH,
            height: available_height,
        });

        let mut y = start_y;

        // "Manage notifications" heading.
        cmds.push(RenderCommand::Text {
            x: PANE_PADDING,
            y,
            text: "Per-App Settings".to_string(),
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(150.0),
        });
        y += 24.0;

        for app in &self.app_settings {
            if y > start_y + available_height {
                break;
            }

            // App card.
            let card_width = PANE_WIDTH - 2.0 * PANE_PADDING;
            cmds.push(RenderCommand::FillRect {
                x: PANE_PADDING,
                y,
                width: card_width,
                height: 100.0,
                color: theme::CARD_BG,
                corner_radii: CornerRadii::all(CARD_RADIUS),
            });

            // App name.
            cmds.push(RenderCommand::Text {
                x: PANE_PADDING + 12.0,
                y: y + 10.0,
                text: app.app_name.clone(),
                color: theme::TEXT,
                font_size: 13.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
            });

            // Priority badge.
            let prio_color = app.priority.accent_color();
            cmds.push(RenderCommand::FillRect {
                x: PANE_PADDING + 12.0,
                y: y + 32.0,
                width: 50.0,
                height: 16.0,
                color: prio_color,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: PANE_PADDING + 16.0,
                y: y + 34.0,
                text: app.priority.label().to_string(),
                color: theme::CRUST,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(45.0),
            });

            // Enabled toggle.
            let enabled_x = card_width - TOGGLE_WIDTH;
            let pill_bg = if app.enabled { theme::GREEN } else { theme::SURFACE2 };
            cmds.push(RenderCommand::FillRect {
                x: enabled_x,
                y: y + 10.0,
                width: TOGGLE_WIDTH,
                height: TOGGLE_HEIGHT,
                color: pill_bg,
                corner_radii: CornerRadii::all(TOGGLE_HEIGHT / 2.0),
            });

            // Status text row.
            let mut status_parts = Vec::new();
            if app.sound {
                status_parts.push("Sound");
            }
            if app.banner {
                status_parts.push("Banner");
            }
            if !app.enabled {
                status_parts.push("Disabled");
            }
            cmds.push(RenderCommand::Text {
                x: PANE_PADDING + 12.0,
                y: y + 60.0,
                text: status_parts.join(" | "),
                color: theme::OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(card_width - 30.0),
            });

            y += 108.0;
        }

        // "Manage notifications" link at bottom.
        if !self.app_settings.is_empty() {
            y += 12.0;
            cmds.push(RenderCommand::Text {
                x: PANE_PADDING,
                y,
                text: "Open full notification settings...".to_string(),
                color: theme::BLUE,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    // ========================================================================
    // Interaction handling
    // ========================================================================

    fn handle_click(&mut self, rx: f32, ry: f32, screen_height: f32) {
        let mut y = PANE_PADDING;

        // Header area.
        if ry < y + HEADER_HEIGHT {
            if !self.show_settings {
                // "Clear all" button region.
                let clear_x = PANE_WIDTH - PANE_PADDING - 60.0;
                if rx >= clear_x && rx <= clear_x + 60.0 {
                    self.clear_all();
                    return;
                }
                // "Settings" link region.
                let gear_x = PANE_WIDTH - PANE_PADDING - 130.0;
                if rx >= gear_x && rx <= gear_x + 60.0 {
                    self.show_settings = true;
                    return;
                }
            } else {
                // "Back" button region.
                let back_x = PANE_WIDTH - PANE_PADDING - 40.0;
                if rx >= back_x && rx <= back_x + 40.0 {
                    self.show_settings = false;
                    return;
                }
            }
            return;
        }
        y += HEADER_HEIGHT;

        // Quick settings area.
        let qs_end = y + QUICK_SETTINGS_HEIGHT;
        if ry >= y && ry < qs_end {
            self.handle_quick_settings_click(rx, ry - y);
            return;
        }
        y = qs_end + 16.0; // separator

        if self.show_settings {
            // App settings: each card is 108px tall.
            self.handle_app_settings_click(rx, ry - y);
        } else {
            // Notifications area.
            self.handle_notification_click(rx, ry, y, screen_height);
        }
    }

    fn handle_quick_settings_click(&mut self, rx: f32, local_y: f32) {
        // Skip section title (20px).
        let content_y = local_y - 20.0;
        if content_y < 0.0 {
            return;
        }

        let qs_list = QuickSetting::all();
        let toggle_count = qs_list.len() as f32;
        let toggle_area_end = toggle_count * QS_ROW_HEIGHT;

        if content_y < toggle_area_end {
            // Which toggle row?
            let idx = (content_y / QS_ROW_HEIGHT) as usize;
            if idx < qs_list.len() {
                // Check if click is on the toggle pill area (right side).
                let pill_x = PANE_WIDTH - PANE_PADDING - TOGGLE_WIDTH - PANE_PADDING;
                if rx >= pill_x {
                    let qs = qs_list[idx];
                    self.quick_settings.toggle(qs);
                    self.events.push(NotifPaneEvent::QuickSettingToggled(qs));
                }
            }
        } else {
            // Slider area: volume then brightness.
            let slider_y = content_y - toggle_area_end - 4.0;
            let track_x = PANE_WIDTH - PANE_PADDING - SLIDER_WIDTH - PANE_PADDING;
            if rx >= track_x && rx <= track_x + SLIDER_WIDTH {
                let frac = ((rx - track_x) / SLIDER_WIDTH).clamp(0.0, 1.0);
                let value = (frac * 100.0) as u8;
                if slider_y < QS_ROW_HEIGHT {
                    self.quick_settings.volume = value;
                } else if slider_y < 2.0 * QS_ROW_HEIGHT {
                    self.quick_settings.brightness = value;
                }
            }
        }
    }

    fn handle_notification_click(&mut self, rx: f32, ry: f32, content_start: f32, _screen_height: f32) {
        let adjusted_y = ry - content_start + self.scroll_offset;
        let mut y: f32 = 0.0;
        let mut current_group: Option<TimeGroup> = None;

        for (idx, notif) in self.notifications.iter().enumerate() {
            let group = TimeGroup::classify(notif.timestamp, self.current_time);
            if current_group != Some(group) {
                current_group = Some(group);
                y += GROUP_HEADER_HEIGHT;
            }

            if adjusted_y >= y && adjusted_y < y + NOTIF_CARD_HEIGHT {
                // Check if dismiss button was clicked.
                let card_width = PANE_WIDTH - 2.0 * PANE_PADDING;
                let btn_x = PANE_PADDING + card_width - DISMISS_BTN_SIZE - 8.0;
                if rx >= btn_x && rx <= btn_x + DISMISS_BTN_SIZE && (adjusted_y - y) < DISMISS_BTN_SIZE + 6.0 {
                    let id = notif.id;
                    self.dismiss_notification(idx);
                    self.events.push(NotifPaneEvent::NotificationDismissed(id));
                } else {
                    // Click on notification body.
                    let id = notif.id;
                    self.notifications[idx].read = true;
                    self.events.push(NotifPaneEvent::NotificationClicked(id));
                }
                return;
            }

            y += NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING;
        }
    }

    fn handle_app_settings_click(&mut self, rx: f32, local_y: f32) {
        // Skip heading (24px).
        let content_y = local_y - 24.0;
        if content_y < 0.0 {
            return;
        }

        let card_height = 108.0_f32;
        let idx = (content_y / card_height) as usize;
        if idx >= self.app_settings.len() {
            return;
        }

        let card_local_y = content_y - (idx as f32 * card_height);
        let card_width = PANE_WIDTH - 2.0 * PANE_PADDING;

        // Toggle area (top-right of card).
        let enabled_x = card_width - TOGGLE_WIDTH;
        if rx >= enabled_x && card_local_y < 35.0 {
            let app = &mut self.app_settings[idx];
            app.enabled = !app.enabled;
            self.events.push(NotifPaneEvent::SettingChanged {
                app: app.app_name.clone(),
                setting: AppSettingKind::Enabled,
                value: SettingValue::Bool(app.enabled),
            });
        }
    }

    fn update_hover(&mut self, _rx: f32, ry: f32, _screen_height: f32) {
        // Determine which notification card is hovered (simplified).
        let content_start = PANE_PADDING + HEADER_HEIGHT + QUICK_SETTINGS_HEIGHT + 16.0;
        if ry < content_start || self.show_settings {
            self.hovered_notif = None;
            return;
        }

        let adjusted_y = ry - content_start + self.scroll_offset;
        let mut y: f32 = 0.0;
        let mut current_group: Option<TimeGroup> = None;

        for (idx, notif) in self.notifications.iter().enumerate() {
            let group = TimeGroup::classify(notif.timestamp, self.current_time);
            if current_group != Some(group) {
                current_group = Some(group);
                y += GROUP_HEADER_HEIGHT;
            }

            if adjusted_y >= y && adjusted_y < y + NOTIF_CARD_HEIGHT {
                self.hovered_notif = Some(idx);
                return;
            }

            y += NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING;
        }

        self.hovered_notif = None;
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    fn clear_all(&mut self) {
        self.notifications.clear();
        self.scroll_offset = 0.0;
        self.hovered_notif = None;
        self.events.push(NotifPaneEvent::ClearAll);
    }

    fn dismiss_notification(&mut self, idx: usize) {
        if idx < self.notifications.len() {
            self.notifications.remove(idx);
            // Adjust hover if needed.
            if let Some(h) = self.hovered_notif
                && h >= self.notifications.len() {
                    self.hovered_notif = None;
                }
        }
    }

    fn format_relative_time(&self, timestamp: u64) -> String {
        if self.current_time == 0 || timestamp > self.current_time {
            return "now".to_string();
        }
        let diff = self.current_time - timestamp;
        if diff < 60 {
            "just now".to_string()
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < SECS_PER_DAY {
            format!("{}h ago", diff / 3600)
        } else if diff < 2 * SECS_PER_DAY {
            "yesterday".to_string()
        } else if diff < 7 * SECS_PER_DAY {
            format!("{}d ago", diff / SECS_PER_DAY)
        } else {
            format!("{}w ago", diff / (7 * SECS_PER_DAY))
        }
    }

    fn truncate_body(body: &str, max_chars: usize) -> String {
        if body.len() <= max_chars {
            body.to_string()
        } else {
            let mut s: String = body.chars().take(max_chars - 3).collect();
            s.push_str("...");
            s
        }
    }
}

impl Default for NotificationPane {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_notif(app: &str, title: &str, ts: u64) -> Notification {
        Notification {
            id: 0,
            app_name: app.to_string(),
            title: title.to_string(),
            body: "Some notification body text".to_string(),
            timestamp: ts,
            priority: NotifPriority::Normal,
            read: false,
            action: None,
        }
    }

    // ========================================================================
    // Notification storage tests
    // ========================================================================

    #[test]
    fn push_assigns_incrementing_ids() {
        let mut pane = NotificationPane::new();
        let id1 = pane.push_notification(make_notif("App1", "First", 1000));
        let id2 = pane.push_notification(make_notif("App2", "Second", 1001));
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn push_inserts_newest_first() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("App1", "First", 1000));
        pane.push_notification(make_notif("App2", "Second", 2000));
        assert_eq!(pane.notifications[0].title, "Second");
        assert_eq!(pane.notifications[1].title, "First");
    }

    #[test]
    fn push_evicts_oldest_at_capacity() {
        let mut pane = NotificationPane::new();
        for i in 0..MAX_NOTIFICATIONS + 10 {
            pane.push_notification(make_notif("App", &format!("Notif {i}"), i as u64));
        }
        assert_eq!(pane.notifications.len(), MAX_NOTIFICATIONS);
        // The most recent should be at the front.
        assert_eq!(
            pane.notifications[0].title,
            format!("Notif {}", MAX_NOTIFICATIONS + 9)
        );
    }

    #[test]
    fn unread_count_reflects_read_state() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("A", "1", 100));
        pane.push_notification(make_notif("B", "2", 200));
        assert_eq!(pane.unread_count(), 2);
        pane.notifications[0].read = true;
        assert_eq!(pane.unread_count(), 1);
    }

    // ========================================================================
    // Time grouping tests
    // ========================================================================

    #[test]
    fn time_group_today() {
        let now = 100_000;
        assert_eq!(TimeGroup::classify(now - 100, now), TimeGroup::Today);
        assert_eq!(TimeGroup::classify(now - 3600, now), TimeGroup::Today);
        assert_eq!(TimeGroup::classify(now, now), TimeGroup::Today);
    }

    #[test]
    fn time_group_yesterday() {
        let now = 200_000;
        let yesterday = now - SECS_PER_DAY - 100;
        assert_eq!(TimeGroup::classify(yesterday, now), TimeGroup::Yesterday);
    }

    #[test]
    fn time_group_this_week() {
        let now = 1_000_000;
        let three_days_ago = now - 3 * SECS_PER_DAY;
        assert_eq!(TimeGroup::classify(three_days_ago, now), TimeGroup::ThisWeek);
    }

    #[test]
    fn time_group_older() {
        let now = 2_000_000;
        let two_weeks_ago = now - 14 * SECS_PER_DAY;
        assert_eq!(TimeGroup::classify(two_weeks_ago, now), TimeGroup::Older);
    }

    #[test]
    fn time_group_future_classified_as_today() {
        let now = 5000;
        assert_eq!(TimeGroup::classify(now + 100, now), TimeGroup::Today);
    }

    // ========================================================================
    // Dismissal tests
    // ========================================================================

    #[test]
    fn dismiss_removes_correct_notification() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("A", "First", 100));
        pane.push_notification(make_notif("B", "Second", 200));
        pane.push_notification(make_notif("C", "Third", 300));

        // Dismiss middle notification (index 1 = "Second" since newest is first).
        // After push order: [Third, Second, First]
        pane.dismiss_notification(1);
        assert_eq!(pane.notifications.len(), 2);
        assert_eq!(pane.notifications[0].title, "Third");
        assert_eq!(pane.notifications[1].title, "First");
    }

    #[test]
    fn clear_all_empties_and_emits_event() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("A", "1", 100));
        pane.push_notification(make_notif("B", "2", 200));
        pane.clear_all();
        assert!(pane.notifications.is_empty());
        let events = pane.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], NotifPaneEvent::ClearAll));
    }

    #[test]
    fn dismiss_out_of_bounds_is_safe() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("A", "1", 100));
        pane.dismiss_notification(99); // should not panic
        assert_eq!(pane.notifications.len(), 1);
    }

    // ========================================================================
    // Settings persistence tests
    // ========================================================================

    #[test]
    fn push_creates_app_settings_entry() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("MyApp", "Hello", 100));
        assert_eq!(pane.app_settings.len(), 1);
        assert_eq!(pane.app_settings[0].app_name, "MyApp");
        assert!(pane.app_settings[0].enabled);
        assert!(pane.app_settings[0].sound);
        assert!(pane.app_settings[0].banner);
        assert_eq!(pane.app_settings[0].priority, NotifPriority::Normal);
    }

    #[test]
    fn push_does_not_duplicate_app_settings() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("MyApp", "First", 100));
        pane.push_notification(make_notif("MyApp", "Second", 200));
        assert_eq!(pane.app_settings.len(), 1);
    }

    #[test]
    fn multiple_apps_get_separate_settings() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("App1", "Hello", 100));
        pane.push_notification(make_notif("App2", "World", 200));
        assert_eq!(pane.app_settings.len(), 2);
    }

    #[test]
    fn quick_setting_toggle_works() {
        let mut pane = NotificationPane::new();
        assert!(!pane.quick_setting_value(QuickSetting::DoNotDisturb));
        pane.quick_settings.toggle(QuickSetting::DoNotDisturb);
        assert!(pane.quick_setting_value(QuickSetting::DoNotDisturb));
        pane.quick_settings.toggle(QuickSetting::DoNotDisturb);
        assert!(!pane.quick_setting_value(QuickSetting::DoNotDisturb));
    }

    #[test]
    fn quick_settings_defaults_correct() {
        let pane = NotificationPane::new();
        assert!(!pane.quick_setting_value(QuickSetting::DoNotDisturb));
        assert!(!pane.quick_setting_value(QuickSetting::NightLight));
        assert!(pane.quick_setting_value(QuickSetting::WiFi));
        assert!(pane.quick_setting_value(QuickSetting::Bluetooth));
        assert!(!pane.quick_setting_value(QuickSetting::FocusMode));
        assert_eq!(pane.volume(), 75);
        assert_eq!(pane.brightness(), 80);
    }

    // ========================================================================
    // Animation state tests
    // ========================================================================

    #[test]
    fn starts_hidden() {
        let pane = NotificationPane::new();
        assert_eq!(pane.pane_state(), PaneState::Hidden);
    }

    #[test]
    fn show_starts_slide_in() {
        let mut pane = NotificationPane::new();
        pane.show();
        assert!(matches!(pane.pane_state(), PaneState::SlideIn(_)));
    }

    #[test]
    fn tick_advances_slide_in() {
        let mut pane = NotificationPane::new();
        pane.show();
        pane.tick(0.05); // 5.0 * 0.05 = 0.25
        match pane.pane_state() {
            PaneState::SlideIn(p) => assert!((p - 0.25).abs() < 0.001),
            other => panic!("Expected SlideIn, got {:?}", other),
        }
    }

    #[test]
    fn slide_in_completes_to_visible() {
        let mut pane = NotificationPane::new();
        pane.show();
        pane.tick(1.0); // 5.0 * 1.0 = 5.0, clamped to 1.0
        assert_eq!(pane.pane_state(), PaneState::Visible);
    }

    #[test]
    fn hide_from_visible_starts_slide_out() {
        let mut pane = NotificationPane::new();
        pane.state = PaneState::Visible;
        pane.hide();
        assert!(matches!(pane.pane_state(), PaneState::SlideOut(_)));
    }

    #[test]
    fn slide_out_completes_to_hidden_with_event() {
        let mut pane = NotificationPane::new();
        pane.state = PaneState::Visible;
        pane.hide();
        pane.tick(1.0); // completes
        assert_eq!(pane.pane_state(), PaneState::Hidden);
        let events = pane.drain_events();
        assert!(events.iter().any(|e| matches!(e, NotifPaneEvent::Closed)));
    }

    #[test]
    fn toggle_from_hidden_shows() {
        let mut pane = NotificationPane::new();
        pane.toggle();
        assert!(matches!(pane.pane_state(), PaneState::SlideIn(_)));
    }

    #[test]
    fn toggle_from_visible_hides() {
        let mut pane = NotificationPane::new();
        pane.state = PaneState::Visible;
        pane.toggle();
        assert!(matches!(pane.pane_state(), PaneState::SlideOut(_)));
    }

    #[test]
    fn reverse_slide_in_to_slide_out() {
        let mut pane = NotificationPane::new();
        pane.show();
        pane.tick(0.1); // progress = 0.5
        let progress_before = match pane.pane_state() {
            PaneState::SlideIn(p) => p,
            _ => panic!("expected SlideIn"),
        };
        pane.hide(); // should reverse
        match pane.pane_state() {
            PaneState::SlideOut(p) => {
                assert!((p - (1.0 - progress_before)).abs() < 0.001);
            }
            other => panic!("Expected SlideOut, got {:?}", other),
        }
    }

    #[test]
    fn visibility_fraction_correct() {
        assert_eq!(PaneState::Hidden.visibility(), 0.0);
        assert_eq!(PaneState::Visible.visibility(), 1.0);
        assert!((PaneState::SlideIn(0.5).visibility() - 0.5).abs() < 0.001);
        assert!((PaneState::SlideOut(0.3).visibility() - 0.7).abs() < 0.001);
    }

    // ========================================================================
    // Event handling tests
    // ========================================================================

    #[test]
    fn escape_key_hides_pane() {
        let mut pane = NotificationPane::new();
        pane.state = PaneState::Visible;
        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: None,
        };
        let result = pane.handle_key_event(&event);
        assert_eq!(result, EventResult::Consumed);
        assert!(matches!(pane.pane_state(), PaneState::SlideOut(_)));
    }

    #[test]
    fn key_events_ignored_when_hidden() {
        let mut pane = NotificationPane::new();
        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: None,
        };
        let result = pane.handle_key_event(&event);
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn render_returns_empty_when_hidden() {
        let pane = NotificationPane::new();
        let cmds = pane.render(1920.0, 1080.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_produces_commands_when_visible() {
        let mut pane = NotificationPane::new();
        pane.state = PaneState::Visible;
        pane.push_notification(make_notif("Test", "Hello", 100));
        let cmds = pane.render(1920.0, 1080.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn drain_events_clears_buffer() {
        let mut pane = NotificationPane::new();
        pane.push_notification(make_notif("A", "1", 100));
        pane.clear_all();
        let events = pane.drain_events();
        assert_eq!(events.len(), 1);
        let events2 = pane.drain_events();
        assert!(events2.is_empty());
    }

    // ========================================================================
    // Relative time formatting
    // ========================================================================

    #[test]
    fn format_relative_time_just_now() {
        let mut pane = NotificationPane::new();
        pane.current_time = 1000;
        assert_eq!(pane.format_relative_time(999), "just now");
        assert_eq!(pane.format_relative_time(950), "just now");
    }

    #[test]
    fn format_relative_time_minutes() {
        let mut pane = NotificationPane::new();
        pane.current_time = 10000;
        assert_eq!(pane.format_relative_time(10000 - 120), "2m ago");
        assert_eq!(pane.format_relative_time(10000 - 3599), "59m ago");
    }

    #[test]
    fn format_relative_time_hours() {
        let mut pane = NotificationPane::new();
        pane.current_time = 100_000;
        assert_eq!(pane.format_relative_time(100_000 - 7200), "2h ago");
    }

    #[test]
    fn format_relative_time_days() {
        let mut pane = NotificationPane::new();
        pane.current_time = 1_000_000;
        assert_eq!(pane.format_relative_time(1_000_000 - 3 * SECS_PER_DAY), "3d ago");
    }

    // ========================================================================
    // Body truncation
    // ========================================================================

    #[test]
    fn truncate_short_body_unchanged() {
        let body = "Short text";
        assert_eq!(NotificationPane::truncate_body(body, 60), "Short text");
    }

    #[test]
    fn truncate_long_body_adds_ellipsis() {
        let body = "A".repeat(100);
        let result = NotificationPane::truncate_body(&body, 60);
        assert_eq!(result.len(), 60);
        assert!(result.ends_with("..."));
    }
}
