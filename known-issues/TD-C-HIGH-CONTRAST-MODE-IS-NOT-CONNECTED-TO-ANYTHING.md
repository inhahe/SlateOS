## `TD-C-HIGH-CONTRAST-MODE-IS-NOT-CONNECTED-TO-ANYTHING` (lane C, 2026-09-07) -- **mostly CLOSED the same day**

**Update 2026-09-07: high contrast now reaches the screen.** The scheme lives
in `gui/appearance` as `HighContrastScheme`, `AppearanceSettings` carries
`high_contrast: Option<HighContrastScheme>` with a YAML round-trip, and
`Palette::from_settings` branches on it -- the single construction point every
shell surface already goes through, so the mode applies everywhere at once
rather than needing each caller taught about a second palette.
`a11y.rs`'s copy of the enum is now a re-export of that one, so the two sets of
colour values that could disagree are one set.

**The open design point is settled: the accent follows the user's setting.**
`design-decisions.md` §816 requires the highlight to be configurable, and a
scheme-fixed accent would have made it the one colour this mode does not let
you change. The contrast risk that argued for the scheme is handled without
overriding anyone: for a named accent the *hue* is kept and the
better-contrasting of its two existing values is used, which is what
`for_mode` already does for every other role. A `Custom` accent is used
verbatim -- an exact colour is an exact request, and there is no second value
to choose between.

Removing the accent from the scheme removed a guarantee (`accent >= 4.5:1`
against its own background, four values checked). It is replaced by a wider
one: `the_worst_accent_on_the_worst_scheme_is_still_legible` sweeps all
fourteen presets against all four schemes. A second test asserts that the
variant choice is what achieves it -- the same sweep against each hue's dark
value alone finds a pairing that fails -- so the mechanism is shown to be
load-bearing rather than incidental.

**What still stands from the entry below:**

- **The duplication is only half-resolved.** `accessibility_settings.rs`
  (2,037 lines) still models contrast a third way, and the keyboard-
  accessibility halves of both modules still overlap. Neither is reachable.
  Both are lane C, so this needs no cross-lane agreement.
- ~~**There is still no control.**~~ **Done 2026-09-07.** Settings ->
  Accessibility -> Visual has a High Contrast row. It turned out a row was
  already *there* -- a switch bound to `ToggleId::HighContrast`, writing to a
  `high_contrast: bool` on the settings app's own state that nothing read.
  The control and the reader both existed and were not connected.

  It is now a list of five (Off, plus the four schemes) rather than a switch
  plus a scheme picker. A switch over a setting with four values has to answer
  "on to what?", and either forgets the user's scheme or hides it in state
  they cannot see -- which is the argument this same page already made for
  Transparency, in a comment: "a switch that meant 'Off or whatever it was'
  would forget a user's choice of Full every time they turned it off and on
  again." The dead bool and `ToggleId::HighContrast` are gone.

  Seven tests, ending with one that carries the choice all the way into
  `Palette` rather than stopping at `AppearanceSettings` -- a setting that
  round-trips and is never consumed is exactly the defect this row had for its
  whole life.
- **The orphan ratchet claim needs qualifying.** I wrote that
  `accessibility_settings.rs` is an island missing from the baseline.
  `scan-orphan-modules.py` counts a module as reached if any of its item
  names is mentioned elsewhere, and this module's names -- `ColorFilter`,
  `MagnifierConfig`, `InputSettings` -- were all mentioned elsewhere. They
  were **homonyms**: separately-defined types of the same name in other
  files, not references to these. So the scanner was not simply wrong, it was
  defeated by four enums sharing a name, which is itself the defect (see
  `TD-C-THE-COLOUR-FILTER-CONTROL-DOES-NOTHING`). With `ColorFilter` now
  defined once, one of the three homonyms is gone. Whether the module is
  flagged now has not been re-checked, and the scanner is lane A's file, so
  this is a report rather than a fix.

Original entry follows.

---


**In short:** SlateOS has four high-contrast colour schemes for users who
cannot read ordinary ones, and there is no way to turn any of them on. The
code that defines them is complete and tested and is called by nothing. There
is also a *second*, separate accessibility module that models the same feature
differently, and it is equally unconnected. A user who needs high contrast
gets the ordinary theme.

