### TD-OILS-A-COMPOUND-LITERAL-AFTER-THE-COMMAND-WORD-DOES-NOT-NAME-THE-TOKEN-THAT-SURPRISED-IT. `echo n=(x y)` says `array assignment is only valid before the command word` where bash names `` `(' `` and echoes the line — 2026-08-05 — ✅ FIXED

**Where:** `userspace/oils/src/parser.rs` — `parse_simple`'s `Tok::ArrayAssign`
arm, the `seen_word && !is_decl_operand` guard, which raises a rule of its own
rather than letting the `(` be reported as the unexpected token it is.

**What.** bash has no rule about compound literals after the command word: the
word is just a word, and the `(` after it is a parser surprise like any other,
reported by the shared `near unexpected token` machinery — which also echoes
the offending line. osh short-circuits with a bespoke message and no second
line. Measured (both forms diverge identically, so this is not about the
subscript):

```text
                         bash                                 osh
echo n=(x y)             syntax error near unexpected token   syntax error: array
                         `('  /  `echo n=(x y)'               assignment is only valid
                                                              before the command word
echo n[1]=(x y)          the same, echoing `echo n[1]=(x y)'  the same message
```

**Proper fix.** Do not raise a rule at all. When an `ArrayAssign` token is
reached after the command word and is not a declaration builtin's operand, the
parser should re-report it as the `(` that surprised it, through the same path
that already produces `` syntax error near unexpected token `(' `` plus the
source line (see the `-c` cases asserted around `interp.rs:51022`). The token
carries the name and the literal, so the line can be reprinted from source.

**Impact.** Cosmetic — both shells fail the parse and a script does not survive
either. But a script that greps its own stderr, or a user reading it, sees a
message bash never prints.

**Fixed 2026-08-05.** `parse_simple`'s guard now returns `` syntax error near
unexpected token `(' `` instead of naming a rule, which is all that was needed:
`Shell::format_parse_error` already echoes the offending source line after any
message containing `syntax error near `, and already tags an `eval` body's
errors `eval: line N:` and echoes the eval *string* rather than the script line.

**Corpus:**
`a-compound-literal-after-the-command-word-names-the-token-that-surprised-it.sh`
— which reaches the shape repeatedly through `eval` (confined, status 2), since
a bare one kills the script and so has to go last.
