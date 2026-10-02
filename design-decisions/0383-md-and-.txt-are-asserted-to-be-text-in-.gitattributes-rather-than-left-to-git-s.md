## §383 — `*.md` and `*.txt` are asserted to be text in `.gitattributes`, rather than left to git's NUL heuristic

**Decided by:** Claude (autonomous)

**Date:** 2026-08-25

**In short:** `known-issues.md` contained two zero bytes, buried 4.5 MB into it,
because a note about `grep -Z` quoted the byte that `grep -Z` prints instead of
writing `\0`. Git decides whether a file is text by looking for a zero byte
anywhere in it, and it does not fix up line endings in a file it thinks is not
text — so the file quietly stopped being protected by the setting that keeps
every other file's line endings consistent. An edit script then rewrote the whole
document with Windows line endings, that went into the repository unnoticed, and
the next merge could not merge it at all: every line looked changed, so the
whole 4.5 MB came back as one conflict. The fix is two-part — spell the zero
bytes, and tell git in `.gitattributes` that these files *are* text so it stops
guessing.

### What was decided

`.gitattributes` gains `*.md text eol=lf` and `*.txt text eol=lf`, alongside the
`*.sh` / `*.py` / `*.yaml` rules already there for the same class of bug. The two
raw NULs (and a raw tab and backspace in the same paragraph) are replaced by
`\0`, `\t` and `\b`, with a one-sentence note saying the escapes are the
entry's notation.

Verified before committing: of 160 tracked `*.md` and 45 tracked `*.txt`, none
held a CRLF in the index and only `known-issues.md` held a NUL. So the attribute
changes no byte of history — it is prevention only, with no renormalisation
churn for the other two lanes to merge.

### Why the attribute and not just the escape

The escape fixes today's file; the attribute fixes the *class*. `core.autocrlf`
consults a heuristic, and a heuristic that reads content will be wrong again the
next time someone documents a byte — which in a tree whose whole job is quoting
what utilities emit is a matter of when, not whether. `text` is an assertion, so
the heuristic is not consulted at all.

The cost is the mirror image: a `.md` or `.txt` that genuinely *is* binary would
be corrupted by normalisation. That is not a file this repository can plausibly
want; a binary artifact named `.txt` would be wrong for a dozen other reasons
first.

### Why it matters more here than in a single-agent tree

These are the documents three lanes append to concurrently — `known-issues.md`,
`design-decisions.md`, `open-questions.md`, `todo.txt`, `roadmap.md`. The
per-lane conventions in `roadmap.md` rule 3 exist so those merges stay clean, and
they do: `design-decisions.md`'s numbering split has auto-merged across a
72-commit divergence. All of that is defeated by one file git has decided not to
normalise, because the conflict it produces is not a hunk anyone can resolve —
it is the entire document, twice.

### Alternatives rejected

- **Escape the NULs and stop there.** Rejected: it leaves the next quoted control
  byte to re-open the hole, and the failure is silent both times.
- **`*.md -text`** (assert *not* text, disabling normalisation everywhere).
  Rejected: it makes the CRLF permanent instead of impossible — whichever
  platform first commits a file decides its line endings forever.
- **A pre-commit hook that rejects CRLF.** Rejected as the primary mechanism: it
  is a check that has to be installed per-worktree to work, where an attribute
  travels with the repository. It would be a reasonable *addition*, not a
  replacement.

### Postscript, same day: it recurred, and the attribute could not have helped

Writing §384 below put **two literal backspace bytes** into this file. The
intent was the two characters `\b` inside a table cell naming grep's `\b`
assertion; what arrived was byte 0x08. The cause is not Python's text mode this
time — it is that a backslash escape written into a shell heredoc or a `python
-c` argument passes through a layer that may consume it, so the Python source
that actually ran said `\b`, which Python read as the backspace escape it is.
Caught by a byte scan run for exactly this reason; the file now has none.

Two things worth carrying forward:

- **The `.gitattributes` rule would not have caught this one.** `text eol=lf`
  governs line endings; a backspace is neither a NUL nor a CR, so nothing about
  it is abnormal to git. What actually protects a document that quotes control
  bytes is the *convention* — write the escape, never the byte — plus a scan
  that checks the convention held. The attribute is the floor, not the ceiling.
- **A byte-level edit belongs in a script file, not in a heredoc or `-c`
  string,** and should spell a control byte as `bytes([8])` rather than as an
  escape sequence. This is the second time an escape has been silently eaten on
  the way to Python in this tree; both times the damage was invisible in every
  renderer and in `git diff`.

The check that found it, worth running after any programmatic edit to a shared
document:

```python
b = open(path, "rb").read()
[(i, b[i]) for i in range(len(b))
 if b[i] < 9 or b[i] in (11, 12) or 13 <= b[i] <= 31]
```

It came back empty for all five shared documents afterwards, this file included.
