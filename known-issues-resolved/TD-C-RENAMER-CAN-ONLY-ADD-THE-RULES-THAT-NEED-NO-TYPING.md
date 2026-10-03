### TD-C-RENAMER-CAN-ONLY-ADD-THE-RULES-THAT-NEED-NO-TYPING — 2026-09-04 — FIXED 2026-09-25

**FIXED 2026-09-25 (lane E).** The renamer has the control this entry asked
for: an Add Rule menu (the toolbar's button, the operations panel's, or `R`)
that adds any of the ten kinds, and a rule editor under the pipeline with a
text box for every string or number a rule takes, a chooser for every choice
(all eight case modes, all five date formats, the three trim modes, the five
extension rules, start / end / at-a-position) and a switch for each flag.
Typing takes effect as it is typed; a counting box refuses what is not a number
and says so with a red rule under the box. `FileEntry::modified_ms` is read
from the folder listing and a date stamp stamps it; `RenameRecord::timestamp_ms`
is the clock's and the History panel shows it. Every rule can be selected
(a press, or Ctrl+Up / Ctrl+Down), edited (F2), moved and removed -- the
selection used to land only on the newest rule, so the others could be neither.

Making the rules reachable showed what three of them would have done, fixed in
the same change: the Regex rule was a literal replace ("only support literal
patterns for now") and is a POSIX extended regular expression through `ere`,
the engine the shell, `grep` and `sed` share, with `&` and `\1`...`\9` in the
replacement; the date stamp was the constant 2026-05-18 and is the file's own
modification date (UTC, `Tz::utc()`); and an empty search put the replacement
between every character. Also: numbering counted every file in the list rather
than the files being renamed, ticking a file did not re-run the preview or the
conflicts, and opening a folder threw the rules away.

The rest of this entry is the history.


**Corrected 2026-09-15.** The paragraph below said the renamer "can actually
rename". It could not: `apply_plan` edited a `Vec<FileEntry>` and the crate
contained no `std::fs` at all, while the status line said "Renamed {count}
files". The sentence was written to mean *the rename action is reachable from
a key*, and what it says is that the program renames files. It is left visible
rather than quietly edited, because the entry crediting a program with an act
it cannot perform is the finding, and `scripts/find-overstated-records.py`
exists now to catch the next one. The renamer does rename real files as of
`TD-C-RENAMER-SAID-RENAMED-N-FILES-AND-RENAMED-NOTHING` below.

~~**In short.** The bulk renamer can now be given files and rules, and can
actually rename.~~ What it still cannot do is add any rule that needs a *string*
typed in — find/replace, insert, remove-at, regex, replace-extension — because
the app draws no text field anywhere.

**Where it was.** Before 2026-09-04 the window would have opened completely
empty and stayed that way: `add_file` and `add_operation` had no caller outside
the tests, so there was no file to rename and no rule to rename it by. Every
preview showed the name unchanged and the program's whole purpose did nothing.
A `#![allow(dead_code)]` at the top of the file is why nobody noticed.

**What is reachable now.** A sample file list at startup, and the rules that
need nothing typed, each on a key: five of the eight case modes (lower, upper,
title, snake, kebab), trim, sequential numbering, lowercase-extension,
remove-extension, plus clearing the rules or the list, undo, redo, and the
rename itself.

**What is not, and why.**

| still unreachable | needs |
|---|---|
| find/replace, insert, remove-at, regex, replace-extension, add-extension | a text field to type the string into |
| the other three case modes, the date-stamp formats, the other trim modes, insert-at-position | a menu — the keyboard is out of letters that read naturally |
| `FileEntry::modified_ms` | a file chooser to read a real modification time from |
| `RenameRecord::timestamp_ms` | a history panel that shows *when*, not just what |

**The fix is one control, not many.** Every row above is waiting on the same
missing thing: somewhere to type, and somewhere to choose. `INPUT_HEIGHT` is
already defined and unused, which is the shape of a text field that was planned
and never drawn. Once it exists, each remaining rule is a match arm.

**No data is at risk.** The rename operates on an in-memory list seeded at
startup; there is no filesystem access at all yet, which is its own gap and the
reason `add_file` takes a path as a string.
