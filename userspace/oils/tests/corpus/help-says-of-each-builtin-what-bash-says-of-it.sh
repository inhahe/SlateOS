# `help -d NAME` prints bash's one-line description of a builtin and `help -s
# NAME` its usage synopsis. Both come out of the same table, and the wording is
# bash's own — down to which builtins share a line (`declare` and `typeset`,
# `test` and `[`) and which describe themselves in bash's older, terser phrasing
# (`dirs - Display directory stack.`, not "the directory stack").
#
# The reserved-word topics live in the same table and are asked for by the names
# bash gives them (`for ((`, `{ ... }`, `(( ... ))`, `[[ ... ]]`), so they are
# walked here too.

for n in $(compgen -b); do
  help -d -- "$n"
done

echo '=== and the synopsis of each'
for n in $(compgen -b); do
  help -s -- "$n"
done

echo '=== the reserved-word topics are in the same table'
for n in 'for' 'for ((' 'while' 'until' 'if' 'case' 'select' 'function' \
         'time' 'coproc' '{ ... }' '(( ... ))' 'variables'; do
  help -d -- "$n"
done

echo '=== and the long form of each: synopsis, description, blank, body'
for n in $(compgen -b); do
  help -- "$n"
done

echo '=== including the reserved words'
for n in 'for' 'for ((' 'while' 'until' 'if' 'case' 'select' 'function' \
         'time' 'coproc' '{ ... }' '(( ... ))' 'variables'; do
  help -- "$n"
done

echo '=== -d beats -m and -s, and -m beats -s'
help -sd cd
help -ds cd
help -md cd

echo '=== a pattern matches every topic that starts with it'
help -d 'read*'
help -s 'unal*'

echo '=== and a topic nothing matches is refused'
help -d 'nosuchtopic'; echo "rc=$?"

echo '=== done'
