//! Notification settings panel for the desktop shell.
//!
//! Provides configuration for how notifications are displayed, grouped,
//! and handled — including per-app notification preferences, banner style,
//! sound settings, and notification history retention.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Banner style — how notifications appear on screen
// ============================================================================

/// How notification banners appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerStyle {
    /// Full banner with icon, title, body, and action buttons.
    Full,
    /// Compact single-line with icon and title only.
    Compact,
    /// No visual banner — notification goes directly to the notification center.
    None,
}

impl BannerStyle {
    fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Compact => "Compact",
            Self::None => "None",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Full => "Show icon, title, body, and actions",
            Self::Compact => "Show icon and title only",
            Self::None => "Send to Notification Center silently",
        }
    }
}

// ============================================================================
// Banner position on screen
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerPosition {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    TopCenter,
}

impl BannerPosition {
    fn label(self) -> &'static str {
        match self {
            Self::TopRight => "Top Right",
            Self::TopLeft => "Top Left",
            Self::BottomRight => "Bottom Right",
            Self::BottomLeft => "Bottom Left",
            Self::TopCenter => "Top Center",
        }
    }
}

// ============================================================================
// Notification grouping
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupingMode {
    /// Group by the app that sent the notification.
    ByApp,
    /// Group by notification category/type.
    ByCategory,
    /// No grouping — show each notification individually.
    None,
}

impl GroupingMode {
    fn label(self) -> &'static str {
        match self {
            Self::ByApp => "By Application",
            Self::ByCategory => "By Category",
            Self::None => "No Grouping",
        }
    }
}

// ============================================================================
// Notification priority
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Urgent,
}

impl NotificationPriority {
    fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Urgent => "Urgent",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Low => OVERLAY0,
            Self::Normal => SUBTEXT0,
            Self::High => YELLOW,
            Self::Urgent => RED,
        }
    }
}

// ============================================================================
// Per-app notification preferences
// ============================================================================

/// Notification preferences for a single application.
#[derive(Clone, Debug)]
pub struct AppNotificationPrefs {
    /// Application identifier.
    pub app_id: String,
    /// Display name shown in the settings UI.
    pub display_name: String,
    /// Whether notifications from this app are enabled at all.
    pub enabled: bool,
    /// Banner display style for this app.
    pub banner_style: BannerStyle,
    /// Whether to play sounds for this app's notifications.
    pub play_sound: bool,
    /// Whether to show badge count on the taskbar icon.
    pub show_badge: bool,
    /// Whether this app's notifications show in the lock screen.
    pub show_on_lock_screen: bool,
    /// Minimum priority to show (lower priorities are silently collected).
    pub min_priority: NotificationPriority,
    /// Maximum number of simultaneous banners from this app (1-5).
    pub max_banners: u8,
    /// Number of notifications received (lifetime counter).
    pub total_received: u64,
    /// Number of notifications the user interacted with.
    pub total_interacted: u64,
}

impl AppNotificationPrefs {
    pub fn new(app_id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            display_name: display_name.into(),
            enabled: true,
            banner_style: BannerStyle::Full,
            play_sound: true,
            show_badge: true,
            show_on_lock_screen: true,
            min_priority: NotificationPriority::Low,
            max_banners: 3,
            total_received: 0,
            total_interacted: 0,
        }
    }

    /// Interaction rate as a percentage (0-100).
    pub fn interaction_rate(&self) -> f32 {
        if self.total_received == 0 {
            return 0.0;
        }
        (self.total_interacted as f32 / self.total_received as f32) * 100.0
    }

    /// Whether this app should show a given priority level.
    pub fn should_show(&self, priority: NotificationPriority) -> bool {
        self.enabled && priority >= self.min_priority
    }
}

// ============================================================================
// Global notification settings
// ============================================================================

/// History retention period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryRetention {
    /// Keep notifications for 1 day.
    OneDay,
    /// Keep notifications for 3 days.
    ThreeDays,
    /// Keep notifications for 7 days.
    OneWeek,
    /// Keep notifications for 30 days.
    OneMonth,
    /// Never automatically clear history.
    Forever,
}

impl HistoryRetention {
    fn label(self) -> &'static str {
        match self {
            Self::OneDay => "1 Day",
            Self::ThreeDays => "3 Days",
            Self::OneWeek => "1 Week",
            Self::OneMonth => "1 Month",
            Self::Forever => "Forever",
        }
    }

    /// Retention period in seconds.
    pub fn seconds(self) -> Option<u64> {
        match self {
            Self::OneDay => Some(86400),
            Self::ThreeDays => Some(259200),
            Self::OneWeek => Some(604800),
            Self::OneMonth => Some(2592000),
            Self::Forever => None,
        }
    }
}

/// Auto-dismiss delay for banners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoDismissDelay {
    /// 3 seconds (quick).
    Short,
    /// 5 seconds (default).
    Medium,
    /// 10 seconds.
    Long,
    /// 20 seconds.
    VeryLong,
    /// Never auto-dismiss — user must close manually.
    Never,
}

impl AutoDismissDelay {
    fn label(self) -> &'static str {
        match self {
            Self::Short => "3 seconds",
            Self::Medium => "5 seconds",
            Self::Long => "10 seconds",
            Self::VeryLong => "20 seconds",
            Self::Never => "Never",
        }
    }

    /// Delay in milliseconds, or None for never.
    pub fn millis(self) -> Option<u64> {
        match self {
            Self::Short => Some(3000),
            Self::Medium => Some(5000),
            Self::Long => Some(10000),
            Self::VeryLong => Some(20000),
            Self::Never => None,
        }
    }
}

