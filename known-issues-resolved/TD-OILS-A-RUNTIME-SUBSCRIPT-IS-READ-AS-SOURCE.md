### TD-OILS-A-RUNTIME-SUBSCRIPT-IS-READ-AS-SOURCE. `unset 'a[$(fi)]'` succeeds silently where bash reports and abandons the command — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs` — the three readers that turn an
already-expanded `name[sub]` string back into a `Word`:
`Shell::element_is_set` (~27316), `Shell::sub_word` (~27793) and
`Shell::unset_element_unchecked` (~43480). All three call
`crate::parser::word_verbatim_from_source(…, Quoting::Bare)`.

**What is wrong.** `Quoting::Bare` says *this text is source a parser read*, and
for these three it never is. The subscript arrives as the **value** of an
already-expanded word — `unset 'a[$(fi)]'` hands `unset` the eight characters
`a[$(fi)]`, quoted so that nothing in them was read — and bash re-reads it at
expansion time (`array_expand_index` → `expand_arith_string (exp,
Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB)`, arrayfunc.c:1354-1358, for an indexed
array; `expand_subscript_string (sub, 0)`, arrayfunc.c:1145, for an associative
key). Two things follow, and osh gets both wrong.

**Reproduce (1) — a `$( … )` in a runtime subscript.** With `declare -a a=(0 1 2)`,
each of these, run in a subshell so the abandonment is visible:

| construct | bash 5.2.37 | osh |
|---|---|---|
| `unset 'a[$(fi)]'` | two-line `command substitution:` syntax-error report; command abandoned, no `rc=` printed, `after=1` | `rc=0`, `after=0`, no report |
| `[[ -v 'a[$(fi)]' ]]` | same | `rc=1`, no report |
| `printf -v 'a[$(fi)]' X` | same | `$(fi): syntax error: operand expected (error token is "$(fi)")` |
| `[ -v 'a[$(fi)]' ]` | same | `rc=1`, no report |
| `read -r 'a[$(fi)]' </dev/null` | same | `$(fi): syntax error: operand expected` |

bash's report and abandonment are `xparse_dolparen`'s: `parse_string` returns
negative and it does `jump_to_top_level (-nc)` (y.tab.c:6639-6646). osh already
models that for a `${ … }` in unread text (`CmdSubBody::Unread` →
`ExtentRead::Aborted`); the subscript readers simply never produce an `Unread`
body, because `Quoting::Bare` tells the lexer the text was parsed. The backquote
form already matches — `[[ -v 'a[`fi`]' ]]` reports from the child and exits 0 on
both — because a backquote body is never parsed by the extent scan.

**Reproduce (2) — `$'…'` in a runtime subscript is not translated.**
`expand_word_internal` translates no `$'…'`, and nothing re-reads a runtime
subscript the way `extract_heredoc_dolbrace_string` re-reads a here-document's
pattern (see TD-OILS-A-BRACE-PATTERN-OPERAND-IS-PARSED-EAGERLY). So with
`declare -A m; m[$'a\tb']=TAB`:

| construct | bash | osh |
|---|---|---|
| `s="m[\$'a\tb']"; [[ -v $s ]]` | false — the key is the six characters `$'a\tb'` | true |
| `s='m[${v#$'"'"'a\tb'"'"'}]'; [[ -v $s ]]` with `v=$'a\tb'` | true — the pattern is untranslated, so `${v#…}` trims nothing and the key is the real-tab one | `m: bad array subscript`, false — osh translates, the trim empties the key |
| `t="a[\$'1']"; [[ -v $t ]]` | `$'1': syntax error: operand expected (error token is "$'1'")` | same message with the `$` eaten: `'1': … (error token is "'1'")` |

Note the second row: the *pattern* of a `${ … }` nested in a runtime subscript is
untranslated too, unlike the pattern of a `${ … }` in a here-document body. Quote
*removal* is already right on both (`m['a']`, `m["a"]`, `m['x y']` and `m[x y]`
all agree), so this is the ANSI-C form alone.

**Proper fix.** A fifth `parser::Quoting` state. The four existing ones cover
source text and here-document/`@P`/`PS4` text; what is missing is *unread text
with bare quoting and no ANSI-C at any depth* — a string handed straight to
`expand_word_internal`, which is what all three readers have. It cannot be
`Quoting::Unread` (that is double-quoted, and would break `m['x y']`) and it
cannot be `Quoting::BareUnread` (that restores the ANSI-C translation, which is
right for a here-document's pattern and wrong here), so `Quoting::as_pattern`
must map the new state to itself.

That in turn wants `Lexer::dq_context` renamed to what it actually gates. It is
read in exactly one place — `read_dollar`'s `quote_form = !in_dquote &&
!self.dq_context` — so it means "`$'…'`/`$"…"` are not quote forms in this scan",
not "this text is double-quoted". Renaming it positively (`ansi_c_quote`) makes
the fifth state expressible, and the lexer entry points that currently take a
bare `unread: bool` (`lex_word_verbatim_opts`, `lex_replacement_verbatim`,
`tokenize`, `lex_operand_in_dquote`) should take the pair instead.

**Fixed** exactly as sketched. The two questions the four lexer entry points
were being asked with one `bool` are now asked with two, as a `lexer::ReadCtx
{ unread, ansi_c }`, and the three named values name the three kinds of text
oils reads:

| `ReadCtx` | what it is | who passes it |
|---|---|---|
| `SOURCE` | text a parser read | the parse, `@P`, `PS4` |
| `HEREDOC_FRAGMENT` | unread, but re-read by `extract_heredoc_dolbrace_string` | a `${ … }` pattern in a here-document body |
| `VALUE` | a string handed straight to `expand_word_internal` | a here-document body, a runtime subscript |

`Lexer::dq_context` became `ansi_c_quote` (positive sense — it gates exactly
`read_dollar`'s `quote_form = !in_dquote && self.ansi_c_quote`), and
`Lexer::apply_ctx` sets both fields from a `ReadCtx` so every entry point
starts the same way. `parser::Quoting` grew its fifth state, `Quoting::Runtime`,
whose `read_ctx()` is `VALUE` and whose `as_pattern()` is itself — that last
part is what makes the nested-`${ … }` row above come out right, since a
pattern inside a runtime subscript is no more translated than the subscript is.
The three `interp.rs` readers now pass `Quoting::Runtime`.

**Corpus:** `a-runtime-subscript-is-unread-text-and-reads-its-own-comsub.sh` —
the five name-operand builtins in subshells with `after=$?`, the backquote
contrast, the untranslated `$'…'` at both depths (with the resulting key set
printed, so the `$a\tb` that quote removal leaves behind is visible), the four
quote-removal rows, the arithmetic error tokens, and a nameref target.

**Still `Quoting::Bare` on purpose**, because nothing measured says otherwise
yet: `interp.rs`'s other runtime word readers at ~6670 (`BASH_ENV`), ~26437,
~26472 and the `parse_braced_param(name, opts, Quoting::Dquote)` pair at ~30085
and ~30440. Each wants its own measurement before it moves.
