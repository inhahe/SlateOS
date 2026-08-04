# `local -I` / `declare -I`: the new local starts as a *copy* of the binding it
# shadows — value, array kind and every attribute — instead of starting fresh.
# It is `shopt -s localvar_inherit` asked for one declaration at a time, and
# bash implements it as exactly that, so all of that option's corners hold here.

p() { declare -p "$@" 2>&1 | sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }
e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }

echo "=== the value comes from the enclosing scope, and is a copy"
outer() { local v=OUT; middle; echo "  outer sees v=[$v]"; }
middle() { local -I v; echo "  middle v=[$v]"; v=MID; inner; }
inner() { echo "  inner v=[$v]"; }
outer

echo "=== the *nearest* scope that has the name wins, so frames are skipped"
a1() { local v1=A1; b1; }
b1() { c1; }
c1() { local -I v1; echo "  [$v1]"; }
a1

echo "=== …and when two frames have it, the immediately enclosing one"
a2() { local v2=A2; b2; }
b2() { local v2=B2; c2; }
c2() { local -I v2; echo "  [$v2]"; }
a2

echo "=== a global serves when no frame has the name"
g3=GLOBAL
f3() { local -I g3; echo "  [$g3]"; }
f3

echo "=== …but an enclosing local hides the global from it"
g3b=GLOB
a3b() { local g3b=LOC; b3b; }
b3b() { local -I g3b; echo "  [$g3b]"; }
a3b

echo "=== a name nothing has is inherited as unset, but still declared"
f4() { local -I never4; echo "  [${never4-UNSET}]"; p never4; }
f4

echo "=== the attributes come with it: -i"
o5() { local -i n=5; m5; }
m5() { p n; n+=3; p n; }
o5

echo "=== …-x, -r and -t"
a6() { local -x v6=X6; b6; }
b6() { local -I v6; p v6; }
a6
a7() { local -r v7=R7; b7; }
b7() { local -I v7 2>err; echo "  rc=$?"; e < err; p v7; }
a7
a8() { local -t v8=T8; b8; }
b8() { local -I v8; p v8; }
a8

echo "=== …and the array kind, with the elements"
o9() { local -a a9=(x y); m9; }
m9() { local -I a9; p a9; }
o9
oa() { local -A ma=([k]=v); ma2; }
ma2() { local -I ma; p ma; }
oa

echo "=== -n is the exception: the marking is dropped, the value kept"
tb=TARGET
ab() { local -n rb=tb; bb; }
bb() { local -I rb; p rb; echo "  [$rb]"; }
ab

echo "=== an explicit value overrides, but an inherited attribute transforms it"
ac() { local -i nc=5; bc; }
bc() { local -I nc=2+3; p nc; }
ac
ad() { local -u ud=ab; bd; }
bd() { local -I ud=cd; p ud; }
ad

echo "=== a flag on the declaration itself applies on top of the inherited one"
ae() { local ve=AE; be; }
be() { local -Ii ve; p ve; }
ae
af() { local vf=AF; bf; }
bf() { local -Ia vf; p vf; }
af

echo "=== a declared-but-unset enclosing binding gives a declared-but-unset local"
ag() { local vg; bg; }
bg() { local -I vg; p vg; echo "  [${vg-U}]"; }
ag
ah() { local -i nh; bh; }
bh() { local -I nh; p nh; }
ah

echo "=== +I inherits too: the letter is recorded without reading its sign"
ai() { local vi=AI; bi; }
bi() { local +I vi 2>err; echo "  rc=$?"; e < err; p vi; }
ai

echo "=== a second -I in the same frame re-declares, it does not shadow again"
aj() { local vj=AJ; bj; }
bj() { local vj=BJ; local -I vj; p vj; }
aj

echo "=== -I on the frame's *own* earlier local of the same name, likewise"
ak() { local vk=AK; bk; p vk; }
bk() { local -I vk; vk=BK; p vk; }
ak

echo "=== a compound operand inherits the same way"
al() { local -a al2=(x y); bl; }
bl() { local -I al2=(p q); p al2; }
al

echo "=== two levels of -I chain, each copying the one before"
am() { local vm=AM; bm; }
bm() { local -I vm; cm; }
cm() { local -I vm; p vm; }
am

echo "=== declare -I outside a function is accepted and silent"
declare -I zz 2>err; echo "  rc=$?"; e < err

echo "=== declare -I inside a function is not local at all"
gn=GN
an() { declare -I gn; p gn; gn=IN; }
an; p gn

echo "=== -g retargets, so the -I has nothing to shadow"
go=GLOB
ao() { local go=LOC; bo; }
bo() { declare -gI go; p go; }
ao; p go

echo "=== and I is in the usage line"
declare -Z 2>err; e < err
local -Z 2>err; e < err
