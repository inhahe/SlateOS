### TD-OILS-ARRAY-POINTER-INDIRECTION. `${!a[@]op}` is indirection through the elements in bash, "bad substitution" in osh — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/parser.rs` — `parse_braced_param`'s indirection
branch (~3446), which refuses any pointer subscript that is not a specific index:

```rust
let index: Option<Box<Word>> = match subscript {
    None => None,
    Some(ArrayIndex::Index(w)) if is_valid_name(name.as_bytes()) => Some(w),
    Some(_) => return Ok(WordPart::BadSubst(raw.to_vec())),
};
```

**What.** `${!a[@]}` is the *keys* of `a` only when it stands alone. Give it any
operator and it stops being the key listing and becomes ordinary indirection
whose **pointer** is `a[@]`: bash reads `${a[@]}` as one string, uses that as a
variable name, and applies the operator to *that* variable.

```sh
declare -a one=(v); v=hello
echo "${!one[@]}"      # bash [0]      osh [0]        ✅ (keys)
echo "${!one[@]#h}"    # bash [ello]   osh ${!one[@]#h}: bad substitution
echo "${!one[@]:1:3}"  # bash [ell]    osh bad substitution
echo "${!one[@]/l/L}"  # bash [heLlo]  osh bad substitution
echo "${!one[@]@Q}"    # bash ['hello'] osh bad substitution
declare -a n=(x y z)
echo "${!n[@]#x}"      # bash: n[@] joins to `x y z` → "x y z: invalid variable name"
                       # osh : bad substitution
```

The scalar form `${!r#h}` and the prefix form `${!al@Q}` are already right — it
is only a `[@]`/`[*]` **pointer** that is refused, which is why this survived:
the spelling looks like the key listing and reads like a typo.

**Measured rules** (bash 5.2.37, all verified):

* The pointer name is formed by the **derived** join (`Shell::join_derived`),
  which contradicts what the spelling suggests. Written out, `x="${a[@]}"` glues
  the elements with a space whatever `$IFS` says; read *through a pointer* the
  same `a[@]` glues them with `$IFS[0]`. With `two=(v v)` and `IFS=:` the
  assignment gives `v v` while the pointer names `v:v`. Only the empty-`$IFS`
  fallback to a space is shared, and it is what lets the `[*]` spelling — which
  joins with nothing — glue `(he llo)` into the single legal name `hello` where
  `[@]` still produces the invalid `he llo`.

  *(This bullet said the opposite when the entry was first written: the first
  probe only varied `$IFS` for the `[*]` spelling and set it to `:` and to
  empty for `[@]`, where the space fallback made "always a space" fit the two
  data points. Varying `$IFS` for both spellings is what separated them.)*
* A pointer array that is **unset** is the fatal `nope[@]: invalid indirect
  expansion`; one that is **set but empty** (`mt=()`) points *nowhere* — empty
  with status 0, `${!mt[@]:-d}` takes the default, `${!mt[@]:+p}` is empty, and
  `set -u` reports `!mt[@]: unbound variable`. One empty *element*
  (`earr=('')`) is different again: the name is the empty string, so it is the
  fatal `: invalid variable name`.
* Everything downstream is the ordinary indirection path: `:=` assigns to the
  *target* (`pt=(tgt)`, `${!pt[@]:=set}` sets `tgt`), `:?` reports `!pt[@]`,
  `@A` gives `v='hello'`, and a nameref target is followed (`np=(nr)` with
  `declare -n nr=v` strips against `$v`).

**Proper fix.** The pointer's subscript is a subscript, so type it as one:
change `WordPart::Indirect`/`IndirectOp`'s `index` from `Option<Box<Word>>` to
`Option<ArrayIndex>`, accept `All`/`Star` in the parser whenever `remaining` is
non-empty (the bare form is already claimed by the `ArrayKeys` branch above),
render the new shapes in an `unparse::name_index` beside `name_sub`, and teach
the pointer read to answer `All`/`Star` with the joined element list —
returning "absent" for a zero-element list so the "points nowhere" outcomes
above fall out of the existing code rather than needing their own branch.

**Impact.** Any script that writes `${!ref[@]op}` gets a hard "bad substitution"
and a discarded command where bash quietly expands. Narrow, but it fails loudly
and in a way that is hard to read, since the message blames the whole
expansion rather than the pointer.

**Fixed in `f41e16289`**, as described. The read went into a new
`Shell::pointer_lookup`, which is where the three subscript shapes now meet:
`None` and `[i]` delegate to `param_elem_lookup` (whose subscript parameter
became a plain `Option<&Word>` borrow, since a pointer holding an `ArrayIndex`
can no longer hand it a `&Option<Box<Word>>`), and `[@]`/`[*]` join the
elements. `indirect_pointer_value` and `indirect_array_elems` both read through
it, so the quoted whole-array path (`ref=a[@]`) and the scalar one cannot drift
apart.

Covered by the corpus case
`a-an-array-subscript-can-point-as-well-as-list-keys.sh` and the lib test
`an_array_subscript_can_point_as_well_as_list_keys`.
