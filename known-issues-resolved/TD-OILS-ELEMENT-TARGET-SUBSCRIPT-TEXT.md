### TD-OILS-ELEMENT-TARGET-SUBSCRIPT-TEXT. An array *element* named by a subscript that is not text cannot be assigned to by `printf -v`, `read` or `declare -n` — ✅ RESOLVED 2026-07-30 (both steps)

**Where:** `userspace/oils/src/interp.rs` — the three surviving callers of
`is_valid_assignment_target` (`:27326`), each of which gates on
`bytes::as_str(whole_word)` *before* the base name is split from the subscript:
- `:19101` — `printf -v 'arr[SUB]'`
- `:19816` — `declare -n r='arr[SUB]'` (and its `+=` form)
- `:22325` / `:22435` — `read 'arr[SUB]'`, first name and later names

**What.** A subscript is an ordinary shell word: `m[$k]` may hold any byte,
and an associative array happily *stores* such a key — `m[$k]=v` works, and as
of the fix below `unset 'm[$k]'` removes it. But the three builtins above ask
whether the **whole** operand is text before looking at its shape, so the
subscript's bytes disqualify the base name:

```
$ k=$(printf '\xa9'); declare -A m
$ printf -v "m[$k]" %s hi
osh: line 3: printf: `m[<0xa9>]': not a valid identifier
$ echo val | read "m[$k]"
osh: line 3: read: `m[<0xa9>]': not a valid identifier
```

bash assigns in both cases. Note the failure is **loud and inert** — a
diagnostic and no assignment — not the silent wrong-target write that the rest
of TD-OILS-BYTE-STRINGS was about. That is why it is debt rather than a bug to
drop everything for.

**How it was found.** The same gate-ordering mistake in a *fourth* caller,
`builtin_unset`, was a real bug: it silently dropped the request, leaving the
element in place with status 0. Fixed at TD-OILS-BYTE-STRINGS step 8, with the
regression test `unset_element_addresses_a_subscript_that_is_not_text`. `unset`
was separable because it only needs the base name as `&str` to index the
variable tables; the other three do not have that luxury (below).

**Proper fix.** Split before gating, and byte-type the nameref target storage:

1. Add `fn split_assignment_target(s: BStr<'_>) -> Option<(&str, Option<BStr<'_>>)>`
   next to `is_valid_assignment_target`, returning the base name as `&str`
   (valid identifier syntax is ASCII by construction, so this narrowing is
   exact, not an approximation) and the subscript as raw bytes. Replace all
   four call sites with it. `is_valid_assignment_target` then becomes
   `split_assignment_target(...).is_some()` and the `&str` overload disappears.
2. `resolve_ref_name(&self, name: &str) -> Option<String>` (`:13503`) and
   `nameref_elem_value(&self, name: &str)` (`:13555`) currently re-split the
   subscript out of a `String`, so a nameref pointing at `a[<non-text>]` cannot
   round-trip even once the callers are fixed. The nameref value must become
   `Str`, which pulls in `nameref_attr` and `set_scalar_checked`.

Step 1 alone fixes `printf -v` and `read`; `declare -n` needs step 2 as well.
Do this **with or after** TD-OILS-BYTE-STRINGS step 9/10 — the nameref value is
one of the last `String`s left, and converting it in isolation would need a
scaffold at every site step 10 is about to satisfy for free.

**Step 1 done 2026-07-30.** `printf -v 'm[$k]'` and `read 'm[$k]'` now assign
the element bash assigns, whatever bytes `$k` holds. Regression test:
`assignment_target_subscript_may_hold_bytes_that_are_not_text` (`interp.rs`),
the write-side counterpart of step 8's `unset` test.

- `is_valid_assignment_target` is gone, replaced by
  `split_assignment_target(s: BStr<'_>) -> Option<(&str, Option<BStr<'_>>)>`
  (`interp.rs`, next to `nameref_target_base`). The two halves are typed
  differently on purpose: a base name is `[A-Za-z_][A-Za-z0-9_]*` and therefore
  ASCII by construction, so `&str` is exact rather than an approximation and it
  can key the name tables directly; a subscript is an ordinary shell word and
  stays bytes.
- `ScalarDest::Elem(String, String)` → `ScalarDest::Elem(String, Str)`, so a
  resolved destination carries the subscript's bytes rather than a re-spelling
  of them. `scalar_write_dest` now splits with `split_assignment_target` instead
  of hand-rolling `find('[')`/`strip_suffix(']')`, which also drops two string
  slicings.
- New `set_scalar_target_checked(&mut self, base: &str, sub: Option<BStr<'_>>,
  val: Str)` for callers that hold a *word* rather than a bare name, so they no
  longer paste the halves back together for `scalar_write_dest` to take apart.
  The readonly guard it shares with `set_scalar_checked` moved into
  `scalar_write_checked`. Only the plain-name arm follows the nameref chain,
  which is not a narrowing: no variable can be *named* `arr[0]`, so a
  subscripted target was never a nameref's name and the chain never applied.
- `read`'s two gates (the early first-name check and the per-field one) now
  split before judging, and the later-name store goes through
  `set_scalar_target_checked`.

**Step 2 done 2026-07-30.** `declare -n r="m[$k]"` now binds the element it
names, and reads and writes through the reference carry the subscript's bytes
both ways. Regression test:
`a_nameref_may_designate_an_element_whose_subscript_is_not_text`.

- `resolve_ref_name`/`resolve_ref_use` return
  `Option<RefTarget { base: String, sub: Option<Str> }>` instead of a single
  pasted `String`. Every caller that cared about an element reference used to
  take the spelling back apart with `find('[')`/`strip_suffix(']')`, which a
  subscript cannot survive as text. The walk itself is unchanged for plain
  names; an element value simply ends it, since no variable can be *named*
  `arr[0]` and so it can carry no nameref attribute of its own.
- Callers now ask for what they mean: `into_name()` (`None` for an element —
  which names no variable, exactly reproducing the lookup misses the pasted
  spelling used to produce), `.base` (the readonly attribute lives on the
  variable), or `spelling()` (`${!ref}`, diagnostics).
- `nameref_value_error` splits before judging, and the self-reference rule is
  measured against the base alone — which is what bash does, so
  `declare -n r=r[0]` is still a self-reference. `nameref_target_base` is gone.
- `ArithElem::Element(String)` → `Element(Str)`;
  `warn_elem_not_identifier(&str)` → `(BStr)`.

**Behaviour fixed on the way past**, because there is no longer a `String` in
which to smuggle a non-name:

- `declare -n r=arr[0]; r[1]=v` and `assign_elem` through such a reference give
  bash's ``  `arr[0]': not a valid identifier `` refusal instead of inventing a
  variable literally named `arr[0]`.
- `declare -n r=arr[0]; unset r` removes `arr[0]`. It used to look up a variable
  named `arr[0]`, find nothing, and quietly do nothing.
- `readarray r` through such a reference quotes the target as spelled.
