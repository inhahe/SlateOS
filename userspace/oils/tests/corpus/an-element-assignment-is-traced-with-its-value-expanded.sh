# `set -x` shows an assignment's two halves from two different places. The
# target is the word bash **scanned** for a name; the value is the string it
# **expanded** — because by the time `do_assignment_internal` traces, only the
# value has been through expansion. The subscript has not: it is evaluated
# later still, inside `assign_array_element`.
#
# So a written subscript appears as typed while the value beside it appears as
# it came out, and a subscript makes no difference to which of the two forms
# the trace takes — that is decided by the value alone. A **compound literal**
# is the one traced whole in source form, since none of it has been expanded
# yet (see
# an-assignments-value-is-expanded-before-the-variable-is-looked-for.sh).
exec 2>&1
f() { echo "v$1"; }
i=2
declare -A m
declare -n r=tgt
declare -a q=(A B); declare -n re='q[1]'
set -x
s=$(f s)
s+=$(f app)
a[$i]=$(f elem)
a[$i]+=$(f elemapp)
a[1+1]=$(f arith)
a[$(f sub)x]=plain
m[$i]=$(f assoc)
m[$(f key)]=$(f val)
r=$(f ref)
re=$(f refelem)
b=(1 $(f lit))
b[0]=$((3 * 4))
set +x
declare -p s a m tgt q b
echo TAIL
