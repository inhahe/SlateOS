## TD-B-LOCALTIME-STRFTIME-DOES-NOT-PAD-YEARS (lane B, 2026-09-25) — FIXED 2026-09-26

**In short:** `date -d 0021-06-15 +%F` prints `21-06-15` where GNU prints
`0021-06-15`, and `date -d 10000-01-01 +%F` prints `10000-01-01` where GNU
prints `+10000-01-01`. Every year from 1000 to 9999 is unaffected, which is
why nothing noticed until `date -d` could reach year 21 at all.

**Where:** `userspace/localtime/src/lib.rs`, `strftime`: `%Y` and `%G` are
`push_int` (the bare number, no width, no padding), and `%F` is
`strftime("%Y-%m-%d")`. gnulib's `nstrftime` -- which coreutils uses instead of
the C library's -- formats every year through `DO_YEARISH`: at least four
digits, zero-padded, a sign when negative, and under the `+` flag a `+` for a
year that needs more digits than the width. `%F` is `%+4Y-%m-%d`, run as a
sub-format that inherits the caller's flags and width (`%_12F`), and `%C`,
`%y` and `%g` are `DO_YEARISH (2, …)` with their own sign rules.

**The proper fix** is to port `DO_YEARISH` and the sign-and-padding step it
shares with `DO_NUMBER` (`do_number_sign_and_padding`), and to run `%F` (and
`%D`, `%T`, `%R`, `%r`, `%c`, `%x`, `%X`) as nstrftime's sub-formats rather than
as a recursive call that forgets the flags. `date-diff.sh`'s
`TD-B-LOCALTIME-STRFTIME-DOES-NOT-PAD-YEARS` rows turn green when it is right.

**Severity: low.** Years before 1000 and after 9999 only -- but silently wrong
where it applies, and ISO 8601 (`%F`) is the format a script is most likely to
parse back.

**How it was closed.** Not by patching `%Y`: the year was the visible end of a
formatter that was neither upstream's. The GNU programs this tree reimplements
use *two* -- gnulib's `nstrftime` (coreutils, diffutils) and the C library's
`strftime` (findutils, procps, tar, `pinky`, bash) -- which differ on years,
`%N`, `%q`, `%:z`, the `+` flag and what `-` does to a width. `localtime`'s
`strftime` module now ports both from source, glibc 2.39's `strftime_l.c` and
coreutils 9.4's `nstrftime.c`, and each caller uses its upstream's. The
`date-diff.sh` rows are `run_case`s again, and `scripts/strftime-diff.sh`
checks every conversion under every flag, width and modifier against both.
