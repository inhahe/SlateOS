//! Focus Assist / Do Not Disturb mode.
//!
//! Suppresses or prioritizes notifications based on rules. The user can
//! configure automatic activation (during presentations, games, specific
//! hours) and set per-app priority levels so critical alerts still come
//! through.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
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
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Types
// ============================================================================

/// Focus assist modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusMode {
    /// All notifications come through normally.
    Off,
    /// Only priority notifications (from priority apps/contacts).
    PriorityOnly,
    /// Only alarms. All other notifications are silenced.
    AlarmsOnly,
    /// Complete silence — no notifications at all.
    TotalSilence,
}

impl FocusMode {
    pub fn label(&self) -> &str {
        match self {
            Self::Off => "Off",
            Self::PriorityOnly => "Priority Only",
            Self::AlarmsOnly => "Alarms Only",
            Self::TotalSilence => "Total Silence",
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            Self::Off => "\u{1F514}",         // bell
            Self::PriorityOnly => "\u{1F515}", // bell with slash
            Self::AlarmsOnly => "\u{23F0}",    // alarm clock
            Self::TotalSilence => "\u{1F6AB}", // prohibited
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Off => "All notifications are shown",
            Self::PriorityOnly => "Only priority app notifications are shown",
            Self::AlarmsOnly => "Only alarms come through",
            Self::TotalSilence => "No notifications at all",
        }
    }
}

/// Notification priority level for an app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotifPriority {
    /// Silenced — never shown in focus mode.
    Silent,
    /// Normal — follows focus mode rules.
    Normal,
    /// Priority — shown in PriorityOnly mode.
    Priority,
    /// Critical — always shown (alarms, security alerts).
    Critical,
}

impl NotifPriority {
    pub fn label(&self) -> &str {
        match self {
            Self::Silent => "Silent",
            Self::Normal => "Normal",
            Self::Priority => "Priority",
            Self::Critical => "Critical",
        }
    }
}

/// An automatic rule that activates focus assist.
#[derive(Clone, Debug, PartialEq)]
pub enum AutoRule {
    /// Activate during specific hours (start_hour, start_min, end_hour, end_min).
    Schedule {
        start_hour: u8,
        start_min: u8,
        end_hour: u8,
        end_min: u8,
        /// Which days (0=Sun..6=Sat).
        days: Vec<u8>,
        mode: FocusMode,
    },
    /// Activate when a fullscreen app is running.
    Fullscreen { mode: FocusMode },
    /// Activate when a presentation/screen share is active.
    Presentation { mode: FocusMode },
    /// Activate when a game is running (detected by capability or heuristic).
    Gaming { mode: FocusMode },
    /// Activate when on battery power below a threshold.
    BatteryLow { threshold_percent: u8, mode: FocusMode },
}

impl AutoRule {
    pub fn label(&self) -> String {
        match self {
            Self::Schedule { start_hour, start_min, end_hour, end_min, .. } => {
                format!("Schedule {start_hour:02}:{start_min:02}–{end_hour:02}:{end_min:02}")
            }
            Self::Fullscreen { .. } => "Fullscreen app".to_string(),
            Self::Presentation { .. } => "Presenting".to_string(),
            Self::Gaming { .. } => "Gaming".to_string(),
            Self::BatteryLow { threshold_percent, .. } => {
                format!("Battery below {threshold_percent}%")
            }
        }
    }

    pub fn mode(&self) -> FocusMode {
        match self {
            Self::Schedule { mode, .. }
            | Self::Fullscreen { mode }
            | Self::Presentation { mode }
            | Self::Gaming { mode }
            | Self::BatteryLow { mode, .. } => *mode,
        }
    }

    /// Check if a schedule rule is currently active.
    pub fn is_schedule_active(&self, hour: u8, minute: u8, day_of_week: u8) -> bool {
        if let Self::Schedule {
            start_hour,
            start_min,
            end_hour,
            end_min,
            days,
            ..
        } = self
        {
            if !days.is_empty() && !days.contains(&day_of_week) {
                return false;
            }
            let now = hour as u16 * 60 + minute as u16;
            let start = *start_hour as u16 * 60 + *start_min as u16;
            let end = *end_hour as u16 * 60 + *end_min as u16;

            if start <= end {
                now >= start && now < end
            } else {
                // Overnight span (e.g., 22:00–06:00).
                now >= start || now < end
            }
        } else {
            false
        }
    }
}

