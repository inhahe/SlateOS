### TD-OILS-WHOLE-ARRAY-TRANSFORM-IS-ONE-WORD. `"${a[@]@A}"` is three words in bash and one in osh — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::bulk_attr_transform`, which
builds the whole-array `@A` declaration as a single field.

**What.** bash's `${a[@]@A}` is a *word list*, and `[@]` in double quotes keeps
list elements apart, so the declaration arrives as three words:

```sh
declare -a n=(x y)
printf '[%s]\n' "${n[@]@A}"
# bash: [declare]  [-a]  [n=([0]="x" [1]="y")]
# osh : [declare -a n=([0]="x" [1]="y")]
set -- "${n[@]@A}"; echo $#      # bash: 3   osh: 1
```

The split is structural, not a re-split on spaces: the third word keeps the space
inside `[0]="x" [1]="y"`. `[*]` joins the three with `IFS`, which is why
`"${n[*]@A}"` is one word in both shells and already matches — and why the
existing lib assertions, which all use `echo`, never saw the difference.

**Proper fix.** *(Rewritten 2026-08-04 after measuring. The original text —
"return the three pieces, the `declare` keyword, the flags word and the
`name=(…)` assignment, as separate fields" — was wrong: there is nothing
three-part about it, and hard-coding three fields would have failed every `IFS`
below except the default.)*

`@A` builds **items**, and an item is not a field:

* an array or assoc has **one** item, the whole re-inputtable `declare`;
* the positional params have **one per parameter**, with the `set -- ` glued to
  the first — not one per word, which is why `IFS=:` gives
  `set -- 'p q':'r'`, colons between the parameters and a space inside the
  statement head.

What each context does with the items is then three different things:

| context | what happens |
|---|---|
| quoted `[@]` (`set -- "${n[@]@A}"`) | field-split **each item** against `$IFS` |
| quoted `[*]`, or any scalar context (`x="${n[@]@A}"`) | join the items **unsplit** — `[*]` with `star_sep`, `[@]` with `at_sep` |
| unquoted (`set -- ${n[@]@A}`) | join, then the **ordinary** field split runs over the result |

The split is a raw character split over the item, not a re-tokenisation — `IFS=a`
cuts `declare` in half into `decl` / `re -` / ` n=([0]="x" [1]="y")` — but it
respects the regions a shell reads as one word (`'…'`, `"…"`, `(…)`), so the
spaces inside the element list never delimit and a `)` in a value does not end
the group. An empty `$IFS` splits on space anyway (bash's `W_SPLITSPACE`) rather
than not at all, which is exactly why the split has to happen *before* the join
and not after: under `IFS=` the `[@]` form is still three words while
`${n[*]@A}` is the intact declaration and `${*@A}` is `set -- 'p q''r'`.

The three contexts differing is also what makes the unquoted form split *twice*
and so cut where the quoted one does not — `IFS=' '` gives the quoted form three
fields and the unquoted form four (`n=([0]="x"` and `[1]="y")`).

Scalars are never split: `${iv[@]@A}` stays the one word `declare -i iv='7'` and
`${s[@]@A}` on `s='a b'` stays `s='a b'`. `@a` is one field per element and is
never split either.

So the shape of the fix is: `bulk_elements` returns items, the *caller* decides —
`expand_part` joins them (`join_derived`), and the quoted `[@]` arm of
`expand_word_annotated` splits each one.

**Impact.** Anything that counts or iterates `"${a[@]@A}"` rather than echoing
it. Rare, but it is the shape `set -- "${a[@]@A}"` and `for w in "${a[@]@A}"`
take, and a script doing either gets one word where bash gives three.

**Fixed in `ed457c1ab`** — `bulk_elements` grew a `fields` flag saying whether
the caller is a context that keeps fields apart, `bulk_attr_transform`'s `A`
branch builds the items and hands them to a new `split_transform_items`, and
`split_transform_item` does the shielded character split.

Two things about the shape are worth keeping, because both were wrong in an
earlier draft of this entry and were only caught by measuring:

* **The flag is "does the caller keep fields apart", not "is this `[*]`."** A
  scalar context does not split either, however it is spelled — `x="${n[@]@A}"`
  is the whole declaration under every `$IFS`. Keying on the star would have
  split the `[@]` half of that.
* **The split stayed inside the transform** rather than moving to the caller, as
  the paragraph above proposed. The caller cannot do it without knowing that the
  op is `A`, since `@a` and every element-wise transform must not be touched —
  and that knowledge already lives here. The flag exports the *decision* to the
  caller while leaving the *rule* in one place.

Covered by the corpus case
`a-a-collection-declaration-is-split-into-words.sh` (a full match against bash
5.2.37 across `IFS` = space, `:`, empty, `a`, `=`, `x`, `e` and unset, in all
three contexts and both spellings) and by the lib test
`a_collection_declaration_is_split_only_where_fields_are_kept`.
