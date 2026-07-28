# $LINENO inside a command substitution. bash's rule here is a parser artifact:
# the value is (line of the closing paren) + (0-based rank of the body line
# among the body's command-bearing lines), so blank body lines do not advance
# it. The numbers below therefore look off even for bash, but they are what
# bash 5.x prints and are therefore the target. See known-issues.md
# BUG-OILS-LINENO-IN-CMDSUB for the probe table this was derived from.
echo "L7=$LINENO"
v=$(
echo "a=$LINENO"
echo "b=$LINENO"
)
echo "$v"
echo "L13=$LINENO"
w=$(echo "oneline=$LINENO")
echo "$w"
# A blank line inside the body must NOT advance the count.
u=$(echo "first=$LINENO"

echo "second=$LINENO")
echo "$u"
# The rebased line reaches diagnostics too: this names the closing paren's line,
# not "line 1" and not the failing command's own line.
d=$(
nosuchcommand_xyz 2>&1
)
echo "$d"
