# `declare -f` prints a stored function back from the parsed body, and bash
# reproduces the *spelling* the source used, not a canonical one: quoting style,
# arithmetic form and the whitespace inside `for ((…))` all survive.
echo "=== quoting is printed back as written"
q() {
  echo a\ b
  echo 'a b'
  echo "a b"
  echo 'it'"'"'s'
  echo "say \"hi\""
  echo '$x'
  echo "\$x"
  echo \$x
  echo \*
  echo a\'b
  echo ''
  echo ""
  echo "a'b"
  echo "a\nb"
  echo $'a\'b'
}
declare -f q

echo "=== arithmetic keeps its spacing and its spelling"
a() {
  echo $((1+2))
  echo $(( 1 + 2 ))
  echo $[1+2]
  ((x=1+2))
  (( x = 1 + 2 ))
  echo $((x))
}
declare -f a

echo "=== a C-style for keeps each section from its first non-blank character"
c() {
  for ((i=0;i<2;i++)); do echo "$i"; done
  for (( i=0; i<2; i++ )); do echo "$i"; done
}
declare -f c

echo "=== a subshell body prints brace-wrapped with the parentheses inside"
s() ( echo one; echo two )
declare -f s

echo "=== compound commands and redirections on the definition"
r() {
  if [ -n "$1" ]; then echo yes; else echo no; fi
  while false; do echo never; done
  case $1 in
    a) echo A ;;
    *) echo other ;;
  esac
  for w in x y; do echo "$w"; done
} 2>/dev/null

declare -f r
