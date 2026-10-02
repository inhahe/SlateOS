## 1309. An application may decline a close request -- in so many words

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous), at lane E's request
(`requests/e-f-let-an-application-decline-a-close-so-it-can-ask-about-unsaved-work.md`,
which proposed exactly this shape).

**In short:** clicking a window's close button used to close it whatever the
application answered, so no editor could stop to ask "save your changes?" --
the text editor, the markdown editor, the hex editor and the JSON viewer all
threw away unsaved work without a word, and the markdown editor's dialog was
drawn into a window that was already gone. An application can now answer a
close request with "not yet" (`KeepOpen`), show its question, and close itself
when the user answers. One that does not answer that way still closes exactly
as before, so no window can end up with a close button that does nothing.

### What changed

`gui/window`: `EventResponse::KeepOpen` for the event loop and
`Response::KeepOpen` for `oswindow::app`. `run_batched` still ends the loop on
`CloseRequested` -- unless the answer to it is `KeepOpen`. To any other event
the new answer is `Continue` (and, for an `App`, `Redraw`). The compositor
already only *asked*: its close button and `request_close` send the client a
close request rather than destroying the window, so the whole change is on the
client's side.

### The choices with two sides

1. **Declining is an explicit answer, not "anything but Exit".** *For:* the
   rule it replaces had a real reason -- most applications answer events they
   do not handle with `Idle`, and a close button that does nothing is worse
   than an application that quits when it would rather not have. That still
   holds for every application that has not thought about closing. *Against:*
   an application has to know the new answer exists to use it; that is the
   point, and every desktop's equivalent (`WM_CLOSE`, `windowShouldClose`,
   GTK's `delete-event`) is equally explicit.
2. **`Response::KeepOpen` redraws.** *For:* an application declining a close is
   about to show something -- its question -- and one that forgot to ask for a
   frame would leave the user clicking the X and seeing nothing happen, which
   is the very failure the default guards against. *Against:* a frame nobody
   needed, if an application declines without drawing anything new; that costs
   one frame.
3. **A hung application is not this rule's to help.** It is not dispatching
   events at all, so it cannot answer either way; closing one that has stopped
   responding is the compositor's, and is not built yet.

### How it is held

`gui/window` tests: a close answered `KeepOpen` leaves the loop running until
the application's own `Exit`; `KeepOpen` to anything else is `Continue`; a close
answered anything else still ends the loop (the existing
`run_ends_on_a_close_request_the_handler_ignores`); and through `App`, a
declined close keeps the window and draws the question. Reverting the loop's
condition fails the first and the last.
