# A whole set read through `${!r}` is normally exempt from `set -u` — it is a
# list, and an empty list is a legitimate answer. But bash grants that exemption
# only where the set could have come apart into fields, and a `*`-spelled one
# cannot do that inside quotes: `"$*"` is one word. So a *quoted* reference whose
# target is `*` or `name[*]`, and whose set has no elements, is an unbound
# variable — named after the reference, `!r`, never after the target it reached.
#
# The `@`-spelled targets are exempt however they are read, and it is the number
# of elements that decides rather than the text they join to: one empty element
# is a set with something in it.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( set -u; eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== quoted, with nothing in the set"
q 'set --; r="*"; echo "[${!r}]"'
q 'declare -a a; r="a[*]"; echo "[${!r}]"'
q 'a=(); r="a[*]"; echo "[${!r}]"'
q 'declare -A m; r="m[*]"; echo "[${!r}]"'
q 'unset a; r="a[*]"; echo "[${!r}]"'
q 'IFS=; set --; r="*"; echo "[${!r}]"'

echo "=== unquoted the same set is exempt"
q 'set --; r="*"; echo [${!r}]'
q 'declare -a a; r="a[*]"; echo [${!r}]'
q 'set --; r="*"; echo "pre"${!r}"post"'
q 'set --; r="*"; for i in ${!r}; do :; done; echo ok'
q 'set --; r="*"; b=(${!r}); echo "[${b[*]}]"'

echo "=== an @-spelled target is exempt either way"
q 'set --; r="@"; echo "[${!r}]"'
q 'set --; r="@"; echo [${!r}]'
q 'declare -a a; r="a[@]"; echo "[${!r}]"'
q 'declare -A m; r="m[@]"; echo "[${!r}]"'

echo "=== it is the element count, not the text they join to"
q 'set -- ""; r="*"; echo "[${!r}]"'
q 'a=(""); r="a[*]"; echo "[${!r}]"'
q 'IFS=; a=("" ""); r="a[*]"; echo "[${!r}]"'
q 'declare -A m; m=([k]=""); r="m[*]"; echo "[${!r}]"'

echo "=== a direct whole-set reference is exempt however it is written"
q 'declare -a a; echo "[${a[*]}]"'
q 'set --; echo "[$*][$@]"'
q 'declare -A m; echo "[${m[*]}]"'

echo "=== the modifiers that can raise it, and the family that cannot"
q 'set --; r="*"; echo "[${!r-D}][${!r:-D}][${!r+S}][${!r:+S}]"'
q 'set --; r="*"; echo "[${!r^^}]"'
q 'set --; r="*"; echo "[${!r,,}]"'
q 'set --; r="*"; echo "[${!r@Q}]"'
q 'set --; r="*"; echo "[${!r#x}]"'
q 'set --; r="*"; echo "[${!r%x}]"'
q 'set --; r="*"; echo "[${!r/x/y}]"'
q 'set --; r="*"; echo "[${!r:1:2}]"'
q 'declare -a a; r="a[*]"; echo "[${!r-D}][${!r:-D}][${!r+S}]"'
q 'declare -a a; r="a[*]"; echo "[${!r^^}]"'
q 'declare -a a; r="a[*]"; echo "[${!r#x}]"'
q 'declare -a a; r="a[*]"; echo "[${!r:1:2}]"'

echo "=== and unquoted no modifier raises it"
q 'set --; r="*"; echo [${!r^^}][${!r#x}][${!r:1:2}]'
q 'declare -a a; r="a[*]"; echo [${!r^^}][${!r#x}]'

# A context that expands a word *whole* is the other way a `*` cannot come
# apart, and bash extends the fault to it — but only for the bare `*` spelling.
# `name[*]` asks about quoting alone, so an unquoted one stays exempt in every
# context below. (bash's `chk_atstar`: the `*` arm consults
# `expand_no_split_dollar_star`, the `name[*]` arm only `quoted`.)
echo "=== a context that never splits counts as quoted, for the bare star only"
q 'set --; r="*"; v=${!r}; echo "[$v]"'
q 'declare -a a; r="a[*]"; v=${!r}; echo "[$v]"'
q 'set --; r="*"; a[0]=${!r}; echo "[${a[0]}]"'
q 'set --; r="*"; [[ -n ${!r} ]]; echo ok'
q 'declare -a a; r="a[*]"; [[ -n ${!r} ]]; echo ok'
q 'set --; r="*"; case x in ${!r}) ;; *) ;; esac; echo ok'
q 'declare -a a; r="a[*]"; case x in ${!r}) ;; *) ;; esac; echo ok'
q 'declare -a a; r="a[*]"; cat <<< [${!r}]'
q 'set --; r="*"; printf "<%s>" ${y:-${!r}}; echo; echo ok'
q 'declare -a a; r="a[*]"; printf "<%s>" ${y:-${!r}}; echo; echo ok'

# A here-document body and an arithmetic string are *quoted* rather than merely
# unsplit, and bash faults on both spellings in them. Those two contexts are
# left out here because osh diverges from bash over what an unbound variable
# does in them *at all*, whatever it is a reference to — see known-issues
# TD-OILS-NOUNSET-IN-A-REDIRECTION-WORD and TD-OILS-NOUNSET-SKIPS-ARITHMETIC —
# so a case here would be measuring those instead of this rule.
#
# A word bash still splits stays exempt, and the assignment-shaped arguments
# of `declare`/`export`/`local`/`readonly` are ordinary words, not assignments.
echo "=== a redirection target and a declaration argument still split"
q 'set --; r="*"; : > x${!r}y; echo ok; rm -f xy'
q 'set --; r="*"; declare v=${!r}; echo ok'
q 'set --; r="*"; export v=${!r}; echo ok'
q 'set --; r="*"; f() { local v=${!r}; echo ok; }; f'
q 'set --; r="*"; readonly v=${!r}; echo ok'

echo "=== it aborts the shell the way any other unbound variable does"
q 'set --; r="*"; echo "[${!r}]"; echo tail'
q 'set --; r="*"; echo [${!r}]; echo tail'

echo "=== with nounset off it is simply empty"
q 'set +u; set --; r="*"; echo "[${!r}][${!r-D}]"'
q 'set +u; declare -a a; r="a[*]"; echo "[${!r}]"'

echo "=== and a set with something in it is never in question"
q 'set -- p q; r="*"; echo "[${!r}]"'
q 'a=(p q); r="a[*]"; echo "[${!r}][${!r^^}]"'
echo "=== done"
