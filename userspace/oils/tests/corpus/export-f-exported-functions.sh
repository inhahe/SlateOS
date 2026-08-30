# `export -f` — functions that cross into a child's environment.
#
# bash encodes an exported function as the environment entry
# `BASH_FUNC_<name>%%=() { <body> }`. The `%%` makes the entry unnameable by any
# shell word, so it cannot collide with a variable. The body is *not* the
# `declare -f` rendering: it is bash's `named_function_string(NULL, cmd, 0)`,
# which drops the name and the newline before `{`, and indents every nesting
# level by one space rather than four-per-level.
#
# `$BASH` names the running shell in both shells, so the round trip through a
# real child is testable here without the harness naming the binary.

echo '--- the encoding, one shape per section'

f1() { echo hi; }
f2() { echo a; echo b; }
f3() { { echo x; }; ( echo y ); }
f4() { echo z; } > /dev/null
f5() { echo "line1
line2"; }
f6() { if true; then for i in 1 2; do echo $i; done; else case $x in a) echo A;; esac; fi; }
f7() { :; }
export -f f1 f2 f3 f4 f5 f6 f7

for n in f1 f2 f3 f4 f5 f6 f7; do
  echo "== $n"
  printenv "BASH_FUNC_$n%%"
done

echo '--- an unexported function has no entry'
g() { echo nope; }
printenv 'BASH_FUNC_g%%'; echo "rc=$?"
env | grep -ac '^BASH_FUNC_g'

echo '--- the round trip: a child gets the function, still exported'
"$BASH" -c 'f1; f2; f5; declare -F | grep -a "^declare -f"'

echo '--- and a grandchild gets it too (re-encoded, not passed through)'
"$BASH" -c '"$BASH" -c "f1; declare -F f1"'

echo '--- the imported definition records source `environment` at line 0'
"$BASH" -c 'shopt -s extdebug; declare -F f1'

echo '--- attribute letters are listed f,r,t,x whatever order they were set'
h() { :; }
export -f h
declare -ft h
readonly -f h
declare -F | grep -a ' h$'
declare -F h

echo '--- listings filter on the export attribute'
declare -Fx | sort
echo "-- -Fxr"
declare -Fxr | sort

echo '--- `export -f` with no operands lists each definition then its attributes'
export -f
echo "-- -pf and -fp are the same listing"
export -pf | md5sum
export -fp | md5sum

echo '--- not a function: reported per name, keeps going, status 1'
export -f nosuch f1 2>&1
echo "rc=$?"
export -f h=1 2>&1
echo "rc=$?"
declare -fx nosuch 2>&1
echo "rc=$?"

echo '--- three ways to drop the attribute'
export -nf f2
declare +x -f f3
unset -f f4
declare -Fx | sort
echo "-- a redefinition after unset -f is not exported again"
f4() { echo z; }
declare -Fx | grep -ac ' f4$'

echo '--- and the dropped ones no longer reach a child'
"$BASH" -c 'declare -F | grep -a "^declare -f" | sort'

echo '--- an ordinary variable named like the encoding is not a function'
# Skipped in silence: the value does not begin `() {`, so it never looked like a
# definition. It is also invisible as a variable — the name is unspellable.
env "BASH_FUNC_v%%=plain text" "$BASH" -c 'declare -F v; echo "rc=$?"; echo "v=${v-unset}"' 2>&1

echo '--- a value that does look like a definition but does not parse is reported'
# The diagnostic is emitted while importing the environment, before `-c` has
# settled `$0`, so its prefix is whatever the shell calls itself at startup —
# bash uses its invocation path, osh its own name (TD-OILS-DOLLAR-ZERO-ARGV0).
# Normalised away here so the three-line *shape* is what is compared.
env "BASH_FUNC_w%%=() { if true; }" "$BASH" -c 'declare -F w; echo "rc=$?"' 2>&1 \
  | sed -e 's/^.*\(w: line 0:\)/SHELL: \1/' -e 's/^.*\(error importing\)/SHELL: \1/'
