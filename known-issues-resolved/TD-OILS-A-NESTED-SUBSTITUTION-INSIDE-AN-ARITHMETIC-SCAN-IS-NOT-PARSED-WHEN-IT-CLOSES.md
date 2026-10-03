### TD-OILS-A-NESTED-SUBSTITUTION-INSIDE-AN-ARITHMETIC-SCAN-IS-NOT-PARSED-WHEN-IT-CLOSES. `echo $(( 1 + $(fi) ))` evaluated instead of failing — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::read_opaque_span`'s nested
`$( … )` arm, reached from `read_balanced_body` with `command = false`.

**What.** bash's `P_ARITH` scan does *not* treat a nested `$( … )` as opaque
text: `parse_matched_pair` calls `parse_comsub` for it (parse.y:3927), which is
a real nested `yyparse`. So a body that will not parse is a **fatal syntax error
in the enclosing unit**, found while the enclosing line is still being read —
long before any arithmetic is evaluated:

```text
echo $(( 1 + $(fi) ))   bash: eval: line 1: syntax error near unexpected token `fi'   rc=1
                        osh : line 1: 1 +  : syntax error: operand expected …          rc=1
echo $(( "$(fi)" ))     bash: eval: line 1: syntax error near unexpected token `fi'   rc=1
                        osh : 0                                                        rc=0
```

osh gets the *unclosed* forms right — `echo $(( $(fi` matches bash, because the
bail path fires — so this is specifically the case where the nested `$(` finds
its `)`. Then `read_opaque_span` skips it as a span, `is_arith_expr` sees a
balanced expression, and the whole thing becomes `Seg::Arith`; the body is never
parsed and its error never happens. The second row is the worse one: osh prints
`0` and succeeds where bash refuses to run the command at all.

**Proper fix:** parse the nested `$( … )` body during the scan, exactly as the
command scan already does, and raise its error as fatal. The extent-finding and
the parse are separate concerns in `read_balanced_body` today; the nested-`$(`
arm needs to do both regardless of `command`. Note this is bash's *only*
exception — `` ` ` ``, `'…'`, `"…"` and `${…}` stay opaque to the arithmetic
scan (see
TD-OILS-AN-UNTERMINATED-ARITHMETIC-EXPANSION-IS-REPARSED-AS-A-SUBSTITUTION-BODY).

**Fixed** as described. The parse itself belongs to `parser`, which the lexer
must not depend on, so the scan *records* each nested body in a new
`Lexer::arith_comsubs: Vec<CmdSubSpan>` and the parser drains it: `Seg::Arith`
gained a third field and `SubBody::ArithFallback` a payload — **both**, because
the eager parse happens during the scan and so before the `chk_arithsub`
classification that tells the two apart (`echo $(( echo $(fi) ) )` falls back to
a command substitution *and* still dies on the `fi`). `parser::seg_to_part` then
runs `parse_cmdsub_body` on each and throws the result away, which is exactly
what bash keeps (`APPEND_NESTRET` saves only the text; the body is read again at
expansion).

Every construct whose text is *copied for a re-lex* takes the list out of the
way first (`read_subst_body`, `read_dollar_brace`, and the nested `$(` inside a
double-quoted span), so a body's own substitutions are parsed once, with that
body, and not a second time out here — otherwise
`echo $(( $(echo "$(( $(fi) ))") ))` would parse the innermost body twice.
`parser::map_arith_comsubs` renumbers the recorded close lines alongside
`Seg::CmdSub`'s, so an error in one is blamed on the enclosing source's line.

Covered by `tests/corpus/arith-scan-parses-a-nested-substitution-in-place.sh`.
