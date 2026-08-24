//! Date, time, and timezone settings panel for the desktop shell.
//!
//! Configures system clock, timezone selection, NTP synchronization,
//! and additional clocks for multiple timezones.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// The same zone engine the libc's `localtime`, osh's `printf '%(…)T'` and the
// taskbar clock use.  This panel is where the machine's zone is *chosen*, so of
// everything on the desktop it is the last place that may hold its own opinion
// about what a zone means.
use tzrules::Tz;

// ============================================================================
// Colour
// ============================================================================
//
// Every colour this panel draws comes from the `&Palette` threaded through
// `render`, so the panel follows the desktop's mode and accent.  Three
// judgements had to be made when the hardcoded hexes came out, because a
// literal carries no role until someone assigns one:
//
// *Four things follow the accent*, at four source sites: the selected tab
// pill, the marker strip beside the zone the machine is actually set to, that
// zone's row label, and the enable switch.  Each is a position or an
// invitation — the two things the accent is for.  Note that a zone row has
// *two* independent kinds of "current": the keyboard cursor, which is a raised
// surface because it says only where you are looking, and the machine's zone,
// which is the accent because it says what is in force.
//
// *Two scales are frozen*, because they report facts rather than offering
// choices, and a fact must not change colour when the desktop's accent does:
// `NtpStatus::color` (four sync states) and the DST badge on a zone whose
// clock is currently shifted.
//
// *A clock face is neither.*  The additional-clock readout used to be blue
// while the main readout on the first tab was body text — the same kind of
// value, two colours, for no stated reason.  A displayed time is a
// measurement, so it follows neither the accent nor a categorical hue; both
// readouts are now `text`, and the emphasis they need is already carried by
// their weight and their size.
//
// The switch knob stays `text` on the accent pill.  That contrast is poor for
// pale accents and is tracked as its own issue; changing it here would be a
// second change hiding inside this one.

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
        // UTC first, and by that name. The table used to have no entry a user
        // could select to mean "no offset": `Atlantic/Reykjavik` reads the same
        // clock, but nobody administering a server, reading a log, or comparing
        // a timestamp with a colleague goes looking for Iceland. It is also the
        // zone the shell falls back to when the configured one cannot be
        // resolved, and a fallback that cannot be named is a state the user
        // cannot deliberately return to.
        TimezoneInfo::new("UTC", "Coordinated Universal Time", "UTC0", "UTC"),
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

    /// The colour this sync state is reported in.
    ///
    /// A fact about the machine, not a choice about the desktop, so the four
    /// states are fixed hues and none of them is the accent: a clock that has
    /// failed to sync is red on a red desktop and red on a green one.
    fn color(self, p: &Palette) -> Color {
        match self {
            Self::Disabled => p.overlay0,
            Self::Syncing => p.yellow,
            Self::Synchronized => p.green,
            Self::Error => p.red,
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

    pub fn render(&self, p: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: 24.0,
            text: "Date & Time".into(),
            font_size: 22.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 48.0),
            overflow: TextOverflow::Ellipsis,
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
                color: if active { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 10.0,
                y: tab_y + 8.0,
                text: tab.label().into(),
                font_size: 13.0,
                color: if active { p.on_accent() } else { p.subtext0 },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tw - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            tx += tw + 8.0;
        }

        let cy = tab_y + 48.0;
        let cw = width - 48.0;

        match self.active_tab {
            DateTimeTab::DateTime => self.render_datetime_tab(&mut cmds, p, 24.0, cy, cw),
            DateTimeTab::Timezone => self.render_timezone_tab(&mut cmds, p, 24.0, cy, cw),
            DateTimeTab::Ntp => self.render_ntp_tab(&mut cmds, p, 24.0, cy, cw),
            DateTimeTab::Clocks => self.render_clocks_tab(&mut cmds, p, 24.0, cy, cw),
        }

        cmds
    }

    fn render_datetime_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;

        // Current time display
        if let Some((hour, minute)) = self.settings.local_time(self.current_utc) {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 80.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(12.0),
            });

            let time_str = format!("{:02}:{:02}", hour, minute);
            cmds.push(RenderCommand::Text {
                x: x + width * 0.5 - 60.0,
                y: cy + 12.0,
                text: time_str,
                font_size: 36.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
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
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
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
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Show Seconds",
            self.settings.show_seconds,
        );
        cy += 32.0;
        self.render_toggle_row(
            cmds,
            p,
            x,
            cy,
            width,
            "Show Day of Week",
            self.settings.show_day_of_week,
        );
        cy += 32.0;
        self.render_toggle_row(cmds, p, x, cy, width, "Show Date", self.settings.show_date);
        let _ = cy;
    }

    fn render_timezone_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;

        // Current timezone
        if let Some(tz) = self.settings.current_timezone() {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 44.0,
                color: p.surface1,
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
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
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
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 52.0;
        }

        // Auto-detect toggle
        self.render_toggle_row(
            cmds,
            p,
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
            color: p.surface0,
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
                p.overlay0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 20.0),
            overflow: TextOverflow::Ellipsis,
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
                color: if is_selected { p.surface1 } else { p.surface0 },
                corner_radii: CornerRadii::all(4.0),
            });

            if is_current {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: cy + 4.0,
                    width: 4.0,
                    height: 28.0,
                    color: p.accent,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: cy + 4.0,
                text: format!("{} — {}", tz.city, tz.display_name),
                font_size: 13.0,
                color: if is_current { p.accent } else { p.text },
                font_weight: if is_current {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(width * 0.65),
                overflow: TextOverflow::Ellipsis,
            });

            cmds.push(RenderCommand::Text {
                x: x + width - 100.0,
                y: cy + 4.0,
                text: tz.offset_string(self.current_utc),
                font_size: 13.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
                overflow: TextOverflow::Ellipsis,
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
                    color: p.yellow,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(40.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            cy += 40.0;
        }
    }

    fn render_ntp_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;
        let ntp = &self.settings.ntp;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Time Synchronization".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_toggle_row(cmds, p, x, cy, width, "Enable NTP Sync", ntp.enabled);
        cy += 36.0;

        // Status
        let status_color = ntp.status.color(p);
        cmds.push(RenderCommand::FillRect {
            x,
            y: cy,
            width,
            height: 36.0,
            color: p.surface0,
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
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 44.0;

        // Last sync
        if let Some(ts) = ntp.last_sync_at {
            self.render_label_value(
                cmds,
                p,
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
            self.render_label_value(
                cmds,
                p,
                x,
                cy,
                width,
                "Clock offset",
                &format!("{}ms", offset),
            );
            cy += 24.0;
        }

        // Interval
        self.render_label_value(
            cmds,
            p,
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
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 24.0;

        for server in &ntp.servers {
            cmds.push(RenderCommand::FillRect {
                x,
                y: cy,
                width,
                height: 28.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 6.0,
                text: server.clone(),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 32.0;
        }
    }

    fn render_clocks_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut cy = y;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Additional Clocks".into(),
            font_size: 15.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
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
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 28.0;

        if self.settings.additional_clocks.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: cy + 20.0,
                text: "No additional clocks. Add one to track time in another city.".into(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
                overflow: TextOverflow::Ellipsis,
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
                color: p.surface0,
                corner_radii: CornerRadii::all(8.0),
            });

            // Clock label
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: cy + 6.0,
                text: clock.label.clone(),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width * 0.5),
                overflow: TextOverflow::Ellipsis,
            });

            // Time display
            if let Some(tz_info) = tz {
                let (h, m) = tz_info.local_time(self.current_utc);
                cmds.push(RenderCommand::Text {
                    x: x + width - 100.0,
                    y: cy + 6.0,
                    text: format!("{:02}:{:02}", h, m),
                    font_size: 20.0,
                    color: p.text,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(100.0),
                    overflow: TextOverflow::Ellipsis,
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
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            // Visibility indicator
            if !clock.visible {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: cy + 44.0,
                    text: "Hidden".into(),
                    font_size: 10.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(60.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }

            cy += 68.0;
        }
    }

    fn render_toggle_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
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
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 80.0),
            overflow: TextOverflow::Ellipsis,
        });
        let sw_x = x + width - 44.0;
        cmds.extend(crate::switch::switch(
            sw_x,
            y + 2.0,
            40.0,
            22.0,
            enabled,
            if enabled { p.accent } else { p.surface2 },
        ));
    }

    fn render_label_value(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
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
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.45,
            y,
            text: value.into(),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.55),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

