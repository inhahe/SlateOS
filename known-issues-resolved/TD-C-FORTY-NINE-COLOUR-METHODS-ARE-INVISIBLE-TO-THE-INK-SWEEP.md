## TD-C-FORTY-NINE-COLOUR-METHODS-ARE-INVISIBLE-TO-THE-INK-SWEEP -- FIXED 2026-09-13

**Date:** 2026-09-12. **Lane:** C.
**Where:** 49 sites across 22 crates; `gui/appearance/ink-text.py --blind`
reports them.

**In short:** §837 makes a site that draws *text* in an accent or a status
colour ask the palette for a readable version of it. The script that converted
1,200 such sites finds them by the role named at the draw site — and 49 sites
name no role, because the colour arrives from a method: `dev.state.color(p)`,
`task.priority.color(&self.palette)`, `alert.severity.color(&self.palette)`.
The script reported zero for those files and was right about the question it
asks, which is not the question anyone reading the number thinks it is.

**How they were found:** by failing tests, one at a time, during the shell
conversion — never by the sweep. Four distinct ways a colour can reach a
`RenderCommand::Text` without naming a role there:

| | example |
|---|---|
| a method in the `color:` field | `app.state.color(p)` |
| a helper's return value | `osd::icon_info -> (glyph, Color)` |
| an argument to a draw helper | `render_icon_text_osd(.., p.red, ..)` |
| a local passed by field shorthand | `let color = ..; Text { .., color, .. }` |

The first has a syntactic signature, so it is now reported. The middle two are
properties of a *function body* rather than of the draw site and need something
that parses Rust. The fourth is reported as well, at 88 candidates, most of
which are benign.

**The 49 is the count of call sites; the count of *methods* is 108.** A survey
for `fn *colour*(.., &Palette) -> Color` whose body names a dual-use role finds
108 of them across `gui/` and `apps/`. The 49 are only those called from a
`color:` field in a `Text` command — the same method is often also called from
a fill, a badge or a legend, and that is the complication:

- **Roughly half have an exempt arm** — they return `overlay0`, `subtext0` or
  `text` for one of their cases. Those cannot be inked wholesale; the
  adjustment goes per arm, skipping the exempt one.
- **Many are used for fills as well as text.** `habits::heatmap_color` and
  `diskanalyzer::color_for_node` fill rectangles; inking inside the method
  would darken those for no reason. So "ink inside the method" is only correct
  when *every* caller draws text with it, which has to be checked per method
  rather than assumed.
- **A method that is both mixed-use and has an exempt arm cannot be fixed
  either way** and needs splitting in two — one for fills, one for text — or
  a caller-side adjustment that knows which arm it got. That combination is
  the reason this is an entry and not an afternoon.

**The 49 call sites split in two, and only one half is this entry's problem:**

- **39 take a palette** (`color(&self.palette)`). These are the ink gap and are
  fixable today. Concentrations: `desktop` (8), `remotedesktop` (5),
  `reminders` (4), `sysmonitor` (3), `weather` (3).
- **10 take nothing** (`cat.color()`, `dev.status.color()`). Those crates have
  no `Palette` at all — they are
  `TD-C-SIXTY-EIGHT-APPS-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`, and the colour
  they return is hardcoded. Inking is not the fix; threading a palette is.
  `devicemanager` (4) and `procexplorer` (4) are both on that entry's list of
  twelve.

**What the fix looks like, per method.** Not "ink the call site" — that was
tried and is wrong. `PermissionState::color` has three arms and one returns
`overlay0`, the muted ink WCAG 1.4.3 exempts; raising it makes a *not decided*
row look decided. The rule that came out of the shell: **ink at the point where
every path through it is text.** For `osd::render_icon_text_osd` that is the
body, because its colour parameter has exactly one use. For a method with arms
it is the arms, individually, skipping any exempt one.

**FIXED 2026-09-13. `colour-methods.py` now reports no method with a text
caller that is neither inked at the site nor inked by the body.** The 39 that
take a palette were done over the intervening day; the last one and the tool's
own reporting were finished today.

**The last real one was `resmon::Resource::color`, and it is the entry's rule
in miniature.** The same hue plots a sparkline -- a line, which takes no
contrast floor and whose test asserts the metric's exact hue appears in the
plot -- and labels the metric, which does take the floor. Inking the *method*
floors the graph and breaks that test; inking the *call site* floors both,
because one binding fed both commands. It needs two bindings: the plot keeps
the raw hue and the label takes `p.ink` of it. Three tests pinned the label to
the raw hue and had to follow, exactly as this entry's sibling predicted.

**The other two reports were not findings at all, and the tool said so badly.**
`privacy_settings::PermissionState::color` and
`network_settings::WiFiSecurity::color` are both already inked *per arm* --
the second carries a comment saying so -- and a method that inks internally
leaves its call sites bare by design. They were reported because both names
are `color`, `defs` is keyed on `(crate, name)`, and `gui/desktop` has many
types with a `color` method: inside an ambiguous name the `inked` flag answers
about whichever definition was seen last.

