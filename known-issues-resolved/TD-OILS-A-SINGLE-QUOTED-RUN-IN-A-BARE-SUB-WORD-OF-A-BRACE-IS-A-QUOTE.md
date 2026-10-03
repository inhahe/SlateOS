### TD-OILS-A-SINGLE-QUOTED-RUN-IN-A-BARE-SUB-WORD-OF-A-BRACE-IS-A-QUOTE. `"${a['$(echo 1)']}"` leaves the substitution unexpanded — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/parser.rs` — the sub-words of a `${ … }` that are
read *bare* (`Quoting::Bare`): a `[ … ]` subscript, a substring offset/length, a
pattern, a replacement. A `' … '` in one becomes a `WordPart::SingleQuoted`, so
nothing inside it is parsed. The operand after `:-`/`:+`/… is not affected — it
inherits the enclosing quoting, and
`tests/corpus/a-single-quoted-run-in-a-double-quoted-brace-operand-is-an-extent-not-a-quote.sh`
covers it.

**Reproduce.**

```sh
a=(A B C)
echo "[${a['$(echo 1)']}]"
echo [${a['$(echo 1)']}]
```

| | bash 5.2.37 | osh |
|---|---|---|
| both lines | `'1': syntax error: operand expected (error token is "'1'")` | `'$(echo 1)': syntax error: operand expected (error token is "'$(echo 1)'")` |

**What bash does.** A subscript is not parsed when the word is; it is carved out
as text and expanded at run time, and *which* expander gets it decides whether a
`'` quotes. An **index** goes to `expand_arith_string (exp,
Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB)` (arrayfunc.c:1354) — `Q_DOUBLE_QUOTES`, so
a `'` is an ordinary character, the `$( … )` inside it *is* expanded, and the
quotes survive into the arithmetic as text. An **associative key** goes to
`expand_subscript_string (sub, 0)` (arrayfunc.c:1145) — quoting 0, so a `'` is a
real quote and is removed; `declare -A m; m['x y']=K; echo "${m['x y']}"` gives
`K` in osh already, by the other route.

So the same text takes two quoting modes depending on the *array's* type, which
is not known until run time. osh currently picks the associative one at parse
time and so gets the index case wrong; it happens to agree for `${a['1']}`
because a `SingleQuoted` part with no expansion in it re-prints with its quotes.

**The fix — done.** A run now carries *both* readings, because which one applies
is not knowable at parse time. `WordPart::SingleQuoted` gained a third field,
`parts: Option<Vec<WordPart>>` — `text` is the quote's reading, `parts` the
arithmetic one, `None` everywhere a run cannot reach arithmetic.
`parser::word_subscript_from_source_at` builds a subscript exactly as before and
then `attach_subscript_reads` fills each top-level run's `parts` by re-reading
its interior with `Quoting::as_unread()` — whatever the quoting around it was,
bash's `parse_matched_pair` read *nothing* between a `'` and its mate, so a
`$( … )` in there is `CmdSubBody::Unread` and its diagnostic is a runtime one.
The re-read is *tolerant*, because the interior can be cut short of a quote it
opened (`'"'` is a `"` with no mate, which bash's expander runs to the end of the
string rather than complaining).

`Shell::arith_string_parts` is the expander side: it copies the two `'` out as
characters and recurses into `parts` between them, dropping the closing one when
a read gave up, since that consumes the rest of the string it was walking — the
subscript, closing quote included. Measured with `c='A${a['"'"'$(fi)'"'"']}B'`,
`${c@P}` ends `': syntax error: operand expected (error token is "'")`, one
quote and not two.

Four call sites got the new builder (the `ArrayAssign` index, `try_assignment`'s
index, `parse_array_elem`'s keyed index, and `${a[sub]}`), `parse_slice_bounds`
attaches to both bounds, `spanning_subscript_assignment` attaches to the index
it builds from segments (`a['$(fi)']=X` takes that path), and the two runtime
subscript readers (`sub_word`, the `unset` element path) moved to it as well.

The gobbler's walk now threads a `dquoted` flag: at the top level its `quoted` is
0, the `'` row is reached and the run goes by unread; inside `" … "` its `quoted`
is `"`, that row is never reached, and it reads straight through — so
`Shell::gobbled_subs` descends into `parts` only when `dquoted`.
`unparse::walk_parts_in` descends unconditionally (the sentinel swap must reach
inside the quotes) while `nested_parts_mut` deliberately has no row for it, since
`extract_dollar_brace_string` steps over a whole subscript.

**Two divergences this measured but did not fix** — both predate the change and
are filed separately: `[[ -v "a['1']" ]]` (quote removal on the `-v` operand's
subscript) and `(( a["'"$s"'"] ))` (CTLESC-quoted quotes in an already-expanded
arithmetic string).

**Tests.** `tests/corpus/a-single-quote-in-a-subscript-is-not-a-quote.sh` (64
rows: the substitution running, side effects, parameters, backquotes, a nested
`" … "`, backslash-under-double-quoting, an unterminated quote, no tilde
expansion, the unparsable body's runtime diagnostic in bare / double-quoted /
`set +B` form, a substring bound, an assignment subscript, associative keys
taking the *other* reading, runtime subscripts through `${!s}` / `unset` /
`printf -v` / `read`, `declare`'s own operand, and `${x@P}`), plus the two rows
restored to
`tests/corpus/the-brace-scanner-reads-the-command-substitutions-a-single-quote-hid.sh`
— `echo "[${a['$(fi)']}]"` and `echo "[${y:'$(fi)':1}]"`, which bash answers
from `brace_gobbler` with the *word's* remainder (`` `fi)']}]"' `` and
`` `fi)':1}]"' ``).
