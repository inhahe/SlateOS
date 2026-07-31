# An array's kind is fixed once set, and a declaration builtin has two quite
# different ways of refusing to change it.
#
#  * A *conversion* refusal — the name already exists under the other kind — is
#    raised by the compound-assignment machinery during word expansion. It
#    carries no builtin tag, nothing binds, and it discards the rest of the
#    parse unit.
#  * A *self*-conflict — `-a` and `-A` both named in the `-` direction by one
#    command — is raised by the builtin. The `-A` wins first, so the name really
#    does become associative and the literal really does bind; the tagged
#    diagnostic then fails the command without discarding anything, and is
#    emitted once per operand.

# `-a` and `-A` in one command: the associative array is made and filled, and
# the `-a` is refused against it.
declare -a -A n1=(x)
echo "rc=$?"
declare -p n1
declare -A -a n2=(x)
echo "rc=$?"
declare -p n2
declare -aA n3=(x)
echo "rc=$?"
declare -p n3
declare -Aa n4=(x)
echo "rc=$?"
declare -p n4

# Repeating one letter is not a conflict, and neither is a `+` letter: only two
# `-` letters name two kinds.
declare -a -a n5=(x)
echo "rc=$?"
declare -p n5
declare -A -A n6=([k]=x)
echo "rc=$?"
declare -p n6
declare -A +a n7=([k]=x)
echo "rc=$?"
declare -p n7
declare -a +A n8=(x)
echo "rc=$?"
declare -p n8

# The same refusal for a bare name, a subscripted operand and a scalar one —
# except that those bind *after* the check, so the value never lands. An
# operand that carried one still leaves the array valued, hence the empty `=()`.
declare -aA b1
echo "rc=$?"
declare -p b1
declare -aA b2[0]=zz
echo "rc=$?"
declare -p b2
declare -aA b3=zz
echo "rc=$?"
declare -p b3

# Every operand reports, and the command keeps going: the rest of the line runs
# and the next operand still binds.
declare -aA m1=(1) m2=(2)
echo "rc=$?"
declare -p m1
declare -p m2
declare -aA m3=(1); echo "same line after"
declare -p m3

# The refusal abandons the operand where the builtin would have applied the
# rest of its flags, so the attributes a successful command takes back off stay
# on: `+i` does not remove the integer attribute the literal bound under, and
# `+u` does not remove the fold. Two *different* case letters still cancel.
declare -aA -x v1=(1)
echo "rc=$?"
declare -p v1
declare -aA -r v2=(1)
echo "rc=$?"
declare -p v2
declare -aA +i v3=(2+3)
echo "rc=$?"
declare -p v3
declare -aA -i +i v4=(2+3)
echo "rc=$?"
declare -p v4
declare -aA +l v5=(AB)
echo "rc=$?"
declare -p v5
declare -aA -l v6=(AB)
echo "rc=$?"
declare -p v6
declare -aA +u v7=(Ab)
echo "rc=$?"
declare -p v7
declare -aA -l -u v8=(Ab)
echo "rc=$?"
declare -p v8

# `+a`/`+A` outrank the self-conflict: both come from the same lookup and the
# destroy complaint is the one bash makes.
declare -a d1=(1 2)
declare -aA +a d1
echo "rc=$?"
declare -p d1
declare -A d2=([k]=1)
declare -aA +A d2
echo "rc=$?"
declare -p d2

# The tag follows the builtin, and `-g` reaches the global.
typeset -aA t1=(1)
echo "rc=$?"
declare -p t1
f1() { local -aA x=(1); echo "rc=$?"; declare -p x; }
f1
f2() { declare -gaA g1=(1); echo "rc=$?"; }
f2
declare -p g1

# `readonly` and `export` are separate entry points that never make the check.
readonly -aA r1=(1)
echo "rc=$?"
declare -p r1
export -aA r2=(1)
echo "rc=$?"
declare -p r2

# Against a name that is already associative there is nothing to convert, so
# the literal binds as usual and only the `-a` is refused.
declare -A e1=([k]=v)
declare -aA e1=(z)
echo "rc=$?"
declare -p e1

# Against one that is already *indexed* the `-A` has a real conversion to do,
# and that is the other refusal: untagged, nothing bound, rest of the line
# gone. A bare name has no compound to expand, so it takes the tagged form.
declare -a e2=(q w)
declare -aA e2=(z); echo "not reached"
echo "rc=$?"
declare -p e2
declare -a e3=(q w)
declare -aA e3
echo "rc=$?"
declare -p e3

# The same two forms with a single kind letter.
declare -A e4=([k]=v)
declare -a e4=(z); echo "not reached"
echo "rc=$?"
declare -p e4
declare -A e5=([k]=v)
declare -a e5
echo "rc=$?"
declare -p e5
declare -a e6=(q)
declare -A e6=([z]=1); echo "not reached"
echo "rc=$?"
declare -p e6
declare -a e7=(q)
declare -A e7
echo "rc=$?"
declare -p e7
