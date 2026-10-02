## B-COREUTILS-UPTIME-SILENTLY-IGNORES-EVERY-ARGUMENT — FIXED 2026-09-11, default line completed 2026-09-12

The same defect as `date` below, in the other bin that has it:

    $ uptime -s
    ours   up 00:26                     <- the uptime DURATION
    GNU    2026-09-11 19:24:28          <- the boot TIME

    $ uptime -p
    ours   up 00:26
    GNU    up 26 minutes

    $ uptime --nosuchoption ; echo $?
    ours   up 00:26
           0

`userspace/coreutils/src/bin/uptime.rs` is 231 lines that read `/proc/uptime`
and print it. Its header says so — *"Usage: uptime"* — and it parses nothing.
The bare form differs too: GNU prints the time of day, the user count and three
load averages, and ours prints none of them.

`-s` is the one that matters most, because the two answers are not even the same
*kind* of thing: a script asking when the machine booted gets how long it has
been up, in a different format, with exit 0.

**FIXED.** `uptime` now parses its command line through `coreutils::getopt`
with procps-ng 4.0.4's own four-option table, and `-p`/`--pretty` and
`-s`/`--since` are implemented exactly — verified interleaved against procps,
where both print the same string including long-option abbreviations (`--pret`,
`--si`) and every refusal matches at exit 1. `scripts/check-argv-ignored.py`
reports `fixed: uptime now reads argv` and its baseline is down to one entry.

*A note on the reference, since the standalone advertised more.* It carried
`-r`, `--json` and `--raw`; **procps has none of those** — its table is exactly
`--pretty`, `--help`, `--since`, `--version`. Read from `uptime --help` rather
than inherited from the crate being replaced. Implementing the standalone's
options would have been implementing an invention.

**THE OTHER HALF — DONE 2026-09-12, and both reasons given for deferring it
were wrong.** The default line now prints all four fields.

*Wrong reason 1: "the user count comes from `utmp`, and this tree has no utmp
reader: there is no `who` bin and no shared module."* The shared module clause
is simply false — `utmpfile` is a workspace crate with **no dependencies**,
exposing `parse(&[u8])` and `count_user_sessions(&[u8])`, and `userspace/who`,
`userspace/last` and `userspace/finger` were all already using it. The `who`
clause is true only under a reading it did not state: there is no *coreutils
bin* named `who`, but `userspace/who` exists. So the field was omitted for a
blocker that could have been disproved by one `grep`, and the paragraph was
persuasive because its *conclusion* — do not invent a number — was right.
**A false premise defending a correct conclusion is the hardest kind to
notice**, and it survived a month.

*Wrong reason 2: "no harness was written for this pair, deliberately …
`uptime`'s output IS the current moment."* That was true of the technique
available when it was written and stopped being true when `scripts/df-diff.sh`
and then `scripts/free-diff.sh` established per-case `unshare -mUr` with the
inputs bind-mounted. `/proc/uptime` and `/proc/loadavg` pin exactly like
`/proc/meminfo`. Only the time-of-day field genuinely moves.

Even the user count pins, which took two experiments to establish. On this
host `uptime` links `libsystemd` and asks logind, so emptying `/run/utmp`
changes nothing while `who` drops to zero — from which a first pass concluded
the field was uncomparable. Masking `/run/systemd` as well makes procps fall
back to `utmp`, and the count becomes a fixture. **The first experiment
answered a narrower question than the one being asked** and its answer looked
like a general one.

That mattered, because the measurement it enabled contradicted what a careful
implementation would have written. procps prints `,  0 user` — **singular at
zero** — so `if n == 1 { "" } else { "s" }` is wrong for precisely the value a
machine with nobody logged in reports. The count is `%2d`, which is
indistinguishable from a two-space literal plus `%d` at every single-digit
count and diverges at ten.

The `up …` field was wrong too, in three ways, and had unit tests asserting
each: `up 00:59` where procps prints `up 59 min`, `up 01:00` where it prints
`up  1:00` (space-padded), and `up 1 day, 00:00` where it prints
`up 1 day, 0 min`. **Six tests encoded the unmeasured format**, which is why it
survived; they now carry the measurements.

### Two findings from `scripts/uptime-diff.sh`, one upstream and one ours

**procps-ng 4.0.4's `uptime -p` is wrong at every unit boundary, and we do not
reproduce it.** Each level of its decomposition rolls over only when the
remainder *exceeds* the unit, never when it equals it, so an exact boundary
falls through to the unit below — and the last level has nothing below it:

| `/proc/uptime` | procps-ng 4.0.4 | ours |
|---|---|---|
| 60 | `up ` — **empty** | `up 1 minute` |
| 3600 | `up 60 minutes` | `up 1 hour` |
| 3660 | `up 1 hour` — loses the minute | `up 1 hour, 1 minute` |
| 86400 | `up 24 hours, 0 minutes` | `up 1 day` |
| 90000 | `up 1 day, 60 minutes` | `up 1 day, 1 hour` |

At exactly one minute of uptime it prints `up ` and stops. §371 makes
bug-for-bug reproduction the default, but that default assumes the reference's
answer is *an* answer; `up ` is not a wrong rendering of one minute, it is
none. Declared in the harness as nine `xfail_uptime` cases with the sweep as
the reason. **The earlier claim that `-p` was "verified interleaved against
procps" is not withdrawn but is narrower than it reads: it compared the live
uptime, which is one point, and generalised.**

**`-s` is an early exit, not a flag — and our last-one-wins parse was wrong.**
`uptime -sp` printed the pretty form here and the boot time in procps. The
first repair inferred "flags resolved after the loop, `since` tested first",
which fits `-ps` and `-sp` both printing the boot time and is still false. The
discriminator is what `-s` does to arguments *after* it:

    uptime -sV      -> boot time         the -V never runs
    uptime -sXYZ    -> boot time, rc=0   no "invalid option -- 'X'"
    uptime -s junk  -> boot time, rc=0   an operand, accepted
    uptime -Vs      -> the version       whichever fires first wins

So procps prints and leaves before the rest of argv is looked at. Two models
fitted the first measurement and the cheap way to separate them was to feed
the option something it should have rejected. **Found only because `-ps`
XPASSed** — it was in the harness as a declared refusal, which it is not, and
asking why put `-sp` in the harness. That is the third false xfail reason in
two days, after `free --help`; a declared divergence is an assertion like any
other, and the only kind never tested by its case passing.
