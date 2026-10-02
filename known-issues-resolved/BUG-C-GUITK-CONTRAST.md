### BUG-C-GUITK-CONTRAST_TEXT-PICKS-WHITE-WHERE-BLACK-IS-FOUR-TIMES-MORE-LEGIBLE

**Status: FIXED** 2026-08-24, the same day it was found — see §537. Kept here
in full because the *prediction* at the bottom of this entry was wrong, and how
it was wrong is the useful part.

**What actually happened.** `contrast_text` and `is_dark` had **no production
callers at all** — only their own six tests, between them checking `#14141E`,
`#F0F0F0`, pure black, pure white, and the Mocha and Latte backgrounds
(`#1E1E2E`, `#EFF1F5`). Every one of those is far from the 0.179 crossover, so
all six passed before the fix and all six pass after it, unchanged: they never
constrained the rule they were named for. The "run of pinned-colour test
failures" this entry
told a future reader to expect was zero, and the blast radius it told them to
reconcile was empty. The entry asserted "a crate with many widget callers"
without grepping for one. **A blast-radius estimate that was not measured
should say so** — an unmarked guess in a tracking file is read later as a
finding, and this one made a fifteen-minute change look like a second project
and kept it queued longer than it needed to be.

The severity figures below were also understated. Measured over the whole
24-bit cube: the wrong rule disagreed with the right one on **41.78 %** of all
colours, and its worst case was `#21D828`, where it returned white at
**1.92:1** with black available at **10.92:1**.

---

**Status when filed: OPEN** — found 2026-08-24 while fixing the same defect in `appearance::readable_on` (§536).

**Where:** `gui/toolkit/src/theme.rs:16-35` — `luminance`, `is_dark`, `contrast_text`.

```rust
fn luminance(c: Color) -> f32 {
    let r = (c.r as f32 / 255.0).powf(2.2);   // ...
    0.2126 * r + 0.7152 * g + 0.0722 * b
}
pub fn is_dark(color: Color) -> bool { luminance(color) < 0.5 }
pub fn contrast_text(background: Color) -> Color {
    if is_dark(background) { Color::WHITE } else { Color::BLACK }
}
```

**What is wrong.** `contrast_text` answers "black or white on this?" by
thresholding relative luminance at **0.5**. The crossover where pure black and
pure white are equally legible is not 0.5 — it is
`sqrt(0.05 * 1.05) - 0.05 = 0.1791`. Every background whose luminance falls
between 0.179 and 0.5 therefore gets **white** when **black** is the better
answer, and the gap is not small: at luminance 0.40 white reaches 1.16:1 while
black reaches 9.0:1. That band is not exotic — it is most mid-tone colours: a
mid grey, a medium blue, a muted green.

Two smaller faults compound it:

- `luminance` uses `powf(2.2)` as a stand-in for the sRGB transfer function.
  sRGB is a linear segment below 0.03928 and `((v+0.055)/1.055)^2.4` above it;
  the 2.2 power is a decent approximation in the middle and drifts at both
  ends. The real function is four lines and there is no reason to approximate
  it.
- `is_dark` is `pub` and reads as a general-purpose question, so the 0.5
  threshold — which is only ever meaningful as (a wrong) answer to the
  black-or-white question — is exported for other code to build on.

**How to see it.** `contrast_text(Color::from_hex(0x6A6A6A))` returns
`Color::WHITE`; white on `#6A6A6A` is 2.85:1, black on it is 7.36:1.

**The proper fix,** which is exactly what §536 did one crate over: delete the
threshold and compare the two candidate inks by contrast ratio.
`appearance::contrast_ratio` is now public and is the single correct
implementation, but `guitk` cannot depend on `appearance` (the dependency runs
the other way — `appearance` depends on `guitk` for `Color`). So the ratio
belongs *here*, in `guitk`, and `appearance::relative_luminance` /
`appearance::contrast_ratio` should become thin re-exports of it. That is the
right direction anyway: a colour metric is a property of the colour type, and
the colour type lives in the toolkit.

Sketch:

```rust
// gui/toolkit/src/color.rs (or theme.rs)
pub fn relative_luminance(c: Color) -> f32 { /* the real sRGB curve */ }
pub fn contrast_ratio(a: Color, b: Color) -> f32 { /* (hi+0.05)/(lo+0.05) */ }
pub fn contrast_text(bg: Color) -> Color {
    if contrast_ratio(bg, Color::BLACK) >= contrast_ratio(bg, Color::WHITE) {
        Color::BLACK
    } else {
        Color::WHITE
    }
}
```

`is_dark` should either go (it has no correct threshold to name) or be redefined
as `contrast_ratio(c, Color::WHITE) > contrast_ratio(c, Color::BLACK)`, which is
the only question it can answer honestly.

**Why it was not done in the same change.** `contrast_text` is `pub` in a crate
with many widget callers, and flipping the ink for the whole mid-tone band will
move colours in widgets whose tests pin them. That is a second diff with its own
test sweep, not a rider on a one-crate palette fix — and mixing them would have
made neither testable as one thing. It is not blocked on anything; it is queued.

**Blast radius to check when doing it:** every caller of `contrast_text` and
`is_dark` in `gui/toolkit/**` and anything outside it that imports them, plus
`guitk`'s own theme tests. Expect a run of pinned-colour test failures, each of
which should be checked individually — the new value is the more legible one by
construction, so a failure is a fixture to update, not a regression, but the
*count* of them should be reconciled against the callers found.
