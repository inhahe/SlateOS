## TD-C-TEN-RECTANGLE-TYPES-IN-THREE-SPELLINGS -- 2026-09-17

**In short:** ten different programs and libraries each declare their own
rectangle. Eight of them are the same four numbers under two different
spellings, so using a toolkit widget from an application means converting
between them a field at a time. The other two are genuinely different and must
stay that way.

**Date:** 2026-09-17. **Lane:** C.

**The first version of this entry said "two", because two was how many I had
met.** Its closing line was that nobody had counted the rest; counting them
took one command and returned ten. Recorded as written, because the gap between
"the two I tripped over" and "what is actually there" is the entry's most
useful content.

| Group | Fields | Where |
|---|---|---|
| **A** | `x, y, w, h` (`f32`) | `gui/toolkit/src/frame.rs`; ~~`gui/desktop/src/lib.rs`~~ **done 2026-09-17** |
| **B** | `x, y, width, height` (`f32`) | ~~`explorer`, `ircclient`, `photomanager`, `podcast`, `radio`, `videoplayer`~~ **all done 2026-09-17**; `apps/whiteboard` — **not a drop-in, see below** |
| **C** | `x, y: i32`, `width, height: u32` | `gui/compositor/src/lib.rs` — **correct as it is, do not sweep** |

**FIXED for seven of the ten; the three that remain are the canonical one and
two that are genuinely different.**

**`apps/whiteboard` is not a drop-in, and its own test says so.** Its
`contains` uses `<=` on both axes -- closed intervals, where the toolkit and
the other five use `<`. So a point exactly on the right or bottom edge is
inside a whiteboard rectangle and outside every other kind. That is deliberate
and pinned: `test_rect_contains` asserts `r.contains(110.0, 60.0)` for a
100x50 rectangle at (10, 10), which is precisely the far corner. It also has an
`intersects` the toolkit lacks.

**And on a second look it is not merely deliberate, it is right for this
program** -- which matters, because "a test pins it" is a weak reason to leave
something alone. A test can pin a bug. The argument is:

* The toolkit's half-open rule exists to stop **two adjacent rectangles both
  claiming a pixel**, where the winner would otherwise depend on the order they
  were recorded in. Whiteboard cannot have that ambiguity: `hit_test_shapes`
  walks the shapes in reverse and returns the first hit, so overlap is settled
  by **z-order**, deterministically, before the rectangle is ever asked.
* A drawing canvas *wants* its edges grabbable -- you draw a rectangle and then
  reach for its border. The same function already widens the target for lines
  and freehand strokes by half the stroke width with a four-pixel floor, which
  is the same "generous hit-region" the roadmap asks for elsewhere.

So the two rules answer different questions: the toolkit's disambiguates
neighbours, whiteboard's makes a shape's own border part of it. Sweeping the
second into the first would have traded a deliberate affordance for a guarantee
this program does not need.

**Two down, six to go, and the two were different jobs.** `apps/explorer` was
group B: 73 field accesses renamed, each at the line and column the compiler
named, because a regex would also have caught `.width` on thumbnails and
surfaces. `gui/desktop` was group A: the same spelling already, so a pure type
swap with no renames at all -- and worth checking rather than assuming, because
its `contains` carried documented half-open semantics. The toolkit's turned out
to document the identical rule in almost the same words, so the swap was
behaviour-preserving. Had they disagreed on which edge belongs to which
rectangle, an identical-looking signature would have silently moved every
button's hit region by one pixel.

**Group C is not a duplicate and must not be swept in.** A compositor rectangle
is device pixels -- integers, and an extent that cannot be negative. A layout
rectangle is logical floats. Unifying those would delete a real distinction and
let a negative width reach a surface, which is the failure recorded in
`TD-C-A-CLEANUP-THAT-REMOVES-A-DISTINCTION`. **A and B are the duplicates**:
same four `f32`s, same meaning, same constructor order, two spellings.

**What it cost, which is how it was found.** The preview panel splits the file
pane using `guitk::splitter`, whose geometry is in the toolkit's rectangle. The
explorer's pane rectangle is in its own. So `preview_panes` converts on the way
in and converts both results on the way back, and the same two lines will
appear at every future call site where an explorer rectangle meets a toolkit
widget. Two lines is nothing; the *pattern* is the cost, because each copy is a
place a `width` can be handed to a `w`-shaped hole that happens to type-check.

**Why it is not obviously a defect.** Nothing is wrong today and no test can
fail for it. That is the shape worth naming: a duplicate type does not break,
it *taxes*, and the tax is paid at call sites that do not exist yet. It only
became visible when a widget and an application first had to share geometry.

**The proper fix** is for group B to use `guitk::frame::Rect` and delete its
own, and for group A's second copy in `gui/desktop` to go the same way.
Mechanical -- rename two fields at every construction and access -- and worth
doing *before* the number of conversion sites grows. Not folded into the
feature commit that found it: converting a rectangle used by the drop zones,
the hit-testing and three view renderers is not something to slip into a
feature change and hope the tests notice.

**Do it one crate at a time, and leave group C alone.** Seven applications
share spelling B, but they do not share a reason -- each declared its own
because it had no other option at the time, and each has its own call sites.
One crate per change keeps a rename that breaks something attributable to the
crate it broke.
