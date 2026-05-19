//! Mandelbrot set explorer for OurOS.
//!
//! Features:
//! - Real-time Mandelbrot set rendering
//! - Zoom in/out with +/- or scroll
//! - Pan with arrow keys
//! - Click to center view
//! - Adjustable iteration count
//! - Multiple color schemes
//! - Coordinate display
//! - Reset view (R key)
//! - Catppuccin Mocha theme for UI

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
#![allow(clippy::needless_range_loop)]
#![allow(unused_imports)]

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha ────────────────────────────────────────────────
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Color schemes ───────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorScheme {
    Classic,
    Fire,
    Ocean,
    Neon,
    Grayscale,
}

impl ColorScheme {
    const ALL: &[ColorScheme] = &[
        ColorScheme::Classic,
        ColorScheme::Fire,
        ColorScheme::Ocean,
        ColorScheme::Neon,
        ColorScheme::Grayscale,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Fire => "Fire",
            Self::Ocean => "Ocean",
            Self::Neon => "Neon",
            Self::Grayscale => "Grayscale",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Classic => Self::Fire,
            Self::Fire => Self::Ocean,
            Self::Ocean => Self::Neon,
            Self::Neon => Self::Grayscale,
            Self::Grayscale => Self::Classic,
        }
    }

    /// Map iteration count to a color
    fn color(self, iter: u32, max_iter: u32) -> Color {
        if iter >= max_iter {
            return Color::from_hex(0x000000); // Black for points in the set
        }

        let t = iter as f64 / max_iter as f64;

        match self {
            Self::Classic => {
                // HSV-based rainbow
                let hue = (t * 360.0) % 360.0;
                hsv_to_color(hue, 1.0, 1.0)
            }
            Self::Fire => {
                let r = (t * 3.0).min(1.0);
                let g = ((t - 0.33).max(0.0) * 3.0).min(1.0);
                let b = ((t - 0.66).max(0.0) * 3.0).min(1.0);
                Color::rgb(
                    (r * 255.0) as u8,
                    (g * 255.0) as u8,
                    (b * 255.0) as u8,
                )
            }
            Self::Ocean => {
                let r = ((t * 2.0 - 0.5).max(0.0).min(1.0) * 100.0) as u8;
                let g = ((t * 1.5).min(1.0) * 200.0) as u8;
                let b = (t.min(1.0) * 255.0) as u8;
                Color::rgb(r, g, b)
            }
            Self::Neon => {
                let phase = t * 6.283;
                let r = ((phase.sin() * 0.5 + 0.5) * 255.0) as u8;
                let g = (((phase + 2.094).sin() * 0.5 + 0.5) * 255.0) as u8;
                let b = (((phase + 4.189).sin() * 0.5 + 0.5) * 255.0) as u8;
                Color::rgb(r, g, b)
            }
            Self::Grayscale => {
                let v = (t * 255.0) as u8;
                Color::rgb(v, v, v)
            }
        }
    }
}

