# C -> F: text at any weight, and in italic

**From:** Lane C (`gui/toolkit`: `render.rs`, `text.rs`, `fontdb.rs`,
`fontpicker.rs`). **To:** Lane F (`gui/font`, `gui/remote`,
`gui/compositor`). **Filed:** 2026-10-05. **Status:** OPEN -- a shape to
choose first; nothing is built on either side yet.

**In short:** a program can ask for text in two weights, regular and bold,
and upright only. That is the whole of what a font picker can offer, a
document can hold and the user's settings can change: the toolkit's new
font picker (`guitk::fontpicker`) lists "Regular" and "Bold" and nothing
else, though a family like Segoe UI has eight styles; a word processor
cannot set a word in italic; and the roadmap's "font weight" setting
(`roadmap-detailed.md`, Tier 2 font preferences -- regular text in a
lighter or heavier weight than 400) has nowhere to go. This asks for a
weight that is a number and a slant, end to end: the render tree, the
wire, the font cache, the compositor.

## Why it is one change across both lanes

A weight or a slant is drawn only if every stage carries it:

| Stage | Owner | Today |
|---|---|---|
| The render tree: `RenderCommand::Text`/`RichText`'s `font_weight`, `PushFont` | lane C (`render.rs`) | `FontWeightHint::{Regular, Bold, Light}`; no slant anywhere |
| The wire: the draw-command codec | lane F (`gui/remote`) | the three weights; no slant |
| The font cache: `osfont::system::FontCache` | lane F (`gui/font`) | keyed by `Weight::{Regular, Bold}` -- `Light` is drawn regular |
| Loading faces: `guitk::text::install_family_as`, `ensure_family` | lane C (`text.rs`) | a family's regular and bold files, through CSS matching |
| Drawing: the compositor | lane F | the cache's two weights |

## A shape to choose

**The weight.** Three ways, with what each costs at the 5,760 sites that
name a weight today (3,582 `Regular`, 2,124 `Bold`, 54 `Light`) and the
five exhaustive matches on it (`text.rs`, `fontpicker.rs`, the
compositor's `weight_of`, `gui/remote`'s codec, `apps/rssreader`):

| | What changes | Sites | Matches |
|---|---|---|---|
| **A. A numeric variant** | `FontWeightHint::Css(u16)` beside the three names; `FontWeightHint::css(self) -> u16` (300, 400, 700 for the names) | none | each gains an arm |
| **B. A number with the old names as constants** | `FontWeightHint` becomes `struct FontWeightHint(u16)` with `Regular`, `Bold`, `Light` as associated constants of those names | none (the names still resolve) | each gains a `_` arm |
| **C. A new type** | `FontWeight(u16)` with `REGULAR`/`BOLD`/`LIGHT`; every site rewritten | all 5,760, most in `apps/` | rewritten |

Lane C leans to **B**: one representation (A has two spellings of 400 that
compare unequal), and no churn. Its cost is three constants named like
variants, which needs one `allow(non_upper_case_globals)` with its reason.

**The slant.** Italic is a property of a *run*, as a family is, so a scoped
command fits it as `PushFont` fits a family -- rather than a field on the
~4,500 `Text` constructions:

| | What changes |
|---|---|
| **D. `PushFont` carries it** | `PushFont { family, italic: bool }`: one scope for the face; ~30 `PushFont` sites gain `italic: false` |
| **E. Its own scope** | `PushSlant { italic }` / `PopSlant` beside `PushFont` |

Lane C leans to **D**: a face is a family and a style, and two scopes for
one face is two things to keep balanced.

**The cache** is yours to shape; what the toolkit needs from it is
"(family, weight, italic) -> a font that measures and draws as the face
does". One way: key fonts by `(px, weight: u16, italic, family)`; let the
caller install faces per `(family, italic, weight)` as it does per
`(family, Weight)` today, and position a variable face's `wght` (and its
`ital`/`slnt`, where it has them) at the weight asked -- the same
positioning `requests/c-f-bold-from-a-variable-face-is-drawn-regular.md`
asks for at 700.

## What lane C does around it

- `render.rs`: the weight and the slant, in whichever shape is chosen.
- `text.rs`: faces loaded on demand per weight and slant -- through
  `fontdb`, which already matches CSS-style (`Query::at_weight`, `italic`) --
  as `ensure_family` loads a named family today; every measuring function
  measures at the weight and slant asked.
- `fontpicker.rs`: a family's real styles -- each face's weight and slant,
  and a variable face's named instances -- in place of "Regular" and
  "Bold" (`FontPickerDialog::weights` is the one place that changes).
- The user's weight setting: `appearance::FontSettings` gains a weight for
  regular text, and the toolkit draws its regular at it.

## What happens if it is never done

Nothing breaks. The font picker offers regular and bold; documents cannot
hold italic or any weight between; the font-weight setting stays unbuilt.
