## 536. `readable_on` measures the contrast ratio rather than estimating brightness, and so guarantees a floor for colours nobody chose in advance

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The shell picks black-ish or white-ish lettering for anything drawn on a coloured fill — a switch knob, a title bar, the label on a selected chip. It used to guess which one by adding up the red, green and blue of the fill and comparing that sum against a fixed number. That guess is close enough for the fourteen accent colours the settings page offers, but the user can also pick *any* colour with a custom-accent picker, and for some of those the guess is badly wrong: a bright green gets pale lettering that is nearly invisible on it. The fix is to stop guessing. Ask the actual legibility ratio — the same number the accessibility standards and all our own tests are stated in — and use whichever of the two inks scores higher.

### What was there

```rust
pub fn readable_on(bg: Color) -> Color {
    let luma = 0.299 * f32::from(bg.r) + 0.587 * f32::from(bg.g) + 0.114 * f32::from(bg.b);
    if luma > 140.0 { DARK_EXTREME } else { LIGHT_EXTREME }
}
```

*Luma* (a weighted sum of the stored bytes, green counting most because the eye is most sensitive to it) is a reasonable estimate of perceived brightness, and 140 was a reasonable place to cut it. Both halves of that sentence are true and the function was still wrong, for a reason worth stating precisely:

**Luma is not a monotone function of contrast.** Contrast ratio (WCAG 2's, the one every legibility floor in this tree is written in) first *linearises* each channel — undoes the sRGB gamma curve, so the number describes light rather than the byte that encodes it — and then weights the results differently again (0.2126/0.7152/0.0722). Two colours with the same luma can therefore sit on opposite sides of the real crossover. So a luma threshold cannot be tuned to agree with the ratio everywhere; it can only be tuned to agree on some particular set of colours.

Which is exactly what had happened. Measured over the whole 24-bit cube, the threshold's worst answer was **1.40:1**, at `#00EE02` — luma 139.93, a hair under the cut, so called dark and inked `#EFF1F5`. That is not a contrived colour: `AccentColor::Custom` lets the appearance page write any RGB triple into `custom_accent`, `effective_accent()` returns it, and `Palette::accent` is what switch pills, title bars and chips are filled with. A user who dragged the picker to a bright green got a switch knob they could not see.

The threshold's *other* failure was quieter and is the one that made this hard to notice: it was never checked against anything. `gui/appearance`'s own colours have a wide empty band around it — the nearest are `#7F849C` at luma 133 and `#9CA0B0` at 161 — so the threshold could have been anywhere from 134 to 160 without a single test changing its verdict. A number that nothing constrains is a number that is only as right as the day it was typed.

### What is there now

```rust
pub fn readable_on(bg: Color) -> Color {
    if contrast_ratio(bg, DARK_EXTREME) >= contrast_ratio(bg, LIGHT_EXTREME) {
        DARK_EXTREME
    } else {
        LIGHT_EXTREME
    }
}
```

with `relative_luminance` and `contrast_ratio` made public, because they are the input to every legibility question the shell asks and there must be exactly one implementation of the question.

The property this buys is the point of the change, and it is not "slightly better on average":

| | luma threshold at 140 | contrast comparison |
|---|---|---|
| worst ink over the whole cube | **1.40:1** (`#00EE02`) | **4.07:1** (`#B82EE5`) |
| depends on which colours are in the palette | yes | no |
| stated in the same units as the floors the tests assert | no | yes |

4.07:1 is not a target anyone picked. It falls out of the two extremes: it is the ratio at the colour equidistant from both, and it is therefore the *best possible* guarantee for a function that must answer with one of two fixed inks. It sits above the 3:1 that SC 1.4.11 asks of a control's outline and below the 4.5:1 that SC 1.4.3 asks of body text, which is the honest statement of what an arbitrary user-chosen accent can promise. Raising it would mean moving the extremes apart, not editing a number.

### Cost paid

**Almost none in what ships.** Exactly two colours in the shell's own palettes change ink, both upward: dark `overlay1` (3.27 → 5.07) and the plain grey `#808080` (3.49 → 4.75). No preset accent moves at all, so the desktop a user sees is pixel-identical unless they had chosen a custom accent in the band where the old rule was wrong — which is precisely the population the change is for.

**Some in arithmetic.** Two `powf(2.4)` per channel per call instead of three multiplies. `readable_on` is called a handful of times per panel render, not per pixel; if it ever moves into a per-pixel path the answer is a 256-entry linearisation table, not a return to the estimate.

**Three test fixtures lost their premise,** and that is the interesting cost. Each had chosen its fixture accent *because of* its luma:

- `accessibility_settings::accented` and `default_apps::accented` used magenta `#FF00FF` because a luma sum calls it dark (105) and it therefore inked pale — which is what let their deleted-constant tables forbid `CRUST`, whose value `#11111B` is also the dark extreme. By ratio, magenta inks dark (5.98:1 against 2.77:1), so the tables could no longer distinguish a leftover `CRUST` literal from the correct answer. Both fixtures moved to indigo `#4B0082`, which is dark under both readings with a margin (11.4:1 pale against 1.4:1 dark) that no retune can close.
- `print_manager` counts exactly three sites that move with the accent, one of which moves only if its two fixture accents *straddle* `readable_on`'s answer. They stopped straddling, so the count silently became a count of two — and, being a count, it still passed the "nothing extra follows the accent" half of its job while quietly dropping the "the derived ink is one of them" half.

In all three the repair was the same shape, and it is the rule this decision leaves behind: **a fixture chosen for a property must assert that property, not describe it in a doc comment.** All three now do; a fourth flip of the ink rule fails at the fixture, naming itself, instead of at a distant assertion whose message is about something else.

`display_settings` had a fourth variant of the problem: it modelled the ink rule *by hand* as a deliberately independent oracle, and the model was the luma threshold. It survived its own replacement only by luck — all fourteen accents fall on the same side under both readings — so it would have gone on passing while asserting a rule the shell no longer follows. It now transcribes the WCAG ratio. An independent oracle is only worth having while it is independently *right*; the independence is what makes it silent when it rots.

### Alternatives rejected

- **Retune the threshold.** No value works: the disagreement between luma and contrast is not an offset, it is a different shape. The best threshold still has a worst case around 1.4:1 somewhere in the cube.
- **Keep the threshold and forbid custom accents outside a safe band.** Takes a feature away from the user to protect an implementation detail, and would need the contrast computation anyway to define the band.
- **Return the ink with the higher contrast *and* widen the extremes to reach 4.5:1 everywhere.** The extremes are `LIGHT_BASE`'s value and Mocha `CRUST`'s, chosen (§532 and the constants' own docs) so that accented surfaces still look like part of this desktop. Pushing them to `#000`/`#fff` to buy 0.4 of a ratio point would make every accented label a different black from the one beside it. Rejected for the same reason `readable_on` is not `guitk::theme::contrast_text`.
- **Leave it and document the corner.** The corner is reachable from a shipping settings picker in two clicks, and the symptom is an invisible control. Documenting a live legibility bug is not a resolution of it.

### Left standing, deliberately

`guitk::theme::contrast_text` has the *same* defect in a worse form: it picks pure black or white by `luminance(c) < 0.5`, where `luminance` uses a `powf(2.2)` approximation and the true crossover for black-vs-white is 0.179. Colours between those two figures get white when black is far better — around 2.3:1 at the midpoint. It is a separate crate with its own callers and its own test surface, it is not what the palette uses, and folding it into this change would have made a two-crate diff untestable as one thing. Logged in `known-issues.md` as its own item.
