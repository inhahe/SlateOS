## A-THE-WIRING-GATE-ASKS-A-QUESTION-IT-COULD-ANSWER-ITSELF (lane A) — FIXED 2026-08-31

**Status:** FIXED 2026-08-31, same day as found. The note is now data: each
gated call site declares the serial line that proves it ran, the checker
verifies the declaration is printable, every boot records which of them
appeared, and a gate fails the build on one that has never appeared. See
"What was done" at the end of this entry.

`scripts/check-self-tests-wired.py` ends every run with a NOTE:

> 6 self-test call site(s) in main.rs sit inside a conditional, so whether they
> run depends on the boot path. Being reachable from main.rs is not the same as
> being reached: a suite behind a condition that is false in CI has never run,
> however green the boot test looks. **Check each against the serial log before
> citing it as coverage.**

The reasoning is exactly right, and it is the reasoning that catches the bug
class that put `[mm] Zeroed frame allocation` in this file — a call site that
exists, type-checks, and is reachable, but whose guard is false on every boot.
The defect is that the gate **stops one step short**: it asks the reader to do
the correlation by hand, on every boot, forever, and never does it itself.

**The manual check is error-prone, and I have the failure to prove it.** Doing
this by hand on 2026-08-31 I first concluded that two of the six —
`self_test_userspace_netstack` and `self_test_netstack_dns_ipc` — had never run,
because grepping the serial log for their names near the word "self-test"
returned nothing. That was a **false negative**: both run, and print
`[spawn] Running userspace netstack daemon (ring 3) integration test...` and
`[spawn]   netstack DNS-over-IPC ...: OK` (serial lines 2786 and 2816). They say
*integration test*, not *self-test*. The hand check fails on a phrasing mismatch
between the function's name and the words it prints — which is precisely the
kind of thing a script does not get wrong and a reader does.

**Verified result, 2026-08-31 — all six do run.** Recorded so the next reader
does not repeat the search:

| Call site | Guard | Runs in CI? | Evidence in serial |
|---|---|---|---|
| `main.rs:1138` / `:1150` `acpi::self_test` | `if let Some(rsdp) = …` / `else` | yes — **cannot not** run | `[acpi] Self-test PASSED` (1727) |
| `main.rs:1400` `mm::swap::self_test_disk` | `init_disk(...).is_ok()` | yes | `[swap] Disk backend self-test PASSED` (2006) |
| `main.rs:1512` `fs::fat::self_test` | `if fat_ok` | yes | `[fat] Running mkfs/format self-test...` (2564) |
| `main.rs:2043` `self_test_userspace_netstack` | `!net.userspace` | yes | `…integration test...` (2786) |
| `main.rs:2054` `self_test_netstack_dns_ipc` | `!net.userspace` | yes | `…: OK — 104.20.23.154` (2816) |

Two of those deserve a note. The `acpi` pair is reported as gated but is an
`if`/`else` over the same test, so one arm always runs — the gate's nesting
model does not know that two branches of one conditional are exhaustive, and
counts both. And the netstack pair's guard is `!userspace_enabled()`, which
reads as fragile but is presently safe because the `net.userspace` cutover
switch defaults **off** (`[spawn]   net.userspace cutover switch: off`, 2842).
That is a default, not an invariant: **the day the cutover flips on, those two
self-tests silently stop running** and nothing says so. The code comment at
`main.rs:2036` documents the skip as deliberate (they would contend for the
exclusive raw-NIC claim, §64) — which makes it a correct decision with no
tripwire, not an accident.

**What was done.** Three commits, one per stage of the pipeline.

1. **The declaration and its enforcement** (`395b7fc6a`). Each of the six gated
   sites in `main.rs` carries a `// RAN-IF: "<literal>"` comment naming the
   serial line that proves it ran. `check-self-tests-wired.py` requires one at
   every gated site and — the part that matters — verifies the literal actually
   occurs in the file that *defines* that suite. A marker that matches nothing
   is worse than no marker: a missing one fails loudly here and now, whereas a
   typo'd one passes this gate and then reports its suite as never-run on every
   boot forever, which is an accusation against working code and therefore the
   failure most likely to be believed and acted on. `--emit-markers` writes the
   set as JSON. 34 tests in `scripts/test-check-self-tests-wired.py`.

   Keyed by *literal*, not by site, so the `acpi` `if`/`else` pair maps to one
   marker: seeing the line proves an arm ran without saying which, which is
   exactly as much as the log can prove. Keying by site would report the losing
   arm as never-seen on code that is correct by construction — the false alarm
   this entry's own table warns about.

2. **The recording** (`21fdf4527`). `boot-history.py` gains `--gated-markers`
   and writes `gated_ran: {literal: bool}` per boot; `boot-test.sh` passes the
   file only when *this run* regenerated it. Two details are load-bearing: the
   field is **omitted**, never emptied, when the markers cannot be read (`{}`
   is an all-clear and must not be forgeable by a plumbing failure), and the
   match is a plain substring test rather than a regex, because every real
   marker contains `[` and four contain `(`/`)` — read as a pattern they would
   match nothing and report all five suites as never-run. 100 tests in
   `scripts/test-boot-history.py`.

3. **The gate** (`9ea7293c6`). `scripts/check-gated-selftests.py` fails the
   build on a marker absent from 100% of the boots that recorded it, N ≥ 10,
   mirroring `check-boot-skips.py`'s evidence discipline and allowlist rules.
   27 tests.

**Deliberate deviation from the fix proposed above:** this is a pre-build gate
over `bench/boot-history.jsonl`, not the post-boot single-log check the OPEN
text suggested. A gated site legitimately not running on *one* boot is not a
defect — that is what "gated" means — so a post-boot check would either fail on
correct code or assert nothing. Only *never* running is the defect, and "never"
is a question about history, which is where `check-boot-skips.py` already lives.

Two things the gate refuses to do, each a false accusation avoided: it measures
each marker only against the boots that recorded *it* (the window as denominator
would make a marker declared today read as never-run out of 25 — failing loudest
at the moment someone does the right thing), and it excludes markers the newest
boot no longer declares (an accusation whose only remedy is allowlisting a line
that is not in the tree).

**The netstack pair is now watched rather than merely noted.** Its guard is
`!net.userspace`, a flag that defaults off and that someone will eventually flip
on purpose; the day it flips, those two suites stop running and `gated_ran`
starts recording `false` for them. Ten boots later the build fails and says so.
That is the tripwire the original entry observed was missing.
