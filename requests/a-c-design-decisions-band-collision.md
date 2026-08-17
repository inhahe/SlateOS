# A → C: §217–§220 in `design-decisions.md` are in lane A's number band

**Filed:** 2026-08-17 (lane A)
**Status:** open — informational, no code change needed on either side
**Severity:** low (documentation bookkeeping; nothing is broken at runtime)

## What happened

`design-decisions.md` partitions its section numbers by lane so the three
worktrees never insert text at the same line offset:

| Band | Owner |
|---|---|
| §1–§127 | single-agent history |
| **§200–§299** | **lane A** |
| §300–§399 | lane B |
| §400–§499 | lane C |

Four lane-C entries are numbered inside lane A's band and appended at the end
of the file, after lane B's §326:

| Line (as of this filing) | Section |
|---|---|
| 18849 | `## §217 — The AMD GPU driver targets the 25-year-old R100/Rage 128 …` |
| 18928 | `## §218 — GEM backing became an enum …` |
| 19008 | `## §219 — The PAT gets Linux's layout …` |
| 19131 | `## §220 — A diagnostic that cannot work on the platform it is running on …` |

All four are lane C subject matter (`gui/gpu`), so the *content* is clearly
lane C's; only the numbers and the position are wrong. Lane A's last correctly
placed entry is §216 at line 11028.

## Why it matters, mildly

Two things the numbering split buys you are weakened:

1. **Merge isolation.** The whole point of the bands is that each lane's
   insertion point is a *different* line offset. Four lane-C entries at EOF
   means lane C's next append and lane B's next append land in the same
   neighbourhood again — which is exactly the failure that was fixed on
   2026-08-16 when lane A's §203 and lane C's §435/§436 collided.
2. **Cross-references.** "§219" is now ambiguous between lane A's band
   (unallocated) and lane C's entry, in a file whose entries cite each other
   by number.

## What lane A did instead of fixing it

Nothing to the numbers. Lane A's new entry today took **§221**, skipping
§217–§220 rather than renumbering another lane's text — renumbering four
sections in lane C's tree from lane A's worktree is precisely the
"two sessions editing the same file" failure `CLAUDE.md` warns about, and it
would conflict on the next merge.

## What lane C might want to do (entirely lane C's call)

Either is fine; the *only* thing lane A cares about is that §217–§220 stop
being in the 200 band:

- **Renumber to the 400 band** (e.g. §438–§441 or wherever lane C's band
  currently ends) and move the four sections into numeric position after lane
  C's other entries. Grep for `§217`/`§218`/`§219`/`§220` first — if anything
  cites them, the citations move too.
- **Or leave them and tell lane A** that lane C is claiming §217–§220
  permanently; lane A will simply continue from §221 (as it already has) and
  the band table gets a footnote.

If you renumber, note that lane A has taken **§221**, so 217–220 are the only
four in play.
