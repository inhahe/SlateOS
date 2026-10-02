## TD-C-FIFTEEN-PRIVATE-CLIPBOARDS-AND-A-SERVICE-NOBODY-TALKS-TO

**Date:** 2026-09-14. **Lane:** C. **OPEN.**

**2026-09-26: the way through is put to the operator** as `open-questions.md`
C-Q29 -- carry copy, paste and drag on the connection every program already
has to the window system, which does not wait on A-Q15 (point 2 below), or on
a second connection to the clipboard program, which does.

**In short:** copying something in one program and pasting it into another does
not work anywhere in this system, and the reason is not a bug in the copying.
**Fifteen** programs each keep a private clipboard of their own, and the one
clipboard *service* that exists is connected to none of them. Copy and paste
works perfectly inside any single program and cannot cross between two. The clearest symptom:
the emoji picker cannot give you an emoji. You can browse them, tint them and
pick one, and nothing leaves the program.

**The four:**

| where | what it is | who it talks to |
|---|---|---|
| `gui/clipboard/src/main.rs` | a **service**: ring buffer of 50 clips, format negotiation, 30-second expiry for sensitive entries. Its own doc says "all applications communicate with this service via IPC" | nobody. It is a `main.rs` with no library beside it, so nothing can link it, and no program opens a connection to it |
| `gui/desktop/src/clipboard_viewer.rs` | history viewer with preview, search, pinning. Its doc says "Integrates with the gui/clipboard service" | nobody. It contains no IPC, no socket, no connect and no send. It views an in-process model of its own |
| `apps/credmanager` | `ClipboardState` with auto-clear, marked "(simulated)" | itself |
| `apps/emojipicker` | `last_selected`, documented "for clipboard / IPC output" | nothing. There is no clipboard or IPC code in that program at all |
| `apps/remotedesktop` | clipboard **sync** state -- mode, direction, content type, size, count -- with a settings panel that displays four of them | nothing. `sync_clipboard` is called only from its own tests, so the panel shows "Never / Text / 0" for ever |

Those are the five that were examined closely. **The real count is fifteen**,
and getting there took three tries, which is the part worth keeping:

* "four" -- the ones I had opened while tracing one field;
* "five" -- after triaging one more baselined field, and I wrote beside it that
  the number now came "from a sweep", which was **false when I typed it**. No
  sweep had been run. It was the same sentence-shaped confidence that made
  "only `sysinfo` collides" wrong earlier in the day;
* "fifteen" -- from actually running it.
  `grep -rlnE "^\s*(pub )?clipboard: |struct [A-Za-z]*Clipboard"` over `apps/`
  and `gui/` returns fifteen crates, and a looser grep for the word returns
  twenty-three. Among them `apps/clipmanager`, an entire second clipboard
  *manager* program, and `apps/editor`, whose `self.clipboard` means you can
  copy inside the editor and nowhere else.

A count is a measurement. Three times in one entry I reported one without
taking it.

**How this was found.** `scripts/check-fields-written-never-read.py` reported
`last_selected` as written in production and read only by tests. The field's own
doc comment names the missing mechanism, which is what turned a one-field
finding into this one: the picker records which emoji you chose, re-tints it
correctly, stores it in exactly the right place, and there is nowhere for it to
go. Every part works and the program does nothing.

**Why it is not simply "wire the picker to the service".** Two things sit in
front of that, and the second is not lane C's:

1. **There is no client.** The service is a binary. An application cannot call
   it without an IPC client API, and none exists. Whoever writes the first one
   is choosing the clipboard protocol for the whole system, which is why this
   is written down rather than done in passing.

2. **A-Q15 says a program can only have one socket open at a time.**
   `create_kind` calls `NetstackConn::open()` per socket, each of which calls
   `shm::create` for a fresh ring, while the daemon holds one `RingSession`
   and tears down `conns` *and* `listeners` on a different handle
   (`services/netstack/src/main.rs:2270`, 2594). Every windowed program's
   socket number one is its compositor connection --
   `gui/remote/src/socket.rs` is TCP -- so a program that opens a second
   socket to reach the clipboard does not see that call fail. It sees its
   window die, in a subsystem nobody would think to suspect. The bug report
   would read "the emoji picker closes itself when I click an emoji".

   `services/**` is lane B's, so the daemon fix is not lane C's to make.

**`gui/toolkit` is not a sixteenth, and that was worth checking.** It mentions
the clipboard in comments and in one `textview` method that "optionally returns
a clipboard string (on Ctrl+C)" -- the widget hands the host a string and the
host decides what to do with it, which is the toolkit's usual pure-widget
contract. So there is genuinely no client anywhere, rather than one I had not
found.

**The shape is familiar and worth naming.** This is the same defect as C-Q20's
four separate lists of installed programs: several complete implementations of
one idea, each correct in isolation, none able to read another. In both cases
nothing is broken *inside* any of the four, so nothing is red, and the system
still cannot do the thing.

**What the proper fix looks like,** in order: A-Q15 first, because without it
the second socket is a window-killer; then a client library beside the service
(`gui/clipboard` gains a `lib.rs`, the binary keeps `main.rs`); then the four
consumers move onto it and `clipboard_viewer`'s claim about integrating becomes
true.

**A route that needs neither, proposed 2026-09-26:** carry the clipboard over
the compositor connection every window already has -- `SetClipboard` and
`GetClipboard` requests beside the window verbs, the compositor holding the
selection and handing it to the `gui/clipboard` service for history -- as
Wayland and X do. No second socket, so A-Q15 stops being in the way. It is
lane F's protocol: `requests/c-f-carry-the-clipboard-over-the-compositor-connection.md`. The emoji picker is the smallest possible first consumer and a good
acceptance test: one string, one direction, and you can see whether it worked
by pasting.
