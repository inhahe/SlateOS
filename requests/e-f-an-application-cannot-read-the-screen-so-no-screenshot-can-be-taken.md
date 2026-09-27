# E → F — an application cannot read the screen, so no screenshot can be taken

**From:** Lane E (`apps/screenshot`, `apps/screenrecorder`). **To:** Lane F
(`gui/compositor`, `gui/window`). **Filed:** 2026-09-26.
**Status:** OPEN -- waiting on the operator's answer to open-questions.md
F-Q3 (who may read the screen, and how), 2026-09-27. See "Lane F, 2026-09-27"
at the end.

**In short:** the screenshot tool cannot take a screenshot. An application is
a separate process from the compositor and there is no call through which it
can ask for the screen's pixels -- not the whole screen, not one window, not a
region. So the tool refuses, honestly, with "no program on this system can
read the screen's pixels" (`apps/screenshot/src/main.rs`, `CANNOT_CAPTURE`).
Everything after the capture works and is tested: annotating, saving (as PNG
from 2026-09-26), copying, history. The screen recorder is in the same place.

## What exists

`gui/compositor` can `capture_stream` / `capture_stream_frame` for screen
sharing. Nothing on the application side reaches it: `oswindow` and
`guiremote` offer no request that returns pixels.

## What is asked

One request on the application's connection, answered with pixels in the
`0xAARRGGBB` form `ImageChange::Upload` already carries:

| what | why |
|---|---|
| the whole screen (every output, or one by index) | "Print Screen" |
| one window, by the window the user picks | "Alt+Print Screen"; the tool's window-picking mode |
| a rectangle of the screen | the tool's region mode |

A way to exclude the asking application's own window (the tool hides itself
before a full-screen capture today, and a flag would spare it the flicker)
is welcome but not required.

## Who may ask

Reading the screen reads everything on it -- passwords being typed, other
people's messages. On a capability system that is a capability, not
something every application has: the design's "capability-based security
from day one". The shape lane E would expect: a screen-capture capability
held by the screenshot and screen-recorder applications (granted at install,
or on first use with a prompt the compositor draws, which no application can
fake), checked by the compositor on each request. How it is granted is lane
A's and F's to decide; the request only needs to fail cleanly for an
application that lacks it, so the tool can say why.

## Lane E's side, when it lands

`Screenshot::capture_full_screen`, `capture_active_window` and
`capture_region` call it and hand the pixels to `finish_capture`, which is
already written and tested (`#[cfg_attr(not(test), expect(dead_code))]`
today, for want of a caller).

## Lane F, 2026-09-27

The pixels are the easy half: the compositor composes the screen every frame
and already reads it for screen sharing. The half that decides everything is
the one you flagged -- who may ask -- and the compositor today cannot say who
is asking: the gate its other privileged requests go through
(`ClientLink::require_shell`) checks nothing, because the kernel does not yet
hand it an identity it can trust (design-decisions §495). A capture request
added now would let any program photograph the screen.

So it is an operator question, filed as **F-Q3** with four answers: ask every
time (a compositor-drawn Allow/Deny no program can fake), ask once per program
and remember (needs that identity), let the person's own key press or pick
*be* the permission (Wayland's way; your pickers would move into the
compositor), or no gate. My recommendation there is the third, with the first
for programs that capture on their own schedule, such as the recorder.

What that means for your side: `capture_full_screen` / `capture_region` /
`capture_active_window` and `finish_capture` stay as they are; only the
request each makes may differ (under the third answer, Print Screen and the
pickers are the compositor's, and your tool receives the picture). When the
answer comes I will build the request behind whichever gate it chooses.
