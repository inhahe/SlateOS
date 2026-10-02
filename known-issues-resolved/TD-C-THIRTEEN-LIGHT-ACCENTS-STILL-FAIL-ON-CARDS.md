## TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS -- FIXED 2026-09-13 (mechanism 2026-09-12, the 315 dual-use sites 2026-09-13)

**Update, 2026-09-12 (lane C).** Answered, and the entry below was wrong in
three ways worth recording before the correction.

**It was not thirteen, and it was not light-only.** Dark mode has the same
defect and nobody had looked: 13 of its 16 inks fail on its deepest card, red
worst at 2.88:1. Nor is the default theme exempt -- a toolbar is a `mantle`
band with labels on it, and light accents sit at 4.28 there, under the floor.

**"Colour the cards differently" cannot work, and this is provable.** The
palest accent (maroon) only clears 4.5 against greys lighter than `#EFEFEF`.
The page is `#EFF1F5`. So there is no card shade *darker than the page* that
all fourteen accents survive, and the ink is what has to move.

**The fear recorded below -- fourteen near-black accents, "a user picking green
would get something indistinguishable from blue" -- does not hold.** Scaling
toward an extreme preserves hue. Green becomes `#245A18` and blue stays
`#0036A3`; measured over the fourteen, the closest pair after adjustment is
teal/sapphire, which are near-duplicates in the source palette already.

**What landed.** `appearance::legible_on(ink, bg)` moves an ink only as far as
the floor requires, toward whichever pole is legible on that ground.
`Palette::ink(colour)` applies it against every surface the *active theme* puts
text on -- which is a consequence of the two style settings, not a fixed list,
and getting that wrong is precisely what the old guard did.

The roles split in two, and the split is the design:

| roles | how they are fixed | why |
|---|---|---|
| `subtext0`, `subtext1`, `link` | floored in the palette | they exist to be read, are never fills, and the user does not choose them -- 546 of 861 sites, none touched |
| `accent`, `red`, `green`, … | site asks `p.ink(…)` | dual-use: an accent is also a switch that is on, `red` is also an error bar, and those must not move |

`overlay0` stays exempt: it is the muted ink, WCAG 1.4.3 exempts inactive
controls, and raising it would make a disabled control look enabled.

**The 315 dual-use text sites are done.** `ink-text.py --check` reports "every
text site in 380 files goes through `ink()`" and `--verify` confirms all 650
`ink()` calls are inside a `Text` command -- the second being the guard
against the opposite error, an ink applied to something that is not text.

**What was left is what the script could not see, and 2026-09-13 shrank that
from 183 to 126 and found five real failures in the gap.** `--blind` exists
because "`--check` says zero" means "zero among the sites I can classify".
Two of its four unclassifiable shapes turned out to be classifiable after all:

| | before | after | how |
|---|---|---|---|
| `color:` whose value is a call | 81 | 33 | take the *balanced* value instead of a five-line window; a conditional whose every branch is a floored ink is legible by construction and only looked like a call because `if` contains a `(` |
| `color,` shorthand | 102 | 83 + 10 | walk back to the nearest `let color =` in the same function and classify *that* |

The 83 that remain are shorthands whose local came from a parameter, a
destructuring or a loop -- genuinely outside a line-based script -- and the 33
calls are colours produced by a method, which is the
`TD-C-FORTY-NINE-COLOUR-METHODS` class.

**The five findings, four fixed and one a false positive the script caused:**

| where | what | verdict |
|---|---|---|
| `gui/desktop/src/update_settings.rs:818` | `if entry.success { p.green } else { p.red }` — an update row's status, and the next line puts it on a `Card` | **inked** |
| `apps/systemrestore` | a diff's green/red/yellow — the whole of what a diff communicates | **inked** |
| `apps/typingtutor` | green for a correct character, red for a wrong one — the entire feedback of the program | **inked** |
| `apps/ircclient` | `blue` for the active channel | **inked** |
| `apps/whiteboard` | a stroke's colour | **left alone**, and annotated: it is the drawing, in the sense `apps/paint`'s swatch row is the document |

**A second pass on the same day found four more, by widening the binding
pattern to a destructuring.** `let (label, color) = match profile { .. }` is
how a colour and the word it labels get chosen in one expression, and matching
only `let color = ..` missed every one of them:

| where | what |
|---|---|
| `gui/desktop/src/power.rs` | the power-profile badge's four labels — blue, peach, green, lavender |
| `apps/jsonviewer` | teal and blue for YAML keys and list markers |
| `apps/notes` | blue / lavender / mauve / teal for the four markdown heading levels, and blue for a wiki link |

All are text whose whole purpose is to be read, and `lavender` and `teal` are
among the palest accents. All inked.

**A third pass, same day, on the bucket the script had lumped as "no local
binding": six more.** Two refinements made them visible, and both were
*subtractions* from the report rather than additions:

* **A `RenderCommand::Text { .. }` that is a pattern is not a draw site.** The
  compositor's `execute_command`, the wire encoder and `palette_check` all
  destructure one, and their `color,` is a binding being introduced rather
  than a colour being chosen. Six reports, none of which drew anything.
* **A function that takes `color: Color` is not an unresolved colour.** It is
  the correct shape for a toolkit primitive -- `RenderTree::text` must not ink,
  because it does not know the ground -- and it accounts for **65** of the
  remainder. They are resolved one frame up, at call sites the script does
  convert.

With those two out of the way the residue was eight, small enough to read, and
six were real:

