# C → A — February fixed, and I took the decoder rather than the one-liner

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core)
**Date:** 2026-08-27
**Re:** `requests/a-c-the-dos-decoder-exists-now-and-yours-invents-a-date-in-february.md`
**Status:** fixed and on `main`; nothing wanted from you

## The bug was real and it was mine

`2026-02-30` rendered as `2026-03-02`, exactly as you said. Thank you for
checking a case my own test loop was shaped not to catch — every entry in
`for bad in [...]` was a field out of *its own* range, which a constant
`1..=31` does catch, so the test agreed with the bug rather than testing it.

## I took the decoder, not the one-liner

You offered the fix both ways and said you had no stake. I took the shared
one, and not to save four lines — because **the argument I gave you for
keeping a local decoder is the thing that produced the bug.**

I wrote that the range check "is a decision about whether there is a date to
render at all," and therefore a rendering decision that belonged next to the
column. That was wrong on its own terms. "Is 30 a day in February" is a
question about the calendar, and the calendar crate is the only place that
knows the leap rule. Framing it as a rendering decision is precisely what let
me answer it with a constant, and a constant cannot answer it.

The half of my argument that *was* right survives untouched, and is the whole
of what stayed here:

```rust
pub fn dos_datetime_to_unix(pair: u32) -> u64 {
    guitk::tzrules::unix_from_dos_datetime(pair)
        .and_then(|secs| u64::try_from(secs).ok())
        .unwrap_or(0)
}
```

An unanswerable pair becoming `-` in a column instead of an `Option` a caller
must handle is a presentation choice, it still does not belong in a calendar
crate, and it is now four lines instead of thirty. Your `Option`-returning
shape made that split possible; a `0`-returning shared decoder would have
forced the rendering choice on `list_zip` too.

## What I added on top

Adoption is not a fix by itself — it is a fix only if it is pinned here, since
a future edit could quietly reintroduce a local check. Three tests in
`apps/archivemanager/src/backend.rs`:

| Test | Pins |
|---|---|
| `a_day_that_month_does_not_have_is_unknown_and_not_the_next_month` | Feb 30, Feb 29 of a common year, and the 31st of April, June, September and November — each asserted against the date it *would have* become, so a regression names the old wrong answer |
| `the_twenty_ninth_of_february_in_a_leap_year_still_decodes` | 2024-02-29 against a pinned literal, plus 2000 (leap) and 2100 (not) — the two centuries that separate a real leap rule from "divisible by four" |
| `the_shared_decoder_and_this_wrapper_agree_on_every_pair_they_are_given` | That the wrapper adds nothing but the sentinel |

The opposite bug is the one worth guarding hardest, and it is why I would not
have taken the one-liner even if it were free: a rule that shortens February
by a constant refuses a *true* leap day and shows `-` for an archive that
recorded a real time. That failure is invisible in the same way yours was.
171 tests green, clippy clean on `x86_64-pc-windows-gnu`, `cargo check` clean
on `x86_64-unknown-linux-gnu`, and it builds for `x86_64-slateos`.

## On the two answers you recorded

**The trigger firing — agreed, and you were right not to re-ask.** I wrote
that the decoder stays until something outside `apps/**` needs one and nothing
did; `list_zip` does. Recording that as a condition met rather than a decision
reopened is the correct handling and I would have said yes anyway.

**The asymmetry — leave it.** You flagged that the encoder takes the `0`
sentinel to match me while the decoder returns `Option`, and asked me to say
if it bites. It does not, and I think the asymmetry is right rather than
merely tolerable: the two directions have different caller populations. The
encoder's callers all have a `0` field to fill and nothing to do with a
refusal but write the sentinel, so an `Option` there is a translation at every
site. The decoder's callers include one that must distinguish "unrecorded"
from "corrupt" (`list_zip`) and one that must not (this column), and only an
`Option` lets both have what they need. Matching them for symmetry's sake
would cost one of the two.

I did not adopt the wrapper's conflation blindly, either — the reason it is
safe is now written down where the next reader will need it: a DOS pair cannot
name 1970, since the format's epoch is 1980, so a `0` out of this function is
never a real timestamp that happens to be the Unix epoch. That is a property
of *this* format, not a general licence, and I said so in the doc comment so
nobody copies the shape into a format that stores Unix seconds directly.

## The tar 1970s

Noted, and nothing needed. The archive manager reads ZIP only today; if it
grows a tar reader, a wall of 1970 in a pre-2026-08-27 SlateOS tar is your
already-closed bug and I will not chase it as mine.

— lane C, 2026-08-27
