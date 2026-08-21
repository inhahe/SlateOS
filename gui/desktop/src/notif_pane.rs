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
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;

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

/// Height of the quick settings section: a section title, one row per toggle,
/// a gap, then the volume and brightness sliders.
///
/// Derived rather than declared, because it was declared once and was wrong.
/// `render_quick_settings` walks the rows and returns what it actually drew;
/// the two hit tests used this constant instead, and the two answers were 276
/// and 200. Every notification card was therefore hit-tested 76 px — most of a
/// card — from where it had been painted, so clicking a notification opened its
/// neighbour and the hover highlight sat under the pointer's card rather than
/// on it. Keeping the arithmetic in one place is what stops that recurring.
const QUICK_SETTINGS_HEIGHT: f32 = QS_TITLE_HEIGHT
    + QuickSetting::COUNT as f32 * QS_ROW_HEIGHT
    + QS_SLIDER_GAP
    + 2.0 * QS_ROW_HEIGHT;

/// Height of the "Quick Settings" caption above the toggle rows.
const QS_TITLE_HEIGHT: f32 = 20.0;

/// Gap between the last toggle row and the first slider.
const QS_SLIDER_GAP: f32 = 4.0;

/// Space between the quick-settings block and the list below it: a gap, the
/// separator rule, and a matching gap.
const QS_SEPARATOR_HEIGHT: f32 = 16.0;

/// Height of the header row (title + clear all).
const HEADER_HEIGHT: f32 = 44.0;

/// Height of a time-group header ("Today", "Yesterday", etc.).
const GROUP_HEADER_HEIGHT: f32 = 28.0;

/// Height of the "Per-App Settings" caption above the first app card.
const APP_HEADING_HEIGHT: f32 = 24.0;

/// Height of the card the per-app settings list paints for one app.
const APP_CARD_HEIGHT: f32 = 100.0;

/// Distance from one app card's top to the next.
///
/// Eight pixels more than [`APP_CARD_HEIGHT`]: the difference is the gutter
/// between two cards, which the renderer paints nothing in. A hit test that
/// divides by the pitch and stops there hands that gutter to the card above
/// it, which is what `handle_app_settings_click` used to do.
const APP_CARD_PITCH: f32 = 108.0;

/// Where the enabled pill sits below its app card's top.
const APP_TOGGLE_TOP: f32 = 10.0;

/// How far one arrow-key press scrolls the list.
const ARROW_KEY_STEP: f32 = 40.0;

/// Viewport height assumed before the pane has been told a real one.
///
/// Only ever used by a pane that has never been rendered or hit-tested, which
/// is a pane the user cannot yet have scrolled.
const DEFAULT_SCREEN_HEIGHT: f32 = 1080.0;

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

/// Horizontal space a notification card's body preview loses to the card's
/// left and right insets. The body is a one-line preview, so it is elided to
/// `card_width - BODY_INSET` — the same width handed to the render command's
/// `max_width`, so the elision and the clip agree by construction.
const BODY_INSET: f32 = 24.0;

/// Font size of the body preview. Named because the elision has to measure the
/// text at exactly the size it will be drawn at.
const BODY_FONT_SIZE: f32 = 12.0;

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

    const fn all() -> &'static [Self] {
        &[
            Self::DoNotDisturb,
            Self::NightLight,
            Self::WiFi,
            Self::Bluetooth,
            Self::FocusMode,
        ]
    }

    /// How many toggles the quick-settings block draws.
    ///
    /// Taken from [`all`](Self::all) rather than written out, so that adding a
    /// toggle moves the list below it by exactly one row instead of leaving
    /// `QUICK_SETTINGS_HEIGHT` behind and putting the hit test back out of step
    /// with the renderer.
    const COUNT: usize = Self::all().len();
}

/// What the quick-settings block has at a given height.
///
/// The block is a caption, five toggle rows, a four-pixel gap and two slider
/// rows. The renderer walked that list adding heights up; the hit test walked
/// it again subtracting them back off. Two hand-inverted walks of one layout
/// is the arrangement that put every notification card 76 px from where it was
/// drawn, and it survived that fix in this one function. It is now a single
/// pair: [`NotificationPane::qs_toggle_top`] / [`NotificationPane::qs_slider_top`]
/// place the rows, and [`NotificationPane::qs_at`] is their inverse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QsHit {
    /// The `idx`-th toggle in [`QuickSetting::all`].
    Toggle(usize),
    Volume,
    Brightness,
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
///
/// `PartialEq` is derived so a test can state the whole expected drain in a
/// single `assert_eq!` rather than picking one variant apart by hand — which is
/// what lets the layout tests below assert "this click selects *that* card"
/// instead of merely "some card was selected".
#[derive(Clone, Debug, PartialEq)]
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
    /// Source of notification IDs.
    ids: IdSeq,
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
    /// Height of the screen the pane was last drawn on, or last handed for a
    /// hit test.
    ///
    /// The pane is as tall as the screen, so this is also the height of its
    /// scrolling viewport — and without it there is no upper bound to clamp
    /// `scroll_offset` against. It used to arrive only as a parameter of
    /// `render` and `handle_mouse_event` and be forgotten immediately, which is
    /// *why* the wheel and the arrow keys could scroll the list past its end
    /// into unbounded empty space with no way back but Home.
    screen_height: f32,
}

