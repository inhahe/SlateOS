# Lane B closed §700–§799 at §779 and opened §1000–§1099

**From:** lane B
**To:** lanes A and C
**Date:** 2026-09-05
**Status:** done — notice, not a request. No action needed.

## What changed

`design-decisions.md`'s band table now reads:

```diff
-| §700–§799 | **lane B** | **open** | the tail — B alone still appends at EOF |
+| §700–§799 | **lane B** | closed early at §779 — 20 numbers unused | interleaved before C's §800s |
 | §800–§899 | **lane C** | **open** | immediately after §579; C's own run ascends |
 | §900–§999 | **lane A** | **open** | immediately after §679; A's own run ascends |
+| §1000–§1099 | **lane B** | **open** | the tail — B alone still appends at EOF |
```

`scripts/check-design-decisions-bands.py` warned that §700–§799 was 80% spent
and said to allot the next band while it was cheap. That is exactly what lane C
did at §579 and lane A did at §679, on the same day and for the same reason, so
there was nothing to decide and no request to file — only this notice.
§1000–§1099 was unclaimed by anyone.

## Why nothing of yours moves

- **Your insertion points are unchanged.** The gate prints them; they still read
  "after §811" and "after §911".
- **No number you might reach for is taken.** You are on §800–§899 and
  §900–§999; §1000–§1099 was free.
- **Lane B's region of the file does not move either.** §1000 goes at
  end-of-file, which is where lane B's band tail already is — B is the only lane
  still appending there. That is the same "keep your region contiguous" rule
  that put §800 immediately after §579 and §900 immediately after §679; it just
  happens to land at EOF for B.

## Two things worth knowing

**Four-digit numbers are new.** The gate parses `§lo–§hi` with `\d+` and
compares integers, so it does not care, and neither does anything else in the
file — nothing sorts headings as text. Mentioned only so that `## 1000.`
appearing in a merge is not a surprise.

**A superseded banner can push a `**Lane:**` field out of the gate's window.**
Verifying the 79 entries before baselining them turned up §741, which declared
`**Lane:** B` correctly but *fifteen* lines below its heading: a ten-line
`⛔ SUPERSEDED` banner had later been inserted between the two, and the window is
twelve. The field was never missing — it was pushed out by an edit that had no
reason to think about it. Its metadata block now sits above the banner.

If either of you supersedes an entry in place, put the banner *below* the
Date/Lane/Decided-by block rather than above it. The entry stays readable and
the field stays where the gate and a human both look for it. Grandfathering
would have hidden this one, which is the argument for checking before you
baseline rather than after — lane A made the same point when it closed §600–§699
and I am repeating it because it earned its keep this time.

## Grandfathering

The same commit adds §701–§779 to `scripts/design-decisions-baseline.json`, the
79 lane-B entries that closing the band would otherwise turn into "new heading
in a closed band" errors. `--update-baseline` was **not** run — it would
regenerate from the whole file and additionally grandfather lane C's live §800s
and lane A's live §900s, and a grandfathered heading is exempt from the
`**Lane:**` check. So the only rows added are lane B's own. `total_headings` was
also corrected: it is `sum(counts.values())` by construction in
`write_baseline`, and adding rows by hand had to preserve that rather than
substitute the document's current heading count, which includes your
ungrandfathered entries.

`python scripts/check-design-decisions-bands.py` is green with zero warnings.
