### TD-OILS-ERE-GLIBC-STRICTNESS. osh's ERE engine accepted patterns glibc rejects — 2026-07-28 — RESOLVED 2026-07-28

**Where:** `userspace/oils/src/ere.rs` — `Parser::parse`, `parse_alt`,
`parse_concat`, `parse_repeat`, `parse_quantifier`, `parse_brace`.

**What:** the in-tree ERE engine was written to the "be liberal in what you
accept" school, so a malformed pattern that bash refuses outright (status 2, the
same status a malformed conditional gets) was silently reinterpreted by osh as
something matchable. Found while resolving TD-OILS-COND-PAREN-REGEX, where
`[[ "a{b" =~ a{b ]]` came out bash 2 / osh 0.

Six rules, each measured against bash 5.2 (glibc `regcomp`, `REG_EXTENDED`):

| Rule | bash rejects | bash accepts |
|---|---|---|
| an unescaped `{` must open a well-formed interval — no literal fallback | `a{b` `a{1` `a{}` `a{,3}` `a{1,2,3}` `{b` | `a\{b` `a[{]b` |
| a quantifier needs a preceding atom; `^` is an assertion, not one | `*a` `+a` `?a` `{2}a` `(*a)` `a\|*b` `^*a` `a^*b` | `a$*` |
| one quantifier per atom | `a**` `a*+` `a*?` `a?+` `a{1}*` `a*{1}` `a{1}{2}` `(a)*+` | |
| every alternation branch must be non-empty | `\|a` `a\|` `(a\|)` `(\|a)` `(a\|\|b)` | `()` `a()b` |
| `{0}` *deletes* its atom rather than repeating it zero times, and errors only if that empties the run | `a{0}` `(a{0})` `a{0}\|b` `a{0}b{0}` | `a{0}b` `^a{0}` `a{0}$` |
| an empty pattern is not a pattern | `""` | |

The fifth is the non-obvious one, and it is a property of the *construction*,
not of the result: `a{0}b` is fine and `()` is fine, but `(a{0})` is not — an
alternation branch that was emptied by a zero count is an error even though a
branch that was empty to begin with is not.

**Fix.** `parse_repeat` now returns `Option<Node>`, with `None` meaning "the atom
was deleted by a zero count"; `parse_concat` tracks that in a `deleted` flag and
errors on an empty run only when the flag is set. `parse_repeat` rejects a
quantifier in an atom slot up front (`is_quantifier_start`), rejects a second
quantifier after consuming one, and rejects a quantifier applied to `Node::Start`.
`parse_quantifier` is split out from `parse_brace`, and `parse_brace` treats any
`{` it cannot parse as an interval as an error rather than backing off to a
literal — the `Some('{')` arm left in `parse_atom` is unreachable but kept so a
future edit cannot silently resurrect the lenient reading. `parse_alt` rejects
`Node::Empty` branches, and `parse` rejects an empty input.

Covered by `ere::tests::rejects_what_glibc_rejects` (40 shapes) and
`tests/corpus/cond-regex-dialect.sh`.
