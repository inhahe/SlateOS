### TD-OILS-CORPUS-FILTER-MATCHED-THE-LINE-NOT-THE-NAME. A corpus case filtered whole-environment listings with `grep -i zz`, so a random scratch-directory name containing `zzz` dragged `PWD` and `DIRSTACK` into the comparison — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:**
`userspace/oils/tests/corpus/posix-mode-makes-export-and-readonly-list-in-their-own-spelling.sh`.

**What:** the case exercises `export -p` / `readonly -p`, which enumerate the
*whole* environment — most of which differs between the two shells — so it
filtered down to the names it makes, all containing `zz`. The harness runs each
case in a randomly named scratch directory; on one sweep in a few that name is
something like `oshdiff-l2655zzz`, and then `declare -x PWD="…zzz"` and
`declare -a DIRSTACK=(…zzz)` match the filter and appear on one side only.

Symptom: a one-case failure in a full sweep that does not reproduce on a rerun,
easily mistaken for a regression from whatever landed just before it.

**Fixed** by anchoring the filter to the *name position* of a listing line
rather than to the line anywhere:

```sh
mine() { grep -E '^[a-z]+( -[A-Za-z]+)* ZZ[A-Z]*(=|$)'; :; }
```

**Standing lesson:** a corpus filter is part of the comparison, not scaffolding
around it. If it matches anywhere on the line it will eventually match
something the environment chose rather than something the case made — so match
the *position* the construct puts the name in. Anything else is a flake waiting
for the right scratch directory.
