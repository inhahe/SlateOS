## §333 — Backreferences get a second, budgeted engine rather than a refusal, and every matching call becomes fallible

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)

**In short:** A handful of classic shell one-liners use a regular expression
that says "match the same text this earlier part matched" — written `\1`. Ours
refused those outright, so `sed '$!N;/^\(.*\)\n\1$/!P;D'` (drop adjacent
duplicate lines) printed an error where GNU prints the file. It refused for a
real reason: the matcher we use physically cannot express that construct. The
fix is to keep that matcher for everything it can do, and hand the few patterns
it cannot to a second, slower one — which can, on a shaped input, take
effectively for ever, so it is given a step budget and allowed to give up.
"I gave up" is a third answer beside "matched" and "did not match", and the
consequence of that is the visible part of this decision: **every call that asks
whether a pattern matches now returns a `Result`**, in the engine and in the
five programs that use it.

### The constraint

`ere`'s matcher is a **Pike VM** (a matcher that walks all the alternatives of a
pattern side by side, one input character at a time). That is what buys the
crate its headline property: no catastrophic backtracking, cost bounded by
`O(len(subject) × len(program))` whatever the pattern.

A backreference cannot be an instruction in such a machine, and this is not an
omission to be filled in later — it is the shape of the machine. Because every
alternative advances together, at the moment a `\1` is reached there is no
single "the" text group 1 captured (each live alternative has its own), and no
single width to advance the input by. The instruction would have to ask "which
of my simultaneous selves am I?", which the design has no way to answer.

So the choice was never "add `\1` to the Pike VM". It was between refusing the
construct and running a *different* matcher for the patterns that use it.

### The decision

Do what glibc does: **compile every pattern the same way, and choose the matcher
once, at compile time, on whether the pattern contains a backreference.**
Patterns without one — every pattern in every script in this tree today — take
the Pike VM and keep its guarantee untouched, bit for bit. Patterns with one
take a backtracker with an explicit stack and a step budget.

| Option | *What changes:* |
|---|---|
| **Refuse `\1`–`\9`** (status quo ante) | `sed '$!N;/^\(.*\)\n\1$/!P;D'` prints `backreference \1 is not supported` and exits 1. |
| **Second engine, budgeted** (chosen) | The same command prints the de-duplicated file, and a *deliberately* pathological pattern stops with a diagnostic instead of running for ever. |
| **Second engine, unbudgeted** | The same command works; a pathological pattern wedges the process until it is killed. |

The second engine is a genuine cost and it is worth naming plainly: **matching a
backreference is NP-hard**, so for the patterns that take it the crate's
no-blowup promise does not hold. It holds for every other pattern, which is what
makes the trade acceptable — the property is not weakened globally, it is
switched off for the constructs that cannot have it in the first place. A
pattern that wants the guarantee keeps it by not using `\1`, and
`Regex::has_backref` reports which engine a pattern got, so a caller that must
have the bound can check rather than guess.

Rejected on the way:

- **Memoize the backtracker.** Sound for a plain regex; not sound with
  backreferences, because "the same position in the same instruction" is no
  longer the same state — the captured text is part of the state, and it is
  unbounded. It would give wrong answers, which is worse than slow ones.
- **Refuse only the patterns that could blow up.** There is no such test. The
  blowup depends on the subject as much as the pattern, and patterns that are
  merely polynomial (not exponential) still take minutes on a large file while
  passing any static screen.

### Why the budget forced the API to change, which is the part with teeth

A budget means a search can end **without an answer**. Somebody has to be told,
and the only place to tell them is the return value.

The tempting shape — keep `fn is_match(&self) -> bool` and return `false` when
the budget runs out — is exactly the failure this crate was written to end.
`sed '/re/!d'` **deletes every line the pattern does not match**. `grep -v`
feeding an `xargs rm` deletes every file whose name did not match. Reporting an
abandoned search as "did not match" destroys the user's data on the strength of
a question we refused to answer, silently, with exit status 0. So:

```rust
pub fn is_match(&self, text: BStr<'_>) -> Result<bool, MatchLimit>
```

and the same for `captures`, `find`, `find_at`, `capture_spans` and
`capture_spans_at`; `find_iter` and `capture_spans_iter` yield `Result` items.

Also rejected here:

