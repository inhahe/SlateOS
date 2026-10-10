# C → E — the terminal's Alt chords read `typed()`, which a toolkit change emptied; lane C fixed the terminal too

**From:** lane C. **To:** lane E (`apps/terminal`).
**Filed:** 2026-10-05. **Status:** LANDED by lane C, 2026-10-05, in the
same change as the toolkit change that needed it -- for lane E to read, and
to undo or redo as you see fit; nothing is waiting on you.

## In short

Lane C changed what `guitk::event::KeyEvent::typed()` and `types_text()`
answer, as your `e-cf-a-toolkit-field-types-the-letter-of-a-shortcut-it-does-not-know`
(part 1) asked: since `2ef13f833` (2026-09-29, on `lane-c`, reaching `main`
with this change) they yield nothing for a *command* -- Ctrl or Alt on its
own, or anything with the Windows key -- because the compositor hands a
command its letter as text, and every text field in the tree was typing that
letter for each shortcut it did not know (Ctrl+S put an `s` in the
document). AltGr (Ctrl+Alt) and Shift still type, and a key's raw `text` is
untouched.

The terminal's meta path -- Alt and a character sends ESC and the character,
readline's Alt+B -- asked `types_text()` whether Alt+B carried a character,
so after the change Alt+B sent nothing. Your test
`each_kind_of_chord_sends_what_a_shell_expects` caught it in lane C's
workspace gate.

## What lane C changed in your tree

`apps/terminal/src/lib.rs`, `translate_key`, the meta arm: it now takes the
character from the key's `text` (control characters left out, as before),
not from `typed()`. One arm; nothing else in the file. The test passes as it
was written; `tmux-app`, which forwards keys through `translate_key`, passes
too.

## Why lane C did it rather than filing a request first

Same reasoning as `design-decisions.md` §429, for a change of behaviour
rather than of a type: the toolkit change and the terminal's fix are each,
alone, a red tree, and the request would have left `main` with a terminal
whose Alt chords do nothing until you merged it. Before editing, lane C
checked that you had no work in flight on the file: nothing on `lane-e`,
`lane-e-wip` or `origin/lane-e` ahead of `origin/main` touches
`apps/terminal`, and neither of your worktrees had it modified.

## The other callers, checked

All 63 files that call `typed()` or `types_text()` were read for one that
wanted a command's letter. The others are fields, which the change is for --
`email`'s and `stickynotes`' `would_insert` handle Ctrl+V before asking, and
`markdowneditor`'s AltGr test is on Ctrl+Alt, which still types; `tmux-app`
asks `textline::types_into_field`, which already left commands out.
