### TD-OILS-ARITH-ARRAY-LITERAL-PARTIAL-COMMIT. A compound array assignment that fails part-way discards the whole literal, where bash keeps what it had already stored — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `Shell::apply_assignment`, the
`AssignRhs::Array(items)` and `AssignRhs::Assoc(items)` arms.

**What.** osh's array-literal path collects the elements into a local
`BTreeMap` and installs it in one go at the end (`self.arrays.insert`), so an
element that fails mid-literal — the only way that happens is a bad `-i`
expression, now that `apply_value_attrs` refuses to store one (see
BUG-OILS-ARITH-WHO-EXPANDS) — throws away the whole assignment and leaves the
previous array untouched. bash binds each element as it goes, so the elements
before the failure survive, and for a non-append *indexed* literal the clearing
survives too. Measured (bash 5.2), each probe reading on a following line
because the failure is fatal to its list:

| written | bash leaves | osh leaves |
|---|---|---|
| `declare -ia b=(5 6)` then `b=(1 1/0 3)` | `(1)` — cleared, then `[0]=1` | `(5 6)` |
| `declare -ia d=(7 7)` then `d=([2]=1 [3]=1/0 [4]=5)` | `([2]=1)` | `(7 7)` |
| `declare -ia c=(7 7 7)` then `c+=(1 1/0 3)` | `(7 7 7 1)` | `(7 7 7)` |
| `declare -iA n=([a]=9)` then `n=([x]=1 [y]=1/0)` | `([a]=9)` — untouched | `([x]=1)` |
| `declare -iA n=([a]=9)` then `n+=([x]=1 [y]=1/0)` | `([x]=1 [a]=9)` | `([x]=1 [a]=9)` ✓ |

Note the fourth row: an **associative** literal with `=` is *atomic* in bash —
neither the flush nor the earlier elements happen — while `+=` on the same array
is incremental, and both *indexed* forms are incremental. So bash appears to
build a fresh hash table for the assoc-`=` case and swap it in at the end, and to
bind in place everywhere else. osh currently has the atomic behaviour everywhere,
which is right for exactly one of the five shapes.

**Fixed (2026-07-30).** Measuring the ordering turned up a *second*, unrelated
divergence in the same code and named the real structure: a compound assignment
has three stages, not one.

1. The **whole literal is expanded** — before anything is cleared. Both flavours
   need this: `a=(1 2); a=(9 ${a[0]})` gives `(9 1)`, `c=(7 7 7);
   c=([${#c[@]}]=x)` puts `x` at index 3, and `declare -A h=([k]=old);
   h=([n]=${h[k]})` keeps `old`. osh's indexed path got this right by accident
   (it accumulated into a local map), but the **associative path cleared the
   table first**, so `h=([n]=${h[k]})` read an already-emptied `h` and stored
   the empty string. That was the second bug, and it needed no failure at all to
   show.
2. A non-append literal **clears** the variable.
3. Each element is **bound in turn**, and an `-i` value is evaluated as it is
   bound — which is the only stage that can fail.

So the fix splits `apply_assignment`'s two literal arms along those stages. The
indexed arm expands into a `Vec<(Option<i64>, String)>` (`None` = positional,
taking the running index at bind time), then clears, then writes each element
straight into `self.arrays` — so a bail-out leaves the clearing and the earlier
elements behind. The associative arm moves its clearing to *after* the loop and,
for `=` only, holds the computed pairs in a `pending` vec that is installed once
the whole list is known; `+=` binds in place as it goes. That reproduces all five
rows above, including the asymmetry between the two associative forms.

Nothing other than a bad `-i` value can fail mid-literal: a keyed element whose
subscript is out of range still only warns and skips that element (`continue`),
which is unchanged.

**Tests.** `interp::tests::a_compound_array_literal_binds_in_three_stages` (all
three stages, both flavours, both append modes, plus the empty literal) and
`tests/corpus/array-literal-partial.sh`. The corpus probes read *named* keys
rather than `${!m[*]}`, because associative key order is bash's hash order and
osh's insertion order — see TD-OILS-ASSOC-ORDER, which is not this bug.
