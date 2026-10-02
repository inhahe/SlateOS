### TD-OILS1. `osh` `[[ … ]]` conditional gaps: `-r`/`-x` file tests approximated as "exists" — MOSTLY RESOLVED 2026-07-18 (`=~` regex match + quote-aware literal RHS implemented; only the permission-bit tests remain, gated on the slateos permission model)

**Where:** `userspace/oils/src/ere.rs` (Pike-VM ERE engine),
`userspace/oils/src/lexer.rs` (`read_word_regex`, `cond_depth`/`regex_next`),
`userspace/oils/src/parser.rs` (`parse_cond_primary` → `CondExpr::Regex`),
`userspace/oils/src/interp.rs` (`cond_regex`, `cond_unary` permission tests).

**What:** The bash conditional command `[[ … ]]` is implemented (string
`==`/`=`/`!=` with glob-or-literal RHS, `<`/`>` ordering, numeric
`-eq…-ge`, unary file/string tests, `!`/`&&`/`||`/`(…)`), and `=~` regex
matching now works:
- `=~` (POSIX ERE regex match) — **RESOLVED.** An in-tree linear-time
  Pike-VM/Thompson-NFA ERE engine (`ere.rs`, ReDoS-safe: no catastrophic
  backtracking) compiles the RHS pattern and matches the LHS. The lexer
  reads the `=~` RHS as one regex word (so `(`, `)`, `|`, `<`, `>` are
  literal metacharacters, not shell operators); the RHS still undergoes
  parameter expansion. On a successful match the `BASH_REMATCH` indexed
  array is populated (`[0]` = whole match, `[i]` = capture group `i`;
  unmatched optional groups become empty strings), and it is cleared on a
  non-match. A malformed pattern reports to stderr and yields false.
  **Quote-aware RHS — RESOLVED 2026-07-18.** `cond_regex` now builds the
  pattern via `regex_pattern_from_rhs`, which walks the RHS `Word`'s parts:
  unquoted `Literal`/dynamic (`$var`, `$(…)`) parts contribute live regex
  syntax, while single- and double-quoted parts (including an expanded
  `"$p"`) are backslash-escaped so their metacharacters match literally —
  so `[[ a.b =~ "a.b" ]]` matches only the literal `a.b`, `[[ axb =~ a.b ]]`
  still matches (regex `.`), and `p='a.b'; [[ axb =~ $p ]]` matches while
  `[[ axb =~ "$p" ]]` does not. Tests: `cond_regex_double_quoted_rhs_is_literal`,
  `cond_regex_single_quoted_rhs_is_literal`, `cond_regex_mixed_quoting`,
  `cond_regex_quoted_var_is_literal`.
- `-r` and `-x` file tests are approximated as "path exists" (`-w` is
  "exists and not read-only") because the host has no portable mode-bit
  check and the slateos permission model isn't wired into `osh` yet.
  **Proper fix:** query the real per-file permission bits once the
  slateos userspace permission API is available.

The remaining `-r`/`-x` approximation is not a correctness bug in the
implemented surface — it is an intentional grow-phase scope limit gated on
the slateos permission model, documented in the `interp.rs` module header.
No test depends on the deferred behavior.
