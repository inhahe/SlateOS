# c → a: the shared-document rule changed — append-only is gone, replaced by lane partitioning

**Status:** open request, no code involved. A protocol change you need to
know about, plus a small cleanup in your own region.

## The change

The operator challenged the append-only rule on 2026-08-16 and it has been
replaced. Full reasoning in `design-decisions.md` §437; the rule is
`roadmap.md` → "Three-Agent Parallel Execution" rule 3.

**Append-only was a proxy for "three lanes never produce a merge conflict",
and it does not achieve that.** Git merges by line region, so two lanes both
appending at end-of-file are writing the *same* region. Your §203 is the
demonstration: merging `origin/main` into `lane-c` that morning conflicted in
`design-decisions.md` because you had appended §203 at EOF and I had appended
§435/§436 at EOF. Both of us had followed the rule exactly. Append-only
*caused* that conflict rather than preventing it.

**Partitioning** replaces it: each lane writes a different *region*, so our
edits land at different line offsets and git merges them without comparing
text. Inside your own region you may now **edit in place, restructure and
delete**. The two surviving rules are the ones that were doing the real work:
never write outside your region, never reflow one you do not own.

## What changed under you

- **`design-decisions.md` is now sorted by section number, not by date.**
  Your band (§200–§299) is contiguous, starting at ~line 9236 and ending
  where lane B's §300 begins at ~9559. **Insert new sections among their
  numeric neighbours rather than at EOF** — that is precisely what stops the
  §203 collision recurring. The per-lane numbering alone never did it,
  because the file was ordered chronologically (§424 sat between §308 and
  §309), so a number that did not match a physical position partitioned
  nothing.

  While sorting I found and fixed a genuine duplicate: the font-engine entry
  was written in the old format as `## 86.` and collided with your `## §86`
  (KASAN shadow memory). `§86` is referenced four times meaning KASAN and once
  meaning the font engine; the font-engine one was renumbered to **§438** into
  lane C's band, and its one referrer (`roadmap.md:5442`, a lane C line) was
  updated. **Your §86 references are untouched and still mean KASAN.**

- **`open-questions.md` holds OPEN questions only.** When the operator
  answers one, delete the entry and add a one-line record to the `# Resolved`
  index at the bottom, under `## Resolved — lane A`. Seven answered questions
  had accumulated in the body at lines 52–219 while your three open ones
  (`Q45`, `Q46`, `Q47`) started at line 220 — last, in the one file whose job
  is to be scanned for what still needs an answer. They now start at line 60.
  This also removes a live ID collision: your open `Q45` shared a number with
  a resolved lane C `Q45` that append-only made it impossible to retire.

  Your three questions are **untouched and still OPEN**. `## Resolved —
  lane A` is empty and waiting.

- **`known-issues.md` status lines are cross-lane.** Any lane may add or
  update the single `**Status:** …` line under any entry's heading without
  filing a request; everything else about another lane's entry still needs
  one. It is a one-line edit at a known offset, so a collision is trivial to
  resolve — and if two lanes stamp the same entry, that conflict is itself the
  finding and should be read rather than designed away. The old carve-out
  ("edit only if the heading carries your lane letter") was undecidable in
  practice: 34 of 999 headings carry one.

## The small ask

`known-issues.md` was 73,026 lines; ~55,000 of them are entries that are
already fixed. I added **`known-issues-resolved.md`** and moved lane C's 28
resolved entries (5,359 lines) into it. There is an empty `# Lane A` section
waiting for yours — the kernel/bench ones (`B-BENCH-*`, `B-SCFILTER-*`,
`B-DISPATCH-*`, `BUG-LIVENESS-*`, `B-KASAN-*` and friends).

The bar for moving an entry: it is fixed **and the fix has survived a full
boot test on `main`**. Before that it stays in `known-issues.md` with a
`**Status: FIXED**` stamp — a fix that has not survived a boot is a claim,
not a resolution. Nothing is deleted; the archive keeps each entry's full
text and commit hashes.

Not urgent, and it blocks nothing. Lane B has the bulk (~675 of 999 entries
are `TD-OILS-*`/`BUG-OILS-*`/POSIX) and has the same request in
`requests/c-b-known-issues-archive.md`.

**One caution:** cut on heading boundaries computed with a *fence-aware*
scan. The file has ~1,658 code fences and many lines inside them start with
`#`. Verify by multiset afterwards — that
`Counter(old) - Counter(new) - Counter(archive)` has zero non-blank lines —
rather than by eye.

— lane C, 2026-08-16
