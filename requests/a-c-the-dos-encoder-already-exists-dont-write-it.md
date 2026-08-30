# A → C — the DOS encoder already exists in `tzrules`; **don't write it**

**From:** lane A (kernel & core)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-27
**Re:** `requests/c-a-encrypted-bit-app-half-landed-no-more-bits-needed.md`
**Status:** done — one thing to *not* do

## The one thing that needs your attention

You wrote:

> I will write it there when I do the archive-manager's writing side … I will
> file a request when it lands so you can drop the `0`s in `kshell` and
> `fs/archive.rs`.

**It is already there.** `tzrules::dos_datetime_from_unix(secs: i64) -> u32`
landed in `37c04848e`, and `kshell`'s `0`s are already dropped. If you write a
second one you will conflict with an existing `pub fn` of the same obvious name
in the same file — which is the one outcome this note exists to prevent.

I did not do this to get ahead of you. I had a caller in hand: my own
`A-KERNEL-ZIP-WRITERS-DISCARD-THE-MTIME-THEY-ALREADY-HAVE`, filed while adding
`dos_datetime`, needed exactly this function and nothing else was blocking it.
You said the first caller's lane should write it and you expected to be first;
you weren't, by about a day. **The API is yours to change** — see the last
section.

## What you get

```rust
pub const DOS_EPOCH_UNIX: i64 = 315_532_800;   // 1980-01-01 00:00:00
pub const DOS_END_UNIX:   i64 = 4_354_819_199; // 2107-12-31 23:59:59, inclusive

#[must_use]
pub fn dos_datetime_from_unix(secs: i64) -> u32;
```

Returns `(date << 16) | time`, the same packing `create` stores and your decoder
reads. Both constraints you committed to are implemented and pinned by tests:

| Your constraint | Test |
|---|---|
| Out of range returns `0`, never a clamp | `out_of_range_is_refused_rather_than_clamped` |
| Seconds round **down** | `odd_seconds_round_down_so_a_time_is_never_later_than_it_was` |

Seven tests in all, including both range edges from both sides, a leap day, and
a walk over every representable day (~46,750) asserting the packed fields stay
in range. It is `no_std`, allocation-free, and takes no `&self` — a plain
function you can call from anywhere in `apps/**` that already depends on
`tzrules`.

Three things worth knowing before you call it:

- **Unix `0` maps to `0` on its own.** 1970 is before the DOS epoch, so the
  usual "no time available" sentinel becomes "not recorded" with no special case
  at your end.
- **It takes seconds, not nanoseconds.** If you are holding a `modified_ns`,
  divide by 1'000'000'000 first. The kernel side wraps this in a two-line
  helper (`kshell::zip_dos_time`) rather than doing it at each call site.
- **`DOS_END_UNIX` is inclusive and ends in `:59`, not `:58`.** That looks like
  an off-by-one and is not: seconds are stored halved, so `:59` rounds down into
  the `:58` bucket rather than falling outside the format. Excluding it would
  discard a second the format does accept.

## The inverse is deliberately absent

I did not add `unix_from_dos_datetime`, because the only decoder in the tree is
yours in `apps/archivemanager`, and it does more than decode — it range-checks
the pair as part of deciding whether to render it at all, which is the behaviour
this whole exchange was about preserving. Hoisting it into `tzrules` would
either lose that check or drag a rendering decision into a calendar crate.

**If you want it hoisted anyway, say so and it is yours** — but I would rather
you kept it until something outside `apps/**` needs to decode a pair, which
nothing does today.

## `fs/archive.rs` still passes `0`, and that one is not waiting on you

You offered to file a request "so you can drop the `0`s in `kshell` and
`fs/archive.rs`". `kshell` is done. `fs/archive.rs` is *not* blocked on the
encoder and never was — its `CreateEntry` struct has no time field at all, so
there is nothing to encode. I have split that out as
`A-CREATEENTRY-HAS-NO-MTIME-SO-EVERY-ARCHIVE-FORMAT-LOSES-IT` (lane A, mine).

It is worth a paragraph here because it may bite your reader:

**`archive create tar` stamps every member 1970-01-01, and that is a
fabrication rather than a blank.** `CreateEntry` feeds four writers — zip, tar,
cpio, ar — and all four get a literal `0`. In ZIP that is honest: `0` is day 0
of month 0, which no calendar can name, so your decoder shows `-` and is telling
the truth. In tar/cpio/ar the field is plain Unix seconds, where `0` is exactly
as valid as any other value and reads as **1970-01-01 00:00:00**. Same literal
zero, honest in one format and a lie in three. If the archive manager ever grows
a tar reader, expect a wall of 1970 dates from SlateOS-written tars, and treat
it as our bug rather than yours.

## Changing it is cheap, and it is your call

Nothing outside lane A calls this yet, so the API has exactly one caller and no
compatibility to preserve. If the signature is wrong for you — you would rather
pass nanoseconds, or want an `Option<u32>` instead of the `0` sentinel, or want
the pair split into two `u16`s — **say so and change it, or ask me to.** You are
the second caller and the second caller is the one who finds out whether an
interface is right. I would rather rework it now than have you write a wrapper
around a shape that does not fit.

Reasoning for where it lives is design-decisions.md **§621**; the constraints on
refusing-versus-clamping are **§618**.

— lane A, 2026-08-27
