# bash negates a `!`-prefixed pipeline at the bottom of
# `execute_command_internal`:
#
#     if (invert)
#       exec_result = (exec_result == EXECUTION_SUCCESS) ? EXECUTION_FAILURE
#                                                        : EXECUTION_SUCCESS;
#                                                 /* execute_cmd.c:1123-1125 */
#
# so a `jump_to_top_level` raised inside the pipeline flies straight past it and
# the status the unwind carried arrives unturned. `! q[1x]=v` is 1, not 0.
#
# The one place a negation *does* survive such an unwind is a `( … )`, because
# `execute_in_subshell` takes the flag off the subshell's single top-level
# command before establishing its own jump target and puts the negation back
# after it:
#
#     invert = (tcom->flags & CMD_INVERT_RETURN) != 0;
#     tcom->flags &= ~CMD_INVERT_RETURN;
#
#     result = setjmp_nosigs (top_level);
#     …
#     else if (result)
#       return_code = (last_command_exit_value == EXECUTION_SUCCESS) ? EXECUTION_FAILURE : last_command_exit_value;
#     …
#     if (invert)
#       return_code = (return_code == EXECUTION_SUCCESS) ? EXECUTION_FAILURE
#                                                        : EXECUTION_SUCCESS;
#                                                 /* execute_cmd.c:1701-1726 */
#
# `tcom` is the whole command between the parens, so the flag is there only when
# that command *is* the negated pipeline: `( ! q[1x]=v )` is 0 while
# `( ! q[1x]=v; echo after )` is 1, the `!` in the second sitting on an inner
# pipeline of a list. `user_subshell` gates the hoist, so a `$( … )` never gets
# one.
#
# `EXITPROG` sets `invert = 0` outright, which is why `( ! exit 3 )` is 3.
#
# Both kinds of unwind are covered: an arithmetic error in an array subscript
# and one binding an integer-attribute value. A discard an `eval` *caught* is
# not an unwind at the `!` any more, and is negated as usual.
#
# Verified against bash 5.2.37.

echo "=== an unwind arrives unturned ==="
! y1[1x]=v 2>/dev/null
echo "1 rc=$?"
! { y2[1x]=v; } 2>/dev/null
echo "2 rc=$?"
! eval 'y3[1x]=v' 2>/dev/null
echo "3 rc=$?"
! declare -i n1=2+ 2>/dev/null
echo "4 rc=$?"
! ! ( eval 'y4[1x]=v' ) 2>/dev/null
echo "5 rc=$?"

echo "=== and it does not matter which side of a function the ! is on ==="
f1() { ! eval 'y5[1x]=v'; }
f1 2>/dev/null
echo "6 rc=$?"
f2() { eval 'y6[1x]=v'; }
! f2 2>/dev/null
echo "7 rc=$?"
f3() { ! eval 'declare -i n2=2+'; }
f3 2>/dev/null
echo "8 rc=$?"

echo "=== nor which condition it stands in ==="
f4() { eval 'y7[1x]=v'; }
until ! f4 2>/dev/null; do break; done
echo "9 rc=$?"
f5() { eval 'y8[1x]=v'; }
while ! f5 2>/dev/null; do break; done
echo "10 rc=$?"
f6() { eval 'y9[1x]=v'; }
if ! f6 2>/dev/null; then echo then; else echo else; fi
echo "11 rc=$?"

echo "=== a discard the eval caught is negated as usual ==="
f7() { eval 'ya[-9]=v'; }
! f7 2>/dev/null
echo "12 rc=$?"
! false
echo "13 rc=$?"
! true
echo "14 rc=$?"

echo "=== a ( ) hoists the ! above its own jump target ==="
( ! yb[1x]=v ) 2>/dev/null
echo "15 rc=$?"
( ! ( yc[1x]=v ) ) 2>/dev/null
echo "16 rc=$?"
( ! { yd[1x]=v; } ) 2>/dev/null
echo "17 rc=$?"
( time ! ye[1x]=v ) 2>/dev/null
echo "18 rc=$?"
( ! declare -i n3=2+ ) 2>/dev/null
echo "19 rc=$?"

echo "=== but only when the ! is on the whole of it ==="
( ! yf[1x]=v; echo after ) 2>/dev/null
echo "20 rc=$?"
( ! yg[1x]=v && echo b ) 2>/dev/null
echo "21 rc=$?"
( ! yh[1x]=v || echo b ) 2>/dev/null
echo "22 rc=$?"
x=$( ! yi1[1x]=v 2>/dev/null )
echo "23 rc=$? x=[$x]"

echo "=== and an exit is never negated ==="
( ! exit 3 )
echo "24 rc=$?"
( ! exit 0 )
echo "25 rc=$?"

echo TAIL
