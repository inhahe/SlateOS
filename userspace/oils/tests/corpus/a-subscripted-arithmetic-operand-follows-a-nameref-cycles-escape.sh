# A *subscripted* arithmetic operand follows a nameref cycle's global escape,
# exactly as the unsubscripted one does.
#
# A cycle that closes on a function-local reference does not resolve to nothing:
# `find_variable_nameref` (variables.c:2074) warns and then escapes to global
# scope —
#
#	  if (v == orig || v == oldv)
#	    {
#	      internal_warning (_("%s: circular name reference"), orig->name);
#	      if (variable_context && v->context)
#		return (find_global_variable_noref (v->name));
#
# — so `f() { declare -n g=z; declare -n z=g; (( g[1] )); }` reads element 1 of
# the *global* `g`, not of the local reference standing in front of it. Every
# question the operand asks has to be asked there: which kind of array it is
# (index vs key), how far back a negative subscript may reach, the element
# itself, and where a store lands.
#
# The warning counts are the second half of the measurement, since each walk
# warns once (expr.c:1180 reads a subscripted operand through
# `array_variable_part` and then again through `get_array_value`, an
# unsubscripted one through `find_variable` alone):
#
#	   read  a[i] 2   read  m[k] 2   read  x 1
#	   write a[i] 3   write m[k] 2   write x 2
#	   a[i] += 5      m[k] += 4
exec 2>&1
declare -a ga=(4 5)
gs=42
declare -A gm=([k]=v)
declare -A gn=([k]=11)
echo "--- the read lands on the global the cycle escaped to"
r1() { declare -n ga=z; declare -n z=ga; echo "[$(( ga[1] ))]"; }; ( r1 )
r2() { declare -n gs=z; declare -n z=gs; echo "[$(( gs[0] ))]"; }; ( r2 )
r3() { declare -n gn=z; declare -n z=gn; echo "[$(( gn[k] ))]"; }; ( r3 )
echo "--- and so does the index-vs-key question: a key is not an expression"
r4() { declare -n gm=z; declare -n z=gm; echo "[$(( gm[k] ))]"; }; ( r4 )
echo "--- set -u asks about the escaped variable, not the reference"
r5() { declare -n gs=z; declare -n z=gs; echo "[$(( gs[0] ))]"; }; ( set -u; r5 ); echo "st=$?"
r6() { declare -n ga=z; declare -n z=ga; echo "[$(( ga[1] ))]"; }; ( set -u; r6 ); echo "st=$?"
r7() { declare -n z1=z2; declare -n z2=z1; echo "[$(( z1[0] ))]"; }; ( set -u; r7 ); echo "st=$?"
r8() { declare -n z1=z2; declare -n z2=z1; echo "[$(( z1[0] ))]"; }; ( r8 )
echo "--- a negative subscript is measured against the escaped array"
r9() { declare -n ga=z; declare -n z=ga; echo "[$(( ga[-1] ))]"; }; ( r9 )
ra() { declare -n ga=z; declare -n z=ga; echo "[$(( ga[-7] ))]"; }; ( ra )
echo "--- the store lands there too"
w1() { declare -n ga=z; declare -n z=ga; (( ga[1] = 9 )); echo "st=$?"; }; w1; declare -p ga
w2() { declare -n gm=z; declare -n z=gm; (( gm[k2] = 3 )); echo "st=$?"; }; w2; declare -p gm
echo "--- read-modify-write walks both ways"
w3() { declare -n ga=z; declare -n z=ga; (( ga[0] += 10 )); }; w3; declare -p ga
w4() { declare -n gn=z; declare -n z=gn; (( gn[k] += 10 )); }; w4; declare -p gn
echo "--- and the reference survives all of it"
w5() { declare -n ga=z; declare -n z=ga; (( ga[1] = 1 )); declare -p ga; }; ( w5 )
echo "--- an unsubscripted operand was already following the escape"
u1() { declare -n gs=z; declare -n z=gs; echo "[$(( gs ))]"; }; ( u1 )
u2() { declare -n gs=z; declare -n z=gs; (( gs = 7 )); }; u2; declare -p gs
echo TAIL
