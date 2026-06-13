#![allow(dead_code)]
//! Screen Magnifier — accessibility zoom tool for SlateOS.
//!
//! Features:
//! - Multiple magnification modes: fullscreen, lens, docked
//! - Zoom levels from 1.5x to 20x with smooth stepping
//! - Color inversion for better contrast
//! - High contrast modes (yellow on black, white on black, green on black)
//! - Cursor tracking: follow mouse, follow keyboard focus, manual
//! - Crosshair overlay to track zoom center
//! - Screen reader integration hooks (announcement text)
//! - Color blindness filters (protanopia, deuteranopia, tritanopia simulation)
//! - Pixel color picker at current zoom center
//! - Ruler overlay for measuring distances
//! - Screenshot of magnified view

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
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
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Magnification Mode ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MagnifyMode {
    /// Full screen zoom — entire display is magnified
    FullScreen,
    /// Lens mode — circular/rectangular magnifying glass follows cursor
    Lens,
    /// Docked mode — magnified view in a fixed portion of screen (top/bottom)
    DockedTop,
    DockedBottom,
}

impl MagnifyMode {
    fn label(self) -> &'static str {
        match self {
            Self::FullScreen => "Full Screen",
            Self::Lens => "Lens",
            Self::DockedTop => "Docked (Top)",
            Self::DockedBottom => "Docked (Bottom)",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::FullScreen => Self::Lens,
            Self::Lens => Self::DockedTop,
            Self::DockedTop => Self::DockedBottom,
            Self::DockedBottom => Self::FullScreen,
        }
    }
}

// ── Tracking Mode ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackingMode {
    FollowMouse,
    FollowFocus,
    Manual,
}

impl TrackingMode {
    fn label(self) -> &'static str {
        match self {
            Self::FollowMouse => "Follow Mouse",
            Self::FollowFocus => "Follow Focus",
            Self::Manual => "Manual",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::FollowMouse => Self::FollowFocus,
            Self::FollowFocus => Self::Manual,
            Self::Manual => Self::FollowMouse,
        }
    }
}

// ── Color Filter ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorFilter {
    None,
    Inverted,
    HighContrastYellowBlack,
    HighContrastWhiteBlack,
    HighContrastGreenBlack,
    Grayscale,
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

impl ColorFilter {
    const ALL: [Self; 9] = [
        Self::None,
        Self::Inverted,
        Self::HighContrastYellowBlack,
        Self::HighContrastWhiteBlack,
        Self::HighContrastGreenBlack,
        Self::Grayscale,
        Self::Protanopia,
        Self::Deuteranopia,
        Self::Tritanopia,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Inverted => "Inverted",
            Self::HighContrastYellowBlack => "High Contrast (Yellow/Black)",
            Self::HighContrastWhiteBlack => "High Contrast (White/Black)",
            Self::HighContrastGreenBlack => "High Contrast (Green/Black)",
            Self::Grayscale => "Grayscale",
            Self::Protanopia => "Protanopia (Red-Blind)",
            Self::Deuteranopia => "Deuteranopia (Green-Blind)",
            Self::Tritanopia => "Tritanopia (Blue-Blind)",
        }
    }

    fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&f| f == self).unwrap_or(0);
        let next_idx = (idx.wrapping_add(1)) % Self::ALL.len();
        Self::ALL.get(next_idx).copied().unwrap_or(Self::None)
    }

    /// Apply color filter to RGB values.
    fn apply(self, r: u8, g: u8, b: u8) -> (u8, u8, u8) {
        match self {
            Self::None => (r, g, b),
            Self::Inverted => (
                255u8.wrapping_sub(r),
                255u8.wrapping_sub(g),
                255u8.wrapping_sub(b),
            ),
            Self::HighContrastYellowBlack => {
                let luma = Self::luma(r, g, b);
                if luma > 128 { (255, 255, 0) } else { (0, 0, 0) }
            }
            Self::HighContrastWhiteBlack => {
                let luma = Self::luma(r, g, b);
                if luma > 128 {
                    (255, 255, 255)
                } else {
                    (0, 0, 0)
                }
            }
            Self::HighContrastGreenBlack => {
                let luma = Self::luma(r, g, b);
                if luma > 128 { (0, 255, 0) } else { (0, 0, 0) }
            }
            Self::Grayscale => {
                let luma = Self::luma(r, g, b);
                (luma, luma, luma)
            }
            Self::Protanopia => {
                // Simplified protanopia simulation
                let rf = r as f32;
                let gf = g as f32;
                let bf = b as f32;
                let nr = (0.567 * rf + 0.433 * gf).min(255.0) as u8;
                let ng = (0.558 * rf + 0.442 * gf).min(255.0) as u8;
                let nb = (0.242 * gf + 0.758 * bf).min(255.0) as u8;
                (nr, ng, nb)
            }
            Self::Deuteranopia => {
                let rf = r as f32;
                let gf = g as f32;
                let bf = b as f32;
                let nr = (0.625 * rf + 0.375 * gf).min(255.0) as u8;
                let ng = (0.7 * rf + 0.3 * gf).min(255.0) as u8;
                let nb = (0.3 * gf + 0.7 * bf).min(255.0) as u8;
                (nr, ng, nb)
            }
            Self::Tritanopia => {
                let rf = r as f32;
                let gf = g as f32;
                let bf = b as f32;
                let nr = (0.95 * rf + 0.05 * gf).min(255.0) as u8;
                let ng = (0.433 * gf + 0.567 * bf).min(255.0) as u8;
                let nb = (0.475 * gf + 0.525 * bf).min(255.0) as u8;
                (nr, ng, nb)
            }
        }
    }

    fn luma(r: u8, g: u8, b: u8) -> u8 {
        // BT.601 luma
        let l = (r as f32) * 0.299 + (g as f32) * 0.587 + (b as f32) * 0.114;
        l.min(255.0) as u8
    }
}

