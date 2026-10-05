## 1424. An event colour too close to the accent is allowed, with a warning -- both ways round

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended C, weakly; the operator chose to warn) &middot; **Lane:** C (the shared test), with E (the two pickers)

**In short:** A calendar event coloured almost like your accent has its dot
vanish into today's circle. The colours stay exactly as you chose them -- the
calendar never alters them. Instead, choosing an event colour too close to the
accent shows a warning, and so does choosing an accent too close to an event's
colour; the second warning says which events clash and offers a way straight
to changing them.

**The question:** `open-questions.md` C-Q19 (now resolved). **Verbatim:**
"Either don't allow the user to change an event color to something too close to
the accent color, or warn them if they do, and then leave it alone. And the same
choice has to to be made in the other direction: the user changing the accent
color to something that conflicts with an event color. I think that choice
should definitely be 'warn' rather than 'don't allow,' and in the warning, tell
how to change the offending event color(s) and/or have a link right there to
changing the event color(s)."

| Part | Whose |
|---|---|
| One test of "too close to see one on the other", so the two warnings cannot disagree | lane C, `appearance` |
| The warning in the calendar's event-colour picker | lane E, `apps/calendar` |
| The warning in Settings' accent picker, naming the clashing events, with a way to each | lane E, `apps/settings` |
| The desktop's calendar drawing | unchanged: it draws the colour chosen (`gui/desktop/src/calendar.rs`) |

**The test, as built (2026-09-27):** `guitk::palette::hard_to_tell_apart`,
re-exported by `appearance` -- true only when the pair is both under WCAG's 3:1
for marks that are not text *and* under 40 apart as a CIE 1976 colour
difference, so a red dot on a blue disc of the same lightness is not flagged
while a lavender one is. The 40 is a judgment: it is set above the roughly 30
lightness units that 3:1 takes, so each half decides something; a warning that
errs toward warning costs a glance.
