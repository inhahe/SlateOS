//! Light auto-hinting: glyph outlines nudged vertically onto the pixel grid.
//!
//! An outline is drawn exactly where its designer put it, which at text sizes
//! means the top of an `x` lands a third of the way into a pixel row and the
//! crossbar of an `e` straddles two: both rows come out half-grey, and small
//! text looks soft. Hinting moves the outline -- vertically only, which is
//! FreeType's "light" mode (fontconfig's `hintslight`, the default on most
//! Linux desktops) -- so that the heights letters share land on pixel
//! boundaries and every stroke keeps its designed thickness.
//!
//! # A port, not a reimplementation
//!
//! This module is FreeType 2.13.2's auto-hinter (`src/autofit`), ported:
//! the data tables generated from its sources ([`tables`]), the algorithm
//! translated function by function ([`glyph`] from `afhints.c`, [`latin`]
//! from `aflatin.c`), and its fixed-point arithmetic kept to the bit
//! ([`fixed`]). Hinting is a long chain of rounding decisions, and a
//! reimplementation that makes different ones produces different text; this
//! makes the same ones, and so draws what fontconfig-configured Linux
//! desktops draw -- which is the look people compare against -- and can be
//! checked against FreeType itself glyph by glyph (see
//! `gui/font/tools/hint_oracle.py`). The alternative, running each font's
//! own TrueType instructions, is a bytecode interpreter executing
//! attacker-supplied programs (design-decisions §86c) and does nothing for
//! CFF fonts or the many fonts shipped without instructions.
//!
//! # What happens, in order
//!
//! 1. **Every glyph gets a style** ([`FaceHints::new`]): the script whose
//!    characters reach it through the `cmap`, the earliest of FreeType's
//!    styles winning a tie; then the glyphs only `GSUB` reaches -- ligatures,
//!    positional forms -- the script whose features produce them; then the
//!    rest, the fallback style.
//! 2. **Each style in use is measured** ([`latin::Metrics::new`]): its
//!    standard stem width and its blue zones, from its script's reference
//!    letters.
//! 3. **At each size** ([`Hinter::new`]) the zones are fitted to the pixel
//!    grid, after nudging the vertical scale so the x-height lands on a pixel.
//! 4. **Each glyph** ([`Hinter::hint`]) is cut into segments and edges, its
//!    edges placed, and its points moved to follow them.
//!
//! # What is not hinted
//!
//! Only FreeType's Latin writing system is ported, which serves nearly every
//! script -- Latin, Greek, Cyrillic, Arabic, Hebrew, Devanagari and the other
//! Brahmic scripts, Thai, Ethiopic, and some fifty more, each with its own
//! reference letters. Its CJK system (Chinese, Japanese and Korean
//! ideographs, and the fallback style every glyph no script claims goes to)
//! and its Indic stub (four scripts) are not ported: their glyphs are drawn
//! unhinted, exactly as before. See known-issues.md, "Ideographs and the
//! fallback style are drawn unhinted".
//!
//! Portions of this module are copyright (C) 2003-2023 by David Turner,
//! Robert Wilhelm and Werner Lemberg, from The FreeType Project
//! (www.freetype.org). Used under the FreeType License: see
//! `gui/font/licenses/FTL.TXT`.

mod fixed;
mod glyph;
mod latin;
mod tables;

#[cfg(test)]
mod fixture;

use alloc::vec::Vec;

use crate::sfnt::{Exact, Face, Outline, TaggedOutline};
use crate::var::Coords;

use fixed::{MAX_SCALE, div_fix, mul_fix};
use glyph::Hints;
use tables::{RANGES, SCRIPTS, STYLES};

/// One blue zone's reference letters -- clusters separated by spaces, as in
/// FreeType's `afblue.dat` -- and its properties.
pub(super) struct Blue {
    pub(super) chars: &'static str,
    pub(super) props: u8,
}

