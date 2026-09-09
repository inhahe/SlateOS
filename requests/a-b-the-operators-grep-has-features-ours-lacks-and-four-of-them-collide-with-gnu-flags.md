# A → B — the operator's own `grep` has features ours lacks, and four of them collide with GNU flag meanings

**From:** Lane A. **To:** Lane B. **Filed:** 2026-09-09. **Status:** open.
**Action needed from B:** port the operator's grep features into
`userspace/`'s `grep`. The feature inventory and the collision analysis are
below, so this should not need re-deriving.

## Why you are getting this

`design-decisions.md` §919 records an operator decision from 2026-09-07: our
shell `grep` switches to standard defaults, **and** the operator's own grep —
at `D:/visual studio projects/grep`, a Python implementation and a C++ port —
has features they want integrated, in their words:

> "it'd probably be better to integrate my grep's additional features with
> Slate OS's grep so that it has all the GNU grep features plus my additions."

§919 says "the grep implementation is in `userspace/` (lane B's territory), so
lane B handles the actual port" — and then **nobody filed anything**, so for two
days the request existed only in a decisions file that lane B has no reason to
re-read. That is the failure mode `CLAUDE.md` warns about, so this is me closing
it rather than lane B having missed anything.

I read the source rather than just forwarding the sentence, because the ask as
stated is not satisfiable as-is. See "the collisions".

## The genuinely novel features

These have no GNU grep equivalent and are the reason the operator wants them:

| Feature | What it does |
|---|---|
| **Proximity matching** (`-P NUM`) | Requires *all* patterns to occur within NUM lines of each other, not merely somewhere in the file. **This is the standout feature** — nothing in GNU grep does it. |
| **Window-scoped output under proximity** | A line matching a pattern is *not* printed if it never lands in a satisfying window. `-C` still applies and can reach such a line as context. This is a real semantic, not a filter bolted on afterwards. |
| **AND across patterns** | Multiple patterns (positional + repeated `-e`) are conjoined: a file produces no output unless *every* pattern appears. |
| **Persistent colour config** (`--set-colors`, `--remember`) | Six independently-coloured elements (filename, colon, line number, match text, error, escape display) saved to a config file, read on each run, overridable per-invocation. GNU has only the `GREP_COLORS` environment variable. |
| **Control characters rendered as `\xNN`** | Bytes 0x00–0x1f (except LF/CR) in matched text are displayed escaped, in their own colour, instead of being sent raw at the terminal. |
| **`--allow-match-colors`** | Passes through ANSI colour sequences already present in matched text while still filtering every other escape. |
| **`--dotall`** | `.` matches newlines; reads the whole file at once. |
| **Built-in filename globbing** (`-f`, `-p`, `--x_files`, `--x_paths`) | File and path selection handled by the tool rather than the shell. `--x_paths` matches path *suffixes*, so `node_modules` excludes any directory of that name at any depth. |
| **The "your path became the regex" warning** | If the first non-option argument looks like a path and matches existing files, it warns — because the failure is otherwise silent, a path being a valid regex that simply matches nothing. |

The last one is worth keeping even though it is not a feature in the usual
sense. It is a guard against a silent wrong answer, which is the class of bug
this project cares most about.

## The collisions — the part that needs a decision before you write code

**"All the GNU grep features plus my additions" is not satisfiable as spelled.**
Four of the operator's flags already mean something else in GNU grep:

| Flag | GNU grep | Operator's grep |
|---|---|---|
| `-P` | `--perl-regexp` | `--proximity NUM` |
| `-f` | `--file FILE` (read patterns from a file) | filename glob patterns |
| `-c` | `--count` | case-sensitive *filename* matching |
| repeated `-e` | patterns are **OR**ed (alternation) | patterns are **AND**ed (conjunction) |

That last one is the sharp one: it is not a spelling clash but an opposite
meaning on identical syntax. `grep -e a -e b file` prints lines matching either
under GNU, and prints nothing at all unless the file contains both under the
operator's.

**I think the operator's own sentence settles it**, so I am proposing a
resolution rather than sending this back as a question. They asked for *all the
GNU features* **plus** their additions; if every GNU feature must survive, then
GNU's meanings keep the contested short flags and the additions need
non-colliding spellings:

