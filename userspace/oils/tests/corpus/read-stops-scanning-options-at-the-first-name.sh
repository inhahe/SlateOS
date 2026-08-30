# `read`'s options stop at its first operand. Everything from there on is a
# name, whatever it looks like: `read a -u 3` assigns `a` and then refuses
# `-u`, where reading it as an option would have read fd 3 instead — a wrong
# answer rather than a refused one, since the assignment it does perform is
# not the one that was asked for.
#
# The names are judged one at a time and only as the assignment reaches them,
# which is why a bad one later in the list does not undo the good ones before
# it: `read a -r` still leaves `a` holding the first field, and only then says
# `read: `-r': not a valid identifier` and exits 1.
#
# `--` is the one word that ends the options without becoming a name, and only
# while the options are still being scanned — after a name it is a name like
# any other, and not a valid one. A lone `-` is never an option at all, so it
# ends the scan and is the first name.
#
# Every probe runs in a subshell so a name a probe assigned cannot reach the
# next one. Stderr is collected and replayed at the end so it can be compared
# in a fixed place; nothing here prints a pid, so it is replayed unfiltered.
printf 'one two three\n' > in
exec 4>&2 2>err

echo "=== an option after the first name is a name"
( read -r a -u 3 < in;   echo "  -u rc=$? a=[$a]" )
( read a -r < in;        echo "  -r rc=$? a=[$a]" )
( read a -d : < in;      echo "  -d rc=$? a=[$a]" )
( read a -N 2 < in;      echo "  -N rc=$? a=[$a]" )
( read a b -s < in;      echo "  two-names rc=$? a=[$a] b=[$b]" )

echo "=== the options before it are still options"
( read -r a b < in;      echo "  rc=$? a=[$a] b=[$b]" )
( read -r -N 3 a < in;   echo "  -N rc=$? a=[$a]" )
( read -ra arr < in;     echo "  -ra rc=$? n=${#arr[@]} 0=[${arr[0]}]" )

echo "=== -- ends the options, once"
( read -- a < in;        echo "  rc=$? a=[$a]" )
( read -r -- a < in;     echo "  after-r rc=$? a=[$a]" )
( read a -- b < in;      echo "  after-name rc=$? a=[$a] b=[$b]" )

echo "=== a lone dash is a name, not an end of options"
( read - < in;           echo "  rc=$?" )
( read - a < in;         echo "  with-name rc=$? a=[$a]" )

echo "=== an unknown option is still an unknown option in front"
( read -Z a < in;        echo "  rc=$?" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
