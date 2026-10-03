## §476 — A slider is one mapping read in two directions, measured from the track the page says it drew

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** all eight sliders in the Settings app painted their value
correctly and none of them could be moved with the mouse — there was nothing
to click. Making them drag could have been done by giving each slider row a
click band and doing the arithmetic where the mouse arrives, which is how the
dropdowns were done before §475 and is why three of them opened nothing. It was
done instead by naming each slider, stating its range in exactly one place, and
asking the page itself where the track was painted; the value a drag produces
and the position the handle is drawn at are now the same formula read forwards
and backwards.

### What was wrong

`PageSink::slider_row` registered no hit band, with a comment saying as much.
Every page also converted its own field into a 0–1 handle position by hand at
the call site:

| Slider | The conversion the page wrote out |
|---|---|
| Volume, Input volume, per-app volume | `percent_norm(self.output_volume)` |
| Text size | `(f32::from(self.text_size_percent) - 50.0) / 200.0` |
| Defer feature updates | `f32::from(self.defer_feature_days) / 365.0` |
| Defer quality updates | `f32::from(self.defer_quality_days) / 30.0` |
| Colour temperature, voice rate | the field, already 0–1 |

and formatted the number beside it from the same field a second time
(`Some(&format!("{}%", self.text_size_percent))`). Nothing yet read those
ranges in the other direction, because nothing could set a slider — so the
moment a drag existed, the range would have been written down twice, in two
directions, in two places, which is the drift §475 was written to stop.

### The decision

Four pieces, none of which knows where anything is drawn:

- **`SliderId`** names the eight sliders (`AppVolume(usize)` for the indexed
  ones) and `SliderId::range()` states each one's low and high value **in the
  units the state stores it in** — percent, days, or a bare fraction. This is
  the only statement of a slider's range anywhere.
- **`slider_fraction` / `set_slider_fraction`** on `SettingsState` are the two
  directions of that one mapping, and `slider_raw` / a `match` on the id are
  the only place a `SliderId` meets the field behind it — the shape
  `toggle_mut` already has for the switches.
- **`slider_track(x, y)`** is the single placement. `render_slider` paints from
  it, `slider_band` widens it into a grab target, and `AnchorId::Slider` reports
  it. A drag then measures `(mx - track_x) / SLIDER_WIDTH` from the track *the
  page said it drew*, not from a constant recomputed at the mouse handler.
- **`dragging: Option<SliderId>`** holds the gesture. `Move` re-derives the
  track every event, so a drag survives the page redrawing underneath it.

`AnchorSink` — which §475 introduced to answer "where is this dropdown's
button?" — was generalised to an `AnchorId` so it answers "where is this
control?" for both. That is one sink and one walk rather than two nearly
identical ones.

### What was given up

- **A page walk per mouse-move while dragging.** The track is re-derived on
  every `Move` event instead of being captured once at press time. Caching it
  in `dragging` would be one walk per gesture rather than one per event — but
  it would also be a coordinate held across redraws, which is precisely the
  stale-geometry bug this design exists to prevent. A page walk is a few dozen
  arithmetic ops and allocates nothing; a mouse move is not a hot path.
- **The grab band stops short of the row label.** Every other control here uses
  a whole-row target (§474), and a slider deliberately does not: a press
  anywhere in the band jumps the value to the pointer, so a whole-row band
  would mean brushing the words "Text Size" slams it to 50%. The band is the
  track plus a handle's radius at each end, full row height.
- **Rounding is now visible.** The integer-valued settings round to the nearest
  whole percent or day, so the handle can sit up to half a step from where the
  pointer released. That is inherent in storing a `u8`, and the test allows
  exactly half a step and no more.

### Alternatives rejected

- **A click band per slider row, arithmetic at the mouse handler.** The obvious
  version, and the one §475 had just finished removing from ten dropdowns and
  eleven other controls. It puts the track's position in the handler and the
  track's drawing in the renderer, which is the same defect wearing a different
  hat.
- **`RowHit::Slider` carrying the track rectangle.** Considered, and it is what
  the original known-issues entry proposed. Rejected because it puts `f32` in a
  type the tests compare with `==` (it would lose `Eq`), and because the answer
  it carries is a snapshot: `AnchorId` gets the same information from the live
  page for the same cost.
- **Keeping `percent_norm` and adding a matching `norm_percent`.** Two
  functions per unit, growing with each new slider, and no structural reason
  the pair stays inverse. `range()` plus one clamp covers all eight.

### How it is held

Seven tests, each verified to fail against a deliberately broken version:

| Mutation | Caught by |
|---|---|
| grab band drifts one row from its own track | `test_a_press_on_the_painted_track_grabs_the_slider` |
| `TextSize` range loses its `50.0` offset | `test_a_slider_readout_agrees_with_its_handle` |
| drag maps to half the track's width | 5 tests |
| `Move` no longer follows a held slider | 5 tests |
| every per-app drag writes app 0 | `test_dragging_one_per_app_volume_leaves_the_others_alone` |

Note which test catches the wrong *range*: not the one that drags to each end,
because that test asks `range()` what the ends are and so moves with the
mutation. It is the readout test, which spells out `"50%"` and `"250%"`
literally. A test that derives its expectation from the code it is testing
proves only self-consistency — the same trap the pre-§475 tests fell into by
recomputing row positions.
