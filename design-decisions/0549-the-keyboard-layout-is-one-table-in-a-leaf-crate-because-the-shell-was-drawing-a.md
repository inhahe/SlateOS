## 549. The keyboard layout is one table in a leaf crate, because the shell was drawing a keyboard nobody could type on

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Choosing a keyboard layout in the desktop shell used to change
the *picture* of the keyboard and nothing else — you would pick French AZERTY,
watch the tray say `FR` and the preview redraw with AZERTY caps, and then type
QWERTY, because the code that turns a key-press into a letter lived somewhere
else entirely and had never heard of your choice. There were two hand-written
tables describing one keyboard, and only one of them was the keyboard. They are
now a single table in a small crate that both sides read: the compositor to
decide what you typed, the shell to draw what the keys say.

### The shape of the problem

Two stores had grown up on either side of a process boundary:

| | Held | Read by | Effect on what you type |
|---|---|---|---|
| `gui/compositor/src/keymap.rs` | one US-QWERTY scancode table | every keystroke in the system | total |
| `gui/desktop/src/input_method.rs` | five layouts as row strings | the preview popup | none |

Neither could be deleted in favour of the other. The compositor's table is
where translation *has* to happen — `design-decisions.md` §456 put it there so
one system keymap governs all 138 client crates and a layout change takes
effect everywhere at once — but it had no letters in it beyond US QWERTY. The
shell's tables had the letters but no path to the keystroke.

### The decision

A third place: `gui/keylayout`, a crate with **no dependencies at all**. That
is the load-bearing property, not an aesthetic one. The compositor must not
link a widget toolkit, and the shell must not link a display server; anything
`keylayout` depended on would become a dependency of both. It holds the eight
built-in layouts and the level model, and nothing else.

**A layout is 48 keys.** Four rows (13/12/12/10) plus the key ISO boards have
between left Shift and `Z`. Enter, the F-row, the arrows and the keypad mean
the same thing on every layout in the world, so they stay in the compositor's
physical table and a layout cannot get them wrong.

**Layouts are written as row strings and expanded.** Four unshifted and four
shifted strings per layout — the shape you can check against a photograph of
the board — expanded at first use into per-key records. This is what makes the
drift structurally impossible rather than merely fixed: the preview draws
`layout.row(n)` and the compositor calls `layout.character(scancode, level)`,
both derived from the same eight strings.

**Four levels, not two.** Plain, shift, AltGr, AltGr+shift. AltGr is a *level
shift*, not a modifier: on a German board AltGr+Q is the only `@` there is. And
Caps Lock is not a level at all but a case latch that applies only where both
faces of a key are alphabetic, which is why `ß`/`?` does not latch and `a`/`A`
does.

### Two enums that could not be touched, and what that cost

Both `guitk::Key` and `guitk::Modifiers` are fixed points for lane C.
`Modifiers` alone is constructed as a literal 99 times across 33 files, one of
them lane A's `kernel/src/fs/kbshortcuts.rs`, so adding a field would red-tree
another lane's file. Neither gained anything:

- **A character `Key` cannot name** — German's `ü` — travels on the
  `character: Option<char>` channel `EventNotification::KeyEvent` already
  carried, and `key` falls back to the physical US identity so a shortcut bound
  to that *key* still fires.
- **AltGr is signalled by clearing `modifiers.alt`** when a keystroke resolved
  through the third level, rather than by adding an `alt_gr` field. Same
  observable effect for the case that matters: without it a German user typing
  an e-mail address sends every application an Alt+Q, and the menu bar answers
  first.

The alternative — proposing a `guitk` change through the `requests/` dropbox and
waiting — would have blocked the whole feature on another lane's schedule for a
gain that is invisible to users. If a client ever needs to distinguish "AltGr
was physically held" from "no Alt", that is the point to file the request.

### What was dropped rather than honoured: `is_rtl`

`known-issues.md` had flagged the shell's unread `is_rtl` field with the
suggestion that the preview should honour it — otherwise "an Arabic or Hebrew
layout would preview with its keys mirrored". On inspection that is backwards.
A right-to-left script is a fact about the *text a layout writes*, not about
where its caps sit: Arabic and Hebrew keyboards put their letters on physically
standard positions (both ض and / are on the key engraved `Q`), and a preview
that mirrored its rows for them would draw a keyboard that does not exist. A
flag that no correct code would read is not a stub, it is a wrong answer
waiting to be believed, so it is gone. `language` stays, because it draws a
real distinction with a real reader: only the English layouts are required to
reach the whole alphabet from the plain level.

### What is deliberately still missing

**Dead keys** (`´` then `e` → `é`) and **compose sequences**. Both need state
carried between two key events, which turns translation from a pure function
into a machine with a memory. No layout that requires them is offered, which is
the honest position: an unfinished dead-key implementation would make the
layouts that need it *look* available.

**Left/right-hand Dvorak and Programmer Dvorak** are absent because their exact
rows were not verifiable here, and a layout typed from memory is worse than an
absent one — the user has no way to tell a wrong table from a broken keyboard.
