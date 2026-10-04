## 1337. A window pick is started by the program the user is working in, and the user's click on the compositor's crosshair is the consent

**Date:** 2026-10-03
**Lane:** F
**Decided by:** Claude (autonomous), for lane E's process explorer
(`requests/e-adf-what-the-process-explorer-still-cannot-ask.md`, part 3),
as lane F's reply there of 2026-09-27 proposed.

**In short:** a program can now ask the user to click a window and be told
which one it was: the window's title, its program and the process that
opened it, never its contents. While the program waits, the pointer is a
crosshair that only the compositor can draw, and the click it takes does not
reach the window it lands on. Only the program the user is working in can
start one, so a program in the background cannot quietly collect the next
click. Escape, any other mouse button, or the program itself ends it without
a choice.

**What was decided.**

- **Who may start a pick: the owner of the focused window**, the clipboard's
  rule (`focus_refusal` in `gui/compositor/src/wire.rs`). The user has just
  pressed that program's "pick" button, so its window has the focus. No
  capability is asked for. A pick reveals a title and a process id of a
  window the user chose to click, not pixels, and lane E's request asks for
  exactly that.
- **The user's click is the consent.** The crosshair is the compositor's
  (`Compositor::pointer` overrides the window's cursor, `Hidden` included),
  so no program can fake the mode or hide it. The click is taken by the pick
  and not delivered. Its release goes nowhere, by the rule for a release whose
  press no window saw.
- **Ways out:** Escape (swallowed with its repeats and release), any other
  button, `CancelPick`, the asking program going away, or a click where there
  is no window. Each is answered `Picked(None)`, so nothing is left waiting.
- **One pick at a time on the desktop.** A second program is refused while one
  is open, and a program asking again replaces its own earlier request.
- **The pid is the kernel's or none.** A window records its opener's
  attested pid at creation (`Window::owner_pid`, from `ClientLink::peer`, §1336).
  A window opened over TCP is picked with `pid: None`, never the
  per-connection number.
- **The click that wakes sleeping displays does not pick.** It reaches
  `absorb_while_asleep` first.

**Alternatives.**

| | For | Against |
|---|---|---|
| The focused program starts it; the click consents (chosen) | works today for any program; the user's intent is in both steps; nothing to grant | a program the user brought to the front can start one -- but then the user sees the crosshair, and Escape ends it |
| A capability the explorer holds | only the explorer could ever ask | the kernel can name the asker (§1336) but has no "pick" capability to check, and inventing one gates a title and a pid more tightly than the window list, which every shell reads |
| Any program, no focus rule | simplest | a program in the background could turn the user's next click anywhere into an answer for itself |

**Not decided here:** reading a window's *pixels*, which is
`open-questions.md` F-Q3 and stays the operator's question. A pick returns
nothing of a window's contents.

**2026-10-04: the route through `oswindow::app`** (lane E's
`requests/e-f-the-window-picker-has-no-route-through-oswindow-app.md`).
Nearly every application runs under `app::drive` and never holds the
`EventLoop`, so a pick is asked for through `App::take_pick` (drained after
every event, as `take_reloads` is) and answered through
`App::window_picked`, a refusal as its own case (`PickRefused`) rather than
folded into "nothing picked". Two things it settled:

- **An answer wakes the loop.** The click that ends a pick lands on another
  program's window and is not delivered, so the answer arrives with no event
  at all, and a loop parked on input would not read it until the user next
  touched the program. `EventLoop::run_batched` now hands over
  `Dispatch::Answered` when a reply the program is waiting on arrives -- a
  pick's or a display sleep's wake, the two requests answered by the user
  rather than the compositor -- once per answer.
- **One pick per program, held by the loop.** A second `Start` while one is
  open is ignored rather than sent: the compositor would replace the first,
  answering it as given up, and the application would hear two answers for
  one press. A pick still open when the loop ends is given up, so the
  crosshair does not outlive the program's interest in it.
