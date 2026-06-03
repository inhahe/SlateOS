#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]

//! OurOS Compass -- digital compass and navigation tool.
//!
//! Features:
//! - Compass rose with cardinal/intercardinal labels and degree tick marks
//! - Heading display in degrees and cardinal direction (keyboard-adjustable)
//! - Red north needle that rotates with heading
//! - Simulated lat/lon coordinates
//! - Waypoint system (up to 10 waypoints) with bearing/distance to selected
//! - Great-circle distance via the Haversine formula
//! - Multiple views: Compass, Waypoint list, Coordinate entry
//! - Magnetic declination offset (-30 to +30 degrees)
//! - km/miles unit toggle

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
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
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Constants ───────────────────────────────────────────────────────
const PI: f64 = core::f64::consts::PI;
const DEG_TO_RAD: f64 = PI / 180.0;
const RAD_TO_DEG: f64 = 180.0 / PI;
const EARTH_RADIUS_KM: f64 = 6371.0;
const KM_TO_MILES: f64 = 0.621_371;

const MAX_WAYPOINTS: usize = 10;

const COMPASS_CX: f32 = 400.0;
const COMPASS_CY: f32 = 340.0;
const COMPASS_RADIUS: f32 = 240.0;
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 720.0;

// ── Types ───────────────────────────────────────────────────────────

/// Active view in the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Compass,
    Waypoints,
    CoordinateEntry,
}

/// Distance unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DistanceUnit {
    Kilometers,
    Miles,
}

/// Which coordinate field is being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoordField {
    Latitude,
    Longitude,
}

/// A geographic coordinate.
#[derive(Clone, Debug)]
struct Coordinate {
    /// Latitude in degrees (-90 to 90).
    lat: f64,
    /// Longitude in degrees (-180 to 180).
    lon: f64,
}

impl Coordinate {
    fn new(lat: f64, lon: f64) -> Self {
        Self {
            lat: lat.clamp(-90.0, 90.0),
            lon: lon.clamp(-180.0, 180.0),
        }
    }

    /// Format latitude as degrees with N/S indicator.
    fn format_lat(&self) -> String {
        let dir = if self.lat >= 0.0 { 'N' } else { 'S' };
        format!("{:.4}{}", self.lat.abs(), dir)
    }

    /// Format longitude as degrees with E/W indicator.
    fn format_lon(&self) -> String {
        let dir = if self.lon >= 0.0 { 'E' } else { 'W' };
        format!("{:.4}{}", self.lon.abs(), dir)
    }
}

/// A saved waypoint.
#[derive(Clone, Debug)]
struct Waypoint {
    name: String,
    coord: Coordinate,
}

// ── Haversine formula ───────────────────────────────────────────────

/// Calculate the great-circle distance between two coordinates in kilometers
/// using the Haversine formula.
fn haversine_distance(a: &Coordinate, b: &Coordinate) -> f64 {
    let d_lat = (b.lat - a.lat) * DEG_TO_RAD;
    let d_lon = (b.lon - a.lon) * DEG_TO_RAD;
    let lat1 = a.lat * DEG_TO_RAD;
    let lat2 = b.lat * DEG_TO_RAD;

    let half_d_lat = (d_lat / 2.0).sin();
    let half_d_lon = (d_lon / 2.0).sin();
    let h = half_d_lat * half_d_lat + lat1.cos() * lat2.cos() * half_d_lon * half_d_lon;
    let c = 2.0 * h.sqrt().asin();
    EARTH_RADIUS_KM * c
}

/// Calculate the initial bearing (forward azimuth) from coordinate `a` to `b`
/// in degrees (0-360).
fn bearing_to(a: &Coordinate, b: &Coordinate) -> f64 {
    let lat1 = a.lat * DEG_TO_RAD;
    let lat2 = b.lat * DEG_TO_RAD;
    let d_lon = (b.lon - a.lon) * DEG_TO_RAD;

    let x = d_lon.sin() * lat2.cos();
    let y = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * d_lon.cos();
    let theta = x.atan2(y) * RAD_TO_DEG;
    (theta + 360.0) % 360.0
}

/// Convert a distance in km to the current unit.
fn convert_distance(km: f64, unit: DistanceUnit) -> f64 {
    match unit {
        DistanceUnit::Kilometers => km,
        DistanceUnit::Miles => km * KM_TO_MILES,
    }
}

/// Unit abbreviation string.
fn unit_label(unit: DistanceUnit) -> &'static str {
    match unit {
        DistanceUnit::Kilometers => "km",
        DistanceUnit::Miles => "mi",
    }
}

// ── Cardinal direction helpers ──────────────────────────────────────

/// Return the 16-point cardinal/intercardinal name for a heading in degrees.
fn cardinal_direction(heading: f64) -> &'static str {
    let h = ((heading % 360.0) + 360.0) % 360.0;
    match h as u32 {
        349..=360 | 0..=11 => "N",
        12..=33 => "NNE",
        34..=56 => "NE",
        57..=78 => "ENE",
        79..=101 => "E",
        102..=123 => "ESE",
        124..=146 => "SE",
        147..=168 => "SSE",
        169..=191 => "S",
        192..=213 => "SSW",
        214..=236 => "SW",
        237..=258 => "WSW",
        259..=281 => "W",
        282..=303 => "WNW",
        304..=326 => "NW",
        327..=348 => "NNW",
        _ => "N",
    }
}

/// Return the simple 8-point cardinal for rendering labels on the compass face.
fn cardinal_label_for_angle(deg: u32) -> Option<&'static str> {
    match deg {
        0 => Some("N"),
        45 => Some("NE"),
        90 => Some("E"),
        135 => Some("SE"),
        180 => Some("S"),
        225 => Some("SW"),
        270 => Some("W"),
        315 => Some("NW"),
        _ => None,
    }
}

// ── Trig helpers (f32) ─────────────────────────────────────────────

/// Sine for degrees (f32).
fn sin_deg(deg: f32) -> f32 {
    (deg as f64 * DEG_TO_RAD).sin() as f32
}

/// Cosine for degrees (f32).
fn cos_deg(deg: f32) -> f32 {
    (deg as f64 * DEG_TO_RAD).cos() as f32
}

// ── Application state ──────────────────────────────────────────────

struct CompassApp {
    /// Current compass heading in degrees (0-359). This is the *magnetic* heading
    /// before declination is applied.
    heading: f64,
    /// Magnetic declination offset in degrees (-30 to +30).
    declination: f64,
    /// Current simulated position.
    position: Coordinate,
    /// Active view.
    view: View,
    /// Distance display unit.
    distance_unit: DistanceUnit,
    /// Saved waypoints.
    waypoints: Vec<Waypoint>,
    /// Index of the currently selected waypoint (if any).
    selected_waypoint: Option<usize>,
    /// Which coordinate field is active in coordinate entry view.
    active_coord_field: CoordField,
    /// Text buffer for coordinate entry: latitude.
    entry_lat_buf: String,
    /// Text buffer for coordinate entry: longitude.
    entry_lon_buf: String,
    /// Text buffer for waypoint name in coordinate entry view.
    entry_name_buf: String,
    /// Status message shown at the bottom.
    status: String,
    /// Scroll offset in waypoint list view.
    wp_scroll: usize,
}

