//! Color types for the GUI toolkit.

/// RGBA color (8 bits per channel).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as u8,
            g: ((hex >> 8) & 0xFF) as u8,
            b: (hex & 0xFF) as u8,
            a: 255,
        }
    }

    /// Blend this color over `below` using alpha compositing.
    pub fn over(self, below: Color) -> Color {
        if self.a == 255 {
            return self;
        }
        if self.a == 0 {
            return below;
        }

        let sa = self.a as u16;
        let da = below.a as u16;
        let inv_sa = 255 - sa;

        let out_a = sa + (da * inv_sa) / 255;
        if out_a == 0 {
            return Color::TRANSPARENT;
        }

        let r = ((self.r as u16 * sa + below.r as u16 * da * inv_sa / 255) / out_a) as u8;
        let g = ((self.g as u16 * sa + below.g as u16 * da * inv_sa / 255) / out_a) as u8;
        let b = ((self.b as u16 * sa + below.b as u16 * da * inv_sa / 255) / out_a) as u8;

        Color::rgba(r, g, b, out_a as u8)
    }

    /// Linear interpolation between two colors.
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let inv_t = 1.0 - t;
        Color::rgba(
            (self.r as f32 * inv_t + other.r as f32 * t) as u8,
            (self.g as f32 * inv_t + other.g as f32 * t) as u8,
            (self.b as f32 * inv_t + other.b as f32 * t) as u8,
            (self.a as f32 * inv_t + other.a as f32 * t) as u8,
        )
    }

    // Common color constants
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(220, 50, 50);
    pub const GREEN: Color = Color::rgb(50, 180, 50);
    pub const BLUE: Color = Color::rgb(50, 100, 220);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    pub const LIGHT_GRAY: Color = Color::rgb(200, 200, 200);
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}