/// Global notification configuration.
#[derive(Clone, Debug)]
pub struct NotificationConfig {
    /// Master toggle — all notifications on/off.
    pub enabled: bool,
    /// Where banners appear on screen.
    pub banner_position: BannerPosition,
    /// Default banner style for apps without specific settings.
    pub default_banner_style: BannerStyle,
    /// How long banners stay visible before auto-dismissing.
    pub auto_dismiss_delay: AutoDismissDelay,
    /// Maximum simultaneous banners on screen.
    pub max_simultaneous: u8,
    /// How notifications are grouped in the notification center.
    pub grouping: GroupingMode,
    /// Whether to play notification sounds globally.
    pub sounds_enabled: bool,
    /// Global notification sound volume (0-100).
    pub sound_volume: u8,
    /// Whether to show notification count badge on the taskbar notification icon.
    pub show_taskbar_badge: bool,
    /// Whether notifications show on the lock screen.
    pub lock_screen_notifications: bool,
    /// Whether to show notification previews on the lock screen (vs just "You have N notifications").
    pub lock_screen_preview: bool,
    /// How long to keep dismissed notifications in history.
    pub history_retention: HistoryRetention,
    /// Maximum number of notifications stored in history.
    pub max_history: usize,
    /// Whether to show the notification bell animation on new notifications.
    pub animate_bell: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            banner_position: BannerPosition::TopRight,
            default_banner_style: BannerStyle::Full,
            auto_dismiss_delay: AutoDismissDelay::Medium,
            max_simultaneous: 4,
            grouping: GroupingMode::ByApp,
            sounds_enabled: true,
            sound_volume: 80,
            show_taskbar_badge: true,
            lock_screen_notifications: true,
            lock_screen_preview: false,
            history_retention: HistoryRetention::OneWeek,
            max_history: 500,
            animate_bell: true,
        }
    }
}

// ============================================================================
// Notification history entry
// ============================================================================

/// A notification that was received and stored in history.
#[derive(Clone, Debug)]
pub struct NotificationHistoryEntry {
    /// Unique notification ID.
    pub id: u64,
    /// App that sent the notification.
    pub app_id: String,
    /// Notification title.
    pub title: String,
    /// Notification body text.
    pub body: String,
    /// Priority level.
    pub priority: NotificationPriority,
    /// Timestamp when received (seconds since epoch).
    pub received_at: u64,
    /// Whether the user interacted with (clicked/actioned) this notification.
    pub interacted: bool,
    /// Whether this notification has been read/seen.
    pub read: bool,
    /// Whether the notification was auto-dismissed or manually closed.
    pub auto_dismissed: bool,
}

// ============================================================================
// Notification settings manager
// ============================================================================

/// Manages all notification settings and history.
pub struct NotificationSettings {
    /// Global configuration.
    pub config: NotificationConfig,
    /// Per-app notification preferences, keyed by app_id.
    pub app_prefs: Vec<AppNotificationPrefs>,
    /// Notification history (most recent first).
    pub history: Vec<NotificationHistoryEntry>,
    /// ID counter for notifications.
    next_id: u64,
}

impl NotificationSettings {
    pub fn new() -> Self {
        Self {
            config: NotificationConfig::default(),
            app_prefs: Vec::new(),
            history: Vec::new(),
            next_id: 1,
        }
    }

    /// Register an app for notification preferences.
    pub fn register_app(&mut self, app_id: impl Into<String>, display_name: impl Into<String>) {
        let app_id = app_id.into();
        if !self.app_prefs.iter().any(|p| p.app_id == app_id) {
            self.app_prefs.push(AppNotificationPrefs::new(app_id, display_name));
        }
    }

    /// Get mutable preferences for an app.
    pub fn get_app_prefs_mut(&mut self, app_id: &str) -> Option<&mut AppNotificationPrefs> {
        self.app_prefs.iter_mut().find(|p| p.app_id == app_id)
    }

    /// Get preferences for an app (immutable).
    pub fn get_app_prefs(&self, app_id: &str) -> Option<&AppNotificationPrefs> {
        self.app_prefs.iter().find(|p| p.app_id == app_id)
    }

    /// Remove an app's notification preferences.
    pub fn unregister_app(&mut self, app_id: &str) -> bool {
        let before = self.app_prefs.len();
        self.app_prefs.retain(|p| p.app_id != app_id);
        self.app_prefs.len() < before
    }

    /// Check whether a notification from an app at a given priority should be shown.
    pub fn should_show(&self, app_id: &str, priority: NotificationPriority) -> bool {
        if !self.config.enabled {
            return false;
        }
        match self.get_app_prefs(app_id) {
            Some(prefs) => prefs.should_show(priority),
            // Unknown apps use defaults — show everything
            None => true,
        }
    }

    /// Check whether sound should play for a notification.
    pub fn should_play_sound(&self, app_id: &str) -> bool {
        if !self.config.enabled || !self.config.sounds_enabled {
            return false;
        }
        match self.get_app_prefs(app_id) {
            Some(prefs) => prefs.enabled && prefs.play_sound,
            None => true,
        }
    }

    /// Get the effective banner style for an app's notification.
    pub fn effective_banner_style(&self, app_id: &str) -> BannerStyle {
        if !self.config.enabled {
            return BannerStyle::None;
        }
        match self.get_app_prefs(app_id) {
            Some(prefs) if prefs.enabled => prefs.banner_style,
            Some(_) => BannerStyle::None,
            None => self.config.default_banner_style,
        }
    }

