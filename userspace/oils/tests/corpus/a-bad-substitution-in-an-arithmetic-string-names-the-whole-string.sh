# Arithmetic text is a word expansion of its own: bash's `expand_arith_string`
# hands it to `expand_word_internal` before `evalexp` ever sees it. So a
# `${ … }` inside it is not a word — it is part of one — and the "bad
# substitution" diagnostic, which reports `string` (subst.c:10041), names the
# whole arithmetic string rather than the offending expansion.
#
# Which string that is differs per context, and only because each one is carved
# out differently: a `$(( … ))` keeps the spaces the writer left around the
# expression, a subscript is exactly what stood between the brackets, and a
# `for (( … ))` clause is one third of the header.
#
# The other half of the same fact: `expand_arith_string` longjmps out on such a
# failure, so the expression is never evaluated and the gap the failure left is
# not complained about a second time.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the arithmetic string is what gets named, spacing and all ==="
e 'echo $(( ${x!} ))'
e 'echo $((${x!}))'
e 'echo $(( 1 + ${x!} + 2 ))'
e 'echo $[ ${x!} ]'
# Quoting and neighbouring text are outside the string, so they do not show up.
e 'echo "$(( ${x!} ))"'
e 'echo p$(( ${x!} ))q'
e 'echo "p$(( ${x!} ))q"'
# A nested one is a string of its own and names itself.
e 'echo $(( 1 + $(( ${x!} )) ))'
e 'echo ${y:-$(( 1 + ${x!} ))}'

echo "=== every arithmetic context carves its own string ==="
e '(( ${x!} )); echo "after=$?"'
e 'let " ${x!} "'
e 'declare -i n; n=${x!}'
# One clause of the header, not the whole of it.
e 'for ((i=${x!}; i<0; i++)); do echo x; done'
# A subscript is the text between the brackets — no `${a[…]}` around it.
e 'a=(1); echo ${a[${x!}]}'
e 'a=(1); echo ${a[ ${x!} ]}'
e 'a=(1); a[${x!}]=9'
e 'unset "a[${x!}]"'
e 'printf -v "n[${x!}]" %s hi'
# A `${x:off:len}` bound is expanded on its own too, so each names itself.
e 'v=abcdef; echo ${v:${x!}:2}'
e 'v=abcdef; echo ${v:1:${x!}}'

echo "=== and the expression is not evaluated afterwards ==="
# One diagnostic: the hole left where `${x!}` would have gone is never put to
# the arithmetic evaluator.
e 'a=(1); echo ${a[1+${x!}]}'
e 'echo $(( 1 + ${y:-${x!}} ))'
# …unless the failure was a *subshell's*, which does not unwind this one — then
# the gap really does reach the evaluator and is reported in its own right.
e 'echo $(( 1 + $(echo ${x!}) ))'
e 'echo $(( 1 + `echo ${x!}` ))'

echo "=== a fatal transform is still fatal in here ==="
# `${x@Z}` returns bash's `expand_wdesc_fatal`, which ends the shell rather than
# discarding the command — the naming is the same, the class is not.
e 'echo $(( ${x@Z} )); echo after'
e 'echo a"b${x@Z}"c'

echo done
