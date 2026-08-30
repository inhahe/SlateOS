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
//!
//! That instant now comes from the machine's clock and is refreshed once a
//! second, which the app's whole subject matter demands and which it did not
//! do: the epoch was the literal `1_721_044_800`, so the world clock read
//! noon on 15 July 2024 forever, and the only thing that moved it was the
//! space bar.
//!
//! # Two numbers, not one
//!
//! The displayed instant is [`base_epoch`](WorldClockApp::base_epoch) — real
//! now — plus [`offset_secs`](WorldClockApp::offset_secs), a shift the user
//! applies to ask "what time is it there when it is *this* time here?". They
//! have to be separate: a single field would have the once-a-second tick
//! silently throw away every step the user had taken.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// The same zone engine the libc's `localtime`, osh's `printf '%(…)T'`, the
// taskbar clock and the date/time settings panel use.
use tzrules::Tz;

/// Seconds in a day.
const DAY: i64 = 86_400;

/// The size the window opens at.
const DEFAULT_WIDTH: f32 = 1100.0;
const DEFAULT_HEIGHT: f32 = 750.0;

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
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Layout ─────────────────────────────────────────────────────────
const HEADER_H: f32 = 50.0;
const STATUS_H: f32 = 28.0;
const CARD_W: f32 = 240.0;
const CARD_H: f32 = 160.0;
const CARD_GAP: f32 = 16.0;
const LIST_ROW_H: f32 = 56.0;
const LIST_HEADER_H: f32 = 28.0;
const PICKER_ITEM_H: f32 = 44.0;
const PICKER_HEAD_H: f32 = 84.0;

/// The three glyph buttons in a card's or a row's top-right corner.
const GLYPH_W: f32 = 20.0;
const GLYPH_GAP: f32 = 4.0;
/// The room the glyph run occupies: three buttons and two gaps.
const GLYPH_RUN_W: f32 = GLYPH_W * 3.0 + GLYPH_GAP * 2.0;

/// The natural width of the header's button run, at full scale.
///
/// Grid 50, List 50, Digital 60, Analog 60, 24h 44, Sec 44, Add City 80, plus
/// the four-pixel gaps within each group and the twelve-pixel gaps between.
const HEADER_RUN_W: f32 = 436.0;
/// Where the run sits when the window is wide enough to leave it there.
const HEADER_RUN_X: f32 = 240.0;
/// The run shrinks rather than running off the right edge, but not below half
/// size — the buttons are the only way to change view with a pointer, so they
/// are the last thing in the header allowed to go, and they never do.
const HEADER_MIN_SCALE: f32 = 0.5;
/// The UTC readout needs this much before it is drawn at all. It restates
/// what every card's offset line already says, so it is what gets dropped.
const UTC_READOUT_W: f32 = 170.0;

/// Everything the pointer can land on.
///
/// Six header buttons, the three per-clock glyphs and every picker row used to
/// be drawn and be inert: the app had no mouse handling of any kind, so `n`,
/// `p`, `Home` and `x` were the only way to reach half its behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    ViewGrid,
    ViewList,
    StyleDigital,
    StyleAnalog,
    Toggle24h,
    ToggleSeconds,
    AddCity,
    /// A clock card, or a list row — index into `clocks`.
    Clock(usize),
    Pin(usize),
    SetHome(usize),
    Remove(usize),
    /// The "+3h from now" chip in the status bar, which resets the shift.
    ResetShift,
    /// A city row in the picker — index into `TIMEZONES`.
    PickerCity(usize),
    PickerClose,
    /// The dimmed area around the picker. Clicking it closes the picker,
    /// and — being recorded before the panel — never steals a click from it.
    PickerBackdrop,
}

pub type Frame = guitk::frame::Frame<Target>;

/// Where everything goes at one particular window size.
///
/// Rebuilt from the live width and height on every frame and never stored: a
/// remembered layout is a layout that disagrees with the window after the
/// first resize.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    /// Between the header and the status bar. Scrolls.
    pub content: Rect,
    pub status: Rect,
    /// Left edge of the header's button run.
    pub run_x: f32,
    /// How much the run is squeezed: 1.0 when it fits, down to
    /// [`HEADER_MIN_SCALE`].
    pub run_scale: f32,
    /// `None` when the window is too narrow to also show the UTC readout.
    pub utc_readout: Option<Rect>,
}

impl Layout {
    pub fn new(width: f32, height: f32) -> Self {
        let window = Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0));
        let header = Rect::new(0.0, 0.0, window.w, HEADER_H.min(window.h));
        let status_h = STATUS_H.min((window.h - header.h).max(0.0));
        let status = Rect::new(0.0, window.h - status_h, window.w, status_h);
        let content = Rect::new(
            0.0,
            header.bottom(),
            window.w,
            (status.y - header.bottom()).max(0.0),
        );

        // The run stays at its usual x while there is room for it there, then
        // slides left, then — only once it is against the left margin — shrinks.
        let run_x = HEADER_RUN_X.min((window.w - 8.0 - HEADER_RUN_W).max(8.0));
        let run_scale = ((window.w - run_x - 8.0) / HEADER_RUN_W).clamp(HEADER_MIN_SCALE, 1.0);
        let run_right = run_x + HEADER_RUN_W * run_scale;
        let utc_readout = (window.w - run_right - 16.0 >= UTC_READOUT_W)
            .then(|| Rect::new(window.w - UTC_READOUT_W - 10.0, 16.0, UTC_READOUT_W, 20.0));

        Self {
            window,
            header,
            content,
            status,
            run_x,
            run_scale,
            utc_readout,
        }
    }

    /// The x of each list column, as a fraction of the window.
    ///
    /// These were the constants 16, 200, 380, 520, 620 and 740, which is a
    /// layout for exactly one window: at 800px the "Tomorrow" column began 60px
    /// off the right edge, and at 1900px every column huddled in the left half.
    pub fn list_columns(&self) -> [f32; 6] {
        // A floor so the columns stay in reading order in a narrow window even
        // once the text within them has ellipsed away.
        let w = self.window.w.max(680.0);
        [16.0, w * 0.20, w * 0.38, w * 0.52, w * 0.62, w * 0.74]
    }

    /// The picker panel, or `None` if the window cannot hold one.
    ///
    /// It used to be 420x500 regardless, so in a window shorter than 500px the
    /// panel hung off both ends and the list ran past the bottom edge.
    pub fn picker(&self) -> Option<Rect> {
        let pw = 420.0_f32.min(self.window.w - 24.0);
        let ph = 500.0_f32.min(self.window.h - 24.0);
        if pw < 200.0 || ph < PICKER_HEAD_H + PICKER_ITEM_H {
            return None;
        }
        Some(Rect::new(
            (self.window.w - pw) / 2.0,
            (self.window.h - ph) / 2.0,
            pw,
            ph,
        ))
    }

    /// How many city rows the picker shows at this height.
    pub fn picker_visible(&self) -> usize {
        self.picker().map_or(0, |p| {
            (((p.h - PICKER_HEAD_H - 12.0) / PICKER_ITEM_H) as usize).max(1)
        })
    }

    /// How many cards fit across the content area.
    pub fn grid_columns(&self) -> core::num::NonZeroUsize {
        guitk::grid::columns_across(self.content.w - 2.0 * CARD_GAP, CARD_W, CARD_GAP)
    }
}

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

pub struct WorldClockApp {
    width: f32,
    height: f32,
    /// Now: seconds since the Unix epoch, UTC, as the machine last reported it.
    ///
    /// A full instant, not a time of day. A zone rule cannot be evaluated
    /// without the date, and neither can the question a world clock is for —
    /// "is it already tomorrow in Tokyo?".
    ///
    /// Refreshed from the system clock once a second by the tick. It used to be
    /// a constant baked into `new`, which meant the app's answer to "what time
    /// is it in Tokyo" was fixed at the moment the source was written.
    base_epoch: i64,
    /// How far the user has stepped away from now, in seconds.
    ///
    /// Separate from `base_epoch` so the tick can move now without discarding
    /// the step, and so "reset to now" is a single assignment rather than a
    /// second reading of the clock.
    offset_secs: i64,
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
    /// Cleared by a close request, which is what ends `App::on_event`.
    running: bool,
}

