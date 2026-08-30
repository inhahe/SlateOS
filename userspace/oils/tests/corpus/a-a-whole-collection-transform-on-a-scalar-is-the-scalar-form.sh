# `${x[@]@A}` asks a collection-shaped question, and a scalar is a collection
# one element deep — so `[@]` names the scalar itself and the answer is the
# *scalar* form, `s='a b'`. It is tempting to reach for `declare -p` here, since
# on an array the two agree exactly; on a scalar they do not. `declare -p`
# double-quotes its value and always leads with `declare --`, while the scalar
# form single-quotes and drops the declaration entirely for a name that carries
# no attributes.
#
# An empty answer is *no field* rather than one empty field. `declare t` has
# nothing to recreate, so `${t[@]@A}` contributes no word at all — where the
# positional form's empty attribute strings each still count, so `${@@a}` is one
# empty word per parameter. The difference only shows in the count, which is why
# `$#` is what gets printed below rather than the expansion.
#
# The variables the shell computes are scalars with no storage cell, and they
# take the same route: their letters come from the shell's own table, so
# `${SECONDS[@]@a}` is `i` and `${SECONDS[@]@A}` recreates it with that `-i`.
# Their values are host state, so only the shapes are printed here.

show() { printf '  count=%s' "$#"; printf ' [%s]' "$@"; echo; }

echo "=== a scalar's collection form is its scalar form"
s='a b'
echo "[${s@A}] [${s[@]@A}] [${s[*]@A}]"
declare -i iv=7
echo "[${iv@A}] [${iv[@]@A}]"
declare -u uv=abc
echo "[${uv@A}] [${uv[@]@A}]"
declare -r rv=z
echo "[${rv@A}] [${rv[@]@A}]"
# Declared and never assigned recreates the bare declaration either way.
declare -x xx
echo "[${xx@A}] [${xx[@]@A}]"

echo "=== …which is not what declare -p says"
declare -p s iv

echo "=== an empty answer contributes no word, unlike an empty one"
declare t
set -- "${t[@]@A}"; show "$@"
set -- "${nope[@]@A}"; show "$@"
set -- "${s[@]@a}"; show "$@"
# The positional form counts its empties, so this is the one place the two part.
set -- a b c
set -- "${@@a}"; show "$@"
set -- a b c
set -- "${*@a}"; show "$@"

echo "=== an attributed scalar has letters, so it does contribute one"
set -- "${iv[@]@a}"; show "$@"
set -- "${xx[@]@a}"; show "$@"

echo "=== the shell's own scalars take the same route"
# The letters live in the shell's table rather than in the variable, and the
# value is host state — so the value is cut out and only the shape compared.
for v in SECONDS RANDOM BASHPID PPID UID PWD SHLVL LINENO EPOCHSECONDS; do
  printf '%-14s a=[%s] A=[%s]\n' "$v" \
    "$(eval "printf '%s' \"\${$v[@]@a}\"")" \
    "$(eval "printf '%s' \"\${$v[@]@A}\"" | sed "s/=.*/=…/")"
done
# And the count follows the letters: no letters, no `@a` word.
set -- "${SECONDS[@]@a}"; show "$@"
set -- "${LINENO[@]@a}"; show "$@"

echo "=== an array is the case where the two forms do agree"
declare -a n=(x y)
declare -A m=([k]=v)
echo "[${n[*]@A}]"
declare -p n
echo "[${m[*]@A}]"
declare -p m
# Declared but never assigned, and assigned empty, are both still arrays.
declare -a du
declare -a em=()
echo "[${du[*]@A}] [${em[*]@A}]"
