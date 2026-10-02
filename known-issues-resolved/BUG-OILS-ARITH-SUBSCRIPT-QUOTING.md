### BUG-OILS-ARITH-SUBSCRIPT-QUOTING. Quotes were removed from arithmetic subscripts — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** `a['']=z` silently overwrote element 0 instead of failing:
```sh
a=(0 1 2)
a['']=z    # bash: "'': syntax error: operand expected"; osh assigned a[0]=z
a['1']=x   # bash: "'1': syntax error: operand expected"; osh assigned a[1]=x
echo "${a['1']}"   # bash: syntax error; osh printed the element
echo "${s:'1'}"    # bash: "s: '1': syntax error"; osh sliced from 1
```
An arithmetic subscript that quietly *succeeds* at the wrong index is the worst
shape of this bug: a typo'd `a['$i']=v` clobbers `a[0]`.

**Root cause.** `eval_arith_index` in `userspace/oils/src/interp.rs` expanded the
subscript with ordinary `expand_to_string`, which performs full quote removal.
bash instead runs an arithmetic subscript (and a `${x:off:len}` bound) through
`expand_arith_string`, which does parameter/command substitution and removes
**double** quotes but leaves **single** quotes in the text — the arithmetic
evaluator then rejects the `'` as an invalid token. So `a["1"]` is index 1 while
`a['1']` is a syntax error, and the two forms are *not* interchangeable. An
*associative* subscript is a different thing entirely: it is an ordinary word, so
its single quotes really do quote (that is how a key with spaces is written), and
that path was already correct.

**Fix.** A new `Shell::expand_to_arith_string` walks the word's parts and
re-emits a `SingleQuoted` run as its own source (`'…'`), expands a
`DoubleQuoted` run normally (quotes removed), and skips tilde expansion — `~`
has no meaning in an arithmetic expression. `eval_arith_index` uses it, which
covers array subscripts on every path (read, write, length, `declare`, `unset`)
and the `${x:off:len}` / `${a[@]:off:len}` bounds, since those share the same
entry point. The evaluator already rejected a `'` that arrived by expansion
(`k="'1'"; a[$k]=v`), so nothing changed there.

**Known remaining gap.** A *backslash*-escaped subscript character diverges:
`a[\1]` is `\1: syntax error` in bash but index 1 in osh, and `a[\"1\"]` names
the token `'"'1'"'` where bash names `"1"`. The lexer deliberately folds a
backslash escape into a one-character `Seg::Sq` (a documented conflation —
`a\*b` ≡ `a'*'b` for globbing, `case` and `[[ == ]]`), with an extra carve-out
folding an escaped *alphanumeric* straight into the plain literal so `\if` /
`\ls` still reach reserved-word and command-name recognition. Distinguishing the
two would need a separate `WordPart::Escaped` variant threaded through ~13 match
sites in `interp.rs` — and, more importantly, past every catch-all arm, where a
missed case would silently break globbing or pattern matching. That is a worse
class of bug than the one it fixes, for a shape (`a[\1]`) that essentially never
occurs. Left alone deliberately; the proper fix, if it is ever wanted, is the
`Escaped` variant plus an audit of every `_ =>` arm over `WordPart`.

**Regression cover.** `arith-subscript-quoting.sh` (write, read, length,
mid-expression, expansion-supplied quotes, substring/slice bounds, the
associative contrast, and the `declare -i` *value* contrast where quote removal
does happen), plus the unit test
`arithmetic_subscripts_keep_single_quotes_and_drop_double_quotes`.
