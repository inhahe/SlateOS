#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::too_many_arguments)]

//! Slate OS World Clock — multi-timezone clock display with analog/digital views.
//!
//! Shows clocks for cities around the world with timezone offset, day/night
//! indicators, time difference from local, and both analog and digital display
//! modes.
//!
//! Time is held as a real epoch instant rather than a time of day, because
//! everything interesting a world clock says needs the date: which offset a
//! zone is on, whether it is already tomorrow there, and how far apart two
//! cities are during the fortnight when one has changed its clocks and the
//! other has not.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// The same zone engine the libc's `localtime`, osh's `printf '%(…)T'`, the
// taskbar clock and the date/time settings panel use.
use tzrules::Tz;

/// Seconds in a day.
const DAY: i64 = 86_400;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Timezone data ──────────────────────────────────────────────────
/// A city and the POSIX `TZ` rule its clock follows.
///
/// The rule, not an offset. This table used to store `offset_minutes: i32` and
/// a hard-coded `abbreviation`, which is a shape that cannot be right: half of
/// these cities observe daylight saving, so their offset *and* their
/// abbreviation both change twice a year. The stored pair was a snapshot of
/// whichever half of the year the table was written in — Sydney was pinned to
/// `AEST` and Sydney is on `AEDT` for a third of the year — and the app whose
/// single purpose is being right about other people's clocks was silently
/// wrong about roughly half of them at any moment.
///
/// Both are now derived from the rule at the instant being displayed.
#[derive(Clone)]
struct TimezoneInfo {
    city: &'static str,
    country: &'static str,
    /// The POSIX `TZ` string tzdata publishes for this zone.
    posix_tz: &'static str,
}

impl TimezoneInfo {
    /// The parsed rule.
    ///
    /// `None` only for a malformed literal in [`TIMEZONES`], which
    /// `test_every_shipped_zone_parses` catches. Callers skip the city rather
    /// than drawing it at UTC under its own name: a world clock showing the
    /// wrong time for Tokyo is worse than one that does not show Tokyo.
    fn rule(&self) -> Option<Tz> {
        Tz::parse(self.posix_tz.as_bytes())
    }
}

