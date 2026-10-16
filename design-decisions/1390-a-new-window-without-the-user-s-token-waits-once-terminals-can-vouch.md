## 1390. A new window opened without the user's token waits in the background -- once the launchers pass tokens on and a terminal can vouch for what it starts

**Date:** 2026-10-11
**Lane:** F
**Decided by:** Operator (F-Q12, `operator-answers/2026-10-11-open-questions-answers.3.txt`:
"B"; Claude's recommendation)

**In short:** a program that opens a new window can still take the keyboard
at a moment it picks, so the rest of a password typed elsewhere can go to it.
§1386 closed every other way in. This closes the last one -- a new window
with no activation token waits behind the window the user is in, its taskbar
button flashing -- but only once two things exist, so that nobody's ordinary
use gets worse: every launcher (start menu, file manager, the rest) hands
the programs it starts a token, and the compositor can tell that a program
was started from the terminal the user is typing in, and treat that as the
user's action. Until both are in, new windows behave as on GNOME and KDE
today (§1386).

**What switches it.** `gui/compositor/src/activation.rs`, the `NewWindows`
policy: one call. It is switched when:

1. lanes C and E's launchers pass tokens on (requested 2026-10-10 in
   `requests/e-cf-a-consent-prompt-is-answered-only-on-purpose.md` and its
   companions), and
2. the compositor sees which process connected (the C library's channel
   calls, `requests/f-d-the-c-library-s-slateos-channel-calls.md`) and can
   ask which process started which, so a program a terminal's shell started
   while that terminal held the keyboard counts as asked for.

**Not chosen:** C, refusing as soon as the launchers pass tokens, terminals
or not -- every program started from a terminal would open behind it, which
is the most common complaint about GNOME's strict setting.
