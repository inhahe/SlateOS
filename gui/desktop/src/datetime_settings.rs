//! Date, time, and timezone settings panel for the desktop shell.
//!
//! Configures system clock, timezone selection, NTP synchronization,
//! and additional clocks for multiple timezones.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;
use guitk::text;

// The same zone engine the libc's `localtime`, osh's `printf '%(…)T'` and the
// taskbar clock use.  This panel is where the machine's zone is *chosen*, so of
// everything on the desktop it is the last place that may hold its own opinion
// about what a zone means.
use tzrules::Tz;

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

/// A selectable timezone: an IANA identifier, display strings, and the POSIX
/// `TZ` rule that says what the clock actually reads.
///
/// The rule is the whole point. A zone that observes daylight saving has *two*
/// offsets and a rule choosing between them; an entry that stored one number
/// (this struct used to store `utc_offset_min: i32`) is wrong for whichever
/// half of the year it is not describing — for Eastern Time, wrong for the
/// roughly eight months of EDT, and wrong *silently*. The `observes_dst: bool`
/// that sat beside it recorded that the entry knew it was incomplete without
/// doing anything about it, which is worse than not knowing.
///
/// `tz_id` stays the stable key — it is what `set_timezone` matches on and what
/// an eventual system configuration file would name — but nothing reads local
/// time from it, because reading a zoneinfo name needs a tzdata database we do
/// not ship (see `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ` in `known-issues.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimezoneInfo {
    /// IANA timezone identifier (e.g. "America/New_York").
    pub tz_id: String,
    /// Display name (e.g. "Eastern Time (US & Canada)").
    pub display_name: String,
    /// City/region for the world clock label.
    pub city: String,
    /// The POSIX `TZ` rule this entry renders with.
    pub rule: Tz,
}

impl TimezoneInfo {
    /// Build an entry from a POSIX `TZ` string.
    ///
    /// Returns `None` if the string is not a POSIX `TZ` rule. A caller cannot
    /// usefully recover from that, but it must not be a panic either: this is
    /// a table of literals, so a malformed rule is a typo, and the right place
    /// to catch a typo is `test_default_timezones_count` noticing the table
    /// came up short — not a fault in a running desktop.
    pub fn new(
        tz_id: impl Into<String>,
        display_name: impl Into<String>,
        posix_tz: &str,
        city: impl Into<String>,
    ) -> Option<Self> {
        Some(Self {
            tz_id: tz_id.into(),
            display_name: display_name.into(),
            city: city.into(),
            rule: Tz::parse(posix_tz.as_bytes())?,
        })
    }

    /// The offset from UTC, in seconds, in force at `utc_secs`.
    pub fn offset_secs_at(&self, utc_secs: u64) -> i32 {
        self.rule.lookup(clamp_to_i64(utc_secs)).gmtoff
    }

    /// Format the offset in force at `utc_secs` (e.g. "UTC-05:00" in January
    /// and "UTC-04:00" in July, for the same New York entry).
    pub fn offset_string(&self, utc_secs: u64) -> String {
        let secs = self.offset_secs_at(utc_secs);
        let mins = secs.div_euclid(60);
        let hours = mins / 60;
        let rem = (mins % 60).unsigned_abs();
        if mins >= 0 {
            format!("UTC+{hours:02}:{rem:02}")
        } else {
            format!("UTC-{:02}:{rem:02}", hours.unsigned_abs())
        }
    }

    /// The zone abbreviation in force at `utc_secs` (e.g. "EST" or "EDT").
    ///
    /// Lossy only for a name that is not UTF-8, which `Tz::parse` cannot
    /// produce — the grammar admits alphanumerics and `+`/`-` only.
    pub fn abbrev_at(&self, utc_secs: u64) -> String {
        let info = self.rule.lookup(clamp_to_i64(utc_secs));
        String::from_utf8_lossy(info.name.as_bytes()).into_owned()
    }