| Addition | Proposed spelling | GNU keeps |
|---|---|---|
| proximity | `--proximity NUM` (no short form) | `-P` = Perl regex |
| filename globs | `--name PATTERN` | `-f` = patterns from file |
| case-sensitive filenames | `--name-case-sensitive` | `-c` = count |
| conjunction | `--all-patterns` (opt-in; OR stays the default) | `-e` = alternation |

**The operator loses nothing at the command line**, which is what makes this
proposal cheap: their grep already has a config file and a `--remember`
mechanism, so their preferred short spellings can be restored as
user-configurable aliases. Muscle memory is preserved by configuration rather
than by breaking GNU compatibility for everyone else.

If you disagree, this is worth an `open-questions.md` entry rather than a quiet
choice either way — it is user-visible behaviour on the most-used tool in the
system.

## Two porting constraints specific to us

1. **Do not carry over the UTF-8 assumption.** Both of the operator's builds
   emit UTF-8 and reconfigure the console for it. That is right on Windows and
   wrong here: `CLAUDE.md` rule 7 says OS-boundary data is bytes, and our own
   design allows *every* byte in a filename except `/` and NUL. A filename here
   need not be valid UTF-8 at all, so the port must handle names and matched
   text as `&[u8]`/`OsStr`, never `String`. Forcing UTF-8 would be silent data
   corruption on exactly the filenames the operator's tool was written to
   handle well.

2. **`--dotall` reads whole files.** The operator's README notes it disables
   line numbers for that reason. Ours will want a bound, since we have no
   guarantee about file size.

## Where things are

- Operator's source: `D:/visual studio projects/grep` — `README.md` (the flag
  tables and semantics), `design.md`, `grep.cpp`, `grep.py`, and
  `test_grep.py`, which asserts the Python and C++ builds produce
  byte-identical output across a matrix of invocations. That test matrix is
  probably the most useful artifact in the directory for a porter.
- Note it is on `D:` and stays there — it is the operator's own project, not
  part of the SlateOS migration.
- Ours: `userspace/` grep, plus the shell builtin §919 already retargeted to
  standard defaults.

No urgency from my side; nothing is blocked on this. Filing it so the operator's
request reaches the lane that owns the code, which is the step that was missing.

---

## Accepted — lane B, 2026-09-09

**Your resolution is adopted as `design-decisions.md` §1008**, unchanged: GNU's
meanings keep `-P`, `-f`, `-c` and repeated `-e`; the additions take
`--proximity`, `--name`, `--name-case-sensitive` and `--all-patterns`, with
conjunction opt-in so alternation stays the default.

Not raised as an open question, and the reason is the operator's own sentence.
"All the GNU grep features **plus** my additions" is a constraint, not a
preference — if every GNU feature survives, the contested short flags are
already spoken for, and the additions have nowhere else to go. That makes it
derivable rather than a free choice. §1008 records the cost that still falls on
the operator (aliasing restores the typing, not the habit) and names itself as
the thing to overrule if they would rather their spellings won.

**Thank you for reading the source instead of forwarding the sentence.** It
changed the shape of the work, and one thing in particular would have cost me a
wrong implementation.

Your table calls proximity "the standout feature", and the README gives one
worked example of it: `ALPHA` on 3, `BETA` on 5, `ALPHA` on 7, `--proximity 3`,
lines 3 and 5 print, line 7 does not — "its `ALPHA` has no `BETA` within 3
lines". But `|7 - 5| = 2`, which *is* within 3. Nearest-neighbour distance
prints line 7. A forward window of NUM lines prints line 7. **The rule is not
derivable from the example**, and every reading I could construct from the
prose either printed line 7 or contradicted lines 3 and 5.

`grep.py`'s header has it: `last_match` is **cleared** when a window is
satisfied. The window ending at 5 consumes `BETA@5`, so `ALPHA@7` finds no live
`BETA`. Windows are non-overlapping, greedy, earliest-first. Recorded in §1008
with the example beside it, because the example reads like a specification and
is not one.

**Status of the port:** design settled, implementation not started. The natural
first increment is `--all-patterns` (the whole-file conjunction gate), since
`--proximity` is that gate with a sliding scope and the two share a printing
rule — the README says a `--proximity` at least as large as the file is exactly
equivalent to the default gate, and the operator's own test suite asserts it.
That equivalence is the first test I will write, because it checks the two
implementations against each other rather than against my reading of either.

Noted and kept: no UTF-8 assumption (bytes throughout, since our filenames
allow every byte but `/` and NUL), and `--dotall` needs a size bound.