**How this was found.** Acting on `design-decisions.md` §816, which says the
green-on-black scheme's highlight becomes white and that the highlight must be
user-configurable. Both halves turned out to mean something other than what
they appear to:

- **The configurability half is already satisfied**, for everything actually
  on screen. The live highlight is `Palette::highlight_fill`, which is
  `with_alpha(self.accent, ...)` -- derived from the accent, which the user
  already picks in Appearance settings. Nothing needed building.
- **The white half lands in code nobody runs.** `HighContrastTheme::accent` in
  `gui/desktop/src/a11y.rs` has no caller outside its own tests.

The colour was changed anyway, because the decision is recorded and the value
will be right when it *is* wired -- but on its own it changes nothing a user
sees, and saying so is the point of this entry.

**Where it lives, and the duplication.**

| Module | Lines | Models | Reachable |
|---|---|---|---|
| `gui/desktop/src/a11y.rs` | 2,291 | `HighContrastTheme` (4 schemes), colour filters, magnifier, sticky/filter/mouse keys, `AccessibilityConfig` | no -- pinned island #53 |
| `gui/desktop/src/accessibility_settings.rs` | 2,037 | `ContrastMode` (`HighContrast`, `HighContrastInverse`), sticky/filter/mouse key configs | no -- `pub mod`, no callers |

Both are `pub mod` in `gui/desktop/src/lib.rs` and neither is referenced
anywhere else. **Both are in lane C**, so unlike the desktop-icon duplication
(`requests/c-a-two-desktop-icon-models-...`) this one needs no cross-lane
agreement -- it is mine to settle.

**A side finding about the ratchet.** `a11y.rs` is on
`scripts/orphan-modules-baseline.txt`; `accessibility_settings.rs` is not,
although by the same test it is equally an island. `scan-orphan-modules.py`
reports "no new islands (45 pinned)" and does not flag it. The scanner's
reachability appears to be name-based -- it reports `a11y.rs` as "also spelled
in" three files that do not import it -- so a module whose *item names* occur
elsewhere reads as reached. Not chased further; noted because the baseline is
used as evidence that nothing new has been stranded, and here it is one short.

**What the proper fix is**, and it is not "wire up `a11y.rs`":

1. **Pick one model and delete the other.** `accessibility_settings.rs`'s
   `ContrastMode` is the smaller and more honest shape (an enum of modes, not
   a palette), but `a11y.rs`'s four named schemes are what the operator's
   question C-Q7 was about and what §816 decides. Neither is obviously right.
2. **Put high contrast where the palette is built.**
   `Palette::from_settings` is a four-line function and the *single*
   construction point every shell surface goes through. A
   `high_contrast: Option<...>` field on `AppearanceSettings`, branched on
   there, makes the feature apply everywhere at once. Wiring the island
   instead would mean teaching every caller about a second palette.
3. **High contrast collapses the palette's gradations rather than shifting
   them.** `crust`/`mantle`/`base`/`surface0..2` all become the background and
   structure is carried by borders, which is what Windows' high contrast does
   and why it works; `overlay0`, documented as "the faintest legible mark" at
   3.4:1, must become the text colour, since a deliberately faint role is
   exactly the thing the mode exists to remove. `panel_alpha` goes to opaque.
4. **The categorical hues should not collapse.** "Red means failed" has to
   survive, so those fields come from whichever mode's palette suits the
   scheme's background rather than being flattened to the text colour.

**The one open design point**, which is why this is written down rather than
already done: **in high contrast, does the accent follow the user's Appearance
setting, or the scheme?** Following the setting satisfies §816's
configurability requirement directly and needs no new machinery, but lets a
user pick a highlight with poor contrast against the scheme's background --
in the mode where that matters most. Following the scheme guarantees contrast
and makes §816's white the visible default, but means the highlight is the one
colour high contrast does *not* let you change. A third option is to follow the
setting only while it clears a contrast bar, which is defensible and is also
the system silently overriding a user's explicit choice.

**How you would notice.** Look for high contrast in Appearance settings. There
is no control, and there is no setting behind it if there were.
