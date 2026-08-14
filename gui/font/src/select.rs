//! Choosing which face of a family to draw with.
//!
//! # Why this is not "pick the closest weight"
//!
//! A family rarely ships exactly the weights that are asked for. This host
//! has faces at 250, 290, 350 and 450 alongside the usual 400 and 700, and
//! plenty of families ship only two of the nine steps. So a selector always
//! ends up substituting, and *which* substitution it makes is visible: asking
//! for regular text and getting a Light face makes a whole UI look washed
//! out, while getting a Medium one barely reads as different.
//!
//! Nearest-numeric-weight gets this wrong in the case that matters most.
//! Asked for 400 from a family offering 300 and 500, both are 100 away and
//! the answer is a coin toss. The rule implemented here — CSS Fonts Level 4
//! §5.2, which browsers have used for long enough that its choices are what
//! users expect — says to prefer 500, because a text face that is slightly
//! too heavy reads as normal text and one that is too light reads as a
//! different, weaker font.
//!
//! # The order things are matched in
//!
//! Width first, then slant, then weight — the same priority CSS uses, and for
//! the same reason: the axes are not equally visible. A condensed face at the
//! right weight looks like the wrong font; an upright face where an italic
//! was asked for changes the meaning of the text; a face one weight step away
//! usually passes unnoticed. So the more disruptive substitutions are avoided
//! first, and weight absorbs whatever mismatch is left.
//!
//! Family is *not* part of the score. A caller filters to one family before
//! scoring, because "wrong family" is not a degree of wrongness that can be
//! traded against a weight difference — it is a different typeface.
//!
//! # Where the candidates come from
//!
//! Not from here. This crate does no I/O (see the crate docs); a caller
//! walks its font directories, parses each file, and scores the resulting
//! [`Style`]s. Keeping the rule here rather than in the caller is what lets
//! it be tested against the table of cases below rather than against whatever
//! happens to be installed.

use crate::sfnt::Style;

/// What a caller wants to draw with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Query {
    /// Desired weight on the CSS scale, 100..=1000.
    pub weight: u16,
    /// Whether an italic or oblique face was asked for.
    pub italic: bool,
    /// Desired `usWidthClass`, 1..=9.
    pub width: u8,
}

impl Query {
    /// Ordinary upright body text.
    #[must_use]
    pub const fn regular() -> Self {
        Self {
            weight: Style::REGULAR,
            italic: false,
            width: Style::NORMAL_WIDTH,
        }
    }

    /// Ordinary upright bold text.
    #[must_use]
    pub const fn bold() -> Self {
        Self {
            weight: Style::BOLD,
            italic: false,
            width: Style::NORMAL_WIDTH,
        }
    }

    /// The same query, slanted.
    #[must_use]
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// The same query at `weight`.
    #[must_use]
    pub const fn at_weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }
}

impl Default for Query {
    fn default() -> Self {
        Self::regular()
    }
}

/// How badly a face misses what was asked for. **Lower is better**, and zero
/// is an exact match.
///
/// Opaque and `Ord` rather than a number, because the comparison is
/// lexicographic across three axes and collapsing it to one scalar is exactly
/// the mistake that makes a selector trade a condensed face for a closer
/// weight. Callers do not need to see the parts; they need to be able to take
/// the minimum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Score {
    /// Distance in width classes, checked first.
    width: u8,
    /// 0 if the slant matches, 1 if not.
    slant: u8,
    /// Which of the CSS substitution tiers this weight falls in.
    weight_tier: u8,
    /// Distance in weight, breaking ties within a tier.
    weight_distance: u16,
}

impl Score {
    /// Whether the face matches on every axis.
    #[must_use]
    pub const fn is_exact(self) -> bool {
        self.width == 0 && self.slant == 0 && self.weight_tier == 0
    }
}

/// How well `have` answers `want`.
#[must_use]
pub fn score(have: Style, want: Query) -> Score {
    let (weight_tier, weight_distance) = weight_rank(have.weight, want.weight);
    Score {
        width: have.width.abs_diff(want.width),
        slant: u8::from(have.italic != want.italic),
        weight_tier,
        weight_distance,
    }
}

/// The best of `candidates`, or `None` if there are none.
///
/// Ties go to the earlier candidate, so a caller that has an order it cares
/// about — files sorted by path, say — gets a stable answer rather than one
/// that depends on directory iteration order.
pub fn best<T>(candidates: impl IntoIterator<Item = T>, want: Query, style_of: impl Fn(&T) -> Style) -> Option<T> {
    candidates
        .into_iter()
        .map(|c| {
            let s = score(style_of(&c), want);
            (s, c)
        })
        // `min_by_key` keeps the *first* minimum, which is the tie rule above.
        .min_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, c)| c)
}

