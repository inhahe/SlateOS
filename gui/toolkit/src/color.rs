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

        // Use u32 for the intermediates: a naive `channel * da * inv_sa` can
        // reach 255*255*255 ≈ 16.6M, which overflows u16 and panics in debug.
        let sa = self.a as u32;
        let da = below.a as u32;
        let inv_sa = 255 - sa;

        // Destination alpha contribution once covered by the source (0..=255).
        let da_contrib = da * inv_sa / 255;
        let out_a = sa + da_contrib;
        if out_a == 0 {
            return Color::TRANSPARENT;
        }

        // Numerator peaks at 255*255 + 255*255 = 130_050, well within u32.
        let blend = |src: u8, dst: u8| -> u8 {
            ((src as u32 * sa + dst as u32 * da_contrib) / out_a) as u8
        };
        let r = blend(self.r, below.r);
        let g = blend(self.g, below.g);
        let b = blend(self.b, below.b);

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
