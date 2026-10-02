## B-FOUR-PROGRAMS-MATCHED-REGULAR-EXPRESSIONS-WITH-`str::contains` (lane B, 2026-08-16) — ✅ **FIXED 2026-08-16** (engine `bed21ae38`; `grep` `bb12be713`, `sed`, `awk`, `expr` `cd9e23600`, `cat` `de06e53e3`)

**In short:** `grep`, `sed`, `awk` and `expr` did not implement regular
expressions. They searched for the pattern as a *literal substring*. So
`grep '^posix'` found nothing at all, `sed 's/^/E:/'` copied its input through
unchanged, and `grep '[ax]'` matched only a line that literally contained the
four characters `[ax]`. Every one of those looks like the program working,
which is how the whole family passed its own test suites: each test asserted the
substring behaviour it had. All four now use the `ere` crate and are checked
against the host's GNU tools; what is left under this heading is `cat`, which
is here only because it was found in the same sweep.

Found while fixing
`B-THE-OILS-TESTS-RESOLVED-grep/sed/cat-FROM-THE-CARGO-BUILD-DIRECTORY` above —
the shell's tests had been running these instead of the host's tools, and what
they were failing on was this.

### The state of it

| caller | wants | has |
|---|---|---|
| `osh`'s `[[ =~ ]]` | ERE | a real ERE engine (now the `ere` crate) |
| `grep` | BRE, and ERE under `-E` | ✅ `ere` (`bb12be713`) |
| `sed` | BRE | ✅ `ere` (rewritten whole; see below) |
| `awk`'s `/re/` and `~` | ERE | ✅ `ere` (rewritten whole; see below) |
| `expr`'s `:` | BRE anchored at the start | ✅ `ere` (rewritten whole; see below) |

It is not four bugs; it is **one missing component, absent four times** — and
the component already existed, inside the shell.

**Correction (2026-08-16):** the `expr` row of this table said `str::contains`,
which was *generous*. `expr` had no `:` operator at all — nor `match`, `substr`
or `index` — so the basename idiom `expr "$path" : '.*/\(.*\)'` was not a wrong
answer, it was `syntax error`. The entry was written by reading the other three
callers and assuming the fourth failed the same way. Recording a bug as milder
than it is costs more than not recording it, because the entry then argues
against looking.

### What has landed

`userspace/ere` (`bed21ae38`): osh's engine moved out to a crate, plus `ch` (the
character model) and `bre` (Basic REs translated to Extended). The shell now
depends on it rather than owning it, so the shell and the utilities cannot drift
about what `[a-z]` means. Two things worth knowing before using it:

* **BRE is not a subset of ERE.** `a+b` is three literal characters in BRE and a
  repetition in ERE; `\(x\)` groups in BRE and is literal in ERE; `*` is a
  literal where nothing precedes it. `ere::bre::compile` is the one translator.
* **Backreferences (`\1`) and word boundaries (`\<`, `\b`) are refused, by
  name.** A Pike VM cannot express a backreference, and quietly turning `\1`
  into a literal `1` would be a wrong answer rather than an error. If a caller
  genuinely needs them, that is a separate design decision, not a patch.

### What is left

Rewrite the four callers on it. Each is its own task and each is more than a
one-line substitution, because the missing regex is not the only thing missing:

* ~~**`grep`** — BRE by default, ERE under `-E`, literal under `-F`. Its argument
  parser also errors on `-q`, on `--`, and on every option it does not know
  (`-w -x -l -L -h -H -o -e -f -s -m`), which is why lane C saw `rc=2` from
  invocations that should have worked.~~ **Done, `bb12be713`.**
* ~~**`sed`** — BRE in both addresses and `s///`. `regex_match_at` goes.~~
  **Done.** It was not a rewiring in the end but a rewrite: the old `sed` also
  had no ranges (`1,5d` deleted lines 1 and 5), no hold space worth the name,
  and `String`-typed lines. Verified differentially against the host's GNU
  `sed` — `scripts/sed-diff.sh`, **88 of 89 cases byte-identical** on stdout and
  exit status. The one difference is the backreference gap below.
* ~~**`awk`** — ERE for `/re/`, `~`, `!~`, and for `split`/`sub`/`gsub`/`match`.~~
  **Done.** Also a rewrite rather than a rewiring, and much the larger one: the
  old `awk` was a line filter with an awk-shaped command line. It had no
  variables — not even `NR` — no assignment, no `if`, no loops, no arrays, no
  user functions, no `printf`, no `getline`, no output redirection and no range
  patterns; and its condition evaluator's fall-through was `true`, so a pattern
  it could not parse (which was most of them) silently matched every line. What
  is there now is POSIX's grammar and POSIX's semantics in eight modules under
  `userspace/coreutils/src/bin/awk/`, including the strnum rule, the lazy
  `$0`/field duality, and a static array-versus-scalar pass (arrays pass by
  reference, so `function fill(a){a[1]="x"}` has to be resolved before the run,
  not during it). Verified differentially against the host's GNU `awk --posix` —
  `scripts/awk-diff.sh`, **112 of 122 cases byte-identical** on stdout and exit
  status, and the other ten differ deliberately: four are character-versus-byte
  counting, two are `printf` edge cases, three are diagnostics we raise
  before the program runs where gawk raises them when first reached, and one is
  `\1` in a pattern — a backreference here, as in GNU `grep -E`, and the octal
  escape `\001` in gawk. The reasons are recorded in the script and in
  `awk/main.rs`, and the script fails if one of them ever stops being true.