    /// Record a notification in history.
    pub fn record_notification(
        &mut self,
        app_id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        priority: NotificationPriority,
        timestamp: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let app_id_str = app_id.into();

        // Update app stats
        if let Some(prefs) = self.get_app_prefs_mut(&app_id_str) {
            prefs.total_received = prefs.total_received.saturating_add(1);
        }

        let entry = NotificationHistoryEntry {
            id,
            app_id: app_id_str,
            title: title.into(),
            body: body.into(),
            priority,
            received_at: timestamp,
            interacted: false,
            read: false,
            auto_dismissed: false,
        };

        self.history.insert(0, entry);

        // Enforce max history
        if self.history.len() > self.config.max_history {
            self.history.truncate(self.config.max_history);
        }

        id
    }

    /// Mark a notification as interacted with.
    pub fn mark_interacted(&mut self, notification_id: u64) -> bool {
        if let Some(entry) = self.history.iter_mut().find(|e| e.id == notification_id) {
            if !entry.interacted {
                entry.interacted = true;
                entry.read = true;
                // Update app interaction stats
                let app_id = entry.app_id.clone();
                if let Some(prefs) = self.get_app_prefs_mut(&app_id) {
                    prefs.total_interacted = prefs.total_interacted.saturating_add(1);
                }
            }
            true
        } else {
            false
        }
    }

    /// Mark a notification as read.
    pub fn mark_read(&mut self, notification_id: u64) -> bool {
        if let Some(entry) = self.history.iter_mut().find(|e| e.id == notification_id) {
            entry.read = true;
            true
        } else {
            false
        }
    }

    /// Mark all notifications as read.
    pub fn mark_all_read(&mut self) {
        for entry in &mut self.history {
            entry.read = true;
        }
    }

    /// Remove a notification from history.
    pub fn remove_notification(&mut self, notification_id: u64) -> bool {
        let before = self.history.len();
        self.history.retain(|e| e.id != notification_id);
        self.history.len() < before
    }

    /// Clear all notification history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Clear history for a specific app.
    pub fn clear_app_history(&mut self, app_id: &str) {
        self.history.retain(|e| e.app_id != app_id);
    }

    /// Expire old notifications based on the retention policy.
    pub fn expire_old(&mut self, current_time: u64) {
        if let Some(retention_secs) = self.config.history_retention.seconds() {
            self.history.retain(|e| {
                current_time.saturating_sub(e.received_at) < retention_secs
            });
        }
    }

    /// Count of unread notifications.
    pub fn unread_count(&self) -> usize {
        self.history.iter().filter(|e| !e.read).count()
    }

    /// Count of unread notifications for a specific app.
    pub fn unread_count_for_app(&self, app_id: &str) -> usize {
        self.history.iter().filter(|e| e.app_id == app_id && !e.read).count()
    }

    /// History entries for a specific app.
    pub fn history_for_app(&self, app_id: &str) -> Vec<&NotificationHistoryEntry> {
        self.history.iter().filter(|e| e.app_id == app_id).collect()
    }

    /// Number of registered apps.
    pub fn app_count(&self) -> usize {
        self.app_prefs.len()
    }

    /// Total notifications in history.
    pub fn history_count(&self) -> usize {
        self.history.len()
    }

    /// Apps sorted by total notifications received (descending).
    pub fn apps_by_activity(&self) -> Vec<&AppNotificationPrefs> {
        let mut apps: Vec<_> = self.app_prefs.iter().collect();
        apps.sort_by(|a, b| b.total_received.cmp(&a.total_received));
        apps
    }

    /// Disable all notifications for an app.
    pub fn mute_app(&mut self, app_id: &str) -> bool {
        if let Some(prefs) = self.get_app_prefs_mut(app_id) {
            prefs.enabled = false;
            true
        } else {
            false
        }
    }

    /// Enable all notifications for an app.
    pub fn unmute_app(&mut self, app_id: &str) -> bool {
        if let Some(prefs) = self.get_app_prefs_mut(app_id) {
            prefs.enabled = true;
            true
        } else {
            false
        }
    }

    /// Reset an app's preferences to defaults.
    pub fn reset_app_prefs(&mut self, app_id: &str) -> bool {
        if let Some(prefs) = self.get_app_prefs_mut(app_id) {
            let name = prefs.display_name.clone();
            let received = prefs.total_received;
            let interacted = prefs.total_interacted;
            *prefs = AppNotificationPrefs::new(app_id.to_string(), name);
            prefs.total_received = received;
            prefs.total_interacted = interacted;
            true
        } else {
            false
        }
    }
}

// ============================================================================
// UI: Notification settings panel
// ============================================================================

/// Active tab in the notification settings UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationSettingsTab {
    /// Global notification settings.
    General,
    /// Per-app notification preferences.
    Apps,
    /// Notification history view.
    History,
}

impl NotificationSettingsTab {
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Apps => "Apps",
            Self::History => "History",
        }
    }
}

/// Notification settings UI state.
pub struct NotificationSettingsUI {
    /// Currently active tab.
    pub active_tab: NotificationSettingsTab,
    /// The underlying settings.
    pub settings: NotificationSettings,
    /// Scroll offset for the history list.
    pub history_scroll: usize,
    /// Currently selected app in the Apps tab (index into app_prefs).
    pub selected_app_index: Option<usize>,
    /// Whether the app detail panel is expanded.
    pub app_detail_expanded: bool,
    /// Search/filter text for the apps list.
    pub app_filter: String,
    /// History filter: show only from this app (None = show all).
    pub history_app_filter: Option<String>,
}

impl NotificationSettingsUI {
    pub fn new() -> Self {
        Self {
            active_tab: NotificationSettingsTab::General,
            settings: NotificationSettings::new(),
            history_scroll: 0,
            selected_app_index: None,
            app_detail_expanded: false,
            app_filter: String::new(),
            history_app_filter: None,
        }
    }

