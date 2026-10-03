### TD-OILS-ASSOC-SLICE-OFFSET-IS-ONE-BASED. `${m[@]:off:len}` on an associative array counts from one, and a length of zero still yields an element — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::slice_elements`, now
dispatching to `Shell::assoc_slice`. The companion rule for *indexed* arrays
(the offset is a subscript, not a position) is implemented there and pinned by
`tests/corpus/an-array-slices-offset-is-a-subscript-not-a-position.sh`; that
case deliberately contains no associative array, because this entry is why it
would have failed.

**Reproduce** (order-independent half — a one-key array, so bash's hash order
cannot confuse the reading):

```sh
declare -A one=([solo]=s)
show() { printf '(%d)' $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }
show x ${one[@]:0}      # bash (1)<s>   osh (1)<s>
show x ${one[@]:1}       # bash (1)<s>   osh (0)<>     <-- diverges
show x ${one[@]:2}       # bash (0)<>    osh (0)<>
show x ${one[@]:0:0}     # bash (1)<s>   osh (0)<>     <-- diverges
show x ${one[@]:1:0}     # bash (1)<s>   osh (0)<>     <-- diverges
```

**The measured rule.** Let the array's values in bash's own hash order be
`e[0..n)`. Then `${m[@]:off:len}` is

* for `off >= 0`: skip `max(off - 1, 0)` elements — so offset `0` and offset `1`
  both name the *first* element, and only from `2` on does each step drop one
  more;
* for `off < 0`: the start is `off + n + 1`, **not** `off + n` — so `-1` is the
  last element, `-(n+1)` is the first, and `-(n+2)` is the first that is out of
  range. (The first draft of this entry guessed `off + n` with no adjustment;
  measuring `-5` and `-6` against a four-key array disproved it — `-5` is still
  inside.) From there the same `max(start - 1, 0)` skip applies.
* when a length is given, take `max(len, 1)` — a length of `0` yields **one**
  element, not none, unless the offset already ran off the end.
* the offset decides whether the length is read at all, on the same `0..=n`
  test the positionals use: `${m[@]:5:j++}` on a four-key array leaves `j`
  alone, and `${m[@]:5:-1}` is silent where `${m[@]:4:-1}` is fatal.

Measured against bash 5.2.37 with `declare -A m=([k1]=v1 [k2]=v2 [k3]=v3
[k4]=v4)`, whose hash order is `v4 v1 v2 v3`: `${m[@]:0}` and `${m[@]:1}` are
both all four, `${m[@]:2}` is three, `${m[@]:4}` is one, `${m[@]:5}` is none;
`${m[@]:0:0}`, `${m[@]:1:0}` and `${m[@]:1:1}` are all the single element `v4`,
`${m[@]:2:0}` is the single element `v1`, `${m[@]:0:3}` is `v4 v1 v2`, and
`${m[@]: -2:1}` is `v2`. Both quirks are independent: the offset is off by one
*and* the length has a floor of one. osh applied the plain `skip(off).take(len)`
reading it uses for a dense list.

**The fix.** `slice_elements` dispatches an associative target to the new
`Shell::assoc_slice`, which implements the rule above over the values in
whatever order the map iterates. An *empty* associative array is left to fall
through instead, so that it meets the shared "nothing to measure" exit rather
than repeating it.

**Pinned by** `tests/corpus/an-associative-slices-offset-counts-from-one.sh`.
It first reported field *counts* for the two- and four-key arrays, ordering
parity being missing at the time; now that
`TD-OILS-ASSOC-ITERATION-ORDER-IS-SORTED-NOT-HASHED` below is fixed it reports
the values too, so a slice names *which* elements and not merely how many. Note
that the case's helpers save their labels into locals before taking the slice:
`set --` replaces a function's own positionals, so reading `$2` afterwards would
print a *value* and quietly turn the case into a hash-order test.