fn hsv_to_color(h: f64, s: f64, v: f64) -> Color {
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    Color::rgb(
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

// ── Mandelbrot computation ──────────────────────────────────────────
/// Compute escape iteration for point (cx, cy) in the complex plane.
/// Returns the iteration at which |z| > 2, or max_iter if it doesn't escape.
fn mandelbrot_iter(cx: f64, cy: f64, max_iter: u32) -> u32 {
    let mut zx = 0.0_f64;
    let mut zy = 0.0_f64;
    let mut i = 0u32;

    while i < max_iter {
        let zx2 = zx * zx;
        let zy2 = zy * zy;
        if zx2 + zy2 > 4.0 {
            return i;
        }
        zy = 2.0 * zx * zy + cy;
        zx = zx2 - zy2 + cx;
        i += 1;
    }
    max_iter
}

// ── App ─────────────────────────────────────────────────────────────
struct MandelbrotApp {
    // View parameters
    center_x: f64,
    center_y: f64,
    scale: f64, // Width in complex plane units
    max_iter: u32,
    color_scheme: ColorScheme,
    // Rendering resolution (pixels per fractal cell)
    pixel_size: f32,
    // UI state
    show_info: bool,
    show_help: bool,
}

impl MandelbrotApp {
    fn new() -> Self {
        Self {
            center_x: -0.5,
            center_y: 0.0,
            scale: 3.5,
            max_iter: 100,
            color_scheme: ColorScheme::Classic,
            pixel_size: 4.0,
            show_info: true,
            show_help: false,
        }
    }

    fn reset_view(&mut self) {
        self.center_x = -0.5;
        self.center_y = 0.0;
        self.scale = 3.5;
        self.max_iter = 100;
    }

    fn zoom_in(&mut self) {
        self.scale *= 0.7;
    }

    fn zoom_out(&mut self) {
        self.scale /= 0.7;
    }

    fn pan(&mut self, dx: f64, dy: f64) {
        self.center_x += dx * self.scale * 0.1;
        self.center_y += dy * self.scale * 0.1;
    }

    fn increase_iterations(&mut self) {
        self.max_iter = (self.max_iter + 50).min(2000);
    }

    fn decrease_iterations(&mut self) {
        self.max_iter = self.max_iter.saturating_sub(50).max(25);
    }

    fn increase_resolution(&mut self) {
        if self.pixel_size > 1.0 {
            self.pixel_size -= 1.0;
        }
    }

    fn decrease_resolution(&mut self) {
        if self.pixel_size < 16.0 {
            self.pixel_size += 1.0;
        }
    }

    fn screen_to_complex(&self, sx: f32, sy: f32, width: f32, height: f32) -> (f64, f64) {
        let aspect = width as f64 / height as f64;
        let cx = self.center_x + (sx as f64 / width as f64 - 0.5) * self.scale * aspect;
        let cy = self.center_y + (sy as f64 / height as f64 - 0.5) * self.scale;
        (cx, cy)
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                if modifiers.ctrl {
                    return;
                }
                match key {
                    Key::Up => self.pan(0.0, -1.0),
                    Key::Down => self.pan(0.0, 1.0),
                    Key::Left => self.pan(-1.0, 0.0),
                    Key::Right => self.pan(1.0, 0.0),
                    Key::Z => self.zoom_in(),
                    Key::X => self.zoom_out(),
                    Key::R => self.reset_view(),
                    Key::C => self.color_scheme = self.color_scheme.next(),
                    Key::I => self.show_info = !self.show_info,
                    Key::F1 => self.show_help = !self.show_help,
                    Key::Num1 => self.increase_iterations(),
                    Key::Num2 => self.decrease_iterations(),
                    Key::Num3 => self.increase_resolution(),
                    Key::Num4 => self.decrease_resolution(),
                    // Preset locations
                    Key::F2 => {
                        // Seahorse valley
                        self.center_x = -0.745;
                        self.center_y = 0.186;
                        self.scale = 0.01;
                        self.max_iter = 300;
                    }
                    Key::F3 => {
                        // Elephant valley
                        self.center_x = 0.281717;
                        self.center_y = 0.5771;
                        self.scale = 0.005;
                        self.max_iter = 300;
                    }
                    Key::F4 => {
                        // Mini Mandelbrot
                        self.center_x = -1.7497;
                        self.center_y = 0.0;
                        self.scale = 0.02;
                        self.max_iter = 500;
                    }
                    _ => {}
                }
            }
            Event::Mouse(MouseEvent { kind: MouseEventKind::Press(MouseButton::Left), x, y, .. }) => {
                // Center on clicked point
                // We need screen dimensions; approximate from the render call
                let (cx, cy) = self.screen_to_complex(*x, *y, 800.0, 600.0);
                self.center_x = cx;
                self.center_y = cy;
                self.zoom_in();
            }
            _ => {}
        }
    }

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width, height,
            color: Color::from_hex(0x000000),
            corner_radii: CornerRadii::ZERO,
        });

        // Render Mandelbrot fractal
        let ps = self.pixel_size;
        let aspect = width as f64 / height as f64;
        let cols = (width / ps) as usize;
        let rows = (height / ps) as usize;

        for py in 0..rows {
            for px in 0..cols {
                let sx = px as f64 / cols as f64;
                let sy = py as f64 / rows as f64;
                let cx = self.center_x + (sx - 0.5) * self.scale * aspect;
                let cy = self.center_y + (sy - 0.5) * self.scale;

                let iter = mandelbrot_iter(cx, cy, self.max_iter);
                let color = self.color_scheme.color(iter, self.max_iter);

                cmds.push(RenderCommand::FillRect {
                    x: px as f32 * ps,
                    y: py as f32 * ps,
                    width: ps,
                    height: ps,
                    color,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Info overlay
        if self.show_info {
            // Semi-transparent background for info
            cmds.push(RenderCommand::FillRect {
                x: 0.0, y: 0.0, width, height: 26.0,
                color: Color::rgba(0, 0, 0, 180),
                corner_radii: CornerRadii::ZERO,
            });

            cmds.push(RenderCommand::Text {
                x: 8.0, y: 5.0,
                text: format!(
                    "Center: ({:.6}, {:.6})  Scale: {:.2e}  Iter: {}  Scheme: {}  Res: {:.0}px",
                    self.center_x, self.center_y, self.scale,
                    self.max_iter, self.color_scheme.name(), self.pixel_size
                ),
                font_size: 12.0,
                color: COL_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Bottom help bar
            cmds.push(RenderCommand::FillRect {
                x: 0.0, y: height - 22.0, width, height: 22.0,
                color: Color::rgba(0, 0, 0, 180),
                corner_radii: CornerRadii::ZERO,
            });

            cmds.push(RenderCommand::Text {
                x: 8.0, y: height - 18.0,
                text: "Arrows=Pan  Z/X=Zoom  C=Color  R=Reset  1/2=Iter  3/4=Res  F2-F4=Presets  Click=Center+Zoom  F1=Help".to_string(),
                font_size: 10.0,
                color: COL_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Help overlay
        if self.show_help {
            self.render_help(&mut cmds, width, height);
        }

        cmds
    }

    fn render_help(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width, height,
            color: Color::rgba(0, 0, 0, 200),
            corner_radii: CornerRadii::ZERO,
        });

        let bx = width / 2.0 - 180.0;
        let by = height / 2.0 - 160.0;
        let bw = 360.0;
        let bh = 320.0;

        cmds.push(RenderCommand::FillRect {
            x: bx, y: by, width: bw, height: bh,
            color: COL_MANTLE,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: bx + bw / 2.0 - 80.0, y: by + 16.0,
            text: "Mandelbrot Explorer".to_string(),
            font_size: 18.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let helps = [
            ("Arrow Keys", "Pan view"),
            ("Z", "Zoom in"),
            ("X", "Zoom out"),
            ("R", "Reset view"),
            ("C", "Cycle color scheme"),
            ("1", "More iterations"),
            ("2", "Fewer iterations"),
            ("3", "Higher resolution"),
            ("4", "Lower resolution"),
            ("I", "Toggle info bar"),
            ("F2", "Seahorse Valley"),
            ("F3", "Elephant Valley"),
            ("F4", "Mini Mandelbrot"),
            ("Click", "Center and zoom"),
        ];

        let mut cy = by + 50.0;
        for (key, desc) in &helps {
            cmds.push(RenderCommand::Text {
                x: bx + 24.0, y: cy,
                text: (*key).to_string(),
                font_size: 12.0,
                color: COL_BLUE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: bx + 140.0, y: cy,
                text: (*desc).to_string(),
                font_size: 12.0,
                color: COL_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cy += 18.0;
        }
    }
}

fn main() {
    let _app = MandelbrotApp::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Mandelbrot iteration tests ──

    #[test]
    fn test_mandelbrot_origin() {
        // Origin (0,0) is in the set
        assert_eq!(mandelbrot_iter(0.0, 0.0, 100), 100);
    }

    #[test]
    fn test_mandelbrot_in_set() {
        // (-1, 0) is in the set (period-2 cycle)
        assert_eq!(mandelbrot_iter(-1.0, 0.0, 100), 100);
    }

    #[test]
    fn test_mandelbrot_outside() {
        // (2, 2) escapes immediately
        let iter = mandelbrot_iter(2.0, 2.0, 100);
        assert!(iter < 5);
    }

    #[test]
    fn test_mandelbrot_boundary() {
        // Near the boundary, should take many iterations
        let iter = mandelbrot_iter(-0.75, 0.01, 1000);
        assert!(iter > 10);
    }

    #[test]
    fn test_mandelbrot_cardioid() {
        // Center of main cardioid
        assert_eq!(mandelbrot_iter(-0.25, 0.0, 100), 100);
    }

    #[test]
    fn test_mandelbrot_far_point() {
        // Far from set, escapes in 1 iteration
        let iter = mandelbrot_iter(10.0, 10.0, 100);
        assert_eq!(iter, 0);
    }

    #[test]
    fn test_mandelbrot_symmetry() {
        // Set is symmetric about x-axis
        let i1 = mandelbrot_iter(-0.5, 0.5, 200);
        let i2 = mandelbrot_iter(-0.5, -0.5, 200);
        assert_eq!(i1, i2);
    }

    // ── Color scheme tests ──

    #[test]
    fn test_color_in_set_black() {
        for scheme in ColorScheme::ALL {
            let c = scheme.color(100, 100);
            assert_eq!(c, Color::from_hex(0x000000));
        }
    }

    #[test]
    fn test_color_outside_not_black() {
        for scheme in ColorScheme::ALL {
            let c = scheme.color(50, 100);
            // Should not be pure black
            assert_ne!(c, Color::from_hex(0x000000));
        }
    }

    #[test]
    fn test_color_scheme_cycle() {
        let mut cs = ColorScheme::Classic;
        for _ in 0..ColorScheme::ALL.len() {
            cs = cs.next();
        }
        assert_eq!(cs, ColorScheme::Classic);
    }

    #[test]
    fn test_color_scheme_names() {
        for cs in ColorScheme::ALL {
            assert!(!cs.name().is_empty());
        }
    }

    #[test]
    fn test_hsv_to_color_red() {
        let c = hsv_to_color(0.0, 1.0, 1.0);
        assert_eq!(c, Color::rgb(255, 0, 0));
    }

    #[test]
    fn test_hsv_to_color_green() {
        let c = hsv_to_color(120.0, 1.0, 1.0);
        assert_eq!(c, Color::rgb(0, 255, 0));
    }

    #[test]
    fn test_hsv_to_color_blue() {
        let c = hsv_to_color(240.0, 1.0, 1.0);
        assert_eq!(c, Color::rgb(0, 0, 255));
    }

    #[test]
    fn test_hsv_to_color_white() {
        let c = hsv_to_color(0.0, 0.0, 1.0);
        assert_eq!(c, Color::rgb(255, 255, 255));
    }

    #[test]
    fn test_hsv_to_color_black() {
        let c = hsv_to_color(0.0, 0.0, 0.0);
        assert_eq!(c, Color::rgb(0, 0, 0));
    }

    // ── App tests ──

    #[test]
    fn test_app_new() {
        let app = MandelbrotApp::new();
        assert_eq!(app.center_x, -0.5);
        assert_eq!(app.center_y, 0.0);
        assert_eq!(app.max_iter, 100);
    }

    #[test]
    fn test_reset_view() {
        let mut app = MandelbrotApp::new();
        app.center_x = 1.0;
        app.center_y = 1.0;
        app.scale = 0.01;
        app.reset_view();
        assert_eq!(app.center_x, -0.5);
        assert_eq!(app.center_y, 0.0);
        assert_eq!(app.scale, 3.5);
    }

    #[test]
    fn test_zoom_in() {
        let mut app = MandelbrotApp::new();
        let old_scale = app.scale;
        app.zoom_in();
        assert!(app.scale < old_scale);
    }

    #[test]
    fn test_zoom_out() {
        let mut app = MandelbrotApp::new();
        let old_scale = app.scale;
        app.zoom_out();
        assert!(app.scale > old_scale);
    }

    #[test]
    fn test_pan() {
        let mut app = MandelbrotApp::new();
        let old_x = app.center_x;
        app.pan(1.0, 0.0);
        assert!(app.center_x > old_x);
    }

    #[test]
    fn test_increase_iterations() {
        let mut app = MandelbrotApp::new();
        app.increase_iterations();
        assert_eq!(app.max_iter, 150);
    }

    #[test]
    fn test_decrease_iterations() {
        let mut app = MandelbrotApp::new();
        app.decrease_iterations();
        assert_eq!(app.max_iter, 50);
    }

    #[test]
    fn test_decrease_iterations_floor() {
        let mut app = MandelbrotApp::new();
        app.max_iter = 25;
        app.decrease_iterations();
        assert_eq!(app.max_iter, 25);
    }

    #[test]
    fn test_increase_iterations_cap() {
        let mut app = MandelbrotApp::new();
        app.max_iter = 2000;
        app.increase_iterations();
        assert_eq!(app.max_iter, 2000);
    }

    #[test]
    fn test_increase_resolution() {
        let mut app = MandelbrotApp::new();
        app.pixel_size = 4.0;
        app.increase_resolution();
        assert_eq!(app.pixel_size, 3.0);
    }

    #[test]
    fn test_increase_resolution_floor() {
        let mut app = MandelbrotApp::new();
        app.pixel_size = 1.0;
        app.increase_resolution();
        assert_eq!(app.pixel_size, 1.0);
    }

    #[test]
    fn test_decrease_resolution() {
        let mut app = MandelbrotApp::new();
        app.pixel_size = 4.0;
        app.decrease_resolution();
        assert_eq!(app.pixel_size, 5.0);
    }

    #[test]
    fn test_decrease_resolution_cap() {
        let mut app = MandelbrotApp::new();
        app.pixel_size = 16.0;
        app.decrease_resolution();
        assert_eq!(app.pixel_size, 16.0);
    }

    #[test]
    fn test_screen_to_complex() {
        let app = MandelbrotApp::new();
        let (cx, cy) = app.screen_to_complex(400.0, 300.0, 800.0, 600.0);
        // Center of screen should map to center_x, center_y
        assert!((cx - app.center_x).abs() < 0.01);
        assert!((cy - app.center_y).abs() < 0.01);
    }

    #[test]
    fn test_arrow_key_pan() {
        let mut app = MandelbrotApp::new();
        let old_y = app.center_y;
        app.event(&Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.center_y < old_y);
    }

    #[test]
    fn test_z_key_zoom_in() {
        let mut app = MandelbrotApp::new();
        let old_scale = app.scale;
        app.event(&Event::Key(KeyEvent {
            key: Key::Z,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.scale < old_scale);
    }

    #[test]
    fn test_x_key_zoom_out() {
        let mut app = MandelbrotApp::new();
        let old_scale = app.scale;
        app.event(&Event::Key(KeyEvent {
            key: Key::X,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.scale > old_scale);
    }

    #[test]
    fn test_r_key_reset() {
        let mut app = MandelbrotApp::new();
        app.center_x = 1.0;
        app.event(&Event::Key(KeyEvent {
            key: Key::R,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.center_x, -0.5);
    }

    #[test]
    fn test_c_key_color_scheme() {
        let mut app = MandelbrotApp::new();
        assert_eq!(app.color_scheme, ColorScheme::Classic);
        app.event(&Event::Key(KeyEvent {
            key: Key::C,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.color_scheme, ColorScheme::Fire);
    }

    #[test]
    fn test_i_key_toggles_info() {
        let mut app = MandelbrotApp::new();
        assert!(app.show_info);
        app.event(&Event::Key(KeyEvent {
            key: Key::I,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(!app.show_info);
    }

    #[test]
    fn test_f1_toggles_help() {
        let mut app = MandelbrotApp::new();
        assert!(!app.show_help);
        app.event(&Event::Key(KeyEvent {
            key: Key::F1,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.show_help);
    }

    #[test]
    fn test_f2_preset() {
        let mut app = MandelbrotApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::F2,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!((app.center_x - (-0.745)).abs() < 0.001);
    }

    #[test]
    fn test_f3_preset() {
        let mut app = MandelbrotApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::F3,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!((app.center_x - 0.281717).abs() < 0.001);
    }

    #[test]
    fn test_f4_preset() {
        let mut app = MandelbrotApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::F4,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!((app.center_x - (-1.7497)).abs() < 0.001);
    }

    #[test]
    fn test_num1_iterations() {
        let mut app = MandelbrotApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::Num1,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.max_iter, 150);
    }

    #[test]
    fn test_num2_iterations() {
        let mut app = MandelbrotApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::Num2,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.max_iter, 50);
    }

    #[test]
    fn test_ctrl_ignored() {
        let mut app = MandelbrotApp::new();
        let old_x = app.center_x;
        app.event(&Event::Key(KeyEvent {
            key: Key::R,
            modifiers: Modifiers { ctrl: true, ..Modifiers::default() },
            pressed: true,
            text: None,
        }));
        assert_eq!(app.center_x, old_x);
    }

    #[test]
    fn test_render_no_panic() {
        let mut app = MandelbrotApp::new();
        app.pixel_size = 16.0; // Low res for fast test
        let cmds = app.render(160.0, 120.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_help_no_panic() {
        let mut app = MandelbrotApp::new();
        app.pixel_size = 16.0;
        app.show_help = true;
        let cmds = app.render(160.0, 120.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_no_info() {
        let mut app = MandelbrotApp::new();
        app.pixel_size = 16.0;
        app.show_info = false;
        let cmds = app.render(160.0, 120.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_main_no_panic() {
        main();
    }

    #[test]
    fn test_fire_color_gradient() {
        // First quarter should be reddish
        let c = ColorScheme::Fire.color(10, 100);
        assert_ne!(c, Color::from_hex(0x000000));
    }

    #[test]
    fn test_ocean_color_gradient() {
        let c = ColorScheme::Ocean.color(50, 100);
        assert_ne!(c, Color::from_hex(0x000000));
    }

    #[test]
    fn test_neon_color_gradient() {
        let c = ColorScheme::Neon.color(25, 100);
        assert_ne!(c, Color::from_hex(0x000000));
    }

    #[test]
    fn test_grayscale_gradient() {
        let c = ColorScheme::Grayscale.color(50, 100);
        // Should be mid-gray
        assert_ne!(c, Color::from_hex(0x000000));
        assert_ne!(c, Color::from_hex(0xFFFFFF));
    }

    #[test]
    fn test_click_centers_and_zooms() {
        let mut app = MandelbrotApp::new();
        let old_scale = app.scale;
        app.event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Press(MouseButton::Left),
            x: 400.0,
            y: 300.0,
            modifiers: Modifiers::default(),
        }));
        assert!(app.scale < old_scale);
    }
}
