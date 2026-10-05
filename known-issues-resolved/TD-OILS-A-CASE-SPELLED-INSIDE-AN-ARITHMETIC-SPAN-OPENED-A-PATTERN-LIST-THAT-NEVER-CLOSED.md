### TD-OILS-A-CASE-SPELLED-INSIDE-AN-ARITHMETIC-SPAN-OPENED-A-PATTERN-LIST-THAT-NEVER-CLOSED. `echo $(echo $(( case )))` — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/lexer.rs`, `read_balanced_body` and `CaseScan`.
`read_balanced_body` fed *every* character of a `$( … )` body to the `case`
tracker, arithmetic spans included. A word spelled `case` inside a `$(( … ))`
therefore opened a pattern list, `open_at_close()` then said the substitution's
`)` belonged to that list, and the `)` was pushed into the body instead of
closing it.

**Reproduce.**

```sh
echo $(echo $(( case )))
```

| | bash 5.2.37 | osh (before) |
|---|---|---|
| output | `0`, `rc=0` | `` syntax error near unexpected token `)' ``, `rc=127` |

Only the *nested* form diverged: a top-level `$(( … ))` never reaches
`read_balanced_body` with `command = true`, so `echo $(( case ))` was already
right.

bash cannot make this mistake because it never runs `read_token` over the span
at all. `parse_comsub` peeks one character past the `$(` and, if it is a second
`(`, hands the whole thing to `parse_matched_pair` with `P_ARITH`
(parse.y:4096-4104), which only counts parens. The text is an *expression*:
`case` is a name worth 0, `<` a comparison, `&&` a conjunction.

**The fix** is the third fold of `last_read_token`, finishing the merge above.
`CaseScan` no longer keeps a `cmd_pos: bool`; it holds a `CmdPos` — the same one
the token stream is folded through — and drives it with a new `Ev` event enum
that a *character* scan can produce, so `CaseScan::feed` and `CmdPos::advance`
reduce to the same nine events. `read_balanced_body` then simply does not call
`feed` while an arithmetic span is open, and emits one `Ev::ArithCmd` when a
`(( … ))` *command* closes.

Four latent bugs in that character scan went with it, none of which had been
observable (they cancelled out on every probe): `&&` counted as two `&`; `&>`
counted as a background `&` rather than a redirection; `>&` as a redirection
then a separator; `;;&` as three tokens. `feed` now recognises each operator
whole and steps over its tail.

Corpus: `an-arithmetic-span-is-not-shell-text.sh` (18 rows).