/// One of FreeType's scripts.
pub(super) struct Script {
    /// FreeType's four-letter name.
    pub(super) name: &'static str,
    /// The OpenType tags its `GSUB` features are filed under.
    pub(super) ot: &'static [[u8; 4]],
    /// Whether its stems are placed from the top down.
    pub(super) top_to_bottom: bool,
    /// The letters its standard stem width is measured from, first found wins.
    pub(super) standard: &'static str,
}

/// Which of FreeType's hinting algorithms a style uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum System {
    /// No hinting.
    Dummy,
    /// The Latin writing system, which is ported.
    Latin,
    /// The CJK writing system, which is not.
    Cjk,
    /// The Indic writing system, which is not.
    Indic,
}

/// One of FreeType's default styles.
pub(super) struct Style {
    /// FreeType's name for it.
    pub(super) name: &'static str,
    /// Index into [`SCRIPTS`].
    pub(super) script: usize,
    pub(super) system: System,
    pub(super) blues: &'static [Blue],
    /// The code points whose glyphs are never snapped to a zone: combining
    /// marks and the like.
    pub(super) nonbase: &'static [(u32, u32)],
}

/// What a cluster of text shapes to: each glyph with its vertical offset in
/// font units.
pub(crate) type Glyphs = Vec<(u16, i32)>;

/// A glyph no style has claimed yet (`AF_STYLE_UNASSIGNED`).
const UNASSIGNED: u8 = 0xFF;

/// The style a code point's glyph belongs to, if its script is one of
/// FreeType's.
fn style_of(cp: u32) -> Option<u8> {
    let i = RANGES.partition_point(|&(_, last, _)| last < cp);
    RANGES
        .get(i)
        .filter(|&&(first, _, _)| first <= cp)
        .map(|&(_, _, style)| style)
}

/// The style every glyph no script claims goes to: FreeType's CJK one
/// (`AF_STYLE_FALLBACK` with `AF_CONFIG_OPTION_CJK`, its default).
fn fallback_style() -> Option<u8> {
    STYLES
        .iter()
        .position(|s| s.name == "hani_dflt")
        .and_then(|i| u8::try_from(i).ok())
}

/// What the hinter knows about one face at one instance, whatever the size:
/// every glyph's style, and each style's measurements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FaceHints {
    /// Per glyph: its style, an index into [`STYLES`].
    styles: Vec<u8>,
    /// Per glyph: whether it is a non-base glyph, never snapped to a zone.
    nonbase: Vec<bool>,
    /// Per style: its Latin metrics, where it is a Latin-system style some
    /// glyph uses and at least one of its zones could be measured.
    metrics: Vec<Option<latin::Metrics>>,
    units_per_em: i64,
    /// How the face's coordinates become whole font units.
    units: glyph::Units,
}

