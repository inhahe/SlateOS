### TD-C-TEXT-ON-THE-LIGHT-THEMES-TWO-PALEST-SURFACES-IS-BELOW-THE-CONTRAST-FLOOR — 2026-08-24 — **CLOSED 2026-09-11 by §826**

**Closed.** The operator chose `#000000` for the light theme's main text
(`design-decisions.md` §826). Every figure in the table below is superseded;
measured again through `palette_check::text_on_background` on 2026-09-11:

| card | light, then | light, now |
|---|---|---|
| `base` | 7.06 | 18.57 |
| `mantle` | 6.57 | 17.27 |
| `crust` | 6.04 | 15.87 |
| `surface0` | 5.17 | 13.60 |
| `surface1` | **4.39** | **11.55** |
| `surface2` | **3.69** | **9.71** |

**It was fixed on 2026-09-09 and nobody noticed until 2026-09-11.** The test
that pinned this entry --
`palette_check::tests::the_two_pale_surfaces_measure_what_the_known_issue_says_they_do`
-- went red the moment §826 landed, because it asserted the *defect*: that
`surface1` measures 4.39 and is under the floor. It stayed red and unseen for
two days, because `palette_check` sits behind the `testing` feature and
`cargo test -p appearance` does not enable it. It surfaces under
`cargo test -p appearance --features testing`, or in any invocation that also
builds a crate which enables that feature -- feature unification then turns it
on for `appearance`'s own test binary.

**Exactly two crates do:** `gui/compositor` and `gui/desktop`, both as
dev-dependencies. `apps/settings` does *not* -- it enables
`settingsfile/testing` only, which is easy to misread as the same thing.
Measured rather than assumed, because the first version of this paragraph said
"a crate that enables it" and I could not have named which:

| invocation | tests in `appearance` |
|---|---|
| `-p appearance` | 102 — `palette_check` absent |
| `-p appearance -p settings` | 102 — still absent |
| `-p appearance -p guitk -p compositor` | **114** — present, and red |

So `cargo test --workspace` *does* catch it, since `compositor` and `desktop`
are members. What does not is the per-crate command this file's own workflow
recommends, which is what was being run.

**Two things worth carrying from that.** A test written to pin a defect becomes
a false alarm the day the defect is fixed, and reads as a regression rather than
as success -- it is now rewritten to assert the *fix*, so a return under 4.5
reopens this entry by name. And a test behind a feature flag nobody passes
reports nothing at all; `cargo test -p <crate>` is not the same command as
`cargo test --workspace` and the difference is invisible until it matters.

The original entry follows, unaltered, because its survey is still the record of
how the measurement was made.



**In short.** The desktop's light theme has six background shades a card or
panel can be painted. On the two palest of them, ordinary text is below the
4.5:1 contrast ratio that readable body text is supposed to reach. (Contrast
ratio: how far apart two colours are in lightness; 1:1 is invisible, 4.5:1 is
the standard floor for text.) Nothing is unreadable, but text on those two
surfaces is measurably harder to read than the theme claims. The dark theme
does not have the problem.

**Where:** `gui/appearance/src/lib.rs`, the Latte (light) role table. Measured
2026-08-24, `text` against each card colour:

| card | dark | light |
|---|---|---|
| `base` | 11.34 | 7.06 |
| `mantle` | 12.14 | 6.57 |
| `crust` | 12.97 | 6.04 |
| `surface0` | 8.69 | 5.17 |
| `surface1` | 6.31 | **4.39** |
| `surface2` | 4.62 | **3.69** |

