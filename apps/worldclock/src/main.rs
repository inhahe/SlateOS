#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]

//! OurOS World Clock — multi-timezone clock display with analog/digital views.
//!
//! Shows clocks for cities around the world with timezone offset, day/night
//! indicators, time difference from local, and both analog and digital display
//! modes.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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

// ── Timezone data ───────────────────────────────────────────────────
#[derive(Clone)]
struct TimezoneInfo {
    city: &'static str,
    country: &'static str,
    /// UTC offset in minutes (e.g., +330 for India, -300 for US Eastern)
    offset_minutes: i32,
    /// Abbreviation (e.g., "EST", "IST", "JST")
    abbreviation: &'static str,
}

const TIMEZONES: &[TimezoneInfo] = &[
    TimezoneInfo { city: "London", country: "United Kingdom", offset_minutes: 0, abbreviation: "GMT" },
    TimezoneInfo { city: "Paris", country: "France", offset_minutes: 60, abbreviation: "CET" },
    TimezoneInfo { city: "Berlin", country: "Germany", offset_minutes: 60, abbreviation: "CET" },
    TimezoneInfo { city: "Moscow", country: "Russia", offset_minutes: 180, abbreviation: "MSK" },
    TimezoneInfo { city: "Dubai", country: "UAE", offset_minutes: 240, abbreviation: "GST" },
    TimezoneInfo { city: "Mumbai", country: "India", offset_minutes: 330, abbreviation: "IST" },
    TimezoneInfo { city: "Dhaka", country: "Bangladesh", offset_minutes: 360, abbreviation: "BST" },
    TimezoneInfo { city: "Bangkok", country: "Thailand", offset_minutes: 420, abbreviation: "ICT" },
    TimezoneInfo { city: "Singapore", country: "Singapore", offset_minutes: 480, abbreviation: "SGT" },
    TimezoneInfo { city: "Beijing", country: "China", offset_minutes: 480, abbreviation: "CST" },
    TimezoneInfo { city: "Tokyo", country: "Japan", offset_minutes: 540, abbreviation: "JST" },
    TimezoneInfo { city: "Seoul", country: "South Korea", offset_minutes: 540, abbreviation: "KST" },
    TimezoneInfo { city: "Sydney", country: "Australia", offset_minutes: 600, abbreviation: "AEST" },
    TimezoneInfo { city: "Auckland", country: "New Zealand", offset_minutes: 720, abbreviation: "NZST" },
    TimezoneInfo { city: "Honolulu", country: "USA", offset_minutes: -600, abbreviation: "HST" },
    TimezoneInfo { city: "Anchorage", country: "USA", offset_minutes: -540, abbreviation: "AKST" },
    TimezoneInfo { city: "Los Angeles", country: "USA", offset_minutes: -480, abbreviation: "PST" },
    TimezoneInfo { city: "Denver", country: "USA", offset_minutes: -420, abbreviation: "MST" },
    TimezoneInfo { city: "Chicago", country: "USA", offset_minutes: -360, abbreviation: "CST" },
    TimezoneInfo { city: "New York", country: "USA", offset_minutes: -300, abbreviation: "EST" },
    TimezoneInfo { city: "São Paulo", country: "Brazil", offset_minutes: -180, abbreviation: "BRT" },
    TimezoneInfo { city: "Cairo", country: "Egypt", offset_minutes: 120, abbreviation: "EET" },
    TimezoneInfo { city: "Istanbul", country: "Turkey", offset_minutes: 180, abbreviation: "TRT" },
    TimezoneInfo { city: "Nairobi", country: "Kenya", offset_minutes: 180, abbreviation: "EAT" },
    TimezoneInfo { city: "Lagos", country: "Nigeria", offset_minutes: 60, abbreviation: "WAT" },
    TimezoneInfo { city: "Kathmandu", country: "Nepal", offset_minutes: 345, abbreviation: "NPT" },
    TimezoneInfo { city: "Kolkata", country: "India", offset_minutes: 330, abbreviation: "IST" },
    TimezoneInfo { city: "Jakarta", country: "Indonesia", offset_minutes: 420, abbreviation: "WIB" },
    TimezoneInfo { city: "Manila", country: "Philippines", offset_minutes: 480, abbreviation: "PHT" },
    TimezoneInfo { city: "Taipei", country: "Taiwan", offset_minutes: 480, abbreviation: "CST" },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Grid,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq)]
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
    /// Simulation time: seconds since midnight UTC
    utc_seconds: u32,
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
            ClockEntry { tz_idx: 19, pinned: true },   // New York
            ClockEntry { tz_idx: 0, pinned: true },     // London
            ClockEntry { tz_idx: 1, pinned: false },    // Paris
            ClockEntry { tz_idx: 10, pinned: true },    // Tokyo
            ClockEntry { tz_idx: 12, pinned: false },   // Sydney
            ClockEntry { tz_idx: 5, pinned: false },    // Mumbai
        ];

        Self {
            width: 1100.0,
            height: 750.0,
            utc_seconds: 43200, // Start at noon UTC
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

    fn advance_time(&mut self, seconds: u32) {
        self.utc_seconds = (self.utc_seconds + seconds) % 86400;
    }

    /// Get time components for a timezone offset.
    fn time_for_offset(&self, offset_minutes: i32) -> (u32, u32, u32) {
        let total_secs = self.utc_seconds as i64 + (offset_minutes as i64) * 60;
        let total_secs = ((total_secs % 86400) + 86400) % 86400;
        let h = (total_secs / 3600) as u32;
        let m = ((total_secs % 3600) / 60) as u32;
        let s = (total_secs % 60) as u32;
        (h, m, s)
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

    fn format_offset(offset_minutes: i32) -> String {
        let sign = if offset_minutes >= 0 { '+' } else { '-' };
        let abs = offset_minutes.unsigned_abs();
        let h = abs / 60;
        let m = abs % 60;
        if m == 0 {
            format!("UTC{sign}{h}")
        } else {
            format!("UTC{sign}{h}:{m:02}")
        }
    }

    fn is_daytime(h: u32) -> bool {
        h >= 6 && h < 18
    }

    fn day_night_color(h: u32) -> Color {
        if h >= 6 && h < 18 { YELLOW } else { LAVENDER }
    }

    fn day_night_icon(h: u32) -> &'static str {
        if h >= 6 && h < 18 { "\u{2600}" } else { "\u{263D}" }
    }

    fn diff_from_home(&self, offset_minutes: i32) -> String {
        let home_offset = TIMEZONES.get(self.home_tz_idx)
            .map_or(0, |tz| tz.offset_minutes);
        let diff = offset_minutes - home_offset;
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
        self.clocks.push(ClockEntry { tz_idx, pinned: false });
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
        TIMEZONES.iter().enumerate()
            .filter(|(_, tz)| {
                if query.is_empty() { return true; }
                tz.city.to_ascii_lowercase().contains(&query)
                    || tz.country.to_ascii_lowercase().contains(&query)
                    || tz.abbreviation.to_ascii_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn handle_key(&mut self, key: &str, _ctrl: bool, _shift: bool) {
        if self.show_picker {
            match key {
                "Escape" => self.show_picker = false,
                "Backspace" => { self.picker_search.pop(); self.picker_scroll = 0; }
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
            "Left" | "h" => {
                if self.selected_clock > 0 { self.selected_clock -= 1; }
            }
            "Right" | "l" => {
                if self.selected_clock + 1 < self.clocks.len() {
                    self.selected_clock += 1;
                }
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
            x: 0.0, y: 0.0, width: self.width, height: self.height,
            color: BASE, corner_radii: CornerRadii::ZERO,
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
            x: 0.0, y: 0.0, width: self.width, height: Self::HEADER_H,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 16.0, y: 14.0,
            text: String::from("\u{1F30D} World Clock"),
            font_size: 20.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // View/style buttons
        let mut bx = 240.0;
        let btn_h = 30.0;
        let btn_y = 10.0;
        let grid_bg = if self.view_mode == ViewMode::Grid { SURFACE1 } else { SURFACE0 };
        let list_bg = if self.view_mode == ViewMode::List { SURFACE1 } else { SURFACE0 };
        for (label, bg, w) in [("Grid", grid_bg, 50.0_f32), ("List", list_bg, 50.0)] {
            cmds.push(RenderCommand::FillRect {
                x: bx, y: btn_y, width: w, height: btn_h,
                color: bg, corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0, y: btn_y + 7.0,
                text: label.to_string(), font_size: 12.0, color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular, max_width: Some(w - 16.0),
            });
            bx += w + 4.0;
        }

        bx += 12.0;
        let dig_bg = if self.clock_style == ClockStyle::Digital { SURFACE1 } else { SURFACE0 };
        let ana_bg = if self.clock_style == ClockStyle::Analog { SURFACE1 } else { SURFACE0 };
        for (label, bg, w) in [("Digital", dig_bg, 60.0_f32), ("Analog", ana_bg, 60.0)] {
            cmds.push(RenderCommand::FillRect {
                x: bx, y: btn_y, width: w, height: btn_h,
                color: bg, corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 6.0, y: btn_y + 7.0,
                text: label.to_string(), font_size: 12.0, color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular, max_width: Some(w - 12.0),
            });
            bx += w + 4.0;
        }

        bx += 12.0;
        let h24_bg = if self.use_24h { BLUE } else { SURFACE0 };
        cmds.push(RenderCommand::FillRect {
            x: bx, y: btn_y, width: 44.0, height: btn_h,
            color: h24_bg, corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: bx + 6.0, y: btn_y + 7.0,
            text: String::from("24h"), font_size: 12.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular, max_width: Some(36.0),
        });
        bx += 52.0;

        // Add city button
        cmds.push(RenderCommand::FillRect {
            x: bx, y: btn_y, width: 80.0, height: btn_h,
            color: BLUE, corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: bx + 10.0, y: btn_y + 7.0,
            text: String::from("+ Add City"), font_size: 12.0, color: CRUST,
            font_weight: FontWeightHint::Bold, max_width: Some(70.0),
        });

        // UTC time
        let (uh, um, us) = self.time_for_offset(0);
        cmds.push(RenderCommand::Text {
            x: self.width - 180.0, y: 16.0,
            text: format!("UTC {uh:02}:{um:02}:{us:02}"),
            font_size: 16.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular, max_width: Some(170.0),
        });
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let start_y = Self::HEADER_H + 12.0 - self.scroll_offset;
        let cols = ((self.width - Self::CARD_GAP) / (Self::CARD_W + Self::CARD_GAP))
            .max(1.0) as usize;

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
        &self, cmds: &mut Vec<RenderCommand>,
        x: f32, y: f32,
        tz: &TimezoneInfo, entry: &ClockEntry, selected: bool,
    ) {
        let (h, m, s) = self.time_for_offset(tz.offset_minutes);
        let is_day = Self::is_daytime(h);
        let border_color = if selected { BLUE } else if is_day { YELLOW } else { LAVENDER };
        let card_bg = if is_day { Color::from_hex(0x2A2A3E) } else { SURFACE0 };

        if selected {
            cmds.push(RenderCommand::StrokeRect {
                x: x - 1.0, y: y - 1.0,
                width: Self::CARD_W + 2.0, height: Self::CARD_H + 2.0,
                color: BLUE, line_width: 2.0,
                corner_radii: CornerRadii::all(9.0),
            });
        }

        cmds.push(RenderCommand::FillRect {
            x, y, width: Self::CARD_W, height: Self::CARD_H,
            color: card_bg, corner_radii: CornerRadii::all(8.0),
        });

        // Day/night indicator strip
        cmds.push(RenderCommand::FillRect {
            x, y, width: Self::CARD_W, height: 4.0,
            color: border_color, corner_radii: CornerRadii { top_left: 8.0, top_right: 8.0, bottom_right: 0.0, bottom_left: 0.0 },
        });

        // Pin / home indicators
        if entry.pinned {
            cmds.push(RenderCommand::Text {
                x: x + Self::CARD_W - 24.0, y: y + 8.0,
                text: String::from("\u{1F4CC}"), font_size: 12.0, color: PEACH,
                font_weight: FontWeightHint::Regular, max_width: Some(20.0),
            });
        }
        if entry.tz_idx == self.home_tz_idx {
            cmds.push(RenderCommand::Text {
                x: x + Self::CARD_W - 44.0, y: y + 8.0,
                text: String::from("\u{1F3E0}"), font_size: 12.0, color: GREEN,
                font_weight: FontWeightHint::Regular, max_width: Some(20.0),
            });
        }

        // City name and country
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + 12.0,
            text: tz.city.to_string(), font_size: 16.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold, max_width: Some(Self::CARD_W - 56.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + 32.0,
            text: tz.country.to_string(), font_size: 11.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular, max_width: Some(Self::CARD_W - 24.0),
        });

        match self.clock_style {
            ClockStyle::Digital => {
                let time_str = self.format_time(h, m, s);
                cmds.push(RenderCommand::Text {
                    x: x + 12.0, y: y + 56.0,
                    text: time_str, font_size: 28.0, color: TEXT_COLOR,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(Self::CARD_W - 24.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + Self::CARD_W - 36.0, y: y + 60.0,
                    text: Self::day_night_icon(h).to_string(),
                    font_size: 20.0, color: Self::day_night_color(h),
                    font_weight: FontWeightHint::Regular, max_width: Some(30.0),
                });
            }
            ClockStyle::Analog => {
                self.render_analog_clock(cmds, x + Self::CARD_W / 2.0, y + 88.0, 35.0, h, m, s);
            }
        }

        // Bottom info
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + Self::CARD_H - 30.0,
            text: format!("{} ({})", Self::format_offset(tz.offset_minutes), tz.abbreviation),
            font_size: 11.0, color: OVERLAY0,
            font_weight: FontWeightHint::Regular, max_width: Some(Self::CARD_W / 2.0 - 16.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + Self::CARD_W / 2.0 + 8.0, y: y + Self::CARD_H - 30.0,
            text: self.diff_from_home(tz.offset_minutes),
            font_size: 11.0, color: TEAL,
            font_weight: FontWeightHint::Regular, max_width: Some(Self::CARD_W / 2.0 - 20.0),
        });
        let dn_label = if is_day { "Daytime" } else { "Nighttime" };
        cmds.push(RenderCommand::Text {
            x: x + 12.0, y: y + Self::CARD_H - 14.0,
            text: dn_label.to_string(), font_size: 10.0, color: Self::day_night_color(h),
            font_weight: FontWeightHint::Regular, max_width: Some(80.0),
        });
    }

    fn render_analog_clock(
        &self, cmds: &mut Vec<RenderCommand>,
        cx: f32, cy: f32, radius: f32,
        h: u32, m: u32, s: u32,
    ) {
        // Clock face
        cmds.push(RenderCommand::FillRect {
            x: cx - radius, y: cy - radius,
            width: radius * 2.0, height: radius * 2.0,
            color: CRUST, corner_radii: CornerRadii::all(radius),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: cx - radius, y: cy - radius,
            width: radius * 2.0, height: radius * 2.0,
            color: SURFACE2, line_width: 1.5,
            corner_radii: CornerRadii::all(radius),
        });

        // Hour markers
        for i in 0..12_u32 {
            let angle = (i as f32 * 30.0 - 90.0) * core::f32::consts::PI / 180.0;
            let outer_r = radius - 3.0;
            let inner_r = if i % 3 == 0 { radius - 10.0 } else { radius - 7.0 };
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
        let h_angle = ((h % 12) as f32 * 30.0 + m as f32 * 0.5 - 90.0) * core::f32::consts::PI / 180.0;
        cmds.push(RenderCommand::Line {
            x1: cx, y1: cy,
            x2: cx + radius * 0.5 * h_angle.cos(),
            y2: cy + radius * 0.5 * h_angle.sin(),
            color: TEXT_COLOR, width: 3.0,
        });

        // Minute hand
        let m_angle = (m as f32 * 6.0 + s as f32 * 0.1 - 90.0) * core::f32::consts::PI / 180.0;
        cmds.push(RenderCommand::Line {
            x1: cx, y1: cy,
            x2: cx + radius * 0.7 * m_angle.cos(),
            y2: cy + radius * 0.7 * m_angle.sin(),
            color: SUBTEXT1, width: 2.0,
        });

        // Second hand
        if self.show_seconds {
            let s_angle = (s as f32 * 6.0 - 90.0) * core::f32::consts::PI / 180.0;
            cmds.push(RenderCommand::Line {
                x1: cx, y1: cy,
                x2: cx + radius * 0.8 * s_angle.cos(),
                y2: cy + radius * 0.8 * s_angle.sin(),
                color: RED, width: 1.0,
            });
        }

        // Center dot
        cmds.push(RenderCommand::FillRect {
            x: cx - 2.0, y: cy - 2.0, width: 4.0, height: 4.0,
            color: TEXT_COLOR, corner_radii: CornerRadii::all(2.0),
        });
    }

    fn render_list(&self, cmds: &mut Vec<RenderCommand>) {
        let start_y = Self::HEADER_H + 4.0 - self.scroll_offset;
        // Column headers
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: start_y, width: self.width, height: 28.0,
            color: CRUST, corner_radii: CornerRadii::ZERO,
        });
        for (hx, label) in [(16.0, "City"), (200.0, "Time"), (380.0, "UTC Offset"), (520.0, "Diff"), (620.0, "Day/Night")] {
            cmds.push(RenderCommand::Text {
                x: hx, y: start_y + 6.0,
                text: label.to_string(), font_size: 12.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Bold, max_width: Some(150.0),
            });
        }

        let row_start = start_y + 32.0;
        for (i, entry) in self.clocks.iter().enumerate() {
            let ry = row_start + i as f32 * Self::LIST_ROW_H;
            if ry + Self::LIST_ROW_H < Self::HEADER_H || ry > self.height - Self::STATUS_H {
                continue;
            }
            if let Some(tz) = TIMEZONES.get(entry.tz_idx) {
                let (h, m, s) = self.time_for_offset(tz.offset_minutes);
                let is_selected = i == self.selected_clock;
                let bg = if is_selected { SURFACE1 } else if i % 2 == 0 { SURFACE0 } else { BASE };

                cmds.push(RenderCommand::FillRect {
                    x: 0.0, y: ry, width: self.width, height: Self::LIST_ROW_H,
                    color: bg, corner_radii: CornerRadii::ZERO,
                });

                let mut markers = String::new();
                if entry.pinned { markers.push_str("\u{1F4CC} "); }
                if entry.tz_idx == self.home_tz_idx { markers.push_str("\u{1F3E0} "); }

                cmds.push(RenderCommand::Text {
                    x: 16.0, y: ry + 8.0,
                    text: format!("{markers}{}", tz.city),
                    font_size: 14.0, color: TEXT_COLOR,
                    font_weight: FontWeightHint::Bold, max_width: Some(180.0),
                });
                cmds.push(RenderCommand::Text {
                    x: 16.0, y: ry + 28.0,
                    text: tz.country.to_string(), font_size: 11.0, color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular, max_width: Some(180.0),
                });
                cmds.push(RenderCommand::Text {
                    x: 200.0, y: ry + 12.0,
                    text: self.format_time(h, m, s), font_size: 20.0, color: TEXT_COLOR,
                    font_weight: FontWeightHint::Bold, max_width: Some(170.0),
                });
                cmds.push(RenderCommand::Text {
                    x: 380.0, y: ry + 16.0,
                    text: format!("{} ({})", Self::format_offset(tz.offset_minutes), tz.abbreviation),
                    font_size: 13.0, color: OVERLAY0,
                    font_weight: FontWeightHint::Regular, max_width: Some(130.0),
                });
                cmds.push(RenderCommand::Text {
                    x: 520.0, y: ry + 16.0,
                    text: self.diff_from_home(tz.offset_minutes),
                    font_size: 13.0, color: TEAL,
                    font_weight: FontWeightHint::Regular, max_width: Some(90.0),
                });
                let dn_icon = Self::day_night_icon(h);
                let dn_label = if Self::is_daytime(h) { "Day" } else { "Night" };
                cmds.push(RenderCommand::Text {
                    x: 620.0, y: ry + 16.0,
                    text: format!("{dn_icon} {dn_label}"),
                    font_size: 13.0, color: Self::day_night_color(h),
                    font_weight: FontWeightHint::Regular, max_width: Some(100.0),
                });
            }
        }
    }

    fn render_picker(&self, cmds: &mut Vec<RenderCommand>) {
        // Overlay
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: self.height,
            color: Color::rgba(0, 0, 0, 160), corner_radii: CornerRadii::ZERO,
        });

        let pw = 420.0_f32;
        let ph = 500.0_f32;
        let px = (self.width - pw) / 2.0;
        let py = (self.height - ph) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: px, y: py, width: pw, height: ph,
            color: MANTLE, corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::Text {
            x: px + 16.0, y: py + 14.0,
            text: String::from("Add City"), font_size: 18.0, color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold, max_width: Some(pw - 32.0),
        });

        // Search input
        cmds.push(RenderCommand::FillRect {
            x: px + 12.0, y: py + 44.0, width: pw - 24.0, height: 32.0,
            color: SURFACE0, corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.picker_search.is_empty() {
            String::from("Search cities...")
        } else {
            format!("{}|", self.picker_search)
        };
        let search_color = if self.picker_search.is_empty() { OVERLAY0 } else { TEXT_COLOR };
        cmds.push(RenderCommand::Text {
            x: px + 20.0, y: py + 52.0,
            text: search_text, font_size: 13.0, color: search_color,
            font_weight: FontWeightHint::Regular, max_width: Some(pw - 48.0),
        });

        // Timezone list
        let list_y = py + 84.0;
        let list_h = ph - 84.0 - 12.0;
        let item_h = 44.0;
        let filtered = self.filtered_timezones();
        let visible_items = (list_h / item_h) as usize;

        for (vis_i, &tz_idx) in filtered.iter().skip(self.picker_scroll).take(visible_items).enumerate() {
            if let Some(tz) = TIMEZONES.get(tz_idx) {
                let iy = list_y + vis_i as f32 * item_h;
                let already = self.clocks.iter().any(|c| c.tz_idx == tz_idx);
                let bg = if already { SURFACE1 } else { SURFACE0 };

                cmds.push(RenderCommand::FillRect {
                    x: px + 8.0, y: iy, width: pw - 16.0, height: item_h - 2.0,
                    color: bg, corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: px + 16.0, y: iy + 6.0,
                    text: format!("{}, {}", tz.city, tz.country),
                    font_size: 13.0,
                    color: if already { OVERLAY0 } else { TEXT_COLOR },
                    font_weight: FontWeightHint::Bold, max_width: Some(pw - 40.0),
                });
                cmds.push(RenderCommand::Text {
                    x: px + 16.0, y: iy + 24.0,
                    text: format!("{} ({})", Self::format_offset(tz.offset_minutes), tz.abbreviation),
                    font_size: 11.0, color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular, max_width: Some(200.0),
                });
                if already {
                    cmds.push(RenderCommand::Text {
                        x: px + pw - 70.0, y: iy + 12.0,
                        text: String::from("Added"), font_size: 11.0, color: GREEN,
                        font_weight: FontWeightHint::Regular, max_width: Some(50.0),
                    });
                }
            }
        }
    }

    fn render_status(&self, cmds: &mut Vec<RenderCommand>) {
        let sy = self.height - Self::STATUS_H;
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: sy, width: self.width, height: Self::STATUS_H,
            color: MANTLE, corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 8.0, y: sy + 6.0,
            text: self.status_msg.clone(), font_size: 12.0, color: SUBTEXT1,
            font_weight: FontWeightHint::Regular, max_width: Some(400.0),
        });
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0, y: sy + 6.0,
            text: format!("{} clocks", self.clocks.len()),
            font_size: 11.0, color: OVERLAY0,
            font_weight: FontWeightHint::Regular, max_width: Some(110.0),
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

    #[test]
    fn test_time_for_utc() {
        let app = WorldClockApp::new();
        let (h, m, _) = app.time_for_offset(0);
        assert_eq!(h, 12);
        assert_eq!(m, 0);
    }

    #[test]
    fn test_time_for_positive_offset() {
        let app = WorldClockApp::new();
        let (h, _, _) = app.time_for_offset(540);
        assert_eq!(h, 21);
    }

    #[test]
    fn test_time_for_negative_offset() {
        let app = WorldClockApp::new();
        let (h, _, _) = app.time_for_offset(-300);
        assert_eq!(h, 7);
    }

    #[test]
    fn test_time_for_half_hour_offset() {
        let app = WorldClockApp::new();
        let (h, m, _) = app.time_for_offset(330);
        assert_eq!(h, 17);
        assert_eq!(m, 30);
    }

    #[test]
    fn test_time_wraps_past_midnight() {
        let mut app = WorldClockApp::new();
        app.utc_seconds = 82800;
        let (h, _, _) = app.time_for_offset(180);
        assert_eq!(h, 2);
    }

    #[test]
    fn test_time_wraps_before_midnight() {
        let mut app = WorldClockApp::new();
        app.utc_seconds = 3600;
        let (h, _, _) = app.time_for_offset(-300);
        assert_eq!(h, 20);
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
        assert_eq!(WorldClockApp::format_offset(0), "UTC+0");
        assert_eq!(WorldClockApp::format_offset(60), "UTC+1");
        assert_eq!(WorldClockApp::format_offset(-300), "UTC-5");
        assert_eq!(WorldClockApp::format_offset(330), "UTC+5:30");
        assert_eq!(WorldClockApp::format_offset(345), "UTC+5:45");
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
        let app = WorldClockApp::new();
        assert_eq!(app.diff_from_home(-300), "(home)");
        assert_eq!(app.diff_from_home(0), "+5h");
        assert_eq!(app.diff_from_home(540), "+14h");
        assert_eq!(app.diff_from_home(-480), "-3h");
        assert_eq!(app.diff_from_home(330), "+10h30m");
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
        let t = app.utc_seconds;
        app.advance_time(60);
        assert_eq!(app.utc_seconds, t + 60);
    }

    #[test]
    fn test_advance_time_wraps() {
        let mut app = WorldClockApp::new();
        app.utc_seconds = 86399;
        app.advance_time(2);
        assert_eq!(app.utc_seconds, 1);
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
        let t = app.utc_seconds;
        app.handle_key("Space", false, false);
        assert_eq!(app.utc_seconds, t + 60);
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
            assert!(tz.offset_minutes >= -720 && tz.offset_minutes <= 840);
        }
    }

    #[test]
    fn test_day_night_icon() {
        assert_eq!(WorldClockApp::day_night_icon(12), "\u{2600}");
        assert_eq!(WorldClockApp::day_night_icon(0), "\u{263D}");
    }

    #[test]
    fn test_kathmandu_offset() {
        let app = WorldClockApp::new();
        let (h, m, _) = app.time_for_offset(345); // +5:45
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
