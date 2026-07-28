# Indexed and associative array semantics: sparse indices survive, `${!a[@]}`
# lists keys in index order, `${#a[@]}` counts elements (not the highest index),
# and unset of one element leaves a hole rather than compacting.
a=(zero one two)
a[7]=seven
echo "vals=${a[*]}"
echo "keys=${!a[@]}"
echo "count=${#a[@]}"
echo "len-of-one=${#a[1]}"
unset 'a[1]'
echo "after-unset keys=${!a[@]} vals=${a[*]}"

# Appending with += extends from the highest index + 1, not from count.
a+=(eight)
echo "appended keys=${!a[@]}"

# Slices are over the *positions* of existing elements, not raw indices.
echo "slice=${a[@]:1:2}"

# A bare scalar assignment to an array name writes element 0.
b=(x y)
b=z
echo "scalar-into-array=${b[*]} keys=${!b[@]}"

# Associative arrays: order is bash's internal hash order, so only probe by key.
declare -A m
m[alpha]=1
m['two words']=2
echo "m-alpha=${m[alpha]} m-two=${m['two words']} n=${#m[@]}"
echo "has-alpha=${m[alpha]+yes} has-nope=${m[nope]+yes}(end)"
unset 'm[alpha]'
echo "after-unset n=${#m[@]}"

# Unset without a subscript removes the whole array.
unset a
echo "gone=(${a[*]}) n=${#a[@]}"
