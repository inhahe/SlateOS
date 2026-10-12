## 1607. The desktop shell is the notification system

**Date:** 2026-10-12 &middot; **Decided by:** Operator (Claude recommended A; the operator chose A) &middot; **Lane:** C

**In short:** two half-built notification systems existed: the desktop
shell, with the notification pane, Do Not Disturb and the user's rules, and a
separate program (`gui/notifications`) that drew pop-ups and kept a list of
its own, started by nothing (`open-questions.md` C-Q32). The operator chose
the shell. Its pop-ups were already built ahead of the answer (§1447); the
separate program is retired, and programs will send their notifications to
the shell.

**The answer, verbatim** (`operator-answers/2026-10-09-open-questions-answers.txt`):

> C-Q32: A

**What it obliges:**

| Part | What | Whose |
|---|---|---|
| Pop-ups, the pane, Do Not Disturb, the rules | in the shell -- done (§1447, §1468, §1495) | lane C |
| The separate program | retired: what it alone had is carried into the shell first, as §1423 asks, then `gui/notifications` is deleted | lane C |
| A channel for programs to send on | the window system's connection, which every program with a window holds -- asked of lane F now that it is decided, as C-Q32 promised | lane F, then lane C |