    /// Switch to a different tab.
    pub fn set_tab(&mut self, tab: NotificationSettingsTab) {
        self.active_tab = tab;
        self.history_scroll = 0;
    }

    /// Get filtered apps list based on current search filter.
    fn filtered_apps(&self) -> Vec<(usize, &AppNotificationPrefs)> {
        let filter_lower = self.app_filter.to_lowercase();
        self.settings.app_prefs.iter().enumerate()
            .filter(|(_, p)| {
                if filter_lower.is_empty() {
                    return true;
                }
                p.display_name.to_lowercase().contains(&filter_lower)
                    || p.app_id.to_lowercase().contains(&filter_lower)
            })
            .collect()
    }

    /// Render the notification settings panel.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Notifications".into(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
        });

        // Tab bar
        let tabs = [
            NotificationSettingsTab::General,
            NotificationSettingsTab::Apps,
            NotificationSettingsTab::History,
        ];
        let tab_width = 100.0;
        let tab_y = 60.0;

        for (i, &tab) in tabs.iter().enumerate() {
            let tx = 24.0 + (i as f32) * (tab_width + 8.0);
            let active = tab == self.active_tab;

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tab_width,
                height: 32.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tab_width - 16.0),
            });
        }

        let content_y = tab_y + 48.0;
        let content_height = height - content_y - 16.0;

        match self.active_tab {
            NotificationSettingsTab::General => {
                self.render_general_tab(&mut cmds, 24.0, content_y, width - 48.0, content_height);
            }
            NotificationSettingsTab::Apps => {
                self.render_apps_tab(&mut cmds, 24.0, content_y, width - 48.0, content_height);
            }
            NotificationSettingsTab::History => {
                self.render_history_tab(&mut cmds, 24.0, content_y, width - 48.0, content_height);
            }
        }

        cmds
    }

    fn render_general_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let cfg = &self.settings.config;
        let mut cy = y;

        // Master toggle
        self.render_toggle_row(cmds, x, cy, width, "Notifications", cfg.enabled);
        cy += 44.0;

        // Banner position
        self.render_label_value(cmds, x, cy, width, "Banner Position", cfg.banner_position.label());
        cy += 36.0;

        // Default banner style
        self.render_label_value(cmds, x, cy, width, "Default Style", cfg.default_banner_style.label());
        cy += 36.0;

        // Auto-dismiss delay
        self.render_label_value(cmds, x, cy, width, "Auto-dismiss", cfg.auto_dismiss_delay.label());
        cy += 36.0;

        // Max simultaneous
        self.render_label_value(cmds, x, cy, width, "Max Banners", &cfg.max_simultaneous.to_string());
        cy += 36.0;

        // Grouping
        self.render_label_value(cmds, x, cy, width, "Grouping", cfg.grouping.label());
        cy += 44.0;

        // Sound section
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Sound".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 28.0;

        self.render_toggle_row(cmds, x, cy, width, "Notification Sounds", cfg.sounds_enabled);
        cy += 36.0;

        // Volume bar
        self.render_label_value(cmds, x, cy, width, "Volume", &format!("{}%", cfg.sound_volume));
        let bar_x = x + 160.0;
        let bar_w = width - 220.0;
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: cy + 6.0,
            width: bar_w,
            height: 6.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        let fill_w = bar_w * (cfg.sound_volume as f32 / 100.0);
        if fill_w > 0.5 {
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: cy + 6.0,
                width: fill_w,
                height: 6.0,
                color: BLUE,
                corner_radii: CornerRadii::all(3.0),
            });
        }
        cy += 44.0;

        // Lock screen section
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Lock Screen".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 28.0;

        self.render_toggle_row(cmds, x, cy, width, "Show on Lock Screen", cfg.lock_screen_notifications);
        cy += 36.0;

        self.render_toggle_row(cmds, x, cy, width, "Show Preview", cfg.lock_screen_preview);
        cy += 44.0;

        // History section
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "History".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 28.0;

        self.render_label_value(cmds, x, cy, width, "Retention", cfg.history_retention.label());
        cy += 36.0;

        self.render_label_value(cmds, x, cy, width, "Max Stored", &cfg.max_history.to_string());
        cy += 36.0;

        // Other toggles
        self.render_toggle_row(cmds, x, cy, width, "Taskbar Badge", cfg.show_taskbar_badge);
        cy += 36.0;

        self.render_toggle_row(cmds, x, cy, width, "Animate Bell Icon", cfg.animate_bell);
        let _ = cy;
    }

    fn render_apps_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let mut cy = y;

        // Search field
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 30.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.app_filter.is_empty() {
            "Search apps...".to_string()
        } else {
            self.app_filter.clone()
        };
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: cy + 7.0,
            text: search_text,
            font_size: 13.0,
            color: if self.app_filter.is_empty() { OVERLAY0 } else { TEXT },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 20.0),
        });
        cy += 40.0;

        let filtered = self.filtered_apps();
        if filtered.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 10.0,
                text: "No registered apps".into(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
            });
            return;
        }

        for (orig_idx, prefs) in &filtered {
            let is_selected = self.selected_app_index == Some(*orig_idx);
            let row_h = if is_selected && self.app_detail_expanded { 160.0 } else { 48.0 };

            // Row background
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: row_h,
                color: if is_selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });

            // App name
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 8.0,
                text: prefs.display_name.clone(),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width * 0.5),
            });

            // Enabled/disabled badge
            let badge_text = if prefs.enabled { "ON" } else { "OFF" };
            let badge_color = if prefs.enabled { GREEN } else { RED };
            cmds.push(RenderCommand::FillRect {
                x: width - 50.0,
                y: cy + 8.0,
                width: 36.0,
                height: 18.0,
                color: badge_color,
                corner_radii: CornerRadii::all(9.0),
            });
            cmds.push(RenderCommand::Text {
                x: width - 44.0,
                y: cy + 10.0,
                text: badge_text.into(),
                font_size: 11.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(30.0),
            });

            // Stats line
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 28.0,
                text: format!(
                    "{} received  {}% interaction",
                    prefs.total_received,
                    prefs.interaction_rate() as u32,
                ),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 80.0),
            });

            // Detail panel if expanded
            if is_selected && self.app_detail_expanded {
                let dy = cy + 52.0;
                self.render_label_value(cmds, x + 12.0, dy, width - 24.0, "Banner", prefs.banner_style.label());
                self.render_label_value(cmds, x + 12.0, dy + 24.0, width - 24.0, "Sound", if prefs.play_sound { "On" } else { "Off" });
                self.render_label_value(cmds, x + 12.0, dy + 48.0, width - 24.0, "Badge", if prefs.show_badge { "On" } else { "Off" });
                self.render_label_value(cmds, x + 12.0, dy + 72.0, width - 24.0, "Lock Screen", if prefs.show_on_lock_screen { "On" } else { "Off" });
                self.render_label_value(cmds, x + 12.0, dy + 96.0, width - 24.0, "Min Priority", prefs.min_priority.label());
            }

            cy += row_h + 6.0;
        }
    }

    fn render_history_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        _height: f32,
    ) {
        let mut cy = y;

        // Summary
        let unread = self.settings.unread_count();
        let total = self.settings.history_count();
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!("{} notifications ({} unread)", total, unread),
            font_size: 13.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        cy += 28.0;

        // Filter badge (if active)
        if let Some(ref filter_app) = self.history_app_filter {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width: 200.0,
                height: 22.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(11.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: cy + 4.0,
                text: format!("Filtered: {}", filter_app),
                font_size: 11.0,
                color: BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
            });
            cy += 30.0;
        }

        // History items
        let entries: Vec<_> = if let Some(ref filter_app) = self.history_app_filter {
            self.settings.history.iter().filter(|e| e.app_id == *filter_app).collect()
        } else {
            self.settings.history.iter().collect()
        };

        let visible_start = self.history_scroll;
        let max_visible = 10;
        let visible_entries = entries.iter().skip(visible_start).take(max_visible);

        for entry in visible_entries {
            // Entry background
            let bg = if entry.read { SURFACE0 } else { SURFACE1 };
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 52.0,
                color: bg,
                corner_radii: CornerRadii::all(6.0),
            });

            // Priority indicator
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: cy + 4.0,
                width: 4.0,
                height: 44.0,
                color: entry.priority.color(),
                corner_radii: CornerRadii::all(2.0),
            });

            // Unread dot
            if !entry.read {
                cmds.push(RenderCommand::FillRect {
                    x: x + 14.0,
                    y: cy + 6.0,
                    width: 8.0,
                    height: 8.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // App name
            cmds.push(RenderCommand::Text {
                x: x + 28.0,
                y: cy + 6.0,
                text: entry.app_id.clone(),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.4),
            });

            // Title
            cmds.push(RenderCommand::Text {
                x: x + 28.0,
                y: cy + 22.0,
                text: entry.title.clone(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 48.0),
            });

            // Body preview
            cmds.push(RenderCommand::Text {
                x: x + 28.0,
                y: cy + 38.0,
                text: truncate_text(&entry.body, 60),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 48.0),
            });

            cy += 58.0;
        }

        // Scroll indicator
        if entries.len() > max_visible {
            cmds.push(RenderCommand::Text {
                x: x + width * 0.5 - 40.0,
                y: cy + 4.0,
                text: format!(
                    "{}-{} of {}",
                    visible_start + 1,
                    (visible_start + max_visible).min(entries.len()),
                    entries.len(),
                ),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });
        }
    }

    // ---- Render helpers ----

    fn render_toggle_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        enabled: bool,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 4.0,
            text: label.into(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 80.0),
        });

        // Toggle switch
        let sw_x = x + width - 44.0;
        cmds.push(RenderCommand::FillRect {
            x: sw_x,
            y: y + 2.0,
            width: 40.0,
            height: 22.0,
            color: if enabled { GREEN } else { SURFACE2 },
            corner_radii: CornerRadii::all(11.0),
        });
        let knob_x = if enabled { sw_x + 20.0 } else { sw_x + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 4.0,
            width: 18.0,
            height: 18.0,
            color: TEXT,
            corner_radii: CornerRadii::all(9.0),
        });
    }

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: label.into(),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: value.into(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.45),
        });
    }
}