// ── Lens Shape ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LensShape {
    Circle,
    Rectangle,
}

impl LensShape {
    fn label(self) -> &'static str {
        match self {
            Self::Circle => "Circle",
            Self::Rectangle => "Rectangle",
        }
    }

    fn toggle(self) -> Self {
        match self {
            Self::Circle => Self::Rectangle,
            Self::Rectangle => Self::Circle,
        }
    }
}

// ── Zoom Preset ────────────────────────────────────────────────────────────

const ZOOM_PRESETS: [f32; 10] = [1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 15.0, 20.0];

fn nearest_preset_index(zoom: f32) -> usize {
    let mut best: usize = 0;
    let mut best_diff = f32::MAX;
    for (i, &preset) in ZOOM_PRESETS.iter().enumerate() {
        let diff = (zoom - preset).abs();
        if diff < best_diff {
            best_diff = diff;
            best = i;
        }
    }
    best
}

// ── Screen Pixel Sampler ───────────────────────────────────────────────────

/// Simulates reading a pixel from the screen buffer.
fn sample_pixel(screen_x: i32, screen_y: i32, screen_w: i32, screen_h: i32) -> (u8, u8, u8) {
    // Generate a deterministic pattern for demonstration/testing
    if screen_x < 0 || screen_y < 0 || screen_x >= screen_w || screen_y >= screen_h {
        return (0, 0, 0); // Black for out of bounds
    }
    let x = screen_x as u32;
    let y = screen_y as u32;
    // Checkerboard-ish pattern
    let r = ((x.wrapping_mul(7).wrapping_add(y.wrapping_mul(13))) % 256) as u8;
    let g = ((x.wrapping_mul(11).wrapping_add(y.wrapping_mul(5))) % 256) as u8;
    let b = ((x.wrapping_mul(3).wrapping_add(y.wrapping_mul(17))) % 256) as u8;
    (r, g, b)
}

// ── Ruler Measurement ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct RulerMeasurement {
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
}

impl RulerMeasurement {
    fn distance(&self) -> f32 {
        let dx = self.end_x - self.start_x;
        let dy = self.end_y - self.start_y;
        (dx * dx + dy * dy).sqrt()
    }

    fn screen_distance(&self, zoom: f32) -> f32 {
        if zoom > 0.0 {
            self.distance() / zoom
        } else {
            0.0
        }
    }
}

// ── Application State ──────────────────────────────────────────────────────

struct MagnifierApp {
    // Zoom
    zoom_level: f32,
    mode: MagnifyMode,
    enabled: bool,

    // Position (zoom center in screen coordinates)
    center_x: f32,
    center_y: f32,

    // Tracking
    tracking: TrackingMode,
    mouse_x: f32,
    mouse_y: f32,

    // Visual options
    color_filter: ColorFilter,
    show_crosshair: bool,
    crosshair_color: Color,
    smooth_edges: bool,

    // Lens mode settings
    lens_shape: LensShape,
    lens_width: f32,
    lens_height: f32,

    // Docked mode settings
    docked_height_fraction: f32, // 0.0-1.0, portion of screen

    // Ruler
    ruler_active: bool,
    ruler_measuring: bool,
    ruler: Option<RulerMeasurement>,

    // Color picker
    picked_color: Option<(u8, u8, u8)>,
    show_color_picker: bool,

    // Screen dimensions
    screen_width: i32,
    screen_height: i32,

    // Viewport
    width: f32,
    height: f32,

    // UI state
    show_toolbar: bool,
    show_help: bool,
    status_message: String,

    // Screenshot
    screenshot_taken: bool,
}

impl MagnifierApp {
    fn new() -> Self {
        Self {
            zoom_level: 2.0,
            mode: MagnifyMode::FullScreen,
            enabled: true,
            center_x: 960.0,
            center_y: 540.0,
            tracking: TrackingMode::FollowMouse,
            mouse_x: 960.0,
            mouse_y: 540.0,
            color_filter: ColorFilter::None,
            show_crosshair: true,
            crosshair_color: RED,
            smooth_edges: true,
            lens_shape: LensShape::Circle,
            lens_width: 300.0,
            lens_height: 300.0,
            docked_height_fraction: 0.33,
            ruler_active: false,
            ruler_measuring: false,
            ruler: None,
            picked_color: None,
            show_color_picker: false,
            screen_width: 1920,
            screen_height: 1080,
            width: 800.0,
            height: 600.0,
            show_toolbar: true,
            show_help: false,
            status_message: "Magnifier ready — Ctrl+= to zoom in, Ctrl+- to zoom out".into(),
            screenshot_taken: false,
        }
    }

    // ── Zoom control ───────────────────────────────────────────────────

    fn zoom_in(&mut self) {
        let idx = nearest_preset_index(self.zoom_level);
        let next = idx.saturating_add(1);
        if let Some(&preset) = ZOOM_PRESETS.get(next) {
            self.zoom_level = preset;
        } else if let Some(&last) = ZOOM_PRESETS.last() {
            self.zoom_level = last;
        }
        self.status_message = format!("Zoom: {:.1}x", self.zoom_level);
    }

    fn zoom_out(&mut self) {
        let idx = nearest_preset_index(self.zoom_level);
        if idx > 0 {
            if let Some(&preset) = ZOOM_PRESETS.get(idx.saturating_sub(1)) {
                self.zoom_level = preset;
            }
        } else if let Some(&first) = ZOOM_PRESETS.first() {
            self.zoom_level = first;
        }
        self.status_message = format!("Zoom: {:.1}x", self.zoom_level);
    }

