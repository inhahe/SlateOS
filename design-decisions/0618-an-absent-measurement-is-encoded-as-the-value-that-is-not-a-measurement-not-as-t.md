## 618. An absent measurement is encoded as the value that is not a measurement — not as the smallest one the format can hold

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When SlateOS makes a ZIP file, each file inside it has a slot for
"when was this last changed". We do not know that time — nothing was passing it
in — so the slot had to hold *something*. It was holding 1980-01-01, the
earliest date the ZIP format can express. That reads to anyone opening the
archive as a real answer: it looks exactly like a file that really was last
saved on 1 January 1980. The fix was to write zero instead, which is not a valid
date at all (day 0 of month 0), so a reader can tell that no time was recorded
rather than being handed a wrong one.

**The general rule this is an instance of:** where a format has a distinguished
"none" encoding, use it. Where it does not, prefer a value that is *impossible*
as a measurement over the minimum of the valid range. The minimum is
indistinguishable from a real reading at the bottom of the range; the impossible
value cannot be mistaken for anything. This is the same principle as
`/proc/numastat` refusing to report modelled distances as measured ones (§617's
neighbourhood) and as the fabricated seed rows removed from the statistics
modules: a plausible number where there is no measurement is worse than no
number, because it destroys the reader's ability to know which they are holding.

**The specific case.** `ziparchive::create` hardcoded `time = 0`,
`date = 0x0021`. The `0x0021` looks like a placeholder and is not one: DOS packs
the date as `(year - 1980) << 9 | month << 5 | day`, so `0x0021` is year bits 0,
month 1, day 1 — 1980-01-01, the format's minimum. Every member of every archive
SlateOS wrote carried it.

This surfaced because lane C asked for the mtime to be exposed on the read side
for the archive manager's Date column (`requests/c-a-ziparchive-drops-the-one-
field-a-date-column-needs.md`). Delivering only the read field would have turned
their column from honestly blank (`-` on every row, which their `format_date`
already renders for zero, with a doc comment explaining that unknown and
epoch are different facts) into `1980-01-01` on every row of every archive we
produced. The request would have been satisfied and the result would have been
worse.

**The alternatives considered.**

| Option | Why not |
|---|---|
| Keep `0x0021` | Fabricates a timestamp. The failure mode is silent and unfalsifiable from the archive alone. |
| Write the current wall-clock time at archive-creation | Worse: a confident, plausible, *wrong* mtime that changes every run. Records when we packed the file, labelled as when the file was modified. |
| Write zero | Chosen. Not a representable date, so it cannot be read as one. |
| Block until `ZipWriteEntry` can carry a real mtime | The right end state, but it is a lane-C interface change (their two struct literals). Filed as a request; meanwhile zero is honest and 1980 is not. |

**The cost, stated plainly.** A zero DOS date is day 0 of month 0. A third-party
tool that converts eagerly may show garbage or refuse: Info-ZIP and `unzip -l`
handle it, and Python's `zipfile` hands back the tuple `(1980, 0, 0, 0, 0, 0)`,
which only breaks if fed to `datetime()`. That is a real interoperability cost
and it was accepted deliberately, because the alternative is not "no cost" but
"the cost is paid by someone who cannot see they are paying it". A tool that
renders our absent date oddly is a visible problem; a tool that renders a minted
date cleanly is an invisible one.

**Where it bites:** `ziparchive/src/lib.rs` — the local file header and the
central directory header, which must agree (there is a test pinning that, since
a reader may trust either). `ZipEntry::dos_datetime` documents the same contract
on the read side. known-issues.md →
`A-ZIPARCHIVE-CREATE-STAMPED-EVERY-MEMBER-1980-01-01`.