**Found by:** `gui/desktop/src/slider.rs`'s
`the_thumb_is_legible_against_every_card_it_can_sit_on`, which was originally
written with a 4.5 floor and failed on light `surface2` at 3.69:1. The slider
test now asserts 3:1, which is the criterion that actually applies to it (WCAG
SC 1.4.11 *Non-text Contrast*, for a graphical object that identifies a
control, as opposed to SC 1.4.3's 4.5:1 for text). That is the right floor for
a 12-pixel disc and the *wrong* floor for a label, so the finding is logged
here rather than absorbed into the slider's test.

**The proper fix** is to darken Latte's `text` (`#4C4F69`) until it clears
4.5:1 on `surface2` (`#ACB0BE`) — roughly `#3C3F55` or darker — or to rule that
`surface1`/`surface2` are not permitted as card backgrounds in light mode and
enforce that. The first is a change to a published palette that the whole shell
and every ported app reads; the second is a rule with no mechanism behind it
today. Both are wider than one module, which is why this is logged rather than
done.

**Whether anything actually draws text on those surfaces has not been
surveyed.** That survey is the first step of any fix, and it may show the
answer is "nothing does," in which case this becomes a latent hazard rather
than a live defect. Do not assume either way from this entry.

**If never fixed:** light-mode users reading a settings row on a `surface1` or
`surface2` card get slightly-too-low contrast. It does not get worse over time,
but it gets *wider* every time a panel picks one of those two surfaces for a
card, since nothing today stops it.

### Surveyed 2026-09-03 — the survey this entry asked for, and it is worse than stated

The entry above says the survey "has not been done", that it is "the first step
of any fix", and that it "may show the answer is 'nothing does'". It has now
been done. The answer is not "nothing does", and the defect is **wider than the
`text`-on-`surface1`/`surface2` pair this entry describes**.

**How it was measured.** `gui/desktop/src/palette_check.rs` gained
`text_on_background(cmds, root)`, which walks a command list in paint order,
tracks the translate and font stacks, composites every earlier `FillRect`
covering the sample point with `Color::over`, and returns each text command's
ink, the colour actually behind it, and their WCAG ratio. `assert_drawn_from`
was then *temporarily* instrumented to report every finding under 4.5:1, which
made all 49 modules' existing sweeps report at once without editing 49 call
sites. The instrumentation was removed before commit; `text_on_background` and
its six tests stayed.

**The table this entry gives is one row of the real table.** It measured only
`text`. Every ink the shell draws body copy in, against every card:

| light mode | base | mantle | crust | surface0 | surface1 | surface2 |
|---|---|---|---|---|---|---|
| `text` | 7.06 | 6.57 | 6.04 | 5.17 | **4.39** | **3.69** |
| `subtext1` | 5.53 | 5.14 | **4.73** | **4.05** | **3.44** | **2.89** |
| `subtext0` | **4.64** | **4.31** | **3.96** | **3.40** | **2.89** | **2.42** |
| `accent` (blue) | **4.63** | **4.31** | **3.96** | **3.39** | **2.88** | **2.42** |
| `overlay0` | 2.30 | 2.14 | 1.97 | 1.69 | 1.43 | 1.20 |

(Bold = under the 4.5:1 body-text floor. `overlay0` is not bold because the
palette documents it as deliberately not carrying body text — placeholder and
disabled text, which SC 1.4.3 exempts.)

**The finding in one sentence: the light palette's secondary and accent inks
clear the floor on `base` and nowhere else.** `subtext0` is 4.64:1 on the base
it was tuned against — design-decisions §525 darkened it to exactly that — and
that tuning was done against `base` alone. Raise the ink onto any card and it
fails: 3.40:1 on `surface0`, which is the ordinary case of a caption on a card,
and *worse* than the 3.69:1 this entry flagged as the worst case.

**And the shell does draw there, in quantity.** Distinct (module, ink, card)
findings from one full pass, light mode:

| pair | sites | some of the modules |
|---|---|---|
| `subtext0` on `surface0` | 533 | AppsList, accessibility_settings, datetime tabs, sound, power, storage, update, bluetooth |
| `overlay0` on `surface0` | 140 | (exempt — placeholder/disabled) |
| `accent` on `surface0` | 126 | launcher, network, backup |
| `text` on `surface1` | 96 | launcher results, device_settings, clipboard viewer, language_settings |
| `subtext0` on `surface1` | 92 | network flyout/panel, device_settings, clipboard viewer |

So this is a **live defect, not a latent hazard**, and the two surfaces named in
the heading are not the main event: `surface0` is.

**Dark mode is clean**, as the entry says. Every real finding in the dark pass
was one of the two artefact classes below.

#### Two artefact classes the survey turned up, neither a defect

Both are recorded because a later reader running the same sweep will see them
and must not "fix" them.

1. **Text on the wallpaper is not text on `p.base`.** The sweep takes a `root`
   colour for what lies beneath the module's own drawing, and `p.base` is right
   for a panel that fills its window. It is wrong for `icons`, whose labels sit
   on an arbitrary photograph under a black shadow and are therefore pale in
   *both* modes on purpose (`Palette::on_wallpaper`; see the note at the top of
   `icons.rs` and `an_icon_label_does_not_change_colour_with_the_mode`). With
   `root = p.base` in light mode the ink and the root are the same value and the
   sweep reports 1.00:1 — the loudest finding in the whole run, and entirely an
   artefact of the assumption. The same applies to the login screen's clock over
   its wallpaper (1.17:1 reported). **Any per-module assertion built on this
   helper must pass the right `root`, or exempt the modules that draw on the
   wallpaper.**

2. **Tests that build a light palette with a dark accent.** Several sweeps
   iterate `AccentColor::presets()` and set `p.accent` to `accent.color()` — the
   *Mocha* value — rather than `p.hue(accent)`. That yields e.g. Mocha pink
   `#F5C2E7` as ink on a light `surface0` at 1.01:1. It cannot happen to a user:
   `AppearanceSettings::effective_accent` picks `color_light()` in light mode.
   Checked, not assumed — `gui/appearance/src/lib.rs:699`. The tests are
   overstating their coverage rather than the shell being wrong, and that is its
   own small defect, close kin to
   `TD-C-THREE-TEST-MODULES-HAND-ROLL-THE-ACCENT-LIST-THAT-A-HELPER-ALREADY-RETURNS`.

   One real hazard does sit next to it: `effective_accent` returns
   `custom_accent` **unmodified** for `AccentColor::Custom`, with no mode
   adjustment and no legibility check. A user who picks a pale custom accent in
   light mode gets exactly the 1.01:1 result the artefact simulates. That is a
   separate question and is filed in `open-questions.md`.

#### Why the fix is not applied here

The entry's two candidate fixes are still the right two, but the survey changes
which is cheap. Darkening `text` alone — what the entry proposes — fixes two
cells of a table with fourteen failing ones, and leaves the 533-site case
untouched. The real choice is a palette-wide one, it changes what every light
render looks like, and it is a user-visible policy rather than a bug with one
correct answer. It is therefore in `open-questions.md` as **C-Q10** with the
options costed, rather than decided here.

**What did land** is the measuring instrument and its tests, so that whichever
option is chosen can be checked rather than asserted, and so the numbers above
can be reproduced by anyone.
