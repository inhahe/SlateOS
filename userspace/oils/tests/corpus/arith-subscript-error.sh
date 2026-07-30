# An array subscript reaches the arithmetic evaluator through an entry point of
# its own, and both halves of that show in what a failure inside one looks like:
# the text blamed is the *subscript's*, without the `((`/`let`/`[[`/`declare` tag
# the expression around it would have carried, and the failure is fatal to the
# rest of the parse unit — the way an expansion error is — rather than merely
# giving the command a non-zero status.
#
# So no probe here shares a line with the `rc=` echo that reports it: the fatal
# error abandons the rest of the line. A script resumes at the *next* line, where
# `$?` is still the abandoned command's status, which is what makes a file of
# these readable at all.

echo "=== the subscript is blamed, and no builtin tag is applied ==="
((a[1/0]=9))
echo "arith-cmd rc=$?"
let 'a[1/0]=9'
echo "let rc=$?"
[[ a[1/0] -eq 1 ]]
echo "cond rc=$?"
((a[1/0]))
echo "read rc=$?"
((b=a[1/0]))
echo "rhs rc=$?"
((a[1/0]++))
echo "incr rc=$?"
((a[1/0]+=9))
echo "compound rc=$?"
declare -i n=a[1/0]
echo "declare-i rc=$?"
echo $((a[1/0]))
echo "arith-sub rc=$?"
for ((i=a[1/0]; i<1; i++)); do echo "unreachable-for"; done
echo "for-arith rc=$?"

echo "=== and it is fatal: the rest of the line does not run ==="
((a[1/0]=9)); echo "unreachable-list"
echo "list rc=$?"
for j in 1; do ((a[1/0]=9)); echo "unreachable-loop"; done
echo "loop rc=$?"
f() { ((a[1/0]=9)); echo "unreachable-fn"; }
f
echo "fn rc=$?"
( ((a[1/0]=9)); echo "unreachable-subshell" )
echo "subshell rc=$?"
v=$( ((a[1/0]=9)); echo "unreachable-comsub" )
echo "comsub rc=$? v=[$v]"

echo "=== the other fatal thing a compound command can hit ==="
# A nested `$(( … ))` failure is an expansion error, so it is fatal too — and
# untagged, because the expansion path never sees the `((`/`[[` it sits inside.
# Each of these is alone on its line to pin that the abort is consumed *here*
# rather than left armed for whatever command runs next.
[[ $((1/0)) -eq 1 ]]
echo "cond-nested rc=$?"
(( $((1/0)) ))
echo "arith-nested rc=$?"

echo "=== the control: the same error outside a subscript ==="
# Tagged with the builtin, and only the command fails — the line goes on.
((a[0]=1/0)); echo "reached-after-plain-error"
echo "plain rc=$?"
let '1/0'; echo "reached-after-let"
echo "plain-let rc=$?"

echo "=== which text is blamed ==="
# A parse error in the subscript is a subscript error too…
((a[1+]=9))
echo "parse rc=$?"
# …as is a rejected number literal, whose echoed text is truncated at the
# literal's end within the subscript rather than within the whole expression.
((a[2#9]=9))
echo "lexeme rc=$?"
# The raw subscript text is echoed with its inner blanks intact.
((  a[  1/0  ]  =9  ))
echo "blanks rc=$?"
# Nesting blames the innermost subscript…
((a[b[1/0]]=9))
echo "nested rc=$?"
# …and a variable whose *value* is the bad expression blames that value.
x=1/0
((a[x]=9))
echo "via-value rc=$?"

echo "=== a refusal inside a subscript ==="
# The readonly wording never carried a tag to begin with, but inside a subscript
# it becomes fatal, where the same refusal outside one only fails the command.
readonly ro=1
((a[ro=5]=9)); echo "unreachable-readonly"
echo "ro-subscript rc=$?"
((ro=5)); echo "reached-after-readonly"
echo "ro-plain rc=$?"

echo "=== a well-formed subscript is unaffected ==="
((a[1+1]=9))
echo "good=${a[2]} rc=$?"
declare -A m
((m[1/0]=9))
echo "assoc-key rc=$? keys=${!m[*]}"

echo done
