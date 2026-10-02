### BUG-OILS-BACKTICK-DQUOTE-ESCAPE. A `\"` inside a backtick substitution inside double quotes was left in the body — 2026-07-31 — ✅ **RESOLVED 2026-07-31**

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::read_backtick`, which
stripped `` \` ``, `\\` and `\$` but never `\"`.

**Reproduce** (found while writing `tests/corpus/bash-subshell-level.sh`,
whose backtick line printed `["[1]"]` instead of `[[1]]`):

```sh
echo "`echo \"x\"`"        # bash: x        osh: "x"
echo "`echo '\"'`"         # bash: "        osh: \"
v="`echo \"y\"`"; echo $v  # bash: y        osh: "y"
```

The escapes in a backtick body belong to the *enclosing* text, not to the
command inside, and are removed before the body is ever read as a command.
Which ones depends on where the substitution sits: everywhere it is
`` \` ``, `\\` and `\$`, and inside a double-quoted string it is also `\"`,
because there the backslash is one of the characters double quotes give
meaning to. The strip happens before parsing, so it applies even where the
body would have quoted the character itself — hence `` `echo '\"'` ``
running as `echo '"'`.

Two contexts nearby are *not* double-quoted strings and keep the
backslash, both measured rather than assumed: an unquoted `` `…` `` word,
and an expanding here-doc body (`<<EOF` … `` `echo \"x\"` `` prints `"x"`
in bash). `$( … )` is a different construct altogether — its body is
ordinary source and nothing is stripped from it.

**Fixed.** `read_backtick` takes an `in_dquote` flag, set only by
`read_double_quote_until`; the here-doc scanner in `scan_heredoc_segs` and
the three word scanners pass `false`. The *verbatim* source slice is
untouched, so `declare -f` still prints the body as written (and the
printed form still re-parses). Covered by
`tests/corpus/backtick-escapes.sh` and the unit test
`a_backtick_body_in_double_quotes_loses_its_quote_escapes`.
