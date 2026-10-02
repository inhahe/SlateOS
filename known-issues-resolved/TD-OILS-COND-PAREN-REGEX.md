### TD-OILS-COND-PAREN-REGEX. `[[ … =~ ( … ]]` — bash treats `(` as conditional grouping, osh treats it as regex — 2026-07-19 — RESOLVED 2026-07-28

**Resolved 2026-07-28**, and the original report below turned out to have the
rule backwards: bash does *not* treat `(` after `=~` as grouping. It reads the
RHS as one word in which an unquoted `( … )` group swallows everything —
blanks, newlines and shell operators alike — while *outside* a group the usual
word boundaries still apply. So the interesting divergence was never the
unbalanced paren; it was that osh rejected every regex with a space in it:

```
[[ "a b" =~ (a b) ]]        bash: 0     osh: 2 (syntax error near `b')
[[ "xa by" =~ x(a b)y ]]    bash: 0     osh: 2
[[ "a b" =~ ^(a b)$ ]]      bash: 0     osh: 2
```

which is a shape real scripts use. In the other direction osh was too
permissive: it made `;`, `&`, `<`, `>` and `)` literal *everywhere* in the RHS,
so `[[ "a;b" =~ a;b ]]` succeeded where bash reports
``unexpected token `;'``.

**Fix.** `Lexer::read_word_regex` now counts unquoted paren depth. At depth 0 the
word ends at a blank, a newline, `;`, `&`, `<`, `>` or `)`, and those characters
go back to the tokenizer — which is what produces bash's conditional-expression
syntax errors. At depth > 0 nothing terminates the word. `|`, `#`, `{`, `}`, `*`,
`^` and `$` stay ordinary regex characters at every depth, and a quoted or
backslash-escaped paren neither opens nor closes a group (quotes and `$…` are
still read by their own sub-readers, so their parens never reach the counter).
A group left open at end of input is the lexer's error, reported with the
existing `eof_matching(')')` — bash's
``unexpected EOF while looking for matching `)'`` — rather than falling through
to a regex-compile failure.

Covered by `tests/corpus/cond-regex-word.sh` (38 shapes) and
`parser::tests::cond_regex_group_spans_blanks_and_operators`.

**Residual (not this entry's):** the one regex-*dialect* divergence this work
turned up — `[[ "a{b" =~ a{b ]]` being status 2 in bash and 0 in osh, because
osh's engine took the unmatched `{` as a literal brace where glibc rejects it —
was fixed separately the same day; see TD-OILS-ERE-GLIBC-STRICTNESS.
Diagnostic-echo differences on the rejected forms (bash's
``syntax error near `a)'`` vs osh's ``near `)'``) belong to
TD-OILS-COND-ERRTEXT's residual.

**Original report (for reference):**

**Where:** `userspace/oils/src/parser.rs` conditional-expression parsing
and `userspace/oils/src/interp.rs` `cond_regex`.

**What:** `[[ abc =~ ( ]]` — bash parses `(`/`)` inside `[[ ]]` as
expression-grouping operators (`[[ ( a || b ) && c ]]`), so a bare `(`
with no closing `)` is a *shell parse error* ("unexpected EOF while
looking for matching `)'"). osh instead treats everything after `=~` up
to `]]` as the regex RHS, so `(` becomes an (invalid) regex and `[[`
returns 2. The common invalid-regex path (`[[ x =~ [ ]]`) now matches
bash exactly (status 2, no message); only the paren-as-grouping vs
paren-as-regex distinction diverges.

**Why deferred:** correctly disambiguating `(` between conditional
grouping and a regex metacharacter requires bash's context-sensitive
`[[` tokenizer (bash special-cases `(`/`)` only *outside* the `=~` RHS,
and the RHS boundary itself depends on word splitting). This is a rare
construct — a literal unbalanced `(` immediately after `=~`. The
exit-code for genuinely-invalid regexes already matches.

**Proper fix:** teach the `[[` parser bash's grouping rules and have the
`=~` RHS consume a single word (bash reads the RHS as one word unless
parenthesized), so `( )` grouping and regex parens are distinguished by
position.
