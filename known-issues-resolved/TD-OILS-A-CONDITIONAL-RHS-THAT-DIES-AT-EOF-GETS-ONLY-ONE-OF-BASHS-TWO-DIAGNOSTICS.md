### TD-OILS-A-CONDITIONAL-RHS-THAT-DIES-AT-EOF-GETS-ONLY-ONE-OF-BASHS-TWO-DIAGNOSTICS. `[[ x =~ ( ]]` — 2026-08-06 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/parser.rs` — `cond_operand_error` and the
conditional-expression parser around it; the error is raised by the lexer
before that code is ever reached.

**What.** An unterminated `(` in the RHS of a conditional binary operator
draws *two* messages from bash and one from osh:

```
$ cat c1.sh
echo A
[[ x =~ ( ]]
echo B

bash                                                  osh
A                                                     A
c1.sh: line 2: unexpected EOF while looking …         c1.sh: line 2: unexpected EOF while looking …
c1.sh: line 4: unexpected argument to conditional
                             binary operator
rc=2                                                  rc=2
```

The residue is the second line. The first was fixed under
TD-OILS-COND-PAREN-REGEX (2026-07-28); this is what is left.

**Why two.** `read_token_word` bails to the *parser*, it does not abort the
parse. In regexp position (`PST_REGEXP`) a `(` is handed to
`parse_matched_pair`, which on EOF prints the first message and returns
`&matched_pair_error` (y.tab.c:6026); the caller then does

```c
	  if (ttok == &matched_pair_error)
	    return -1;		/* Bail immediately. */
```

(y.tab.c:7297). `-1` is not `WORD`, so `cond_term`'s right-hand-side branch
falls into its `else` and prints its own diagnostic (y.tab.c:7095) —
`error_token_from_token(-1)` is `NULL`, which is why it is the bare form
with no `` `X' `` in it. And `line_number` has by then run to *one past the
last line of the file*, because `shell_getc` counted every line the failed
scan swallowed:

| file | last line | second message reported at |
|---|---|---|
| `echo A` / `[[ x =~ ( ]]` / `echo B` | 3 | line 4 |
| `echo A` / `[[ x =~ ( ]]` | 2 | line 3 |
| `[[ x =~ ( ]]` | 1 | line 2 |

Reached by `[[ x =~ ( ]]`, `[[ x =~ (a ]]`, ``[[ x =~ ` ]]`` and
`[[ x =~ $(echo a ]]` — the shapes whose RHS word dies at EOF. Not reached
by `[[ x == ( ]]` (no `PST_REGEXP`, so `(` is never given to
`parse_matched_pair`), nor by `[[ ( ]]`, `[[ x =~ ) ]]`, `[[ x =~ ( ) ]]`,
all of which already match byte for byte.

**✅ FIXED (2026-08-07)**, and it turned out to want no bail *token*.

The prerequisite was
TD-OILS-A-LINE-THAT-FAILS-TO-LEX-IS-NOT-REPORTED-AS-A-LINE, which stopped the
lexer throwing away the failing line's tokens. With `[[`, `x` and `=~` kept,
osh's conditional parser already reached the RHS slot and already raised an
error there — it was simply the wrong error, and was then discarded in favour
of the parked lexer one. So all that was needed was to tell the two apart and
join them:

- `Parser::truncated_at: Option<u32>` says the stream was cut short by the
  lexer rather than by the input ending, and carries the line the reader ran
  to. That is the one fact `cond_operand_error` cannot see from the inside: a
  bail leaves bash's parser holding a token, where osh's truncated stream just
  has nothing left. `IncrementalParser::line_past_end` computes the line — the
  input's last (a final line counts even with no newline of its own) plus one.
- `cond_operand_error` grew the three bail forms, all bare of any `` `X' ``
  because `error_token_from_token(-1)` can name none, and all without a `near`
  line because `COND_RETURN_ERROR` returns before `report_syntax_error`. The
  primary form is a `%c` of `-1`, i.e. the single byte 0xFF. `parse_cond_not`'s
  end-of-input arm was routed through the same builder, since bash reaches the
  same `else` from both places.
- `CondError::Cond.tail` became `Option<Str>` — the bail is the one conditional
  diagnostic with no last line. The group clauses still accumulate under it, so
  `[[ ( ( x =~ " ) ) ]]` prints the bail message and two `expected `)'` lines.
- `ParseError::bail_sequel` marks such an error, and `next_unit` prints it
  *under* the parked lexer error (`ParseError::under`) instead of letting the
  parked one replace it. It also counts as "ran dry", so the parked error is
  still released.

All three operand positions are covered, not just the binary one the entry was
written about: `[[ -n " ]]` gets the unary form and `[[ " ]]` the primary one.

**Also pinned by** `parser.rs::a_conditional_operand_that_dies_at_eof_draws_a_second_diagnostic`
and `tests/corpus/cond-operand-that-dies-at-eof.sh`.

**Found while fixing:** `[[ x =~ $(( ]]` — see
TD-OILS-AN-UNTERMINATED-ARITHMETIC-EXPANSION-IS-REPARSED-AS-A-SUBSTITUTION-BODY
below. Its *second* line is right; only the lexer's own first line is wrong,
and for a reason that has nothing to do with conditionals.
