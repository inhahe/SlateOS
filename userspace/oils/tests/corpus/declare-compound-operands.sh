# A *compound* operand of a declaration builtin (`declare -a name=(…)`) is not
# assigned by the builtin. It is bound during the command's word-expansion pass,
# because only a compound can bind a whole array, and there is no way to hand one
# to a builtin as an argv string. A *scalar* operand is merely expanded there and
# left for the builtin to assign. Everything below follows from that split.

# Compounds bind in operand order, so a later one sees an earlier one's value.
declare -a x=(1 2) y=("${x[1]}" "${#x[@]}")
declare -p x y

# A compound does not see a *scalar* operand of the same command — not even one
# written to its left, since the builtin has not run yet.
c=old
declare -a b=($c) c=2
echo "rc=$?"
declare -p b c
unset b c
c=old
declare -a c=2 b=($c)
declare -p b c

# The kind (`-a`/`-A`) and the value-transforming attributes (`-i`/`-l`/`-u`/`-c`)
# are in force while the literal binds, so the stored values are already
# evaluated and folded.
declare -ai n=(1+1 3*3)
declare -p n
declare -al lo=(AB Cd)
declare -p lo
declare -Au up=([k]=ab)
declare -p up

# A compound that *fails* stops the command dead: the earlier operands stay
# bound, the later ones never bind, the builtin never runs, and the rest of the
# parse unit is discarded — so the `echo` below never speaks.
readonly r=1
declare -a first=(0) r=(2) ok=(3); echo "never-printed"
echo "after-failure"
declare -p first
declare -p ok 2>&1
echo "ok-status=$?"

# Because the builtin never ran, the attributes only *it* applies (`-x`, `-r`,
# `-n`) were never applied to the survivor either — while `-i`/`-l` were, having
# been in force at bind time. `declare -p` shows exactly that split.
unset g
declare -ailx g=(2+3 AB) r=(1)
echo "after-failure-2"
declare -p g

# The array-kind conflict is the same shape: a literal cannot reinterpret an
# existing array under the other kind.
declare -A A2=([k]=v)
declare -a firstk=(0) A2=(9) okk=(3)
echo "after-conflict"
declare -p firstk
declare -p okk 2>&1
echo "okk-status=$?"

# The *builtin* failing is a milder thing — no discard, status 1 — and the
# compounds bound before it keep the attributes it did apply.
readonly s=1
unset v
declare -ax v=(1) s=2; echo "builtin-fail rc=$?"
declare -p v

# An array-literal-only `readonly`/`export` has no builtin call at all, so the
# attribute is applied to the compound's name directly.
readonly -a w=(1 2)
declare -p w
w[0]=9
echo "w-status=$?"
export -a e=(1)
declare -p e
