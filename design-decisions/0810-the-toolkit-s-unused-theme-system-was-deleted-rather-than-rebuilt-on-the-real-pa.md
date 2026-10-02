## 810. The toolkit's unused theme system was deleted rather than rebuilt on the real palette

**Date:** 2026-09-03
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The widget toolkit contained a complete second system for
deciding what colour everything on screen should be — about 450 lines: a
`Theme` struct of thirty-two colours, four ready-made themes written out as hex
numbers, a manager to switch between them, and change notifications. Nothing in
the operating system ever used it. The desktop's colours come from somewhere
else entirely (`appearance::Palette`, which reads the user's saved light/dark
and accent choices), and the unused copy had drifted: its light theme's
secondary text was a shade the *real* palette had already measured, found too
faint to read, and replaced. The choice was between rewriting the unused system
to read its colours from the real one, and deleting it. It was deleted.

### The situation as found

known-issues.md's `TD-C-THE-TOOLKIT-HOLDS-A-THIRD-COPY-OF-THE-PALETTE-AND-DISAGREES-WITH-ITSELF-ABOUT-IT`
proposed the rewrite, and both of the facts it rested on turned out to be
wrong. Checking them is what produced this decision, so they are worth stating:

1. **It said `appearance` sits *below* the toolkit.** It sits above:
   `gui/appearance/Cargo.toml` depends on `guitk`. So the proposed
   `Theme::from_palette(&appearance::Palette)` is a dependency cycle — not an
   awkward design, a build error.
2. **It assumed the type had users.** It has none. Across the whole tree, every
   file that imports `guitk::theme` imports a colour *function*
   (`with_alpha`, `contrast_ratio`, `relative_luminance`); not one imports
   `Theme`, `ThemeColors`, `ThemeManager` or `ThemeMode`. A blanket
   `#![allow(dead_code)]` at the top of the file is why the compiler had never
   said so.

### The options

| | *What changes* | Cost |
|---|---|---|
| **A. Delete it** (chosen) | Nothing on screen. `guitk::theme` becomes seven colour functions and no types. | The widget-role vocabulary (`scrollbar_thumb`, `border_focus`, `primary_hover`) is no longer written down anywhere. |
| **B. Move the palette down into `guitk`, rebuild `Theme` on it** | Nothing on screen. Two crates change instead of one. | Keeps 450 lines of unreachable code alive, and puts a second *live-looking* theming API next to the real one. |
| **C. Fix only the one illegible colour, leave the rest** | Nothing on screen. | Leaves the third copy, which is the actual complaint. |

**Why A over B.** B is the entry's own proposal, made workable: `Palette`
cannot move to `guitk` as it stands, because its `from_settings` and `hue`
methods need `AppearanceSettings` and `AccentColor`, and Rust's orphan rule
forbids `appearance` from adding inherent methods to a type `guitk` defines. So
B means moving the raw colour ladder down and re-exporting it upward — real
work in a live, heavily-tested crate — and what it buys is that unreachable
code becomes unreachable code with better colours in it.

Worse, it *creates* a hazard. Two theming APIs where one is real is a trap laid
for the next author, who reaches for the one in the toolkit — the obvious place
— and gets colours that the light/dark switch, the accent setting and the
contrast sweeps cannot reach. That is not a hypothetical failure mode; it is
precisely how this copy came to ship a `text_secondary` of `#6C6F85`, which
measures 4.37:1 on its own background against the 4.5:1 that body text needs,
while the crate next door had already rejected that exact value and used
`#686B80` (4.64:1). An unused API that is *wrong* is better than an unused API
that looks right.

**What A costs, honestly.** `ThemeColors` had a genuinely reasonable idea in it
— derived states (hover, pressed, disabled) computed once rather than at each
call site — and `Theme` named widget roles that `appearance::Palette`
deliberately does not carry. Deleting it means that when widget theming is
actually wired up, someone writes that vocabulary again. Two things make this
acceptable: it is in git, named in the deleting commit; and whoever writes it
will be deriving it from `appearance::Palette`, which is a different type from
this one, so what they would have inherited is a shape to copy, not code to
reuse. Keeping 450 unreachable lines so that a future author might save twenty
is not a trade worth making, and the years those lines sit there are years in
which they can drift further from the palette they are supposed to agree with.

**Why this was not put to the operator.** Nothing on screen changes — the code
had no callers, so it rendered nothing. It is reversible with `git revert`. And
the alternative is not a matter of taste but of a build error and a hazard. If
that reasoning is wrong, the thing to revisit is whether the toolkit should own
widget theming at all, which is a real question (see below) and a different
one.

### The related duplicate, removed with it

`theme::mix(a, b, ratio)` was `Color::lerp(self, other, t)` written a second
time, and the second one had a bug the first does not: `lerp` reads a NaN
factor as "no progress", `mix` passed NaN through because `f32::clamp` does,
and `NaN as u8` is 0 in every channel — so mixing at a NaN factor returned
transparent black instead of either end colour. A NaN factor is what an
animation gets the instant it divides elapsed time by a zero duration. Its only
non-test caller was `ThemeColors::disabled_state`, which went with the rest. A
comment now stands where `mix` did, so the next author reaches for the method.

This is the same failure as the palette, one function wide: two spellings of
one operation, and no mechanism to notice they had come apart.

### Reversing this

`git revert` restores the file. But the question worth reopening is not "should
`Theme` come back" — it is **"where should widget-level colour roles live once
widgets actually take a theme?"** That is genuinely undecided, and the answer
is probably a type derived from `appearance::Palette` that lives wherever the
widgets that consume it live. It is not this deleted struct, whose values were
hand-written and whose roles were named in comments that pointed one rung off
the values beside them. A test,
`this_module_names_no_colours_of_its_own`, now fails if a colour literal
returns to `guitk::theme`; that test is the thing to delete first if this is
ever reversed, and its failure message says where the palette lives instead.
