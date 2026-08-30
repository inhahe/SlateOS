# `${!ref}` through a nameref hands back the *name* the reference holds rather
# than indirecting a second time, and it reads that name off whatever
# `find_variable_last_nameref` walked to (subst.c:7642). That walk hands back
# the *last link* it followed, so a chain whose end names nothing still points:
# `declare -n n=nosuch` yields `nosuch`. What it does not survive is a reference
# with nothing in its cell (variables.c):
#
#     newname = nameref_cell (v);
#     if (newname == 0 || *newname == '\0')
#       return ((vflags && invisible_p (v)) ? v : (SHELL_VAR *)0);
#
# — the indirection asks with `vflags == 0`, so the answer is a null variable and
# the name is treated as though it did not exist at all.
b() { printf '%-14s ' "$1"; c=$2; ( eval "printf '<%s>' $c"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
echo "--- a reference with no cell"
declare -n n1
b '${!n1}' '${!n1}'
b '${!n1:-D}' '${!n1:-D}'
b '${!n1-D}' '${!n1-D}'
b '${!n1@Q}' '${!n1@Q}'
b '${!n1#x}' '${!n1#x}'
b '${n1}' '${n1}'
b '${n1:-D}' '${n1:-D}'
echo "--- and the forms that are not indirections at all still answer"
b '${!n1[@]}' '${!n1[@]}'
b '${!n1@}' '${!n1@}'
b '${!n1*}' '${!n1*}'
echo "--- a chain that reaches one"
declare -n n3=n1
b '${!n3}' '${!n3}'
b '${n3:-D}' '${n3:-D}'
echo "--- a chain whose end merely names nothing does point"
declare -n n2=nosuch
b '${!n2}' '${!n2}'
b '${n2:-D}' '${n2:-D}'
declare u1; declare -n n6=u1
b '${!n6}' '${!n6}'
b '${!n6:-D}' '${!n6:-D}'
declare -n n7=tgt; tgt=T
b '${!n7}' '${!n7}'
unset tgt
b '${!n7}' '${!n7}'
echo "--- a chain that resolves, and one that names an element"
v=V; declare -n n4=v; declare -n n5=n4
b '${!n4}' '${!n4}'
b '${!n5}' '${!n5}'
declare -a ar=(x y); declare -n er=ar[1]; declare -n er2=er
b '${!er}' '${!er}'
b '${!er2}' '${!er2}'
# A reference that names *itself* is a different question — its cell is not
# empty, so this is not the shape being measured here. A cycle closing on a
# local escapes to global scope instead; see
# a-nameref-cycle-that-closes-on-a-local-resolves-at-global-scope.sh.
echo "--- a local reference with no cell"
g() { local -n l1; b '${!l1}' '${!l1}'; }
g
echo "--- a subscripted reference through one is the array question instead"
b '${!n1[0]}' '${!n1[0]}'
b '${!n3[0]}' '${!n3[0]}'
b '${!n2[0]}' '${!n2[0]}'
echo TAIL
