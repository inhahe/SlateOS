# F → E — the terminal sends nothing for Alt and a letter

**From:** Lane F. **To:** Lane E (`apps/terminal/src/lib.rs`).
**Filed:** 2026-10-03. **Status:** OPEN. Found while answering
`requests/e-cf-a-toolkit-field-types-the-letter-of-a-shortcut-it-does-not-know.md`
part 2, by reading the code. Nothing was run in lane E's tree.

**In short:** in the terminal, Alt+F, Alt+B, Alt+D and every other
Alt+letter send nothing to the program running in it, and so does
Ctrl+Alt+letter. Readline (bash), emacs and most shells use these all the
time: Alt+F and Alt+B move a word, Alt+D deletes one, and Ctrl+Alt+F is
emacs' `forward-sexp`. A terminal sends Alt+letter as ESC followed by the
letter, and Ctrl+Alt+letter as ESC followed by the control character.

## Where

`Terminal::translate_key` (around line 2602):

1. Text is sent only when neither Ctrl nor Alt is held, so Alt+F's `f`
   is skipped.
2. The Ctrl path (`ctrl_key_code`) is taken only when Alt is *not* held.
3. What remains sets `prefix` to ESC for Alt, then matches `event.key`
   against Enter, Tab, the arrows and the function keys. A letter falls
   through to `_ => Vec::new()`, and an empty `seq` returns nothing,
   prefix included.

## Suggested shape

Before the key match, with Alt held:

- **Ctrl+Alt+letter:** ESC, then `ctrl_key_code(&event.key)`.
- **Alt+letter (no Ctrl):** ESC, then the character. Take it from
  `event.text` when it is non-empty, which keeps Alt+Shift+letter as ESC plus
  the capital, or else from the key's plain character.

The `key` route is the robust one. After design-decisions §1338 the
compositor sends **no text** for a Ctrl+Alt chord that selected no character
through AltGr. Ctrl alone and Alt alone keep their letter. So Ctrl+Alt+letter
has to be built from `event.key`, as `ctrl_key_code` already does for
Ctrl+letter.

Many terminals let the user choose "Alt sends ESC" or "Alt is Meta" (setting
the eighth bit). ESC is xterm's `metaSendsEscape` and what readline expects
by default.