impl NotificationPane {
    /// Create a new notification pane (starts hidden).
    pub fn new() -> Self {
        Self {
            state: PaneState::Hidden,
            notifications: Vec::new(),
            ids: IdSeq::new(),
            quick_settings: QuickSettingsState::default(),
            app_settings: Vec::new(),
            scroll_offset: 0.0,
            events: Vec::new(),
            current_time: 0,
            hovered_notif: None,
            show_settings: false,
            anim_speed: 5.0, // complete slide in ~0.2s
            screen_height: DEFAULT_SCREEN_HEIGHT,
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
        let id = self.ids.issue_infallible();
        notif.id = id;

        // Ensure per-app settings exist.
        if !self
            .app_settings
            .iter()
            .any(|s| s.app_name == notif.app_name)
        {
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
    pub fn handle_mouse_event(
        &mut self,
        event: &MouseEvent,
        screen_width: f32,
        screen_height: f32,
    ) -> EventResult {
        if !self.state.is_visible() {
            return EventResult::Ignored;
        }
        // The viewport height arrives here and nowhere else on the input path,
        // so this is where the scroll bound learns about it.
        self.note_screen_height(screen_height);

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
                // `dy` is in notches, not pixels. This was `dy * 30.0` -- a
                // private pixels-per-notch constant, one of the twelve
                // different ones `MouseEventKind::Scroll` was invented to put
                // an end to. A notch is three rows, and a row here is a card.
                self.scroll_offset += wheel::pixels(*dy, NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING);
                self.clamp_scroll();
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
                // Each of these four used to clamp at one end only -- the
                // two that scrolled towards the end clamped at neither -- so
                // holding Down walked the list into empty space indefinitely.
                Key::Down => {
                    self.scroll_offset += ARROW_KEY_STEP;
                    self.clamp_scroll();
                    return EventResult::Consumed;
                }
                Key::Up => {
                    self.scroll_offset -= ARROW_KEY_STEP;
                    self.clamp_scroll();
                    return EventResult::Consumed;
                }
                Key::PageDown => {
                    self.scroll_offset += self.list_height();
                    self.clamp_scroll();
                    return EventResult::Consumed;
                }
                Key::PageUp => {
                    self.scroll_offset -= self.list_height();
                    self.clamp_scroll();
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

    // ========================================================================
    // Layout
    //
    // One definition each, because there used to be three: the renderer walked
    // the sections adding up measured heights, while `handle_click` and
    // `update_hover` each rebuilt the same total from constants, and the
    // totals disagreed. Anything that needs to know where a card sits asks
    // here.
    // ========================================================================

    /// Where the quick-settings block begins, in pane-local coordinates.
    const fn qs_start_y() -> f32 {
        PANE_PADDING + HEADER_HEIGHT
    }

    /// Where the scrolling list begins, in pane-local coordinates.
    const fn list_start_y() -> f32 {
        Self::qs_start_y() + QUICK_SETTINGS_HEIGHT + QS_SEPARATOR_HEIGHT
    }

    /// Top of the `idx`-th toggle row, relative to the quick-settings block's
    /// own top.
    #[allow(clippy::cast_precision_loss)]
    fn qs_toggle_top(idx: usize) -> f32 {
        QS_TITLE_HEIGHT + (idx as f32) * QS_ROW_HEIGHT
    }

    /// Top of the `slot`-th slider row — `0` is volume, `1` is brightness —
    /// relative to the quick-settings block's own top.
    #[allow(clippy::cast_precision_loss)]
    fn qs_slider_top(slot: usize) -> f32 {
        Self::qs_toggle_top(QuickSetting::COUNT) + QS_SLIDER_GAP + (slot as f32) * QS_ROW_HEIGHT
    }

    /// What the quick-settings block put at `local_y`, measured from the
    /// block's own top.
    ///
    /// `None` for the section caption above the toggles, for the gap between
    /// the last toggle and the volume slider, and for anything past the
    /// brightness slider. The gap in particular used to answer *volume*: the
    /// old walk computed `content_y - toggle_area_end - QS_SLIDER_GAP` and
    /// then asked only whether the result was below one row, never whether it
    /// was above zero, so the four blank pixels the renderer leaves between
    /// the toggles and the sliders set the volume when clicked.
    fn qs_at(local_y: f32) -> Option<QsHit> {
        if !local_y.is_finite() || local_y < QS_TITLE_HEIGHT {
            return None;
        }
        let toggles_end = Self::qs_toggle_top(QuickSetting::COUNT);
        if local_y < toggles_end {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let idx = ((local_y - QS_TITLE_HEIGHT) / QS_ROW_HEIGHT) as usize;
            return (idx < QuickSetting::COUNT).then_some(QsHit::Toggle(idx));
        }
        let first_slider = Self::qs_slider_top(0);
        if local_y < first_slider {
            // The gap. The renderer paints nothing here.
            return None;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let slot = ((local_y - first_slider) / QS_ROW_HEIGHT) as usize;
        match slot {
            0 => Some(QsHit::Volume),
            1 => Some(QsHit::Brightness),
            _ => None,
        }
    }

    /// Height of the scrolling viewport.
    fn list_height(&self) -> f32 {
        (self.screen_height - Self::list_start_y()).max(0.0)
    }

    /// Width of an app card in the per-app settings list, and of a
    /// notification card: both are the pane inset by its padding on each side.
    fn card_width() -> f32 {
        PANE_WIDTH - 2.0 * PANE_PADDING
    }

    /// Top of the `idx`-th app card, relative to the list's own top.
    #[allow(clippy::cast_precision_loss)]
    fn app_card_top(idx: usize) -> f32 {
        APP_HEADING_HEIGHT + (idx as f32) * APP_CARD_PITCH
    }

    /// The app card painted at `local_y`, measured from the list's own top.
    ///
    /// `None` for the caption above the first card, for the eight-pixel gutter
    /// between two cards, for anything past the last app, and for anything
    /// below the clip the renderer draws inside — each of which the old walk
    /// answered with a card, because it divided by [`APP_CARD_PITCH`] and
    /// asked only whether the quotient was in range. A click in the gutter
    /// toggled the card above it; a click below the pane's bottom edge toggled
    /// an app that is not on screen.
    fn app_card_at(&self, local_y: f32) -> Option<usize> {
        // The `is_finite` test is belt-and-braces: a NaN is not less than zero
        // and not past the viewport, so it clears both bounds below by failing
        // them -- but it also fails the two comparisons in the walk, so the
        // walk refuses it anyway. What the guard actually protects against is
        // someone rewriting the walk back into `(local_y / PITCH) as usize`,
        // which is what the old code did and which turns a NaN into card 0.
        if !local_y.is_finite() || local_y < 0.0 || local_y >= self.list_height() {
            return None;
        }
        // A walk rather than a division, so the gutter between two cards can
        // answer honestly: it is inside no card's painted height, and
        // `position`-style search says so by finding nothing.
        (0..self.app_settings.len()).find(|&idx| {
            let top = Self::app_card_top(idx);
            local_y >= top && local_y < top + APP_CARD_HEIGHT
        })
    }

    /// The enabled pill on the app card whose top is at `card_top`, as
    /// `(x, y, width, height)` in the same coordinates as `card_top`.
    ///
    /// The renderer paints this rectangle and the hit test accepts exactly it.
    /// It used to accept `rx >= x` with no right edge at all and a 35-pixel
    /// band starting at the card's top rather than the pill's, so the app
    /// name beside the pill, and a strip running off the side of the pane,
    /// both switched the app's notifications off.
    fn app_toggle_rect(card_top: f32) -> (f32, f32, f32, f32) {
        (
            Self::card_width() - TOGGLE_WIDTH,
            card_top + APP_TOGGLE_TOP,
            TOGGLE_WIDTH,
            TOGGLE_HEIGHT,
        )
    }

    /// The top of each notification card, relative to the start of the list
    /// and before scrolling, in the order the cards are drawn.
    ///
    /// A card is not simply `idx * (height + spacing)`: a time-group header is
    /// inserted wherever the group changes, so the offsets depend on the
    /// timestamps. That is precisely the arithmetic the renderer and the two
    /// hit tests each used to carry a copy of.
    fn card_tops(&self) -> Vec<f32> {
        let mut tops = Vec::with_capacity(self.notifications.len());
        let mut y = 0.0_f32;
        let mut current_group: Option<TimeGroup> = None;
        for notif in &self.notifications {
            let group = TimeGroup::classify(notif.timestamp, self.current_time);
            if current_group != Some(group) {
                current_group = Some(group);
                y += GROUP_HEADER_HEIGHT;
            }
            tops.push(y);
            y += NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING;
        }
        tops
    }

    /// Total height of the list's contents, headers included.
    fn content_height(&self) -> f32 {
        match self.card_tops().last() {
            // The trailing spacing is not content: it would let the list scroll
            // eight pixels past its own last card.
            Some(&last) => last + NOTIF_CARD_HEIGHT,
            None => 0.0,
        }
    }

    /// The index of the card at `local_y`, measured from the start of the list
    /// with the scroll offset already added back.
    ///
    /// `None` for a point in a group header or in the gap between two cards —
    /// which is the honest answer, and the one the old walks gave by falling
    /// out of their loops.
    fn card_at(&self, content_y: f32) -> Option<usize> {
        self.card_tops()
            .iter()
            .position(|&top| content_y >= top && content_y < top + NOTIF_CARD_HEIGHT)
    }

    /// The furthest the list can scroll and still end on its last card.
    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.list_height()).max(0.0)
    }

    /// Pull the offset back inside the list.
    ///
    /// Called after anything that can change either term: a scroll, a resize,
    /// a dismissal, or a new notification arriving.
    fn clamp_scroll(&mut self) {
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll());
    }

    /// Tell the pane how tall the screen is, so the scroll bound can be
    /// computed from it.
    ///
    /// Mouse events carry the height and call this themselves. A shell that
    /// drives the pane from the keyboard alone should call it on resize:
    /// `handle_key_event` has no height to work from, so without it the arrow
    /// keys clamp against `DEFAULT_SCREEN_HEIGHT` rather than the real one.
    pub fn set_screen_height(&mut self, screen_height: f32) {
        self.note_screen_height(screen_height);
    }

    /// Note the viewport height, so the scroll bound can be computed from it.
    fn note_screen_height(&mut self, screen_height: f32) {
        if screen_height.is_finite() && screen_height > 0.0 {
            self.screen_height = screen_height;
            self.clamp_scroll();
        }
    }

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
        cmds.push(RenderCommand::PushTranslate {
            dx: pane_x,
            dy: 0.0,
        });

        // Render sections. The offsets come from the same helpers the hit
        // tests use, rather than from adding up what each renderer reports
        // having drawn -- that is how the two drifted 76 px apart.
        self.render_header(&mut cmds, PANE_PADDING);
        self.render_quick_settings(&mut cmds, Self::qs_start_y());

        // Separator, centred in the gap above the list.
        let y = Self::list_start_y();
        cmds.push(RenderCommand::Line {
            x1: PANE_PADDING,
            y1: y - QS_SEPARATOR_HEIGHT / 2.0,
            x2: PANE_WIDTH - PANE_PADDING,
            y2: y - QS_SEPARATOR_HEIGHT / 2.0,
            color: theme::SURFACE1,
            width: 1.0,
        });

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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
        }

        HEADER_HEIGHT
    }

    /// Draw the quick-settings block and report how tall it turned out.
    ///
    /// Every row is placed by [`Self::qs_toggle_top`] or
    /// [`Self::qs_slider_top`], which are exactly what [`Self::qs_at`]
    /// inverts, so a click lands on the row it is looking at. The returned
    /// height is derived from those same helpers and is checked against
    /// `QUICK_SETTINGS_HEIGHT` — declared independently from the raw
    /// constants — by `the_quick_settings_block_is_as_tall_as_what_it_draws`.
    fn render_quick_settings(&self, cmds: &mut Vec<RenderCommand>, start_y: f32) -> f32 {
        // Section title.
        cmds.push(RenderCommand::Text {
            x: PANE_PADDING,
            y: start_y,
            text: "Quick Settings".to_string(),
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Toggle rows.
        for (idx, qs) in QuickSetting::all().iter().enumerate() {
            let enabled = self.quick_settings.get(*qs);
            self.render_toggle_row(
                cmds,
                PANE_PADDING,
                start_y + Self::qs_toggle_top(idx),
                qs.label(),
                enabled,
            );
        }

        // Volume slider.
        self.render_slider_row(
            cmds,
            PANE_PADDING,
            start_y + Self::qs_slider_top(0),
            "Volume",
            self.quick_settings.volume,
        );

        // Brightness slider.
        self.render_slider_row(
            cmds,
            PANE_PADDING,
            start_y + Self::qs_slider_top(1),
            "Brightness",
            self.quick_settings.brightness,
        );

        Self::qs_slider_top(1) + QS_ROW_HEIGHT
    }

    fn render_toggle_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        label: &str,
        enabled: bool,
    ) {
        // Label.
        cmds.push(RenderCommand::Text {
            x,
            y: y + 8.0,
            text: label.to_string(),
            color: theme::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(180.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Toggle pill.
        let pill_x = PANE_WIDTH - PANE_PADDING - TOGGLE_WIDTH - PANE_PADDING;
        let pill_bg = if enabled {
            theme::BLUE
        } else {
            theme::SURFACE2
        };
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

    fn render_slider_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        label: &str,
        value: u8,
    ) {
        // Label + value.
        cmds.push(RenderCommand::Text {
            x,
            y: y + 8.0,
            text: format!("{label}  {value}%"),
            color: theme::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(180.0),
            overflow: TextOverflow::Ellipsis,
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

    fn render_notifications(
        &self,
        cmds: &mut Vec<RenderCommand>,
        start_y: f32,
        available_height: f32,
    ) {
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
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::PopClip);
            return;
        }

        // `card_tops` is the single definition of where each card sits; the
        // renderer only turns those into screen coordinates.
        let tops = self.card_tops();
        let origin = start_y - self.scroll_offset;
        let mut current_group: Option<TimeGroup> = None;

        for ((idx, notif), &top) in self.notifications.iter().enumerate().zip(&tops) {
            let y = origin + top;
            let group = TimeGroup::classify(notif.timestamp, self.current_time);

            // Render group header if changed.
            if current_group != Some(group) {
                current_group = Some(group);
                let header_y = y - GROUP_HEADER_HEIGHT;
                if header_y + GROUP_HEADER_HEIGHT > start_y - 20.0 {
                    cmds.push(RenderCommand::Text {
                        x: PANE_PADDING,
                        y: header_y + 6.0,
                        text: group.label().to_string(),
                        color: theme::SUBTEXT0,
                        font_size: 11.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(100.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }

            // Skip rendering if above visible area.
            if y + NOTIF_CARD_HEIGHT < start_y {
                continue;
            }
            // Stop rendering if below visible area.
            if y > start_y + available_height {
                break;
            }

            self.render_notification_card(cmds, idx, notif, PANE_PADDING, y);
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
        let card_width = Self::card_width();
        let is_hovered = self.hovered_notif == Some(idx);
        let bg = if is_hovered {
            theme::HOVER_BG
        } else {
            theme::CARD_BG
        };

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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });

        // Body — a one-line preview, elided against the width it is drawn in
        // rather than a character budget picked independently of the box.
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 46.0,
            text: text::elide(
                &notif.body,
                card_width - BODY_INSET,
                "...",
                BODY_FONT_SIZE,
                FontWeightHint::Regular,
            ),
            color: theme::SUBTEXT1,
            font_size: BODY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(card_width - BODY_INSET),
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_app_settings(
        &self,
        cmds: &mut Vec<RenderCommand>,
        start_y: f32,
        available_height: f32,
    ) {
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: start_y,
            width: PANE_WIDTH,
            height: available_height,
        });

        // "Manage notifications" heading.
        cmds.push(RenderCommand::Text {
            x: PANE_PADDING,
            y: start_y,
            text: "Per-App Settings".to_string(),
            color: theme::SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(150.0),
            overflow: TextOverflow::Ellipsis,
        });

        let card_width = Self::card_width();

        for (idx, app) in self.app_settings.iter().enumerate() {
            // The card tops come from `app_card_top`, which is what
            // `app_card_at` inverts -- walking a running total here instead is
            // how the click ended up eight pixels out of step per card.
            let y = start_y + Self::app_card_top(idx);
            if y > start_y + available_height {
                break;
            }

            // App card.
            cmds.push(RenderCommand::FillRect {
                x: PANE_PADDING,
                y,
                width: card_width,
                height: APP_CARD_HEIGHT,
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
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });

            // Enabled toggle.
            let (pill_x, pill_y, pill_w, pill_h) = Self::app_toggle_rect(y);
            let pill_bg = if app.enabled {
                theme::GREEN
            } else {
                theme::SURFACE2
            };
            cmds.push(RenderCommand::FillRect {
                x: pill_x,
                y: pill_y,
                width: pill_w,
                height: pill_h,
                color: pill_bg,
                corner_radii: CornerRadii::all(pill_h / 2.0),
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
                overflow: TextOverflow::Ellipsis,
            });
        }

        // "Manage notifications" link at bottom -- below where the card after
        // the last one would have started, from the same walk the cards use.
        if !self.app_settings.is_empty() {
            cmds.push(RenderCommand::Text {
                x: PANE_PADDING,
                y: start_y + Self::app_card_top(self.app_settings.len()) + 12.0,
                text: "Open full notification settings...".to_string(),
                color: theme::BLUE,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(250.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    // ========================================================================
    // Interaction handling
    // ========================================================================

    fn handle_click(&mut self, rx: f32, ry: f32, screen_height: f32) {
        // Header area.
        if ry < PANE_PADDING + HEADER_HEIGHT {
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
        // Quick settings area.
        let qs_y = Self::qs_start_y();
        if ry >= qs_y && ry < qs_y + QUICK_SETTINGS_HEIGHT {
            self.handle_quick_settings_click(rx, ry - qs_y);
            return;
        }
        let list_top = Self::list_start_y();

        if self.show_settings {
            self.handle_app_settings_click(rx, ry - list_top);
        } else {
            // Notifications area.
            self.handle_notification_click(rx, ry, list_top, screen_height);
        }
    }

    fn handle_quick_settings_click(&mut self, rx: f32, local_y: f32) {
        match Self::qs_at(local_y) {
            Some(QsHit::Toggle(idx)) => {
                // Only the pill on the right is the control; the label is not.
                let pill_x = PANE_WIDTH - PANE_PADDING - TOGGLE_WIDTH - PANE_PADDING;
                if rx < pill_x {
                    return;
                }
                if let Some(&qs) = QuickSetting::all().get(idx) {
                    self.quick_settings.toggle(qs);
                    self.events.push(NotifPaneEvent::QuickSettingToggled(qs));
                }
            }
            Some(hit @ (QsHit::Volume | QsHit::Brightness)) => {
                let track_x = PANE_WIDTH - PANE_PADDING - SLIDER_WIDTH - PANE_PADDING;
                if rx < track_x || rx > track_x + SLIDER_WIDTH {
                    return;
                }
                let frac = ((rx - track_x) / SLIDER_WIDTH).clamp(0.0, 1.0);
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let value = (frac * 100.0) as u8;
                if hit == QsHit::Volume {
                    self.quick_settings.volume = value;
                } else {
                    self.quick_settings.brightness = value;
                }
            }
            None => {}
        }
    }

    fn handle_notification_click(
        &mut self,
        rx: f32,
        ry: f32,
        content_start: f32,
        _screen_height: f32,
    ) {
        let adjusted_y = ry - content_start + self.scroll_offset;
        let Some(idx) = self.card_at(adjusted_y) else {
            return;
        };
        let Some(&top) = self.card_tops().get(idx) else {
            return;
        };
        let Some(id) = self.notifications.get(idx).map(|n| n.id) else {
            return;
        };

        // Check if dismiss button was clicked.
        let card_width = Self::card_width();
        let btn_x = PANE_PADDING + card_width - DISMISS_BTN_SIZE - 8.0;
        if rx >= btn_x
            && rx <= btn_x + DISMISS_BTN_SIZE
            && (adjusted_y - top) < DISMISS_BTN_SIZE + 6.0
        {
            self.dismiss_notification(idx);
            self.events.push(NotifPaneEvent::NotificationDismissed(id));
        } else if let Some(notif) = self.notifications.get_mut(idx) {
            // Click on notification body.
            notif.read = true;
            self.events.push(NotifPaneEvent::NotificationClicked(id));
        }
    }

    fn handle_app_settings_click(&mut self, rx: f32, local_y: f32) {
        let Some(idx) = self.app_card_at(local_y) else {
            return;
        };
        let (pill_x, pill_y, pill_w, pill_h) = Self::app_toggle_rect(Self::app_card_top(idx));
        if rx < pill_x || rx >= pill_x + pill_w || local_y < pill_y || local_y >= pill_y + pill_h {
            // On the card but not on its one control. The name, the priority
            // badge and the status line are labels, not buttons.
            return;
        }
        let Some(app) = self.app_settings.get_mut(idx) else {
            return;
        };
        app.enabled = !app.enabled;
        self.events.push(NotifPaneEvent::SettingChanged {
            app: app.app_name.clone(),
            setting: AppSettingKind::Enabled,
            value: SettingValue::Bool(app.enabled),
        });
    }

    fn update_hover(&mut self, _rx: f32, ry: f32, _screen_height: f32) {
        // Determine which notification card is hovered (simplified).
        let content_start = Self::list_start_y();
        if ry < content_start || self.show_settings {
            self.hovered_notif = None;
            return;
        }
        self.hovered_notif = self.card_at(ry - content_start + self.scroll_offset);
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
            // The list just got shorter, which can put the offset past its new
            // end -- dismissing the last few cards would otherwise leave the
            // pane showing blank space.
            self.clamp_scroll();
            // Adjust hover if needed.
            if let Some(h) = self.hovered_notif
                && h >= self.notifications.len()
            {
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

    // Note: there is deliberately no `truncate_body` helper here any more. It
    // compared `body.len()` (bytes) against a *character* budget of 60 that was
    // unrelated to the `card_width - BODY_INSET` box the text is drawn in, so a
    // body of accented text or CJK was cut far shorter than it needed to be
    // while a run of narrow characters still overflowed. It also underflowed on
    // `max_chars - 3`. The call site now uses `text::elide`, which measures both
    // the body and the ellipsis at the real font size against the real width.
    // See known-issues.md TD-APPS-ESTIMATE-TEXT-WIDTH.
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

    /// A pane showing `n` notifications, all in one time group, on a screen
    /// short enough that the list genuinely overflows.
    fn scrollable_pane(n: usize) -> NotificationPane {
        let mut pane = NotificationPane::new();
        pane.show();
        pane.state = PaneState::Visible;
        for i in 0..n {
            // One timestamp for all of them, so exactly one group header is
            // inserted and the arithmetic below stays legible.
            pane.push_notification(make_notif("App", &format!("N{i}"), 1000));
        }
        pane.current_time = 1000;
        pane.set_screen_height(TEST_SCREEN_H);
        pane
    }

    const TEST_SCREEN_H: f32 = 800.0;

    fn wheel_at(pane: &mut NotificationPane, dy: f32) -> EventResult {
        let event = MouseEvent {
            x: SCREEN_W - 100.0,
            y: 400.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        };
        pane.handle_mouse_event(&event, SCREEN_W, TEST_SCREEN_H)
    }

    const SCREEN_W: f32 = 1920.0;

    fn press_key(pane: &mut NotificationPane, key: Key) {
        pane.handle_key_event(&KeyEvent {
            key,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: None,
        });
    }

    // ========================================================================
    // Layout: the renderer and the hit tests must agree
    // ========================================================================

    /// Clicking where a card is drawn must select *that* card.
    ///
    /// `render_quick_settings` walks its rows and returns what it drew: 276 px.
    /// The two hit tests used `QUICK_SETTINGS_HEIGHT`, which said 200. So every
    /// card was hit-tested 76 px from where it was painted — and a card is 80 px
    /// tall with 8 px between, so a click landed on the card *above* the one
    /// under the pointer, or in the gap and on nothing at all.
    /// Where the renderer actually painted each card's background, read out of
    /// the render commands themselves.
    ///
    /// The point of going through `render` rather than through `card_tops` is
    /// that a test which asks the layout helper where the cards are cannot
    /// catch the renderer and the hit test disagreeing — they would both be
    /// wrong together and the test would still pass. Only the drawn output is
    /// independent evidence.
    fn painted_card_tops(pane: &NotificationPane) -> Vec<f32> {
        let card_width = PANE_WIDTH - 2.0 * PANE_PADDING;
        pane.render(SCREEN_W, TEST_SCREEN_H)
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if *x == PANE_PADDING && *width == card_width && *height == NOTIF_CARD_HEIGHT => {
                    Some(*y)
                }
                _ => None,
            })
            .collect()
    }

    /// What clicking at `y` inside the pane selects, if anything.
    fn click_at(pane: &mut NotificationPane, y: f32) -> Option<u64> {
        pane.events.clear();
        let event = MouseEvent {
            x: SCREEN_W - PANE_WIDTH + PANE_PADDING + 10.0,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        pane.handle_mouse_event(&event, SCREEN_W, TEST_SCREEN_H);
        match pane.events.first() {
            Some(&NotifPaneEvent::NotificationClicked(id)) => Some(id),
            _ => None,
        }
    }

    #[test]
    fn clicking_a_card_where_it_is_drawn_selects_that_card() {
        let mut pane = scrollable_pane(6);
        let ids: Vec<u64> = pane.notifications.iter().map(|n| n.id).collect();
        let painted = painted_card_tops(&pane);
        // Not all six need fit; the renderer stops at the bottom of the pane.
        assert!(painted.len() >= 4, "only {} cards drawn", painted.len());

        for (idx, &top) in painted.iter().enumerate() {
            // The middle of the card, at the y the renderer put it at. The pane
            // is drawn under a translate in x only, so this y is the same y the
            // hit test is handed.
            let y = top + NOTIF_CARD_HEIGHT / 2.0;
            assert_eq!(
                click_at(&mut pane, y),
                Some(ids[idx]),
                "click at y={y} should select card {idx}"
            );
        }
    }

    /// The hit rectangle must be the *painted* rectangle — the whole of it and
    /// nothing beyond it. A test that only probes card centres passes just as
    /// happily when both regions are shifted together; probing each edge is
    /// what makes the two independent.
    ///
    /// Scrolled, so the offset has to be applied identically by both.
    #[test]
    fn a_cards_hit_rectangle_is_exactly_the_rectangle_it_is_painted_in() {
        let mut pane = scrollable_pane(40);
        wheel_at(&mut pane, -2.0);
        assert!(pane.scroll_offset > 0.0);

        let painted = painted_card_tops(&pane);
        assert!(painted.len() >= 4, "only {} cards drawn", painted.len());
        let list_top = NotificationPane::list_start_y();

        for &top in &painted {
            // Only cards drawn wholly inside the list, so the clip does not
            // account for a miss.
            if top < list_top || top + NOTIF_CARD_HEIGHT > TEST_SCREEN_H {
                continue;
            }
            let inside_top = click_at(&mut pane, top + 1.0);
            let middle = click_at(&mut pane, top + NOTIF_CARD_HEIGHT / 2.0);
            let inside_bottom = click_at(&mut pane, top + NOTIF_CARD_HEIGHT - 1.0);
            assert!(middle.is_some(), "nothing painted at y={top} is clickable");
            assert_eq!(
                inside_top, middle,
                "the card's top edge is not where it is drawn"
            );
            assert_eq!(
                inside_bottom, middle,
                "the card's bottom edge is not where it is drawn"
            );
            // The gap below it belongs to no card.
            let gap = top + NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING / 2.0;
            assert_eq!(
                click_at(&mut pane, gap),
                None,
                "the gap under the card at y={top} selected something"
            );
        }
    }

    /// The quick-settings block's height is derived from what it draws, so a
    /// click just below the last slider falls into the list rather than into
    /// 76 px of no-man's-land that both regions thought belonged to the other.
    #[test]
    fn the_quick_settings_block_is_as_tall_as_what_it_draws() {
        let mut pane = NotificationPane::new();
        pane.state = PaneState::Visible;
        let mut cmds = Vec::new();
        let drawn = pane.render_quick_settings(&mut cmds, 0.0);
        assert_eq!(
            drawn, QUICK_SETTINGS_HEIGHT,
            "the constant the hit test uses must be what the renderer draws"
        );
    }

    // ---- Quick settings ----
    //
    // The block used to be laid out twice: `render_quick_settings` walked the
    // caption, the toggles, the gap and the sliders adding heights up, and
    // `handle_quick_settings_click` walked the same list subtracting them
    // back off, with both walks spelling `20.0` and `4.0` by hand. That is
    // the arrangement that once put every notification card 76 px from where
    // it was drawn. These tests read the rows out of the commands the
    // renderer actually pushed and probe those.

    /// Top of every toggle row the renderer drew, recovered from the pill it
    /// paints six pixels down.
    fn painted_toggle_tops(pane: &NotificationPane) -> Vec<f32> {
        let mut cmds = Vec::new();
        pane.render_quick_settings(&mut cmds, 0.0);
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    y, width, height, ..
                } if *width == TOGGLE_WIDTH && *height == TOGGLE_HEIGHT => Some(*y - 6.0),
                _ => None,
            })
            .collect()
    }

    /// Top of the volume and brightness rows, recovered from the slider track
    /// each paints fourteen pixels down. The filled portion and the thumb are
    /// excluded by colour, so a slider at 100% is not counted twice.
    fn painted_slider_tops(pane: &NotificationPane) -> Vec<f32> {
        let mut cmds = Vec::new();
        pane.render_quick_settings(&mut cmds, 0.0);
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    y,
                    width,
                    height,
                    color,
                    ..
                } if *width == SLIDER_WIDTH
                    && *height == SLIDER_HEIGHT
                    && *color == theme::SURFACE2 =>
                {
                    Some(*y - 14.0)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_quick_setting_row_answers_where_it_was_drawn() {
        let pane = NotificationPane::new();
        let toggles = painted_toggle_tops(&pane);
        let sliders = painted_slider_tops(&pane);
        assert_eq!(toggles.len(), QuickSetting::COUNT);
        assert_eq!(sliders.len(), 2, "volume and brightness");

        // Sweep each painted row rather than probing its centre: a hit test
        // that has drifted from the renderer is still correct in the middle.
        for (idx, &top) in toggles.iter().enumerate() {
            for step in 0..8 {
                let probe = top + (step as f32 + 0.5) * QS_ROW_HEIGHT / 8.0;
                assert_eq!(
                    NotificationPane::qs_at(probe),
                    Some(QsHit::Toggle(idx)),
                    "the toggle drawn at {top} does not answer y={probe}"
                );
            }
        }
        for (slot, &top) in sliders.iter().enumerate() {
            let expected = if slot == 0 {
                QsHit::Volume
            } else {
                QsHit::Brightness
            };
            for step in 0..8 {
                let probe = top + (step as f32 + 0.5) * QS_ROW_HEIGHT / 8.0;
                assert_eq!(
                    NotificationPane::qs_at(probe),
                    Some(expected),
                    "the slider drawn at {top} does not answer y={probe}"
                );
            }
        }
    }

    #[test]
    fn the_gap_above_the_volume_slider_does_not_set_the_volume() {
        // It used to. `handle_quick_settings_click` computed
        // `content_y - toggle_area_end - 4.0` and asked only whether the
        // result was below one row height, never whether it was above zero,
        // so the four pixels the renderer leaves blank between the last
        // toggle and the volume slider dragged the volume to wherever the
        // pointer happened to be along the track.
        let toggles_end = *painted_toggle_tops(&NotificationPane::new())
            .last()
            .expect("there is at least one toggle")
            + QS_ROW_HEIGHT;
        let first_slider = painted_slider_tops(&NotificationPane::new())[0];
        assert!(
            first_slider > toggles_end,
            "there is a gap between the toggles and the sliders to test"
        );

        let track_x = PANE_WIDTH - PANE_PADDING - SLIDER_WIDTH - PANE_PADDING;
        let mut probe = toggles_end;
        while probe < first_slider {
            assert_eq!(NotificationPane::qs_at(probe), None, "y={probe} is blank");

            let mut pane = NotificationPane::new();
            pane.state = PaneState::Visible;
            let before = (pane.quick_settings.volume, pane.quick_settings.brightness);
            // Right at the far end of the track, so a hit would be loud.
            pane.handle_quick_settings_click(track_x + SLIDER_WIDTH, probe);
            assert_eq!(
                (pane.quick_settings.volume, pane.quick_settings.brightness),
                before,
                "a click in the blank gap at y={probe} moved a slider"
            );
            probe += 1.0;
        }
    }

    #[test]
    fn the_caption_and_the_space_past_the_last_slider_belong_to_nothing() {
        let mut probe = 0.0;
        while probe < QS_TITLE_HEIGHT {
            assert_eq!(
                NotificationPane::qs_at(probe),
                None,
                "y={probe} is the section caption"
            );
            probe += 1.0;
        }
        let past = NotificationPane::qs_slider_top(1) + QS_ROW_HEIGHT;
        assert_eq!(
            past, QUICK_SETTINGS_HEIGHT,
            "the block ends after brightness"
        );
        assert_eq!(NotificationPane::qs_at(past), None);
        assert_eq!(NotificationPane::qs_at(past + 100.0), None);
        assert_eq!(NotificationPane::qs_at(f32::NAN), None);
        assert_eq!(NotificationPane::qs_at(f32::INFINITY), None);
        assert_eq!(NotificationPane::qs_at(f32::NEG_INFINITY), None);
        assert_eq!(NotificationPane::qs_at(-1.0), None);
    }

    #[test]
    fn a_click_through_the_pane_reaches_the_toggle_that_was_drawn() {
        // The whole path this time: pane-local coordinates, the dispatcher's
        // quick-settings bounds, and the pill's own x range.
        let pill_x = PANE_WIDTH - PANE_PADDING - TOGGLE_WIDTH - PANE_PADDING;
        for (idx, &top) in painted_toggle_tops(&NotificationPane::new())
            .iter()
            .enumerate()
        {
            let mut pane = NotificationPane::new();
            pane.state = PaneState::Visible;
            let qs = QuickSetting::all()[idx];
            let before = pane.quick_settings.get(qs);
            pane.handle_click(
                pill_x + TOGGLE_WIDTH / 2.0,
                NotificationPane::qs_start_y() + top + 1.0,
                TEST_SCREEN_H,
            );
            assert_ne!(
                pane.quick_settings.get(qs),
                before,
                "the toggle drawn at {top} was not the one that flipped"
            );
            assert!(
                pane.events
                    .contains(&NotifPaneEvent::QuickSettingToggled(qs)),
                "flipping {qs:?} must announce itself"
            );
        }
    }

    /// Hover must land on the same card a click would.
    #[test]
    fn hover_and_click_agree_on_which_card_is_under_the_pointer() {
        let mut pane = scrollable_pane(6);
        let tops = pane.card_tops();
        for (idx, &top) in tops.iter().enumerate() {
            let y = NotificationPane::list_start_y() + top + NOTIF_CARD_HEIGHT / 2.0
                - pane.scroll_offset;
            let event = MouseEvent {
                x: SCREEN_W - PANE_WIDTH + PANE_PADDING + 10.0,
                y,
                kind: MouseEventKind::Move,
            };
            pane.handle_mouse_event(&event, SCREEN_W, TEST_SCREEN_H);
            assert_eq!(pane.hovered_notif, Some(idx));
        }
    }

    /// The gap between two cards belongs to neither of them.
    #[test]
    fn a_point_in_the_gap_between_cards_names_no_card() {
        let pane = scrollable_pane(3);
        let tops = pane.card_tops();
        let gap = tops[0] + NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING / 2.0;
        assert_eq!(pane.card_at(gap), None);
    }

    // ========================================================================
    // Layout: the per-app settings list
    // ========================================================================

    /// A pane on the settings page, with `n` distinct apps in the list.
    fn settings_pane(n: usize) -> NotificationPane {
        let mut pane = NotificationPane::new();
        pane.show();
        pane.state = PaneState::Visible;
        for i in 0..n {
            pane.push_notification(make_notif(&format!("App{i}"), "body", 1000));
        }
        pane.current_time = 1000;
        pane.set_screen_height(TEST_SCREEN_H);
        pane.show_settings = true;
        pane
    }

    /// Where the renderer actually painted each app card's background, and each
    /// card's enabled pill, read back out of the render commands.
    ///
    /// Deliberately not `app_card_top` / `app_toggle_rect`: a test that asks
    /// the layout helper where the cards are cannot catch the renderer drawing
    /// somewhere else, because both sides would move together. Only the drawn
    /// output is independent evidence. The cards are picked out by their width
    /// and height, which are spelled here from the pane's own dimensions.
    /// Everything the pane drew from the "Per-App Settings" caption onwards.
    ///
    /// The quick-settings block above the list paints its own pills at the
    /// same size *and the same x* as an app card's, so filtering the whole
    /// command list by shape alone picks up nine rectangles for four cards.
    /// The caption is the boundary between the two.
    fn app_settings_commands(pane: &NotificationPane) -> Vec<RenderCommand> {
        let cmds = pane.render(SCREEN_W, TEST_SCREEN_H);
        let start = cmds
            .iter()
            .position(
                |c| matches!(c, RenderCommand::Text { text, .. } if text == "Per-App Settings"),
            )
            .expect("the settings page draws its caption");
        cmds.get(start..).unwrap_or_default().to_vec()
    }

    fn painted_app_cards(pane: &NotificationPane) -> Vec<f32> {
        let card_width = PANE_WIDTH - 2.0 * PANE_PADDING;
        app_settings_commands(pane)
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if *x == PANE_PADDING && *width == card_width && *height == APP_CARD_HEIGHT => {
                    Some(*y)
                }
                _ => None,
            })
            .collect()
    }

    /// The pills, as `(x, y, width, height)`, in paint order.
    fn painted_app_pills(pane: &NotificationPane) -> Vec<(f32, f32, f32, f32)> {
        app_settings_commands(pane)
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if *width == TOGGLE_WIDTH && *height == TOGGLE_HEIGHT => {
                    Some((*x, *y, *width, *height))
                }
                _ => None,
            })
            .collect()
    }

    /// Click at a pane-local point and report which app, if any, was toggled.
    fn settings_click(pane: &mut NotificationPane, local_x: f32, local_y: f32) -> Option<String> {
        pane.events.clear();
        let event = MouseEvent {
            x: SCREEN_W - PANE_WIDTH + local_x,
            y: local_y,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        pane.handle_mouse_event(&event, SCREEN_W, TEST_SCREEN_H);
        pane.events.iter().find_map(|e| match e {
            NotifPaneEvent::SettingChanged {
                app,
                setting: AppSettingKind::Enabled,
                ..
            } => Some(app.clone()),
            _ => None,
        })
    }

    /// Every pixel of every painted pill toggles the app that pill belongs to.
    ///
    /// Swept rather than probed at the centre: the click used to accept a
    /// 35-pixel band measured from the *card's* top while the pill is painted
    /// ten pixels lower and is twenty-two tall, and a centre probe cannot see
    /// either end of that.
    #[test]
    fn every_pixel_of_a_painted_pill_toggles_its_own_app() {
        let mut pane = settings_pane(5);
        let names: Vec<String> = pane
            .app_settings
            .iter()
            .map(|a| a.app_name.clone())
            .collect();
        let pills = painted_app_pills(&pane);
        assert!(pills.len() >= 3, "only {} pills drawn", pills.len());

        for (idx, &(px, py, pw, ph)) in pills.iter().enumerate() {
            for sx in 0_u8..4 {
                for sy in 0_u8..4 {
                    let x = px + f32::from(sx) * pw / 4.0;
                    let y = py + f32::from(sy) * ph / 4.0;
                    assert_eq!(
                        settings_click(&mut pane, x, y).as_deref(),
                        Some(names[idx].as_str()),
                        "({x}, {y}) is inside the pill painted for {} at \
                         ({px}, {py}, {pw}, {ph})",
                        names[idx]
                    );
                }
            }
            // The pill owns its top and left edges and not its bottom or right.
            assert_eq!(
                settings_click(&mut pane, px + pw - 0.01, py + ph - 0.01).as_deref(),
                Some(names[idx].as_str())
            );
        }
    }

    /// Nothing outside a painted pill toggles anything.
    ///
    /// The four places the old hit test answered and the renderer had drawn no
    /// control: right of the pill (the test was `rx >= enabled_x` with no
    /// right edge, so the strip ran off the side of the pane), above it (the
    /// band started at the card's top, where the app's *name* is drawn), below
    /// it (the band was 35 px and the pill ends at 32), and the eight-pixel
    /// gutter between one card and the next.
    #[test]
    fn nothing_outside_a_painted_pill_toggles_anything() {
        let mut pane = settings_pane(4);
        let pills = painted_app_pills(&pane);
        let &(px, py, pw, ph) = pills.first().expect("a pill is drawn");

        for (label, x, y) in [
            ("right of the pill", px + pw + 1.0, py + ph / 2.0),
            ("the pane's right edge", PANE_WIDTH - 1.0, py + ph / 2.0),
            ("above the pill, on the app name", px + pw / 2.0, py - 1.0),
            ("below the pill", px + pw / 2.0, py + ph + 1.0),
            ("left of the pill", px - 1.0, py + ph / 2.0),
        ] {
            assert_eq!(
                settings_click(&mut pane, x, y),
                None,
                "{label}: ({x}, {y}) is outside the pill painted at \
                 ({px}, {py}, {pw}, {ph})"
            );
        }

        // The gutter between the first and second cards: the renderer paints
        // nothing there, at any x.
        let cards = painted_app_cards(&pane);
        let gutter = cards[0] + APP_CARD_HEIGHT + (APP_CARD_PITCH - APP_CARD_HEIGHT) / 2.0;
        assert!(
            gutter < cards[1],
            "the gutter must be above the next card's top"
        );
        for x in [px - 1.0, px + pw / 2.0, px + pw + 1.0] {
            assert_eq!(
                settings_click(&mut pane, x, gutter),
                None,
                "the gutter at y={gutter} is not part of any card"
            );
        }
    }

    /// The eight pixels between one app card and the next belong to neither.
    ///
    /// Asked of `app_card_at` directly rather than through a click, because a
    /// click in the gutter is refused twice over -- once for being on no card
    /// and again for being nowhere near the pill -- so the second refusal
    /// hides the first. Which card the gutter names is what any future control
    /// lower down a card would inherit.
    #[test]
    fn the_gutter_between_two_app_cards_names_no_card() {
        let pane = settings_pane(4);
        let first = NotificationPane::app_card_top(0);
        assert_eq!(pane.app_card_at(first + APP_CARD_HEIGHT - 0.01), Some(0));
        for offset in 0_u8..8 {
            let y = first + APP_CARD_HEIGHT + f32::from(offset);
            assert_eq!(
                pane.app_card_at(y),
                None,
                "y={y} is in the gutter below card 0"
            );
        }
        assert_eq!(pane.app_card_at(first + APP_CARD_PITCH), Some(1));
    }

    /// The pills are painted exactly where `app_toggle_rect` says they are.
    ///
    /// This is what stops the two sweeps above from passing while renderer and
    /// hit test have drifted *together*: they read the painted rectangles, so
    /// a shared shift moves the probes with the paint and is invisible to
    /// them. Here the painted rectangle is checked against the pane's own
    /// constants, spelled out.
    #[test]
    fn the_pills_are_painted_on_the_cards_the_walk_places() {
        let pane = settings_pane(4);
        let cards = painted_app_cards(&pane);
        let pills = painted_app_pills(&pane);
        assert_eq!(cards.len(), pills.len(), "one pill per painted card");

        let list_top = NotificationPane::list_start_y();
        for (idx, (&card_y, &(px, py, pw, ph))) in cards.iter().zip(&pills).enumerate() {
            let expected_card = list_top + APP_HEADING_HEIGHT + idx as f32 * APP_CARD_PITCH;
            assert_eq!(card_y, expected_card, "card {idx}");
            assert_eq!(px, PANE_WIDTH - 2.0 * PANE_PADDING - TOGGLE_WIDTH);
            assert_eq!(py, card_y + APP_TOGGLE_TOP);
            assert_eq!(pw, TOGGLE_WIDTH);
            assert_eq!(ph, TOGGLE_HEIGHT);
        }
    }

    /// A card the renderer clipped away is not clickable.
    ///
    /// The old hit test had no bottom bound at all: it divided by the pitch and
    /// checked only that the quotient was a valid index, so a click below the
    /// pane -- on the desktop behind it, or on another window -- switched off
    /// an app that was never on screen.
    #[test]
    fn a_card_below_the_panes_bottom_edge_is_not_clickable() {
        let mut pane = settings_pane(20);
        let painted = painted_app_cards(&pane);
        assert!(
            painted.len() < pane.app_settings.len(),
            "the fixture must overflow the pane: {} of {} drawn",
            painted.len(),
            pane.app_settings.len()
        );

        let list_top = NotificationPane::list_start_y();
        for idx in painted.len()..pane.app_settings.len() {
            let card_y = list_top + NotificationPane::app_card_top(idx);
            let (px, py, pw, ph) = NotificationPane::app_toggle_rect(card_y);
            assert_eq!(
                settings_click(&mut pane, px + pw / 2.0, py + ph / 2.0),
                None,
                "app {idx} is below the pane's bottom edge and was not drawn"
            );
        }
        assert!(
            pane.app_settings.iter().all(|a| a.enabled),
            "no app should have been toggled"
        );
    }

    /// A coordinate that is not a number selects no app card.
    ///
    /// `content_y < 0.0` is false for a NaN, so the old guard passed it
    /// through, and `NaN as usize` is 0 rather than a trap: a pointer position
    /// that is nowhere at all named the first app in the list.
    #[test]
    fn an_app_settings_coordinate_that_is_not_a_number_names_no_card() {
        let pane = settings_pane(4);
        for y in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            assert_eq!(pane.app_card_at(y), None, "y={y}");
        }
    }

    /// The caption above the first card is not the first card.
    #[test]
    fn the_per_app_heading_is_not_part_of_the_first_card() {
        let pane = settings_pane(3);
        assert_eq!(pane.app_card_at(0.0), None);
        assert_eq!(pane.app_card_at(APP_HEADING_HEIGHT - 0.01), None);
        assert_eq!(pane.app_card_at(APP_HEADING_HEIGHT), Some(0));
    }

    // ========================================================================
    // Scrolling
    // ========================================================================

    /// `dy` is in notches. This handler multiplied it by a private `30.0`, one
    /// of the dozen different pixels-per-notch constants the notch convention
    /// exists to abolish. A notch is three rows; here a row is a card.
    #[test]
    fn one_wheel_notch_moves_three_cards() {
        let mut pane = scrollable_pane(40);
        // Negative `dy` is towards the user, which moves towards the end.
        wheel_at(&mut pane, -1.0);
        assert_eq!(
            pane.scroll_offset,
            3.0 * (NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING)
        );
        wheel_at(&mut pane, 1.0);
        assert_eq!(pane.scroll_offset, 0.0);
    }

    /// The offset had no upper bound at all: `.max(0.0)` clamps one end only,
    /// so the wheel walked the list off into empty space indefinitely and the
    /// pane went blank with no way back but scrolling all the way up again.
    #[test]
    fn the_wheel_stops_with_the_last_card_on_screen() {
        let mut pane = scrollable_pane(40);
        for _ in 0..200 {
            wheel_at(&mut pane, -1.0);
        }
        assert_eq!(pane.scroll_offset, pane.max_scroll());
        assert!(
            pane.max_scroll() > 0.0,
            "the fixture must actually overflow"
        );
        // The last card's bottom is exactly at the bottom of the viewport.
        let last = *pane.card_tops().last().unwrap();
        assert_eq!(
            last + NOTIF_CARD_HEIGHT - pane.scroll_offset,
            pane.list_height()
        );
    }

    /// A list shorter than the pane cannot scroll at all.
    #[test]
    fn a_list_shorter_than_the_pane_does_not_scroll() {
        let mut pane = scrollable_pane(1);
        assert_eq!(pane.max_scroll(), 0.0);
        wheel_at(&mut pane, -5.0);
        assert_eq!(pane.scroll_offset, 0.0);
    }

    /// The arrow keys clamped at one end only, and Page/Down at neither.
    #[test]
    fn the_arrow_keys_stop_at_both_ends() {
        let mut pane = scrollable_pane(40);
        for _ in 0..500 {
            press_key(&mut pane, Key::Down);
        }
        assert_eq!(pane.scroll_offset, pane.max_scroll());
        for _ in 0..500 {
            press_key(&mut pane, Key::Up);
        }
        assert_eq!(pane.scroll_offset, 0.0);

        for _ in 0..500 {
            press_key(&mut pane, Key::PageDown);
        }
        assert_eq!(pane.scroll_offset, pane.max_scroll());
        for _ in 0..500 {
            press_key(&mut pane, Key::PageUp);
        }
        assert_eq!(pane.scroll_offset, 0.0);
    }

    /// A page is the pane's own height, not a constant that happens to be near
    /// it — otherwise the step is wrong on every screen but one.
    #[test]
    fn a_page_is_the_panes_own_height() {
        let mut pane = scrollable_pane(40);
        press_key(&mut pane, Key::PageDown);
        assert_eq!(pane.scroll_offset, pane.list_height());
    }

    /// Dismissing cards shortens the list, which can strand the offset past the
    /// new end and leave the pane showing blank space.
    #[test]
    fn dismissing_the_last_cards_pulls_the_view_back_inside() {
        let mut pane = scrollable_pane(40);
        for _ in 0..200 {
            wheel_at(&mut pane, -1.0);
        }
        assert!(pane.scroll_offset > 0.0);
        while pane.notifications.len() > 1 {
            pane.dismiss_notification(pane.notifications.len() - 1);
        }
        assert_eq!(pane.max_scroll(), 0.0);
        assert_eq!(pane.scroll_offset, 0.0);
    }

    /// A shrinking screen shrinks the viewport, which raises the scroll bound.
    #[test]
    fn a_shorter_screen_pulls_a_scrolled_list_back_inside_it() {
        let mut pane = scrollable_pane(40);
        for _ in 0..200 {
            wheel_at(&mut pane, -1.0);
        }
        let tall = pane.scroll_offset;
        pane.set_screen_height(TEST_SCREEN_H * 2.0);
        assert!(
            pane.scroll_offset < tall,
            "a taller screen shows more, so the furthest offset is smaller"
        );
        assert_eq!(pane.scroll_offset, pane.max_scroll());
    }

    /// Input events come from outside the process, and an infinity stored in
    /// the offset would blank the pane for the rest of the run.
    #[test]
    fn a_nonfinite_delta_does_not_freeze_the_pane() {
        let mut pane = scrollable_pane(40);
        wheel_at(&mut pane, f32::NAN);
        wheel_at(&mut pane, f32::INFINITY);
        assert_eq!(pane.scroll_offset, 0.0);
        wheel_at(&mut pane, -1.0);
        assert_eq!(
            pane.scroll_offset,
            3.0 * (NOTIF_CARD_HEIGHT + NOTIF_CARD_SPACING)
        );
    }

    /// A nonsense screen height must not become the scroll bound.
    #[test]
    fn a_nonfinite_screen_height_is_ignored() {
        let mut pane = scrollable_pane(40);
        let before = pane.list_height();
        pane.set_screen_height(f32::NAN);
        pane.set_screen_height(0.0);
        pane.set_screen_height(-100.0);
        assert_eq!(pane.list_height(), before);
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
        assert_eq!(
            TimeGroup::classify(three_days_ago, now),
            TimeGroup::ThisWeek
        );
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
        assert_eq!(
            pane.format_relative_time(1_000_000 - 3 * SECS_PER_DAY),
            "3d ago"
        );
    }

    // ========================================================================
    // Body preview elision
    // ========================================================================

    /// Build a visible pane holding one notification with the given body, and
    /// return its render commands.
    fn pane_showing_body(body: &str) -> Vec<RenderCommand> {
        let mut pane = NotificationPane::new();
        let mut notif = make_notif("Mail", "A title", 1_000);
        notif.body = body.to_string();
        pane.push_notification(notif);
        pane.state = PaneState::Visible;
        pane.render(1920.0, 1080.0)
    }

    /// The body previews drawn by `pane_showing_body`: `(text, max_width)`.
    fn body_previews(cmds: &[RenderCommand]) -> Vec<(String, f32)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    font_size,
                    max_width: Some(w),
                    overflow: TextOverflow::Ellipsis,
                    color,
                    ..
                } if (*font_size - BODY_FONT_SIZE).abs() < f32::EPSILON
                    && *color == theme::SUBTEXT1 =>
                {
                    Some((text.clone(), *w))
                }
                _ => None,
            })
            .collect()
    }

    /// The elision must be measured, not counted: whatever characters the body
    /// is made of, what is drawn has to fit the box it is drawn in.
    ///
    /// The old `truncate_body` compared `body.len()` (bytes) against a
    /// character budget of 60 that had nothing to do with `card_width`, so wide
    /// glyphs overflowed the card and multibyte text was cut far too short.
    #[test]
    fn a_body_preview_fits_the_card_it_is_drawn_in() {
        let bodies = [
            "W".repeat(200),
            "i".repeat(200),
            "Ünïcödé wíth áccents repeated many times over and over again".repeat(4),
            "Short.".to_string(),
        ];
        let mut checked = 0;
        for body in &bodies {
            for (drawn, max_width) in body_previews(&pane_showing_body(body)) {
                let measured = text::measure(&drawn, BODY_FONT_SIZE, FontWeightHint::Regular);
                assert!(
                    measured <= max_width + 0.5,
                    "body preview {drawn:?} measures {measured} but its box is {max_width}",
                );
                checked += 1;
            }
        }
        // Guard against the test passing vacuously on an empty command list.
        assert!(
            checked >= 4,
            "expected a preview per body, checked {checked}"
        );
    }

    /// A body that already fits is drawn verbatim — no ellipsis, no truncation.
    #[test]
    fn a_short_body_is_not_elided() {
        let previews = body_previews(&pane_showing_body("Short text"));
        assert_eq!(previews.len(), 1, "expected exactly one body preview");
        assert_eq!(previews[0].0, "Short text");
    }

    /// A body too wide for the card is shortened and marked as shortened, so the
    /// user can tell the preview is partial.
    #[test]
    fn an_overlong_body_is_marked_as_elided() {
        let body = "W".repeat(200);
        let previews = body_previews(&pane_showing_body(&body));
        assert_eq!(previews.len(), 1, "expected exactly one body preview");
        assert!(
            previews[0].0.ends_with("..."),
            "expected an ellipsis, got {:?}",
            previews[0].0,
        );
        assert!(
            previews[0].0.len() < body.len(),
            "expected the body to be shortened"
        );
    }
}
