### TD-OILS-SCALAR-SLICE-IS-NOT-A-SUBSTRING. `${v[@]:off:len}` on a scalar is bash's substring operator, not a one-element list — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::slice_elements`, which
treated a scalar as a one-element list at subscript `0`.

**Reproduce:**

```sh
v=scalar
echo "[${v[@]:1}]"      # bash [calar]   osh []
echo "[${v[@]:1:3}]"    # bash [cal]     osh []
echo "[${v[*]:1:3}]"    # bash [cal]     osh []
echo "[${v[@]: -1}]"    # bash [r]       osh [scalar]
echo "[${v[@]: -3}]"    # bash [lar]     osh []
```

**The rule.** A `[@]`/`[*]` subscript on a variable that is not an array at all
does not make a list of one — bash falls through to the plain
`${v:off:len}` substring operator, negative offsets and all. osh instead reads
the scalar as a single element at subscript `0`, so any non-zero offset selects
nothing and `-1` selects the whole value.

**The fix.** `slice_elements` resolves the name once, and a target that is in
`vars` but in neither `arrays` nor `assoc` goes to the new `Shell::scalar_slice`,
which delegates to the same `param_substr` the written-out `${v:off:len}` uses.
So the negative-offset and negative-end-position rules are shared rather than
reimplemented, and a negative length is an end offset here (`${v[@]:1:-1}` is
`cala`) instead of the fatal "substring expression < 0" an array's slice makes
of it.

**One thing the delegation could not inherit,** because a substring yields a
string where a slice yields a *field list*: whether there is a field at all.
Measured, the boundary is the same "did the offset land inside" test an array
uses, with the value's character count for a length — a start anywhere in
`0..=n` is inside, so `"${v[@]:6}"` (with `v=scalar`, `n=6`) is one **empty**
argument while `"${v[@]:7}"` is **no** argument, and `-6` is inside where `-7`
is not. Out of range, the length is not evaluated at all — see the commit for
`a-slice-offset-past-the-end-never-reads-the-length.sh`, which pins the same
rule for arrays and the positionals.

**Pinned by** `tests/corpus/a-slice-of-a-name-that-is-no-array-is-a-substring.sh`,
which also covers the `[*]` spelling, an empty scalar, an integer, an export, a
nameref, an explicit `[0]` subscript, and the assignment and glued contexts.
