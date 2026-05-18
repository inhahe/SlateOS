//! Date, time, and timezone settings panel for the desktop shell.
//!
//! Configures system clock, timezone selection, NTP synchronization,
//! and additional clocks for multiple timezones.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
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
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Timezone
// ============================================================================

/// A timezone entry with identifier and UTC offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimezoneInfo {
    /// IANA timezone identifier (e.g. "America/New_York").
    pub tz_id: String,
    /// Display name (e.g. "Eastern Time (US & Canada)").
    pub display_name: String,
    /// UTC offset in minutes (e.g. -300 for UTC-5).
    pub utc_offset_min: i32,
    /// Whether this timezone currently observes DST.
    pub observes_dst: bool,
    /// City/region for the world clock label.
    pub city: String,
}

impl TimezoneInfo {
    pub fn new(
        tz_id: impl Into<String>,
        display_name: impl Into<String>,
        utc_offset_min: i32,
        observes_dst: bool,
        city: impl Into<String>,
    ) -> Self {
        Self {
            tz_id: tz_id.into(),
            display_name: display_name.into(),
            utc_offset_min,
            observes_dst,
            city: city.into(),
        }
    }

    /// Format the UTC offset as a string (e.g. "UTC-05:00").
    pub fn offset_string(&self) -> String {
        let hours = self.utc_offset_min / 60;
        let mins = (self.utc_offset_min % 60).unsigned_abs();
        if self.utc_offset_min >= 0 {
            format!("UTC+{:02}:{:02}", hours, mins)
        } else {
            format!("UTC-{:02}:{:02}", hours.unsigned_abs(), mins)
        }
    }

    /// Convert a UTC timestamp to local time in this timezone.
    /// Returns (hour, minute) in 24-hour format.
    pub fn local_time(&self, utc_secs: u64) -> (u32, u32) {
        let total_secs = utc_secs as i64 + (self.utc_offset_min as i64 * 60);
        let day_secs = total_secs.rem_euclid(86400);
        let hour = (day_secs / 3600) as u32;
        let minute = ((day_secs % 3600) / 60) as u32;
        (hour, minute)
    }
}

/// Default timezones.
pub fn default_timezones() -> Vec<TimezoneInfo> {
    vec![
        TimezoneInfo::new("Pacific/Honolulu", "Hawaii", -600, false, "Honolulu"),
        TimezoneInfo::new("America/Anchorage", "Alaska", -540, true, "Anchorage"),
        TimezoneInfo::new("America/Los_Angeles", "Pacific Time", -480, true, "Los Angeles"),
        TimezoneInfo::new("America/Denver", "Mountain Time", -420, true, "Denver"),
        TimezoneInfo::new("America/Chicago", "Central Time", -360, true, "Chicago"),
        TimezoneInfo::new("America/New_York", "Eastern Time", -300, true, "New York"),
        TimezoneInfo::new("America/Sao_Paulo", "Brasilia Time", -180, true, "S\u{00e3}o Paulo"),
        TimezoneInfo::new("Atlantic/Reykjavik", "Iceland", 0, false, "Reykjavik"),
        TimezoneInfo::new("Europe/London", "GMT/BST", 0, true, "London"),
        TimezoneInfo::new("Europe/Paris", "Central European", 60, true, "Paris"),
        TimezoneInfo::new("Europe/Berlin", "Central European", 60, true, "Berlin"),
        TimezoneInfo::new("Europe/Helsinki", "Eastern European", 120, true, "Helsinki"),
        TimezoneInfo::new("Europe/Moscow", "Moscow Time", 180, false, "Moscow"),
        TimezoneInfo::new("Asia/Dubai", "Gulf Standard", 240, false, "Dubai"),
        TimezoneInfo::new("Asia/Kolkata", "India Standard", 330, false, "Mumbai"),
        TimezoneInfo::new("Asia/Shanghai", "China Standard", 480, false, "Shanghai"),
        TimezoneInfo::new("Asia/Tokyo", "Japan Standard", 540, false, "Tokyo"),
        TimezoneInfo::new("Asia/Seoul", "Korea Standard", 540, false, "Seoul"),
        TimezoneInfo::new("Australia/Sydney", "Australian Eastern", 600, true, "Sydney"),
        TimezoneInfo::new("Pacific/Auckland", "New Zealand", 720, true, "Auckland"),
    ]
}

