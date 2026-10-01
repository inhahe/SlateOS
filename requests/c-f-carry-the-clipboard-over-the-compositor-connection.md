# C → F — Carry the clipboard over the compositor connection

**From:** Lane C (`gui/toolkit`, `gui/desktop`). **To:** Lane F (`gui/remote`, `gui/compositor`, `gui/window`).
**Filed:** 2026-09-26. **Status:** OPEN -- a proposal; the design is yours.

**In short:** copying in one program and pasting into another works nowhere
on SlateOS (`known-issues.md`
`TD-C-FIFTEEN-PRIVATE-CLIPBOARDS-AND-A-SERVICE-NOBODY-TALKS-TO`). Every text
field keeps a private clipboard -- the toolkit's `TextInput` and the new
multi-line `TextArea` included -- and the clipboard service in
`gui/clipboard` has no clients. The route written down so far is a client
library that opens its own socket to that service, and it is blocked twice:
on `open-questions.md` A-Q15 (a program's second socket takes down its
first, which is its compositor connection, so the first program to copy
would lose its window) and on nobody having chosen the protocol.

**The proposal:** every windowed program already has one connection, to the
compositor, and it carries requests with replies (`CREQ`/`CRSP`, correlated
by `seq`). The clipboard can ride on it, as it does on Wayland (the
compositor mediates the selection) and on X (the server does):

- `SetClipboard { text }` -- the program's copy or cut. No reply needed.
- `GetClipboard` -- answered with the text, or with nothing. A paste.
- Later, formats beside text (`roadmap-detailed.md` §3.5 asks for text,
  HTML, image and structured data) as a MIME-typed list, and the history
  the `gui/clipboard` service keeps -- the compositor can hand what it holds
  to that service over its own connection, so the service keeps its job
  and no application needs a second socket.

This needs neither A-Q15 nor a new socket, and it puts the clipboard where
the other thing every window shares already is.

## What lane C does once the verbs exist

`TextInput` and `TextArea` already separate the field's copy/paste from the
store: `clipboard()` is what a copy put aside and `set_clipboard(..)` what a
paste will insert. The glue is small -- after a copy or cut, send
`clipboard()` with `SetClipboard`; before a paste, fill it from
`GetClipboard` -- and the desktop's own fields (Run box, start menu search,
icon rename, notes) take it first. Where the glue belongs -- in `oswindow`,
so that every application gets it, or in each program -- is part of your
design.

## What happens until it is done

Copy and paste keep working inside each program and nowhere across them.
