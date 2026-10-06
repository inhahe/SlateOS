## 1481. One character picker: every emoji once, in the tone chosen, found by name or keyword, and typed into the field it was opened over

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** there is now one "Emoji & Symbols" dialog: every emoji, plus
the symbols, maths, arrows, currency signs and Latin, Greek and Cyrillic
letters a keyboard lacks, browsed by category or found by typing a name, a
keyword ("money" finds the dollar sign) or a code point (`U+00E9`). The
shell's own text fields offer it on their right-click menu and on Ctrl+.,
and what is picked is typed into the field. Other programs' fields, and the
tray's emoji entry, need the window system to carry typed text to them,
which has been asked of lane F.

**Where:** `gui/charnames` (the data and the search), `gui/charpicker` (the
dialog), `gui/desktop/src/char_picker.rs` (the shell's fields).
`roadmap-detailed.md`: "the Unicode selection dialog ... a reusable OS
component (the same one surfaced by the tray emoji-input entry and available
to apps), so users learn one dialog"; `design.txt` line 711's emoji input.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **The names, keywords and order are generated from Unicode's own files** -- emoji-test.txt, UnicodeData.txt and CLDR's English annotations (`gui/charnames/gen.py`) -- into a crate of their own | a table written by hand, as lane E's emoji picker has (a few hundred emoji) | Every emoji of the current version (18.0), in the order every keyboard's palette uses, each with the keywords people search by; a new version is one run of the generator. The data is a crate only programs offering a picker link. | Some hundreds of kilobytes in each program that links it. |
| **The dialog is a crate of its own, not part of the toolkit** | `guitk::charpicker`, which every text field could open itself | The toolkit is linked by every program; the tables would be in all of them. A program -- the shell today -- that offers the picker depends on it. | A text field in an ordinary program cannot open it by itself; that is the window system's part (below). |
| **Each emoji is listed once and drawn in the skin tone chosen**; the tone applies to every emoji that has tones, the recent ones included, which are remembered untoned | listing every toned variant (a hand six times), or a tone per emoji | The grid stays one-emoji-one-cell, as every system picker does; one choice serves all. | A pair of people in two different tones is not offered: one tone cannot name it. |
| **A search ranks a name that is the query first, then names that match, then what matched only by a keyword**, emoji before characters within each | CLDR order alone | "cat" puts the cat first, not the grinning cat; "money" puts the money bag before the dollar sign. Words match the starts of a name's words in any order and case, so "arrow right" finds RIGHTWARDS ARROW. | -- |
| **A code point names a character whether this knows its name or not** | only characters with names here | `U+4E00` is asked for by someone who knows what they want; the picker offers it, unnamed. Control characters are refused. | -- |
| **A pick reaches a field as a key that types it** | an "insert" call per field | Each field already does what follows typing -- the Run box suggests, the start menu searches, a note is saved, a name is kept -- and the window system's eventual delivery to other programs will be the same typed key. | A field that treated a key with no keyboard key differently would need to learn it; none here does. |
| **In the shell, the picker closes when something is picked** | staying open for more | It was asked for over a field the user was typing in, as a menu row is chosen. The dialog itself stays open on a pick; a host decides. | Several characters take several openings. |
| **Ctrl+. opens it over a field** | Super+. (Windows) | It is the toolkits' chord (GTK's emoji chooser), types nothing in any field here, and leaves Super+. for the system-wide picker, which needs the window system. | Windows users reach for Super+. first. |

**Not done here, and why:**

- *Other programs' fields and the tray's emoji entry.* A pick has no way into
  another program until the compositor can deliver typed text to the focused
  window on the shell's behalf --
  `requests/c-f-let-the-shell-type-into-the-focused-window.md`. That is also
  what the roadmap's "a hotkey types a chosen character" needs.
- *Characters no installed face draws are offered all the same*, and draw as
  boxes, until the font stack can say whether it can draw one --
  `requests/c-f-ask-whether-a-character-can-be-drawn.md`.
- *The recent picks and the tone last only while the shell runs* --
  `known-issues/TD-C-THE-SHELLS-CHARACTER-PICKER-FORGETS-ITS-RECENT-PICKS-AT-LOGOUT.md`.
