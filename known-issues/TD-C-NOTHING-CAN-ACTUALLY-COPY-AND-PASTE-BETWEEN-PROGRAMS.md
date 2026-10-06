### TD-C-NOTHING-CAN-ACTUALLY-COPY-AND-PASTE-BETWEEN-PROGRAMS — 2026-08-26 — LANE C, OPEN

**Update 2026-10-06 -- the shell's half is done; the programs' half is one
ask to lane F.** The clipboard now travels through the window system for
text: lane F's compositor holds it (`SetClipboard`/`GetClipboard`, only for
the window with the keyboard), and the toolkit's clipboard -- which every
field in a program shares -- is joined to it by two calls
(`guitk::clipboard::take_outgoing`, `adopt_incoming`; `design-decisions.md`
§1488). The desktop's session makes both for the shell's own fields, so a
copy in the Run box pastes in any program that reads the system's clipboard
and the reverse. Ordinary programs get it when `oswindow::app::drive` makes
the same two calls, asked in
`requests/c-f-carry-the-clipboard-over-the-compositor-connection.md`; the
apps that keep a private `String` (below) are lane E's to move onto the
toolkit's clipboard. Pictures and formats beside text wait on the system's
clipboard carrying them. Which transport is to stay is still C-Q29's.

**In short:** Copy and Paste do not cross between programs. Every window that
has a Copy button — the colour picker, the text editor, the clipboard manager,
the Run box — copies into a `String` field of its own, which no other program
can see. Copying a hex colour and pasting it into the editor puts nothing in
the editor. The clipboard *service* that is supposed to hold the one shared
copy exists and is written, but nothing is connected to it at either end.

**Where:**

| Piece | State |
|---|---|
| `gui/clipboard/src/main.rs` | The service. Formats, history ring, sensitive-entry expiry and a full request/response enum are all written and tested. `fn main` runs a self-test and returns; three `TODO`s at lines 788-790 mark the missing register-with-service-manager, open-endpoint and event-loop steps. |
| `gui/clipboard/Cargo.toml` | **Binary only — there is no `lib` target**, so no program can even link against `ClipboardRequest`/`ClipboardResponse` to build a message. |
| `gui/toolkit` | No clipboard API of any kind. A widget that wants to copy has nowhere to call. |
| `apps/colorpicker`, `apps/editor`, `apps/tmux`, `gui/desktop/src/run_dialog.rs` | Each keeps a private `clipboard: String`. Correct in isolation, invisible to everyone else. |
| `apps/clipmanager` | A clipboard *manager* over its own in-process store: it shows and searches a history the rest of the system does not put anything into. |

**Why it is like this:** the service was written before there was any IPC to
carry it, and each app grew a local `String` in the meantime because a Copy
button that does nothing at all is worse than one that at least feeds the
program's own Paste. Nothing here is wrong so much as unconnected — the shape
of the fix is not in doubt, only the plumbing.

**Proper fix, in the order the pieces have to land:**

1. **Give `gui/clipboard` a `lib` target** (`src/lib.rs` holding the types,
   `src/main.rs` reduced to the service loop). Without this there is no shared
   vocabulary and every caller would invent its own message encoding.
2. **Finish the service loop** — register with the service manager, open a
   channel endpoint, and dispatch `ClipboardRequest` off it. That is the
   existing three `TODO`s, and it is a lane B/A dependency: it needs the
   service-manager registration path and channel IPC to be callable from a
   userspace binary.
3. **Add `guitk::clipboard`** — a `get(format)` / `set(entry)` pair over that
   channel, which is what every widget and app should call. This is the piece
   lane C owns, and it is the one that deletes all four private `String`s.
4. **Rewire the four apps and `clipmanager`** onto it. `clipmanager` in
   particular should be reading the service's history rather than its own,
   which is the whole point of the program.

Nothing above should be started before step 2 is possible, and step 2 is not
lane C's to make possible; this entry is here so that whoever gets there does
not conclude the service is missing and write a second one.
