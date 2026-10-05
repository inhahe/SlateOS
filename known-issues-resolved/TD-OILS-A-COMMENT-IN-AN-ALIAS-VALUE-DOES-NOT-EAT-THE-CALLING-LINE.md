### TD-OILS-A-COMMENT-IN-AN-ALIAS-VALUE-DOES-NOT-EAT-THE-CALLING-LINE. `alias A='echo hi #c'` plus `A there` still echoes `there` — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/lexer.rs` — the alias-value sub-lex
(`tokenize_alias_body` via `expand_aliases_inner`), which drops a comment at the
end of a value and then goes on emitting the calling line's tokens.

**What.**

```text
shopt -s expand_aliases
alias A='echo hi #c'
A there
echo next

bash: hi          — `there` was swallowed by the comment
      next
osh : hi there
      next
```

Same shape at a here-document seam, where it changes which token the grammar
error names:

```text
alias B='cat <<'
alias A='B #c'
A E

bash: syntax error near unexpected token `newline'   — the comment ate ` E' too
osh : syntax error near unexpected token `E'
```

**Why.** bash's comment test is in `read_token` and its body is
`discard_until ('\n')`, which reads through `shell_getc` — and `shell_getc` pops
the pushed alias string transparently when it runs dry (parse.y's `pop_string`).
So a comment opened inside an alias replacement keeps eating in the *calling*
line, up to that line's real newline. osh lexes the replacement as a text of its
own, so the comment can only reach the end of the value; the calling line's
remaining tokens were lexed separately and survive.

Both shells agree that the `<<` above has no delimiter — osh stops the
`pop_string` chain on a comment (`DelimAt::None` → `Dangle::Sealed`) exactly as
it does on a separator — so the status and the fact of the error match. Only the
token named differs, because osh still has the calling line's `E` to name.

**Proper fix.** The same textual splice that
TD-OILS-AN-ALIAS-SPLICED-OPERATOR-DOES-NOT-EXTEND-INTO-THE-CALLING-LINE needs:
lex `value ++ rest-of-this-text-after-the-alias-word` as one text and the comment
runs on by itself. Short of that, `tokenize_alias_body` would have to report
"this text ended inside a comment that never met a newline", and
`expand_aliases_inner` would then drop the calling text's tokens up to (not
including) the next `Tok::Newline` — which is `discard_until('\n')` spelled at
the token level, and correct as far as it goes, but it is one more patch on the
token-level model rather than a fix of it.

**As fixed.** By the textual splice written up under
TD-OILS-AN-ALIAS-SPLICED-OPERATOR-DOES-NOT-EXTEND-INTO-THE-CALLING-LINE, with no
code of its own: the comment and the calling line are one text, so the ordinary
comment scan runs off the end of the value and on to that line's real newline —
`discard_until('
')` reproduced by construction rather than simulated at the
token level.

**Pinned by** the corpus case `nothing-lexical-stops-at-the-alias-seam.sh`, whose
`=== a comment opened in the value eats the calling line` section is both shapes
above.
