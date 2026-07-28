# `declare`/`local`/`readonly` attributes: integer, case-folding, arrays, and
# what `declare -p` prints back for each.
declare -i n
n=5+5
echo "int=$n"
n=oops
echo "int-bad=$n"
declare -i m=3*4
echo "int-init=$m"

# Case-folding attributes apply on every subsequent assignment.
declare -u up=hello
declare -l lo=WORLD
echo "up=$up lo=$lo"
up=changed
echo "up2=$up"

# `declare -p` round-trips the attribute set and the quoted value.
declare -p n up lo

# Arrays and associative arrays.
declare -a arr=(x y z)
declare -A map=([k1]=v1 [k2]=v2)
echo "arr=${arr[*]} n=${#arr[@]}"
echo "map k1=${map[k1]} k2=${map[k2]} n=${#map[@]}"
declare -p arr
echo "keys=$(for k in "${!map[@]}"; do echo -n "$k,"; done)"

# readonly: assignment fails with a diagnostic and a non-zero status, but the
# shell keeps running (non-interactive, no `set -e`).
readonly ro=fixed
ro=changed
echo "ro-status=$? ro=$ro"

# `declare` inside a function is local by default only with `local`; a bare
# `declare` in a function is also local in bash.
f() { declare inner=1; g=2; echo "in-f inner=$inner"; }
f
echo "after-f inner=[${inner-unset}] g=$g"

# `local` restores the previous value on return, including "was unset".
outer=keep
h() { local outer=temp; echo "in-h=$outer"; }
h
echo "after-h=$outer"

# -x exports; -n makes a nameref.
declare -x XV=exported
echo "exported=$(env | grep -c '^XV=exported')"
target=real
declare -n ref=target
echo "ref=$ref"
ref=viaref
echo "target=$target"
