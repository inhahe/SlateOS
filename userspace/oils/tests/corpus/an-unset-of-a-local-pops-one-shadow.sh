# `unset NAME` does not simply "make the name unset". It takes the *innermost*
# binding off the variable scope stack and lets the next one show through — so
# in a function that is not the one which declared the local, the unset reveals
# the outer local, or the global, and a second `unset` reveals the one after
# that.
#
# The exception is the frame that declared it: unsetting a local of the
# *current* function marks it unset rather than removing it. The name is still
# there — `declare -p` reports the bare `declare -- c` — it is still local, a
# later write in the same frame still goes to it, and repeating the `unset`
# never reaches past it.
#
# `shopt -s localvar_unset` extends the current-frame rule to every frame:
# "identical to the behavior of unsetting local variables at the current
# function scope", as the manual puts it. It is off by default, so the popping
# is what a script sees.
#
# When the binding really is removed the frame no longer has one to restore, so
# a write after the unset lands on whatever was revealed and outlives the
# function that had the local. Everything else about `unset` is unchanged: a
# readonly local is refused first, and a global is simply removed.
#
# A call's *temporary environment* — the `q=3` in `q=3 f` — sits in the same
# scope level as that call's locals, so the two behave as one binding: a `local`
# of the name writes over the prefix's rather than shadowing it, and an `unset`
# from a deeper frame pops both at once, revealing what the prefix displaced.
# The one attribute the marking keeps is the export marking such a binding came
# in with. A prefix on anything that is *not* a function call — `q=3 eval 'f'` —
# is a scope of its own that a function called underneath it shadows normally.

show() { declare -p "$1" 2>&1 | sed 's/^/    /'; }

echo "### the current frame's own local is marked, not removed"
c=global
f() { local c=one; unset c; show c; echo "    [${c-U}]"; }
f
echo "  top: [$c]"

echo "### …and repeating it never reaches the global"
g() { local c=one; unset c; unset c; unset c; show c; }
g
echo "  top: [$c]"

echo "### …and it is still the local that a later write goes to"
h() { local c=one; unset c; c=rewritten; show c; }
h
echo "  top: [$c]"

echo "### an enclosing frame's local is popped, and the next binding shows"
a=global
i1() { unset a; echo "    i1: [${a-U}]"; }
i2() { local a=two; i1; echo "    i2: [${a-U}]"; }
i2
echo "  top: [${a-U}]"

echo "### one level per unset, three frames deep"
a=global
l1() { unset a; echo "    after 1: [${a-U}]"; unset a; echo "    after 2: [${a-U}]"; }
l2() { local a=two; l1; echo "    l2: [${a-U}]"; }
l3() { local a=three; l2; echo "    l3: [${a-U}]"; }
l3
echo "  top: [${a-U}]"

echo "### the revealed binding keeps its attributes"
declare -i b=1
o1() { unset b; show b; }
o2() { local b=two; o1; show b; }
o2
show b

echo "### the frame has nothing left to restore, so a later write outlives it"
a=global
q1() { unset a; a=fromq1; }
q2() { local a=two; q1; show a; }
q2
show a

echo "### an array local pops just the same"
declare -a arr=(g1 g2)
r1() { unset arr; show arr; }
r2() { local -a arr=(l1 l2); r1; }
r2
show arr

echo "### localvar_unset makes every frame behave like the current one"
shopt -s localvar_unset
a=global
i2
echo "  top: [${a-U}]"
a=global
n1() { unset a; a=written; echo "    n1: [${a-U}]"; }
n2() { local a=two; n1; echo "    n2: [${a-U}]"; }
n2
echo "  top: [${a-U}]"
echo "  and the current-frame rule is unchanged:"
c=global
g
shopt -u localvar_unset

echo "### with the option off, that same write lands on the revealed global"
a=global
n2
echo "  top: [${a-U}]"

echo "### a readonly local is refused before any of it"
k1() { unset ro; echo "    rc=$?"; show ro; }
k2() { local -r ro=frozen; k1; show ro; }
k2

echo "### a global is removed outright"
x=1
y1() { unset x; show x; }
y1
show x

echo "### and a name nothing has ever bound is a quiet success"
z1() { unset zznever; echo "    rc=$?"; show zznever; }
z1

echo "### a call's prefix is the same scope level as that call's locals"
q=1
t1() { unset q; show q; }
t2() { local q=2; t1; }
q=3 t2
show q
echo "  …and with no local in the way it is the same one binding"
q=1
u1() { unset q; show q; }
u2() { u1; }
q=3 u2
show q

echo "### unsetting it in the frame it belongs to marks it, keeping the -x"
q=1
v1() { local q=2; unset q; show q; q=w; show q; }
q=3 v1
show q
echo "  …but a -x the declaration itself asked for is dropped like any other"
w1() { local -x w=1; unset w; show w; w=y; show w; }
w1
echo "  …and so is every other attribute of the inherited binding"
q=1
x1() { local -i q=2; unset q; show q; q=3+4; show q; }
q=3 x1
show q

echo "### the same binding with no local at all is popped, not marked"
q=1
y2() { unset q; show q; q=w; show q; }
q=3 y2
show q

echo "### a prefix on something that is not a call is a scope of its own"
q=1
aa1() { unset q; show q; }
aa2() { local q=2; aa1; }
q=3 eval 'aa2'
show q

echo "### one level per unset, with a prefix at each call"
q=1
ab1() { unset q; show q; unset q; show q; unset q; show q; }
ab2() { q=5 ab1; }
q=3 ab2
show q

echo "### localvar_unset marks an inherited binding too, -x and all"
shopt -s localvar_unset
q=1
ac1() { unset q; show q; q=w; show q; }
ac2() { local q=2; ac1; show q; }
q=3 ac2
show q
shopt -u localvar_unset
