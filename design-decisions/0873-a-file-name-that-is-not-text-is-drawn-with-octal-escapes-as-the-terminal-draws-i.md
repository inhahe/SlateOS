## 873. A file name that is not text is drawn with octal escapes, as the terminal draws it, not with U+FFFD

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C &middot; proposed by lane E

**In short:** A file on SlateOS may have a name that is not text -- any byte
but `/` and NUL is legal. Wherever the desktop draws such a name (the file
dialog, the path bar, the folder tree, the Run box), each byte that is not
text used to show as `�`, so `caf\351.txt` and `caf\350.txt` looked identical
and neither looked like what the file is called. It now shows each such byte
as a three-digit octal escape, `caf\351.txt` -- exactly as the command-line
tools already print it (§369). Nothing is ever *derived* from the drawing:
opening, navigating and renaming still use the exact bytes kept beside it.

### The choice

| | `caf\351.txt` beside `caf\350.txt` | Cost |
|---|---|---|
| **U+FFFD** (what it was) | `caf�.txt` and `caf�.txt` -- identical | a name the user cannot tell from another, and that is not its name |
| **Octal escape** -- chosen | `caf\351.txt` and `caf\350.txt` | four characters where there was one, so a long name elides sooner |

The length cost falls where it is worth paying: names that are not text are
rare on a SlateOS disk and common only on media from other systems, which is
exactly where telling two files apart matters.

**Why the terminal's rule and not a new one.** A name drawn one way in the file
manager and another in `ls` would be two answers to "what is this file
called". `pathcodec::display_os` is `quoting::escape_unprintable` over the
name's bytes, and a test holds the two to the same spelling.

**The ambiguity accepted.** A name that really contains a backslash and three
digits reads the same as one with that byte, as §369 accepted for the
terminal. The rendering is for a person to read, not for a program to parse
back -- which is why nothing parses it back.

### Smaller calls made with it

- **Editable fields show the escape too** (the path bar in edit mode, the file
  dialog's name field, the Run box). Confirming an unedited field still uses the
  exact bytes, because "did the user edit it?" compares the field with the
  rendering of the kept bytes, and any fixed rendering answers that the same
  way. A field the user *does* edit means what was typed -- a backslash
  followed by digits is four characters, not a byte; typed escapes are not
  interpreted, the same rule as the terminal's.
- **Text nobody can see is escaped as well** -- a newline, a line separator --
  since `escape_unprintable` does, and a one-line label with a line break in
  it is not one line.

### How to reverse

`display_os` is the one place: make it `to_string_lossy` and every label goes
back to U+FFFD.
