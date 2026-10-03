## C-SPREADSHEET-FREEZE-ACCEPTS-A-BAND-BIGGER-THAN-THE-WINDOW (lane C, 2026-08-20) -- **FIXED 2026-09-07**

**Fixed 2026-09-07 (lane C).** A freeze whose band would fill the window is
refused and says why; the sheet is left as it was. Unfreezing is never
refused, because the rule is about entering the state, not leaving it.

The policy call this entry deferred was taken rather than escalated, and
recorded as `design-decisions.md` 820 with the two rejected alternatives, so
the operator can overrule it cheaply. The reasoning: Excel refuses the same
operation, the old behaviour was a trap rather than merely imperfect, and
silently capping to the largest band that fits would have told the user a lie
about what they asked for -- the same shape as every other defect found here
this week.

The status bar had no channel for a refusal at all: it was derived entirely
from the selection. It now prefers a `notice`, cleared by any keystroke.

**Status:** open — degrades safely, but the state is not useful.

`toggle_freeze_panes` pins at the active cell with no upper bound, so selecting
column T in a 1280 px window and freezing gives a 2000 px frozen band inside a
~1216 px viewport. The result is safe — `push_clip_rect` clamps the resulting
negative extent to zero and `render_row_band` returns early on a non-positive
height, both of which are covered by
`tests::no_clip_has_a_negative_extent` — but the sheet is then entirely frozen
band with nothing able to scroll, and the only way out is to freeze again to
unfreeze.

Excel refuses the operation instead. **The fix** is to cap the freeze at what
leaves a usable pane (Excel's rule is essentially "the frozen band may not fill
the viewport"), and to say so in the status bar rather than silently pinning
less than was asked for. Deferred because it needs a user-visible policy call
on *which* rule — cap silently, refuse with a message, or scroll the sheet so
the requested split fits — rather than because it is hard.
