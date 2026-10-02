## `A-CREATEENTRY-HAS-NO-MTIME-SO-EVERY-ARCHIVE-FORMAT-LOSES-IT` (lane A, 2026-08-27)

**Status:** **FIXED 2026-08-27.** Split out of
`A-KERNEL-ZIP-WRITERS-DISCARD-THE-MTIME-THEY-ALREADY-HAVE` above, whose `kshell`
half is fixed. This is the other half, and it is a different change to a
different struct.

**How it was fixed.** `CreateEntry` gained `pub modified_ns: Timestamp`;
`create_zip` encodes it with `tzrules::dos_datetime_from_unix`, and
`create_tar`/`create_cpio`/`create_ar` store whole Unix seconds via the new
`unix_seconds_u64` / `unix_seconds_u32` helpers. `kshell`'s `archive create`
stats each input next to the `read_file` it already did. A new self-test,
`test_mtime_reaches_every_writer`, asserts the *encoding* in all four formats
against a hand-computed expectation, plus that an unavailable time still reaches
ZIP as `0` rather than clamping to 1980.

**On step 3's open question (the unknown case in tar/cpio/ar):** resolved by
making it unreachable rather than by choosing a fabrication. `0` now arises only
when the clock was never set, a state in which *every* timestamp in the system
is `0` and a 1970 reading is at least systemically consistent rather than
uniquely wrong. No survey of GNU tar was needed: the question only had force
while the caller was *guaranteed* to supply no time, and it no longer is.

**One thing this did not fix, deliberately:** `list_zip` still discards the
DOS pair it can now read back, so `archive list` reports `0` for a ZIP whose
time `archive create` just wrote correctly. Tracked separately as
`A-ARCHIVE-LIST-DISCARDS-THE-ZIP-TIMESTAMP-IT-JUST-LEARNED-TO-WRITE` below.

**In short:** the `archive create` command can write four archive formats — zip,
tar, cpio and ar. None of them records when any file was last changed, because
the struct the command hands to all four (`CreateEntry`) has no field for a
time. For tar, cpio and ar this is worse than merely missing: those formats have
no way to say "unknown", so the zero being written **is** a date, and it reads
as **1970-01-01 00:00:00**. Every file in every tar SlateOS writes claims to
have been last modified at the dawn of the Unix epoch.

**Why zip and tar differ, and why that matters.** In ZIP, `0` is not a date at
all — it is day 0 of month 0, which no calendar can name, so a reader shows a
blank and is telling the truth (design-decisions.md §618, §621). In tar, cpio
and ar the field is a plain count of seconds since 1970, so `0` is exactly as
valid as any other value and a reader has no way to tell a missing time from a
genuine one. The same literal zero is honest in one format and a fabrication in
the other three. This is the same trap as
`A-ZIPARCHIVE-CREATE-STAMPED-EVERY-MEMBER-1980-01-01`, arrived at from the
opposite direction: there a made-up date was written where none was known; here
a "none" sentinel is written into a format that reads it as a date.

**Where it lives.** `kernel/src/fs/archive.rs`:

| Site | Field | Currently | What a reader sees |
|---|---|---|---|
| `CreateEntry` (~115) | — | `{ name, data, kind }`, no time at all | — |
| `create_zip` (~692) | `dos_datetime` | `0` | blank — honest |
| `create_tar` (~712) | `mtime` | `0` | `1970-01-01` — a fabrication |
| `create_cpio` (~735) | `mtime` | `0` | `1970-01-01` — a fabrication |
| `create_ar` (~760) | `mtime` | `0` | `1970-01-01` — a fabrication |

Note that three of the four writers **already have the field** and are passing a
literal zero into it. Only the source of the value is missing, which is why this
is one change to `CreateEntry` and not four changes to four writers.

**What the proper fix looks like.**

1. Add `pub modified_ns: Timestamp` to `CreateEntry`, using the same `0 = not
   available` convention as `FileMeta::modified_ns`, and update the construction
   sites (there are several in `archive.rs`'s own self-tests).
2. `create_zip` converts with `tzrules::dos_datetime_from_unix(modified_ns /
   1_000_000_000)`, which already maps 0 → 0 because 1970 is before the DOS
   epoch. That helper exists as of `37c04848e`; this is now the second caller,
   which is the condition §621 said should trigger sharing it.
3. tar/cpio/ar take `modified_ns / 1_000_000_000` directly — no calendar needed,
   the field is already Unix seconds. **They still need a decision for the
   unknown case**, since 0 means 1970 to them and there is no sentinel. The
   likely answer is that the caller must supply a real time and the writers stop
   pretending otherwise; a survey of what GNU tar does with an unknown mtime
   would settle it.
4. Whoever calls `archive::create` must then actually stat its inputs. Check
   each caller — a widened struct that everyone fills with `0` fixes nothing.

**Why it is worth doing separately.** Widening `CreateEntry` touches every
construction site and all four writers, and step 3 contains a real open question
about formats with no "unknown" encoding. Bundling that into the `kshell` fix
would have held a working, tested, boot-verified change behind an unresolved
design question.

**How it was found.** By fixing the entry above: `archive.rs` was the second of
the two sites listed there, and reading it showed that the missing time was not
merely unwired but had nowhere to live.
