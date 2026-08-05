# `(( … ))` is an arithmetic command only when its two closing parentheses are
# *adjacent*. bash reads the body with `parse_matched_pair` and then tests for
# the second `)` with a read of its own — one that does not delete a
# `\<newline>` — so nothing at all may come between the two: not a space, not a
# tab, not a newline, not a continuation.
#
# What happens when the test fails is not an error. `parse_arith_cmd` hands the
# text back to the ordinary grammar as `( ( … ) )`, so the whole thing is a pair
# of nested subshells and the "expression" is a command:
#
#   ((echo hi) )          prints `hi`
#   ((a=1) )              runs the assignment, confined to a subshell
#   ((1+1) )              1+1: command not found
#
# The *body* is read with continuation removal on, which is why `((1 +\<newline>1))`
# is still arithmetic — the rule is about the closers alone. So is the opening
# pair: `(\<newline>(` is `((` by the time the parser sees it.
#
# A `for` header is the one place with nothing to fall back to — `for` cannot be
# followed by a subshell — so there the same failed test is simply an error.
#
# Two shapes are left out because bash's fallback is visibly lossy there, and
# one because bash's *string* input and its *stream* input disagree; see
# TD-OILS-AN-ARITHMETIC-COMMAND-NEEDS-ITS-CLOSERS-ADJACENT.

echo "=== adjacent is arithmetic"
((1+1)) && echo hit
((1 +\
1)) && echo hit
(\
( 1+1 )) && echo hit
((1+1))\
 && echo hit
if ((1)); then echo if-yes; fi

echo "=== anything in between makes it two subshells"
((echo hi) )
((a=1) ); echo "a=$a"
((1+1) )
((1+1)  )
(( 1+1 ) )
((1+1)	)
(( ( a ) ) )
((echo one) ) ; ((echo two) )

echo "=== a newline between the closers is nothing special either"
((echo nl)
)
((1+1)
)

echo "=== and the subshells are real subshells"
b=outer; ((b=inner) ); echo "b=$b"
((cd / ) ); echo "pwd-unchanged=$?"
((exit 3) ); echo "status=$?"
((echo sub; echo sub2) )

echo "=== wherever the construct is written"
echo $( ((echo cmdsub) ) )
{ ((echo brace) ) ; }
f() { ((echo func) ) ; }; f
while ((false) ); do echo no; done; echo "while-done=$?"
for i in a; do ((echo loop) ); done

echo "=== the expansion form has no such test"
echo $((1+1))
echo $((1 +\
1))
echo $((1+1)\
)
x=$((1+1) ); echo "x=$x"

echo "=== but a for header has nothing to fall back to"
# Only that it *fails* is pinned here; osh spells the failure differently from
# bash, which reports `syntax error near `)'` — see the same known issue.
( for ((i=0;i<2;i++)); do echo "i=$i"; done )
( eval 'for ((i=0;i<1;i++) ) ; do echo $i; done' ) 2>/dev/null; echo "for-status=$?"
echo done
