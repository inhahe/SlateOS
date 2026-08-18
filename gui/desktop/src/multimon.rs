//! Multi-monitor management for the desktop shell.
//!
//! Provides monitor discovery, layout computation, configuration persistence,
//! and window placement helpers for multi-display setups. Works with the
//! compositor's per-monitor DPI scaling infrastructure
//! (see `guitk::scaling::ScaleContext`).
//!
//! # Architecture
//!
//! The [`MonitorManager`] owns the current [`MonitorLayout`] and mediates all
//! changes (hot-plug, user rearrangement, resolution/rotation changes).
//! [`MonitorConfig`] serialises per-connector settings so the layout survives
//! reboots. [`WindowPlacement`] provides helpers for centering, moving, and
//! clamping windows across monitors.

use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// MonitorId — opaque monitor handle
// ---------------------------------------------------------------------------

/// Opaque monitor identifier assigned by the compositor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonitorId(pub u32);

impl fmt::Display for MonitorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "monitor-{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// VirtualRect — a rectangle in virtual desktop space
// ---------------------------------------------------------------------------

/// Narrow an `i64` to an `i32`, clamping rather than wrapping.
///
/// Every rectangle computation in this module widens to `i64` first, so that
/// the intermediate — a right edge, a union, a snapped position — can never
/// wrap. This is where the result comes back down. Clamping is the right
/// failure: a monitor pushed past `i32::MAX` lands at the far edge of the
/// coordinate space rather than reappearing on the opposite side of it.
fn narrow(v: i64) -> i32 {
    i32::try_from(v).unwrap_or(if v.is_negative() { i32::MIN } else { i32::MAX })
}

/// A rectangle in virtual desktop space: a position in signed pixels and a
/// size in unsigned ones.
///
/// This exists because the four numbers were previously passed around loose,
/// as `(i32, i32, u32, u32)`, and every geometric question — where is the
/// right edge, does this contain that point, what box holds both of these —
/// was re-derived by hand at each call site, in `i32`, with the `w as i32`
/// cast and the overflow that comes with it written out fresh each time.
/// Sixteen monitors at 8K would be enough to overflow one of those additions;
/// so is one monitor placed near `i32::MAX` by a corrupt config file. Here the
/// widening happens once, inside the accessors, and no caller can forget it.
///
/// The rectangle is half-open: it covers `x..x + w` and `y..y + h`, so two
/// monitors placed edge to edge neither overlap nor leave a seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VirtualRect {
    /// Left edge in virtual desktop pixels.
    pub x: i32,
    /// Top edge in virtual desktop pixels.
    pub y: i32,
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
}

impl VirtualRect {
    /// A rectangle at `(x, y)` measuring `w` by `h`.
    #[must_use]
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// The rectangle spanning `left..right` by `top..bottom`.
    ///
    /// Corners are taken as `i64` because they are usually themselves the
    /// result of arithmetic on other rectangles. A reversed pair yields a
    /// zero-sized rectangle rather than a wrapped enormous one.
    #[must_use]
    pub fn from_corners(left: i64, top: i64, right: i64, bottom: i64) -> Self {
        let w = u32::try_from(right.saturating_sub(left).max(0)).unwrap_or(u32::MAX);
        let h = u32::try_from(bottom.saturating_sub(top).max(0)).unwrap_or(u32::MAX);
        Self::new(narrow(left), narrow(top), w, h)
    }

    /// Left edge, widened.
    #[must_use]
    pub const fn left(self) -> i64 {
        self.x as i64
    }

    /// Top edge, widened.
    #[must_use]
    pub const fn top(self) -> i64 {
        self.y as i64
    }

    /// The first column *past* the rectangle.
    ///
    /// Widened, so it is exact for every position and size a rectangle can
    /// hold: `i32::MAX + u32::MAX` is nowhere near `i64::MAX`. The saturating
    /// add is therefore unreachable, and is written that way rather than
    /// commented that way — a proof that lives in a comment is one the next
    /// change to the type is free to invalidate.
    #[must_use]
    pub const fn right(self) -> i64 {
        (self.x as i64).saturating_add(self.w as i64)
    }

    /// The first row *past* the rectangle. Exact, as [`right`](Self::right).
    #[must_use]
    pub const fn bottom(self) -> i64 {
        (self.y as i64).saturating_add(self.h as i64)
    }

    /// Width as a signed value, for mixing with positions. Saturates.
    #[must_use]
    pub fn width_i32(self) -> i32 {
        i32::try_from(self.w).unwrap_or(i32::MAX)
    }

    /// Height as a signed value. Saturates.
    #[must_use]
    pub fn height_i32(self) -> i32 {
        i32::try_from(self.h).unwrap_or(i32::MAX)
    }

    /// Whether the rectangle covers no pixels at all.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Whether `(x, y)` lies inside, treating the right and bottom edges as
    /// exclusive.
    #[must_use]
    pub const fn contains(self, x: i32, y: i32) -> bool {
        let (px, py) = (x as i64, y as i64);
        px >= self.left() && py >= self.top() && px < self.right() && py < self.bottom()
    }

    /// The centre point, rounded towards the top-left.
    #[must_use]
    pub fn center(self) -> (i32, i32) {
        (
            narrow(self.left().saturating_add(self.w as i64 / 2)),
            narrow(self.top().saturating_add(self.h as i64 / 2)),
        )
    }

    /// The smallest rectangle containing both.
    ///
    /// Empty rectangles are *not* skipped: a monitor configured with a
    /// zero-sized mode still has a position, and the desktop's bounding box
    /// has to reach it or the monitor becomes unreachable by the pointer.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self::from_corners(
            self.left().min(other.left()),
            self.top().min(other.top()),
            self.right().max(other.right()),
            self.bottom().max(other.bottom()),
        )
    }

    /// Area in pixels. Widened so a 4-billion-square desktop cannot wrap.
    #[must_use]
    pub const fn area(self) -> u64 {
        (self.w as u64).saturating_mul(self.h as u64)
    }
}

// ---------------------------------------------------------------------------
// Rotation
// ---------------------------------------------------------------------------

/// Physical rotation of a display panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rotation {
    /// Landscape (default orientation).
    Normal,
    /// 90 degrees clockwise — portrait with top at left.
    Left,
    /// 90 degrees counter-clockwise — portrait with top at right.
    Right,
    /// 180 degrees — upside-down landscape.
    Inverted,
}

impl Rotation {
    /// Effective pixel dimensions after rotation.
    ///
    /// For `Normal` and `Inverted` the native resolution is unchanged.
    /// For `Left` and `Right` width and height are swapped.
    pub fn effective_resolution(self, native_w: u32, native_h: u32) -> (u32, u32) {
        match self {
            Self::Normal | Self::Inverted => (native_w, native_h),
            Self::Left | Self::Right => (native_h, native_w),
        }
    }

