## 534. A repeated control gets a module that draws it, and the ink is chosen for whatever the shape's outline is read against

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Two controls in the desktop shell — the on/off switch and the
horizontal slider — were each being drawn from scratch in every panel that
needed one: seventeen hand-written copies of the switch and five of the slider.
Every copy had to pick the colour of the small moving part (the switch's knob,
the slider's handle), and they picked wrong in ways that made that part hard or
impossible to see. Rather than correct twenty-two copies, each control now has
one module that draws it and picks that colour itself. The second half of the
decision is *which* colour each one picks — and the two answers are opposite,
because the two shapes sit on different backgrounds.

### What was decided

1. **A control drawn by more than one module gets its own module.**
   `gui/desktop/src/switch.rs` and `gui/desktop/src/slider.rs` draw the
   geometry and choose the ink; call sites pass position, size, state and the
   *background* colours only.
2. **A switch knob is `appearance::readable_on(track)`.**
3. **A slider thumb is `p.text`** — deliberately *not* derived from the fill.
4. **The general rule behind 2 and 3:** an ink is chosen for the background the
   shape's outline is read against, and *containment* is what decides which
   background that is.

### Why the ink rules are opposite

This is the part that looks like an inconsistency and is not.

A switch knob is inset two pixels from every edge of its track. It is
*contained*: the track is the only thing behind it, so the track is what it must
be legible against, and `readable_on(track)` — which returns near-black on a
pale fill and near-white on a deep one — is exactly right. Measured, the
seventeen copies were using `p.text`, which on the stock dark accent is 1.35:1.

A slider thumb is *bigger* than its track: a 10-to-14 pixel disc on a track 4 or
6 pixels tall. Most of it hangs over the card behind the control, so the round
outline that says *this is the handle* is drawn against the card, not the fill.
Apply the switch rule and you get `readable_on(accent)` = `#11111B`, which on
the stock dark theme is 1.1:1 against a `base` card — the handle disappears into
the page while gaining a crisp interior edge nobody can see. `p.text` is
11.34:1 against the card and 1.46:1 against the fill; the weak number is the one
that costs nothing, because the fill only ever touches the thumb's *interior*,
which carries no information once the circle is visible.

`slider.rs` asserts the containment premise directly rather than relying on the
prose, so a future shape whose thumb fitted *inside* its track fails a test
instead of silently inheriting reasoning that no longer applies to it.

### Alternatives considered

**(a) Fix the colour in place at each of the twenty-two sites.** Smaller diff,
no new module, nothing else can regress. Rejected: that is what the tree already
was. `mouse_settings.rs` had *already* worked out that a handle on an accent
fill needs a derived colour and had already fixed its own slider, and that fix
stayed in the one file where it was made while four other sliders and seventeen
switches carried the defect. A correct answer callers cannot reach grows wrong
copies — the defect the whole 49-module palette conversion existed to remove. In
-place fixes also leave the shape open for the eighteenth panel, which is not a
hypothetical: this shell gains panels.

**(b) One `controls.rs` holding both.** Rejected on a thin margin: two modules
named for two controls read better at the call site (`crate::switch::switch`,
`crate::slider::Slider`), and each control's ink reasoning is long enough to
want its own module doc. Revisit if a third and fourth control arrive.

**(c) Make the thumb's ink a parameter, as the track's is.** Rejected — that is
precisely how one site came to ink its thumb with the same accent as the fill
underneath it. The track *is* a parameter, because panels disagree about it on
purpose (most use `p.accent` for *on*, but the accessibility and notification
panels use `p.green`, since there *on* means safe rather than selected). The ink
is not a disagreement anyone is entitled to have.

**(d) Unify both controls on one rule for consistency's sake.** Rejected. The
consistency would be superficial and the result would be worse on one of the two
— which is the whole content of the containment argument above.

### What it cost

The switch conversion touched 18 files and broke 20 tests, every one of them
correctly: they had pinned the knob to `p.text`, i.e. they asserted the bug.
Each was rewritten to state the claim the fix makes rather than relaxed. The
slider conversion broke two more, one of which
(`touchpad::every_control_that_offers_something_follows_the_accent`) had listed
the slider knob as a *fourth* thing that correctly follows the accent — the bug
written down as a requirement. It now asserts the opposite, with the reason
beside it.

### Two limits worth recording

- **The switch's 4.5:1 floor could not be asserted over every palette role.**
  `readable_on` picks between exactly two inks, so on a mid-grey (`overlay0`,
  `#6C7086`) the better of the two is 4.32:1 and 4.5 is unreachable without a
  third ink. The floor is asserted over the colours a switch track can actually
  be; a separate test asserts the weaker but universal claim — that the choice
  between the two inks is the right way round — on all 42 role-mode pairs. That
  second test earns its place: `readable_on` decides with a fast weighted-luma
  threshold while legibility is governed by gamma-corrected WCAG relative
  luminance, and nothing before now had checked that the approximation never
  crosses over. It does not.
- **The two controls use different floors on purpose.** The switch asserts
  4.5:1 and the slider 3:1. 3:1 is WCAG SC 1.4.11 (*Non-text Contrast*), the
  criterion that actually applies to a graphical object identifying a control;
  4.5:1 is SC 1.4.3, for text. The stricter number is kept where it is
  reachable and not invented where it is not — ten of the slider's twelve
  card-mode pairs clear 4.5 anyway, and the two that do not are a palette fact
  logged separately as
  `TD-C-TEXT-ON-THE-LIGHT-THEMES-TWO-PALEST-SURFACES-IS-BELOW-THE-CONTRAST-FLOOR`.
