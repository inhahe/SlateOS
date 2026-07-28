# The `readonly` attribute as a *refusal*: every way of putting a value into a
# variable has to ask permission first, and the interesting part is not that the
# answer is no but what each writer does with a no — where the message comes
# from, what status is left behind, and how much of the surrounding work is
# abandoned. Those three answers are not the same for any two writers.
#
# Diagnostics are compared on their own stream, so a missing or extra complaint
# shows up without being mixed into stdout. A bare `readonly -p` is deliberately
# never used: it dumps the shell's own identity variables, which the two shells
# have no reason to agree on.

echo "=== a plain assignment gives up the rest of its list"
readonly r=1
r=2; echo "  not reached"
echo "  rc=$? r=$r"
# `+=` is the same assignment and is given up the same way.
r+=x; echo "  not reached either"
echo "  rc=$? r=$r"
# But only the list: the script itself carries on, and so does a *prefix*
# assignment's command, which runs with its status of its own.
r=9 true; echo "  prefix rc=$? r=$r"

echo "=== a builtin that assigns reports it as its own failure and returns 1"
# Here the complaint carries the builtin's name, because the builtin is the one
# refusing, and nothing beyond that call is given up.
declare r=3; echo "  declare rc=$?"
typeset r=4; echo "  typeset rc=$?"
f() { local r=5; echo "  local rc=$?"; }; f
unset r; echo "  unset rc=$?"
# `export NAME` without a value only adds an attribute, so there is nothing to
# refuse; `export NAME=v` is an assignment again.
export r; echo "  export-bare rc=$?"
export r=6; echo "  export rc=$?"
echo "  still r=$r"

echo "=== the writers that take their value from somewhere else"
# These name the *variable* rather than themselves — the attribute is the
# variable's property, not the builtin's doing — and leave the value alone.
read r <<< hi; echo "  read rc=$? r=$r"
printf -v r zz; echo "  printf-v rc=$? r=$r"
mapfile r <<< hi; echo "  mapfile rc=$? r=$r"
# `getopts` writes its answer into the name, so a refusal costs the answer its
# status: 2 where it would have been 0, but still 1 when the 1 is the answer.
set -- -a
getopts ab r; echo "  getopts rc=$? r=$r"
set --
OPTIND=1
getopts ab r; echo "  getopts-end rc=$? r=$r"

echo "=== arithmetic stops where it is refused, keeping what it already did"
# The complaint is about the variable, so it carries neither the `((`/`let` tag
# nor the expression that was being evaluated.
(( r=2 )); echo "  arith rc=$? r=$r"
let r=3; echo "  let rc=$? r=$r"
# The refusal aborts the expression mid-way: `y` is assigned, `z` is not.
(( y=1, r=2, z=3 )); echo "  comma rc=$? y=$y z=[${z-unset}]"
for (( r=0; r<1; r++ )); do echo "  no body"; done
echo "  for-arith rc=$?"
# `$(( ))` is an expansion, so its refusal is fatal to the command list the way
# a division by zero is.
echo "  sub=$(( r=4 ))"; echo "  not reached"
echo "  after rc=$? r=$r"

echo "=== a loop variable is bound like anything else"
# The refusal happens at the first attempt to bind, so a loop over an *empty*
# list never gets that far and is silent. Either way the loop is given up with
# status 1 and the list carries on.
for r in a b; do echo "  no body"; done
echo "  for rc=$? r=$r"
for r in; do echo "  no body"; done
echo "  for-empty rc=$? r=$r"

echo "=== the attribute is the array's, not the element's"
declare -a arr=(p q); readonly arr
arr[0]=X; echo "  not reached"
echo "  elem rc=$? arr=${arr[*]}"
(( arr[1] = 9 )); echo "  arith-elem rc=$? arr=${arr[*]}"
declare -A m=([k]=v); readonly m
m[k]=X; echo "  not reached"
echo "  assoc rc=$? m=${m[k]}"

echo "=== readonly and export take an identifier and nothing more"
# Which text a rejected operand is quoted back as is decided by how far the scan
# for the `=` gets — `a[0]=9` reaches one, `h[a=1` does not.
readonly 'a[0]=9'; echo "  sub rc=$?"
readonly '1bad=2'; echo "  badname rc=$?"
export 'h[a=1'; echo "  unbalanced rc=$?"
export ''; echo "  empty rc=$?"
# One bad name costs only itself; the good ones on the same command are declared.
readonly '1bad=2' okname=7; echo "  mixed rc=$?"
declare -p okname

echo "=== declaring is not assigning"
# A bare `readonly n` brings the name into being without giving it a value, so
# `declare -p` reports it while everything that lists *set* variables does not.
readonly ra rb=2
declare -p ra rb
readonly -p ra rb
echo "  in-set=$(set | grep -c '^ra=')"
echo "  prefix=[${!ra@}]"
echo "  compgen=$(compgen -v | grep -c '^ra$')"
[ -v ra ]; echo "  -v rc=$?"
echo "  expand=[${ra-DEF}]"
# …and it is readonly already, so the value it never got can never arrive.
ra=late; echo "  not reached"
echo "  late rc=$? ra=[${ra-unset}]"
