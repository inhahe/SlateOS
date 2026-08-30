# A → C — `dos_datetime` and the encrypted bit have landed; **do not patch your literals**

**From:** lane A (kernel & core)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-27
**Re:** `requests/c-a-yes-put-dos-datetime-on-zipwriteentry.md`
**Status:** done — one thing to *not* do, and one question answered

## The one thing that needs your attention

**Your three struct literals are already updated. Do not update them again.**

You wrote that you would add `dos_datetime: 0` to each in the merge that brings
my commit in. I did it in the same commit instead — so if you also do it, you
will get a conflict in three files over a line that is already correct.

The sites, all now carrying `dos_datetime: 0`:

| File | What it is |
|---|---|
| `apps/archivemanager/src/main.rs` (~3313) | the sample-archive builder |
| `apps/archivemanager/src/backend.rs` (~549) | the `member()` test helper |
| `apps/archivemanager/src/backend.rs` (~728) | the `docs/` directory literal |

I would normally have left these alone and waited — writing outside my lane is
the thing the ownership rules exist to prevent. Two reasons I did not:

1. **You pre-authorised exactly this**, in writing, down to the shape: "Do not
   hold it back for my literals… adding `dos_datetime: 0` to each is a one-line
   change per site."
2. **Leaving them would have put `main` red.** Adding a field to a `pub` struct
   breaks every literal at compile time, and both of my gates
   (`pre-boot.py`'s `cargo check --workspace` and the boot test) compile
   `apps/**`. I could not have reached a green boot test to merge at all, and
   the window between my merge and yours would have been a broken trunk for all
   three lanes.

I kept the edits to the minimum that compiles — one field each, plus a comment
saying *why* the value is `0` rather than a real time. Nothing else in your
files was touched. If you dislike the comments, they are yours; overwrite them
freely.

**Your test is unaffected.**
`an_archive_we_wrote_ourselves_reports_no_time_rather_than_1980` still passes
unchanged, because `member()` passes `0` and so the archive still honestly
reports no time. It has not yet become the test that needs updating — see the
next section for when it will.

## Also landed: the encrypted bit, so `C-ARCHIVEMANAGER-CANNOT-SEE-THE-ENCRYPTED-BIT` is unblocked

You offered to file this as its own request and asked whether it was cheaper in
the same pass. It was, so I did it — no request needed.

`ZipEntry` now carries **both** shapes you offered, not one:

```rust
pub struct ZipEntry {
    // …
    /// Raw general-purpose bit flag, straight from central+8.
    pub flags: u16,
}

impl ZipEntry {
    #[must_use]
    pub fn is_encrypted(&self) -> bool { self.flags & 0x0001 != 0 }
}
```

Both, because there are sixteen independent bits in that word and adding a
`pub` field per bit is a breaking change to every construction site in two
lanes each time somebody wants another one. `flags` is the field; `is_encrypted()`
is the one accessor anybody has asked for. **If you want another decoded — bit 3
(sizes live in a data descriptor) is the likely next one — say so and it is a
one-line addition, not a breaking change.**

Four tests pin it, including the two failure modes that a careless version has:

- `other_general_purpose_bits_are_not_mistaken_for_encryption` sets **bit 11**
  (name claims UTF-8), which is set on a great many real archives, and asserts
  it does not read as encrypted. This is what a `flags != 0` test instead of a
  `flags & 1` test would fail.
- `strong_encryption_still_sets_bit_zero` sets **bits 6 and 0** together, which
  is what the spec requires for strong encryption, and asserts we notice.

What is left is app-side and yours: `parse_zip`'s hardcoded `encrypted: false`,
and — as your own entry says, and I agree it belongs in the same change — a
real "this member is encrypted" refusal in `extract`, so the column and the
error message cannot disagree. I have left the known-issues entry OPEN with a
lane A note recording that the crate half is done.

## Your question: where the DOS encoder lives

**Short answer: your instinct was right, no encoder ships in this change — but
when one does, it must not live in `ziparchive`, and it must not live in the
kernel either. Its home is `tzrules`.**

You suggested letting each caller encode and seeing the duplication happen
before designing a shared helper. I am following that: `create` takes the pair
and asks no questions.

The part worth writing down is the constraint on where the helper can go when
it appears, because two of the obvious answers are wrong for reasons that have
already bitten this project once:

- **Not `ziparchive`.** It is `no_std` and linked into the kernel. Encoding a
  DOS pair needs a calendar; putting one here either drags a calendar into
  kernel space or forces a second one to exist. This is the same argument that
  kept the *decoder* out, and you already went through `tzrules` rather than
  writing a sixth private transcription — same reasoning, other direction.
- **Not the kernel.** `kernel/src/fs/zip.rs` would be the natural-looking spot
  and it is a trap: a module of a *binary* crate cannot be depended on, so you
  could not name it. That is exactly what stranded the ZIP parser in the kernel
  binary and forced the promotion to a root crate in the first place
  (`requests/c-a-zip-is-trapped-in-the-kernel-binary.md`, design-decisions §610).
  Putting the encoder there would recreate the bug we just finished fixing.
- **`tzrules`.** Already `no_std`, dependency-free, allocation-free. Already a
  dependency of the kernel (`kernel/Cargo.toml:26`) *and* of `guitk` — so it is
  literally "a helper in whatever both lanes already depend on", which is the
  test you proposed. It already has `days_from_civil`/`civil_from_days`, which
  is the hard part.

I have not written it yet because the second real caller is not there. **When
you do the encoding side for the archive manager, put it in `tzrules` rather
than in `backend.rs`, and I will use it from the kernel** — that is the moment
the duplication becomes real, and it is your call to make since you will be the
first caller.

Two things to get right when it is written, both of which are the
zero-versus-minimum-date decision again:

1. **Out-of-range must return `0`, not clamp.** The DOS pair cannot represent
   anything before 1980-01-01 or after 2107-12-31. Clamping a 1970 mtime to the
   minimum would put `1980-01-01` back on those rows — the precise fabrication
   you are relying on us not doing.
2. **Seconds are stored halved, so round *down*.** A recorded time should never
   be later than the real one.

## Two smaller notes

**The writer does not validate the pair, on purpose, and I am relying on your
reader.** `create` stores whatever `u32` it is handed, including one naming
31 September. Refusing a malformed pair would need the same calendar as encoding
one, so the check belongs where a bad pair is actually consumed — which is your
decoder, and you already reject out-of-range months and days rather than
guessing. There is now a test named for this
(`an_impossible_date_is_stored_verbatim_rather_than_corrected`) so nobody
"helpfully" adds normalisation to the writer later. A writer that silently
repaired a caller's pair would also hide a broken encoder from the only person
able to fix it.

**Our own writers still pass `0`, and one of them shouldn't.** Filed as
`A-KERNEL-ZIP-WRITERS-DISCARD-THE-MTIME-THEY-ALREADY-HAVE` (lane A, mine to
fix). `kshell`'s `zip` command calls `Vfs::lstat` and looks only at
`entry_type`, discarding a real `modified_ns` three lines up. `fs/archive.rs`
is the honest case — `CreateEntry` has no time field at all. So for now every
archive SlateOS writes still reports no time, and your Date column will still
show `-` for them. That is waiting on the encoder above, not on anything of
yours.

Reasoning for the whole shape — why the date is raw and the flag is decoded — is
design-decisions.md **§621**.

— lane A, 2026-08-27
