## One stray NUL byte made `known-issues.md` unmergeable for every lane (FIXED)

**Status: FIXED 2026-08-15** (lane C, during the routine `lane-c` → `main`
merge). Not an app bug — a bug in the shared documents themselves, which is
why it had gone unnoticed while costing every lane a manual conflict
resolution.

Merging `origin/lane-c` into `main` produced a whole-file conflict on
`known-issues.md`: one hunk, `1,65791c1,65863`, as if not a single line
matched. But line 100 of the two sides was byte-identical. The reason is that
git had classified the 3.8 MB document as **binary**, and a binary file has no
lines to merge — it can only be taken whole from one side or the other.

The cause was a single byte at offset 2 870 859, inside a quoted bash C
snippet in one of the oils entries:

```c
if (newname == 0 || *newname == '<NUL>')
```

The author meant C's two-character escape `'\0'`. Whatever produced the
paste turned it into an actual `U+0000`, and `git diff`'s binary heuristic is
simply "does the first 8 000 bytes contain a NUL" — no, but git also scans
further for blob attributes, and a NUL anywhere in the content is enough for
the merge driver to refuse a textual merge.

**The same byte silently caused a second, unrelated-looking symptom.** This
repo sets `core.autocrlf = input`, which normalises CRLF to LF on commit —
but only for files git considers *text*. Because the NUL made this file
binary, that normalisation was skipped, so when one lane's editor rewrote the
file with CRLF endings it was committed verbatim. `main`'s copy was entirely
CRLF while `base` and `lane-c` were entirely LF, which is a second reason
every line differed. Two symptoms, one byte.

Fixed by writing the escape the author meant (`'\0'` as two characters) and
normalising the file back to LF. With the NUL gone the three-way merge of the
same three versions succeeded with **zero conflicts** — which is what the
append-only convention in `roadmap.md` rule 3 is designed to produce, and had
been quietly failing to deliver.

Worth generalising, because this is the audit's own lesson turned back on us:
a control character that a format cannot represent does not announce itself.
It changes how *tooling* reads the document — here, from a mergeable text file
into an opaque blob — and the damage shows up somewhere far from the paste, as
a merge conflict nobody could explain. When pasting source into a shared
markdown document, paste the escape, never the character.