    /// Convert to a human-readable label for config serialisation.
    fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Left => "left",
            Self::Right => "right",
            Self::Inverted => "inverted",
        }
    }

    /// Parse from a config string.
    fn from_str_config(s: &str) -> Option<Self> {
        match s.trim() {
            "normal" => Some(Self::Normal),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "inverted" => Some(Self::Inverted),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// MonitorInfo — describes a single connected display
// ---------------------------------------------------------------------------

/// Full description of a connected display.
#[derive(Clone, Debug)]
pub struct MonitorInfo {
    /// Unique identifier assigned by the compositor.
    pub id: MonitorId,
    /// Human-readable display name (e.g. "Dell U2723QE").
    pub name: String,
    /// Connector name (e.g. "DP-1", "HDMI-2").
    pub connector: String,
    /// Native (panel) resolution in pixels.
    pub resolution: (u32, u32),
    /// Vertical refresh rate in Hz.
    pub refresh_rate_hz: u32,
    /// Physical panel size in millimetres (width, height) for DPI calculation.
    pub physical_size_mm: (u32, u32),
    /// Top-left position in virtual desktop space.
    pub position: (i32, i32),
    /// Panel rotation.
    pub rotation: Rotation,
    /// Per-monitor DPI scale factor (1.0 = 96 DPI).
    pub scale_factor: f32,
    /// Whether this is the primary monitor.
    pub primary: bool,
    /// Whether a display is physically connected.
    pub connected: bool,
    /// Whether the user has enabled this display (a connected monitor can be
    /// software-disabled).
    pub enabled: bool,
}

impl MonitorInfo {
    /// Effective resolution after rotation.
    pub fn effective_resolution(&self) -> (u32, u32) {
        self.rotation
            .effective_resolution(self.resolution.0, self.resolution.1)
    }

    /// Bounding rectangle in virtual desktop space, after rotation.
    pub fn bounds(&self) -> VirtualRect {
        let (w, h) = self.effective_resolution();
        VirtualRect::new(self.position.0, self.position.1, w, h)
    }

    /// Whether the point `(x, y)` in virtual desktop space lies within this
    /// monitor's bounds.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.bounds().contains(x, y)
    }

    /// Calculate DPI from physical size and resolution.
    ///
    /// Returns the horizontal DPI, or `None` if physical size is unknown
    /// (zero).
    pub fn calculated_dpi(&self) -> Option<f32> {
        if self.physical_size_mm.0 == 0 {
            return None;
        }
        let inches = self.physical_size_mm.0 as f32 / 25.4;
        Some(self.resolution.0 as f32 / inches)
    }
}

// ---------------------------------------------------------------------------
// ArrangeMode
// ---------------------------------------------------------------------------

/// Strategy for automatic monitor arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArrangeMode {
    /// Line up monitors left-to-right in insertion order.
    Horizontal,
    /// Stack monitors top-to-bottom in insertion order.
    Vertical,
    /// All monitors display the same content (positions all overlap).
    Mirror,
    /// Only the primary monitor is enabled; others are disabled.
    Primary,
}

// ---------------------------------------------------------------------------
// Snap — the running best edge alignment along one axis
// ---------------------------------------------------------------------------

/// How near two monitor edges must come, in virtual desktop pixels, before a
/// drag snaps them together.
pub const SNAP_THRESHOLD: u32 = 32;

/// The best snap candidate found so far along one axis of a drag.
struct Snap {
    /// Gap between the two edges the winning candidate aligns.
    ///
    /// Seeded one past the threshold so that "nothing offered" and "nothing
    /// offered was close enough" are the same state, and neither needs a
    /// separate test at the end.
    distance: u64,
    /// Where the dragged monitor lands if this candidate wins.
    position: i32,
}

impl Snap {
    /// No candidate yet: the monitor stays at `position`.
    fn new(position: i32) -> Self {
        Self {
            distance: (SNAP_THRESHOLD as u64).saturating_add(1),
            position,
        }
    }

    /// Offer to move the monitor — currently at `origin` on this axis — so
    /// that its `edge` lands on `target`.
    ///
    /// Strictly closer wins, so among equally good candidates the first
    /// offered is kept.
    fn offer(&mut self, origin: i32, edge: i64, target: i64) {
        let distance = edge.abs_diff(target);
        if distance < self.distance {
            self.distance = distance;
            self.position = narrow(i64::from(origin).saturating_add(target.saturating_sub(edge)));
        }
    }
}

// ---------------------------------------------------------------------------
// MonitorLayout — arrangement of all monitors
// ---------------------------------------------------------------------------

/// The arrangement of all monitors in virtual desktop space.
#[derive(Clone, Debug, Default)]
pub struct MonitorLayout {
    /// All known monitors (both enabled and disabled).
    pub monitors: Vec<MonitorInfo>,
}

impl MonitorLayout {
    /// Bounding box encompassing every **enabled** monitor.
    ///
    /// An all-disabled layout has no bounding box at all, and gets the empty
    /// rectangle at the origin — which `is_empty` reports, so callers that
    /// must not divide by the desktop's size have one question to ask.
    pub fn virtual_bounds(&self) -> VirtualRect {
        self.monitors
            .iter()
            .filter(|m| m.enabled)
            .map(MonitorInfo::bounds)
            .reduce(VirtualRect::union)
            .unwrap_or_default()
    }

    /// The primary monitor, if any.
    pub fn primary(&self) -> Option<&MonitorInfo> {
        self.monitors.iter().find(|m| m.primary && m.enabled)
    }

    /// Find which enabled monitor contains the point `(x, y)`.
    pub fn monitor_at(&self, x: i32, y: i32) -> Option<&MonitorInfo> {
        self.monitors.iter().find(|m| m.enabled && m.contains(x, y))
    }

    /// Snap a monitor's position so its edges align with neighbouring monitors.
    ///
    /// The returned position is the one closest to `(x, y)` that puts one of
    /// the moving monitor's edges within [`SNAP_THRESHOLD`] pixels of an edge
    /// of some other enabled monitor. The two axes are decided independently:
    /// a drag can snap horizontally against one neighbour and vertically
    /// against another.
    ///
    /// Four candidates are offered per neighbour per axis, and they are all
    /// the same operation — *move so that this edge of mine lands on that edge
    /// of theirs*. Two of them abut (my left to their right, my right to their
    /// left) and two align (my left to their left, my right to their right).
    /// They were previously eight hand-written blocks, each recomputing the
    /// resulting position from scratch, which is eight chances to write
    /// `ox + ow - tw` when the case called for `ox + ow`.
    pub fn snap_position(&self, id: MonitorId, x: i32, y: i32) -> (i32, i32) {
        let Some(target) = self.monitors.iter().find(|m| m.id == id) else {
            return (x, y);
        };
        let (tw, th) = target.effective_resolution();
        let moving = VirtualRect::new(x, y, tw, th);

        let mut snap_x = Snap::new(x);
        let mut snap_y = Snap::new(y);

        for other in &self.monitors {
            if other.id == id || !other.enabled {
                continue;
            }
            let b = other.bounds();

            snap_x.offer(x, moving.left(), b.right());
            snap_x.offer(x, moving.right(), b.left());
            snap_x.offer(x, moving.left(), b.left());
            snap_x.offer(x, moving.right(), b.right());

            snap_y.offer(y, moving.top(), b.bottom());
            snap_y.offer(y, moving.bottom(), b.top());
            snap_y.offer(y, moving.top(), b.top());
            snap_y.offer(y, moving.bottom(), b.bottom());
        }

        (snap_x.position, snap_y.position)
    }

