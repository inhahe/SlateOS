### TD-OILS-BAD-ARRAY-LITERAL-IS-FATAL. an operator inside `name=( … )` aborts the script instead of discarding one unit — ✅ **RESOLVED 2026-08-03** — 2026-08-03

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::try_array_assign`, the
`segs.is_empty()` arm that returns
`LexError::new("unexpected operator in array assignment")`. The error is parked
in `IncrementalParser::pending_lex_err` (`parser.rs`) and surfaced by
`next_unit`, which routes it through `Shell::parse_error_flow`
(`interp.rs`) as an ordinary syntax error: status 2, rest of the input
abandoned.

**Reproduce:**

```sh
a=(x; y)
echo after=$?
```

| | bash 5.2.37 | osh |
|---|---|---|
| stderr | ``S: line 1: syntax error near unexpected token `;'`` then ``S: line 1: `a=(x; y)'`` | `S: line 1: unexpected operator in array assignment` |
| stdout | `after=1` | *(nothing)* |
| exit | 0 | 2 |

The same split shows for an *unterminated* literal — `a=(x` alone exits **1**
under bash and **2** under osh — while every other unterminated construct
(`echo "…`, `if …`, `$(…`, `(…`) exits 2 under both. So the two symptoms are one
bug, not two.

**What bash actually does.** Not a compound-assignment quirk: bash reports its
standard two-line syntax error naming the offending operator, discards the
*parse unit* that contains the literal, sets `$?` to 1, and **carries on with
the next unit**. Measured boundaries:

* The unit, not the line, is what dies. `echo one; a=(x; y); echo after` runs
  *nothing* — `one` is never printed — because `;` chains it all into one unit.
  Same for `if true; then a=(x; y); fi`, `echo A && a=(x; y)`, `a=(x; y) | cat`
  and a function body.
* Recovery resumes after the literal's closing `)`. In

  ```sh
  a=(x
  y; z)
  echo after=$?
  ```

  bash blames line **2**, echoes ``y; z)`` as the source line, and line 3 still
  runs (`after=1`, exit 0).
* It is specific to this construct. A syntax error inside `$( … )` stays fatal
  (exit 2, osh already agrees), and an arithmetic error is already non-fatal in
  both (`echo $((1 +))` → `after=1`, exit 0).

**Proper fix.** osh needs a *recoverable* lexer error, which it has no notion of
today — `pending_lex_err` only ever ends the input. Three parts:

1. `try_array_assign`: on a bad operator, skip forward to the matching `)` and
   emit a poison token (e.g. `Tok::Invalid(Str)` carrying the offending
   operator's spelling) instead of returning `Err`, so the rest of the source
   still tokenizes and later units survive.
2. `Parser`: turn `Tok::Invalid` into `syntax error near unexpected token \`…'`
   built the usual way (`unexpected_here`/`token_display`), flagged recoverable,
   and resync `next_unit` to the following `Tok::Newline` so the next unit starts
   cleanly.
3. `ParseError`: a third flavour alongside `fatal` — call it `recoverable` — that
   `parse_error_flow` answers with `last_status = 1` and *continues* the
   `run_source_flow_units` loop rather than returning `Flow::Next` (which today
   abandons the whole input).

**Impact.** Only malformed scripts, so nothing correct behaves differently — but
the failure mode is the bad kind: a script with one bad literal loses every
command after it *and* every command before it on other lines, where bash loses
only the one unit. It also makes osh's diagnostic unrecognisable to anyone
grepping for bash's wording.

**Fixed 2026-08-03**, exactly as the three parts above describe.

* `lexer.rs`: `Tok::Invalid(Str)` carries the spelling to blame.
  `try_array_assign` no longer returns `Err` on a bad element — it records the
  first offending operator and keeps consuming to the literal's closing `)`,
  reusing the loop that already knows the literal's extent rather than adding a
  second paren scanner. `take_operator` names the token (longest-first against
  `OPERATOR_SPELLINGS`) and always consumes at least one character, so the loop
  cannot spin on something that is not an operator.

  Probing widened the bug past what was first logged: *every* reader error
  inside a literal is worth 1, not just a bad operator. So the unterminated
  `eof_matching(')')` is marked `.recoverable()` (making `a=(x` worth 1), and so
  is anything `read_array_elem_word` raises — an unterminated `'`, `"`, `${`,
  backquote or `$(` inside a literal, each worth 1 where the identical construct
  *outside* one is still worth 2.

  A recorded element also outranks a missing `)`, and then recovery rewinds to
  just past that element instead of consuming to end of input: `a=(x <<EOF` /
  `echo body` / `EOF` blames `<<` and runs `echo body` as an ordinary command
  rather than swallowing it as a here-document body, exactly as bash does. The
  rewind only moves the cursor, which is safe because `Lexer::cur_line` derives
  the line from it.
* `parser.rs`: `ParseError::recoverable`, carried across from `LexError`.
  `token_display` spells a `Tok::Invalid`, so `unexpected_here` builds bash's
  wording with no special case, and flags the error recoverable. `next_unit`
  gained an arm ahead of the general `Err` one that resyncs past the unit's
  closing newline instead of abandoning the input.
* `interp.rs`: `run_source_flow_units` answers a recoverable error with
  `last_status = 1` and `continue`, leaving `parse_error_flow` (status 2 /
  abandon) for everything else.

Verified against bash 5.2.37 on all 14 operator spellings, the multi-line
literal, every unterminated construct that can sit inside a literal, and every
context a literal appears in (`declare -a`, `+=`, inside `if`/a function/a
pipeline/`&&`). Lib test
`a_malformed_array_literal_costs_only_its_own_unit`; corpus case
`a-malformed-array-literal-costs-only-its-own-unit.sh`.
