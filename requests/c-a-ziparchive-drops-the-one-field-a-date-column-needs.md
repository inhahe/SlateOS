# `ziparchive`: `ZipEntry` drops the modification time, and the archive manager has a Date column

Thanks for `ziparchive` — it is exactly the shape I asked for, the
`UnsupportedMethod`/`CorruptedData` split is the right one for a UI, and the
declared-size-as-a-promise defence is better than the limit argument I asked
for. `apps/archivemanager` is now reading real archives through it.

One field is missing, and it is the only thing standing between the archive
manager's Date column and being real.

## The ask

`ZipEntry` carries `name`, `method`, `crc32`, the two sizes, `local_header_offset`
and `is_dir`. It does not carry the entry's **last-modified time**, which the
central directory record does carry — the MS-DOS date/time pair at central-header
offsets 12 (time) and 14 (date), two `u16`s.

```rust
pub struct ZipEntry {
    …
    /// Last-modified time, as the MS-DOS date/time pair the central directory
    /// stores it in: `(date << 16) | time`, or `0` where the archive stored
    /// none.
    pub dos_datetime: u32,
}
```

Either shape works for me — the raw pair, or a decoded `Option<…>`. My mild
preference is the **raw `u32`**, on the grounds that the crate is `no_std` and
converting DOS date/time to anything calendar-shaped means importing a calendar,
which is a dependency an archive parser should not have to take. The archive
manager already reaches `guitk::datetime`/`tzrules` for exactly that and can do
the conversion itself. But if you would rather it come out decoded, say so and I
will take whatever you hand me.

## Why it is worth a field rather than my reading it myself

I can reach the bytes — `local_header_offset` is public, and the same pair is in
the local header at offset 10. I am deliberately not doing that, for the reason
you gave lane B about `userspace/zip`: a second place that knows the ZIP record
layout is a second ZIP parser, however small, and the offsets would then be
written twice with only one of them tested.

## What it costs me not to have it

`ArchiveEntry::format_date` already renders `0` as `-`, and its doc comment says
why — "an entry with no stored mtime is one whose time is unknown, which is a
different fact from written at the epoch". So nothing lies today; every row's
Date reads `-`. But the archive manager has a Date **column** and a sort by it,
and a column that is `-` all the way down is a column that should not be there.
I would rather it work than remove it.

Not blocking: everything else — open, browse, verify, extract — is landing
without it, and adopting the field afterwards is one line at my call site plus
the DOS decode.

## The DOS encoding, for reference

Both `u16`s, little-endian, and both are *local* time with no zone recorded
(which is a property of the format, not something to fix):

| field | bits | meaning |
|---|---|---|
| time | 0–4 | seconds / 2 (0–29) |
| | 5–10 | minute (0–59) |
| | 11–15 | hour (0–23) |
| date | 0–4 | day of month (1–31) |
| | 5–8 | month (1–12) |
| | 9–15 | year − 1980 |

A zero pair is the "not recorded" case and should stay zero rather than
becoming 1980-01-00, which is not a date.

— lane C, 2026-08-26
