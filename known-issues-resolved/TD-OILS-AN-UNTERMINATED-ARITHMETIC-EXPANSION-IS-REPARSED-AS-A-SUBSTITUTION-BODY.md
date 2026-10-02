### TD-OILS-AN-UNTERMINATED-ARITHMETIC-EXPANSION-IS-REPARSED-AS-A-SUBSTITUTION-BODY. `echo $(( fi` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/parser.rs` — `resolve_subst_bail`, and the
`SubstBail` the lexer raises for it in `userspace/oils/src/lexer.rs`.

**What.** An unterminated `$(( … )` whose text happens not to parse as a
command list is reported as that text's grammar error instead of as the missing
paren:

| input | bash | osh |
|---|---|---|
| `echo $((` | ``line 1: unexpected EOF while looking for matching `)'`` | same ✅ |
| `echo a$(( b` | ``… matching `)'`` | same ✅ |
| `echo $(( fi` | ``… matching `)'`` | ``syntax error near unexpected token `fi'`` + echo |
| `echo $(( ]]` | ``… matching `)'`` | ``syntax error near unexpected token `]]'`` + echo |
| `echo $(( )` | ``… matching `)'`` | ``syntax error near unexpected token `)'`` + echo |
| `[[ x =~ $(( ]]` | ``… matching `)'`` + the conditional sequel | grammar error + the sequel |

The rows that already agree are the ones whose body happens to *be* a valid
command list, so the two paths coincide by accident.

**Why.** `SubstBail` exists because bash parses a `$( … )` body **as it reads
it** — `parse_comsub` runs a nested `yyparse` — so a body error comes out before
the missing `)` is ever noticed (``echo $(fi`` is ``near `fi'``, not ``matching
`)'``). But `parse_comsub` does not do that for `$((`. Its first act is:

```c
  /* Posix interp 217 says arithmetic expressions have precedence, so
     assume $(( introduces arithmetic expansion and parse accordingly. */
  if (open == '(')		/*)*/
    {
      peekc = shell_getc (1);
      shell_ungetc (peekc);
      if (peekc == '(')		/*)*/
	return (parse_matched_pair (qc, open, close, lenp, P_ARITH));
    }
```

(parse.y:4095). `parse_matched_pair` is a pure character scan — it never parses
commands, so there is no body error to come out first and the unterminated-paren
message stands. osh applies the bail to both.

**Fix (2026-08-07).** Suppressing the bail was the *entry point*, not the fix.
Measuring the rest of `$((` showed the two halves are separate bash mechanisms
and osh had merged them, so both were rebuilt:

1. **Parse time is extent only.** `parse_matched_pair (…, P_ARITH)` counts
   parens with `count` starting at 1 (the `$(`'s), which is why `$((` needs
   *two* `)`. It steps over `'…'`, `"…"`, `` `…` ``, `\X` and a nested
   `$( … )`, but **not** `${ … }` — that needs `P_ARRAYSUB|P_DOLBRACE`
   (parse.y:3929), so `echo $(( 1 + ${x:-)} ))` really does end at the `)`
   inside the brace. `Lexer::read_arith` is gone; the `$((` scan is now
   `read_balanced_body` with `command = false`, sharing `read_opaque_span`
   with the command scan and differing only where bash's flags differ.
2. **Classification happens at expansion time.** `param_expand`'s `LPAREN`
   case (subst.c:10580) strips the parens and asks `chk_arithsub`
   (subst.c:9487) whether the text balances; on failure it `goto comsub` and
   calls `command_substitute` — *the very call a backtick body makes*.
   `is_arith_expr` is that check, transcribed.
3. **A failed classification therefore defers the body.** That is the part
   that was missing: osh parsed the fallback body eagerly through
   `parse_cmdsub_body`, which both rewrote its `unexpected end of file` into
   ``near unexpected token `)' `` and marked it `fatal` (→ 127 under `-c`).
   bash never reads it until the word expands, so
   `if false; then echo $(( fi ) ); fi` is *silent* — measured, and matching
   `` `fi` `` rather than `$(fi)`. Modelled as a third `CmdSubBody` variant,
   `ArithFallback`, fed by a new lexer-side `SubBody` enum (the lexer must
   not depend on `ast`); it takes the backtick path in `interp.rs` — the
   plain `close_line - 1` line offset included, also measured — and prints
   back as `$( … )` in `unparse.rs`, which reproduces the source because the
   text still carries the `(` the scan counted.

Two smaller facts fell out and are fixed with it: `shell_getc (1)` at
`parse_matched_pair`'s loop head (parse.y:3705) deletes a `\<newline>` *before*
the scan sees the backslash, so `read_balanced_body` deletes it too when
`!command` (a command body keeps the pair for its re-lex to delete again) —
without that, `echo $((1+1)\<newline>)` ends `raw` on a newline, fails the `)`
suffix and is misread as a substitution. And `$[ … ]` has **no** fallback at
all (`echo $[ fi ]` is 0, `echo $[ ( echo a ) ]` an arithmetic error), so it
stays `Seg::Arith`.

**Tests.** `tests/corpus/arith-expansion-scan-and-classify.sh`, eight sections
covering each of the above.
