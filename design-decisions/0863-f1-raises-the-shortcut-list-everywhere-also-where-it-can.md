## 863. `F1` raises the shortcut list everywhere; `?` also, where it can

**Date:** 2026-09-18
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Seven apps now show a list of their keys when asked. The question
was which key asks. `?` reads well and is what a terminal user reaches for, but
`apps/spreadsheet` has to be able to put a `?` in a cell, so `?` cannot mean
"help" there -- and a help key that works in six apps out of seven is a key
nobody can rely on. `F1` works in all seven; `?` works as well in the six that
are not obliged to type one.

**The options.**

| | *What changes* | Against |
|---|---|---|
| `?` only | one key, the one a terminal user tries | in `apps/spreadsheet`, `?` puts a `?` in the cell and no help appears -- a dead end reached by pressing the very key you learned |
| `F1` only | one key, the desktop convention, works everywhere | `?` does nothing in six apps where it easily could, and `?` is what somebody coming from a terminal or a web page tries first |
| **`F1` always, `?` as well where free** (chosen) | pressing `F1` shows the keys in every app; pressing `?` does too, except in the spreadsheet, where it types a `?` | two keys to document per app, and one app where they differ |

**Why the third.** The failure worth avoiding is *learning a key that then does
not work*, which is worse than not knowing a key at all -- it teaches the user
that the feature is absent. `F1` is the key that never fails, so the sentence
"press F1 to see the keys" is true of the whole suite. `?` costs one extra
match arm where it is free and saves a user who tries it first.

**Why `?` is not simply dropped from the six.** It is what a reader of a
terminal program, a web page or a text editor tries, and in an app with no text
entry there is nothing for it to collide with. Removing it would make the suite
uniform by taking away something that works.

**What this does not settle.** `apps/minesweeper`, `apps/mixer` and
`apps/wordsearch` print their keys in a permanent footer rather than behind a
key, and neither `F1` nor `?` does anything in them. That is not an
inconsistency to fix: a list already on screen needs no key to raise it, and
adding one would mean drawing the same list twice.

**How it is held.** Each app's `SHORTCUTS` row reads `"F1 / ?"`, and
`every_advertised_key_does_something` presses *both* keys named in a row -- so
the binding cannot be dropped from one app without that app's own test saying
so. See `known-issues.md` -> `TD-C-A-PRINTED-KEY-LIST-IS-A-SECOND-COPY`.