impl CompassApp {
    fn new() -> Self {
        Self {
            heading: 0.0,
            declination: 0.0,
            position: Coordinate::new(40.7128, -74.0060), // New York City
            view: View::Compass,
            distance_unit: DistanceUnit::Kilometers,
            waypoints: Vec::new(),
            selected_waypoint: None,
            active_coord_field: CoordField::Latitude,
            entry_lat_buf: String::new(),
            entry_lon_buf: String::new(),
            entry_name_buf: String::new(),
            status: String::from("Digital Compass"),
            wp_scroll: 0,
        }
    }

    /// True heading = magnetic heading + declination.
    fn true_heading(&self) -> f64 {
        let h = self.heading + self.declination;
        ((h % 360.0) + 360.0) % 360.0
    }

    /// Rotate the heading by `delta` degrees, wrapping to 0-360.
    fn rotate(&mut self, delta: f64) {
        self.heading = ((self.heading + delta) % 360.0 + 360.0) % 360.0;
    }

    /// Adjust declination, clamped to [-30, 30].
    fn adjust_declination(&mut self, delta: f64) {
        self.declination = (self.declination + delta).clamp(-30.0, 30.0);
    }

    /// Move the simulated position by a small delta in degrees.
    fn move_position(&mut self, d_lat: f64, d_lon: f64) {
        self.position.lat = (self.position.lat + d_lat).clamp(-90.0, 90.0);
        self.position.lon = (self.position.lon + d_lon).clamp(-180.0, 180.0);
    }

