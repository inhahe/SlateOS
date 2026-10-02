## 621. A format crate carries the format's own facts, and refuses the calendar — so `flags` is decoded and `dos_datetime` is not

**Date:** 2026-08-27
**Decided by:** Claude (autonomous), answering a question lane C left open in
`requests/c-a-yes-put-dos-datetime-on-zipwriteentry.md`
**Lane:** A

**In short:** a ZIP file records, for each member, when it was last modified
and whether it is encrypted. Both arrive as raw numbers. We decided to hand the
*date* to callers as a raw number and let each one convert it, but to decode
the *encrypted* flag inside the crate and expose a plain `is_encrypted()`. The
difference is not effort — it is that "bit 0 means encrypted" is a fact about
the ZIP format itself, whereas "day 26 of month 8 of 2026" needs a calendar,
and a calendar is a thing this crate deliberately does not own.

**Context.** `ziparchive` is the shared ZIP reader/writer, promoted to a root
crate (§610) so that the kernel and `apps/archivemanager` use one parser rather
than two. It is `no_std`, allocation-light and dependency-light, because it is
linked into the kernel as well as into a graphical application.

Two fields landed in it at once, and they pull in opposite directions:

| Field | On disk | What a caller wants |
|---|---|---|
| `dos_datetime` | packed MS-DOS date/time pair, `(date << 16) \| time` | a real date, in the user's timezone |
| `flags` | general-purpose bit flag, 16 independent bits | "is this member encrypted?" |

**The choice, and why the two differ.**

*Dates stay raw.* Turning a DOS pair into a displayable date requires a
calendar and, to be correct, a timezone — DOS timestamps are local time with no
zone recorded. `ziparchive` has neither and should acquire neither: it is
linked into the kernel, where there is no user timezone, and into a GUI app,
where there is. Lane C's decoder already goes through `tzrules`, the shared
calendar the taskbar clock and `guitk::datetime` use, rather than a private
transcription. Pushing that dependency down into `ziparchive` would either drag
a calendar into the kernel or force a second one to exist. So the raw `u32` is
the whole contract, in both directions — symmetric on `ZipEntry` and
`ZipWriteEntry`.

*Flags are decoded.* The opposite argument applies with the same force. Bit 0
meaning "encrypted" is not external knowledge that `ziparchive` would have to
import — it *is* the ZIP specification, which is precisely what this crate is
the authority on. Leaving it raw would put `entry.flags & 1` in every caller,
which is four transcriptions of a magic constant in two lanes, and each one is
a chance to write `& 0x10` or to check the wrong field. `flags` is still
exposed raw as well, because there are sixteen independent bits and adding a
`pub` field per bit would break every construction site each time one is
decoded; `is_encrypted()` sits on top of it as the one bit anybody has asked
for so far.

**The alternative considered.** Lane C suggested — and explicitly left to us —
letting every caller encode its own DOS pair, on the grounds that the
duplication should be observed before a shared helper is designed for it. That
is the right instinct and we are following it for now: no encoder ships in this
change. What tips it is that the encoder, when it does appear, must not live in
`ziparchive` for the reason above, and must not live in the kernel either,
because a module of a *binary* crate cannot be depended on — the exact trap
that stranded the ZIP parser in the kernel and forced §610. Its eventual home
is `tzrules`, which is already `no_std`, dependency-free, and already a
dependency of both the kernel and `guitk`.

**The cost we are accepting.** Until that encoder exists, every SlateOS-written
archive records "no modification time" (`0`) rather than a real one, even where
the writer had an mtime in hand — `kshell`'s `zip` command calls `Vfs::lstat`
and discards `modified_ns`. That is honest rather than wrong: `0` renders as
unknown, whereas the previous behaviour minted 1980-01-01 for every member
(§618). Tracked as `A-KERNEL-ZIP-WRITERS-DISCARD-THE-MTIME-THEY-ALREADY-HAVE`.

**Why the writer does not validate the pair.** `create` stores whatever `u32`
it is given, including one that names 31 September. Refusing a malformed pair
would require decoding it, which requires the calendar this crate is refusing
to own — so validation lives on the read side, where a bad pair is actually
consumed and where `apps/archivemanager` already rejects out-of-range months
and days rather than guessing. A writer that silently repaired a caller's pair
would also hide a broken encoder from the only person who could fix it. Pinned
by `an_impossible_date_is_stored_verbatim_rather_than_corrected`.