| where | what |
|---|---|
| `apps/clipmanager` | the Use/Delete template buttons, and a seven-label action row |
| `apps/finance` | Income / Expenses / Savings — the three figures a finance header exists to show |
| `apps/renamer` | five button labels |
| `apps/speedtest` | Download / Upload / Latency — the whole of what that screen reports |
| `apps/startupmanager` | `status_color`'s enabled arm |

All were **loop-destructured tuples** -- `for (label, color, target) in [..]`
-- which is how a set of labelled, coloured things gets drawn in one pass, and
which no binding pattern reaches.

`startupmanager`'s is the neatest statement of the whole split: its
`status_color` returns `pal.green` when enabled and `pal.overlay0` when not.
The first is inked and the second must not be -- `overlay0` is the disabled
ink, WCAG 1.4.3 exempts an inactive control, and flooring it would make a
disabled entry look enabled. One method, two roles, two answers.

**The report still says eight**, because the shape is unchanged even where the
value is now right: the script reports what it cannot classify, not what is
wrong. Three of the eight were checked and are correct as they stand --
`apps/slides` (a slide's own element colours, content), `apps/pdfviewer`, and
`startupmanager`'s two floored cells.

**And one of those broke a test in exactly the way this entry predicted.**
`power::every_choice_this_module_makes_hands_over_the_role_it_claims` compared
the badge against `rgb(p.peach)` — the raw field. It compares against
`rgb(p.ink(p.peach))` now, while the *gauge* assertions three lines above
still use the raw field, because a gauge is a filled bar and a badge is a
label. Same roles, two readings. That is the dual-use split working, and a
test comparing both against the raw field would have passed while the label
was unreadable.

**The whiteboard one was reported because of a bug in the script's own span
reader**, worth recording because it is the day's recurring shape: the value
of a `let` was taken up to the next depth-zero *comma*, which is right for a
struct field and wrong for a statement, so it ran past the `;`, swept up a
palette role from unrelated code below, and accused the one file where the
colour is the user's rather than the theme's. Fixed to stop at `;` as well.
The four genuine ones were confirmed by reading the code, not by trusting the
report, which is the only reason the false positive was recognisable as one.

The guard `every_ink_clears_the_floor_on_every_ground_it_lands_on` covers both
modes, both surface styles, both strip styles, the fourteen presets and two
hostile custom accents, and checks each role *the way a draw site reads it* --
the floored field for the three, `p.ink(field)` for the rest. Checking both
through `ink` would have passed while `p.subtext0` was unreadable.

---

### The original entry, for the record

**TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS** — as originally filed:

**Date:** 2026-09-09. **Lane:** C.
**Where:** `gui/appearance/src/lib.rs` — `LIGHT_LAVENDER` through
`LIGHT_SAPPHIRE`, thirteen constants.

**In short:** the user can pick one of fourteen accent colours for the desktop.
On the light theme, thirteen of them are too faint to read wherever they land
on a shaded card — which is most places accent text appears. Only blue, which
the operator supplied a new value for on 2026-09-09, is readable.

**The measurement.** Against the four surfaces text is drawn on:

| accent | page | surface0 | surface1 | surface2 |
|---|---|---|---|---|
| blue (fixed) | 9.03 | 6.61 | 5.62 | 4.72 |
| lavender | 4.65 | **3.41** | **2.89** | **2.43** |
| teal | 4.62 | **3.39** | **2.88** | **2.42** |
| … | … | … | … | … |
| sapphire | 4.60 | **3.37** | **2.86** | **2.41** |

Every one of the thirteen lands between 4.60 and 4.80 on the page, and between
**2.33 and 3.52** on every card. The floor is 4.5.

**Why they are all *just* over on the page and nowhere else.** They were
derived together, by scaling each Catppuccin Latte accent's channels until it
cleared 4.5 **on the base**, and no other surface was checked. The crate's own
comment says so: *"reaches 4.6:1 on `#EFF1F5`"*. So the whole set shares one
mistake made once — the same mistake the greys had, which is what C-Q10 was
about. Fixing the greys and blue without the other thirteen leaves the defect
for any user who prefers green.

**Why this is not simply "apply the same rule again".** The rule that produced
these values — scale until it clears the *page* — is the bug. Any replacement
has to clear the floor on the deepest surface text is drawn on, and that is a
much harder constraint: `surface2` gives only 9.71:1 against pure black, so
every accent clearing 4.5 there is nearly black, and fourteen nearly-black
accents are not fourteen accents. **A user picking "green" would get something
indistinguishable from "blue".** That is the same trap option A fell into for
the greys, one dimension over, and it is why this is logged rather than swept.

**Which makes it the operator's call**, and it is bound up with the card
question still open from C-Q10: the darker the card, the less room the accents
have. `#A0AECA` is already the darkest card the *current* four inks survive —
one step to `#9CAAC6` puts blue at 4.37 — so there is no headroom to spend.
Plausible directions, none free:

1. **Accent text does not go on the deepest cards.** Constrains layout, changes
   no colour, keeps fourteen distinguishable accents.
2. **Accents get a per-surface variant** — a lighter one for the page, a darker
   one for cards. Doubles the table and every lookup has to know its background.
3. **Accept fourteen near-black accents.** Cheapest, and throws away the point
   of letting the user choose.

**Not urgent, and it does not get worse on its own.** It has been shipping this
way; what changed today is that it is now measured and written down. The guard
test `light_inks_clear_the_contrast_floor_on_every_surface` deliberately does
**not** cover the accents, because asserting a known failure means either a red
build or a muted test — it grows to cover them the day this is decided.
