### TD-OILS-A-BRACE-BODY-THAT-OPENS-NO-NAME-EXPANDS-TO-NOTHING. `echo "${.}"` should be a bad substitution — 2026-08-08 — ✅ FIXED 2026-08-08

**Fixed** in the commit adding `is_special_param_char` to
`userspace/oils/src/parser.rs`. Corpus case:
`tests/corpus/a-brace-body-that-opens-no-name-is-a-bad-substitution.sh`.

**Where:** `userspace/oils/src/parser.rs`, `split_name_subscript` — the
fall-through branch that read *any* leading character as a one-character special
parameter (`else { i = 1; }`), and the `chs.is_empty()` guard above it that
raised a parse error.

**What.** A `${…}` body has to name something. bash accepts exactly four kinds
of name — `valid_brace_expansion_word` (subst.c) is all-digits, or
`SPECIAL_VAR` (subst.c:125, the `CSPECVAR` class built by
`mksyntax.c:247`, `addcstr ("@*#?-$!", CSPECVAR); /* omits $0...$9 and $_ */`),
or an array reference, or a `legal_identifier` — and anything else reaches
`bad_substitution` (subst.c:10039). osh had no such gate: it took the first
character as the name whatever it was, looked up a parameter of that name, found
it unset, and expanded to the empty string with status 0.

```text
                      bash                              osh (before)
echo "${.}"           ${.}: bad substitution   rc=1     (empty)          rc=0
echo "${ }"           ${ }: bad substitution   rc=1     (empty)          rc=0
echo "${:-x}"         ${:-x}: bad substitution rc=1     x                rc=0
echo "${.:-x}"        ${.:-x}: bad substitution rc=1    x                rc=0
echo "${.#p}"         ${.#p}: bad substitution rc=1     (empty)          rc=0
echo "${}"            ${}: bad substitution    rc=1     syntax error     rc=2
```

Twenty-odd characters were affected — everything that is neither a digit, nor a
name character, nor one of the seven specials. Two details the fix had to get
right:

* **The refusal is of the name, before the operator.** `${.:-x}` does not
  substitute the default and `${.#p}` does not strip: bash rejects the name
  first, so the whole `${…}` fails however it was going to be used. Putting the
  check in `split_name_subscript`, where the name is carved off, gets this for
  free; a check at expansion time would have had to be repeated per operator.
* **`${}` is a *runtime* error, not a parse error.** bash reaches it through the
  same `valid_brace_expansion_word` (an empty name is not a legal identifier),
  so `if false; then echo "${}"; fi` is silent and a reached one discards its
  parse unit without killing the shell. osh raised a parse error, which took the
  script down with status 2 before it ran a line. The empty case now returns
  `NameSubscript::Deferred` like every other deferred body.

The seven specials, digits and identifiers are unaffected, with or without an
operator: `${#}`, `${@}`, `${*}`, `${?}`, `${!:-n}`, `${-:+s}`, `${$}`, `${1}`,
`${12:-e}`, `${v}`, `${_v:-u}` all still behave.
