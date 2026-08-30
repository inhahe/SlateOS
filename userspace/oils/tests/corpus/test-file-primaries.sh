# Every `test`/`[` primary that asks a question about a *file* answers "no"
# when there is no file — including the ones a Windows host cannot really
# answer (device nodes, FIFOs, sockets, the three extra mode bits). The two
# string primaries `-n`/`-z` are the exception: they ask about the operand.
e() { sed 's/^.*: line [0-9]*: //'; }

touch f
mkdir -p d
: > empty
printf 'x' > full
ln -s f sl 2>/dev/null
ln -s nowhere broken 2>/dev/null

echo "=== the whole primary table over a file, a directory and a nonexistent name"
for op in -a -b -c -d -e -f -g -h -k -p -r -s -u -w -x -G -L -N -O -S; do
  printf '%s:' "$op"
  for t in f d nosuch; do
    if [ $op "$t" ]; then printf ' %s=0' "$t"; else printf ' %s=1' "$t"; fi
  done
  echo
done

echo "=== …and the same table through [[ ]]"
for op in -a -b -c -d -e -f -g -h -k -p -r -s -u -w -x -G -L -N -O -S; do
  printf '%s:' "$op"
  for t in f d nosuch; do
    if [[ $op "$t" ]]; then printf ' %s=0' "$t"; else printf ' %s=1' "$t"; fi
  done
  echo
done

echo "=== -s is about size, not existence"
( test -s empty; echo "empty=$?"; test -s full; echo "full=$?"; test -s nosuch; echo "n=$?" ) 2>&1 | e

echo "=== -e follows a symlink; -L and -h do not"
( test -e sl; echo "rc=$?"; test -L sl; echo "rc=$?"; test -h sl; echo "rc=$?" ) 2>&1 | e
( test -e broken; echo "rc=$?"; test -L broken; echo "rc=$?"; test -f broken; echo "rc=$?" ) 2>&1 | e

echo "=== -a is 'exists' in the operator position and AND in the connective one"
( test -a f; echo "rc=$?" ) 2>&1 | e
( test -a nosuch; echo "rc=$?" ) 2>&1 | e
( test x -a y; echo "rc=$?" ) 2>&1 | e
( test x -a ''; echo "rc=$?" ) 2>&1 | e
( test -a f -a -a d; echo "rc=$?" ) 2>&1 | e

echo "=== -n and -z ask about the string"
( test -n nosuch; echo "rc=$?"; test -z nosuch; echo "rc=$?" ) 2>&1 | e
( test -n ''; echo "rc=$?"; test -z ''; echo "rc=$?" ) 2>&1 | e

echo "=== a negated primary is the primary's opposite, not a bare-word test"
( test ! -p nosuch; echo "rc=$?" ) 2>&1 | e
( test ! -S nosuch; echo "rc=$?" ) 2>&1 | e
( test ! -k f; echo "rc=$?" ) 2>&1 | e

echo "=== an empty operand names no file"
( test -e ''; echo "rc=$?"; test -f ''; echo "rc=$?"; test -p ''; echo "rc=$?" ) 2>&1 | e

echo "=== done"
