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

echo "=== a local cannot shadow a readonly name, even with no value"
# bash's `make_local_variable` refuses outright rather than binding a mutable
# copy, so the valueless declaration reports and abandons the operand — while
# the same words at top level are a plain attribute update.
readonly ro=5
declare ro; echo "  top declare rc=$?"
declare -i ro; echo "  top declare -i rc=$?"
declare -p ro
f1() { declare ro; echo "  declare rc=$?"; }; f1
f2() { local ro; echo "  local rc=$?"; }; f2
f3() { declare -i ro; echo "  declare -i rc=$?"; }; f3
f4() { local -a ro; echo "  local -a rc=$?"; }; f4
f5() { declare +i ro; echo "  declare +i rc=$?"; }; f5
f6() { declare ro=6; echo "  valued rc=$?"; }; f6
# Only a *shadow* is refused: these three reach the readonly global itself.
f7() { declare -g ro; echo "  declare -g rc=$?"; }; f7
f8() { export ro; echo "  export rc=$?"; }; f8
f9() { readonly ro; echo "  readonly rc=$?"; }; f9
# The refusal costs only its own operand.
f10() { declare ro other=7; echo "  rc=$?"; declare -p other; }; f10
f11() { declare other2=7 ro; echo "  rc=$?"; declare -p other2; }; f11
# A name already local to this frame is re-declared, not shadowed afresh.
f12() { local -r x=1; declare x; echo "  same-frame declare rc=$?"; }; f12
f13() { local -r x=1; local x; echo "  same-frame local rc=$?"; }; f13
# A nested call still sees the outer frame's readonly local as unshadowable.
f14() { local -r y=1; g14; }; g14() { local y; echo "  nested local rc=$?"; }; f14
declare -p ro

echo "=== but a readonly *local* is shadowed freely by a deeper call"
# It belongs to a call that is still running, and the new local is a separate
# writable binding — so the value, the flags and later writes all land on it,
# and the outer readonly is back when the call returns.
s1() { local -r y=1; t1; echo "  after y=[$y]"; }; t1() { local y; echo "  local rc=$? y=[${y-unset}]"; }; s1
s2() { local -r y=1; t2; }; t2() { declare y=9; echo "  declare y=9 rc=$? y=[$y]"; }; s2
s3() { local -r y=1; t3; echo "  after y=[$y]"; }; t3() { local y=9; y=7; echo "  write rc=$? y=[$y]"; }; s3
s4() { local -r y=1; t4; }; t4() { local -a y=(9); echo "  local -a rc=$?"; declare -p y; }; s4
s5() { local -r y=1; t5; }; t5() { declare -A y=([k]=9); echo "  declare -A rc=$?"; declare -p y; }; s5
# `+r` meets a fresh local that was never readonly, so it is the plain no-op.
s6() { local -r y=1; t6; }; t6() { declare -i +r y; echo "  -i +r rc=$?"; declare -p y; }; s6
# `declare -r` inside a function makes a *local* readonly, so the same applies.
s7() { declare -r j=1; t7; }; t7() { local j; echo "  local rc=$? j=[${j-unset}]"; }; s7
# Re-declaring a name this frame already holds is not a shadow: the operand
# meets the readonly binding itself, and is refused.
s8() { local -r x=1; local x=9; echo "  same frame rc=$? x=[$x]"; }; s8
s9() { local -r x=1; declare +r x; echo "  same frame +r rc=$?"; }; s9
# `readonly` inside a function marks the *global*, which is unshadowable again.
s10() { readonly h=1; t10; }; t10() { local h; echo "  after readonly h rc=$? h=[$h]"; }; s10
# A compound operand naming a readonly global reports twice — once from the
# assignment machinery, tagged with the function, once from the builtin.
readonly rog=5
s11() { local -a rog=(1); echo "  compound rc=$?"; declare -p rog; }; s11
s12() { declare -ga rog=(1); echo "  -ga rc=$?"; }; s12
