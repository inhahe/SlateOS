## 815. The Settings screens split by kind: what the desktop shows you stays in the shell, what you open lives in the app -- and both follow the Aero demo's look

**Date:** 2026-09-07
**Lane:** C
**Decided by:** Operator (Claude recommended C; the operator chose C and added the styling mandate, which was not part of the question)

**In short:** the settings screens exist twice -- once inside the desktop
shell, once in a standalone Settings app -- and neither copy was finished, so
every change had to be made in two places. From now on the dividing line is
"is this something the desktop shows you, or a screen you open?". The volume
overlay and the login screen are the desktop showing you something, so they
stay in the shell and get wired up. Everything you open from a menu moves to
the Settings app, and the shell's copies are deleted. Separately, the operator
wants both to *look* like the `Aero Desktop (offline).html` demo.

**The question.** `open-questions.md` -> C-Q6. Four options: delete the
shell's panels, delete the app's, split by kind, or leave it.

**The answer: C, split by kind.** The dividing line is a real one rather than
a compromise between two half-finished copies. It is also the most work, which
is why it was worth asking rather than assuming.

**The styling mandate, which the operator added unprompted.** The shell and
the settings pages are to follow `.\Aero Desktop (offline).html`. The split
the operator specified:

- whatever is *themeable* reads from the current settings, so the OS can carry
  different themes;
- everything else follows the demo;
- and the demo's look is the **default theme**.

The operator's closing sentence -- "If this isn't already in
roadmap-detailed.md, it should be" -- is an instruction, carried out in the
same change as this entry.

**What this settles that was in flight.** The palette conversion. The shell's
549 hardcoded colours are worth converting because those modules survive; the
app's 2,258 are worth converting because the app is where the settings pages
are going. Neither half is wasted, which was the thing C-Q6 was blocking.
