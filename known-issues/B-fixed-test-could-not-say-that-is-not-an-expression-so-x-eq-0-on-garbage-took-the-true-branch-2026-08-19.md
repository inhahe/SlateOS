## [B] FIXED — `test` could not say "that is not an expression", so `[ "$x" -eq 0 ]` on garbage took the *true* branch (2026-08-19)

**In short:** `test` (and its other name, `[`) is the program a shell script
calls to decide an `if`. It answers with an exit status, and there are three of
them: 0 true, 1 false, and **2 "what you gave me is not an expression"**. Ours
had no way to produce the third — its whole evaluator returned a `bool` — so
every malformed expression came back as a plain yes or no. `test abc -eq 0`
answered **true**, because both sides were parsed with `unwrap_or(0)` and
`0 -eq 0` is true. A script guarding on `if [ "$count" -eq 0 ]` therefore ran
its zero branch whenever `$count` held anything that was not a number, silently
and with no message. It is rewritten against GNU coreutils 9.4;
`scripts/test-diff.sh` now agrees with GNU on **194 of 194** cases, with no
declared divergences.

### Why this one was worse than a wrong answer

Every other coreutils bug in this tree produces visible wrong output. This one
produced a *branch*. The failure is invisible at the point it happens and shows
up later as the wrong file deleted, the wrong service started, the wrong
default applied — with nothing on stderr tying it back. It is also the
most-executed program in the system: nearly every line of every shell script
reaches it or the builtin copy of it.

The old implementation was 337 lines with the signature
`fn evaluate(args: &[String]) -> bool`. The signature *is* the bug: a type with
two inhabitants cannot express a three-valued answer, so no amount of care
inside the function could have produced status 2.

### What was missing

| | |
|---|---|
| status 2 | **the entire concept**. No diagnostic was ever printed, for any input |
| the argument-count rules | absent. POSIX defines `test` by argument *count* first and only runs a parser past four; ours scanned for `-a`/`-o` at any depth |
| parentheses | absent. `test '(' a = a ')'` was false |
| `-l STRING` | absent |
| `-ef` `-nt` `-ot` | absent |
| `-b -c -p -S -g -u -k -O -G -N -t` | absent |
| non-UTF-8 argv | `env::args()`, which **panics**. A file name is bytes |
| integer width | `parse::<i64>()`, where `test` compares decimal *text* at arbitrary precision |

### The rules that only reading the source could have supplied

Four of these decide answers, and none of them is recoverable by probing an
implementation you already believe is nearly right, because each one changes
what the cases *around* it mean:

1. **Argument count is checked before operators are, and the order inside each
   count is load-bearing.** At three arguments the binary operator is looked
   for *first*, so `test ! = x` compares the string `!` with `x` rather than
   negating anything, and `test ( = )` compares `(` with `)`. Checking `!`
   first — which is what reads naturally — silently changes the answer for
   every expression whose left operand happens to be `!` or `(`.
2. **Neither `-a` nor `-o` short-circuits.** GNU accumulates with
   `value |= and()` and `value &= term()`, so the far side is evaluated even
   once the answer is settled, and a syntax error over there is still reported:
   `test x = x -o abc -eq 1` is status 2, not true. A short-circuiting
   implementation passes every case whose far side is well-formed, which is
   every case anyone writes deliberately.
3. **Integers are compared as decimal text, so `test` is arbitrary-precision.**
   `test 99999999999999999999999999 -eq 99999999999999999999999999` is true.
   Any `i64` implementation reports an error or, worse, wraps.
4. **`-l STRING` is an integer** equal to that string's length, accepted
   wherever an integer is (`test -l abc -eq 3`, and on either side, and on
   both), recognised only when enough arguments remain, and refused *by name*
   on the three file comparisons (`-ef does not accept -l`). It is documented
   only in the last line of `--help`.

### How the rewrite was verified

`scripts/test-diff.sh`, 194 cases, run against `/usr/bin/test` under WSL —
the real binary, not bash's builtin, which is a different implementation.
Unlike the other harnesses in this directory it is **batched**: `test` writes no
files and no stdout, so every case is encoded into one file (arguments joined
by `\x1f`, prefixed by an explicit argument count) and the whole sweep runs in a
single WSL launch instead of one per case.

The count prefix is not redundant. Joining arguments alone encodes `test` and
`test ''` identically, and those are different expressions with different
answers — zero arguments is false, one empty argument is also false, but
`test '' = ''` is true and the count is what keeps the three apart.

Baseline, before the rewrite: **19 of 157 cases differed**, every one of them a
malformed expression answered 0 or 1 with nothing on stderr.

### Two bugs the harness found in the rewrite itself

Worth recording because both are the same failure mode the rewrite exists to
remove, surviving *inside* it:

- **`test -t x` answered false where GNU says `invalid integer 'x'`.** `-t`
  routes its descriptor number through `find_int`, the same routine the numeric
  comparisons use, so a malformed one is an error. Only a *well-formed* number
  too large to be a descriptor is quietly false, because upstream learns that
  from `strtol`'s `ERANGE` rather than from the syntax check. I had written the
  opposite in a comment and been confident about it.
- **Two harness cases were declared expected-to-differ on a false premise.** I
  had marked `test --help` and `test --version` as divergences on the
  assumption that they are options. They are not: POSIX requires `test --help`
  to be the one-argument form applied to the non-empty string `--help` — status
  0, not a byte of output — and GNU obeys (measured). The long options exist
  only under the name `[`, only as the sole argument, and only without the
  closing `]`. The harness reported them as XPASS, which is the only reason
  they were re-examined; an `xfail` that is never checked for having started
  passing is a permanent blind spot.

### Also in this change

The harness's comparison loop ran `awk 'NR==n'` over both result files once per
case and forked three subshells per case — about 470 process creations, which
took thirteen minutes on MSYS for work that is one linear pass. It is now a
single `paste` + `awk`. The sweep went from ~30 minutes to a few, and the cost
that remains is Windows process creation for the 194 invocations of our own
binary, which nothing here can avoid.

Diagnostics now name the *invoked* program: a failing `[ … ]` says `[:`, a
failing `test …` says `test:`. GNU echoes `argv[0]` verbatim (so an absolute
invocation reads `/usr/bin/test:`); we print the bare name to match every other
utility in this crate, and the harness normalises the prefix on both sides so
that choice cannot masquerade as a behavioural difference. On the first run it
did exactly that — 37 "failures", all of them the prefix and none of them the
message.
