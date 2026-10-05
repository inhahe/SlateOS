## C-ALARMCLOCK-SCROLLS-BY-CLIP-ALONE (lane C)

**Status:** FIXED 2026-09-01 (`cd35986ac`)

**Fix.** Both parts of the proper fix below were done, and the sweeps then found
two more faults in the same file that no part of this entry predicted:

* Each of the three loops now tests the *item's* own edges against the pane and
  `break`s at the first item past the bottom.
* `text` (`:337`) asks `Frame::is_visible` before pushing, exactly as
  `guitk::put_text` does — so ink and hit boxes are governed by one rule again.
* `text_centred` declared its `max_width` from where the run *starts* rather
  than from the box it was told to centre in, so every centred run in the file
  was declared to overhang its box by half the slack. "Stopwatch" in a
  120-point tab was declared to run to 385 in a 360-point window, and the
  renderer trims at the bound it is given, so there was no ellipsis either.
* The alarm editor laid its stack out at a fixed 308 points against a content
  area that is 248 points tall at this app's own minimum window size, so Save
  and Cancel were painted below the panel — hidden by the clip and with their
  hit boxes dropped by `Frame::hit`. The editor covers the alarm tab, so with
  neither button reachable it was a trap the pointer could not leave. The stack
  is now solved for the height it was given.

All three geometry sweeps are in the suite, which is 180 tests, and
`the_editor_can_always_be_left_by_pointer` asks through `frame` at ten window
sizes and clicks Cancel at each.

**In short:** the alarm clock has three scrolling panes — the alarms, the timers
and the stopwatch's lap table. Each one draws *every* item it holds, however
many that is, and relies on the clip (the rectangle that hides ink outside a
box) to make the ones past the bottom edge invisible. Hiding is not the same as
not drawing. Twenty alarms in a pane with room for four means sixteen cards
painted where nothing can ever show them, and unlike the contacts fault that
produced Lesson 107, this app's helpers do not check visibility at all, so the
*text* is emitted too.

**Where.** `apps/alarmclock/src/main.rs`:

| pane | loop | clip |
|---|---|---|
| lap table | `:1598` `for (i, lap) in self.laps.iter().rev().enumerate()` | `:1595` |
| alarms | `:2433` `for alarm in &self.alarms` | `:2430` |
| timers | `:2687` `for (i, timer) in self.timers.iter().enumerate()` | `:2685` |

All three follow `f.clip(pane)` with `f.translate(0.0, -scroll)` and then walk
the whole collection. None tests the item's rectangle against the pane's bottom
edge, and none tests the top either — with the pane scrolled down, the items
above it are drawn as well.

The two module-level helpers are the reason it costs more here than it did in
contacts: `fill` (`:325`) and `text` (`:337`) both `f.push(..)` unconditionally.
`guitk`'s own `put_text` asks `Frame::is_visible` first; these do not, so a lap
row four hundred points below the table is a `RenderCommand::Text` in the frame
claiming to be a label that is present.

**How it is reachable.** Add more alarms/timers/laps than the pane has room
for — trivially, at the app's own opening size — or shrink the window. There is
no upper bound on any of the three collections.

**Why no test caught it.** The app has 175 tests and none of the three geometry
sweeps this campaign now writes: not `nothing_is_painted_entirely_outside_the_clip_in_force`,
not `every_run_of_text_is_bounded_and_inside_the_window`, not
`no_pass_paints_outside_the_box_it_was_given`. It was wired before Lesson 107
was written.

**Found by** running Lesson 107's own "where else to look" over the campaign:
a `for` loop inside an `f.clip(..)`/`f.unclip()` pair that advances a vertical
position and never compares it to the bottom of the box. Of 140 apps, four
loops matched and three of them are the ones above; the fourth
(`apps/typingtutor/src/main.rs:1751`) is a false positive — it states the same
guard as a line-index window (`line >= first_line + lines_visible`) rather than
as a pixel comparison, which is correct.

**Proper fix.** Two parts, and the second is the one that keeps it fixed.
(1) Give each of the three loops the guard the shape asks for: compute the
item's rectangle in pane coordinates, `continue` past it when it is entirely
above the pane and `break` when it is entirely below — the item, not the
cursor, since an item is drawn whole or not at all. (2) Add the three geometry
sweeps to the suite, phrased over fills for the overrun rule (Lesson 107: the
clip suppresses text and hit boxes, so only a fill can carry that assertion),
so the guard cannot be removed again without a test going red.

Not fixed in the commit that found it because that commit is the dbviewer
wiring, and a fault in a different crate found by a scan does not belong inside
it. It is the next task in the lane after dbviewer's roadmap entry lands.