/// The cities offered in the picker.
///
/// Each rule is the POSIX `TZ` string tzdata publishes for that zone, so the
/// transition dates are the real ones and differ between regions as they
/// actually do — the US changes on the second Sunday in March, the EU on the
/// last Sunday, Australia and New Zealand in the opposite half of the year, and
/// Egypt on a Thursday at midnight.
const TIMEZONES: &[TimezoneInfo] = &[
    TimezoneInfo {
        city: "London",
        country: "United Kingdom",
        posix_tz: "GMT0BST,M3.5.0/1,M10.5.0",
    },
    TimezoneInfo {
        city: "Paris",
        country: "France",
        posix_tz: "CET-1CEST,M3.5.0,M10.5.0/3",
    },
    TimezoneInfo {
        city: "Berlin",
        country: "Germany",
        posix_tz: "CET-1CEST,M3.5.0,M10.5.0/3",
    },
    TimezoneInfo {
        city: "Moscow",
        country: "Russia",
        posix_tz: "MSK-3",
    },
    TimezoneInfo {
        city: "Dubai",
        country: "UAE",
        posix_tz: "<+04>-4",
    },
    TimezoneInfo {
        city: "Mumbai",
        country: "India",
        posix_tz: "IST-5:30",
    },
    TimezoneInfo {
        city: "Dhaka",
        country: "Bangladesh",
        posix_tz: "<+06>-6",
    },
    TimezoneInfo {
        city: "Bangkok",
        country: "Thailand",
        posix_tz: "<+07>-7",
    },
    TimezoneInfo {
        city: "Singapore",
        country: "Singapore",
        posix_tz: "<+08>-8",
    },
    TimezoneInfo {
        city: "Beijing",
        country: "China",
        posix_tz: "CST-8",
    },
    TimezoneInfo {
        city: "Tokyo",
        country: "Japan",
        posix_tz: "JST-9",
    },
    TimezoneInfo {
        city: "Seoul",
        country: "South Korea",
        posix_tz: "KST-9",
    },
    TimezoneInfo {
        city: "Sydney",
        country: "Australia",
        posix_tz: "AEST-10AEDT,M10.1.0,M4.1.0/3",
    },
    TimezoneInfo {
        city: "Auckland",
        country: "New Zealand",
        posix_tz: "NZST-12NZDT,M9.5.0,M4.1.0/3",
    },
    TimezoneInfo {
        city: "Honolulu",
        country: "USA",
        posix_tz: "HST10",
    },
    TimezoneInfo {
        city: "Anchorage",
        country: "USA",
        posix_tz: "AKST9AKDT,M3.2.0,M11.1.0",
    },
    TimezoneInfo {
        city: "Los Angeles",
        country: "USA",
        posix_tz: "PST8PDT,M3.2.0,M11.1.0",
    },
    TimezoneInfo {
        city: "Denver",
        country: "USA",
        posix_tz: "MST7MDT,M3.2.0,M11.1.0",
    },
    TimezoneInfo {
        city: "Chicago",
        country: "USA",
        posix_tz: "CST6CDT,M3.2.0,M11.1.0",
    },
    TimezoneInfo {
        city: "New York",
        country: "USA",
        posix_tz: "EST5EDT,M3.2.0,M11.1.0",
    },
    TimezoneInfo {
        city: "São Paulo",
        country: "Brazil",
        posix_tz: "<-03>3",
    },
    TimezoneInfo {
        city: "Cairo",
        country: "Egypt",
        posix_tz: "EET-2EEST,M4.5.4/24,M10.5.4/24",
    },
    TimezoneInfo {
        city: "Istanbul",
        country: "Turkey",
        posix_tz: "<+03>-3",
    },
    TimezoneInfo {
        city: "Nairobi",
        country: "Kenya",
        posix_tz: "EAT-3",
    },
    TimezoneInfo {
        city: "Lagos",
        country: "Nigeria",
        posix_tz: "WAT-1",
    },
    TimezoneInfo {
        city: "Kathmandu",
        country: "Nepal",
        posix_tz: "<+0545>-5:45",
    },
    TimezoneInfo {
        city: "Kolkata",
        country: "India",
        posix_tz: "IST-5:30",
    },
    TimezoneInfo {
        city: "Jakarta",
        country: "Indonesia",
        posix_tz: "WIB-7",
    },
    TimezoneInfo {
        city: "Manila",
        country: "Philippines",
        posix_tz: "PST-8",
    },
    TimezoneInfo {
        city: "Taipei",
        country: "Taiwan",
        posix_tz: "CST-8",
    },
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ViewMode {
    Grid,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClockStyle {
    Digital,
    Analog,
}

struct ClockEntry {
    tz_idx: usize,
    pinned: bool,
}

struct WorldClockApp {
    width: f32,
    height: f32,
    /// Simulation time: seconds since the Unix epoch, UTC.
    ///
    /// A full instant, not a time of day. A zone rule cannot be evaluated
    /// without the date, and neither can the question a world clock is for —
    /// "is it already tomorrow in Tokyo?".
    utc_epoch: i64,
    /// Active clocks (indices into TIMEZONES)
    clocks: Vec<ClockEntry>,
    /// Local timezone index (home)
    home_tz_idx: usize,
    view_mode: ViewMode,
    clock_style: ClockStyle,
    /// Timezone picker
    show_picker: bool,
    picker_search: String,
    picker_scroll: usize,
    /// Settings
    use_24h: bool,
    show_seconds: bool,
    /// UI state
    scroll_offset: f32,
    selected_clock: usize,
    status_msg: String,
}

impl WorldClockApp {
    fn new() -> Self {
        let default_clocks = vec![
            ClockEntry {
                tz_idx: 19,
                pinned: true,
            }, // New York
            ClockEntry {
                tz_idx: 0,
                pinned: true,
            }, // London
            ClockEntry {
                tz_idx: 1,
                pinned: false,
            }, // Paris
            ClockEntry {
                tz_idx: 10,
                pinned: true,
            }, // Tokyo
            ClockEntry {
                tz_idx: 12,
                pinned: false,
            }, // Sydney
            ClockEntry {
                tz_idx: 5,
                pinned: false,
            }, // Mumbai
        ];

        Self {
            width: 1100.0,
            height: 750.0,
            // 2024-07-15 12:00:00 UTC. Northern summer, so the default view
            // exercises the daylight-saving path in both hemispheres at once:
            // New York and London are shifted, Sydney and Auckland are not.
            utc_epoch: 1_721_044_800,
            clocks: default_clocks,
            home_tz_idx: 19, // New York as home
            view_mode: ViewMode::Grid,
            clock_style: ClockStyle::Digital,
            show_picker: false,
            picker_search: String::new(),
            picker_scroll: 0,
            use_24h: false,
            show_seconds: true,
            scroll_offset: 0.0,
            selected_clock: 0,
            status_msg: String::from("World Clock"),
        }
    }

    /// Advance the simulated clock.
    ///
    /// No longer wraps at midnight: the clock now runs on a calendar, so
    /// stepping past midnight has to roll the date over — that is precisely
    /// what makes "tomorrow in Tokyo" and the DST transitions observable.
    fn advance_time(&mut self, seconds: u32) {
        self.utc_epoch = self.utc_epoch.saturating_add(i64::from(seconds));
    }

    /// The local instant in `rule`, as seconds since the epoch shifted by the
    /// offset actually in force then.
    fn local_epoch(&self, rule: &Tz) -> i64 {
        self.utc_epoch
            .saturating_add(i64::from(rule.lookup(self.utc_epoch).gmtoff))
    }

    /// Local wall-clock time in `rule`, as (hour, minute, second).
    fn local_hms(&self, rule: &Tz) -> (u32, u32, u32) {
        let day_secs = self.local_epoch(rule).rem_euclid(DAY);
        // `rem_euclid` gives 0..DAY, so all three casts are exact.
        let h = u32::try_from(day_secs / 3600).unwrap_or(0);
        let m = u32::try_from((day_secs % 3600) / 60).unwrap_or(0);
        let s = u32::try_from(day_secs % 60).unwrap_or(0);
        (h, m, s)
    }

    /// The zone abbreviation in force now (`EST` in winter, `EDT` in summer).
    ///
    /// Lossy only for a name that is not UTF-8, which `Tz::parse` cannot
    /// produce — its grammar admits alphanumerics and `+`/`-` only.
    fn abbrev(&self, rule: &Tz) -> String {
        String::from_utf8_lossy(rule.lookup(self.utc_epoch).name.as_bytes()).into_owned()
    }

    /// The rule the home city follows, or UTC if the home index is somehow
    /// stale — the differences below are then all measured from UTC, which is
    /// visibly odd rather than quietly plausible.
    fn home_rule(&self) -> Tz {
        TIMEZONES
            .get(self.home_tz_idx)
            .and_then(TimezoneInfo::rule)
            .unwrap_or(Tz::UTC)
    }

    /// Calendar days this zone is ahead of (or behind) the home city: `-1`,
    /// `0` or `+1`.
    ///
    /// This is the answer the app could not give at all before, because it
    /// held a time of day with no date behind it.
    fn day_delta_from_home(&self, rule: &Tz) -> i64 {
        let home = self.home_rule();
        self.local_epoch(rule).div_euclid(DAY) - self.local_epoch(&home).div_euclid(DAY)
    }

    /// "Tomorrow" / "Yesterday" / "" relative to the home city.
    fn day_label(&self, rule: &Tz) -> &'static str {
        match self.day_delta_from_home(rule) {
            d if d > 0 => "Tomorrow",
            d if d < 0 => "Yesterday",
            _ => "",
        }
    }

    fn format_time(&self, h: u32, m: u32, s: u32) -> String {
        if self.use_24h {
            if self.show_seconds {
                format!("{h:02}:{m:02}:{s:02}")
            } else {
                format!("{h:02}:{m:02}")
            }
        } else {
            let period = if h < 12 { "AM" } else { "PM" };
            let h12 = if h == 0 {
                12
            } else if h > 12 {
                h - 12
            } else {
                h
            };
            if self.show_seconds {
                format!("{h12}:{m:02}:{s:02} {period}")
            } else {
                format!("{h12}:{m:02} {period}")
            }
        }
    }

    /// Format the offset in force in `rule` right now.
    fn format_offset(&self, rule: &Tz) -> String {
        let mins = rule.lookup(self.utc_epoch).gmtoff.div_euclid(60);
        let sign = if mins >= 0 { '+' } else { '-' };
        let abs = mins.unsigned_abs();
        let h = abs / 60;
        let m = abs % 60;
        if m == 0 {
            format!("UTC{sign}{h}")
        } else {
            format!("UTC{sign}{h}:{m:02}")
        }
    }

    fn is_daytime(h: u32) -> bool {
        (6..18).contains(&h)
    }

    fn day_night_color(h: u32) -> Color {
        if (6..18).contains(&h) { YELLOW } else { LAVENDER }
    }

    fn day_night_icon(h: u32) -> &'static str {
        if (6..18).contains(&h) {
            "\u{2600}"
        } else {
            "\u{263D}"
        }
    }

    /// How far ahead of the home city this zone is, right now.
    ///
    /// Both sides are looked up at the current instant, which is the whole
    /// point: New York is five hours behind London for most of the year but
    /// *four* for the fortnight in March after the US has sprung forward and
    /// the EU has not, and again for the week in autumn when the EU falls back
    /// first. A difference of two stored offsets can never show that.
    fn diff_from_home(&self, rule: &Tz) -> String {
        let home = self.home_rule();
        let here = rule.lookup(self.utc_epoch).gmtoff.div_euclid(60);
        let there = home.lookup(self.utc_epoch).gmtoff.div_euclid(60);
        let diff = here.saturating_sub(there);
        let diff_h = diff / 60;
        let diff_m = (diff % 60).abs();
        if diff == 0 {
            String::from("(home)")
        } else if diff_m == 0 {
            if diff > 0 {
                format!("+{diff_h}h")
            } else {
                format!("{diff_h}h")
            }
        } else if diff > 0 {
            format!("+{diff_h}h{diff_m:02}m")
        } else {
            format!("{diff_h}h{diff_m:02}m")
        }
    }

    fn add_clock(&mut self, tz_idx: usize) {
        if self.clocks.iter().any(|c| c.tz_idx == tz_idx) {
            self.status_msg = String::from("City already added");
            return;
        }
        self.clocks.push(ClockEntry {
            tz_idx,
            pinned: false,
        });
        if let Some(tz) = TIMEZONES.get(tz_idx) {
            self.status_msg = format!("Added {}", tz.city);
        }
    }

    fn remove_clock(&mut self, idx: usize) {
        if idx < self.clocks.len() {
            self.clocks.remove(idx);
            if self.selected_clock >= self.clocks.len() && !self.clocks.is_empty() {
                self.selected_clock = self.clocks.len().saturating_sub(1);
            }
            self.status_msg = String::from("Clock removed");
        }
    }

    fn toggle_pin(&mut self, idx: usize) {
        if let Some(entry) = self.clocks.get_mut(idx) {
            entry.pinned = !entry.pinned;
            let state = if entry.pinned { "pinned" } else { "unpinned" };
            self.status_msg = format!("Clock {state}");
        }
    }

    fn set_home(&mut self, idx: usize) {
        if let Some(entry) = self.clocks.get(idx) {
            self.home_tz_idx = entry.tz_idx;
            if let Some(tz) = TIMEZONES.get(entry.tz_idx) {
                self.status_msg = format!("Home set to {}", tz.city);
            }
        }
    }

    fn filtered_timezones(&self) -> Vec<usize> {
        let query = self.picker_search.to_ascii_lowercase();
        TIMEZONES
            .iter()
            .enumerate()
            .filter(|(_, tz)| {
                if query.is_empty() {
                    return true;
                }
                // Match the abbreviation *currently* in force, which is the one
                // shown on screen: if a card reads AEDT, searching "aedt" must
                // find Sydney.
                let abbrev = tz
                    .rule()
                    .map(|r| self.abbrev(&r).to_ascii_lowercase())
                    .unwrap_or_default();
                tz.city.to_ascii_lowercase().contains(&query)
                    || tz.country.to_ascii_lowercase().contains(&query)
                    || abbrev.contains(&query)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn handle_key(&mut self, key: &str, _ctrl: bool, _shift: bool) {
        if self.show_picker {
            match key {
                "Escape" => self.show_picker = false,
                "Backspace" => {
                    self.picker_search.pop();
                    self.picker_scroll = 0;
                }
                "Enter" => {
                    let filtered = self.filtered_timezones();
                    if let Some(&tz_idx) = filtered.first() {
                        self.add_clock(tz_idx);
                        self.show_picker = false;
                    }
                }
                _ => {}
            }
            return;
        }
        match key {
            "Left" | "h"
                if self.selected_clock > 0 => {
                    self.selected_clock -= 1;
                }
            "Right" | "l"
                if self.selected_clock + 1 < self.clocks.len() => {
                    self.selected_clock += 1;
                }
            "Space" => self.advance_time(60),
            "n" => {
                self.show_picker = true;
                self.picker_search.clear();
                self.picker_scroll = 0;
            }
            "Delete" | "x" => {
                let idx = self.selected_clock;
                self.remove_clock(idx);
            }
            "p" => self.toggle_pin(self.selected_clock),
            "Home" => self.set_home(self.selected_clock),
            "g" => self.view_mode = ViewMode::Grid,
            "v" => self.view_mode = ViewMode::List,
            "a" => {
                self.clock_style = match self.clock_style {
                    ClockStyle::Digital => ClockStyle::Analog,
                    ClockStyle::Analog => ClockStyle::Digital,
                };
            }
            "t" => self.use_24h = !self.use_24h,
            "s" => self.show_seconds = !self.show_seconds,
            _ => {}
        }
    }

    fn handle_picker_text(&mut self, text: &str) {
        if self.show_picker {
            self.picker_search.push_str(text);
            self.picker_scroll = 0;
        }
    }

    // ── Layout constants ────────────────────────────────────────────
    const HEADER_H: f32 = 50.0;
    const STATUS_H: f32 = 28.0;
    const CARD_W: f32 = 240.0;
    const CARD_H: f32 = 160.0;
    const CARD_GAP: f32 = 16.0;
    const LIST_ROW_H: f32 = 56.0;

    // ── Rendering ───────────────────────────────────────────────────
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut cmds);

        match self.view_mode {
            ViewMode::Grid => self.render_grid(&mut cmds),
            ViewMode::List => self.render_list(&mut cmds),
        }

        self.render_status(&mut cmds);

        if self.show_picker {
            self.render_picker(&mut cmds);
        }

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: Self::HEADER_H,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 14.0,
            text: String::from("\u{1F30D} World Clock"),
            font_size: 20.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // View/style buttons
        let mut bx = 240.0;
        let btn_h = 30.0;
        let btn_y = 10.0;
        let grid_bg = if self.view_mode == ViewMode::Grid {
            SURFACE1
        } else {
            SURFACE0
        };
        let list_bg = if self.view_mode == ViewMode::List {
            SURFACE1
        } else {
            SURFACE0
        };
        for (label, bg, w) in [("Grid", grid_bg, 50.0_f32), ("List", list_bg, 50.0)] {
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: btn_y,
                width: w,
                height: btn_h,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: btn_y + 7.0,
                text: label.to_string(),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            bx += w + 4.0;
        }

        bx += 12.0;
        let dig_bg = if self.clock_style == ClockStyle::Digital {
            SURFACE1
        } else {
            SURFACE0
        };
        let ana_bg = if self.clock_style == ClockStyle::Analog {
            SURFACE1
        } else {
            SURFACE0
        };
        for (label, bg, w) in [("Digital", dig_bg, 60.0_f32), ("Analog", ana_bg, 60.0)] {
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: btn_y,
                width: w,
                height: btn_h,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 6.0,
                y: btn_y + 7.0,
                text: label.to_string(),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
            bx += w + 4.0;
        }

        bx += 12.0;
        let h24_bg = if self.use_24h { BLUE } else { SURFACE0 };
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: btn_y,
            width: 44.0,
            height: btn_h,
            color: h24_bg,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: bx + 6.0,
            y: btn_y + 7.0,
            text: String::from("24h"),
            font_size: 12.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(36.0),
            overflow: TextOverflow::Ellipsis,
        });
        bx += 52.0;

        // Add city button
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: btn_y,
            width: 80.0,
            height: btn_h,
            color: BLUE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: bx + 10.0,
            y: btn_y + 7.0,
            text: String::from("+ Add City"),
            font_size: 12.0,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(70.0),
            overflow: TextOverflow::Ellipsis,
        });

        // UTC time
        let (uh, um, us) = self.local_hms(&Tz::UTC);
        cmds.push(RenderCommand::Text {
            x: self.width - 180.0,
            y: 16.0,
            text: format!("UTC {uh:02}:{um:02}:{us:02}"),
            font_size: 16.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(170.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let start_y = Self::HEADER_H + 12.0 - self.scroll_offset;
        // `(width - GAP) / (CARD_W + GAP)`, which is `columns_across` over the
        // room left after the margin on each side. The floor of one lives in
        // the return type now rather than in a `.max(1.0)` this site happened
        // to write and `apps/colorpicker` happened not to.
        let cols = guitk::grid::columns_across(
            self.width - 2.0 * Self::CARD_GAP,
            Self::CARD_W,
            Self::CARD_GAP,
        );

        for (i, entry) in self.clocks.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let cx = Self::CARD_GAP + col as f32 * (Self::CARD_W + Self::CARD_GAP);
            let cy = start_y + row as f32 * (Self::CARD_H + Self::CARD_GAP);

            if cy + Self::CARD_H < Self::HEADER_H || cy > self.height {
                continue;
            }

            if let Some(tz) = TIMEZONES.get(entry.tz_idx) {
                let is_selected = i == self.selected_clock;
                self.render_clock_card(cmds, cx, cy, tz, entry, is_selected);
            }
        }
    }

    fn render_clock_card(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        tz: &TimezoneInfo,
        entry: &ClockEntry,
        selected: bool,
    ) {
        // Skip a city whose rule does not parse rather than drawing it at UTC
        // under its own name. `test_every_shipped_zone_parses` means this
        // cannot happen for a shipped entry.
        let Some(rule) = tz.rule() else {
            return;
        };
        let (h, m, s) = self.local_hms(&rule);
        let is_day = Self::is_daytime(h);
        let border_color = if selected {
            BLUE
        } else if is_day {
            YELLOW
        } else {
            LAVENDER
        };
        let card_bg = if is_day {
            Color::from_hex(0x2A2A3E)
        } else {
            SURFACE0
        };

        if selected {
            cmds.push(RenderCommand::StrokeRect {
                x: x - 1.0,
                y: y - 1.0,
                width: Self::CARD_W + 2.0,
                height: Self::CARD_H + 2.0,
                color: BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(9.0),
            });
        }

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: Self::CARD_W,
            height: Self::CARD_H,
            color: card_bg,
            corner_radii: CornerRadii::all(8.0),
        });

        // Day/night indicator strip
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: Self::CARD_W,
            height: 4.0,
            color: border_color,
            corner_radii: CornerRadii {
                top_left: 8.0,
                top_right: 8.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
        });

        // Pin / home indicators
        if entry.pinned {
            cmds.push(RenderCommand::Text {
                x: x + Self::CARD_W - 24.0,
                y: y + 8.0,
                text: String::from("\u{1F4CC}"),
                font_size: 12.0,
                color: PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: Some(20.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        if entry.tz_idx == self.home_tz_idx {
            cmds.push(RenderCommand::Text {
                x: x + Self::CARD_W - 44.0,
                y: y + 8.0,
                text: String::from("\u{1F3E0}"),
                font_size: 12.0,
                color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(20.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // City name and country
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 12.0,
            text: tz.city.to_string(),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(Self::CARD_W - 56.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 32.0,
            text: tz.country.to_string(),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Self::CARD_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        match self.clock_style {
            ClockStyle::Digital => {
                let time_str = self.format_time(h, m, s);
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: y + 56.0,
                    text: time_str,
                    font_size: 28.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(Self::CARD_W - 24.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: x + Self::CARD_W - 36.0,
                    y: y + 60.0,
                    text: Self::day_night_icon(h).to_string(),
                    font_size: 20.0,
                    color: Self::day_night_color(h),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(30.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ClockStyle::Analog => {
                self.render_analog_clock(cmds, x + Self::CARD_W / 2.0, y + 88.0, 35.0, h, m, s);
            }
        }

        // Bottom info
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + Self::CARD_H - 30.0,
            text: format!("{} ({})", self.format_offset(&rule), self.abbrev(&rule)),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Self::CARD_W / 2.0 - 16.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + Self::CARD_W / 2.0 + 8.0,
            y: y + Self::CARD_H - 30.0,
            text: self.diff_from_home(&rule),
            font_size: 11.0,
            color: TEAL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(Self::CARD_W / 2.0 - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        let dn_label = if is_day { "Daytime" } else { "Nighttime" };
        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + Self::CARD_H - 14.0,
            text: dn_label.to_string(),
            font_size: 10.0,
            color: Self::day_night_color(h),
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });
        // The date rollover, which the old time-of-day-only model could not
        // express: the single most useful thing a world clock tells you is
        // that it is already tomorrow somewhere.
        let day_label = self.day_label(&rule);
        if !day_label.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + Self::CARD_W - 76.0,
                y: y + Self::CARD_H - 14.0,
                text: day_label.to_string(),
                font_size: 10.0,
                color: PEACH,
                font_weight: FontWeightHint::Bold,
                max_width: Some(64.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_analog_clock(
        &self,
        cmds: &mut Vec<RenderCommand>,
        cx: f32,
        cy: f32,
        radius: f32,
        h: u32,
        m: u32,
        s: u32,
    ) {
        // Clock face
        cmds.push(RenderCommand::FillRect {
            x: cx - radius,
            y: cy - radius,
            width: radius * 2.0,
            height: radius * 2.0,
            color: CRUST,
            corner_radii: CornerRadii::all(radius),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: cx - radius,
            y: cy - radius,
            width: radius * 2.0,
            height: radius * 2.0,
            color: SURFACE2,
            line_width: 1.5,
            corner_radii: CornerRadii::all(radius),
        });

        // Hour markers
        for i in 0..12_u32 {
            let angle = (i as f32 * 30.0 - 90.0) * core::f32::consts::PI / 180.0;
            let outer_r = radius - 3.0;
            let inner_r = if i % 3 == 0 {
                radius - 10.0
            } else {
                radius - 7.0
            };
            cmds.push(RenderCommand::Line {
                x1: cx + inner_r * angle.cos(),
                y1: cy + inner_r * angle.sin(),
                x2: cx + outer_r * angle.cos(),
                y2: cy + outer_r * angle.sin(),
                color: TEXT_COLOR,
                width: if i % 3 == 0 { 2.0 } else { 1.0 },
            });
        }

        // Hour hand
        let h_angle =
            ((h % 12) as f32 * 30.0 + m as f32 * 0.5 - 90.0) * core::f32::consts::PI / 180.0;
        cmds.push(RenderCommand::Line {
            x1: cx,
            y1: cy,
            x2: cx + radius * 0.5 * h_angle.cos(),
            y2: cy + radius * 0.5 * h_angle.sin(),
            color: TEXT_COLOR,
            width: 3.0,
        });

        // Minute hand
        let m_angle = (m as f32 * 6.0 + s as f32 * 0.1 - 90.0) * core::f32::consts::PI / 180.0;
        cmds.push(RenderCommand::Line {
            x1: cx,
            y1: cy,
            x2: cx + radius * 0.7 * m_angle.cos(),
            y2: cy + radius * 0.7 * m_angle.sin(),
            color: SUBTEXT1,
            width: 2.0,
        });

        // Second hand
        if self.show_seconds {
            let s_angle = (s as f32 * 6.0 - 90.0) * core::f32::consts::PI / 180.0;
            cmds.push(RenderCommand::Line {
                x1: cx,
                y1: cy,
                x2: cx + radius * 0.8 * s_angle.cos(),
                y2: cy + radius * 0.8 * s_angle.sin(),
                color: RED,
                width: 1.0,
            });
        }

        // Center dot
        cmds.push(RenderCommand::FillRect {
            x: cx - 2.0,
            y: cy - 2.0,
            width: 4.0,
            height: 4.0,
            color: TEXT_COLOR,
            corner_radii: CornerRadii::all(2.0),
        });
    }

    fn render_list(&self, cmds: &mut Vec<RenderCommand>) {
        let start_y = Self::HEADER_H + 4.0 - self.scroll_offset;
        // Column headers
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: start_y,
            width: self.width,
            height: 28.0,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        for (hx, label) in [
            (16.0, "City"),
            (200.0, "Time"),
            (380.0, "UTC Offset"),
            (520.0, "Diff"),
            (620.0, "Day/Night"),
        ] {
            cmds.push(RenderCommand::Text {
                x: hx,
                y: start_y + 6.0,
                text: label.to_string(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(150.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        let row_start = start_y + 32.0;
        for (i, entry) in self.clocks.iter().enumerate() {
            let ry = row_start + i as f32 * Self::LIST_ROW_H;
            if ry + Self::LIST_ROW_H < Self::HEADER_H || ry > self.height - Self::STATUS_H {
                continue;
            }
            if let Some((tz, rule)) = TIMEZONES
                .get(entry.tz_idx)
                .and_then(|tz| Some((tz, tz.rule()?)))
            {
                let (h, m, s) = self.local_hms(&rule);
                let is_selected = i == self.selected_clock;
                let bg = if is_selected {
                    SURFACE1
                } else if i % 2 == 0 {
                    SURFACE0
                } else {
                    BASE
                };

                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: ry,
                    width: self.width,
                    height: Self::LIST_ROW_H,
                    color: bg,
                    corner_radii: CornerRadii::ZERO,
                });

                let mut markers = String::new();
                if entry.pinned {
                    markers.push_str("\u{1F4CC} ");
                }
                if entry.tz_idx == self.home_tz_idx {
                    markers.push_str("\u{1F3E0} ");
                }

                cmds.push(RenderCommand::Text {
                    x: 16.0,
                    y: ry + 8.0,
                    text: format!("{markers}{}", tz.city),
                    font_size: 14.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(180.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: 16.0,
                    y: ry + 28.0,
                    text: tz.country.to_string(),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(180.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: 200.0,
                    y: ry + 12.0,
                    text: self.format_time(h, m, s),
                    font_size: 20.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(170.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: 380.0,
                    y: ry + 16.0,
                    text: format!("{} ({})", self.format_offset(&rule), self.abbrev(&rule)),
                    font_size: 13.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(130.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: 520.0,
                    y: ry + 16.0,
                    text: self.diff_from_home(&rule),
                    font_size: 13.0,
                    color: TEAL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(90.0),
                    overflow: TextOverflow::Ellipsis,
                });
                let dn_icon = Self::day_night_icon(h);
                let dn_label = if Self::is_daytime(h) { "Day" } else { "Night" };
                cmds.push(RenderCommand::Text {
                    x: 620.0,
                    y: ry + 16.0,
                    text: format!("{dn_icon} {dn_label}"),
                    font_size: 13.0,
                    color: Self::day_night_color(h),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(100.0),
                    overflow: TextOverflow::Ellipsis,
                });
                let day_label = self.day_label(&rule);
                if !day_label.is_empty() {
                    cmds.push(RenderCommand::Text {
                        x: 740.0,
                        y: ry + 16.0,
                        text: day_label.to_string(),
                        font_size: 13.0,
                        color: PEACH,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(90.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
        }
    }

    fn render_picker(&self, cmds: &mut Vec<RenderCommand>) {
        // Overlay
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        let pw = 420.0_f32;
        let ph = 500.0_f32;
        let px = (self.width - pw) / 2.0;
        let py = (self.height - ph) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: pw,
            height: ph,
            color: MANTLE,
            corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::Text {
            x: px + 16.0,
            y: py + 14.0,
            text: String::from("Add City"),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(pw - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Search input
        cmds.push(RenderCommand::FillRect {
            x: px + 12.0,
            y: py + 44.0,
            width: pw - 24.0,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.picker_search.is_empty() {
            String::from("Search cities...")
        } else {
            format!("{}|", self.picker_search)
        };
        let search_color = if self.picker_search.is_empty() {
            OVERLAY0
        } else {
            TEXT_COLOR
        };
        cmds.push(RenderCommand::Text {
            x: px + 20.0,
            y: py + 52.0,
            text: search_text,
            font_size: 13.0,
            color: search_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(pw - 48.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Timezone list
        let list_y = py + 84.0;
        let list_h = ph - 84.0 - 12.0;
        let item_h = 44.0;
        let filtered = self.filtered_timezones();
        let visible_items = (list_h / item_h) as usize;

        for (vis_i, &tz_idx) in filtered
            .iter()
            .skip(self.picker_scroll)
            .take(visible_items)
            .enumerate()
        {
            if let Some(tz) = TIMEZONES.get(tz_idx) {
                let iy = list_y + vis_i as f32 * item_h;
                let already = self.clocks.iter().any(|c| c.tz_idx == tz_idx);
                let bg = if already { SURFACE1 } else { SURFACE0 };

                cmds.push(RenderCommand::FillRect {
                    x: px + 8.0,
                    y: iy,
                    width: pw - 16.0,
                    height: item_h - 2.0,
                    color: bg,
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: px + 16.0,
                    y: iy + 6.0,
                    text: format!("{}, {}", tz.city, tz.country),
                    font_size: 13.0,
                    color: if already { OVERLAY0 } else { TEXT_COLOR },
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(pw - 40.0),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: px + 16.0,
                    y: iy + 24.0,
                    text: tz.rule().map_or_else(
                        || String::from("(unavailable)"),
                        |r| format!("{} ({})", self.format_offset(&r), self.abbrev(&r)),
                    ),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(200.0),
                    overflow: TextOverflow::Ellipsis,
                });
                if already {
                    cmds.push(RenderCommand::Text {
                        x: px + pw - 70.0,
                        y: iy + 12.0,
                        text: String::from("Added"),
                        font_size: 11.0,
                        color: GREEN,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(50.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
        }
    }

    fn render_status(&self, cmds: &mut Vec<RenderCommand>) {
        let sy = self.height - Self::STATUS_H;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sy,
            width: self.width,
            height: Self::STATUS_H,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 8.0,
            y: sy + 6.0,
            text: self.status_msg.clone(),
            font_size: 12.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(400.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0,
            y: sy + 6.0,
            text: format!("{} clocks", self.clocks.len()),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(110.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

fn main() {
    let _app = WorldClockApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_clocks() {
        let app = WorldClockApp::new();
        assert_eq!(app.clocks.len(), 6);
        assert_eq!(app.home_tz_idx, 19);
    }

    /// The default instant: 2024-07-15 12:00:00 UTC.
    const NOON_JUL: i64 = 1_721_044_800;
    /// The same time of day in January, when the northern zones are on
    /// standard time and the southern ones are not: 2024-01-15 12:00:00 UTC.
    const NOON_JAN: i64 = 1_705_320_000;

    /// The rule for a shipped city. Every assertion below is therefore about a
    /// zone a user can actually pick.
    fn zone(city: &str) -> Tz {
        TIMEZONES
            .iter()
            .find(|t| t.city == city)
            .and_then(TimezoneInfo::rule)
            .unwrap_or_else(|| panic!("{city} should be a shipped zone with a valid rule"))
    }

    #[test]
    fn test_time_for_utc() {
        let app = WorldClockApp::new();
        let (h, m, _) = app.local_hms(&Tz::UTC);
        assert_eq!((h, m), (12, 0));
    }

    #[test]
    fn test_time_for_positive_offset() {
        let app = WorldClockApp::new();
        let (h, _, _) = app.local_hms(&zone("Tokyo"));
        assert_eq!(h, 21);
    }

    #[test]
    fn test_time_for_negative_offset() {
        // 08:00, not 07:00: at the default instant New York is on EDT. The old
        // fixed `-300` table asserted 07:00 here, which is the bug.
        let app = WorldClockApp::new();
        let (h, _, _) = app.local_hms(&zone("New York"));
        assert_eq!(h, 8);
    }

    #[test]
    fn test_time_for_half_hour_offset() {
        let app = WorldClockApp::new();
        let (h, m, _) = app.local_hms(&zone("Kolkata"));
        assert_eq!((h, m), (17, 30));
    }

    #[test]
    fn test_time_rolls_past_midnight() {
        // 23:00 UTC: Moscow is already 02:00 the next morning, and says so.
        let mut app = WorldClockApp::new();
        app.utc_epoch = NOON_JUL + 11 * 3600;
        let (h, _, _) = app.local_hms(&zone("Moscow"));
        assert_eq!(h, 2);
        assert_eq!(app.day_label(&zone("Moscow")), "Tomorrow");
    }

    #[test]
    fn test_time_rolls_before_midnight() {
        // 22:00 UTC with Tokyo as home: Tokyo is 07:00 on the next day, so
        // Honolulu — still on the previous afternoon — reads as yesterday.
        let mut app = WorldClockApp::new();
        app.utc_epoch = NOON_JUL + 10 * 3600;
        app.home_tz_idx = 10; // Tokyo
        let (h, _, _) = app.local_hms(&zone("Honolulu"));
        assert_eq!(h, 12);
        assert_eq!(app.day_label(&zone("Honolulu")), "Yesterday");
        assert_eq!(app.day_label(&zone("Tokyo")), "");
    }

    #[test]
    fn test_advance_time_rolls_the_date_rather_than_wrapping() {
        // The old model wrapped at 86400, which is why it could never say what
        // day it was anywhere.
        let mut app = WorldClockApp::new();
        app.utc_epoch = NOON_JUL + 4 * 3600; // 16:00 UTC
        app.home_tz_idx = 0; // London, on BST at UTC+1
        // 17:00 in London but already 01:00 the next day in Tokyo.
        assert_eq!(app.day_delta_from_home(&zone("Tokyo")), 1);
        app.advance_time(8 * 3600); // → 00:00 UTC, i.e. 01:00 in London
        assert_eq!(app.utc_epoch, NOON_JUL + 12 * 3600);
        // London has crossed midnight too, so Tokyo's *relative* day is back
        // to level — the delta is a comparison, not a running count.
        assert_eq!(app.day_delta_from_home(&zone("Tokyo")), 0);
    }

    // ---- Daylight saving ----

    #[test]
    fn test_a_dst_zone_reads_differently_in_january_and_july() {
        let mut app = WorldClockApp::new();
        let ny = zone("New York");
        app.utc_epoch = NOON_JUL;
        assert_eq!(app.local_hms(&ny).0, 8);
        assert_eq!(app.abbrev(&ny), "EDT");
        assert_eq!(app.format_offset(&ny), "UTC-4");
        app.utc_epoch = NOON_JAN;
        assert_eq!(app.local_hms(&ny).0, 7);
        assert_eq!(app.abbrev(&ny), "EST");
        assert_eq!(app.format_offset(&ny), "UTC-5");
    }

    #[test]
    fn test_the_southern_hemisphere_shifts_in_the_other_half_of_the_year() {
        let mut app = WorldClockApp::new();
        let sydney = zone("Sydney");
        app.utc_epoch = NOON_JAN;
        assert_eq!(app.abbrev(&sydney), "AEDT");
        assert_eq!(app.format_offset(&sydney), "UTC+11");
        app.utc_epoch = NOON_JUL;
        assert_eq!(app.abbrev(&sydney), "AEST");
        assert_eq!(app.format_offset(&sydney), "UTC+10");
        // The old table pinned Sydney to AEST/+10 year-round, so it was an
        // hour wrong for a third of the year and never said "AEDT".
    }

    #[test]
    fn test_the_gap_between_two_cities_narrows_when_only_one_has_changed() {
        // 2024-03-12 12:00 UTC. The US sprang forward on March 10, the UK does
        // not until March 31, so for that fortnight New York is four hours
        // behind London instead of the usual five. Two stored offsets can
        // never produce this.
        let mut app = WorldClockApp::new();
        app.home_tz_idx = 0; // London
        let ny = zone("New York");
        app.utc_epoch = 1_710_244_800;
        assert_eq!(app.diff_from_home(&ny), "-4h");
        app.utc_epoch = NOON_JUL;
        assert_eq!(app.diff_from_home(&ny), "-5h");
        app.utc_epoch = NOON_JAN;
        assert_eq!(app.diff_from_home(&ny), "-5h");
    }

    #[test]
    fn test_home_reads_as_home_whichever_zone_it_is() {
        let mut app = WorldClockApp::new();
        for (idx, city) in [(19, "New York"), (10, "Tokyo"), (13, "Auckland")] {
            app.home_tz_idx = idx;
            assert_eq!(app.diff_from_home(&zone(city)), "(home)");
            assert_eq!(app.day_label(&zone(city)), "");
        }
    }

    #[test]
    fn test_every_shipped_zone_parses() {
        // The guard on the rule literals: a typo shows up here rather than as
        // a city that silently vanishes from the grid.
        for tz in TIMEZONES {
            assert!(
                tz.rule().is_some(),
                "{}: {:?} is not a POSIX TZ string",
                tz.city,
                tz.posix_tz
            );
        }
    }

    #[test]
    fn test_a_fixed_offset_zone_never_shifts() {
        // `Tz::parse` substitutes the US transition rules only when a DST
        // *name* is present, so these must stay put across the year.
        let mut app = WorldClockApp::new();
        for city in ["Tokyo", "Beijing", "Mumbai", "Nairobi", "S\u{e3}o Paulo"] {
            let z = zone(city);
            app.utc_epoch = NOON_JAN;
            let winter = app.format_offset(&z);
            app.utc_epoch = NOON_JUL;
            assert_eq!(app.format_offset(&z), winter, "{city} should not shift");
        }
    }

    #[test]
    fn test_format_time_12h() {
        let app = WorldClockApp::new();
        assert_eq!(app.format_time(0, 0, 0), "12:00:00 AM");
        assert_eq!(app.format_time(12, 0, 0), "12:00:00 PM");
        assert_eq!(app.format_time(13, 30, 0), "1:30:00 PM");
        assert_eq!(app.format_time(23, 59, 59), "11:59:59 PM");
    }

    #[test]
    fn test_format_time_24h() {
        let mut app = WorldClockApp::new();
        app.use_24h = true;
        assert_eq!(app.format_time(0, 0, 0), "00:00:00");
        assert_eq!(app.format_time(13, 30, 0), "13:30:00");
    }

    #[test]
    fn test_format_time_no_seconds() {
        let mut app = WorldClockApp::new();
        app.show_seconds = false;
        assert_eq!(app.format_time(14, 30, 45), "2:30 PM");
    }

    #[test]
    fn test_format_offset() {
        let mut app = WorldClockApp::new();
        // January, so every northern zone is on standard time and the two
        // southern ones are the odd pair out.
        app.utc_epoch = NOON_JAN;
        assert_eq!(app.format_offset(&Tz::UTC), "UTC+0");
        assert_eq!(app.format_offset(&zone("London")), "UTC+0");
        assert_eq!(app.format_offset(&zone("Paris")), "UTC+1");
        assert_eq!(app.format_offset(&zone("New York")), "UTC-5");
        assert_eq!(app.format_offset(&zone("Mumbai")), "UTC+5:30");
        assert_eq!(app.format_offset(&zone("Kathmandu")), "UTC+5:45");
    }

    #[test]
    fn test_is_daytime() {
        assert!(!WorldClockApp::is_daytime(5));
        assert!(WorldClockApp::is_daytime(6));
        assert!(WorldClockApp::is_daytime(12));
        assert!(WorldClockApp::is_daytime(17));
        assert!(!WorldClockApp::is_daytime(18));
    }

    #[test]
    fn test_diff_from_home() {
        let mut app = WorldClockApp::new();
        // July, so home (New York) is on EDT at UTC-4 and the gaps are measured
        // from there rather than from the winter offset baked into the table.
        app.utc_epoch = NOON_JUL;
        assert_eq!(app.diff_from_home(&zone("New York")), "(home)");
        assert_eq!(app.diff_from_home(&zone("London")), "+5h");
        assert_eq!(app.diff_from_home(&zone("Tokyo")), "+13h");
        assert_eq!(app.diff_from_home(&zone("Los Angeles")), "-3h");
        assert_eq!(app.diff_from_home(&zone("Mumbai")), "+9h30m");
    }

    #[test]
    fn test_add_clock() {
        let mut app = WorldClockApp::new();
        let n = app.clocks.len();
        app.add_clock(3);
        assert_eq!(app.clocks.len(), n + 1);
    }

    #[test]
    fn test_add_duplicate_clock() {
        let mut app = WorldClockApp::new();
        let n = app.clocks.len();
        let tz = app.clocks[0].tz_idx;
        app.add_clock(tz);
        assert_eq!(app.clocks.len(), n);
    }

    #[test]
    fn test_remove_clock() {
        let mut app = WorldClockApp::new();
        let n = app.clocks.len();
        app.remove_clock(0);
        assert_eq!(app.clocks.len(), n - 1);
    }

    #[test]
    fn test_remove_adjusts_selection() {
        let mut app = WorldClockApp::new();
        app.selected_clock = app.clocks.len() - 1;
        app.remove_clock(app.clocks.len() - 1);
        assert!(app.selected_clock < app.clocks.len());
    }

    #[test]
    fn test_toggle_pin() {
        let mut app = WorldClockApp::new();
        let was = app.clocks[0].pinned;
        app.toggle_pin(0);
        assert_ne!(app.clocks[0].pinned, was);
    }

    #[test]
    fn test_set_home() {
        let mut app = WorldClockApp::new();
        app.set_home(1);
        assert_eq!(app.home_tz_idx, app.clocks[1].tz_idx);
    }

    #[test]
    fn test_advance_time() {
        let mut app = WorldClockApp::new();
        let t = app.utc_epoch;
        app.advance_time(60);
        assert_eq!(app.utc_epoch, t + 60);
    }

    #[test]
    fn test_filtered_empty_query() {
        let app = WorldClockApp::new();
        assert_eq!(app.filtered_timezones().len(), TIMEZONES.len());
    }

    #[test]
    fn test_filtered_city_search() {
        let mut app = WorldClockApp::new();
        app.picker_search = String::from("tokyo");
        let f = app.filtered_timezones();
        assert!(!f.is_empty());
        assert!(f.iter().any(|&i| TIMEZONES[i].city == "Tokyo"));
    }

    #[test]
    fn test_filtered_country_search() {
        let mut app = WorldClockApp::new();
        app.picker_search = String::from("usa");
        let f = app.filtered_timezones();
        assert!(f.len() >= 4);
    }

    #[test]
    fn test_filtered_no_match() {
        let mut app = WorldClockApp::new();
        app.picker_search = String::from("xyznotacity");
        assert!(app.filtered_timezones().is_empty());
    }

    #[test]
    fn test_handle_key_navigation() {
        let mut app = WorldClockApp::new();
        app.handle_key("Right", false, false);
        assert_eq!(app.selected_clock, 1);
        app.handle_key("Left", false, false);
        assert_eq!(app.selected_clock, 0);
    }

    #[test]
    fn test_handle_key_advance_time() {
        let mut app = WorldClockApp::new();
        let t = app.utc_epoch;
        app.handle_key("Space", false, false);
        assert_eq!(app.utc_epoch, t + 60);
    }

    #[test]
    fn test_handle_key_toggle_24h() {
        let mut app = WorldClockApp::new();
        assert!(!app.use_24h);
        app.handle_key("t", false, false);
        assert!(app.use_24h);
    }

    #[test]
    fn test_handle_key_toggle_seconds() {
        let mut app = WorldClockApp::new();
        assert!(app.show_seconds);
        app.handle_key("s", false, false);
        assert!(!app.show_seconds);
    }

    #[test]
    fn test_handle_key_view_mode() {
        let mut app = WorldClockApp::new();
        app.handle_key("v", false, false);
        assert_eq!(app.view_mode, ViewMode::List);
        app.handle_key("g", false, false);
        assert_eq!(app.view_mode, ViewMode::Grid);
    }

    #[test]
    fn test_handle_key_clock_style() {
        let mut app = WorldClockApp::new();
        assert_eq!(app.clock_style, ClockStyle::Digital);
        app.handle_key("a", false, false);
        assert_eq!(app.clock_style, ClockStyle::Analog);
        app.handle_key("a", false, false);
        assert_eq!(app.clock_style, ClockStyle::Digital);
    }

    #[test]
    fn test_handle_key_open_picker() {
        let mut app = WorldClockApp::new();
        app.handle_key("n", false, false);
        assert!(app.show_picker);
    }

    #[test]
    fn test_handle_key_picker_close() {
        let mut app = WorldClockApp::new();
        app.show_picker = true;
        app.handle_key("Escape", false, false);
        assert!(!app.show_picker);
    }

    #[test]
    fn test_handle_key_delete() {
        let mut app = WorldClockApp::new();
        let n = app.clocks.len();
        app.handle_key("Delete", false, false);
        assert_eq!(app.clocks.len(), n - 1);
    }

    #[test]
    fn test_handle_key_pin() {
        let mut app = WorldClockApp::new();
        let was = app.clocks[0].pinned;
        app.handle_key("p", false, false);
        assert_ne!(app.clocks[0].pinned, was);
    }

    #[test]
    fn test_picker_text_input() {
        let mut app = WorldClockApp::new();
        app.show_picker = true;
        app.handle_picker_text("lon");
        assert_eq!(app.picker_search, "lon");
    }

    #[test]
    fn test_picker_backspace() {
        let mut app = WorldClockApp::new();
        app.show_picker = true;
        app.picker_search = String::from("tok");
        app.handle_key("Backspace", false, false);
        assert_eq!(app.picker_search, "to");
    }

    #[test]
    fn test_picker_enter_adds_first() {
        let mut app = WorldClockApp::new();
        app.show_picker = true;
        app.picker_search = String::from("anchorage");
        let n = app.clocks.len();
        app.handle_key("Enter", false, false);
        assert_eq!(app.clocks.len(), n + 1);
        assert!(!app.show_picker);
    }

    #[test]
    fn test_render_grid() {
        let app = WorldClockApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_list() {
        let mut app = WorldClockApp::new();
        app.view_mode = ViewMode::List;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_analog() {
        let mut app = WorldClockApp::new();
        app.clock_style = ClockStyle::Analog;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_picker() {
        let mut app = WorldClockApp::new();
        app.show_picker = true;
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_timezone_data_valid() {
        assert!(TIMEZONES.len() >= 30);
        for tz in TIMEZONES {
            assert!(!tz.city.is_empty());
            assert!(!tz.country.is_empty());
            assert!(!tz.posix_tz.is_empty());
            // The offset is derived from the rule, so the range check has to be
            // made at an instant — and at both halves of the year, because a
            // zone that is in range in January can be an hour out of it in July.
            let rule = tz.rule().expect("shipped zone must parse");
            for at in [NOON_JAN, NOON_JUL] {
                let mins = rule.lookup(at).gmtoff / 60;
                assert!(
                    (-720..=840).contains(&mins),
                    "{} is {mins} minutes from UTC at {at}",
                    tz.city
                );
            }
        }
    }

    #[test]
    fn test_day_night_icon() {
        assert_eq!(WorldClockApp::day_night_icon(12), "\u{2600}");
        assert_eq!(WorldClockApp::day_night_icon(0), "\u{263D}");
    }

    #[test]
    fn test_kathmandu_offset() {
        // The 45-minute zone is the one most likely to be quietly rounded away
        // by an offset-in-hours shortcut, so it gets its own test.
        let mut app = WorldClockApp::new();
        app.utc_epoch = NOON_JUL; // 12:00 UTC
        let (h, m, _) = app.local_hms(&zone("Kathmandu"));
        assert_eq!(h, 17);
        assert_eq!(m, 45);
    }

    #[test]
    fn test_left_boundary() {
        let mut app = WorldClockApp::new();
        app.selected_clock = 0;
        app.handle_key("Left", false, false);
        assert_eq!(app.selected_clock, 0);
    }

    #[test]
    fn test_right_boundary() {
        let mut app = WorldClockApp::new();
        app.selected_clock = app.clocks.len() - 1;
        app.handle_key("Right", false, false);
        assert_eq!(app.selected_clock, app.clocks.len() - 1);
    }

    #[test]
    fn test_remove_out_of_bounds() {
        let mut app = WorldClockApp::new();
        let n = app.clocks.len();
        app.remove_clock(100);
        assert_eq!(app.clocks.len(), n);
    }

    #[test]
    fn test_set_home_out_of_bounds() {
        let mut app = WorldClockApp::new();
        let home = app.home_tz_idx;
        app.set_home(100);
        assert_eq!(app.home_tz_idx, home);
    }

    #[test]
    fn test_toggle_pin_out_of_bounds() {
        let mut app = WorldClockApp::new();
        app.toggle_pin(100); // should not panic
    }

    #[test]
    fn test_multiple_adds() {
        let mut app = WorldClockApp::new();
        app.add_clock(3); // Moscow
        app.add_clock(4); // Dubai
        app.add_clock(6); // Dhaka
        assert_eq!(app.clocks.len(), 9);
    }

    #[test]
    fn test_format_time_12h_noon() {
        let app = WorldClockApp::new();
        assert_eq!(app.format_time(12, 0, 0), "12:00:00 PM");
    }

    #[test]
    fn test_format_time_12h_1am() {
        let app = WorldClockApp::new();
        assert_eq!(app.format_time(1, 0, 0), "1:00:00 AM");
    }
}