impl FaceHints {
    /// Sort `face`'s glyphs into styles and measure the styles in use, at
    /// `coords` (FreeType's `af_face_globals_compute_style_coverage` and the
    /// metrics initialisation it triggers).
    ///
    /// `shape` shapes a cluster of reference letters the way the face shapes
    /// text; FreeType uses HarfBuzz for the same. A face with no Unicode
    /// `cmap` has every glyph in the fallback style, and is drawn unhinted.
    pub(crate) fn new(face: &Face, coords: &Coords, shape: &dyn Fn(&str) -> Glyphs) -> Self {
        let units_per_em = i64::from(face.units_per_em());
        let count = usize::from(face.num_glyphs());
        let mut styles = alloc::vec![UNASSIGNED; count];
        let mut nonbase = alloc::vec![false; count];

        // 1. The characters that reach each glyph: the earliest style of any
        //    of them wins, which is FreeType's style-by-style,
        //    first-come-first-served order.
        let mut mappings: Vec<(u32, u16)> = Vec::new();
        face.for_each_unicode_mapping(|cp, gid| mappings.push((cp, gid)));
        for &(cp, gid) in &mappings {
            if let (Some(style), Some(slot)) = (style_of(cp), styles.get_mut(usize::from(gid))) {
                *slot = (*slot).min(style);
            }
        }
        // A glyph is non-base if any character reaching it is in its own
        // style's non-base list.
        for &(cp, gid) in &mappings {
            let Some(&style) = styles.get(usize::from(gid)) else {
                continue;
            };
            let listed = STYLES
                .get(usize::from(style))
                .is_some_and(|s| s.nonbase.iter().any(|&(a, b)| (a..=b).contains(&cp)));
            if listed && let Some(slot) = nonbase.get_mut(usize::from(gid)) {
                *slot = true;
            }
        }
        drop(mappings);

        // 2. The glyphs only `GSUB` reaches, for the script whose features
        //    produce them -- and then for Latin again with `DFLT`'s, FreeType's
        //    default script.
        let assign = |styles: &mut Vec<u8>, style: usize, glyphs: Vec<u16>| {
            let Ok(style) = u8::try_from(style) else {
                return;
            };
            for gid in glyphs {
                if let Some(slot) = styles.get_mut(usize::from(gid))
                    && *slot == UNASSIGNED
                {
                    *slot = style;
                }
            }
        };
        for (i, style) in STYLES.iter().enumerate() {
            let Some(script) = SCRIPTS.get(style.script) else {
                continue;
            };
            if !script.ot.is_empty() {
                assign(&mut styles, i, face.gsub_outputs(script.ot));
            }
        }
        if let Some(latin) = STYLES
            .iter()
            .position(|s| SCRIPTS.get(s.script).is_some_and(|sc| sc.name == "latn"))
        {
            assign(&mut styles, latin, face.gsub_outputs(&[*b"latn", *b"DFLT"]));
        }

        // 3. The rest.
        let fallback = fallback_style().unwrap_or(UNASSIGNED);
        for slot in &mut styles {
            if *slot == UNASSIGNED {
                *slot = fallback;
            }
        }

        // Measure every Latin-system style some glyph uses.
        let outline = |gid: u16| face.tagged_outline_at(gid, coords).ok();
        let units = if face.has_cff_outlines() {
            glyph::Units::Floored
        } else {
            glyph::Units::Rounded
        };
        let source = latin::Source {
            shape,
            outline: &outline,
            units,
        };
        let mut used = alloc::vec![false; STYLES.len()];
        for &s in &styles {
            if let Some(u) = used.get_mut(usize::from(s)) {
                *u = true;
            }
        }
        let metrics = STYLES
            .iter()
            .zip(&used)
            .map(|(style, &used)| {
                if !used || style.system != System::Latin {
                    return None;
                }
                let script = SCRIPTS.get(style.script)?;
                latin::Metrics::new(
                    units_per_em,
                    script.standard,
                    style.blues,
                    script.top_to_bottom,
                    &source,
                )
            })
            .collect();
        Self {
            styles,
            nonbase,
            metrics,
            units_per_em,
            units,
        }
    }
}

/// Hinting at one size: the face's measurements fitted to its pixel grid.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hinter {
    face: FaceHints,
    /// Per style, as [`FaceHints::metrics`]: the zones fitted to this size.
    scaled: Vec<Option<latin::Scaled>>,
    /// The size's own scale, 16.16 (FreeType's `x_scale`), for the
    /// horizontal coordinates light hinting leaves alone: only the vertical
    /// one is fitted to a style's x-height.
    x_scale: i64,
}

impl Hinter {
    /// Fit `face` to `px_per_em` pixels per em. `None` for a size beyond the
    /// range the hinter's arithmetic is proved for -- over a thousand pixels a
    /// font unit, where hinting would be meaningless anyway.
    pub(crate) fn new(face: FaceHints, px_per_em: f32) -> Option<Self> {
        let size = (px_per_em * 64.0).round();
        if !size.is_finite() || size < 1.0 || size > 1.0e9 {
            return None;
        }
        // FreeType's `y_scale`: the size in 26.6 over the em, as 16.16.
        let y_scale = div_fix(size as i64, face.units_per_em);
        if !(1..=MAX_SCALE).contains(&y_scale) {
            return None;
        }
        let scaled = face
            .metrics
            .iter()
            .map(|m| m.as_ref().map(|m| m.scale(y_scale)))
            .collect();
        Some(Self {
            x_scale: y_scale,
            face,
            scaled,
        })
    }

