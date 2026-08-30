//! The sweep that proves a module was actually converted off its own colours.
//!
//! Part 2 of `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`
//! replaces 549 hand-written `const … : Color` across 49 modules with roles
//! read out of a [`Palette`]. That is a large mechanical edit, and the way a
//! large mechanical edit fails is that one substitution is missed — which is
//! invisible, because a missed constant still *compiles* and still draws the
//! colour it always drew.
//!
//! It is invisible to the eye and to every ordinary test, but it is not
//! invisible to arithmetic. Every constant being deleted is a **Catppuccin
//! Mocha** value. So render the module twice, once per mode, and check the
//! *light* render: a leftover constant is a dark value, the light palette does
//! not contain it, and it names itself. That is what [`assert_drawn_from`]
//! does, and it is the reason the conversion can be trusted at all rather than
//! reviewed 549 times.
//!
//! # What counts as "from the palette"
//!
//! A renderer does not only emit palette members verbatim; it emits them at an
//! alpha, and it emits a few colours that are deliberately *not* themed. The
//! allowed set is therefore:
//!
//! - any [`Palette::roles`] entry, **compared on RGB only** — `with_alpha` is
//!   how a panel, a wash and a hover state are built, and re-listing each of
//!   those as a separate legal value would allow anything;
//! - **black at any alpha** — a scrim and a drop shadow are an absence of
//!   light rather than a colour (§525 decision 3), which is why they do not
//!   flip with the mode;
//! - whatever the caller declares in `derived`.
//!
//! # `readable_on` ink is declared, not exempt
//!
//! Text chosen for a coloured fill is a function of that fill's brightness
//! rather than a role, so it is not a palette member and has to be allowed
//! somehow. This module used to allow it *unconditionally*: the two values
//! [`readable_on`](appearance::readable_on) can return, `0x11111B` and
//! `0xEFF1F5`, passed the sweep in any module and in either mode.
//!
//! That was a hole big enough to drive the whole conversion through, because
//! **those two values are also palette roles.** `0xEFF1F5` is Latte `base` and
//! `0x11111B` is Mocha `crust` — which is to say, the single most likely
//! cross-mode literal to be left behind in each direction was the one thing
//! the sweep was told to wave past. It was found by the reintroduction
//! harness in module 48: `A×78` (a stray Mocha-base literal) was caught, and
//! `B×78` — the identical defect with Latte's base — was not. Two defects that
//! differ only in which mode's leftover they represent must not get different
//! answers.
//!
//! So the exemption is gone, and a module that draws readable ink declares
//! what it draws it on:
//!
//! ```ignore
//! assert_drawn_from(&p, &cmds, &[readable_on(p.accent)], "the panel");
//! ```
//!
//! That is a stronger statement than the blanket allowance in three ways: it
//! names the fill the ink belongs to, so a reader can check the claim; it is
//! confined to the module that makes it, so the other thirty-six get the
//! endpoints checked as ordinary roles; and it goes stale loudly — draw the
//! ink on something else and the declaration no longer covers it.
//!
//! The cost is real and worth stating: thirteen of the forty-nine shell
//! modules draw such ink and each now carries a declaration. The old comment
//! here estimated that cost as "every module that draws a coloured button,
//! which is most of them" and used it to justify keeping the hole. Measured,
//! it is 27% of them and one line each.
//!
//! # Why `derived` is a parameter and not a blanket allowance
//!
//! [`emphasized`](appearance::emphasized) and `Color::lerp` produce colours
//! that are genuinely in no palette. Allowing "anything near a role" to cover
//! them would gut the check. Instead each module names its own derivations at
//! the call site, so a colour that is not a role has to be *claimed* by
//! someone — which turns the exception into documentation of what the module
//! computes.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::RenderCommand;

/// Every colour `cmd` will put on the screen.
///
/// Commands that carry no colour — the clip, translate and font scopes, and
/// [`RenderCommand::Image`] — contribute nothing, which is correct rather than
/// a gap: an image's pixels are not the shell's to theme.
fn colors_of(cmd: &RenderCommand) -> Vec<Color> {
    match cmd {
        RenderCommand::FillRect { color, .. }
        | RenderCommand::StrokeRect { color, .. }
        | RenderCommand::Text { color, .. }
        | RenderCommand::Line { color, .. }
        | RenderCommand::BoxShadow { color, .. } => vec![*color],
        // The trailing `color` *and* every span: a syntax-highlighted run
        // carries most of its colours in the spans, so checking only the
        // fallback would check the one colour least likely to be wrong.
        RenderCommand::RichText { color, spans, .. } => {
            let mut v = vec![*color];
            v.extend(spans.iter().map(|s| s.color));
            v
        }
        RenderCommand::Image { .. }
        | RenderCommand::PushClip { .. }
        | RenderCommand::PopClip
        | RenderCommand::PushTranslate { .. }
        | RenderCommand::PopTranslate
        | RenderCommand::PushFont { .. }
        | RenderCommand::PopFont => Vec::new(),
    }
}

