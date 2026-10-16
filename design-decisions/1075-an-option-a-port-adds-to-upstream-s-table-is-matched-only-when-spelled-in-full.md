## 1075. An option a port adds to upstream's table is matched only when spelled in full

**Date:** 2026-10-15
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** GNU-style programs accept any unambiguous abbreviation of a
long option -- `pkill --term pts/1` is `pkill --terminal pts/1`. When a
SlateOS port adds options of its own to an upstream program, the new names
can make an abbreviation that worked on Linux ambiguous, or make it mean the
new option instead: adding `--terminate` to `pkill` would turn `--term` into
an error. So an added option is recognised only when typed whole, and takes
no part in abbreviation: every command line that worked on Linux means the
same thing here.

### The case that raised it

§1072 gives `kill`, `pkill` and `killall` the options `--close`,
`--terminate`, `--force`, `--via` and `--timeout`. Matched as glibc matches,
they would have changed:

| command line | upstream | with the options matched as glibc does |
|---|---|---|
| `pkill --term pts/1 x` | `--terminal` | ambiguous: `--terminal`, `--terminate` |
| `pkill --f x` | `--full` | ambiguous: `--full`, `--force` |
| `kill --t` | `--table` | ambiguous: `--table`, `--terminate`, `--timeout` |
| `killall --v x` | ambiguous already | (unchanged) |

### What was done

`getoptlong`'s `Parser::whole_only(names)`: the options named are found by an
exact match only, and are left out when an abbreviation is resolved. The
three programs pass `coreutils::procps::requests::NAMES`.

### Alternatives

| | for | against |
|---|---|---|
| **Whole names only (chosen)** | no upstream command line changes meaning; the rule is one line in each program | a user cannot abbreviate SlateOS's options (`--clo` is not `--close`), unlike every other long option |
| Glibc's matching for everything | uniform | silently breaks scripts and habits that abbreviate upstream options, which is what a port must not do |
| Names chosen to collide with nothing | abbreviable | each program's free prefixes differ (`kill`'s `--t`, `pkill`'s `--term`), so the three commands could not share their spelling, which §1072 asks for |

### How to reverse

Drop the `.whole_only(...)` calls; the harnesses (`kill-diff.sh`,
`pgrep-diff.sh`, `killall-diff.sh`) then show which upstream cases change.

### Applies to

Any later port that adds options to an upstream table: list them with
`whole_only`.