    /// Whether this zone observes daylight saving *at all*.
    pub fn observes_dst(&self) -> bool {
        self.rule.has_dst()
    }

    /// Whether daylight saving is actually in force at `utc_secs`. This is the
    /// live fact a timezone picker should show; `observes_dst` is only the
    /// standing property.
    pub fn is_dst_at(&self, utc_secs: u64) -> bool {
        self.rule.lookup(clamp_to_i64(utc_secs)).is_dst
    }

    /// Convert a UTC timestamp to local time in this timezone.
    /// Returns (hour, minute) in 24-hour format.
    pub fn local_time(&self, utc_secs: u64) -> (u32, u32) {
        let t = clamp_to_i64(utc_secs);
        let local = t.saturating_add(i64::from(self.rule.lookup(t).gmtoff));
        let day_secs = local.rem_euclid(86_400);
        // 0..86_400 by construction, so both casts are exact.
        let hour = u32::try_from(day_secs / 3600).unwrap_or(0);
        let minute = u32::try_from((day_secs % 3600) / 60).unwrap_or(0);
        (hour, minute)
    }
}

/// Timestamps reach this module as `u64`; `tzrules` speaks `i64`. Saturating
/// rather than wrapping, so an absurd clock reading stays absurd in the same
/// direction instead of becoming a plausible date in 1901.
fn clamp_to_i64(utc_secs: u64) -> i64 {
    i64::try_from(utc_secs).unwrap_or(i64::MAX)
}

