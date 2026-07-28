# Namerefs (`declare -n` / `local -n`): a variable whose *value* is the name of
# another variable, so reads and writes of the reference reach the target.
#
# The focus here is the part osh used to skip entirely — validating the value at
# declaration time. bash refuses a value that cannot name a variable, and
# refuses a reference to the variable's own name, so a nameref that could never
# resolve is never created in the first place. Diagnostics go to stderr and the
# harness compares stderr and status separately from stdout, so a missing or
# extra message is visible on its own.

echo "=== the basics: reads and writes reach the target"
x=1; declare -n r=x; echo "  r=[$r]"; r=2; echo "  x=[$x]"
echo "--- a target that does not exist yet is bound when it appears"
declare -n q=late; late=5; echo "  q=[$q]"
echo "--- and a chain is followed to its end"
declare -n a=b; declare -n b=c; c=3; echo "  a=[$a]"
echo "--- ${!ref} names the target, not the target's value"
echo "  [${!r}]"

echo "=== an element reference"
arr=(p q z); declare -n e="arr[1]"; echo "  e=[$e]"
e=Q; echo "  arr=[${arr[*]}]"
declare -A m=([k]=v); declare -n am="m[k]"; echo "  am=[$am]"
echo "--- built up with += , which extends the *name*"
declare -n g=arr; declare -n g+="[2]"; declare -p g; echo "  g=[$g]"

echo "=== the value must be able to name a variable"
for v in '' '1' 'a-b' 'a b' 'a[' 'a[]' '@'; do
  declare -n bad="$v" 2>/dev/null
  echo "  [$v] rc=$?"
done
echo "--- with the diagnostic on stderr ( empty takes the generic wording )"
declare -n bad=
declare -n bad='a b'
declare -n bad='a['
echo "--- but a subscript is not evaluated here, so these are fine"
declare -n ok1='a[x+1]'; declare -p ok1
declare -n ok2='a[@]'; declare -p ok2
declare -n ok3='_x9'; declare -p ok3

echo "=== += validates the *result*, and reports the appended text"
declare -n p1=x; declare -n p1+=' y'; echo "  rc=$?"; declare -p p1
declare -n p2=a; declare -n p2+='[1]'; echo "  rc=$?"; declare -p p2

echo "=== a nameref may not name itself"
declare -n self=self; echo "  rc=$?"; declare -p self
echo "--- the base name is what is compared"
declare -n self2=self2[0]; echo "  rc=$?"
echo "--- and the check precedes the readonly check"
readonly ro; declare -n ro='a b'; echo "  rc=$?"

echo "=== the same rules under local, typeset and declare -g"
f() { local -n bad='a b'; echo "  local rc=$?"; }; f
typeset -n bad='a b'; echo "  typeset rc=$?"
g() { declare -g -n bad='a b'; echo "  declare -g rc=$?"; }; g

echo "=== a failing operand does not stop the ones after it"
declare -n bad='a b' good=x; echo "  rc=$?"; declare -p good

echo "=== unset -n removes the reference, unset removes the target"
t=1; declare -n u=t; unset -n u; declare -p t; declare -p u 2>&1 >/dev/null
t2=1; declare -n u2=t2; unset u2; declare -p t2 2>&1 >/dev/null; declare -p u2
