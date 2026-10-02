## 845. The system tray is the shell's: fold `apps/systray` in — the operator's answer

**Date:** 2026-09-14
**Lane:** C
**Decided by:** Operator (Claude recommended A; the operator chose A). Relayed through lane A, who noted that filing it is the owning lane's job.

**In short:** the row of little icons at the right of the taskbar existed
twice — once inside the desktop shell, which drew a clock but could not hold
a program's icon, and once as `apps/systray`, a separate program that held
icons but which nothing ever launched. The drag-and-drop the spec asks for
was written, 1 184 lines of it, and lived with the half that had no icons.
The tray is the shell's. `apps/systray` stops being a program and its
unique parts move in.

**The question.** `open-questions.md` — C-Q12, with three options: A fold the
app into the shell, B give the app a strip of the taskbar and a protocol to
negotiate it, C leave both and delete the drag-and-drop.

**Why A, in the operator's terms rather than mine.** One taskbar drawn by
one program, so the icons and the clock cannot disagree about where the
tray starts or how wide it is. B's cost was a protocol that does not exist:
the shell would have to tell the tray how much room it has on every clock
tick that changes the clock's width, and two programs would have to agree
about the theme, the scale factor and the autohide animation, all of which
the shell owns outright today.

**This confirms §842 rather than replacing it.** That entry reached the same
answer on 2026-09-13 by applying §815's dividing line, and said so:
*"Claude (autonomous), but only in the sense of applying an Operator
decision."* The operator has now made it directly. Where they differ is
worth keeping: 842 argued the tray *belongs in* the shell; this settles
that `apps/systray` **stops existing**, which 842 did not decide and the
work has been proceeding as though it had.

**What is already built under it.** The shell-side tray was implemented
across 2026-09-13 on 842's strength: a control verb to register an icon, a
registry in the compositor reaped per client, a `TRAY` frame and
subscription, the shell drawing the row, a click routed back to the owning
program, drag to reorder, an overflow chevron, and hover tooltips. See
`known-issues.md`
`TD-C-FOUR-MODELS-OF-A-TRAY-ICON-AND-NO-PROCESS-BOUNDARY-BETWEEN-ANY-OF-THEM`.

**What is left, and it is now unambiguous.** `apps/systray`'s unique parts
— quick settings, the volume and network popups — move into the shell, and
the program goes. Until this answer that deletion was a judgement nobody
had made; it is now the decision.

**The one cost worth restating, because it does not go away.** A crash in a
tray popup now takes the taskbar with it, where a separate program could
have died alone. That is the price of one program owning one bar, and it is
an argument for the popups being simple rather than for them living
elsewhere.