    /// Detect axis-aligned rectangular gaps between enabled monitors.
    ///
    /// A "gap" is a rectangle inside the virtual bounding box that is not
    /// covered by any monitor. The implementation rasterises a grid defined by
    /// the horizontal and vertical edges of every monitor, then reports
    /// uncovered cells. Testing the cell's centre suffices: the grid lines are
    /// exactly the monitor edges, so no monitor boundary can pass through the
    /// interior of a cell, and a cell is therefore covered either wholly or
    /// not at all.
    pub fn detect_gaps(&self) -> Vec<VirtualRect> {
        let enabled: Vec<VirtualRect> = self
            .monitors
            .iter()
            .filter(|m| m.enabled)
            .map(MonitorInfo::bounds)
            .collect();
        if enabled.len() < 2 {
            return Vec::new();
        }

        // The grid lines: every distinct monitor edge, on each axis.
        let mut xs: Vec<i64> = Vec::new();
        let mut ys: Vec<i64> = Vec::new();
        for b in &enabled {
            xs.push(b.left());
            xs.push(b.right());
            ys.push(b.top());
            ys.push(b.bottom());
        }
        xs.sort_unstable();
        xs.dedup();
        ys.sort_unstable();
        ys.dedup();

        let mut gaps = Vec::new();
        for column in xs.windows(2) {
            let [left, right] = *column else { continue };
            for row in ys.windows(2) {
                let [top, bottom] = *row else { continue };
                let cell = VirtualRect::from_corners(left, top, right, bottom);
                if cell.is_empty() {
                    continue;
                }
                let (mid_x, mid_y) = cell.center();
                if !enabled.iter().any(|b| b.contains(mid_x, mid_y)) {
                    gaps.push(cell);
                }
            }
        }

        gaps
    }

    /// Automatically reposition all enabled monitors according to `mode`.
    pub fn auto_arrange(&mut self, mode: ArrangeMode) {
        match mode {
            ArrangeMode::Horizontal => {
                let mut x: i32 = 0;
                for m in &mut self.monitors {
                    if !m.enabled {
                        continue;
                    }
                    m.position = (x, 0);
                    let (ew, _) = m.effective_resolution();
                    x = x.saturating_add(ew as i32);
                }
            }
            ArrangeMode::Vertical => {
                let mut y: i32 = 0;
                for m in &mut self.monitors {
                    if !m.enabled {
                        continue;
                    }
                    m.position = (0, y);
                    let (_, eh) = m.effective_resolution();
                    y = y.saturating_add(eh as i32);
                }
            }
            ArrangeMode::Mirror => {
                for m in &mut self.monitors {
                    if !m.enabled {
                        continue;
                    }
                    m.position = (0, 0);
                }
            }
            ArrangeMode::Primary => {
                // Disable everything except the primary.
                let primary_id = self.monitors.iter().find(|m| m.primary).map(|m| m.id);
                for m in &mut self.monitors {
                    if Some(m.id) == primary_id {
                        m.enabled = true;
                        m.position = (0, 0);
                    } else {
                        m.enabled = false;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MonitorManager
// ---------------------------------------------------------------------------

/// Manages the live set of monitors and mediates all mutations.
pub struct MonitorManager {
    layout: MonitorLayout,
}

impl MonitorManager {
    /// Create an empty manager with no monitors.
    pub fn new() -> Self {
        Self {
            layout: MonitorLayout::default(),
        }
    }

    /// Register a new monitor.
    pub fn add_monitor(&mut self, info: MonitorInfo) {
        // Avoid duplicates.
        if self.layout.monitors.iter().any(|m| m.id == info.id) {
            return;
        }
        self.layout.monitors.push(info);
    }

    /// Unregister a monitor (hot-unplug).
    ///
    /// If the removed monitor was primary, the first remaining enabled monitor
    /// becomes primary.
    pub fn remove_monitor(&mut self, id: MonitorId) {
        let was_primary = self
            .layout
            .monitors
            .iter()
            .find(|m| m.id == id)
            .is_some_and(|m| m.primary);

        self.layout.monitors.retain(|m| m.id != id);

        if was_primary {
            // Promote the first enabled monitor.
            if let Some(first) = self.layout.monitors.iter_mut().find(|m| m.enabled) {
                first.primary = true;
            }
        }
    }

    /// Change which monitor is primary.
    ///
    /// The previous primary (if any) is demoted.
    pub fn set_primary(&mut self, id: MonitorId) {
        for m in &mut self.layout.monitors {
            m.primary = m.id == id;
        }
    }

    /// Move a monitor to a new position in virtual desktop space.
    pub fn set_position(&mut self, id: MonitorId, x: i32, y: i32) {
        if let Some(m) = self.layout.monitors.iter_mut().find(|m| m.id == id) {
            m.position = (x, y);
        }
    }

    /// Change a monitor's rotation.
    pub fn set_rotation(&mut self, id: MonitorId, rotation: Rotation) {
        if let Some(m) = self.layout.monitors.iter_mut().find(|m| m.id == id) {
            m.rotation = rotation;
        }
    }

    /// Change a monitor's native resolution.
    pub fn set_resolution(&mut self, id: MonitorId, width: u32, height: u32) {
        if let Some(m) = self.layout.monitors.iter_mut().find(|m| m.id == id) {
            m.resolution = (width, height);
        }
    }

    /// Change a monitor's DPI scale factor.
    pub fn set_scale(&mut self, id: MonitorId, scale: f32) {
        if let Some(m) = self.layout.monitors.iter_mut().find(|m| m.id == id) {
            m.scale_factor = scale.clamp(0.25, 8.0);
        }
    }

    /// Enable a monitor (make it part of the active desktop).
    pub fn enable(&mut self, id: MonitorId) {
        if let Some(m) = self.layout.monitors.iter_mut().find(|m| m.id == id) {
            m.enabled = true;
        }
    }

    /// Disable a monitor (remove from active desktop without unplugging).
    pub fn disable(&mut self, id: MonitorId) {
        if let Some(m) = self.layout.monitors.iter_mut().find(|m| m.id == id) {
            m.enabled = false;
        }
    }

    /// Current monitor arrangement.
    pub fn layout(&self) -> &MonitorLayout {
        &self.layout
    }

    /// Auto-arrange all monitors.
    pub fn auto_arrange(&mut self, mode: ArrangeMode) {
        self.layout.auto_arrange(mode);
    }
}

impl Default for MonitorManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Configuration persistence
// ---------------------------------------------------------------------------

/// Error returned when loading a monitor configuration fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// A required key is missing from a monitor section.
    MissingKey(String),
    /// A value could not be parsed.
    InvalidValue(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey(k) => write!(f, "missing key: {k}"),
            Self::InvalidValue(v) => write!(f, "invalid value: {v}"),
        }
    }
}

/// Saved settings for a single monitor, keyed by connector name.
#[derive(Clone, Debug, PartialEq)]
pub struct PerMonitorConfig {
    pub resolution: (u32, u32),
    pub position: (i32, i32),
    pub rotation: Rotation,
    pub scale: f32,
    pub enabled: bool,
}

/// Persistent multi-monitor configuration.
///
/// Keyed by connector name so that the layout is restored when the same
/// physical cables are plugged in, even if monitor IDs change across boots.
#[derive(Clone, Debug, Default)]
pub struct MonitorConfig {
    pub configs: HashMap<String, PerMonitorConfig>,
}

impl MonitorConfig {
    /// Serialise to a simple key=value text format.
    ///
    /// Each monitor section starts with `[connector]` and is followed by
    /// key=value pairs, one per line. Sections are separated by blank lines.
    pub fn save_to_string(&self) -> String {
        let mut out = String::new();
        // Sorted for deterministic output. Iterating the pairs rather than the
        // keys means the value is never looked up a second time — `configs[k]`
        // panics if the key is absent, which is a panic that only a bug can
        // cause but which nothing in the type system rules out.
        let mut entries: Vec<(&String, &PerMonitorConfig)> = self.configs.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (i, (conn, cfg)) in entries.into_iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push('[');
            out.push_str(conn);
            out.push_str("]\n");
            out.push_str(&format!(
                "resolution={}x{}\n",
                cfg.resolution.0, cfg.resolution.1
            ));
            out.push_str(&format!("position={},{}\n", cfg.position.0, cfg.position.1));
            out.push_str(&format!("rotation={}\n", cfg.rotation.as_str()));
            out.push_str(&format!("scale={}\n", cfg.scale));
            out.push_str(&format!("enabled={}\n", cfg.enabled));
        }
        out
    }

    /// Deserialise from the key=value text format produced by
    /// [`save_to_string`](Self::save_to_string).
    pub fn load_from_string(s: &str) -> Result<Self, ConfigError> {
        let mut configs: HashMap<String, PerMonitorConfig> = HashMap::new();
        let mut current_connector: Option<String> = None;
        let mut current_res: Option<(u32, u32)> = None;
        let mut current_pos: Option<(i32, i32)> = None;
        let mut current_rot: Option<Rotation> = None;
        let mut current_scale: Option<f32> = None;
        let mut current_enabled: Option<bool> = None;

        let flush = |connector: &Option<String>,
                     res: &Option<(u32, u32)>,
                     pos: &Option<(i32, i32)>,
                     rot: &Option<Rotation>,
                     scale: &Option<f32>,
                     enabled: &Option<bool>,
                     out: &mut HashMap<String, PerMonitorConfig>|
         -> Result<(), ConfigError> {
            if let Some(conn) = connector {
                let r = res.ok_or_else(|| ConfigError::MissingKey("resolution".into()))?;
                let p = pos.ok_or_else(|| ConfigError::MissingKey("position".into()))?;
                let ro = rot.unwrap_or(Rotation::Normal);
                let sc = scale.unwrap_or(1.0);
                let en = enabled.unwrap_or(true);
                out.insert(
                    conn.clone(),
                    PerMonitorConfig {
                        resolution: r,
                        position: p,
                        rotation: ro,
                        scale: sc,
                        enabled: en,
                    },
                );
            }
            Ok(())
        };

        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Section header: [connector]. Stripping both delimiters in one
            // expression is what makes the name safe to take: the previous
            // `line[1..line.len() - 1]` was correct only because the
            // `starts_with`/`ends_with` pair above it had already run, and
            // `"["` alone satisfies both of those.
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                // Flush previous section.
                flush(
                    &current_connector,
                    &current_res,
                    &current_pos,
                    &current_rot,
                    &current_scale,
                    &current_enabled,
                    &mut configs,
                )?;
                current_connector = Some(name.to_string());
                current_res = None;
                current_pos = None;
                current_rot = None;
                current_scale = None;
                current_enabled = None;
                continue;
            }

            // key=value
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim();

            match key {
                "resolution" => {
                    let Some((ws, hs)) = val.split_once('x') else {
                        return Err(ConfigError::InvalidValue(format!("resolution: {val}")));
                    };
                    let w: u32 = ws.trim().parse().map_err(|_| {
                        ConfigError::InvalidValue(format!("resolution width: {ws}"))
                    })?;
                    let h: u32 = hs.trim().parse().map_err(|_| {
                        ConfigError::InvalidValue(format!("resolution height: {hs}"))
                    })?;
                    current_res = Some((w, h));
                }
                "position" => {
                    let Some((xs, yst)) = val.split_once(',') else {
                        return Err(ConfigError::InvalidValue(format!("position: {val}")));
                    };
                    let x: i32 = xs
                        .trim()
                        .parse()
                        .map_err(|_| ConfigError::InvalidValue(format!("position x: {xs}")))?;
                    let y: i32 = yst
                        .trim()
                        .parse()
                        .map_err(|_| ConfigError::InvalidValue(format!("position y: {yst}")))?;
                    current_pos = Some((x, y));
                }
                "rotation" => {
                    let rot = Rotation::from_str_config(val)
                        .ok_or_else(|| ConfigError::InvalidValue(format!("rotation: {val}")))?;
                    current_rot = Some(rot);
                }
                "scale" => {
                    let s: f32 = val
                        .parse()
                        .map_err(|_| ConfigError::InvalidValue(format!("scale: {val}")))?;
                    current_scale = Some(s);
                }
                "enabled" => {
                    let b: bool = val
                        .parse()
                        .map_err(|_| ConfigError::InvalidValue(format!("enabled: {val}")))?;
                    current_enabled = Some(b);
                }
                _ => {
                    // Ignore unknown keys for forward compatibility.
                }
            }
        }

        // Flush last section.
        flush(
            &current_connector,
            &current_res,
            &current_pos,
            &current_rot,
            &current_scale,
            &current_enabled,
            &mut configs,
        )?;

        Ok(Self { configs })
    }
}

// ---------------------------------------------------------------------------
// WindowPlacement
// ---------------------------------------------------------------------------

/// Helpers for placing and moving windows across monitors.
pub struct WindowPlacement;

impl WindowPlacement {
    /// How much of a window must remain inside the desktop, in pixels on each
    /// axis, for it to still be reachable with the pointer.
    pub const MIN_VISIBLE: u32 = 48;