So those sites now print `CANNOT TELL WHICH METHOD` rather than `NOT INKED`.
The population is unchanged; what changed is that the tool no longer states a
verdict it cannot reach. Twenty minutes went into chasing two already-correct
files before that distinction was made, which is the same cost this entry
opens by describing: *"the script reported zero for those files and was right
about the question it asks, which is not the question anyone reading the
number thinks it is."*

**How urgent.** Low and non-worsening. These sites draw exactly what they drew
before §837, so nothing regressed; they are simply not yet *improved*, and the
text they draw can fall below 4.5:1 on a card under the optional filled theme.
The guard `every_ink_clears_the_floor_on_every_ground_it_lands_on` does not
catch them, because it tests the palette's arithmetic rather than which call
sites use it — which is the same gap in a different place, and worth
remembering when reading it.

---

**Update 2026-09-13 — the survey is now a program, and the easy half is done.**

`gui/appearance/colour-methods.py` answers the question this entry could only
pose: it follows each colour from the function that produces it to the command
that draws it, through all three hops recorded above -- the call is the field,
a local carries it, an argument carries it -- and solves parameter usage to a
fixpoint, because a colour is routinely handed down two levels.

| bucket | n | meaning |
|---|---|---|
| INK | 3 | every caller draws text, no exempt arm — **all done** |
| PER-ARM | 5 | ditto, but one arm is `overlay0`/`text` — **all done** |
| INK IF THE REST CHECK OUT | 3 | text callers plus ones the tool would not guess at — **all done, by hand** |
| SPLIT | 37 | callers disagree: the same method draws text *and* fills |
| NO TEXT CALLER | 29 | fills and strokes only; correctly left alone |
| UNRESOLVED | 18 | the tool declines to classify |

**What is left is the hard half, and it is the SPLIT column.** A method that
returns the colour of a status *and* the colour of the badge behind it cannot
be fixed in one place: inking it darkens the badge, and not inking it leaves
the label unreadable on a pale theme. Each needs either two functions -- one
for the ink, one for the fill -- or the ink moved to the text call sites. That
is a per-method design decision about 37 methods, not a sweep.

**Four ways the tool was wrong before it was right**, all now comments in it,
because each is a shape this lane keeps meeting:

1. Matching only the `color:` line reported 27 functions as never drawn. They
   were drawn a line or two below, inside a conditional.
2. Refusing a leading dot in the call pattern hid every *method*, which is most
   of them; nine functions reported no caller at all, and a function with no
   callers needs no decision -- the quietest way to be wrong.
3. Matching by bare name across the tree gave `apps/weather`'s `color` 270 call
   sites, including `gui/desktop`'s Bluetooth list.
4. A `color,` shorthand was not recorded as a colour field, so it defaulted to
   `Text`. `touchpad_status` feeds a 12px status dot *and* its label, both by
   shorthand: read as two text sites it looked safe to ink, and inking it would
   have darkened the dot.

And one rule change that is the general form of all four: **an unresolved
caller does not count as an absent one.** `rate_color` was in the INK bucket
with eight callers the tool could not follow; two of them fill a stat card.
Unknowns now block the verdict instead of being dropped from it.

---

**Update 2026-09-13 (later) — closed.** Every text site behind a colour
method is legible. The tally the tool reports:

| bucket | n | outstanding |
|---|---|---|
| INK | 5 | 0 |
| PER-ARM | 7 | 0 |
| INK IF THE REST CHECK OUT | 4 | 0 |
| SPLIT | 15 | 0 |
| NO TEXT CALLER | 30 | 0 — correctly untouched; inking would darken a fill |
| UNRESOLVED | 19 | 0 |
| AMBIGUOUS | 15 | 2 known false positives (see below) |

**SPLIT was not solved the way this entry proposed.** Splitting 37 methods in
two would have duplicated each mapping and invited the halves to drift. The ink
went to the *text call site* instead, which needs no split, leaves every fill
caller untouched, and is where design decision 837 says legibility lives. 24 of
those sites were mechanical enough for `ink-text.py` to do; the rest were hand
work, because the colour reached the draw through an argument, a local, or a
helper that also filled with it.

**The two remaining marks are false positives, verified by reading them:**
`gui/desktop/src/privacy_settings.rs:641` calls a method that already inks its
own body, and `resmon.rs:548` binds a colour that only ever fills. Both belong
to the AMBIGUOUS bucket, where `gui/desktop` defines twenty-three methods named
`color` and nothing here can tell a call site's receiver apart from another's.
That bucket's per-site marks are a hint, not a verdict — though following one
did find real work: `WiFiSecurity::color`, the padlock glyph, was uninked.

**Four more tool corrections, all the same family as the first four:** a
binding is scoped to its block and not its function; `color: p.text,` contains
the word `color` and is not a use of a local called `color`; a colour handed to
a helper that inks it inside is already handled; and `--verify` was scanning
test code, so it flagged `launcher`'s own fixtures.

**What is left of this entry is nothing.** The residue lives on
`TD-C-SIXTY-EIGHT-APPS-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`: a handful of
colour methods still take no palette at all (`procexplorer`'s two), and threading
one in is that entry's work, not this one's.
