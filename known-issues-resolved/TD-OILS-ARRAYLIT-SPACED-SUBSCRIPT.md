### TD-OILS-ARRAYLIT-SPACED-SUBSCRIPT. `osh` tokenizes a *space-containing* array-literal subscript (`a=([3 x]=99)`) into two positional words instead of one keyed element — ✅ FIXED 2026-07-20 (stale; array-literal tokenizer already scans `[ … ]=` as one unit)

**Status:** FIXED. The array-literal element tokenizer now scans a leading
`[ … ]=` as a single unit (up to the matching `]`) before deciding word
boundaries, so a whitespace-containing subscript is one keyed element, matching
bash. Verified byte-for-byte against bash 5.2: `a=([1 + 2]=99)` → index 3;
`a=([3 x]=99)` → fatal `3 x: syntax error in expression (error token is "x")`
(status 1); `declare -A m; m=([a b]=v)` → key `a b` preserved. Regression-guarded
by `array_literal_spaced_subscript`.

**Original report (for reference):**

**Where:** `userspace/oils/src/parser.rs` (array-literal element tokenizer) —
the code that splits the words inside `a=( … )` and recognises `[subscript]=value`
keyed elements. The subscript is scanned only up to the first unquoted whitespace,
so a bracketed subscript that itself contains a space is not treated as a single
`[...]=` unit.

**What:** for an array literal whose keyed subscript contains whitespace, osh and
bash disagree on tokenization:

```
a=([3 x]=99)
  bash:  one keyed element with subscript `3 x`, which is then an *arithmetic*
         subscript that errors ("3 x": bad arithmetic) -> fatal, status 1.
  osh:   two positional words `[3` and `x]=99`, i.e. `declare -a a=([0]="[3" [1]="x]=99")`
         -> no error, status 0.
```

Valid keyed subscripts *without* spaces behave identically in both shells:
`a=([3]=99)`, `a=([1+2]=99)` both assign index-computed elements the same way, and
a space-containing subscript that would be a valid arith expression is the only
divergent case.

**Why deferred (separate from the arithmetic-fatality work):** this is a
*tokenization* difference in the array-literal parser, orthogonal to the
subscript-arithmetic-fatality fix (which correctly makes `${a[3 x]}` reads fatal).
To match bash the array-literal element splitter must recognise a leading
`[ … ]=` where the `…` may contain spaces (scan to the matching `]` before
deciding word boundaries), then feed that subscript through the same fatal
`eval_arith_index` path. Low value (obscure literal form; the space inside a
subscript is almost always a typo), so parked here rather than reworking the
element tokenizer now.
