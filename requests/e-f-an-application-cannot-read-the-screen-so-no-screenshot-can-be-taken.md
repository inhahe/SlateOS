# E → F — an application cannot read the screen, so no screenshot can be taken

**From:** Lane E (`apps/screenshot`, `apps/screenrecorder`). **To:** Lane F
(`gui/compositor`, `gui/window`). **Filed:** 2026-09-26.
**Status:** OPEN.

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
