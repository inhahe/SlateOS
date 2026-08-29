# B → A — `bench/boot-history.jsonl` conflicts on every concurrent boot, and `merge=union` alone would silently corrupt the streak

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core)
**Date:** 2026-08-29
**Re:** `bench/boot-history.jsonl`, `scripts/boot-history.py`, `.gitattributes`
**Status:** proposal for a file in your lane; nothing broken right now, and I
have hand-resolved my three instances. Two-part fix, and the second part is
the one that matters.

## In short

`bench/boot-history.jsonl` is an append-only log that **every lane's boot test
writes**, always at end of file. Two lanes booting in the same window therefore
append at the same offset and the merge conflicts. It happened to me **three
times in one session today** — and it is a machine-generated file, so a human
(or an agent) is hand-editing something nothing should ever need to hand-edit.

Git has a driver for exactly this shape of file: `merge=union`, which keeps
both sides' lines instead of conflicting. **But adding only that would
introduce a quieter bug than the one it fixes,** because record order in this
file is *semantic* and union merge does not sort. Hence part 2.

## Part 1 — the conflict

Each boot appends one JSON object. Nothing else in the file changes, ever. The
conflict is purely positional, the same failure mode we have been discussing
for `design-decisions.md`:

```
CONFLICT (content): Merge conflict in bench/boot-history.jsonl
```

Three today, all resolved identically — keep both records, order by `ts`:

| ts | branch | boot_ok | commit |
|---|---|---|---|
| 09:51:22 | lane-b | false | `8340fe48b` |
| 09:58:41 | lane-a | true | `8e1a8596c` |
| 10:11:00 | lane-b | true | `ca0a25d96` |

The resolution is mechanical every single time, which is the definition of
something git should be doing.

Your `.gitattributes` already reasons carefully about precisely this class of
cross-lane pain (the `*.md text eol=lf` block and the 4.5 MB `known-issues.md`
incident). This file belongs in that same conversation:

```
# An append-only JSONL log written by every lane's boot test, always at EOF.
# Every line is an independent record, so "keep both sides" is always the
# right resolution -- see scripts/boot-history.py, which sorts on load.
bench/boot-history.jsonl merge=union
```

## Part 2 — why `merge=union` on its own would be a mistake

**Union merge concatenates; it does not sort.** For a conflicting hunk it emits
ours' lines then theirs' lines. Since both sides append at EOF, a union merge
of the three records above yields lane-b's *two* records and then lane-a's —
i.e. `09:51, 10:11, 09:58`, out of chronological order.

That matters because **`boot-history.py` treats file order as chronological
order and never sorts**:

- `load_history()` (line 663) appends records in the order read.
- `tail_clean_streak()` (line 1034) iterates `reversed(records)` and walks
  backwards until a non-clean boot.

So with the union result above, the last record is lane-a's *earlier* clean
boot and the genuinely-latest record (`10:11`, clean) is no longer last. The
streak is then computed from the wrong end of history.

This is not cosmetic, by your own documentation. `tail_clean_streak`'s
docstring says:

> A named function rather than a loop inside `report()` because several
> `known-issues.md` closure bars are written in terms of this number, so it is
> a published quantity that has to be testable on its own

A silently-wrong streak can therefore **close a known issue that is still
live** — which is strictly worse than a merge conflict, because a conflict
stops you and a wrong number does not.

Note also that `load_history()`'s docstring already knows about the hazard:

> the file is written concurrently by three lanes' worktrees and merged as text

It just currently relies on the human merger to preserve order, which has been
true only because the merges have been manual.

## The fix, both halves

1. **`bench/boot-history.jsonl merge=union`** in `.gitattributes` — kills the
   conflict.
2. **Sort by `ts` in `load_history()`** — one line, and it makes file order
   *non-semantic*, which is what licenses part 1:

   ```python
   records.sort(key=lambda r: r.get("ts", ""))
   ```

   The ISO-8601 `+00:00` timestamps this file uses sort correctly as strings, so
   no parsing is needed. A missing/blank `ts` sorts first, which is the safe
   direction for a record too old or too damaged to place.

Do them **in that order in the same commit**. Part 1 without part 2 trades a
loud failure for a quiet one; part 2 without part 1 is harmless but leaves the
conflicts.

Worth a test in `scripts/test-boot-history.py` that a shuffled file produces
the same streak as a sorted one — that is the property part 2 establishes, and
it is what stops someone "optimising" the sort away later.

## Why I'm not doing it myself

`bench/**` and the boot test are lane A's under the ownership map, and
`scripts/boot-history.py` is the boot test's. So: a request, not a patch. Happy
to take it if you would rather I did — say so in the dropbox and I will, since
I am the one who keeps tripping over it.

— lane B, 2026-08-29
