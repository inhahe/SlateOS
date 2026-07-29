# A modifier written after `${!ref…}` does not simply attach itself to the
# variable the reference names. The reference is resolved to a *value* first,
# and the modifier is handed that. For an ordinary pointer the two amount to
# the same thing — the value at the name it read — but a nameref answers with
# the *name* it holds, and so it is the name the modifier goes to work on.
#
# The exception is a name that reaches an array, which the per-value modifiers
# look through to the variable so that they keep meaning what they mean there.
# The `${x:-…}` family never looks through: it works on whatever the reference
# itself expanded to, whether that is a name or a value.

echo "=== an ordinary pointer hands over the value it read"
t=hello
p=t
echo "  [${!p^^}][${!p#h}][${!p:-D}][${!p@Q}][${!p:0:2}][${!p/l/L}]"
# With nothing at the name, the modifiers see an unset parameter.
unset t
echo "  unset [${!p:-D}][${!p+S}][${!p#h}][${!p@Q}]"
# An assignment through the reference lands on the name that was read.
echo "  assign [${!p:=made}] t=[$t]"

echo "=== a nameref answers with the name, so the name is what is modified"
unset t
declare -n r=t
t=hello
echo "  [${!r}][${!r^^}][${!r#t}][${!r:-D}][${!r@Q}][${!r:0:1}]"
# The name is there to be read whether or not anything is stored under it, so
# the reference is never null and the `:=`/`:?` forms have nothing to do.
unset t
echo "  empty-target [${!r:+S}][${!r:=v}] t=[${t-unset}]"
echo "  [${!r@U}][${!r,,}][${!r%%t*}]"
# A chain is followed to its end, and the end is the name that is modified.
unset r
declare -n r=b
declare -n b=c
c=v
echo "  chain [${!r}][${!r^^}]"

echo "=== a name that reaches an array is looked through to the variable"
unset r b c
declare -n r=arr
arr=(one two)
# …but only by the per-value modifiers: `:-` still sees the name.
echo "  [${!r}][${!r^^}][${!r#o}][${!r:0:1}][${!r:-D}][${!r:+S}]"
declare -n e="arr[1]"
echo "  elem [${!e}][${!e^^}][${!e#t}][${!e:-D}]"
unset r e
declare -A h=([k]=hit)
declare -n r=h
echo "  assoc [${!r}][${!r^^}][${!r:-D}]"

echo "=== a pointer that names an element reads that element"
unset r h
b=(p q)
ptr="b[1]"
echo "  [${!ptr}][${!ptr^^}][${!ptr#q}][${!ptr:-D}][${!ptr@Q}]"
declare -A m=([key]=val)
ptr="m[key]"
echo "  assoc [${!ptr}][${!ptr^^}][${!ptr:-D}]"
# An element that is not there is an unset parameter, not an error.
ptr="b[9]"
echo "  gap [${!ptr}][${!ptr:-D}][${!ptr+S}]"
# A whole-array reference expands to all of it, and the modifier reaches all
# of it too.
ptr="b[@]"
echo "  all [${!ptr}][${!ptr^^}][${!ptr:-D}]"
ptr="b[*]"
echo "  star [${!ptr^^}]"

echo "=== assigning through a reference needs a name to assign to"
# Reading through an element reference is fine; storing through one is not,
# and bash says so about the name it resolved rather than the one written.
ptr="b[1]"
( echo "  [${!ptr:=v}]" ) ; echo "  rc=$?"
# A reference that reached nothing has no name at all to store under.
a=(x)
( echo "  [${!a[9]:=v}]" ) ; echo "  rc=$?"
# Which is not to say the reference cannot be *read* through: it simply
# expands to nothing, the way an unset parameter does.
echo "  read [${!a[9]:-D}][${!a[9]+S}][${!a[9]#z}]"
