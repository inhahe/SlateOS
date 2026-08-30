# A `$( … )` body is read TWICE. The enclosing parse recurses into it to find
# the matching `)` (and raises a fatal syntax error there — that half is
# covered by cmdsub-error-fatality.sh); then, at expansion time, bash hands the
# body *text* to parse_and_execute, which is a read-eval loop. So the body is
# re-read one logical line at a time, and a state change made by line N is
# visible to the parse of line N+1 — exactly like a backtick body, and unlike a
# single pre-parsed program.
#
# The two spellings differ only in when the *first* read happens and in how the
# body's lines are numbered: `$( … )` numbers them by rank from the closing
# `)`'s line, `` ` … ` `` by a plain offset (see backtick-body-deferred.sh).
#
# See known-issues.md TD-OILS-CMDSUB-DOLLAR-NOT-INCREMENTAL.

echo "=== an alias defined by the body applies to the rest of the body"
shopt -s expand_aliases
x=$(alias q="echo aliased"
q)
echo "  status=$? x=[$x]"
x=$(alias r="echo one"; r
r)
echo "  same line, then next: status=$? x=[$x]"

echo "=== ... and an alias defined mid-body does not leak to the caller"
q 2>/dev/null || echo "  q is not a command out here: $?"

echo "=== a shopt in the body changes how the rest of the body parses"
# `shopt -s extglob` cannot be shown this way: `@(` is rejected by the *first*
# read, which is fatal to the whole script, so it never reaches the re-read.
# `expand_aliases` is the one shopt whose effect is purely on the read-eval
# loop, so it is the one that shows the second read happening.
shopt -u expand_aliases
x=$(shopt -s expand_aliases
alias s="echo shopt-then-alias"
s)
echo "  status=$? x=[$x]"
shopt expand_aliases
shopt -s expand_aliases

echo "=== an expansion-time syntax error is not fatal to the caller"
y=$(alias bad=for
echo one
bad)
echo "  status=$? y=[$y]"
echo "  the script kept going"

echo "=== commands before the error have already run"
: > side.txt
z=$(alias bad2=for
echo written > side.txt
bad2)
echo "  status=$? side=[$(cat side.txt)]"

echo "=== the body is re-read on every expansion, so the error repeats"
alias bad3=for
for i in 1 2; do echo "  iter $i:"; echo "  [$(echo pre
bad3)]"; done

echo "=== \$LINENO still uses the rank rule, counting from the closing paren"
y=$(echo $LINENO
echo $LINENO)
echo "  two lines: $y"
q2=$(echo $LINENO

echo $LINENO)
echo "  blank line between: $q2"
r2=$(echo $LINENO; echo $LINENO
echo $LINENO)
echo "  two commands sharing a line: $r2"

echo "=== ... and the expansion-time diagnostic uses the same numbering"
w=$(echo ok

alias bad4=for
bad4)
echo "  status=$? w=[$w]"

echo "=== an eval inside the body still reports as itself"
w=$(eval "for"; echo after)
echo "  status=$? w=[$w]"

echo "=== the \$(< file) fast path is unaffected, and stays whole-body only"
printf 'hello\nworld\n' > data.txt
echo "  [$(< data.txt)]"
echo "  [$(0< data.txt)]"
echo "  [$(< data.txt < data.txt)]"
echo "  [$(< data.txt
echo tail)]"

echo "=== a good body is unaffected"
echo "  [$(echo fine)]"
echo "  status=$?"
shopt -u expand_aliases
