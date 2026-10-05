## 537. The contrast arithmetic moves down into the toolkit, so the crate every widget depends on holds the only copy

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Two different parts of the desktop were each working out, on their own, whether to write on a coloured button in dark ink or light ink. The appearance settings had a correct method as of §536; the widget toolkit had its own, and the toolkit's was wrong for nearly half of all colours — on a plain red button it chose white lettering when black is noticeably easier to read. The toolkit could not simply call the good one, because the good one lived in a crate that sits *above* it. So the good one moved down into the toolkit, and the appearance crate now points at it instead of keeping a copy. There is one implementation in the tree again.

### The defect

`guitk::theme::contrast_text` chose between pure black and pure white by thresholding relative luminance at **0.5**:

```rust
fn luminance(c: Color) -> f32 {
    let r = (c.r as f32 / 255.0).powf(2.2);  // and g, b
    0.2126 * r + 0.7152 * g + 0.0722 * b
}
pub fn is_dark(color: Color) -> bool { luminance(color) < 0.5 }
pub fn contrast_text(bg: Color) -> Color {
    if is_dark(bg) { Color::WHITE } else { Color::BLACK }
}
```

Black and white are equally legible not at luminance 0.5 but at `sqrt(0.0525) - 0.05` ≈ **0.179**. The `+0.05` term in the WCAG ratio is a fixed allowance for ambient flare; it is a large fraction of a dark colour's luminance and a negligible fraction of a light one's, so it pushes the balance point down to roughly a sixth of the range. Everything between 0.179 and 0.5 therefore got the worse ink.

Measured over all 16 777 216 colours:

| | Old rule | New rule |
|---|---|---|
| Worst ink returned | **1.92:1**, at `#21D828` (black was available at 10.92:1) | **4.58:1** |
| Colours given the worse ink | **41.78 %** | 0 |
| Guarantee it can state | none — depends on which colours were nearby when 0.5 was picked | 4.58:1 for *any* colour, from the crossover |

`#FF0000` and `#808080` are both in the wrong band: red was inked white at 4.00:1 with black available at 5.25:1, grey white at 3.95:1 with black at 5.32:1. These are not exotic colours.

A second, independent error compounded it: `luminance` used `powf(2.2)` where sRGB is a linear segment below 0.03928 and `((v+0.055)/1.055)^2.4` above. On its own that flips only 0.56 % of the cube — small, but it is four lines to do exactly and there was no reason to approximate.

### The decision, and why it is a move rather than a fix

The obvious repair is to write the ratio comparison into `guitk` as well. That fixes the behaviour and leaves the actual problem in place: two crates, two implementations of relative luminance, correct today and free to drift tomorrow. This is the §534 shape — *the tree holds one correct answer that callers cannot reach, so it grows wrong copies* — and repairing the copy without removing it is treating the symptom.

`guitk` cannot call `appearance`; the dependency runs `appearance → guitk`. So the single copy has to live in `guitk`, and it does now:

- `guitk::theme::relative_luminance` and `guitk::theme::contrast_ratio` are public, and use the real piecewise sRGB curve.
- `contrast_text` compares the two candidate inks by ratio, exactly as `readable_on` does one crate up.
- `appearance` **re-exports** them — `pub use guitk::theme::{contrast_ratio, relative_luminance};` — rather than wrapping them. A wrapper is a place where two things can come apart; a re-export is not.

`appearance`'s own Cargo.toml already made this argument, about `Color`, before the contrast helpers existed: the toolkit is *"the one crate whose job is to stop there being two definitions of anything."* The helpers were put in `appearance` in §536 only because that is where the bug being fixed was, which is a reason about the diff, not about where the code belongs.

`readable_on` stays in `appearance` and stays separate. It answers the same question with the *palette's* near-black and near-white instead of pure black and white, because §532 requires that ink on an accented surface belong to the palette. Now that the two share their arithmetic, the only thing that differs between them is which pair of inks they choose from — which is the real distinction, and is now the only one.

### `is_dark`

Redefined as `contrast_text(color) == Color::WHITE` rather than given a corrected threshold of its own. Two functions with two thresholds can disagree about a colour; one defined in terms of the other cannot. The name still reads as a question about brightness and the definition is now a question about legibility, which is a mismatch — but legibility is the only question it has ever been asked, and a precise answer under an imprecise name beats the reverse. The doc comment states the crossover explicitly so the next reader does not have to rediscover that it is not 0.5.

Deleting it was the alternative. It has no callers, so nothing would break. Keeping it costs two lines and gives the "someone re-adds a threshold" failure a place to be caught, which `is_dark_and_contrast_text_cannot_disagree` now does.

### Cost paid

**None in behaviour, and that is itself the finding.** `contrast_text` and `is_dark` had no production callers — only six tests of their own, between them checking `#14141E`, `#F0F0F0`, pure black, pure white, and the two Catppuccin backgrounds. All six are nowhere near the 0.179 crossover, so all six passed before this change and pass after it, unchanged. They were tests that could not fail for the reason they were named — which is why the defect survived in a file that looked well tested.

The `known-issues.md` entry filed the day before had predicted "a run of pinned-colour test failures" from "a crate with many widget callers", and deferred the work partly on that basis. Nobody had grepped for a caller. The entry is kept, marked FIXED, with the wrong prediction left visible and labelled: **a blast radius that was estimated rather than measured has to say which it is**, or a later reader treats a guess as a finding and prices the work off it.

### Rejected

- **Fix `contrast_text` in place, leave `appearance`'s copy alone.** Cheaper diff, and leaves the tree with two implementations of the WCAG curve — the exact condition that produced this bug. The whole point of §534 is that the second copy is the defect, not its symptom.
- **Move the helpers to `guitk::color` instead of `guitk::theme`.** Defensible — a colour metric is arguably a property of the colour type. But `contrast_text` and `is_dark` are in `theme`, `color.rs` is currently pure data and conversion, and splitting the metric from its only two consumers buys nothing. If `color.rs` ever grows other metrics, move all of them together.
- **Have `appearance` keep thin wrappers with its own doc comments.** The docs read better attached to the crate a shell author is already in. But a wrapper is an editable body, and an editable body is where a divergence starts; the re-export makes divergence a compile-time impossibility. `readable_on`'s doc carries the shell-facing explanation instead.
- **Widen `contrast_text` to return the palette extremes.** That is `readable_on`, and it is already written. The toolkit has no palette and must not acquire one.
