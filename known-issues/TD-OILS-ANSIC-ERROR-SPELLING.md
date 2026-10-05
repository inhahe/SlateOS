### TD-OILS-ANSIC-ERROR-SPELLING. A syntax error spells an ANSI-C word the way `declare -f` would, not the way it was written — 2026-07-28 — OPEN (diagnostic wording only)

**Where:** `userspace/oils/src/parser.rs` `token_display`, which renders the
offending word with `crate::unparse::word_src` — the same renderer
`declare -f` uses.

**What:** bash names the offending word in a syntax error by its *source*
spelling, so `[[ a $'q\tr' b ]]` is reported ``near `$'q\tr''``. osh reports
``near `'q<TAB>r''`` (a literal tab inside single quotes), because the lexer
decodes an ANSI-C string at scan time and keeps only the decoded bytes, and
`word_src` then re-quotes them. Every other quoting — `'<'`, `"<"`, `\<`,
`"a b"`, a word with an unexpanded `$x` — already matches bash exactly, and
that is what `tests/corpus/syntax-error-token.sh` covers.

The two renderings are not a single bug with one right answer: `declare -f`
*should* re-quote, and does so in bash too. Verified against bash 5.2.37 —
`f() { echo $'q\tr'; }; declare -f f` prints `echo 'q<TAB>r'`, i.e. bash
itself drops the `$'…'` spelling when printing a function body. So
diagnostics want the verbatim source and the printer wants the re-quoted
form; osh currently has only the latter.

**Proper fix:** keep the ANSI-C source spelling alongside the decoded bytes,
mirroring what `backtick_src` does for `` ` … ` `` — a `src: Option<String>`
on `Seg::Sq` / `WordPart::SingleQuoted`, plus a verbatim word renderer used
*only* by `token_display` (never by `declare -f`, which must keep
re-quoting). Deferred: it touches the lexer, the AST and two renderers to fix
the wording of one error message, and no behaviour — exit status, what runs,
and the source-line echo — differs today.

**Reproduce:**

```sh
( eval "[[ a \$'q\tr' b ]]" )
# bash: syntax error near `$'q\tr''    osh: syntax error near `'q<TAB>r''
```
