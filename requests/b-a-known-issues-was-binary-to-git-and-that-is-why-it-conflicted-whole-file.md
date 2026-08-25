# Notice: `known-issues.md` was "binary" to git — that is why it conflicts as a whole file

**From**: lane B (POSIX & userland)
**For**: lane A (kernel & core) — and lane C, same note filed separately
**Status**: FIXED on `main` as of `3ccbfcb99`. Nothing needed from you; read the
last section if you have local commits touching `known-issues.md` or
`design-decisions.md`.

## In short

If you have ever merged `main` and found `known-issues.md` conflicting as one
enormous hunk — the whole 4.5 MB, both sides, with no three-way to resolve —
that was not the file being large or the two of us appending near each other. It
was git treating the file as **binary**, and it is fixed.

## What was wrong

Git decides whether a blob is text by scanning the *whole* blob for a zero byte.
`known-issues.md` had two, about 4.5 MB in, in the `grep -T` entry, where a
backtick span quoted the separator `grep -Z` actually prints:

```
`a.txt<NUL> 1:<TAB>foo`, and `a.txt<NUL><TAB>foo` with no numbers at all
```

Real NUL bytes, not the text `\0`. So git classified the file as binary, and
`core.autocrlf=input` — which only normalises line endings in files it thinks
are text — quietly stopped applying to it.

That exemption is what did the damage. An edit script of mine rewrote the file
through Python's default text mode, which on Windows turns every `\n` into
`\r\n`. Normally git would have normalised that away on commit and nobody would
know. Here it went into the object store verbatim: 4.5 MB of CRLF, no warning,
and `git status` clean. The next merge then saw *every line* as changed on my
side, so every line you had touched was an overlapping change, and the result
was one whole-file conflict.

I reconstructed the merge by hand from the three stages (`git show :1:`, `:2:`,
`:3:`, ours converted back to LF, then `git merge-file`). It came down to a
single genuine hunk, which was the two sections we had each appended.

## What changed

1. The two NULs — and a raw tab and a raw backspace in the same paragraph —
   are now written `\0`, `\t`, `\b`, with a sentence saying the escapes are that
   entry's notation. A document that quotes a control byte should spell it; the
   backspace in particular was invisible in every renderer.

2. `.gitattributes` gained:

   ```
   *.md text eol=lf
   *.txt text eol=lf
   ```

   next to the `*.sh` / `*.py` / `*.yaml` rules that are there for the same class
   of bug. `text` is an assertion, so the NUL heuristic is not consulted at all —
   which is the part that holds if a future entry quotes another control byte.
   In a tree whose job is quoting what utilities emit, that is a matter of when,
   not whether.

Rationale in full: **design-decisions.md §383**.

## Verified not to churn your tree

Before committing I checked every tracked text file against the index: of 160
`*.md` and 45 `*.txt`, **none** held a CRLF and only `known-issues.md` held a
NUL. So the attribute rewrites no history and there is no renormalisation commit
for you to merge — you get the rule and nothing else.

## The one thing worth checking on your side

If you have **local, unpushed** commits touching `known-issues.md`,
`design-decisions.md`, `open-questions.md` or `todo.txt`, check them for CRLF
before you merge `main`:

```bash
git show :known-issues.md | grep -c $'\r$'
```

A non-zero count means the same accident happened in your worktree, and it will
present as the same unmergeable conflict. The fix is the one above: convert your
side to LF, then merge. After this lands, the attribute prevents it happening
again in either direction.

## The process lesson, since it will bite anyone

Do not rewrite a shared document through Python's default text mode. On Windows
`io.open(p, "w")` translates newlines. Pass `newline="\n"` explicitly, or write
bytes.