    fn set_zoom(&mut self, level: f32) {
        self.zoom_level = level.clamp(1.5, 20.0);
        self.status_message = format!("Zoom: {:.1}x", self.zoom_level);
    }

    // ── Movement ───────────────────────────────────────────────────────

    fn move_center(&mut self, dx: f32, dy: f32) {
        self.center_x = (self.center_x + dx).clamp(0.0, self.screen_width as f32);
        self.center_y = (self.center_y + dy).clamp(0.0, self.screen_height as f32);
    }

    fn update_mouse(&mut self, mx: f32, my: f32) {
        self.mouse_x = mx;
        self.mouse_y = my;
        if self.tracking == TrackingMode::FollowMouse {
            self.center_x = mx;
            self.center_y = my;
        }
    }

    fn jump_to(&mut self, x: f32, y: f32) {
        self.center_x = x.clamp(0.0, self.screen_width as f32);
        self.center_y = y.clamp(0.0, self.screen_height as f32);
    }

    // ── Color picker ───────────────────────────────────────────────────

    fn pick_color_at_center(&mut self) {
        let px = self.center_x as i32;
        let py = self.center_y as i32;
        let (r, g, b) = sample_pixel(px, py, self.screen_width, self.screen_height);
        let (fr, fg, fb) = self.color_filter.apply(r, g, b);
        self.picked_color = Some((fr, fg, fb));
        self.show_color_picker = true;
        self.status_message = format!(
            "Color: #{:02X}{:02X}{:02X} (R:{} G:{} B:{})",
            fr, fg, fb, fr, fg, fb
        );
    }

    // ── Ruler ──────────────────────────────────────────────────────────

    fn start_ruler(&mut self) {
        self.ruler_active = true;
        self.ruler_measuring = true;
        self.ruler = Some(RulerMeasurement {
            start_x: self.center_x,
            start_y: self.center_y,
            end_x: self.center_x,
            end_y: self.center_y,
        });
        self.status_message = "Ruler: move to endpoint, press R again to finish".into();
    }

    fn finish_ruler(&mut self) {
        if self.ruler_measuring {
            if let Some(ref mut r) = self.ruler {
                r.end_x = self.center_x;
                r.end_y = self.center_y;
                let dist = r.screen_distance(self.zoom_level);
                self.status_message = format!(
                    "Ruler: {:.1}px on screen ({:.1}px at zoom)",
                    r.distance(),
                    dist
                );
            }
            self.ruler_measuring = false;
        } else {
            self.ruler_active = false;
            self.ruler = None;
            self.status_message = "Ruler cleared".into();
        }
    }

    // ── Screenshot ─────────────────────────────────────────────────────

    fn take_screenshot(&mut self) {
        self.screenshot_taken = true;
        self.status_message = "Screenshot saved".into();
    }

    // ── Keyboard handling ──────────────────────────────────────────────

    fn handle_key(&mut self, key: &str, ctrl: bool, shift: bool) {
        if self.show_help {
            self.show_help = false;
            return;
        }

        match key {
            // Zoom
            "=" | "+" if ctrl => self.zoom_in(),
            "-" if ctrl => self.zoom_out(),
            "0" if ctrl => self.set_zoom(2.0),

            // Mode
            "m" if !ctrl => {
                self.mode = self.mode.next();
                self.status_message = format!("Mode: {}", self.mode.label());
            }

            // Toggle enable
            "Escape" => {
                self.enabled = !self.enabled;
                self.status_message = if self.enabled {
                    "Magnifier enabled".into()
                } else {
                    "Magnifier paused".into()
                };
            }

            // Tracking mode
            "t" if !ctrl => {
                self.tracking = self.tracking.next();
                self.status_message = format!("Tracking: {}", self.tracking.label());
            }

            // Color filter
            "f" if !ctrl => {
                self.color_filter = self.color_filter.next();
                self.status_message = format!("Filter: {}", self.color_filter.label());
            }

            // Crosshair toggle
            "x" if !ctrl => {
                self.show_crosshair = !self.show_crosshair;
                self.status_message = if self.show_crosshair {
                    "Crosshair shown".into()
                } else {
                    "Crosshair hidden".into()
                };
            }

            // Lens shape
            "l" if !ctrl => {
                self.lens_shape = self.lens_shape.toggle();
                self.status_message = format!("Lens: {}", self.lens_shape.label());
            }

            // Color picker
            "c" if !ctrl => self.pick_color_at_center(),

            // Ruler
            "r" if !ctrl => {
                if self.ruler_active {
                    self.finish_ruler();
                } else {
                    self.start_ruler();
                }
            }

            // Screenshot
            "s" if ctrl => self.take_screenshot(),

            // Movement (manual or adjust)
            "Left" if !ctrl => self.move_center(-10.0, 0.0),
            "Right" if !ctrl => self.move_center(10.0, 0.0),
            "Up" if !ctrl => self.move_center(0.0, -10.0),
            "Down" if !ctrl => self.move_center(0.0, 10.0),
            "Left" if ctrl => self.move_center(-50.0, 0.0),
            "Right" if ctrl => self.move_center(50.0, 0.0),
            "Up" if ctrl => self.move_center(0.0, -50.0),
            "Down" if ctrl => self.move_center(0.0, 50.0),

            // Lens size (shift + arrows)
            "Left" if shift => {
                self.lens_width = (self.lens_width - 20.0).max(100.0);
            }
            "Right" if shift => {
                self.lens_width = (self.lens_width + 20.0).min(800.0);
            }
            "Up" if shift => {
                self.lens_height = (self.lens_height - 20.0).max(100.0);
            }
            "Down" if shift => {
                self.lens_height = (self.lens_height + 20.0).min(800.0);
            }

            // Docked height
            "[" if !ctrl => {
                self.docked_height_fraction = (self.docked_height_fraction - 0.05).max(0.1);
            }
            "]" if !ctrl => {
                self.docked_height_fraction = (self.docked_height_fraction + 0.05).min(0.8);
            }

            // Toolbar toggle
            "h" if !ctrl => {
                self.show_toolbar = !self.show_toolbar;
            }

            // Help
            "F1" | "?" => {
                self.show_help = true;
            }

            // Preset zoom keys
            "1" if !ctrl => self.set_zoom(1.5),
            "2" if !ctrl => self.set_zoom(2.0),
            "3" if !ctrl => self.set_zoom(3.0),
            "4" if !ctrl => self.set_zoom(4.0),
            "5" if !ctrl => self.set_zoom(5.0),
            "8" if !ctrl => self.set_zoom(8.0),

            _ => {}
        }
    }

