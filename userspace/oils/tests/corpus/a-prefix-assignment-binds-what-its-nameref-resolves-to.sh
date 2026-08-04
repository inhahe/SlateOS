# A command's assignment prefix (`FOO=bar cmd`) binds the name a nameref
# *resolves to*, not the reference itself. `declare -n r=t; r=V cmd` puts `t=V`
# into the command's temporary environment — under that name and only that one,
# so `env` shows `t=V` and never `r=V` — while `r` stays a nameref, which is how
# `$r` still reads the binding. A chain is followed to its end, and a target
# nothing has bound is created for the duration and gone afterwards.
#
# Two endings name no variable a scope could bind, and both fall back to binding
# the name as written, which then shadows the nameref cell for the command's
# duration:
#
#   - a chain ending in an *element* (`declare -n r='a[1]'`) — the temporary
#     environment holds variables, not elements, so bash binds `r` itself and
#     leaves the array untouched;
#   - a **circular** chain, which warns first, as every use of such a name does,
#     naming the variable written rather than any member of the cycle. The
#     command still runs and its own status stands.
#
# The readonly attribute is tested on whatever the binding would land on, but
# reported by the name **as written** — `declare -n n=ro; readonly ro; n=V cmd`
# says `n: readonly variable`, where the same refusal reached through `((n=5))`
# says `ro`. Testing the bound name is also why a readonly *array* behind an
# element reference does not refuse: nothing would be written to it. A refusal
# still lets the command run, with its own exit status.
#
# The binding is a fresh exported variable that inherits none of the target's
# attributes, exactly as a prefix on a plain name does — a `-i` target keeps its
# integer attribute afterwards but does not evaluate the prefix's value.
#
# The trace is the one place the written name survives: `set -x` echoes the
# assignment as written, nameref and all.

show() { declare -p "$@" 2>&1 | sed 's/^/    /'; }

echo "=== the target is what reaches the environment, under its own name"
t1=orig
declare -n r1=t1
r1=V env | grep -a -E '^(t1|r1)=' | sort | sed 's/^/    /'
show r1 t1

echo "=== …and it is bound, exported, for the duration, then restored"
t2=orig
declare -n r2=t2
f2() { echo "    in f: t2=[$t2] r2=[$r2]"; show t2; }
r2=V f2
show r2 t2

echo "=== a target nothing has bound is created for the duration"
declare -n r3=never3
f3() { echo "    in f: never3=[${never3-U}]"; }
r3=V f3
show r3 never3

echo "=== a chain is followed to its end"
u4=orig
declare -n m4=u4
declare -n n4=m4
f4() { echo "    in f: u4=[$u4] m4=[$m4] n4=[$n4]"; }
n4=V f4
show n4 m4 u4

echo "=== …and only the far end is in the environment"
z5=orig
declare -n a5=z5
declare -n b5=a5
declare -n c5=b5
c5=V env | grep -a -E '^(z5|a5|b5|c5)=' | sort | sed 's/^/    /'

echo "=== the nameref cell itself is not shadowed"
t6=orig
declare -n r6=t6
f6() { show r6 t6; }
r6=V f6

echo "=== a chain ending in an element binds the name as written"
declare -a a7=(p q r)
declare -n r7="a7[1]"
f7() { echo "    in f: r7=[$r7] elem=[${a7[1]}]"; show r7; }
r7=V f7
show r7 a7

echo "=== …even two links away"
declare -a a8=(x y)
declare -n e8="a8[0]"
declare -n f8=e8
f8=V env | grep -a -E '^(a8|e8|f8)=' | sort | sed 's/^/    /'

echo "=== …and a readonly array behind one does not refuse"
declare -a a9=(p q)
readonly a9
declare -n r9="a9[1]"
r9=V eval 'echo "    in: r9=[${r9-U}] elem=[${a9[1]}]"'
echo "  rc=$?"

echo "=== a circular chain warns, names the variable written, and binds it"
declare -n c10=d10
declare -n d10=c10
c10=V eval 'echo "    in: c10=[${c10-U}]"'
echo "  rc=$?"

echo "=== …and the command still runs with its own status"
declare -n c11=d11
declare -n d11=c11
c11=V false
echo "  rc=$?"

echo "=== a readonly target refuses, blaming the name as written"
readonly ro12=frozen
declare -n n12=ro12
n12=V eval 'echo "    in: n12=[${n12-U}] ro12=[$ro12]"'
echo "  rc=$?"
show n12 ro12

echo "=== …and the command still runs"
declare -n n13=ro12
n13=V echo "    command RAN"
echo "  rc=$?"

echo "=== marking the *reference* readonly marks the target, so it refuses too"
t14=orig
declare -n r14=t14
readonly r14
show r14 t14
r14=V eval 'echo "    in: t14=[$t14]"'
echo "  rc=$?"

echo "=== the target's attributes do not reach the prefix's value"
declare -i t15=1
declare -n r15=t15
f15() { echo "    in f: t15=[$t15]"; show t15; }
r15=3*4 f15
show r15 t15

echo "=== a prefix through a nameref to a caller's local binds that local"
tg=global
f16() {
  local lv=inner
  declare -n ln=lv
  ln=V g16
  echo "    after: lv=[$lv]"
}
g16() { echo "    in g: lv=[${lv-U}]"; }
f16

echo "=== a deeper local of the target's name shadows the binding normally"
t17=global
declare -n r17=t17
h17a() { local t17=deeper; h17b; }
h17b() { echo "    h17b sees t17=[$t17]"; }
r17=V h17a
echo "    after: t17=[$t17]"

echo "=== a plain prefix beside a nameref one is unaffected"
t18=orig
declare -n r18=t18
p18=orig
f18() { echo "    in f: t18=[$t18] p18=[$p18]"; }
r18=V p18=W f18
show r18 t18 p18

echo "=== a prefix with no command at all is a plain assignment through it"
t19=orig
declare -n r19=t19
r19=V
show r19 t19

echo "=== the trace shows the assignment as written"
t20=orig
declare -n r20=t20
set -x
r20=V true
set +x
