## TD-B-TOUCH-REFUSES-DASH-T-AND-DASH-D (lane B, 2026-09-25) — FIXED 2026-09-25 (`-t` and `-d`)

**In short:** `touch -d '2020-01-01 12:00' f` -- one of the two ways to give a
file a chosen time rather than now -- answers `option -d is not implemented by
this touch` and exits 1. Build scripts and test fixtures use it. The other way,
`touch -t 202001011200 f`, works since 2026-09-25.

**Where:** `userspace/coreutils/src/bin/touch.rs`, `parse_args`: `Opt::Short(flag
@ b'd', _) => return Err(unimplemented_short(flag))`.

**How `-t` was closed.** gnulib's `lib/posixtm.c` is `coreutils::posixtm`:
`[[CC]YY]MMDDhhmm[.ss]` under upstream's syntax bits, read as a local time
through `localtime::Zone::epoch` and refused when that normalises it to
something else (September 31st, 25:00, a spring-forward gap), with a sixtieth
second taken as the next one. `touch -t` is `CENTURY | SECONDS`. The obsolete
`touch MMDDhhmm[YY] FILE…` operand came with it -- `TRAILING_YEAR | PRE_2000`,
read only while `_POSIX2_VERSION` is below 200112, warned about unless
`POSIXLY_CORRECT` is set -- replacing the module docs' reasoning for leaving it
out, which was that a date-shaped operand would be a date only sometimes: that
is upstream's behaviour, and the edition decides it. `-t` with `-r` is `cannot
specify times from more than one source`. Pinned by `scripts/touch-diff.sh`
section 11 (139 passed, 0 differed): lengths, two-digit years either side of
69, the leap second, invalid stamps, the order of errors, both halves, and the
obsolete operand under three editions.

**The fix for `-d`** is `lib/parse-datetime.y` (2438 lines) as
`coreutils::parse_datetime`. `date -d` and `find -newerXt` each carry a
measured subset of the same language today (`date.rs`'s module docs list what
it covers); one transcription of the grammar would replace both, with
`date-diff.sh` and `find-diff.sh` checking that nothing they pass today is
lost. `touch -r FILE -d REL` then needs the reference time's nanoseconds kept
through the relative items, as upstream's `date_relative` keeps them, and `-d
now` needs upstream's special case that turns it back into `UTIME_NOW`.

**How `-d` was closed.** Exactly as above: `coreutils::parse_datetime` is
gnulib's `parse-datetime.y` -- the Bison tables coreutils 9.4 ships, copied by
`gen_tables.py`, with `yacc.c`'s driver, the actions, the lexer and
`parse_datetime_body`, over a port of glibc's `mktime` in `localtime`
(`design-decisions.md` §1031). `touch` reads `-d` after the options, as upstream
does: relative to each of `-r`'s times when `-r` is given, else to the clock,
nanoseconds kept, and `-d now` turned back into the kernel's *now* when both
halves are set -- checked by a second parse against a clock one second away, as
upstream checks it. `-d` with `-t` is `cannot specify times from more than one
source`. `date -d` and `find -newerXt` dropped their subsets for the same
module. Pinned by `scripts/parse-datetime-diff.sh` (the corpus through `touch
-r REF -d`, `date -d`, `date --debug` and `date -f`, in five zones) and the new
`-d` rows of `touch-diff.sh`.