/// Where `have` falls in the CSS substitution order for `want`.
///
/// Returns `(tier, distance)`, compared in that order — tier first, then the
/// nearest within a tier. This is CSS Fonts Level 4 §5.2 verbatim, in the
/// three cases it splits the scale into:
///
/// **The body-text band, `400..=500`.** Weights from the target up to 500 are
/// tried first, ascending; then everything below the target, descending; then
/// everything above 500, ascending. So a family offering 300 and 500 answers
/// a request for 400 with 500 — a body face that is slightly too heavy still
/// reads as body text, while a Light one makes the whole UI look washed out.
/// Note that the band is searched *upward from the target*, so 450 beats 500:
/// the rule is "the least heavier face that is still not display weight", not
/// "500 specifically".
///
/// **Below 400.** Lighter first, descending, then heavier ascending. A
/// request for Thin answered with Bold would not read as thin at all, so the
/// lighter direction is exhausted before crossing over.
///
/// **Above 500.** Heavier first, ascending, then lighter descending — the
/// mirror, for the same reason.
///
/// The crossover tiers exist because refusing is not an option: a family with
/// only a Regular must still answer a request for Bold, or the text is not
/// drawn at all.
fn weight_rank(have: u16, want: u16) -> (u8, u16) {
    if have == want {
        return (0, 0);
    }
    if (400..=500).contains(&want) {
        if have <= 500 {
            // `have != want`, so this is either above the target and inside
            // the band (tier 1) or below it (tier 2).
            if have > want {
                return (1, have.saturating_sub(want));
            }
            return (2, want.saturating_sub(have));
        }
        // Above 500: display weights, the last resort for body text.
        return (3, have.saturating_sub(500));
    }
    // Outside the band the preferred direction is "away from the middle":
    // lighter for a light request, heavier for a bold one.
    let preferred = if want < 400 { have < want } else { have > want };
    let tier = if preferred { 1 } else { 2 };
    (tier, have.abs_diff(want))
}