// ============================================================================
// NTP synchronization
// ============================================================================

/// NTP synchronization status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NtpStatus {
    /// NTP is disabled.
    Disabled,
    /// Attempting to synchronize.
    Syncing,
    /// Successfully synchronized.
    Synchronized,
    /// Failed to synchronize.
    Error,
}

impl NtpStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Syncing => "Syncing...",
            Self::Synchronized => "Synchronized",
            Self::Error => "Error",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Disabled => OVERLAY0,
            Self::Syncing => YELLOW,
            Self::Synchronized => GREEN,
            Self::Error => RED,
        }
    }
}

/// NTP configuration.
#[derive(Clone, Debug)]
pub struct NtpConfig {
    /// Whether NTP time sync is enabled.
    pub enabled: bool,
    /// NTP server addresses.
    pub servers: Vec<String>,
    /// Sync interval in seconds.
    pub sync_interval_secs: u64,
    /// Current status.
    pub status: NtpStatus,
    /// Last successful sync timestamp (seconds since epoch).
    pub last_sync_at: Option<u64>,
    /// Measured clock offset in milliseconds (positive = ahead, negative = behind).
    pub offset_ms: Option<i64>,
}

impl Default for NtpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: vec![
                "pool.ntp.org".to_string(),
                "time.google.com".to_string(),
                "time.cloudflare.com".to_string(),
            ],
            sync_interval_secs: 3600,
            status: NtpStatus::Disabled,
            last_sync_at: None,
            offset_ms: None,
        }
    }
}

impl NtpConfig {
    /// Add an NTP server (max 8).
    pub fn add_server(&mut self, server: impl Into<String>) -> bool {
        if self.servers.len() >= 8 {
            return false;
        }
        let s = server.into();
        if !self.servers.contains(&s) {
            self.servers.push(s);
            true
        } else {
            false
        }
    }

    /// Remove an NTP server.
    pub fn remove_server(&mut self, server: &str) -> bool {
        let before = self.servers.len();
        self.servers.retain(|s| s != server);
        self.servers.len() < before
    }
}

// ============================================================================
// Additional clock
// ============================================================================

/// An additional clock displayed in the system tray.
#[derive(Clone, Debug)]
pub struct AdditionalClock {
    /// Timezone identifier.
    pub tz_id: String,
    /// Custom label (e.g. "Office" or "Home").
    pub label: String,
    /// Whether this clock is shown.
    pub visible: bool,
}

impl AdditionalClock {
    pub fn new(tz_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            tz_id: tz_id.into(),
            label: label.into(),
            visible: true,
        }
    }
}

// ============================================================================
// Date/time settings aggregate
// ============================================================================

/// All date/time settings.
#[derive(Clone, Debug)]
pub struct DateTimeSettings {
    /// Current timezone.
    pub timezone: String,
    /// Whether to auto-detect timezone.
    pub auto_timezone: bool,
    /// NTP configuration.
    pub ntp: NtpConfig,
    /// Additional clocks (max 4).
    pub additional_clocks: Vec<AdditionalClock>,
    /// Available timezones.
    pub available_timezones: Vec<TimezoneInfo>,
    /// Whether to show seconds in the taskbar clock.
    pub show_seconds: bool,
    /// Whether to show day of week in the taskbar clock.
    pub show_day_of_week: bool,
    /// Whether to show date in the taskbar clock.
    pub show_date: bool,
}

