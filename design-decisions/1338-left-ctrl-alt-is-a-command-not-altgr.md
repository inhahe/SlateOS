## 1338. Left Ctrl+Alt is a command, not AltGr: a Ctrl+Alt chord that selected no character carries no text

**Date:** 2026-10-03
**Lane:** F
**Decided by:** Claude (autonomous), answering part 2 of
`requests/e-cf-a-toolkit-field-types-the-letter-of-a-shortcut-it-does-not-know.md`,
whose author (lane E) offered both options and had no preference.

**In short:** on Windows, holding left Ctrl and left Alt together works like
the AltGr key, so Ctrl+Alt+E types `€` on a German keyboard. SlateOS does not
copy that. AltGr is the right-hand Alt key only, as it already was. A Ctrl+Alt
combination that does not produce a character through AltGr is now treated as
a keyboard shortcut and types nothing. Before this, it sent the plain letter
(`e`), which a text box could end up typing. Someone used to Windows who types
`€` with Ctrl+Alt+E must use AltGr+E instead, as on Linux and macOS.

**What was decided.** In `Compositor::dispatch_key`, a press whose modifiers
-- after the AltGr fold and the sticky-keys merge -- include both Ctrl and Alt,
on a key the layout did not resolve through AltGr, is sent with empty text.
It does not pass through the dead-key machine, so an accent waiting for its
vowel is still waiting afterwards, as after any command chord. Untouched:

- **A key that did resolve through AltGr types its character**, even with left
  Ctrl and Alt held beside it.
- **Ctrl alone and Alt alone still carry their letter.** A terminal
  builds control and escape sequences from them, and part 1 of the request
  (lane C's toolkit) decides whether a text field types them.
- **A source's own character is still typed** -- the Windows host window,
  which has resolved Windows' AltGr itself and reports it as Ctrl+Alt.

**Why this option, not Windows' rule.**

| | For | Against |
|---|---|---|
| Ctrl+Alt is a command (chosen) | one meaning per chord: Ctrl+Alt+T is a shortcut on every layout, so a shortcut can never be shadowed by a third-level character and never type one; matches the existing rule that AltGr is the right-hand key (`dispatch_key`'s sticky-keys note), Linux and macOS | a Windows user's Ctrl+Alt+E habit types nothing; AltGr+E does it |
| Ctrl+Alt resolves through AltGr, as on Windows | Windows habits carry over | Windows' long-standing collision: on a layout with a third level, Ctrl+Alt+Z is `ż` in Polish and no longer the shortcut, so an application's Ctrl+Alt bindings work on some layouts and not others; and the chords with nothing on the level still need the no-text rule anyway |

**If it ever needs to change:** a keyboard setting "Ctrl+Alt acts as AltGr"
would be one condition in `ModifierState::level`. The rule here stays as it
is for chords that select nothing.
