# A negative arithmetic subscript that reaches back past the start of what the
# name holds names nowhere, and bash refuses it — twice over, in two different
# spellings, because the read and the store are two different pieces of code.
#
#  1. The *read* goes through `expr_streval` → `array_reference`, which finds the
#     array first and blames it by the name the reference resolved to. It quotes
#     no subscript at all: `a: bad array subscript`. A reference that reaches no
#     array — one landing on an element, or a chain closing on itself — has no
#     other name to give, so it is blamed as written.
#
#  2. The *store* goes through `expr_bind_variable`, which is handed the whole
#     `a[sub]` *text*: a read that failed never cached an index in `curlval.ind`,
#     so `lind` stays -1 and `expr_bind_array_element` — which would have
#     rewritten the subscript to its value (expr.c:365-388) — is never reached.
#     So the complaint spells the subscript's source back verbatim, whitespace
#     and all, and names the variable as *written* rather than as resolved.
#
#     `+=` earns both lines, in that order; a plain `=` never reads and so earns
#     only the second.
#
#  3. The two ask different questions. A negative subscript counts back from one
#     past the highest index the name *holds*, and a plain scalar holds none —
#     but a store is about to make it element 0 of the array it brings into
#     being, so by then there is one element to count back over. Hence `c=9;
#     (( c[-1] ))` is refused while `c=9; (( c[-1] = 4 ))` succeeds and replaces
#     the 9.
#
#  4. A refused store stops where it stands: it never converts a scalar, never
#     creates an unset name, and never pays for the walk it did not take — a
#     circular reference earns two warnings instead of three and survives as a
#     reference.
#
#  5. Only an *indexed* array asks. An associative one takes `-7` as an ordinary
#     key. And nothing is asked under suppression, where no read and no store
#     happens at all.
#
# Verified against bash 5.2.37.

# --- 1. the read names, the store spells -----------------------------------
( a=(1 2 3); (( a[-7] )); echo "1  rc=$?" )
( a=(1 2 3); (( a[-7] = 1 )); echo "2  rc=$? $(declare -p a)" )
( a=(1 2 3); (( a[-7] += 1 )); echo "3  rc=$?" )
( a=(1 2 3); (( a[-7]++ )); echo "4  rc=$?" )
( a=(1 2 3); (( ++a[-7] )); echo "5  rc=$?" )
# The subscript source is echoed exactly, however it was written.
( a=(1 2 3); (( a[-3-4] = 1 )); echo "6  rc=$?" )
( a=(1 2 3); (( a[ -7 ] = 1 )); echo "7  rc=$?" )
( a=(1 2 3); q=7; (( a[-q] = 1 )); echo "8  rc=$?" )
# …including after a read that had already evaluated it.
( a=(1 2 3); q=7; (( a[-q] += 1 )); echo "9  rc=$?" )
# The boundary: -3 is the first element, -4 is past the start.
( a=(1 2 3); echo "10 [$(( a[-3] ))] rc=$?" )
( a=(1 2 3); echo "11 [$(( a[-4] ))] rc=$?" )
( a=(1 2 3); (( a[-4] = 9 )); echo "12 rc=$? $(declare -p a)" )
( a=(1 2 3); (( a[-3] = 9 )); echo "13 rc=$? $(declare -p a)" )
# It counts back from the highest index, not from the element count.
( a=(1 2 3); unset 'a[1]'; echo "14 [$(( a[-3] ))] rc=$?" )
( a=(1 2 3); unset 'a[1]'; (( a[-3] = 9 )); echo "15 rc=$? $(declare -p a)" )

