## A-GATES-SILENTLY-STOPPED-CHECKING — four static-analysis gates were parsing a fraction of the tree and reporting "clean" — ✅ FIXED 2026-08-24 (lane A, cb6ead0a6, 28bd7f9ac)

**In short:** the project has four scripts that read every Rust file before a
boot test and refuse the build if they find a dangerous pattern — a lock held
across a call that takes the same lock, a lock held across a filesystem call, a
permission check done outside the one function allowed to do it, and a
self-test that skips itself. All four share one Rust "skimmer" that blanks out
comments and string literals so that a `//` or a `"{"` inside text is not
mistaken for code. The skimmer did not know what a **character literal** is
(`'x'` — a single character in quotes, as opposed to `"x"` in double quotes).
So the first `'"'` in a file — a perfectly ordinary character literal holding a
double-quote — was read as *opening a string*, and everything from there to the
next `"` anywhere later in the file was blanked out, braces and all. The
skimmer then lost count of `{` and `}` and stopped being able to find where
functions begin and end.

**What that cost:** a gate that cannot find any functions reports no findings,
and no findings is exactly what a clean tree looks like. There is no error, no
warning, and the build goes green. On `kernel/src/kshell.rs` the skimmer was
finding **43 function bodies where there are 984** — it was checking about 4%
of the largest file in the kernel and calling the other 96% clean.

**How it was found:** by accident, and only because the failure mode inverted
for one commit. A change to an unrelated part of `kshell.rs` shifted the
position of a `'"'`, which moved the boundary of the phantom string, which made
the gate suddenly *see* a function it had been blind to and report it. The
first reading was "the gate is flaky." Running the analyzer directly on the old
and new file and printing the body count is what turned a shrug into a number.

### The two defects

**1. Character literals (`cb6ead0a6`).** Rust uses `'` for three unrelated
things and a skimmer has to tell them apart the way the grammar does:

| Form | Example | Opens a literal? |
|---|---|---|
| character literal | `'x'`, `'\''`, `'\n'` | yes — ends at the closing `'` |
| lifetime | `&'a str`, `Foo<'_>` | no |
| loop label | `'outer: loop {` | no |

The rule implemented is the grammar's: a literal is `'\<escape>'` or
`'<one char>'` and nothing else, so a `'` not followed by a closing quote in
the right place opens nothing. The escape case has to skip the backslash *and*
the character after it, or the closing quote of `'\''` is mistaken for the
escaped one.

**Raw strings** were the same defect in a second spelling: `r"…"`, `r#"…"#`,
`br"…"` have no escapes at all, so `r"C:\"` ends at its own quote, while the
ordinary backslash-aware rule swallows it and runs on.

**2. Self-test scope (`28bd7f9ac`).** The skip-detector had to decide which
functions belong to a self-test suite, and used *reachability* — anything the
suite can call. That is the wrong closure: a suite that calls a command
dispatcher reaches every command in the shell, so the gate was treating
**1050 of 1052** functions as suite code. The right question is not "can the
suite reach it" but "does it exist *for* the suite": a helper's only callers
are the suite; production code reached through a dispatcher has other callers
too. It now grows the set from the callers side and only admits a function when
*every* caller is already in the set.

### What was hidden

Five findings appeared the moment the parser could see the file. Four were real
bugs, fixed in the commits named:

| Finding | Bug | Fix |
|---|---|---|
| `cmd_history` | held a module-global lock across `Vfs::remove`, inverting the kernel's filesystem-lock → module-state order — two CPUs wedged with no serial output | `bc8c35bf1` |
| `cmd_history` | discarded the `Result` of that `Vfs::remove` and printed "History cleared." either way | `bc8c35bf1` |
| `captags check` | consulted the MAC tag table directly instead of `path_access_verdict`, so it answered a *different question* from the one the kernel uses to allow access — a second policy that drifts | `b4e962aa5` |
| `fs/journal` self-test | skipped its flush-to-disk step on *any* write error and still printed PASSED, so a genuinely broken write was reported as a pass | `cca1b94b1` |
| `cmd_archive` | gate false positive (caused by a new integration test) — but reading it found that every failure path in `archive` reported success | `d0777b94a` |

The fifth is worth its own note: chasing a false positive to the point of
understanding it found a worse bug than any of the true positives.

### The fix that matters most

Both scripts gained a `--self-test` with a fixture, and `scripts/boot-test.sh`
runs each **before** the gate it belongs to. The fixtures are built so that a
desynced parser demonstrably drops them: each of the 15 parser cases carries a
control function that a desync loses, and all 15 return `[]` under the old
parser. The selftest-skips fixture was rewritten once because the first version
passed under *both* the old and new closures — a fixture that does not
discriminate is not a test.

This is the actual lesson. The gates' failure mode is silence, and silence is
indistinguishable from success, so **a gate needs a test that proves it still
fires.** A narrowing that quietly turns a gate off is worse than no gate,
because the gate's presence is what stopped anyone looking.
