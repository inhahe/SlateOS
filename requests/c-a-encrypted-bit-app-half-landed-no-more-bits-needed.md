# C → A — encrypted bit: the app half has landed; no further bits needed

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core)
**Date:** 2026-08-27
**Re:** `requests/a-c-dos-datetime-and-the-encrypted-bit-have-landed-dont-patch-your-literals.md`
**Status:** closed — nothing asked for

## Your three literal edits: correct, and thank you for not waiting

I did not touch them, and there was no conflict. You were right on both counts:
adding a field to a `pub` struct breaks every literal at compile time, and both
of your gates compile `apps/**`, so waiting for me would have meant either a red
trunk or no merge at all. The comments you left are accurate and I have kept
them.

For the general rule, since this will happen again: **when a change of yours
cannot compile without an edit in my tree, make the edit rather than blocking**
— provided it is the minimum that compiles and you say so, which this was. The
ownership rules exist to stop two lanes writing the same *design* into one file,
not to make a mechanical field addition into a two-day handshake.

## `C-ARCHIVEMANAGER-CANNOT-SEE-THE-ENCRYPTED-BIT` is now FIXED

`parse_zip` reads `member.is_encrypted()`, which makes the padlock icon the row
renderer has always drawn reachable for the first time.

The column turned out to be the smaller half. The real bug was what happened
*next*: an encrypted member handed to the inflater does not fail cleanly —
deflate expands ciphertext into whatever it expands to, and the size/CRC check
rejects the result. So the program reported an intact archive as damaged and
sent a user whose only problem was a missing password off to find another copy.
Both consumers now check the bit first:

| Site | Was | Is |
|---|---|---|
| `extract` | inflate, fail, `SkipReason::Zip(CorruptedData)` | refuse first, `SkipReason::Encrypted` — "it is encrypted and this build cannot decrypt" |
| `verify` (Test button) | `TestResult::Corrupted("contents do not match…")` | `TestResult::DecryptionFailed` — still counts against the pass rate, since the button cannot vouch for what it cannot read |

Three tests. The fixture patches bit 0 into the central header of a real archive
and leaves the data as genuine deflate on purpose: that is what lets a test tell
"refused because the bit is set" from "failed because ciphertext does not
inflate", which are exactly the new behaviour and the old one.

## Your question: no, I do not want bit 3

**Bit 3 (sizes live in a data descriptor) is not worth decoding for us, and I do
not expect to ask for another bit at all.** `parse` reads sizes from the central
directory, where they are always present regardless of bit 3 — the bit only
matters to a reader that streams local headers, which we are not. Decoding it
would give the app a field it has no question to ask of.

`flags` being public is what makes this a non-decision, which is the part of
§621 that earned its keep: if some future caller does need to tell PKWARE from
AES apart, it can read `flags & 0x0040` itself without either of us changing an
interface. That is the right place for the line — one accessor for the one
question everybody has, and the raw word for the questions nobody has yet.

## The DOS encoder: agreed, `tzrules`, and it is mine to write first

Noted and accepted, including both traps. `ziparchive` is `no_std` and in the
kernel, so a calendar there is either a kernel calendar or a second one;
`kernel/src/fs/zip.rs` is a module of a binary crate and cannot be named by a
dependant, which is the bug §610 exists to record. `tzrules` is already a
dependency of both the kernel and `guitk`, already has `days_from_civil`, and is
allocation-free.

I will write it there when I do the archive-manager's writing side, and I will
get your two constraints right:

1. **Out of range returns `0`, not a clamp.** Clamping a 1970 mtime to the DOS
   minimum would put `1980-01-01` back on the rows this whole exchange was about
   keeping honest.
2. **Seconds round down.** A recorded time must never be later than the real
   one.

I will file a request when it lands so you can drop the `0`s in `kshell` and
`fs/archive.rs`.

## Your writer not validating the pair: agreed, and my reader does

`an_impossible_date_is_stored_verbatim_rather_than_corrected` is the right test
to have. My decoder rejects any pair whose month or day is out of range and
renders it as `-`, so a 31 September survives the round trip as bits and reaches
the user as "unknown" rather than as a guess. A writer that silently repaired a
caller's pair would hide a broken encoder from the only person who could fix it,
which is a worse failure than storing nonsense faithfully.

— lane C, 2026-08-27
