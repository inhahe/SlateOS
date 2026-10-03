## §498 — Rounded rectangles are rasterized as scanline spans inside the compositor, with coverage carried on the existing opacity channel rather than a new backend primitive

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Lots of things on screen are supposed to have rounded corners —
buttons, tabs, menus, the window frames themselves. Every one of them was
asking for rounding, that request was being carried faithfully all the way
across to the display server, and then the very last piece of code — the one
that actually colours pixels — threw the request away and drew a square. So
nothing in the entire graphical system has ever had a round corner. This entry
records how the drawing was implemented and the three judgement calls it
needed.

**The bug.** `RenderEngine::execute_command` in `gui/compositor/src/lib.rs`
matched `RenderCommand::FillRect`, `StrokeRect` and `BoxShadow` and bound
`corner_radii: _` on all three — the Rust spelling of "there is a field here
and I am deliberately ignoring it". Producers were everywhere and correct:
`guitk`'s widget borders, the SVG renderer's `rx`/`ry`, the tab strip, the
launcher's search field, the power menu, the resource monitor, and the desktop
shell's `corner_radii()` fed from the user's `WindowCorners` setting. The wire
protocol serialized them faithfully (`gui/remote/src/lib.rs`'s `write_radii` /
`read_radii`). There is exactly one rasterizer, and it discarded them.

**This is the sweep's recurring shape yet again**, with a twist worth naming:
usually the tree holds one correct answer that callers cannot reach. Here every
*caller* was correct and the single implementation was the one that had quietly
opted out — and because a square corner is a perfectly plausible-looking
rectangle, nothing about the output announced that a setting was being ignored.
A user who chose `Square` and a user who chose `ExtraRounded` got identical
pixels.

### Decision 1 — spans in the render engine, not a new `RenderTarget` method

`RenderTarget` is the seam between the compositor and its backends (software
`Framebuffer`, `RenderBackend`, the test `Recorder`, and a GPU path later). A
rounded rectangle could have been a new trait method, letting a GPU backend
draw it in one shader pass.

| | Trait method | Scanline spans above the seam |
|---|---|---|
| Backends to implement | every one, now and future | none |
| GPU quality | ideal — analytic in a shader | quads, as good as the CPU path |
| Draw calls, 48×48 at r=12 | 1 | ~1 + 3 per corner scanline ≈ 76 |
| Risk of two backends disagreeing | real | none — one implementation |

Chosen: **spans above the seam.** The decisive argument is not the effort of
implementing it four times, it is that implementing it four times means four
chances for the arc to differ, and this codebase has just spent three entries
on bugs that were exactly "the same shape computed two ways". A GPU backend
that wants the shader version can add the trait method later as an
*optimisation of a behaviour that is already defined and tested*, which is a
much safer thing to add than a behaviour.

The cost is bounded by construction and that is what makes it affordable:
`RoundRect::new` returns `None` for any radius under half a pixel, so
`CornerRadii::ZERO` — which is what almost every command in the tree carries —
falls through to the flat `fill_rect` completely untouched. And the rounded
path only walks scanlines *in the corner bands*; the straight middle of a
window, which is nearly all of it, stays a single quad. A tall window's border
costs three quads plus its corners, not one per row.

### Decision 2 — coverage rides in on `opacity`, and only horizontal coverage is measured

An arc drawn to whole pixels is visibly jagged at the 16 px radius that
`WindowCorners::ExtraRounded` asks for. Antialiasing needs a way to say "this
pixel is 40% covered", and there was no coverage channel.

There did not need to be one. For a solid colour, a pixel 40% covered by an
opaque fill and a pixel fully covered by a 40%-transparent fill composite to
exactly the same result, and `RenderTarget::fill_rect` already blends by
opacity. So a 1×1 fill at `opacity × coverage` *is* an antialiased pixel, and
every backend already implements it. No new channel, no new method.

The approximation taken: **horizontal coverage only.** Each scanline of the
shape is one span, and the pixel at each end is blended by the fraction of it
the span covers. Where the arc runs steeply — the middle of each quadrant —
this is very nearly exact. Where it runs flat, at the extreme top and bottom of
a corner, it understates the smoothing. The exact alternative is per-pixel area
sampling over each corner's bounding box, which is `radius²` blends per corner
instead of two per scanline: 1024 blends for a single 16 px corner against 32.
At the three radii this codebase actually uses (4, 8, 16 — `WindowCorners`) the
difference is not visible, and the cost ratio is thirty-fold.

