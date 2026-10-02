### TD-OILS-ARITH-GROUP-ERROR-TOKEN. An arithmetic syntax error inside a parenthesised group names one token too many — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/arith.rs` — the error-token capture, which reports
the remaining text from the start of the failing *group* rather than from the
failing token.

**Symptom.** The message and the exit status agree with bash; only the quoted
error token differs, and only when the failure is inside a `( … )` group:

```
$ bash -c 'echo $(( ( echo deep ) ))'
bash: ( echo deep ) : missing `)' (error token is "deep ) ")
$ osh -c 'echo $(( ( echo deep ) ))'
osh: ( echo deep ) : missing `)' (error token is "echo deep ) ")
```

bash has consumed `echo` as an operand before it discovers the group is
unterminated, so its token starts at `deep`; osh reports from `echo`.

This is **not** a consequence of the `$((` backtrack — it is reached identically
with or without one, and it predates that work. Three control shapes without a
group (`1 +* 2`, `foo bar`, `1 2 3`) all match bash byte-for-byte, so the
divergence is specific to a group.

**Impact.** Cosmetic: one word of one diagnostic, for an expression that is a
syntax error either way.

**Fixed.** The missing-`)` error now reports the source from the **cursor** —
where the parser is standing when the `)` fails to appear — instead of from
`last_atom_start`. Measuring 39 shapes against bash turned up three rules, only
the first of which was the originally-logged symptom:

- **From the cursor.** `( a b` names `b`, `((1)(2)` names `(2)`, `( (1 2) 3`
  names `2) 3` (the *inner* group fails first, and reports from inside itself).
- **At end of input, the last token read.** There is no cursor token then, so
  bash names the token its lexer is still holding — which is the operand for
  `(2+3` (→ `3`) but the *close paren* for `((2+3)` (→ `)`). That needed a new
  `last_tok_start`, updated for tokens of every kind rather than only operands
  (`last_atom_start`) and operators (`last_op_start`); the three are now set
  through `mark_atom`/`mark_op`/`mark_tok` so a new token site cannot forget one.
  A name with a subscript is a single token, so `(a[0]` names `a[0]`.
- **An untokenisable character never reaches the check.** bash's lexer rejects it
  first, so `(1 @` and `(1;2` are "invalid arithmetic operator", not "missing
  `)'". Brackets count as untokenisable in operator position — `a[0]` is lexed as
  one token, so a stray `]` is not a token at all. `is_arith_token_char` no longer
  claims `[`/`]`, which also fixes the same misclassification *outside* a group
  (`let '2+3]'`, `let '2+3['`), where osh had been saying "syntax error in
  expression". `:` is a real token and stays an expression error.

Covered by `arith.rs::a_missing_close_paren_reports_from_the_cursor` (27 shapes)
and the corpus case `tests/corpus/arith-error-token.sh` (33 shapes).