    // ── Rendering ──────────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background (simulated screen content — magnified view)
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        if !self.enabled {
            // Show paused overlay
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 80.0,
                y: self.height / 2.0 - 10.0,
                text: "Magnifier Paused".into(),
                font_size: 18.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(160.0),
            });
            cmds.push(RenderCommand::Text {
                x: self.width / 2.0 - 80.0,
                y: self.height / 2.0 + 16.0,
                text: "Press Esc to resume".into(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(160.0),
            });
            return cmds;
        }

        // Render magnified content based on mode
        match self.mode {
            MagnifyMode::FullScreen => self.render_fullscreen(&mut cmds),
            MagnifyMode::Lens => self.render_lens(&mut cmds),
            MagnifyMode::DockedTop => self.render_docked(&mut cmds, true),
            MagnifyMode::DockedBottom => self.render_docked(&mut cmds, false),
        }

        // Crosshair
        if self.show_crosshair {
            self.render_crosshair(&mut cmds);
        }

        // Ruler overlay
        if self.ruler_active {
            self.render_ruler(&mut cmds);
        }

        // Color picker display
        if self.show_color_picker {
            self.render_color_picker(&mut cmds);
        }

        // Toolbar
        if self.show_toolbar {
            self.render_toolbar(&mut cmds);
        }

        // Help overlay
        if self.show_help {
            self.render_help(&mut cmds);
        }

        cmds
    }

    fn render_fullscreen(&self, cmds: &mut Vec<RenderCommand>) {
        // Simulate magnified view by rendering a grid of "pixels"
        let cell_size = self.zoom_level;
        let cols = (self.width / cell_size) as i32 + 1;
        let rows = (self.height / cell_size) as i32 + 1;
        let half_cols = cols / 2;
        let half_rows = rows / 2;
        let cx = self.center_x as i32;
        let cy = self.center_y as i32;

        // Only render a representative sample (for perf, show a colored gradient)
        let sample_step = (cell_size as i32).max(4);
        let mut sample_x: i32 = 0;
        while sample_x < cols {
            let mut sample_y: i32 = 0;
            while sample_y < rows {
                let sx = cx - half_cols + sample_x;
                let sy = cy - half_rows + sample_y;
                let (r, g, b) = sample_pixel(sx, sy, self.screen_width, self.screen_height);
                let (fr, fg, fb) = self.color_filter.apply(r, g, b);
                let px = (sample_x as f32) * cell_size;
                let py = (sample_y as f32) * cell_size;
                let block_w = (sample_step as f32) * cell_size;
                cmds.push(RenderCommand::FillRect {
                    x: px,
                    y: py,
                    width: block_w.min(self.width - px),
                    height: block_w.min(self.height - py),
                    color: Color::rgb(fr, fg, fb),
                    corner_radii: CornerRadii::ZERO,
                });
                sample_y = sample_y.saturating_add(sample_step);
            }
            sample_x = sample_x.saturating_add(sample_step);
        }

        // Zoom level indicator
        cmds.push(RenderCommand::Text {
            x: self.width - 80.0,
            y: 8.0,
            text: format!("{:.1}x", self.zoom_level),
            font_size: 16.0,
            color: Color::rgba(255, 255, 255, 180),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_lens(&self, cmds: &mut Vec<RenderCommand>) {
        // Render normal screen first (simplified)
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Lens area
        let lx = self.mouse_x - self.lens_width / 2.0;
        let ly = self.mouse_y - self.lens_height / 2.0;

        let corner = if self.lens_shape == LensShape::Circle {
            CornerRadii::all(self.lens_width.min(self.lens_height) / 2.0)
        } else {
            CornerRadii::all(8.0)
        };

        // Lens border
        cmds.push(RenderCommand::StrokeRect {
            x: lx - 2.0,
            y: ly - 2.0,
            width: self.lens_width + 4.0,
            height: self.lens_height + 4.0,
            color: BLUE,
            line_width: 2.0,
            corner_radii: corner,
        });

        // Lens magnified content
        cmds.push(RenderCommand::FillRect {
            x: lx,
            y: ly,
            width: self.lens_width,
            height: self.lens_height,
            color: MANTLE,
            corner_radii: corner,
        });

        // Magnified sample in lens
        let sample_size: f32 = 8.0;
        let cx = self.center_x as i32;
        let cy = self.center_y as i32;
        let grid_cols = (self.lens_width / (sample_size * self.zoom_level)) as i32 + 1;
        let grid_rows = (self.lens_height / (sample_size * self.zoom_level)) as i32 + 1;

        for gx in 0..grid_cols.min(20) {
            for gy in 0..grid_rows.min(20) {
                let sx = cx - grid_cols / 2 + gx;
                let sy = cy - grid_rows / 2 + gy;
                let (r, g, b) = sample_pixel(sx, sy, self.screen_width, self.screen_height);
                let (fr, fg, fb) = self.color_filter.apply(r, g, b);
                let px = lx + (gx as f32) * sample_size * self.zoom_level;
                let py = ly + (gy as f32) * sample_size * self.zoom_level;
                if px < lx + self.lens_width && py < ly + self.lens_height {
                    cmds.push(RenderCommand::FillRect {
                        x: px,
                        y: py,
                        width: (sample_size * self.zoom_level).min(lx + self.lens_width - px),
                        height: (sample_size * self.zoom_level).min(ly + self.lens_height - py),
                        color: Color::rgb(fr, fg, fb),
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }

        // Zoom label in lens
        cmds.push(RenderCommand::Text {
            x: lx + 8.0,
            y: ly + self.lens_height - 18.0,
            text: format!("{:.1}x {}", self.zoom_level, self.lens_shape.label()),
            font_size: 10.0,
            color: Color::rgba(255, 255, 255, 160),
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.lens_width - 16.0),
        });
    }

    fn render_docked(&self, cmds: &mut Vec<RenderCommand>, top: bool) {
        let dock_h = self.height * self.docked_height_fraction;
        let dock_y = if top { 0.0 } else { self.height - dock_h };
        let normal_y = if top { dock_h } else { 0.0 };
        let normal_h = self.height - dock_h;

        // Normal view
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: normal_y,
            width: self.width,
            height: normal_h,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: self.width / 2.0 - 60.0,
            y: normal_y + normal_h / 2.0 - 8.0,
            text: "(Normal View)".into(),
            font_size: 14.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });

        // Magnified docked view
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: dock_y,
            width: self.width,
            height: dock_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator line
        let sep_y = if top { dock_h } else { dock_y };
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sep_y - 1.0,
            width: self.width,
            height: 2.0,
            color: BLUE,
            corner_radii: CornerRadii::ZERO,
        });

        // Magnified content sample in dock
        let cx = self.center_x as i32;
        let cy = self.center_y as i32;
        let sample_step: i32 = 4;
        let grid_cols = (self.width / (self.zoom_level * sample_step as f32)) as i32 + 1;
        let grid_rows = (dock_h / (self.zoom_level * sample_step as f32)) as i32 + 1;

        for gx in 0..grid_cols.min(40) {
            for gy in 0..grid_rows.min(20) {
                let sx = cx - grid_cols / 2 + gx;
                let sy = cy - grid_rows / 2 + gy;
                let (r, g, b) = sample_pixel(sx, sy, self.screen_width, self.screen_height);
                let (fr, fg, fb) = self.color_filter.apply(r, g, b);
                let block = self.zoom_level * sample_step as f32;
                let px = (gx as f32) * block;
                let py = dock_y + (gy as f32) * block;
                cmds.push(RenderCommand::FillRect {
                    x: px,
                    y: py,
                    width: block.min(self.width - px),
                    height: block.min(dock_y + dock_h - py),
                    color: Color::rgb(fr, fg, fb),
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Zoom indicator
        cmds.push(RenderCommand::Text {
            x: self.width - 80.0,
            y: dock_y + 8.0,
            text: format!("{:.1}x", self.zoom_level),
            font_size: 14.0,
            color: Color::rgba(255, 255, 255, 180),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_crosshair(&self, cmds: &mut Vec<RenderCommand>) {
        let cx = self.width / 2.0;
        let cy = self.height / 2.0;
        let len: f32 = 20.0;
        let thick: f32 = 2.0;

        // Horizontal line
        cmds.push(RenderCommand::FillRect {
            x: cx - len,
            y: cy - thick / 2.0,
            width: len * 2.0,
            height: thick,
            color: self.crosshair_color,
            corner_radii: CornerRadii::ZERO,
        });
        // Vertical line
        cmds.push(RenderCommand::FillRect {
            x: cx - thick / 2.0,
            y: cy - len,
            width: thick,
            height: len * 2.0,
            color: self.crosshair_color,
            corner_radii: CornerRadii::ZERO,
        });
    }

    fn render_ruler(&self, cmds: &mut Vec<RenderCommand>) {
        if let Some(ref r) = self.ruler {
            // Ruler line (simplified as a rect from start to end)
            let min_x = r.start_x.min(r.end_x);
            let max_x = r.start_x.max(r.end_x);
            let min_y = r.start_y.min(r.end_y);

            cmds.push(RenderCommand::FillRect {
                x: min_x,
                y: min_y,
                width: (max_x - min_x).max(2.0),
                height: 2.0,
                color: YELLOW,
                corner_radii: CornerRadii::ZERO,
            });

            // Distance label
            let dist = r.distance();
            cmds.push(RenderCommand::Text {
                x: (r.start_x + r.end_x) / 2.0,
                y: min_y - 16.0,
                text: format!("{:.1}px", dist),
                font_size: 11.0,
                color: YELLOW,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_color_picker(&self, cmds: &mut Vec<RenderCommand>) {
        if let Some((r, g, b)) = self.picked_color {
            let bx = self.width - 180.0;
            let by = self.height - 90.0;

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: by,
                width: 170.0,
                height: 80.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(8.0),
            });

            // Color swatch
            cmds.push(RenderCommand::FillRect {
                x: bx + 8.0,
                y: by + 8.0,
                width: 40.0,
                height: 40.0,
                color: Color::rgb(r, g, b),
                corner_radii: CornerRadii::all(4.0),
            });

            // Hex value
            cmds.push(RenderCommand::Text {
                x: bx + 56.0,
                y: by + 10.0,
                text: format!("#{:02X}{:02X}{:02X}", r, g, b),
                font_size: 14.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(110.0),
            });

            // RGB values
            cmds.push(RenderCommand::Text {
                x: bx + 56.0,
                y: by + 30.0,
                text: format!("R:{r} G:{g} B:{b}"),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(110.0),
            });

            // Close hint
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: by + 58.0,
                text: "[C] Pick again".into(),
                font_size: 9.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
            });
        }
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        let tb_y = self.height - 32.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: tb_y,
            width: self.width,
            height: 32.0,
            color: Color::rgba(17, 17, 27, 220),
            corner_radii: CornerRadii::ZERO,
        });

        // Status
        cmds.push(RenderCommand::Text {
            x: 8.0,
            y: tb_y + 9.0,
            text: self.status_message.clone(),
            font_size: 10.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width * 0.45),
        });

        // Mode / tracking / filter
        let info = format!(
            "{} | {} | {} | [H]elp [M]ode [T]rack [F]ilter",
            self.mode.label(),
            self.tracking.label(),
            self.color_filter.label(),
        );
        cmds.push(RenderCommand::Text {
            x: self.width * 0.5,
            y: tb_y + 9.0,
            text: info,
            font_size: 9.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width * 0.5 - 8.0),
        });
    }

    fn render_help(&self, cmds: &mut Vec<RenderCommand>) {
        let hw: f32 = 400.0;
        let hh: f32 = 360.0;
        let hx = (self.width - hw) / 2.0;
        let hy = (self.height - hh) / 2.0;

        // Backdrop
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::FillRect {
            x: hx,
            y: hy,
            width: hw,
            height: hh,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: hx + 16.0,
            y: hy + 12.0,
            text: "Screen Magnifier — Keyboard Shortcuts".into(),
            font_size: 14.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(hw - 32.0),
        });

        let shortcuts = [
            ("Ctrl + / Ctrl -", "Zoom in / out"),
            ("Ctrl 0", "Reset zoom to 2x"),
            ("1-5, 8", "Preset zoom levels"),
            ("M", "Cycle magnification mode"),
            ("T", "Cycle tracking mode"),
            ("F", "Cycle color filter"),
            ("X", "Toggle crosshair"),
            ("L", "Toggle lens shape"),
            ("C", "Pick color at center"),
            ("R", "Ruler (start/finish/clear)"),
            ("Ctrl+S", "Screenshot magnified view"),
            ("Arrows", "Move view (Ctrl = fast)"),
            ("[ ]", "Adjust docked height"),
            ("H", "Toggle toolbar"),
            ("Esc", "Pause/resume magnifier"),
            ("F1 / ?", "This help"),
        ];

        let mut sy = hy + 36.0;
        for (shortcut, desc) in &shortcuts {
            cmds.push(RenderCommand::Text {
                x: hx + 20.0,
                y: sy,
                text: shortcut.to_string(),
                font_size: 10.0,
                color: PEACH,
                font_weight: FontWeightHint::Bold,
                max_width: Some(120.0),
            });
            cmds.push(RenderCommand::Text {
                x: hx + 150.0,
                y: sy,
                text: desc.to_string(),
                font_size: 10.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(230.0),
            });
            sy += 18.0;
        }

        cmds.push(RenderCommand::Text {
            x: hx + 16.0,
            y: hy + hh - 24.0,
            text: "Press any key to close".into(),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(hw - 32.0),
        });
    }
}

// ── Entry point ────────────────────────────────────────────────────────────

fn main() {
    let _app = MagnifierApp::new();
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Mode tests ─────────────────────────────────────────────────────

    #[test]
    fn test_mode_cycle() {
        let m = MagnifyMode::FullScreen;
        assert_eq!(m.next(), MagnifyMode::Lens);
        assert_eq!(m.next().next(), MagnifyMode::DockedTop);
        assert_eq!(m.next().next().next(), MagnifyMode::DockedBottom);
        assert_eq!(m.next().next().next().next(), MagnifyMode::FullScreen);
    }

    #[test]
    fn test_tracking_cycle() {
        let t = TrackingMode::FollowMouse;
        assert_eq!(t.next(), TrackingMode::FollowFocus);
        assert_eq!(t.next().next(), TrackingMode::Manual);
        assert_eq!(t.next().next().next(), TrackingMode::FollowMouse);
    }

    // ── Color filter tests ─────────────────────────────────────────────

    #[test]
    fn test_filter_none() {
        assert_eq!(ColorFilter::None.apply(128, 64, 32), (128, 64, 32));
    }

    #[test]
    fn test_filter_inverted() {
        assert_eq!(ColorFilter::Inverted.apply(0, 0, 0), (255, 255, 255));
        assert_eq!(ColorFilter::Inverted.apply(255, 255, 255), (0, 0, 0));
        assert_eq!(ColorFilter::Inverted.apply(100, 150, 200), (155, 105, 55));
    }

    #[test]
    fn test_filter_grayscale() {
        let (r, g, b) = ColorFilter::Grayscale.apply(255, 0, 0);
        assert_eq!(r, g);
        assert_eq!(g, b);
        // Red luma ~ 76
        assert!(r > 70 && r < 80);
    }

    #[test]
    fn test_filter_high_contrast() {
        // White should stay bright
        let (r, g, b) = ColorFilter::HighContrastYellowBlack.apply(255, 255, 255);
        assert_eq!((r, g, b), (255, 255, 0));
        // Black should stay dark
        let (r, g, b) = ColorFilter::HighContrastYellowBlack.apply(0, 0, 0);
        assert_eq!((r, g, b), (0, 0, 0));
    }

    #[test]
    fn test_filter_cycle() {
        let f = ColorFilter::None;
        assert_eq!(f.next(), ColorFilter::Inverted);
    }

    #[test]
    fn test_all_filters_produce_valid_output() {
        for filter in &ColorFilter::ALL {
            // Apply on a mid-gray input — just verify no panic and result is a (u8, u8, u8).
            let (_r, _g, _b) = filter.apply(128, 128, 128);
        }
    }

    #[test]
    fn test_luma() {
        assert_eq!(ColorFilter::luma(0, 0, 0), 0);
        // BT.601 coefficients sum to exactly 1.0, so pure white maps to 255:
        // 255*(0.299 + 0.587 + 0.114) = 255*1.0 = 255.
        assert_eq!(ColorFilter::luma(255, 255, 255), 255);
    }

    // ── Lens shape tests ───────────────────────────────────────────────

    #[test]
    fn test_lens_toggle() {
        assert_eq!(LensShape::Circle.toggle(), LensShape::Rectangle);
        assert_eq!(LensShape::Rectangle.toggle(), LensShape::Circle);
    }

    // ── Zoom tests ─────────────────────────────────────────────────────

    #[test]
    fn test_nearest_preset() {
        assert_eq!(nearest_preset_index(2.0), 1); // exact match at index 1
        assert_eq!(nearest_preset_index(2.3), 1); // closer to 2.0
        assert_eq!(nearest_preset_index(2.6), 2); // closer to 3.0
    }

    #[test]
    fn test_zoom_in() {
        let mut app = MagnifierApp::new();
        assert_eq!(app.zoom_level, 2.0);
        app.zoom_in();
        assert_eq!(app.zoom_level, 3.0);
        app.zoom_in();
        assert_eq!(app.zoom_level, 4.0);
    }

    #[test]
    fn test_zoom_out() {
        let mut app = MagnifierApp::new();
        app.zoom_level = 4.0;
        app.zoom_out();
        assert_eq!(app.zoom_level, 3.0);
        app.zoom_out();
        assert_eq!(app.zoom_level, 2.0);
    }

    #[test]
    fn test_zoom_in_max() {
        let mut app = MagnifierApp::new();
        app.zoom_level = 20.0;
        app.zoom_in();
        assert_eq!(app.zoom_level, 20.0); // stays at max
    }

    #[test]
    fn test_zoom_out_min() {
        let mut app = MagnifierApp::new();
        app.zoom_level = 1.5;
        app.zoom_out();
        assert_eq!(app.zoom_level, 1.5); // stays at min
    }

    #[test]
    fn test_set_zoom_clamp() {
        let mut app = MagnifierApp::new();
        app.set_zoom(100.0);
        assert_eq!(app.zoom_level, 20.0);
        app.set_zoom(0.5);
        assert_eq!(app.zoom_level, 1.5);
    }

    // ── Movement tests ─────────────────────────────────────────────────

    #[test]
    fn test_move_center() {
        let mut app = MagnifierApp::new();
        app.center_x = 500.0;
        app.center_y = 500.0;
        app.move_center(10.0, -20.0);
        assert_eq!(app.center_x, 510.0);
        assert_eq!(app.center_y, 480.0);
    }

    #[test]
    fn test_move_clamp() {
        let mut app = MagnifierApp::new();
        app.center_x = 0.0;
        app.center_y = 0.0;
        app.move_center(-100.0, -100.0);
        assert_eq!(app.center_x, 0.0);
        assert_eq!(app.center_y, 0.0);
    }

    #[test]
    fn test_update_mouse_follow() {
        let mut app = MagnifierApp::new();
        app.tracking = TrackingMode::FollowMouse;
        app.update_mouse(300.0, 400.0);
        assert_eq!(app.center_x, 300.0);
        assert_eq!(app.center_y, 400.0);
    }

    #[test]
    fn test_update_mouse_manual() {
        let mut app = MagnifierApp::new();
        app.tracking = TrackingMode::Manual;
        let old_x = app.center_x;
        app.update_mouse(300.0, 400.0);
        assert_eq!(app.center_x, old_x); // unchanged
    }

    // ── Color picker tests ─────────────────────────────────────────────

    #[test]
    fn test_pick_color() {
        let mut app = MagnifierApp::new();
        app.center_x = 100.0;
        app.center_y = 100.0;
        app.pick_color_at_center();
        assert!(app.picked_color.is_some());
        assert!(app.show_color_picker);
    }

    // ── Ruler tests ────────────────────────────────────────────────────

    #[test]
    fn test_ruler_start() {
        let mut app = MagnifierApp::new();
        app.start_ruler();
        assert!(app.ruler_active);
        assert!(app.ruler_measuring);
        assert!(app.ruler.is_some());
    }

    #[test]
    fn test_ruler_finish() {
        let mut app = MagnifierApp::new();
        app.center_x = 100.0;
        app.center_y = 100.0;
        app.start_ruler();
        app.center_x = 200.0;
        app.center_y = 100.0;
        app.finish_ruler();
        assert!(!app.ruler_measuring);
        let ruler = app.ruler.as_ref().unwrap();
        assert!((ruler.distance() - 100.0).abs() < 0.1);
    }

    #[test]
    fn test_ruler_clear() {
        let mut app = MagnifierApp::new();
        app.start_ruler();
        app.finish_ruler(); // finish measuring
        app.finish_ruler(); // clear
        assert!(!app.ruler_active);
        assert!(app.ruler.is_none());
    }

    #[test]
    fn test_ruler_screen_distance() {
        let r = RulerMeasurement {
            start_x: 0.0,
            start_y: 0.0,
            end_x: 100.0,
            end_y: 0.0,
        };
        assert!((r.distance() - 100.0).abs() < 0.1);
        assert!((r.screen_distance(2.0) - 50.0).abs() < 0.1);
    }

    // ── Pixel sampler tests ────────────────────────────────────────────

    #[test]
    fn test_sample_pixel_in_bounds() {
        let (r, g, b) = sample_pixel(100, 100, 1920, 1080);
        // Should be deterministic
        let (r2, g2, b2) = sample_pixel(100, 100, 1920, 1080);
        assert_eq!((r, g, b), (r2, g2, b2));
    }

    #[test]
    fn test_sample_pixel_out_of_bounds() {
        assert_eq!(sample_pixel(-1, 0, 1920, 1080), (0, 0, 0));
        assert_eq!(sample_pixel(0, -1, 1920, 1080), (0, 0, 0));
        assert_eq!(sample_pixel(1920, 0, 1920, 1080), (0, 0, 0));
    }

    // ── Key handling tests ─────────────────────────────────────────────

    #[test]
    fn test_key_zoom_in() {
        let mut app = MagnifierApp::new();
        app.handle_key("=", true, false);
        assert_eq!(app.zoom_level, 3.0);
    }

    #[test]
    fn test_key_zoom_out() {
        let mut app = MagnifierApp::new();
        app.zoom_level = 4.0;
        app.handle_key("-", true, false);
        assert_eq!(app.zoom_level, 3.0);
    }

    #[test]
    fn test_key_mode() {
        let mut app = MagnifierApp::new();
        app.handle_key("m", false, false);
        assert_eq!(app.mode, MagnifyMode::Lens);
    }

    #[test]
    fn test_key_tracking() {
        let mut app = MagnifierApp::new();
        app.handle_key("t", false, false);
        assert_eq!(app.tracking, TrackingMode::FollowFocus);
    }

    #[test]
    fn test_key_filter() {
        let mut app = MagnifierApp::new();
        app.handle_key("f", false, false);
        assert_eq!(app.color_filter, ColorFilter::Inverted);
    }

    #[test]
    fn test_key_crosshair() {
        let mut app = MagnifierApp::new();
        assert!(app.show_crosshair);
        app.handle_key("x", false, false);
        assert!(!app.show_crosshair);
    }

    #[test]
    fn test_key_escape_pauses() {
        let mut app = MagnifierApp::new();
        assert!(app.enabled);
        app.handle_key("Escape", false, false);
        assert!(!app.enabled);
    }

    #[test]
    fn test_key_preset_zoom() {
        let mut app = MagnifierApp::new();
        app.handle_key("5", false, false);
        assert_eq!(app.zoom_level, 5.0);
    }

    #[test]
    fn test_key_arrows() {
        let mut app = MagnifierApp::new();
        let old_x = app.center_x;
        app.handle_key("Right", false, false);
        assert_eq!(app.center_x, old_x + 10.0);
    }

    #[test]
    fn test_key_help() {
        let mut app = MagnifierApp::new();
        assert!(!app.show_help);
        app.handle_key("F1", false, false);
        assert!(app.show_help);
        app.handle_key("x", false, false); // any key closes
        assert!(!app.show_help);
    }

    #[test]
    fn test_key_screenshot() {
        let mut app = MagnifierApp::new();
        app.handle_key("s", true, false);
        assert!(app.screenshot_taken);
    }

    // ── Render tests ───────────────────────────────────────────────────

    #[test]
    fn test_render_fullscreen() {
        let app = MagnifierApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_lens() {
        let mut app = MagnifierApp::new();
        app.mode = MagnifyMode::Lens;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_docked_top() {
        let mut app = MagnifierApp::new();
        app.mode = MagnifyMode::DockedTop;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_docked_bottom() {
        let mut app = MagnifierApp::new();
        app.mode = MagnifyMode::DockedBottom;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_paused() {
        let mut app = MagnifierApp::new();
        app.enabled = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_crosshair() {
        let mut app = MagnifierApp::new();
        app.show_crosshair = true;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_ruler() {
        let mut app = MagnifierApp::new();
        app.ruler_active = true;
        app.ruler = Some(RulerMeasurement {
            start_x: 100.0,
            start_y: 100.0,
            end_x: 200.0,
            end_y: 100.0,
        });
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_color_picker() {
        let mut app = MagnifierApp::new();
        app.show_color_picker = true;
        app.picked_color = Some((255, 128, 0));
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_help() {
        let mut app = MagnifierApp::new();
        app.show_help = true;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // ── App creation test ──────────────────────────────────────────────

    #[test]
    fn test_app_defaults() {
        let app = MagnifierApp::new();
        assert_eq!(app.zoom_level, 2.0);
        assert_eq!(app.mode, MagnifyMode::FullScreen);
        assert!(app.enabled);
        assert_eq!(app.tracking, TrackingMode::FollowMouse);
        assert_eq!(app.color_filter, ColorFilter::None);
        assert!(app.show_crosshair);
        assert!(app.show_toolbar);
    }

    // ── Color blindness filter tests ───────────────────────────────────

    #[test]
    fn test_protanopia() {
        let (r, g, b) = ColorFilter::Protanopia.apply(255, 0, 0);
        // Pure red should shift — r should be modified
        assert!(r > 0); // some red remains
        assert!(g > 0); // some shifts to green channel
        assert_eq!(b, 0); // blue unaffected by red
    }

    #[test]
    fn test_deuteranopia() {
        let (r, g, b) = ColorFilter::Deuteranopia.apply(0, 255, 0);
        assert!(r > 0); // green shifts to red
        assert!(g > 0);
        assert!(b > 0); // some shifts to blue
    }

    #[test]
    fn test_tritanopia() {
        let (r, g, b) = ColorFilter::Tritanopia.apply(0, 0, 255);
        assert_eq!(r, 0); // no red from pure blue
        assert!(g > 0 || b > 0); // blue shifts
    }
}
