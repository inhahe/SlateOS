## 531. Blur tint weights are anchored at the most-transparent setting and interpolated up to opaque, rather than being independent of the setting

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop blurs what is behind a taskbar, a menu or a
notification, and then lays a thin wash of colour over the blur so the panel
reads as a surface rather than as a smear. How strong that wash is was a fixed
number per panel type — a menu 100 of 255, a taskbar 160 — and it took no
notice whatsoever of the user's transparency setting in Appearance. So a user
who set transparency to **Off** still had a taskbar about 37% see-through.
This entry decides how the setting and those numbers are reconciled: the
existing numbers are kept and reinterpreted as the values for the *most*
transparent setting, and every setting above that interpolates toward fully
opaque.

**The problem.** `BlurEffect::taskbar()`, `::title_bar()`, `::menu()`,
`::notification()` and `::standard()` each hardcoded a tint alpha. Those five
numbers are not arbitrary — they are a deliberate hierarchy (a menu is the
lightest touch, a taskbar the heaviest, a notification and the generic surface
in between), and they look right. But `TransparencyLevel` — `Off`, `Subtle`,
`Moderate`, `Full`, exposed as a user setting and already honoured by
`Palette::panel_bg()` via `panel_alpha` — was read by nothing in this module.
Two things were therefore wrong at once: the setting did nothing to blurred
surfaces, and `Off` in particular was a promise the shell did not keep.

**The options.**

- **Leave the tints independent of the setting.** *What changes:* nothing; the
  transparency slider continues to affect solid panels and not blurred ones.
  Defensible only if blur tint is considered a separate concept from panel
  translucency — but a user cannot see two concepts here, only one surface that
  disobeys the control above it.
- **Multiply each preset by `panel_alpha / 255`.** *What changes:* every panel
  gets *more* see-through as the setting rises, and `Off` (255) leaves the
  presets exactly as they are — i.e. `Off` still means 37% see-through. This
  gets the direction of the control right and the endpoint wrong, which is the
  worse half to get wrong: the endpoint is the only part of the scale that is
  not a matter of taste.
- **Anchor at `Full` and interpolate up to opaque.** *(chosen)* *What changes:*
  at `Full` every panel looks exactly as it does today; at `Off` every tint is
  fully opaque and nothing shows through; `Subtle` and `Moderate` sit in
  between, each preset keeping its position in the hierarchy the whole way up.

**Why the third.** The five numbers were unquestionably written while looking
at a translucent desktop, so treating them as the *Full* row of a table costs
nothing — the look everyone has seen is preserved bit-for-bit at that setting.
And the constraint that actually decides it is at the other end: **`Off` must
mean opaque.** That is not a preference, it is what the word means, and it is
the one point on the scale where a wrong answer is a bug rather than a taste
disagreement. Anchoring at `Full` is the only one of the three that satisfies
it without moving anything a user has already grown used to.

The anchor is a named constant (`TINT_ANCHOR = 160`, which is
`TransparencyLevel::Full.panel_alpha()`) rather than an inlined 160, because
"these weights were written for Full" is the entire premise of the scheme and a
premise should be checkable. `the_tint_weights_are_written_for_the_full_setting`
checks it, and `transparency_off_leaves_no_blurred_surface_see_through` checks
the endpoint that decided the question.

**The cost, and why it is accepted.** `Palette::panel_alpha` is a public field,
so a caller can put a value *below* the anchor there — more transparency than
the shell's own most-transparent setting offers. There is nothing below the
anchor to interpolate toward, so such a value is clamped up to it, and
`a_panel_alpha_below_the_anchor_is_clamped` pins that. The alternative —
extending the scale downward past `Full` — would invent a look no setting can
produce and no one has approved.
