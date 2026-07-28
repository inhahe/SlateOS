# EXPECT-DIFF: BUG-OILS-LINENO-IN-CMDSUB — $LINENO restarts at 1 inside $( )
# $LINENO inside a command substitution. bash continues the enclosing script's
# numbering; osh restarts the substitution body at 1. bash's exact rule is a
# parser artifact: the value is (line of the closing paren) + (0-based rank of
# the body line among the body's command-bearing lines), so blank body lines do
# not advance it. The numbers below therefore look off even for bash, but they
# are what bash 5.x prints and are therefore the target. See
# known-issues.md BUG-OILS-LINENO-IN-CMDSUB for the full probe table.
echo "L7=$LINENO"
v=$(
echo "a=$LINENO"
echo "b=$LINENO"
)
echo "$v"
echo "L14=$LINENO"
w=$(echo "oneline=$LINENO")
echo "$w"
# A blank line inside the body must NOT advance the count.
u=$(echo "first=$LINENO"

echo "second=$LINENO")
echo "$u"