impl Default for DateTimeSettingsUI {
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
    // These tests assert a float equals the exact literal the code under test
    // was handed. That is the assertion meant: a tolerance would let a value
    // that has drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::draw_check::assert_nothing_is_drawn_and_never_seen;
    use crate::palette_check::assert_drawn_from;
    use appearance::readable_on;

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
        assert_eq!(default_timezones().len(), 21);
    }

    /// The picker must offer UTC under its own name, not only as Iceland.
    #[test]
    fn the_picker_offers_utc_by_name() {
        let utc = shipped("UTC");
        assert_eq!(utc.offset_secs_at(JAN), 0);
        assert_eq!(utc.offset_secs_at(JUL), 0, "UTC does not observe DST");
        assert_eq!(utc.local_time(43_200), (12, 0));

        // A search for it must find it, since the picker is search-driven.
        let settings = DateTimeSettings::default();
        assert!(
            settings
                .search_timezones("utc")
                .iter()
                .any(|t| t.tz_id == "UTC"),
            "typing the most common name for the zone must offer the zone"
        );
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
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_timezone() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Timezone);
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_ntp() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Ntp);
        ui.settings.ntp.status = NtpStatus::Synchronized;
        ui.settings.ntp.last_sync_at = Some(ui.current_utc - 300);
        ui.settings.ntp.offset_ms = Some(-23);
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_clocks_empty() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Clocks);
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_clocks_with_entries() {
        let mut ui = DateTimeSettingsUI::new();
        ui.set_tab(DateTimeTab::Clocks);
        ui.settings.add_clock("Asia/Tokyo", "Tokyo");
        ui.settings.add_clock("Europe/London", "London");
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
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

    // ==== The palette conversion ============================================

    /// A panel with every branch wound up: the machine on a zone that is both
    /// listed and currently shifted, the keyboard cursor parked on a
    /// *different* zone, four world clocks of which one is hidden and one
    /// names a zone the table cannot resolve, and a sync block that has a
    /// last-sync time, a measured offset and a non-default state.
    ///
    /// July rather than January, so the northern-hemisphere zones at the top
    /// of the shipped table are shifted and the DST badge actually renders.
    fn wound(tab: DateTimeTab) -> DateTimeSettingsUI {
        let mut ui = DateTimeSettingsUI::new();
        ui.active_tab = tab;
        ui.current_utc = JUL;
        ui.settings.auto_timezone = true;
        // Left off, so one taskbar-clock switch renders its other arm.
        ui.settings.show_seconds = false;
        ui.settings.ntp.enabled = true;
        ui.settings.ntp.status = NtpStatus::Error;
        ui.settings.ntp.last_sync_at = Some(JUL - 900);
        ui.settings.ntp.offset_ms = Some(-42);
        // Anchorage is row 2; the machine is on New York, row 6. They must not
        // coincide, or no assertion below could tell the zone you are looking
        // at from the zone that is in force.
        ui.selected_tz_index = Some(2);
        ui.settings.add_clock("Asia/Tokyo", "Office");
        ui.settings.add_clock("Europe/London", "Support");
        ui.settings.add_clock("Australia/Sydney", "Night shift");
        ui.settings.add_clock("Mars/Olympus", "Rover");
        ui.settings.additional_clocks[2].visible = false;
        ui
    }

    /// Every state the panel can be in that draws something a previous state
    /// did not, so a leftover constant on any branch is rendered at least once.
    fn every_state() -> Vec<(DateTimeSettingsUI, String)> {
        let mut out = Vec::new();
        for tab in [
            DateTimeTab::DateTime,
            DateTimeTab::Timezone,
            DateTimeTab::Ntp,
            DateTimeTab::Clocks,
        ] {
            out.push((wound(tab), format!("{tab:?} tab, wound")));
        }
        // Each sync state is a different hue, so each is its own render.
        for status in [
            NtpStatus::Disabled,
            NtpStatus::Syncing,
            NtpStatus::Synchronized,
            NtpStatus::Error,
        ] {
            let mut ui = wound(DateTimeTab::Ntp);
            ui.settings.ntp.status = status;
            ui.settings.ntp.enabled = status != NtpStatus::Disabled;
            out.push((ui, format!("Sync tab, {status:?}")));
        }
        // A sync block that has never synced and has measured no offset: the
        // two label/value rows are skipped.
        let mut fresh = wound(DateTimeTab::Ntp);
        fresh.settings.ntp.last_sync_at = None;
        fresh.settings.ntp.offset_ms = None;
        out.push((fresh, "Sync tab, never synced".into()));
        // January, when the northern zones are not shifted.
        let mut winter = wound(DateTimeTab::Timezone);
        winter.current_utc = JAN;
        out.push((winter, "Timezone tab, January".into()));
        // A search that matches nothing, and one that matches a few.
        let mut none = wound(DateTimeTab::Timezone);
        none.tz_search = "zzzz".into();
        none.selected_tz_index = None;
        out.push((none, "Timezone tab, search matches nothing".into()));
        let mut some = wound(DateTimeTab::Timezone);
        some.tz_search = "america".into();
        out.push((some, "Timezone tab, search matches some".into()));
        // A zone the table cannot resolve: the clock card and the
        // current-zone card are both skipped.
        for tab in [DateTimeTab::DateTime, DateTimeTab::Timezone] {
            let mut lost = wound(tab);
            lost.settings.timezone = "Mars/Olympus".into();
            out.push((lost, format!("{tab:?} tab, unresolvable zone")));
        }
        // No world clocks at all: the empty caption instead of the cards.
        let mut empty = wound(DateTimeTab::Clocks);
        empty.settings.additional_clocks.clear();
        out.push((empty, "Clocks tab, none configured".into()));
        // Every taskbar-clock switch off, then every one on.
        for on in [false, true] {
            let mut ui = wound(DateTimeTab::DateTime);
            ui.settings.show_seconds = on;
            ui.settings.show_day_of_week = on;
            ui.settings.show_date = on;
            ui.settings.auto_timezone = on;
            ui.settings.ntp.enabled = on;
            out.push((ui, format!("Date & Time tab, all switches {on}")));
        }
        out
    }

    fn render(ui: &DateTimeSettingsUI, p: &Palette) -> Vec<RenderCommand> {
        ui.render(p, 600.0, 800.0)
    }

    // ---- Extractors --------------------------------------------------------
    //
    // Every one is scoped to a single tab's render on purpose. This module's
    // geometry collides *across* tabs rather than within one: a full-width fill
    // 36 tall is a timezone row on the Timezone tab and the sync-status card on
    // the Sync tab, and a text at size 10 is the DST badge on one tab and the
    // "Hidden" mark on the other. A shape is only unambiguous once you know
    // which tab drew it, so every caller below names the tab it rendered.

    /// Every fill exactly `h` tall, in draw order.
    fn fills_h(cmds: &[RenderCommand], h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { height, color, .. } if *height == h => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every fill exactly `w` by `h`, in draw order.
    fn fills_wh(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every text drawn at `size`, in draw order.
    fn texts_at(cmds: &[RenderCommand], size: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    font_size, color, ..
                } if *font_size == size => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every text whose content is exactly `s`.
    fn texts_named(cmds: &[RenderCommand], s: &str) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == s => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The tab strip's labels, in draw order.
    ///
    /// The strip sits at y 60 and its labels at y 68; every tab body starts at
    /// y 108, so nothing else can be mistaken for one.
    fn tab_labels(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { y: 68.0, color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// A timezone row's name, in draw order.
    ///
    /// Keyed on x rather than on size: the row name, the offset beside it and
    /// the search field are all 13pt. A row's name is the only text the panel
    /// draws at x 40 (the tab bodies start at x 24, and the tab labels sit at
    /// x 34 and beyond).
    fn row_labels(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x: 40.0, color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Where `tab` sits in the strip.
    fn tab_index(tab: DateTimeTab) -> usize {
        match tab {
            DateTimeTab::DateTime => 0,
            DateTimeTab::Timezone => 1,
            DateTimeTab::Ntp => 2,
            DateTimeTab::Clocks => 3,
        }
    }

    /// Accents that are not themselves a role this panel freezes.
    ///
    /// `NtpStatus::color` answers green, yellow, red and overlay0, so an
    /// accent drawn from that set would make `assert_ne!(state, accent)` fail
    /// on a panel that is behaving correctly. These seven avoid it in both
    /// modes: the palette's own green/yellow/red move between Mocha and Latte,
    /// but none of them is ever one of these.
    const SAFE_ACCENTS: [Color; 7] = [
        appearance::BLUE,
        appearance::PEACH,
        appearance::MAUVE,
        appearance::TEAL,
        appearance::PINK,
        appearance::SAPPHIRE,
        appearance::SKY,
    ];

    // ---- The membership sweep ----------------------------------------------

    /// Nothing the panel draws is outside its palette.
    ///
    /// Every constant this module used to hold was a Catppuccin *Mocha* value,
    /// so the light render is where a survivor gives itself away — Latte does
    /// not contain it, and the failure names the colour back.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (ui, what) in every_state() {
                // A switch knob is `readable_on` its own track — one of the two
                // extremes, not a role. The tracks are named rather than the
                // extremes, so the exemption stays tied to the fill it sits on.
                assert_drawn_from(
                    &p,
                    &render(&ui, &p),
                    &[p.on_accent(), readable_on(p.surface2)],
                    &format!("{what}, light={light}"),
                );
            }
        }
    }

    /// Nothing is painted and then erased before anyone could see it.
    #[test]
    fn the_panel_draws_nothing_that_is_immediately_erased() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (ui, what) in every_state() {
                assert_nothing_is_drawn_and_never_seen(
                    &render(&ui, &p),
                    &format!("{what}, light={light}"),
                );
            }
        }
    }

    // ---- What follows the accent, and what must not ------------------------

    /// Every colour the panel draws that is *not* one of the accent controls.
    ///
    /// Excludes the four accent shapes and the timezone row names (one of
    /// which is the accent). It deliberately *keeps* the tab labels, which are
    /// `on_accent()`: both accents the frozen-union test below uses are pale,
    /// so `readable_on` answers the same near-black for each and the label
    /// belongs in the union. Adding a dark accent to that pair means moving
    /// the labels into this exclusion first.
    fn colors_apart_from_the_controls(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } => {
                    let is_control = *height == 32.0
                        || (*width == 4.0 && *height == 28.0)
                        || (*width == 40.0 && *height == 22.0);
                    if is_control { None } else { Some(*color) }
                }
                RenderCommand::Text { x: 40.0, .. } => None,
                RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The four things that follow the accent, one assertion per source site.
    ///
    /// Four sites, four negative assertions: the selected tab's pill, the
    /// marker strip beside the zone in force, that zone's row name, and the
    /// enable switch. The switch is *drawn* from five call sites but *written*
    /// once, and a loop cannot disagree with itself, so it is one assertion
    /// plus a count rather than five assertions.
    ///
    /// Each site is pinned by equality with the accent rather than by
    /// inequality with the literal it used to be. Equality is the stronger of
    /// the two — the loop runs seven accents, and no fixed value satisfies all
    /// seven — and inequality with a literal cannot be written honestly here
    /// anyway: the accent set contains blue, so `assert_ne!(pill, BLUE)` fails
    /// on a correct panel whose accent happens to be blue.
    ///
    /// The closing check is the other half: with two different accents, every
    /// colour outside those four is unchanged. Without it a conversion that
    /// painted the whole panel in the accent would pass every assertion above.
    #[test]
    fn every_control_that_offers_something_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let what = format!("light={light}, accent={accent:?}");

                let tz = render(&wound(DateTimeTab::Timezone), &p);

                let pills = fills_h(&tz, 32.0);
                assert_eq!(pills.len(), 4, "four tabs are drawn ({what})");
                assert_eq!(
                    pills[1], accent,
                    "the selected tab's pill does not follow the accent \
                     ({what}); it used to be a hardcoded blue, and no fixed \
                     value can satisfy this over seven accents"
                );

                let strips = fills_wh(&tz, 4.0, 28.0);
                assert_eq!(
                    strips.len(),
                    1,
                    "exactly one zone is marked as the one in force ({what})"
                );
                assert_eq!(
                    strips[0], accent,
                    "the marker strip does not follow the accent ({what})"
                );

                let names = row_labels(&tz);
                assert_eq!(names.len(), 10, "ten zone rows are named ({what})");
                assert_eq!(
                    names[6], accent,
                    "the name of the zone in force does not follow the accent \
                     ({what})"
                );

                let switches = fills_wh(&tz, 40.0, 22.0);
                assert_eq!(
                    switches.len(),
                    1,
                    "the Timezone tab offers one switch ({what})"
                );
                assert_eq!(
                    switches[0], accent,
                    "the switch does not follow the accent ({what}); it used \
                     to be a hardcoded green"
                );
            }
        }

        // Two accents, one frozen union. Both are pale, which is what lets the
        // `on_accent()` tab labels stay in it — see the extractor's note.
        for light in [false, true] {
            let mut a = Palette::for_mode(light);
            a.accent = appearance::MAUVE;
            let mut b = Palette::for_mode(light);
            b.accent = appearance::TEAL;
            for (ui, what) in every_state() {
                assert_eq!(
                    colors_apart_from_the_controls(&render(&ui, &a)),
                    colors_apart_from_the_controls(&render(&ui, &b)),
                    "something outside the four accent controls moved when the \
                     accent did ({what}, light={light})"
                );
            }
        }
    }

    /// The panel's own surfaces are the palette's, and are the *named* role.
    ///
    /// Stronger than the membership sweep, and stronger in a way that matters:
    /// membership must accept `#11111B` at any alpha because it is one of the
    /// two answers `readable_on` gives, and `#11111B` is also Mocha's `crust`.
    /// Equality with the role the code is supposed to have asked for closes
    /// that hole, and it fails in dark mode too, where membership never could.
    #[test]
    fn the_panels_own_surfaces_come_from_the_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let what = format!("light={light}");

            for tab in [
                DateTimeTab::DateTime,
                DateTimeTab::Timezone,
                DateTimeTab::Ntp,
                DateTimeTab::Clocks,
            ] {
                let cmds = render(&wound(tab), &p);
                let RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } = &cmds[0]
                else {
                    panic!("the first command is not the backdrop ({what}, {tab:?})");
                };
                assert_eq!(
                    (*width, *height),
                    (600.0, 800.0),
                    "the backdrop does not cover the panel ({what}, {tab:?})"
                );
                assert_eq!(*color, p.base, "the backdrop ({what}, {tab:?})");

                for (i, pill) in fills_h(&cmds, 32.0).iter().enumerate() {
                    if i != tab_index(tab) {
                        assert_eq!(*pill, p.surface0, "unselected tab {i} ({what}, {tab:?})");
                    }
                }
            }

            let dt = render(&wound(DateTimeTab::DateTime), &p);
            assert_eq!(
                fills_h(&dt, 80.0),
                vec![p.surface0],
                "the clock card ({what})"
            );

            let tz = render(&wound(DateTimeTab::Timezone), &p);
            assert_eq!(
                fills_h(&tz, 44.0),
                vec![p.surface1],
                "the current-zone card ({what})"
            );
            assert_eq!(
                fills_h(&tz, 30.0),
                vec![p.surface0],
                "the search field ({what})"
            );
            let rows = fills_h(&tz, 36.0);
            assert_eq!(rows.len(), 10, "ten zone rows ({what})");
            for (i, row) in rows.iter().enumerate() {
                let want = if i == 2 { p.surface1 } else { p.surface0 };
                assert_eq!(*row, want, "zone row {i} ({what})");
            }

            let ntp = render(&wound(DateTimeTab::Ntp), &p);
            assert_eq!(
                fills_h(&ntp, 36.0),
                vec![p.surface0],
                "the sync-status card ({what})"
            );
            let servers = fills_h(&ntp, 28.0);
            assert_eq!(
                servers.len(),
                wound(DateTimeTab::Ntp).settings.ntp.servers.len(),
                "one row per configured server ({what})"
            );
            for (i, row) in servers.iter().enumerate() {
                assert_eq!(*row, p.surface0, "server row {i} ({what})");
            }

            let clocks = render(&wound(DateTimeTab::Clocks), &p);
            let cards = fills_h(&clocks, 60.0);
            assert_eq!(cards.len(), 4, "four world-clock cards ({what})");
            for (i, card) in cards.iter().enumerate() {
                assert_eq!(*card, p.surface0, "world-clock card {i} ({what})");
            }
        }
    }

    /// The selected tab's label is chosen from the pill under it.
    ///
    /// A fixed near-black reads on a pale accent and vanishes on a dark one,
    /// and the accent set spans both — Latte's mauve is dark where Mocha's is
    /// pale. `readable_on` of the pill's own colour makes legibility a
    /// property of the pair rather than a coincidence of one palette.
    #[test]
    fn the_selected_tabs_label_is_legible_on_the_pill_beneath_it() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::GREEN,
                appearance::RED,
                appearance::YELLOW,
                appearance::MAUVE,
                appearance::LIGHT_MAUVE,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let what = format!("light={light}, accent={accent:?}");

                for tab in [
                    DateTimeTab::DateTime,
                    DateTimeTab::Timezone,
                    DateTimeTab::Ntp,
                    DateTimeTab::Clocks,
                ] {
                    let i = tab_index(tab);
                    let cmds = render(&wound(tab), &p);
                    let pills = fills_h(&cmds, 32.0);
                    let labels = tab_labels(&cmds);
                    assert_eq!(labels.len(), 4, "four tabs are labelled ({what})");
                    assert_eq!(pills[i], accent, "the selected pill ({what}, {tab:?})");
                    assert_eq!(
                        labels[i],
                        readable_on(accent),
                        "the selected tab's label is not chosen for its own \
                         fill ({what}, {tab:?}); a fixed colour is legible on \
                         one mode's accents and not the other's"
                    );
                    for (j, label) in labels.iter().enumerate() {
                        if j != i {
                            assert_eq!(*label, p.subtext0, "unselected tab {j} ({what})");
                        }
                    }
                }
            }
        }
    }

    /// No sync state follows the accent.
    ///
    /// "The clock failed to sync" is a fact about the machine, not a choice
    /// about the desktop. A state painted in the accent would read as failed
    /// on a red desktop and as healthy on a green one.
    #[test]
    fn no_sync_state_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                for status in [
                    NtpStatus::Disabled,
                    NtpStatus::Syncing,
                    NtpStatus::Synchronized,
                    NtpStatus::Error,
                ] {
                    assert_ne!(
                        status.color(&p),
                        accent,
                        "{status:?} is painted in the accent, so a fact about \
                         the machine follows a choice about the desktop \
                         (light={light}, accent={accent:?})"
                    );
                }
            }
        }
    }

    /// The four sync states never collapse onto each other.
    ///
    /// The sweep above cannot see this: every role is a member of both
    /// palettes, so two states mapped to the same role pass membership while
    /// making the panel unable to say whether the clock is synced or broken.
    #[test]
    fn every_sync_state_stays_distinct_under_every_accent() {
        let states = [
            NtpStatus::Disabled,
            NtpStatus::Syncing,
            NtpStatus::Synchronized,
            NtpStatus::Error,
        ];
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                for (i, a) in states.iter().enumerate() {
                    for b in states.iter().skip(i + 1) {
                        assert_ne!(
                            a.color(&p),
                            b.color(&p),
                            "{a:?} and {b:?} are the same colour \
                             (light={light}, accent={accent:?})"
                        );
                    }
                }
            }
        }
    }

    /// The sync dot reports the state the panel says it is in.
    ///
    /// Ties the render to `NtpStatus::color`, so the two tests above are about
    /// something a user can see rather than about a function nobody calls.
    #[test]
    fn the_sync_dot_reports_the_state_it_is_in() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for status in [
                NtpStatus::Disabled,
                NtpStatus::Syncing,
                NtpStatus::Synchronized,
                NtpStatus::Error,
            ] {
                let mut ui = wound(DateTimeTab::Ntp);
                ui.settings.ntp.status = status;
                assert_eq!(
                    fills_wh(&render(&ui, &p), 12.0, 12.0),
                    vec![status.color(&p)],
                    "the sync dot does not report {status:?} (light={light})"
                );
            }
        }
    }

    /// The DST badge is the palette's yellow and stays there.
    ///
    /// "This zone's clock is shifted right now" is a fact about the zone. It
    /// must not follow the accent, and it must not follow whatever hue the
    /// nearest control happens to use either.
    #[test]
    fn the_dst_badge_does_not_follow_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let what = format!("light={light}, accent={accent:?}");
                let badges = texts_named(&render(&wound(DateTimeTab::Timezone), &p), "DST");
                assert!(
                    !badges.is_empty(),
                    "no listed zone is shifted in the July render, so this \
                     test proves nothing ({what})"
                );
                for (i, badge) in badges.iter().enumerate() {
                    assert_eq!(*badge, p.yellow, "DST badge {i} ({what})");
                }
            }
        }
    }

    /// Both clock faces are the panel's body text.
    ///
    /// This is the judgement the conversion had to make. The world-clock
    /// readout used to be blue while the readout on the first tab was body
    /// text — the same kind of value in two colours, for no stated reason. A
    /// displayed time is a measurement, so it follows neither the accent nor a
    /// categorical hue; the emphasis it needs is already carried by the weight
    /// and the size it has. Both are `text` now, and this test is what stops
    /// them drifting apart again.
    #[test]
    fn both_clock_faces_are_the_panels_body_text() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let what = format!("light={light}, accent={accent:?}");

                let dt = render(&wound(DateTimeTab::DateTime), &p);
                assert_eq!(
                    texts_at(&dt, 36.0),
                    vec![p.text],
                    "the main clock face ({what})"
                );

                let clocks = render(&wound(DateTimeTab::Clocks), &p);
                let faces = texts_at(&clocks, 20.0);
                assert_eq!(
                    faces.len(),
                    3,
                    "three of the four world clocks name a zone the table can \
                     resolve ({what})"
                );
                for (i, face) in faces.iter().enumerate() {
                    assert_eq!(*face, p.text, "world-clock face {i} ({what})");
                }
            }
        }
    }

    /// The zone you are looking at is not the zone that is in force.
    ///
    /// A zone row carries two independent kinds of "current", and they are
    /// marked by different means on purpose: the keyboard cursor is a raised
    /// surface, because it says only where you are looking, and the machine's
    /// zone is the accent, because it says what is actually in force. Nothing
    /// asserted they differed before — both were fixed literals — so an edit
    /// that mapped the cursor to the accent too would have left the panel
    /// unable to say which zone the machine is on.
    #[test]
    fn the_zone_you_are_looking_at_is_not_the_zone_in_force() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let what = format!("light={light}, accent={accent:?}");

                let cmds = render(&wound(DateTimeTab::Timezone), &p);
                let rows = fills_h(&cmds, 36.0);
                let names = row_labels(&cmds);
                assert_eq!(rows.len(), 10, "ten zone rows ({what})");
                assert_eq!(names.len(), 10, "ten zone rows are named ({what})");

                assert_eq!(rows[2], p.surface1, "the cursor's row is raised ({what})");
                assert_eq!(
                    names[2], p.text,
                    "the cursor's row is named in body text ({what})"
                );
                assert_eq!(
                    rows[6], p.surface0,
                    "the zone in force is marked by its name and its strip, \
                     not by raising its row ({what})"
                );
                assert_eq!(names[6], accent, "the zone in force ({what})");

                assert_ne!(
                    rows[2], rows[6],
                    "the row you are looking at and the row in force are the \
                     same colour ({what})"
                );
                assert_ne!(
                    names[2], names[6],
                    "the name you are looking at and the name in force are the \
                     same colour ({what})"
                );
                assert_eq!(
                    fills_wh(&cmds, 4.0, 28.0),
                    vec![accent],
                    "exactly one marker strip, in the accent ({what})"
                );
            }
        }
    }
}