    /// `gid` hinted, as a path in *pixels* (y up, origin on the baseline), or
    /// `None` when this glyph is drawn unhinted: its style is not a ported
    /// one, it has no outline, or it is beyond what the hinter takes on.
    pub(crate) fn hint(&self, face: &Face, coords: &Coords, gid: u16) -> Option<Outline> {
        self.hint_points(face, coords, gid)
            .map(|points| points.to_path())
    }

    /// [`hint`](Self::hint)'s answer before it becomes a path: the glyph's
    /// stored points, hinted, in pixels.
    pub(crate) fn hint_points(
        &self,
        face: &Face,
        coords: &Coords,
        gid: u16,
    ) -> Option<TaggedOutline> {
        let style = usize::from(*self.face.styles.get(usize::from(gid))?);
        let metrics = self.face.metrics.get(style)?.as_ref()?;
        let scaled = self.scaled.get(style)?.as_ref()?;
        let nonbase = self
            .face
            .nonbase
            .get(usize::from(gid))
            .copied()
            .unwrap_or(false);
        let outline = face.tagged_outline_at(gid, coords).ok()?;
        let mut hints = Hints::load(
            &outline,
            self.face.units,
            scaled.y_scale,
            self.face.units_per_em,
        )?;
        latin::hint(&mut hints, metrics, scaled, nonbase)?;
        Some(self.to_pixels(&outline, &hints))
    }

    /// FreeType's name for the style `gid` was sorted into, and whether it is
    /// a non-base glyph -- for diagnostics.
    pub(crate) fn style_of_glyph(&self, gid: u16) -> Option<(&'static str, bool)> {
        let style = *self.face.styles.get(usize::from(gid))?;
        let nonbase = self
            .face
            .nonbase
            .get(usize::from(gid))
            .copied()
            .unwrap_or(false);
        STYLES.get(usize::from(style)).map(|s| (s.name, nonbase))
    }

    /// The hinted points in pixels: horizontal positions as the size scales
    /// them, vertical ones as hinting placed them.
    ///
    /// Horizontal positions are scaled as FreeType scales them, from the
    /// whole font units the hinter read (`af_glyph_hints_reload`), not from
    /// the exact coordinates: a CFF glyph's fractional ones are floored
    /// first, which moves a point by up to a unit.
    #[allow(
        clippy::cast_precision_loss,
        reason = "26.6 positions are far inside f64's exact range"
    )]
    fn to_pixels(&self, outline: &TaggedOutline, hints: &Hints) -> TaggedOutline {
        let points = hints
            .points
            .iter()
            .map(|h| Exact::new(mul_fix(h.fx, self.x_scale) as f64 / 64.0, h.y as f64 / 64.0))
            .collect();
        TaggedOutline {
            points,
            tags: outline.tags.clone(),
            ends: outline.ends.clone(),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test fixtures"
)]
mod tests {
    use super::*;

    #[test]
    fn the_range_table_names_the_scripts_freetype_does() {
        let name = |cp: u32| style_of(cp).map(|s| STYLES[usize::from(s)].name);
        assert_eq!(name('x' as u32), Some("latn_dflt"));
        assert_eq!(name('Ж' as u32), Some("cyrl_dflt"));
        assert_eq!(name('λ' as u32), Some("grek_dflt"));
        assert_eq!(name('א' as u32), Some("hebr_dflt"));
        assert_eq!(name('ح' as u32), Some("arab_dflt"));
        assert_eq!(name('क' as u32), Some("deva_dflt"));
        assert_eq!(name('中' as u32), Some("hani_dflt"));
        assert_eq!(name(0x10_FFFF), None);
    }

    #[test]
    fn the_range_table_is_sorted_and_disjoint() {
        for w in RANGES.windows(2) {
            assert!(w[0].1 < w[1].0, "{:X?} then {:X?}", w[0], w[1]);
        }
        for r in RANGES {
            assert!(r.0 <= r.1);
            assert!(usize::from(r.2) < STYLES.len());
        }
    }

