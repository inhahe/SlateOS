### TD-OILS-A-READ-THROUGH-A-CHAIN-THAT-REACHED-NOTHING-STILL-JUDGES-ITS-SUBSCRIPT. `${a[-1]}` through a circular nameref expanded to nothing in silence where bash refuses the subscript — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, [`Shell::expand_array_ref`] (plain
`${a[i]}` / `${#a[i]}`) and [`Shell::param_elem_lookup`] (every operator form).
Both returned empty the moment the nameref walk came up short, so the
subscript that was written was never expanded, never evaluated as arithmetic,
and never refused.

```sh
$ ( declare -n a=b; declare -n b=a; i=0; echo "[${a[i++]}]"; echo "i=$i" )
bash: [] / i=1          # the side effect still happens
osh : [] / i=0

$ ( declare -n a=b; declare -n b=a; echo "[${a[1+]}]" )
bash: 1+: syntax error: operand expected (error token is "+")
osh : []

$ ( declare -n a=b; declare -n b=a; echo "[${a[-1]}]" )
bash: a: bad array subscript / []
osh : []

$ ( declare -n r='n[1]'; n=(a b c); echo "[${r[-1]}]" )
bash: r: bad array subscript / []
osh : []
```

**Why bash does it.** `array_variable_part` hands `array_value_internal`
whatever `find_variable` found, and it is allowed to have found *nothing* — a
circular chain, or a reference that already designates one element and so has
no array left for a *second* subscript. The read carries on without a
variable (arrayfunc.c:1487):

```c
  var = array_variable_part (s, flags, &t, &len);  /* XXX */

  /* Expand the index, even if the variable doesn't exist, in case side
     effects are needed, like ${w[i++]} where w is unset. */
#if 0
  if (var == 0)
    return (char *)NULL;
#endif
```

So the subscript is still expanded, still judged as arithmetic, and a
negative one still underflows — because only an array counts back from
anything:

```c
  ind = array_expand_index (var, t, len, flags);
  if (ind < 0)
    {
      if (var && array_p (var))
        ind = array_max_index (array_cell (var)) + 1 + ind;
      if (ind < 0)
        INDEX_ERROR();
    }
```

`INDEX_ERROR()` (arrayfunc.c:1456) picks a different name either side of that
same question —

```c
  if (var) err_badarraysub (var->name);
  else { t[-1] = '\0'; err_badarraysub (s); t[-1] = '['; }
```

— the variable the walk *found*, or, where it found none, the word as
**written**, cut off at its `[`. Which is the mirror image of an element
*write*, where the diagnostic always quotes the word as written and only the
store follows the chain
(TD-OILS-A-NAMEREF-TO-AN-ELEMENT-IS-JUDGED-BEFORE-THE-SUBSCRIPT-IT-WAS-WRITTEN-WITH).

The length form takes none of this route: `array_length_reference`
(subst.c:7213) answers a null `var` with 0 before the subscript is looked at,
and faults under `set -u` naming the base alone — so `${#a[1+]}` is `0`, not
a syntax error, and `${#a[0]}` under `set -u` says `a: unbound variable` where
`${a[0]}` says `a[0]: unbound variable`.

**Fixed** in this commit. [`Shell::expand_array_ref`] no longer returns early
when [`Shell::resolve_ref_use_walks`] reaches no variable; it calls
[`Shell::expand_array_ref_resolved`] with `name: None` and the word as
written, and that function now takes `Option<&str>` throughout — a `None`
name has no shape, no elements, and (per `var && array_p (var)`) nothing to
count back from, so every negative subscript underflows. Which name the
complaint quotes is `INDEX_ERROR()`'s choice, spelled here as
`name.unwrap_or(written)`. [`Shell::param_elem_lookup`] gained the matching
[`Shell::param_elem_unfound`], which takes the arithmetic branch
unconditionally — `assoc_p (var)` is false for a null `var`, so an empty key
is never judged as one there.

Corpus:
`a-read-through-a-chain-that-reached-nothing-still-judges-its-subscript.sh`.
Unit test:
`a_read_through_a_chain_that_reached_nothing_still_judges_its_subscript`.

**How it was found:** probing the read side of the same walk, while fixing
TD-OILS-A-NAMEREF-TO-AN-ELEMENT-IS-JUDGED-BEFORE-THE-SUBSCRIPT-IT-WAS-WRITTEN-WITH.
