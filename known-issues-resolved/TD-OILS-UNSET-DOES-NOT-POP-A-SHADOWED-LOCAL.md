### TD-OILS-UNSET-DOES-NOT-POP-A-SHADOWED-LOCAL. `unset` of an outer frame's local marks it unset where bash removes the binding, and a current-frame local loses its name entirely — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::unbind_var`, and
`Shell::local_frames`.

**What:** bash's `unset NAME` pops the *innermost* binding of `NAME` off the
variable scope stack, exposing the next one; only when that binding belongs to
the **current** function frame is it merely *marked* unset instead. osh always
marks, and its mark also drops the name from `declared`, so `declare -p` cannot
see it. Two separate divergences, both in bash's **default** mode
(`localvar_unset` off). Measured against bash 5.2.37:

```sh
# (a) a current-frame local stays a *name* after unset
c=global
f() { local c=one; unset c; declare -p c; }
f            # bash: declare -- c        osh: declare: c: not found
             # a second and third `unset c` change nothing in either shell

# (b) an enclosing frame's local is popped, not marked
a=global
i1() { unset a; echo "[$a]"; }
i2() { local a=two; i1; echo "[$a]"; }
i2           # bash: [global] [global]   osh: [] []
# …and the frame entry is gone, so a later write lands on the exposed binding
# and survives the outer return: bash ends with a=fromi1 where osh restores
# a=global.
```

Three frames deep, `unset` from the innermost pops one level per call:
`l3 (local a=three) → l2 (local a=two) → l1 (unset a)` leaves `three` visible,
and a second `unset` leaves the global. The popped binding's attributes come
back with it (`declare -i b=1` global under a `local b=two` reads back as
`declare -i b="1"`). A readonly local is refused before any of this, which osh
already matches.

`shopt -s localvar_unset` is what turns (b) into (a) — "identical to the
behavior of unsetting local variables at the current function scope", as the
manual puts it — so osh is currently hard-wired to the *non-default* half of
that option, and gets the default wrong.

**Fixed 2026-08-04.** `unbind_var` now decides, before it clears anything,
which `local_frames` entry owns the innermost binding of the name, and acts on
that instead of only on the live tables:

- No frame owns it → a global, or an assignment prefix's binding: removed
  outright, or handed to `reveal_temp_shadow`, both as before.
- The **last** frame owns it → marked unset: the value and array go, but the
  name is put back into `declared`, so `declare -p` still reports
  `declare -- c` and the binding is still there to take a later write.
- An **earlier** frame owns it and `localvar_unset` is off → popped: that
  frame's saved `VarSnapshot` is restored and the entry *removed*, so the next
  `unset` pops the level below and the frame's return restores nothing.
- An earlier frame owns it and `localvar_unset` is on → marked, as for the last
  frame.

Assignment prefixes have their own pop (`reveal_temp_shadow`), and making the
two agree turned out to be most of the work. Measured, all bash 5.2.37:

- **The two pops must not both run.** A prefix scope that opened *inside* the
  owning frame is the nearer binding, so the frame is not the owner after all
  and `reveal_temp_shadow` does the pop; one *outside* it stays hidden under
  what the frame pop revealed, so `reveal_temp_shadow` has to be skipped.
  Scopes record `local_frames.len()` when they open, which is exactly the
  comparison that separates the two.
- **A call's temporary environment is the same scope level as that call's
  locals.** bash keeps the two in one variable context, so `local q=2` under a
  `q=3 f` writes over the prefix's binding rather than shadowing it (which
  `declare_local`'s `inherit_temp` already modelled), and one `unset` from a
  deeper frame pops *both*: `q=1; f() { local q=2; g; }; g() { unset q; }; q=3 f`
  reveals the global `q=1`, not the prefix's `q=3`. This needed a new
  `TempScope::call_env` flag, because `local_depth` alone cannot tell `q=3 f`
  from `q=3 eval 'f'` — the latter is a scope of its own that the function
  called underneath it shadows normally, and popping the local there does
  reveal `q=3`.
- **The marking keeps the export attribute of such a binding, and only of such
  a binding** — bash's "preserve the export attribute if the variable came from
  a temporary environment". `q=1; f() { local q=2; unset q; declare -p q; }; q=3 f`
  reports `declare -x q`, and a later `q=w` in the frame `declare -x q="w"`,
  where the same frame's `local -x q=2` with no prefix in force reports the
  bare `declare -- q`. Every other attribute goes.
- **With no local in the way, the prefix's binding is popped even in the frame
  it belongs to**: `q=1; f() { unset q; q=w; }; q=3 f` leaves `q=w` at top
  level. It is the *local* that the current frame marks rather than removes.

**Pinned by** `tests/corpus/an-unset-of-a-local-pops-one-shadow.sh` and the
`unset_of_a_local_pops_one_shadow` unit test in `src/interp.rs`.
