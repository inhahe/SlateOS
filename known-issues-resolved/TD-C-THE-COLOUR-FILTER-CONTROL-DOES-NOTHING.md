## `TD-C-THE-COLOUR-FILTER-CONTROL-DOES-NOTHING` (lane C, 2026-09-07) -- **CLOSED the same day**

**Closed 2026-09-07 (lane C).** Choosing a filter now filters the screen.
`AppearanceSettings` carries `color_filter`, the compositor reads it, and
`Server::show` applies it to the frame on its way to the display.

**Where it is applied, and why there.** At the hand-off to the display, over
the whole buffer -- not during composition. Composition writes only the
damaged rectangles, so a filter applied there would leave the rest of the
screen unfiltered, and re-filtering a region that was already filtered would
compound on every frame. The hand-off is the one point that sees every pixel
exactly once. `filtering_does_not_touch_the_composed_frame` shows two
successive `show` calls leave the compositor's own buffer alone, which is what
makes that safe.

**It costs nothing when it is off.** `ColorFilter::None` returns the pixel it
was handed without unpacking it, and `show` hands the composed frame straight
over without copying; `no_filter_means_no_buffer_is_allocated` checks the
scratch buffer stays empty. That matters -- a full-buffer matrix multiply at
1920x1080 and 60 Hz is not free, and nobody who has not asked for a filter
should pay for one.

**`apply_argb` is an encoding of `apply`, not a second implementation.** A
framebuffer needs packed pixels; writing the unpack/repack at the call site
would have put a second definition of the filter in the compositor, free to
drift. `the_packed_filter_agrees_with_the_unpacked_one` runs both over every
channel value of every filter.

**Stored names are separate from labels** (`yaml_name` / `from_yaml_name`), so
rewording a caption cannot silently change what an existing config file means.

11 tests across the three crates. Mutation-checked at both ends: removing the
filter branch in `show` fails two compositor tests, and making the dropdown
write nothing fails the settings one.

The de-duplication described below is what made this reachable at all -- the
enum the dropdown used had no transform behind it. `apps/magnifier` still has
its own for the reason given there, and that remains open.

Original entry follows.

---


**In short:** Settings -> Accessibility -> Visual has a "Color Filter" list
offering Grayscale and the three colour-blindness filters. Choosing one
changes nothing on screen. The value is kept in a variable belonging to the
Settings window and is read by nothing else, so a colourblind user selects
"Deuteranopia" and the display carries on exactly as before.

**Found while checking a claim I had made about something else** -- that the
orphan-module ratchet was one island short. Chasing why the scanner thought
`accessibility_settings.rs` was reachable turned up the names it shares with
other files, and those names turned out to be four *different* enums.

**The duplication, now fixed.** `ColorFilter` was defined four times and the
four disagreed:

| Where | Variants | Null variant | Transform? |
|---|---|---|---|
| `gui/desktop/src/a11y.rs` | 6, incl. `Inverted` | `None` | **yes** -- channel matrices and `apply` |
| `gui/desktop/src/accessibility_settings.rs` | 5 | `Off` | no |
| `apps/settings/src/main.rs` | 5 | `None` | no |
| `apps/magnifier/src/main.rs` | 9 | `None` | its own |

They disagreed on the null variant's name, on membership, and on the spelling
of grayscale (`Grayscale` against magnifier's `Greyscale`). Only one of them
could actually transform a colour, and it was in a module nothing calls; the
one the user's dropdown was bound to was a list of labels.

The definition now lives once, in `gui/appearance` beside the palette, with
its matrix machinery and its fourteen tests. `a11y.rs` and
`accessibility_settings.rs` re-export it; `apps/settings` imports it.

**`apps/magnifier` was resolved on 2026-09-07 too, and differently.** Its
nine variants glue two concepts together, so a straight substitution was the
wrong fix; what it needed was to keep the *menu* and give up the *arithmetic*.
It is now `LensMode`, and its `apply` delegates: five modes to
`appearance::ColorFilter`, three to `appearance::HighContrastScheme`'s two
colours, one to the identity. Its Brettel matrices, its `mix`, and its literal
`(255, 255, 0)` for yellow-on-black are gone. Two tests assert the lens agrees
with the system filter it names -- which nothing could have checked before,
because neither side knew the other existed. `mix`'s only remaining caller was
its own test, so both went; the shared `ChannelMix` checks the same invariant
at compile time.

**The variant-name swap that fell out of it.** `HighContrastScheme::BlackOnWhite`
drew *white* text on black -- the names read backwards, inherited from the
module the enum came from, and documented as such. That was survivable until
the magnifier had to map onto them, because `apps/magnifier` has its own
`WhiteOnBlack` meaning white-on-black, and a mapping written by name would
have picked the opposite scheme in silence. The two variants were swapped; the
stored `yaml_name` strings did not move, so existing config files still mean
what they meant.

**The original entry's text follows.** Its nine variants glue two concepts
together. `Inverted`, `Protanopia`, `Deuteranopia`, `Tritanopia` and
`Greyscale` are colour-vision filters, but `YellowOnBlack`, `WhiteOnBlack` and
`GreenOnBlack` are high-contrast *schemes* -- the same three that
`HighContrastScheme` now names. Folding all nine into the shared enum would
put display schemes into a colour-filter type; folding only the five would
leave the magnifier with two enums to consult. Unpicking it is a change to
that app's model, not a substitution, so it is left and recorded here.

**What is still not done: nothing applies the filter.** This entry is not
closed by the de-duplication. A colour filter is a per-pixel transform of the
finished frame, so unlike the high-contrast palette it cannot be delivered by
`Palette::from_settings` -- and it must not be half-delivered by filtering
palette colours alone, because then the window chrome would shift and the
photographs would not, which is worse than doing nothing.

**The proper fix**, in order:

1. Apply it at **present** time, over the whole buffer, rather than during
   composition. The compositor composes damaged rectangles only; a filter
   applied per-rect is correct but must be applied to every pixel written,
   and doing it once at the hand-off to the display is simpler to reason
   about and impossible to apply inconsistently.
2. **Skip the pass entirely when the filter is `None`**, which is nearly
   every user, so the cost of the feature is zero for them. A full-buffer
   matrix multiply at 1920x1080 and 60 Hz is not free and should not be paid
   by people who have not asked for it.
3. Only then add `color_filter` to `AppearanceSettings` and point the
   dropdown at it. **Not before** -- the setting and the code that reads it
   should land together. This tree has six controls that were wired to fields
   nothing read; five were found in the last two days, and adding a seventh
   while fixing the sixth would be a poor joke.

**How you would notice.** Settings -> Accessibility -> Visual -> Color
Filter, choose Deuteranopia. The label changes; the screen does not.
