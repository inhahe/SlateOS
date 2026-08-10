# bash's arithmetic evaluator has no parse phase. `readtok` resolves a NAME the
# moment it lexes it —
#
#     if (lasttok == PREINC || lasttok == PREDEC || peektok != EQ)
#       { lastlval = curlval; tokval = expr_streval (tokstr, e, &curlval); }
#     else
#       tokval = 0;                                        expr.c:1384-1391
#
# — and everything below follows from *when* that happens rather than from what
# the expression means.
#
#  1. Suppression is a runtime counter, not a branch not taken. `expcond`
#     (expr.c:630-663) and `explor`/`expland` (expr.c:668-718) still *walk* the
#     operand they have decided against, with `noeval` raised, and
#     `expr_streval` short-circuits at the top of it (expr.c:1157-1160). So the
#     untaken operand is read for its syntax and not for its value: a variable
#     holding nonsense is silent there, `set -u` is not asked, no store happens,
#     and a zero divisor is quietly 1 (expr.c:906-916).
#
#     `**` is the exception, and it is bash's, not a rule: `exppower`
#     (expr.c:960-978) has no `noeval` guard at all, so a negative exponent is an
#     error even in a branch that was never taken.
#
#  2. A NAME about to be *assigned* is not read (`peektok != EQ`) — which is what
#     makes `bad='1 + '; (( bad = 5 ))` a silent store where `(( bad += 5 ))` and
#     `(( bad == 5 ))` both fail inside `bad`. A prefix `++`/`--` overrides that
#     and reads anyway; the comment in expr.c calls it deliberate ("preserve old
#     behavior if a construct like --x=9 is given"), and it is observable,
#     because the increment is stored before the assignment is refused.
#
#  3. A read caches its subscript in `curlval.ind`, which `expassign` reuses
#     (`lind = curlval.ind`, expr.c:535-536). A reference that was never read has
#     no cached index, so the whole `a[sub]` *text* goes to `expr_bind_variable`
#     (expr.c:600-604) and its subscript is evaluated by the assignment
#     machinery — that is, *after* the right-hand side.
#
#  4. The result of a parenthesised group, of a unary operator and of an
#     increment is `lasttok = NUM` with `curlval` untouched, so none of them
#     names a location: `(x) = 3` and `x++ = 3` are assignments to a
#     non-variable even though `x` alone is not.
#
# Verified against bash 5.2.37.

# --- 1. suppression -------------------------------------------------------
bad='1 + '
echo "1  [$(( 0 ? bad : 5 ))] rc=$?"
echo "2  [$(( 1 ? 5 : bad ))] rc=$?"
echo "3  [$(( 0 && bad ))] rc=$?"
echo "4  [$(( 1 || bad ))] rc=$?"
echo "5  [$(( 0 ? 1/0 : 5 ))] rc=$?"
echo "6  [$(( 0 && 1/0 ))] rc=$?"
echo "7  [$(( 0 ? 1%0 : 5 ))] rc=$?"
( set -u; unset nope; echo "8  [$(( 0 ? nope : 5 ))] rc=$?" )
( unset x; echo "9  [$(( 0 ? (x=1) : 5 ))] x=[$x]" )
( unset x; echo "10 [$(( 0 && (x=1) ))] x=[$x]" )
( unset x; echo "11 [$(( 1 || (x=1) ))] x=[$x]" )
# `**` has no guard, so an untaken branch still errors.
echo "12 [$(( 0 ? 2**-1 : 5 ))] rc=$?"
echo "13 [$(( 0 && 2**-1 ))] rc=$?"
# A suppressed operand is still *walked*, so its syntax still has to hold.
echo "14 [$(( 0 ? 1 + : 5 ))] rc=$?"
echo "14b [$(( 0 && 1 + ))] rc=$?"

# --- 2. a name about to be assigned is not read ---------------------------
( bad='1 + '; (( bad = 5 )); echo "15 rc=$? bad=[$bad]" )
( bad='1 + '; (( bad += 5 )); echo "16 rc=$? bad=[$bad]" )
( bad='1 + '; (( bad == 5 )); echo "17 rc=$? bad=[$bad]" )
( bad='1 + '; (( bad++ )); echo "18 rc=$? bad=[$bad]" )
# The prefix operators read past the `=`, and store before refusing it.
( x=5; (( --x=9 )); echo "19 rc=$? x=$x" )
( x=5; (( ++x=9 )); echo "20 rc=$? x=$x" )
# `set -u` rides on the read, so it fires for the compound form and not the
# plain one — which reaches the subscript first instead.
( set -u; unset nada; (( nada[nope] = 1 )); echo "21 rc=$?" )
( set -u; unset nada; (( nada[nope] += 1 )); echo "22 rc=$?" )

# --- 3. an unread subscript is evaluated after the right-hand side --------
( unset a q; (( a[q=1] = (q+9) )); echo "23 q=$q a1=[${a[1]}] a0=[${a[0]}]" )
( unset a q; (( a[q=1] += (q+9) )); echo "24 q=$q a1=[${a[1]}] a0=[${a[0]}]" )
( unset a q; (( a[q=1]++ )); echo "25 q=$q a1=[${a[1]}] a0=[${a[0]}]" )
( unset a q; (( a[q=1] = q )); echo "26 q=$q a1=[${a[1]}] a0=[${a[0]}]" )
# The unread subscript still reaches the assignment, so its own errors do too.
( unset a; (( a[1/0] = 5 )); echo "27 rc=$?" )
# An empty or whole-array subscript is refused where it is *read*, so the plain
# assignment earns only the store's complaint and the compound one earns both.
( unset a; (( a[] = 3 )); echo "28 rc=$?" )
( unset a; (( a[] += 3 )); echo "29 rc=$?" )

# --- 4. what does and does not name a location ----------------------------
( x=5; (( (x) = 3 )); echo "30 rc=$? x=$x" )
( x=5; (( (x)++ )); echo "31 rc=$? x=$x" )
( x=5; (( -x = 3 )); echo "32 rc=$? x=$x" )
( x=5; (( +x = 3 )); echo "33 rc=$? x=$x" )
( x=5; (( x++ = 3 )); echo "34 rc=$? x=$x" )
( x=5; (( ++(x) )); echo "35 rc=$? x=$x" )
( x=5; (( (c=1) ? x : 0 )); echo "36 rc=$? c=$c" )

# --- the value a name holds is the expression the failure is reported from -
( bad='1 + '; echo "37 [$(( bad + ))] rc=$?" )
( bad='1 + '; echo "38 [$(( bad ))] rc=$?" )
( d=' 1/0 '; echo "39 [$(( d ))] rc=$?" )

echo "done $?"