    /// Center a window on the given monitor, preserving its size.
    pub fn place_on_monitor(window: VirtualRect, monitor: &MonitorInfo) -> VirtualRect {
        let m = monitor.bounds();
        let slack_x = i64::from(m.w).saturating_sub(i64::from(window.w)) / 2;
        let slack_y = i64::from(m.h).saturating_sub(i64::from(window.h)) / 2;
        VirtualRect::new(
            narrow(m.left().saturating_add(slack_x)),
            narrow(m.top().saturating_add(slack_y)),
            window.w,
            window.h,
        )
    }

    /// Move a window from one monitor to another, preserving proportional
    /// position within the monitor.
    pub fn move_to_monitor(
        window: VirtualRect,
        from: &MonitorInfo,
        to: &MonitorInfo,
    ) -> VirtualRect {
        let f = from.bounds();
        let t = to.bounds();

        // Proportional offset within the source monitor. A zero-sized source
        // has no interior to be proportional to, so the window goes to the
        // target's origin rather than dividing by nothing.
        let fraction = |offset: i64, extent: u32| -> f64 {
            if extent == 0 {
                0.0
            } else {
                offset as f64 / f64::from(extent)
            }
        };
        let rel_x = fraction(window.left().saturating_sub(f.left()), f.w);
        let rel_y = fraction(window.top().saturating_sub(f.top()), f.h);

        VirtualRect::new(
            narrow(t.left().saturating_add((rel_x * f64::from(t.w)) as i64)),
            narrow(t.top().saturating_add((rel_y * f64::from(t.h)) as i64)),
            window.w,
            window.h,
        )
    }

