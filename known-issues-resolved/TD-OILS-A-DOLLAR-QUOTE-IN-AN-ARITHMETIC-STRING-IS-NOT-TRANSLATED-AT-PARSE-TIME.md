### TD-OILS-A-DOLLAR-QUOTE-IN-AN-ARITHMETIC-STRING-IS-NOT-TRANSLATED-AT-PARSE-TIME. `echo $(( $"a" ))` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/lexer.rs` — whatever reads the body of a `$(( … ))`
/ `$[ … ]`. osh hands the raw text on with the `$'…'` and `$"…"` still in it.

**What:** the two dollar-quotes are translated by the *parser*, not the
expander: `parse_matched_pair` (parse.y:3854, 3898) rewrites them in place as it
reads the matched pair, so by the time arithmetic sees the string they are gone.
The arithmetic body is read without `P_DQUOTE`, which picks these arms:

* `$'…'` → `ansiexpand`, then `sh_single_quote` (parse.y:3884) — the result is
  re-quoted, so `$'5'` becomes `'5'` and a single-quoted run is *never* valid
  arithmetic. `$'a\'b'` comes back as `'a'\''b'`. So a `$'…'` in an arithmetic
  string is always an error in bash too; only the text of the error differs.
* `$"…"` → `locale_expand`, then (with the default `singlequote_translations`
  off) `sh_mkdoublequoted` (parse.y:3919) — *not* re-quoted, so the translation
  is rescanned as arithmetic. `$"a"` is the variable `a`.

**Repro** (bash 5.2.37 left, osh right):

| input | bash | osh |
|---|---|---|
| `echo $(( $'a' ))` | ``'a' : syntax error: operand expected (error token is "'a' ")`` | the same with `$'a'` for `'a'` |
| `echo $(( $'a\'b' ))` | error token ``'a'\''b'`` | error token `$'a\'b'` |
| `a=5; echo $(( $"a" ))` | `5` | ``$a : syntax error: operand expected`` |
| `echo $(( $"a" ))` | `0` | the same syntax error |
| `echo $(( ${x:-$'5'} + 0 ))` | ``'5' + 0 : syntax error…`` | `5` |

osh already evaluates a bare `"a"` inside `$(( … ))` correctly (`echo $(( "a" ))`
prints `5`), so the whole of the `$"…"` case falls out of doing the translation.

**Proper fix:** translate `$'…'` and `$"…"` where the arithmetic body is *read*,
with parse.y's re-quoting rule — `sh_single_quote` for `$'…'`, the double-quoted
form for `$"…"` — rather than leaving them for the arithmetic evaluator, which
by then cannot tell them from text the writer quoted themselves.

**Fixed 2026-08-07, in three steps.** The first was
`Lexer::read_opaque_span`, which now translates both forms in every grouping
construct (commit "a dollar-quote inside a grouping construct is translated at
parse time"); that closed rows 1–4. Row 5 — a `$'…'` in a `${ … }` body inside
arithmetic — took two more, and the interesting part is that only *one* of them
is about translation at all.

**The other half of row 5 was not the dollar-quote.** Measuring first showed
`echo $(( ${x:-'5'} ))` — plain quotes, no `$` — diverging the same way: bash
errors on `'5'`, osh printed `5`. So osh was doing quote *removal* on a
single-quoted run in an arithmetic `${ … }` operand. bash does not: it expands
an arithmetic string with `Q_DOUBLE_QUOTES|Q_ARITH` (subst.c:8134), and
`expand_word_internal`'s `'` arm is `if (quoted & (Q_DOUBLE_QUOTES|
Q_HERE_DOCUMENT)) goto add_character` (subst.c:11577) — a `'` in the operand is
an ordinary character handed to the evaluator, which rejects it. Fixed by
threading the quoting through `parser::parse_braced_param`, which had it
hard-coded to `Quoting::Bare`; `interp.rs`'s `expand_arith_params` and
`arith_indir_resolves` now pass `Quoting::Dquote`. This is the same rule the
pass around it already followed for quotes written *outside* a `${ … }`.

**Then the translation itself**, in `read_dollar_brace_body`: a `$'…'` is
`ansiexpand`ed and re-quoted with `sh_single_quote` when the enclosing scan
carries no `P_DQUOTE` (parse.y:3882), and still copied when it does. With the
quoting fix already in place, arithmetic then falls out for free — the
translation lands as `'5'`, which the operand rule above already rejects.
Deciding before the `$` is written to the body is `retind -= 2` (parse.y:3893)
without the rewind.

The translation is observable outside arithmetic too, and the new corpus case
`an-ansi-c-string-in-a-brace-body-is-translated-at-parse-time.sh` pins it: a NUL
ends the string (`${x:-$'a\0b'}` is `${x:-'a'}`), a `$'\x27'` becomes `\'`
(shquote.c:103), the body that reaches `declare -f` holds no `$'` at all, and a
`$'x\ny'` puts a real newline where the source had none — so a `$LINENO` after
it in the same `$( … )` body sits one line lower.

**Still open, and deliberately so:** the same `$'…'` written *inside* double
quotes, where bash 5.2 translates but does not re-quote (branch 4,
parse.y:3887). That is the `#if 0 /* TAG:bash-5.3 */` defect, tracked separately
as `TD-OILS-AN-ANSI-C-STRING-IS-NOT-REQUOTED-AFTER-A-BRACE-BODY-TRANSLATES-IT`.