#[cfg(test)]
// A test that cannot find a candidate among the ones it just supplied should
// panic with the reason, which is what these say.
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A face at `weight`, upright and normal width.
    const fn at(weight: u16) -> Style {
        Style {
            weight,
            italic: false,
            width: Style::NORMAL_WIDTH,
        }
    }

    /// The weight picked out of `available` when `want` is asked for.
    fn pick(available: &[u16], want: u16) -> u16 {
        let styles: Vec<Style> = available.iter().map(|w| at(*w)).collect();
        best(styles, Query::regular().at_weight(want), |s| *s)
            .expect("candidates were not empty")
            .weight
    }

    #[test]
    fn an_exact_weight_always_wins() {
        for w in [100, 300, 400, 500, 700, 900] {
            assert_eq!(pick(&[100, 300, 400, 500, 700, 900], w), w);
        }
    }

    #[test]
    fn a_body_request_reaches_up_into_the_400_to_500_band_first() {
        // The case nearest-numeric gets wrong: 300 and 500 are both 100 away
        // from 400, and picking the Light one makes body text look washed out.
        assert_eq!(pick(&[300, 500], 400), 500);
        // The band is searched upward *from the target*, so a Medium-ish face
        // beats 500 itself — the rule is "the least heavier face that is not
        // yet display weight", not "500 specifically".
        assert_eq!(pick(&[450, 500], 400), 450);
        // Above 500 is the last resort, behind even a much lighter face.
        assert_eq!(pick(&[100, 600], 400), 100);
    }

    #[test]
    fn a_medium_request_drops_to_regular_before_reaching_for_bold() {
        // Nothing in (500, 500] to reach up into, so 500 falls back to
        // lighter before display weights: Regular stands in for Medium.
        assert_eq!(pick(&[400, 600], 500), 400);
        assert_eq!(pick(&[300, 400], 500), 400);
    }

    #[test]
    fn a_light_request_reaches_down_before_up() {
        // Below 400 the preferred direction is lighter: a request for 200
        // answered with 700 would not read as light at all.
        assert_eq!(pick(&[100, 700], 200), 100);
        assert_eq!(pick(&[300, 500], 200), 300);
        // Nearest within the preferred direction.
        assert_eq!(pick(&[100, 200, 300], 350), 300);
    }

    #[test]
    fn a_bold_request_reaches_up_before_down() {
        // Above 500 the preferred direction is heavier.
        assert_eq!(pick(&[400, 900], 700), 900);
        assert_eq!(pick(&[300, 800], 700), 800);
        assert_eq!(pick(&[700, 800, 900], 750), 800);
    }

    #[test]
    fn the_other_direction_is_used_when_the_preferred_one_is_empty() {
        // A family with only Regular must answer a request for Bold, and a
        // family with only Bold must answer a request for Regular. Refusing
        // would mean drawing nothing.
        assert_eq!(pick(&[400], 700), 400);
        assert_eq!(pick(&[700], 400), 700);
        assert_eq!(pick(&[900], 100), 900);
    }

    #[test]
    fn the_nearest_within_a_direction_wins() {
        assert_eq!(pick(&[100, 200, 300], 900), 300, "all lighter: take the heaviest");
        assert_eq!(pick(&[600, 800, 900], 100), 600, "all heavier: take the lightest");
    }

    #[test]
    fn slant_outranks_weight() {
        // An upright face where an italic was asked for changes how the text
        // reads; a face one weight step away usually passes unnoticed. So a
        // far-off italic beats a perfect upright.
        let upright_400 = at(400);
        let italic_900 = Style {
            weight: 900,
            italic: true,
            width: Style::NORMAL_WIDTH,
        };
        let want = Query::regular().italic();
        assert!(score(italic_900, want) < score(upright_400, want));
    }

    #[test]
    fn width_outranks_slant_and_weight() {
        // A condensed face looks like a different typeface, so it loses to a
        // normal-width one even when the normal one misses on both other
        // axes.
        let condensed_exact = Style {
            weight: 400,
            italic: true,
            width: 3,
        };
        let normal_wrong = Style {
            weight: 900,
            italic: false,
            width: Style::NORMAL_WIDTH,
        };
        let want = Query::regular().italic();
        assert!(score(normal_wrong, want) < score(condensed_exact, want));
    }

    #[test]
    fn a_condensed_request_prefers_a_condensed_face() {
        // The mirror: asking for Arial Narrow must not be answered with Arial.
        let want = Query {
            weight: 400,
            italic: false,
            width: 3,
        };
        let narrow = Style {
            weight: 400,
            italic: false,
            width: 3,
        };
        let normal = at(400);
        assert!(score(narrow, want) < score(normal, want));
        assert!(score(narrow, want).is_exact());
    }

    #[test]
    fn an_exact_match_scores_zero_on_every_axis() {
        let want = Query::bold().italic();
        let have = Style {
            weight: Style::BOLD,
            italic: true,
            width: Style::NORMAL_WIDTH,
        };
        assert!(score(have, want).is_exact());
        // And nothing else can tie with it, or `best` could return a worse
        // face when an exact one is present.
        assert!(score(have, want) < score(at(700), want));
    }

    #[test]
    fn nearer_widths_beat_further_ones() {
        let want = Query::regular();
        let w = |width| {
            score(
                Style {
                    weight: 400,
                    italic: false,
                    width,
                },
                want,
            )
        };
        assert!(w(5) < w(4));
        assert!(w(4) < w(3));
        assert!(w(6) < w(9));
    }

    #[test]
    fn nothing_to_choose_from_is_not_a_choice() {
        let none: Vec<Style> = Vec::new();
        assert!(best(none, Query::regular(), |s| *s).is_none());
    }

    #[test]
    fn ties_go_to_the_first_candidate() {
        // Two faces that are equally good must resolve the same way every
        // time, or which font the UI uses depends on directory order.
        let a = Style {
            weight: 300,
            italic: false,
            width: Style::NORMAL_WIDTH,
        };
        let b = Style {
            weight: 500,
            italic: false,
            width: Style::NORMAL_WIDTH,
        };
        // Equidistant from 400 in the *same* tier only if neither is the
        // 400/500 special case, so use a request where they genuinely tie.
        let want = Query::regular().at_weight(100);
        assert_eq!(score(a, want).cmp(&score(b, want)), core::cmp::Ordering::Less);
        // A real tie: the same style twice, distinguished by a payload.
        let picked = best([(1_u8, a), (2, a)], want, |(_, s)| *s).expect("some");
        assert_eq!(picked.0, 1);
    }

    #[test]
    fn the_weights_this_host_actually_ships_resolve_sensibly() {
        // Drawn from the weight histogram of the development host's 556
        // installed fonts, which is where the odd values come from.
        let family = [250, 290, 300, 350, 400, 450, 500, 600, 700, 800, 900];
        assert_eq!(pick(&family, 400), 400);
        assert_eq!(pick(&family, 700), 700);
        // A family that ships the odd ones but not the round one: 450 is
        // inside the 400..=500 band, so it is reached before anything lighter.
        assert_eq!(pick(&[250, 290, 350, 450], 400), 450);
        // With nothing in the band, the nearest lighter face takes over.
        assert_eq!(pick(&[250, 290, 350], 400), 350);
        assert_eq!(pick(&[600, 800], 700), 800);
    }
}