/// Per-app notification override.
#[derive(Clone, Debug)]
pub struct AppNotifOverride {
    /// Application identifier.
    pub app_id: String,
    /// Display name.
    pub app_name: String,
    /// Priority level override.
    pub priority: NotifPriority,
    /// Whether to show banners for this app.
    pub show_banner: bool,
    /// Whether to play sound for this app.
    pub play_sound: bool,
}

impl AppNotifOverride {
    pub fn new(app_id: &str, app_name: &str) -> Self {
        Self {
            app_id: app_id.to_string(),
            app_name: app_name.to_string(),
            priority: NotifPriority::Normal,
            show_banner: true,
            play_sound: true,
        }
    }

    pub fn with_priority(mut self, priority: NotifPriority) -> Self {
        self.priority = priority;
        self
    }
}

// ============================================================================
// Focus Assist Manager
// ============================================================================

/// Manages focus assist state and rules.
pub struct FocusAssistManager {
    /// Current manual mode.
    pub manual_mode: FocusMode,
    /// Whether manual override is active (overrides auto rules).
    pub manual_override: bool,
    /// Automatic rules.
    pub auto_rules: Vec<AutoRule>,
    /// Per-app overrides.
    pub app_overrides: Vec<AppNotifOverride>,
    /// Whether auto rules are enabled.
    pub auto_rules_enabled: bool,
    /// Show summary when focus assist deactivates.
    pub show_summary: bool,
    /// Number of suppressed notifications (for summary).
    pub suppressed_count: u32,
    /// Whether currently in an auto-activated mode.
    auto_active: bool,
    /// Which auto mode is currently active.
    auto_mode: FocusMode,
    /// System state flags (set by external signals).
    pub fullscreen_active: bool,
    pub presenting: bool,
    pub gaming: bool,
    pub battery_percent: u8,
    pub on_battery: bool,
}

impl FocusAssistManager {
    pub fn new() -> Self {
        Self {
            manual_mode: FocusMode::Off,
            manual_override: false,
            auto_rules: Vec::new(),
            app_overrides: Vec::new(),
            auto_rules_enabled: true,
            show_summary: true,
            suppressed_count: 0,
            auto_active: false,
            auto_mode: FocusMode::Off,
            fullscreen_active: false,
            presenting: false,
            gaming: false,
            battery_percent: 100,
            on_battery: false,
        }
    }

    /// Set the manual focus mode.
    pub fn set_mode(&mut self, mode: FocusMode) {
        self.manual_mode = mode;
        self.manual_override = mode != FocusMode::Off;
        if mode == FocusMode::Off {
            self.suppressed_count = 0;
        }
    }

    /// Cycle through modes (for quick toggle).
    pub fn cycle_mode(&mut self) {
        self.manual_mode = match self.manual_mode {
            FocusMode::Off => FocusMode::PriorityOnly,
            FocusMode::PriorityOnly => FocusMode::AlarmsOnly,
            FocusMode::AlarmsOnly => FocusMode::TotalSilence,
            FocusMode::TotalSilence => FocusMode::Off,
        };
        self.manual_override = self.manual_mode != FocusMode::Off;
    }

    /// Get the effective mode (manual override > auto rules > off).
    pub fn effective_mode(&self) -> FocusMode {
        if self.manual_override {
            self.manual_mode
        } else if self.auto_active {
            self.auto_mode
        } else {
            FocusMode::Off
        }
    }

    /// Whether focus assist is currently active (any mode).
    pub fn is_active(&self) -> bool {
        self.effective_mode() != FocusMode::Off
    }

    /// Add an auto rule.
    pub fn add_auto_rule(&mut self, rule: AutoRule) {
        self.auto_rules.push(rule);
    }

    /// Remove an auto rule by index.
    pub fn remove_auto_rule(&mut self, index: usize) -> bool {
        if index < self.auto_rules.len() {
            self.auto_rules.remove(index);
            true
        } else {
            false
        }
    }

