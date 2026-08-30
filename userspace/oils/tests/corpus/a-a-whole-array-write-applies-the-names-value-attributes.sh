# Filling a whole array is not one write; it is a series of ordinary element
# assignments. So the array's value attributes shape every one of them, exactly
# as they shape `a[0]=…`.
#
# `declare -ai k; read -a k <<< '3+4 5*2'` therefore leaves `7 10` and not the
# text it read, `-u`/`-l`/`-c` fold each element's case, and a malformed `-i`
# element is the store's own refusal: it is signed by the builtin that named
# the array and it abandons the command where it stands, so the elements before
# it stay and the ones after it are never assigned. The array having already
# been emptied, what is left behind is a prefix — unless `-O` was given, which
# does not empty it, and then the survivors outside the written range are still
# there too.
#
# The attributes are the *target's*. A nameref carries none of its own, and a
# name that is not an array yet keeps the scalar ones it was declared with.

echo "=== every element is attributed, for each builtin that fills one"
for b in 'read -a k' 'mapfile -t k' 'readarray -t k'; do
  echo "--- $b"
  ( declare -ai k; $b <<< '3+4'; declare -p k ) 2>&1
  ( declare -au k; $b <<< 'ab'; declare -p k ) 2>&1
  ( declare -al k; $b <<< 'AB'; declare -p k ) 2>&1
  ( declare -ac k; $b <<< 'ab'; declare -p k ) 2>&1
done

echo "=== every field, and the raw one -N reads"
( declare -ai k; read -a k <<< '3+4 5*2'; declare -p k ) 2>&1
( declare -au k; read -a k <<< 'ab cd'; declare -p k ) 2>&1
( declare -ai k; read -N 3 -a k <<< '3+4'; declare -p k ) 2>&1
( declare -au k; read -N 2 -a k <<< 'ab'; declare -p k ) 2>&1
( declare -ai j; mapfile j <<< '1+1'; declare -p j ) 2>&1

echo "=== the attributes are the target's"
( declare -ai t; declare -n rr=t; read -a rr <<< '2*3'; declare -p t ) 2>&1
( declare -i w; read -a w <<< '1+1'; declare -p w ) 2>&1
( declare -a plain; read -a plain <<< '1+1'; declare -p plain ) 2>&1

echo "=== a malformed -i element stops the write where it stands"
declare -ai k=(z z z)
read -a k <<< '1 q+ 3'
declare -p k
declare -ai j=(z z z)
mapfile -t j <<< $'1\nq+\n3'
declare -p j
declare -ai p=(9 9 9 9)
mapfile -t -O 1 p <<< $'1\nq+\n3'
declare -p p
echo still-here

echo "=== …and it abandons the rest of the command"
( declare -ai k; read -a k <<< 'q+ 5'; echo NOT-REACHED ) 2>&1; echo "rc=$?"
( declare -ai k; mapfile -t k <<< 'q+'; echo NOT-REACHED ) 2>&1; echo "rc=$?"
