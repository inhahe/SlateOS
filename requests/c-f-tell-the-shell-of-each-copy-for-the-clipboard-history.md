# C → F — Tell the shell of each copy, for the clipboard history

**From:** Lane C (`gui/desktop`). **To:** Lane F (`gui/compositor`,
`gui/remote`, `gui/window`). **Filed:** 2026-10-06. **Status:** OPEN.

**In short:** the roadmap asks for a clipboard history -- a list of what was
copied recently, to pick an older copy from and paste it again (Windows'
Win+V; `roadmap-detailed.md` §3.5, *Clipboard history with view and
select*). The shell has the list drawn and tested already
(`gui/desktop/src/clipboard_viewer.rs`: entries, preview, search, pinning)
and nothing to fill it with. Since your clipboard went into the compositor
(`requests/c-f-carry-the-clipboard-over-the-compositor-connection.md`), the
compositor is the one place every copy passes through -- but only the window
with the keyboard may read it, so the shell sees a copy made in another
program only when the shell next gets the keyboard, and of several copies
made meanwhile, only the last. A history that misses most copies would be
worse than none.

## What is asked

**An event to the shell's connection, and to no other, for each copy:**
something like `Event::ClipboardChanged { text }` -- or the text left out and
a `GetClipboard` the shell may make without the keyboard, whichever you
prefer -- sent when any client's `SetClipboard` is accepted. Only the shell:
the reason the focus rule exists (a program in the background must not
collect what people copy) is the reason this must be a privilege of the
connection that holds the shell's key, as the window list's titles are
(`TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`, `--require-shell-key`).

The shell already takes over the clipboard the other way: picking an entry
in the history sets it with `SetClipboard` while the history's own surface
has the keyboard, which your focus rule already allows.

## What the shell does with it

- Keeps the history: each copy's text, newest first, with its time; the
  user's pins kept across sessions; a copy marked sensitive by its program
  (a password field's) never kept -- which needs a flag on `SetClipboard`
  some day, and until then nothing from a password field is copied at all
  (the toolkit's secret field refuses Copy).
- Super+V opens it (unbound by default, as §1416 has every optional chord);
  picking an entry puts it back on the clipboard for the next paste.

## If this is never done

There is no clipboard history. Copy and paste work as they do now.