impl Default for DateTimeSettings {
    fn default() -> Self {
        Self {
            timezone: "America/New_York".to_string(),
            auto_timezone: false,
            ntp: NtpConfig::default(),
            additional_clocks: Vec::new(),
            available_timezones: default_timezones(),
            show_seconds: false,
            show_day_of_week: true,
            show_date: true,
        }
    }
}

impl DateTimeSettings {
    /// Get info about the current timezone.
    pub fn current_timezone(&self) -> Option<&TimezoneInfo> {
        self.available_timezones.iter().find(|t| t.tz_id == self.timezone)
    }

    /// Set the timezone (validates against available list).
    pub fn set_timezone(&mut self, tz_id: &str) -> bool {
        if self.available_timezones.iter().any(|t| t.tz_id == tz_id) {
            self.timezone = tz_id.to_string();
            true
        } else {
            false
        }
    }

    /// Add an additional clock (max 4).
    pub fn add_clock(&mut self, tz_id: impl Into<String>, label: impl Into<String>) -> bool {
        if self.additional_clocks.len() >= 4 {
            return false;
        }
        self.additional_clocks.push(AdditionalClock::new(tz_id, label));
        true
    }

    /// Remove an additional clock by index.
    pub fn remove_clock(&mut self, index: usize) -> bool {
        if index < self.additional_clocks.len() {
            self.additional_clocks.remove(index);
            true
        } else {
            false
        }
    }

    /// Search available timezones.
    pub fn search_timezones(&self, query: &str) -> Vec<&TimezoneInfo> {
        let q = query.to_lowercase();
        self.available_timezones.iter()
            .filter(|t| {
                t.tz_id.to_lowercase().contains(&q)
                    || t.display_name.to_lowercase().contains(&q)
                    || t.city.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Get local time in the current timezone for a given UTC timestamp.
    pub fn local_time(&self, utc_secs: u64) -> Option<(u32, u32)> {
        self.current_timezone().map(|tz| tz.local_time(utc_secs))
    }
}

// ============================================================================
// UI
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateTimeTab {
    DateTime,
    Timezone,
    Ntp,
    Clocks,
}

impl DateTimeTab {
    fn label(self) -> &'static str {
        match self {
            Self::DateTime => "Date & Time",
            Self::Timezone => "Timezone",
            Self::Ntp => "Sync",
            Self::Clocks => "World Clocks",
        }
    }
}

pub struct DateTimeSettingsUI {
    pub active_tab: DateTimeTab,
    pub settings: DateTimeSettings,
    pub tz_search: String,
    pub selected_tz_index: Option<usize>,
    /// Mock current UTC time for display.
    pub current_utc: u64,
}

impl DateTimeSettingsUI {
    pub fn new() -> Self {
        Self {
            active_tab: DateTimeTab::DateTime,
            settings: DateTimeSettings::default(),
            tz_search: String::new(),
            selected_tz_index: None,
            current_utc: 1747612800, // ~2025-05-18 UTC
        }
    }

    pub fn set_tab(&mut self, tab: DateTimeTab) {
        self.active_tab = tab;
    }

    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width, height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: 24.0, y: 24.0,
            text: "Date & Time".into(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
        });

        // Tabs
        let tabs = [DateTimeTab::DateTime, DateTimeTab::Timezone, DateTimeTab::Ntp, DateTimeTab::Clocks];
        let tab_y = 60.0;
        let mut tx = 24.0;
        for &tab in &tabs {
            let active = tab == self.active_tab;
            let tw = tab.label().len() as f32 * 7.5 + 20.0;
            cmds.push(RenderCommand::FillRect {
                x: tx, y: tab_y, width: tw, height: 32.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0, y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tw - 20.0),
            });
            tx += tw + 8.0;
        }

        let cy = tab_y + 48.0;
        let cw = width - 48.0;

