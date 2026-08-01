# `test` has two complaints about words it could not use, and the first
# unusable word picks between them: one starting with `-` was probably meant
# to be an operator, so it gets named, while anything else is merely surplus
# and gets the anonymous "too many arguments". The parenthesis rules are
# stranger still: exactly four fully parenthesised words are the *two*-argument
# test of what is inside, so they produce the two-argument diagnostic rather
# than a grouping one.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== surplus words that do not look like operators"
( test a b c d e; echo "rc=$?" ) 2>&1 | e
( test a b c d; echo "rc=$?" ) 2>&1 | e
( test x y z w; echo "rc=$?" ) 2>&1 | e
( test a -a b c; echo "rc=$?" ) 2>&1 | e
( test a -a b -a c d; echo "rc=$?" ) 2>&1 | e
( test x -o y z; echo "rc=$?" ) 2>&1 | e
( test '(' a ')' b; echo "rc=$?" ) 2>&1 | e
( test '(' x ')' '('; echo "rc=$?" ) 2>&1 | e
( test ! ! ! x y; echo "rc=$?" ) 2>&1 | e
( test x y z -q; echo "rc=$?" ) 2>&1 | e

echo "=== …and ones that do"
( test x -a y -Q z; echo "rc=$?" ) 2>&1 | e
( test x -a y - z; echo "rc=$?" ) 2>&1 | e

echo "=== a group names the word it found instead of the paren"
( test '(' a b c ')'; echo "rc=$?" ) 2>&1 | e
( test '(' a b c d ')'; echo "rc=$?" ) 2>&1 | e

echo "=== four parenthesised words are the two-argument test"
( test '(' x y ')'; echo "rc=$?" ) 2>&1 | e
( test '(' -Q y ')'; echo "rc=$?" ) 2>&1 | e
( test '(' = y ')'; echo "rc=$?" ) 2>&1 | e
( test '(' -n x ')'; echo "rc=$?" ) 2>&1 | e
( test '(' -z x ')'; echo "rc=$?" ) 2>&1 | e
( test '(' ! x ')'; echo "rc=$?" ) 2>&1 | e
( test '(' -f nosuch ')'; echo "rc=$?" ) 2>&1 | e

echo "=== the grouping that really is grouping still groups"
( test '(' '(' x ')' ')'; echo "rc=$?" ) 2>&1 | e
( test '(' a ')' -a '(' b ')'; echo "rc=$?" ) 2>&1 | e
( test ! '(' x ')'; echo "rc=$?" ) 2>&1 | e
( test '(' a b; echo "rc=$?" ) 2>&1 | e
( test '(' a; echo "rc=$?" ) 2>&1 | e

echo "=== [ still needs its bracket, and says so bash's way"
( [ x; echo "rc=$?" ) 2>&1 | e
( [; echo "rc=$?" ) 2>&1 | e
( [ ] extra; echo "rc=$?" ) 2>&1 | e
( [ x ] extra; echo "rc=$?" ) 2>&1 | e
( [ ]; echo "rc=$?" ) 2>&1 | e
( [ ']'; echo "rc=$?" ) 2>&1 | e

echo "=== done"