    /// Clamp a window rectangle so that at least a minimum portion is visible
    /// on some enabled monitor.
    ///
    /// The window is shifted, never resized, so that at least
    /// [`MIN_VISIBLE`](Self::MIN_VISIBLE) pixels of it lie within the virtual
    /// bounding box — or the whole of it, if it is smaller than that.
    pub fn clamp_to_visible(rect: VirtualRect, layout: &MonitorLayout) -> VirtualRect {
        let bounds = layout.virtual_bounds();
        if bounds.is_empty() {
            // No enabled monitors — there is nowhere to be visible.
            return rect;
        }

        // How much of the window has to stay on-screen, and how much may hang
        // off the far side.
        let keep_x = i64::from(Self::MIN_VISIBLE.min(rect.w));
        let keep_y = i64::from(Self::MIN_VISIBLE.min(rect.h));
        let overhang_x = i64::from(rect.w).saturating_sub(keep_x);
        let overhang_y = i64::from(rect.h).saturating_sub(keep_y);

        let far_x = bounds.right().saturating_sub(keep_x);
        let near_x = bounds.left().saturating_sub(overhang_x);
        let far_y = bounds.bottom().saturating_sub(keep_y);
        let near_y = bounds.top().saturating_sub(overhang_y);

        // `.min(far).max(near)`, not `.clamp(near, far)`: a desktop narrower
        // than `MIN_VISIBLE` puts `near` above `far`, and `clamp` panics on an
        // inverted range. Written this way the near edge wins, which is the
        // behaviour the sequential comparisons this replaces already had.
        VirtualRect::new(
            narrow(rect.left().min(far_x).max(near_x)),
            narrow(rect.top().min(far_y).max(near_y)),
            rect.w,
            rect.h,
        )
    }

