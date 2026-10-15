## 1392. A desktop a person can use, and a test that checks it, is every lane's next goal

**Date:** 2026-10-11
**Lane:** F
**Decided by:** Operator (F-Q15, `operator-answers/2026-10-11-open-questions-answers.3.txt`:
"A"; Claude's recommendation)

**In short:** SlateOS boots in the emulator and passes thousands of
self-tests, but starts no desktop: the compositor, the desktop, the login
screen and the apps have never been on its disk image, and nothing checks
that a person could use it. Every lane now puts its step on the path to a
usable desktop ahead of its own list, until a boot test boots to the desktop,
checks the screen and passes -- and from then on fails when the desktop
stops working.

**The path, and whose each step is:**

| Step | Lane |
|---|---|
| The graphical programs built for SlateOS and put on the image (`requests/f-d-the-image-carries-no-graphical-program.md`) | D |
| `init` starts the compositor with its permissions (`requests/f-bd-the-display-service-needs-two-grants-and-a-flag-from-the-session.md`) | D, B |
| An emulator window a person can use -- screen, keyboard, mouse -- and a guest that can run a graphical program (`requests/f-a-the-guest-cannot-run-a-graphical-program-or-show-its-screen.md`) | A |
| The compositor shows a picture and takes keys and clicks on SlateOS | F |
| The login screen, then the desktop shell, start | B, C |
| A terminal and the apps open from the desktop | C, E |
| A boot test that boots to the desktop and checks it, with QEMU's screen capture and simulated input | A, with each lane's part |

**What "ahead of its own list" means.** A lane with a step above that is not
done works on it first. A lane whose step waits on another's does the work
that can be done before it (its half of an interface, a test against a
stand-in) and otherwise goes back to its own list -- waiting idle is not what
this asks.