    /// Add a waypoint from the entry buffers. Returns `true` on success.
    fn add_waypoint_from_entry(&mut self) -> bool {
        if self.waypoints.len() >= MAX_WAYPOINTS {
            self.status = String::from("Maximum 10 waypoints reached");
            return false;
        }
        let lat: f64 = match self.entry_lat_buf.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                self.status = String::from("Invalid latitude value");
                return false;
            }
        };
        let lon: f64 = match self.entry_lon_buf.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                self.status = String::from("Invalid longitude value");
                return false;
            }
        };
        if !(-90.0..=90.0).contains(&lat) {
            self.status = String::from("Latitude must be between -90 and 90");
            return false;
        }
        if !(-180.0..=180.0).contains(&lon) {
            self.status = String::from("Longitude must be between -180 and 180");
            return false;
        }
        let name = if self.entry_name_buf.trim().is_empty() {
            format!("WP{}", self.waypoints.len() + 1)
        } else {
            self.entry_name_buf.trim().to_string()
        };
        self.waypoints.push(Waypoint {
            name,
            coord: Coordinate::new(lat, lon),
        });
        self.selected_waypoint = Some(self.waypoints.len() - 1);
        self.entry_lat_buf.clear();
        self.entry_lon_buf.clear();
        self.entry_name_buf.clear();
        self.status = String::from("Waypoint added");
        true
    }

    /// Add a waypoint at the current position.
    fn add_waypoint_at_current_position(&mut self) -> bool {
        if self.waypoints.len() >= MAX_WAYPOINTS {
            self.status = String::from("Maximum 10 waypoints reached");
            return false;
        }
        let name = format!("WP{}", self.waypoints.len() + 1);
        self.waypoints.push(Waypoint {
            name,
            coord: Coordinate::new(self.position.lat, self.position.lon),
        });
        self.selected_waypoint = Some(self.waypoints.len() - 1);
        self.status = String::from("Waypoint added at current position");
        true
    }

    /// Remove the selected waypoint.
    fn remove_selected_waypoint(&mut self) {
        if let Some(idx) = self.selected_waypoint
            && idx < self.waypoints.len() {
                self.waypoints.remove(idx);
                if self.waypoints.is_empty() {
                    self.selected_waypoint = None;
                } else if idx >= self.waypoints.len() {
                    self.selected_waypoint = Some(self.waypoints.len() - 1);
                }
                self.status = String::from("Waypoint removed");
            }
    }

    /// Bearing and distance from the current position to the selected waypoint.
    fn waypoint_bearing_distance(&self) -> Option<(f64, f64)> {
        let idx = self.selected_waypoint?;
        let wp = self.waypoints.get(idx)?;
        let dist_km = haversine_distance(&self.position, &wp.coord);
        let brg = bearing_to(&self.position, &wp.coord);
        Some((brg, dist_km))
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me),
            _ => {}
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = event.kind {
            // Waypoint list click detection
            if self.view == View::Waypoints {
                let list_y_start: f32 = 100.0;
                let row_h: f32 = 36.0;
                if event.x >= 40.0 && event.x <= 860.0 && event.y >= list_y_start {
                    let row = ((event.y - list_y_start) / row_h) as usize + self.wp_scroll;
                    if row < self.waypoints.len() {
                        self.selected_waypoint = Some(row);
                        self.status = format!(
                            "Selected: {}",
                            self.waypoints.get(row).map_or("?", |w| w.name.as_str())
                        );
                    }
                }
            }
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        let shift = event.modifiers.shift;

        match self.view {
            View::Compass => self.handle_key_compass(event, shift),
            View::Waypoints => self.handle_key_waypoints(event),
            View::CoordinateEntry => self.handle_key_coord_entry(event),
        }
    }

    fn handle_key_compass(&mut self, event: &KeyEvent, shift: bool) {
        let step = if shift { 10.0 } else { 1.0 };
        match event.key {
            Key::Left => self.rotate(-step),
            Key::Right => self.rotate(step),
            Key::Up => self.move_position(0.01, 0.0),
            Key::Down => self.move_position(-0.01, 0.0),
            Key::D => {
                if event.modifiers.ctrl {
                    // Ctrl+D: switch to magnetic declination adjust mode
                    self.adjust_declination(if shift { 5.0 } else { 1.0 });
                    self.status = format!("Declination: {:+.0}", self.declination);
                } else {
                    self.adjust_declination(if shift { -5.0 } else { -1.0 });
                    self.status = format!("Declination: {:+.0}", self.declination);
                }
            }
            Key::U => {
                self.distance_unit = match self.distance_unit {
                    DistanceUnit::Kilometers => DistanceUnit::Miles,
                    DistanceUnit::Miles => DistanceUnit::Kilometers,
                };
                self.status = format!("Units: {}", unit_label(self.distance_unit));
            }
            Key::W => {
                self.view = View::Waypoints;
                self.status = String::from("Waypoint List");
            }
            Key::C => {
                self.view = View::CoordinateEntry;
                self.active_coord_field = CoordField::Latitude;
                self.status = String::from("Coordinate Entry");
            }
            Key::M => {
                self.add_waypoint_at_current_position();
            }
            Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 | Key::Num5 | Key::Num6 | Key::Num7
            | Key::Num8 | Key::Num9 | Key::Num0 => {
                let digit = match event.key {
                    Key::Num1 => 0,
                    Key::Num2 => 1,
                    Key::Num3 => 2,
                    Key::Num4 => 3,
                    Key::Num5 => 4,
                    Key::Num6 => 5,
                    Key::Num7 => 6,
                    Key::Num8 => 7,
                    Key::Num9 => 8,
                    Key::Num0 => 9,
                    _ => return,
                };
                if digit < self.waypoints.len() {
                    self.selected_waypoint = Some(digit);
                    self.status = format!(
                        "Selected: {}",
                        self.waypoints.get(digit).map_or("?", |w| w.name.as_str())
                    );
                }
            }
            Key::Escape => {
                self.selected_waypoint = None;
                self.status = String::from("Digital Compass");
            }
            _ => {}
        }
    }

    fn handle_key_waypoints(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Escape => {
                self.view = View::Compass;
                self.status = String::from("Digital Compass");
            }
            Key::Up => {
                if let Some(idx) = self.selected_waypoint {
                    if idx > 0 {
                        self.selected_waypoint = Some(idx - 1);
                    }
                } else if !self.waypoints.is_empty() {
                    self.selected_waypoint = Some(0);
                }
            }
            Key::Down => {
                if let Some(idx) = self.selected_waypoint {
                    if idx + 1 < self.waypoints.len() {
                        self.selected_waypoint = Some(idx + 1);
                    }
                } else if !self.waypoints.is_empty() {
                    self.selected_waypoint = Some(0);
                }
            }
            Key::Delete | Key::Backspace => {
                self.remove_selected_waypoint();
            }
            Key::Enter => {
                self.view = View::Compass;
                self.status = String::from("Digital Compass");
            }
            Key::C => {
                self.view = View::CoordinateEntry;
                self.active_coord_field = CoordField::Latitude;
                self.status = String::from("Coordinate Entry");
            }
            _ => {}
        }
    }

    fn handle_key_coord_entry(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Escape => {
                self.view = View::Compass;
                self.status = String::from("Digital Compass");
            }
            Key::Tab => {
                self.active_coord_field = match self.active_coord_field {
                    CoordField::Latitude => CoordField::Longitude,
                    CoordField::Longitude => CoordField::Latitude,
                };
            }
            Key::Enter => {
                self.add_waypoint_from_entry();
            }
            Key::Backspace => {
                let buf = match self.active_coord_field {
                    CoordField::Latitude => &mut self.entry_lat_buf,
                    CoordField::Longitude => &mut self.entry_lon_buf,
                };
                buf.pop();
            }
            // Digit and symbol keys for numeric entry
            key => {
                let ch = key_to_char(key, event.modifiers.shift);
                if let Some(c) = ch {
                    let buf = match self.active_coord_field {
                        CoordField::Latitude => &mut self.entry_lat_buf,
                        CoordField::Longitude => &mut self.entry_lon_buf,
                    };
                    if buf.len() < 16 {
                        buf.push(c);
                    }
                }
            }
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        // Full background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        match self.view {
            View::Compass => self.render_compass_view(&mut cmds),
            View::Waypoints => self.render_waypoint_view(&mut cmds),
            View::CoordinateEntry => self.render_coord_entry_view(&mut cmds),
        }

        // Status bar at bottom
        self.render_status_bar(&mut cmds);

        cmds
    }

    fn render_compass_view(&self, cmds: &mut Vec<RenderCommand>) {
        // Title
        cmds.push(RenderCommand::Text {
            x: COMPASS_CX - 60.0,
            y: 16.0,
            text: String::from("Digital Compass"),
            color: LAVENDER,
            font_size: 22.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        self.render_compass_rose(cmds);
        self.render_heading_display(cmds);
        self.render_coordinate_display(cmds);
        self.render_declination_display(cmds);
        self.render_waypoint_info_panel(cmds);
        self.render_compass_help(cmds);
    }

    fn render_compass_rose(&self, cmds: &mut Vec<RenderCommand>) {
        let cx = COMPASS_CX;
        let cy = COMPASS_CY;
        let r = COMPASS_RADIUS;
        let heading_f32 = self.true_heading() as f32;

        // Outer ring (dark background circle approximated with rounded rect)
        cmds.push(RenderCommand::FillRect {
            x: cx - r,
            y: cy - r,
            width: r * 2.0,
            height: r * 2.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(r),
        });

        // Outer circle border
        cmds.push(RenderCommand::StrokeRect {
            x: cx - r,
            y: cy - r,
            width: r * 2.0,
            height: r * 2.0,
            color: SURFACE1,
            line_width: 2.0,
            corner_radii: CornerRadii::all(r),
        });

        // Inner ring
        let inner_r = r - 30.0;
        cmds.push(RenderCommand::StrokeRect {
            x: cx - inner_r,
            y: cy - inner_r,
            width: inner_r * 2.0,
            height: inner_r * 2.0,
            color: SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(inner_r),
        });

        // Tick marks every 10 degrees, with longer ticks at 30-degree intervals
        for deg_i in 0..36 {
            let deg = deg_i * 10;
            // Rotate tick by subtracting heading so compass rotates
            let angle = deg as f32 - heading_f32;
            let is_major = deg % 30 == 0;
            let tick_inner = if is_major { r - 28.0 } else { r - 16.0 };
            let tick_outer = r - 4.0;

            let x1 = cx + sin_deg(angle) * tick_inner;
            let y1 = cy - cos_deg(angle) * tick_inner;
            let x2 = cx + sin_deg(angle) * tick_outer;
            let y2 = cy - cos_deg(angle) * tick_outer;

            let tick_color = if is_major { TEXT_COLOR } else { OVERLAY0 };

            cmds.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: tick_color,
                width: if is_major { 2.0 } else { 1.0 },
            });
        }

        // Degree labels every 30 degrees
        for deg_i in 0..12 {
            let deg = deg_i * 30;
            let angle = deg as f32 - heading_f32;
            let label_r = r - 44.0;
            let lx = cx + sin_deg(angle) * label_r - 10.0;
            let ly = cy - cos_deg(angle) * label_r - 8.0;

            cmds.push(RenderCommand::Text {
                x: lx,
                y: ly,
                text: format!("{deg}"),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(30.0),
            });
        }

        // Cardinal and intercardinal labels
        let labels: &[(u32, &str, Color, f32)] = &[
            (0, "N", RED, 18.0),
            (45, "NE", SUBTEXT0, 13.0),
            (90, "E", TEXT_COLOR, 18.0),
            (135, "SE", SUBTEXT0, 13.0),
            (180, "S", TEXT_COLOR, 18.0),
            (225, "SW", SUBTEXT0, 13.0),
            (270, "W", TEXT_COLOR, 18.0),
            (315, "NW", SUBTEXT0, 13.0),
        ];
        for &(deg, label, color, size) in labels {
            let angle = deg as f32 - heading_f32;
            let label_r = r - 64.0;
            // Center text roughly by offsetting
            let offset_x = label.len() as f32 * size * 0.25;
            let lx = cx + sin_deg(angle) * label_r - offset_x;
            let ly = cy - cos_deg(angle) * label_r - size * 0.5;

            cmds.push(RenderCommand::Text {
                x: lx,
                y: ly,
                text: String::from(label),
                color,
                font_size: size,
                font_weight: if size > 14.0 {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }

        // North needle (red line from center toward north)
        // North is at 0 degrees, rotated by -heading
        let needle_angle = -heading_f32;
        let needle_len = r - 70.0;
        let nx = cx + sin_deg(needle_angle) * needle_len;
        let ny = cy - cos_deg(needle_angle) * needle_len;

        // Needle: red half (north-pointing)
        cmds.push(RenderCommand::Line {
            x1: cx,
            y1: cy,
            x2: nx,
            y2: ny,
            color: RED,
            width: 3.0,
        });

        // Needle: opposite half (south, dimmer)
        let sx = cx - sin_deg(needle_angle) * (needle_len * 0.6);
        let sy = cy + cos_deg(needle_angle) * (needle_len * 0.6);
        cmds.push(RenderCommand::Line {
            x1: cx,
            y1: cy,
            x2: sx,
            y2: sy,
            color: SURFACE2,
            width: 2.0,
        });

        // Center dot
        let dot_r: f32 = 6.0;
        cmds.push(RenderCommand::FillRect {
            x: cx - dot_r,
            y: cy - dot_r,
            width: dot_r * 2.0,
            height: dot_r * 2.0,
            color: RED,
            corner_radii: CornerRadii::all(dot_r),
        });

        // Bearing indicator triangle at top (fixed pointer showing where north is)
        let tri_y = cy - r - 8.0;
        cmds.push(RenderCommand::Line {
            x1: cx,
            y1: tri_y,
            x2: cx - 8.0,
            y2: tri_y - 14.0,
            color: PEACH,
            width: 2.0,
        });
        cmds.push(RenderCommand::Line {
            x1: cx,
            y1: tri_y,
            x2: cx + 8.0,
            y2: tri_y - 14.0,
            color: PEACH,
            width: 2.0,
        });
        cmds.push(RenderCommand::Line {
            x1: cx - 8.0,
            y1: tri_y - 14.0,
            x2: cx + 8.0,
            y2: tri_y - 14.0,
            color: PEACH,
            width: 2.0,
        });
    }

    fn render_heading_display(&self, cmds: &mut Vec<RenderCommand>) {
        let true_h = self.true_heading();
        let cardinal = cardinal_direction(true_h);

        // Heading box
        let hx: f32 = 680.0;
        let hy: f32 = 60.0;
        cmds.push(RenderCommand::FillRect {
            x: hx,
            y: hy,
            width: 200.0,
            height: 80.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: hx + 12.0,
            y: hy + 8.0,
            text: String::from("HEADING"),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: hx + 12.0,
            y: hy + 28.0,
            text: format!("{:.0}", true_h),
            color: BLUE,
            font_size: 32.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: hx + 100.0,
            y: hy + 28.0,
            text: String::from(cardinal),
            color: GREEN,
            font_size: 28.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_coordinate_display(&self, cmds: &mut Vec<RenderCommand>) {
        let bx: f32 = 680.0;
        let by: f32 = 160.0;
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: 200.0,
            height: 80.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: bx + 12.0,
            y: by + 8.0,
            text: String::from("POSITION"),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: bx + 12.0,
            y: by + 30.0,
            text: self.position.format_lat(),
            color: TEXT_COLOR,
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: bx + 12.0,
            y: by + 52.0,
            text: self.position.format_lon(),
            color: TEXT_COLOR,
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_declination_display(&self, cmds: &mut Vec<RenderCommand>) {
        let bx: f32 = 680.0;
        let by: f32 = 260.0;
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: 200.0,
            height: 56.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: bx + 12.0,
            y: by + 8.0,
            text: String::from("DECLINATION"),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: bx + 12.0,
            y: by + 28.0,
            text: format!("{:+.0}", self.declination),
            color: YELLOW,
            font_size: 20.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_waypoint_info_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let bx: f32 = 680.0;
        let by: f32 = 336.0;
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: 200.0,
            height: 130.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: bx + 12.0,
            y: by + 8.0,
            text: String::from("WAYPOINT"),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        if let Some(idx) = self.selected_waypoint {
            if let Some(wp) = self.waypoints.get(idx) {
                cmds.push(RenderCommand::Text {
                    x: bx + 12.0,
                    y: by + 28.0,
                    text: wp.name.clone(),
                    color: TEAL,
                    font_size: 16.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(180.0),
                });

                cmds.push(RenderCommand::Text {
                    x: bx + 12.0,
                    y: by + 50.0,
                    text: format!("{} {}", wp.coord.format_lat(), wp.coord.format_lon()),
                    color: TEXT_COLOR,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(180.0),
                });

                if let Some((brg, dist_km)) = self.waypoint_bearing_distance() {
                    let dist = convert_distance(dist_km, self.distance_unit);
                    let lbl = unit_label(self.distance_unit);

                    cmds.push(RenderCommand::Text {
                        x: bx + 12.0,
                        y: by + 72.0,
                        text: format!("BRG: {brg:.0}"),
                        color: PEACH,
                        font_size: 15.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });

                    cmds.push(RenderCommand::Text {
                        x: bx + 12.0,
                        y: by + 94.0,
                        text: format!("DST: {dist:.1} {lbl}"),
                        color: PEACH,
                        font_size: 15.0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: bx + 12.0,
                y: by + 40.0,
                text: String::from("No waypoint"),
                color: OVERLAY0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: bx + 12.0,
                y: by + 60.0,
                text: String::from("selected"),
                color: OVERLAY0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_compass_help(&self, cmds: &mut Vec<RenderCommand>) {
        let bx: f32 = 680.0;
        let by: f32 = 486.0;
        let lines: &[&str] = &[
            "Left/Right: Rotate",
            "Shift+L/R: Rotate 10",
            "Up/Down: Move position",
            "D: Declination -1",
            "Ctrl+D: Declination +1",
            "U: Toggle km/mi",
            "M: Mark waypoint",
            "1-0: Select waypoint",
            "W: Waypoint list",
            "C: Coordinate entry",
        ];

        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: 200.0,
            height: (lines.len() as f32) * 18.0 + 16.0,
            color: CRUST,
            corner_radii: CornerRadii::all(8.0),
        });

        for (i, line) in lines.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: bx + 10.0,
                y: by + 8.0 + i as f32 * 18.0,
                text: String::from(*line),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(185.0),
            });
        }
    }

    fn render_waypoint_view(&self, cmds: &mut Vec<RenderCommand>) {
        // Title
        cmds.push(RenderCommand::Text {
            x: 40.0,
            y: 20.0,
            text: String::from("Waypoints"),
            color: LAVENDER,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Column headers
        cmds.push(RenderCommand::Text {
            x: 40.0,
            y: 68.0,
            text: String::from("#  Name          Lat          Lon          Bearing    Distance"),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Line {
            x1: 40.0,
            y1: 90.0,
            x2: 860.0,
            y2: 90.0,
            color: SURFACE1,
            width: 1.0,
        });

        if self.waypoints.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 40.0,
                y: 110.0,
                text: String::from("No waypoints. Press C to add one, or Esc to go back."),
                color: OVERLAY0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        let row_h: f32 = 36.0;
        let start_y: f32 = 100.0;

        for (i, wp) in self.waypoints.iter().enumerate() {
            let y = start_y + i as f32 * row_h;
            let is_selected = self.selected_waypoint == Some(i);

            // Row highlight
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 36.0,
                    y,
                    width: 828.0,
                    height: row_h - 2.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let text_color = if is_selected { BLUE } else { TEXT_COLOR };

            let dist_km = haversine_distance(&self.position, &wp.coord);
            let dist = convert_distance(dist_km, self.distance_unit);
            let brg = bearing_to(&self.position, &wp.coord);
            let lbl = unit_label(self.distance_unit);

            let row_text = format!(
                "{}  {:<14} {:<12} {:<12} {:<10.0} {:.1} {}",
                i + 1,
                wp.name,
                wp.coord.format_lat(),
                wp.coord.format_lon(),
                brg,
                dist,
                lbl,
            );

            cmds.push(RenderCommand::Text {
                x: 40.0,
                y: y + 8.0,
                text: row_text,
                color: text_color,
                font_size: 13.0,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(820.0),
            });
        }

        // Help text at bottom
        let help_y = start_y + self.waypoints.len() as f32 * row_h + 20.0;
        cmds.push(RenderCommand::Text {
            x: 40.0,
            y: help_y,
            text: String::from("Up/Down: select  |  Del: remove  |  C: add new  |  Enter/Esc: back"),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_coord_entry_view(&self, cmds: &mut Vec<RenderCommand>) {
        // Title
        cmds.push(RenderCommand::Text {
            x: 40.0,
            y: 20.0,
            text: String::from("Add Waypoint"),
            color: LAVENDER,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let field_w: f32 = 300.0;
        let field_h: f32 = 40.0;
        let start_x: f32 = 40.0;

        // Name field
        cmds.push(RenderCommand::Text {
            x: start_x,
            y: 80.0,
            text: String::from("Name (optional):"),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: start_x,
            y: 100.0,
            width: field_w,
            height: field_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: start_x + 10.0,
            y: 110.0,
            text: if self.entry_name_buf.is_empty() {
                String::from("WP auto-name")
            } else {
                self.entry_name_buf.clone()
            },
            color: if self.entry_name_buf.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 20.0),
        });

        // Latitude field
        let lat_active = self.active_coord_field == CoordField::Latitude;
        cmds.push(RenderCommand::Text {
            x: start_x,
            y: 160.0,
            text: String::from("Latitude (-90 to 90):"),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: start_x,
            y: 180.0,
            width: field_w,
            height: field_h,
            color: if lat_active { SURFACE1 } else { SURFACE0 },
            corner_radii: CornerRadii::all(6.0),
        });
        if lat_active {
            cmds.push(RenderCommand::StrokeRect {
                x: start_x,
                y: 180.0,
                width: field_w,
                height: field_h,
                color: BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }
        cmds.push(RenderCommand::Text {
            x: start_x + 10.0,
            y: 190.0,
            text: if self.entry_lat_buf.is_empty() {
                String::from("e.g. 48.8566")
            } else {
                self.entry_lat_buf.clone()
            },
            color: if self.entry_lat_buf.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 20.0),
        });

        // Longitude field
        let lon_active = self.active_coord_field == CoordField::Longitude;
        cmds.push(RenderCommand::Text {
            x: start_x,
            y: 240.0,
            text: String::from("Longitude (-180 to 180):"),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: start_x,
            y: 260.0,
            width: field_w,
            height: field_h,
            color: if lon_active { SURFACE1 } else { SURFACE0 },
            corner_radii: CornerRadii::all(6.0),
        });
        if lon_active {
            cmds.push(RenderCommand::StrokeRect {
                x: start_x,
                y: 260.0,
                width: field_w,
                height: field_h,
                color: BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }
        cmds.push(RenderCommand::Text {
            x: start_x + 10.0,
            y: 270.0,
            text: if self.entry_lon_buf.is_empty() {
                String::from("e.g. 2.3522")
            } else {
                self.entry_lon_buf.clone()
            },
            color: if self.entry_lon_buf.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 20.0),
        });

        // Submit button
        cmds.push(RenderCommand::FillRect {
            x: start_x,
            y: 320.0,
            width: 140.0,
            height: 36.0,
            color: BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: start_x + 20.0,
            y: 328.0,
            text: String::from("Add (Enter)"),
            color: CRUST,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Help
        cmds.push(RenderCommand::Text {
            x: start_x,
            y: 380.0,
            text: String::from("Tab: switch field  |  Enter: add waypoint  |  Esc: cancel"),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let bar_y = WINDOW_HEIGHT - 32.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: WINDOW_WIDTH,
            height: 32.0,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: bar_y + 8.0,
            text: self.status.clone(),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(WINDOW_WIDTH - 24.0),
        });

        // Unit indicator on right
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 80.0,
            y: bar_y + 8.0,
            text: format!("[{}]", unit_label(self.distance_unit)),
            color: OVERLAY0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

/// Map a `Key` to an ASCII character suitable for numeric coordinate entry.
fn key_to_char(key: Key, _shift: bool) -> Option<char> {
    match key {
        Key::Num0 => Some('0'),
        Key::Num1 => Some('1'),
        Key::Num2 => Some('2'),
        Key::Num3 => Some('3'),
        Key::Num4 => Some('4'),
        Key::Num5 => Some('5'),
        Key::Num6 => Some('6'),
        Key::Num7 => Some('7'),
        Key::Num8 => Some('8'),
        Key::Num9 => Some('9'),
        // Period / decimal point -- mapped from the Period key
        Key::A => Some('.'),
        // Minus sign -- use the M key as a fallback since Key layout varies
        Key::B => Some('-'),
        _ => None,
    }
}

fn main() {
    let _app = CompassApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;

    // ── Helpers ─────────────────────────────────────────────────────

    fn default_app() -> CompassApp {
        CompassApp::new()
    }

    fn make_key_event(key: Key, shift: bool, ctrl: bool) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers {
                shift,
                ctrl,
                alt: false,
                super_key: false,
            },
            text: None,
        }
    }

    fn make_release_event(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers {
                shift: false,
                ctrl: false,
                alt: false,
                super_key: false,
            },
            text: None,
        }
    }

    // ── Heading tests ───────────────────────────────────────────────

    #[test]
    fn test_initial_heading() {
        let app = default_app();
        assert!((app.heading - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rotate_right() {
        let mut app = default_app();
        app.rotate(45.0);
        assert!((app.heading - 45.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rotate_left() {
        let mut app = default_app();
        app.rotate(-10.0);
        assert!((app.heading - 350.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rotate_wrap_360() {
        let mut app = default_app();
        app.rotate(370.0);
        assert!((app.heading - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_rotate_negative_wrap() {
        let mut app = default_app();
        app.heading = 5.0;
        app.rotate(-20.0);
        assert!((app.heading - 345.0).abs() < 0.001);
    }

    #[test]
    fn test_rotate_full_circle() {
        let mut app = default_app();
        for _ in 0..360 {
            app.rotate(1.0);
        }
        assert!(app.heading.abs() < 0.001);
    }

    #[test]
    fn test_true_heading_no_declination() {
        let app = default_app();
        assert!((app.true_heading() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_true_heading_with_declination() {
        let mut app = default_app();
        app.heading = 90.0;
        app.declination = 10.0;
        assert!((app.true_heading() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_true_heading_negative_declination() {
        let mut app = default_app();
        app.heading = 10.0;
        app.declination = -15.0;
        assert!((app.true_heading() - 355.0).abs() < 0.001);
    }

    // ── Cardinal direction tests ────────────────────────────────────

    #[test]
    fn test_cardinal_north() {
        assert_eq!(cardinal_direction(0.0), "N");
        assert_eq!(cardinal_direction(360.0), "N");
        assert_eq!(cardinal_direction(5.0), "N");
        assert_eq!(cardinal_direction(355.0), "N");
    }

    #[test]
    fn test_cardinal_east() {
        assert_eq!(cardinal_direction(90.0), "E");
    }

    #[test]
    fn test_cardinal_south() {
        assert_eq!(cardinal_direction(180.0), "S");
    }

    #[test]
    fn test_cardinal_west() {
        assert_eq!(cardinal_direction(270.0), "W");
    }

    #[test]
    fn test_cardinal_ne() {
        assert_eq!(cardinal_direction(45.0), "NE");
    }

    #[test]
    fn test_cardinal_se() {
        assert_eq!(cardinal_direction(135.0), "SE");
    }

    #[test]
    fn test_cardinal_sw() {
        assert_eq!(cardinal_direction(225.0), "SW");
    }

    #[test]
    fn test_cardinal_nw() {
        assert_eq!(cardinal_direction(315.0), "NW");
    }

    #[test]
    fn test_cardinal_nne() {
        assert_eq!(cardinal_direction(22.5), "NNE");
    }

    #[test]
    fn test_cardinal_sse() {
        assert_eq!(cardinal_direction(157.0), "SSE");
    }

    #[test]
    fn test_cardinal_wnw() {
        assert_eq!(cardinal_direction(300.0), "WNW");
    }

    #[test]
    fn test_cardinal_negative_heading() {
        assert_eq!(cardinal_direction(-90.0), "W");
    }

    #[test]
    fn test_cardinal_label_for_angle() {
        assert_eq!(cardinal_label_for_angle(0), Some("N"));
        assert_eq!(cardinal_label_for_angle(90), Some("E"));
        assert_eq!(cardinal_label_for_angle(180), Some("S"));
        assert_eq!(cardinal_label_for_angle(270), Some("W"));
        assert_eq!(cardinal_label_for_angle(45), Some("NE"));
        assert_eq!(cardinal_label_for_angle(30), None);
    }

    // ── Coordinate tests ────────────────────────────────────────────

    #[test]
    fn test_coordinate_clamp_lat() {
        let c = Coordinate::new(100.0, 0.0);
        assert!((c.lat - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coordinate_clamp_lat_negative() {
        let c = Coordinate::new(-100.0, 0.0);
        assert!((c.lat - -90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coordinate_clamp_lon() {
        let c = Coordinate::new(0.0, 200.0);
        assert!((c.lon - 180.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coordinate_format_lat_north() {
        let c = Coordinate::new(40.7128, 0.0);
        assert_eq!(c.format_lat(), "40.7128N");
    }

    #[test]
    fn test_coordinate_format_lat_south() {
        let c = Coordinate::new(-33.8688, 0.0);
        assert_eq!(c.format_lat(), "33.8688S");
    }

    #[test]
    fn test_coordinate_format_lon_east() {
        let c = Coordinate::new(0.0, 151.2093);
        assert_eq!(c.format_lon(), "151.2093E");
    }

    #[test]
    fn test_coordinate_format_lon_west() {
        let c = Coordinate::new(0.0, -74.006);
        assert_eq!(c.format_lon(), "74.0060W");
    }

    // ── Haversine distance tests ────────────────────────────────────

    #[test]
    fn test_haversine_same_point() {
        let a = Coordinate::new(40.0, -74.0);
        let dist = haversine_distance(&a, &a);
        assert!(dist.abs() < 0.001);
    }

    #[test]
    fn test_haversine_new_york_to_london() {
        // NYC (40.7128, -74.0060) to London (51.5074, -0.1278)
        let nyc = Coordinate::new(40.7128, -74.0060);
        let london = Coordinate::new(51.5074, -0.1278);
        let dist = haversine_distance(&nyc, &london);
        // Expected ~5570 km
        assert!((dist - 5570.0).abs() < 30.0);
    }

    #[test]
    fn test_haversine_antipodal() {
        // North pole to south pole
        let np = Coordinate::new(90.0, 0.0);
        let sp = Coordinate::new(-90.0, 0.0);
        let dist = haversine_distance(&np, &sp);
        // Half circumference ~20015 km
        assert!((dist - 20015.0).abs() < 20.0);
    }

    #[test]
    fn test_haversine_equator_quarter() {
        // 0,0 to 0,90 -- quarter equator
        let a = Coordinate::new(0.0, 0.0);
        let b = Coordinate::new(0.0, 90.0);
        let dist = haversine_distance(&a, &b);
        // ~10018 km
        assert!((dist - 10018.0).abs() < 20.0);
    }

    #[test]
    fn test_haversine_short_distance() {
        // Two close points ~ 1 degree apart along equator
        let a = Coordinate::new(0.0, 0.0);
        let b = Coordinate::new(0.0, 1.0);
        let dist = haversine_distance(&a, &b);
        // ~111 km
        assert!((dist - 111.0).abs() < 2.0);
    }

    #[test]
    fn test_haversine_symmetry() {
        let a = Coordinate::new(35.0, 139.0);
        let b = Coordinate::new(48.0, 2.0);
        let d1 = haversine_distance(&a, &b);
        let d2 = haversine_distance(&b, &a);
        assert!((d1 - d2).abs() < 0.001);
    }

    // ── Bearing tests ───────────────────────────────────────────────

    #[test]
    fn test_bearing_due_north() {
        let a = Coordinate::new(0.0, 0.0);
        let b = Coordinate::new(10.0, 0.0);
        let brg = bearing_to(&a, &b);
        assert!((brg - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_bearing_due_east() {
        let a = Coordinate::new(0.0, 0.0);
        let b = Coordinate::new(0.0, 10.0);
        let brg = bearing_to(&a, &b);
        assert!((brg - 90.0).abs() < 0.1);
    }

    #[test]
    fn test_bearing_due_south() {
        let a = Coordinate::new(10.0, 0.0);
        let b = Coordinate::new(0.0, 0.0);
        let brg = bearing_to(&a, &b);
        assert!((brg - 180.0).abs() < 0.1);
    }

    #[test]
    fn test_bearing_due_west() {
        let a = Coordinate::new(0.0, 10.0);
        let b = Coordinate::new(0.0, 0.0);
        let brg = bearing_to(&a, &b);
        assert!((brg - 270.0).abs() < 0.1);
    }

    #[test]
    fn test_bearing_range() {
        // Bearing should always be in [0, 360)
        for lat in &[-45.0_f64, 0.0, 30.0, 60.0] {
            for lon in &[-120.0_f64, 0.0, 90.0, 170.0] {
                let a = Coordinate::new(*lat, *lon);
                let b = Coordinate::new(lat + 5.0, lon + 5.0);
                let brg = bearing_to(&a, &b);
                assert!(brg >= 0.0 && brg < 360.0, "bearing out of range: {brg}");
            }
        }
    }

    // ── Waypoint management tests ───────────────────────────────────

    #[test]
    fn test_initial_no_waypoints() {
        let app = default_app();
        assert!(app.waypoints.is_empty());
        assert!(app.selected_waypoint.is_none());
    }

    #[test]
    fn test_add_waypoint_at_current_position() {
        let mut app = default_app();
        assert!(app.add_waypoint_at_current_position());
        assert_eq!(app.waypoints.len(), 1);
        assert_eq!(app.selected_waypoint, Some(0));
        assert!((app.waypoints[0].coord.lat - 40.7128).abs() < 0.001);
    }

    #[test]
    fn test_add_waypoint_from_entry() {
        let mut app = default_app();
        app.entry_lat_buf = String::from("48.8566");
        app.entry_lon_buf = String::from("2.3522");
        app.entry_name_buf = String::from("Paris");
        assert!(app.add_waypoint_from_entry());
        assert_eq!(app.waypoints.len(), 1);
        assert_eq!(app.waypoints[0].name, "Paris");
        assert!((app.waypoints[0].coord.lat - 48.8566).abs() < 0.001);
    }

    #[test]
    fn test_add_waypoint_auto_name() {
        let mut app = default_app();
        app.entry_lat_buf = String::from("10.0");
        app.entry_lon_buf = String::from("20.0");
        assert!(app.add_waypoint_from_entry());
        assert_eq!(app.waypoints[0].name, "WP1");
    }

    #[test]
    fn test_add_waypoint_invalid_lat() {
        let mut app = default_app();
        app.entry_lat_buf = String::from("abc");
        app.entry_lon_buf = String::from("0.0");
        assert!(!app.add_waypoint_from_entry());
        assert!(app.waypoints.is_empty());
    }

    #[test]
    fn test_add_waypoint_lat_out_of_range() {
        let mut app = default_app();
        app.entry_lat_buf = String::from("95.0");
        app.entry_lon_buf = String::from("0.0");
        assert!(!app.add_waypoint_from_entry());
    }

    #[test]
    fn test_add_waypoint_lon_out_of_range() {
        let mut app = default_app();
        app.entry_lat_buf = String::from("0.0");
        app.entry_lon_buf = String::from("200.0");
        assert!(!app.add_waypoint_from_entry());
    }

    #[test]
    fn test_max_waypoints() {
        let mut app = default_app();
        for i in 0..MAX_WAYPOINTS {
            app.entry_lat_buf = format!("{}.0", i);
            app.entry_lon_buf = format!("{}.0", i);
            assert!(app.add_waypoint_from_entry());
        }
        assert_eq!(app.waypoints.len(), MAX_WAYPOINTS);
        // 11th should fail
        app.entry_lat_buf = String::from("50.0");
        app.entry_lon_buf = String::from("50.0");
        assert!(!app.add_waypoint_from_entry());
    }

    #[test]
    fn test_max_waypoints_at_position() {
        let mut app = default_app();
        for _ in 0..MAX_WAYPOINTS {
            assert!(app.add_waypoint_at_current_position());
        }
        assert!(!app.add_waypoint_at_current_position());
    }

    #[test]
    fn test_remove_selected_waypoint() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.add_waypoint_at_current_position();
        app.selected_waypoint = Some(0);
        app.remove_selected_waypoint();
        assert_eq!(app.waypoints.len(), 1);
        assert_eq!(app.selected_waypoint, Some(0));
    }

    #[test]
    fn test_remove_last_waypoint() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.selected_waypoint = Some(0);
        app.remove_selected_waypoint();
        assert!(app.waypoints.is_empty());
        assert!(app.selected_waypoint.is_none());
    }

    #[test]
    fn test_remove_waypoint_adjusts_selection() {
        let mut app = default_app();
        for _ in 0..3 {
            app.add_waypoint_at_current_position();
        }
        app.selected_waypoint = Some(2);
        app.remove_selected_waypoint();
        // Selection should clamp to last index
        assert_eq!(app.selected_waypoint, Some(1));
    }

    #[test]
    fn test_waypoint_bearing_distance_none_when_no_selection() {
        let app = default_app();
        assert!(app.waypoint_bearing_distance().is_none());
    }

    #[test]
    fn test_waypoint_bearing_distance_some() {
        let mut app = default_app();
        app.entry_lat_buf = String::from("51.5074");
        app.entry_lon_buf = String::from("-0.1278");
        app.entry_name_buf = String::from("London");
        app.add_waypoint_from_entry();
        let result = app.waypoint_bearing_distance();
        assert!(result.is_some());
        let (brg, dist) = result.unwrap();
        assert!(brg >= 0.0 && brg < 360.0);
        assert!(dist > 5000.0); // NYC to London > 5000 km
    }

    // ── Declination tests ───────────────────────────────────────────

    #[test]
    fn test_declination_initial() {
        let app = default_app();
        assert!((app.declination - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_declination_adjust_positive() {
        let mut app = default_app();
        app.adjust_declination(5.0);
        assert!((app.declination - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_declination_adjust_negative() {
        let mut app = default_app();
        app.adjust_declination(-10.0);
        assert!((app.declination - -10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_declination_clamp_max() {
        let mut app = default_app();
        app.adjust_declination(50.0);
        assert!((app.declination - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_declination_clamp_min() {
        let mut app = default_app();
        app.adjust_declination(-50.0);
        assert!((app.declination - -30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_declination_affects_true_heading() {
        let mut app = default_app();
        app.heading = 350.0;
        app.declination = 20.0;
        assert!((app.true_heading() - 10.0).abs() < 0.001);
    }

    // ── Unit conversion tests ───────────────────────────────────────

    #[test]
    fn test_convert_distance_km() {
        let d = convert_distance(100.0, DistanceUnit::Kilometers);
        assert!((d - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_convert_distance_miles() {
        let d = convert_distance(100.0, DistanceUnit::Miles);
        assert!((d - 62.1371).abs() < 0.001);
    }

    #[test]
    fn test_unit_label_km() {
        assert_eq!(unit_label(DistanceUnit::Kilometers), "km");
    }

    #[test]
    fn test_unit_label_mi() {
        assert_eq!(unit_label(DistanceUnit::Miles), "mi");
    }

    // ── Key handling tests ──────────────────────────────────────────

    #[test]
    fn test_key_rotate_right() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::Right, false, false));
        app.handle_event(&event);
        assert!((app.heading - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_rotate_left() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::Left, false, false));
        app.handle_event(&event);
        assert!((app.heading - 359.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_rotate_right_shift() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::Right, true, false));
        app.handle_event(&event);
        assert!((app.heading - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_rotate_left_shift() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::Left, true, false));
        app.handle_event(&event);
        assert!((app.heading - 350.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = default_app();
        let event = Event::Key(make_release_event(Key::Right));
        app.handle_event(&event);
        assert!((app.heading - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_toggle_units() {
        let mut app = default_app();
        assert_eq!(app.distance_unit, DistanceUnit::Kilometers);
        let event = Event::Key(make_key_event(Key::U, false, false));
        app.handle_event(&event);
        assert_eq!(app.distance_unit, DistanceUnit::Miles);
        app.handle_event(&event);
        assert_eq!(app.distance_unit, DistanceUnit::Kilometers);
    }

    #[test]
    fn test_key_switch_to_waypoints() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::W, false, false));
        app.handle_event(&event);
        assert_eq!(app.view, View::Waypoints);
    }

    #[test]
    fn test_key_switch_to_coord_entry() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::C, false, false));
        app.handle_event(&event);
        assert_eq!(app.view, View::CoordinateEntry);
    }

    #[test]
    fn test_key_escape_from_waypoints() {
        let mut app = default_app();
        app.view = View::Waypoints;
        let event = Event::Key(make_key_event(Key::Escape, false, false));
        app.handle_event(&event);
        assert_eq!(app.view, View::Compass);
    }

    #[test]
    fn test_key_escape_from_coord_entry() {
        let mut app = default_app();
        app.view = View::CoordinateEntry;
        let event = Event::Key(make_key_event(Key::Escape, false, false));
        app.handle_event(&event);
        assert_eq!(app.view, View::Compass);
    }

    #[test]
    fn test_key_mark_waypoint() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::M, false, false));
        app.handle_event(&event);
        assert_eq!(app.waypoints.len(), 1);
    }

    #[test]
    fn test_key_move_position_up() {
        let mut app = default_app();
        let original_lat = app.position.lat;
        let event = Event::Key(make_key_event(Key::Up, false, false));
        app.handle_event(&event);
        assert!(app.position.lat > original_lat);
    }

    #[test]
    fn test_key_move_position_down() {
        let mut app = default_app();
        let original_lat = app.position.lat;
        let event = Event::Key(make_key_event(Key::Down, false, false));
        app.handle_event(&event);
        assert!(app.position.lat < original_lat);
    }

    #[test]
    fn test_key_declination_d() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::D, false, false));
        app.handle_event(&event);
        assert!((app.declination - -1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_declination_ctrl_d() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::D, false, true));
        app.handle_event(&event);
        assert!((app.declination - 1.0).abs() < f64::EPSILON);
    }

    // ── Waypoint list navigation tests ──────────────────────────────

    #[test]
    fn test_waypoint_list_navigate_down() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.add_waypoint_at_current_position();
        app.view = View::Waypoints;
        app.selected_waypoint = Some(0);

        let event = Event::Key(make_key_event(Key::Down, false, false));
        app.handle_event(&event);
        assert_eq!(app.selected_waypoint, Some(1));
    }

    #[test]
    fn test_waypoint_list_navigate_up() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.add_waypoint_at_current_position();
        app.view = View::Waypoints;
        app.selected_waypoint = Some(1);

        let event = Event::Key(make_key_event(Key::Up, false, false));
        app.handle_event(&event);
        assert_eq!(app.selected_waypoint, Some(0));
    }

    #[test]
    fn test_waypoint_list_navigate_up_at_top() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.view = View::Waypoints;
        app.selected_waypoint = Some(0);

        let event = Event::Key(make_key_event(Key::Up, false, false));
        app.handle_event(&event);
        assert_eq!(app.selected_waypoint, Some(0));
    }

    #[test]
    fn test_waypoint_list_delete() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.view = View::Waypoints;
        app.selected_waypoint = Some(0);

        let event = Event::Key(make_key_event(Key::Delete, false, false));
        app.handle_event(&event);
        assert!(app.waypoints.is_empty());
    }

    #[test]
    fn test_waypoint_list_enter_goes_back() {
        let mut app = default_app();
        app.view = View::Waypoints;
        let event = Event::Key(make_key_event(Key::Enter, false, false));
        app.handle_event(&event);
        assert_eq!(app.view, View::Compass);
    }

    // ── Coordinate entry tests ──────────────────────────────────────

    #[test]
    fn test_coord_entry_tab_switches_field() {
        let mut app = default_app();
        app.view = View::CoordinateEntry;
        assert_eq!(app.active_coord_field, CoordField::Latitude);

        let event = Event::Key(make_key_event(Key::Tab, false, false));
        app.handle_event(&event);
        assert_eq!(app.active_coord_field, CoordField::Longitude);

        app.handle_event(&event);
        assert_eq!(app.active_coord_field, CoordField::Latitude);
    }

    #[test]
    fn test_coord_entry_backspace() {
        let mut app = default_app();
        app.view = View::CoordinateEntry;
        app.entry_lat_buf = String::from("12.3");
        let event = Event::Key(make_key_event(Key::Backspace, false, false));
        app.handle_event(&event);
        assert_eq!(app.entry_lat_buf, "12.");
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_returns_nonempty() {
        let app = default_app();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_starts_with_background() {
        let app = default_app();
        let cmds = app.render();
        match &cmds[0] {
            RenderCommand::FillRect { width, height, .. } => {
                assert!(*width > 0.0);
                assert!(*height > 0.0);
            }
            _ => panic!("first render command should be background FillRect"),
        }
    }

    #[test]
    fn test_render_compass_view_has_text() {
        let app = default_app();
        let cmds = app.render();
        let has_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. }));
        assert!(has_text);
    }

    #[test]
    fn test_render_compass_view_has_lines() {
        let app = default_app();
        let cmds = app.render();
        let has_lines = cmds.iter().any(|c| matches!(c, RenderCommand::Line { .. }));
        assert!(has_lines);
    }

    #[test]
    fn test_render_waypoint_view() {
        let mut app = default_app();
        app.view = View::Waypoints;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_waypoint_view_with_waypoints() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.view = View::Waypoints;
        let cmds = app.render();
        // Should have text for the waypoint
        let text_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Text { .. }))
            .count();
        assert!(text_count > 3);
    }

    #[test]
    fn test_render_coord_entry_view() {
        let mut app = default_app();
        app.view = View::CoordinateEntry;
        let cmds = app.render();
        assert!(!cmds.is_empty());
        // Should have input fields (FillRect)
        let rect_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
            .count();
        assert!(rect_count >= 3);
    }

    #[test]
    fn test_render_status_bar_present() {
        let app = default_app();
        let cmds = app.render();
        // Last few commands should include the status bar
        let has_status = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "Digital Compass"
            } else {
                false
            }
        });
        assert!(has_status);
    }

    // ── Trig helper tests ───────────────────────────────────────────

    #[test]
    fn test_sin_deg_zero() {
        assert!(sin_deg(0.0).abs() < 0.001);
    }

    #[test]
    fn test_sin_deg_90() {
        assert!((sin_deg(90.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cos_deg_zero() {
        assert!((cos_deg(0.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cos_deg_90() {
        assert!(cos_deg(90.0).abs() < 0.001);
    }

    // ── Move position tests ─────────────────────────────────────────

    #[test]
    fn test_move_position_clamps_lat() {
        let mut app = default_app();
        app.position = Coordinate::new(89.99, 0.0);
        app.move_position(1.0, 0.0);
        assert!((app.position.lat - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_move_position_clamps_lon() {
        let mut app = default_app();
        app.position = Coordinate::new(0.0, 179.99);
        app.move_position(0.0, 1.0);
        assert!((app.position.lon - 180.0).abs() < f64::EPSILON);
    }

    // ── key_to_char tests ───────────────────────────────────────────

    #[test]
    fn test_key_to_char_digits() {
        assert_eq!(key_to_char(Key::Num0, false), Some('0'));
        assert_eq!(key_to_char(Key::Num5, false), Some('5'));
        assert_eq!(key_to_char(Key::Num9, false), Some('9'));
    }

    #[test]
    fn test_key_to_char_dot() {
        // Key::A maps to '.' for decimal point entry
        assert_eq!(key_to_char(Key::A, false), Some('.'));
    }

    #[test]
    fn test_key_to_char_minus() {
        // Key::B maps to '-' for negative coordinates
        assert_eq!(key_to_char(Key::B, false), Some('-'));
    }

    #[test]
    fn test_key_to_char_unknown() {
        assert_eq!(key_to_char(Key::Escape, false), None);
    }

    // ── Mouse event tests ───────────────────────────────────────────

    #[test]
    fn test_mouse_click_waypoint_list() {
        let mut app = default_app();
        for _ in 0..3 {
            app.add_waypoint_at_current_position();
        }
        app.view = View::Waypoints;
        app.selected_waypoint = Some(0);

        // Click on the second row (y = 100 + 36 * 1 + 10 = 146)
        let event = Event::Mouse(MouseEvent {
            x: 100.0,
            y: 146.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        app.handle_event(&event);
        assert_eq!(app.selected_waypoint, Some(1));
    }

    // ── View state tests ────────────────────────────────────────────

    #[test]
    fn test_initial_view_is_compass() {
        let app = default_app();
        assert_eq!(app.view, View::Compass);
    }

    #[test]
    fn test_initial_unit_is_km() {
        let app = default_app();
        assert_eq!(app.distance_unit, DistanceUnit::Kilometers);
    }

    #[test]
    fn test_initial_position_is_nyc() {
        let app = default_app();
        assert!((app.position.lat - 40.7128).abs() < 0.001);
        assert!((app.position.lon - -74.0060).abs() < 0.001);
    }

    #[test]
    fn test_waypoint_select_by_number() {
        let mut app = default_app();
        for _ in 0..3 {
            app.add_waypoint_at_current_position();
        }
        app.selected_waypoint = None;

        let event = Event::Key(make_key_event(Key::Num2, false, false));
        app.handle_event(&event);
        assert_eq!(app.selected_waypoint, Some(1));
    }

    #[test]
    fn test_waypoint_select_by_number_out_of_range() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.selected_waypoint = None;

        // Trying to select waypoint 5 when only 1 exists
        let event = Event::Key(make_key_event(Key::Num6, false, false));
        app.handle_event(&event);
        assert!(app.selected_waypoint.is_none());
    }

    #[test]
    fn test_escape_clears_waypoint_selection() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.selected_waypoint = Some(0);

        let event = Event::Key(make_key_event(Key::Escape, false, false));
        app.handle_event(&event);
        assert!(app.selected_waypoint.is_none());
    }
}
