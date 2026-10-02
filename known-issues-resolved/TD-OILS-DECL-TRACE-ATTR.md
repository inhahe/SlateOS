### TD-OILS-DECL-TRACE-ATTR. `declare -t` is accepted and ignored — 2026-07-30 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare`'s flag loop (the
catch-all `_ => {}` arm) and `attr_flag_letters`.

**What.** bash's `-t` marks a variable or function as *traced*: a traced function
inherits the caller's DEBUG and RETURN traps. osh parses the letter and drops it, so

```sh
declare -tirx v=1; declare -p v   # bash: declare -irtx v="1"   osh: declare -irx v="1"
```

and `declare -ft f` has no effect on trap inheritance.

**Proper fix.** A `trace_attr: HashSet<String>` alongside the other attribute sets,
its letter emitted between `r` and `x` in `attr_flag_letters`, and — the part with
actual behaviour behind it — `funcname`-scoped DEBUG/RETURN trap inheritance keyed
off it in the function-call path. Low priority: the letter is cosmetic, and trap
inheritance only matters to scripts that already use `declare -ft`.

**Fixed 2026-07-31.** The entry was stale on its most interesting half: the
*function* trace attribute — `declare -ft f` plus DEBUG/RETURN trap inheritance —
was already implemented (`Shell::fn_trace_attr`, `Shell::trap_suppress`,
`set_func_trace`) and matches bash. What was actually missing was the *variable*
attribute and the listings.

* `Shell::trace_attr` records `-t`/`+t` on a variable. It is inert in bash too,
  so it does nothing beyond being reported — but it is a real per-name attribute:
  snapshotted/restored by `local`, cleared by `unset`, inherited by subshell
  clones.
* Re-measuring the letter *order* found a second bug: bash emits a fixed
  `[aA] i n r t x [luc]`, not the order the flags were written, and osh had `n`
  before `i`. `declare -ni v` printed `declare -ni v` where bash prints
  `declare -in v`. Fixed in `attr_flag_letters`, which is also what `${v@a}` and
  `${v@A}` render through.
* A nameless `declare -t` is a listing filtered by the attribute (`listing_flags`
  / `listing_names` gained the letter).
* Function listings now render the attributes they carry: `declare -F` prints
  `declare -frt f`, and a nameless `declare -f` prints each body followed by that
  line when the function has an attribute. `-r`/`-t` in a *nameless* function
  listing are filters that union (`declare -Frt` = readonly ∪ traced), where
  `declare -fr NAME` is still the mutation. `declare -Ft` no longer routes to
  `set_func_trace` with an empty name list (a silent no-op); it lists.
* `declare -ft nope` was printing `nope: not found`. Measured bash prints nothing
  and exits 1 — the same silent failure `declare -fr nope` already did in osh.
  The old code's doc comment asserted the diagnostic; it was never measured.

Covered by `declare_t_records_the_trace_attribute_on_a_variable` and
`a_function_listing_reports_the_readonly_and_trace_attributes`; verified against
bash 5.2.37 over 18 variable cases and 9 listing forms.
