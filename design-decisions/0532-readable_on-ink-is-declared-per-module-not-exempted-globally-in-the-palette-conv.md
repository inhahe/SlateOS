## 532. `readable_on` ink is declared per module, not exempted globally, in the palette-conversion sweep

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The automated check that proves a shell module was converted off
its hard-coded colours had a hole in it. It waved past two specific colours —
a near-black and a near-white — on the grounds that they are what the
"pick text that will be legible on this button" function returns, and so are
not really theme colours. But those two colours *are* theme colours: the
near-white is the light theme's background, and the near-black is the dark
theme's darkest shade. So the check was blind to the single most likely
mistake in each direction — a leftover copy of the light theme's background,
or of the dark theme's darkest shade — everywhere in the shell. The exemption
is now gone; a module that draws such text says so, one line, at the place it
runs the check.

### What the exemption was

`gui/desktop/src/palette_check.rs` sweeps a module's render output and fails
if any command carries a colour the palette in play cannot account for. The
accounted-for set was: any role of the palette (compared on RGB, so alpha
variants pass), black at any alpha (scrims and shadows, §525 decision 3),
anything the caller declared in `derived` — and, unconditionally, the two
values `appearance::readable_on` can return:

```rust
const READABLE_ON_ENDPOINTS: [u32; 2] = [0x0011_111B, 0x00EF_F1F5];
```

The reasoning was that ink chosen for a coloured fill is a function of that
fill's brightness rather than a role, so it is not a palette member and had to
be allowed somehow. The module doc justified the blanket form by asserting the
alternative would "fail every module that draws a coloured button, which is
most of them."

### Why it had to go

`0xEFF1F5` is Latte `base`. `0x11111B` is Mocha `crust`. The conversion's
whole method is that a leftover literal from one mode is a colour the *other*
mode's palette does not contain, and therefore names itself — and these two
values are precisely the two leftovers most likely to exist, since `base` and
`crust` are the most-copied constants in the shell. The sweep was told to
ignore exactly the case it was built to find.

This was not deduced, it was measured. Module 48's reintroduction sweep
produced an asymmetry that has no legitimate reading:

| Defect | Reintroduced | Result |
|---|---|---|
| `A×78` | the desktop background as a Mocha `base` literal | **caught** |
| `B×78` | the same site as a Latte `base` literal | **not caught** |

Two defects that differ only in which mode's leftover they represent must not
get different answers. `K×78` — a fresh config naming Latte `base` as the
user's choice — failed the same way, for the same reason.

### The options

1. **Keep the blanket exemption, and rely on each module's per-site tables to
   cover the two values.**
   *What changes:* nothing; the sweep keeps certifying modules it did not
   fully check, and every future module inherits the hole.
2. **Exempt the endpoints only in the mode where they are not a role** — i.e.
   allow `0x11111B` in a light render and `0xEFF1F5` in a dark one.
   *What changes:* the leftover-literal case is caught in one direction but
   the rule becomes a special case with no statement behind it, and a module
   that draws *both* inks (most of the thirteen do) still gets both waved past
   in both modes.
3. **Delete the exemption; each module that draws readable ink declares it.**
   *What changes:* thirteen call sites gain a `derived` argument naming the
   fill the ink sits on; the other thirty-six modules get both values checked
   as ordinary roles.

### Decided: option 3

A declaration is strictly more informative than an exemption:

- **It names the fill.** `&[readable_on(p.accent)]` says *which* button the
  ink belongs to, so a reader can check the claim. The exemption said only
  "some colour somewhere might be legibility ink."
- **It is confined.** Thirty-six modules that never compute such a colour now
  have both values checked as ordinary roles, which is what they always
  should have been.
- **It goes stale loudly.** Draw the ink on a different fill and the
  declaration stops covering it, and the sweep says so.

This is also what `palette_check`'s own module doc already argued for in the
`derived` section — "a colour that is not a role has to be *claimed* by
someone" — so the exemption was inconsistent with the file it lived in.

Option 2 was rejected because it encodes a coincidence (which mode happens to
own which extreme) rather than a rule, and because it does not help the
thirteen modules that are the actual subject.

### The cost, measured rather than estimated

The old doc's claim that removal would fail "most" modules was checked by
patching the branch to `if false && …` and running the suite: **14 tests
across 13 of the 49 modules** — 27% — each fixable with one declaration. The
thirteen are `accessibility_settings`, `calendar`, `default_apps`,
`device_settings`, `display_settings`, `icons`, `login_screen`, `notif_pane`,
`osd`, `snap`, `startup_settings`, `user_accounts` and `window_peek`.

### The residual limit, stated rather than discovered later

A declaration is a claim about a *value*, not about where that value may
appear. A module that legitimately draws both inks — `user_accounts` letters
on eleven different fills — ends up declaring both endpoints, which locally
reproduces the old allowance for that one module. That is honest: those
colours really are on its screen, and no membership test can distinguish them
from a stray copy. What the per-module form buys is that the claim is *stated
where it is true* instead of assumed everywhere, and the modules that do not
draw such ink no longer pay for the ones that do. Catching a misplaced copy
inside those thirteen remains the job of their per-site tables (e.g.
`none_of_the_nine_deleted_constants_is_still_drawn`), which is what those
tests were written for.

### Regression pin

`palette_check.rs` now carries
`a_readable_on_endpoint_is_not_exempt_in_the_mode_that_lacks_it`, which offers
each endpoint to the palette of the *other* mode and requires a rejection —
reproducing the `A×78`/`B×78` asymmetry literally, so restoring the exemption
fails a test rather than silently un-checking 49 modules.
