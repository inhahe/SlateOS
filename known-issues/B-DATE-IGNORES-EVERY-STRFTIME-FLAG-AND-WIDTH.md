## B-DATE-IGNORES-EVERY-STRFTIME-FLAG-AND-WIDTH (lane B, 2026-09-14)

**Status:** flags and widths **FIXED 2026-09-14**; the `-d` grammar remains open

`localtime::strftime` now reads flags and a width before the conversion, so
all seven flag/width cases pass: `scripts/date-diff.sh` went **80 passed / 41
differed to 87 / 34**. The remaining 34 are the second cluster below — `-d`
accepting anything but `@SECONDS` — which is GNU's whole date grammar and a
separate job.

**One rule needed a third measurement.** `%#a` was implemented as a
per-character case swap, the obvious reading of "opposite case", which gives
`sUN`. Measuring six fields showed `#` flips the FIELD, with the direction
taken from its text — `%#a` `SUN`, `%#p` `am`, `%#B` `SEPTEMBER`, `%#Z` `utc`
— and that `%P` is exempt, because `%P` is already the flipped spelling of
`%p`. Six tests pin the measured rules, including `%1d` and last-flag-wins.

`date +%-d` prints the literal text `%-d`. So does `%_d`, `%0e`, `%^a`, `%#a`,
`%5S` and every other flagged or width-qualified conversion:
`localtime::strftime` reads exactly one byte after the `%`, so a flag is an
unrecognised specifier and comes out verbatim.

Found by running `date` over `scripts/date-diff.sh`'s own 77 cases against GNU
directly: **45 differ**, in two clusters.

| cluster | cases | size |
|---|---|---|
| flags and widths (`%-d`, `%_d`, `%0e`, `%^a`, `%#a`, `%5S`, `%-5S`) | ~7 | bounded, specified below |
| `-d` accepting anything but `@SECONDS` (`-d '2021-03-04 05:06:07'`) | ~30 | GNU's whole date grammar; a separate job |

### The measured specification

Every line below was run, not recalled. `date -d @1000000000 +'[%X]'`, TZ=UTC:

| format | output | what it shows |
|---|---|---|
| `%d` `%-d` `%_d` `%0d` | `09` `9` `⎵9` `09` | `-` no pad, `_` space pad, `0` zero pad |
| `%e` `%0e` `%_e` | `⎵9` `09` `⎵9` | `0` overrides a space-padded field |
| `%a` `%^a` `%#a` | `Sun` `SUN` `SUN` | `^` upper, `#` swap case |
| `%S` `%5S` `%-5S` `%_5S` `%05S` | `40` `00040` `40` `⎵⎵⎵40` `00040` | width; `-` discards the width too |
| `%5e` `%5a` `%10B` `%5Z` | `⎵⎵⎵⎵9` `⎵⎵Sun` `⎵September` | **string fields pad with SPACE** |
| `%0a` `%_a` | `Sun` `Sun` | a pad flag on a string field does nothing |
| `%3H` `%3j` `%12N` | `001` `252` `000000000000` | width applies to any numeric field |

**Two rules that a reasonable implementation would get wrong:**

1. **`%1d` is `9`, not `09`.** The width *replaces* the field's default width;
   it is not a minimum applied to the default rendering. An implementation that
   renders `%d` as `09` and then pads to the requested width returns `09` here
   and is wrong. The value must be formatted *with* the requested width.
2. **The last flag wins.** `%-0d` is `09` and `%0-d` is `9`.

### Why this was measured twice

The first sweep captured both sides with `.strip()`, which showed `%_d` as `9`.
Implementing from that reading would have produced no space padding at all and
looked right against the stripped comparison. Re-measuring with `[` `]`
delimiters is what turned it into ` 9`. **A comparison that normalises
whitespace cannot be used to specify something whose whole content is
whitespace.**

### What the fix needs

`strftime` currently formats each field inline, so there is no single place a
width or pad can be applied. The numeric arms need to go through one helper
taking `(value, default_width, default_pad)` with the flags and width
overriding both — which is a real change to a ~40-arm function shared by
`date`, `ls` and `diff`'s header, and wants doing with attention rather than at
the end of a tick.
