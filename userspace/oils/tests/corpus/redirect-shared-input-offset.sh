# fd 0 is an *open file description*, not a private copy of the bytes.
#
# `< file` opens the file once, and everything that then reads fd 0 — a `read`
# builtin, an external child, a pipeline stage, a subshell, a command
# substitution, a `&` job — shares one position. So consumption by any of them
# is consumption for all of them: `{ head -n 1; read b; } < f` puts the *second*
# line in `b`, not the first.
#
# The same holds for `exec < f` and `exec 3< f`, for a descriptor made by a dup
# (`exec 4<&3` names the same description, so the two advance together), and —
# because bash spools one to a temp file — for a here-document.
#
# The counterpart is that a `< file` on a *simple* command is its own open, so
# two of them each start at the beginning.

printf 'r1\nr2\nr3\nr4\nr5\nr6\n' > six.txt

echo "=== a builtin and an external take turns"
{ read a; head -n 1; read b; } < six.txt; echo "  a=[$a] b=[$b]"
{ head -n 2; read c; } < six.txt; echo "  c=[$c]"
{ read a; cat; } < six.txt; echo "  a=[$a]"
{ sed -n 1p; sed -n 1p; } < six.txt

echo "=== ... and so does a subshell"
{ ( read a; echo "  sub=[$a]" ); read b; echo "  outer=[$b]"; } < six.txt
{ ( read a; read b; ); read c; echo "  outer=[$c]"; } < six.txt
# A command substitution is a subshell too, so what it reads is gone.
{ x=$(read a; echo "$a"); read b; } < six.txt; echo "  sub=[$x] outer=[$b]"
{ x=$(head -n 2); read b; } < six.txt; echo "  sub=[${x//$'\n'/,}] outer=[$b]"

echo "=== a pipeline stage reads the same descriptor"
{ head -n 1 | cat; head -n 1; } < six.txt
{ read a; { read b; echo "  stage=[$b]"; } | cat; read c; echo "  after=[$c]"; } < six.txt
# The stage is a subshell, so only the *position* comes back, not the variable.
{ read v | cat; read w; echo "  w=[$w]"; } < six.txt

echo "=== a persistent exec < is one description too"
exec < six.txt
read a; head -n 1; read b; echo "  a=[$a] b=[$b]"
x=$(read v; echo "$v"); read c; echo "  sub=[$x] after=[$c]"
exec <&-

echo "=== ... as is exec 3<"
exec 3< six.txt
read -u 3 a; ( read -u 3 b ); read -u 3 c; echo "  a=[$a] c=[$c]"
exec 4<&3
read -u 3 d; read -u 4 e; echo "  d=[$d] e=[$e]"
exec 3<&- 4<&-

echo "=== a here-document is spooled, so it shares a position as well"
{ read a; ( read b; echo "  sub=[$b]" ); read c; echo "  a=[$a] c=[$c]"; } <<'H'
h1
h2
h3
H
{ read a; cat; } <<'H'
h1
h2
H

echo "=== but two simple-command redirects are two opens"
read p < six.txt; read q < six.txt; echo "  p=[$p] q=[$q]"
head -n 1 < six.txt; head -n 1 < six.txt

echo "=== a loop consumes the whole file exactly once"
n=0; while read l; do n=$((n+1)); done < six.txt; echo "  lines=$n"
n=0; while read l; do n=$((n+1)); head -n 1 > /dev/null; done < six.txt; echo "  lines=$n"

echo "=== a & job holds the same description"
{ read a; { read b; echo "  job=[$b]" ; } & wait; read c; echo "  a=[$a] c=[$c]"; } < six.txt

echo "=== the file is opened at redirection time"
{ read a; } < nosuch.txt; echo "  st=$?"