/// Default timezones.
///
/// Each rule is the POSIX `TZ` string tzdata publishes for that zone, so the
/// offsets and transition dates below are the real ones rather than a snapshot
/// of whatever was in force the day the table was written. Note two entries
/// that a fixed-offset table got wrong: São Paulo abolished DST in 2019 and is
/// now a plain `-03`, and Sydney and Auckland are southern-hemisphere, so their
/// DST window straddles New Year.
pub fn default_timezones() -> Vec<TimezoneInfo> {
    // `flatten` drops any entry whose rule fails to parse. That can only be a
    // typo in the literals above, and `test_default_timezones_count` fails when
    // it happens.
    [
        TimezoneInfo::new("Pacific/Honolulu", "Hawaii", "HST10", "Honolulu"),
        TimezoneInfo::new(
            "America/Anchorage",
            "Alaska",
            "AKST9AKDT,M3.2.0,M11.1.0",
            "Anchorage",
        ),
        TimezoneInfo::new(
            "America/Los_Angeles",
            "Pacific Time",
            "PST8PDT,M3.2.0,M11.1.0",
            "Los Angeles",
        ),
        TimezoneInfo::new(
            "America/Denver",
            "Mountain Time",
            "MST7MDT,M3.2.0,M11.1.0",
            "Denver",
        ),
        TimezoneInfo::new(
            "America/Chicago",
            "Central Time",
            "CST6CDT,M3.2.0,M11.1.0",
            "Chicago",
        ),
        TimezoneInfo::new(
            "America/New_York",
            "Eastern Time",
            "EST5EDT,M3.2.0,M11.1.0",
            "New York",
        ),
        TimezoneInfo::new(
            "America/Sao_Paulo",
            "Brasilia Time",
            "<-03>3",
            "S\u{00e3}o Paulo",
        ),
        TimezoneInfo::new("Atlantic/Reykjavik", "Iceland", "GMT0", "Reykjavik"),
        TimezoneInfo::new(
            "Europe/London",
            "GMT/BST",
            "GMT0BST,M3.5.0/1,M10.5.0",
            "London",
        ),
        TimezoneInfo::new(
            "Europe/Paris",
            "Central European",
            "CET-1CEST,M3.5.0,M10.5.0/3",
            "Paris",
        ),
        TimezoneInfo::new(
            "Europe/Berlin",
            "Central European",
            "CET-1CEST,M3.5.0,M10.5.0/3",
            "Berlin",
        ),
        TimezoneInfo::new(
            "Europe/Helsinki",
            "Eastern European",
            "EET-2EEST,M3.5.0/3,M10.5.0/4",
            "Helsinki",
        ),
        TimezoneInfo::new("Europe/Moscow", "Moscow Time", "MSK-3", "Moscow"),
        TimezoneInfo::new("Asia/Dubai", "Gulf Standard", "<+04>-4", "Dubai"),
        TimezoneInfo::new("Asia/Kolkata", "India Standard", "IST-5:30", "Mumbai"),
        TimezoneInfo::new("Asia/Shanghai", "China Standard", "CST-8", "Shanghai"),
        TimezoneInfo::new("Asia/Tokyo", "Japan Standard", "JST-9", "Tokyo"),
        TimezoneInfo::new("Asia/Seoul", "Korea Standard", "KST-9", "Seoul"),
        TimezoneInfo::new(
            "Australia/Sydney",
            "Australian Eastern",
            "AEST-10AEDT,M10.1.0,M4.1.0/3",
            "Sydney",
        ),
        TimezoneInfo::new(
            "Pacific/Auckland",
            "New Zealand",
            "NZST-12NZDT,M9.5.0,M4.1.0/3",
            "Auckland",
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
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
        self.available_timezones
            .iter()
            .find(|t| t.tz_id == self.timezone)
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
        self.additional_clocks
            .push(AdditionalClock::new(tz_id, label));
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
        self.available_timezones
            .iter()
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
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Date & Time".into(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
        });

        // Tabs
        let tabs = [
            DateTimeTab::DateTime,
            DateTimeTab::Timezone,
            DateTimeTab::Ntp,
            DateTimeTab::Clocks,
        ];
        let tab_y = 60.0;
        let mut tx = 24.0;
        for &tab in &tabs {
            let active = tab == self.active_tab;
            let tw = text::padded_width_any_weight(tab.label(), 10.0, 13.0);
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: tab_y,
                width: tw,
                height: 32.0,
                color: if active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { CRUST } else { SUBTEXT0 },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
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
                x,
                y: cy,
                width,
                height: 80.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(12.0),
            });

            let time_str = format!("{:02}:{:02}", hour, minute);
            cmds.push(RenderCommand::Text {
                x: x + width * 0.5 - 60.0,
                y: cy + 12.0,
                text: time_str,
                font_size: 36.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
            });

            if let Some(tz) = self.settings.current_timezone() {
                cmds.push(RenderCommand::Text {
                    x: x + width * 0.5 - 60.0,
                    y: cy + 56.0,
                    text: format!(
                        "{} — {} ({})",
                        tz.display_name,
                        tz.abbrev_at(self.current_utc),
                        tz.offset_string(self.current_utc)
                    ),
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
            x,
            y: cy,
            text: "Taskbar Clock".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 26.0;

        self.render_toggle_row(
            cmds,
            x,
            cy,
            width,
            "Show Seconds",
            self.settings.show_seconds,
        );
        cy += 32.0;
        self.render_toggle_row(
            cmds,
            x,
            cy,
            width,
            "Show Day of Week",
            self.settings.show_day_of_week,
        );
        cy += 32.0;
        self.render_toggle_row(cmds, x, cy, width, "Show Date", self.settings.show_date);
        let _ = cy;
    }

    fn render_timezone_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut cy = y;

        // Current timezone
        if let Some(tz) = self.settings.current_timezone() {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 44.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 6.0,
                text: format!(
                    "Current: {} ({})",
                    tz.display_name,
                    tz.offset_string(self.current_utc)
                ),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 26.0,
                // The abbreviation is the live half of this line: a zone that
                // observes DST reads `EST` in January and `EDT` in July, and
                // saying so is what tells the user the offset above is not a
                // constant.
                text: format!("{} — {}", tz.tz_id, tz.abbrev_at(self.current_utc)),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
            });
            cy += 52.0;
        }

        // Auto-detect toggle
        self.render_toggle_row(
            cmds,
            x,
            cy,
            width,
            "Auto-detect Timezone",
            self.settings.auto_timezone,
        );
        cy += 40.0;

        // Search
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 30.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.tz_search.is_empty() {
            "Search timezones...".to_string()
        } else {
            self.tz_search.clone()
        };
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: cy + 7.0,
            text: search_text,
            font_size: 13.0,
            color: if self.tz_search.is_empty() {
                OVERLAY0
            } else {
                TEXT
            },
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
                x,
                y: cy,
                width,
                height: 36.0,
                color: if is_selected { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });

            if is_current {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: cy + 4.0,
                    width: 4.0,
                    height: 28.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: cy + 4.0,
                text: format!("{} — {}", tz.city, tz.display_name),
                font_size: 13.0,
                color: if is_current { BLUE } else { TEXT },
                font_weight: if is_current {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(width * 0.65),
            });

            cmds.push(RenderCommand::Text {
                x: x + width - 100.0,
                y: cy + 4.0,
                text: tz.offset_string(self.current_utc),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });

            // Badge the zones whose clock is *currently* shifted, not the ones
            // that observe DST at some point in the year.  The offset printed
            // just above already moved; without this the badge would sit on
            // Sydney all through the northern summer, when Sydney is on
            // standard time and it is New York that has jumped.
            if tz.is_dst_at(self.current_utc) {
                cmds.push(RenderCommand::Text {
                    x: x + width - 100.0,
                    y: cy + 20.0,
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
            x,
            y: cy,
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
            x,
            y: cy,
            width,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: cy + 12.0,
            width: 12.0,
            height: 12.0,
            color: status_color,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 28.0,
            y: cy + 10.0,
            text: format!("Status: {}", ntp.status.label()),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 40.0),
        });
        cy += 44.0;

        // Last sync
        if let Some(ts) = ntp.last_sync_at {
            self.render_label_value(
                cmds,
                x,
                cy,
                width,
                "Last sync",
                &format!("{}s ago", self.current_utc.saturating_sub(ts)),
            );
            cy += 24.0;
        }

        // Offset
        if let Some(offset) = ntp.offset_ms {
            self.render_label_value(cmds, x, cy, width, "Clock offset", &format!("{}ms", offset));
            cy += 24.0;
        }

        // Interval
        self.render_label_value(
            cmds,
            x,
            cy,
            width,
            "Sync interval",
            &format!("{} min", ntp.sync_interval_secs / 60),
        );
        cy += 36.0;

        // NTP servers
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "NTP Servers".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 24.0;

        for server in &ntp.servers {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 6.0,
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
            x,
            y: cy,
            text: "Additional Clocks".into(),
            font_size: 15.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        cy += 24.0;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: format!(
                "{}/4 clocks configured",
                self.settings.additional_clocks.len()
            ),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
        });
        cy += 28.0;

        if self.settings.additional_clocks.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 20.0,
                text: "No additional clocks. Add one to track time in another city.".into(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
            });
            return;
        }

        for clock in &self.settings.additional_clocks {
            let tz = self
                .settings
                .available_timezones
                .iter()
                .find(|t| t.tz_id == clock.tz_id);

            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 60.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            // Clock label
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 6.0,
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
                    x: x + width - 100.0,
                    y: cy + 6.0,
                    text: format!("{:02}:{:02}", h, m),
                    font_size: 20.0,
                    color: BLUE,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(100.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: cy + 28.0,
                    text: format!(
                        "{} — {} ({})",
                        tz_info.display_name,
                        tz_info.abbrev_at(self.current_utc),
                        tz_info.offset_string(self.current_utc)
                    ),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                });
            }

            // Visibility indicator
            if !clock.visible {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: cy + 44.0,
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
            max_width: Some(width * 0.4),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.45,
            y,
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

    /// 2024-01-15 12:00:00 UTC — northern winter, southern summer.
    const JAN: u64 = 1_705_320_000;
    /// 2024-07-15 12:00:00 UTC — northern summer, southern winter.
    const JUL: u64 = 1_721_044_800;

    /// Look an entry up in the shipped table rather than constructing one, so
    /// every assertion below is about a rule a user can actually select.
    fn shipped(tz_id: &str) -> TimezoneInfo {
        default_timezones()
            .into_iter()
            .find(|t| t.tz_id == tz_id)
            .unwrap_or_else(|| panic!("{tz_id} should be in the default table"))
    }

    #[test]
    fn test_offset_string_positive() {
        assert_eq!(shipped("Asia/Tokyo").offset_string(JAN), "UTC+09:00");
    }

    #[test]
    fn test_offset_string_negative() {
        assert_eq!(shipped("America/New_York").offset_string(JAN), "UTC-05:00");
    }

    #[test]
    fn test_offset_string_half_hour() {
        assert_eq!(shipped("Asia/Kolkata").offset_string(JAN), "UTC+05:30");
        assert_eq!(shipped("Asia/Kolkata").offset_string(JUL), "UTC+05:30");
    }

    #[test]
    fn test_offset_string_zero() {
        assert_eq!(
            shipped("Atlantic/Reykjavik").offset_string(JUL),
            "UTC+00:00"
        );
    }

    #[test]
    fn test_local_time_basic() {
        // Reykjavik is UTC all year, so its local time is the UTC reading.
        let (h, m) = shipped("Atlantic/Reykjavik").local_time(43200);
        assert_eq!((h, m), (12, 0));
    }

    #[test]
    fn test_local_time_offset() {
        // 00:00:00 UTC -> 09:00 JST.
        assert_eq!(shipped("Asia/Tokyo").local_time(0), (9, 0));
    }

    #[test]
    fn test_local_time_negative_offset() {
        // 03:00 UTC on a January day is 22:00 the previous day in New York,
        // which is on standard time then.
        assert_eq!(
            shipped("America/New_York").local_time(JAN - 32_400),
            (22, 0)
        );
    }

    #[test]
    fn test_default_timezones_count() {
        // Also the guard on the rule strings: an entry whose POSIX `TZ` fails
        // to parse is dropped by `flatten`, so a typo shows up here.
        assert_eq!(default_timezones().len(), 20);
    }

    /// The bug this struct used to have: one stored offset is right for at most
    /// half the year in a zone that observes daylight saving.
    #[test]
    fn test_a_dst_zone_reads_differently_in_january_and_july() {
        let ny = shipped("America/New_York");
        assert_eq!(ny.offset_string(JAN), "UTC-05:00");
        assert_eq!(ny.offset_string(JUL), "UTC-04:00");
        assert_eq!(ny.abbrev_at(JAN), "EST");
        assert_eq!(ny.abbrev_at(JUL), "EDT");
        assert_eq!(ny.local_time(JAN), (7, 0));
        assert_eq!(ny.local_time(JUL), (8, 0));
        assert!(!ny.is_dst_at(JAN));
        assert!(ny.is_dst_at(JUL));
        // The standing property does not change with the instant.
        assert!(ny.observes_dst());
    }

    /// Sydney's daylight-saving window straddles New Year, so it is shifted in
    /// exactly the months New York is not. A table of fixed offsets cannot
    /// express this at all, and a "DST" badge keyed off a static flag would sit
    /// on both cities at once.
    #[test]
    fn test_a_southern_hemisphere_zone_is_shifted_in_january_not_july() {
        let sydney = shipped("Australia/Sydney");
        assert_eq!(sydney.offset_string(JAN), "UTC+11:00");
        assert_eq!(sydney.offset_string(JUL), "UTC+10:00");
        assert_eq!(sydney.abbrev_at(JAN), "AEDT");
        assert_eq!(sydney.abbrev_at(JUL), "AEST");
        assert!(sydney.is_dst_at(JAN));
        assert!(!sydney.is_dst_at(JUL));
        // The point of the pair: on one instant the two hemispheres disagree.
        assert_ne!(
            sydney.is_dst_at(JAN),
            shipped("America/New_York").is_dst_at(JAN)
        );
    }

    /// A zone that does not observe DST must not acquire one from the default
    /// US rules — `Tz::parse` only substitutes those when a DST *name* is
    /// present, and none of these entries has one.
    #[test]
    fn test_fixed_offset_zones_never_shift() {
        for (tz_id, offset) in [
            ("Pacific/Honolulu", "UTC-10:00"),
            ("Europe/Moscow", "UTC+03:00"),
            ("Asia/Dubai", "UTC+04:00"),
            ("Asia/Shanghai", "UTC+08:00"),
            ("Asia/Seoul", "UTC+09:00"),
            // Brazil abolished daylight saving in 2019; the old table still
            // carried `observes_dst: true` for São Paulo.
            ("America/Sao_Paulo", "UTC-03:00"),
        ] {
            let tz = shipped(tz_id);
            assert!(!tz.observes_dst(), "{tz_id} should have no DST rule");
            assert_eq!(tz.offset_string(JAN), offset, "{tz_id} in January");
            assert_eq!(tz.offset_string(JUL), offset, "{tz_id} in July");
        }
    }

    /// Europe changes on the last Sunday of March/October, a fortnight earlier
    /// and a week later than the US — so there are days when the Atlantic gap
    /// is four hours, not five. Getting this right is the whole reason for
    /// carrying transition rules rather than offsets.
    #[test]
    fn test_europe_and_the_us_do_not_change_on_the_same_day() {
        let london = shipped("Europe/London");
        let ny = shipped("America/New_York");
        // 2024-03-12 12:00 UTC: the US has sprung forward (March 10), the UK
        // has not (March 31).
        let between = 1_710_244_800;
        assert!(ny.is_dst_at(between));
        assert!(!london.is_dst_at(between));
        assert_eq!(london.offset_string(between), "UTC+00:00");
        assert_eq!(ny.offset_string(between), "UTC-04:00");
        // By July both have, and the usual five-hour gap is restored.
        assert_eq!(london.offset_string(JUL), "UTC+01:00");
        assert_eq!(ny.offset_string(JUL), "UTC-04:00");
    }

    #[test]
    fn test_a_malformed_rule_is_refused_rather_than_defaulted_to_utc() {
        // Above all a zoneinfo name: silently reading it as UTC would put a
        // wrong time under a label saying "New York".
        for bad in ["America/New_York", "", "Mars", "EST5EDT,garbage", ":::"] {
            assert!(
                TimezoneInfo::new("x", "x", bad, "x").is_none(),
                "{bad:?} should not parse as a POSIX TZ string"
            );
        }
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
        // The default zone is America/New_York, so the aggregate's reading has
        // to move with the season too — this used to be an `is_some()` check,
        // which the fixed-offset version also passed.
        let s = DateTimeSettings::default();
        assert_eq!(s.local_time(JAN), Some((7, 0)));
        assert_eq!(s.local_time(JUL), Some((8, 0)));
    }

    #[test]
    fn test_local_time_is_none_for_a_zone_not_in_the_table() {
        let mut s = DateTimeSettings::default();
        // `set_timezone` refuses it, so the reading stays on the real zone …
        assert!(!s.set_timezone("Mars/Olympus_Mons"));
        assert_eq!(s.local_time(JAN), Some((7, 0)));
        // … and only a field written past the setter can desynchronise it.
        s.timezone = "Mars/Olympus_Mons".to_string();
        assert_eq!(s.local_time(JAN), None);
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
