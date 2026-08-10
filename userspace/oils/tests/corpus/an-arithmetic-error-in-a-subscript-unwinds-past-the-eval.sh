# bash has two depths of DISCARD. The ordinary one — every *expansion* error —
# goes through `exp_jump_to_top_level`, which asks where it is standing before
# it cleans up:
#
#     if (parse_and_execute_level == 0)
#       top_level_cleanup ();			/* from sig.c */
#     jump_to_top_level (v);                       /* subst.c:12151-12153 */
#
# so a nested read-eval loop — `eval`, `.`/`source` — catches it and the caller
# goes on. An arithmetic error in an array *subscript* is not that one. bash
# evaluates a subscript from `array_expand_index`, which cleans up
# unconditionally:
#
#     val = evalexp (t, eflag, &expok);
#     …
#     if (expok == 0)
#       {
#         set_exit_status (EXECUTION_FAILURE);
#         if (no_longjmp_on_fatal_error)
#           return 0;
#         top_level_cleanup ();
#         jump_to_top_level (DISCARD);            /* arrayfunc.c:1363-1375 */
#       }
#
# so it unwinds past the `eval` that would have confined it, and past the
# function around that, out to the parse unit the reader was on. Every context
# that reaches a subscript reaches it through this one function, so `(( … ))`,
# `[[ … ]]`, `let`, `declare`, `printf -v` and a compound literal all behave the
# same way. `unset 'n[1x]'` does not — it never evaluates the subscript through
# it. Neither does a `${v:off:len}` bound, which is `verify_substring_values`
# and so the ordinary depth; but a subscript written *inside* such a bound is
# the fatal one again.
#
# The status is a fixed 1. `set_exit_status (EXECUTION_FAILURE)` stands on the
# line above the jump, so whatever the subscript's own expansion had left in
# `$?` is overwritten — unlike the integer-binding DISCARD, which carries it.
#
# Verified against bash 5.2.37.

echo "=== it unwinds past an eval, and past the function around it ==="
a1() { eval 'q1[1x]=v'; echo "in-func"; }
a1
echo "1 rc=$?"
a2() { eval 'echo ${q2[1x]}'; echo "in-func"; }
a2
echo "2 rc=$?"
a3() { eval 'eval "q3[1x]=v"'; echo "in-func"; }
a3
echo "3 rc=$?"
a4() { eval 'q4[$(exit 3)1x]=v'; echo "in-func"; }
a4
echo "4 rc=$?"

echo "=== and past a sourced file ==="
printf 'q5[1x]=v\necho after-source\n' > sub-unwind-inc.sh
a5() { . ./sub-unwind-inc.sh; echo "in-func"; }
a5
echo "5 rc=$?"
rm -f sub-unwind-inc.sh

echo "=== every context that reaches a subscript reaches this ==="
b1() { eval '(( q6[1x] ))'; echo "in-func"; }
b1
echo "6 rc=$?"
b2() { eval '[[ 1 -eq q7[1x] ]]'; echo "in-func"; }
b2
echo "7 rc=$?"
b3() { eval 'let "q8[1x]"'; echo "in-func"; }
b3
echo "8 rc=$?"
b4() { eval 'q9=( [1x]=v )'; echo "in-func"; }
b4
echo "9 rc=$?"
b5() { eval 'declare "qa[1x]=v"'; echo "in-func"; }
b5
echo "10 rc=$?"
b6() { eval 'printf -v "qb[1x]" %s v'; echo "in-func"; }
b6
echo "11 rc=$?"

echo "=== but unset and a substring bound are the ordinary depth ==="
c1() { eval 'unset "qc[1x]"'; echo "in-func"; }
c1
echo "12 rc=$?"
c2() { v=abcdef; eval 'echo ${v:1x}'; echo "in-func"; }
c2
echo "13 rc=$?"
c3() { v=abcdef; eval 'echo ${v:1:1x}'; echo "in-func"; }
c3
echo "14 rc=$?"
c4() { eval 'qd[-9]=x'; echo "in-func"; }
c4
echo "15 rc=$?"

echo "=== a subscript inside a bound is the fatal one again ==="
d1() { v=abcdef; eval 'echo ${v:qe[1x]}'; echo "in-func"; }
d1
echo "16 rc=$?"
d2() { v=abcdef; eval 'echo ${v:1:qf[1x]}'; echo "in-func"; }
d2
echo "17 rc=$?"

echo "=== nothing between it and the reader stops it ==="
e1() { eval 'qg[1x]=v' || echo "or-else"; echo "in-func"; }
e1
echo "18 rc=$?"
e2() { eval 'qh[1x]=v'; }
if e2; then echo then; else echo else; fi
echo "19 rc=$?"
e3() { eval 'qi1[1x]=v'; }
while e3; do break; done
echo "20 rc=$?"
e4() { eval 'qj[1x]=v'; }
for z in 1; do e4; echo in-loop; done
echo "21 rc=$?"

echo "=== except a fork, which only loses its own side ==="
e5() { eval 'qk[1x]=v'; }
e5 | cat
echo "22 rc=$?"
( ql[1x]=v; echo "in-sub" )
echo "23 rc=$?"
echo "[$(qm[1x]=v; echo body)]"
echo "24 rc=$?"

echo TAIL
