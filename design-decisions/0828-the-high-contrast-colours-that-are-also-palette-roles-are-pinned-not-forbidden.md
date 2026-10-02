## 828. The high-contrast colours that are also palette roles are pinned, not forbidden

**Date:** 2026-09-09
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** a test used to check that none of the high-contrast colour schemes
happened to use exactly the same colour as any normal theme colour. Choosing
pure black for the light theme's text (§826) made that false — black is now both
the ordinary text colour and the background of three high-contrast schemes. The
test now checks for *exactly which* overlaps exist rather than for none, so the
four unavoidable ones are written down and a new one still fails.

**Why neither side can move.** `#000000` is the operator's choice for main text.
A scheme named "white on black" cannot be given a black that is not black. And
these draws cannot be routed through `Palette::text` instead: that role is black
only in light mode and near-white in dark mode, so a high-contrast scheme built
on it would invert itself along with the theme and stop being a high-contrast
scheme. The overlap is real and permanent.

**What the test was protecting.** `a11y.rs`'s header states the rule: the
high-contrast values are *pinned by an exact hand-written table rather than
merely excused*, because an exemption with nothing behind it is a region the
sweep stops looking at. The old assertion supported that by proving the pinned
values were disjoint from the palette.

**Alternatives.**

| option | *What changes* | why not |
|---|---|---|
| Delete the test | nothing observable; the guard is gone | it is the only thing keeping the exception honest |
| Skip black specifically | the test passes; black is never checked again | this is the "exemption with nothing behind it" the module forbids by name |
| **Pin the exact set** | the test names the four overlaps and fails on a fifth | chosen |

Pinning is strictly stronger than the emptiness check it replaces: making
`LIGHT_BASE` pure white, which would collide with `BlackOnWhite`'s background,
fails the test today and would have failed it before. What changed is that the
four permanent overlaps are now recorded with their reason instead of standing
between the guard and a green build.

**Found by:** running `cargo test -p desktop` after §826, which I had not done —
§826 changed a palette that four crates read and I tested only the crate I had
edited. The failure was sitting on `main`.
