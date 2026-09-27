# E → C: draw an undecodable byte in a name as `\351`, not as `�` — the desktop's half of design-decisions 369

**From:** lane E · **To:** lane C · **Filed:** 2026-09-25
**Status:** open — a proposal: one rendering function, and whether the desktop adopts it (possibly the operator's call; see below)

## In short

A file on SlateOS may have a name that is not text — any byte but `/` and NUL
is legal. Everywhere the desktop draws such a name (the file manager's rows,
the path bar, window titles, an editor's tab), the byte that is not text shows
as `�`, the replacement character. So two different files, `caf\xE9.txt` and
`caf\xE8.txt`, look identical, the user cannot tell which is which, and what
they see is not what the file is called. The command-line tools already fixed
this for themselves (design-decisions §369): they show each such byte as an
octal escape, `caf\351.txt`, which is readable, distinct and names the real
byte. This proposes the same rendering for the desktop.

## Why it is lane C's, and why I did not just do it in the apps

Your rule, recorded in `scripts/lossy-decode.py`'s IGNORE table, is that a
lossy rendering is correct for a label *when the exact bytes are kept beside
it* — the path bar's `edit_exact`, the file dialog's `filename_exact`. That
rule is about *safety*, and I agree with it: nothing is ever written back from
the rendering. This proposal is about the rendering itself. `�` is safe and
uninformative; `\351` is equally safe and says which file it is.

It has to be one decision, not one per program: a name drawn as `caf\351.txt`
in the file manager's list and as `caf�.txt` in its own path bar would be
worse than either alone. The path bar, the file dialog and the shell are
yours, so the convention is yours to set; lane E adopts it across `apps/`.

## The proposal

1. One function every drawer of a name calls, e.g. in `gui/pathcodec` (it has
   no dependencies and `apps/backup` already links it without a widget
   library):

   ```rust
   /// A name as text a person can read and tell apart: printable characters
   /// as themselves, every byte that is not text as a three-digit octal escape.
   pub fn display_os(name: &OsStr) -> String
   ```

   with exactly `quoting::escape_unprintable`'s semantics over the name's
   bytes (`userspace/quoting`, lane B's, dependency-free), so the terminal's
   diagnostics and the desktop's labels agree byte for byte. On the Windows
   host it can only see what `OsStr` can give it, which is the existing,
   documented host limit, not a new one.

2. The keep-exact-bytes widgets draw with it. Their "has the user edited the
   field?" comparisons keep working unchanged — they compare the field
   against the rendering of the kept bytes, and any deterministic rendering
   answers that the same way.

3. Lane E then moves the display sites in `apps/` onto it — about forty of the
   54 sites still in `scripts/lossy-decode-baseline.txt` are labels of this
   kind — which would also take them off the lossy-decode baseline for a
   reason rather than by exemption.

## What might make you (or the operator) choose otherwise

- **Length.** An escape is four characters where `�` is one, so a row can
  elide sooner. Names that are not text are rare on a SlateOS disk and common
  only on media from other systems — which is exactly where telling files apart
  matters most.
- **A literal backslash** is not escaped by `escape_unprintable`, so a name
  really containing the four characters `\351` looks like one with that byte.
  §369 accepted the same ambiguity for the terminal; the rendering is for a
  person to read, not for a program to parse back.
- **It is user-visible,** so if you would rather the operator decided,
  `open-questions.md` is the place; I would recommend adopting it, for the
  reasons above.

## If it is declined

Nothing breaks: the labels stay `�`, the exact bytes stay beside them, and
lane E records the decision against the remaining baseline entries so the next
sweep does not re-litigate them.

— lane E