/// Whether `c` is a colour `p` can account for.
fn is_accounted_for(p: &Palette, c: Color, derived: &[Color]) -> bool {
    // Black at any alpha: scrims and shadows.
    if c.r == 0 && c.g == 0 && c.b == 0 {
        return true;
    }
    // RGB only: alpha is how a role becomes a panel, a wash or a hover.
    if p.roles()
        .iter()
        .any(|(_, r)| r.r == c.r && r.g == c.g && r.b == c.b)
    {
        return true;
    }
    derived
        .iter()
        .any(|d| d.r == c.r && d.g == c.g && d.b == c.b)
}

/// Assert every colour in `cmds` is one `p` can account for.
///
/// `what` names the module in the failure message, and `derived` lists the
/// colours the module computes rather than reads — see the module docs for why
/// that is a parameter.
///
/// # Panics
///
/// When a command carries a colour that is neither a role of `p`, nor black,
/// nor listed in `derived`. The message gives the offending value, the command
/// index and the mode, because "some colour is wrong somewhere in 300 commands"
/// is not an actionable failure.
pub fn assert_drawn_from(p: &Palette, cmds: &[RenderCommand], derived: &[Color], what: &str) {
    for (i, cmd) in cmds.iter().enumerate() {
        for c in colors_of(cmd) {
            assert_one(p, &format!("command {i}"), c, derived, what);
        }
    }
}

/// Assert every colour in `named` is one `p` can account for.
///
/// The sibling of [`assert_drawn_from`] for a module whose themed colours are
/// *values* rather than draw commands — a blur tint, a wallpaper fallback, a
/// preset. Such a module carries exactly the same defect (a Catppuccin Mocha
/// literal that a light palette cannot contain) and is invisible to
/// [`assert_drawn_from`] for the uninteresting reason that it never builds a
/// [`RenderCommand`]. Rather than have those modules synthesise a fake command
/// to get checked — an artifice that would make the test about the wrapper
/// instead of the value — they hand their colours over directly, each with the
/// name of the site it belongs to.
///
/// The name is what makes the failure actionable: `command 7` is a useful
/// locator inside a scene, but a preset table has no scene, so "the taskbar's
/// tint" is the only thing that tells the reader *which* value is wrong.
///
/// # Panics
///
/// On the same condition as [`assert_drawn_from`]: a colour that is neither a
/// role of `p`, nor black, nor listed in `derived`.
pub fn assert_colours_from(p: &Palette, named: &[(&str, Color)], derived: &[Color], what: &str) {
    for (label, c) in named {
        assert_one(p, label, *c, derived, what);
    }
}

