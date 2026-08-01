# A word-expansion error that is not fatal still throws something away: bash
# jumps to its read-parse-execute loop, so the whole *parse unit* being run is
# abandoned and reading resumes with the next one. The unit is not a line — a
# compound command spanning several lines, and whatever follows its closing
# word, are one unit. And the jump ends there only in the main shell
# environment: inside a subshell it keeps going and the subshell is what ends,
# even when an `eval` or a command substitution's own loop stands in the way.
#
# The resumption cases run their input through an `eval` at the top level of the
# main shell, which is a read-eval loop just like the script's own, and drop the
# diagnostic — bash's line counter does not survive one of these jumps (it loses
# every line of the unit that went unrun and never resynchronises), which osh
# deliberately does not reproduce, so a line number here would not compare.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== reading resumes with the next unit"
eval 'a[-9]=v; echo no
echo next' 2>/dev/null
eval 'readonly r=1
r=2; echo no
echo next' 2>/dev/null

echo "=== …and the unit is what goes, not the line"
eval '{ a[-9]=v
echo no; }; echo after
echo next' 2>/dev/null
eval 'if true
then a[-9]=v
fi; echo after
echo next' 2>/dev/null
eval 'for i in 1
do a[-9]=v
done; echo after
echo next' 2>/dev/null
eval 'x=abc
{ echo "${x:0:-9}"; echo no; }; echo after
echo next' 2>/dev/null

echo "=== a subshell is ended, not merely trimmed"
( ( a[-9]=v; echo no ); echo "rc=$?" ) 2>&1 | e
( ( eval 'a[-9]=v'; echo no ); echo "rc=$?" ) 2>&1 | e
( x=$( { a[-9]=v
echo no ; } 2>&1 ); echo "[$x] rc=$?" ) 2>&1 | e
( x=`{ a[-9]=v
echo no ; } 2>&1`; echo "[$x] rc=$?" ) 2>&1 | e
( true | { a[-9]=v; echo no; }; echo "rc=$?" ) 2>&1 | e

echo "=== a function and a brace group are not subshells"
( f() { eval 'a[-9]=v'; echo yes; }; f; echo "rc=$?" ) 2>&1 | e
( { eval 'a[-9]=v'; echo yes; }; echo "rc=$?" ) 2>&1 | e

echo "=== an eval at the main shell's top level ends the jump"
( eval 'a[-9]=v'; echo "rc=$?" ) 2>&1 | e

echo "=== which errors discard at all"
( declare -i z; z=1/0; echo no ) 2>&1 | e
( echo "${nosucharr[@]=v}"; echo no ) 2>&1 | e
( declare -a q=(1); echo "[${q[-9]}]"; echo "rc=$?" ) 2>&1 | e
( q2=(1); q2[-9]=v true; echo "rc=$?" ) 2>&1 | e

echo "=== done"
