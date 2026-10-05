### TD-OILS-A-COMPOUND-LITERAL-REFUSES-A-KIND-CHANGE-THE-BUILTIN-BEFORE-IT-HAS-ALREADY-MADE. `readonly -A g=([k]=v)` over a frame-local indexed array refuses where bash converts — 2026-08-12 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/interp.rs`, [`Shell::array_kind_conflict`] /
[`Shell::compound_kind_refusal`] and their caller in
[`Shell::declare_compounds_scoped`]. osh checks the kind conflict against the
name the **compound literal** binds. bash checks it in a different command:

* Step 1, `make_internal_declare` (`declare -gAG g`), refuses at
  declare.def:876 — `builtin_error` — and a failed step 1 **discards the whole
  parse unit** for `readonly`/`export`, so the elements never arrive.
* Step 2, `do_compound_assignment`, does **not** refuse in either of its
  scoped branches (subst.c:3484, 3513): `make_local_assoc_variable` and
  `convert_var_to_assoc` replace or convert without a word. Only the unscoped
  `else` branch — `assign_array_from_string` → `find_or_make_array_variable`
  (arrayfunc.c:504) — reports, and that one is a `report_error`, so it carries
  no builtin tag.

So where step 1's own name resolution takes it *elsewhere*, nothing refuses at
all and the literal simply converts the frame's array:

```sh
( declare -n g=z                          # step 1 restarts on `z`
  f() { local -a g=(9); readonly -A g=([k]=v); declare -p g; }; f )
# bash: declare -Ar g=([k]="v" )   — the local indexed array converted
# osh : g: cannot convert indexed to associative array
```

**Proper fix.** Move the kind refusal onto step 1 — i.e. ask it of
[`Shell::global_bind_names`]' answer for the *builtin* half, before the literal
runs — and let the literal's own binding convert rather than refuse. The
existing refusal then keeps firing for the plain-assignment road (`g=([k]=v)`
and top-level declarations), which is the `find_or_make_array_variable` one.

**Affected:** 4 shapes of the 240-shape kind matrix (`/tmp/kind_matrix.sh`).

**Narrowed 2026-08-14** to `readonly`/`export` alone. Fixing
TD-OILS-THE-SHAPE-AND-FOLD-LETTERS-ARE-APPLIED-BY-THE-LITERALS-HALF-ONLY-AND-NOT-BY-THE-BUILTIN-AFTER-IT
put the `declare` family's step 3 back on the operand loop and its own restart,
so the same shape spelled with `declare` now agrees:

```sh
( declare -n g=z; f() { local -a g=(9); declare -gGA g=([k]=v); declare -p g; }; f
  declare -p z )
# declare -a g=([0]="9")  /  declare -A z=([k]="v" )   — osh and bash alike
```

`readonly`/`export` still diverge because their step 3 is *not*
`declare_internal` and so is deliberately excepted from that routing; their step
1 still has no voice of its own here. Measured ground truth for the fix (bash
5.2.37) — the refusal is asked of **step 1's** name, and the answer turns on
what that name already holds:

```sh
declare -n g=z;                  f() { local -a g=(9); readonly -A g=([k]=v); }; f
# declare -Ar g=([k]="v" ) and declare -A z    — `z` unset, so no conflict
declare -n g=z; declare -a z=(7); f() { local -a g=(9); readonly -A g=([k]=v); }; f
# g: cannot convert indexed to associative array — `z` is indexed, so it refuses
```

Note the second row: the refusal must be raised *before*
[`Shell::make_empty_global`] runs, since that would otherwise put an empty
associative entry beside the indexed `z` it was asked to refuse.

**Fixed 2026-08-14.** The refusal is asked of step 1's name, and the literal
converts rather than refuses:

* [`Shell::in_declare_global_scope`] records, for each compound operand, the
  name **step 1** resolved to — `global_chain_path(name).last()`, the
  `nameref_cell (refvar)` of declare.def:735-741, falling back on the name
  where that walk found no reference at all. It is read *before*
  [`Shell::enter_global_scope`], because the swap is about to rewrite the chain
  it is read from, and stashed in the new `Shell::declare_step1_names` for the
  length of the expansion half. `None` there means no swap, which is exactly
  when the literal takes `do_compound_assignment`'s unscoped `else` branch and
  so does own the refusal.
* [`Shell::declare_compounds_scoped`] asks [`Shell::array_kind_conflict`] of
  that name instead of the literal's target. The *lookup* still happens after
  the swap, which is what puts the global binding of a name step 1 read
  globally in reach; only the name is carried across. The
  `make_empty_global` ordering worry above turns out not to arise — the empty
  variable step 1 makes is made in the kind that was asked for, so it can never
  be the conflict — but it is moot either way, since a `carried` name is only
  produced where the chain reached *nothing*, and a name nothing answers to has
  no kind to conflict with.
* Past the refusal with the other kind still in the table is a road only the
  swap opens, and there the literal *converts*: `convert_var_to_assoc` /
  `convert_var_to_array` (subst.c:3520-3527) replace the storage rather than
  reinterpret it, so `declare_compounds_scoped` now drops the other kind's
  entry before [`Shell::array_kind_apply`] runs. Nothing is carried in as
  element 0 — that is what `declare -a` over a *scalar* does, not this.

Corpus: `the-kind-of-an-array-is-refused-by-the-command-that-would-convert-it.sh`;
test `the_kind_of_an_array_is_refused_by_the_command_that_would_convert_it`.
The one shape still diverging is split out below as
TD-OILS-AN-ASSOCIATIVE-LITERALS-WORDS-ARE-NOT-REQUOTED-WHEN-THE-CONVERSION-GOES-THE-OTHER-WAY.

**How it was found:** writing the corpus case
`the-restart-happens-before-chklocal-so-step-one-outlives-the-frames-own-binding.sh`;
the `local -a g=(9)` section had to be dropped from it.
