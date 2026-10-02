### TD-OILS-INDIRECT-OP-ON-AN-ARRAY-IS-NOT-A-LIST. `ref='a[@]'; ${!ref:-d}` is one field in osh and three in bash — 2026-08-04 — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the `IndirectOp` arm of
`Shell::expand_dynamic_with`. It resolves the pointer to a target *name*, reads
that name's value as a single string (`indirect_target_value`), rewrites the
modifier to name the target and re-enters `expand_dynamic_with`. Everything
downstream of the rewrite therefore works on text, so the list is already gone
before the modifier runs — and `Shell::split_items` cannot classify the part,
because the renamed node is a scalar `ParamTrim`/`ParamOp`, not an `ArrayBulk`.

**What.** When the reference names a whole array, bash keeps the elements apart
exactly as it does for the written-out spelling:

```sh
declare -a n=(ax by cz); r='n[@]'
IFS=:
printf '<%s>' ${!r:-d};  echo    # bash <ax><by><cz>   osh <ax by cz>
printf '<%s>' ${!r#a};   echo    # bash <x><by><cz>    osh <x by cz>
IFS=
printf '<%s>' ${!r:-d};  echo    # bash <ax><by><cz>   osh <ax by cz>
```

The bare `${!r}` (no modifier) is correct — that is `WordPart::Indirect`, which
`split_items` handles through `indirect_array_elems`. Only the
modifier-carrying `${!r<op>}` spelling loses the list.

**Fix.** Not a second implementation of the modifier rules but a *translation*.
A bash-vs-bash diff of the two spellings — `${!r<op>}` against the written-out
`${n[@]<op>}` — over 15 modifier forms × 3 `$IFS` settings, quoted and not,
comes back byte-identical, so the honest answer is to hand back the array node
and let the ordinary array code run:

* `Shell::indirect_whole_array` — the shared silent lookup, `(base, star)` for a
  referent spelled `base[@]`/`base[*]` and `None` for anything else.
  `indirect_array_elems` now delegates to it instead of repeating the parse.
* `array_target_part(target, name, star)` — a free fn beside
  `rename_param_target` turning the scalar node the parser built into its array
  twin: `ParamOp`→`ArrayOp`, `ParamTrim`/`ParamReplace`/`ParamCase`/
  `ParamTransform`/`BadTransform`→`ArrayBulk`, `ParamSubstr`→`ArraySlice`.
* `Shell::indirect_op_part` — the recogniser both of those compose into,
  answering the array node a `${!ref<op>}` really is.
* Four callers ask it: the `IndirectOp` arm of `expand_dynamic_with` (an early
  array path before the scalar resolution), `split_items`, `joined_value`, and
  the new `Shell::quoted_per_element` — the `DoubleQuoted` recogniser extracted
  verbatim out of `expand_word_annotated` so the indirect arm could recurse into
  it. Each swaps `ref_label` for the duration so a complaint still names `!r`.
* `array_op_fields`'s `ErrorIfUnset` diagnostic now honours that label, so
  `r='e[@]'; ${!r:?boom}` reports `!r`, not `e[@]`.

Two combinations do **not** look through, both measured: a **nameref** whose
modifier is the `${x:-…}` family answers with the name it holds (bash's
`get_var_and_type` split), and a nameref's look-through never keeps the elements
apart *inside quotes* — `declare -n g='n[@]'; "${!g#a}"` is one field.

Covered by
`tests/corpus/an-indirect-reference-to-a-whole-array-carries-its-modifier-to-the-array.sh`
(4 referent spellings × 3 `$IFS` settings × 27 forms, plus a pointer held in an
array element, the nameref exceptions, an empty and a missing array, the `:?`
complaint's naming and the `:=` bad subscript).

**Impact.** Indirection through a reference that spells a whole array, combined
with a modifier — an exotic corner. Nothing in the corpus depended on it, and the
answers agree under the default `$IFS`, which is why it went unnoticed.
