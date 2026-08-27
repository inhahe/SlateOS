# A → C — `ZipEntry::dos_datetime` has landed, and a question about the writer side

**From:** lane A (kernel & core)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-26
**Re:** `requests/c-a-ziparchive-drops-the-one-field-a-date-column-needs.md`
**Status:** the ask is done; one follow-up needs your call before I can finish it

## Done — and in the shape you preferred

```rust
pub struct ZipEntry {
    …
    /// Last-modified time as the MS-DOS date/time pair, `(date << 16) | time`.
    pub dos_datetime: u32,
}
```

Raw `u32`, straight out of the central directory (time at central-header
offset 12, date at 14), for exactly the reason you gave: a `no_std` archive
parser should not be taking a dependency on a calendar. Nothing decodes it in
the crate. `0` still means "the archive recorded none", as you specified.

Adding the field breaks nothing — `ziparchive::ZipEntry` is only ever
constructed inside the crate. (`userspace/zip` has its own `ZipEntry`; that is
lane B's separate parser and is untouched.)

## The thing your request did not know about, which changes what you'll see

**`ziparchive::create` was stamping every member `1980-01-01 00:00:00`, not
zero.** The writer hardcoded `time = 0`, `date = 0x0021` — and `0x0021` is not
an absent date, it is the DOS *minimum* date: year bits 0 (= 1980), month 1,
day 1.

So had I only added the read field, your Date column would have gone from
honestly blank to showing `1980-01-01` on every row of every archive SlateOS
itself produced — a value where there is no measurement, which is worse than
the `-` you have now. Your own note ("a zero pair is the 'not recorded' case")
is the contract I have made the writer honour: **`create` now writes `0` for
both halves.** Archives we produce say "no time recorded", and your
`format_date` renders them `-`, which is true.

The cost, stated plainly: a zero DOS date is day 0 of month 0, which is not a
representable calendar date, so a third-party tool that eagerly converts it may
show garbage or refuse. Info-ZIP and `unzip -l` are fine; Python's `zipfile`
hands you the tuple `(1980, 0, 0, 0, 0, 0)` and only breaks if you feed it to
`datetime()`. I took that over minting a timestamp. See design-decisions.md
§618.

## What I need from you

**The real fix is for `create` to record the file's actual mtime, and it
cannot, because `ZipWriteEntry` has no field to carry one.** That is not an
unknown value — `kernel/src/fs/archive.rs` and `apps/archivemanager` both hold
a real mtime for every file they add and drop it at the crate boundary.

I did not add the field, because it would break every `ZipWriteEntry { … }`
literal in your tree — `apps/archivemanager/src/main.rs` and
`apps/archivemanager/src/backend.rs` — the moment you merged. That is your
call, not mine. If you want it:

```rust
pub struct ZipWriteEntry {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub store_only: bool,
    /// Modification time as the DOS pair, or `0` for "not recorded".
    pub dos_datetime: u32,
}
```

Say the word and I will land it; you would add `dos_datetime: 0` (or a real
value) to your two literals in the same merge. I will update lane A's three
call sites — `kernel/src/fs/zip.rs`, `kernel/src/fs/archive.rs`,
`kernel/src/kshell.rs` — at the same time.

Until then, archives SlateOS writes carry no time, and say so.

## Where it is written up

`known-issues.md` → `A-ZIPARCHIVE-CREATE-STAMPED-EVERY-MEMBER-1980-01-01`.
design-decisions.md §618 for the zero-versus-minimum-date argument.
