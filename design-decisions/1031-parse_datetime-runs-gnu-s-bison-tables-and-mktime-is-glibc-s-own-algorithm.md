## 1031. `parse_datetime` runs GNU's Bison tables, and `mktime` is glibc's own algorithm

**Date:** 2026-09-25
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `date -d`, `touch -d` and `find -newermt` all read GNU's date
language (`next Tuesday`, `3 days ago`, `2021-06-15 12:00 -0500`), which has
no specification except the program that parses it: gnulib's
`parse-datetime.y`, a Bison grammar with 31 deliberate ambiguities that the
generated tables settle. `date` had a hand-written subset that was measured
form by form and still could not say `12:30 -5` (half past twelve at UTC-5).
The port copies the LALR tables out of the `parse-datetime.c` that coreutils
9.4 ships instead of re-deriving them, and ports glibc's `mktime` beside it,
because which strings are refused -- and what a skipped or repeated
daylight-saving hour means -- is decided there.

### How the grammar is carried

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **GNU's generated tables, copied by a script (chosen)** | `tables.rs` is `gen_tables.py`'s transcription of the release tarball's `lib/parse-datetime.c`; `grammar.rs` is `yacc.c`'s driver and the 92 rule actions | the parser *is* GNU's: the same shift/reduce resolutions, the same default reductions, so the same `--debug` output in the same order and the same place to stop on an error; the script refuses a table it does not recognise, and records the source's sha256 | the tables are opaque numbers; a reader checks them by regenerating, not by reading |
| A hand-written recursive-descent parser | readable rules in Rust | no generated data in the tree | an LALR parser never backtracks and reduces before reading in some states; a greedy hand parser accepts more, rejects differently, and prints `--debug` lines in a different order -- the first test written by hand got that order wrong, and the tables got it right |
| Our own `userspace/yacc` generating the tables | a checked-in `.y`, built at compile time | the grammar stays source | our yacc is a separate, unverified implementation; any difference in its conflict resolution is a different parser |

### `mktime`

`localtime::Zone::epoch` already inverted a local time, but by its own rules:
it resolved the skipped hour to one side and the repeated hour to one of the
two, and it could not honour an explicit `tm_isdst`. `parse_datetime` needs
glibc's answers for all three -- it refuses a date by comparing the fields it
asked for with the fields `mktime` normalised them to, and it hands `mktime`
`tm_isdst = 0` for `EST` in July -- so `localtime/src/mktime.rs` is a port of
gnulib's `mktime.c`, which is the file glibc builds its own from. That
includes the **process-wide offset guess**: glibc starts each search from the
offset the previous call found, which decides which of a repeated hour's two
instants comes back, so `date -f` answers line 7 as GNU does only if lines 1-6
moved the guess as GNU's did. `Zone::mktime` keeps the same static;
`mktime_internal` takes the guess as a parameter for callers (and tests) that
must not share it.

`Zone::epoch` stayed, for callers that wanted an instant that always
exists, while `posixtm` (and so `touch -t`) and `cal` still called it.
**Update 2026-09-26:** both moved to `mktime` -- called when and as upstream
calls it, with `tm_isdst` -1 -- and with no callers left `Zone::epoch` was
removed, a second inverse with its own answers for the skipped and repeated
hours being an invitation to pick the one GNU's never give.

### Where C's integers show through

Upstream assigns `intmax_t` values into `int` fields in three places
(`tm.tm_min = pc.minutes`); gcc keeps the low 32 bits, so `10:4294967326` is
10:30 to GNU. The port does the same (`as i32`), and a test pins it. Every
`ckd_*` upstream is a `checked_*` here, failing the same way.

**Revisit when** coreutils is upgraded past 9.4: regenerate `tables.rs` from
the new release's `lib/parse-datetime.c` with `gen_tables.py`, and diff the
actions in `grammar.rs` against the new grammar's.

**Where:** `userspace/coreutils/src/parse_datetime/` (`gen_tables.py`,
`tables.rs`, `grammar.rs`, `lex.rs`, `mod.rs`); `userspace/localtime/src/mktime.rs`;
used by `date.rs`, `touch.rs`, `find.rs`. Checked by
`scripts/parse-datetime-diff.sh`.