* ~~**`expr`** — BRE anchored at the start, with the POSIX `:` return rule (the
  first group if there is one, else the match length).~~ **Done, `cd9e23600`.**
  A rewrite, and for the same reason as the other two: the regex was not the
  only thing missing. There was no `:`, `match`, `substr` or `index` at all; the
  arithmetic was `i64` and would wrap or abort where GNU is
  arbitrary-precision; a non-numeric operand became a silent `0` via
  `unwrap_or(0)` instead of a diagnostic; and the comparison level did not loop,
  so `expr 1 = 1 = 1` was a syntax error. What is there now is the whole
  grammar — seven looping, left-associative precedence levels over byte
  strings — with `:` on `ere::bre` and the arithmetic on the new `bignum` crate.

  Anchoring is worth writing down, because the obvious implementation is wrong:
  `:` is anchored by checking that the leftmost match *begins at offset 0*, not
  by splicing a `^` onto the pattern. The check is exact only because the engine
  is leftmost-longest; the splice would break `^a|b`, where it would anchor the
  first branch alone.

  Verified differentially against the host's GNU `expr` — `scripts/expr-diff.sh`,
  **161 of 161 cases byte-identical** on stdout and exit status, plus one
  recorded divergence the script *requires* to keep diverging (`a**` is refused
  where GNU folds it to `a*`). Backreferences were a second such divergence
  until `ere` grew a backtracking matcher for them; those five cases now agree.
  The cases it took to find GNU's corners are the value here: `expr '' '|' ''`
  prints `0`, not an empty line; `+0` is true and `-0` is false, because null is
  exactly `^-?0+$` or empty; `index` searches for any character of a *set*; and
  `:` binds tighter than `*`.
* ~~**`cat`**, separately: `-v` and `-A` are missing, and an unknown option is
  treated as a filename and **exits 0**, so a typo silently succeeds.~~
  **Done, `de06e53e3`.** The missing options were the least of it, and this
  entry understated it in the same way the `expr` row did. `cat` exited **0 on
  every path**, including a file it could not open, so `cat "$f" > out || die`
  never fired. And `-n` read through `BufRead::lines()`, which is UTF-8 and
  `String`: on a file that is not valid UTF-8 it stopped at the first bad byte,
  and on a CRLF file it **silently deleted the CR**. That last one is `cat`
  failing at the only thing it does — its correctness condition is byte-for-byte
  identity — and no option list mentions it.

  Now: `-A -b -e -E -n -s -t -T -u -v` and the long forms, lines split with
  `read_until` and copied as bytes, filenames carried as `OsString` from
  `args_os` to `File::open`, and diagnostics through `coreutils::errmsg`.
  `scripts/cat-diff.sh` compares **80 of 80** command lines against the host's
  GNU `cat` — as hex dumps, because `$(...)` strips the trailing newlines and
  eats the NULs that are exactly what is at issue here.

### What the sweep turned out to be about

Four of the five were rewrites, not rewirings, and the pattern is worth naming
because it will recur in the rest of the coreutils. **The missing regex was
never the whole bug; it was the part of the bug that was legible.** Underneath
it, in each program, was the same shape: a plausible-looking implementation of
a *subset*, with the rest of the specification simply absent, and a test suite
that asserted the subset. `sed` had no ranges. `awk` had no variables — not
`NR` — and its condition evaluator fell through to `true`. `expr` had no `:`,
no `match`, no `substr`, no `index`, and `i64` arithmetic. `cat` exited 0 on
every path and corrupted CRLF files under `-n`.

Two consequences for how the remaining utilities get audited:

* **A unit test cannot find this class of bug, because it is written from the
  same belief as the code.** Every one of these programs passed its own tests.
  What found the bugs was the differential harness: `scripts/{sed,awk,expr,cat}-diff.sh`
  run the real GNU tool beside ours on identical input and compare bytes, which
  is the one check that does not consult our opinion. Any coreutil claiming to
  match a POSIX tool should acquire one before it is called done.
* **A bug entry that describes the *symptom you noticed* will understate the
  defect.** This entry's `expr` and `cat` rows were both written by reading the
  neighbours and assuming the same failure, and both were milder than the truth
  — `expr`'s `:` was recorded as a substring search when there was no `:` at
  all. An entry that overstates how well something works is worse than no entry,
  because it argues against going to look.

### One thing already fixed in passing

The engine bounded a single `{m,n}` at 1000 and nothing else — but intervals
multiply under nesting, so `((a{1000}){1000}){1000}` asked for ~10⁹
instructions, tens of gigabytes, from a 24-byte pattern. `MAX_PROG` now bounds
the compiled program. This mattered more the moment the engine became shared:
`grep -f patterns.txt` and `sed -f script.sed` read the pattern from a **file**,
so it is as much untrusted input as the subject is.

### Why not a third-party regex crate

`posix/src/regex.rs` stays a separate implementation on purpose — it is
`no_std`, fixed-buffer, C-ABI, and answers to POSIX's error codes byte for byte.
For the Rust programs, see `design-decisions.md` §322.
