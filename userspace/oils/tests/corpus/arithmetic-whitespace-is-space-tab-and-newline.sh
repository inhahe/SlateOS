# The arithmetic evaluator's notion of whitespace is not `isspace`. expr.c names
# it once, and only three characters are in it:
#
#     /* Here is a macro which accepts newlines, tabs and spaces as whitespace. */
#     #define cr_whitespace(c) (whitespace(c) || ((c) == '\n'))    /* expr.c:92-93 */
#
# with `whitespace(c)` being `((c) == ' ' || (c) == '\t')` (general.h:77). So a
# vertical tab, a carriage return and a form feed are ordinary *characters* to
# the lexer, and land inside the error token rather than beside it.
#
# Three separate rules follow from that, and they are not the same rule:
#
#  1. The lexer skips `cr_whitespace` between tokens (expr.c:1321, 1470).
#  2. `subexpr` skips it too — but *only* to decide the expression is empty. The
#     string it then evaluates and echoes is the one it was handed, untouched:
#
#         for (p = expr; p && *p && cr_whitespace (*p); p++)
#           ;
#         if (p == NULL || *p == '\0')
#           return (0);
#         pushexp ();
#         expression = savestring (expr);        /* expr.c:457-464 */
#
#  3. `evalerror` skips the *narrower* set when echoing — bare `whitespace`, so
#     space and tab but **not** newline:
#
#         for (t = expression; t && whitespace (*t); t++)
#           ;                                    /* expr.c:1524-1525 */
#
# Hence a leading newline survives into the diagnostic while a leading tab does
# not, and a trailing blank survives either way — it is simply part of what was
# lexed.
#
# The text reaching the evaluator is likewise verbatim: `expand_arith_string` is
# ordinary word expansion and has no whitespace rule of its own, so a variable's
# value, a command substitution's output and a nested `$(( ))`'s digits are all
# spliced in as they stand. `$(( $v ))` with `v=$' 3 w '` therefore reports
# `3 w  ` — the value's own trailing space and the `$(( … ))`'s beside it.
#
# A variable read *as* arithmetic goes the same way: `expr_streval` does
# `tval = (value && *value) ? subexpr (value) : 0` (expr.c:1231), so the value
# arrives whole and rule 3 alone decides what the echo drops off its front.
#
# Verified against bash 5.2.37.

# --- rule 3: what the echo keeps ------------------------------------------
m=$'\t\n 3 w';        echo "1 $(( $m ))"
n=$'\n3 w';           echo "2 $(( $n ))"
t1=$'\t3 w';          echo "3 $(( $t1 ))"
s1='   3 w';          echo "4 $(( $s1 ))"

# --- the value is spliced in verbatim -------------------------------------
v1='1 + ';            echo "5 $(( $v1 ))"
v2=$' 3 w ';          echo "6 $(( $v2 ))"
echo "7 $(( $(printf ' 3 w ') ))"
echo "8 $(( `printf ' 3 w '` ))"
echo "9 $(( $(printf '3\n\n') + 1 ))"
v3=' 4 ';             echo "10 $(( $v3 + 1 ))"
echo "11 $(( $((  )) + 1 ))"

# --- \v, \r and \f are characters, not separators -------------------------
b1=$'\vZ';            echo "12 $(( $b1 ))"
b2=$'\v3 w';          echo "13 $(( $b2 ))"
b3=$'\r5';            echo "14 $(( $b3 ))"
b4=$'\f5';            echo "15 $(( $b4 ))"

# --- rule 2: only these three make an expression "empty" ------------------
e1=$'\n \t';          echo "16 [$(( $e1 ))]"
e2=$'\v';             echo "17 [$(( $e2 ))]"
echo "18 [$(( ))]"

# --- the same three rules through a substring bound and a subscript -------
s2=abcdef
q1=$'\nq)r';          echo "19 [${s2:$q1}]"
q2=$'  2';            echo "20 [${s2:$q2}]"
declare -a arr=(0 1 2)
i1=$'\n1';            echo "21 [${arr[$i1]}]"
i2=$'\v1';            echo "22 [${arr[$i2]}]"

# --- a value read *as* arithmetic keeps its own leading newline -----------
vv=$'\n3 w';          echo "23 $(( vv ))"
vw=$' \t3 w';         echo "24 $(( vw ))"
vx=$'\v3 w';          echo "25 $(( vx ))"

# --- and the tail is never trimmed, in either reading ---------------------
vy='1 + ';            echo "26 $(( vy ))"
vz=$' 1/0 ';          echo "27 $(( vz ))"

echo "done $?"