    /// Set an app override.
    pub fn set_app_override(&mut self, override_entry: AppNotifOverride) {
        // Replace existing or add new.
        if let Some(existing) = self
            .app_overrides
            .iter_mut()
            .find(|o| o.app_id == override_entry.app_id)
        {
            *existing = override_entry;
        } else {
            self.app_overrides.push(override_entry);
        }
    }

    /// Get the notification priority for an app.
    pub fn app_priority(&self, app_id: &str) -> NotifPriority {
        self.app_overrides
            .iter()
            .find(|o| o.app_id == app_id)
            .map(|o| o.priority)
            .unwrap_or(NotifPriority::Normal)
    }

    /// Should a notification from this app be shown right now?
    pub fn should_show_notification(&self, app_id: &str) -> bool {
        let mode = self.effective_mode();
        let priority = self.app_priority(app_id);

        match mode {
            FocusMode::Off => true,
            FocusMode::PriorityOnly => priority >= NotifPriority::Priority,
            FocusMode::AlarmsOnly => priority >= NotifPriority::Critical,
            FocusMode::TotalSilence => false,
        }
    }

    /// Record a suppressed notification.
    pub fn record_suppressed(&mut self) {
        self.suppressed_count += 1;
    }

    /// Evaluate auto rules given current time and system state.
    pub fn evaluate_auto_rules(&mut self, hour: u8, minute: u8, day_of_week: u8) {
        if !self.auto_rules_enabled || self.manual_override {
            self.auto_active = false;
            return;
        }

        // Find the highest-priority triggered rule.
        let mut triggered_mode: Option<FocusMode> = None;

        for rule in &self.auto_rules {
            let active = match rule {
                AutoRule::Schedule { .. } => rule.is_schedule_active(hour, minute, day_of_week),
                AutoRule::Fullscreen { .. } => self.fullscreen_active,
                AutoRule::Presentation { .. } => self.presenting,
                AutoRule::Gaming { .. } => self.gaming,
                AutoRule::BatteryLow {
                    threshold_percent, ..
                } => self.on_battery && self.battery_percent < *threshold_percent,
            };

            if active {
                let rule_mode = rule.mode();
                // Take the most restrictive mode.
                triggered_mode = Some(match triggered_mode {
                    None => rule_mode,
                    Some(current) => most_restrictive(current, rule_mode),
                });
            }
        }

        if let Some(mode) = triggered_mode {
            if !self.auto_active {
                self.suppressed_count = 0; // reset on activation
            }
            self.auto_active = true;
            self.auto_mode = mode;
        } else {
            self.auto_active = false;
            self.auto_mode = FocusMode::Off;
        }
    }

