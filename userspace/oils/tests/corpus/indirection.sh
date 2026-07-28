# Indirect expansion `${!name}` and the prefix/key listing forms, which share
# the same `!` sigil but mean quite different things.
target=value
ptr=target
echo "indirect=${!ptr}"

# The indirection is one level only, and re-reads the *current* value.
ptr2=ptr
echo "one-level=${!ptr2}"
target=changed
echo "reread=${!ptr}"

# Indirection composes with the default/alternate operators.
unset nothing
p=nothing
echo "default=${!p-fallback}"
echo "alt=${!ptr+present}"

# `${!prefix*}` / `${!prefix@}` list *variable names* that start with prefix.
zzz_a=1
zzz_b=2
zzz_c=3
echo "names=${!zzz*}"
echo "names-at=${!zzz@}"
for n in ${!zzz@}; do echo "name=$n value=${!n}"; done

# For an array, `${!arr[@]}` lists indices — including the gaps a sparse array
# leaves behind.
arr=(x y z)
unset 'arr[1]'
echo "indices=${!arr[@]} values=${arr[*]} n=${#arr[@]}"
declare -A m=([alpha]=1 [beta]=2)
echo "assoc-keys-sorted=$(for k in "${!m[@]}"; do echo "$k"; done | sort | tr '\n' ' ')"

# Indirection through an array element reference.
first=arr[0]
echo "elem-indirect=${!first}"

# Indirection to a positional parameter.
set -- pos1 pos2
i=2
echo "positional=${!i}"

# An indirect reference to an unset name is an empty expansion (and, under
# `set -u`, an error — checked in set-options.sh).
unset novar
q=novar
echo "unset-indirect=[${!q}]"
