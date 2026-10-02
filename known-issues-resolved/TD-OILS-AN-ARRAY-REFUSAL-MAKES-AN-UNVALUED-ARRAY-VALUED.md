### TD-OILS-AN-ARRAY-REFUSAL-MAKES-AN-UNVALUED-ARRAY-VALUED. `declare -a g; declare +a g=5` left `declare -a g=()` where bash leaves the bare `declare -a g` — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, `Shell::builtin_declare_scoped`. Two
sites, both spelled `if value.is_some() { self.array_valued.insert(…) }`: the
`cannot destroy array variables in this way` refusal, and the `assoc && indexed`
self-conflict refusal below it.

**What it is.** Whether `declare -p` prints an array as `=()` or bare is decided
**once**, when the variable is made — a refusal raised against it afterwards
never changes the answer. `declare -p` prints the `=()` only for a variable that
is not `att_invisible`, and the only place a declaration sets that flag is the
global creation branch, declare.def:802:

```c
  if (var == 0)
    { … var = mkglobal ? … : make_new_variable (…);
      if (offset == 0) VSETATTR (var, att_invisible); … }
```

— inside `var == 0`, and only when the operand carried **no** value. So a global
array this command made *with* a value is visible, one made without is not, and
one the command merely *found* keeps whatever it had. A local is never made
visible this way: `make_local_variable` returns an existing same-context local
untouched (`/* local foo; local foo; is a no-op. */`, variables.c:2622) and flags
a fresh one invisible at the end (:2757), which `make_local_array_variable` does
not undo. `-g` is off the local branch and answers like the global spellings.

**Reproduce.**

```sh
declare -a g; declare +a g=5; declare -p g
#  bash: declare -a g
#  osh : declare -a g=()

declare -A ex; declare -aA ex=5; declare -p ex     # the second site
#  bash: declare -A ex
#  osh : declare -A ex=()
```

The neighbours pin the rule down and already agree: `declare -a g=(); declare +a
g=5` keeps `=()` because it was valued to begin with, `declare -a g=(1); declare
+a g=5` keeps `([0]="1")`, `declare +a 'g[1]=5'` gives `declare -a g=()` because
*this* command created it with a value, `declare -aA fresh` (no value) gives the
bare `declare -A fresh`, and the conversion refusal beside the two (`declare -a
g; declare -A g=5`) never inserts in osh either.

**The fix.** Both inserts are now `value.is_some() && !name_existed &&
!make_local` — bash's `var == 0 && offset != 0` on the global branch. That was
already the shape of the third site a few branches down, the subscript-creation
one, whose comment records the same two limits.

**Corpus:** `an-array-refusals-visibility-is-decided-when-the-array-was-made.sh`.
Unit test: `an_array_refusals_visibility_is_decided_when_the_array_was_made`.