    /// Render the tray indicator for focus assist.
    pub fn render_tray_indicator(&self, x: f32, y: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let mode = self.effective_mode();

        if mode == FocusMode::Off {
            return commands;
        }

        // Background pill.
        let pill_w = 24.0;
        let pill_h = 16.0;
        let color = match mode {
            FocusMode::Off => return commands,
            FocusMode::PriorityOnly => BLUE,
            FocusMode::AlarmsOnly => YELLOW,
            FocusMode::TotalSilence => RED,
        };

        commands.push(RenderCommand::FillRect {
            x,
            y,
            width: pill_w,
            height: pill_h,
            color,
            corner_radii: CornerRadii::all(pill_h / 2.0),
        });

        // Moon/bell icon.
        commands.push(RenderCommand::Text {
            x: x + 5.0,
            y: y + 1.0,
            text: mode.icon().to_string(),
            font_size: 10.0,
            color: BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        commands
    }

    /// Render the focus assist settings panel.
    pub fn render_settings(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let padding = 12.0;
        let mut cy = y + padding;

        // Title.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Focus Assist".to_string(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 32.0;

        // Current mode.
        let mode = self.effective_mode();
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: format!("Current: {}", mode.label()),
            font_size: 14.0,
            color: if mode == FocusMode::Off { SUBTEXT0 } else { BLUE },
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 24.0;

        if self.is_active() && self.suppressed_count > 0 {
            commands.push(RenderCommand::Text {
                x: x + padding,
                y: cy,
                text: format!("{} notifications suppressed", self.suppressed_count),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cy += 20.0;
        }
        cy += 8.0;

        // Mode selector.
        let modes = [
            FocusMode::Off,
            FocusMode::PriorityOnly,
            FocusMode::AlarmsOnly,
            FocusMode::TotalSilence,
        ];
        for m in &modes {
            let selected = self.manual_mode == *m;
            let bg = if selected { SURFACE0 } else { MANTLE };
            commands.push(RenderCommand::FillRect {
                x: x + padding,
                y: cy,
                width: width - padding * 2.0,
                height: 40.0,
                color: bg,
                corner_radii: CornerRadii::all(8.0),
            });
            commands.push(RenderCommand::Text {
                x: x + padding + 12.0,
                y: cy + 4.0,
                text: m.icon().to_string(),
                font_size: 16.0,
                color: if selected { BLUE } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            commands.push(RenderCommand::Text {
                x: x + padding + 36.0,
                y: cy + 4.0,
                text: m.label().to_string(),
                font_size: 13.0,
                color: if selected { TEXT } else { SUBTEXT0 },
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            commands.push(RenderCommand::Text {
                x: x + padding + 36.0,
                y: cy + 22.0,
                text: m.description().to_string(),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - padding * 2.0 - 48.0),
            });
            cy += 46.0;
        }
        cy += 12.0;

        // Auto rules section.
        commands.push(RenderCommand::Text {
            x: x + padding,
            y: cy,
            text: "Automatic Rules".to_string(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cy += 24.0;

        if self.auto_rules.is_empty() {
            commands.push(RenderCommand::Text {
                x: x + padding + 8.0,
                y: cy,
                text: "No automatic rules configured".to_string(),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            for rule in &self.auto_rules {
                commands.push(RenderCommand::FillRect {
                    x: x + padding,
                    y: cy,
                    width: width - padding * 2.0,
                    height: 28.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
                commands.push(RenderCommand::Text {
                    x: x + padding + 8.0,
                    y: cy + 6.0,
                    text: rule.label(),
                    font_size: 12.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                commands.push(RenderCommand::Text {
                    x: x + width - padding - 80.0,
                    y: cy + 7.0,
                    text: rule.mode().label().to_string(),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Light,
                    max_width: None,
                });
                cy += 32.0;
            }
        }

        commands
    }
}

impl Default for FocusAssistManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Return the more restrictive of two focus modes.
fn most_restrictive(a: FocusMode, b: FocusMode) -> FocusMode {
    let rank = |m: FocusMode| match m {
        FocusMode::Off => 0,
        FocusMode::PriorityOnly => 1,
        FocusMode::AlarmsOnly => 2,
        FocusMode::TotalSilence => 3,
    };
    if rank(a) >= rank(b) { a } else { b }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mgr() -> FocusAssistManager {
        FocusAssistManager::new()
    }

    // ---- FocusMode ----

    #[test]
    fn mode_labels() {
        assert_eq!(FocusMode::Off.label(), "Off");
        assert_eq!(FocusMode::PriorityOnly.label(), "Priority Only");
        assert_eq!(FocusMode::AlarmsOnly.label(), "Alarms Only");
        assert_eq!(FocusMode::TotalSilence.label(), "Total Silence");
    }

    #[test]
    fn mode_icons_not_empty() {
        assert!(!FocusMode::Off.icon().is_empty());
        assert!(!FocusMode::PriorityOnly.icon().is_empty());
    }

    // ---- Manual mode ----

    #[test]
    fn initial_state_off() {
        let mgr = make_mgr();
        assert_eq!(mgr.effective_mode(), FocusMode::Off);
        assert!(!mgr.is_active());
    }

    #[test]
    fn set_mode() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::PriorityOnly);
        assert_eq!(mgr.effective_mode(), FocusMode::PriorityOnly);
        assert!(mgr.is_active());
        assert!(mgr.manual_override);
    }

    #[test]
    fn set_mode_off_clears() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::TotalSilence);
        mgr.suppressed_count = 5;
        mgr.set_mode(FocusMode::Off);
        assert_eq!(mgr.suppressed_count, 0);
        assert!(!mgr.manual_override);
    }

    #[test]
    fn cycle_mode() {
        let mut mgr = make_mgr();
        mgr.cycle_mode();
        assert_eq!(mgr.manual_mode, FocusMode::PriorityOnly);
        mgr.cycle_mode();
        assert_eq!(mgr.manual_mode, FocusMode::AlarmsOnly);
        mgr.cycle_mode();
        assert_eq!(mgr.manual_mode, FocusMode::TotalSilence);
        mgr.cycle_mode();
        assert_eq!(mgr.manual_mode, FocusMode::Off);
    }

    // ---- Notification filtering ----

    #[test]
    fn all_shown_when_off() {
        let mgr = make_mgr();
        assert!(mgr.should_show_notification("any_app"));
    }

    #[test]
    fn priority_only_filters_normal() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::PriorityOnly);
        mgr.set_app_override(
            AppNotifOverride::new("chat", "Chat").with_priority(NotifPriority::Priority),
        );
        assert!(mgr.should_show_notification("chat"));
        assert!(!mgr.should_show_notification("other_app")); // Normal priority
    }

    #[test]
    fn alarms_only_filters_most() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::AlarmsOnly);
        mgr.set_app_override(
            AppNotifOverride::new("alarm", "Alarm").with_priority(NotifPriority::Critical),
        );
        mgr.set_app_override(
            AppNotifOverride::new("chat", "Chat").with_priority(NotifPriority::Priority),
        );
        assert!(mgr.should_show_notification("alarm"));
        assert!(!mgr.should_show_notification("chat"));
        assert!(!mgr.should_show_notification("other"));
    }

    #[test]
    fn total_silence_blocks_all() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::TotalSilence);
        mgr.set_app_override(
            AppNotifOverride::new("alarm", "Alarm").with_priority(NotifPriority::Critical),
        );
        assert!(!mgr.should_show_notification("alarm"));
    }

    // ---- App overrides ----

    #[test]
    fn app_priority_default() {
        let mgr = make_mgr();
        assert_eq!(mgr.app_priority("unknown"), NotifPriority::Normal);
    }

    #[test]
    fn set_app_override_replaces() {
        let mut mgr = make_mgr();
        mgr.set_app_override(AppNotifOverride::new("chat", "Chat").with_priority(NotifPriority::Silent));
        mgr.set_app_override(AppNotifOverride::new("chat", "Chat").with_priority(NotifPriority::Priority));
        assert_eq!(mgr.app_priority("chat"), NotifPriority::Priority);
        assert_eq!(mgr.app_overrides.len(), 1);
    }

    // ---- Auto rules ----

    #[test]
    fn schedule_rule_active_within_range() {
        let rule = AutoRule::Schedule {
            start_hour: 22,
            start_min: 0,
            end_hour: 7,
            end_min: 0,
            days: vec![],
            mode: FocusMode::AlarmsOnly,
        };
        assert!(rule.is_schedule_active(23, 30, 0)); // 11:30 PM
        assert!(rule.is_schedule_active(3, 0, 0));   // 3:00 AM
        assert!(!rule.is_schedule_active(12, 0, 0));  // noon
    }

    #[test]
    fn schedule_rule_respects_days() {
        let rule = AutoRule::Schedule {
            start_hour: 9,
            start_min: 0,
            end_hour: 17,
            end_min: 0,
            days: vec![1, 2, 3, 4, 5], // weekdays
            mode: FocusMode::PriorityOnly,
        };
        assert!(rule.is_schedule_active(10, 0, 1));  // Monday
        assert!(!rule.is_schedule_active(10, 0, 0)); // Sunday
    }

    #[test]
    fn schedule_rule_daytime() {
        let rule = AutoRule::Schedule {
            start_hour: 9,
            start_min: 0,
            end_hour: 17,
            end_min: 0,
            days: vec![],
            mode: FocusMode::PriorityOnly,
        };
        assert!(rule.is_schedule_active(12, 0, 0));
        assert!(!rule.is_schedule_active(20, 0, 0));
    }

    #[test]
    fn evaluate_fullscreen_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::AlarmsOnly,
        });
        mgr.fullscreen_active = true;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::AlarmsOnly);
    }

    #[test]
    fn evaluate_presentation_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Presentation {
            mode: FocusMode::TotalSilence,
        });
        mgr.presenting = true;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::TotalSilence);
    }

    #[test]
    fn evaluate_gaming_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Gaming {
            mode: FocusMode::PriorityOnly,
        });
        mgr.gaming = true;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::PriorityOnly);
    }

    #[test]
    fn evaluate_battery_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::BatteryLow {
            threshold_percent: 20,
            mode: FocusMode::AlarmsOnly,
        });
        mgr.on_battery = true;
        mgr.battery_percent = 15;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::AlarmsOnly);
    }

    #[test]
    fn battery_rule_not_active_when_charging() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::BatteryLow {
            threshold_percent: 20,
            mode: FocusMode::AlarmsOnly,
        });
        mgr.on_battery = false;
        mgr.battery_percent = 15;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::Off);
    }

    #[test]
    fn manual_overrides_auto() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::AlarmsOnly,
        });
        mgr.fullscreen_active = true;
        mgr.set_mode(FocusMode::PriorityOnly);
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::PriorityOnly);
    }

    #[test]
    fn multiple_rules_most_restrictive() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::PriorityOnly,
        });
        mgr.add_auto_rule(AutoRule::Presentation {
            mode: FocusMode::TotalSilence,
        });
        mgr.fullscreen_active = true;
        mgr.presenting = true;
        mgr.evaluate_auto_rules(12, 0, 1);
        assert_eq!(mgr.effective_mode(), FocusMode::TotalSilence);
    }

    #[test]
    fn remove_auto_rule() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::AlarmsOnly,
        });
        assert!(mgr.remove_auto_rule(0));
        assert!(mgr.auto_rules.is_empty());
    }

    #[test]
    fn remove_auto_rule_out_of_bounds() {
        let mut mgr = make_mgr();
        assert!(!mgr.remove_auto_rule(0));
    }

    // ---- most_restrictive ----

    #[test]
    fn most_restrictive_fn() {
        assert_eq!(most_restrictive(FocusMode::Off, FocusMode::PriorityOnly), FocusMode::PriorityOnly);
        assert_eq!(most_restrictive(FocusMode::AlarmsOnly, FocusMode::PriorityOnly), FocusMode::AlarmsOnly);
        assert_eq!(most_restrictive(FocusMode::TotalSilence, FocusMode::AlarmsOnly), FocusMode::TotalSilence);
    }

    // ---- Rendering ----

    #[test]
    fn tray_indicator_hidden_when_off() {
        let mgr = make_mgr();
        let cmds = mgr.render_tray_indicator(0.0, 0.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn tray_indicator_shown_when_active() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::PriorityOnly);
        let cmds = mgr.render_tray_indicator(0.0, 0.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn settings_render_not_empty() {
        let mgr = make_mgr();
        let cmds = mgr.render_settings(0.0, 0.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn settings_render_with_rules() {
        let mut mgr = make_mgr();
        mgr.add_auto_rule(AutoRule::Fullscreen {
            mode: FocusMode::AlarmsOnly,
        });
        mgr.add_auto_rule(AutoRule::Schedule {
            start_hour: 22,
            start_min: 0,
            end_hour: 7,
            end_min: 0,
            days: vec![],
            mode: FocusMode::AlarmsOnly,
        });
        let cmds = mgr.render_settings(0.0, 0.0, 400.0);
        assert!(cmds.len() > 10);
    }

    #[test]
    fn suppressed_count() {
        let mut mgr = make_mgr();
        mgr.set_mode(FocusMode::TotalSilence);
        mgr.record_suppressed();
        mgr.record_suppressed();
        assert_eq!(mgr.suppressed_count, 2);
    }

    // ---- AutoRule label ----

    #[test]
    fn auto_rule_labels() {
        let r = AutoRule::Schedule {
            start_hour: 22,
            start_min: 0,
            end_hour: 7,
            end_min: 0,
            days: vec![],
            mode: FocusMode::AlarmsOnly,
        };
        assert!(r.label().contains("22:00"));
        assert_eq!(AutoRule::Fullscreen { mode: FocusMode::Off }.label(), "Fullscreen app");
        assert_eq!(AutoRule::Presentation { mode: FocusMode::Off }.label(), "Presenting");
        assert_eq!(AutoRule::Gaming { mode: FocusMode::Off }.label(), "Gaming");
    }

    // ---- NotifPriority ordering ----

    #[test]
    fn priority_ordering() {
        assert!(NotifPriority::Critical > NotifPriority::Priority);
        assert!(NotifPriority::Priority > NotifPriority::Normal);
        assert!(NotifPriority::Normal > NotifPriority::Silent);
    }
}
