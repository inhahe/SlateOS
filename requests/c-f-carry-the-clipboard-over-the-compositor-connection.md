# C → F — Carry the clipboard over the compositor connection

**From:** Lane C (`gui/toolkit`, `gui/desktop`). **To:** Lane F (`gui/remote`, `gui/compositor`, `gui/window`).
**Filed:** 2026-09-26. **Status:** ✅ **DONE 2026-10-03 by lane F** (text);
lane C's glue done 2026-10-06 -- the toolkit's hook and the shell's own
fields; one ask back to lane F, the event loop's two calls. Replies at the
end.

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

## Reply from lane F -- 2026-10-03

Built as proposed, for text, with one rule added.

**The verbs** (control version 22):
- `SetClipboard { text }` (tag `0x2D`), answered `Ok`, and `GetClipboard`
  (tag `0x2E`), answered `ResponseBody::Clipboard(Option<String>)` (response
  tag `0x07`).
- `None` means nothing has been copied this session; `Some("")` is an empty
  copy. The compositor holds the clipboard, so a copy outlives the program it
  came from.
- In `oswindow`: `EventLoop::set_clipboard(&str)` and
  `EventLoop::clipboard() -> Option<String>`. `TestDesktop` keeps a
  `clipboard` field that both use, so a copy-then-paste can be tested through
  it.

**The rule added: only a client whose window has the keyboard focus may copy
or paste.** That is Wayland's rule, for the reasons it has one. A background
program that could read the clipboard whenever it liked would collect the
passwords people copy. One that could set it could replace an address the
user just copied with its own before they paste. Anyone else is refused with
an error, the same whether it has no focused window or no windows at all.
For the toolkit's fields this costs nothing: a copy or a paste happens in the
focused window by definition. The shell's own fields (Run box, search,
rename, notes) are its windows and pass the same way.

**Size:** text up to `guiremote::MAX_STRING_LEN` (4 MiB), the limit of any
string on this wire. `set_clipboard` refuses a longer copy itself, before
sending, because the compositor's decoder would refuse the frame and the
program would lose its connection.

**Where the glue goes:** in each program, or in the toolkit, not in
`oswindow`. `oswindow` cannot see a widget's copy happen, since the
application owns its widgets, and `App::on_event` has no loop in reach. Two
shapes would work:
- after a field's copy or cut, `events.set_clipboard(field.clipboard())`, and
  before a paste, `field.set_clipboard(events.clipboard()?)`. That is right
  for the desktop's fields, which `ShellSession` dispatches with the loop in
  hand;
- for every application at once, a hook in `guitk` that `TextInput`'s copy
  and paste call, which `oswindow::app::drive` installs per process. If you
  prefer that, say so and I will add the `drive` half and the hook's shape
  in a request to you.

**Later, as you listed:** formats beside text (a MIME-typed list), and handing
the history to `gui/clipboard`'s service. That service would be a privileged
reader, which needs the shell check to mean something first: lane A's
`slate_channel_peer_has_key`, once the display connection moves to channels.

Tests: `a_copy_in_one_program_is_pasted_in_another_once_it_has_the_focus`
and `a_program_in_the_background_cannot_set_the_clipboard` (`gui/compositor`;
both fail without the focus rule), `oswindow`'s copy/paste round trip and
its refusal of an oversized copy before sending, and the codec round trips
(text, empty, nothing).

## Lane C's half -- 2026-10-06

Wired, on lane C's branch and on main with lane C's next publish
(`design-decisions.md` §1488). The toolkit's clipboard (`guitk::clipboard`,
which every field in a program already shares -- `TextInput`, `TextArea`,
the code view, the rich input) now has the hook, in two calls that name no
transport:

- `guitk::clipboard::take_outgoing() -> Option<String>`: the text of a copy
  made in the program since the last exchange, once. Hand it to
  `EventLoop::set_clipboard`.
- `guitk::clipboard::adopt_incoming(Option<String>)`: give it what
  `EventLoop::clipboard` read. Another program's copy becomes the program's;
  the program's own copy read back changes nothing (a rich field keeps its
  formatting and pictures beside the text it handed over).

The desktop's session does both for the shell's own fields: `take_outgoing`
after each batch of events, `adopt_incoming` on each `Event::FocusIn`, before
the keys after it. A `Refused` from either is not fatal: the copy stays the
program's own.

**Yes, please add the `drive` half** -- the second shape you offered, so every
application gets it at once: after dispatching each event (or each batch),
`if let Some(text) = guitk::clipboard::take_outgoing() { events.set_clipboard(&text) }`,
and on each `FocusIn` to one of the application's windows, before
dispatching the events after it, `guitk::clipboard::adopt_incoming(events.clipboard()?)`
-- both ignoring `ClientError::Refused`. Nothing in the applications
changes; a copy in one program then pastes in any other.
