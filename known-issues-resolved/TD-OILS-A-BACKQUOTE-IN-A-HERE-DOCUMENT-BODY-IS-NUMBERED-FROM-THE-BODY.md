### TD-OILS-A-BACKQUOTE-IN-A-HERE-DOCUMENT-BODY-IS-NUMBERED-FROM-THE-BODY. `` `fi` `` in a here-doc is blamed to line 1 — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs`, `command_sub_body_inner`'s
`CmdSubBody::Backtick | CmdSubBody::ArithFallback` arm, which numbered the body
`LineMap::Offset(close_line - 1)`.

**Reproduce:**

```
$ cat -n s1.sh
     1  echo one
     2  cat <<E
     3  `fi`
     4  E
     5  echo "after rc=$?"

bash: s1.sh: command substitution: line 2: syntax error near unexpected token `fi'
osh:  s1.sh: command substitution: line 1: syntax error near unexpected token `fi'
```

**Why.** bash numbers *every* substitution's run from the global `line_number`
at expansion time, less one (`parse_and_execute` does `line_number--`,
evalstring.c:329). For a substitution in a word the parser read, osh's
`close_line` happens to coincide with that — verified on a multi-line backquote
in an ordinary word, which both shells blame to line 5 — but a here-document
body is scanned by `scan_heredoc_segs` from a fresh `Lexer` over the body alone,
so its `close_line` is body-relative and the coincidence breaks.

`CmdSubBody::Unread` already sidestepped this: it numbers from the shell's own
line, which is what bash's `line_number` holds. The same turned out to be right
for `Backtick`, `ArithFallback` **and `Parsed`** — `close_line` was a
coincidence everywhere, not a rule.

**Fixed.** All three arms now take `LineMap::Offset(self.source_line() - 1)`.
Two further things fell out of the measurement:

* **The base is the *true* line, not the reported one.** A `LineMap` base is one
  of the few readers that wants `Shell::source_line()` rather than
  `Shell::current_line`, because the child re-applies `Shell::line_bias` to
  whatever the map produces. Using the biased value subtracted the drift twice,
  which showed up as `declare -i m; if true; then m=1+; fi; echo "$(echo
  $LINENO)"` printing 3 where bash prints 4 — caught by the existing
  `a_discard_out_of_a_compound_command_loses_a_line` test. `Unread` had the same
  bug and is fixed with the rest.
* **The shapes that tell the two schemes apart** are the ones where the word is
  not on the line its command is numbered by: an element of a compound
  assignment (`arr=(` … `)` takes the *closing* line; `declare -A m=(` … `)`
  takes the *opening* one) and a here-document body (takes the redirecting
  command's line). Pinned in
  `tests/corpus/a-substitution-body-is-numbered-from-the-line-the-shell-is-on.sh`
  and in the two `lineno_inside_*` unit tests, both renamed off "the closing
  paren/tick".

`close_line` is gone from `Backtick` and `ArithFallback`, which had no other
reader. It stays on `Parsed` and `Unread`, where `comsub_reparse_error` still
needs it to name the line of the *extent-finding* read — that one really is a
property of the text rather than of the shell, and it lands one line above the
child's.
