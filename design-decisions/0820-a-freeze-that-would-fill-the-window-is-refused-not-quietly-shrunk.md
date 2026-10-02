## 820. A freeze that would fill the window is refused, not quietly shrunk

**Date:** 2026-09-07
**Lane:** C
**Decided by:** Claude (autonomous) -- and revisitable; the alternatives are
below and the operator may prefer one of them

**In short:** in the spreadsheet you can pin rows and columns so they stay put
while the rest scrolls. If you picked a cell far enough right or far enough
down, the pinned band filled the whole window: nothing was left that could
scroll, and the only way out was to use the same command again to unpin. Now
the spreadsheet refuses that and says why, instead of doing it.

**Why this was decided rather than asked.** `known-issues.md` deferred it as
"a user-visible policy call". It is one, but it has a dominant convention
(Excel refuses the same operation), the current behaviour is a trap rather
than merely imperfect, and the operator's queue already has several unanswered
questions. Leaving a trap in place to protect a small decision is the wrong
trade. It is recorded here so it can be overruled cheaply.

**What was rejected.**

- *Cap silently at the largest band that would fit.* You ask to freeze at
  column T, you get column H, and nothing says so -- the result tells you a
  lie about what you asked for. This is the same shape as every other defect
  found in this tree this week: an operation that quietly does something
  other than what it was told.
- *Scroll the sheet so the requested split fits.* Clever, and surprising: the
  view jumps somewhere the user did not ask to look, to satisfy a command
  about pinning.

**Where it bites.** `apps/spreadsheet/src/main.rs` --
`toggle_freeze_panes`, and a new `notice` field the status bar prefers over
its sum.

**The status bar had nowhere to say it.** It was derived entirely from the
selection, so an operation that declined to act had no channel at all. The
notice is cleared by any keystroke rather than by a timer: a message that
vanishes on its own is one the user can miss, and one that never vanishes is
one they stop reading.

**Unfreezing is never refused.** The rule is about *entering* a state with
nothing to scroll; leaving one must always work, or a sheet frozen by an older
build could not be recovered.