# --- 2. the read is stricter than the store --------------------------------
( c=9; (( c[-1] )); echo "16 rc=$?" )
( c=9; (( c[-1] = 4 )); echo "17 rc=$? $(declare -p c)" )
( c=9; (( c[-2] = 4 )); echo "18 rc=$? $(declare -p c)" )
( c=9; (( c[-1] += 1 )); echo "19 rc=$? $(declare -p c)" )
# A store that goes through carries the scalar into element 0 first.
( c=9; (( c[1] = 4 )); echo "20 rc=$? $(declare -p c)" )
( c=9; declare -i c; (( c[2] = 4 )); echo "21 rc=$? $(declare -p c)" )
# An unset name and a declared-empty array both hold nothing.
( unset u; (( u[-1] = 4 )); echo "22 rc=$? $(declare -p u 2>&1)" )
( unset u; (( u[-1] )); echo "23 rc=$?" )
( declare -a e=(); (( e[-1] = 9 )); echo "24 rc=$? $(declare -p e)" )
( declare -a e=(); (( e[0] = 9 )); echo "25 rc=$? $(declare -p e)" )

# --- 3. through a reference ------------------------------------------------
# The read follows it, the store does not.
( a=(1 2 3); declare -n r=a; (( r[-7] )); echo "26 rc=$?" )
( a=(1 2 3); declare -n r=a; (( r[-7] = 1 )); echo "27 rc=$?" )
# The bound is the target's all the same.
( q=(5 6 7); declare -n r=q; (( r[-1] = 9 )); echo "28 rc=$? $(declare -p q)" )
( c=9; declare -n r=c; (( r[-1] = 4 )); echo "29 rc=$? $(declare -p c)" )
( c=9; declare -n r=c; (( r[-2] = 4 )); echo "30 rc=$? $(declare -p c)" )
# A reference already naming an element reaches no array, so every negative
# subscript on it is bad — and that refusal comes *before* the one for the
# subscript having nothing to apply to.
( q=(5 6 7); declare -n r='q[0]'; (( r[1] = 9 )); echo "31 rc=$? $(declare -p q)" )
( q=(5 6 7); declare -n r='q[0]'; (( r[-1] = 9 )); echo "32 rc=$? $(declare -p q)" )
( q=(5 6 7); declare -n r='q[0]'; (( r[-1] )); echo "33 rc=$?" )
( declare -n r=undef; (( r[-1] = 9 )); echo "34 rc=$? $(declare -p undef 2>&1)" )
( declare -n r=undef; (( r[0] = 9 )); echo "35 rc=$? $(declare -p undef 2>&1)" )
# A refused store never pays for the bind it does not perform.
( declare -n c1=c2; declare -n c2=c1; (( c1[-1] )); echo "36 rc=$?" )
( declare -n c1=c2; declare -n c2=c1; (( c1[-1] = 5 )); echo "37 rc=$? $(declare -p c1 2>&1)" )
( declare -n c1=c2; declare -n c2=c1; (( c1[3] = 5 )); echo "38 rc=$? $(declare -p c1 2>&1)" )

# --- 4. who does not ask ---------------------------------------------------
( declare -A m=([x]=1); (( m[-7] )); echo "39 rc=$?" )
( declare -A m=([x]=1); (( m[-7] = 2 )); echo "40 rc=$? $(declare -p m)" )
( a=(1 2 3); (( 0 ? a[-7] : 5 )); echo "41 rc=$?" )
( a=(1 2 3); (( 0 && (a[-7] = 1) )); echo "42 rc=$? $(declare -p a)" )
# The refusal is a complaint, not an error: the expression keeps its value.
( a=(1 2 3); x=$(( a[-7] = 1 )); echo "43 x=$x rc=$?" )
( a=(1 2 3); x=$(( a[-7] + 1 )); echo "44 x=$x rc=$?" )
# Untagged by the builtin, unlike an arithmetic error.
( a=(1 2 3); let 'a[-7] = 1'; echo "45 rc=$?" )
( a=(1 2 3); let 'a[-7]'; echo "46 rc=$?" )
# A readonly target is judged after the subscript.
( declare -ar a=(1 2 3); (( a[-7] = 1 )); echo "47 rc=$?" )
( declare -ar a=(1 2 3); (( a[0] = 1 )); echo "48 rc=$?" )
# `set -e` ends the shell over either line, though neither fails on its own.
( set -e; a=(1 2 3); x=$(( a[-7] + 1 )); echo "49 after x=$x" )
( set -e; a=(1 2 3); (( a[-7] = 1 )); echo "50 after" )

echo "done $?"
