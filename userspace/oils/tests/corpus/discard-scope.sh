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
# diagnostic, because what they are asking is only which commands still ran.
#
# Everything else keeps its line number. bash's counter does not survive one of
# these jumps — it is left below where the lexer had driven it and never
# resynchronises — and osh reproduces that (known-issues
# TD-OILS-A-DISCARD-OUT-OF-A-COMPOUND-COMMAND-LOSES-BASH-A-LINE), so the numbers
# compare byte for byte. They used to be stripped through a `sed` helper; this
# drift was the only reason for it, and it is gone.

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
( ( a[-9]=v; echo no ); echo "rc=$?" ) 2>&1
( ( eval 'a[-9]=v'; echo no ); echo "rc=$?" ) 2>&1
( x=$( { a[-9]=v
echo no ; } 2>&1 ); echo "[$x] rc=$?" ) 2>&1
( x=`{ a[-9]=v
echo no ; } 2>&1`; echo "[$x] rc=$?" ) 2>&1
( true | { a[-9]=v; echo no; }; echo "rc=$?" ) 2>&1

echo "=== a function and a brace group are not subshells"
( f() { eval 'a[-9]=v'; echo yes; }; f; echo "rc=$?" ) 2>&1
( { eval 'a[-9]=v'; echo yes; }; echo "rc=$?" ) 2>&1

echo "=== an eval at the main shell's top level ends the jump"
( eval 'a[-9]=v'; echo "rc=$?" ) 2>&1

echo "=== which errors discard at all"
( declare -i z; z=1/0; echo no ) 2>&1
( echo "${nosucharr[@]=v}"; echo no ) 2>&1
( declare -a q=(1); echo "[${q[-9]}]"; echo "rc=$?" ) 2>&1
( q2=(1); q2[-9]=v true; echo "rc=$?" ) 2>&1

echo "=== done"
