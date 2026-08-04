# The order an associative array enumerates in is not insertion order and not
# sorted order: it is the walk of bash's own open hash table, buckets in index
# order and each bucket's chain from its head. `${!m[@]}`, `${m[@]}`,
# `declare -p` and `for k in "${!m[@]}"` all show it, so it is observable, and
# only the same table reproduces it — which is what `src/assoc.rs` now is.
#
# What the table's shape does to the order, and what each section below pins:
#
#   * FNV-1 (multiply then xor) over the key's bytes, xored in as a *signed*
#     char, masked to 1024 buckets — so `a b c` comes out `c b a` and the run
#     `key6 key7 key4 key5` is two collided pairs.
#   * a new entry goes at the *head* of its chain, so within one bucket the
#     order is the reverse of insertion — and so the insertion order shows in
#     the result exactly where keys collided, and nowhere else.
#   * the table grows when it holds more than two entries per bucket, ×4, and
#     the rehash pushes onto heads too, so a growth reorders far more than it
#     redistributes.
#   * removal only unlinks; the table never shrinks and nothing else moves.
#   * re-assigning a key writes through the entry rather than relinking it.

echo '--- the bucket walk ---'
unset m; declare -A m
for k in a b c; do m[$k]=1; done
printf 'abc:'; printf ' %s' ${!m[@]}; echo

unset m; declare -A m
for k in b a c; do m[$k]=1; done
printf 'bac:'; printf ' %s' ${!m[@]}; echo

unset m; declare -A m
i=0; while [ $i -lt 40 ]; do m[key$i]=$i; i=$((i+1)); done
printf 'seq40:'; printf ' %s' ${!m[@]}; echo
printf 'seq40v:'; printf ' %s' ${m[@]}; echo

unset m; declare -A m
for k in 1 2 3 10 20 100; do m[$k]=1; done
printf 'numeric:'; printf ' %s' ${!m[@]}; echo

echo '--- the head of the chain ---'
# `aaa`, `fan`, `jfk` and `pkb` all hash into the same bucket, so they are one
# chain and the insertion order is what orders them — reversed, the head being
# where a new entry goes.
unset m; declare -A m
for k in aaa fan jfk pkb; do m[$k]=1; done
printf 'chain:'; printf ' %s' ${!m[@]}; echo
unset m; declare -A m
for k in pkb jfk fan aaa; do m[$k]=1; done
printf 'chain-rev:'; printf ' %s' ${!m[@]}; echo
# Eight keys that collide with nothing, forwards and backwards: with no chain
# longer than one, the insertion order leaves no trace at all.
unset m; declare -A m
for k in alpha beta gamma delta epsilon zeta eta theta; do m[$k]=1; done
printf 'greek:'; printf ' %s' ${!m[@]}; echo
unset m; declare -A m
for k in theta eta zeta epsilon delta gamma beta alpha; do m[$k]=1; done
printf 'greek-rev:'; printf ' %s' ${!m[@]}; echo

echo '--- growth ---'
# 2100 keys is past 1024 buckets × 2 entries, so the table has grown once and
# every one of these keys was rehashed into the ×4 table. Printing all of them
# would drown the case, so this takes the ends and the count.
unset m; declare -A m
i=0; while [ $i -lt 2100 ]; do m[key$i]=v; i=$((i+1)); done
set -- ${!m[@]}
printf 'grown-count: %s\n' "$#"
printf 'grown-head:'; printf ' %s' "${@:1:12}"; echo
printf 'grown-tail:'; printf ' %s' "${@: -12}"; echo
# One below the threshold, for the boundary itself.
unset m; declare -A m
i=0; while [ $i -lt 2048 ]; do m[key$i]=v; i=$((i+1)); done
set -- ${!m[@]}
printf 'ungrown-head:'; printf ' %s' "${@:1:6}"; echo

echo '--- removal unlinks and nothing else moves ---'
unset m; declare -A m
i=0; while [ $i -lt 40 ]; do m[key$i]=v; i=$((i+1)); done
unset 'm[key7]' 'm[key20]' 'm[key0]'
printf 'after-unset:'; printf ' %s' ${!m[@]}; echo
printf 'after-unset-count: %s\n' "${#m[@]}"
# Re-adding goes to the head of the key's own bucket, not into the hole.
m[key7]=again
printf 'after-readd:'; printf ' %s' ${!m[@]}; echo

unset m; declare -A m
for k in a b c d; do m[$k]=$k; done
unset 'm[b]'
m[e]=e
printf 'abcd-b+e:'; printf ' %s' ${!m[@]}; echo

echo '--- a rewrite does not relink ---'
unset m; declare -A m
for k in x y; do m[$k]=1; done
m[x]=2; m[y]+=tail; m[z]=new
printf 'rewritten:'; printf ' %s=%s' ${m[@]@k}; echo

echo '--- an emptied array starts a fresh table ---'
unset m; declare -A m
i=0; while [ $i -lt 40 ]; do m[key$i]=v; i=$((i+1)); done
m=()
i=0; while [ $i -lt 8 ]; do m[key$i]=v; i=$((i+1)); done
printf 'reset:'; printf ' %s' ${!m[@]}; echo

echo '--- a high byte is a signed char ---'
# bash hashes through a `char *`, so `\xff` xors in as 0xffffffff, not 0xff.
unset m; declare -A m
for k in $'\xc3\xa9' $'\x80x' $'\xff' hi $'\x01\x02'; do m[$k]=1; done
printf 'high:'; printf ' %s' "${!m[@]}" | od -An -tx1 | tr -d '\n'; echo

echo '--- every reader shows the same order ---'
unset m; declare -A m
m[@]=1; m['a b']=2; m['#x']=3; m[plain]=4
declare -p m
printf 'keys:'; printf ' [%s]' "${!m[@]}"; echo
printf 'vals:'; printf ' [%s]' "${m[@]}"; echo
printf 'K: [%s]\n' "${m[@]@K}"
printf 'for:'; for k in "${!m[@]}"; do printf ' <%s=%s>' "$k" "${m[$k]}"; done; echo
printf 'slice:'; printf ' [%s]' "${m[@]:1:2}"; echo
