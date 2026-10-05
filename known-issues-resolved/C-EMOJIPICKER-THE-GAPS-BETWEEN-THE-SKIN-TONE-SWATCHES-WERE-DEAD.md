## `C-EMOJIPICKER-THE-GAPS-BETWEEN-THE-SKIN-TONE-SWATCHES-WERE-DEAD` (lane C, 2026-08-20) — FIXED

**In short:** the emoji picker's six skin-tone swatches are drawn as 18-pixel
circles six pixels apart. Only the circles were clickable, so a click that
landed in one of the five gaps between them did nothing at all — and, worse,
was reported as *not handled*, which offers it to whatever is behind the popup.
A click off the left edge of the window, meanwhile, quietly switched to the
recently-used tab. Both come from the same root: this file wrote out its layout
arithmetic twice — once where a thing is painted, once where a click is
resolved — and the two copies were free to disagree. They now come from one set
of named functions.

**Where:** `apps/emojipicker/src/main.rs` — the new `// === Layout ===`
section, and every renderer and hit test that used to do the arithmetic itself.

### What was wrong

Four separate quantities were spelled out at more than one site:

| Quantity | Times written | Consumers |
|---|---|---|
| the grid's left margin | 5 | `render_grid`, `grid_hit_test`, 2 tests |
| the grid's bottom edge | 6 | clip rect, scroll clamp, 3 branches of `handle_mouse` |
| the tab list and tab width | 2 in full | `render_tab_bar`, `handle_tab_click` |
| the swatch strip's origin | 2 | `render_skin_tone_bar`, `handle_skin_tone_click` |

Nothing but care kept them equal, and in two places care had already run out:

**The swatch gaps belonged to nobody.** `render_skin_tone_bar` steps by
`CIRCLE + SPACING` (24px) and paints an 18px circle at each step;
`handle_skin_tone_click` re-derived the index but then tested membership against
the *circle*, so six pixels out of every twenty-four were dead. A user aiming
between two tones — the natural thing to do when they are 6px apart — got
nothing.

**A click left of the tab bar landed on tab 0.** `(x / tab_width) as usize` is a
float-to-integer cast, which in Rust *saturates*: `-30.0 as usize` is `0`, not a
wrap or a panic. A click off the left edge of the window silently switched the
picker to recently-used.

**A click on the strip's label was reported as ignored.** The strip is part of
the popup; declining it hands the click to whatever is underneath.

### Fixed 2026-08-20

A `// === Layout ===` section holding every number that decides *where*
something is drawn: `grid_left()`, `grid_bottom()` (defined as
`skin_tone_bar_y()`, so the grid ends exactly where the strip begins by
construction rather than by two matching subtractions), `skin_tone_swatch_x()`,
`skin_tone_swatch_at()`, `tabs()`, `tab_width()`, `tab_x()`, `tab_at()`. Every
renderer and every hit test now reads them.

**A swatch's click cell is its whole pitch**, not the circle drawn inside it:
`skin_tone_swatch_at` divides by `SKIN_TONE_PITCH` from a strip origin pulled
half a gap to the left, so the six swatches tile the strip with no dead pixels
and the gaps go to the nearer circle. `tab_at` rejects negative `x` explicitly,
with a comment naming the saturating cast so nobody "simplifies" the guard away.
`handle_skin_tone_click` consumes either way.

### Testing

Nine new geometry tests (72 green, clippy clean, rustfmt clean). Fourteen
mutations, all fourteen caught — but only after two escaped the first run, and
those two are the interesting part:

- **Sweeping the strip has to sweep past the end of it.** "A click past the last
  swatch wraps onto a real one" (`Some(index % len)`) survived, because the
  sweep stopped one circle-width after the last swatch and the wrap region
  starts further out. The test now sweeps the full window width and asserts the
  tone is *unchanged* outside the swatch run — with the fixture pre-set to a
  non-default tone, so "unchanged" cannot be confused with "landed on swatch 0".
- **Collapsing paint and hit test into one function means a wrong function
  moves them together.** "The grid is centred on the wrong width" (`/4.0`
  instead of `/2.0`) survived every test that clicked where the grid was
  painted, because both readings came from `grid_left()`. What catches it is a
  measurement against something with **no shared origin**: the painted columns
  are checked for equal left and right margins against `WINDOW_WIDTH`, a claim
  `grid_left` does not itself get to decide. Same shape for
  `the_grid_is_clipped_to_exactly_where_the_strip_begins`, which cross-checks
  the clip rectangle against the *painted strip* rather than against
  `grid_bottom()`.

See `design-decisions.md` §481.
