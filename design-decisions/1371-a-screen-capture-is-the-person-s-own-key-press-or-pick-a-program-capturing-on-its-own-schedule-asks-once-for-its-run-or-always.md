## 1371. A screen capture is the person's own key press or pick; a program capturing on its own schedule asks once, for its run, or always

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator ("F-Q3: Claude's recommendation, but for programs that capture on their own schedule, I think we should have three options: Just this once, every time for this instance of the program, or forever?", `operator-answers/2026-10-09-open-questions-answers.txt`), answering `open-questions/F-Q3.md`. Claude recommended C with A for scheduled capture; the operator added the three choices.

**In short:** no program may read the screen's pixels on its own say-so. When
the person presses Print Screen, or picks a region or window in the
compositor's own picker, that action is the permission: the compositor (the
program that draws every window) takes the picture and hands it to the
screenshot program. A program that needs pictures on its own schedule -- a
screen recorder, automation -- asks through a prompt only the compositor can
draw, offering three answers: just this once, for as long as this program
runs, or always.

**Decision.**
- **The person's action is the permission** (F-Q3's option C). Print Screen
  and the compositor's own region and window picker capture; the picture goes
  to the program that asked for a capture to be offered. A program cannot
  start one of these by itself.
- **Scheduled capture asks** (option A, with the operator's three choices):
  "Just this once", "Every time while this program runs", "Always". The
  prompt is the compositor's, drawn where no program can draw or click it.
- **"Always" is kept in lane A's store of always-allowed grants**, keyed by
  the program's fingerprint (A-Q26, §1568), not in a second store of the
  compositor's: lane A's note of 2026-10-09 (`operator-answers/2026-10-09-open-questions-claude-answers.txt`, on lane B's branch until it reaches main) points out the three choices are A-Q26's own.
- **The compositor knows who is asking**: the kernel now passes a caller's
  identity over its channel (`SYS_CHANNEL_PEER_CRED`, 286;
  `SYS_CHANNEL_PEER_HAS_KEY`, 1103), which F-Q3 described as missing.

**Rationale.** Every capture is either something the person did just then, or
something they agreed to for a span they chose. Nothing captures silently, and
the common case -- a screenshot -- costs no click at all.

**Alternatives.** B alone (ask once per program, remember): convenient, but a
trusted capturer can be used as a proxy by other programs. D (no permission):
any program could photograph the screen.

**What it asks, and where it will be done.** `roadmap.md` (lane F): the
capture request in `gui/remote`'s protocol, its handler and the picker and
prompt in `gui/compositor`, the call in `gui/window`; lane E's
`requests/e-f-an-application-cannot-read-the-screen-so-no-screenshot-can-be-taken.md`
is answered by it. Until built, the screenshot tool keeps saying it cannot
capture.

**How to reverse.** The choices live in the compositor's capture handler and
its prompt; dropping one is local to them.
