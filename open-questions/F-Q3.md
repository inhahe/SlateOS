## F-Q3 — [F] Screenshots: how does a program get permission to read what is on the screen? — Status: OPEN (raised 2026-09-27)

**In short:** the screenshot tool cannot take screenshots, because no program
can ask the display server (the compositor, the program that draws every
window) for the screen's pixels. Adding that request is straightforward. The
hard part is who may use it: whatever is on the screen -- a password being
typed, someone's private messages -- would be readable by any program that
asks. Today the compositor cannot even tell which program is asking, so the
choice is how a program earns the right, and what the person at the screen
sees when it does.

**The options.**

| Option | *What changes:* |
|---|---|
| **A.** Ask every time | Before each capture the compositor shows its own "Screenshot wants to capture the screen -- Allow / Deny" box, which no program can draw or click for you. One extra click per screenshot. |
| **B.** Ask once per program, remember | The first capture asks as in A; after "Allow", that program captures without asking, until the permission is removed in Settings. |
| **C.** The user's own action is the permission | The compositor does the choosing: Print Screen, or its own region/window picker, captures and hands the picture to the screenshot program. A program cannot start a capture by itself at all. |
| **D.** No permission | Any program can read the screen whenever it likes, as on X11 Linux and on Windows. |

**What each means.**

- **A** needs nothing that does not exist yet, and it cannot be abused
  silently: every capture is something the person agreed to just then. The
  cost is the click, which a screen *recorder* pays once per recording and a
  screenshot tool once per shot.
- **B** is the convenient one (macOS works this way), but it depends on the
  compositor knowing *which program* is connected, which it cannot yet: the
  kernel does not pass it an identity it can trust (design-decisions §495,
  lane A's side). It also lets another program use the trusted one as a
  proxy -- for example by starting it with arguments that capture and save.
- **C** is the most secure and the smoothest for the common case (the key
  press *is* the consent -- the way Wayland desktops do it), but the tool's
  own region and window pickers would move into the compositor, and a program
  that needs pictures on its own schedule (a recorder, automation) would still
  need A or B.
- **D** is simple and dangerous: any program, including one downloaded a
  minute ago, could quietly photograph the screen.

**If never answered:** safe. The screenshot tool and screen recorder keep
saying honestly that they cannot capture; nothing else is affected. Lane F
builds the rest of the request -- the pixels, the protocol -- behind a gate
that refuses, so that answering this is the last step rather than the first.

**Claude's recommendation:** **C, with A for programs that capture on their
own schedule** -- the person's own key press or pick is the permission, and a
recorder asks once per recording. B once the kernel can say which program is
connected, if the prompt proves tiresome. Not D.

**Where it bites:** lane E's request
`requests/e-f-an-application-cannot-read-the-screen-so-no-screenshot-can-be-taken.md`;
`gui/remote` (a capture request), `gui/compositor` (its handler, and the
prompt or picker), `gui/window` (the call an application makes);
`apps/screenshot` and the screen recorder on lane E's side.
