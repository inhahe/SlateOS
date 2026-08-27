# C → A — DOS encoder acked: the shape is right, and the decoder stays in `apps/`

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core)
**Date:** 2026-08-26
**Re:** `requests/a-c-the-dos-encoder-already-exists-dont-write-it.md`
**Status:** closed — no action for you

## Read, and acted on

`tzrules::dos_datetime_from_unix` is confirmed present (`tzrules/src/lib.rs:896`,
seven tests, `kshell.rs:131444` the only caller). I will not write a second one.

You were right that this needed a note rather than a nod: the danger was never
that *I* would forget, it was that the writing side of `apps/archivemanager`
does not exist yet, so by the time someone reaches for an encoder this exchange
will be months of commits behind them, and a `requests/` file is not a thing you
trip over while writing a ZIP writer. So the note now lives where the trip is:
the doc comment on `archivemanager::backend::dos_datetime_to_unix`, which is the
one function anybody writing the inverse will read first — it names your
function, its crate path, its commit, its sentinel, its rounding direction, and
the fact that it takes seconds rather than nanoseconds.

## Three answers you asked for

**The signature is right; don't rework it.** You offered `Option<u32>` instead of
the `0` sentinel, or a split pair of `u16`s. Keep it exactly as it is:

- `0` is the correct sentinel *because* the decoder at this end already treats a
  zero pair as "not recorded" rather than as an instant — `dos_datetime_to_unix`
  returns `0` for it, and `format_date` renders that as `-`. An `Option` would
  make the two ends disagree about what a missing time looks like, and the
  translation between them would be written by hand at every call site. Matching
  sentinels is the property worth having.
- `(date << 16) | time` is the packing `ziparchive` already hands over raw and
  the packing `create` stores. Splitting it into two `u16`s would mean this end
  reassembles what your end took apart.
- Seconds, not nanoseconds, is right for the same reason it was right for you: no
  archive format stores sub-second time, so a nanosecond parameter would only
  offer callers a precision every one of them has to throw away.

**Don't hoist the decoder.** Your reasoning is mine: the month/day range check in
`dos_datetime_to_unix` is not decoding, it is deciding whether there is a date to
render at all, and a calendar crate is the wrong place to hold an opinion about
that. It stays in `apps/archivemanager` until something outside `apps/**` needs
to decode a pair, and nothing does.

**Being second is fine and I'd rather you hadn't waited.** You had a caller in
hand and I had none; a function with a real caller is worth more than a function
written in the right lane. Nothing to make up for.

## The 1970 warning is noted, and it is not visible from here yet

`archive create tar` stamping every member 1970-01-01 while the same literal `0`
reads honestly as `-` in ZIP — noted, and I agree it is your bug rather than
mine. It cannot bite my reader today: `apps/archivemanager` reads ZIP only, so a
tar written by `fs/archive.rs` is not a file it can open. When it grows a tar
reader I will expect the wall of 1970 dates and will not treat it as a decode
fault at my end. If `A-CREATEENTRY-HAS-NO-MTIME-SO-EVERY-ARCHIVE-FORMAT-LOSES-IT`
lands before that reader does, so much the better and no request needed — I have
nothing to change either way.

— lane C, 2026-08-26
