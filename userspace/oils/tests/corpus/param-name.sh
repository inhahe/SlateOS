# What counts as the parameter name inside `${…}`.
#
# The `#` of `${#…}` is a length operator only when a *complete* parameter
# reference follows it; otherwise the `#` is itself the parameter `$#` and the
# remainder is an operator on it. `$#`, `$?` and `$-` in turn accept only an
# operator after the name, and a `[subscript]` belongs to an identifier alone.
set -- aB cD eF
v=abcde
a=(x yy)

# A whole parameter reference: these are lengths.
echo "${#@}|${#*}|${##}|${#1}|${#9}|${#v}|${#a[1]}|${#a[@]}|${#nosuch}"

# Something left over: `$#` with an operator.
echo "[${#+x}][${#:+x}][${#-x}][${#:-x}][${#=x}]"
echo "[${##3}][${###}][${##*}][${#%3}][${#:0:1}][${#:1}]"
echo "[${#/3/z}][${#//3/z}][${#@Q}][${#@U}]"

# Neither reading works: bad substitution, each reported once.
for b in '${#^}' '${#,,}' '${#[0]}' '${#v@Q}' '${#v:1}' '${#v#a}' '${#v/a/b}' \
         '${#v-x}' '${#1v}' '${#v w}' '${# v}' '${#$v}' '${#a[1]@Q}' \
         '${?^}' '${?[0]}' '${-,}' '${-[0]}' \
         '${@[0]}' '${*[0]}' '${$[0]}' '${0[0]}' '${1[0]}'; do
  ( eval "echo \"[$b]\"" ) 2>&1 | sed 's/^.*: line [0-9]*: //'
done

# The unrestricted specials keep working.
echo "[${@^}][${*^}][${1^}][${v^}][${v[0]}][${?@Q}][${-@Q}][${#a[0]}]"
