### TD-OILS-EXTGLOB-UNGATED. Extglob patterns are always recognised, where bash gates them on `shopt -s extglob` — 2026-07-28 — ✅ RESOLVED 2026-07-28

**Where:** `userspace/oils/src/lexer.rs` — `read_word_inner`'s `ext_depth`
tracking treats `?(`, `*(`, `+(`, `@(` and `!(` as an extglob group whenever it
sees them; `userspace/oils/src/ere.rs` / the pattern matcher likewise. Nothing
consults a shell option.

**What:** bash recognises extglob only when the `extglob` shopt is on, and the
decision is made by the *lexer*, at parse time — so `shopt -s extglob` on the
same line as a use has no effect. With it off, `?(` is an ordinary `?` word
followed by a `(` metacharacter, which is usually a syntax error:

```sh
echo ?(a)                      # bash: syntax error near unexpected token `('   osh: ?(a)
f?() { echo hi; }; "f?"        # bash: hi (defines the function `f?')          osh: syntax error near `}'
```

The second line is the more damaging one: because osh swallows `?(` as an
extglob group, a perfectly ordinary function definition whose name ends in a
glob character cannot be parsed. The same applies to `*`, `+`, `@` and `!`.

Note also that bash defaults `extglob` **on** for interactive shells and off
for non-interactive ones, so a script's behaviour differs from what the same
text does when pasted at a prompt.

**Proper fix:** thread the `extglob` option into the lexer (it already receives
shell state for alias expansion) and gate the `ext_depth` entry on it, and gate
pattern *matching* on the same flag so that a literal `@(a|b)` string compares
as itself. Then `shopt -s extglob` must take effect only for input lexed after
it runs, matching bash. Add a corpus case covering both readings and the
function-name spelling above.

**Fixed:** the option now travels with the source instead of being consulted at
run time. `lexer::LexOpts` is a small `Copy` struct (`extglob`) threaded through
every entry point that turns text into tokens — `tokenize`, `tokenize_spanned`,
`tokenize_deferred`, `tokenize_spanned_strict`, `expand_aliases`, `parse_opts`,
`parse_with_aliases`, `parse_cmdsub_body`, `parse_braced_param` — and stored on
`Parser` so a body re-lexed *during* parsing (`$( … )`, `<( … )`, a `${…}`
containing either) is read the same way as its enclosing text. `read_word_inner`
enters `ext_depth` only when the option is on; matching was already gated, so
only lexing changed.

Read-parse-execute order is reproduced exactly. `Tokenized` now carries a
`offsets` vector — the character offset each token starts at — and
`IncrementalParser` keeps the source, its line base, and the options the tokens
were lexed under. `next_unit` takes the current options; when they differ it
throws away the unread tail and lexes it again from `offsets[pos]`, shifting the
fresh line numbers by the newlines before that offset. So `shopt -s extglob`
governs the *next* unit and never its own line — `shopt -s extglob; echo @(a)`
is still a syntax error, exactly as in bash, while the same two commands on
separate lines work. `Shell::lex_opts()` samples the shopt immediately before
each parse, and the incremental driver re-samples it per unit.

bash's one exception is reproduced too: the pattern operand of a `[[ … ]]`
`==`/`!=`/`=` always lexes extglob, whatever the option, so `[[ abc == @(abc|x)
]]` matches in a default shell while `[[ @(a) == b ]]` and `[[ -n @(a) ]]` are
syntax errors. `emit_word` sets a one-shot `extpat_next` flag when it emits one
of those operators inside `[[ … ]]`, and the next word consumes it. `case`
patterns get no such exemption, matching bash.

Covered by `tests/corpus/extglob-lexing.sh` (the default reading, the negated
subshell `!(cmd)`, the `[[ ]]` exception, the deferred effect of `shopt -s`,
inheritance into a `$( )` body, and `shopt -u` restoring the metacharacter) and
by the `extglob_in_test_and_case` unit test, which now asserts the same-line
form is a syntax error. Supersedes the older TD-OILS-EXTGLOB-PARSE entry below.

**Left over:** bash's `[[ … ]]` diagnostics name the partial word `@(a` where
osh names the `(`, and bash adds an `unexpected token `X'` clause when a binary
operator was expected. Split out as TD-OILS-COND-TOKEN-SPELLING.
