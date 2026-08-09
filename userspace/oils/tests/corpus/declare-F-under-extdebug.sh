# `shopt -s extdebug` is what makes `declare -F NAME` worth asking: the name is
# followed by where the shell watched the definition being read — the line it
# began on, then the source label, which is the script's path, the sourced
# file's path as it was written, or `environment` for `-c` and stdin. The line
# is the definition's own, so a body spread over several lines still names the
# one the `f()` was on, and a definition inside `eval` counts within the eval'd
# text on top of the line the `eval` itself was reached at.
#
# Only the *named* form says so. `declare -F` with no names prints its
# `declare -f NAME` attribute lines exactly the same either way, and so does
# `declare -pF`. `-F` beats `-f` whichever order they are written in, and a name
# that is no function is still a silent status 1.
n() { sed 's/^.*: line [0-9]*: //'; }
q() { ( eval "$1"; echo "rc=$?" ) 2>&1 | n; }

echo "=== the name alone until extdebug is on"
f() { :; }
declare -F f
shopt -s extdebug
declare -F f
shopt -u extdebug
declare -F f
shopt -s extdebug

echo "=== the line is the definition's own"
g() {
  :
}
declare -F g
h() { :; }; i() { :; }
declare -F h i

# bash's `function_dstart` is written by the lexer in two places and the later
# one wins: on a `)` that closes a `(` following a WORD (parse.y:3580), and on
# the word right after the `function` keyword (parse.y:5349). A `\<newline>`
# between the pieces tells the two rules apart — and `function NAME ()` shows
# the first rule firing again on top of the second. None of them is the line the
# *body* opens on, which is what a call stands on instead.
echo "=== which token carries the stamp"
w1 \
  () { :; }
declare -F w1
w2( \
  ) { :; }
declare -F w2
function \
  w3 { :; }
declare -F w3
function w4 \
  () { :; }
declare -F w4
function w5 \
  () \
  { :; }
declare -F w5

echo "=== a redefinition moves it, and unset -f forgets it"
declare -F f
f() { echo again; }
declare -F f
unset -f f
declare -F f; echo "gone=$?"

echo "=== a definition made from inside something else"
j() { k() { :; }; }
j
declare -F k
eval "
m() { :; }"
declare -F m
( n2() { :; }; declare -F n2 )
declare -F n2; echo "sub=$?"

echo "=== and one read from a sourced file"
mkdir -p lib
printf '\n\nsourced() { :; }\n' > lib/defs.sh
. lib/defs.sh
declare -F sourced
. ./lib/defs.sh
declare -F sourced

echo "=== the nameless listing is the same either way"
q 'shopt -s extdebug; declare -F'
q 'declare -F'
q 'shopt -s extdebug; declare -pF'

echo "=== -F beats -f in either order"
q 'shopt -s extdebug; declare -Ff g'
q 'shopt -s extdebug; declare -fF g'
q 'shopt -s extdebug; declare -f g'
q 'shopt -s extdebug; declare -F nosuch; echo "n=$?"'
echo "=== done"
