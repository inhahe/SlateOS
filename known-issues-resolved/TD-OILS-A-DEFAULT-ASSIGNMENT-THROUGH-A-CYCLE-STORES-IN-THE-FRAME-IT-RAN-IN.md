### TD-OILS-A-DEFAULT-ASSIGNMENT-THROUGH-A-CYCLE-STORES-IN-THE-FRAME-IT-RAN-IN. `${a[0]:=w}` through a circular nameref inside a function writes a local, where bash writes the global — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, [`Shell::assign_elem_owing`]. Unlike
[`Shell::scalar_write_checked`], which wraps its store in
`self.in_dest_scope(dest, |sh| …)`, `assign_elem_owing` never enters the
resolved target's scope. So a `${name[sub]:=value}` whose walk fell back on a
frame-local binding stores *there* instead of where the chain points — and,
because the fallback is then seen as a live local array, the bound a negative
subscript counts back from is the local's rather than none at all.

```sh
$ f() { declare -n a=b; declare -n b=a; echo "[${a[0]:=w}]"; declare -p a b; }
$ ( f; echo --; declare -p a )
bash: [w] / declare -n a="b" / declare -n b="a" / -- / declare -a a=([0]="w")
osh : [w] / declare -an a=([0]="w") / declare -n b="a" / -- / declare: a: not found

$ g() { declare -n a=b; declare -n b=a; echo "[${a[-1]:=w}]"; }; ( g )
bash: a: bad array subscript … a[-1]: bad array subscript
osh : a: bad array subscript … [w]           # stored at index 0
```

At **top level** both shells already agree (the fallback *is* the global
there), and the sibling writes agree in a frame too — `read 'a[1]'` and
`a[1]=9` both reach the global — because those go through
[`Shell::scalar_write_checked`], which does enter the scope. It is only the
`:=` path that does not.

bash's rule for the count-back is `if (entry && ind < 0)`
(`assign_array_element_internal`, arrayfunc.c:398): when `find_variable
(vname)` returned null there is nothing to count back from, so the negative
subscript is refused outright. In osh the equivalent is
`assign_elem_owing`'s

```rust
let bound = if elem_invalid_name.is_some() || debt.unmade { 0 } else { self.elem_write_bound(name) };
```

and `debt.unmade` is false here — the `Some(target) if target.scope !=
RefScope::Live` arm of [`Shell::resolve_ref_elem_write_deferred`] — so the
frame-local scalar's bound of 1 is used and `a[-1]` lands on index 0.

**Fixed** in this commit. The body of [`Shell::assign_elem_owing`] below the
resolution — the readonly guard, the count-back bound, and the store, which
have to agree about *which* binding they are talking about — moved into a new
[`Shell::assign_elem_placed`], and the resolver now wraps it in
[`Shell::in_scope`] with the target's own scope. That is what
[`Shell::scalar_write_checked`] already did for the writes that are not
expansions; those callers arrive with the swap in place, and the re-walk in
`assign_elem_owing` then answers [`RefScope::Live`], so nothing nests.

Both arms move together: `${a:=w}` through a cycle in a function had been
overwriting the *frame's own reference value* (`declare -n a="w"`) where bash
leaves it standing and makes a global scalar.

The extra bound falls out with it. With the global binding live,
[`Shell::elem_write_bound`] sees no variable rather than the local nameref
scalar, answers 0, and `${a[-1]:=w}` underflows exactly as bash's
`if (entry && ind < 0)` has it.

Corpus:
`a-default-assignment-through-a-cycle-stores-in-the-frame-it-ran-in.sh`.
Unit test:
`a_default_assignment_through_a_cycle_stores_in_the_frame_it_ran_in`.

**How it was found:** writing the corpus for
TD-OILS-A-READ-THROUGH-A-CHAIN-THAT-REACHED-NOTHING-STILL-JUDGES-ITS-SUBSCRIPT,
whose `:=` rows had to be moved to top-level subshells to keep the file green.
