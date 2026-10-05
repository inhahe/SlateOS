### TD-OILS-A-BACKQUOTE-IN-A-QUOTED-SUBSCRIPT-IS-A-PARSE-ERROR-WHERE-BASH-EXPANDS — 2026-08-14 — ✅ FIXED 2026-08-14 (in-scope half; see the scope note at the end)

**Where:** the `' … '` interior parse of an arithmetic fragment —
`attach_subscript_reads` (`userspace/oils/src/parser.rs`) and the lexer path
behind it.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
echo "[${arr['x`fi']}]"
echo TAIL
```

| | |
|---|---|
| bash | ``bad substitution: no closing "`" in `fi'`` at line 2, then `TAIL` |
| osh, before | ``unexpected EOF while looking for matching `` ` ``'`` at line 4 — the script never runs |
| osh, now | identical to bash |

osh turned a runtime diagnostic into a *parse* error, so the whole script was
rejected. bash's parser stops at the `'` and resumes at its mate, so the
backquote inside is text as far as any parse is concerned; it is met only by
`param_expand`'s own `string_extract (…, SX_REQMATCH)` at expansion time
(subst.c:11269), which names `string + t_index` — the text from the backquote on.

**The fix.** Three parts:

- `Lexer::read_word_verbatim`'s `` ` `` arm used a bare `?`, which let the
  `LexError` escape as a parse error. It now converts to an `Unclosed::Backquote`
  segment via `unclosed_seg`, exactly as the `$` arm does for an unmatched `${`.
  This is the part that stopped the script being rejected.
- `Unclosed::Backquote` gained a `text` field, because its `%s` is
  `string + t_index` and not `string`: the report runs from the backquote to the
  end of the **fragment**, whereas `src` is also what `part_src`/`parts_src`
  re-print and so cannot be widened in place.
- `name_unclosed_after_the_fragment` (`parser.rs`) widens that `text` with the
  fragment tail past the run's interior — the run's own closing quote and
  whatever follows it — mirroring what it already did for `BadSubst`.

**Verified.** `userspace/oils/tests/corpus/an-unmated-backquote-in-a-quoted-subscript-is-met-at-expansion-and-not-by-a-parse.sh`
is byte-identical to bash 5.2.37, as are probes `build/pr28.sh` and
`build/pr29.sh`. Full sweep green.

**SCOPE: one residue is out of frozen scope (§305) and is deliberately left
unfixed.** Where the unmated backquote sits inside a *nested double quote* within
the run — `build/pr30.sh` d2, `echo "[${arr['x"`fi"']}]"` — bash reports
``no closing "`" in `fi"'`` and osh reports ``no closing "`" in `fi"``: osh is one
trailing `'` short. Everything else matches, including the script surviving, the
exit status and all other output. The cause is known:
`name_unclosed_after_the_fragment` visits only the run's own top level and does
not descend into a nested `DoubleQuoted` part (`crate::unparse::nested_parts_mut`
would give the descent; note its `SingleQuoted { .. }` arm returns `Vec::new()`,
so it can only supplement the outer loop, not replace it).

This is **the exact substring an error message echoes**, which design-decisions
§305 names as out of scope: nothing SlateOS runs will ever depend on it. Fix it only
if it turns up as part of something that does. The in-scope half of this
entry — a whole script being rejected where bash runs it — is closed.

**Fixed by:** the corpus case named above, plus `lexer.rs` (`Unclosed::Backquote`
`text` field, `read_word_verbatim`'s `` ` `` arm), `interp.rs`
(`Unclosed::Backquote` report) and `parser.rs`
(`name_unclosed_after_the_fragment`).
