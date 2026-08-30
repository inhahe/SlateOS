# bash has no rule about compound literals after the command word, so there is
# no rule for it to cite: `echo n=(x y)` is an ordinary word followed by a `(`
# the grammar had no place for, and what the diagnostic names is the **token** —
# `` syntax error near unexpected token `(' `` — followed, as every `near`
# diagnostic is, by the offending source line echoed back verbatim.
#
# The line echoed is the *whole* physical line, not the command that failed on
# it, and inside an `eval` it is the eval string, tagged `eval: line N:`.
#
# A subscript changes nothing: the word rule is about the word, and `n[1]=(x y)`
# before the command word is an assignment refused at runtime instead — see
# a-subscripted-compound-assignment-parses-and-is-refused-when-it-binds.sh.
#
# A declaration builtin is the exception, since a compound literal *is* an
# operand there. The last case has to go last: a script does not survive a
# syntax error.

echo '=== an eval confines it, and reports 2'
eval 'echo n=(x y)'; echo "s=$?"
eval 'echo n[1]=(x y)'; echo "s=$?"
eval 'true; echo n=(x y); echo NOT'; echo "s=$?"
eval 'n=(a b); echo "${n[0]}"'; echo "s=$?"
echo '=== the word before it does not have to be a command name'
eval 'echo hi n=(x y)'; echo "s=$?"
eval 'x=1 echo n=(x y)'; echo "s=$?"
eval ': ; echo n=(x y)'; echo "s=$?"
echo '=== a declaration builtin takes one as an operand, as ever'
eval 'declare -A m=([k]=v); declare -p m'; echo "s=$?"
eval 'declare n[1]=(x y)'; echo "s=$?"
eval 'f() { local -a q=(1 2); declare -p q; }; f'; echo "s=$?"
eval 'readonly r=(1 2); declare -p r'; echo "s=$?"
echo '=== a quoted one is only a word'
echo 'n=(x y)'; echo "s=$?"
eval 'echo "n=(x y)"'; echo "s=$?"
eval 'echo n=\(x y\)'; echo "s=$?"
echo '=== the whole physical line is echoed, not the failing command'
eval 'true; echo n=(x y); echo NOT
echo NOT2'; echo "s=$?"
echo '=== and a bare one kills the script, so it goes last'
( echo n=(x y) ); echo "s=$?"
echo NOT-REACHED
