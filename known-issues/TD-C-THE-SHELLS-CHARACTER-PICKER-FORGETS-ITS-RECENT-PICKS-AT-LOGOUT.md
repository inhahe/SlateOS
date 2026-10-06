### [C] TD-C-THE-SHELLS-CHARACTER-PICKER-FORGETS-ITS-RECENT-PICKS-AT-LOGOUT -- 2026-10-06

**Status:** OPEN

**In short:** the character picker over the shell's text fields ("Emoji &
Symbols", Ctrl+.) opens on what was picked lately and in the skin tone chosen
last -- but only while the shell runs. At the next login both are gone: the
recent list is empty and the tone is back to none.

**Where:** `gui/desktop/src/char_picker.rs` -- `DesktopShell::char_recent`
and `char_tone` are kept in memory and handed to
`charpicker::CharPicker::with_recent`/`with_tone` on each opening. Nothing
writes them anywhere.

**Why not done with the picker:** the shell reads and writes no files itself
(the session does its I/O), and which settings file a picker's history
belongs in is worth deciding once, for every program that will host the
picker, rather than once per host.

**The proper fix:** a small settings group -- `charpicker` in the user's
settings directory, through `settingsfile`, the way `inputsettings` keeps
the keyboard's -- holding the recent picks (as text, most recent first, at
most `charpicker::MAX_RECENT`) and the tone (`light` ... `dark`, or none).
The session loads it into the shell at start, as it loads the input
settings, and writes it when the picker closes having changed either. Every
host of the picker reads the same file, so a character picked in one program
is recent in the next.
