### TD-OILS-LOCAL-ARRAY-KIND-VALUED. `local -a` on an existing local scalar leaves the name unvalued where bash reports an empty array — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare`'s local path and
`array_kind_apply`.

**What.** Inside a function, `local x=5; local -a x; declare -p x` gives bash
`declare -a x=()` — the widening discards the scalar *and* still marks the name
valued. osh prints the bare `declare -a x`. At global scope the same pair is
handled correctly (`x=5; declare -a x` → `declare -a x=([0]="5")`, pinned by
`giving_a_scalar_an_array_kind_keeps_its_value_as_element_zero`); it is only the
second `local` on a name already local in the *same* frame that differs, because
that re-declaration resets the frame slot before `array_kind_apply` can carry
the old value in. Note bash does not carry it either here — it produces `()`,
not `([0]="5")` — so the fix is not to route the local path through
`array_kind_apply` but to mark the name valued when a re-`local` widens it.

**Proper fix.** In the local path, when `-a`/`-A` is applied to a name already
present in the current frame as a scalar, insert it into `array_valued` after
creating the (empty) array. Needs a probe first to confirm bash's behaviour when
the earlier `local` had no value at all (`local x; local -a x`).

**Fixed 2026-07-30 — and the probe the entry asked for found a bigger bug
underneath.** Probing the full matrix against bash 5.2.37 first (rather than
implementing the sketch above) showed the diagnosis was incomplete. Two separate
rules were wrong:

**1. A second `local` of a name in the same frame re-declares; it does not
re-shadow.** bash leaves the existing local binding's value *and* attributes
alone and applies the new flags on top, so `local -i n=5; local n` still reports
`declare -i n="5"` and `local -a a=(1 2); local a` keeps both elements. osh's
`declare_local` cleared unconditionally, so a re-declaration silently destroyed
the value the caller had just set — a data-losing bug, and the reason the entry's
symptom looked like a mere display flag. `declare_local` now returns early when
the name is already in the frame; the clearing runs only on the first `local`,
which is the one that actually shadows an outer binding (and there it clears the
value, the attributes, *and* the array kind, as bash does: `declare -a g=(1)`
then `local g` gives the plain `declare -- g`).

**2. Widening a *local* scalar to an array drops the value.** bash uses
`make_local_array_variable`, which rebinds the name as a fresh array rather than
converting the variable in place — so `local x=5; local -a x` is
`declare -a x=()`, while the global `x=5; declare -a x` keeps the 5 as
`([0]="5")`. The name is still *valued*, which is what selects the empty `=()`
over a bare declaration. The `drop_local_scalar` flag in `builtin_declare` is
read *after* the shadow step, so a first `local -a g` over a global `g=9` is
correctly unvalued — by then there is no local scalar to drop. Other attributes
survive the widening (`local -i n=5; local -a n` → `declare -ai n=()`).

All 13 cases in the probe matrix now match bash byte-for-byte. Covered by
`a_second_local_of_a_name_redeclares_rather_than_reshadows` and
`giving_a_local_scalar_an_array_kind_drops_its_value_but_not_its_valuedness`,
which pin both directions (re-declare preserves, first-local clears) plus the
unchanged global widening.
