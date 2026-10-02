## TD-B-DATE-IS-THE-SECOND-PAIR-THE-STANDALONE-WINS (lane B, 2026-09-11)

`scripts/date-diff.sh`, 121 cases against a GNU coreutils 9.4 built from source:

| half | passed | differed |
|---|---|---|
| `coreutils` | **3** | 118 |
| standalone | **52** | 69 |

Seventeen times as many passes. After `diff` (43 against 21) this is the second
pair where the standalone is clearly the better half, and by a much wider
margin. §1005 keeps `coreutils` as the one home, so the resolution is to **port
the standalone into `coreutils/src/bin/date.rs` and then delete the crate** —
not to delete the crate now.

**What the standalone still gets wrong, which is the work list for that port:**

| | cases | what |
|---|---|---|
| format specifiers | 30 | `%C`, `%g`, `%G`, `%V`, `%U`, `%W` (century and the ISO week-year family) and `%:z`/`%::z` (the extended zone forms) all differ while both sides exit 0. |
| options absent | 20 | `--date=` (the `--opt=value` form — `-d X` works and `--date=X` does not), `--uct`, `--rfc-822`, `--rfc-2822`, `--rfc-3339`, `--reference=`, `-f`/`--file`. |
| the `-d` language | 11 | `05:06:07`, `Mar 4 2021`, `4 March 2021`, a trailing `UTC`, a trailing `+0200`, `now`, `today` — all refused, all accepted by GNU. |
| message shape | 7 | |
| accepts what GNU refuses | 1 | `-d @0 -r stamped.txt`; GNU rejects the combination. |

The 20 missing options are mostly the same `--opt=value`/abbreviation family
that `uname` and `env` were fixed for by routing through `coreutils::getopt`, so
a port should start there rather than end there.

**A note on the harness: it has no "now" cases, deliberately.** `date` with no
arguments prints the current time, and the two sides run milliseconds apart;
when the second ticks between them the harness reports a difference that is a
property of the clock. Every case is a function of its arguments — `-d @<epoch>`,
`-d <fixed string>`, `-r <file with a stamped mtime>` — with `TZ` pinned to UTC.
That covers the formatter completely, since `date -d @0 +%F` exercises the same
code as `date +%F`. What it does not cover is reading the clock, which is one
line.
