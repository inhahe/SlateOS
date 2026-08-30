# `${!ptr}` complains `invalid indirect expansion` when the *pointer* names no
# variable — and bash's question there is `find_variable_last_nameref (name, 0)
# != 0` (subst.c:7642,7657), the existence of a `SHELL_VAR`, not whether reading
# it produced any text. So every name that has a cell points perfectly well and
# the whole `${!ptr}` is simply an unset parameter, however empty the cell is:
# an associative array, an array built from `[1]` upwards, an empty `a=()`, a
# bare `declare`, an `export` never assigned. Only a name with no cell at all —
# and a function, which is not a variable — earns the complaint.
declare -A s=([k]=v)
t[1]=v
declare -a e0=()
declare -a d0
declare z
export EU
declare -i di
declare -r rr
fn() { :; }
# A subshell, because the complaint is *fatal*: the shell reading it exits, and
# `alive` is the only way to tell that apart from an empty expansion.
b() { printf '%-14s ' "$1"; c=$2; ( eval "printf '<%s>' $c"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
echo "--- a name with a cell but no value still points"
for p in s t e0 d0 z EU di rr; do b "\${!$p}" "\${!$p}"; b "\${!$p:-D}" "\${!$p:-D}"; done
echo "--- a name with no cell at all does not"
for p in nope fn; do b "\${!$p}" "\${!$p}"; b "\${!$p:-D}" "\${!$p:-D}"; done
echo "--- a function local counts while its frame is live"
g() { local lv; b '${!lv}' '${!lv}'; local -a la; b '${!la}' '${!la}'; }
g
b '${!lv}' '${!lv}'
echo "--- the complaint is about the pointer, never about what it points at"
d0x=one
b '${!d0x}' '${!d0x}'
b '${!d0x:-x}' '${!d0x:-x}'
echo "--- a positional or special pointer is never complained about"
( set --; b '${!9:-D}' '${!9:-D}'; b '${!9}' '${!9}' )
echo "--- and a subscripted pointer asks the same of the array part alone"
b '${!s[k]}' '${!s[k]}'
b '${!e0[0]}' '${!e0[0]}'
b '${!d0[0]}' '${!d0[0]}'
b '${!z[0]}' '${!z[0]}'
b '${!nope[0]}' '${!nope[0]}'
echo "--- under nounset the pointer is an unbound parameter instead"
( set -u; b '${!nope}' '${!nope}'; b '${!s}' '${!s}' )
echo TAIL
