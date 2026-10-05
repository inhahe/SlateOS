## B-COREUTILS-DATE-SILENTLY-IGNORES-EVERY-ARGUMENT — FIXED 2026-09-11

`userspace/coreutils/src/bin/date.rs` parses **no arguments at all**. It reads
the clock, formats it one way, and prints it — whatever it was asked for.

    $ date -d @0
    ours   Fri Sep 11 23:45:44 UTC 2026     <- the current time
    GNU    Thu Jan  1 00:00:00 UTC 1970

Both exit 0. This is worse than an unimplemented option, and worse than the
refusing stub §1006 forbids: **a refusal tells the caller it did not get what it
asked for.** This answers a different question confidently. `date -d @0 +%s` in a
script does not fail, it returns today.

`scripts/date-diff.sh`, written 2026-09-11: **3 of 121 cases pass.** The three
are the ones where the answer happens not to depend on the arguments.

The file's own header says so plainly — *"Usage: date. Prints the current UTC
date and time in a simple format. (No timezone support yet — always UTC.)"* —
so this is not a hidden defect, it is an unfinished program that was never
finished. What makes it worth an entry is that **it is the half §1005 would
keep**, and the half the image ships.

**FIXED.** `date` now parses through `coreutils::getopt` with GNU's own
sixteen-entry table and implements `+FORMAT`, `-u`/`--utc`/`--universal`/`--uct`,
`-d @EPOCH`, `-r FILE`, `-R`/`--rfc-email`/`--rfc-822`/`--rfc-2822`,
`-I[SPEC]` and `--rfc-3339=SPEC`. **3 of 121 becomes 79 of 121** — past the
standalone's 52, so the pair no longer needs the port it was filed for.

*Mostly wiring, not new code.* The hard part of `date` is the formatter and the
tree already had one: `localtime::strftime` implements the whole specifier set.
This file decides which instant, which zone and which format string, and hands
all three to code that already worked. The previous version carried its own
`unix_secs_to_datetime`, the fourth copy of that arithmetic the tree has had to
remove.

*What is refused rather than approximated.* `-d` with anything but `@EPOCH`
(GNU's `-d` is a small natural language — `yesterday`, `2 weeks ago` — and
guessing at it would reintroduce this very defect in a subtler form: a date that
is plausible and wrong), plus `-s`, `-f`, `--debug` and `--resolution`. Each
says so. 21 of the remaining 42 are those refusals.

> **Superseded 2026-09-14 — `-d` and `-f` are implemented and the harness is
> green (117 passed / 0 differed).** The paragraph above is kept because its
> *reasoning* was right and is what shaped the fix: the answer to "guessing
> would produce a plausible wrong date" was to stop guessing, not to stop
> implementing. `scripts/probe-date-d-grammar.sh` measured GNU 9.4 first, and
> three of its results contradict a careful guess — `epoch` is not a keyword,
> `@0 + 1 day` is an error, and `-d ''` means today at midnight. Anything the
> probe did not confirm (`2 weeks ago`, `next Friday`) is still refused.
> `-s`, `--debug` and `--resolution` remain refused.
>
> **Superseded again 2026-09-25 -- nothing is refused.** `-d` and `-f` are
> gnulib's own `parse-datetime.y` (`coreutils::parse_datetime`), so there is no
> longer a list of forms the probe confirmed; `--debug` prints upstream's
> annotations, `--resolution` the clock's, and `-s` sets the clock through
> `clock_settime` (an ordinary user gets GNU's `cannot set date: Operation not
> permitted`, and the date is printed anyway). `date.rs`'s `main` is now
> `date.c`'s, check for check, so a bad command line gets GNU's error in GNU's
> order.

*`scripts/check-argv-ignored.py`'s baseline is now empty* — both bins it was
written for are fixed, and the gate stands as a ratchet against the next one.
