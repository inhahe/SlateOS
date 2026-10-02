### TD-OILS-A-FAILED-EXTENT-PARSE-CONSUMES-THE-REST-OF-THE-STRING. `v='$(fi)tail'; echo "${v@P}"` prints `tail`; bash prints nothing — 2026-08-09 — ✅ RESOLVED 2026-08-09

**Where:** `userspace/oils/src/interp.rs`, `command_sub_body_inner`'s
`CmdSubBody::Unread` arm — the `self.prompt_expanding` path, which today runs
`src` and lets the parts after it expand normally.

**Reproduce** (bash 5.2.37 left, osh right):

```
$ cat vt.sh
u='$(fi)tail'
printf '3 [%s]\n' "${u@P}"

bash: vt.sh: command substitution: line 2: syntax error near unexpected token `fi'
      vt.sh: command substitution: line 2: `fi)tail'
      vt.sh: command substitution: line 1: syntax error near unexpected token `fi'
      vt.sh: command substitution: line 1: `fi)tai'      <-- body is `fi)tai'
      3 []                                               <-- `tail' was eaten

osh:  … line 2: `fi)tail'
      … line 1: `fi'                                     <-- body is `fi'
      3 [tail]
```

**Why.** Only reachable under `no_longjmp_on_fatal_error` (a prompt expansion),
because otherwise the failure aborts before any of this matters. There,
`parse_string` takes the `SEVAL_NOLONGJMP` branch on a syntax error — it calls
`reset_parser ()` and `break`s (builtins/evalstring.c:688-695) — with
`bash_input.location.string` already at the end of the string, since a string
input's reader swallows the whole thing as one "line". So:

* `nc = ep - ostring` is the **whole remainder**, and `xparse_dolparen` returns
  `substring (ostring, 0, nc - 1)` (parse.y:4377) — the remainder **less its
  last byte**. That is the text `command_substitute` gets, hence the child's
  echoed `` `fi)tai' ``.
* `*indp = ep - base - 1` (parse.y:4348) leaves the caller's index at the last
  byte, so `param_expand` consumes everything after the substitution and the
  expansion contributes nothing but the (empty) substitution result. Hence
  `[]`, and hence a `PS4` of `'$(fi)+ '` printing xtrace lines with no `+ `.

Note the `ep[-1] != ')'` guard just above (parse.y:4338) strips trailing
*newlines* only, so a body ending in a newline loses them before the `nc - 1`.

**Proper fix.** In the `prompt_expanding` branch: run
`src + ")" + tail`, minus trailing newlines when the last byte is not `)`, minus
one final byte; and signal the enclosing expansion that the rest of *its* string
is consumed. The second half is the invasive part — it needs a `Shell` flag the
word-part loop honours and the enclosing string level clears, where a "string
level" is the top-level word `expand_double_quoted` was given or a nested
operand word (bash recurses `expand_word_internal` per operand, so the
consumption is bounded by the level the substitution sits in). Deferred because
it touches the general word-expansion loop for a corner (a syntax-error `$( … )`
with text after it, inside a prompt string) that nothing else reaches.

**Corpus:** `a-here-document-body-is-not-a-word-the-parser-read.sh` deliberately
puts nothing after the substitution on its prompt rows, and says so.

**Resolved.** Both halves, and the second turned out far less invasive than the
sketch feared. `Shell::comsub_reparse_read` now answers with an `ExtentRead`
rather than a bool, so the caller can tell "the read found its `)`" from "the
read failed but the jump was suppressed"; the latter runs
`Shell::failed_extent_body` — the whole remainder (`src`, then `)` if it closed,
then `tail`) less its last byte — and sets `Shell::extent_consumed`.
`expand_double_quoted`, which *is* bash's `expand_word_internal`, breaks its
part walk on that flag and clears it, which is exactly the scope bash's `sindex`
belongs to. No general word-expansion surgery was needed.

**This subsumed the unclosed-`$(` case.** `Shell::unclosed_comsub_body` is gone:
a body with no `)` is simply a read that cannot succeed, so it is the same
`ExtentRead::Abandoned` path with an empty `tail`, and the same one-byte
arithmetic. Two branches became one rule.

**The consumption is the string's, not the word's.** Measured:
`b='A$(fi)B${y}C'; echo "${b@P}"` is `A` — the `${y}` between the literals is
skipped too, so the flag had to be honoured by the part *loop* rather than by
literal concatenation. Likewise `g='A$(fi)B$(echo hi)C'` is `A`, the second
substitution never running because its text is inside the body the first one
swallowed (bash echoes that body as `` `fi)B$(echo hi)' ``).

**Corpus:** `a-failed-extent-parse-consumes-the-rest-of-the-string.sh`.
