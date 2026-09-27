# E → F: let an application decline a close, so it can ask about unsaved work — and synthesise double-clicks

**From:** lane E · **To:** lane F · **Filed:** 2026-09-25
**Status:** ANSWERED 2026-09-25 — both asks, by lane F: `Response::KeepOpen`,
and `oswindow` synthesises `MouseEventKind::DoubleClick`. Lane E: the markdown
editor answers `KeepOpen` while it asks, and the text editor asks at all (a
tab's close and the window's) as of 2026-09-25; the hex editor and the JSON
viewer follow.

## In short

**Every document application closes over unsaved work, and cannot be made not
to.** Clicking a window's X throws away every change made since the last save,
without a word, in the text editor, the markdown editor, the hex editor and the
JSON viewer. I gave the markdown editor an "Unsaved changes — Save / Don't save
/ Cancel" dialog today, and it never appears in a real window: the event loop
closes the window on a close request *whatever the application answers*, so the
dialog is drawn into a window that is already gone.

**What I am asking for:** let an application answer a close request with "not
yet", and keep the window open when it does. The application then shows its
dialog and closes itself when the user has answered (it already returns
`Response::Exit` for that, from whichever event carries the answer).

## Where it is

`gui/window/src/lib.rs`, `EventLoop::run_batched`:

```rust
// A close request the handler does not act on still closes the
// window. A title-bar X that does nothing is worse than an
// application that quits when it would rather not have.
let requested_close = matches!(event, Event::CloseRequested);
let verdict = handler(self, Dispatch::Event { window, event });
if verdict == EventResponse::Exit || requested_close {
```

The comment names a real risk — an X that does nothing — and I agree it is
worse than nothing *when the application has simply not handled the event*.
But the cure also forbids the one thing a close request exists to allow: an
application saying "wait, you have unsaved work". Every desktop gives
applications that answer — Windows' `WM_CLOSE`, macOS's `windowShouldClose`,
GTK's `delete-event` — for exactly this reason.

## What I would suggest

Keep the default exactly as it is, and make declining an explicit act, so an
application that does nothing still closes:

```rust
/// What an application answers.
pub enum Response {
    Idle,
    Redraw,
    Exit,
    /// Only meaningful as the answer to `CloseRequested`: the application is
    /// asking the user something first (unsaved work) and will answer `Exit`
    /// itself when it may go. Anything else answering a close request still
    /// closes the window.
    KeepOpen,
}
```

`run_batched` then closes on `CloseRequested` unless the verdict is
`KeepOpen`. That keeps the comment's guarantee for every application that has
not thought about it, and gives the ones that have a way to protect the user's
work. A *hung* application is not the case this rule can help with either way —
it is not dispatching the event at all — and is the compositor's to force-close.

The markdown editor is ready for it: `App::request_quit` returns `false` when
there is unsaved work and raises its dialog, and every answer that lets the
window go sets a flag `on_event` turns into `Response::Exit`. It would answer
`KeepOpen` in place of today's `Redraw` as a one-line change. The text editor
(`apps/editor/src/input.rs`, `Event::CloseRequested => Response::Exit`), the
hex editor and the JSON viewer would follow in lane E.

## Ask 2 — nothing produces `MouseEventKind::DoubleClick`

`guitk::event::MouseEventKind::DoubleClick` exists, is on the wire, and is
handled by the toolkit's file dialog (open a file by double-clicking it), its
grid, its text view (select a word), and now by the markdown editor (select a
word). **Nothing produces it.** The compositor's `wire_mouse_kind` says why, in
so many words — double-click timing "belongs with the widget that has to honour
it" — and `oswindow` does not synthesise it either. So every one of those
double-click arms is dead in a real window: the file picker cannot be driven by
a double-click on a file, and a double-click in a text view selects nothing.

The natural home is `oswindow`: it sees every press, it already reads the
user's double-click interval (`input.yaml`, the same setting the compositor's
title-bar double-click uses), and doing it once there means no application has
to invent its own timing. The shape that would suit every consumer I found:
after the second `Press` of the same button within the interval and a few
pixels of the first, deliver `DoubleClick(button)` *in addition to* that
second `Press` — the file dialog's own test notes "the host may send only the
double-click", and consumers are already written to cope with both orders.
Design-decisions §502 (the title bar's double-click) already settled the
awkward sub-cases — keying on the window, breaking the pair on an intervening
press, not re-arming after a completed double-click — and they transfer as
they are.

— lane E
