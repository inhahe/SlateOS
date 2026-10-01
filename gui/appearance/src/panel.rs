//! The taskbar's panel: how much of the Aero reference's glass it wears, and
//! how far apart its tiles are -- what a theme's `taskbar-panel` section sets
//! ([`crate::themes::PanelTheme`]) and what the desktop draws and lays out its
//! taskbar from. Its colours are the palette's, and whether it is see-through
//! is the user's own choice (`taskbar_style` and `transparency`): this is its
//! finish and its spacing. `design-decisions.md` §1460.
//!
//! [`PanelStyle::AERO`] is the built-in theme's -- the bar the desktop has
//! drawn since it followed the reference: all of the glass, tiles a pixel
//! apart, six pixels from the start button to the first tile and nineteen
//! between the pinned programs and the windows. A theme of gloss 0 is the
//! flat bar the roadmap asks a theme to be able to give ("users who want a
//! flat/opaque look can disable them without losing the rest of the default
//! visual identity").

/// How the taskbar's panel is finished, and its tiles spaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelStyle {
    /// How much of the reference's glass the bar and its tiles wear, in
    /// hundredths: 100 is the reference's -- lines of light along the bar's
    /// top, a glow under them and a shade over its lower half, and a sheen
    /// and a top highlight on a window's tile -- and 0 is none of it, a flat
    /// bar of the theme's colour whose tiles are drawn by their edges. In
    /// between, every one of those is drawn at that share of its strength.
    pub gloss: u8,
    /// Between two tiles, in pixels at scale 1.
    pub tile_gap: u16,
    /// From the start button to the first tile.
    pub start_gap: u16,
    /// Between the last pinned program's tile and the first window's, with
    /// the divider in it.
    pub section_gap: u16,
}

impl Default for PanelStyle {
    fn default() -> Self {
        Self::AERO
    }
}

impl PanelStyle {
    /// The built-in theme's panel: the Aero reference's.
    pub const AERO: Self = Self {
        gloss: Self::MAX_GLOSS,
        tile_gap: 1,
        start_gap: 6,
        section_gap: 19,
    };

    /// All of the glass: the reference's strengths.
    pub const MAX_GLOSS: u8 = 100;
    /// The widest gap between two tiles.
    pub const MAX_TILE_GAP: u16 = 24;
    /// The widest gap after the start button.
    pub const MAX_START_GAP: u16 = 48;
    /// The narrowest gap between the sections: the divider and a pixel of
    /// room on each side of it.
    pub const MIN_SECTION_GAP: u16 = 3;
    /// The widest gap between the sections.
    pub const MAX_SECTION_GAP: u16 = 64;

    /// One of the reference's strengths of light or shade -- an alpha --
    /// at this panel's gloss: itself at 100, nothing at 0.
    #[must_use]
    pub fn glossed(self, alpha: u8) -> u8 {
        let share = f32::from(self.gloss.min(Self::MAX_GLOSS)) / f32::from(Self::MAX_GLOSS);
        // In 0..=255: an alpha times a share of at most one, rounded.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an alpha times a share in 0..=1"
        )]
        let glossed = (f32::from(alpha) * share).round() as u8;
        glossed
    }

    /// Where the divider stands in the gap between the sections, from the
    /// gap's start, in pixels at scale 1: eight nineteenths of the way
    /// across, as the reference has it -- its row's gap and seven of margin
    /// before the divider, nine of margin and the gap again after -- at any
    /// width the theme gives the gap.
    #[must_use]
    pub fn divider_offset(self) -> f32 {
        // Multiplied first, so the reference's 19 gives exactly 8.
        f32::from(self.section_gap) * 8.0 / 19.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The built-in panel is the reference's**, which the desktop drew
    /// before there was a style: its gaps, and every strength unchanged.
    #[test]
    fn the_built_in_panel_is_the_references() {
        let aero = PanelStyle::AERO;
        assert_eq!(
            (aero.tile_gap, aero.start_gap, aero.section_gap),
            (1, 6, 19)
        );
        assert_eq!(PanelStyle::default(), aero);
        for alpha in [0, 1, 18, 36, 115, 128, 255] {
            assert_eq!(aero.glossed(alpha), alpha);
        }
        assert!((aero.divider_offset() - 8.0).abs() < f32::EPSILON);
    }

    /// **Gloss scales every strength, and none is left at 0.**
    #[test]
    fn gloss_scales_the_glass() {
        let flat = PanelStyle {
            gloss: 0,
            ..PanelStyle::AERO
        };
        let half = PanelStyle {
            gloss: 50,
            ..PanelStyle::AERO
        };
        for alpha in [1, 115, 255] {
            assert_eq!(flat.glossed(alpha), 0);
        }
        assert_eq!(half.glossed(128), 64);
        assert_eq!(half.glossed(115), 58);
        // Past 100 is 100.
        let over = PanelStyle {
            gloss: 250,
            ..PanelStyle::AERO
        };
        assert_eq!(over.glossed(115), 115);
    }

    /// **The divider keeps the reference's place in a gap of any width.**
    #[test]
    fn the_divider_keeps_its_place() {
        let gap = |section_gap| PanelStyle {
            section_gap,
            ..PanelStyle::AERO
        };
        assert!((gap(38).divider_offset() - 16.0).abs() < 1e-4);
        assert!((gap(3).divider_offset() - 24.0 / 19.0).abs() < 1e-4);
    }
}