impl WorldClockApp {
    /// Build the app at a given size, showing a given instant.
    ///
    /// The instant is a parameter rather than a literal in here, which is the
    /// whole of the fix: `main` passes the system clock, and the tests pass a
    /// fixed midsummer noon so that what they assert about Sydney's offset
    /// does not depend on the day they are run.
    pub fn new(width: f32, height: f32, utc_epoch: i64) -> Self {
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
            width,
            height,
            base_epoch: utc_epoch,
            offset_secs: 0,
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
            running: true,
        }
    }

    /// The instant every clock on screen is showing: now, plus the user's shift.
    pub fn utc_epoch(&self) -> i64 {
        self.base_epoch.saturating_add(self.offset_secs)
    }

    /// Jump to an instant and forget any shift. Used by `main` only through
    /// the constructor; the tests use it to pin midsummer or midwinter.
    pub fn set_epoch(&mut self, utc_epoch: i64) {
        self.base_epoch = utc_epoch;
        self.offset_secs = 0;
    }

    /// Step the shift, forward or back.
    ///
    /// This moves the *shift*, not the clock: the point is to ask "when it is
    /// three hours from now here, what will it be in Tokyo?", and the answer
    /// has to survive the next tick. Stepping does not wrap at midnight — the
    /// clock runs on a calendar, so a step past midnight rolls the date over,
    /// which is precisely what makes "tomorrow in Tokyo" and the daylight
    /// saving transitions observable.
    pub fn advance_time(&mut self, seconds: i64) {
        self.offset_secs = self.offset_secs.saturating_add(seconds);
    }

    /// Drop the shift and go back to showing the real current time.
    pub fn reset_shift(&mut self) {
        self.offset_secs = 0;
        self.status_msg = String::from("Showing now");
    }

    /// The shift as a label, or `None` when there is no shift to report.
    fn shift_label(&self) -> Option<String> {
        if self.offset_secs == 0 {
            return None;
        }
        let mins = self.offset_secs.div_euclid(60);
        let sign = if mins >= 0 { '+' } else { '-' };
        let abs = mins.unsigned_abs();
        let (h, m) = (abs / 60, abs % 60);
        Some(if m == 0 {
            format!("{sign}{h}h from now")
        } else {
            format!("{sign}{h}h{m:02}m from now")
        })
    }

    /// The local instant in `rule`, as seconds since the epoch shifted by the
    /// offset actually in force then.
    fn local_epoch(&self, rule: &Tz) -> i64 {
        let now = self.utc_epoch();
        now.saturating_add(i64::from(rule.lookup(now).gmtoff))
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
        String::from_utf8_lossy(rule.lookup(self.utc_epoch()).name.as_bytes()).into_owned()
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
        self.local_epoch(rule)
            .div_euclid(DAY)
            .saturating_sub(self.local_epoch(&home).div_euclid(DAY))
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
                h.saturating_sub(12)
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
        let mins = rule.lookup(self.utc_epoch()).gmtoff.div_euclid(60);
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
        if (6..18).contains(&h) {
            YELLOW
        } else {
            LAVENDER
        }
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
        let here = rule.lookup(self.utc_epoch()).gmtoff.div_euclid(60);
        let there = home.lookup(self.utc_epoch()).gmtoff.div_euclid(60);
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

    /// Open the city picker, empty and scrolled to the top.
    pub fn open_picker(&mut self) {
        self.show_picker = true;
        self.picker_search.clear();
        self.picker_scroll = 0;
    }

    fn handle_picker_text(&mut self, text: &str) {
        if self.show_picker {
            self.picker_search.push_str(text);
            self.picker_scroll = 0;
        }
    }

    // ── Geometry ────────────────────────────────────────────────────
    /// Adopt a new window size.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        self.clamp_scroll();
        // Growing the window shows more picker rows, which can leave the stored
        // scroll past the new end. The drawing clamps too, but a stored value
        // that only the renderer corrects is a value the next reader gets wrong.
        self.clamp_picker_scroll();
    }

    /// The layout for the size the window is right now.
    pub fn layout(&self) -> Layout {
        Layout::new(self.width, self.height)
    }

    /// The header's buttons, left to right: the gap before each, its label, its
    /// natural width, whether it is the state currently in force, and what a
    /// click on it means.
    ///
    /// The gaps and widths sum to [`HEADER_RUN_W`], which is what
    /// `the_header_run_is_as_wide_as_it_says_it_is` checks — the constant and
    /// this table are two spellings of one number and would otherwise drift.
    fn header_buttons(&self) -> [(f32, &'static str, f32, bool, Target); 7] {
        [
            (
                0.0,
                "Grid",
                50.0,
                self.view_mode == ViewMode::Grid,
                Target::ViewGrid,
            ),
            (
                4.0,
                "List",
                50.0,
                self.view_mode == ViewMode::List,
                Target::ViewList,
            ),
            (
                12.0,
                "Digital",
                60.0,
                self.clock_style == ClockStyle::Digital,
                Target::StyleDigital,
            ),
            (
                4.0,
                "Analog",
                60.0,
                self.clock_style == ClockStyle::Analog,
                Target::StyleAnalog,
            ),
            (12.0, "24h", 44.0, self.use_24h, Target::Toggle24h),
            (4.0, "Sec", 44.0, self.show_seconds, Target::ToggleSeconds),
            (12.0, "+ Add City", 80.0, false, Target::AddCity),
        ]
    }

    /// How tall the scrolling content is in the current view.
    fn content_height(&self, layout: &Layout) -> f32 {
        match self.view_mode {
            ViewMode::Grid => {
                let rows = self.clocks.len().div_ceil(layout.grid_columns().get());
                12.0 + rows as f32 * (CARD_H + CARD_GAP)
            }
            ViewMode::List => 4.0 + LIST_HEADER_H + 4.0 + self.clocks.len() as f32 * LIST_ROW_H,
        }
    }

    /// How far the content can be scrolled before it runs out.
    ///
    /// `scroll_offset` existed and was subtracted by both views, but nothing in
    /// the program ever assigned it: there was no wheel handler and no key
    /// bound to it, so a seventh clock in list view was simply unreachable.
    pub fn max_scroll(&self, layout: &Layout) -> f32 {
        (self.content_height(layout) - layout.content.h).max(0.0)
    }

    fn clamp_scroll(&mut self) {
        let layout = self.layout();
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll(&layout));
    }

    /// Scroll the content, stopping at both ends.
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_offset += dy;
        self.clamp_scroll();
    }

    /// The furthest the picker list can be scrolled.
    ///
    /// `picker_scroll` was only ever assigned zero, so of the thirty-odd cities
    /// in [`TIMEZONES`] the picker could only reach the nine that fit on screen;
    /// typing a search was the sole way to see any of the rest.
    pub fn max_picker_scroll(&self, layout: &Layout) -> usize {
        self.filtered_timezones()
            .len()
            .saturating_sub(layout.picker_visible())
    }

    fn clamp_picker_scroll(&mut self) {
        let layout = self.layout();
        self.picker_scroll = self.picker_scroll.min(self.max_picker_scroll(&layout));
    }

    /// Scroll the picker list by whole rows, stopping at both ends.
    pub fn scroll_picker(&mut self, rows: isize) {
        let layout = self.layout();
        let max = self.max_picker_scroll(&layout);
        let next = isize::try_from(self.picker_scroll)
            .unwrap_or(isize::MAX)
            .saturating_add(rows)
            .max(0);
        self.picker_scroll = usize::try_from(next).unwrap_or(0).min(max);
    }

    /// What the pointer would hit at this point, at the current window size.
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    // ── Rendering ───────────────────────────────────────────────────
    /// Draw the whole window, recording a hit box for everything clickable as
    /// it goes.
    ///
    /// Ink and hit boxes come out of one pass, so a button cannot be drawn
    /// somewhere other than where it can be clicked.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let mut frame = Frame::new(width, height);
        let layout = Layout::new(width, height);

        fill(&mut frame, layout.window, BASE, 0.0);
        self.draw_header(&mut frame, &layout);

        // Clip first, then translate: `Frame` keeps each command's own
        // coordinates and lets the compositor apply the translation, but moves
        // and trims the *hit* boxes — so a card scrolled up behind the header
        // is neither drawn over it nor clickable through it.
        let scroll = self.scroll_offset.clamp(0.0, self.max_scroll(&layout));
        frame.clip(layout.content);
        frame.translate(0.0, -scroll);
        match self.view_mode {
            ViewMode::Grid => self.draw_grid(&mut frame, &layout),
            ViewMode::List => self.draw_list(&mut frame, &layout),
        }
        frame.untranslate();
        frame.unclip();

        self.draw_status(&mut frame, &layout);

        if self.show_picker {
            self.draw_picker(&mut frame, &layout);
        }

        frame
    }

    fn draw_header(&self, frame: &mut Frame, layout: &Layout) {
        fill(frame, layout.header, MANTLE, 0.0);
        label(
            frame,
            16.0,
            14.0,
            "\u{1F30D} World Clock",
            20.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
            Some((layout.run_x - 24.0).max(24.0)),
        );

        let scale = layout.run_scale;
        let mut bx = layout.run_x;
        for (gap, text, natural_w, active, target) in self.header_buttons() {
            bx += gap * scale;
            let w = natural_w * scale;
            let rect = Rect::new(bx, 10.0, w, 30.0);
            let primary = target == Target::AddCity;
            let (bg, fg) = if primary {
                (BLUE, CRUST)
            } else if active {
                (SURFACE1, TEXT_COLOR)
            } else {
                (SURFACE0, TEXT_COLOR)
            };
            fill(frame, rect, bg, 4.0);
            label(
                frame,
                rect.x + 6.0 * scale,
                rect.y + 7.0,
                text,
                12.0,
                fg,
                if primary {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                Some((w - 12.0 * scale).max(1.0)),
            );
            frame.hit(target, rect);
            bx += w;
        }

        if let Some(readout) = layout.utc_readout {
            let (uh, um, us) = self.local_hms(&Tz::UTC);
            label(
                frame,
                readout.x,
                readout.y,
                format!("UTC {uh:02}:{um:02}:{us:02}"),
                16.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some(readout.w),
            );
        }
    }

    fn draw_grid(&self, frame: &mut Frame, layout: &Layout) {
        let cols = layout.grid_columns();
        let area = layout.content;
        // No visibility cull here: the clip already drops both the ink and the
        // hit box of anything outside the content area, and the cull that used
        // to be here compared against `self.height` rather than the bottom of
        // the content, so it kept drawing cards behind the status bar.
        for (i, entry) in self.clocks.iter().enumerate() {
            let (col, row) = (i % cols, i / cols);
            let x = area.x + CARD_GAP + col as f32 * (CARD_W + CARD_GAP);
            let y = area.y + 12.0 + row as f32 * (CARD_H + CARD_GAP);
            if let Some(tz) = TIMEZONES.get(entry.tz_idx) {
                self.draw_clock_card(frame, x, y, tz, entry, i);
            }
        }
    }

    fn draw_clock_card(
        &self,
        frame: &mut Frame,
        x: f32,
        y: f32,
        tz: &TimezoneInfo,
        entry: &ClockEntry,
        index: usize,
    ) {
        // Skip a city whose rule does not parse rather than drawing it at UTC
        // under its own name. `test_every_shipped_zone_parses` means this
        // cannot happen for a shipped entry.
        let Some(rule) = tz.rule() else {
            return;
        };
        let card = Rect::new(x, y, CARD_W, CARD_H);
        let (h, m, s) = self.local_hms(&rule);
        let is_day = Self::is_daytime(h);
        let selected = index == self.selected_clock;
        let strip_color = if selected {
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
            stroke(
                frame,
                Rect::new(x - 1.0, y - 1.0, CARD_W + 2.0, CARD_H + 2.0),
                BLUE,
                2.0,
                9.0,
            );
        }
        fill(frame, card, card_bg, 8.0);
        // The card itself is clickable, and is recorded before the glyph
        // buttons that sit on top of it so that they win the reverse-order hit
        // test rather than the card swallowing them.
        frame.hit(Target::Clock(index), card);

        // Day/night indicator strip
        frame.push(RenderCommand::FillRect {
            x,
            y,
            width: CARD_W,
            height: 4.0,
            color: strip_color,
            corner_radii: CornerRadii {
                top_left: 8.0,
                top_right: 8.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
        });

        self.draw_entry_glyphs(
            frame,
            Rect::new(
                x + CARD_W - 8.0 - GLYPH_RUN_W,
                y + 8.0,
                GLYPH_RUN_W,
                GLYPH_W,
            ),
            entry,
            index,
        );

        label(
            frame,
            x + 12.0,
            y + 12.0,
            tz.city,
            16.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
            Some(CARD_W - 32.0 - GLYPH_RUN_W),
        );
        label(
            frame,
            x + 12.0,
            y + 32.0,
            tz.country,
            11.0,
            SUBTEXT0,
            FontWeightHint::Regular,
            Some(CARD_W - 24.0),
        );

        match self.clock_style {
            ClockStyle::Digital => {
                label(
                    frame,
                    x + 12.0,
                    y + 56.0,
                    self.format_time(h, m, s),
                    28.0,
                    TEXT_COLOR,
                    FontWeightHint::Bold,
                    Some(CARD_W - 24.0),
                );
                label(
                    frame,
                    x + CARD_W - 36.0,
                    y + 60.0,
                    Self::day_night_icon(h),
                    20.0,
                    Self::day_night_color(h),
                    FontWeightHint::Regular,
                    Some(30.0),
                );
            }
            ClockStyle::Analog => {
                self.draw_analog_clock(frame, x + CARD_W / 2.0, y + 88.0, 35.0, h, m, s);
            }
        }

        label(
            frame,
            x + 12.0,
            y + CARD_H - 30.0,
            format!("{} ({})", self.format_offset(&rule), self.abbrev(&rule)),
            11.0,
            OVERLAY0,
            FontWeightHint::Regular,
            Some(CARD_W / 2.0 - 16.0),
        );
        label(
            frame,
            x + CARD_W / 2.0 + 8.0,
            y + CARD_H - 30.0,
            self.diff_from_home(&rule),
            11.0,
            TEAL,
            FontWeightHint::Regular,
            Some(CARD_W / 2.0 - 20.0),
        );
        label(
            frame,
            x + 12.0,
            y + CARD_H - 14.0,
            if is_day { "Daytime" } else { "Nighttime" },
            10.0,
            Self::day_night_color(h),
            FontWeightHint::Regular,
            Some(80.0),
        );
        // The date rollover, which the old time-of-day-only model could not
        // express: the single most useful thing a world clock tells you is
        // that it is already tomorrow somewhere.
        let day_label = self.day_label(&rule);
        if !day_label.is_empty() {
            label(
                frame,
                x + CARD_W - 76.0,
                y + CARD_H - 14.0,
                day_label,
                10.0,
                PEACH,
                FontWeightHint::Bold,
                Some(64.0),
            );
        }
    }

    /// The pin, home and remove buttons a card and a list row both carry.
    ///
    /// All three are drawn whether or not they are in force, dimmed when they
    /// are not. Drawing the pin only when already pinned — which is what the
    /// old card did — gives a control that can be turned off and never on.
    fn draw_entry_glyphs(&self, frame: &mut Frame, run: Rect, entry: &ClockEntry, index: usize) {
        let is_home = entry.tz_idx == self.home_tz_idx;
        let buttons = [
            ("\u{1F4CC}", entry.pinned, PEACH, Target::Pin(index)),
            ("\u{1F3E0}", is_home, GREEN, Target::SetHome(index)),
            ("\u{2715}", false, RED, Target::Remove(index)),
        ];
        for (i, (glyph, on, accent, target)) in buttons.into_iter().enumerate() {
            let rect = Rect::new(
                run.x + i as f32 * (GLYPH_W + GLYPH_GAP),
                run.y,
                GLYPH_W,
                GLYPH_W,
            );
            fill(frame, rect, if on { SURFACE1 } else { SURFACE0 }, 4.0);
            label(
                frame,
                rect.x + 4.0,
                rect.y + 4.0,
                glyph,
                11.0,
                if on { accent } else { OVERLAY0 },
                FontWeightHint::Regular,
                Some(GLYPH_W),
            );
            frame.hit(target, rect);
        }
    }

    fn draw_analog_clock(
        &self,
        frame: &mut Frame,
        cx: f32,
        cy: f32,
        radius: f32,
        h: u32,
        m: u32,
        s: u32,
    ) {
        let face = Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
        fill(frame, face, CRUST, radius);
        stroke(frame, face, SURFACE2, 1.5, radius);

        // Hour markers
        for i in 0..12_u32 {
            let angle = (i as f32 * 30.0 - 90.0) * core::f32::consts::PI / 180.0;
            let quarter = i % 3 == 0;
            let outer_r = radius - 3.0;
            let inner_r = if quarter { radius - 10.0 } else { radius - 7.0 };
            line(
                frame,
                cx + inner_r * angle.cos(),
                cy + inner_r * angle.sin(),
                cx + outer_r * angle.cos(),
                cy + outer_r * angle.sin(),
                TEXT_COLOR,
                if quarter { 2.0 } else { 1.0 },
            );
        }

        let h_angle = ((h % 12) as f32).mul_add(30.0, (m as f32).mul_add(0.5, -90.0))
            * core::f32::consts::PI
            / 180.0;
        line(
            frame,
            cx,
            cy,
            (radius * 0.5).mul_add(h_angle.cos(), cx),
            (radius * 0.5).mul_add(h_angle.sin(), cy),
            TEXT_COLOR,
            3.0,
        );

        let m_angle =
            (m as f32).mul_add(6.0, (s as f32).mul_add(0.1, -90.0)) * core::f32::consts::PI / 180.0;
        line(
            frame,
            cx,
            cy,
            (radius * 0.7).mul_add(m_angle.cos(), cx),
            (radius * 0.7).mul_add(m_angle.sin(), cy),
            SUBTEXT1,
            2.0,
        );

        if self.show_seconds {
            let s_angle = (s as f32).mul_add(6.0, -90.0) * core::f32::consts::PI / 180.0;
            line(
                frame,
                cx,
                cy,
                (radius * 0.8).mul_add(s_angle.cos(), cx),
                (radius * 0.8).mul_add(s_angle.sin(), cy),
                RED,
                1.0,
            );
        }

        fill(
            frame,
            Rect::new(cx - 2.0, cy - 2.0, 4.0, 4.0),
            TEXT_COLOR,
            2.0,
        );
    }

    fn draw_list(&self, frame: &mut Frame, layout: &Layout) {
        let area = layout.content;
        let cols = layout.list_columns();
        let head_y = area.y + 4.0;

        fill(
            frame,
            Rect::new(area.x, head_y, area.w, LIST_HEADER_H),
            CRUST,
            0.0,
        );
        for (hx, text) in
            cols.into_iter()
                .zip(["City", "Time", "UTC Offset", "Diff", "Day/Night", "Date"])
        {
            label(
                frame,
                area.x + hx,
                head_y + 6.0,
                text,
                12.0,
                SUBTEXT0,
                FontWeightHint::Bold,
                Some(150.0),
            );
        }

        let row_start = head_y + LIST_HEADER_H + 4.0;
        for (i, entry) in self.clocks.iter().enumerate() {
            let Some((tz, rule)) = TIMEZONES
                .get(entry.tz_idx)
                .and_then(|tz| Some((tz, tz.rule()?)))
            else {
                continue;
            };
            let ry = row_start + i as f32 * LIST_ROW_H;
            let row = Rect::new(area.x, ry, area.w, LIST_ROW_H);
            let (h, m, s) = self.local_hms(&rule);
            let bg = if i == self.selected_clock {
                SURFACE1
            } else if i % 2 == 0 {
                SURFACE0
            } else {
                BASE
            };
            fill(frame, row, bg, 0.0);
            frame.hit(Target::Clock(i), row);

            let name_w = (cols[1] - cols[0] - 12.0).max(40.0);
            label(
                frame,
                area.x + cols[0],
                ry + 8.0,
                tz.city,
                14.0,
                TEXT_COLOR,
                FontWeightHint::Bold,
                Some(name_w),
            );
            label(
                frame,
                area.x + cols[0],
                ry + 28.0,
                tz.country,
                11.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some(name_w),
            );
            label(
                frame,
                area.x + cols[1],
                ry + 12.0,
                self.format_time(h, m, s),
                20.0,
                TEXT_COLOR,
                FontWeightHint::Bold,
                Some((cols[2] - cols[1] - 10.0).max(40.0)),
            );
            label(
                frame,
                area.x + cols[2],
                ry + 16.0,
                format!("{} ({})", self.format_offset(&rule), self.abbrev(&rule)),
                13.0,
                OVERLAY0,
                FontWeightHint::Regular,
                Some((cols[3] - cols[2] - 10.0).max(40.0)),
            );
            label(
                frame,
                area.x + cols[3],
                ry + 16.0,
                self.diff_from_home(&rule),
                13.0,
                TEAL,
                FontWeightHint::Regular,
                Some((cols[4] - cols[3] - 10.0).max(30.0)),
            );
            let dn_icon = Self::day_night_icon(h);
            let dn_label = if Self::is_daytime(h) { "Day" } else { "Night" };
            label(
                frame,
                area.x + cols[4],
                ry + 16.0,
                format!("{dn_icon} {dn_label}"),
                13.0,
                Self::day_night_color(h),
                FontWeightHint::Regular,
                Some((cols[5] - cols[4] - 10.0).max(30.0)),
            );
            let day_label = self.day_label(&rule);
            if !day_label.is_empty() {
                label(
                    frame,
                    area.x + cols[5],
                    ry + 16.0,
                    day_label,
                    13.0,
                    PEACH,
                    FontWeightHint::Bold,
                    Some(90.0),
                );
            }

            self.draw_entry_glyphs(
                frame,
                Rect::new(
                    row.right() - 12.0 - GLYPH_RUN_W,
                    ry + (LIST_ROW_H - GLYPH_W) / 2.0,
                    GLYPH_RUN_W,
                    GLYPH_W,
                ),
                entry,
                i,
            );
        }
    }

    fn draw_status(&self, frame: &mut Frame, layout: &Layout) {
        let bar = layout.status;
        fill(frame, bar, MANTLE, 0.0);

        let count_x = (bar.right() - 110.0).max(8.0);
        let mut msg_limit = count_x;
        if let Some(shift) = self.shift_label() {
            let chip = Rect::new(count_x - 158.0, bar.y + 4.0, 150.0, 20.0);
            if chip.x > 8.0 {
                fill(frame, chip, SURFACE1, 4.0);
                label(
                    frame,
                    chip.x + 6.0,
                    chip.y + 4.0,
                    format!("{shift} \u{21BA}"),
                    11.0,
                    PEACH,
                    FontWeightHint::Bold,
                    Some(chip.w - 12.0),
                );
                frame.hit(Target::ResetShift, chip);
                msg_limit = chip.x;
            }
        }

        label(
            frame,
            8.0,
            bar.y + 6.0,
            self.status_msg.clone(),
            12.0,
            SUBTEXT1,
            FontWeightHint::Regular,
            Some((msg_limit - 16.0).max(40.0)),
        );
        label(
            frame,
            count_x,
            bar.y + 6.0,
            format!("{} clocks", self.clocks.len()),
            11.0,
            OVERLAY0,
            FontWeightHint::Regular,
            Some(102.0),
        );
    }

    fn draw_picker(&self, frame: &mut Frame, layout: &Layout) {
        fill(frame, layout.window, Color::rgba(0, 0, 0, 160), 0.0);
        // Recorded before the panel, so every control drawn on the panel wins
        // the reverse-order hit test against it. It also covers the header,
        // which is what makes the picker modal.
        frame.hit(Target::PickerBackdrop, layout.window);

        let Some(panel) = layout.picker() else {
            // Too small for a panel. Say so rather than drawing a dialog off
            // both edges, and leave the backdrop clickable so it can be closed.
            label(
                frame,
                16.0,
                layout.window.h / 2.0,
                "Window too small to pick a city",
                14.0,
                TEXT_COLOR,
                FontWeightHint::Bold,
                Some((layout.window.w - 32.0).max(1.0)),
            );
            return;
        };

        fill(frame, panel, MANTLE, 12.0);
        label(
            frame,
            panel.x + 16.0,
            panel.y + 14.0,
            "Add City",
            18.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
            Some(panel.w - 64.0),
        );

        let close = Rect::new(panel.right() - 34.0, panel.y + 10.0, 24.0, 24.0);
        fill(frame, close, SURFACE0, 4.0);
        label(
            frame,
            close.x + 7.0,
            close.y + 5.0,
            "\u{2715}",
            12.0,
            SUBTEXT0,
            FontWeightHint::Regular,
            Some(20.0),
        );
        frame.hit(Target::PickerClose, close);

        let search = Rect::new(panel.x + 12.0, panel.y + 44.0, panel.w - 24.0, 32.0);
        fill(frame, search, SURFACE0, 6.0);
        let (search_text, search_color) = if self.picker_search.is_empty() {
            (String::from("Search cities..."), OVERLAY0)
        } else {
            (format!("{}|", self.picker_search), TEXT_COLOR)
        };
        label(
            frame,
            search.x + 8.0,
            search.y + 8.0,
            search_text,
            13.0,
            search_color,
            FontWeightHint::Regular,
            Some(search.w - 16.0),
        );

        let list = Rect::new(
            panel.x,
            panel.y + PICKER_HEAD_H,
            panel.w,
            panel.h - PICKER_HEAD_H - 12.0,
        );
        let filtered = self.filtered_timezones();
        let visible = layout.picker_visible();
        let scroll = self
            .picker_scroll
            .min(filtered.len().saturating_sub(visible));

        // Clipped, so the last row is trimmed rather than drawn past the panel,
        // and a row trimmed to nothing is not clickable either.
        frame.clip(list);
        for (vis_i, &tz_idx) in filtered.iter().skip(scroll).take(visible).enumerate() {
            let Some(tz) = TIMEZONES.get(tz_idx) else {
                continue;
            };
            let iy = list.y + vis_i as f32 * PICKER_ITEM_H;
            let already = self.clocks.iter().any(|c| c.tz_idx == tz_idx);
            let row = Rect::new(panel.x + 8.0, iy, panel.w - 16.0, PICKER_ITEM_H - 2.0);
            fill(frame, row, if already { SURFACE1 } else { SURFACE0 }, 4.0);
            frame.hit(Target::PickerCity(tz_idx), row);

            label(
                frame,
                row.x + 8.0,
                iy + 6.0,
                format!("{}, {}", tz.city, tz.country),
                13.0,
                if already { OVERLAY0 } else { TEXT_COLOR },
                FontWeightHint::Bold,
                Some(row.w - 80.0),
            );
            label(
                frame,
                row.x + 8.0,
                iy + 24.0,
                tz.rule().map_or_else(
                    || String::from("(unavailable)"),
                    |r| format!("{} ({})", self.format_offset(&r), self.abbrev(&r)),
                ),
                11.0,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some(200.0),
            );
            if already {
                label(
                    frame,
                    row.right() - 58.0,
                    iy + 12.0,
                    "Added",
                    11.0,
                    GREEN,
                    FontWeightHint::Regular,
                    Some(50.0),
                );
            }
        }
        frame.unclip();

        if filtered.len() > visible {
            label(
                frame,
                panel.x + 16.0,
                panel.bottom() - 12.0,
                format!(
                    "{}-{} of {}",
                    scroll.saturating_add(1),
                    scroll.saturating_add(visible).min(filtered.len()),
                    filtered.len()
                ),
                10.0,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(120.0),
            );
        }
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────
fn fill(frame: &mut Frame, r: Rect, color: Color, radius: f32) {
    frame.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    });
}

fn stroke(frame: &mut Frame, r: Rect, color: Color, line_width: f32, radius: f32) {
    frame.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

fn line(frame: &mut Frame, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
    frame.push(RenderCommand::Line {
        x1,
        y1,
        x2,
        y2,
        color,
        width,
    });
}

fn label(
    frame: &mut Frame,
    x: f32,
    y: f32,
    text: impl Into<String>,
    font_size: f32,
    color: Color,
    font_weight: FontWeightHint,
    max_width: Option<f32>,
) {
    frame.push(RenderCommand::Text {
        x,
        y,
        text: text.into(),
        font_size,
        color,
        font_weight,
        max_width,
        overflow: TextOverflow::Ellipsis,
    });
}

// ── Input ───────────────────────────────────────────────────────────
/// The one body both the window and the probe drive.
///
/// `App::on_event` and `Probe::click_at`/`key_at` are adapters over this, so a
/// test cannot exercise a code path the window does not take.
pub fn handle_event(state: &mut WorldClockApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) if key.pressed => handle_key(state, key),
        Event::Mouse(mouse) => handle_mouse(state, mouse),
        Event::Resize { width, height } => {
            state.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // The reason this app has a tick at all: it draws seconds hands.
        Event::Tick { .. } => match now_utc() {
            Some(now) if now != state.base_epoch => {
                state.base_epoch = now;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        },
        Event::CloseRequested => {
            state.running = false;
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

fn handle_key(state: &mut WorldClockApp, key: &KeyEvent) -> EventResult {
    if state.show_picker {
        return handle_picker_key(state, key);
    }
    match key.key {
        Key::Left | Key::H if state.selected_clock > 0 => {
            state.selected_clock = state.selected_clock.saturating_sub(1);
        }
        Key::Right | Key::L if state.selected_clock.saturating_add(1) < state.clocks.len() => {
            state.selected_clock = state.selected_clock.saturating_add(1);
        }
        // An hour, not the old minute. With the clock frozen, a minute step was
        // the only thing that made time pass at all; now the tick does that,
        // and the step's remaining job is "what time is it there at 3pm here?",
        // whose unit is the hour.
        Key::Space if key.modifiers.shift => state.advance_time(-3600),
        Key::Space => state.advance_time(3600),
        Key::R => state.reset_shift(),
        Key::N => state.open_picker(),
        Key::Delete | Key::X => {
            let idx = state.selected_clock;
            state.remove_clock(idx);
            state.clamp_scroll();
        }
        Key::P => state.toggle_pin(state.selected_clock),
        Key::Home => state.set_home(state.selected_clock),
        Key::G => state.view_mode = ViewMode::Grid,
        Key::V => state.view_mode = ViewMode::List,
        Key::A => {
            state.clock_style = match state.clock_style {
                ClockStyle::Digital => ClockStyle::Analog,
                ClockStyle::Analog => ClockStyle::Digital,
            };
        }
        Key::T => state.use_24h = !state.use_24h,
        Key::S => state.show_seconds = !state.show_seconds,
        Key::Up => state.scroll_by(-LIST_ROW_H),
        Key::Down => state.scroll_by(LIST_ROW_H),
        Key::PageUp => state.scroll_by(-state.layout().content.h),
        Key::PageDown => state.scroll_by(state.layout().content.h),
        _ => return EventResult::Ignored,
    }
    EventResult::Consumed
}

fn handle_picker_key(state: &mut WorldClockApp, key: &KeyEvent) -> EventResult {
    match key.key {
        Key::Escape => state.show_picker = false,
        Key::Backspace => {
            state.picker_search.pop();
            state.picker_scroll = 0;
        }
        Key::Enter => {
            if let Some(&tz_idx) = state.filtered_timezones().first() {
                state.add_clock(tz_idx);
                state.show_picker = false;
            }
        }
        Key::Up => state.scroll_picker(-1),
        Key::Down => state.scroll_picker(1),
        Key::PageUp => state.scroll_picker(-4),
        Key::PageDown => state.scroll_picker(4),
        _ => {
            let typed: String = key.typed().collect();
            if typed.is_empty() {
                return EventResult::Ignored;
            }
            state.handle_picker_text(&typed);
        }
    }
    EventResult::Consumed
}

fn handle_mouse(state: &mut WorldClockApp, mouse: &MouseEvent) -> EventResult {
    match mouse.kind {
        MouseEventKind::Press(MouseButton::Left) => {
            let Some(target) = state.target_at(mouse.x, mouse.y) else {
                return EventResult::Ignored;
            };
            match target {
                Target::ViewGrid => state.view_mode = ViewMode::Grid,
                Target::ViewList => state.view_mode = ViewMode::List,
                Target::StyleDigital => state.clock_style = ClockStyle::Digital,
                Target::StyleAnalog => state.clock_style = ClockStyle::Analog,
                Target::Toggle24h => state.use_24h = !state.use_24h,
                Target::ToggleSeconds => state.show_seconds = !state.show_seconds,
                Target::AddCity => state.open_picker(),
                Target::Clock(i) => state.selected_clock = i,
                Target::Pin(i) => {
                    state.selected_clock = i;
                    state.toggle_pin(i);
                }
                Target::SetHome(i) => {
                    state.selected_clock = i;
                    state.set_home(i);
                }
                Target::Remove(i) => {
                    state.remove_clock(i);
                    state.clamp_scroll();
                }
                Target::ResetShift => state.reset_shift(),
                Target::PickerCity(idx) => {
                    state.add_clock(idx);
                    state.clamp_scroll();
                }
                Target::PickerClose | Target::PickerBackdrop => state.show_picker = false,
            }
            // A view or a removal can shorten the content under a scroll that
            // was valid for the old one.
            state.clamp_scroll();
            EventResult::Consumed
        }
        // `dy` is in wheel notches, not pixels, which is why it goes through
        // `wheel` rather than being added to an offset directly. Both converters
        // already answer in *offset* space — `wheel::pixels(-1.0, 20.0) == 60.0`,
        // "one notch down is a larger offset" — so the result is added as it
        // comes. Negating it here is the trap `wheel`'s own docs warn about, and
        // it scrolls the list backwards.
        MouseEventKind::Scroll { dy, .. } => {
            if state.show_picker {
                let rows = wheel::rows_f(dy);
                if rows == 0.0 {
                    return EventResult::Ignored;
                }
                state.scroll_picker(rows as isize);
            } else {
                state.scroll_by(wheel::pixels(dy, LIST_ROW_H));
            }
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

// ── Window ──────────────────────────────────────────────────────────
impl App for WorldClockApp {
    fn title(&self) -> String {
        String::from("World Clock")
    }

    fn app_id(&self) -> String {
        String::from("worldclock")
    }

    fn initial_size(&self) -> (u32, u32) {
        (DEFAULT_WIDTH as u32, DEFAULT_HEIGHT as u32)
    }

    /// Once a second. A clock that draws a seconds hand and a seconds digit
    /// has to redraw at least that often, and this is the interval that used
    /// not to exist at all.
    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_secs(1))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        let result = handle_event(self, event);
        if !self.running {
            return Response::Exit;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for WorldClockApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (DEFAULT_WIDTH, DEFAULT_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

/// The current instant, or `None` if the machine's clock is before 1970 or
/// past the end of `i64` seconds — neither of which a caller can do anything
/// about beyond falling back to a fixed instant.
fn now_utc() -> Option<i64> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

fn main() -> ExitCode {
    // The fallback is the instant this app used to be frozen at: 2024-07-15
    // 12:00:00 UTC, northern summer, which exercises daylight saving in both
    // hemispheres at once. It is now reached only by a machine with no clock.
    let epoch = now_utc().unwrap_or(1_721_044_800);
    let mut app = WorldClockApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, epoch);
    app::launch("worldclock", &mut app)
}

// ── Tests ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    use guitk::probe;

    /// The app the tests drive: default window, pinned to a fixed instant.
    ///
    /// The instant is passed in rather than baked into `new`, so what these
    /// tests assert about Sydney's offset does not depend on the day they run.
    fn sample_app() -> WorldClockApp {
        WorldClockApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, NOON_JUL)
    }

    fn render(app: &WorldClockApp) -> Vec<RenderCommand> {
        app.frame(app.width, app.height).commands().to_vec()
    }

    /// Send a key the way the window does.
    fn press(app: &mut WorldClockApp, k: Key) -> EventResult {
        probe::key(app, &probe::press(k))
    }

    #[test]
    fn test_default_clocks() {
        let app = sample_app();
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
        let app = sample_app();
        let (h, m, _) = app.local_hms(&Tz::UTC);
        assert_eq!((h, m), (12, 0));
    }

    #[test]
    fn test_time_for_positive_offset() {
        let app = sample_app();
        let (h, _, _) = app.local_hms(&zone("Tokyo"));
        assert_eq!(h, 21);
    }

    #[test]
    fn test_time_for_negative_offset() {
        // 08:00, not 07:00: at the default instant New York is on EDT. The old
        // fixed `-300` table asserted 07:00 here, which is the bug.
        let app = sample_app();
        let (h, _, _) = app.local_hms(&zone("New York"));
        assert_eq!(h, 8);
    }

    #[test]
    fn test_time_for_half_hour_offset() {
        let app = sample_app();
        let (h, m, _) = app.local_hms(&zone("Kolkata"));
        assert_eq!((h, m), (17, 30));
    }

    #[test]
    fn test_time_rolls_past_midnight() {
        // 23:00 UTC: Moscow is already 02:00 the next morning, and says so.
        let mut app = sample_app();
        app.set_epoch(NOON_JUL + 11 * 3600);
        let (h, _, _) = app.local_hms(&zone("Moscow"));
        assert_eq!(h, 2);
        assert_eq!(app.day_label(&zone("Moscow")), "Tomorrow");
    }

    #[test]
    fn test_time_rolls_before_midnight() {
        // 22:00 UTC with Tokyo as home: Tokyo is 07:00 on the next day, so
        // Honolulu — still on the previous afternoon — reads as yesterday.
        let mut app = sample_app();
        app.set_epoch(NOON_JUL + 10 * 3600);
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
        let mut app = sample_app();
        app.set_epoch(NOON_JUL + 4 * 3600); // 16:00 UTC
        app.home_tz_idx = 0; // London, on BST at UTC+1
        // 17:00 in London but already 01:00 the next day in Tokyo.
        assert_eq!(app.day_delta_from_home(&zone("Tokyo")), 1);
        app.advance_time(8 * 3600); // → 00:00 UTC, i.e. 01:00 in London
        assert_eq!(app.utc_epoch(), NOON_JUL + 12 * 3600);
        // London has crossed midnight too, so Tokyo's *relative* day is back
        // to level — the delta is a comparison, not a running count.
        assert_eq!(app.day_delta_from_home(&zone("Tokyo")), 0);
    }

    // ---- Daylight saving ----

    #[test]
    fn test_a_dst_zone_reads_differently_in_january_and_july() {
        let mut app = sample_app();
        let ny = zone("New York");
        app.set_epoch(NOON_JUL);
        assert_eq!(app.local_hms(&ny).0, 8);
        assert_eq!(app.abbrev(&ny), "EDT");
        assert_eq!(app.format_offset(&ny), "UTC-4");
        app.set_epoch(NOON_JAN);
        assert_eq!(app.local_hms(&ny).0, 7);
        assert_eq!(app.abbrev(&ny), "EST");
        assert_eq!(app.format_offset(&ny), "UTC-5");
    }

    #[test]
    fn test_the_southern_hemisphere_shifts_in_the_other_half_of_the_year() {
        let mut app = sample_app();
        let sydney = zone("Sydney");
        app.set_epoch(NOON_JAN);
        assert_eq!(app.abbrev(&sydney), "AEDT");
        assert_eq!(app.format_offset(&sydney), "UTC+11");
        app.set_epoch(NOON_JUL);
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
        let mut app = sample_app();
        app.home_tz_idx = 0; // London
        let ny = zone("New York");
        app.set_epoch(1_710_244_800);
        assert_eq!(app.diff_from_home(&ny), "-4h");
        app.set_epoch(NOON_JUL);
        assert_eq!(app.diff_from_home(&ny), "-5h");
        app.set_epoch(NOON_JAN);
        assert_eq!(app.diff_from_home(&ny), "-5h");
    }

    #[test]
    fn test_home_reads_as_home_whichever_zone_it_is() {
        let mut app = sample_app();
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
        let mut app = sample_app();
        for city in ["Tokyo", "Beijing", "Mumbai", "Nairobi", "S\u{e3}o Paulo"] {
            let z = zone(city);
            app.set_epoch(NOON_JAN);
            let winter = app.format_offset(&z);
            app.set_epoch(NOON_JUL);
            assert_eq!(app.format_offset(&z), winter, "{city} should not shift");
        }
    }

    #[test]
    fn test_format_time_12h() {
        let app = sample_app();
        assert_eq!(app.format_time(0, 0, 0), "12:00:00 AM");
        assert_eq!(app.format_time(12, 0, 0), "12:00:00 PM");
        assert_eq!(app.format_time(13, 30, 0), "1:30:00 PM");
        assert_eq!(app.format_time(23, 59, 59), "11:59:59 PM");
    }

    #[test]
    fn test_format_time_24h() {
        let mut app = sample_app();
        app.use_24h = true;
        assert_eq!(app.format_time(0, 0, 0), "00:00:00");
        assert_eq!(app.format_time(13, 30, 0), "13:30:00");
    }

    #[test]
    fn test_format_time_no_seconds() {
        let mut app = sample_app();
        app.show_seconds = false;
        assert_eq!(app.format_time(14, 30, 45), "2:30 PM");
    }

    #[test]
    fn test_format_offset() {
        let mut app = sample_app();
        // January, so every northern zone is on standard time and the two
        // southern ones are the odd pair out.
        app.set_epoch(NOON_JAN);
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
        let mut app = sample_app();
        // July, so home (New York) is on EDT at UTC-4 and the gaps are measured
        // from there rather than from the winter offset baked into the table.
        app.set_epoch(NOON_JUL);
        assert_eq!(app.diff_from_home(&zone("New York")), "(home)");
        assert_eq!(app.diff_from_home(&zone("London")), "+5h");
        assert_eq!(app.diff_from_home(&zone("Tokyo")), "+13h");
        assert_eq!(app.diff_from_home(&zone("Los Angeles")), "-3h");
        assert_eq!(app.diff_from_home(&zone("Mumbai")), "+9h30m");
    }

    #[test]
    fn test_add_clock() {
        let mut app = sample_app();
        let n = app.clocks.len();
        app.add_clock(3);
        assert_eq!(app.clocks.len(), n + 1);
    }

    #[test]
    fn test_add_duplicate_clock() {
        let mut app = sample_app();
        let n = app.clocks.len();
        let tz = app.clocks[0].tz_idx;
        app.add_clock(tz);
        assert_eq!(app.clocks.len(), n);
    }

    #[test]
    fn test_remove_clock() {
        let mut app = sample_app();
        let n = app.clocks.len();
        app.remove_clock(0);
        assert_eq!(app.clocks.len(), n - 1);
    }

    #[test]
    fn test_remove_adjusts_selection() {
        let mut app = sample_app();
        app.selected_clock = app.clocks.len() - 1;
        app.remove_clock(app.clocks.len() - 1);
        assert!(app.selected_clock < app.clocks.len());
    }

    #[test]
    fn test_toggle_pin() {
        let mut app = sample_app();
        let was = app.clocks[0].pinned;
        app.toggle_pin(0);
        assert_ne!(app.clocks[0].pinned, was);
    }

    #[test]
    fn test_set_home() {
        let mut app = sample_app();
        app.set_home(1);
        assert_eq!(app.home_tz_idx, app.clocks[1].tz_idx);
    }

    #[test]
    fn test_advance_time() {
        let mut app = sample_app();
        let t = app.utc_epoch();
        app.advance_time(60);
        assert_eq!(app.utc_epoch(), t + 60);
    }

    #[test]
    fn test_filtered_empty_query() {
        let app = sample_app();
        assert_eq!(app.filtered_timezones().len(), TIMEZONES.len());
    }

    #[test]
    fn test_filtered_city_search() {
        let mut app = sample_app();
        app.picker_search = String::from("tokyo");
        let f = app.filtered_timezones();
        assert!(!f.is_empty());
        assert!(f.iter().any(|&i| TIMEZONES[i].city == "Tokyo"));
    }

    #[test]
    fn test_filtered_country_search() {
        let mut app = sample_app();
        app.picker_search = String::from("usa");
        let f = app.filtered_timezones();
        assert!(f.len() >= 4);
    }

    #[test]
    fn test_filtered_no_match() {
        let mut app = sample_app();
        app.picker_search = String::from("xyznotacity");
        assert!(app.filtered_timezones().is_empty());
    }

    #[test]
    fn test_handle_key_navigation() {
        let mut app = sample_app();
        press(&mut app, Key::Right);
        assert_eq!(app.selected_clock, 1);
        press(&mut app, Key::Left);
        assert_eq!(app.selected_clock, 0);
    }

    /// Space shifts by an hour, shift-space back, and the shift survives a tick.
    ///
    /// It used to be a minute, because with the clock frozen at a literal the
    /// space bar was the only thing that made time pass at all. Now the tick
    /// does that, and the step's remaining job — "what time is it there when
    /// it is 3pm here?" — has the hour as its unit.
    #[test]
    fn test_handle_key_advance_time() {
        let mut app = sample_app();
        let t = app.utc_epoch();
        press(&mut app, Key::Space);
        assert_eq!(app.utc_epoch(), t + 3600);

        probe::key(&mut app, &probe::shift(Key::Space));
        assert_eq!(app.utc_epoch(), t, "shift-space steps back");

        press(&mut app, Key::Space);
        press(&mut app, Key::Space);
        assert_eq!(app.utc_epoch(), t + 2 * 3600);

        // The whole reason the instant is two numbers: a tick moves now
        // without discarding the two hours the user just stepped.
        handle_event(&mut app, &Event::Tick { elapsed_ms: 1000 });
        assert_eq!(
            app.utc_epoch(),
            app.base_epoch + 2 * 3600,
            "the tick moves now, not the shift"
        );

        press(&mut app, Key::R);
        assert_eq!(app.utc_epoch(), app.base_epoch, "r goes back to now");
        assert!(app.shift_label().is_none());
    }

    #[test]
    fn test_handle_key_toggle_24h() {
        let mut app = sample_app();
        assert!(!app.use_24h);
        press(&mut app, Key::T);
        assert!(app.use_24h);
    }

    #[test]
    fn test_handle_key_toggle_seconds() {
        let mut app = sample_app();
        assert!(app.show_seconds);
        press(&mut app, Key::S);
        assert!(!app.show_seconds);
    }

    #[test]
    fn test_handle_key_view_mode() {
        let mut app = sample_app();
        press(&mut app, Key::V);
        assert_eq!(app.view_mode, ViewMode::List);
        press(&mut app, Key::G);
        assert_eq!(app.view_mode, ViewMode::Grid);
    }

    #[test]
    fn test_handle_key_clock_style() {
        let mut app = sample_app();
        assert_eq!(app.clock_style, ClockStyle::Digital);
        press(&mut app, Key::A);
        assert_eq!(app.clock_style, ClockStyle::Analog);
        press(&mut app, Key::A);
        assert_eq!(app.clock_style, ClockStyle::Digital);
    }

    #[test]
    fn test_handle_key_open_picker() {
        let mut app = sample_app();
        press(&mut app, Key::N);
        assert!(app.show_picker);
    }

    #[test]
    fn test_handle_key_picker_close() {
        let mut app = sample_app();
        app.show_picker = true;
        press(&mut app, Key::Escape);
        assert!(!app.show_picker);
    }

    #[test]
    fn test_handle_key_delete() {
        let mut app = sample_app();
        let n = app.clocks.len();
        press(&mut app, Key::Delete);
        assert_eq!(app.clocks.len(), n - 1);
    }

    #[test]
    fn test_handle_key_pin() {
        let mut app = sample_app();
        let was = app.clocks[0].pinned;
        press(&mut app, Key::P);
        assert_ne!(app.clocks[0].pinned, was);
    }

    #[test]
    fn test_picker_text_input() {
        let mut app = sample_app();
        app.show_picker = true;
        probe::type_str(&mut app, "lon");
        assert_eq!(app.picker_search, "lon");
    }

    #[test]
    fn test_picker_backspace() {
        let mut app = sample_app();
        app.show_picker = true;
        app.picker_search = String::from("tok");
        press(&mut app, Key::Backspace);
        assert_eq!(app.picker_search, "to");
    }

    #[test]
    fn test_picker_enter_adds_first() {
        let mut app = sample_app();
        app.show_picker = true;
        app.picker_search = String::from("anchorage");
        let n = app.clocks.len();
        press(&mut app, Key::Enter);
        assert_eq!(app.clocks.len(), n + 1);
        assert!(!app.show_picker);
    }

    #[test]
    fn test_render_grid() {
        let app = sample_app();
        let cmds = render(&app);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_list() {
        let mut app = sample_app();
        app.view_mode = ViewMode::List;
        let cmds = render(&app);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_analog() {
        let mut app = sample_app();
        app.clock_style = ClockStyle::Analog;
        let cmds = render(&app);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_picker() {
        let mut app = sample_app();
        app.show_picker = true;
        let cmds = render(&app);
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
        let mut app = sample_app();
        app.set_epoch(NOON_JUL); // 12:00 UTC
        let (h, m, _) = app.local_hms(&zone("Kathmandu"));
        assert_eq!(h, 17);
        assert_eq!(m, 45);
    }

    #[test]
    fn test_left_boundary() {
        let mut app = sample_app();
        app.selected_clock = 0;
        press(&mut app, Key::Left);
        assert_eq!(app.selected_clock, 0);
    }

    #[test]
    fn test_right_boundary() {
        let mut app = sample_app();
        app.selected_clock = app.clocks.len() - 1;
        press(&mut app, Key::Right);
        assert_eq!(app.selected_clock, app.clocks.len() - 1);
    }

    #[test]
    fn test_remove_out_of_bounds() {
        let mut app = sample_app();
        let n = app.clocks.len();
        app.remove_clock(100);
        assert_eq!(app.clocks.len(), n);
    }

    #[test]
    fn test_set_home_out_of_bounds() {
        let mut app = sample_app();
        let home = app.home_tz_idx;
        app.set_home(100);
        assert_eq!(app.home_tz_idx, home);
    }

    #[test]
    fn test_toggle_pin_out_of_bounds() {
        let mut app = sample_app();
        app.toggle_pin(100); // should not panic
    }

    #[test]
    fn test_multiple_adds() {
        let mut app = sample_app();
        app.add_clock(3); // Moscow
        app.add_clock(4); // Dubai
        app.add_clock(6); // Dhaka
        assert_eq!(app.clocks.len(), 9);
    }

    #[test]
    fn test_format_time_12h_noon() {
        let app = sample_app();
        assert_eq!(app.format_time(12, 0, 0), "12:00:00 PM");
    }

    #[test]
    fn test_format_time_12h_1am() {
        let app = sample_app();
        assert_eq!(app.format_time(1, 0, 0), "1:00:00 AM");
    }

    // ── Wiring: the window, the layout, the pointer and the clock ───

    /// Sizes a user can plausibly drag the window to.
    const SIZES: [(f32, f32); 6] = [
        (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        (1920.0, 1080.0),
        (900.0, 400.0),
        (640.0, 480.0),
        (460.0, 300.0),
        (320.0, 240.0),
    ];

    #[test]
    fn the_window_declares_the_size_the_probe_draws_at() {
        let app = sample_app();
        let (w, h) = app.initial_size();
        assert_eq!(
            (w as f32, h as f32),
            <WorldClockApp as Probe>::SIZE,
            "a probe that draws at a size the window never opens at proves nothing"
        );
        assert_eq!(app.title(), "World Clock");
        assert_eq!(app.app_id(), "worldclock");
    }

    /// A clock that draws seconds has to be told to redraw every second.
    #[test]
    fn the_clock_asks_to_be_woken_once_a_second() {
        let app = sample_app();
        assert_eq!(app.tick_interval(), Some(Duration::from_secs(1)));
    }

    /// Every clip is closed, every translate undone, at every size and in every
    /// view — including with the picker open over the top.
    #[test]
    fn every_view_draws_a_balanced_frame_at_every_reasonable_size() {
        for (w, h) in SIZES {
            for view in [ViewMode::Grid, ViewMode::List] {
                for style in [ClockStyle::Digital, ClockStyle::Analog] {
                    for picker in [false, true] {
                        let mut app = sample_app();
                        app.view_mode = view;
                        app.clock_style = style;
                        app.show_picker = picker;
                        let frame = app.frame(w, h);
                        assert!(
                            frame.is_balanced(),
                            "{view:?}/{style:?} picker={picker} at {w}x{h}"
                        );
                        assert!(!frame.commands().is_empty());
                    }
                }
            }
        }
    }

    /// The constant and the table that has to add up to it.
    #[test]
    fn the_header_run_is_as_wide_as_it_says_it_is() {
        let app = sample_app();
        let total: f32 = app
            .header_buttons()
            .iter()
            .map(|&(gap, _, w, _, _)| gap + w)
            .sum();
        assert_eq!(total, HEADER_RUN_W);
    }

    #[test]
    fn the_layout_follows_the_window_instead_of_a_constant() {
        for (w, h) in SIZES {
            let layout = Layout::new(w, h);
            assert_eq!(layout.window.w, w);
            assert_eq!(
                layout.status.bottom(),
                h,
                "the status bar sits on the floor"
            );
            assert_eq!(layout.content.y, layout.header.bottom());
            assert_eq!(layout.content.bottom(), layout.status.y);
            assert!(layout.content.h >= 0.0);
        }
    }

    /// The buttons are the only pointer route to a different view, so they are
    /// the one thing in the header that may never leave the window.
    #[test]
    fn the_button_run_never_leaves_the_window() {
        let app = sample_app();
        for (w, h) in SIZES {
            let layout = Layout::new(w, h);
            let frame = app.frame(w, h);
            for (_, name, _, _, target) in app.header_buttons() {
                let rect = frame
                    .rect_of(|t| *t == target)
                    .unwrap_or_else(|| panic!("{name} missing at {w}x{h}"));
                assert!(rect.x >= 0.0, "{name} runs off the left at {w}x{h}");
                assert!(
                    rect.right() <= layout.window.w + 0.5,
                    "{name} runs off the right at {w}x{h}: {rect:?}"
                );
                assert!(rect.w >= 20.0, "{name} shrank to nothing at {w}x{h}");
            }
        }
    }

    /// The UTC readout restates what every card's offset line already says, so
    /// when the header runs out of room it is what goes.
    #[test]
    fn a_narrow_window_drops_the_utc_readout_before_a_button() {
        assert!(
            Layout::new(DEFAULT_WIDTH, DEFAULT_HEIGHT)
                .utc_readout
                .is_some()
        );
        assert!(Layout::new(640.0, 480.0).utc_readout.is_none());

        let app = sample_app();
        let wide = app.frame(DEFAULT_WIDTH, DEFAULT_HEIGHT);
        let narrow = app.frame(640.0, 480.0);
        assert!(text_containing(&wide, "UTC 12:").is_some());
        assert!(
            text_containing(&narrow, "UTC 12:").is_none(),
            "the readout should be gone, not drawn under a button"
        );
        // …but every button is still there. "There" means drawn, not merely
        // clickable: a hit box would survive a label that had been squeezed
        // out of existence, and a blue rectangle with nothing written on it
        // is not a button the user can find.
        let add = narrow
            .rect_of(|t| *t == Target::AddCity)
            .expect("the Add City button lost its hit box");
        assert!(
            narrow.commands().iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, x, y, .. }
                    if text == "+ Add City" && add.contains(*x, *y)
            )),
            "the Add City button is clickable but unlabelled"
        );
    }

    fn text_containing(frame: &Frame, needle: &str) -> Option<String> {
        frame.commands().iter().find_map(|c| match c {
            RenderCommand::Text { text, .. } if text.contains(needle) => Some(text.clone()),
            _ => None,
        })
    }

    /// Six header buttons that were drawn and inert: the app had no mouse
    /// handling at all, so `g`, `v`, `a`, `t`, `s` and `n` were the only way in.
    #[test]
    fn the_header_buttons_do_what_they_say() {
        let mut app = sample_app();

        probe::click(&mut app, Target::ViewList);
        assert_eq!(app.view_mode, ViewMode::List);
        probe::click(&mut app, Target::ViewGrid);
        assert_eq!(app.view_mode, ViewMode::Grid);

        probe::click(&mut app, Target::StyleAnalog);
        assert_eq!(app.clock_style, ClockStyle::Analog);
        probe::click(&mut app, Target::StyleDigital);
        assert_eq!(app.clock_style, ClockStyle::Digital);

        assert!(!app.use_24h);
        probe::click(&mut app, Target::Toggle24h);
        assert!(app.use_24h);

        assert!(app.show_seconds);
        probe::click(&mut app, Target::ToggleSeconds);
        assert!(!app.show_seconds);

        assert!(!app.show_picker);
        probe::click(&mut app, Target::AddCity);
        assert!(app.show_picker);
    }

    /// The pin and home glyphs used to be drawn only when already set, which is
    /// a control that can be turned off and never on. Both are now always drawn,
    /// and both are clickable.
    #[test]
    fn a_clock_can_be_pinned_homed_and_removed_by_pointer() {
        let mut app = sample_app();
        // Index 2 is Paris: neither pinned nor home to begin with.
        assert!(!app.clocks[2].pinned);
        assert_ne!(app.clocks[2].tz_idx, app.home_tz_idx);

        probe::click(&mut app, Target::Pin(2));
        assert!(app.clocks[2].pinned);
        probe::click(&mut app, Target::Pin(2));
        assert!(!app.clocks[2].pinned, "and off again");

        probe::click(&mut app, Target::SetHome(2));
        assert_eq!(app.home_tz_idx, app.clocks[2].tz_idx);

        let paris = app.clocks[2].tz_idx;
        let before = app.clocks.len();
        probe::click(&mut app, Target::Remove(2));
        assert_eq!(app.clocks.len(), before - 1);
        assert!(
            !app.clocks.iter().any(|c| c.tz_idx == paris),
            "the one that was clicked is the one that went"
        );
    }

    /// A glyph button sits on top of the card, and is recorded after it, so it
    /// wins the reverse-order hit test rather than the card swallowing it.
    #[test]
    fn a_glyph_button_takes_the_click_off_the_card_it_sits_on() {
        let app = sample_app();
        let card = probe::rect_of(&app, Target::Clock(1)).expect("card 1");
        let pin = probe::rect_of(&app, Target::Pin(1)).expect("pin 1");
        assert!(
            card.contains(pin.x + 1.0, pin.y + 1.0),
            "the pin should sit within its own card"
        );
        let (px, py) = pin.centre();
        assert_eq!(app.target_at(px, py), Some(Target::Pin(1)));
    }

    #[test]
    fn the_clock_a_click_lands_on_is_the_clock_that_was_drawn() {
        let mut app = sample_app();
        let card = probe::rect_of(&app, Target::Clock(3)).expect("card 3");
        // Low and to the left, clear of the glyph run in the top-right corner.
        let outcome = app.click_at(
            card.x + 12.0,
            card.bottom() - 8.0,
            MouseButton::Left,
            <WorldClockApp as Probe>::SIZE,
        );
        assert_eq!(outcome, EventResult::Consumed);
        assert_eq!(app.selected_clock, 3);
    }

    /// `scroll_offset` was subtracted by both views and assigned by nothing:
    /// there was no wheel handler and no key bound to it, so in a short window
    /// the clocks past the fold were simply unreachable.
    #[test]
    fn the_wheel_scrolls_the_content_and_stops_at_both_ends() {
        let mut app = sample_app();
        app.view_mode = ViewMode::List;
        app.resize(900.0, 400.0);
        let layout = app.layout();
        assert!(
            app.max_scroll(&layout) > 0.0,
            "six rows should not fit in a 400px window"
        );

        assert_eq!(app.scroll_offset, 0.0);
        handle_event(&mut app, &scroll_at(450.0, 200.0, -3.0));
        assert!(app.scroll_offset > 0.0, "the wheel moved nothing");

        for _ in 0..40 {
            handle_event(&mut app, &scroll_at(450.0, 200.0, -3.0));
        }
        assert_eq!(
            app.scroll_offset,
            app.max_scroll(&layout),
            "it should stop at the end, not run past it"
        );

        for _ in 0..60 {
            handle_event(&mut app, &scroll_at(450.0, 200.0, 3.0));
        }
        assert_eq!(app.scroll_offset, 0.0, "and stop at the top");
    }

    fn scroll_at(x: f32, y: f32, dy: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })
    }

    /// A row scrolled up behind the header must not be clickable through it.
    #[test]
    fn a_row_scrolled_under_the_header_is_not_clickable() {
        let mut app = sample_app();
        app.view_mode = ViewMode::List;
        app.resize(900.0, 400.0);
        let first = probe::rect_of_sized(&app, Target::Clock(0), (900.0, 400.0)).expect("row 0");

        app.scroll_by(app.max_scroll(&app.layout()));
        let after = probe::rect_of_sized(&app, Target::Clock(0), (900.0, 400.0));
        assert!(
            after.is_none_or(|r| r.y < first.y),
            "row 0 should have moved up or gone entirely"
        );
        assert_ne!(
            app.target_at(first.x + 10.0, 10.0),
            Some(Target::Clock(0)),
            "and must not be reachable through the header"
        );
    }

    #[test]
    fn growing_the_window_gives_back_the_scroll_it_no_longer_needs() {
        let mut app = sample_app();
        app.view_mode = ViewMode::List;
        app.resize(900.0, 300.0);
        app.scroll_by(10_000.0);
        assert!(app.scroll_offset > 0.0);

        app.resize(900.0, 1600.0);
        assert_eq!(
            app.scroll_offset, 0.0,
            "a window tall enough for everything has nothing left to scroll"
        );
    }

    /// `picker_scroll` was only ever assigned zero, so of the thirty cities
    /// [`TIMEZONES`] ships the picker could only reach the nine that fit; a
    /// search was the sole way to see any of the rest.
    #[test]
    fn every_city_in_the_picker_is_reachable_by_scrolling() {
        let mut app = sample_app();
        app.open_picker();
        let layout = app.layout();
        let visible = layout.picker_visible();
        assert!(
            TIMEZONES.len() > visible,
            "this test is only meaningful when the list overflows"
        );

        let last = TIMEZONES.len() - 1;
        assert!(
            !probe::is_visible(&app, Target::PickerCity(last)),
            "the last city should start off screen"
        );

        for _ in 0..TIMEZONES.len() {
            app.scroll_picker(1);
        }
        assert_eq!(app.picker_scroll, app.max_picker_scroll(&layout));
        assert!(
            probe::is_visible(&app, Target::PickerCity(last)),
            "scrolling to the end should reach the last city"
        );

        for _ in 0..TIMEZONES.len() {
            app.scroll_picker(-1);
        }
        assert_eq!(app.picker_scroll, 0);
    }

    #[test]
    fn the_picker_adds_the_city_that_was_clicked() {
        let mut app = sample_app();
        app.open_picker();
        // The first city that is not already on the board.
        let idx = (0..TIMEZONES.len())
            .find(|i| !app.clocks.iter().any(|c| c.tz_idx == *i))
            .expect("some city is not yet added");
        assert!(probe::is_visible(&app, Target::PickerCity(idx)));

        let before = app.clocks.len();
        probe::click(&mut app, Target::PickerCity(idx));
        assert_eq!(app.clocks.len(), before + 1);
        assert_eq!(app.clocks.last().map(|c| c.tz_idx), Some(idx));
    }

    /// The backdrop covers the whole window, so it is recorded *before* the
    /// panel: a click on a city row must reach the row, and only a click
    /// genuinely outside the panel closes the picker.
    #[test]
    fn the_backdrop_closes_the_picker_but_never_steals_from_the_panel() {
        let mut app = sample_app();
        app.open_picker();
        let panel = app.layout().picker().expect("a panel at the default size");
        let (cx, cy) = panel.centre();
        assert_ne!(
            app.target_at(cx, cy),
            Some(Target::PickerBackdrop),
            "the panel's own controls must win inside the panel"
        );

        let size = <WorldClockApp as Probe>::SIZE;
        assert_eq!(
            app.target_at(4.0, size.1 - 4.0),
            Some(Target::PickerBackdrop)
        );
        app.click_at(4.0, size.1 - 4.0, MouseButton::Left, size);
        assert!(!app.show_picker);

        app.open_picker();
        probe::click(&mut app, Target::PickerClose);
        assert!(!app.show_picker);
    }

    /// While the picker is open it is modal: the header behind it is covered.
    #[test]
    fn the_picker_covers_the_header_it_is_drawn_over() {
        let mut app = sample_app();
        let button = probe::rect_of(&app, Target::ViewList).expect("List button");
        let (bx, by) = button.centre();
        assert_eq!(app.target_at(bx, by), Some(Target::ViewList));

        app.open_picker();
        assert_eq!(
            app.target_at(bx, by),
            Some(Target::PickerBackdrop),
            "a click meant for the modal must not fall through to the header"
        );
    }

    /// The list columns were the constants 16, 200, 380, 520, 620 and 740 —
    /// a layout for exactly one window width.
    #[test]
    fn the_list_columns_follow_the_window() {
        let narrow = Layout::new(900.0, 600.0).list_columns();
        let wide = Layout::new(1900.0, 600.0).list_columns();
        assert!(
            wide[5] > narrow[5],
            "a wider window should spread the columns out"
        );
        for cols in [narrow, wide] {
            assert!(
                cols.windows(2).all(|w| w[1] > w[0]),
                "the columns must stay in reading order"
            );
        }
        assert!(
            wide[5] < 1900.0,
            "the last column must stay inside the window"
        );
    }

    /// The shift is reported, and the report is the button that clears it.
    #[test]
    fn the_shift_chip_says_what_the_shift_is_and_resets_it() {
        let mut app = sample_app();
        assert!(app.shift_label().is_none());
        assert!(!probe::is_visible(&app, Target::ResetShift));

        press(&mut app, Key::Space);
        press(&mut app, Key::Space);
        press(&mut app, Key::Space);
        assert_eq!(app.shift_label().as_deref(), Some("+3h from now"));
        let frame = app.frame(DEFAULT_WIDTH, DEFAULT_HEIGHT);
        assert!(text_containing(&frame, "+3h from now").is_some());

        probe::click(&mut app, Target::ResetShift);
        assert_eq!(app.offset_secs, 0);
        assert!(!probe::is_visible(&app, Target::ResetShift));
    }

    /// The whole point of the exercise: the instant comes from the machine.
    #[test]
    fn the_time_comes_from_the_clock_rather_than_a_literal() {
        let now = now_utc().expect("the machine has a clock");
        // 2026-01-01. Any build of this program runs after that, so a `now`
        // below it means the epoch came from somewhere other than the clock.
        assert!(
            now > 1_767_225_600,
            "now_utc returned {now}, which is not a current instant"
        );
        assert_ne!(
            now, 1_721_044_800,
            "that is the literal the app used to be frozen at"
        );

        let app = WorldClockApp::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, now);
        assert_eq!(app.utc_epoch(), now);
    }

    /// A tick that finds the second has changed redraws; one that does not,
    /// does not — an idle wake-up must not cost a frame.
    #[test]
    fn a_tick_moves_the_clock_and_only_then_asks_for_a_frame() {
        let mut app = sample_app();
        assert_eq!(
            handle_event(&mut app, &Event::Tick { elapsed_ms: 1000 }),
            EventResult::Consumed,
            "the first tick replaces the seeded instant with the real one"
        );
        assert_eq!(
            handle_event(&mut app, &Event::Tick { elapsed_ms: 1000 }),
            EventResult::Ignored,
            "a second tick within the same second has nothing to report"
        );
        assert_eq!(
            app.base_epoch,
            now_utc().expect("the machine has a clock"),
            "the tick is what carries the app's idea of now forward"
        );
    }

    /// And a second's worth of movement is visible: the seconds hand is drawn
    /// somewhere else. `RenderCommand` is not `PartialEq`, so the comparison is
    /// on its `Debug` form, which is what the difference would be read from
    /// anyway.
    #[test]
    fn one_second_moves_the_seconds_hand() {
        let mut app = sample_app();
        app.clock_style = ClockStyle::Analog;
        assert!(app.show_seconds);

        let lines = |a: &WorldClockApp| -> Vec<String> {
            a.frame(DEFAULT_WIDTH, DEFAULT_HEIGHT)
                .commands()
                .iter()
                .filter(|c| matches!(c, RenderCommand::Line { .. }))
                .map(|c| format!("{c:?}"))
                .collect()
        };

        let before = lines(&app);
        app.set_epoch(NOON_JUL + 1);
        let after = lines(&app);
        assert_eq!(before.len(), after.len(), "same hands, moved");
        assert_ne!(before, after, "a second passed and nothing was redrawn");

        // Turning the seconds hand off takes away exactly one line per clock —
        // the hand itself, and nothing else on the face.
        app.show_seconds = false;
        let fewer = lines(&app);
        assert_eq!(
            after.len() - fewer.len(),
            app.clocks.len(),
            "one seconds hand per clock, no more and no less"
        );

        // It does not, however, make a second invisible: the minute hand's angle
        // carries `s * 0.1`, so it creeps continuously rather than jumping once a
        // minute. A face that froze for 59 seconds and then twitched would be the
        // bug, not this.
        app.set_epoch(NOON_JUL);
        assert_ne!(
            fewer,
            lines(&app),
            "the minute hand creeps with the seconds even when the hand is hidden"
        );
    }

    #[test]
    fn a_resize_event_is_what_moves_the_layout() {
        let mut app = sample_app();
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Resize {
                    width: 800,
                    height: 600
                }
            ),
            EventResult::Consumed
        );
        assert_eq!((app.width, app.height), (800.0, 600.0));
        assert_eq!(app.layout().window.w, 800.0);
    }

    #[test]
    fn closing_the_window_stops_the_app() {
        let mut app = sample_app();
        assert!(app.running);
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::G))),
            Response::Redraw
        ));
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
        assert!(!app.running);
    }

    /// An unhandled key must not cost a redraw.
    #[test]
    fn a_key_nothing_is_bound_to_is_ignored() {
        let mut app = sample_app();
        assert_eq!(press(&mut app, Key::F7), EventResult::Ignored);
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::F7))),
            Response::Idle
        ));
    }
}
