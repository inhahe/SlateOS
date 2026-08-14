# An array's kind is refused by whichever command is asked to change it — and
# under a scope swap that is **not** the compound literal.
#
# `make_internal_declare` runs first and refuses at declare.def:876. The literal
# that follows takes one of `do_compound_assignment`'s two *scoped* branches
# (subst.c:3484, 3513), and neither of those has a refusal in it:
# `make_local_assoc_variable`/`convert_var_to_assoc` and their indexed twins
# replace or convert without a word. Only the unscoped `else` branch —
# `assign_array_from_string` → `find_or_make_array_variable` (arrayfunc.c:504) —
# reports, and that is the road taken where there is no swap at all.
#
# The two commands do not resolve the same name, so *which* variable is asked
# is the whole of it.

echo '=== no swap: the literal refuses, in the untagged spelling'
( declare -a g=(7); declare -A g=([k]=v); declare -p g )
( declare -A g=([q]=1); f() { declare -a g=(1 2); declare -p g; }; f )

echo '=== a swap, and the two commands agree on the name'
( declare -A g=([j]=9); f() { readonly -a g=(x y); declare -p g; }; f )
( declare -a g=(7); f() { declare -gA g=([k]=v); declare -p g; }; f )
( f() { local -a g=(9); readonly -A g=([k]=v); declare -p g; }; f )

echo '=== a restart parts them, and step 1 is the one that is asked'
# `z` is unset, so step 1 makes it and refuses nothing — and the literal, kept
# home by `chklocal`, converts the frame's own indexed array without a word.
( declare -n g=z
  f() { local -a g=(9); readonly -A g=([k]=v); declare -p g; }; f; declare -p z )
( declare -n g=z
  f() { local -a g=(9); export -A g=([k]=v); declare -p g; }; f; declare -p z )
# `z` already holds the kind that was asked for, so again nothing refuses.
( declare -n g=z; declare -A z=([q]=1)
  f() { local -a g=(9); readonly -A g=([k]=v); declare -p g; }; f; declare -p z )
# And when `z` holds the *other* kind it refuses — naming `g`, the name as
# written, the restart not having happened.
( declare -n g=z; declare -a z=(7)
  f() { local -a g=(9); readonly -A g=([k]=v); declare -p g; }; f; declare -p z )
( declare -n g=z; declare -A z=([q]=1)
  f() { local -a g=(9); readonly -a g=(k v); declare -p g; }; f; declare -p z )

echo '=== the refusal ends the parse unit, and everything after it with it'
# `readonly` is no `LOCALVAR_BUILTIN`, so its word carries no `W_FORCELOCAL`
# and step 1's failure is `exp_jump_to_top_level (DISCARD)` (subst.c:12762):
# the builtin never runs, and neither does anything else the shell had left.
( declare -n g=z; declare -a z=(7)
  f() { local -a g=(9); readonly -A h=([k]=v) g=([k]=v); declare -p h; }
  f; echo UNREACHED; declare -p h )
# Where nothing refuses, the whole word list binds.
( declare -n g=z
  f() { local -a g=(9); readonly -A g=([k]=v) h=1; declare -p g h; }; f; declare -p z )

echo '=== the conversion replaces the storage, carrying nothing across'
# `convert_var_to_assoc` (subst.c:3520) builds a new hash and drops the old
# array — there is no element 0 holding what was there, as there is when
# `declare -a` widens a *scalar*.
( declare -n g=z
  f() { local -a g=(1 2 3); readonly -A g=([k]=v); declare -p g; }; f )
