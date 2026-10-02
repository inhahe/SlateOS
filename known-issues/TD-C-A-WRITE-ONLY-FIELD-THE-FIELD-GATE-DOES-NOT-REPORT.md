## `TD-C-A-WRITE-ONLY-FIELD-THE-FIELD-GATE-DOES-NOT-REPORT` (lane C, 2026-09-17)

**In short:** `apps/reminders`'s `last_file_action` was written on every open
and save and read by nothing at all, and neither the compiler nor
`scripts/check-fields-written-never-read.py` said a word. The field is fixed --
it now reaches the screen, which is what it was for -- but **the gate's silence
is not explained**, and a gate that misses one of these may be missing others.

**What is verified.**

| | |
|---|---|
| the field | `apps/reminders/src/main.rs`, declared, initialised, assigned once, never read -- three occurrences in the only file the crate has |
| the compiler | silent. Still silent with the field made private: the crate genuinely recompiled and warned about nothing |
| the gate | passes. `--list` shows 46 findings, all baselined, and this is not among them |

**What is ruled out.** Each of these was a hypothesis I checked and disproved,
which is worth recording so nobody spends the time twice:

* *not the corpus* -- the gate scans `apps/`; `terminal`, `sysmonitor`,
  `diskimager`, `diskcleanup` and `colorpicker` all appear in its findings.
* *not "it only reports test-read fields"* -- its `--list` includes entries
  worded "written in production and read by nothing at all", so that category
  is covered; `net80211`'s `mic_algo` is one.
* *not a second source file* -- `apps/reminders/src/` contains only `main.rs`.
* *not the read/write discrimination* -- the gate classifies correctly:
  `written = after.startswith("=") and not after.startswith(("==", "=>"))`,
  so `self.last_file_action = Some(...)` is a write, not a read.
* *not `#![allow(dead_code)]`* -- the crate's allows are all `clippy::`.

**Why this is filed rather than fixed.** I do not know which of the gate's
steps drops it, and I have been wrong three times guessing. Somebody who knows
the script will find it faster than I will by elimination, and a wrong
mechanism written down confidently is worse than an open question -- it sends
the next person to audit the wrong thing.

**How to start.** Run the gate against that one file and print the field table
it builds; the decl regex, the `in_struct` tracking and the baseline
comparison are the three places it can be lost, and two of them are five lines
each.

**The reason it matters beyond one field.** This is the same defect lane A's
sweep caught in `apps/photomanager` the same morning -- a message recorded for
a reader that was never wired up. That one the gate *did* find. This one it
did not, and the difference between the two is the thing worth knowing.
