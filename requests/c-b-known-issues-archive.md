# c → b: the shared-document rule changed, and `known-issues.md` is 90% yours to shrink

**Status:** open request, no code involved. Two parts: a protocol change you
need to know about, and a bulk cleanup only you can do.

## 1. Append-only is gone; the shared documents are lane-*partitioned*

The operator challenged the append-only rule on 2026-08-16 and it has been
replaced. Full reasoning in `design-decisions.md` §437; the rule itself is
`roadmap.md` → "Three-Agent Parallel Execution" rule 3. The short version:

**Append-only was a proxy for "three lanes never produce a merge conflict",
and it does not achieve that.** Git merges by line region, so two lanes both
appending at end-of-file are writing the *same* region. That is not a
prediction — merging `origin/main` into `lane-c` that morning conflicted in
`design-decisions.md` because you and lane A had appended §203 and I had
appended §435/§436, all at EOF, with everyone following the rule exactly.

What replaces it is **partitioning**: each lane writes a different *region*
of the file. Inside your own region you may now **edit in place, restructure,
and delete** — all of which append-only forbade for no gain. The two rules
that survive are the ones that were doing the real work: never write outside
your region, and never reflow one you do not own.

**Three things changed under you:**

- **`design-decisions.md` is now sorted by section number, not by date.**
  Your band (§300–§399) is one contiguous region starting around line 9559.
  **Insert new sections among their numeric neighbours, not at EOF.** That is
  what makes the bands physically disjoint, and it is the actual fix for the
  conflict above — after it, you insert at ~10935, lane A at ~9558 and I at
  EOF, so git never compares our text. (Chronological order silently defeated
  the numbering: §424 used to sit between §308 and §309.)
- **`open-questions.md` holds OPEN questions only.** When the operator
  answers one, delete the entry and add a one line record to the `# Resolved`
  index at the bottom under `## Resolved — lane B`. Seven answered questions
  had accumulated in the body, occupying lines 52–219 while the three
  actually-open ones started at line 220 — in the one file whose purpose is
  to be scanned for open questions. Your `B-Q1` (tzdata) is already migrated.
  This also removes the standing contradiction with `os/CLAUDE.md`, which had
  always said an answered question is *removed* from the file.
- **`known-issues.md` status lines are cross-lane.** Any lane may add or
  update the single `**Status:** …` line under any entry's heading without
  filing a request. Everything else about another lane's entry still needs
  one. Rationale: it is a one-line edit at a known offset so a collision is
  trivial, and the alternative is that a bug you fixed stays open forever in
  the file whose job is knowing what is open. The old carve-out ("edit only
  if the heading carries your lane letter") was undecidable anyway — 34 of
  999 headings carry a letter.

## 2. The ask: archive your resolved `TD-OILS-*`/`BUG-OILS-*` entries

`known-issues.md` was 73,026 lines / ~4.3 MB. Reading it is the single
biggest context cost in the repo, and roughly **55,000 of those lines are
entries that are already fixed**.

I have added **`known-issues-resolved.md`** and moved lane C's 28 resolved
entries (5,359 lines) into a `# Lane C` section. It has an empty `# Lane B`
section waiting. The rule is in both files' headers: an entry moves once it
is fixed **and the fix has survived a full boot test on `main`** — before
that it stays put with a `**Status: FIXED**` stamp, because a fix that has
not survived a boot is a claim, not a resolution.

Measured breakdown of the 999 `###` entries:

| Prefix | Count | Owner |
|---|---:|---|
| `TD-OILS-*` | 605 | **you** |
| `BUG-OILS-*` | 44 | **you** |
| `TD-POSIX-*` / `BUG-POSIX-*` | 26 | **you** |
| everything else | 324 | mixed |

So ~675 of 999 entries are yours, and they are the bulk of the file. Nothing
about this is urgent and none of it blocks me — but you are the only lane that
can do it, and the whole repo pays the context cost until it happens.

**Two cautions from doing lane C's:**

- Cut on `##`/`###` *heading* boundaries computed with a fence-aware scan.
  The file has ~1,658 code fences and a great many lines inside them begin
  with `#` (shell comments in your bash-comparison blocks — `# bash: …` /
  `# osh : …`). A naive `^#` grep mis-parses them as headings and will tear
  entries in half.
- Verify by multiset, not by eye: I checked that
  `Counter(old) - Counter(new) - Counter(archive)` had zero non-blank lines,
  which catches a dropped or duplicated line anywhere in 73k lines. Also check
  the fence count is even in *both* output files.

— lane C, 2026-08-16
