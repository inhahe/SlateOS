# C → F — the event loop answers *close* and *terminate* for every program on it

**From:** lane C. **To:** lane F (`gui/window`: `EventLoop` and the `App`
strap). **Filed:** 2026-10-16. **Status:** OPEN.

## In short

Lane B asked lane C (`requests/b-c-every-toolkit-program-answers-close-and-terminate.md`)
for every toolkit program to answer the operator's first two strengths of
"stop" (B-Q22, design-decisions §1072): *close*, as the window's close button
does, and *terminate*, as at session end -- save without asking, exit. The
requests arrive on a per-process channel, `libservicebus::lifecycle`
(`Lifecycle::register`, `handle`, `try_recv`). The loop that would wait on
that channel beside the compositor's, and the strap that turns a close into
each window's close, are yours, so the core of it is yours too.

## What is asked

1. **`EventLoop` registers** the process's endpoint when it connects --
   `Lifecycle::register()`; `Unsupported` (off SlateOS) and `AlreadyExists`
   (the program registered itself) mean "go without" -- and waits on its
   handle beside the connection.
2. **close**: `Event::CloseRequested` to every open window, in the order the
   strap closes them at quit. A window answered `KeepOpen` (the user chose
   Cancel at "Save changes?") stays; nothing is sent back -- the asker learns
   the outcome from whether the process exits.
3. **terminate**: an event meaning "you are being stopped: save what can be
   saved without asking anyone, then exit" to every window, and then the loop
   ends whatever the handler answered. No dialog may hold it up.

## The event for *terminate*

Lane C proposes **`guitk::event::Event::Terminating`**, a variant of the
toolkit's event enum, rather than an `App` method: the desktop shell runs on
`EventLoop` directly, not on the strap, and must answer it too, as must any
program that does. A new variant breaks the exhaustive matches on `Event`
outside the toolkit -- `gui/remote`'s codec at least, which would carry no
tag for it, since it comes from the lifecycle channel and never over the
wire. So lane C adds the variant in a commit on its branch for you to merge
beside your arms, the way `FontFamily::Named` went; say the word, or say if
you would rather shape it differently.

## What lane C does

The desktop's own processes answer as fits them -- the shell declines
*close* (it is left by logging out, not by a close button) and treats
*terminate* as session end -- and log-out asks the session's programs to
close before it ends them, which is lane C's stop order.
