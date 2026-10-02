### BUG-OILS-UNSET-SUBSCRIPT. `unset name[sub]` did not evaluate the subscript — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** `unset` matched the subscript's *raw text* against the array, so
every non-literal-decimal subscript silently did nothing while reporting
success — the worst possible failure mode for a removal:
```sh
a=(0 1 2 3 4); i=2
unset 'a[1+1]'   # bash removes index 2; osh removed nothing, status 0
unset 'a[i]'     # likewise
unset 'a[-1]'    # bash removes the last element; osh removed nothing
unset 'a[@]'     # bash empties the array; osh removed nothing
unset 'm[$key]'  # bash expands $key; osh looked for a key literally named "$key"
```
Conversely it removed too much on the associative side: `unset 'q[a"b]'` matched
the literal key `a"b`, which bash (whose subscript goes through quote removal)
does not. And it returned on the first failing name, so `unset 'a[-99]' b c` left
`b` and `c` set where bash unsets them and returns 1 at the end.

**Root cause.** `builtin_unset` in `userspace/oils/src/interp.rs` did
`idx_src.parse::<usize>()` for an indexed array and an exact string compare for
an associative one, on the substring between `[` and the trailing `]`. Neither
arithmetic evaluation nor word expansion ever ran, and the nameref resolution
was applied to the whole `ref[1]` token rather than to the base name.

**Fix.** A new `Shell::unset_element(name, sub_src)` re-parses the subscript
with `parser::word_verbatim_from_source` (made `pub(crate)`) and then routes it
by array kind, matching bash's `unbind_array_element`:
* **indexed** — arithmetic evaluation via `eval_arith_index` (arith tag cleared,
  as bash never tags a subscript error with the builtin name), negatives
  resolved against `highest_index + 1` by `resolve_index`, underflow reported as
  `unset: [<src>]: bad array subscript` (status 1, source text — not the
  expansion — as bash does), and `[@]`/`[*]` clearing the elements while leaving
  the array declared with its attributes;
* **associative** — expansion + quote removal to a string key, so `[@]`/`[*]`
  are ordinary keys that never flush the map, and an empty result is
  `unset: [<src>]: bad array subscript`;
* **scalar** — addressable only as `[0]` (which unsets the whole variable); any
  other subscript, including `[@]`, is `unset: <name>: not an array variable`;
* **unset variable** — never probed at all, so `unset 'nosuch[x y]'` is silent.

Tokens that are not valid array references (`n[]`, `n[0]junk`, `1abc[0]`) now
fall through to the plain-name path, as bash does. `builtin_unset` accumulates
status instead of returning on the first failure. The base name is resolved
through namerefs *after* the subscript is split off, so `unset ref[1]` reaches
the target array.

**Known remaining gap (pre-existing, separate).** A quoted arithmetic subscript
(`unset "a['1']"`, `a['1']=v`) has its quotes removed and evaluates to 1; bash
keeps them and reports `'1': syntax error: operand expected`. This is the same
single-quote-preservation gap that already affects assignment subscripts and is
tracked on its own; the `unset` path deliberately shares the existing behaviour
rather than diverging from the assignment path.

**Regression cover.** `unset-subscript.sh` (37 probes across arithmetic
subscripts, negatives, sparse arrays, `[@]`/`[*]` by array kind, associative
keys with quotes/expansions/spaces, scalars, unset variables, non-reference
tokens, namerefs, readonly and the discarding arithmetic error), plus five unit
tests named `unset_*`.