All of it goes through one function, `RenderEngine::fill_span_row`. Both the
fill and the outline reduce to spans and route through it, so a curve cannot be
smooth in a fill and jagged in the outline drawn on top of it.

### Decision 3 — the outline is "outer shape minus inner shape", sharing the fill's arc code

`stroke_round_rect` could have drawn four arcs of its own. Instead it builds
the inset shape with `RoundRect::inset_by` and, per scanline, paints the two
gaps between the outer span and the inner one.

The reason is the failure mode, not the line count: an outline whose curve is
computed differently from the fill it surrounds parts company with that fill by
a pixel somewhere along the arc, and the result is a border that floats free of
its own window at the corners. Sharing `RoundRect::span` makes that
unrepresentable. There is a test for it — the outline may not paint a single
pixel the fill does not cover — and it is a real one: reintroducing a
separately-computed outer curve fails it.

The straight middle is still coalesced into two vertical bars rather than walked
per row, with a guard for the case where a border thick enough to meet in the
middle would otherwise blend the overlap twice and leave a darker stripe.

### Decision 4 — a shadow's rounding grows with the shadow

`BoxShadow` is drawn as a solid rectangle expanded by `spread + blur`. Its
radii are now expanded by the same amount rather than used as-is, because a
shadow is a copy of its box pushed outward: at the same radius as the box it
would be visibly *squarer* than the thing casting it, and its corners would
stick out past the curve they are meant to sit behind.

### Two clamps that are not cosmetic

- **Overlapping radii.** A client picks the radii and is under no obligation to
  pick sensible ones. `RoundRect::new` applies CSS Backgrounds 3 §5.5: two radii
  sharing a side may not together exceed it, and when any pair does, *every*
  radius is scaled by the same worst-case factor — scaling only the offending
  pair rounds one corner of a small box and leaves its neighbour square, which
  reads as a bug rather than as a clamp. Without this, `r² - dy²` stays positive
  far outside the box and the span walks off the shape.
- **Corner bands that would cross.** The side rule above does not imply it:
  radii on opposite ends of a diagonal each satisfy it at a full `height` yet
  together span twice it. Unclamped, the middle quad's height underflows to
  roughly four billion rows.

### Verification

Twelve tests, and — as in §497 — each was put to a reintroduction of the bug it
claims to guard. The first pass caught 8 of 11 and the three misses were worth
more than the eight hits:

- **The hollow-outline test probed only the centre of the shape**, which lies
  in the straight middle band — code that coalesces into two bars and never
  consults the inner shape at all. An outline that had entirely forgotten to
  hollow itself out still left that pixel bare and passed. Fixed by probing a
  scanline that runs through the rounded corners too.
- **The "a tiny radius draws what the flat path draws" test cannot fail**, for
  the structural reason that a tiny radius takes the flat path — so it never
  enters the scanline code and no arithmetic error inside it can reach the
  assertion. It is kept, because what it does pin (the fall-through, on which
  every pre-existing decoration test depends) is worth pinning, but a second
  test was added that actually runs the rounded path and compares its straight
  middle against the flat primitive row for row. That one catches a band
  boundary off by one, which shows up as a seam down a window's side where no
  corner assertion would ever look.
- **The nonsense-radius test does not guard the `sane` filter**, and this is
  recorded rather than papered over. Removing the filter leaves the behaviour
  unchanged, because degradation to a square is overdetermined three ways at
  once: `f32::max` discards NaN in favour of its other operand, `NaN > 0.0` is
  false so the overlap clamp skips it, and `NaN as u32` saturates to zero,
  collapsing both corner bands. The filter is kept anyway — "correct by
  accident along three independent paths" is not a property anyone can
  maintain, and the next edit to any one of them would break it silently — but
  the test is documented as pinning the user-visible behaviour rather than the
  line, which is the honest claim.

The other reintroductions, each failing deterministically and naming the right
test: discarding a client's radii in `execute_command` (the original bug);
drawing a chamfer instead of a quarter circle; snapping spans to whole pixels
so nothing is antialiased; dropping the overlap clamp; never hollowing an
outline; giving the outline its own curve; forcing every corner to the largest
radius; sending a span straight to the framebuffer past the clip stack;
disabling rounding entirely; starting the middle band a row late; and making
the middle quad a pixel narrow.
