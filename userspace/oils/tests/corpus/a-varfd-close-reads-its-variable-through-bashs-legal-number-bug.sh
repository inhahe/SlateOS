# `{v}>&-` names the descriptor it closes by the *value* of `$v`, and bash reads
# that value in `redir_varvalue` (`redir.c`) in three steps:
#
#     if (val == 0 || *val == 0)
#       return -1;
#     if (legal_number (val, &vmax) < 0)
#       return -1;
#     i = vmax;  /* integer truncation */
#     return i;
#
# Only the first of those refuses anything. `legal_number` (`general.c`) returns
# `1` or `0` and *never* a negative, and it writes `*result = 0` before it even
# looks at the string — so the second test never fires, and every value that is
# not a number silently becomes **0**. `v=nope; {v}>&-` closes stdin.
#
# The third step is a C narrowing conversion to `int`, so it wraps rather than
# saturating: `4294967296` closes fd 0 and `4294967297` closes fd 1, while
# `2147483648` wraps to `INT_MIN` and is refused — `do_redirection_internal`
# turns a negative descriptor into `ambiguous redirect`, which is the only
# refusal past the unset-or-empty one.
#
# So the whole rule is: unset or empty, or a value whose low 32 bits are
# negative, is `v: ambiguous redirect`; anything else closes *some* descriptor,
# and a value that is not a number closes fd 0.

echo '=== a value that is a number closes that descriptor'
( v=3; exec 3>&1; { echo gone; } {v}>&-; echo "rc=$?" ) 2>&1
( v=" 3 "; exec 3>&1; { echo gone; } {v}>&-; echo "rc=$?" ) 2>&1
( v="+3"; exec 3>&1; { echo gone; } {v}>&-; echo "rc=$?" ) 2>&1
( v="003"; exec 3>&1; { echo gone; } {v}>&-; echo "rc=$?" ) 2>&1

echo '=== a value that is not a number closes fd 0, because `legal_number` leaves 0'
( v=nope; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$? x=[$x]" ) </dev/null 2>&1
( v=0x10; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1
( v="1 2"; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1
( v="1x"; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1
( v="+"; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1
( v="-"; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1
( v=" "; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1

echo '=== an overflow is one of those, so it is fd 0 rather than a refusal'
( v=999999999999999999999; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1
( v=-999999999999999999999; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1

echo '=== `i = vmax` truncates to `int`, and wraps'
( v=4294967296; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1
( v=4294967297; exec 1>&2; { echo gone; } {v}>&-; echo "rc=$?" ) 2>&1
( v=2147483648; exec {v}>&-; echo "rc=$?" ) 2>&1
( v=2147483649; exec {v}>&-; echo "rc=$?" ) 2>&1

echo '=== only unset, empty, and a negative descriptor are refused'
( unset v; exec {v}>&-; echo "rc=$?" ) 2>&1
( v=; exec {v}>&-; echo "rc=$?" ) 2>&1
( v=-1; exec {v}>&-; echo "rc=$?" ) 2>&1
( v=" -1 "; exec {v}>&-; echo "rc=$?" ) 2>&1
( v=-0; exec {v}>&-; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1

echo '=== a command undoes the close; `exec` keeps it'
( v=3; exec 3>&1; true {v}>&-; echo "rc=$?"; echo back >&3 )
( v=3; exec 3>&1; exec {v}>&-; echo "rc=$?"; echo gone >&3 ) 2>&1
( v=nope; true {v}>&-; echo "rc=$?"; read -r x; echo "read=$? x=[$x]" ) </dev/null 2>&1

echo '=== and the same value rule drives the word form that becomes a close'
( v=3; e=-; exec 3>&1; exec {v}>&$e; echo "rc=$?"; echo gone >&3 ) 2>&1
( v=nope; e=-; exec {v}>&$e; echo "rc=$?"; read -r x; echo "read=$?" ) </dev/null 2>&1
( v=-1; e=-; exec {v}>&$e; echo "rc=$?" ) 2>&1

echo '=== done'
