//! Date, time, and timezone settings panel for the desktop shell.
//!
//! Configures system clock, timezone selection, NTP synchronization,
//! and additional clocks for multiple timezones.
//!
//! **The panel is never on screen** (`known-issues.md`
//! `TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`; by
//! `design-decisions.md` §815 a page you open lives in the Settings app). What
//! the shell *obeys* -- the zone, the taskbar clock's switches, the world
//! clocks -- is `datetimesettings`, re-exported below, which has a file behind
//! it (§875). The NTP and auto-detect state is this panel's own and is saved
//! nowhere: nothing obeys it.

use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// The model, from the crate the shell and the Settings app share. Re-exported
// under this module's old paths so the shell's call sites did not all have to
// learn a new one; the zone engine behind it is `tzrules`, the one the libc's
// `localtime`, osh's `printf '%(…)T'` and the taskbar clock all read time
// through.
pub use datetimesettings::{
    AdditionalClock, DateTimeSettings, MAX_CLOCKS, TimezoneInfo, search_zones, zone, zones,
};

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

/// What the page says beside the sync interval it reads back to you.
///
/// `sync_interval_secs` is read in exactly one place: the `format!` that
/// draws it as minutes. Found by `scripts/find-echoed-settings.py`.
///
/// **`userspace/ntpd` exists and does not read this.** It is a real NTP
/// client, and its poll interval is the protocol's own -- log2 seconds,
/// between 64 and 1024, moved by the server's reachability rather than by a
/// preference. So this setting is inert twice over: nothing carries it to
/// `ntpd`, and `ntpd` would not honour a number outside that range if
/// something did.
///
/// That second half is why the notice says "chooses its own" rather than
/// "not implemented". A reader told only the first half would file a request
/// to wire it up, and the answer would be no.
const NTP_INTERVAL_NOT_APPLIED: &str = "Not applied: the time client chooses \
its own polling interval, as the protocol requires.";

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
    /// The NTP tab's state. The panel's own, not a setting: nothing obeys it
    /// (see `NTP_INTERVAL_NOT_APPLIED`), so it is not in `datetime.yaml`.
    pub ntp: NtpConfig,
    /// The "Auto-detect Timezone" switch. The panel's own for the same reason:
    /// nothing on the machine can detect a zone yet.
    pub auto_timezone: bool,
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
            ntp: NtpConfig::default(),
            auto_timezone: false,
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
        if let Some((hour, minute)) = self
            .settings
            .current_zone()
            .map(|tz| tz.local_time(self.current_utc))
        {
            p.push_surface(cmds, x, cy, width, 80.0, 12.0, Surface::Card);

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

            if let Some(tz) = self.settings.current_zone() {
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
            color: p.ink(p.lavender),
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
        if let Some(tz) = self.settings.current_zone() {
            p.push_surface(cmds, x, cy, width, 44.0, 8.0, Surface::Card);
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
            self.auto_timezone,
        );
        cy += 40.0;

        // Search
        p.push_surface(cmds, x, cy, width, 30.0, 6.0, Surface::Card);
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
                p.subtext0
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
            zones().iter().collect::<Vec<_>>()
        } else {
            search_zones(&self.tz_search)
        };

        for (i, tz) in filtered.iter().enumerate().take(10) {
            let is_selected = self.selected_tz_index == Some(i);
            let is_current = self.settings.zone.as_deref() == Some(tz.tz_id.as_str());

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
                color: if is_current { p.ink(p.accent) } else { p.text },
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
                    color: p.ink(p.yellow),
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
        let ntp = &self.ntp;

        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "Time Synchronization".into(),
            font_size: 15.0,
            color: p.ink(p.lavender),
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 26.0;

        self.render_toggle_row(cmds, p, x, cy, width, "Enable NTP Sync", ntp.enabled);
        cy += 36.0;

        // Status
        let status_color = ntp.status.color(p);
        p.push_surface(cmds, x, cy, width, 36.0, 6.0, Surface::ControlTrack);
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
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: NTP_INTERVAL_NOT_APPLIED.to_owned(),
            font_size: 11.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 28.0;

        // NTP servers
        cmds.push(RenderCommand::Text {
            x,
            y: cy,
            text: "NTP Servers".into(),
            font_size: 15.0,
            color: p.ink(p.lavender),
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 24.0;

        for server in &ntp.servers {
            p.push_surface(cmds, x, cy, width, 28.0, 4.0, Surface::Card);
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
            color: p.ink(p.lavender),
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
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 20.0),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        for clock in &self.settings.additional_clocks {
            let tz = zone(&clock.tz_id);

            p.push_surface(cmds, x, cy, width, 60.0, 8.0, Surface::Card);

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
                    color: p.subtext0,
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
    use appearance::palette_check::assert_drawn_from;
    use appearance::readable_on;

    /// 2024-01-15 12:00:00 UTC — northern winter, southern summer.
    const JAN: u64 = 1_705_320_000;
    /// 2024-07-15 12:00:00 UTC — northern summer, southern winter.
    const JUL: u64 = 1_721_044_800;

    /// The sync interval is drawn with the fact that the client picks its own.
    ///
    /// `userspace/ntpd` exists, is a real NTP client, and does not read this.
    /// Its poll interval is the protocol's own -- log2 seconds, 64 to 1024,
    /// moved by the server's reachability rather than by a preference. The
    /// setting is inert twice over: nothing carries it to `ntpd`, and `ntpd`
    /// would not honour a number outside that range if something did.
    ///
    /// Found by `scripts/find-echoed-settings.py`.
    #[test]
    fn the_sync_interval_is_drawn_with_the_fact_that_the_client_picks_its_own() {
        let mut ui = DateTimeSettingsUI::default();
        // The Sync tab. The default is Date & Time, which draws eleven text
        // commands and none of them an interval -- the control caught that
        // rather than the assertion, which is what a control is for.
        ui.set_tab(DateTimeTab::Ntp);
        ui.ntp.enabled = true;
        let cmds = ui.render(&Palette::for_mode(false), 600.0, 800.0);
        let texts: Vec<String> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        // The control. Without it this passes against a page that draws no
        // such row at all, which is not what is being pinned.
        assert!(
            texts.iter().any(|t| t == "Sync interval"),
            "control: the page must be drawing a sync interval for this test to be \
about anything -- it drew {} text command(s)",
            texts.len()
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Not applied") && t.contains("chooses its own")),
            "the page drew a sync interval and did not say nothing applies it"
        );
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
        ui.ntp.status = NtpStatus::Synchronized;
        ui.ntp.last_sync_at = Some(ui.current_utc - 300);
        ui.ntp.offset_ms = Some(-23);
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
        ui.auto_timezone = true;
        // Left off, so one taskbar-clock switch renders its other arm.
        ui.settings.show_seconds = false;
        ui.ntp.enabled = true;
        ui.ntp.status = NtpStatus::Error;
        ui.ntp.last_sync_at = Some(JUL - 900);
        ui.ntp.offset_ms = Some(-42);
        // Anchorage is row 2; the machine is on New York, row 6. They must not
        // coincide, or no assertion below could tell the zone you are looking
        // at from the zone that is in force. Chosen, not defaulted: the
        // default is the machine's own zone, which no row names.
        ui.selected_tz_index = Some(2);
        assert!(ui.settings.set_zone(Some("America/New_York")));
        assert!(ui.settings.add_clock("Asia/Tokyo", "Office"));
        assert!(ui.settings.add_clock("Europe/London", "Support"));
        assert!(ui.settings.add_clock("Australia/Sydney", "Night shift"));
        // Past `add_clock`, which refuses a zone the table does not have: this
        // is the clock a hand-edited file would produce.
        ui.settings
            .additional_clocks
            .push(AdditionalClock::new("Mars/Olympus", "Rover"));
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
            ui.ntp.status = status;
            ui.ntp.enabled = status != NtpStatus::Disabled;
            out.push((ui, format!("Sync tab, {status:?}")));
        }
        // A sync block that has never synced and has measured no offset: the
        // two label/value rows are skipped.
        let mut fresh = wound(DateTimeTab::Ntp);
        fresh.ntp.last_sync_at = None;
        fresh.ntp.offset_ms = None;
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
            lost.settings.zone = Some("Mars/Olympus".into());
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
            ui.auto_timezone = on;
            ui.ntp.enabled = on;
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
            .filter_map(appearance::painted_rect)
            .filter(|(_, _, _, height, _)| *height == h)
            .map(|t| t.4)
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
                    names[6],
                    p.ink(accent),
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
                vec![p.painted(appearance::Surface::Card)],
                "the clock card ({what})"
            );

            let tz = render(&wound(DateTimeTab::Timezone), &p);
            assert_eq!(
                fills_h(&tz, 44.0),
                vec![p.painted(appearance::Surface::Card)],
                "the current-zone card ({what})"
            );
            assert_eq!(
                fills_h(&tz, 30.0),
                vec![p.painted(appearance::Surface::Card)],
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
                vec![p.painted(appearance::Surface::ControlTrack)],
                "the sync-status card ({what})"
            );
            let servers = fills_h(&ntp, 28.0);
            assert_eq!(
                servers.len(),
                wound(DateTimeTab::Ntp).ntp.servers.len(),
                "one row per configured server ({what})"
            );
            for (i, row) in servers.iter().enumerate() {
                assert_eq!(
                    *row,
                    p.painted(appearance::Surface::Card),
                    "server row {i} ({what})"
                );
            }

            let clocks = render(&wound(DateTimeTab::Clocks), &p);
            let cards = fills_h(&clocks, 60.0);
            assert_eq!(cards.len(), 4, "four world-clock cards ({what})");
            for (i, card) in cards.iter().enumerate() {
                assert_eq!(
                    *card,
                    p.painted(appearance::Surface::Card),
                    "world-clock card {i} ({what})"
                );
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
                ui.ntp.status = status;
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
                    assert_eq!(*badge, p.ink(p.yellow), "DST badge {i} ({what})");
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
                assert_eq!(names[6], p.ink(accent), "the zone in force ({what})");

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
