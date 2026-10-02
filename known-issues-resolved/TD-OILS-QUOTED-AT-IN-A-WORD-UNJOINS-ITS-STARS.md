### TD-OILS-QUOTED-AT-IN-A-WORD-UNJOINS-ITS-STARS. a quoted `[@]` makes the `[*]` beside it expand as a list too — 2026-08-04 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — the `DoubleQuoted` arm of
`Shell::expand_word_annotated`, `Shell::quoted_per_element_parts` and
`Shell::cond_dquote_items`.

**What.** bash sets a flag when a quoted `$@`/`${a[@]}` is expanded, and once it
is set the *derived* `[*]` forms expanded alongside it stop joining and come out
one field per item — whichever order the two appear in, and whatever `$IFS` says:

```sh
declare -a n=('a:b' 'c d' e)
IFS=:
printf '<%s>' "${n[*]@Q}"                 ; echo   # <'a:b':'c d':'e'>
printf '<%s>' "${n[@]@Q} ${n[*]@Q}"       ; echo   # <'a:b'><'c d'><'e' 'a:b'><'c d'><'e'>
```

The second line is not a re-split of the joined string — `'a:b'` survives with
its colon — so the star form genuinely produced three items. osh gave one joined
field for the star half either way. Before TD-OILS-STAR-BULK-JOIN-IGNORES-IFS it
joined with a space and so happened to agree with bash here while being wrong
everywhere else; the fix for that traded this corner away deliberately, and this
entry recorded the trade.

**Fixed 2026-08-05.** `Shell::run_at_unjoins` is the flag, set from a static
scan (`parts_have_quoted_at`) of a double-quoted run before any of it expands,
and `Shell::star_unjoins` is that flag plus the `$IFS` gate. The four derived
star forms are then the same arms of `quoted_per_element_parts` their `[@]`
spellings already used, under a guard — the items were never in question, only
whether they stayed one field.

The rule was **measured** against bash 5.2.37 (six `$IFS` settings × ~40 shapes)
and is narrower than this entry originally described. bash builds a derived star
list by gluing its items with an **unquoted** `$IFS[0]`, the items themselves
quote-protected, and splits the finished word iff `quoted_dollar_at` was set. So:

* Only the *derived* stars unjoin — `${a[*]@Q}`/`@A`/`^^`/`#p`/`%p`/`/p/r`,
  `${!a[*]}`, `${!pre*}`, `${a[*]:i:j}`, and the `$*` spellings of those. A plain
  `${a[*]}`/`$*` never does (its separator is as much the value as the elements
  are), nor does `${a[*]:-w}` (it joins *elements*, and its operand is a word
  with fields of its own), nor `${#a[*]}`, nor an associative `${h[*]}`.
* The scope is the **quoted run**, not the word — the original entry had this
  wrong. bash re-enters `expand_word_internal` on a run's contents, and
  `quoted_dollar_at` is that call's: `"${n[*]@Q} ${n[@]}"` unjoins where
  `"${n[*]@Q}""${n[@]}"` and `"${n[*]@Q}"x"${n[@]}"` do not.
* An **unquoted** `[@]` does not count (`"${n[*]@Q}"${n[@]}` stays joined); an
  `[@]` with **no elements** does (`"${n[*]@Q}" "$@"` with no positionals still
  unjoins). Order is nothing.
* A **null** `$IFS` turns it off entirely: there is no separator to write, so
  nothing is left for the split to find and the items run together —
  `IFS=` gives the one field `'a:b''c d''e' a:b`. That is `star_sep`'s emptiness,
  not `at_sep`'s, which substitutes a space and is never the star's separator.
* A context that never splits keeps the star joined: `v="${n[*]@Q} ${n[@]}"` is
  one string. `SplitMode::Text` clears the flag for the whole word.
* An **operand is not a run of its own**. The deciding `[@]` may be outside it
  (`"${x:-${n[*]@Q}} ${n[@]}"`), inside it (`"${n[*]@Q} ${x:-${n[@]}}"`), nested
  (`"${n[@]} ${x:-${y:-${n[*]@Q}}}"`) or quoted within it — all unjoin. So the
  scan descends into `${x:-w}`/`${a[*]:-w}` operand words and the operand modes
  inherit the flag rather than recompute it. A command substitution *is* its own
  word and takes nothing with it.
* `[[ ]]`/`case` runs answer to it too (`cond_dquote_items`), which is why the
  flag is set there as well.

One deliberate asymmetry: a star arm does **not** set `Shell::saw_quoted_list`.
The fields are the final split's, not a quoted list's, so bash's `WORD_LIST`
path is not reached by them.

The claim this entry used to make that the fix would "also fix the plainer
divergence next door, where `IFS=:; printf '<%s>' "x${n[@]}y"` is three fields in
bash and one in osh" was already stale when written: that shape has been correct
in osh since TD-OILS-UNQUOTED-ARRAY-EXPANSION-IS-A-JOINED-STRING.

**Corpus:** `a-quoted-at-in-a-run-unjoins-the-derived-stars-beside-it.sh`.
