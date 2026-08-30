# A command's assignment prefix may name the same variable more than once
# (`w=b w=c cmd`). That is *one* binding which the later assignment overwrites,
# not two stacked ones — bash's temporary environment holds one variable per
# name, so the command's environment carries a single entry.
#
# What that means is visible only in what comes back: the displaced value is the
# one that stood there **before the prefix**, never an intermediate. It comes
# back both when the scope closes at the end of the command and when an `unset`
# inside the command reveals what is underneath, and the two agree.
#
# The repeats are still each an assignment of their own — traced separately, and
# refused separately when the name is readonly.

echo "=== the last assignment is what the command sees"
w=a
w=b w=c eval 'echo "  in: w=[$w]"'
echo "  after: w=[$w]"

echo "=== …and the environment carries one entry, not two"
u=1
u=2 u=3 env | grep -a -c '^u='
u=2 u=3 env | grep -a '^u=' | sed 's/^/    /'

echo "=== an unset inside reveals what stood there before the prefix"
q=1
q=2 q=3 eval 'unset q; echo "  in after unset: q=[${q-U}]"'
echo "  after: q=[${q-U}]"

echo "=== …three deep, the same"
r=1
r=2 r=3 r=4 eval 'echo "  in: r=[$r]"; unset r; echo "  after unset: r=[${r-U}]"'
echo "  after: r=[${r-U}]"

echo "=== a name that was unset before the prefix is unset again after"
s=2 s=3 eval 'echo "  in: s=[$s]"'
echo "  after: s=[${s-U}]"

echo "=== …and an unset inside reveals nothing"
p=2 p=3 eval 'unset p; echo "  in after unset: p=[${p-U}]"'
echo "  after: p=[${p-U}]"

echo "=== a repeat still displaces only once, whatever stood there"
declare -a v=(x y)
v=1 v=2 eval 'declare -p v | sed "s/^/    /"'
declare -p v | sed 's/^/    /'

echo "=== a function call's temporary environment behaves the same"
t=1
f() { echo "  in f: t=[$t]"; }
t=2 t=3 f
echo "  after: t=[$t]"

echo "=== …and a local of the name inside it still shadows one binding"
g() { local t=L; echo "  in g: t=[$t]"; unset t; echo "  after unset: t=[${t-U}]"; }
t=1
t=2 t=3 g
echo "  after: t=[$t]"

echo "=== attributes of the shadowed variable are restored intact"
declare -i n=5
n=7 n=9 eval 'declare -p n | sed "s/^/    /"'
declare -p n | sed 's/^/    /'

echo "=== a readonly repeat is refused as many times as it is written"
readonly ro=frozen
ro=1 ro=2 eval 'echo "  in: ro=[$ro]"'
echo "  rc=$?"

echo "=== each repeat is traced"
x1=a
set -x
x1=b x1=c true
set +x

echo "=== under posix, a repeat on a special builtin persists as the last one"
set -o posix
y1=1
y1=2 y1=3 :
echo "  after: y1=[$y1]"
set +o posix
