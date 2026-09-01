//! Slate OS Compass -- digital compass and navigation tool.
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
//!
//! # The window
//!
//! Everything above is drawn into a real window through [`oswindow`]. It was
//! not, until this was written: `main` built a `CompassApp` and dropped it,
//! without so much as calling `render`, so the rose, the three views and the
//! whole waypoint system had nothing to appear in and nothing to be typed at.
//!
//! The geometry follows from the window rather than preceding it. [`Layout`]
//! is solved from the width and height handed to [`CompassApp::frame`] on
//! every frame and never remembered, so the rose is as large as the room it
//! has and the waypoint rows are as many as fit. Every control the user can
//! press is a [`Target`] whose rectangle is recorded by the drawing pass as
//! it paints, which is what makes the thing seen and the thing clicked one
//! fact rather than two kept equal by hand.

use std::process::ExitCode;
use std::time::Duration;

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

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

// ── Constants ───────────────────────────────────────────────────────
const PI: f64 = core::f64::consts::PI;
const DEG_TO_RAD: f64 = PI / 180.0;
const RAD_TO_DEG: f64 = 180.0 / PI;
const EARTH_RADIUS_KM: f64 = 6371.0;
const KM_TO_MILES: f64 = 0.621_371;

const MAX_WAYPOINTS: usize = 10;

/// The size the window asks the compositor for. It is a *request*, not a
/// promise: nothing in the drawing pass may read it, because the size the
/// window ends up is whatever the user drags it to.
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 720.0;

/// The gap the rows of the waypoint list are drawn shorter than their pitch,
/// so they read as separate. The gap still belongs to the row above it for
/// hit-testing, so the pointer can never fall between two rows and select
/// neither.
const WP_ROW_GAP: f32 = 2.0;

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

/// Which coordinate-entry field is being edited.
///
/// `Name` is in this list because the coordinate-entry view has always drawn a
/// name field and, before it was added here, there was no way to put a
/// character in it: `Tab` cycled between latitude and longitude only, and
/// every typed character went to whichever of those two was active. The field
/// was painted, labelled, and unreachable, so every waypoint anyone could
/// actually make was called `WP1`, `WP2`, ... whatever they typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoordField {
    Latitude,
    Longitude,
    Name,
}

impl CoordField {
    /// The three fields in the order `Tab` walks them.
    fn all() -> &'static [CoordField] {
        &[Self::Latitude, Self::Longitude, Self::Name]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Latitude => "Latitude",
            Self::Longitude => "Longitude",
            Self::Name => "Name",
        }
    }

    /// The range the field is parsed against, for its caption.
    fn hint(self) -> &'static str {
        match self {
            Self::Latitude => "(-90 to 90)",
            Self::Longitude => "(-180 to 180)",
            Self::Name => "(optional)",
        }
    }

    /// The field after this one, wrapping.
    fn next(self) -> Self {
        match self {
            Self::Latitude => Self::Longitude,
            Self::Longitude => Self::Name,
            Self::Name => Self::Latitude,
        }
    }
}

/// Which way a stepper moves the thing beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nudge {
    Down,
    Up,
}

/// Everything in the window a click can land on.
///
/// A `Target`'s rectangle is recorded by the drawing pass at the moment it
/// paints the control, so a control that did not fit -- and was therefore not
/// drawn -- has no hit box either, and a click there reaches nothing instead
/// of reaching whatever used to be at those coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// One of the three view tabs across the top.
    Tab(View),
    /// The rose itself. A press sets the heading to the bearing of the point
    /// pressed, which is the only way to steer the compass with a pointer.
    Rose,
    /// The kilometres/miles toggle.
    Units,
    /// One end of the declination stepper.
    Declination(Nudge),
    /// "Mark here" -- a waypoint at the current position.
    MarkHere,
    /// A row of the waypoint list.
    Waypoint(usize),
    /// Delete the selected waypoint.
    DeleteWaypoint,
    /// A coordinate-entry field, which a press gives the keyboard to.
    Field(CoordField),
    /// The button that turns the entry fields into a waypoint.
    AddWaypoint,
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

/// `deg` brought into `0..360`, from either side.
///
/// Rust's `%` keeps the sign of the left operand, so `-10.0 % 360.0` is
/// `-10.0` and not `350.0`; the second `+ 360.0` is what makes this a compass
/// bearing rather than a signed offset.
fn wrap_360(deg: f64) -> f64 {
    if deg.is_finite() {
        ((deg % 360.0) + 360.0) % 360.0
    } else {
        0.0
    }
}

// ── Layout ─────────────────────────────────────────────────────────

/// Where everything goes, for one window size.
///
/// Solved fresh on every frame from the size the compositor hands us and
/// never stored, so there is no remembered size that can disagree with the
/// window the user is looking at. The order the parts are given up in is the
/// order of what they are worth: the side panel goes before the rose, and the
/// rose is never given up at all -- a compass with no rose is not a compass.
#[derive(Clone, Copy, Debug)]
struct Layout {
    window: Rect,
    /// The row of view tabs across the top. Empty in a window too short.
    header: Rect,
    /// Everything between the header and the status line.
    body: Rect,
    /// The one line of prose along the bottom.
    status: Rect,
    /// The readouts column down the right of the compass view. Empty in a
    /// window too narrow to afford it, in which case the rose takes the room.
    panel: Rect,
    /// The square the rose is inscribed in.
    rose: Rect,
    /// The rose's centre and radius, derived from `rose`.
    cx: f32,
    cy: f32,
    radius: f32,
    /// The height of one list row, one tab, one button.
    row: f32,
    pad: f32,
    heading: f32,
    font: f32,
    small: f32,
}

impl Layout {
    /// Solve the layout for a window of `w` x `h`.
    fn solve(w: f32, h: f32) -> Self {
        // A window size that is not a number is not a smaller window, it is a
        // question with no answer; treating it as zero gives every rectangle
        // below something finite to be clamped against rather than letting a
        // NaN propagate into every coordinate in the frame.
        let w = if w.is_finite() { w.max(0.0) } else { 0.0 };
        let h = if h.is_finite() { h.max(0.0) } else { 0.0 };
        let window = Rect::new(0.0, 0.0, w, h);

        // Type sizes track the window so a small window is legible rather than
        // being the same text in less room. The upper bound matters as much as
        // the lower one: without it a wall-sized window gets 90-point labels.
        let scale = (w / 900.0).min(h / 720.0).clamp(0.55, 1.6);
        let font = (15.0 * scale).clamp(9.0, 24.0);
        let small = (12.0 * scale).clamp(8.0, 19.0);
        let heading = (22.0 * scale).clamp(11.0, 34.0);
        let pad = (10.0 * scale).clamp(3.0, 18.0);

        // A row is what the window can afford, not what the font would like.
        // Sized from the font alone, a row in a short window is taller than
        // the strip it sits in and paints over what is below it.
        let wanted_row = (font * 2.0).max(14.0);
        let header_h = (wanted_row + pad).min(h);
        let row = (header_h - pad).clamp(0.0, wanted_row);
        let header = Rect::new(0.0, 0.0, w, header_h);

        // The status line is one line and always affordable, but "always"
        // still has to survive a window shorter than one line.
        let status_h = (small * 1.8 + pad * 0.6).min((h - header_h).max(0.0));
        let status = Rect::new(0.0, (h - status_h).max(header_h), w, status_h);

        let body_h = (h - header_h - status_h).max(0.0);
        let body = Rect::new(0.0, header_h, w, body_h);

        // The readouts column is worth having only if what it holds still fits
        // in it. A column too narrow for its own numbers is a column of
        // ellipses, and the rose is the better use of that width.
        //
        // The second half of this test used to be `w - wanted_panel >=
        // wanted_panel` -- "the panel must not take more than it leaves". The
        // mutation sweep could not make that clause matter, and it cannot: the
        // panel is 32% of the window, so what it leaves is 68%, and above the
        // 300-point clamp the window is already wider than 600. It was a
        // condition that had never once been false. Taking 32% is what keeps
        // the panel the smaller half, and that is what the test asserts now.
        let wanted_panel = (w * 0.32).clamp(0.0, 300.0);
        let panel_w = if wanted_panel >= small * 8.0 {
            wanted_panel
        } else {
            0.0
        };
        let panel = if panel_w > 0.0 && body_h > 0.0 {
            Rect::new(w - panel_w, body.y, panel_w, body_h)
        } else {
            Rect::EMPTY
        };

        let rose_w = (w - panel_w).max(0.0);
        let rose_area = Rect::new(0.0, body.y, rose_w, body_h);
        // Inscribed in the *smaller* of the two dimensions, so the rose is a
        // circle in a wide window and a circle in a tall one.
        let side = rose_w.min(body_h);
        let (cx, cy) = rose_area.centre();
        let radius = (side * 0.5 - pad).max(0.0);
        let rose = Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0);

