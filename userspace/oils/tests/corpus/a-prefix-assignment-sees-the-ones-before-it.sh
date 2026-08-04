# A command's assignment prefix is evaluated one assignment at a time, with the
# ones before it already **in effect** — bash binds each into the temporary
# environment before it expands the next. The binding is visible every way an
# in-effect variable is: to a later value's parameter expansion, to its
# arithmetic, to a command substitution inside it (and in that substitution's own
# environment, since a prefix binding is exported), through a nameref, and it
# shadows a name the shell would otherwise compute for itself.
#
# `+=` is that same fact one step further along: it appends to whatever the bound
# name holds at that point — the target's own value for a name's first
# assignment, the staged binding for a repeat. The rule is the one an ordinary
# `+=` follows, so `-i` adds numerically and the case attributes fold the new
# text before it is concatenated. A staged binding is a fresh environment
# variable carrying no attributes, which is why only the *first* append of a name
# can be arithmetic: `declare -i n=5; n+=3 n+=4 cmd` passes `84`, not `12`.
#
# A plain `=` skips all of that, which is why the target's `-i` does not reach
# it: `declare -i n; n=3+4 cmd` passes the string `3+4`.
#
# None of it outlives the command, and none of it reaches the command's own words
# or redirections — those are expanded with the prefix *not* in effect. The trace
# shows what was actually bound, so an append is traced under a plain `=`.

echo "=== a later value sees an earlier binding"
v=a
v=X v=${v}Z eval 'echo "  in: v=[$v]"'
echo "  after: v=[$v]"

echo "=== …including in arithmetic"
n=1
n=5 m=$((n+1)) eval 'echo "  n=[$n] m=[$m]"'

echo "=== …and as *set* to a default expansion"
unset uu
uu=SET vv=${uu-nope} eval 'echo "  vv=[$vv]"'

echo "=== …and through a command substitution"
v2=orig
v2=X w2=$(echo "[$v2]") eval 'echo "  w2=$w2"'

echo "=== …which has it in its own environment, a prefix binding being exported"
v3=X w3=$(env | grep -ac '^v3=') eval 'echo "  count=$w3"'

echo "=== …and through a function called from one"
f4() { echo -n "[$v4]"; }
v4=orig
v4=X w4=$(f4) eval 'echo "  w4=$w4"'

echo "=== …and through a nameref, whose target is what was bound"
t5=orig
declare -n r5=t5
r5=X y5=$t5 eval 'echo "  y5=[$y5]"'

echo "=== a name the shell computes is shadowed for the later values too"
SECONDS=100 z6=$SECONDS eval 'echo "  z6=[$z6] SECONDS=[$SECONDS]"'

echo "=== += appends to what the name held before the prefix"
t7=orig
t7+=V eval 'echo "  in: t7=[$t7]"'
echo "  after: t7=[$t7]"

echo "=== …and to nothing at all when the name was unset"
u8+=V eval 'echo "  in: u8=[$u8]"'
echo "  after: u8=[${u8-U}]"

echo "=== …with an empty value it is just what was there"
h9=abc
h9+= eval 'echo "  in: h9=[$h9]"'

echo "=== an -i target makes the append arithmetic"
declare -i n10=5
n10+=3 eval 'echo "  in: n10=[$n10]"; declare -p n10 | sed "s/^/    /"'
declare -p n10 | sed 's/^/    /'

echo "=== …evaluating the whole value as an expression"
declare -i m11=10
m11+=2*3 eval 'echo "  in: m11=[$m11]"'
declare -i k11=10
k11+=-3 eval 'echo "  in: k11=[$k11]"'

echo "=== …onto an unset -i name it is just the value"
declare -i k12
k12+=7 eval 'echo "  in: k12=[$k12]"'

echo "=== …and a non-numeric old value counts as zero"
s13=abc
declare -i s13
s13+=3 eval 'echo "  in: s13=[$s13]"'

echo "=== a bad -i expression is fatal to the command"
declare -i n14=5
n14+=2+ eval 'echo "  RAN"'
echo "  rc=$? after=[$n14]"

echo "=== the case attributes fold the new text, then it is concatenated"
declare -u p15=ab
p15+=cd eval 'echo "  in: p15=[$p15]"; declare -p p15 | sed "s/^/    /"'
declare -l q15=AB
q15+=CD eval 'echo "  in: q15=[$q15]"'

echo "=== a staged binding carries no attributes, so only the first append adds"
declare -i n16=5
n16+=3 n16+=4 eval 'echo "  in: n16=[$n16]"'
declare -i m16=5
m16=10 m16+=4 eval 'echo "  in: m16=[$m16]"'
declare -u q16=ab
q16+=cd q16+=ef eval 'echo "  in: q16=[$q16]"'

echo "=== plain repeats accumulate the same way"
p17=a
p17+=b p17+=c eval 'echo "  in: p17=[$p17]"'
echo "  after: p17=[$p17]"

echo "=== an array name appends onto its element zero, as a scalar read does"
declare -a a18=(x y)
a18+=z eval 'echo "  in: a18=[$a18]"; declare -p a18 | sed "s/^/    /"'
declare -p a18 | sed 's/^/    /'

echo "=== …and an associative one onto its key 0, which is usually nothing"
declare -A A19=([k]=v)
A19+=z eval 'echo "  in: A19=[$A19]"'
declare -p A19 | sed 's/^/    /'

echo "=== += through a nameref appends to the target's value"
tn20=orig
declare -n rn20=tn20
rn20+=V eval 'echo "  in: tn20=[$tn20]"'
echo "  after: tn20=[$tn20]"

echo "=== …and picks up the target's -i"
declare -i ti21=5
declare -n ri21=ti21
ri21+=3 eval 'echo "  in: ti21=[$ti21]"'

echo "=== …and creates an unset target"
declare -n rn22=never22
rn22+=V eval 'echo "  in: never22=[$never22]"'

echo "=== += onto a caller's local"
f23() { local lv=inner; lv+=V g23; echo "  after in f: lv=[$lv]"; }
g23() { echo "  in g: lv=[$lv]"; }
f23

echo "=== += onto a readonly is refused like any other prefix assignment"
readonly ro24=frozen
ro24+=V eval 'echo "  in: ro24=[$ro24]"'
echo "  rc=$?"

echo "=== += onto an exported name still restores"
export e25=a
e25+=b env | grep -a '^e25=' | sed 's/^/    /'
declare -p e25 | sed 's/^/    /'

echo "=== the trace shows what was bound, under a plain ="
y26=a
set -x
y26+=b true
set +x

echo "=== …and the same for an arithmetic append"
declare -i z27=5
set -x
z27+=3 true
set +x

echo "=== a prefix with no command at all is an ordinary append"
y28=a
y28+=b
declare -p y28 | sed 's/^/    /'

echo "=== the binding does not reach the command's own words"
v29=orig
v29=NEW printf '  arg=[%s]\n' "$v29"
