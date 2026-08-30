# A bare `$…` inside `$(( … ))` is read with the shell's parameter rule, not
# with "the longest run of name characters". Only the alphabetic form is
# greedy: `$x1` is one name, but `$12` is `$1` and then a literal `2` — the
# twelfth parameter needs the braces — and each special parameter is a single
# character, so `$#x` is the count and then a literal `x`.
#
# The special parameters really do expand here. `$#` is the count, `$?` the
# status, `$$` the pid, `$-` the option letters; `$*` and `$@` splice in their
# joined text, which is usually a syntax error and says so with the text in it.
#
# What follows a `$` may name nothing at all, and then the `$` is a literal and
# reaches the evaluator as one: `$(( 1 + $ 2 ))` is the same complaint bash
# makes about a stray `$`, not a silent zero.

set -- a b c

echo "=== the special parameters expand"
echo "count  $(( $# ))"
echo "count-1 $(( $# - 1 ))"
true;  echo "ok     $(( $? ))"
false; echo "fail   $(( $? ))"
echo "pid    $(( $$ == $$ ))"
echo "dash   $(( $- ))"
echo "bang   $(( $! + 0 ))"

echo "=== a digit is one digit, a special parameter is one character"
set -- 7 8 9 10 11 12 13
echo "one    $(( $1 ))"
echo "glued  $(( $12 ))"
echo "braced $(( ${12} ))"
echo "hashx  $(( $#x ))" 2>&1
x=3
x1=99
echo "name   $(( $x1 ))"
echo "twice  $(( $x$x ))"

echo "=== \$* and \$@ splice their joined text in"
set -- 4 5 6
echo "star   $(( $* ))" 2>&1
echo "at     $(( $@ ))" 2>&1
set -- 42
echo "one*   $(( $* ))" 2>&1
echo "one@   $(( $@ ))" 2>&1
set --
echo "none*  $(( $* + 1 ))" 2>&1
echo "none#  $(( $# ))"

echo "=== a \$ that names nothing stays a \$"
echo "space  $(( 1 + $ 2 ))" 2>&1
echo "plus   $(( $+2 ))" 2>&1
echo "alone  $(( $ ))" 2>&1
echo "paren  $(( ($) ))" 2>&1

echo "=== the same rule in the other arithmetic contexts"
set -- p q
(( $# == 2 )) && echo "cmd yes"
let "n = $# + 1"; echo "let $n"
declare -a arr=(zero one two)
echo "index  ${arr[$#]}"
echo "for:"; for ((i = 0; i < $#; i++)); do echo "  i=$i"; done