        Self {
            window,
            header,
            body,
            status,
            panel,
            rose,
            cx,
            cy,
            radius,
            row,
            pad,
            heading,
            font,
            small,
        }
    }

    /// How many rows of pitch `pitch` fit in `height`.
    ///
    /// A row that half fits is not a row: it would be painted with its lower
    /// half outside the pane it belongs to. `floor`, never `ceil`, and never a
    /// bare `as usize` on a value that could be negative.
    fn rows_in(height: f32, pitch: f32) -> usize {
        if !(height.is_finite() && pitch.is_finite()) || pitch <= 0.0 || height <= 0.0 {
            return 0;
        }
        let n = (height / pitch).floor();
        if n <= 0.0 { 0 } else { n as usize }
    }
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
    /// The size the last frame was drawn at.
    ///
    /// A press arrives with no size attached and has to be answered against
    /// the picture the user was actually looking at, which is the one `render`
    /// last produced. Answering it against anything else -- a constant, most
    /// of all -- answers a click against a window that is not on the screen.
    width: f32,
    height: f32,
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
            // The size we will *ask* for. It is replaced by the real one on
            // the first `render`, which happens before any event can arrive.
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// True heading = magnetic heading + declination.
    fn true_heading(&self) -> f64 {
        wrap_360(self.heading + self.declination)
    }

    /// Rotate the heading by `delta` degrees, wrapping to 0-360.
    fn rotate(&mut self, delta: f64) {
        self.heading = wrap_360(self.heading + delta);
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
            format!("WP{}", self.waypoints.len().saturating_add(1))
        } else {
            self.entry_name_buf.trim().to_string()
        };
        self.waypoints.push(Waypoint {
            name,
            coord: Coordinate::new(lat, lon),
        });
        self.selected_waypoint = Some(self.waypoints.len().saturating_sub(1));
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
        let name = format!("WP{}", self.waypoints.len().saturating_add(1));
        self.waypoints.push(Waypoint {
            name,
            coord: Coordinate::new(self.position.lat, self.position.lon),
        });
        self.selected_waypoint = Some(self.waypoints.len().saturating_sub(1));
        self.status = String::from("Waypoint added at current position");
        true
    }

    /// Remove the selected waypoint.
    fn remove_selected_waypoint(&mut self) {
        if let Some(idx) = self.selected_waypoint
            && idx < self.waypoints.len()
        {
            self.waypoints.remove(idx);
            if self.waypoints.is_empty() {
                self.selected_waypoint = None;
            } else if idx >= self.waypoints.len() {
                self.selected_waypoint = Some(self.waypoints.len().saturating_sub(1));
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

    fn handle_event(&mut self, event: &Event, size: (f32, f32)) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me, size),
            _ => {}
        }
    }

    /// Route a press to whatever the drawing pass recorded under it.
    ///
    /// The hit boxes come from the frame the same size produced, so a control
    /// the window was too small to draw is a control that cannot be pressed --
    /// which used to be the other way round: the waypoint list re-derived its
    /// rows arithmetically from constants, so a row that had scrolled or been
    /// clipped away was still clickable at the coordinates it no longer
    /// occupied.
    fn handle_mouse(&mut self, event: &MouseEvent, size: (f32, f32)) {
        let MouseEventKind::Press(MouseButton::Left) = event.kind else {
            return;
        };
        let (w, h) = size;
        let frame = self.frame(w, h);
        let Some(target) = frame.hit_test(event.x, event.y) else {
            return;
        };
        match target {
            Target::Tab(view) => self.set_view(view),
            Target::Rose => {
                // The bearing of the pressed point from the middle of the
                // rose, which is where the user is pointing the compass.
                let l = Layout::solve(w, h);
                let (dx, dy) = (event.x - l.cx, l.cy - event.y);
                if dx.abs() > f32::EPSILON || dy.abs() > f32::EPSILON {
                    let deg = f64::from(dx.atan2(dy)) * RAD_TO_DEG;
                    self.heading = wrap_360(deg - self.declination);
                    self.status = format!("Heading: {:.0}", self.true_heading());
                }
            }
            Target::Units => self.toggle_units(),
            Target::Declination(n) => {
                self.adjust_declination(if n == Nudge::Up { 1.0 } else { -1.0 });
                self.status = format!("Declination: {:+.0}", self.declination);
            }
            Target::MarkHere => {
                self.add_waypoint_at_current_position();
            }
            Target::Waypoint(row) => {
                if row < self.waypoints.len() {
                    self.selected_waypoint = Some(row);
                    self.status = format!(
                        "Selected: {}",
                        self.waypoints.get(row).map_or("?", |w| w.name.as_str())
                    );
                }
            }
            Target::DeleteWaypoint => self.remove_selected_waypoint(),
            Target::Field(f) => self.active_coord_field = f,
            Target::AddWaypoint => {
                self.add_waypoint_from_entry();
            }
        }
    }

    /// Switch to `view`, resetting whatever that view starts from.
    fn set_view(&mut self, view: View) {
        self.view = view;
        match view {
            View::Compass => self.status = String::from("Digital Compass"),
            View::Waypoints => self.status = String::from("Waypoint List"),
            View::CoordinateEntry => {
                self.active_coord_field = CoordField::Latitude;
                self.status = String::from("Coordinate Entry");
            }
        }
    }

    fn toggle_units(&mut self) {
        self.distance_unit = match self.distance_unit {
            DistanceUnit::Kilometers => DistanceUnit::Miles,
            DistanceUnit::Miles => DistanceUnit::Kilometers,
        };
        self.status = format!("Units: {}", unit_label(self.distance_unit));
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
            Key::U => self.toggle_units(),
            Key::W => self.set_view(View::Waypoints),
            Key::C => self.set_view(View::CoordinateEntry),
            Key::M => {
                self.add_waypoint_at_current_position();
            }
            Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9
            | Key::Num0 => {
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
            Key::Escape => self.set_view(View::Compass),
            Key::Up => {
                if let Some(idx) = self.selected_waypoint {
                    if idx > 0 {
                        self.selected_waypoint = Some(idx.saturating_sub(1));
                    }
                } else if !self.waypoints.is_empty() {
                    self.selected_waypoint = Some(0);
                }
            }
            Key::Down => {
                if let Some(idx) = self.selected_waypoint {
                    if idx.saturating_add(1) < self.waypoints.len() {
                        self.selected_waypoint = Some(idx.saturating_add(1));
                    }
                } else if !self.waypoints.is_empty() {
                    self.selected_waypoint = Some(0);
                }
            }
            Key::Delete | Key::Backspace => {
                self.remove_selected_waypoint();
            }
            Key::Enter => self.set_view(View::Compass),
            Key::C => self.set_view(View::CoordinateEntry),
            _ => {}
        }
    }

    fn handle_key_coord_entry(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Escape => self.set_view(View::Compass),
            Key::Tab => self.active_coord_field = self.active_coord_field.next(),
            Key::Enter => {
                self.add_waypoint_from_entry();
            }
            Key::Backspace => {
                self.active_buffer().pop();
            }
            // Anything else that produced a character goes into the field.
            _ => {
                let field = self.active_coord_field;
                // What the keyboard *produced*, not where the key sits. The
                // old route was a `key_to_char` table mapping `Key::Num1` to
                // `'1'`, which is a claim about a US layout: on any other one
                // the digits and the minus sign are elsewhere, and a
                // coordinate could not be typed at all. It also ignored shift,
                // so its own table could not have produced a `+`.
                for c in event.text.chars() {
                    if !accepts_char(field, c) {
                        continue;
                    }
                    let buf = self.active_buffer();
                    if buf.chars().count() < 16 {
                        buf.push(c);
                    }
                }
            }
        }
    }

    /// The buffer the keyboard is currently pointed at.
    fn active_buffer(&mut self) -> &mut String {
        match self.active_coord_field {
            CoordField::Latitude => &mut self.entry_lat_buf,
            CoordField::Longitude => &mut self.entry_lon_buf,
            CoordField::Name => &mut self.entry_name_buf,
        }
    }

    /// The text in a field, for drawing it.
    fn buffer_of(&self, field: CoordField) -> &str {
        match field {
            CoordField::Latitude => &self.entry_lat_buf,
            CoordField::Longitude => &self.entry_lon_buf,
            CoordField::Name => &self.entry_name_buf,
        }
    }

    // ── Drawing ─────────────────────────────────────────────────────

    /// The whole window -- every command and every hit box -- at `w` x `h`.
    ///
    /// The hit boxes are recorded by the same statements that paint the
    /// controls, so where a thing is drawn and where it answers a press are
    /// one fact rather than two kept equal by hand. Nothing below reads
    /// [`WINDOW_WIDTH`] or [`WINDOW_HEIGHT`]: the geometry comes from the
    /// arguments, which is what makes the window resizable.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let mut f = Frame::new(l.window.w, l.window.h);
        f.push(fill(l.window, BASE, 0.0));

        self.draw_header(&mut f, &l);

        // The body clip is what makes a pane too big for its room disappear
        // at the edge instead of painting over the status line -- and,
        // because `Frame::hit` trims to the clip in force, what makes
        // anything outside it unclickable as well.
        f.clip(l.body);
        match self.view {
            View::Compass => self.draw_compass_view(&mut f, &l),
            View::Waypoints => self.draw_waypoint_view(&mut f, &l),
            View::CoordinateEntry => self.draw_coord_entry_view(&mut f, &l),
        }
        f.unclip();

        self.draw_status(&mut f, &l);
        f
    }

    /// The three view tabs and the unit toggle, across the top.
    ///
    /// The tabs are the only pointer route between the views -- before this
    /// the views could be reached with `W`, `C` and `Esc` and by nothing
    /// else, which is a keyboard-only program wearing a window.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        f.push(fill(l.header, MANTLE, 0.0));
        f.clip(l.header);

        let units_w = (l.small * 4.0).min(l.header.w * 0.22).max(0.0);
        let tabs_w = (l.header.w - l.pad * 3.0 - units_w).max(0.0);
        let tab_w = tabs_w / 3.0;
        let y = l.header.y + (l.header.h - l.row) * 0.5;

        for (i, (view, name)) in TABS.iter().enumerate() {
            let r = Rect::new(
                l.pad + i as f32 * tab_w,
                y,
                (tab_w - l.pad * 0.4).max(0.0),
                l.row,
            );
            let active = self.view == *view;
            f.push(fill(r, if active { SURFACE1 } else { SURFACE0 }, 4.0));
            centred(
                f,
                r,
                name,
                if active { LAVENDER } else { SUBTEXT0 },
                l.small,
                if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
            f.hit(Target::Tab(*view), r);
        }

        let units = Rect::new(l.header.right() - l.pad - units_w, y, units_w, l.row);
        f.push(fill(units, SURFACE0, 4.0));
        centred(
            f,
            units,
            unit_label(self.distance_unit),
            TEAL,
            l.small,
            FontWeightHint::Bold,
        );
        f.hit(Target::Units, units);

        f.unclip();
    }

    /// The one line of prose along the bottom.
    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.status.is_empty() {
            return;
        }
        f.push(fill(l.status, CRUST, 0.0));
        f.clip(l.status);
        bounded(
            f,
            inset(l.status, l.pad * 0.5),
            self.status.clone(),
            SUBTEXT0,
            l.small,
            FontWeightHint::Regular,
        );
        f.unclip();
    }

    // ── Compass view ────────────────────────────────────────────────

    fn draw_compass_view(&self, f: &mut Frame<Target>, l: &Layout) {
        self.draw_rose(f, l);
        if l.panel.is_empty() {
            // Too narrow for the readouts column. The heading is the one
            // number a compass exists to show, so it is drawn over the rose
            // rather than being given up with the panel around it.
            self.draw_heading_strip(f, l);
        } else {
            self.draw_panel(f, l);
        }
    }

    /// The rose: ring, ticks, labels and needle, all sized from the radius
    /// the layout worked out rather than from a constant.
    fn draw_rose(&self, f: &mut Frame<Target>, l: &Layout) {
        let r = l.radius;
        if r <= 0.0 {
            return;
        }
        f.push(fill(l.rose, MANTLE, r));
        f.push(stroke(l.rose, SURFACE1, r, 2.0));
        let inner = r * 0.88;
        f.push(stroke(
            Rect::new(l.cx - inner, l.cy - inner, inner * 2.0, inner * 2.0),
            SURFACE0,
            inner,
            1.0,
        ));

        let heading = self.true_heading() as f32;
        for step in 0..36 {
            let angle = step as f32 * 10.0 - heading;
            let major = step % 3 == 0;
            let from = if major { r * 0.86 } else { r * 0.93 };
            let to = r * 0.98;
            f.push(RenderCommand::Line {
                x1: l.cx + sin_deg(angle) * from,
                y1: l.cy - cos_deg(angle) * from,
                x2: l.cx + sin_deg(angle) * to,
                y2: l.cy - cos_deg(angle) * to,
                color: if major { TEXT_COLOR } else { OVERLAY0 },
                width: if major { 2.0 } else { 1.0 },
            });
        }

        self.draw_rose_labels(f, l);
        self.draw_needle(f, l);

        // The rose answers a press over its whole square. It is the only
        // pointer route to a heading there is: without it the compass can be
        // turned with the arrow keys and in no other way.
        f.hit(Target::Rose, l.rose);
    }

    /// Degree numbers every 30 degrees and the eight cardinal names.
    fn draw_rose_labels(&self, f: &mut Frame<Target>, l: &Layout) {
        let heading = self.true_heading() as f32;
        let r = l.radius;

        // Degree numbers are the first thing given up in a small rose. Drawn
        // anyway they would be twelve overlapping runs across the middle of
        // the face, which is worse than not drawing them.
        let deg_size = (l.small * 0.85).max(8.0);
        if r >= deg_size * 9.0 {
            for step in 0..12 {
                let deg = step as f32 * 30.0;
                let angle = deg - heading;
                let ring = r * 0.76;
                let label = format!("{deg:.0}");
                let (cx, cy) = (l.cx + sin_deg(angle) * ring, l.cy - cos_deg(angle) * ring);
                // The box is the measured width of the text and no wider.
                // A generous box would be a `max_width` the run cannot fill,
                // which is a promise to the compositor that the label ends
                // somewhere it does not.
                let wide = text::measure(&label, deg_size, FontWeightHint::Regular);
                bounded(
                    f,
                    Rect::new(cx - wide * 0.5, cy - deg_size, wide, deg_size * 2.0),
                    label,
                    SUBTEXT0,
                    deg_size,
                    FontWeightHint::Regular,
                );
            }
        }

        for step in 0..8_u32 {
            let deg = step.saturating_mul(45);
            let Some(label) = cardinal_label_for_angle(deg) else {
                continue;
            };
            let principal = step % 2 == 0;
            let size = if principal {
                l.font * 1.15
            } else {
                l.small * 0.9
            };
            let weight = if principal {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            let color = if deg == 0 {
                RED
            } else if principal {
                TEXT_COLOR
            } else {
                SUBTEXT0
            };
            let angle = deg as f32 - heading;
            let ring = r * 0.64;
            let (cx, cy) = (l.cx + sin_deg(angle) * ring, l.cy - cos_deg(angle) * ring);
            let wide = text::measure(label, size, weight);
            bounded(
                f,
                Rect::new(cx - wide * 0.5, cy - size, wide, size * 2.0),
                label,
                color,
                size,
                weight,
            );
        }
    }

    /// The needle, the hub and the fixed pointer the heading is read against.
    fn draw_needle(&self, f: &mut Frame<Target>, l: &Layout) {
        let angle = -(self.true_heading() as f32);
        let len = l.radius * 0.55;
        f.push(RenderCommand::Line {
            x1: l.cx,
            y1: l.cy,
            x2: l.cx + sin_deg(angle) * len,
            y2: l.cy - cos_deg(angle) * len,
            color: RED,
            width: 3.0,
        });
        f.push(RenderCommand::Line {
            x1: l.cx,
            y1: l.cy,
            x2: l.cx - sin_deg(angle) * len * 0.6,
            y2: l.cy + cos_deg(angle) * len * 0.6,
            color: SURFACE2,
            width: 2.0,
        });
        let dot = (l.radius * 0.03).max(2.0);
        f.push(fill(
            Rect::new(l.cx - dot, l.cy - dot, dot * 2.0, dot * 2.0),
            RED,
            dot,
        ));

        let ty = l.cy - l.radius;
        let s = (l.radius * 0.06).max(3.0);
        for (x1, y1, x2, y2) in [
            (l.cx, ty, l.cx - s, ty - s * 1.6),
            (l.cx, ty, l.cx + s, ty - s * 1.6),
            (l.cx - s, ty - s * 1.6, l.cx + s, ty - s * 1.6),
        ] {
            f.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: PEACH,
                width: 2.0,
            });
        }
    }

    /// The heading, across the top of the body, when there is no panel.
    fn draw_heading_strip(&self, f: &mut Frame<Target>, l: &Layout) {
        let h = self.true_heading();
        let strip = Rect::new(
            l.body.x,
            l.body.y,
            l.body.w,
            (l.heading * 1.6).min(l.body.h),
        );
        f.push(fill(inset(strip, l.pad * 0.4), MANTLE, 4.0));
        centred(
            f,
            strip,
            &format!("{h:.0}  {}", cardinal_direction(h)),
            BLUE,
            l.heading,
            FontWeightHint::Bold,
        );
    }

    /// The readouts column down the right of the compass view.
    ///
    /// Cards are laid top to bottom against a cursor and each one is skipped
    /// entirely once the cursor has passed the bottom of the panel, so a
    /// short window loses the least important readout rather than painting
    /// all of them over one another.
    fn draw_panel(&self, f: &mut Frame<Target>, l: &Layout) {
        f.clip(l.panel);
        let area = inset(l.panel, l.pad);
        let mut y = area.y;

        if let Some(b) = card(f, l, area, &mut y, "HEADING", l.heading * 1.3) {
            let h = self.true_heading();
            let split = b.w * 0.52;
            bounded(
                f,
                Rect::new(b.x, b.y, split, b.h),
                format!("{h:.0}"),
                BLUE,
                l.heading,
                FontWeightHint::Bold,
            );
            bounded(
                f,
                Rect::new(b.x + split, b.y, (b.w - split).max(0.0), b.h),
                cardinal_direction(h),
                GREEN,
                l.heading * 0.8,
                FontWeightHint::Bold,
            );
        }

        if let Some(b) = card(f, l, area, &mut y, "POSITION", l.font * 2.8) {
            let line = b.h * 0.5;
            bounded(
                f,
                Rect::new(b.x, b.y, b.w, line),
                self.position.format_lat(),
                TEXT_COLOR,
                l.font,
                FontWeightHint::Regular,
            );
            bounded(
                f,
                Rect::new(b.x, b.y + line, b.w, line),
                self.position.format_lon(),
                TEXT_COLOR,
                l.font,
                FontWeightHint::Regular,
            );
        }

        self.draw_declination_card(f, l, area, &mut y);
        self.draw_waypoint_card(f, l, area, &mut y);
        self.draw_mark_button(f, l, area, &mut y);
        draw_help(f, l, area, y);

        f.unclip();
    }

    /// The declination readout and the two steppers that change it.
    ///
    /// The steppers are new. Declination was adjustable with `D` and `Ctrl+D`
    /// and by no other means, and `D` alone *decreased* it -- a control whose
    /// only mention on screen was a help line reading "D: Declination -1".
    fn draw_declination_card(&self, f: &mut Frame<Target>, l: &Layout, area: Rect, y: &mut f32) {
        let Some(b) = card(f, l, area, y, "DECLINATION", l.row) else {
            return;
        };
        let step = l.row.min(b.w * 0.28);
        let minus = Rect::new(b.right() - step * 2.0 - l.pad * 0.4, b.y, step, b.h);
        let plus = Rect::new(b.right() - step, b.y, step, b.h);
        bounded(
            f,
            Rect::new(b.x, b.y, (minus.x - b.x).max(0.0), b.h),
            format!("{:+.0}", self.declination),
            YELLOW,
            l.font,
            FontWeightHint::Bold,
        );
        for (r, sign, nudge) in [(minus, "-", Nudge::Down), (plus, "+", Nudge::Up)] {
            f.push(fill(r, SURFACE2, 4.0));
            centred(f, r, sign, TEXT_COLOR, l.font, FontWeightHint::Bold);
            f.hit(Target::Declination(nudge), r);
        }
    }

    /// Name, position, bearing and distance for the selected waypoint.
    fn draw_waypoint_card(&self, f: &mut Frame<Target>, l: &Layout, area: Rect, y: &mut f32) {
        let Some(b) = card(f, l, area, y, "WAYPOINT", l.font * 4.4) else {
            return;
        };
        let line = b.h * 0.25;
        let Some(wp) = self.selected_waypoint.and_then(|i| self.waypoints.get(i)) else {
            bounded(
                f,
                Rect::new(b.x, b.y, b.w, line),
                "No waypoint selected",
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
            return;
        };
        bounded(
            f,
            Rect::new(b.x, b.y, b.w, line),
            wp.name.clone(),
            TEAL,
            l.font,
            FontWeightHint::Bold,
        );
        bounded(
            f,
            Rect::new(b.x, b.y + line, b.w, line),
            format!("{} {}", wp.coord.format_lat(), wp.coord.format_lon()),
            TEXT_COLOR,
            l.small,
            FontWeightHint::Regular,
        );
        if let Some((brg, dist_km)) = self.waypoint_bearing_distance() {
            let dist = convert_distance(dist_km, self.distance_unit);
            bounded(
                f,
                Rect::new(b.x, b.y + line * 2.0, b.w, line),
                format!("BRG {brg:.0}"),
                PEACH,
                l.small,
                FontWeightHint::Regular,
            );
            bounded(
                f,
                Rect::new(b.x, b.y + line * 3.0, b.w, line),
                format!("DST {dist:.1} {}", unit_label(self.distance_unit)),
                PEACH,
                l.small,
                FontWeightHint::Regular,
            );
        }
    }

    /// The button that saves the current position as a waypoint.
    ///
    /// It keeps its hit box when the list is full: pressing it then produces
    /// the status line saying so, which is more use than a press that lands
    /// on nothing and leaves the user guessing why.
    fn draw_mark_button(&self, f: &mut Frame<Target>, l: &Layout, area: Rect, y: &mut f32) {
        if *y + l.row > area.bottom() {
            return;
        }
        let r = Rect::new(area.x, *y, area.w, l.row);
        let full = self.waypoints.len() >= MAX_WAYPOINTS;
        f.push(fill(r, if full { SURFACE0 } else { SURFACE2 }, 6.0));
        centred(
            f,
            r,
            if full { "List full" } else { "Mark here (M)" },
            if full { OVERLAY0 } else { TEXT_COLOR },
            l.small,
            FontWeightHint::Bold,
        );
        f.hit(Target::MarkHere, r);
        *y = r.bottom() + l.pad * 0.6;
    }

    // ── Waypoint list view ──────────────────────────────────────────

    fn draw_waypoint_view(&self, f: &mut Frame<Target>, l: &Layout) {
        let area = inset(l.body, l.pad);
        if area.is_empty() {
            return;
        }

        let head = Rect::new(area.x, area.y, area.w, l.row);
        for (r, (name, _)) in wp_columns(head, l.pad * 0.4).iter().zip(WP_COLUMNS) {
            bounded(f, *r, name, SUBTEXT0, l.small, FontWeightHint::Bold);
        }
        f.push(RenderCommand::Line {
            x1: area.x,
            y1: head.bottom(),
            x2: area.right(),
            y2: head.bottom(),
            color: SURFACE1,
            width: 1.0,
        });

        let bar_h = (l.row + l.pad * 0.5).min(area.h);
        let top = head.bottom() + l.pad * 0.4;
        let list = Rect::new(area.x, top, area.w, (area.bottom() - bar_h - top).max(0.0));

        if self.waypoints.is_empty() {
            bounded(
                f,
                Rect::new(list.x, list.y, list.w, l.row.min(list.h)),
                "No waypoints. Press C to add one, or Esc to go back.",
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
        } else {
            self.draw_waypoint_rows(f, l, list);
        }

        self.draw_waypoint_bar(
            f,
            l,
            Rect::new(area.x, area.bottom() - bar_h, area.w, bar_h),
        );
    }

    /// The rows of the waypoint list, in real columns.
    fn draw_waypoint_rows(&self, f: &mut Frame<Target>, l: &Layout, list: Rect) {
        let pitch = l.row;
        let visible = Layout::rows_in(list.h, pitch).min(self.waypoints.len());
        let first = self.first_visible_waypoint(visible);
        f.clip(list);
        for i in first..first.saturating_add(visible) {
            let Some(wp) = self.waypoints.get(i) else {
                break;
            };
            let row = Rect::new(
                list.x,
                list.y + i.saturating_sub(first) as f32 * pitch,
                list.w,
                pitch,
            );
            let selected = self.selected_waypoint == Some(i);
            if selected {
                f.push(fill(
                    Rect::new(row.x, row.y, row.w, (row.h - WP_ROW_GAP).max(0.0)),
                    SURFACE0,
                    4.0,
                ));
            }
            let color = if selected { BLUE } else { TEXT_COLOR };
            let weight = if selected {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            let dist = convert_distance(
                haversine_distance(&self.position, &wp.coord),
                self.distance_unit,
            );
            let brg = bearing_to(&self.position, &wp.coord);
            let cells = [
                format!("{}", i.saturating_add(1)),
                wp.name.clone(),
                wp.coord.format_lat(),
                wp.coord.format_lon(),
                format!("{brg:.0}"),
                format!("{dist:.1} {}", unit_label(self.distance_unit)),
            ];
            let text_row = Rect::new(row.x, row.y, row.w, (row.h - WP_ROW_GAP).max(0.0));
            for (c, cell) in wp_columns(text_row, l.pad * 0.4).iter().zip(cells) {
                bounded(f, *c, cell, color, l.small, weight);
            }
            // The gap below a row belongs to the row above it, so a press can
            // never land between two rows and select neither.
            f.hit(Target::Waypoint(i), row);
        }
        f.unclip();
    }

    /// The strip under the list: the key help, and the delete button.
    fn draw_waypoint_bar(&self, f: &mut Frame<Target>, l: &Layout, bar: Rect) {
        if bar.is_empty() {
            return;
        }
        let btn_w = (l.small * 5.0).min(bar.w * 0.4);
        let btn = Rect::new(bar.right() - btn_w, bar.y, btn_w, l.row.min(bar.h));
        let armed = self.selected_waypoint.is_some();
        f.push(fill(btn, if armed { RED } else { SURFACE0 }, 6.0));
        centred(
            f,
            btn,
            "Delete",
            if armed { CRUST } else { OVERLAY0 },
            l.small,
            FontWeightHint::Bold,
        );
        // With nothing selected there is nothing to delete, and a button that
        // accepts the press and does nothing is indistinguishable from one
        // that is broken.
        if armed {
            f.hit(Target::DeleteWaypoint, btn);
        }
        bounded(
            f,
            Rect::new(
                bar.x,
                bar.y,
                (btn.x - bar.x - l.pad).max(0.0),
                l.row.min(bar.h),
            ),
            "Up/Down: select  |  Del: remove  |  C: add new  |  Enter/Esc: back",
            OVERLAY0,
            l.small * 0.9,
            FontWeightHint::Regular,
        );
    }

    /// The first row drawn, chosen so the selection is always on screen.
    ///
    /// A selection scrolled out of the pane is one the user can neither see,
    /// act on, nor discover they still have -- and the delete key would then
    /// remove a waypoint that was nowhere in the window.
    fn first_visible_waypoint(&self, visible: usize) -> usize {
        if visible == 0 {
            return 0;
        }
        let sel = self.selected_waypoint.unwrap_or(0);
        if sel < visible {
            0
        } else {
            sel.saturating_add(1).saturating_sub(visible)
        }
    }

    // ── Coordinate entry view ───────────────────────────────────────

    fn draw_coord_entry_view(&self, f: &mut Frame<Target>, l: &Layout) {
        let area = inset(l.body, l.pad);
        if area.is_empty() {
            return;
        }
        let mut y = area.y;
        let caption_h = l.small * 1.5;

        for field in CoordField::all().iter().copied() {
            if y + caption_h + l.row > area.bottom() {
                return;
            }
            bounded(
                f,
                Rect::new(area.x, y, area.w, caption_h),
                format!("{} {}", field.label(), field.hint()),
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
            let entry = Rect::new(area.x, y + caption_h, area.w.min(l.font * 22.0), l.row);
            f.push(fill(entry, SURFACE1, 6.0));
            if self.active_coord_field == field {
                f.push(stroke(entry, BLUE, 6.0, 2.0));
            }
            let typed = self.buffer_of(field);
            let (shown, color) = if typed.is_empty() {
                (placeholder(field), OVERLAY0)
            } else {
                (typed, TEXT_COLOR)
            };
            bounded(
                f,
                inset(entry, l.pad * 0.6),
                shown,
                color,
                l.font,
                FontWeightHint::Regular,
            );
            // Clicking a field is what gives it the keyboard. `Tab` was the
            // only way to move between them, which is not a thing a pointer
            // user would think to try.
            f.hit(Target::Field(field), entry);
            y = entry.bottom() + l.pad * 0.6;
        }

        if y + l.row <= area.bottom() {
            let btn = Rect::new(area.x, y, (l.font * 8.0).min(area.w), l.row);
            f.push(fill(btn, GREEN, 6.0));
            centred(f, btn, "Add (Enter)", CRUST, l.small, FontWeightHint::Bold);
            f.hit(Target::AddWaypoint, btn);
            y = btn.bottom() + l.pad * 0.6;
        }

        if y + caption_h <= area.bottom() {
            bounded(
                f,
                Rect::new(area.x, y, area.w, caption_h),
                "Tab: switch field  |  Enter: add waypoint  |  Esc: cancel",
                OVERLAY0,
                l.small * 0.9,
                FontWeightHint::Regular,
            );
        }
    }
}

// ── Drawing helpers ────────────────────────────────────────────────

/// The three view tabs, in the order they are drawn.
const TABS: [(View, &str); 3] = [
    (View::Compass, "Compass"),
    (View::Waypoints, "Waypoints"),
    (View::CoordinateEntry, "Add waypoint"),
];

/// The waypoint list's columns and their share of the list's width.
///
/// The list used to be one padded `format!` per row -- `{:<14}`, `{:<12}` and
/// friends -- which lines up only in a monospace font. The UI font is not
/// monospaced, so the columns were ragged, and a name a few characters longer
/// than another pushed every column after it sideways.
const WP_COLUMNS: [(&str, f32); 6] = [
    ("#", 0.06),
    ("Name", 0.30),
    ("Latitude", 0.17),
    ("Longitude", 0.17),
    ("Bearing", 0.14),
    ("Distance", 0.16),
];

/// The six column rectangles of one list row.
fn wp_columns(row: Rect, gap: f32) -> [Rect; 6] {
    let mut out = [Rect::EMPTY; 6];
    let mut x = row.x;
    for (i, (_, share)) in WP_COLUMNS.iter().enumerate() {
        let span = row.w * share;
        if let Some(slot) = out.get_mut(i) {
            *slot = Rect::new(x, row.y, (span - gap).max(0.0), row.h);
        }
        x += span;
    }
    out
}

/// `r` shrunk by `d` on every side, never past nothing.
fn inset(r: Rect, d: f32) -> Rect {
    Rect::new(
        r.x + d,
        r.y + d,
        (r.w - d * 2.0).max(0.0),
        (r.h - d * 2.0).max(0.0),
    )
}

/// A filled rectangle with `radius`-rounded corners.
fn fill(r: Rect, color: Color, radius: f32) -> RenderCommand {
    RenderCommand::FillRect {
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
    }
}

/// An outlined rectangle with `radius`-rounded corners.
fn stroke(r: Rect, color: Color, radius: f32, line_width: f32) -> RenderCommand {
    RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    }
}

/// Draw `text` in `r`: left-aligned, vertically centred, and bounded to `r`'s
/// width.
///
/// Every run in this window goes through here or through [`centred`]. Text
/// drawn with `max_width: None` -- which is how all but three runs of this
/// app used to be drawn -- runs out of its box and over its neighbour the
/// moment the window is narrower than the string, and no amount of layout can
/// stop it.
fn bounded(
    f: &mut Frame<Target>,
    r: Rect,
    text: impl Into<String>,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) {
    if !(r.w.is_finite() && r.h.is_finite()) || r.w <= 0.0 || r.h <= 0.0 || size <= 0.0 {
        return;
    }
    // A run taller than its box, centred in it, sticks out of both ends --
    // which is how the status line came to be drawn below the bottom of a
    // 30-pixel window. Shrinking to the box is what a strip too short for its
    // type should do; the alternative is text outside the box it belongs to,
    // and no caller can prevent that from where it stands.
    let size = size.min(r.h);
    f.push(RenderCommand::Text {
        x: r.x,
        y: r.y + (r.h - size) * 0.5,
        text: text.into(),
        color,
        font_size: size,
        font_weight: weight,
        max_width: Some(r.w),
        overflow: TextOverflow::Ellipsis,
    });
}

/// [`bounded`], centred horizontally in `r`.
fn centred(
    f: &mut Frame<Target>,
    r: Rect,
    text: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) {
    // Measured at the size it will actually be drawn at, which is the size
    // that fits the box: centring a 15-point string in a 4-point-tall strip
    // against a 15-point measurement puts it in the wrong place.
    let size = size.min(r.h);
    let x = text::center_x(text, r.x + r.w * 0.5, size, weight).max(r.x);
    bounded(
        f,
        Rect::new(x, r.y, (r.right() - x).max(0.0), r.h),
        text,
        color,
        size,
        weight,
    );
}

/// A captioned card in the readouts panel, or `None` once the panel is full.
///
/// Returning `None` rather than drawing off the bottom is what makes the
/// panel degrade instead of overlapping: the caller skips the whole card,
/// contents and hit boxes together.
fn card(
    f: &mut Frame<Target>,
    l: &Layout,
    area: Rect,
    y: &mut f32,
    caption: &str,
    body_h: f32,
) -> Option<Rect> {
    let caption_h = l.small * 1.4;
    let h = caption_h + body_h + l.pad * 0.5;
    if *y + h > area.bottom() {
        return None;
    }
    let r = Rect::new(area.x, *y, area.w, h);
    f.push(fill(r, SURFACE0, 8.0));
    let inner_x = r.x + l.pad * 0.6;
    let inner_w = (r.w - l.pad * 1.2).max(0.0);
    bounded(
        f,
        Rect::new(inner_x, r.y, inner_w, caption_h),
        caption,
        SUBTEXT0,
        l.small * 0.85,
        FontWeightHint::Regular,
    );
    *y = r.bottom() + l.pad * 0.6;
    Some(Rect::new(inner_x, r.y + caption_h, inner_w, body_h))
}

/// The key help at the foot of the readouts panel, as many lines as fit.
fn draw_help(f: &mut Frame<Target>, l: &Layout, area: Rect, y: f32) {
    let lines = [
        "Left/Right: rotate",
        "Shift+arrows: rotate 10",
        "Up/Down: move position",
        "D / Ctrl+D: declination",
        "U: toggle km/mi",
        "M: mark waypoint",
        "1-0: select waypoint",
        "W: waypoint list",
        "C: coordinate entry",
    ];
    let pitch = l.small * 1.5;
    let n = Layout::rows_in((area.bottom() - y).max(0.0), pitch).min(lines.len());
    for (i, line) in lines.iter().take(n).enumerate() {
        bounded(
            f,
            Rect::new(area.x, y + i as f32 * pitch, area.w, pitch),
            *line,
            OVERLAY0,
            l.small * 0.9,
            FontWeightHint::Regular,
        );
    }
}

/// What an entry field shows before anything is typed into it.
fn placeholder(field: CoordField) -> &'static str {
    match field {
        CoordField::Latitude => "e.g. 48.8566",
        CoordField::Longitude => "e.g. 2.3522",
        CoordField::Name => "WP auto-name",
    }
}

/// Whether `c` belongs in `field`.
///
/// The coordinate fields are parsed as `f64`, so they take what a number is
/// written with and nothing else -- a stray letter in a latitude is not a
/// typo the parser can recover from, it is a waypoint that silently fails to
/// be added. The name field takes anything printable, which is the point of
/// having a name field at all.
fn accepts_char(field: CoordField, c: char) -> bool {
    match field {
        CoordField::Latitude | CoordField::Longitude => {
            c.is_ascii_digit() || c == '.' || c == '-' || c == '+'
        }
        CoordField::Name => !c.is_control(),
    }
}

// ── The window ─────────────────────────────────────────────────────

impl App for CompassApp {
    fn title(&self) -> String {
        String::from("Compass")
    }

    fn app_id(&self) -> String {
        String::from("compass")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<Duration> {
        // Nothing here moves on its own: the heading changes when the user
        // turns it and at no other time. A tick would be a redraw per second
        // of a picture that had not changed.
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::CloseRequested => Response::Exit,
            Event::Resize { width, height } => {
                // The size is remembered here as well as in `render`, because
                // a click can arrive after a resize and before the next frame,
                // and it must be answered against the window's real size.
                self.width = *width as f32;
                self.height = *height as f32;
                Response::Redraw
            }
            _ => {
                self.handle_event(event, (self.width, self.height));
                Response::Redraw
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.width = width;
        self.height = height;
        self.frame(width, height).into_tree()
    }
}

impl Probe for CompassApp {
    type Target = Target;
    type Outcome = ();
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) {
        self.width = size.0;
        self.height = size.1;
        self.handle_event(
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
            size,
        );
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) {
        self.width = size.0;
        self.height = size.1;
        self.handle_event(&Event::Key(key.clone()), size);
    }
}

fn main() -> ExitCode {
    // The previous `main` was `let _app = CompassApp::new();`. It built the
    // whole program -- rose, waypoints, three views -- and dropped it without
    // drawing a pixel or reading a key, then exited zero.
    let mut app = CompassApp::new();
    app::launch("compass", &mut app)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    // `float_cmp` is deliberately *not* in this list: two floats compared for
    // equality is as wrong in a test as anywhere else, and this file's
    // geometry assertions all carry a tolerance.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::event::Modifiers;

    // ── Helpers ─────────────────────────────────────────────────────

    /// The window size the keyboard tests deliver their events at.
    ///
    /// A key does not depend on the size, but `handle_event` takes one
    /// because a press does, and there is one entry point for both -- which
    /// is the point: the tests reach the program through the door the
    /// compositor uses.
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

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
            text: String::new(),
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
            text: String::new(),
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
                assert!((0.0..360.0).contains(&brg), "bearing out of range: {brg}");
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
        assert!((0.0..360.0).contains(&brg));
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
        app.handle_event(&event, SIZE);
        assert!((app.heading - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_rotate_left() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::Left, false, false));
        app.handle_event(&event, SIZE);
        assert!((app.heading - 359.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_rotate_right_shift() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::Right, true, false));
        app.handle_event(&event, SIZE);
        assert!((app.heading - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_rotate_left_shift() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::Left, true, false));
        app.handle_event(&event, SIZE);
        assert!((app.heading - 350.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = default_app();
        let event = Event::Key(make_release_event(Key::Right));
        app.handle_event(&event, SIZE);
        assert!((app.heading - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_toggle_units() {
        let mut app = default_app();
        assert_eq!(app.distance_unit, DistanceUnit::Kilometers);
        let event = Event::Key(make_key_event(Key::U, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.distance_unit, DistanceUnit::Miles);
        app.handle_event(&event, SIZE);
        assert_eq!(app.distance_unit, DistanceUnit::Kilometers);
    }

    #[test]
    fn test_key_switch_to_waypoints() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::W, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.view, View::Waypoints);
    }

    #[test]
    fn test_key_switch_to_coord_entry() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::C, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.view, View::CoordinateEntry);
    }

    #[test]
    fn test_key_escape_from_waypoints() {
        let mut app = default_app();
        app.view = View::Waypoints;
        let event = Event::Key(make_key_event(Key::Escape, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.view, View::Compass);
    }

    #[test]
    fn test_key_escape_from_coord_entry() {
        let mut app = default_app();
        app.view = View::CoordinateEntry;
        let event = Event::Key(make_key_event(Key::Escape, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.view, View::Compass);
    }

    #[test]
    fn test_key_mark_waypoint() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::M, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.waypoints.len(), 1);
    }

    #[test]
    fn test_key_move_position_up() {
        let mut app = default_app();
        let original_lat = app.position.lat;
        let event = Event::Key(make_key_event(Key::Up, false, false));
        app.handle_event(&event, SIZE);
        assert!(app.position.lat > original_lat);
    }

    #[test]
    fn test_key_move_position_down() {
        let mut app = default_app();
        let original_lat = app.position.lat;
        let event = Event::Key(make_key_event(Key::Down, false, false));
        app.handle_event(&event, SIZE);
        assert!(app.position.lat < original_lat);
    }

    #[test]
    fn test_key_declination_d() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::D, false, false));
        app.handle_event(&event, SIZE);
        assert!((app.declination - -1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_key_declination_ctrl_d() {
        let mut app = default_app();
        let event = Event::Key(make_key_event(Key::D, false, true));
        app.handle_event(&event, SIZE);
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
        app.handle_event(&event, SIZE);
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
        app.handle_event(&event, SIZE);
        assert_eq!(app.selected_waypoint, Some(0));
    }

    #[test]
    fn test_waypoint_list_navigate_up_at_top() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.view = View::Waypoints;
        app.selected_waypoint = Some(0);

        let event = Event::Key(make_key_event(Key::Up, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.selected_waypoint, Some(0));
    }

    #[test]
    fn test_waypoint_list_delete() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.view = View::Waypoints;
        app.selected_waypoint = Some(0);

        let event = Event::Key(make_key_event(Key::Delete, false, false));
        app.handle_event(&event, SIZE);
        assert!(app.waypoints.is_empty());
    }

    #[test]
    fn test_waypoint_list_enter_goes_back() {
        let mut app = default_app();
        app.view = View::Waypoints;
        let event = Event::Key(make_key_event(Key::Enter, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.view, View::Compass);
    }

    // ── Coordinate entry tests ──────────────────────────────────────

    #[test]
    fn test_coord_entry_tab_switches_field() {
        let mut app = default_app();
        app.view = View::CoordinateEntry;
        assert_eq!(app.active_coord_field, CoordField::Latitude);

        let event = Event::Key(make_key_event(Key::Tab, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.active_coord_field, CoordField::Longitude);

        // Tab reaches the name field. It used to skip it: `next` cycled
        // latitude and longitude only, so the name box was painted, labelled
        // and impossible to put a character in.
        app.handle_event(&event, SIZE);
        assert_eq!(app.active_coord_field, CoordField::Name);

        app.handle_event(&event, SIZE);
        assert_eq!(app.active_coord_field, CoordField::Latitude);
    }

    #[test]
    fn test_coord_entry_backspace() {
        let mut app = default_app();
        app.view = View::CoordinateEntry;
        app.entry_lat_buf = String::from("12.3");
        let event = Event::Key(make_key_event(Key::Backspace, false, false));
        app.handle_event(&event, SIZE);
        assert_eq!(app.entry_lat_buf, "12.");
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
        app.handle_event(&event, SIZE);
        assert_eq!(app.selected_waypoint, Some(1));
    }

    #[test]
    fn test_waypoint_select_by_number_out_of_range() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.selected_waypoint = None;

        // Trying to select waypoint 5 when only 1 exists
        let event = Event::Key(make_key_event(Key::Num6, false, false));
        app.handle_event(&event, SIZE);
        assert!(app.selected_waypoint.is_none());
    }

    #[test]
    fn test_escape_clears_waypoint_selection() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        app.selected_waypoint = Some(0);

        let event = Event::Key(make_key_event(Key::Escape, false, false));
        app.handle_event(&event, SIZE);
        assert!(app.selected_waypoint.is_none());
    }

    // ── The window ──────────────────────────────────────────────────
    //
    // Everything below drives the program the way the compositor does: at a
    // size, through `frame` and `handle_event`, with no access to a constant
    // the drawing pass is not allowed to read either. The tests above check
    // what the compass computes; these check that what it computes reaches a
    // window and that a pointer can reach it back.

    /// The window sizes every geometry sweep is run at.
    ///
    /// Not a grid for its own sake: each entry is a shape that breaks a
    /// different assumption -- narrower than the panel is worth, shorter than
    /// the header plus the status line, wider than it is tall and the other
    /// way round, and two that are not really windows at all.
    const GRID: [(f32, f32); 12] = [
        (900.0, 720.0),
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (640.0, 480.0),
        (480.0, 900.0),
        (1600.0, 300.0),
        (400.0, 300.0),
        (320.0, 240.0),
        (200.0, 160.0),
        (120.0, 90.0),
        (40.0, 30.0),
        (2.0, 2.0),
    ];

    /// A waypoint with a name of `len` characters, for the states that need
    /// a label too long for its column.
    fn long_named(app: &mut CompassApp, len: usize) {
        app.entry_lat_buf = String::from("12.5");
        app.entry_lon_buf = String::from("-3.25");
        app.entry_name_buf = "W".repeat(len);
        assert!(
            app.add_waypoint_from_entry(),
            "the fixture must add a waypoint"
        );
    }

    /// The states every geometry sweep is run over.
    ///
    /// A window is only as right as its worst state. Sweeping the default
    /// state proves the default state: it is the list with more waypoints
    /// than rows, the entry view with every buffer full, and the compass with
    /// no room for its panel that have somewhere to go wrong.
    fn states() -> Vec<(&'static str, CompassApp)> {
        let mut out: Vec<(&'static str, CompassApp)> = Vec::new();

        out.push(("compass, fresh", default_app()));

        let mut turned = default_app();
        turned.heading = 217.0;
        turned.declination = -13.0;
        out.push(("compass, turned and declined", turned));

        let mut marked = default_app();
        for _ in 0..3 {
            marked.add_waypoint_at_current_position();
        }
        out.push(("compass, three waypoints", marked));

        let mut full = default_app();
        for _ in 0..MAX_WAYPOINTS {
            full.add_waypoint_at_current_position();
        }
        full.selected_waypoint = Some(MAX_WAYPOINTS - 1);
        out.push(("compass, list full", full));

        let mut empty_list = default_app();
        empty_list.view = View::Waypoints;
        out.push(("list, empty", empty_list));

        let mut list = default_app();
        list.view = View::Waypoints;
        for _ in 0..MAX_WAYPOINTS {
            list.add_waypoint_at_current_position();
        }
        list.selected_waypoint = Some(MAX_WAYPOINTS - 1);
        out.push(("list, full and last selected", list));

        let mut wordy = default_app();
        wordy.view = View::Waypoints;
        long_named(&mut wordy, 64);
        out.push(("list, one very long name", wordy));

        let mut entry = default_app();
        entry.view = View::CoordinateEntry;
        out.push(("entry, empty", entry));

        let mut typed = default_app();
        typed.view = View::CoordinateEntry;
        typed.active_coord_field = CoordField::Name;
        typed.entry_lat_buf = String::from("-89.123456789012");
        typed.entry_lon_buf = String::from("179.123456789012");
        typed.entry_name_buf = String::from("Longest name yet");
        out.push(("entry, every buffer full", typed));

        out
    }

    /// Every run of text the frame drew, as `(text, x, y, size, max_width)`.
    fn text_runs(frame: &Frame<Target>) -> Vec<(String, f32, f32, f32, Option<f32>)> {
        frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    max_width,
                    ..
                } => Some((text.clone(), *x, *y, *font_size, *max_width)),
                _ => None,
            })
            .collect()
    }

    /// Where the picture drew a given run of text, as a point inside it.
    ///
    /// This is how a control is found when the point of the test is *which*
    /// control the press reaches. Asking `probe::rect_of` for the `Target`
    /// asks the drawing pass where it thinks its own labels are, and a pass
    /// that recorded every row one row off would answer with the same offset
    /// it made the mistake with -- so finding and clicking would cancel out
    /// and the test would pass over a list where every row chose its
    /// neighbour. The words are the only thing in the frame a user can read.
    fn text_point(app: &CompassApp, size: (f32, f32), wanted: &str) -> Option<(f32, f32)> {
        let frame = app.frame(size.0, size.1);
        let found: Vec<(f32, f32)> = text_runs(&frame)
            .into_iter()
            .filter(|(text, ..)| text == wanted)
            .map(|(_, x, y, font_size, max_width)| {
                (
                    x + max_width.unwrap_or(font_size) * 0.5,
                    y + font_size * 0.5,
                )
            })
            .collect();
        // Two runs reading the same words make "press the thing that says X"
        // ambiguous, and the caller would silently get whichever came first.
        // Better to stop and be told than to test the wrong rectangle.
        assert!(
            found.len() <= 1,
            "{} runs of text read {wanted:?}",
            found.len()
        );
        found.first().copied()
    }

    #[test]
    fn the_window_is_painted_edge_to_edge_at_every_size() {
        for (w, h) in GRID {
            let frame = default_app().frame(w, h);
            let first = frame.commands().first().expect("a frame draws something");
            match first {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => {
                    assert!(
                        (*x - 0.0).abs() < 0.01 && (*y - 0.0).abs() < 0.01,
                        "{w}x{h}: the background starts at ({x}, {y}), not the corner"
                    );
                    assert!(
                        (*width - w).abs() < 0.01 && (*height - h).abs() < 0.01,
                        "{w}x{h}: the background is {width}x{height} -- the compositor \
                         would show whatever was in the window before us in the rest"
                    );
                }
                other => panic!("{w}x{h}: the frame opens with {other:?}, not a background"),
            }
        }
    }

    #[test]
    fn the_frame_is_balanced_at_every_size_and_state() {
        for (name, app) in states() {
            for (w, h) in GRID {
                assert!(
                    app.frame(w, h).is_balanced(),
                    "{name} at {w}x{h}: a clip or translation was pushed and not popped, \
                     so every later hit box is measured in the wrong space"
                );
            }
        }
    }

    #[test]
    fn every_control_lies_inside_the_window() {
        for (name, app) in states() {
            for (w, h) in GRID {
                let frame = app.frame(w, h);
                for (target, rect) in frame.hits() {
                    assert!(
                        rect.x >= -0.01
                            && rect.y >= -0.01
                            && rect.right() <= w + 0.01
                            && rect.bottom() <= h + 0.01,
                        "{name} at {w}x{h}: {target:?} answers presses at {rect:?}, \
                         which is partly outside the window"
                    );
                    assert!(
                        !rect.is_empty(),
                        "{name} at {w}x{h}: {target:?} has an empty hit box"
                    );
                }
            }
        }
    }

    #[test]
    fn every_run_of_text_is_bounded_and_inside_the_window() {
        for (name, app) in states() {
            for (w, h) in GRID {
                let frame = app.frame(w, h);
                for (text, x, y, size, max_width) in text_runs(&frame) {
                    let Some(bound) = max_width else {
                        panic!(
                            "{name} at {w}x{h}: {text:?} is drawn with no max_width, so it \
                             runs as far as the string is long and over whatever is beside it"
                        );
                    };
                    assert!(
                        bound.is_finite() && bound > 0.0,
                        "{name} at {w}x{h}: {text:?} is bounded to {bound}"
                    );
                    assert!(
                        x >= -0.01 && x + bound <= w + 0.01,
                        "{name} at {w}x{h}: {text:?} spans {x}..{} across a {w}-wide window",
                        x + bound
                    );
                    assert!(
                        y >= -0.01 && y + size <= h + 0.01,
                        "{name} at {w}x{h}: {text:?} spans {y}..{} down a {h}-tall window",
                        y + size
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_is_drawn_over_the_status_line() {
        // The status line is the last thing drawn, so it cannot be painted
        // over -- but a control that reaches under it would still take the
        // press, and the user would be clicking a button they cannot see.
        for (name, app) in states() {
            for (w, h) in GRID {
                let l = Layout::solve(w, h);
                if l.status.is_empty() {
                    continue;
                }
                for (target, rect) in app.frame(w, h).hits() {
                    // The *bottom* edge, not the top. Asking only where a
                    // control starts lets one through that starts above the
                    // line and ends below it, which is the whole shape of the
                    // bug: the visible half is what the user aims at and the
                    // hidden half is what takes the press.
                    assert!(
                        rect.bottom() < l.status.y + 0.01,
                        "{name} at {w}x{h}: {target:?} at {rect:?} reaches under the \
                         status line, which is drawn after it"
                    );
                }
            }
        }
    }

    #[test]
    fn a_size_that_is_not_a_size_still_produces_a_window() {
        // A compositor that hands us a zero or a NaN is misbehaving, but the
        // answer to that is an empty window, not a frame full of NaN
        // rectangles that the renderer will read as enormous.
        for (w, h) in [
            (0.0, 0.0),
            (-100.0, -100.0),
            (f32::NAN, 720.0),
            (900.0, f32::NAN),
            (f32::INFINITY, f32::INFINITY),
        ] {
            let app = default_app();
            let frame = app.frame(w, h);
            assert!(frame.is_balanced(), "{w}x{h}: unbalanced");
            for (target, rect) in frame.hits() {
                assert!(
                    rect.x.is_finite()
                        && rect.y.is_finite()
                        && rect.w.is_finite()
                        && rect.h.is_finite(),
                    "{w}x{h}: {target:?} has hit box {rect:?}"
                );
            }
            for (text, x, y, _, max_width) in text_runs(&frame) {
                assert!(
                    x.is_finite() && y.is_finite() && max_width.is_some_and(f32::is_finite),
                    "{w}x{h}: {text:?} is drawn at ({x}, {y}) bounded to {max_width:?}"
                );
            }
        }
    }

    #[test]
    fn the_rose_is_a_circle_wherever_it_is_drawn() {
        // Inscribed in the larger dimension the rose would be an ellipse in
        // any window that is not 5:4, which is what a fixed radius against a
        // resizable window amounts to.
        for (w, h) in GRID {
            let l = Layout::solve(w, h);
            if l.radius <= 0.0 {
                continue;
            }
            assert!(
                (l.rose.w - l.rose.h).abs() < 0.01,
                "{w}x{h}: the rose is {}x{}",
                l.rose.w,
                l.rose.h
            );
            assert!(
                l.rose.x >= -0.01
                    && l.rose.right() <= w + 0.01
                    && l.rose.y >= l.body.y - 0.01
                    && l.rose.bottom() <= l.body.bottom() + 0.01,
                "{w}x{h}: the rose at {:?} leaves the body {:?}",
                l.rose,
                l.body
            );
        }
    }

    #[test]
    fn the_panel_is_given_up_before_the_rose_is() {
        // The order things are surrendered in is the order of what they are
        // worth. A compass with readouts and no rose is not a compass.
        let mut narrow = 0;
        for (w, h) in GRID {
            let l = Layout::solve(w, h);
            if l.panel.is_empty() {
                narrow += 1;
                continue;
            }
            assert!(
                l.radius > 0.0,
                "{w}x{h}: room for the panel but none for the rose"
            );
            assert!(
                l.panel.w <= w - l.panel.w + 0.01,
                "{w}x{h}: the panel takes {} of {w}, more than it leaves",
                l.panel.w
            );
        }
        assert!(
            narrow > 0,
            "no size in the grid is narrow enough to drop the panel, so the branch \
             that drops it is never exercised"
        );
    }

    #[test]
    fn a_window_with_no_panel_still_shows_the_heading() {
        // The heading is the one number the program exists to report. It
        // lives in the panel, and the panel is the first thing given up.
        let mut app = default_app();
        app.heading = 137.0;
        let mut checked = 0;
        for (w, h) in GRID {
            let l = Layout::solve(w, h);
            if !l.panel.is_empty() || l.body.h <= 0.0 {
                continue;
            }
            checked += 1;
            let frame = app.frame(w, h);
            assert!(
                text_runs(&frame)
                    .iter()
                    .any(|(text, ..)| text.starts_with("137")),
                "{w}x{h}: no panel and no heading either -- runs were {:?}",
                text_runs(&frame)
                    .iter()
                    .map(|(t, ..)| t.clone())
                    .collect::<Vec<_>>()
            );
        }
        assert!(checked > 0, "the no-panel case was never reached");
    }

    // ── The pointer ─────────────────────────────────────────────────
    //
    // Everything below finds a control by the words drawn in it and then
    // presses that point. Nothing here asks the drawing pass where it put
    // its own hit boxes: a pass that recorded every row one row off would
    // answer such a question with the same offset it made the mistake with,
    // and finding and pressing would cancel out.

    /// Press the left button at `at`, through the entry point the compositor
    /// uses.
    fn press(app: &mut CompassApp, at: (f32, f32), size: (f32, f32)) {
        app.click_at(at.0, at.1, MouseButton::Left, size);
    }

    /// The middle of the run of text reading `wanted`, or a failure naming it.
    fn point(app: &CompassApp, size: (f32, f32), wanted: &str) -> (f32, f32) {
        match text_point(app, size, wanted) {
            Some(p) => p,
            None => panic!("nothing on screen reads {wanted:?} at {size:?}"),
        }
    }

    /// Whether anything drawn reads exactly `wanted`.
    fn shows(app: &CompassApp, size: (f32, f32), wanted: &str) -> bool {
        text_runs(&app.frame(size.0, size.1))
            .iter()
            .any(|(text, ..)| text == wanted)
    }

    /// Add a waypoint with a name of our choosing, through the entry view.
    fn named(app: &mut CompassApp, name: &str, lat: f64, lon: f64) {
        app.entry_lat_buf = format!("{lat}");
        app.entry_lon_buf = format!("{lon}");
        app.entry_name_buf = String::from(name);
        assert!(app.add_waypoint_from_entry(), "the fixture must add {name}");
    }

    /// A list of `n` waypoints whose names are all different.
    ///
    /// Different names are the whole point: a test that presses "the row
    /// reading Delta" cannot tell which row it reached if two rows read the
    /// same thing.
    fn listed(n: usize) -> CompassApp {
        let mut app = default_app();
        app.view = View::Waypoints;
        for i in 0..n {
            named(
                &mut app,
                &format!("Mark{i}"),
                10.0 + i as f64,
                20.0 + i as f64,
            );
        }
        app.selected_waypoint = None;
        app
    }

    /// The middle of the rose, taken from the picture rather than the layout.
    ///
    /// `N` and `S` are drawn on opposite ends of the same ring, so the
    /// midpoint of the two is the centre they are drawn around -- exactly,
    /// at any heading, without asking `Layout` anything.
    fn rose_centre(app: &CompassApp, size: (f32, f32)) -> (f32, f32) {
        let n = point(app, size, "N");
        let s = point(app, size, "S");
        ((n.0 + s.0) * 0.5, (n.1 + s.1) * 0.5)
    }

    /// Which waypoints the frame recorded a hit box for.
    fn clickable_rows(app: &CompassApp, size: (f32, f32)) -> Vec<usize> {
        let mut rows: Vec<usize> = app
            .frame(size.0, size.1)
            .hits()
            .iter()
            .filter_map(|(t, _)| match t {
                Target::Waypoint(i) => Some(*i),
                _ => None,
            })
            .collect();
        rows.sort_unstable();
        rows
    }

    #[test]
    fn pressing_a_tab_switches_to_the_view_it_names() {
        // Before the header existed the views were reachable with `W`, `C`
        // and `Esc` and by nothing else.
        for start in [View::Compass, View::Waypoints, View::CoordinateEntry] {
            for (label, view, proof) in [
                (
                    "Waypoints",
                    View::Waypoints,
                    "No waypoints. Press C to add one, or Esc to go back.",
                ),
                ("Add waypoint", View::CoordinateEntry, "Add (Enter)"),
                ("Compass", View::Compass, "HEADING"),
            ] {
                let mut app = default_app();
                app.set_view(start);
                let at = point(&app, SIZE, label);
                press(&mut app, at, SIZE);
                assert_eq!(
                    app.view, view,
                    "pressing {label:?} from {start:?} reached the wrong view"
                );
                assert!(
                    shows(&app, SIZE, proof),
                    "pressing {label:?} from {start:?} left {proof:?} unpainted"
                );
            }
        }
    }

    #[test]
    fn the_unit_toggle_reads_the_unit_in_force_and_changes_it() {
        let mut app = default_app();
        assert!(shows(&app, SIZE, "km"), "the toggle must read the unit");

        let at = point(&app, SIZE, "km");
        press(&mut app, at, SIZE);
        assert!(
            shows(&app, SIZE, "mi"),
            "the press did not reach the toggle"
        );
        assert!(!shows(&app, SIZE, "km"), "both units are on screen at once");

        let at = point(&app, SIZE, "mi");
        press(&mut app, at, SIZE);
        assert!(shows(&app, SIZE, "km"), "the toggle only goes one way");
    }

    #[test]
    fn the_declination_steppers_move_it_the_way_they_are_labelled() {
        // `D` alone used to *decrease* declination, and the only mention of
        // it on screen was a help line. A control labelled `-` that adds is
        // the same bug wearing a button.
        let mut app = default_app();
        assert!(
            shows(&app, SIZE, "+0"),
            "the card must read the declination"
        );

        let minus = point(&app, SIZE, "-");
        let plus = point(&app, SIZE, "+");

        press(&mut app, minus, SIZE);
        assert!(
            (app.declination + 1.0).abs() < 1e-9,
            "the button reading - moved declination to {}",
            app.declination
        );
        assert!(shows(&app, SIZE, "-1"), "the readout did not follow");

        for _ in 0..2 {
            press(&mut app, plus, SIZE);
        }
        assert!(
            (app.declination - 1.0).abs() < 1e-9,
            "the button reading + moved declination to {}",
            app.declination
        );
        assert!(shows(&app, SIZE, "+1"), "the readout did not follow");
    }

    #[test]
    fn pressing_the_rose_points_the_compass_at_the_pressed_point() {
        // The rose is the only pointer route to a heading there is: without
        // it the compass turns with the arrow keys and in no other way.
        let mut app = default_app();
        // A magnetic heading and a declination that do not cancel, so a press
        // that ignored declination would answer differently. 35 - 13 = 22,
        // whose cardinal name is `NNE` -- three letters, so the panel readout
        // cannot be mistaken for one of the rose's own labels.
        app.heading = 35.0;
        app.declination = -13.0;

        let (cx, cy) = rose_centre(&app, SIZE);
        let n = point(&app, SIZE, "N");
        let ring = ((n.0 - cx).powi(2) + (n.1 - cy).powi(2)).sqrt();
        assert!(ring > 1.0, "the rose is too small to press meaningfully");

        // Up and to the right in equal measure: 45 degrees clockwise of north.
        let d = ring * std::f32::consts::FRAC_1_SQRT_2;
        press(&mut app, (cx + d, cy - d), SIZE);

        assert!(
            (app.true_heading() - 45.0).abs() < 0.5,
            "pressing 45 degrees round the rose gave {}",
            app.true_heading()
        );
        assert!(
            shows(&app, SIZE, "45"),
            "the heading readout did not follow the press"
        );
        assert!(
            (app.heading - 58.0).abs() < 0.5,
            "declination was not taken off the pressed bearing: magnetic {}",
            app.heading
        );

        // 45 degrees is the one bearing a compass that measured its angle from
        // east rather than from north would still get right, because the two
        // offsets are equal there. So press somewhere they are not: 20 degrees
        // reads back as 70 from a rose whose axes are swapped, and 20 is not a
        // multiple of 30, so the rose's own degree labels cannot supply it.
        let (cx, cy) = rose_centre(&app, SIZE);
        let n = point(&app, SIZE, "N");
        let ring = ((n.0 - cx).powi(2) + (n.1 - cy).powi(2)).sqrt();
        let twenty = 20.0_f32.to_radians();
        press(
            &mut app,
            (cx + ring * twenty.sin(), cy - ring * twenty.cos()),
            SIZE,
        );
        assert!(
            (app.true_heading() - 20.0).abs() < 0.5,
            "pressing 20 degrees round the rose gave {}",
            app.true_heading()
        );
        assert!(
            shows(&app, SIZE, "20"),
            "the heading readout did not follow the press"
        );
    }

    #[test]
    fn pressing_a_row_selects_the_waypoint_whose_name_it_shows() {
        let mut app = listed(6);
        for i in 0..6 {
            let name = format!("Mark{i}");
            let at = point(&app, SIZE, &name);
            press(&mut app, at, SIZE);
            let picked = app
                .selected_waypoint
                .and_then(|s| app.waypoints.get(s))
                .map(|w| w.name.clone());
            assert_eq!(
                picked.as_deref(),
                Some(name.as_str()),
                "pressing the row reading {name:?} selected {picked:?}"
            );
        }
    }

    #[test]
    fn no_press_inside_the_list_falls_between_two_rows() {
        // The gap under a row is drawn, and a press that lands in it has to
        // belong to somebody. Before the hit boxes covered the gaps a press
        // could land between two rows and select neither.
        let app = listed(6);
        let first = point(&app, SIZE, "Mark0");
        let last = point(&app, SIZE, "Mark5");
        let frame = app.frame(SIZE.0, SIZE.1);
        let mut y = first.1;
        while y <= last.1 {
            let hit = frame.hit_test(first.0, y);
            assert!(
                matches!(hit, Some(Target::Waypoint(_))),
                "a press at y={y} inside the list reached {hit:?}"
            );
            y += 0.5;
        }
    }

    /// A window short enough that the list cannot show all ten rows.
    const SHORT: (f32, f32) = (760.0, 230.0);

    #[test]
    fn a_row_that_does_not_fit_is_neither_drawn_nor_clickable() {
        // The old list re-derived its rows arithmetically from constants, so
        // a row that had been clipped away was still clickable at the
        // coordinates it no longer occupied.
        let app = listed(MAX_WAYPOINTS);
        let rows = clickable_rows(&app, SHORT);
        assert!(
            rows.len() < MAX_WAYPOINTS,
            "{SHORT:?} still fits every row -- the test proves nothing"
        );
        for i in 0..MAX_WAYPOINTS {
            let drawn = shows(&app, SHORT, &format!("Mark{i}"));
            assert_eq!(
                drawn,
                rows.contains(&i),
                "row {i}: drawn={drawn}, clickable={}",
                rows.contains(&i)
            );
        }
    }

    #[test]
    fn the_selected_waypoint_is_always_on_screen() {
        // A selection you cannot see is a selection you cannot act on: the
        // delete button would be armed for a row nowhere in the window.
        let mut app = listed(MAX_WAYPOINTS);
        for i in 0..MAX_WAYPOINTS {
            app.selected_waypoint = Some(i);
            assert!(
                shows(&app, SHORT, &format!("Mark{i}")),
                "waypoint {i} is selected and off screen at {SHORT:?}"
            );
        }
    }

    /// Type `text` at the window, one character at a time, as a keyboard does.
    fn type_str(app: &mut CompassApp, text: &str) {
        for ch in text.chars() {
            let mut buf = [0u8; 4];
            app.key_at(&guitk::probe::typing(ch.encode_utf8(&mut buf)), SIZE);
        }
    }

    #[test]
    fn delete_is_only_offered_when_nothing_would_be_lost_by_pressing_it() {
        // A button that accepts the press and does nothing is
        // indistinguishable from one that is broken.
        let mut app = listed(3);
        assert_eq!(app.selected_waypoint, None, "the fixture must start clear");

        let at = point(&app, SIZE, "Delete");
        let hit = app.frame(SIZE.0, SIZE.1).hit_test(at.0, at.1);
        assert!(
            !matches!(hit, Some(Target::DeleteWaypoint)),
            "delete answers a press with nothing selected"
        );
        press(&mut app, at, SIZE);
        assert_eq!(app.waypoints.len(), 3, "an unarmed delete removed a row");

        app.selected_waypoint = Some(1);
        let hit = app.frame(SIZE.0, SIZE.1).hit_test(at.0, at.1);
        assert!(
            matches!(hit, Some(Target::DeleteWaypoint)),
            "delete is drawn armed and answers nothing: {hit:?}"
        );
    }

    #[test]
    fn delete_removes_the_row_that_was_pressed_to_select_it() {
        let mut app = listed(4);
        let at = point(&app, SIZE, "Mark2");
        press(&mut app, at, SIZE);
        let del = point(&app, SIZE, "Delete");
        press(&mut app, del, SIZE);

        assert_eq!(app.waypoints.len(), 3, "delete removed the wrong number");
        assert!(!shows(&app, SIZE, "Mark2"), "the pressed row survived");
        for kept in ["Mark0", "Mark1", "Mark3"] {
            assert!(shows(&app, SIZE, kept), "{kept} was taken with it");
        }
    }

    #[test]
    fn pressing_a_field_gives_it_the_keyboard_and_typing_reaches_it() {
        // The name box was painted, labelled, and impossible to type into:
        // `Tab` cycled two fields and the pointer reached none of them.
        let mut app = default_app();
        app.set_view(View::CoordinateEntry);
        assert_eq!(app.active_coord_field, CoordField::Latitude);

        let at = point(&app, SIZE, "WP auto-name");
        press(&mut app, at, SIZE);
        assert_eq!(
            app.active_coord_field,
            CoordField::Name,
            "pressing the name box did not focus it"
        );
        type_str(&mut app, "Base camp");
        assert_eq!(app.entry_name_buf, "Base camp");
        assert!(
            shows(&app, SIZE, "Base camp"),
            "the typing is not on screen"
        );
        assert!(
            !shows(&app, SIZE, "WP auto-name"),
            "the placeholder outlived what was typed over it"
        );

        // And a second field, so that "the pointer focuses the name box" is
        // not passing because every press focuses the name box.
        let at = point(&app, SIZE, "e.g. 2.3522");
        press(&mut app, at, SIZE);
        assert_eq!(app.active_coord_field, CoordField::Longitude);
        type_str(&mut app, "2.35");
        assert_eq!(app.entry_lon_buf, "2.35");
        assert_eq!(app.entry_name_buf, "Base camp", "the wrong buffer took it");
    }

    #[test]
    fn the_add_button_makes_the_waypoint_the_fields_describe() {
        let mut app = default_app();
        app.set_view(View::CoordinateEntry);
        app.entry_lat_buf = String::from("48.8566");
        app.entry_lon_buf = String::from("2.3522");
        app.entry_name_buf = String::from("Paris");

        let at = point(&app, SIZE, "Add (Enter)");
        press(&mut app, at, SIZE);

        assert_eq!(app.waypoints.len(), 1, "the button added nothing");
        let wp = &app.waypoints[0];
        assert_eq!(wp.name, "Paris");
        assert!(
            (wp.coord.lat - 48.8566).abs() < 1e-9,
            "latitude {}",
            wp.coord.lat
        );
        assert!(
            (wp.coord.lon - 2.3522).abs() < 1e-9,
            "longitude {}",
            wp.coord.lon
        );
    }

    #[test]
    fn the_mark_button_makes_a_waypoint_at_the_position_on_screen() {
        let mut app = default_app();
        let at = point(&app, SIZE, "Mark here (M)");
        press(&mut app, at, SIZE);

        assert_eq!(app.waypoints.len(), 1, "the button added nothing");
        let wp = &app.waypoints[0];
        assert!((wp.coord.lat - app.position.lat).abs() < 1e-9);
        assert!((wp.coord.lon - app.position.lon).abs() < 1e-9);
        // The readouts agree, which is what a user checks it against.
        assert!(shows(&app, SIZE, "WP1"), "the new waypoint is not shown");
        assert!(
            shows(&app, SIZE, &app.position.format_lat()),
            "the position it was taken from is not on screen"
        );
    }

    #[test]
    fn a_full_list_says_so_rather_than_swallowing_the_press() {
        let mut app = default_app();
        for _ in 0..MAX_WAYPOINTS {
            assert!(app.add_waypoint_at_current_position());
        }
        // The button keeps its hit box and changes its words: a press that
        // landed on nothing would leave the user guessing why.
        let at = point(&app, SIZE, "List full");
        press(&mut app, at, SIZE);

        assert_eq!(app.waypoints.len(), MAX_WAYPOINTS, "the cap did not hold");
        assert!(
            shows(&app, SIZE, "Maximum 10 waypoints reached"),
            "the refusal was silent"
        );
    }

    #[test]
    fn a_press_on_bare_background_changes_nothing() {
        let mut app = default_app();
        app.add_waypoint_at_current_position();
        let frame = app.frame(SIZE.0, SIZE.1);

        let mut bare = None;
        let mut y = 3.0;
        while y < SIZE.1 && bare.is_none() {
            let mut x = 3.0;
            while x < SIZE.0 {
                if frame.hit_test(x, y).is_none() {
                    bare = Some((x, y));
                    break;
                }
                x += 7.0;
            }
            y += 7.0;
        }
        let bare = bare.expect("the whole window is covered in controls");

        let before = text_runs(&frame);
        press(&mut app, bare, SIZE);
        let after = text_runs(&app.frame(SIZE.0, SIZE.1));
        assert_eq!(
            before, after,
            "a press at {bare:?}, where nothing is drawn, changed the picture"
        );
    }

    /// A second window size, unlike the first in both dimensions.
    const OTHER: (f32, f32) = (1400.0, 900.0);

    #[test]
    fn a_press_is_answered_against_the_size_the_last_frame_was_drawn_at() {
        // The compositor calls `render(w, h)` and then hands over presses with
        // no size attached. Answering them against a constant answers them
        // against a window that is not on the screen.
        let mut app = default_app();
        let _ = App::render(&mut app, OTHER.0, OTHER.1);
        let at = point(&app, OTHER, "Waypoints");

        // The point has to be one the two sizes disagree about, or the test
        // would pass against a program that ignored the size entirely.
        let elsewhere = default_app().frame(SIZE.0, SIZE.1).hit_test(at.0, at.1);
        assert!(
            !matches!(elsewhere, Some(Target::Tab(View::Waypoints))),
            "{at:?} names the same tab at both sizes -- pick another control"
        );

        let response = App::on_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x: at.0,
                y: at.1,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        assert!(matches!(response, Response::Redraw));
        assert_eq!(
            app.view,
            View::Waypoints,
            "the press was answered against some other window"
        );
    }

    #[test]
    fn a_resize_moves_where_the_controls_answer() {
        let mut app = default_app();
        let _ = App::render(&mut app, SIZE.0, SIZE.1);
        let before = point(&app, SIZE, "Waypoints");

        let response = App::on_event(
            &mut app,
            &Event::Resize {
                width: OTHER.0 as u32,
                height: OTHER.1 as u32,
            },
        );
        assert!(matches!(response, Response::Redraw));

        let after = point(&app, OTHER, "Waypoints");
        assert!(
            (after.0 - before.0).abs() > 1.0 || (after.1 - before.1).abs() > 1.0,
            "the tab did not move when the window did"
        );

        // No `render` in between: the resize alone has to be enough, because
        // a press can arrive before the next frame is asked for.
        App::on_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x: after.0,
                y: after.1,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        assert_eq!(
            app.view,
            View::Waypoints,
            "the resize did not reach the hit boxes"
        );
    }

    #[test]
    fn the_picture_is_drawn_at_the_size_render_is_given() {
        // `render` is the only door the compositor draws through. Drawing a
        // constant-sized picture there would leave the window a border of
        // whatever was on the screen before.
        let mut app = default_app();
        let tree = App::render(&mut app, OTHER.0, OTHER.1);
        let want = app.frame(OTHER.0, OTHER.1).into_tree();
        assert_eq!(
            format!("{:?}", tree.commands),
            format!("{:?}", want.commands),
            "render drew something other than the picture for {OTHER:?}"
        );
    }

    #[test]
    fn the_close_button_closes_the_window_and_nothing_else_does() {
        let mut app = default_app();
        assert!(matches!(
            App::on_event(&mut app, &Event::CloseRequested),
            Response::Exit
        ));
        for event in [
            Event::FocusIn,
            Event::FocusOut,
            Event::Tick { elapsed_ms: 16 },
        ] {
            assert!(
                matches!(App::on_event(&mut app, &event), Response::Redraw),
                "{event:?} was answered with something other than a redraw"
            );
        }
    }

    #[test]
    fn every_clickable_row_is_a_whole_row() {
        // `Frame::hit` trims a hit box to the clip in force, so a row the list
        // only half has room for answers presses over the half of it that
        // shows -- a row painted with its lower half cut off and clickable
        // anyway. Every row is recorded at the same pitch, so a hit box
        // shorter than its neighbours is a row that did not fit.
        for size in [SIZE, SHORT, (620.0, 250.0), (900.0, 200.0)] {
            let app = listed(MAX_WAYPOINTS);
            let frame = app.frame(size.0, size.1);
            let heights: Vec<f32> = frame
                .hits()
                .iter()
                .filter(|(t, _)| matches!(t, Target::Waypoint(_)))
                .map(|(_, r)| r.h)
                .collect();
            let Some(first) = heights.first().copied() else {
                continue;
            };
            for h in &heights {
                assert!(
                    (h - first).abs() < 0.01,
                    "{size:?}: rows answer over {heights:?} -- one of them is a part row"
                );
            }
        }
    }

    #[test]
    fn a_panel_narrow_enough_to_squeeze_its_readouts_is_not_taken() {
        // The heading is the one thing a compass exists to show. A panel too
        // narrow to spell it is worse than no panel at all: the heading strip
        // that replaces it gets the whole width of the window.
        let mut app = default_app();
        app.heading = 217.0;
        app.declination = -13.0;
        // 204 -- not a multiple of 30, so the rose's own degree labels cannot
        // be mistaken for the readout.
        let reading = format!("{:.0}", app.true_heading());

        let mut seen = 0;
        for (w, h) in GRID {
            if Layout::solve(w, h).panel.is_empty() {
                continue;
            }
            seen += 1;
            let frame = app.frame(w, h);
            let run = text_runs(&frame)
                .into_iter()
                .find(|(t, ..)| *t == reading)
                .unwrap_or_else(|| panic!("{w}x{h}: a panel is drawn without the heading in it"));
            let (_, _, _, size, max_width) = run;
            let need = text::measure(&reading, size, FontWeightHint::Bold);
            let have = max_width.unwrap_or(0.0);
            assert!(
                have + 0.01 >= need,
                "{w}x{h}: the heading reads {reading} in {have} points of room and needs {need}"
            );
        }
        assert!(seen > 0, "no size in the grid draws a panel");
    }
}