        match self.active_tab {
            DateTimeTab::DateTime => self.render_datetime_tab(&mut cmds, 24.0, cy, cw),
            DateTimeTab::Timezone => self.render_timezone_tab(&mut cmds, 24.0, cy, cw),
            DateTimeTab::Ntp => self.render_ntp_tab(&mut cmds, 24.0, cy, cw),
            DateTimeTab::Clocks => self.render_clocks_tab(&mut cmds, 24.0, cy, cw),
        }

        cmds
    }

    fn render_datetime_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Current time display
        if let Some((hour, minute)) = self.settings.local_time(self.current_utc) {
            cmds.push(RenderCommand::FillRect {
                x, y: cy, width, height: 80.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(12.0),
            });

            let time_str = format!("{:02}:{:02}", hour, minute);
            cmds.push(RenderCommand::Text {
                x: x + width * 0.5 - 60.0, y: cy + 12.0,
                text: time_str,
                font_size: 36.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
            });

            if let Some(tz) = self.settings.current_timezone() {
                cmds.push(RenderCommand::Text {
                    x: x + width * 0.5 - 60.0, y: cy + 56.0,
                    text: format!("{} ({})", tz.display_name, tz.offset_string()),
                    font_size: 13.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                });
            }
            cy += 96.0;
        }

        // Clock display options
        cmds.push(RenderCommand::Text {
            x, y: cy,
            text: "Taskbar Clock".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_toggle_row(cmds, x, cy, width, "Show Seconds", self.settings.show_seconds);
        cy += 32.0;
        self.render_toggle_row(cmds, x, cy, width, "Show Day of Week", self.settings.show_day_of_week);
        cy += 32.0;
        self.render_toggle_row(cmds, x, cy, width, "Show Date", self.settings.show_date);
        let _ = cy;
    }

    fn render_timezone_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Current timezone
        if let Some(tz) = self.settings.current_timezone() {
            cmds.push(RenderCommand::FillRect {
                x, y: cy, width, height: 44.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: cy + 6.0,
                text: format!("Current: {} ({})", tz.display_name, tz.offset_string()),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: cy + 26.0,
                text: format!("{}{}", tz.tz_id, if tz.observes_dst { " (DST)" } else { "" }),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
            });
            cy += 52.0;
        }

        // Auto-detect toggle
        self.render_toggle_row(cmds, x, cy, width, "Auto-detect Timezone", self.settings.auto_timezone);
        cy += 40.0;

        // Search
        cmds.push(RenderCommand::FillRect {
            x, y: cy, width, height: 30.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.tz_search.is_empty() {
            "Search timezones...".to_string()
        } else {
            self.tz_search.clone()
        };
        cmds.push(RenderCommand::Text {
            x: x + 10.0, y: cy + 7.0,
            text: search_text,
            font_size: 13.0,
            color: if self.tz_search.is_empty() { OVERLAY0 } else { TEXT },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 20.0),
        });
        cy += 40.0;

        // Timezone list
        let filtered = if self.tz_search.is_empty() {
            self.settings.available_timezones.iter().collect::<Vec<_>>()
        } else {
            self.settings.search_timezones(&self.tz_search)
        };

        for (i, tz) in filtered.iter().enumerate().take(10) {
            let is_selected = self.selected_tz_index == Some(i);
            let is_current = tz.tz_id == self.settings.timezone;

            cmds.push(RenderCommand::FillRect {
                x, y: cy, width, height: 36.0,
                color: if is_selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });

            if is_current {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0, y: cy + 4.0, width: 4.0, height: 28.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 16.0, y: cy + 4.0,
                text: format!("{} — {}", tz.city, tz.display_name),
                font_size: 13.0,
                color: if is_current { BLUE } else { TEXT },
                font_weight: if is_current { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(width * 0.65),
            });

            cmds.push(RenderCommand::Text {
                x: x + width - 100.0, y: cy + 4.0,
                text: tz.offset_string(),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });

            if tz.observes_dst {
                cmds.push(RenderCommand::Text {
                    x: x + width - 100.0, y: cy + 20.0,
                    text: "DST".into(),
                    font_size: 10.0,
                    color: YELLOW,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(40.0),
                });
            }

            cy += 40.0;
        }
    }

    fn render_ntp_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;
        let ntp = &self.settings.ntp;

        cmds.push(RenderCommand::Text {
            x, y: cy,
            text: "Time Synchronization".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_toggle_row(cmds, x, cy, width, "Enable NTP Sync", ntp.enabled);
        cy += 36.0;

        // Status
        let status_color = ntp.status.color();
        cmds.push(RenderCommand::FillRect {
            x, y: cy, width, height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0, y: cy + 12.0, width: 12.0, height: 12.0,
            color: status_color,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 28.0, y: cy + 10.0,
            text: format!("Status: {}", ntp.status.label()),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 40.0),
        });
        cy += 44.0;

        // Last sync
        if let Some(ts) = ntp.last_sync_at {
            self.render_label_value(cmds, x, cy, width, "Last sync", &format!("{}s ago", self.current_utc.saturating_sub(ts)));
            cy += 24.0;
        }

        // Offset
        if let Some(offset) = ntp.offset_ms {
            self.render_label_value(cmds, x, cy, width, "Clock offset", &format!("{}ms", offset));
            cy += 24.0;
        }

        // Interval
        self.render_label_value(
            cmds, x, cy, width,
            "Sync interval",
            &format!("{} min", ntp.sync_interval_secs / 60),
        );
        cy += 36.0;

        // NTP servers
        cmds.push(RenderCommand::Text {
            x, y: cy,
            text: "NTP Servers".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 24.0;

        for server in &ntp.servers {
            cmds.push(RenderCommand::FillRect {
                x, y: cy, width, height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 10.0, y: cy + 6.0,
                text: server.clone(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
            });
            cy += 32.0;
        }
    }

    fn render_clocks_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        cmds.push(RenderCommand::Text {
            x, y: cy,
            text: "Additional Clocks".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 24.0;

        cmds.push(RenderCommand::Text {
            x, y: cy,
            text: format!("{}/4 clocks configured", self.settings.additional_clocks.len()),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        cy += 28.0;

        if self.settings.additional_clocks.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 10.0, y: cy + 20.0,
                text: "No additional clocks. Add one to track time in another city.".into(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
            });
            return;
        }

        for clock in &self.settings.additional_clocks {
            let tz = self.settings.available_timezones.iter().find(|t| t.tz_id == clock.tz_id);

            cmds.push(RenderCommand::FillRect {
                x, y: cy, width, height: 60.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            // Clock label
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: cy + 6.0,
                text: clock.label.clone(),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width * 0.5),
            });

            // Time display
            if let Some(tz_info) = tz {
                let (h, m) = tz_info.local_time(self.current_utc);
                cmds.push(RenderCommand::Text {
                    x: x + width - 100.0, y: cy + 6.0,
                    text: format!("{:02}:{:02}", h, m),
                    font_size: 20.0,
                    color: BLUE,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(100.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 12.0, y: cy + 28.0,
                    text: format!("{} ({})", tz_info.display_name, tz_info.offset_string()),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                });
            }

            // Visibility indicator
            if !clock.visible {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0, y: cy + 44.0,
                    text: "Hidden".into(),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(60.0),
                });
            }

            cy += 68.0;
        }
    }

    fn render_toggle_row(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, label: &str, enabled: bool) {
        cmds.push(RenderCommand::Text {
            x, y: y + 4.0,
            text: label.into(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 80.0),
        });
        let sw_x = x + width - 44.0;
        cmds.push(RenderCommand::FillRect {
            x: sw_x, y: y + 2.0, width: 40.0, height: 22.0,
            color: if enabled { GREEN } else { SURFACE2 },
            corner_radii: CornerRadii::all(11.0),
        });
        let knob_x = if enabled { sw_x + 20.0 } else { sw_x + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x, y: y + 4.0, width: 18.0, height: 18.0,
            color: TEXT,
            corner_radii: CornerRadii::all(9.0),
        });
    }

    fn render_label_value(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, label: &str, value: &str) {
        cmds.push(RenderCommand::Text {
            x, y,
            text: label.into(),
            font_size: 13.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.45, y,
            text: value.into(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.55),
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- TimezoneInfo ----

    #[test]
    fn test_offset_string_positive() {
        let tz = TimezoneInfo::new("Asia/Tokyo", "JST", 540, false, "Tokyo");
        assert_eq!(tz.offset_string(), "UTC+09:00");
    }

    #[test]
    fn test_offset_string_negative() {
        let tz = TimezoneInfo::new("America/New_York", "ET", -300, true, "New York");
        assert_eq!(tz.offset_string(), "UTC-05:00");
    }

    #[test]
    fn test_offset_string_half_hour() {
        let tz = TimezoneInfo::new("Asia/Kolkata", "IST", 330, false, "Mumbai");
        assert_eq!(tz.offset_string(), "UTC+05:30");
    }

    #[test]
    fn test_offset_string_zero() {
        let tz = TimezoneInfo::new("UTC", "UTC", 0, false, "UTC");
        assert_eq!(tz.offset_string(), "UTC+00:00");
    }

    #[test]
    fn test_local_time_basic() {
        let tz = TimezoneInfo::new("UTC", "UTC", 0, false, "UTC");
        // 12:00:00 UTC
        let (h, m) = tz.local_time(43200);
        assert_eq!(h, 12);
        assert_eq!(m, 0);
    }

    #[test]
    fn test_local_time_offset() {
        let tz = TimezoneInfo::new("test", "test", 540, false, "test"); // UTC+9
        // 00:00:00 UTC -> 09:00 JST
        let (h, m) = tz.local_time(0);
        assert_eq!(h, 9);
        assert_eq!(m, 0);
    }

    #[test]
    fn test_local_time_negative_offset() {
        let tz = TimezoneInfo::new("test", "test", -300, false, "test"); // UTC-5
        // 03:00:00 UTC -> 22:00 previous day
        let (h, m) = tz.local_time(10800);
        assert_eq!(h, 22);
        assert_eq!(m, 0);
    }

    #[test]
    fn test_default_timezones_count() {
        assert_eq!(default_timezones().len(), 20);
    }

    // ---- NtpStatus ----

    #[test]
    fn test_ntp_status_labels() {
        assert_eq!(NtpStatus::Disabled.label(), "Disabled");
        assert_eq!(NtpStatus::Synchronized.label(), "Synchronized");
    }

    // ---- NtpConfig ----

    #[test]
    fn test_ntp_config_default() {
        let c = NtpConfig::default();
        assert!(c.enabled);
        assert_eq!(c.servers.len(), 3);
    }

    #[test]
    fn test_ntp_add_server() {
        let mut c = NtpConfig::default();
        assert!(c.add_server("time.apple.com"));
        assert_eq!(c.servers.len(), 4);
    }

    #[test]
    fn test_ntp_add_duplicate() {
        let mut c = NtpConfig::default();
        assert!(!c.add_server("pool.ntp.org"));
    }

    #[test]
    fn test_ntp_add_max() {
        let mut c = NtpConfig::default();
        // Already has 3, max is 8
        for i in 0..5 {
            c.add_server(format!("server{}.example.com", i));
        }
        assert_eq!(c.servers.len(), 8);
        assert!(!c.add_server("one-more.example.com"));
    }

    #[test]
    fn test_ntp_remove_server() {
        let mut c = NtpConfig::default();
        assert!(c.remove_server("pool.ntp.org"));
        assert_eq!(c.servers.len(), 2);
        assert!(!c.remove_server("nonexistent"));
    }

    // ---- DateTimeSettings ----

    #[test]
    fn test_settings_default() {
        let s = DateTimeSettings::default();
        assert_eq!(s.timezone, "America/New_York");
        assert!(!s.auto_timezone);
        assert!(s.show_day_of_week);
    }

    #[test]
    fn test_current_timezone() {
        let s = DateTimeSettings::default();
        let tz = s.current_timezone().unwrap();
        assert_eq!(tz.city, "New York");
    }

    #[test]
    fn test_set_timezone_valid() {
        let mut s = DateTimeSettings::default();
        assert!(s.set_timezone("Asia/Tokyo"));
        assert_eq!(s.timezone, "Asia/Tokyo");
    }

    #[test]
    fn test_set_timezone_invalid() {
        let mut s = DateTimeSettings::default();
        assert!(!s.set_timezone("Invalid/Zone"));
    }

    #[test]
    fn test_add_clock() {
        let mut s = DateTimeSettings::default();
        assert!(s.add_clock("Asia/Tokyo", "Tokyo Office"));
        assert_eq!(s.additional_clocks.len(), 1);
    }

    #[test]
    fn test_add_clock_max() {
        let mut s = DateTimeSettings::default();
        for i in 0..4 {
            s.add_clock(format!("tz{}", i), format!("Clock {}", i));
        }
        assert!(!s.add_clock("extra", "Extra"));
        assert_eq!(s.additional_clocks.len(), 4);
    }

    #[test]
    fn test_remove_clock() {
        let mut s = DateTimeSettings::default();
        s.add_clock("Asia/Tokyo", "Tokyo");
        assert!(s.remove_clock(0));
        assert!(s.additional_clocks.is_empty());
    }

    #[test]
    fn test_remove_clock_invalid() {
        let mut s = DateTimeSettings::default();
        assert!(!s.remove_clock(0));
    }

    #[test]
    fn test_search_timezones() {
        let s = DateTimeSettings::default();
        let results = s.search_timezones("tokyo");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].city, "Tokyo");
    }

    #[test]
    fn test_search_timezones_multiple() {
        let s = DateTimeSettings::default();
        let results = s.search_timezones("europe");
        assert!(results.len() >= 3);
    }

    #[test]
    fn test_local_time() {
        let s = DateTimeSettings::default();
        let result = s.local_time(43200);
        assert!(result.is_some());
    }

    // ---- DateTimeSettingsUI ----

    #[test]
    fn test_ui_new() {
        let ui = DateTimeSettingsUI::new();
        assert_eq!(ui.active_tab, DateTimeTab::DateTime);
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Timezone);
        assert_eq!(ui.active_tab, DateTimeTab::Timezone);
    }

    #[test]
    fn test_ui_render_datetime() {
        let ui = DateTimeSettingsUI::new();
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_timezone() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Timezone);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_ntp() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Ntp);
        ui.settings.ntp.status = NtpStatus::Synchronized;
        ui.settings.ntp.last_sync_at = Some(ui.current_utc - 300);
        ui.settings.ntp.offset_ms = Some(-23);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_clocks_empty() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Clocks);
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_clocks_with_entries() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Clocks);
        ui.settings.add_clock("Asia/Tokyo", "Tokyo");
        ui.settings.add_clock("Europe/London", "London");
        let cmds = ui.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    // ---- Tab labels ----

    #[test]
    fn test_tab_labels() {
        assert_eq!(DateTimeTab::DateTime.label(), "Date & Time");
        assert_eq!(DateTimeTab::Timezone.label(), "Timezone");
        assert_eq!(DateTimeTab::Ntp.label(), "Sync");
        assert_eq!(DateTimeTab::Clocks.label(), "World Clocks");
    }

    // ---- AdditionalClock ----

    #[test]
    fn test_additional_clock_new() {
        let c = AdditionalClock::new("Asia/Tokyo", "Office");
        assert_eq!(c.tz_id, "Asia/Tokyo");
        assert_eq!(c.label, "Office");
        assert!(c.visible);
    }
}