    #[test]
    fn every_style_names_a_script_and_latins_zones_are_freetypes() {
        for s in &STYLES {
            assert!(s.script < SCRIPTS.len(), "{}", s.name);
        }
        let latin = STYLES.iter().find(|s| s.name == "latn_dflt").unwrap();
        let chars: Vec<&str> = latin.blues.iter().map(|b| b.chars).collect();
        assert_eq!(
            chars,
            [
                "T H E Z O C Q S",
                "H E Z L O C U S",
                "f i j k d b h",
                "u v x z o e s c",
                "n r x z o e s c",
                "p q g j y"
            ]
        );
        assert_eq!(SCRIPTS[latin.script].standard, "o O 0");
        assert!(fallback_style().is_some());
    }

    /// Every point of every glyph of `font`, hinted at each size in
    /// `expected`, against where FreeType put it.
    fn matches_freetype(font: &[u8], expected: &[(f32, u16, &str, &[i32])]) {
        use crate::raster::Rendering;
        use crate::scaled::ScaledFont;
        let face = alloc::sync::Arc::new(Face::parse(font.to_vec()).unwrap());
        let mut failures = Vec::new();
        let mut current: Option<(f32, ScaledFont)> = None;
        for &(px, gid, name, want) in expected {
            if current
                .as_ref()
                .is_none_or(|(p, _)| p.to_bits() != px.to_bits())
            {
                let mut font = ScaledFont::shared(face.clone(), px).unwrap();
                font.set_rendering(Rendering {
                    hinting: true,
                    ..Rendering::default()
                });
                current = Some((px, font));
            }
            let (_, font) = current.as_mut().unwrap();
            // x and y in turn, as the fixture has them. Both are whole 64ths
            // exactly -- x a `FT_MulFix` of a whole font unit, y a hinted
            // 26.6 position -- so the rounding here only converts.
            let got: Vec<i32> = font
                .hinted_points(gid)
                .unwrap_or_default()
                .iter()
                .flat_map(|&(x, y, _)| [(x * 64.0).round() as i32, (y * 64.0).round() as i32])
                .collect();
            if got != want {
                failures.push(alloc::format!("{px}px {name}: {got:?}, FreeType {want:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} disagree:\n{}",
            failures.len(),
            expected.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn a_truetype_face_is_hinted_exactly_as_freetype_hints_it() {
        matches_freetype(&fixture::TTF, &fixture::TTF_EXPECTED);
    }

    #[test]
    fn a_cff_face_is_hinted_exactly_as_freetype_hints_it() {
        matches_freetype(&fixture::OTF, &fixture::OTF_EXPECTED);
    }

    #[test]
    fn the_fixture_is_sorted_into_the_styles_freetype_uses() {
        use crate::raster::Rendering;
        use crate::scaled::ScaledFont;
        let face = Face::parse(fixture::TTF.to_vec()).unwrap();
        let gid = |ch: char| face.glyph_index(ch).unwrap();
        let (x, e_acute, comb) = (gid('x'), gid('\u{E9}'), gid('\u{301}'));
        let mut font = ScaledFont::new(face, 13.0).unwrap();
        font.set_rendering(Rendering {
            hinting: true,
            ..Rendering::default()
        });
        assert_eq!(font.hint_style(x), Some(("latn_dflt", false)));
        assert_eq!(font.hint_style(e_acute), Some(("latn_dflt", false)));
        // A combining mark is never snapped to a zone.
        assert_eq!(font.hint_style(comb), Some(("latn_dflt", true)));
        // Glyph 0 is reached by no character: the fallback style, whose CJK
        // hinting is not ported, so it is drawn as designed.
        assert_eq!(font.hint_style(0), Some(("hani_dflt", false)));
    }

    #[test]
    fn hinting_off_leaves_every_glyph_as_designed() {
        use crate::scaled::ScaledFont;
        let face = Face::parse(fixture::TTF.to_vec()).unwrap();
        let x = face.glyph_index('x').unwrap();
        let mut font = ScaledFont::new(face, 13.0).unwrap();
        assert_eq!(font.hinted_points(x), None);
        assert_eq!(font.hint_style(x), None);
    }
}
