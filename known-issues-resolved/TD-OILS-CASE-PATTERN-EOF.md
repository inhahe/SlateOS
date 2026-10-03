### TD-OILS-CASE-PATTERN-EOF. `osh` blames end of file where bash blames the newline after an unterminated `case` pattern — 2026-07-28 — RESOLVED same day

**Where:** `userspace/oils/src/lexer.rs` `Lexer::run_into`.

**What (was):** `case x in y` with no further input reported
``syntax error near unexpected token `newline'`` in bash, plus the offending
source-line echo, but `syntax error: unexpected end of file` in osh.

**Root cause (not what this entry first guessed):** nothing to do with `case`.
bash's reader hands the parser a **newline when the input runs out**, so a
`-c` string — which never has a trailing newline — and a file that happens to
lack one both parse exactly as if one were there. osh's tokenizer just stopped.
The same divergence therefore showed up anywhere a newline is not a valid
token: `case x in y`, `case x in y|z`, `case x in (y`, `coproc`, `for`,
`echo <`, `echo >`. Writing the *same* script to a file with a trailing
newline already produced bash's message, which is what gave the cause away.

**Fix shipped:** `Lexer::run_into` appends a `Tok::Newline` when the input does
not already end in one. Fixing it in the lexer means every entry point (`-c`,
a script file, `eval`, `source`, a command substitution body) gets it at once,
and the two spellings of the same script can no longer diverge. This also
required teaching the `[[ … ]]` parser where newlines are legal — see the
follow-on below, which fixed a functional bug in its own right.

**Residue (cosmetic, accepted):** two error *layouts* still differ, both
because a `ParseError` carries a single line number for a message that bash
may spread over two. `[[ -n a` (no `]]`) reports
``unexpected EOF while looking for `]]'`` at line 1 in bash and line 2 in osh
— the second line, and the exit status, match. And `[[ (` at end of input
gains a third ``expected `)'`` line in bash, one of its per-open-paren unwind
lines that osh deliberately does not reproduce (same family as the `[[ ]]`
unwind quirk noted under TD-OILS-COND-ERRTEXT).

#### Follow-on (newlines inside `[[ … ]]`) — 2026-07-28, RESOLVED same day

**Where:** `userspace/oils/src/parser.rs` `parse_cond_not` /
`parse_cond_primary` / `cond_operand_error`, new `skip_cond_newlines` and
`cond_near`.

**What (was):** a **functional** bug, found while checking the lexer change
above did no harm — and pre-existing, not caused by it. A `[[ … ]]` test
broken across lines did not parse in osh at all:

```sh
[[ -n a
]] && echo ok        # bash: ok      osh (before): syntax error
```

Multi-line conditionals are ordinary style in real scripts, so this was not a
diagnostic nicety. bash's rule, measured against 5.2.37: the conditional
parser skips newlines wherever it waits for the **start** of a term or for
**what follows a finished one** (so `&&`, `||`, `)` and `]]` may all begin a
new line), and reads directly in the two positions where it must not — the
token after a bare word (which decides bare-word-vs-binary-operator) and a
binary operator's right-hand operand. Hence `[[ a == b <newline> ]]` parses
while `[[ a <newline> == b ]]` does not.

**Fix shipped:** `skip_cond_newlines` at the start of a term and after a
finished group/unary/binary term, and nowhere else. The two non-skipping
positions now report bash's wording, including its convention that the
``near `X'`` line names the last **word** read — which is the offending token
when that is a real word (``[[ -n ]]`` → near `` `]]' ``) but the *previous*
one when a newline walked into the slot (``[[ a`` → near `` `a' ``,
``[[ a -eq`` → near `` `-eq' ``). `cond_near` implements exactly that.
Covered by `tests/corpus/cond-multiline.sh`.
