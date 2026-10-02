### TD-OILS-ESCAPED-METACHAR. Backslash-escaped glob/pattern metacharacters in *unquoted* words are treated as live metacharacters — 2026-07-20 — ✅ RESOLVED 2026-07-20 (glob/`case`/`[[ == ]]`/param-expansion/`=~` all fixed; only reserved-word suppression `\if` remains, tracked below)

**Where (root cause):** the two word lexers folded a backslash-escaped
character into a plain unquoted `Seg::Lit`, dropping the "this character was
quoted/literal" information. The glob/pattern machinery keys off a per-character
`EChar.quoted` flag (`interp.rs`, `push_chars`, `field_has_glob_meta`), but an
escaped char reached it tagged `quoted:false`, so an escaped `*`/`?`/`[` was
(wrongly) matched as a live metacharacter.

**Fix (2026-07-20):** a backslash-escaped character is semantically identical to
that same character single-quoted (`a\*b` ≡ `a'*'b`), so both lexers now emit an
escaped **non-alphanumeric** character as a one-char `Seg::Sq` — routing it
through the fully-tested single-quote path (literal, `quoted:true`, no
expansion/splitting/globbing) with zero new AST variants:
- `lexer.rs` `read_word_inner` (`'\\'` arm) — command words, `for` lists,
  `case`/`[[ ]]` subject and pattern words. Guarded by `ext_depth == 0` (see the
  extglob-group note below).
- `lexer.rs` `read_word_verbatim` (`'\\'` arm) — the `${…#pat}` / `${…%pat}` /
  `${…/pat/repl}` / `${…^pat}` pattern (and replacement) sub-lexer, preserving
  the existing `\&`/`\\` replacement special-case.
- `interp.rs` — new `Shell::expand_word_pattern` (quoted-preserving, no field
  splitting) + `glob_match_echars_ci` route `case`/`[[ == ]]` through
  quoting-aware matching; `param_trim`/`param_case`/`param_replace`/
  `glob_match_at` now take `&[EChar]` patterns and match via `glob_match_e`.
- Escaped **alphanumerics** are still folded into the plain literal so
  command-name recognition (`\ls` → `ls`) is unchanged — see the remaining item.
- Bonus fix: `echo \~` no longer performs tilde expansion (the escaped `~` is now
  literal, matching bash). The `=~` regex RHS was already fixed earlier.

See test `escaped_metachar_is_literal_in_patterns`.

**Still OPEN (narrow, low priority): reserved-word / keyword suppression.**
`\if true; then …` — bash treats `\if` as an ordinary **command** named `if`
(the backslash suppresses reserved-word recognition) and reports a syntax error
at `then`; osh still recognizes `\if` as the `if` keyword and runs the
conditional. This is intentionally *not* fixed by the change above: because the
escape of an alphanumeric is folded into the flattened `Seg::Lit` (so `\ls`→`ls`
command-name detection keeps working), keyword detection — which keys off the
same single flattened literal — cannot distinguish `\if` from `if`. A proper fix
would need to carry an "was-escaped" bit into reserved-word lookup only (not
command-name lookup), e.g. a dedicated `Seg::Escaped`/`WordPart::EscapedChar`
variant threaded through the ~15 `[Seg::Lit(name)]` detection sites, splitting
command-name (fold) from keyword (don't fold) handling. Deferred: escaping a
reserved word is a vanishingly rare idiom and the refactor touches core command
parsing. Also unaddressed (same low bar): an escaped metacharacter *inside* an
extglob group (`@(a\*b)`, `ext_depth > 0`), where the group body is accumulated
as one contiguous literal; only observable when `extglob` was enabled in a prior
parse unit (otherwise the whole line is a parse error either way — see TD-OILS8).
