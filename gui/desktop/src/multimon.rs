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

    /// Bounding rectangle in virtual desktop space: `(x, y, w, h)`.
    pub fn bounds(&self) -> (i32, i32, u32, u32) {
        let (w, h) = self.effective_resolution();
        (self.position.0, self.position.1, w, h)
    }

    /// Whether the point `(x, y)` in virtual desktop space lies within this
    /// monitor's bounds.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        let (mx, my, mw, mh) = self.bounds();
        x >= mx && y >= my && x < mx + mw as i32 && y < my + mh as i32
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
    /// Returns `(x, y, width, height)`. If no monitors are enabled the result
    /// is `(0, 0, 0, 0)`.
    pub fn virtual_bounds(&self) -> (i32, i32, u32, u32) {
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;

        let mut any = false;
        for m in &self.monitors {
            if !m.enabled {
                continue;
            }
            any = true;
            let (mx, my, mw, mh) = m.bounds();
            if mx < min_x {
                min_x = mx;
            }
            if my < min_y {
                min_y = my;
            }
            let right = mx.saturating_add(mw as i32);
            let bottom = my.saturating_add(mh as i32);
            if right > max_x {
                max_x = right;
            }
            if bottom > max_y {
                max_y = bottom;
            }
        }

        if !any {
            return (0, 0, 0, 0);
        }

        let w = (max_x - min_x).max(0) as u32;
        let h = (max_y - min_y).max(0) as u32;
        (min_x, min_y, w, h)
    }

    /// The primary monitor, if any.
    pub fn primary(&self) -> Option<&MonitorInfo> {
        self.monitors.iter().find(|m| m.primary && m.enabled)
    }

    /// Find which enabled monitor contains the point `(x, y)`.
    pub fn monitor_at(&self, x: i32, y: i32) -> Option<&MonitorInfo> {
        self.monitors
            .iter()
            .find(|m| m.enabled && m.contains(x, y))
    }

    /// Snap a monitor's position so its edges align with neighbouring monitors.
    ///
    /// The snap threshold is 32 pixels. The returned position is the closest
    /// aligned position to `(x, y)` for the monitor identified by `id`.
    pub fn snap_position(&self, id: MonitorId, x: i32, y: i32) -> (i32, i32) {
        const SNAP_THRESHOLD: i32 = 32;

        // Find the target monitor to determine its size.
        let target = match self.monitors.iter().find(|m| m.id == id) {
            Some(m) => m,
            None => return (x, y),
        };
        let (tw, th) = target.effective_resolution();
        let tw = tw as i32;
        let th = th as i32;

        let mut best_x = x;
        let mut best_y = y;
        let mut dx_best = SNAP_THRESHOLD + 1;
        let mut dy_best = SNAP_THRESHOLD + 1;

        for other in &self.monitors {
            if other.id == id || !other.enabled {
                continue;
            }
            let (ox, oy, ow, oh) = other.bounds();
            let ow = ow as i32;
            let oh = oh as i32;

            // Snap target left edge to other right edge.
            let d = (x - (ox + ow)).abs();
            if d < dx_best {
                dx_best = d;
                best_x = ox + ow;
            }
            // Snap target right edge to other left edge.
            let d = ((x + tw) - ox).abs();
            if d < dx_best {
                dx_best = d;
                best_x = ox - tw;
            }
            // Snap target left edge to other left edge (alignment).
            let d = (x - ox).abs();
            if d < dx_best {
                dx_best = d;
                best_x = ox;
            }
            // Snap target right edge to other right edge.
            let d = ((x + tw) - (ox + ow)).abs();
            if d < dx_best {
                dx_best = d;
                best_x = ox + ow - tw;
            }

            // Snap target top edge to other bottom edge.
            let d = (y - (oy + oh)).abs();
            if d < dy_best {
                dy_best = d;
                best_y = oy + oh;
            }
            // Snap target bottom edge to other top edge.
            let d = ((y + th) - oy).abs();
            if d < dy_best {
                dy_best = d;
                best_y = oy - th;
            }
            // Snap target top edge to other top edge (alignment).
            let d = (y - oy).abs();
            if d < dy_best {
                dy_best = d;
                best_y = oy;
            }
            // Snap target bottom edge to other bottom edge.
            let d = ((y + th) - (oy + oh)).abs();
            if d < dy_best {
                dy_best = d;
                best_y = oy + oh - th;
            }
        }

        (best_x, best_y)
    }

    /// Detect axis-aligned rectangular gaps between enabled monitors.
    ///
    /// A "gap" is a rectangle inside the virtual bounding box that is not
    /// covered by any monitor. The implementation rasterises a grid defined by
    /// the horizontal and vertical edges of every monitor, then reports
    /// uncovered cells.
    pub fn detect_gaps(&self) -> Vec<(i32, i32, u32, u32)> {
        let enabled: Vec<&MonitorInfo> =
            self.monitors.iter().filter(|m| m.enabled).collect();
        if enabled.len() < 2 {
            return Vec::new();
        }

        // Collect unique x and y coordinates (edges of every monitor).
        let mut xs: Vec<i32> = Vec::new();
        let mut ys: Vec<i32> = Vec::new();
        for m in &enabled {
            let (mx, my, mw, mh) = m.bounds();
            xs.push(mx);
            xs.push(mx + mw as i32);
            ys.push(my);
            ys.push(my + mh as i32);
        }
        xs.sort_unstable();
        xs.dedup();
        ys.sort_unstable();
        ys.dedup();

        let mut gaps = Vec::new();

        // Check every grid cell formed by adjacent x/y boundaries.
        for xi in 0..xs.len().saturating_sub(1) {
            for yi in 0..ys.len().saturating_sub(1) {
                let cx = xs[xi];
                let cy = ys[yi];
                let cw = (xs[xi + 1] - cx) as u32;
                let ch = (ys[yi + 1] - cy) as u32;

                if cw == 0 || ch == 0 {
                    continue;
                }

                // Check if any monitor covers this cell.
                let mid_x = cx + (cw as i32) / 2;
                let mid_y = cy + (ch as i32) / 2;
                let covered = enabled.iter().any(|m| m.contains(mid_x, mid_y));
                if !covered {
                    gaps.push((cx, cy, cw, ch));
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
                let primary_id = self
                    .monitors
                    .iter()
                    .find(|m| m.primary)
                    .map(|m| m.id);
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
            if let Some(first) = self
                .layout
                .monitors
                .iter_mut()
                .find(|m| m.enabled)
            {
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
        // Sort connectors for deterministic output.
        let mut connectors: Vec<&String> = self.configs.keys().collect();
        connectors.sort();
        for (i, conn) in connectors.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let cfg = &self.configs[*conn];
            out.push('[');
            out.push_str(conn);
            out.push_str("]\n");
            out.push_str(&format!(
                "resolution={}x{}\n",
                cfg.resolution.0, cfg.resolution.1
            ));
            out.push_str(&format!(
                "position={},{}\n",
                cfg.position.0, cfg.position.1
            ));
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

        let flush =
            |connector: &Option<String>,
             res: &Option<(u32, u32)>,
             pos: &Option<(i32, i32)>,
             rot: &Option<Rotation>,
             scale: &Option<f32>,
             enabled: &Option<bool>,
             out: &mut HashMap<String, PerMonitorConfig>|
             -> Result<(), ConfigError> {
                if let Some(conn) = connector {
                    let r =
                        res.ok_or_else(|| ConfigError::MissingKey("resolution".into()))?;
                    let p =
                        pos.ok_or_else(|| ConfigError::MissingKey("position".into()))?;
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

            // Section header: [connector]
            if line.starts_with('[') && line.ends_with(']') {
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
                current_connector =
                    Some(line[1..line.len() - 1].to_string());
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
                        return Err(ConfigError::InvalidValue(format!(
                            "resolution: {val}"
                        )));
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
                        return Err(ConfigError::InvalidValue(format!(
                            "position: {val}"
                        )));
                    };
                    let x: i32 = xs.trim().parse().map_err(|_| {
                        ConfigError::InvalidValue(format!("position x: {xs}"))
                    })?;
                    let y: i32 = yst.trim().parse().map_err(|_| {
                        ConfigError::InvalidValue(format!("position y: {yst}"))
                    })?;
                    current_pos = Some((x, y));
                }
                "rotation" => {
                    let rot = Rotation::from_str_config(val).ok_or_else(|| {
                        ConfigError::InvalidValue(format!("rotation: {val}"))
                    })?;
                    current_rot = Some(rot);
                }
                "scale" => {
                    let s: f32 = val.parse().map_err(|_| {
                        ConfigError::InvalidValue(format!("scale: {val}"))
                    })?;
                    current_scale = Some(s);
                }
                "enabled" => {
                    let b: bool = val.parse().map_err(|_| {
                        ConfigError::InvalidValue(format!("enabled: {val}"))
                    })?;
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
    /// Center a window on the given monitor, preserving its size.
    ///
    /// Returns `(x, y, w, h)` in virtual desktop coordinates.
    pub fn place_on_monitor(
        window_rect: (i32, i32, u32, u32),
        monitor: &MonitorInfo,
    ) -> (i32, i32, u32, u32) {
        let (_, _, ww, wh) = window_rect;
        let (mx, my, mw, mh) = monitor.bounds();
        let x = mx + (mw as i32 - ww as i32) / 2;
        let y = my + (mh as i32 - wh as i32) / 2;
        (x, y, ww, wh)
    }

    /// Move a window from one monitor to another, preserving proportional
    /// position within the monitor.
    ///
    /// Returns the new `(x, y, w, h)` on the target monitor.
    pub fn move_to_monitor(
        window_rect: (i32, i32, u32, u32),
        from: &MonitorInfo,
        to: &MonitorInfo,
    ) -> (i32, i32, u32, u32) {
        let (wx, wy, ww, wh) = window_rect;
        let (fx, fy, fw, fh) = from.bounds();
        let (tx, ty, tw, th) = to.bounds();

        // Proportional offset within the source monitor.
        let rel_x = if fw > 0 {
            (wx - fx) as f64 / fw as f64
        } else {
            0.0
        };
        let rel_y = if fh > 0 {
            (wy - fy) as f64 / fh as f64
        } else {
            0.0
        };

        let new_x = tx + (rel_x * tw as f64) as i32;
        let new_y = ty + (rel_y * th as f64) as i32;

        (new_x, new_y, ww, wh)
    }

    /// Clamp a window rectangle so that at least a minimum portion is visible
    /// on some enabled monitor.
    ///
    /// The window is shifted (not resized) so that at least 48 pixels of width
    /// and 48 pixels of height are within the virtual bounding box.
    pub fn clamp_to_visible(
        rect: (i32, i32, u32, u32),
        layout: &MonitorLayout,
    ) -> (i32, i32, u32, u32) {
        let (bx, by, bw, bh) = layout.virtual_bounds();
        if bw == 0 || bh == 0 {
            // No enabled monitors -- return unchanged.
            return rect;
        }
        let (mut x, mut y, w, h) = rect;

        // Minimum overlap that must remain visible.
        let min_visible: i32 = 48;

        let right_limit = bx + bw as i32 - min_visible.min(w as i32);
        let bottom_limit = by + bh as i32 - min_visible.min(h as i32);
        let left_limit = bx - (w as i32 - min_visible.min(w as i32));
        let top_limit = by - (h as i32 - min_visible.min(h as i32));

        if x > right_limit {
            x = right_limit;
        }
        if x < left_limit {
            x = left_limit;
        }
        if y > bottom_limit {
            y = bottom_limit;
        }
        if y < top_limit {
            y = top_limit;
        }

        (x, y, w, h)
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
            .max_by_key(|m| {
                let (w, h) = m.effective_resolution();
                (w as u64) * (h as u64)
            })
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
        assert_eq!(m.bounds(), (100, 200, 1920, 1080));
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
        assert_eq!(Rotation::Normal.effective_resolution(1920, 1080), (1920, 1080));
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
        assert_eq!(Rotation::Left.effective_resolution(1920, 1080), (1080, 1920));
    }

    #[test]
    fn rotation_right_swaps_dimensions() {
        assert_eq!(Rotation::Right.effective_resolution(1920, 1080), (1080, 1920));
    }

    #[test]
    fn rotated_monitor_bounds() {
        let mut m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m.rotation = Rotation::Left;
        assert_eq!(m.effective_resolution(), (1080, 1920));
        assert_eq!(m.bounds(), (0, 0, 1080, 1920));
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
        assert_eq!(layout.virtual_bounds(), (0, 0, 1920, 1080));
    }

    #[test]
    fn virtual_bounds_two_horizontal() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                make_monitor(2, "DP-2", 2560, 1440, 1920, 0, false),
            ],
        };
        assert_eq!(layout.virtual_bounds(), (0, 0, 4480, 1440));
    }

    #[test]
    fn virtual_bounds_negative_coordinates() {
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, -1920, 0, false),
                make_monitor(2, "DP-2", 1920, 1080, 0, 0, true),
            ],
        };
        assert_eq!(layout.virtual_bounds(), (-1920, 0, 3840, 1080));
    }

    #[test]
    fn virtual_bounds_ignores_disabled() {
        let mut m2 = make_monitor(2, "DP-2", 2560, 1440, 1920, 0, false);
        m2.enabled = false;
        let layout = MonitorLayout {
            monitors: vec![
                make_monitor(1, "DP-1", 1920, 1080, 0, 0, true),
                m2,
            ],
        };
        assert_eq!(layout.virtual_bounds(), (0, 0, 1920, 1080));
    }

    #[test]
    fn virtual_bounds_all_disabled() {
        let mut m1 = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m1.enabled = false;
        let layout = MonitorLayout {
            monitors: vec![m1],
        };
        assert_eq!(layout.virtual_bounds(), (0, 0, 0, 0));
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
        assert!(!gaps.is_empty());
        let (gx, gy, gw, gh) = gaps[0];
        assert_eq!(gx, 1920);
        assert_eq!(gy, 0);
        assert_eq!(gw, 80);
        assert_eq!(gh, 1080);
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
        let total_gap_area: u64 = gaps.iter().map(|&(_, _, w, h)| w as u64 * h as u64).sum();
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
        let (x, y, w, h) = WindowPlacement::place_on_monitor((0, 0, 800, 600), &m);
        assert_eq!(w, 800);
        assert_eq!(h, 600);
        assert_eq!(x, (1920 - 800) / 2);
        assert_eq!(y, (1080 - 600) / 2);
    }

    #[test]
    fn place_on_monitor_offset_position() {
        let m = make_monitor(1, "DP-1", 1920, 1080, 1920, 0, true);
        let (x, y, _, _) = WindowPlacement::place_on_monitor((0, 0, 800, 600), &m);
        // Should be centered on the second monitor.
        assert_eq!(x, 1920 + (1920 - 800) / 2);
        assert_eq!(y, (1080 - 600) / 2);
    }

    #[test]
    fn move_to_monitor_proportional() {
        let from = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        let to = make_monitor(2, "DP-2", 2560, 1440, 1920, 0, false);

        // Window at the center of monitor 1.
        let (x, y, w, h) =
            WindowPlacement::move_to_monitor((960, 540, 400, 300), &from, &to);
        assert_eq!(w, 400);
        assert_eq!(h, 300);

        // Proportional position: 960/1920 = 0.5 of from => 0.5 * 2560 + 1920 = 3200
        assert_eq!(x, 1920 + 1280);
        assert_eq!(y, 720);
    }

    #[test]
    fn clamp_to_visible_within_bounds() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        let rect = (100, 100, 800, 600);
        assert_eq!(WindowPlacement::clamp_to_visible(rect, &layout), rect);
    }

    #[test]
    fn clamp_to_visible_off_right() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        let (x, _y, w, h) =
            WindowPlacement::clamp_to_visible((5000, 500, 800, 600), &layout);
        assert_eq!(w, 800);
        assert_eq!(h, 600);
        // Window should be pulled back so that at least 48px is visible.
        assert!(x < 5000);
        assert!(x + 48 <= 1920);
    }

    #[test]
    fn clamp_to_visible_off_left() {
        let layout = MonitorLayout {
            monitors: vec![make_monitor(1, "DP-1", 1920, 1080, 0, 0, true)],
        };
        let (x, _y, w, _h) =
            WindowPlacement::clamp_to_visible((-5000, 500, 800, 600), &layout);
        // At least 48px must overlap the monitor.
        assert!(x + w as i32 >= 48);
    }

    #[test]
    fn clamp_no_enabled_monitors() {
        let mut m = make_monitor(1, "DP-1", 1920, 1080, 0, 0, true);
        m.enabled = false;
        let layout = MonitorLayout { monitors: vec![m] };
        let rect = (5000, 5000, 800, 600);
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
        assert_eq!(layout.virtual_bounds(), (0, 0, 1920, 2160));
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
}