    /// Suggest the best monitor for placing a new window.
    ///
    /// Returns the primary monitor if available, otherwise the enabled monitor
    /// with the largest area.
    pub fn suggest_default_monitor(layout: &MonitorLayout) -> Option<MonitorId> {
        if let Some(p) = layout.primary() {
            return Some(p.id);
        }
        layout
            .monitors
            .iter()
            .filter(|m| m.enabled)
            .max_by_key(|m| m.bounds().area())
            .map(|m| m.id)
    }
}

// ---------------------------------------------------------------------------
// Test helper — build a MonitorInfo with sensible defaults
// ---------------------------------------------------------------------------

#[cfg(test)]
fn make_monitor(
    id: u32,
    connector: &str,
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    primary: bool,
) -> MonitorInfo {
    MonitorInfo {
        id: MonitorId(id),
        name: format!("Monitor {id}"),
        connector: connector.to_string(),
        resolution: (w, h),
        refresh_rate_hz: 60,
        physical_size_mm: (600, 340),
        position: (x, y),
        rotation: Rotation::Normal,
        scale_factor: 1.0,
        primary,
        connected: true,
        enabled: true,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

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

    use super::*;

    // -- MonitorInfo basics ------------------------------------------------

    #[test]
    fn monitor_creation_and_fields() {
        let m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        assert_eq!(m.id, MonitorId(1));
        assert_eq!(m.resolution, (1920, 1080));
        assert!(m.primary);
        assert!(m.enabled);
        assert!(m.connected);
    }

    #[test]
    fn monitor_bounds() {
        let m = make_monitor(1, "DP-1", 1920, 1080, 100, 200, false);
        assert_eq!(m.bounds(), VirtualRect::new(100, 200, 1920, 1080));
    }

    #[test]
    fn monitor_contains_point() {
        let m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        assert!(m.contains(0, 0));
        assert!(m.contains(960, 540));
        assert!(m.contains(1919, 1079));
        assert!(!m.contains(1920, 540)); // right edge exclusive
        assert!(!m.contains(-1, 0));
    }

    #[test]
    fn monitor_calculated_dpi() {
        let mut m = make_monitor(1, "DP-1", 3840, 2160, 0, 0, true);
        m.physical_size_mm = (600, 340);
        let dpi = m.calculated_dpi().expect("should compute");
        // 3840 / (600 / 25.4) = 3840 / 23.622 ~ 162.56
        assert!((dpi - 162.56).abs() < 1.0);
    }

    #[test]
    fn monitor_dpi_zero_physical_size() {
        let mut m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m.physical_size_mm = (0, 0);
        assert!(m.calculated_dpi().is_none());
    }

    // -- Rotation ----------------------------------------------------------

    #[test]
    fn rotation_normal_preserves_resolution() {
        assert_eq!(
            Rotation::Normal.effective_resolution(1920, 1080),
            (1920, 1080)
        );
    }

    #[test]
    fn rotation_inverted_preserves_resolution() {
        assert_eq!(
            Rotation::Inverted.effective_resolution(1920, 1080),
            (1920, 1080)
        );
    }

    #[test]
    fn rotation_left_swaps_dimensions() {
        assert_eq!(
            Rotation::Left.effective_resolution(1920, 1080),
            (1080, 1920)
        );
    }

    #[test]
    fn rotation_right_swaps_dimensions() {
        assert_eq!(
            Rotation::Right.effective_resolution(1920, 1080),
            (1080, 1920)
        );
    }

    #[test]
    fn rotated_monitor_bounds() {
        let mut m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m.rotation = Rotation::Left;
        assert_eq!(m.effective_resolution(), (1080, 1920));
        assert_eq!(m.bounds(), VirtualRect::new(0, 0, 1080, 1920));
    }

    #[test]
    fn rotated_monitor_contains() {
        let mut m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m.rotation = Rotation::Left;
        // Effective: 1080x1920
        assert!(m.contains(500, 1000));
        assert!(!m.contains(1080, 0)); // right edge exclusive
    }

    // -- MonitorLayout virtual_bounds --------------------------------------

    #[test]
    fn virtual_bounds_single_monitor() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        assert_eq!(layout.virtual_bounds(), VirtualRect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn virtual_bounds_two_horizontal() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 2560, 1440, 1920, 0, false),
            ],
        };
        assert_eq!(layout.virtual_bounds(), VirtualRect::new(0, 0, 4480, 1440));
    }

    #[test]
    fn virtual_bounds_negative_coordinates() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, -1920, 0, false),
                make_monitor(2, "DP-2", 1920, 1080, 0, 0, true),
            ],
        };
        assert_eq!(
            layout.virtual_bounds(),
            VirtualRect::new(-1920, 0, 3840, 1080)
        );
    }

    #[test]
    fn virtual_bounds_ignores_disabled() {
        let mut m2 = make_monitor(2, "DP-2", 2560, 1440, 1920, 0, false);
        m2.enabled = false;
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true), m2],
        };
        assert_eq!(layout.virtual_bounds(), VirtualRect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn virtual_bounds_all_disabled() {
        let mut m1 = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m1.enabled = false;
        let layout = MonitorLayout { monitors: vec![m1] };
        assert_eq!(layout.virtual_bounds(), VirtualRect::new(0, 0, 0, 0));
    }

    // -- MonitorLayout primary ---------------------------------------------

    #[test]
    fn layout_primary() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, false),
                make_monitor(2, "DP-2", 2560, 1440, 1920, 0, true),
            ],
        };
        let p = layout.primary().expect("should have primary");
        assert_eq!(p.id, MonitorId(2));
    }

    #[test]
    fn layout_no_primary() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, false)],
        };
        assert!(layout.primary().is_none());
    }

    // -- MonitorLayout monitor_at ------------------------------------------

    #[test]
    fn monitor_at_finds_correct_monitor() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 2560, 1440, 1920, 0, false),
            ],
        };
        assert_eq!(
            layout.monitor_at(500, 500).map(|m| m.id),
            Some(MonitorId(1))
        );
        assert_eq!(
            layout.monitor_at(2000, 500).map(|m| m.id),
            Some(MonitorId(2))
        );
    }

    #[test]
    fn monitor_at_gap_returns_none() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 1920, 1080, 2000, 0, false),
            ],
        };
        // The 80px gap between monitors.
        assert!(layout.monitor_at(1950, 500).is_none());
    }

    #[test]
    fn monitor_at_disabled_ignored() {
        let mut m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m.enabled = false;
        let layout = MonitorLayout { monitors: vec![m] };
        assert!(layout.monitor_at(500, 500).is_none());
    }

    // -- snap_position -----------------------------------------------------

    #[test]
    fn snap_aligns_edges() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 1920, 1080, 1950, 10, false),
            ],
        };
        // Monitor 2 is at x=1950, only 30px from monitor 1's right edge (1920).
        // Snapping should bring it to x=1920.
        let (sx, sy) = layout.snap_position(MonitorId(2), 1950, 10);
        assert_eq!(sx, 1920);
        // y=10 is within 32px of y=0, should snap.
        assert_eq!(sy, 0);
    }

    #[test]
    fn snap_unknown_monitor_returns_original() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        assert_eq!(layout.snap_position(MonitorId(99), 100, 200), (100, 200));
    }

    #[test]
    fn snap_far_apart_no_snap() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 1920, 1080, 5000, 5000, false),
            ],
        };
        // Moving monitor 2 to (5000, 5000) -- too far from monitor 1 for snapping.
        let (sx, sy) = layout.snap_position(MonitorId(2), 5000, 5000);
        assert_eq!(sx, 5000);
        assert_eq!(sy, 5000);
    }

    // -- detect_gaps -------------------------------------------------------

    #[test]
    fn no_gaps_when_adjacent() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 1920, 1080, 1920, 0, false),
            ],
        };
        assert!(layout.detect_gaps().is_empty());
    }

    #[test]
    fn detect_gap_between_monitors() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 1920, 1080, 2000, 0, false),
            ],
        };
        let gaps = layout.detect_gaps();
        // There should be a gap at x=[1920..2000], y=[0..1080].
        assert_eq!(gaps.first(), Some(&VirtualRect::new(1920, 0, 80, 1080)));
    }

    #[test]
    fn detect_gaps_single_monitor() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        assert!(layout.detect_gaps().is_empty());
    }

    #[test]
    fn detect_gaps_stacked_with_offset() {
        // Two monitors stacked vertically but the bottom one is narrower,
        // creating a gap region to the right.
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 1280, 720, 0, 1080, false),
            ],
        };
        let gaps = layout.detect_gaps();
        // Gap should be at x=[1280..1920], y=[1080..1800].
        assert!(!gaps.is_empty());
        let total_gap_area: u64 = gaps.iter().map(|g| g.area()).sum();
        let expected = (1920 - 1280) as u64 * 720;
        assert_eq!(total_gap_area, expected);
    }

    // -- auto_arrange ------------------------------------------------------

    #[test]
    fn auto_arrange_horizontal() {
        let mut layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 999, 999, true),
                make_monitor(2, "DP-2", 2560, 1440, 999, 999, false),
            ],
        };
        layout.auto_arrange(ArrangeMode::Horizontal);
        assert_eq!(layout.monitors[0].position, (0, 0));
        assert_eq!(layout.monitors[1].position, (1920, 0));
    }

    #[test]
    fn auto_arrange_vertical() {
        let mut layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 2560, 1440, 0, 0, false),
            ],
        };
        layout.auto_arrange(ArrangeMode::Vertical);
        assert_eq!(layout.monitors[0].position, (0, 0));
        assert_eq!(layout.monitors[1].position, (0, 1080));
    }

    #[test]
    fn auto_arrange_mirror() {
        let mut layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 100, 200, true),
                make_monitor(2, "DP-2", 2560, 1440, 300, 400, false),
            ],
        };
        layout.auto_arrange(ArrangeMode::Mirror);
        assert_eq!(layout.monitors[0].position, (0, 0));
        assert_eq!(layout.monitors[1].position, (0, 0));
    }

    #[test]
    fn auto_arrange_primary_disables_others() {
        let mut layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 2560, 1440, 1920, 0, false),
            ],
        };
        layout.auto_arrange(ArrangeMode::Primary);
        assert!(layout.monitors[0].enabled);
        assert!(!layout.monitors[1].enabled);
        assert_eq!(layout.monitors[0].position, (0, 0));
    }

    // -- MonitorManager hot-plug -------------------------------------------

    #[test]
    fn manager_add_and_remove() {
        let mut mgr = MonitorManager::new();
        let m1 = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        let m2 = make_monitor(2, "HDMI-1", 2560, 1440, 1920, 0, false);

        mgr.add_monitor(m1);
        mgr.add_monitor(m2);
        assert_eq!(mgr.layout().monitors.len(), 2);

        mgr.remove_monitor(MonitorId(1));
        assert_eq!(mgr.layout().monitors.len(), 1);
        assert_eq!(mgr.layout().monitors[0].id, MonitorId(2));
    }

    #[test]
    fn manager_remove_primary_promotes() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 0, 0, true));
        mgr.add_monitor(make_monitor(2, "HDMI-1", 2560, 1440, 1920, 0, false));

        mgr.remove_monitor(MonitorId(1));
        // Monitor 2 should be promoted to primary.
        assert!(mgr.layout().monitors[0].primary);
    }

    #[test]
    fn manager_duplicate_add_ignored() {
        let mut mgr = MonitorManager::new();
        let m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        mgr.add_monitor(m.clone());
        mgr.add_monitor(m);
        assert_eq!(mgr.layout().monitors.len(), 1);
    }

    #[test]
    fn manager_set_primary() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 0, 0, true));
        mgr.add_monitor(make_monitor(2, "HDMI-1", 2560, 1440, 1920, 0, false));

        mgr.set_primary(MonitorId(2));
        assert!(!mgr.layout().monitors[0].primary);
        assert!(mgr.layout().monitors[1].primary);
    }

    #[test]
    fn manager_set_position() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 0, 0, true));
        mgr.set_position(MonitorId(1), 500, 300);
        assert_eq!(mgr.layout().monitors[0].position, (500, 300));
    }

    #[test]
    fn manager_set_rotation() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 0, 0, true));
        mgr.set_rotation(MonitorId(1), Rotation::Right);
        assert_eq!(mgr.layout().monitors[0].rotation, Rotation::Right);
    }

    #[test]
    fn manager_set_resolution() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 0, 0, true));
        mgr.set_resolution(MonitorId(1), 3840, 2160);
        assert_eq!(mgr.layout().monitors[0].resolution, (3840, 2160));
    }

    #[test]
    fn manager_set_scale() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 0, 0, true));
        mgr.set_scale(MonitorId(1), 2.0);
        assert!((mgr.layout().monitors[0].scale_factor - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn manager_scale_clamped() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 0, 0, true));
        mgr.set_scale(MonitorId(1), 100.0);
        assert!((mgr.layout().monitors[0].scale_factor - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn manager_enable_disable() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 0, 0, true));
        mgr.disable(MonitorId(1));
        assert!(!mgr.layout().monitors[0].enabled);
        mgr.enable(MonitorId(1));
        assert!(mgr.layout().monitors[0].enabled);
    }

    // -- MonitorConfig round-trip ------------------------------------------

    #[test]
    fn config_save_load_roundtrip() {
        let mut cfg = MonitorConfig::default();
        cfg.configs.insert(
            "DP-1".into(),
            PerMonitorConfig {
                resolution: (3840, 2160),
                position: (0, 0),
                rotation: Rotation::Normal,
                scale: 2.0,
                enabled: true,
            },
        );
        cfg.configs.insert(
            "HDMI-1".into(),
            PerMonitorConfig {
                resolution: (1920, 1080),
                position: (3840, 0),
                rotation: Rotation::Left,
                scale: 1.0,
                enabled: false,
            },
        );

        let text = cfg.save_to_string();
        let loaded = MonitorConfig::load_from_string(&text).expect("should parse");

        assert_eq!(loaded.configs.len(), 2);

        let dp = &loaded.configs["DP-1"];
        assert_eq!(dp.resolution, (3840, 2160));
        assert_eq!(dp.position, (0, 0));
        assert_eq!(dp.rotation, Rotation::Normal);
        assert!((dp.scale - 2.0).abs() < f32::EPSILON);
        assert!(dp.enabled);

        let hdmi = &loaded.configs["HDMI-1"];
        assert_eq!(hdmi.resolution, (1920, 1080));
        assert_eq!(hdmi.position, (3840, 0));
        assert_eq!(hdmi.rotation, Rotation::Left);
        assert!((hdmi.scale - 1.0).abs() < f32::EPSILON);
        assert!(!hdmi.enabled);
    }

    #[test]
    fn config_load_missing_key() {
        let text = "[DP-1]\nposition=0,0\n";
        let err = MonitorConfig::load_from_string(text).unwrap_err();
        assert!(matches!(err, ConfigError::MissingKey(_)));
    }

    #[test]
    fn config_load_invalid_resolution() {
        let text = "[DP-1]\nresolution=abc\nposition=0,0\n";
        let err = MonitorConfig::load_from_string(text).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue(_)));
    }

    #[test]
    fn config_empty_string() {
        let cfg = MonitorConfig::load_from_string("").expect("empty is valid");
        assert!(cfg.configs.is_empty());
    }

    // -- WindowPlacement ---------------------------------------------------

    #[test]
    fn place_on_monitor_centers() {
        let m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        let placed = WindowPlacement::place_on_monitor(VirtualRect::new(0, 0, 800, 600), &m);
        assert_eq!(
            placed,
            VirtualRect::new((1920 - 800) / 2, (1080 - 600) / 2, 800, 600)
        );
    }

    #[test]
    fn place_on_monitor_offset_position() {
        let m = make_monitor(1, "DP-1", 1920, 1080, 1920, 0, true);
        let placed = WindowPlacement::place_on_monitor(VirtualRect::new(0, 0, 800, 600), &m);
        // Should be centered on the second monitor.
        assert_eq!(placed.x, 1920 + (1920 - 800) / 2);
        assert_eq!(placed.y, (1080 - 600) / 2);
    }

    #[test]
    fn move_to_monitor_proportional() {
        let from = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        let to = make_monitor(2, "DP-2", 2560, 1440, 1920, 0, false);

        // Window at the center of monitor 1.
        let moved =
            WindowPlacement::move_to_monitor(VirtualRect::new(960, 540, 400, 300), &from, &to);

        // Proportional position: 960/1920 = 0.5 of from => 0.5 * 2560 + 1920 = 3200
        assert_eq!(moved, VirtualRect::new(1920 + 1280, 720, 400, 300));
    }

    #[test]
    fn clamp_to_visible_within_bounds() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        let rect = VirtualRect::new(100, 100, 800, 600);
        assert_eq!(WindowPlacement::clamp_to_visible(rect, &layout), rect);
    }

    #[test]
    fn clamp_to_visible_off_right() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        let clamped =
            WindowPlacement::clamp_to_visible(VirtualRect::new(5000, 500, 800, 600), &layout);
        assert_eq!(clamped.w, 800);
        assert_eq!(clamped.h, 600);
        // Window should be pulled back so that at least 48px is visible.
        assert!(clamped.x < 5000);
        assert!(clamped.x + 48 <= 1920);
    }

    #[test]
    fn clamp_to_visible_off_left() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        let clamped =
            WindowPlacement::clamp_to_visible(VirtualRect::new(-5000, 500, 800, 600), &layout);
        // At least 48px must overlap the monitor.
        assert!(clamped.right() >= 48);
    }

    #[test]
    fn clamp_no_enabled_monitors() {
        let mut m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m.enabled = false;
        let layout = MonitorLayout { monitors: vec![m] };
        let rect = VirtualRect::new(5000, 5000, 800, 600);
        // With no enabled monitors, clamping is a no-op.
        assert_eq!(WindowPlacement::clamp_to_visible(rect, &layout), rect);
    }

    #[test]
    fn suggest_default_monitor_primary() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 3840, 2160, 1920, 0, false),
            ],
        };
        assert_eq!(
            WindowPlacement::suggest_default_monitor(&layout),
            Some(MonitorId(1))
        );
    }

    #[test]
    fn suggest_default_monitor_largest_when_no_primary() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, false),
                make_monitor(2, "DP-2", 3840, 2160, 1920, 0, false),
            ],
        };
        // No primary -- pick the largest.
        assert_eq!(
            WindowPlacement::suggest_default_monitor(&layout),
            Some(MonitorId(2))
        );
    }

    #[test]
    fn suggest_default_monitor_empty_layout() {
        let layout = MonitorLayout {
            monitors: Vec::new(),
        };
        assert_eq!(WindowPlacement::suggest_default_monitor(&layout), None);
    }

    // -- Edge cases --------------------------------------------------------

    #[test]
    fn virtual_desktop_coordinates_with_stacked_layout() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 1920, 1080, 0, 1080, false),
            ],
        };
        assert_eq!(layout.virtual_bounds(), VirtualRect::new(0, 0, 1920, 2160));
        assert_eq!(
            layout.monitor_at(960, 500).map(|m| m.id),
            Some(MonitorId(1))
        );
        assert_eq!(
            layout.monitor_at(960, 1500).map(|m| m.id),
            Some(MonitorId(2))
        );
    }

    #[test]
    fn auto_arrange_skips_disabled() {
        let mut m2 = make_monitor(2, "DP-2", 2560, 1440, 0, 0, false);
        m2.enabled = false;
        let mut layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                m2,
                make_monitor(3, "DP-3", 1920, 1080, 0, 0, false),
            ],
        };
        layout.auto_arrange(ArrangeMode::Horizontal);
        // Disabled monitor should keep its position unchanged.
        assert_eq!(layout.monitors[0].position, (0, 0));
        assert!(!layout.monitors[1].enabled);
        assert_eq!(layout.monitors[2].position, (1920, 0));
    }

    #[test]
    fn manager_auto_arrange_delegates() {
        let mut mgr = MonitorManager::new();
        mgr.add_monitor(make_monitor(1, "DP-1", 1920, 1080, 999, 999, true));
        mgr.add_monitor(make_monitor(2, "DP-2", 2560, 1440, 999, 999, false));
        mgr.auto_arrange(ArrangeMode::Horizontal);
        assert_eq!(mgr.layout().monitors[0].position, (0, 0));
        assert_eq!(mgr.layout().monitors[1].position, (1920, 0));
    }

    // -- VirtualRect -------------------------------------------------------

    #[test]
    fn a_rectangle_at_the_far_edge_of_the_coordinate_space_does_not_wrap() {
        // The old code answered "where is the right edge?" as `x + w as i32`,
        // which for a monitor placed here overflows — a debug-build panic, and
        // in release a right edge to the *left* of the left edge.
        let r = VirtualRect::new(i32::MAX - 10, 0, 1000, 1000);
        assert_eq!(r.right(), i64::from(i32::MAX) - 10 + 1000);
        assert!(r.contains(i32::MAX, 500));
        assert!(!r.contains(i32::MIN, 500));
    }

    #[test]
    fn the_bounding_box_of_two_far_apart_monitors_saturates_rather_than_wrapping() {
        // `(max_x - min_x)` in i32, which is what this replaces, overflows for
        // any pair of monitors more than 2^31 pixels apart. A config file that
        // named such a position was enough to crash the shell at startup.
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, i32::MIN, 0, true),
                make_monitor(2, "DP-2", 1920, 1080, i32::MAX - 1920, 0, false),
            ],
        };
        let b = layout.virtual_bounds();
        assert_eq!(b.x, i32::MIN);
        assert_eq!(b.w, u32::MAX);
    }

    #[test]
    fn a_rectangle_built_from_reversed_corners_is_empty_not_enormous() {
        let r = VirtualRect::from_corners(100, 100, 40, 40);
        assert!(r.is_empty());
        assert_eq!(r, VirtualRect::new(100, 100, 0, 0));
    }

    #[test]
    fn the_union_reaches_a_monitor_even_if_that_monitor_shows_nothing() {
        // A monitor stuck in a zero-sized mode still has a position, and the
        // pointer can still be moved to it. Skipping empty rectangles in the
        // union would put it outside the desktop.
        let a = VirtualRect::new(0, 0, 100, 100);
        let b = VirtualRect::new(500, 500, 0, 0);
        assert_eq!(a.union(b), VirtualRect::new(0, 0, 500, 500));
    }

    #[test]
    fn a_rectangles_edges_are_half_open() {
        let r = VirtualRect::new(10, 20, 5, 5);
        assert!(r.contains(10, 20));
        assert!(r.contains(14, 24));
        // The right and bottom edges belong to whatever is on the other side.
        assert!(!r.contains(15, 24));
        assert!(!r.contains(14, 25));
    }

    // -- snap_position: all four alignments ---------------------------------

    #[test]
    fn every_snap_case_moves_the_edge_it_names_onto_the_edge_it_names() {
        // One neighbour at x = 1000..2000, and a 500-wide monitor offered four
        // positions, each 5px away from a different alignment. The four cases
        // share one implementation now, so this is the test that the shared
        // one is right for each of them rather than for the first one only.
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1000, 1000, 1000, 0, true),
                make_monitor(2, "DP-2", 500, 500, 0, 0, false),
            ],
        };
        let snap = |x: i32| layout.snap_position(MonitorId(2), x, 5000).0;

        // My left edge onto their right edge: land at 2000.
        assert_eq!(snap(2005), 2000);
        // My right edge onto their left edge: land at 1000 - 500 = 500.
        assert_eq!(snap(505), 500);
        // My left edge onto their left edge: land at 1000.
        assert_eq!(snap(1005), 1000);
        // My right edge onto their right edge: land at 2000 - 500 = 1500.
        assert_eq!(snap(1495), 1500);
    }

    #[test]
    fn a_snap_exactly_at_the_threshold_still_snaps_and_one_past_it_does_not() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1000, 1000, 0, 0, true),
                make_monitor(2, "DP-2", 500, 500, 0, 0, false),
            ],
        };
        let threshold = SNAP_THRESHOLD as i32;
        assert_eq!(
            layout.snap_position(MonitorId(2), 1000 + threshold, 0).0,
            1000
        );
        assert_eq!(
            layout
                .snap_position(MonitorId(2), 1000 + threshold + 1, 0)
                .0,
            1000 + threshold + 1
        );
    }

    #[test]
    fn a_disabled_neighbour_offers_nothing_to_snap_to() {
        let mut other = make_monitor(1, "DP-1", 1000, 1000, 0, 0, true);
        other.enabled = false;
        let layout = MonitorLayout {
            monitors: vec![other, make_monitor(2, "DP-2", 500, 500, 0, 0, false)],
        };
        assert_eq!(layout.snap_position(MonitorId(2), 1005, 1005), (1005, 1005));
    }

    // -- clamp_to_visible ---------------------------------------------------

    #[test]
    fn a_desktop_smaller_than_the_visible_minimum_does_not_panic() {
        // `MIN_VISIBLE` is 48, so a 1x1 desktop asks for more of the window to
        // stay on-screen than the desktop has room for: the near limit ends up
        // *past* the far one. Written as `.clamp(near, far)` that is a panic,
        // not a clamp — and it is reachable from any config file naming a
        // 1x1 mode.
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1, 1, 0, 0, true)],
        };
        let clamped =
            WindowPlacement::clamp_to_visible(VirtualRect::new(5000, 5000, 48, 48), &layout);
        // The near edge wins, putting the window's top-left on the desktop.
        assert_eq!(clamped, VirtualRect::new(0, 0, 48, 48));
    }

    #[test]
    fn a_window_smaller_than_the_visible_minimum_is_kept_whole_on_screen() {
        // `MIN_VISIBLE.min(w)` is what stops a 10px window being clamped as if
        // 48px of it had to remain visible, which would let it sit 38px off
        // the edge — entirely invisible.
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        let clamped =
            WindowPlacement::clamp_to_visible(VirtualRect::new(9000, 9000, 10, 10), &layout);
        assert_eq!(clamped, VirtualRect::new(1910, 1070, 10, 10));
        assert!(clamped.right() <= 1920);
        assert!(clamped.bottom() <= 1080);
    }

    // -- config parsing -----------------------------------------------------

    #[test]
    fn a_line_that_is_only_an_open_bracket_is_not_a_section_header() {
        let cfg =
            MonitorConfig::load_from_string("[\n[DP-1]\nresolution=1920x1080\nposition=0,0\n")
                .unwrap();
        assert_eq!(cfg.configs.len(), 1);
        assert!(cfg.configs.contains_key("DP-1"));
    }

    #[test]
    fn an_empty_section_name_round_trips_rather_than_slicing_off_a_bracket() {
        let cfg =
            MonitorConfig::load_from_string("[]\nresolution=800x600\nposition=1,2\n").unwrap();
        assert_eq!(cfg.configs.len(), 1);
        let entry = cfg.configs.get("").unwrap();
        assert_eq!(entry.resolution, (800, 600));
        assert_eq!(entry.position, (1, 2));
    }
}
