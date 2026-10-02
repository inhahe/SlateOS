### TD-OILS-PREFIX-DUPLICATE-NAME-RESTORE. A name assigned twice in one prefix restored an intermediate value — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::push_temp_shadow`.

**What:** `w=a; w=b w=c eval :; echo "$w"` left `w` as `b` instead of `a`. osh
recorded a `VarSnapshot` per *assignment*, so a repeated name stacked two
bindings and `pop_temp_shadow` restored them in order, ending on the
intermediate. bash's temporary environment holds one variable per name.

**Fixed 2026-08-04.** Only the first occurrence of a name in a prefix records
what it displaced; a repeat overwrites the value in place. Confirmed against
bash 5.2.37:

- What comes back is what stood there **before the prefix**, never an
  intermediate — and the `unset` that reveals a prefix binding from inside the
  command agrees with the restore, both showing that same value
  (`q=1; q=2 q=3 eval 'unset q; echo $q'` → `1`). osh's `unset` was already
  right, which is what made the mismatch visible.
- A name unset before the prefix is unset again after it, and a name that held
  an array comes back an array — it is displaced only once.
- The repeats are still each an assignment of their own: traced separately under
  `set -x`, and refused separately when the name is readonly.

**Pinned by** `tests/corpus/a-name-assigned-twice-in-one-prefix-is-one-binding.sh`
and the `a_name_assigned_twice_in_one_prefix_is_one_binding` unit test in
`src/interp.rs`.
