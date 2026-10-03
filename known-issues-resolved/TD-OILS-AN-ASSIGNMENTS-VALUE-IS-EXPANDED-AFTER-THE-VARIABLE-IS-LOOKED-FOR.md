### TD-OILS-AN-ASSIGNMENTS-VALUE-IS-EXPANDED-AFTER-THE-VARIABLE-IS-LOOKED-FOR. bash expands first and judges the name second — FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs`, `Shell::apply_assignment_inner` —
which resolved the target (and could refuse it) before expanding the right-hand
side. `do_assignment_internal` (subst.c:396) has the value in hand before it goes
looking for anything to put it in:

```c
  if (t = mbschr (name, '['))	/* ] */
    { … entry = assign_array_element (name, value, aflags, &estate); … }
  else
    entry = bind_variable (name, value, aflags);
```

So every objection the *name* earns — a reference designating a whole array, one
designating an element that a written subscript has nowhere to go on, a readonly
target — is reported only after the value's side effects have run:

```sh
f() { echo "  RAN($1)" >&2; echo "v$1"; }
declare -a n=(1 2); declare -n r1='n[@]'
r1=$(f whole)      # bash: RAN(whole) *then* n[@]: bad array subscript
readonly ro=1
ro=$(f ro)         # bash: RAN(ro) *then* ro: readonly variable
```

A **compound literal** is the one exception, and bash's own: it is not expanded
until `assign_compound_array_list`, which all of those come before — so
`readonly ra=1; ra=($(f c))` refuses without running `f`.

**The fix.** `apply_assignment_inner` gained a `pre: Option<Str>` parameter and
expands a scalar right-hand side at the top, before anything is resolved; the
scope-swap re-entry (`apply_assignment_expanded`) threads the already-expanded
value through instead of re-expanding it. That last part is not cosmetic:
`i1=${i1:=V}` re-expanded under the swap read the *target's* scope and answered
`V` where bash answers the value the caller's scope held.

**Corpus:** `an-assignments-value-is-expanded-before-the-variable-is-looked-for.sh`.

**How it was found:** measuring the per-context write walk (see the entry above),
whose `${x:=v}` form came out wrong for a reason that had nothing to do with the
walk.
