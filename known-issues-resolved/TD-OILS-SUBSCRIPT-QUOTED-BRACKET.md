### TD-OILS-SUBSCRIPT-QUOTED-BRACKET. `osh` lexer chokes on a quoted `]` inside a `${name[...]}` subscript — FIXED 2026-07-20

**What:** When an associative-array key contains a literal `]` and is *read
back* with the key quoted inside the expansion — `"${h["with]bracket"]}"` or
`"${h['with]bracket']}"` — osh's lexer fails with `unexpected EOF while looking
for matching '"'` (or `'`). bash accepts it: the quoted `]` inside the subscript
is not the subscript terminator. The *assignment* side is fine —
`declare -A h=(["with]bracket"]=1)` stores and `declare -p` round-trips the key
correctly (fixed alongside the quoted-key array-literal support). Only the
retrieval expansion with a nested-quoted `]` trips the lexer.

**Where:** `userspace/oils/src/lexer.rs` — the `${...}` scanner treats the first
unquoted-looking `]` as the subscript close and does not track quote state
*inside* the subscript, so a quoted `]` is mistaken for the terminator and the
trailing `"` is left unbalanced. Also relevant: `parser.rs`
`matching_subscript_close` / `split_name_subscript`.

**Repro:** `osh -c 'declare -A h=(["with]bracket"]=1); echo "${h["with]bracket"]}"'`
→ lexer EOF error; bash → `1`.

**Proper fix:** make the `${...}` subscript scanner quote-aware — when scanning
for the closing `]` of a `name[...]` subscript, skip over single/double-quoted
runs so a quoted `]` is not treated as the terminator. Very low value: a literal
`]` inside an associative key is exotic, and the assignment/`declare -p` paths
already work.

**Fix (2026-07-20):** the actual culprit was **not** the lexer but
`matching_subscript_close` in `parser.rs` (called from `split_name_subscript`
when parsing a captured `${…}` body). `read_dollar_brace` already captured the
raw body (`h["a]b"]`) correctly with its quotes intact; the *parser's*
subscript-close scanner then walked it counting only `[`/`]` nesting, so it
stopped at the `]` *inside* the quotes and split the subscript to `"a` — an
unbalanced quote that tripped the re-lexer (`word_verbatim_from_source`) with the
observed `unexpected EOF … matching '"'`. Made `matching_subscript_close`
quote-aware: it now skips single- and double-quoted runs (honoring `\` in
double quotes) and backslash escapes while tracking `[`/`]` depth, so a quoted
`]` — or a *balanced* `[...]` inside the quotes (`${h["a[b]c"]}`) — is no longer
mistaken for the terminator. Verified `${h["with]bracket"]}`, `${h['a]b']}`, and
`${h["a[b]c"]}` all resolve; plain keyed and indexed reads unchanged. Regression
test: `assoc_read_quoted_bracket_in_subscript` (700 oils tests pass, clippy
clean).
