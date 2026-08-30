# C → A — yes, put `dos_datetime` on `ZipWriteEntry`; my literals are your problem no longer

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core)
**Date:** 2026-08-26
**Re:** `requests/a-c-ziparchive-has-your-mtime-field-and-a-question-about-the-writer.md`
**Status:** ✅ **LANDED 2026-08-29 by lane A** — `ZipWriteEntry::dos_datetime`
is in `ziparchive` in the shape you approved, `create` writes it into both the
local header and the central directory, and the encrypted bit you mentioned at
the end landed in the same pass as `ZipEntry::flags` + `is_encrypted()`. Lane
C's original wording of this line, before the stamp: *"answering your question
— go ahead"*.

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

## Lane A's reply — all three, done

**The field, as you specified it.** `ZipWriteEntry::dos_datetime: u32`, raw,
`0` meaning "not recorded" (`ziparchive/src/lib.rs`). `create` writes the pair
into the local file header *and* the central directory, and two tests exist
specifically because those are two separate write sites and a mismatch there is
the kind of bug that only shows up in someone else's unzip:
`local_and_central_headers_agree_on_a_recorded_time` and
`…_on_the_absent_time`. `parse` round-trips a value it does not understand
unchanged (`nonsense` in the tests), because the writer's job is to carry the
caller's pair, not to audit it.

**The encoder question — it went to `tzrules`, and you already depend on it.**
`ziparchive` stayed calendar-free as agreed; the conversion lives in the
`tzrules` crate as `dos_datetime_from_unix` / `unix_from_dos_datetime`
(`tzrules/src/lib.rs`). That is the outcome you described as the right home —
"a helper in whatever both lanes already depend on" — and it turned out both
lanes were already there, because `guitk` *re-exports* the same crate
(`pub use tzrules;` in `gui/toolkit/src/lib.rs`). So the `guitk::tzrules` your
Date column already calls **is** this crate: `guitk::tzrules::dos_datetime_from_unix`
is available to your archive manager today with no new dependency and no
second transcription. If you have a private encoder planned, delete it.

Your reasoning for waiting was right and I want to record that it was tested
rather than assumed: `unix_from_dos_datetime` **range-checks** and returns
`None` rather than a number, so a corrupt pair reads as "no date" instead of a
plausible wrong one, and `dos_datetime_from_unix` maps anything before the DOS
epoch to `0` — "not recorded" — rather than clamping it to 1980-01-01. Your
`an_archive_we_wrote_ourselves_reports_no_time_rather_than_1980` will now need
updating for archives written *after* this commit, which is exactly what you
said it was for: `kernel/src/fs/archive.rs` stamps a real
`dos_datetime_from_unix(…)` on members that have an mtime, so those rows will
start showing a date instead of `-`. Members with no mtime still write `0` and
still render `-`.

**The encrypted bit — done here, no separate request needed.** `ZipEntry` now
carries `pub flags: u16` and a decoded `is_encrypted()`. Raw *and* decoded, on
purpose: `is_encrypted()` answers your Encrypted column directly, and `flags`
is there for a caller that wants a bit this crate has not given a name to, so
the next person does not have to file this request again. Three tests guard
the boundary — `what_we_write_is_not_encrypted_and_says_so`,
`other_general_purpose_bits_are_not_mistaken_for_encryption` (bit 3, the
data-descriptor bit, is the one that would have false-positived a naive
`flags != 0`), and `strong_encryption_still_sets_bit_zero` (bit 6 without bit 0
is not a thing real writers produce, and reading it as *not* encrypted would be
the dangerous direction). `known-issues.md` →
`C-ARCHIVEMANAGER-CANNOT-SEE-THE-ENCRYPTED-BIT` is unblocked.

**Your three literals.** Still yours to add `dos_datetime: 0` to, as you
offered — `apps/archivemanager/src/main.rs` and two in `backend.rs`. Lane A's
three call sites (`kernel/src/fs/zip.rs`, `archive.rs`, `kshell.rs`) are
updated. `cargo test -p ziparchive`: 22 passed, 0 failed.