/// The one assertion both public entry points are made of.
fn assert_one(p: &Palette, label: &str, c: Color, derived: &[Color], what: &str) {
    let mode = if p.light { "light" } else { "dark" };
    assert!(
        is_accounted_for(p, c, derived),
        "{what}: {label} in {mode} mode draws \
         #{:02X}{:02X}{:02X} (alpha {}), which is not a role of the \
         {mode} palette, not black, and not declared as derived. \
         (Ink from readable_on is declared, not exempt — if this is \
         such a colour, name the fill it sits on in `derived`.) \
         A colour constant was probably left \
         behind by the conversion — see known-issues.md \
         TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE.",
        c.r,
        c.g,
        c.b,
        c.a,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sweep's own proof: it must reject the thing it exists to find.
    ///
    /// A converted module handed the *light* palette but still drawing a Mocha
    /// constant is the entire defect, so a helper that passed that case would
    /// be worse than no helper — it would certify 49 modules as converted
    /// without checking any of them.
    #[test]
    fn a_leftover_mocha_constant_fails_the_light_sweep() {
        let light = Palette::for_mode(true);
        let leftover = RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            // Mocha `base`, the single most-copied constant in the shell.
            color: Color::from_hex(0x1E1E2E),
            corner_radii: guitk::style::CornerRadii::all(0.0),
        };
        let result = std::panic::catch_unwind(|| {
            assert_drawn_from(&light, std::slice::from_ref(&leftover), &[], "probe");
        });
        assert!(
            result.is_err(),
            "the sweep accepted Mocha base in a light render, so it would \
             certify an unconverted module as converted"
        );
    }

    /// And it must accept the palette it was given, or every module fails.
    #[test]
    fn every_role_at_every_alpha_passes_its_own_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (name, role) in p.roles() {
                for alpha in [0_u8, 60, 140, 255] {
                    let cmd = RenderCommand::FillRect {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                        color: Color::rgba(role.r, role.g, role.b, alpha),
                        corner_radii: guitk::style::CornerRadii::all(0.0),
                    };
                    assert_drawn_from(&p, std::slice::from_ref(&cmd), &[], name);
                }
            }
        }
    }

    /// The scrim and the shadows, which are black in both modes on purpose.
    #[test]
    fn black_at_any_alpha_is_accounted_for_in_both_modes() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for c in [p.scrim(), p.shadow(), p.text_shadow()] {
                assert!(
                    is_accounted_for(&p, c, &[]),
                    "the {} palette's own scrim/shadow was rejected",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// A span's colour is checked, not just the run's fallback.
    ///
    /// `RichText` carries most of its colours in `spans`; a sweep that read
    /// only `color` would pass a run whose every visible glyph was wrong.
    #[test]
    fn a_bad_span_colour_is_caught_even_when_the_fallback_is_fine() {
        let light = Palette::for_mode(true);
        let cmd = RenderCommand::RichText {
            x: 0.0,
            y: 0.0,
            text: "hi".to_string(),
            spans: vec![guitk::render::TextSpan {
                end: 2,
                color: Color::from_hex(0x1E1E2E),
            }],
            color: light.text,
            font_size: 12.0,
            font_weight: guitk::render::FontWeightHint::Regular,
            max_width: None,
            overflow: guitk::render::TextOverflow::Clip,
        };
        let result = std::panic::catch_unwind(|| {
            assert_drawn_from(&light, std::slice::from_ref(&cmd), &[], "probe");
        });
        assert!(
            result.is_err(),
            "a leftover colour in a span went unchecked"
        );
    }

    /// The value-shaped entry point must reject what the command-shaped one
    /// rejects, and must say which site was wrong.
    ///
    /// A second entry point is a second place for the predicate to be applied
    /// wrongly — the obvious way to write [`assert_colours_from`] badly is to
    /// iterate the names and forget to check the colours, which passes
    /// everything silently. So this asserts both halves: that the leftover
    /// fails at all, and that the panic message carries the site's name rather
    /// than an index the caller has no scene to look up.
    #[test]
    fn a_leftover_mocha_value_fails_and_names_its_site() {
        let light = Palette::for_mode(true);
        let named = [("the taskbar's tint", Color::from_hex(0x1E1E2E))];
        let result = std::panic::catch_unwind(|| {
            assert_colours_from(&light, &named, &[], "probe");
        });
        let payload = result.err();
        assert!(
            payload.is_some(),
            "the value sweep accepted Mocha base in a light render, so it \
             would certify an unconverted module as converted"
        );
        let msg = payload
            .as_ref()
            .and_then(|p| p.downcast_ref::<String>())
            .map_or_else(String::new, Clone::clone);
        assert!(
            msg.contains("the taskbar's tint"),
            "the failure did not name the offending site: {msg}"
        );
    }

    /// Neither `readable_on` endpoint is waved past any more.
    ///
    /// This is the hole module 48 found: both endpoints are also roles —
    /// `0xEFF1F5` is Latte `base`, `0x11111B` is Mocha `crust` — so exempting
    /// them globally un-checked the likeliest cross-mode leftover in each
    /// direction. The asymmetry that exposed it is reproduced literally here:
    /// each endpoint is offered to the palette of the *other* mode, where it is
    /// not a role, and must be rejected exactly as any other foreign literal
    /// would be.
    #[test]
    fn a_readable_on_endpoint_is_not_exempt_in_the_mode_that_lacks_it() {
        // (the endpoint, the mode whose palette does not contain it)
        for (hex, light) in [(0x00EF_F1F5_u32, false), (0x0011_111B, true)] {
            let p = Palette::for_mode(light);
            let c = Color::from_hex(hex);
            assert!(
                !is_accounted_for(&p, c, &[]),
                "#{hex:06X} was accepted undeclared by the {} palette; the \
                 blanket readable_on exemption is back, and with it the \
                 cross-mode leftover it hid",
                if light { "light" } else { "dark" }
            );
            // …and declaring it is how a module that really does draw that ink
            // gets to keep drawing it.
            assert!(
                is_accounted_for(&p, c, &[c]),
                "#{hex:06X} was rejected even though the caller declared it"
            );
        }
    }

    /// And it must accept the palette it was given, at every alpha, or the
    /// modules that hold their colours as values all fail.
    #[test]
    fn every_role_at_every_alpha_passes_the_value_sweep() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (name, role) in p.roles() {
                for alpha in [0_u8, 60, 140, 255] {
                    let named = [(name, Color::rgba(role.r, role.g, role.b, alpha))];
                    assert_colours_from(&p, &named, &[], "probe");
                }
            }
        }
    }
}
