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

# The letters come back in bash's *internal attribute-table* order, not the order
# they were written: the kind, then `n`, `i`, `r`, `x`, and last the case-folding
# trio `l`/`u`/`c`. `${v@a}` and `${v@A}` use the same order.
declare -alrx ord1=A
declare -p ord1
echo "at=${ord1@a} A=${ord1@A}"
declare -Aurx ord2
declare -p ord2
declare -cirx ord3=q
declare -p ord3
echo "at=${ord3@a}"
declare -lx ord4=q
declare -p ord4

# A *scalar* operand of an array declaration binds index/key 0 — and the value
# attributes fold or evaluate it there just as they would a scalar's value, `+=`
# included (numeric addition under `-i`, concatenation of the folded text
# otherwise).
declare -al sa=QQ
declare -p sa
declare -Au sb=qq
declare -p sb
declare -ac sc=hELLO
declare -p sc
declare -ai sd=2+3
declare -p sd
declare -ail se=Q
declare -p se
declare -ai sf=5
declare -ai sf+=3
declare -p sf
declare -al sg=AB
declare -al sg+=CD
declare -p sg
declare -au sh=(x)
declare -au sh+=yy
declare -p sh

# A bad `-i` value discards the command, leaves an array valued-but-empty (a
# scalar merely created-but-unset), and abandons every operand after it.
declare -ai bad1=2+ nope1=1
echo "rc=$?"
declare -p bad1
declare -p nope1 2>/dev/null
echo "nope1=$?"
declare -i bad2=2+ nope2=1
declare -p bad2
declare -p nope2 2>/dev/null
echo "nope2=$?"
declare -ai bad3[0]=2+ nope3=1
declare -p bad3
declare -p nope3 2>/dev/null
echo "nope3=$?"

# -x exports; -n makes a nameref.
declare -x XV=exported
echo "exported=$(env | grep -c '^XV=exported')"
target=real
declare -n ref=target
echo "ref=$ref"
ref=viaref
echo "target=$target"