/// Truncate text to a maximum number of characters with ellipsis.
fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let end = text.char_indices()
            .nth(max_len.saturating_sub(3))
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        format!("{}...", &text[..end])
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- NotificationPriority ----

    #[test]
    fn test_priority_ordering() {
        assert!(NotificationPriority::Low < NotificationPriority::Normal);
        assert!(NotificationPriority::Normal < NotificationPriority::High);
        assert!(NotificationPriority::High < NotificationPriority::Urgent);
    }

    #[test]
    fn test_priority_labels() {
        assert_eq!(NotificationPriority::Low.label(), "Low");
        assert_eq!(NotificationPriority::Urgent.label(), "Urgent");
    }

    #[test]
    fn test_priority_colors() {
        let c = NotificationPriority::Urgent.color();
        assert_eq!(c.r, RED.r);
    }

    // ---- BannerStyle ----

    #[test]
    fn test_banner_style_labels() {
        assert_eq!(BannerStyle::Full.label(), "Full");
        assert_eq!(BannerStyle::Compact.label(), "Compact");
        assert_eq!(BannerStyle::None.label(), "None");
    }

    #[test]
    fn test_banner_style_descriptions() {
        assert!(!BannerStyle::Full.description().is_empty());
        assert!(!BannerStyle::None.description().is_empty());
    }

    // ---- BannerPosition ----

    #[test]
    fn test_banner_position_labels() {
        assert_eq!(BannerPosition::TopRight.label(), "Top Right");
        assert_eq!(BannerPosition::BottomLeft.label(), "Bottom Left");
    }

    // ---- GroupingMode ----

    #[test]
    fn test_grouping_labels() {
        assert_eq!(GroupingMode::ByApp.label(), "By Application");
        assert_eq!(GroupingMode::None.label(), "No Grouping");
    }

    // ---- HistoryRetention ----

    #[test]
    fn test_retention_seconds() {
        assert_eq!(HistoryRetention::OneDay.seconds(), Some(86400));
        assert_eq!(HistoryRetention::OneWeek.seconds(), Some(604800));
        assert_eq!(HistoryRetention::Forever.seconds(), None);
    }

    #[test]
    fn test_retention_labels() {
        assert_eq!(HistoryRetention::ThreeDays.label(), "3 Days");
        assert_eq!(HistoryRetention::OneMonth.label(), "1 Month");
    }

    // ---- AutoDismissDelay ----

    #[test]
    fn test_dismiss_delay_millis() {
        assert_eq!(AutoDismissDelay::Short.millis(), Some(3000));
        assert_eq!(AutoDismissDelay::Medium.millis(), Some(5000));
        assert_eq!(AutoDismissDelay::Never.millis(), None);
    }

    // ---- AppNotificationPrefs ----

    #[test]
    fn test_app_prefs_new() {
        let p = AppNotificationPrefs::new("com.test.app", "Test App");
        assert_eq!(p.app_id, "com.test.app");
        assert_eq!(p.display_name, "Test App");
        assert!(p.enabled);
        assert_eq!(p.banner_style, BannerStyle::Full);
        assert!(p.play_sound);
        assert!(p.show_badge);
    }

    #[test]
    fn test_app_interaction_rate_zero() {
        let p = AppNotificationPrefs::new("test", "Test");
        assert_eq!(p.interaction_rate(), 0.0);
    }

    #[test]
    fn test_app_interaction_rate() {
        let mut p = AppNotificationPrefs::new("test", "Test");
        p.total_received = 10;
        p.total_interacted = 7;
        assert!((p.interaction_rate() - 70.0).abs() < 0.1);
    }

    #[test]
    fn test_app_should_show_enabled() {
        let p = AppNotificationPrefs::new("test", "Test");
        assert!(p.should_show(NotificationPriority::Low));
        assert!(p.should_show(NotificationPriority::Urgent));
    }

    #[test]
    fn test_app_should_show_disabled() {
        let mut p = AppNotificationPrefs::new("test", "Test");
        p.enabled = false;
        assert!(!p.should_show(NotificationPriority::Urgent));
    }

    #[test]
    fn test_app_should_show_min_priority() {
        let mut p = AppNotificationPrefs::new("test", "Test");
        p.min_priority = NotificationPriority::High;
        assert!(!p.should_show(NotificationPriority::Low));
        assert!(!p.should_show(NotificationPriority::Normal));
        assert!(p.should_show(NotificationPriority::High));
        assert!(p.should_show(NotificationPriority::Urgent));
    }

    // ---- NotificationConfig ----

    #[test]
    fn test_config_defaults() {
        let cfg = NotificationConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.banner_position, BannerPosition::TopRight);
        assert_eq!(cfg.max_simultaneous, 4);
        assert_eq!(cfg.sound_volume, 80);
        assert!(cfg.show_taskbar_badge);
    }

    // ---- NotificationSettings ----

    #[test]
    fn test_settings_new() {
        let s = NotificationSettings::new();
        assert_eq!(s.app_count(), 0);
        assert_eq!(s.history_count(), 0);
    }

    #[test]
    fn test_register_app() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App One");
        assert_eq!(s.app_count(), 1);
        assert!(s.get_app_prefs("app1").is_some());
    }

    #[test]
    fn test_register_app_duplicate() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App One");
        s.register_app("app1", "App One Again");
        assert_eq!(s.app_count(), 1);
    }

    #[test]
    fn test_unregister_app() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App One");
        assert!(s.unregister_app("app1"));
        assert_eq!(s.app_count(), 0);
        assert!(!s.unregister_app("app1"));
    }

    #[test]
    fn test_should_show_global_disabled() {
        let mut s = NotificationSettings::new();
        s.config.enabled = false;
        assert!(!s.should_show("any_app", NotificationPriority::Urgent));
    }

    #[test]
    fn test_should_show_unknown_app() {
        let s = NotificationSettings::new();
        assert!(s.should_show("unknown", NotificationPriority::Low));
    }

    #[test]
    fn test_should_show_registered_app() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App");
        assert!(s.should_show("app1", NotificationPriority::Normal));

        s.mute_app("app1");
        assert!(!s.should_show("app1", NotificationPriority::Normal));
    }

    #[test]
    fn test_should_play_sound() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App");
        assert!(s.should_play_sound("app1"));

        s.config.sounds_enabled = false;
        assert!(!s.should_play_sound("app1"));
    }

    #[test]
    fn test_effective_banner_style() {
        let mut s = NotificationSettings::new();
        assert_eq!(s.effective_banner_style("unknown"), BannerStyle::Full);

        s.register_app("app1", "App");
        assert_eq!(s.effective_banner_style("app1"), BannerStyle::Full);

        if let Some(p) = s.get_app_prefs_mut("app1") {
            p.banner_style = BannerStyle::Compact;
        }
        assert_eq!(s.effective_banner_style("app1"), BannerStyle::Compact);

        s.config.enabled = false;
        assert_eq!(s.effective_banner_style("app1"), BannerStyle::None);
    }

    #[test]
    fn test_record_notification() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App");
        let id = s.record_notification("app1", "Title", "Body", NotificationPriority::Normal, 1000);
        assert_eq!(id, 1);
        assert_eq!(s.history_count(), 1);
        assert_eq!(s.get_app_prefs("app1").unwrap().total_received, 1);
    }

    #[test]
    fn test_record_notification_max_history() {
        let mut s = NotificationSettings::new();
        s.config.max_history = 3;
        for i in 0..5 {
            s.record_notification("app", &format!("Title {}", i), "Body", NotificationPriority::Normal, i);
        }
        assert_eq!(s.history_count(), 3);
        // Most recent should be first
        assert_eq!(s.history[0].title, "Title 4");
    }

    #[test]
    fn test_mark_interacted() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App");
        let id = s.record_notification("app1", "Title", "Body", NotificationPriority::Normal, 1000);
        assert!(s.mark_interacted(id));
        assert!(s.history[0].interacted);
        assert!(s.history[0].read);
        assert_eq!(s.get_app_prefs("app1").unwrap().total_interacted, 1);
    }

    #[test]
    fn test_mark_interacted_nonexistent() {
        let mut s = NotificationSettings::new();
        assert!(!s.mark_interacted(999));
    }

    #[test]
    fn test_mark_read() {
        let mut s = NotificationSettings::new();
        let id = s.record_notification("app", "Title", "Body", NotificationPriority::Normal, 1000);
        assert!(s.mark_read(id));
        assert!(s.history[0].read);
    }

    #[test]
    fn test_mark_all_read() {
        let mut s = NotificationSettings::new();
        s.record_notification("a", "T1", "B", NotificationPriority::Normal, 1);
        s.record_notification("b", "T2", "B", NotificationPriority::Normal, 2);
        assert_eq!(s.unread_count(), 2);
        s.mark_all_read();
        assert_eq!(s.unread_count(), 0);
    }

    #[test]
    fn test_remove_notification() {
        let mut s = NotificationSettings::new();
        let id = s.record_notification("app", "T", "B", NotificationPriority::Normal, 1);
        assert!(s.remove_notification(id));
        assert_eq!(s.history_count(), 0);
        assert!(!s.remove_notification(id));
    }

    #[test]
    fn test_clear_history() {
        let mut s = NotificationSettings::new();
        s.record_notification("a", "T", "B", NotificationPriority::Normal, 1);
        s.record_notification("b", "T", "B", NotificationPriority::Normal, 2);
        s.clear_history();
        assert_eq!(s.history_count(), 0);
    }

    #[test]
    fn test_clear_app_history() {
        let mut s = NotificationSettings::new();
        s.record_notification("a", "T1", "B", NotificationPriority::Normal, 1);
        s.record_notification("b", "T2", "B", NotificationPriority::Normal, 2);
        s.record_notification("a", "T3", "B", NotificationPriority::Normal, 3);
        s.clear_app_history("a");
        assert_eq!(s.history_count(), 1);
        assert_eq!(s.history[0].app_id, "b");
    }

    #[test]
    fn test_expire_old() {
        let mut s = NotificationSettings::new();
        s.config.history_retention = HistoryRetention::OneDay;
        s.record_notification("a", "Old", "B", NotificationPriority::Normal, 1000);
        s.record_notification("b", "New", "B", NotificationPriority::Normal, 100000);
        s.expire_old(100000);
        assert_eq!(s.history_count(), 1);
        assert_eq!(s.history[0].title, "New");
    }

    #[test]
    fn test_expire_old_forever() {
        let mut s = NotificationSettings::new();
        s.config.history_retention = HistoryRetention::Forever;
        s.record_notification("a", "Old", "B", NotificationPriority::Normal, 1);
        s.expire_old(999999999);
        assert_eq!(s.history_count(), 1);
    }

    #[test]
    fn test_unread_count() {
        let mut s = NotificationSettings::new();
        s.record_notification("a", "T1", "B", NotificationPriority::Normal, 1);
        s.record_notification("a", "T2", "B", NotificationPriority::Normal, 2);
        assert_eq!(s.unread_count(), 2);
        s.mark_read(1);
        assert_eq!(s.unread_count(), 1);
    }

    #[test]
    fn test_unread_count_for_app() {
        let mut s = NotificationSettings::new();
        s.record_notification("a", "T1", "B", NotificationPriority::Normal, 1);
        s.record_notification("b", "T2", "B", NotificationPriority::Normal, 2);
        s.record_notification("a", "T3", "B", NotificationPriority::Normal, 3);
        assert_eq!(s.unread_count_for_app("a"), 2);
        assert_eq!(s.unread_count_for_app("b"), 1);
    }

    #[test]
    fn test_history_for_app() {
        let mut s = NotificationSettings::new();
        s.record_notification("a", "T1", "B", NotificationPriority::Normal, 1);
        s.record_notification("b", "T2", "B", NotificationPriority::Normal, 2);
        s.record_notification("a", "T3", "B", NotificationPriority::Normal, 3);
        let h = s.history_for_app("a");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn test_apps_by_activity() {
        let mut s = NotificationSettings::new();
        s.register_app("a", "Alpha");
        s.register_app("b", "Beta");
        s.record_notification("b", "T", "B", NotificationPriority::Normal, 1);
        s.record_notification("b", "T", "B", NotificationPriority::Normal, 2);
        s.record_notification("a", "T", "B", NotificationPriority::Normal, 3);
        let sorted = s.apps_by_activity();
        assert_eq!(sorted[0].app_id, "b");
        assert_eq!(sorted[1].app_id, "a");
    }

    #[test]
    fn test_mute_unmute_app() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App");
        assert!(s.mute_app("app1"));
        assert!(!s.get_app_prefs("app1").unwrap().enabled);
        assert!(s.unmute_app("app1"));
        assert!(s.get_app_prefs("app1").unwrap().enabled);
    }

    #[test]
    fn test_mute_nonexistent() {
        let mut s = NotificationSettings::new();
        assert!(!s.mute_app("nope"));
    }

    #[test]
    fn test_reset_app_prefs() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App");
        s.record_notification("app1", "T", "B", NotificationPriority::Normal, 1);
        if let Some(p) = s.get_app_prefs_mut("app1") {
            p.banner_style = BannerStyle::None;
            p.play_sound = false;
        }
        assert!(s.reset_app_prefs("app1"));
        let p = s.get_app_prefs("app1").unwrap();
        assert_eq!(p.banner_style, BannerStyle::Full);
        assert!(p.play_sound);
        // Stats should be preserved
        assert_eq!(p.total_received, 1);
    }

    // ---- NotificationSettingsUI ----

    #[test]
    fn test_ui_new() {
        let ui = NotificationSettingsUI::new();
        assert_eq!(ui.active_tab, NotificationSettingsTab::General);
        assert!(ui.selected_app_index.is_none());
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = NotificationSettingsUI::new();
        ui.history_scroll = 5;
        ui.set_tab(NotificationSettingsTab::History);
        assert_eq!(ui.active_tab, NotificationSettingsTab::History);
        assert_eq!(ui.history_scroll, 0);
    }

    #[test]
    fn test_ui_filtered_apps_empty() {
        let ui = NotificationSettingsUI::new();
        assert!(ui.filtered_apps().is_empty());
    }

    #[test]
    fn test_ui_filtered_apps_with_filter() {
        let mut ui = NotificationSettingsUI::new();
        ui.settings.register_app("mail", "Mail Client");
        ui.settings.register_app("browser", "Web Browser");
        ui.app_filter = "mail".to_string();
        let filtered = ui.filtered_apps();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].1.app_id, "mail");
    }

    #[test]
    fn test_ui_render_general_produces_commands() {
        let ui = NotificationSettingsUI::new();
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_apps_produces_commands() {
        let mut ui = NotificationSettingsUI::new();
        ui.set_tab(NotificationSettingsTab::Apps);
        ui.settings.register_app("app1", "Test App");
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_history_produces_commands() {
        let mut ui = NotificationSettingsUI::new();
        ui.set_tab(NotificationSettingsTab::History);
        ui.settings.record_notification("app1", "Hello", "World", NotificationPriority::Normal, 1000);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_app_detail_expanded() {
        let mut ui = NotificationSettingsUI::new();
        ui.set_tab(NotificationSettingsTab::Apps);
        ui.settings.register_app("app1", "Test App");
        ui.selected_app_index = Some(0);
        ui.app_detail_expanded = true;
        let cmds = ui.render(600.0, 800.0);
        // Should have more commands than collapsed view
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_ui_render_history_with_filter() {
        let mut ui = NotificationSettingsUI::new();
        ui.set_tab(NotificationSettingsTab::History);
        ui.settings.record_notification("a", "T1", "B", NotificationPriority::Normal, 1);
        ui.settings.record_notification("b", "T2", "B", NotificationPriority::Normal, 2);
        ui.history_app_filter = Some("a".to_string());
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    // ---- truncate_text ----

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate_text("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate_text("this is a very long notification body text", 20);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 23); // 20-3+3
    }

    // ---- Tab labels ----

    #[test]
    fn test_tab_labels() {
        assert_eq!(NotificationSettingsTab::General.label(), "General");
        assert_eq!(NotificationSettingsTab::Apps.label(), "Apps");
        assert_eq!(NotificationSettingsTab::History.label(), "History");
    }

    // ---- Interacted-already ----

    #[test]
    fn test_mark_interacted_idempotent() {
        let mut s = NotificationSettings::new();
        s.register_app("app1", "App");
        let id = s.record_notification("app1", "T", "B", NotificationPriority::Normal, 1);
        s.mark_interacted(id);
        s.mark_interacted(id);
        // Should only count once
        assert_eq!(s.get_app_prefs("app1").unwrap().total_interacted, 1);
    }

    #[test]
    fn test_multiple_apps_register() {
        let mut s = NotificationSettings::new();
        s.register_app("a", "Alpha");
        s.register_app("b", "Beta");
        s.register_app("c", "Charlie");
        assert_eq!(s.app_count(), 3);
    }

    #[test]
    fn test_notification_id_uniqueness() {
        let mut s = NotificationSettings::new();
        let id1 = s.record_notification("a", "T", "B", NotificationPriority::Normal, 1);
        let id2 = s.record_notification("a", "T", "B", NotificationPriority::Normal, 2);
        let id3 = s.record_notification("a", "T", "B", NotificationPriority::Normal, 3);
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
    }
}
