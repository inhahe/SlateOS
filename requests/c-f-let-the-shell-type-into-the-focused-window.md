# C → F — Let the shell type a picked character into the focused window

**From:** Lane C (`gui/desktop`, `gui/charpicker`). **To:** Lane F
(`gui/compositor`, `gui/remote`, `gui/window`). **Filed:** 2026-10-06.
**Status:** OPEN.

**In short:** there is now one "Emoji & Symbols" picker (`gui/charpicker`,
`design-decisions.md` §1481). The shell opens it over its own text fields and
types what is picked into them. It cannot do that for any other program: the
tray's emoji entry (`design.txt` line 711, "emoji input" among the tray's
icons) and a system-wide chord would pick a character and have nowhere to put
it, because nothing lets the shell hand text to the window that has the
keyboard. The same gap blocks the roadmap's "a hotkey types a chosen
character or string" (`roadmap-detailed.md`, *Hotkey → emit an arbitrary
emoji / Unicode character or string*: "inserts that text into the focused
input via the same synthetic-input ... path the OS uses for other text
injection").

## What is asked

A way for the **shell's** connection -- and no other client's -- to say "type
this text into the window that has the keyboard focus", which the compositor
delivers to that window as a key event that types the text and is nothing
else:

```rust
KeyEvent {
    key: Key::Unknown(0),      // or a dedicated "no key" code, if you prefer one
    pressed: true,
    modifiers: Modifiers::NONE,
    text: "\u{1F44B}\u{1F3FD}".to_string(),
}
```

-- with no release following it, as a key that came from no keyboard has none
to report. Every toolkit field already inserts `key.text` for such a key: the
shell's own fields take a pick exactly this way today
(`gui/desktop/src/char_picker.rs`, `DesktopShell::type_into_field`), and
`guitk::textinput::TextInput::edit_key`, `TextArea` and the rich input insert
the text of any key that types and is not a Ctrl chord.

### Why the shell only

A client that could type into other windows could type a command into a
terminal the user has open. The shell already speaks for the user to the
compositor (`ShellControlAction` over its control connection), and it is the
one program that hosts the picker and the hotkeys; refusing the request from
any other connection keeps "type into another program" an act of the user's.

### The shape on the wire

Yours to choose. `ShellControlAction` is one byte per action and addressed
to a window; this is neither (it carries a string, and goes to whichever
window has the focus *when it arrives* -- the user may have clicked elsewhere
since the pick). A message of its own on the control connection, carrying
UTF-8 text with a length limit (a pick is one grapheme; a hotkey's snippet is
a short string -- 4 KiB is generous), seems the natural fit.

### What lane C does when it lands

- the tray's emoji entry opens the picker and sends the pick;
- a system-wide chord (Super+. , as Windows has it) does the same over any
  program's field;
- the shortcut editor offers "Type text…" as an action, bound to a string
  typed or chosen in the picker.

Nothing in lane C waits on this beyond those three; the picker itself and the
shell's own fields are done.
