# Under `set -u`, a nameref designating an array **element** — or a whole array
# — is exempt from the unset-parameter fault **unbraced, and only unbraced**.
#
# bash reads `$name` and `${name}` with two different functions, and only the
# braced one asks whether the parameter is set: `param_expand` answers an array
# reference out of its own branch, with the empty string, and never reaches the
# check. So `echo "$V"` prints nothing and carries on where `echo "${V}"` faults
# — and so does every braced form built on it, the trims, the case operators,
# the slices and the transforms.
#
# What the base names does not come into it. An unset base, and a circular one,
# are exempt too; what matters is only that the reference resolves to a
# subscripted target at all. A chain ending on a plain name is not one, and
# still faults.
#
# The `-`/`:-`/`+`/`:+` operators are a separate question and answer it the
# other way: the element *is* unset for them, so `${V-DEF}` is `DEF`.

run() { echo "--- $1"; ( set -u; eval "$2" ); echo "s=$?"; }

n=(a b c)
declare -A mm=([k]=K)
declare -n V='n[9]'          # missing element of a real array
declare -n W='n[1]'          # element that is there
declare -n M='mm[zz]'        # missing key
declare -n G='k[@]'          # whole array, base unset
declare -n Q='nosuch[0]'     # element of an unset base
declare -n E='nosuch'        # plain name, not an element
declare -n b1='n[9]'; declare -n a1=b1
declare -n c1=c2; declare -n c2=c1
declare -n R='c1[0]'         # element of a circular base

echo '=== unbraced is exempt'
run 'missing element' 'echo "[$V]"'
run 'present element' 'echo "[$W]"'
run 'missing assoc key' 'echo "[$M]"'
run 'whole array of an unset base' 'echo "[$G]"'
run 'element of an unset base' 'echo "[$Q]"'
run 'through a chain' 'echo "[$a1]"'
run 'circular base' 'echo "[$R]"'
run 'unquoted, as a for list' 'for x in $V; do echo "x=[$x]"; done; echo ok'
run 'concatenated in a word' 'echo "[pre$V post]"'

echo '=== braced is not'
run 'braced' 'echo "[${V}]"'
run 'braced through a chain' 'echo "[${a1}]"'
run 'trim' 'echo "[${V#a}]"'
run 'case operator' 'echo "[${V^^}]"'
run 'slice' 'echo "[${V:0:1}]"'
run 'transform' 'echo "[${V@Q}]"'
run 'both spellings in one word' 'echo "[$V${V}]"'

echo '=== a reference that is not to an element still faults'
run 'plain unset target' 'echo "[$E]"'
run 'plain circular, unbraced' 'echo "[$c1]"'
run 'plain circular, braced' 'echo "[${c1}]"'
run 'ordinary unset variable' 'echo "[$zzz]"'
run 'a written element' 'echo "[${n[9]}]"'

echo '=== the operators still call the element unset'
run 'dash' 'echo "[${V-DEF}]"'
run 'colon-dash' 'echo "[${V:-DEF}]"'
run 'plus' 'echo "[${V+SET}]"'
run 'colon-plus' 'echo "[${V:+SET}]"'
run 'question' 'echo "[${V?msg}]"'
run 'length' 'echo "[${#V}]"'
run 'test -v' '[[ -v V ]] && echo yes || echo no'
run 'arithmetic' 'echo "[$(( V + 1 ))]"'
run 'indirect' 'q=V; echo "[${!q}]"'
