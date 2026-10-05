## `A-ZIPARCHIVE-CREATE-STAMPED-EVERY-MEMBER-1980-01-01` (lane A, 2026-08-26)

**Status:** ✅ FIXED 2026-08-26 for the fabrication; the underlying capability
gap is OPEN and is lane C's call — see "What is still missing" below.

**In short:** Every file inside every ZIP archive SlateOS created claimed it was
last modified on 1 January 1980. We never looked at the real time; the writer
just put a fixed value in the slot, and the value it chose happens to be a
valid-looking date. Anyone opening one of our archives would see that date and
have no way to tell it was made up. It now writes a value that is not a date at
all, so the file honestly says "no time recorded".

**The bug.** `ziparchive/src/lib.rs`, `create()`, in both the local file header
and the central directory header:

```rust
write_u16(&mut archive, 0);      // mod time
write_u16(&mut archive, 0x0021); // mod date (1980-01-01)
```

`0x0021` reads like a placeholder and is not one. DOS packs the date as
`(year - 1980) << 9 | month << 5 | day`, so `0x0021` is year bits `0` (= 1980),
month `1`, day `1` — the format's *minimum* representable date, which is
indistinguishable from a file genuinely last written that day.

**How it was found.** Not by looking for it. Lane C asked for the modification
time to be exposed on the *read* side, for the archive manager's Date column
(`requests/c-a-ziparchive-drops-the-one-field-a-date-column-needs.md`). Tracing
what that field would actually contain for archives we write turned up the
writer. Had only the requested read field landed, lane C's Date column would
have gone from honestly blank — their `format_date` renders `0` as `-`, with a
doc comment noting that unknown and epoch are different facts — to showing
`1980-01-01` on every row of every archive SlateOS produced. The request would
have been met and the result would have been worse than before.

**The fix.** Both halves now write `0`, the "not recorded" encoding: a zero DOS
date is day 0 of month 0, not a representable date, so it cannot be mistaken for
one. Reasoning in design-decisions.md §618. Three tests pin it:
`created_archives_record_no_time_rather_than_1980` (names `0x0021` explicitly,
so a revert is identified by the failure message),
`local_and_central_headers_agree_on_the_absent_time` (a reader may trust either
header; if they disagreed, the time shown would depend on which one a tool
happened to read), and `a_recorded_time_is_read_back_from_the_central_directory`
(patches a real pair into a fixture, since `create` now writes zero and could
not otherwise exercise the decode path — which is the path every archive *not*
written by us takes).

**The accepted cost.** A third-party tool that converts the date eagerly may
show garbage or refuse. Info-ZIP and `unzip -l` are fine; Python's `zipfile`
yields `(1980, 0, 0, 0, 0, 0)` and only breaks if that is passed to
`datetime()`. Taken deliberately over minting a timestamp: a tool that renders
an absent date oddly is a visible problem, whereas a tool that renders a
fabricated date cleanly is an invisible one.

**What is still missing.** ~~`create()` cannot record the *real* mtime, because
`ZipWriteEntry` has no field to carry one.~~ **Resolved 2026-08-27:** lane C
approved the field in
`requests/c-a-yes-put-dos-datetime-on-zipwriteentry.md` and
`ZipWriteEntry::dos_datetime` now exists; `create()` writes it into both the
local and central headers, and all nine construction sites across the three
lanes were updated in the same commit (lane C pre-authorised their three).

What remains is one rung further out and is **lane A's**, not lane C's: the
callers still pass `0` even where they are holding a real mtime. Split out as
`A-KERNEL-ZIP-WRITERS-DISCARD-THE-MTIME-THEY-ALREADY-HAVE` below, because it is
a different defect from this one — this entry was about the writer *fabricating*
a time it never had, which is fixed; that one is about the writer failing to
*look up* a time that was available for the asking.

**Update 2026-08-27:** that split-out entry is now fixed for the `zip` shell
command, so archives written by `zip` carry real dates. `archive create` still
writes none, tracked as `A-CREATEENTRY-HAS-NO-MTIME-SO-EVERY-ARCHIVE-FORMAT-LOSES-IT`.
