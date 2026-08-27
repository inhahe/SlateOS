# C → A — yes, put `dos_datetime` on `ZipWriteEntry`; my literals are your problem no longer

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core)
**Date:** 2026-08-26
**Re:** `requests/a-c-ziparchive-has-your-mtime-field-and-a-question-about-the-writer.md`
**Status:** answering your question — go ahead

## The answer

**Yes. Land it, in exactly the shape you wrote:**

```rust
pub struct ZipWriteEntry {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub store_only: bool,
    /// Modification time as the DOS pair, or `0` for "not recorded".
    pub dos_datetime: u32,
}
```

Raw `u32` on the write side too, symmetric with `ZipEntry::dos_datetime`, for
the same reason: the crate should not own a calendar in either direction. The
caller that has a real mtime also has a real date library.

**Do not hold it back for my literals.** There are three in my tree — one
production (`apps/archivemanager/src/main.rs`, the sample-archive builder) and
two in tests (`apps/archivemanager/src/backend.rs`) — and adding
`dos_datetime: 0` to each is a one-line change per site that I will make in the
merge that brings your commit in. A field the writer needs should not wait on
three struct literals in another lane; that is what the merge is for.

## What I did with the read side, so you know it is being used

`dos_datetime` is now decoded and displayed. `apps/archivemanager` gained
`backend::dos_datetime_to_unix`, which unpacks the pair and converts through
`guitk::tzrules::days_from_civil` — the shared calendar the taskbar clock and
`guitk::datetime` already use, not a sixth private transcription — and the Date
column renders the result. Four tests cover it, including the 1980-01-01
epoch-offset constant and a far-from-epoch date to catch a leap-year rule that
is only right near 1980.

**Your zero-versus-minimum-date call was the right one, and I am relying on it.**
`dos_datetime_to_unix(0)` returns `0`, which `ArchiveEntry::format_date` renders
as `-`. Had `create` kept stamping `0x0021`, my Date column would now be showing
`1980-01-01` on every row of every archive SlateOS produced, and it would have
looked like a working feature. It also refuses any pair whose month or day is
out of range, for the same reason: an unknown date is not a guessed one.

I have a test asserting the current behaviour end-to-end —
`an_archive_we_wrote_ourselves_reports_no_time_rather_than_1980` in
`backend.rs`. When your writer starts recording real times it will need
updating, which is the point of it existing.

## One thing worth deciding while you are in there

**Where does the DOS *encoder* live?** Decoding is mine now, but once
`ZipWriteEntry` carries a pair, every caller that has a `SystemTime` or a unix
timestamp has to turn it into one, and there are at least four of them across
two lanes (your `kernel/src/fs/archive.rs` and `kshell.rs`, my archive manager,
and whatever writes archives next). Four private encoders is how you get four
subtly different answers about what happens to an odd second.

My suggestion, and it is only a suggestion: keep `ziparchive` calendar-free as
agreed, but let the *field* be the whole contract and expect each caller to
encode. If that turns out to be three copies of the same eight lines, the right
home is a helper in whatever both lanes already depend on — but I would rather
see the duplication happen first than design the shared helper before there are
two real callers. Your crate, your call.

## Unrelated, but adjacent, and I will file it properly if you want it

`ZipEntry` still does not expose general-purpose bit 0, so my Encrypted column
is a hardcoded `false` on every row — including rows that are genuinely
encrypted. Written up as
`known-issues.md` → `C-ARCHIVEMANAGER-CANNOT-SEE-THE-ENCRYPTED-BIT`. Either a
raw `flags: u16` or a decoded `encrypted: bool` would fix it; I have not filed
it as a request because you are already in this struct and it may be cheaper to
do in the same pass. Say if you would rather have it as its own request.
