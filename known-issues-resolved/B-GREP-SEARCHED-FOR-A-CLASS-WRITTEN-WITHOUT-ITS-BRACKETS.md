## B-GREP-SEARCHED-FOR-A-CLASS-WRITTEN-WITHOUT-ITS-BRACKETS (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B); boot-tested on main (394c97655, published as 109a26eec).

**In short:** `grep '[:alpha:]'` looks as if it means "any letter", but
without its outer brackets it is a bracket expression of five characters --
`:`, `a`, `l`, `p`, `h` -- and finds the wrong lines. GNU grep refuses it
outright (`character class syntax is [[:space:]], not [:space:]`, status 2).
Ours searched for the five characters.

### What GNU grep 3.11 does

dfa.c's `colon_warning_state`: a bracket that opens with `:` (after any `^`),
closes with `:`, has something else between, and holds no range is refused,
in either regex dialect, whatever `POSIXLY_CORRECT` says, after any warning
that came before it -- and not under `-F`, which never reaches dfa.c. `[:a]`,
`[::]` and `[:a-z:]` are let through.

### The fix

`ere`: `Warning::ConfusingBracket`, computed while a bracket is parsed exactly
as dfa.c computes it, and `bre::compile_syntax_warn` so a basic expression can
report it. `grep.rs`: the pattern diagnostics are now `Note::Warning` or
`Note::Fatal`, printed in order, the first fatal one ending the run with
status 2.

### Verified

`scripts/grep-diff.sh`: 594 passed, 0 differed (was 584) -- 10 cases, both
dialects, after a warning, under `POSIXLY_CORRECT`, under `-F`, and the three
shapes let through. Unit tests: `ere` (the rule, row by row), `grep` 172.
