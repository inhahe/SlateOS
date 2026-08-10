# bash binds a compound array literal in two passes, and the split between them
# is where a bad subscript lands. `expand_words_no_vars` expands the whole word
# list first — which is why `a=(9 ${a[0]})` still reads the old `a` — and only
# then does `assign_compound_array_list` walk it, calling `array_expand_index`
# (arrayfunc.c:1358-1375) on each `[sub]=v` element as it goes:
#
#     t = expand_arith_string (exp, Q_DOUBLE_QUOTES|Q_ARITH|Q_ARRAYSUB);
#     val = evalexp (t, eflag, &expok);
#     …
#     if (expok == 0)
#       {
#         set_exit_status (EXECUTION_FAILURE);
#         if (no_longjmp_on_fatal_error)
#           return 0;
#         top_level_cleanup ();
#         jump_to_top_level (DISCARD);
#       }
#
# So a subscript that will not evaluate is a *binding* failure, not an expansion
# one, and the `DISCARD` is taken from inside the walk. Everything the walk has
# already done stands — the `set -x` trace, the variable's creation and its
# type, the clear a non-append literal did, and every element bound before the
# bad one — while the rest of the list, and the rest of the command list around
# it, never happen. Hence `z=([0]=A [1x]=B [2]=C)` leaves `declare -a
# z=([0]="A")`, and even `q=([1x]=R)`, whose every element fails, leaves an
# empty `q` behind.
#
# The `DISCARD` is why each row below reports on its own line: a `;` after the
# assignment would be discarded with the rest of the list.
#
# What the walk does *not* reach is the array's own visibility. bash sheds
# `att_invisible` per element bound (`bind_array_var_internal`,
# arrayfunc.c:245) and again when the assignment completes
# (`assign_array_var_from_string`, arrayfunc.c:896), and a `DISCARD` reaches
# neither — so what is left is whatever the *creation* decided. A bare
# `q=(…)` creates through `find_or_make_array_variable`, before the walk,
# and so is already visible when the jump fires (`declare -a q=()`); a
# `declare`/`local` operand's name is created invisible and stays that way
# (`declare -a y`). A name that was already invisible keeps its invisibility
# whichever form assigns to it, and a name that was already visible keeps
# that.
#
# An *associative* literal never enters this — its subscripts are keys, not
# expressions — so `[1x]` is simply a key there.
#
# Verified against bash 5.2.37.

echo "=== the elements before the failure stand ==="
declare -a y=([1x]=R)
echo "1 rc=$?"
declare -p y
declare -a z=([0]=A [1x]=B [2]=C)
echo "2 rc=$?"
declare -p z
q=([1x]=R)
echo "3 rc=$?"
declare -p q
r=(p q [1x]=B [2]=C)
echo "4 rc=$?"
declare -p r

echo "=== the clear stands too ==="
s=(A B C)
s=([0]=X [1x]=Y)
echo "5 rc=$?"
declare -p s
t=(A B C)
t+=([1x]=Y)
echo "6 rc=$?"
declare -p t

echo "=== the type stands ==="
declare -ai n=([0]=5 [1x]=6)
echo "7 rc=$?"
declare -p n

echo "=== and the trace is written before any of it ==="
set -x
declare -a v=([0]=A [1x]=B)
set +x
echo "8 rc=$?"
declare -p v
set -x
w=([0]=A [1x]=B)
set +x
echo "9 rc=$?"
declare -p w

echo "=== an associative literal has no arithmetic to fail ==="
declare -A k=([a]=A [1x]=B)
echo "10 rc=$?"
declare -p k

echo "=== the walk never reaches the visibility, so the creation decides it ==="
# Already visible, either form: stays visible (and cleared).
c1=5
declare -a c1=([1x]=R)
echo "12 rc=$?"
declare -p c1
declare -a c2=(A B)
c2=([1x]=R)
echo "13 rc=$?"
declare -p c2
# Already invisible, either form: stays invisible.
declare -a c3
c3=([1x]=R)
echo "14 rc=$?"
declare -p c3
declare c4
c4=([1x]=R)
echo "15 rc=$?"
declare -p c4
export c5
c5=([1x]=R)
echo "16 rc=$?"
declare -p c5
# Brand new: born visible bare, born invisible under a declaration builtin —
# including the append form, and including one without an explicit `-a`.
declare -a c6+=([1x]=R)
echo "17 rc=$?"
declare -p c6
declare c7=([1x]=R)
echo "18 rc=$?"
declare -p c7
c8=([1x]=R)
c8=([1x]=R)
echo "19 rc=$?"
declare -p c8
# The per-element clear is sticky, so emptying the array afterwards does not
# put the invisibility back.
declare -a c9
c9=(A [1x]=B)
unset 'c9[0]'
echo "20 rc=$?"
declare -p c9
declare -ai c10
c10=(1 2+)
unset 'c10[0]'
echo "21 rc=$?"
declare -p c10

echo "=== a subscript the array cannot hold is not this: it skips one element ==="
declare -a m=([0]=A [-9]=B [2]=C)
echo "11 rc=$?"
declare -p m

echo TAIL