- **Infallible spellings beside `try_` twins.** A footgun with a friendly name:
  the short spelling is the one that gets typed, and it is the one that is
  wrong. There is no call site in this tree where "treat it as no match" is
  correct.
- **A flag on the `Regex`, checked afterwards.** Requires interior mutability,
  which costs `Regex` its `Sync`, and it is opt-in error checking — the caller
  who forgets gets the silent wrong answer, which is the case we are trying to
  make unrepresentable.

Each of the five callers maps `MatchLimit` onto the failure channel it already
has, and none of them maps it onto "no match":

| Caller | Where it lands |
|---|---|
| `grep` | an I/O-class error for that file: diagnostic, non-zero status |
| `sed` | a new `Stop::Limit`, distinct from `Stop::Io` so the message is not "couldn't write" |
| `awk` | `Fatal` — via `From<MatchLimit>`; the run ends with a diagnostic and status 2 |
| `expr` | `Fail`, the same channel a bad pattern uses |
| `osh`'s `[[ =~ ]]` | `cond_regex_error`: status **2**, which bash already distinguishes from the 1 that means "no match" |

Threading it through cost `awk` a small refactor — `FS` and `RS` are regexes the
program chose, so splitting a record is fallible, and `ensure_split`,
`get_field`/`set_field` and `get_var`/`set_var` became fallible with it. That is
the honest shape: a field split that gave up halfway would misnumber every field
on the line, and `$2` reading a value out of a split that never finished is a
wrong answer, not an error-free one.

### The budget, and two things it is not

`BACKTRACK_BUDGET_BASE` (1e6) `+ 1000 ×` subject length, capped at 1e8 steps —
generous enough that no honest pattern reaches it (a 20 000-character subject
matched against `^(a)\1*$` finishes well inside it) and small enough that a
pattern built to blow up stops in well under a second. A backreference also
charges its own length to the budget, so a long comparison cannot be free.

The budget is **not** a security boundary — a pattern is not usually
attacker-supplied — and it is **not** a substitute for `MAX_PROG`, which still
bounds compiled program size. It is the thing that turns "this hangs" into "this
says why it stopped".

Two details in the backtracker are worth recording, because getting either wrong
is a crash rather than a wrong answer:

- **Frames live in a `Vec`, not on the call stack.** Recursion depth would grow
  with the number of repetitions matched, so `\(a\)\1a*` against a megabyte of
  `a` would be a stack overflow — in `grep`, `sed`, `awk`, `expr` and the shell
  at once.
- **A backward jump is refused twice at the same input position on one path.**
  Without it `\(a*\)*` loops for ever having consumed nothing. Refusing it is
  also the answer Perl gives: an iteration that consumed nothing ends the loop.

And one POSIX detail that is a wrong answer rather than a crash: a reference to
a group that did not participate **fails**, it does not match the empty string.
`^\(\(a\)\|b\)\2$` must not match `b`.

### `\1` is honoured in *extended* expressions too, which gawk does not do

POSIX puts backreferences in basic expressions only and leaves `\1` in an
extended one **undefined**. There is therefore no "correct" reading to defer to,
only two extensions in the field: GNU `grep -E` treats it as a backreference,
and `gawk` treats it as the octal escape `\001`.

We take `grep -E`'s. Two reasons, in order of weight:

1. **`bre` is a translator, not a second parser.** A basic expression is
   rewritten into the extended dialect and compiled by the one engine — the
   whole point of `design-decisions.md` §322, so that `[a-z]` cannot mean two
   things in two programs. If the extended parser refused `\1`, the translated
   basic expression would be refused too, and the feature would exist nowhere.
2. **Five programs share the engine.** `grep`, `sed`, `expr`, `awk` and the
   shell's `[[ =~ ]]` would otherwise disagree about one pattern, which is the
   state §322 was written to leave behind.

The cost is a new deliberate divergence from `gawk --posix`: `awk '/(.)\1/'`
selects a line with a doubled character here and a line containing byte 0x01
there. It is recorded in `awk`'s module docs and pinned by an `xfail_case` in
`scripts/awk-diff.sh`, so if `gawk` ever changes, the harness says so.

**Revisited 2026-10-01, for awk only (§1055).** The premise was wrong for awk:
POSIX's awk table defines `\ddd` octal in awk EREs, so `\1` is byte 0x01 there,
as gawk reads it. awk now resolves its escapes before the engine, as gawk does,
and the engine's backreference reading is unchanged for every other program.
