### A-KSHELL-SUBSTRING-OPERANDS-WERE-PARSED-NOT-EVALUATED. `${x:off:len}` read its two operands with `parse::<i64>()` instead of evaluating them, and indexed by byte — **Status: FIXED** 2026-08-29 (boot test green, rung 105 present in the serial log; merged to `main` 2026-08-30)

**In short:** the shell's "give me part of this string" syntax got the wrong
part in six different situations. `${x:0:n}` — take the first `n` characters,
the most ordinary use there is — ignored `n` entirely and returned the whole
string; `${x:1+1:2}` returned nothing at all; and on any string containing a
non-ASCII character, cutting it usually produced nothing. Every one of these
was silent: no message, no failure status, just a different answer.

**Where:** `kernel/src/kshell.rs` — one arm of `expand_brace_expr`'s
eleven-arm operator chain, now split out as `expand_substring`.

**What was wrong.** Bash evaluates both operands as *arithmetic expressions*,
in the same context `$(( ))` uses: a bare name is that variable's value, an
unset name is `0`, an empty expression is `0`. Ours called
`parse::<i64>()` / `parse::<usize>()` on them, which is correct on every
literal and wrong on every name — which is exactly why it lasted. With
`x=abcdefgh`:

| Expression | bash | ours | direction |
|---|---|---|---|
| `${x:1:abc}` | `` (unset name ⇒ 0) | `bcdefgh` | too much |
| `${x:0:n}`, `n=3` | `abc` | `abcdefgh` | too much |
| `${x:2:-2}` | `cdef` | `cdefgh` | too much |
| `${x: -3:-1}` | `fg` | `fgh` | too much |
| `${x:1+1:2}` | `cd` | `` | too little |
| `${x:abc}` | `abcdefgh` (offset 0) | `` | too little |
| `${x:2}`, `x=héllo` | `llo` | `` | too little |
| `${x:7:-2}` | error, `` | `h` | too much |

**Which way it failed depended on which operand it could not read**, and that
is worth stating plainly because the tempting summary — "it returned too much"
— would send the next reader looking for a clamp. An unreadable *length* left
`end` at its initialiser, "the rest of the string"; an unreadable *offset*
skipped the whole `if let Ok` and pushed nothing.

The `-2` rows are a rule rather than an off-by-one: **a negative length is an
offset from the end of the whole value, not a count**, so `${x:2:-2}` means
"from character 2 up to two from the end". `parse::<usize>()` rejected it and
the code fell through to "the rest of the string" — the exact opposite of
dropping the tail. A negative length landing *before* the offset is the single
case bash diagnoses (`substring expression < 0`); an out-of-range *offset* it
silently expands to nothing. That asymmetry is bash's and is kept.

The `héllo` row is separate and was not in the original report. Indices were
**byte** offsets, so a cut landing inside a UTF-8 sequence made `str::get`
return `None`, which `.unwrap_or("")` turned into the empty string — silently,
with no diagnostic and no failure status. `${#x}` had the matching fault in the
other direction: it reported `6` for a five-character string.

**The fix.** Both operands now go through `eval_arithmetic`, the same evaluator
`$(( ))` uses, so a name, an expression and an empty string all mean what they
mean everywhere else in the shell. Indices count **characters**: that is what
SlateOS's real shell does (`userspace/oils`, see `TD-OILS-STRLEN-CHARS` in this
file) and what bash does in a UTF-8 locale, which is the only locale this OS
has — a debug shell that disagreed with the system shell about `${x:1:2}` would
be its own trap. `${#NAME}` was changed to count characters in the same commit,
because a shell that measures a string one way and cuts it another makes
`${x:0:${#x}}` an expression whose two halves disagree about what a unit is.

**How it was found:** by reading, not by a failure. It was noticed while
auditing the 17 false positives of the option-refusal gate's D1 shape (see the
batch-40 section above, where it is recorded as deferred) and deferred there on
the grounds that fixing it needed bash's actual arithmetic-context rules rather
than a guess at them. Those rules were then measured against bash directly —
each row of the table above is an observed result, not a reading of the manual.

**Pinned by** self-test rung 105, which asserts every row of the table plus the
`substring expression < 0` diagnostic, the silent-empty out-of-range cases, and
that `${#x}` and a cut of that exact length agree. The error case is captured
around `expand_vars` rather than around a command: the diagnostic belongs to the
expander, not to any command, and `check-selftest-wording.py` correctly refused
the first draft, which had asserted it against `echo`.

**Still divergent, deliberately not fixed here:** `${x:}` — an empty spec — is
`bad substitution` in bash and expands to the variable's value here, because
the substring arm is guarded by `rest.len() > 1`. That is a question about the
shell's error *reporting* (kshell has no "bad substitution" concept at all),
not about substring arithmetic, and folding it in would have made this commit
two changes.
