### TD-OILS-TIME-REPORT-ERROR-LINE-IS-THE-COMPOUND-COMMANDS. bash reports a bad `$TIMEFORMAT` against the enclosing compound command's line — 2026-08-03 — ⛔ **WONTFIX** (the same bash parser artifact as above, deliberately not replicated)

**Where:** nothing in osh implements it. `userspace/oils/src/interp.rs` —
`Shell::render_time_report` prefixes `err_prefix()`, which names
`current_line`, the line the `time` pipeline was actually written on.

An unrecognised directive is reported as `case.sh: line N: TIMEFORMAT: `z':
invalid format character`. As with the redirect entry above, bash's `N` is a
parser artifact whenever the `time` sits inside a compound command: the
report is printed by the executor long after the parser has moved the line
counter on, so it names the compound's own line — usually the *closing*
token's. Measured across six contexts (`target/dvscratch/t5/ln2.sh`), with the
`time` on the second line of each:

| context | true `time` line (osh) | bash says |
|---|---|---|
| `for … done` | 6 | 5 (the `for`) |
| `while … done` | 9 | 11 (the `done`) |
| `if … fi` | 13 | 14 (the `fi`) |
| `{ …; }` | 16 | 17 (the `}`) |
| function body | 19 | 18 (the `f() {`) |
| `( … )` | 23 | 24 (the `)`) |

At top level both shells agree. A `shift` error in the identical position
reports the same line (3) under both, which confirms this is specific to how
the *report* is emitted rather than a general divergence in `current_line`.

osh names the line the `time` was written on. The corpus case
`the-time-report-is-rendered-from-timeformat.sh` filters the prefix off with a
`nol()` helper in the section that provokes the diagnostic from inside a loop;
its two top-level provocations keep the prefix and match exactly.
